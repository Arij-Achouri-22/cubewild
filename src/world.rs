//! Voxel world: block types, terrain generation, greedy meshing, chunk streaming.

use crate::gfx::{ChunkMesh, Gfx};
use glam::IVec3;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const CW: i32 = 16; // chunk width (x and z)
pub const CH: i32 = 256; // chunk height
/// The terrain generators build a world `GH` tall with its sea at `GSEA`; it stands on
/// `LIFT` blocks of deep slate (`under_layer`), so in the world everything they make is
/// `LIFT` higher. Worlds saved before the slate are moved up by `migrate` (in main).
pub const GH: i32 = 128;
pub const LIFT: i32 = 64;
pub const GSEA: i32 = 40;
pub const SEA: i32 = GSEA + LIFT;
pub(crate) const VOL: usize = (CW * CW * CH) as usize;
/// a chunk as the generators make it, and as worlds before the slate saved it
pub(crate) const GVOL: usize = (CW * CW * GH) as usize;

pub type Block = u16;
pub const ANVIL: Block = 256;
pub const ENCHANTING_TABLE: Block = 257;
pub const AIR: Block = 0;
pub const GRASS: Block = 1;
pub const DIRT: Block = 2;
pub const STONE: Block = 3;
pub const SAND: Block = 4;
pub const LOG: Block = 5;
pub const LEAVES: Block = 6;
pub const PLANKS: Block = 7;
pub const COBBLE: Block = 8;
pub const BRICK: Block = 9;
pub const SNOW: Block = 10;
pub const WATER: Block = 11;
pub const GLASS: Block = 12;
pub const BEDROCK: Block = 13;
pub const STONE_BRICK: Block = 14;
pub const LEATHER: Block = 15;
pub const COAL_ORE: Block = 16;
pub const IRON_ORE: Block = 17;
pub const CRYSTAL_ORE: Block = 18;
pub const SANDSTONE: Block = 19;
pub const CACTUS: Block = 20;
pub const WORKBENCH: Block = 21;
pub const FURNACE: Block = 22;
pub const TORCH: Block = 23;
pub const LAMP: Block = 24;
pub const BED: Block = 25;
pub const BIRCH_LOG: Block = 26;
pub const BIRCH_LEAVES: Block = 27;
pub const PINE_LEAVES: Block = 28;
pub const GRAVEL: Block = 29;
pub const CLAY: Block = 30;
pub const GOLD_ORE: Block = 31;
pub const TALL_GRASS: Block = 32;
pub const FLOWER_RED: Block = 33;
pub const FLOWER_YELLOW: Block = 34;
pub const MUSHROOM: Block = 35;
pub const BERRY_BUSH: Block = 36;
pub const DEAD_BUSH: Block = 37;
pub const BIRCH_PLANKS: Block = 38;
pub const GOLD_BLOCK: Block = 39;
pub const IRON_BLOCK: Block = 40;
pub const MOSSY_COBBLE: Block = 41;
pub const CLOTH: Block = 42;
pub const THATCH: Block = 43;
pub const FARMLAND: Block = 44;
/// wheat, from sprout to ripe
pub const WHEAT_0: Block = 45;
pub const WHEAT_1: Block = 46;
pub const WHEAT_2: Block = 47;
pub const WHEAT_3: Block = 48;
/// a thin window: glass in a wooden frame, joining up with whatever stands beside it
pub const PANE: Block = 49;
/// lower half slabs: oak, birch, cobblestone, stone brick (see `SHAPE_FULL`)
pub const SLAB: Block = 50;
pub const SLAB_END: Block = 54;
pub const FENCE: Block = 54;
pub const BIRCH_FENCE: Block = 55;
pub const CHEST: Block = 56;
pub const LAST_BLOCK: Block = CHEST;
/// Molten rock in the deepest caves: it glows, burns whoever wades into it and destroys
/// items. Like water it is not an item.
pub const LAVA: Block = 57;
/// lava fills the caves up to this height
pub const LAVA_LEVEL: i32 = 6;
/// Blocks of the biomes. These five are items too; the ones from `SNOWY_GRASS` on live
/// only in the world (see `items::item_of` for what they give).
pub const ICE: Block = 58;
pub const RED_SAND: Block = 59;
pub const TERRACOTTA: Block = 60;
pub const TERRACOTTA_PALE: Block = 61;
pub const TERRACOTTA_DARK: Block = 62;
pub const FLOWER_BLUE: Block = 63;
/// grass under snow, and the grass of the savanna, the jungle and the swamp
pub const SNOWY_GRASS: Block = 64;
pub const DRY_GRASS: Block = 65;
pub const LUSH_GRASS: Block = 66;
pub const SWAMP_GRASS: Block = 67;
pub const JUNGLE_LEAVES: Block = 68;
pub const ACACIA_LEAVES: Block = 69;
/// pine needles under snow
pub const SNOWY_LEAVES: Block = 70;
pub const FERN: Block = 71;
pub const REEDS: Block = 72;
/// the tall yellow grass of the savanna
pub const DRY_TUFT: Block = 73;
/// A torch fixed to a wall: + the side of the cell the wall is on (see `SIDE_DIR`). It
/// leans out of the wall and is the `TORCH` item again when broken.
pub const WALL_TORCH: Block = 74;
pub const WALL_TORCH_END: Block = 78;
/// Dyed wool, colours 1..=15 of `COLOURS` (colour 0, white, is `CLOTH`). World-only
/// ids: their items are `items::WOOL_ITEM` + colour - 1.
pub const WOOL: Block = 78;
pub const WOOL_END: Block = 93;
/// first texture layer of the dyed wools
pub const WOOL_LAYER: u32 = 76;
/// The sixteen colours of wool and dye: white, orange, magenta, light blue, yellow,
/// lime, pink, grey, light grey, cyan, purple, blue, brown, green, red, black.
pub const COLOURS: [[u8; 3]; 16] = [
    [234, 236, 236], [240, 118, 20], [190, 68, 180], [58, 176, 218], [248, 198, 40], [112, 186, 26], [238, 142, 172], [62, 68, 72],
    [142, 142, 134], [22, 138, 146], [122, 42, 172], [52, 56, 158], [114, 72, 40], [84, 108, 28], [160, 38, 34], [22, 22, 26],
];
/// The veil of an Ember Gate: + 0 across x, + 1 across z. You walk into it.
pub const GATE: Block = 94;
/// Blocks of the Emberdeep (world-only ids; their items are `items::DEEP_ITEM` + n).
/// Cinderstone is also made above, where water meets lava: gates are built of it.
pub const CINDERSTONE: Block = 96;
pub const ASHSTONE: Block = 97;
/// soft grey ash: slows whoever wades through it
pub const ASH: Block = 98;
pub const SCORIA: Block = 99;
/// smouldering rock: burns feet
pub const SMOULDER: Block = 100;
pub const EMBER_MOSS: Block = 101;
pub const CINDERWOOD: Block = 102;
/// the glowing cap of the grove's giant fungi
pub const GLOWCAP: Block = 103;
pub const CINDER_PLANKS: Block = 104;
/// glowing crystal that hangs from the roof of the deep
pub const SUNSHARD: Block = 105;
pub const EMBER_ORE: Block = 106;
pub const ASHSTONE_BRICK: Block = 107;
pub const DEEP_BLOCKS: Block = 12;
/// Deep slate: the rock under the old bedrock, all the way down to the new bedrock.
pub const SLATE: Block = 108;
/// the blackened bricks of the ruined fortresses of the Emberdeep, whole and cracked
pub const FORT_BRICK: Block = 109;
pub const FORT_CRACKED: Block = 110;
/// pyre ore: the rare metal of the Emberdeep, found deep down near the lava sea
pub const PYRE_ORE: Block = 111;
/// The blocks above have items (`items::EXTRA_ITEM` + n of `SLATE` + n).
pub const EXTRA_BLOCKS: Block = 4;
/// Flowing water, + 0 (strongest, also falling water) .. + 6 (weakest): see `fluid`.
pub const WATER_FLOW: Block = 112;
pub const WATER_FLOW_END: Block = 119;
/// Flowing lava, + 0 .. + 2: lava runs three blocks.
pub const LAVA_FLOW: Block = 119;
pub const LAVA_FLOW_END: Block = 122;
/// Saplings (plants): oak, birch, pine, jungle, acacia. Their items are
/// `items::SAPLING_ITEM` + n; they fall from leaves and grow into trees.
pub const SAPLING: Block = 122;
pub const SAPLING_END: Block = 127;
#[inline]
pub fn is_sapling(b: Block) -> bool {
    (SAPLING..SAPLING_END).contains(&b)
}
/// A tree trunk.
#[inline]
pub fn is_log(b: Block) -> bool {
    b == LOG || b == BIRCH_LOG
}
/// The sapling that leaves of a kind let fall.
pub fn sapling_of(leaves: Block) -> Block {
    match leaves {
        BIRCH_LEAVES => SAPLING + 1,
        PINE_LEAVES | SNOWY_LEAVES => SAPLING + 2,
        JUNGLE_LEAVES => SAPLING + 3,
        ACACIA_LEAVES => SAPLING + 4,
        _ => SAPLING,
    }
}
#[inline]
pub fn is_extra_block(b: Block) -> bool {
    (SLATE..SLATE + EXTRA_BLOCKS).contains(&b)
}
/// Still or flowing water.
#[inline]
pub fn is_water(b: Block) -> bool {
    b == WATER || (WATER_FLOW..WATER_FLOW_END).contains(&b)
}
/// Still or flowing lava.
#[inline]
pub fn is_lava(b: Block) -> bool {
    b == LAVA || (LAVA_FLOW..LAVA_FLOW_END).contains(&b)
}
#[inline]
pub fn is_fluid(b: Block) -> bool {
    is_water(b) || is_lava(b)
}
/// How full a fluid cell is: 1 for still water and lava, less for flowing blocks, the
/// weaker the lower (0 for anything else).
#[inline]
pub fn fluid_height(b: Block) -> f32 {
    if b == WATER || b == LAVA {
        1.0
    } else if (WATER_FLOW..WATER_FLOW_END).contains(&b) {
        (7 - (b - WATER_FLOW)) as f32 / 8.0
    } else if (LAVA_FLOW..LAVA_FLOW_END).contains(&b) {
        (3 - (b - LAVA_FLOW)) as f32 / 4.0
    } else {
        0.0
    }
}

/// Which fluid a block holds: 0 none, 1 water, 2 lava (so a face between still and
/// flowing water is not drawn).
#[inline]
pub fn fluid_kind(b: Block) -> u8 {
    if is_water(b) {
        1
    } else if is_lava(b) {
        2
    } else {
        0
    }
}
#[inline]
pub fn is_gate(b: Block) -> bool {
    b == GATE || b == GATE + 1
}
#[inline]
pub fn is_deep_block(b: Block) -> bool {
    (CINDERSTONE..CINDERSTONE + DEEP_BLOCKS).contains(&b)
}
#[inline]
pub fn is_wool(b: Block) -> bool {
    b == CLOTH || (WOOL..WOOL_END).contains(&b)
}
/// A torch, standing or on a wall.
#[inline]
pub fn is_torch(b: Block) -> bool {
    b == TORCH || (WALL_TORCH..WALL_TORCH_END).contains(&b)
}
/// The small box a torch is aimed at by (it is never collided with).
pub fn torch_box(b: Block) -> [f32; 6] {
    match b {
        TORCH => [0.4, 0.0, 0.4, 0.6, 0.65, 0.6],
        _ => {
            let (dx, dz) = SIDE_DIR[(b - WALL_TORCH) as usize];
            let (cx, cz) = (0.5 + dx as f32 * 0.29, 0.5 + dz as f32 * 0.29);
            [cx - 0.16, 0.2, cz - 0.16, cx + 0.16, 0.96, cz + 0.16]
        }
    }
}

/// The terrain generator a world was made with: 1 the first one, 2 the one with biomes
/// (`worldgen`), 3 the same with villages in it (`village`). A world keeps its generator
/// for good, or old and new chunks would not fit.
pub const GEN_LATEST: u8 = 3;

/// Shapes that live only in the world, one id per state (their items are in `items`).
/// Upper half slabs, same order as `SLAB`.
pub const SLAB_TOP: Block = 133;
/// + the side of the cell whose wall the ladder hangs on
pub const LADDER_BASE: Block = 137;
/// bits 0-1 the side it swings up against, 2 open, 3 in the upper half of the cell
pub const TRAP_BASE: Block = 141;
pub const TRAP_END: Block = 157;
/// A chest that world generation left in a cave, not opened yet: it becomes an ordinary
/// chest, filled by `loot::roll_chest`, the first time it is opened or broken.
pub const LOOT_CHEST: Block = 158;
pub const TRAP_OPEN: Block = 4;
pub const TRAP_TOP: Block = 8;
/// bits 0-1 the side the high step is on, 2 upside down, 3-4 material (see `SHAPE_FULL`)
pub const STAIR_BASE: Block = 224;
pub const STAIR_FLIP: Block = 4;
/// the full block each slab / stair material is cut from
pub const SHAPE_FULL: [Block; 8] = [PLANKS, BIRCH_PLANKS, COBBLE, STONE_BRICK, BRICK, SANDSTONE, SLATE, CINDER_PLANKS];

/// Doors live only in the world (the item is `items::OAK_DOOR` / `BIRCH_DOOR`): one id per
/// state. Bits above `DOOR_BASE`: 0-1 the cell side the closed door stands on (see
/// `SIDE_DIR`), 2 open, 3 upper half, 4 hinge on the right, 5 wood (0 oak, 1 birch).
pub const DOOR_BASE: Block = 160;
pub const DOOR_END: Block = 224;
pub const DOOR_OPEN: Block = 4;
pub const DOOR_UPPER: Block = 8;
pub const DOOR_HINGE_RIGHT: Block = 16;
/// thickness of a door, and of the box a window pane collides with
pub const DOOR_THICK: f32 = 3.0 / 16.0;
const PANE_HALF: f32 = 1.0 / 16.0;
/// cell sides: 0 = -X, 1 = +X, 2 = -Z, 3 = +Z, as (dx, dz)
pub const SIDE_DIR: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
/// the side to the left / right of someone who faces a door standing on side `s` from outside
pub const SIDE_LEFT: [u8; 4] = [2, 3, 1, 0];
pub const SIDE_RIGHT: [u8; 4] = [3, 2, 0, 1];

#[inline]
pub fn is_door(b: Block) -> bool {
    (DOOR_BASE..DOOR_END).contains(&b)
}
/// Blocks that do not fill their cell: collision, aiming and drawing use `shape_boxes`.
#[inline]
pub fn is_shaped(b: Block) -> bool {
    (PANE..=CHEST).contains(&b) || (SLAB_TOP..=255).contains(&b) || is_slab(b) || is_stairs(b) || crate::building::is_fence_gate(b) || crate::building::is_sign(b)
}
#[inline]
pub fn is_slab(b: Block) -> bool {
    (SLAB..SLAB_END).contains(&b) || (SLAB_TOP..LADDER_BASE).contains(&b) || (crate::building::SLABS..crate::building::SLABS+8).contains(&b)
}
#[inline]
pub fn slab_top(b: Block) -> bool {
    (SLAB_TOP..LADDER_BASE).contains(&b) || (crate::building::SLABS+4..crate::building::SLABS+8).contains(&b)
}
/// index into `SHAPE_FULL`
#[inline]
pub fn slab_mat(b: Block) -> Block {
    if b >= crate::building::SLABS {4+(b-crate::building::SLABS)%4} else if b >= SLAB_TOP { b - SLAB_TOP } else { b - SLAB }
}
#[inline]
pub fn is_fence(b: Block) -> bool {
    b == FENCE || b == BIRCH_FENCE
}
#[inline]
pub fn is_ladder(b: Block) -> bool {
    (LADDER_BASE..TRAP_BASE).contains(&b)
}
#[inline]
pub fn is_trapdoor(b: Block) -> bool {
    (TRAP_BASE..TRAP_END).contains(&b)
}
#[inline]
pub fn trap_open(b: Block) -> bool {
    (b - TRAP_BASE) & TRAP_OPEN != 0
}
#[inline]
pub fn is_stairs(b: Block) -> bool {
    (STAIR_BASE..=255).contains(&b) || (crate::building::STAIRS..crate::building::STAIRS+32).contains(&b)
}
#[inline]
pub fn stair_mat(b: Block) -> Block {
    if b>=crate::building::STAIRS {4+((b-crate::building::STAIRS)>>3)} else {(b-STAIR_BASE)>>3}
}
/// Blocks that keep the daylight out of what lies under them, like a full block does.
#[inline]
fn roofs(b: Block) -> bool {
    (is_opaque(b) && !is_leaves(b)) || is_slab(b) || is_stairs(b) || (is_trapdoor(b) && !trap_open(b))
}
#[inline]
pub fn door_open(b: Block) -> bool {
    (b - DOOR_BASE) & DOOR_OPEN != 0
}
#[inline]
pub fn door_upper(b: Block) -> bool {
    (b - DOOR_BASE) & DOOR_UPPER != 0
}
/// 0 oak, 1 birch
#[inline]
pub fn door_wood(b: Block) -> Block {
    (b - DOOR_BASE) >> 5
}
/// (side of the cell the door leaf stands on right now, side its hinge is at)
pub fn door_sides(b: Block) -> (u8, u8) {
    let d = b - DOOR_BASE;
    let closed = (d & 3) as u8;
    let swing = if d & DOOR_HINGE_RIGHT != 0 { SIDE_RIGHT } else { SIDE_LEFT }[closed as usize];
    if d & DOOR_OPEN != 0 { (swing, closed) } else { (closed, swing) }
}
/// Texture layer of a door half; the mirrored set keeps the knob away from the hinge.
fn door_layer(b: Block) -> u32 {
    let (_, hinge) = door_sides(b);
    crate::textures::DOOR_TEX as u32 + door_wood(b) as u32 * 2 + !door_upper(b) as u32 + if hinge & 1 == 1 { 4 } else { 0 }
}
/// What a window pane joins up with.
#[inline]
fn pane_joins(b: Block) -> bool {
    b == PANE || b == GLASS || is_door(b) || (is_opaque(b) && !is_leaves(b))
}
/// (runs along x, runs along z) for a pane, from whether it has a neighbour on each axis.
#[inline]
fn pane_axes(join_x: bool, join_z: bool) -> (bool, bool) {
    if join_x || join_z { (join_x, join_z) } else { (true, true) }
}
/// What a fence joins up with.
#[inline]
fn fence_joins(b: Block) -> bool {
    if crate::building::is_fence_gate(b) {return true;}
    is_fence(b) || (is_opaque(b) && !is_leaves(b))
}
/// A slab of thickness `t` standing against side `s` of the cell.
fn side_box(s: u8, from: f32, t: f32) -> [f32; 6] {
    match s {
        0 => [from, 0.0, 0.0, t, 1.0, 1.0],
        1 => [1.0 - t, 0.0, 0.0, 1.0 - from, 1.0, 1.0],
        2 => [0.0, 0.0, from, 1.0, 1.0, t],
        _ => [0.0, 0.0, 1.0 - t, 1.0, 1.0, 1.0 - from],
    }
}

/// up to five boxes, and how many are used
pub type Boxes = ([[f32; 6]; 5], usize);

/// Boxes a shaped block fills inside its cell, as [x0, y0, z0, x1, y1, z1]; `at` reads
/// the neighbouring blocks on its level by offset. A fence stands taller than its cell.
pub fn shape_boxes(b: Block, at: impl Fn(i32, i32) -> Block) -> Boxes {
    let mut v = [[0.0f32; 6]; 5];
    if crate::building::is_fence_gate(b) {return crate::building::gate_boxes(b,false);}
    if crate::building::is_sign(b) {return crate::building::sign_boxes(b);}
    let n = if is_door(b) {
        v[0] = side_box(door_sides(b).0, 0.0, DOOR_THICK);
        1
    } else if b == PANE {
        let (ax, az) = pane_axes(pane_joins(at(-1, 0)) || pane_joins(at(1, 0)), pane_joins(at(0, -1)) || pane_joins(at(0, 1)));
        let (lo, hi) = (0.5 - PANE_HALF, 0.5 + PANE_HALF);
        let mut n = 0;
        if ax {
            v[n] = [0.0, 0.0, lo, 1.0, 1.0, hi];
            n += 1;
        }
        if az {
            v[n] = [lo, 0.0, 0.0, hi, 1.0, 1.0];
            n += 1;
        }
        n
    } else if is_slab(b) {
        v[0] = if slab_top(b) { [0.0, 0.5, 0.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0, 0.5, 1.0] };
        1
    } else if is_stairs(b) {
        let d = b - STAIR_BASE;
        let flip = d & STAIR_FLIP != 0;
        v[0] = if flip { [0.0, 0.5, 0.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0, 0.5, 1.0] };
        // the high part is a half, a quarter (outer corner) or three quarters (inner
        // corner), turning to meet stairs set crosswise in front of or behind this one
        let s = (d & 3) as u8;
        let like = |o: Block| is_stairs(o) && (o - STAIR_BASE) & STAIR_FLIP == d & STAIR_FLIP;
        let side = |o: Block| ((o - STAIR_BASE) & 3) as u8;
        let nb = |t: u8| at(SIDE_DIR[t as usize].0, SIDE_DIR[t as usize].1);
        // no stairs in line with this one on side `t`
        let free = |t: u8| !(like(nb(t)) && side(nb(t)) == s);
        let quarter = |a: u8, t: u8| {
            let (p, q) = (side_box(a, 0.0, 0.5), side_box(t, 0.0, 0.5));
            [p[0].max(q[0]), 0.0, p[2].max(q[2]), p[3].min(q[3]), 1.0, p[5].min(q[5])]
        };
        let (front, back) = (nb(s), nb(s ^ 1));
        let n = if like(front) && side(front) >> 1 != s >> 1 && free(side(front) ^ 1) {
            v[1] = quarter(s, side(front));
            2
        } else if like(back) && side(back) >> 1 != s >> 1 && free(side(back)) {
            v[1] = side_box(s, 0.0, 0.5);
            v[2] = quarter(s ^ 1, side(back));
            3
        } else {
            v[1] = side_box(s, 0.0, 0.5);
            2
        };
        for bx in &mut v[1..n] {
            (bx[1], bx[4]) = if flip { (0.0, 0.5) } else { (0.5, 1.0) };
        }
        n
    } else if is_fence(b) {
        let (p0, p1, r0, r1) = (0.375, 0.625, 0.4375, 0.5625);
        v[0] = [p0, 0.0, p0, p1, 1.5, p1];
        let mut n = 1;
        for (s, arm) in [[0.0, 0.0, r0, p0, 1.5, r1], [p1, 0.0, r0, 1.0, 1.5, r1], [r0, 0.0, 0.0, r1, 1.5, p0], [r0, 0.0, p1, r1, 1.5, 1.0]].into_iter().enumerate() {
            if fence_joins(at(SIDE_DIR[s].0, SIDE_DIR[s].1)) {
                v[n] = arm;
                n += 1;
            }
        }
        n
    } else if b == CHEST || b == LOOT_CHEST {
        v[0] = [0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375];
        1
    } else if is_ladder(b) {
        v[0] = side_box((b - LADDER_BASE) as u8, 0.0, DOOR_THICK);
        1
    } else if is_trapdoor(b) {
        let d = b - TRAP_BASE;
        v[0] = if d & TRAP_OPEN != 0 {
            side_box((d & 3) as u8, 0.0, DOOR_THICK)
        } else if d & TRAP_TOP != 0 {
            [0.0, 1.0 - DOOR_THICK, 0.0, 1.0, 1.0, 1.0]
        } else {
            [0.0, 0.0, 0.0, 1.0, DOOR_THICK, 1.0]
        };
        1
    } else {
        v[0] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        1
    };
    (v, n)
}

/// The boxes of a shaped block as they are drawn, each with the block whose textures it
/// wears: finer than `shape_boxes` (fence rails, a ladder standing off its wall).
fn draw_boxes(b: Block, at: impl Fn(i32, i32) -> Block) -> Boxes {
    if crate::building::is_fence_gate(b) {return crate::building::gate_boxes(b,true);}
    if is_fence(b) {
        let mut v = [[0.0f32; 6]; 5];
        v[0] = [0.375, 0.0, 0.375, 0.625, 1.0, 0.625];
        let mut n = 1;
        // one box per joined side, cut into two rails by the caller
        for (s, arm) in [[0.0, 0.0, 0.4375, 0.375, 1.0, 0.5625], [0.625, 0.0, 0.4375, 1.0, 1.0, 0.5625], [0.4375, 0.0, 0.0, 0.5625, 1.0, 0.375], [0.4375, 0.0, 0.625, 0.5625, 1.0, 1.0]].into_iter().enumerate() {
            if fence_joins(at(SIDE_DIR[s].0, SIDE_DIR[s].1)) {
                v[n] = arm;
                n += 1;
            }
        }
        (v, n)
    } else if is_ladder(b) {
        let mut v = [[0.0f32; 6]; 5];
        v[0] = side_box((b - LADDER_BASE) as u8, 0.0625, 0.125);
        (v, 1)
    } else {
        shape_boxes(b, at)
    }
}

/// Small plants drawn as two crossed planes; you walk through them.
#[inline]
pub fn is_plant(b: Block) -> bool {
    crate::farming::crop(b).is_some() || (TALL_GRASS..=DEAD_BUSH).contains(&b) || (WHEAT_0..=WHEAT_3).contains(&b) || b == FLOWER_BLUE || (FERN..=DRY_TUFT).contains(&b) || is_sapling(b)
}
/// Grass tufts and their kin: cut for fibre and seeds.
#[inline]
pub fn is_tuft(b: Block) -> bool {
    b == TALL_GRASS || b == FERN || b == DRY_TUFT
}
/// Soil with grass on it, whatever the biome.
#[inline]
pub fn is_grassy(b: Block) -> bool {
    b == GRASS || (SNOWY_GRASS..=SWAMP_GRASS).contains(&b)
}
#[inline]
pub fn is_leaves(b: Block) -> bool {
    b == LEAVES || b == BIRCH_LEAVES || b == PINE_LEAVES || (JUNGLE_LEAVES..=SNOWY_LEAVES).contains(&b)
}
/// Blocks a new block may be placed into.
#[inline]
pub fn is_replaceable(b: Block) -> bool {
    b == AIR || is_fluid(b) || is_tuft(b) || b == DEAD_BUSH
}

#[inline]
pub fn is_opaque(b: Block) -> bool {
    b != AIR && !is_water(b) && b != GLASS && !is_torch(b) && !is_plant(b) && !is_shaped(b) && !is_gate(b) && !(LAVA_FLOW..LAVA_FLOW_END).contains(&b)
}
/// Blocks you collide with / can target.
#[inline]
pub fn is_solid(b: Block) -> bool {
    b != AIR && !is_fluid(b) && !is_torch(b) && !is_plant(b) && !is_ladder(b) && !is_gate(b) && !crate::building::is_sign(b)
}
/// Blocks the crosshair can select (torches are walk-through but breakable).
#[inline]
pub fn is_target(b: Block) -> bool {
    b != AIR && !is_fluid(b) && !is_gate(b)
}
/// Blocks that light their surroundings.
#[inline]
pub fn is_light(b: Block) -> bool {
    is_torch(b) || b == LAMP || is_gate(b) || b == SUNSHARD || b == GLOWCAP
}

/// Texture layer for a block face (face: 0 +X, 1 -X, 2 +Y, 3 -Y, 4 +Z, 5 -Z).
#[inline]
pub fn tex_for(b: Block, face: usize) -> u32 {
    if crate::building::is_fence_gate(b) {return tex_for(if (b-crate::building::FENCE_GATE)&4!=0 {BIRCH_PLANKS} else {PLANKS},face);}
    if crate::building::is_sign(b) {return tex_for(PLANKS,face);}

    if let Some(m) = crate::farming::moisture(b) {
        return if face == 2 { if m > 0 { crate::textures::NEW_TEX as u32 + 3 } else { 50 } } else { 2 };
    }
    if let Some((base, stage)) = crate::farming::crop(b) {
        if base != WHEAT_0 { return crate::textures::NEW_TEX as u32 + 4 + if base == crate::farming::POTATOES { 4 } else { 0 } + stage as u32; }
    }

    match b {
        ANVIL => crate::textures::NEW_TEX as u32,
        ENCHANTING_TABLE => crate::textures::NEW_TEX as u32 + 1,
        GRASS => match face {
            2 => 0,
            3 => 2,
            _ => 1,
        },
        DIRT => 2,
        STONE => 3,
        SAND => 4,
        LOG => {
            if face == 2 || face == 3 {
                6
            } else {
                5
            }
        }
        LEAVES => 7,
        PLANKS => 8,
        COBBLE => 9,
        BRICK => 10,
        SNOW => 11,
        b if is_water(b) => 12,
        GLASS => 13,
        STONE_BRICK => 15,
        LEATHER => 16,
        COAL_ORE => 17,
        IRON_ORE => 18,
        CRYSTAL_ORE => 19,
        SANDSTONE => 20,
        CACTUS => {
            if face == 2 || face == 3 {
                22
            } else {
                21
            }
        }
        WORKBENCH => match face {
            2 => 23,
            3 => 8,
            _ => 24,
        },
        FURNACE => match face {
            2 | 3 => 9,
            4 | 5 => 25,
            _ => 26,
        },
        LAMP => LAMP_LAYER,
        BED => match face {
            2 => 28,
            3 => 8,
            _ => 29,
        },
        TORCH => 30,
        BIRCH_LOG => {
            if face == 2 || face == 3 {
                32
            } else {
                31
            }
        }
        BIRCH_LEAVES => 33,
        PINE_LEAVES => 34,
        GRAVEL => 35,
        CLAY => 36,
        GOLD_ORE => 37,
        TALL_GRASS => 38,
        FLOWER_RED => 39,
        FLOWER_YELLOW => 40,
        MUSHROOM => 41,
        BERRY_BUSH => 42,
        DEAD_BUSH => 43,
        BIRCH_PLANKS => 44,
        GOLD_BLOCK => 45,
        IRON_BLOCK => 46,
        MOSSY_COBBLE => 47,
        CLOTH => 48,
        THATCH => 49,
        FARMLAND => {
            if face == 2 {
                50
            } else {
                2
            }
        }
        WHEAT_0..=WHEAT_3 => 51 + (b - WHEAT_0) as u32,
        PANE => PANE_LAYER,
        FENCE => 8,
        BIRCH_FENCE => 44,
        b if is_lava(b) => LAVA_LAYER,
        CHEST | LOOT_CHEST => {
            if face == 2 || face == 3 {
                CHEST_TOP_LAYER
            } else {
                CHEST_LAYER
            }
        }
        ICE => 57,
        RED_SAND => 58,
        TERRACOTTA => 59,
        TERRACOTTA_PALE => 60,
        TERRACOTTA_DARK => 61,
        SNOWY_GRASS => match face {
            2 => 11,
            3 => 2,
            _ => 62,
        },
        DRY_GRASS | LUSH_GRASS | SWAMP_GRASS => match face {
            2 => 63 + (b - DRY_GRASS) as u32 * 2,
            3 => 2,
            _ => 64 + (b - DRY_GRASS) as u32 * 2,
        },
        JUNGLE_LEAVES => 69,
        ACACIA_LEAVES => 70,
        SNOWY_LEAVES => 71,
        FERN => 72,
        REEDS => 73,
        FLOWER_BLUE => 74,
        DRY_TUFT => 75,
        b if (WOOL..WOOL_END).contains(&b) => WOOL_LAYER + (b - WOOL) as u32,
        b if is_deep_block(b) => {
            let t = crate::textures::DEEP_TEX as u32;
            let top = face == 2 || face == 3;
            t + match b {
                CINDERSTONE => 0,
                ASHSTONE => 1,
                ASH => 2,
                SCORIA if top => 4,
                SCORIA => 3,
                SMOULDER => 5,
                EMBER_MOSS if face == 2 => 6,
                EMBER_MOSS if face == 3 => 1,
                EMBER_MOSS => 7,
                CINDERWOOD if top => 9,
                CINDERWOOD => 8,
                GLOWCAP => 10,
                CINDER_PLANKS => 11,
                SUNSHARD => 12,
                EMBER_ORE => 13,
                _ => 14,
            }
        }
        b if is_extra_block(b) => crate::textures::EXTRA_TEX as u32 + (b - SLATE) as u32,
        b if is_sapling(b) => crate::textures::sapling_layer((b - SAPLING) as usize) as u32,
        b if is_slab(b) => tex_for(SHAPE_FULL[slab_mat(b) as usize], face),
        b if is_ladder(b) => LADDER_LAYER,
        b if is_trapdoor(b) => TRAP_LAYER,
        b if is_door(b) => door_layer(b),
        b if is_stairs(b) => tex_for(SHAPE_FULL[stair_mat(b) as usize], face),
        _ => 14,
    }
}

/// texture layer the shader renders at full brightness
pub const LAMP_LAYER: u32 = 27;
pub const LAVA_LAYER: u32 = 56;
/// window pane texture; the shader draws quads with this layer through the middle of the cell
pub const PANE_LAYER: u32 = 55;
/// textures past the item icons (only shaped blocks use them: see `MeshData::detail`)
pub const CHEST_LAYER: u32 = crate::textures::EXTRA_BASE as u32;
pub const CHEST_TOP_LAYER: u32 = CHEST_LAYER + 1;
pub const LADDER_LAYER: u32 = CHEST_LAYER + 2;
pub const TRAP_LAYER: u32 = CHEST_LAYER + 3;

// ---------------------------------------------------------------- hashing

#[derive(Default)]
pub struct FxHasher(u64);
impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517cc1b727220a95);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.0 = (self.0.rotate_left(5) ^ i as u32 as u64).wrapping_mul(0x517cc1b727220a95);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}
pub type Fx = BuildHasherDefault<FxHasher>;
pub type Key = (i32, i32);

// ---------------------------------------------------------------- noise

#[inline]
pub(crate) fn hash2(seed: u32, x: i32, z: i32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x85eb_ca6b)
        ^ (z as u32).wrapping_mul(0xc2b2_ae35).rotate_left(13);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

#[inline]
fn grad(seed: u32, ix: i32, iz: i32, dx: f32, dz: f32) -> f32 {
    const G: [(f32, f32); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (0.7071, 0.7071),
        (-0.7071, 0.7071),
        (0.7071, -0.7071),
        (-0.7071, -0.7071),
    ];
    let g = G[(hash2(seed, ix, iz) & 7) as usize];
    g.0 * dx + g.1 * dz
}

fn perlin(seed: u32, x: f32, z: f32) -> f32 {
    let (fx, fz) = (x.floor(), z.floor());
    let (ix, iz) = (fx as i32, fz as i32);
    let (dx, dz) = (x - fx, z - fz);
    let u = dx * dx * dx * (dx * (dx * 6.0 - 15.0) + 10.0);
    let v = dz * dz * dz * (dz * (dz * 6.0 - 15.0) + 10.0);
    let a = grad(seed, ix, iz, dx, dz);
    let b = grad(seed, ix + 1, iz, dx - 1.0, dz);
    let c = grad(seed, ix, iz + 1, dx, dz - 1.0);
    let d = grad(seed, ix + 1, iz + 1, dx - 1.0, dz - 1.0);
    let ab = a + (b - a) * u;
    let cd = c + (d - c) * u;
    (ab + (cd - ab) * v) * 1.5
}

pub(crate) fn fbm(seed: u32, mut x: f32, mut z: f32, oct: u32) -> f32 {
    let (mut amp, mut sum, mut norm) = (1.0, 0.0, 0.0);
    // (shifted off the lattice, where the noise is zero whatever the seed: otherwise the
    // middle of every world would be the same low plain)
    x += 0.37;
    z += 0.61;
    for i in 0..oct {
        sum += perlin(seed.wrapping_add(i * 131), x, z) * amp;
        norm += amp;
        amp *= 0.5;
        x *= 2.03;
        z *= 2.03;
    }
    sum / norm
}

pub(crate) fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Terrain surface height (y of the top solid block) at a world column.
pub fn height_at(seed: u32, x: i32, z: i32) -> i32 {
    let (xf, zf) = (x as f32, z as f32);
    let c = fbm(seed, xf / 340.0, zf / 340.0, 4);
    let hills = fbm(seed.wrapping_add(1000), xf / 95.0, zf / 95.0, 3);
    let detail = fbm(seed.wrapping_add(2000), xf / 26.0, zf / 26.0, 2);
    let mut h = 45.0 + c * 30.0 + hills * 7.0 * smoothstep(-0.3, 0.2, c) + detail * 1.6;
    let m = smoothstep(0.12, 0.5, c);
    if m > 0.0 {
        let r = 1.0 - fbm(seed.wrapping_add(3000), xf / 150.0, zf / 150.0, 4).abs() * 1.6;
        let r = r.max(0.0);
        h += m * r * r * 58.0;
    }
    // the highest peaks level off gently instead of being cut flat at the build limit
    if h > 96.0 {
        h = 96.0 + 20.0 * (1.0 - (-(h - 96.0) / 20.0).exp());
    }
    (h as i32).clamp(3, GH - 12)
}

/// Where a new world's player starts: the nearest spot to the origin on fairly level
/// green land, clear of sea, desert and mountains. Returns the column and its height.
pub fn spawn_point(seed: u32) -> (i32, i32, i32) {
    let ok = |x: i32, z: i32, strict: bool| -> Option<i32> {
        let h = height_at(seed, x, z);
        if h <= GSEA + 2 || h >= 78 {
            return None;
        }
        if strict {
            let level = [(6, 0), (-6, 0), (0, 6), (0, -6)].iter().all(|&(dx, dz)| (height_at(seed, x + dx, z + dz) - h).abs() <= 3);
            if !level || is_desert(seed, x, z) {
                return None;
            }
        }
        Some(h)
    };
    // rings of points 16 apart, growing outwards; first the choosy pass, then any land
    for strict in [true, false] {
        for ring in 0..96i32 {
            for i in -ring..=ring {
                for (x, z) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                    if let Some(h) = ok(x * 16, z * 16, strict) {
                        return (x * 16, h, z * 16);
                    }
                }
            }
        }
    }
    (0, height_at(seed, 0, 0).max(GSEA), 0)
}

#[inline]
fn grad3(seed: u32, ix: i32, iy: i32, iz: i32, dx: f32, dy: f32, dz: f32) -> f32 {
    let h = hash2(seed ^ (iy as u32).wrapping_mul(0x27d4_eb2f), ix, iz) & 15;
    let u = if h < 8 { dx } else { dy };
    let v = if h < 4 {
        dy
    } else if h == 12 || h == 14 {
        dx
    } else {
        dz
    };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// 3D gradient noise, roughly -1..1.
fn perlin3(seed: u32, x: f32, y: f32, z: f32) -> f32 {
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
    let (dx, dy, dz) = (x - fx, y - fy, z - fz);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v, w) = (fade(dx), fade(dy), fade(dz));
    let g = |ox: i32, oy: i32, oz: i32| {
        grad3(seed, ix + ox, iy + oy, iz + oz, dx - ox as f32, dy - oy as f32, dz - oz as f32)
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(g(0, 0, 0), g(1, 0, 0), u);
    let x10 = lerp(g(0, 1, 0), g(1, 1, 0), u);
    let x01 = lerp(g(0, 0, 1), g(1, 0, 1), u);
    let x11 = lerp(g(0, 1, 1), g(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w)
}

/// The two fields whose zero surfaces the tunnel networks follow. They change slowly, so
/// `generate` samples them on every other level of a column and fills in between.
pub(crate) fn tunnel_fields(seed: u32, x: i32, y: i32, z: i32) -> (f32, f32) {
    // (off the lattice, like the terrain)
    let (xf, yf, zf) = (x as f32 + 16.3, y as f32 + 9.7, z as f32 + 26.9);
    (perlin3(seed.wrapping_add(8000), xf / 44.0, yf / 26.0, zf / 44.0), perlin3(seed.wrapping_add(8500), xf / 30.0, yf / 34.0, zf / 30.0))
}

/// Winding tunnels: the space where two noise fields are both near zero. Two networks
/// of them cross each other; the first widens into rooms here and there, the second is
/// finer and steeper, so it links the levels of the first. `a` and `c` are the values
/// of `tunnel_fields` at the cell.
pub(crate) fn in_tunnel(seed: u32, x: i32, y: i32, z: i32, a: f32, c: f32) -> bool {
    if y < 2 {
        return false;
    }
    let (xf, yf, zf) = (x as f32 + 16.3, y as f32 + 9.7, z as f32 + 26.9);
    if a.abs() < 0.13 {
        let b = perlin3(seed.wrapping_add(8100), xf / 44.0, yf / 26.0, zf / 44.0);
        let room = perlin3(seed.wrapping_add(8200), xf / 90.0, yf / 60.0, zf / 90.0).max(0.0);
        if a * a + b * b < 0.0042 + room * 0.008 {
            return true;
        }
    }
    if c.abs() < 0.09 {
        let d = perlin3(seed.wrapping_add(8600), xf / 30.0, yf / 34.0, zf / 30.0);
        return c * c + d * d < 0.0030;
    }
    false
}

/// True if a tunnel passes through the cell.
pub fn is_cave(seed: u32, x: i32, y: i32, z: i32) -> bool {
    // (between the sampled levels the fields are the mean of their neighbours, as in `generate`)
    let (p, q) = (tunnel_fields(seed, x, y - y % 2, z), tunnel_fields(seed, x, y + y % 2, z));
    in_tunnel(seed, x, y, z, (p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5)
}

/// Caverns: wide halls in the deep rock, rare near the surface and commoner and larger
/// the further down. Their floors lie under lava below `LAVA_LEVEL`. This is the slow,
/// broad field that places them; `generate` samples it every fourth block of a column
/// and fills in between.
pub(crate) fn cavern_field(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    perlin3(seed.wrapping_add(8300), (x as f32 + 5.9) / 58.0, (y as f32 + 3.1) / 22.0, (z as f32 + 12.7) / 58.0)
}

/// True if the cell lies in a cavern, given the cavern field's value there.
pub(crate) fn in_cavern(seed: u32, x: i32, y: i32, z: i32, hall: f32) -> bool {
    if hall < 0.12 || y < 2 || y > CAVERN_TOP {
        return false;
    }
    let deep = ((CAVERN_TOP - y) as f32 / 34.0).min(1.0);
    // (a finer field roughens the walls and leaves pillars standing)
    let rough = perlin3(seed.wrapping_add(8400), (x as f32 + 5.9) / 15.0, (y as f32 + 3.1) / 11.0, (z as f32 + 12.7) / 15.0);
    hall + rough * 0.22 > 0.47 - 0.16 * deep
}
/// no cavern reaches higher than this
pub(crate) const CAVERN_TOP: i32 = 44;

/// Where a chunk's cave chest stands, if it has one: on a cave floor of a column picked
/// by the seed, the deepest that is clear of the lava.
pub(crate) fn cave_chest(seed: u32, cx: i32, cz: i32, blocks: &[Block], h: impl Fn(i32, i32) -> i32) -> Option<(i32, i32, i32)> {
    let r = hash2(seed ^ 0x7c3a_91d5, cx, cz);
    if r % 5 >= 2 {
        return None;
    }
    // a few columns are tried, since most of the rock holds no cave
    (0..12).find_map(|k| {
        let q = hash2(r, k, 77);
        let (x, z) = ((q & 15) as i32, ((q >> 4) & 15) as i32);
        (LAVA_LEVEL + 1..(h(x, z) - 8).min(40)).find(|&y| {
            blocks[bidx(x, y, z)] == AIR && blocks[bidx(x, y + 1, z)] == AIR && matches!(blocks[bidx(x, y - 1, z)], STONE | COAL_ORE | IRON_ORE | GOLD_ORE | CRYSTAL_ORE)
        })
        .map(|y| (x, y, z))
    })
}

/// Ore pockets inside stone, rarer and deeper for the better ones.
pub(crate) fn ore_at(seed: u32, x: i32, y: i32, z: i32) -> Block {
    let (xf, yf, zf) = (x as f32, y as f32, z as f32);
    // (pockets grow towards the bottom of the world)
    let n = perlin3(seed.wrapping_add(9000), xf / 5.5, yf / 5.5, zf / 5.5) + ((44 - y) as f32 / 44.0).clamp(0.0, 1.0) * 0.07;
    if n < 0.5 {
        return STONE;
    }
    // one ore type per pocket
    let pick = hash2(seed ^ 0x51ed_270b ^ ((y + 4) as u32 >> 3).wrapping_mul(0x9e37), (x + 4) >> 3, (z + 4) >> 3) % 100;
    if y < 22 && pick < 14 && n > 0.56 {
        CRYSTAL_ORE
    } else if y < 34 && pick >= 86 && n > 0.54 {
        GOLD_ORE
    } else if y < 60 && pick < 50 && n > 0.53 {
        IRON_ORE
    } else if pick >= 40 {
        COAL_ORE
    } else {
        STONE
    }
}

/// Dry sandy regions with cacti instead of trees.
pub fn is_desert(seed: u32, x: i32, z: i32) -> bool {
    fbm(seed.wrapping_add(7000), x as f32 / 420.0, z as f32 / 420.0, 3) > 0.2
}

/// Surface height of a column in a world made by generator `ver`.
pub fn ground_height(ver: u8, seed: u32, x: i32, z: i32) -> i32 {
    LIFT + if ver >= 2 { crate::worldgen::height_at(seed, x, z) } else { height_at(seed, x, z) }
}
/// Where a new player starts in a world made by generator `ver`.
pub fn spawn_for(ver: u8, seed: u32) -> (i32, i32, i32) {
    let (x, y, z) = if ver >= 2 { crate::worldgen::spawn_point(seed) } else { spawn_point(seed) };
    (x, y + LIFT, z)
}
fn generate_for(ver: u8, seed: u32, cx: i32, cz: i32) -> ChunkData {
    let made = if ver == crate::deep::DEEP_VER {
        crate::deep::generate(seed, cx, cz)
    } else if ver >= 2 {
        crate::worldgen::generate(seed, cx, cz, ver >= 3)
    } else {
        generate(seed, cx, cz)
    };
    if ver == crate::deep::DEEP_VER {
        // the Emberdeep keeps its height: a roof of bedrock, then nothing
        return grow(&made.blocks, 0);
    }
    let mut c = grow(&made.blocks, LIFT);
    under_layer(seed, cx, cz, &mut c, true);
    c
}

/// A chunk `GH` tall (as generated, or saved before the slate) placed `lift` blocks up
/// in a full-height chunk, with nothing under or over it.
pub(crate) fn grow(old: &[Block], lift: i32) -> ChunkData {
    let mut blocks = vec![AIR; VOL].into_boxed_slice();
    let at = bidx(0, lift, 0);
    blocks[at..at + GVOL].copy_from_slice(&old[..GVOL]);
    ChunkData { blocks }
}

/// A chunk of a world saved before the slate, moved up onto it.
pub(crate) fn lift_saved(seed: u32, cx: i32, cz: i32, old: &[Block]) -> ChunkData {
    let mut c = grow(old, LIFT);
    under_layer(seed, cx, cz, &mut c, false);
    c
}

/// The deep slate under the old world, from the new bedrock up to where the old
/// bedrock was (which becomes slate): tunnels and wide halls of its own, richer ore,
/// and lava pooled at the bottom. With `fresh`, the old world's lowest caves (where
/// lava used to stand) are opened down into it.
fn under_layer(seed: u32, cx: i32, cz: i32, c: &mut ChunkData, fresh: bool) {
    let s2 = seed.wrapping_add(0x51a7e);
    for z in 0..CW {
        for x in 0..CW {
            let (wx, wz) = (cx * CW + x, cz * CW + z);
            // (fields sampled every other level, as in `generate`)
            let mut fields = [(0.0f32, 0.0f32); (LIFT / 2 + 4) as usize];
            for (k, v) in fields.iter_mut().enumerate() {
                *v = tunnel_fields(s2, wx, k as i32 * 2, wz);
            }
            let mut halls = [0.0f32; (LIFT / 4 + 3) as usize];
            for (k, v) in halls.iter_mut().enumerate() {
                *v = cavern_field(s2, wx, k as i32 * 4 - 8, wz);
            }
            // the old world's lowest caves held lava, which now drains into the slate
            // (in new chunks: saved ones keep what they had)
            if fresh {
                for y in LIFT + 1..=LIFT + LAVA_LEVEL {
                    if c.blocks[bidx(x, y, z)] == LAVA {
                        c.blocks[bidx(x, y, z)] = AIR;
                    }
                }
            }
            for y in 0..=LIFT + 2 {
                let i = bidx(x, y, z);
                let h = hash2(seed ^ 0xbed, wx, wz ^ (y * 7919));
                if y == 0 || (y <= 3 && h % 4 < 4 - y as u32) {
                    c.blocks[i] = BEDROCK;
                    continue;
                }
                let (p, q) = (fields[(y / 2) as usize], fields[(y / 2 + y % 2) as usize]);
                let tunnel = in_tunnel(s2, wx, y, wz, (p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
                if y >= LIFT {
                    // where the old bedrock was: slate, holed by the slate's tunnels so
                    // they meet the caves above (never under water)
                    let here = c.blocks[i];
                    let dry = !is_fluid(c.blocks[bidx(x, y + 1, z)]) && !is_fluid(c.blocks[bidx(x, y + 2, z)]);
                    if fresh && tunnel && dry && (here == BEDROCK || here == STONE || here == SLATE || here == AIR) {
                        c.blocks[i] = AIR;
                    } else if here == BEDROCK || (y == LIFT && here == AIR) {
                        c.blocks[i] = SLATE;
                    }
                    continue;
                }
                let (k, t) = (((y + 8) / 4) as usize, ((y + 8) % 4) as f32 * 0.25);
                let hall = halls[k] + (halls[k + 1] - halls[k]) * t;
                // (the halls are big down here: the field is read at its deepest)
                let cavern = y > 3 && y < LIFT - 4 && in_cavern(s2, wx, y - 6, wz, hall);
                c.blocks[i] = if tunnel || cavern {
                    if y <= 10 { LAVA } else { AIR }
                } else {
                    deep_ore(seed, wx, y, wz)
                };
            }
        }
    }
}

/// Ore in the deep slate: the better kinds are commoner here than anywhere above.
fn deep_ore(seed: u32, x: i32, y: i32, z: i32) -> Block {
    let (xf, yf, zf) = (x as f32, y as f32, z as f32);
    let n = perlin3(seed.wrapping_add(9100), xf / 5.0, yf / 5.0, zf / 5.0);
    if n < 0.55 {
        return SLATE;
    }
    let pick = hash2(seed ^ 0x0de_e9 ^ ((y + 4) as u32 >> 3).wrapping_mul(0x9e37), (x + 4) >> 3, (z + 4) >> 3) % 100;
    match pick {
        0..=21 => CRYSTAL_ORE,
        22..=43 => GOLD_ORE,
        44..=79 => IRON_ORE,
        _ => COAL_ORE,
    }
}

// ---------------------------------------------------------------- chunk data

#[derive(Clone)]
pub struct ChunkData {
    pub blocks: Box<[Block]>,
}

#[inline]
pub(crate) fn bidx(x: i32, y: i32, z: i32) -> usize {
    ((y * CW + z) * CW + x) as usize
}

impl ChunkData {
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        self.blocks[bidx(x, y, z)]
    }
}

pub fn generate(seed: u32, cx: i32, cz: i32) -> ChunkData {
    let mut blocks = vec![AIR; GVOL].into_boxed_slice();
    // heights for the chunk plus a 2-block apron (needed for trees crossing borders)
    const A: i32 = 2;
    const HW: i32 = CW + 2 * A;
    let mut hs = [0i32; (HW * HW) as usize];
    for z in 0..HW {
        for x in 0..HW {
            hs[(z * HW + x) as usize] = height_at(seed, cx * CW + x - A, cz * CW + z - A);
        }
    }
    let hget = |x: i32, z: i32| hs[((z + A) * HW + x + A) as usize];

    for z in 0..CW {
        for x in 0..CW {
            let h = hget(x, z);
            let (wx, wz) = (cx * CW + x, cz * CW + z);
            let desert = h > GSEA + 1 && h < 82 && is_desert(seed, wx, wz);
            // lowest neighbouring surface: caves stay clear of sea and lake beds
            let seabed = if h > GSEA + 1 {
                SAND
            } else {
                let g = fbm(seed.wrapping_add(11000), wx as f32 / 38.0, wz as f32 / 38.0, 2);
                if g > 0.3 {
                    GRAVEL
                } else if g < -0.36 && h < GSEA {
                    CLAY
                } else {
                    SAND
                }
            };
            let low = h.min(hget(x - 1, z)).min(hget(x + 1, z)).min(hget(x, z - 1)).min(hget(x, z + 1));
            // the cavern field up the column, at y = 0, 4, 8, ...
            let mut halls = [0.0f32; (CAVERN_TOP / 4 + 2) as usize];
            for (k, v) in halls.iter_mut().enumerate() {
                *v = cavern_field(seed, wx, k as i32 * 4, wz);
            }
            // and the tunnel fields at y = 0, 2, 4, ...
            let mut fields = [(0.0f32, 0.0f32); (GH / 2 + 1) as usize];
            for (k, v) in fields.iter_mut().enumerate().take((h / 2 + 2) as usize) {
                *v = tunnel_fields(seed, wx, k as i32 * 2, wz);
            }
            let tunnel = |y: i32| {
                let (p, q) = (fields[(y / 2) as usize], fields[(y / 2 + y % 2) as usize]);
                in_tunnel(seed, wx, y, wz, (p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5)
            };
            let cavern = |y: i32| {
                let (k, t) = ((y / 4) as usize, (y % 4) as f32 * 0.25);
                y < low - 7 && y <= CAVERN_TOP && in_cavern(seed, wx, y, wz, halls[k] + (halls[k + 1] - halls[k]) * t)
            };
            for y in 0..=h {
                let mut b = if y == 0 {
                    BEDROCK
                } else if h <= GSEA + 1 {
                    if y > h - 3 {
                        seabed
                    } else {
                        STONE
                    }
                } else if h >= 98 {
                    if y > h - 2 {
                        SNOW
                    } else {
                        STONE
                    }
                } else if h >= 86 {
                    STONE
                } else if desert {
                    if y > h - 3 {
                        SAND
                    } else if y > h - 7 {
                        SANDSTONE
                    } else {
                        STONE
                    }
                } else if y == h {
                    GRASS
                } else if y > h - 4 {
                    DIRT
                } else {
                    STONE
                };
                // caves; the caverns keep a roof of rock under the land and the sea bed
                if b != BEDROCK && !(y <= GSEA + 1 && y >= low - 4) && (tunnel(y) || cavern(y)) {
                    if y <= LAVA_LEVEL {
                        blocks[bidx(x, y, z)] = LAVA;
                    }
                    continue;
                }
                if b == STONE && y < h - 3 {
                    b = ore_at(seed, wx, y, wz);
                }
                blocks[bidx(x, y, z)] = b;
            }
            for y in (h + 1)..=GSEA {
                blocks[bidx(x, y, z)] = WATER;
            }
            if desert && h + 4 < GH {
                let r = hash2(seed ^ 0x0cac_7005, wx, wz);
                if r % 230 == 0 && blocks[bidx(x, h, z)] == SAND {
                    for dy in 1..=(2 + (r >> 9) % 2) as i32 {
                        blocks[bidx(x, h + dy, z)] = CACTUS;
                    }
                }
            }
        }
    }

    if let Some((x, y, z)) = cave_chest(seed, cx, cz, &blocks, &hget) {
        blocks[bidx(x, y, z)] = LOOT_CHEST;
    }

    // trees
    for tz in -A..CW + A {
        for tx in -A..CW + A {
            let h = hget(tx, tz);
            if h <= GSEA + 1 || h >= 86 {
                continue;
            }
            let (wx, wz) = (cx * CW + tx, cz * CW + tz);
            if is_desert(seed, wx, wz) || is_cave(seed, wx, h, wz) {
                continue;
            }
            let r = hash2(seed ^ 0x9e37_79b9, wx, wz);
            let forest = fbm(seed.wrapping_add(5000), wx as f32 / 110.0, wz as f32 / 110.0, 2);
            let p = 0.0025 + forest.max(0.0) * 0.07;
            if ((r & 0xffff) as f32) / 65536.0 >= p {
                continue;
            }
            let mut put = |x: i32, y: i32, z: i32, b: Block, force: bool| {
                if x >= 0 && x < CW && z >= 0 && z < CW && y > 0 && y < GH {
                    let i = bidx(x, y, z);
                    if force || blocks[i] == AIR {
                        blocks[i] = b;
                    }
                }
            };
            if h >= 68 + ((r >> 24) % 7) as i32 {
                // pine: a tall trunk with tiers of needles narrowing to a point
                let th = 6 + ((r >> 16) % 3) as i32;
                for dy in 2..=th + 1 {
                    let rad: i32 = if dy == th + 1 {
                        0
                    } else if dy == th {
                        1
                    } else {
                        1 + (th - dy) % 2
                    };
                    for dz in -rad..=rad {
                        for dx in -rad..=rad {
                            if rad > 0 && dx.abs() == rad && dz.abs() == rad {
                                continue;
                            }
                            put(tx + dx, h + dy, tz + dz, PINE_LEAVES, false);
                        }
                    }
                }
                for dy in 1..=th {
                    put(tx, h + dy, tz, LOG, true);
                }
                continue;
            }
            let birch = (r >> 20) % 4 == 0;
            let (log, leaves) = if birch { (BIRCH_LOG, BIRCH_LEAVES) } else { (LOG, LEAVES) };
            let th = 4 + ((r >> 16) % 3) as i32 + birch as i32;
            for dy in (th - 2)..=(th + 1) {
                let rad: i32 = if dy <= th - 1 { 2 } else { 1 };
                for dz in -rad..=rad {
                    for dx in -rad..=rad {
                        if dx.abs() == rad && dz.abs() == rad {
                            let keep = rad == 2
                                && dy == th - 2
                                && hash2(seed ^ 77, wx + dx * 7, wz + dz * 13 + dy) & 1 == 0;
                            if !keep {
                                continue;
                            }
                        }
                        put(tx + dx, h + dy, tz + dz, leaves, false);
                    }
                }
            }
            for dy in 1..=th {
                put(tx, h + dy, tz, log, true);
            }
        }
    }

    // ground plants: grass tufts, flower patches, mushrooms and berries in the woods
    for z in 0..CW {
        for x in 0..CW {
            let h = hget(x, z);
            if h <= GSEA + 1 || h >= 86 || blocks[bidx(x, h + 1, z)] != AIR {
                continue;
            }
            let (wx, wz) = (cx * CW + x, cz * CW + z);
            let r = hash2(seed ^ 0x71a2_55ed, wx, wz);
            let roll = r % 1000;
            let plant = match blocks[bidx(x, h, z)] {
                GRASS => {
                    let patch = fbm(seed.wrapping_add(12000), wx as f32 / 28.0, wz as f32 / 28.0, 2);
                    let forest = fbm(seed.wrapping_add(5000), wx as f32 / 110.0, wz as f32 / 110.0, 2);
                    let flowers = if patch.abs() > 0.3 { 70 } else { 3 };
                    let woods = if forest > 0.12 { 14 } else { 2 };
                    if roll < 120 {
                        TALL_GRASS
                    } else if roll < 120 + flowers {
                        if patch > 0.0 { FLOWER_RED } else { FLOWER_YELLOW }
                    } else if roll < 120 + flowers + woods {
                        MUSHROOM
                    } else if roll < 120 + flowers + woods * 2 {
                        BERRY_BUSH
                    } else {
                        AIR
                    }
                }
                SAND if h > GSEA + 2 && roll < 9 => DEAD_BUSH,
                _ => AIR,
            };
            if plant != AIR {
                blocks[bidx(x, h + 1, z)] = plant;
            }
        }
    }
    ChunkData { blocks }
}

// ---------------------------------------------------------------- meshing

pub struct MeshData {
    /// Shared light levels in the center chunk, for simulation queries.
    pub block_light: Arc<[u8]>,
    /// packed quad vertices (4 per quad): opaque quads first, then transparent
    pub verts: Vec<[u32; 2]>,
    pub opaque_quads: u32,
    /// how many of the opaque quads, the last ones, are buried: walls of caves well
    /// below the ground that no daylight reaches. From afar they cannot be seen from
    /// above ground, and need not be drawn.
    pub deep_quads: u32,
    /// plants: alpha-tested, drawn double sided
    pub cut_quads: u32,
    pub trans_quads: u32,
    /// shaped blocks (stairs, slabs, fences, doors...): quads of two words per vertex,
    /// x 9 bits | y 13 | z 9 in sixteenths of a block, then face 3 | layer 16 | sky 2 | brightness 8
    pub detail: Vec<u32>,
    /// highest y used by any vertex (for tight frustum culling)
    pub top: f32,
}

const PW: usize = (CW + 2) as usize;
const PH: usize = (CH + 2) as usize;
const MESH_SKIP: u8 = 1;
const MESH_OPAQUE: u8 = 2;

fn mesh_flags() -> &'static [u8; crate::registry::LIMIT] {
    static FLAGS: std::sync::LazyLock<[u8; crate::registry::LIMIT]> = std::sync::LazyLock::new(|| std::array::from_fn(|i| {
        let b = i as Block;
        let skip = b == AIR || is_torch(b) || is_plant(b) || is_shaped(b) || is_gate(b);
        skip as u8 | (is_opaque(b) as u8) << 1 | fluid_kind(b) << 2
    }));
    &FLAGS
}

/// Greedy mesher with ambient occlusion. `n` is the 3x3 neighbourhood, index (dz+1)*3+(dx+1).
/// A terrain vertex's corner: x and z (0..=16) share 9 bits, y (0..=256) has the next 9.
/// The shader (`vs_chunk`) undoes this.
#[inline]
fn pack_xyz(x: u32, y: u32, z: u32) -> u32 {
    (x + z * 17) | y << 9
}

#[cfg(test)]
pub fn mesh(n: &[Arc<ChunkData>; 9]) -> MeshData {mesh_cached(n,None)}
fn mesh_cached(n:&[Arc<ChunkData>;9], cached:Option<Box<[Arc<[u8]>;9]>>)->MeshData {
    let flags = mesh_flags();
    let lighting = cached.map_or_else(||crate::lighting::Field::build(n),crate::lighting::Field::cached);
    // padded copy so neighbour lookups never branch
    let mut pad = vec![AIR; PW * PW * PH];
    for i in 0..PW * PW {
        pad[i] = BEDROCK; // below the world: hide bottom faces
    }
    for y in 0..CH {
        for z in -1..=CW {
            let (cz, lz) = if z < 0 {
                (0, CW - 1)
            } else if z >= CW {
                (2, 0)
            } else {
                (1, z)
            };
            let row = ((y + 1) as usize * PW + (z + 1) as usize) * PW;
            let src = bidx(0, y, lz);
            pad[row] = n[cz * 3].blocks[src + (CW - 1) as usize];
            pad[row + 1..row + 1 + CW as usize]
                .copy_from_slice(&n[cz * 3 + 1].blocks[src..src + CW as usize]);
            pad[row + 1 + CW as usize] = n[cz * 3 + 2].blocks[src];
        }
    }
    // highest roof block per column, two columns beyond the chunk, for sky exposure
    const TW: i32 = CW + 4;
    let mut tops = [0i32; (TW * TW) as usize];
    for z in -2..CW + 2 {
        for x in -2..CW + 2 {
            let c = &n[(((z + CW) / CW) * 3 + (x + CW) / CW) as usize];
            let (lx, lz) = (x & 15, z & 15);
            let mut y = CH - 1;
            while y > 0 {
                let b = c.blocks[bidx(lx, y, lz)];
                if roofs(b) {
                    break;
                }
                y -= 1;
            }
            tops[((z + 2) * TW + x + 2) as usize] = y;
        }
    }
    let top_at = |x: i32, z: i32| tops[((z + 2) * TW + x + 2) as usize];
    // faces below this that no daylight reaches count as buried
    let buried_below = tops.iter().copied().min().unwrap_or(0) - 6;
    let mut deep: Vec<[u32; 2]> = Vec::new();
    // 2 = open sky, 1 = next to open sky, 0 = enclosed
    let sky_at = |x: i32, y: i32, z: i32| -> u32 {
        if y > top_at(x, z) {
            2
        } else if y > top_at(x - 1, z) || y > top_at(x + 1, z) || y > top_at(x, z - 1) || y > top_at(x, z + 1) {
            1
        } else {
            0
        }
    };
    let stride: [isize; 3] = [1, (PW * PW) as isize, PW as isize]; // x, y, z
    // nothing above the chunk's highest block has a face: the empty sky is skipped
    let layer = (CW * CW) as usize;
    let ylim = (0..CH as usize).rev().find(|&y| n[4].blocks[y * layer..(y + 1) * layer].iter().any(|&b| b != AIR)).map_or(1, |y| y as i32 + 1);
    let dims: [i32; 3] = [CW, ylim, CW];
    let pidx = |p: [i32; 3]| -> isize {
        (((p[1] + 1) as usize * PW + (p[2] + 1) as usize) * PW + (p[0] + 1) as usize) as isize
    };
    let at = |i: isize| -> Block { pad[i as usize] };
    // The surface of a fluid at the corner (x, z) of the grid on level y, as a code for
    // the vertex (3 = full height .. 0 = a quarter): full where it touches a still block
    // or fluid falling from above, otherwise the mean of the flowing cells round the
    // corner. Cells that share a corner share its height, so the surface is unbroken.
    let corner = |x: i32, y: i32, z: i32, kind: u8| -> u32 {
        let (mut sum, mut n) = (0.0f32, 0u32);
        for (dx, dz) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
            let c = at(pidx([x + dx, y, z + dz]));
            if fluid_kind(c) != kind {
                continue;
            }
            let h = fluid_height(c);
            if h >= 1.0 || fluid_kind(at(pidx([x + dx, y + 1, z + dz]))) == kind {
                return 3;
            }
            sum += h;
            n += 1;
        }
        if n == 0 {
            return 3;
        }
        3 - ((1.0 - sum / n as f32) * 4.0).round().clamp(0.0, 3.0) as u32
    };

    let mut opaque: Vec<[u32; 2]> = Vec::with_capacity(4096);
    let mut trans: Vec<[u32; 2]> = Vec::new();
    let mut mask = vec![0u64; (CW * CH) as usize];

    for face in 0..6usize {
        let (a, u, v) = match face / 2 {
            0 => (0usize, 1usize, 2usize),
            1 => (1, 2, 0),
            _ => (2, 0, 1),
        };
        let positive = face % 2 == 0;
        let (na, nu, nv) = (dims[a], dims[u] as usize, dims[v] as usize);
        let sa = if positive { stride[a] } else { -stride[a] };
        let (su, sv) = (stride[u], stride[v]);

        for s in 0..na {
            let mut any = false;
            for vv in 0..nv {
                for uu in 0..nu {
                    let mut p = [0i32; 3];
                    p[a] = s;
                    p[u] = uu as i32;
                    p[v] = vv as i32;
                    let i = pidx(p);
                    let b = at(i);
                    let material = flags[b as usize];
                    let m = &mut mask[vv * nu + uu];
                    *m = 0;
                    if material & MESH_SKIP != 0 {
                        continue;
                    }
                    let q = i + sa;
                    let nb = at(q);
                    let op = material & MESH_OPAQUE != 0;
                    let fk = material >> 2;
                    let neighbour = flags[nb as usize];
                    let mut visible = if op {
                        neighbour & MESH_OPAQUE == 0 && !(fk != 0 && neighbour >> 2 == fk)
                    } else {
                        nb != b && neighbour & MESH_OPAQUE == 0 && !(fk != 0 && neighbour >> 2 == fk)
                    };
                    // a fluid's corners stand lower the weaker it runs: the two bits of
                    // shading each vertex has carry that instead (see `vs_chunk`)
                    let mut heights = None;
                    if fk != 0 && (visible || (face == 2 && neighbour >> 2 != fk)) {
                        let mut codes = [3u32; 4];
                        for (ci, (cu, cv)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
                            let mut c = p;
                            c[a] += positive as i32;
                            c[u] += cu;
                            c[v] += cv;
                            if c[1] == p[1] + 1 {
                                codes[ci] = corner(c[0], p[1], c[2], fk);
                            }
                        }
                        // a lowered surface shows under whatever is above it
                        visible |= face == 2 && codes.iter().any(|&c| c < 3);
                        heights = Some(codes[0] | codes[1] << 2 | codes[2] << 4 | codes[3] << 6);
                    }
                    if !visible {
                        continue;
                    }
                    let mut ao = 0xffu32;
                    if let Some(h) = heights {
                        ao = h;
                    } else if op {
                        let o = |i: isize| ((flags[at(i) as usize] & MESH_OPAQUE) != 0) as u32;
                        let (um, up, vm, vp) = (o(q - su), o(q + su), o(q - sv), o(q + sv));
                        let c = |s1: u32, s2: u32, ci: isize| -> u32 {
                            if s1 + s2 == 2 {
                                0
                            } else {
                                3 - (s1 + s2 + o(ci))
                            }
                        };
                        ao = c(um, vm, q - su - sv)
                            | c(up, vm, q + su - sv) << 2
                            | c(up, vp, q + su + sv) << 4
                            | c(um, vp, q - su + sv) << 6;
                    }
                    let mut qp = p;
                    qp[a] += if positive { 1 } else { -1 };
                    let sky = sky_at(qp[0], qp[1], qp[2]);
                    // (flowing lava hides nothing beside it, but is drawn with the solid blocks)
                    let solid = op || fk == 2;
                    *m = 1 << 63 | (!solid as u64) << 62 | (lighting.at(qp[0], qp[1], qp[2]) as u64) << 26 | (sky as u64) << 24 | (ao as u64) << 16 | tex_for(b, face) as u64;
                    any = true;
                }
            }
            if !any {
                continue;
            }
            for vv in 0..nv {
                let mut uu = 0;
                while uu < nu {
                    let k = mask[vv * nu + uu];
                    if k == 0 {
                        uu += 1;
                        continue;
                    }
                    let ao = ((k >> 16) & 0xff) as u32;
                    let aoc = [ao & 3, (ao >> 2) & 3, (ao >> 4) & 3, (ao >> 6) & 3];
                    // only stretch a quad along a direction in which its AO is constant
                    let u_const = aoc[0] == aoc[1] && aoc[3] == aoc[2];
                    let v_const = aoc[0] == aoc[3] && aoc[1] == aoc[2];
                    let mut w = 1;
                    if u_const {
                        while uu + w < nu && mask[vv * nu + uu + w] == k {
                            w += 1;
                        }
                    }
                    let mut h = 1;
                    if v_const {
                        'outer: while vv + h < nv {
                            for i in 0..w {
                                if mask[(vv + h) * nu + uu + i] != k {
                                    break 'outer;
                                }
                            }
                            h += 1;
                        }
                    }
                    for j in 0..h {
                        for i in 0..w {
                            mask[(vv + j) * nu + uu + i] = 0;
                        }
                    }
                    let ac = s + positive as i32;
                    let corners = [
                        (uu, vv),
                        (uu + w, vv),
                        (uu + w, vv + h),
                        (uu, vv + h),
                    ];
                    let mut order: [usize; 4] = if positive { [0, 1, 2, 3] } else { [0, 3, 2, 1] };
                    // pick the diagonal that gives smooth AO interpolation
                    if aoc[order[0]] + aoc[order[2]] > aoc[order[1]] + aoc[order[3]] {
                        order = [order[1], order[2], order[3], order[0]];
                    }
                    let mut quad = [0u32; 4];
                    let mut high = 0;
                    for (i, &ci) in order.iter().enumerate() {
                        let mut p = [0u32; 3];
                        p[a] = ac as u32;
                        p[u] = corners[ci].0 as u32;
                        p[v] = corners[ci].1 as u32;
                        high = high.max(p[1] as i32);
                        quad[i] = pack_xyz(p[0], p[1], p[2])
                            | (face as u32) << 18
                            | ((k as u32) & 127) << 21
                            | aoc[ci] << 28
                            | (((k >> 24) as u32) & 3) << 30;
                    }
                    let out = if k & (1 << 62) != 0 {
                        &mut trans
                    } else if (k >> 24) & 3 == 0 && high < buried_below {
                        &mut deep
                    } else {
                        &mut opaque
                    };
                    out.extend(quad.map(|v| [v, ((k as u32 & 65535) << 8) | crate::lighting::brightness(((k >> 26) as u8)&15)]));
                    uu += w;
                }
            }
        }
    }
    // plants: two crossed diagonal planes per block (face code 6)
    let mut cut: Vec<[u32; 2]> = Vec::new();
    let mut detail: Vec<u32> = Vec::new();
    for y in 1..ylim {
        for z in 0..CW {
            for x in 0..CW {
                let i = pidx([x, y, z]);
                let b = at(i);
                if b == PANE {
                    // a flat upright quad (face code 7) the shader moves to the middle of
                    // the cell: `code` 0 = towards +x, 2 = towards +z
                    let sky = sky_at(x, y, z);
                    let (x, y, z) = (x as u32, y as u32, z as u32);
                    let quad = |out: &mut Vec<[u32; 2]>, code: u32| {
                        let tail = 7 << 18 | PANE_LAYER << 21 | code << 28 | sky << 30;
                        let corners = if code == 0 {
                            [(x, y, z), (x, y, z + 1), (x, y + 1, z + 1), (x, y + 1, z)]
                        } else {
                            [(x, y, z), (x + 1, y, z), (x + 1, y + 1, z), (x, y + 1, z)]
                        };
                        for (px, py, pz) in corners {
                            out.push([pack_xyz(px, py, pz) | tail, PANE_LAYER << 8 | crate::lighting::brightness(lighting.at(x as i32, y as i32, z as i32))]);
                        }
                    };
                    let j = |d: isize| pane_joins(at(i + d));
                    let (ax, az) = pane_axes(j(-stride[0]) || j(stride[0]), j(-stride[2]) || j(stride[2]));
                    if ax {
                        quad(&mut trans, 2);
                    }
                    if az {
                        quad(&mut trans, 0);
                    }
                    continue;
                }
                if is_shaped(b) {
                    let side = |dx: i32, dz: i32| at(i + dx as isize * stride[0] + dz as isize * stride[2]);
                    let (boxes, n) = draw_boxes(b, side);
                    let up = sky_at(x, y + 1, z);
                    // light and neighbour for each face: 0 +X, 1 -X, 2 +Y, 3 -Y, 4 +Z, 5 -Z
                    let next = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];
                    let mut sky = [0u32; 6];
                    let mut hidden = [false; 6];
                    for (f, (dx, dy, dz)) in next.into_iter().enumerate() {
                        let s = sky_at(x + dx, y + dy, z + dz);
                        sky[f] = if f == 3 { s } else { s.max(up) };
                        hidden[f] = is_opaque(at(i + dx as isize * stride[0] + dy as isize * stride[1] + dz as isize * stride[2]));
                    }
                    let fence = is_fence(b);
                    for (k, q) in boxes[..n].iter().enumerate() {
                        // a fence arm is drawn as two rails
                        let rails: &[(f32, f32)] = if fence && k > 0 { &[(0.375, 0.5625), (0.75, 0.9375)] } else { &[(q[1], q[4])] };
                        for &(y0, y1) in rails {
                            let q = [q[0], y0, q[2], q[3], y1, q[5]];
                            let g = |v: f32, base: i32| (base * 16 + (v * 16.0).round() as i32) as u32;
                            let (x0, y0, z0, x1, y1, z1) = (g(q[0], x), g(q[1], y), g(q[2], z), g(q[3], x), g(q[4], y), g(q[5], z));
                            let faces = [
                                ([(x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1)], q[3] >= 1.0),
                                ([(x0, y0, z0), (x0, y0, z1), (x0, y1, z1), (x0, y1, z0)], q[0] <= 0.0),
                                ([(x0, y1, z0), (x0, y1, z1), (x1, y1, z1), (x1, y1, z0)], q[4] >= 1.0),
                                ([(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)], q[1] <= 0.0),
                                ([(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)], q[5] >= 1.0),
                                ([(x0, y0, z0), (x0, y1, z0), (x1, y1, z0), (x1, y0, z0)], q[2] <= 0.0),
                            ];
                            for (f, (corners, outer)) in faces.into_iter().enumerate() {
                                // a face flush with the cell wall is covered by a full block there
                                if outer && hidden[f] {
                                    continue;
                                }
                                let light = lighting.at(x, y, z).max(lighting.at(x + next[f].0, y + next[f].1, z + next[f].2));
                                let tail = f as u32 | tex_for(b, f) << 3 | sky[f] << 19 | crate::lighting::brightness(light) << 21;
                                for (px, py, pz) in corners {
                                    detail.push(px | py << 9 | pz << 22);
                                    detail.push(tail);
                                }
                            }
                        }
                    }
                    continue;
                }
                if !is_plant(b) {
                    continue;
                }
                let tail = 6 << 18 | (tex_for(b, 0)&127) << 21 | 3 << 28 | sky_at(x, y, z) << 30;
                let (x, y, z) = (x as u32, y as u32, z as u32);
                for (z0, z1) in [(z, z + 1), (z + 1, z)] {
                    for (px, py, pz) in [(x, y, z0), (x + 1, y, z1), (x + 1, y + 1, z1), (x, y + 1, z0)] {
                        cut.push([pack_xyz(px, py, pz) | tail, tex_for(b, 0) << 8 | crate::lighting::brightness(lighting.at(x as i32, y as i32, z as i32))]);
                    }
                }
            }
        }
    }
    let dq = (deep.len() / 4) as u32;
    opaque.extend_from_slice(&deep);
    let (oq, cq, tq) = ((opaque.len() / 4) as u32, (cut.len() / 4) as u32, (trans.len() / 4) as u32);
    opaque.extend_from_slice(&cut);
    opaque.extend_from_slice(&trans);
    let top = opaque.iter().map(|v| (v[0] >> 9) & 511).max().unwrap_or(0);
    let top = top.max(detail.chunks_exact(2).map(|v| (((v[0] >> 9) & 8191) + 15) / 16).max().unwrap_or(0)) as f32;
    MeshData { block_light: lighting.center(), verts: opaque, opaque_quads: oq, deep_quads: dq, cut_quads: cq, trans_quads: tq, detail, top }
}

// ---------------------------------------------------------------- world / streaming

#[cfg(target_os = "linux")]
fn lower_thread_priority() {
    unsafe {
        // Keep rendering ahead of chunk work without starving generation whenever
        // the game or the graphics driver has runnable threads.
        libc::setpriority(libc::PRIO_PROCESS, 0, 5);
    }
}

#[cfg(target_os = "windows")]
fn lower_thread_priority() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn lower_thread_priority() {}

enum Job {
    Gen(Key, Option<Arc<ChunkData>>),
    Mesh { key: Key, ticket: u64, version: u32, urgent: bool, neighbours: Box<[Arc<ChunkData>; 9]>, lighting:Option<Box<[Arc<[u8]>;9]>> },
}
enum JobResult {
    Gen(Key, Arc<ChunkData>, bool, bool, Vec<(IVec3, Block)>),
    LoadError(Key, String),
    Mesh(Key, u64, u32, MeshData),
}

fn distance(a: Key, b: Key) -> u64 {
    let dx = (a.0 as i64 - b.0 as i64).unsigned_abs();
    let dz = (a.1 as i64 - b.1 as i64).unsigned_abs();
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

impl Job {
    fn priority(&self, center: Key) -> (u8, u64) {
        let (key, urgent, generation) = match self {
            Job::Gen(key, _) => (*key, false, true),
            Job::Mesh { key, urgent, .. } => (*key, *urgent, false),
        };
        let d = distance(key, center);
        let tier = if urgent { 0 } else if generation && d <= 2 { 1 } else { 2 };
        (tier, d.saturating_mul(2).saturating_add(generation as u64))
    }
}

#[derive(Default)]
struct JobState {
    jobs: Vec<Job>,
    center: Key,
    closed: bool,
}

/// The render thread bounds the queue by `max_in_flight`. Workers choose the nearest
/// useful job at pickup time, so an old travel direction cannot hide a recent edit.
#[derive(Default)]
struct Jobs {
    state: Mutex<JobState>,
    ready: Condvar,
}

impl Jobs {
    fn push(&self, job: Job) {
        let mut state = self.state.lock().unwrap();
        if !state.closed {
            state.jobs.push(job);
            self.ready.notify_one();
        }
    }

    fn pop(&self) -> Option<Job> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.closed { return None; }
            if let Some((i, _)) = state.jobs.iter().enumerate().min_by_key(|(_, job)| job.priority(state.center)) {
                return Some(state.jobs.remove(i));
            }
            state = self.ready.wait(state).unwrap();
        }
    }

    fn set_center(&self, center: Key) {
        self.state.lock().unwrap().center = center;
    }

    fn cancel_mesh(&self, ticket: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        if let Some(i) = state.jobs.iter().position(|job| matches!(job, Job::Mesh { ticket: t, .. } if *t == ticket)) {
            state.jobs.remove(i);
            true
        } else {
            false
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.jobs.clear();
        self.ready.notify_all();
    }
}

pub struct Slot {
    block_light: Option<Arc<[u8]>>,
    pub data: Arc<ChunkData>,
    pub mesh: Option<ChunkMesh>,
    /// world positions of the light-emitting blocks in this chunk
    pub lights: Vec<(IVec3, Block)>,
    dirty: bool,
    pending: Option<(u64, u32)>,
    version: u32,
    modified: bool,
}

pub struct World {
    /// places edited since the game last looked, and what stood there before: fluids
    /// run into them, loose blocks fall, leaves round a felled trunk wilt
    pub touched: Vec<(IVec3, Block)>,
    /// Chunks just loaded, and whether they contain saved edits to resume.
    pub activated: Vec<(Key, bool)>,
    pub seed: u32,
    /// the generator this world's terrain comes from (see `GEN_LATEST`)
    pub ver: u8,
    pub chunks: HashMap<Key, Slot, Fx>,
    gen_pending: HashSet<Key, Fx>,
    jobs: Arc<Jobs>,
    res_rx: Receiver<JobResult>,
    in_flight: usize,
    max_in_flight: usize,
    pub radius: i32,
    /// chunk offsets sorted near -> far; bool = inside mesh radius
    pub spiral: Vec<(i32, i32, bool)>,
    uploads: HashMap<Key, (u32, MeshData), Fx>,
    edited: HashSet<Key, Fx>,
    next_mesh_ticket: u64,
    /// Edits outside the view stay here until a coordinated checkpoint commits them.
    unloaded: HashMap<Key, Arc<ChunkData>, Fx>,
    pub load_error: Option<String>,
    tick: u32,
}

impl Drop for World {
    fn drop(&mut self) {
        self.jobs.close();
    }
}

fn chunk_path(dir: &Path, k: Key) -> PathBuf {
    dir.join(format!("c.{}.{}.bin", k.0, k.1))
}

fn load_updates(path: &Path) -> std::io::Result<Vec<(IVec3, Block)>> {
    let mut touched = Vec::new();
    for line in crate::save::read_optional(path)?.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let parsed = (|| Some((
            IVec3::new(parts.first()?.parse().ok()?, parts.get(1)?.parse().ok()?, parts.get(2)?.parse().ok()?),
            parts.get(3)?.parse::<Block>().ok()?,
        )))();
        match parsed {
            Some((p, was)) if parts.len() == 4 && (1..CH).contains(&p.y) => touched.push((p, was)),
            _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid pending world update")),
        }
    }
    Ok(touched)
}

fn load_chunk(seed: u32, ver: u8, dir: Option<&Path>, k: Key) -> std::io::Result<(Arc<ChunkData>, bool, bool)> {
    let bytes = match dir.map(|d| std::fs::read(chunk_path(d, k))) {
        None => None,
        Some(Ok(bytes)) => Some(bytes),
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Some(Err(e)) => return Err(e),
    };
    let saved = bytes.is_some();
    let (data, modified) = match bytes {
        Some(b) if b.len() == VOL => (ChunkData { blocks: b.into_iter().map(Block::from).collect() }, false),
        Some(b) if b.len() == GVOL => {
            let old: Vec<Block> = b.into_iter().map(Block::from).collect();
            let data = if ver == crate::deep::DEEP_VER { grow(&old, 0) } else { lift_saved(seed, k.0, k.1, &old) };
            (data, true)
        }
        Some(b) if b.starts_with(crate::registry::MAGIC) => (crate::registry::decode(&b)?, false),
        Some(b) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("damaged chunk {k:?}: {} bytes", b.len()))),
        None => (generate_for(ver, seed, k.0, k.1), false),
    };
    Ok((Arc::new(data), modified, saved))
}

/// Finds light sources in a loaded chunk; above the height limit is open air.
fn chunk_lights(k: Key, data: &ChunkData) -> Vec<(IVec3, Block)> {
    data.blocks.iter().enumerate().filter_map(|(i, &b)| {
        let lava = b == LAVA && i & 0x33 == 0x11 && data.blocks.get(i + 256) != Some(&LAVA);
        (is_light(b) || lava).then(|| {
            let i = i as i32;
            (IVec3::new(k.0 * CW + (i & 15), i >> 8, k.1 * CW + ((i >> 4) & 15)), b)
        })
    }).collect()
}

impl World {
    pub fn new(seed: u32, ver: u8, radius: i32, save_dir: Option<PathBuf>) -> World {
        let (res_tx, res_rx) = channel::<JobResult>();
        let jobs = Arc::new(Jobs::default());
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        let workers = std::env::var("CUBEWILD_WORKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or((threads / 2).clamp(1, 4))
            .clamp(1, 8);
        for _ in 0..workers {
            let queue = jobs.clone();
            let tx = res_tx.clone();
            let dir = save_dir.clone();
            std::thread::spawn(move || {
                lower_thread_priority();
                while let Some(job) = queue.pop() {
                    let res = match job {
                        Job::Gen(k, cached) => {
                            match cached.map_or_else(|| load_chunk(seed, ver, dir.as_deref(), k), |data| Ok((data, true, true))) {
                                Ok((data, modified, saved)) => {
                                    let lights = chunk_lights(k, &data);
                                    JobResult::Gen(k, data, modified, saved, lights)
                                }
                                Err(e) => JobResult::LoadError(k, format!("Could not load chunk {k:?}: {e}")),
                            }
                        }
                        Job::Mesh { key, ticket, version, neighbours, lighting, .. } => JobResult::Mesh(key, ticket, version, mesh_cached(&neighbours,lighting)),
                    };
                    if tx.send(res).is_err() {
                        return;
                    }
                }
            });
        }
        let mut w = World {
            touched: Vec::new(),
            activated: Vec::new(),
            seed,
            ver,
            chunks: HashMap::default(),
            gen_pending: HashSet::default(),
            jobs,
            res_rx,
            in_flight: 0,
            max_in_flight: workers * 2,
            radius,
            spiral: Vec::new(),
            uploads: HashMap::default(),
            edited: HashSet::default(),
            next_mesh_ticket: 0,
            unloaded: HashMap::default(),
            load_error: None,
            tick: 0,
        };
        if let Some(dir) = &save_dir {
            match load_updates(&dir.join("updates.txt")) {
                Ok(touched) => w.touched = touched,
                Err(e) => w.load_error = Some(format!("Could not load pending world updates: {e}")),
            }
        }
        w.set_radius(radius);
        w
    }

    /// This is the Emberdeep, not the world above.
    pub fn is_deep(&self) -> bool {
        self.ver == crate::deep::DEEP_VER
    }

    pub fn set_radius(&mut self, r: i32) {
        let r = r.clamp(3, 20);
        self.radius = r;
        let g = r + 1;
        let lim = |r: i32| (r as f32 + 0.5) * (r as f32 + 0.5);
        let mut v = Vec::new();
        for dz in -g..=g {
            for dx in -g..=g {
                let d = (dx * dx + dz * dz) as f32;
                if d <= lim(g) {
                    v.push((dx, dz, d <= lim(r)));
                }
            }
        }
        v.sort_by_key(|&(dx, dz, _)| dx * dx + dz * dz);
        self.spiral = v;
    }

    fn neighbours(&self, k: Key) -> Option<Box<[Arc<ChunkData>; 9]>> {
        let get = |dx, dz| self.chunks.get(&(k.0 + dx, k.1 + dz)).map(|s| s.data.clone());
        Some(Box::new([
            get(-1, -1)?, get(0, -1)?, get(1, -1)?,
            get(-1, 0)?, get(0, 0)?, get(1, 0)?,
            get(-1, 1)?, get(0, 1)?, get(1, 1)?,
        ]))
    }

    /// Streams chunks around `center` (chunk coords). Call once per frame.
    pub fn update(&mut self, gfx: &Gfx, center: Key) {
        self.tick = self.tick.wrapping_add(1);
        self.receive();
        self.upload(gfx, center);
        self.schedule(center);
        self.unload_far(center);
    }

    fn receive(&mut self) {
        while let Ok(res) = self.res_rx.try_recv() {
            self.in_flight -= 1;
            self.accept(res);
        }
    }

    fn accept(&mut self, res: JobResult) {
        match res {
            JobResult::Gen(k, data, modified, saved, lights) => {
                self.gen_pending.remove(&k);
                self.unloaded.remove(&k);
                self.activated.push((k, saved));
                self.chunks.insert(
                    k,
                    Slot {
                        block_light: None,
                        data,
                        mesh: None,
                        lights,
                        dirty: true,
                        pending: None,
                        version: 0,
                        modified,
                    },
                );
            }
            JobResult::LoadError(k, message) => {
                // Keep this key pending: retrying every frame would hide the
                // error in repeated generation and could overwrite the save.
                self.gen_pending.insert(k);
                eprintln!("cubewild: {message}");
                self.load_error = Some(message);
            }
            JobResult::Mesh(k, ticket, ver, m) => {
                if let Some(s) = self.chunks.get_mut(&k) {
                    if s.pending == Some((ticket, ver)) {
                        s.pending = None;
                        if s.version == ver {
                            s.block_light = Some(m.block_light.clone());
                            self.uploads.insert(k, (ver, m));
                        }
                    }
                }
            }
        }
    }

    fn upload(&mut self, gfx: &Gfx, center: Key) {
        // Obsolete results do not consume an upload slot or GPU bandwidth.
        self.uploads.retain(|k, (version, _)| self.chunks.get(k).is_some_and(|s| s.version == *version));
        let mut keys: Vec<_> = self.uploads.keys().copied().collect();
        keys.sort_by_key(|k| (!self.edited.contains(k), distance(*k, center)));
        let start = Instant::now();
        for (i, k) in keys.into_iter().take(4).enumerate() {
            if i > 0 && start.elapsed() >= Duration::from_micros(1000) { break; }
            let (_, mesh) = self.uploads.remove(&k).unwrap();
            let slot = self.chunks.get_mut(&k).unwrap();
            slot.mesh = gfx.upload_mesh(&mesh, slot.mesh.take());
            self.edited.remove(&k);
        }
    }

    fn queue_mesh(&mut self, k: Key, urgent: bool) -> bool {
        let Some(s) = self.chunks.get(&k) else { return false };
        if !s.dirty { return false; }
        if let Some((ticket, version)) = s.pending {
            if version == s.version || !self.jobs.cancel_mesh(ticket) { return false; }
            self.in_flight -= 1;
            self.chunks.get_mut(&k).unwrap().pending = None;
        }
        if self.in_flight >= self.max_in_flight || self.uploads.len() >= 12 { return false; }
        let Some(neighbours) = self.neighbours(k) else { return false };
        let cached:Vec<_> = (-1..=1).flat_map(|dz|(-1..=1).map(move |dx|(k.0+dx,k.1+dz)))
            .map(|key|self.chunks.get(&key).and_then(|s|s.block_light.clone())).collect();
        let lighting=if cached.iter().all(Option::is_some) {Some(Box::new(std::array::from_fn(|i|cached[i].clone().unwrap())))} else {None};
        self.next_mesh_ticket = self.next_mesh_ticket.wrapping_add(1);
        let s = self.chunks.get_mut(&k).unwrap();
        let ticket = self.next_mesh_ticket;
        s.pending = Some((ticket, s.version));
        s.dirty = false;
        self.jobs.push(Job::Mesh { key: k, ticket, version: s.version, urgent, neighbours, lighting });
        self.in_flight += 1;
        true
    }

    fn schedule(&mut self, center: Key) {
        self.jobs.set_center(center);
        let mut edits: Vec<_> = self.edited.iter().copied().collect();
        edits.sort_by_key(|k| distance(*k, center));
        for k in edits {
            self.queue_mesh(k, true);
        }
        // Fill the player's immediate neighbourhood before spending slots on meshes.
        for i in 0..self.spiral.len().min(9) {
            if self.in_flight >= self.max_in_flight { break; }
            let (dx, dz, _) = self.spiral[i];
            let k = (center.0 + dx, center.1 + dz);
            if !self.chunks.contains_key(&k) && self.gen_pending.insert(k) {
                self.jobs.push(Job::Gen(k, self.unloaded.get(&k).cloned()));
                self.in_flight += 1;
            }
        }
        if self.in_flight < self.max_in_flight {
            for i in 0..self.spiral.len() {
                if self.in_flight >= self.max_in_flight {
                    break;
                }
                let (dx, dz, inner) = self.spiral[i];
                let k = (center.0 + dx, center.1 + dz);
                match self.chunks.get(&k) {
                    None => {
                        if self.gen_pending.insert(k) {
                            self.jobs.push(Job::Gen(k, self.unloaded.get(&k).cloned()));
                            self.in_flight += 1;
                        }
                    }
                    Some(s) => {
                        if inner && s.dirty {
                            self.queue_mesh(k, false);
                        }
                    }
                }
            }
        }
    }

    fn unload_far(&mut self, center: Key) {
        if self.tick % 60 == 0 {
            let lim = self.radius + 3;
            let far: Vec<Key> = self
                .chunks
                .keys()
                .filter(|k| (k.0 - center.0).abs() > lim || (k.1 - center.1).abs() > lim)
                .copied()
                .collect();
            for k in far {
                if let Some(s) = self.chunks.remove(&k) {
                    self.edited.remove(&k);
                    self.uploads.remove(&k);
                    if s.modified {
                        self.unloaded.insert(k, s.data);
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub fn stage_chunks(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("updates.txt"), self.encode_updates())?;
        for (&k, data) in &self.unloaded {
            std::fs::write(chunk_path(dir, k), crate::registry::encode(data))?;
        }
        for (&k, s) in &self.chunks {
            if s.modified {
                std::fs::write(chunk_path(dir, k), crate::registry::encode(&s.data))?;
            }
        }
        Ok(())
    }

    pub fn encode_updates(&self) -> String {
        self.touched.iter().map(|(p, was)| format!("{} {} {} {}\n", p.x, p.y, p.z, was)).collect()
    }

    pub fn snapshot_chunks(&self) -> Vec<(Key, Arc<ChunkData>)> {
        self.unloaded.iter().map(|(&k, data)| (k, data.clone()))
            .chain(self.chunks.iter().filter(|(_, s)| s.modified).map(|(&k, s)| (k, s.data.clone())))
            .collect()
    }

    /// New edits made while a snapshot writes stay dirty for the next checkpoint.
    pub fn snapshot_committed(&mut self, chunks: &[(Key, Arc<ChunkData>)]) {
        for (key, saved) in chunks {
            if let Some(slot) = self.chunks.get_mut(key) {
                if Arc::ptr_eq(&slot.data, saved) { slot.modified = false; }
            }
            if self.unloaded.get(key).is_some_and(|current| Arc::ptr_eq(current, saved)) {
                self.unloaded.remove(key);
            }
        }
    }

    /// Keep edits whose simulation neighbourhood is still unloaded for a later tick.
    pub fn take_ready_updates(&mut self) -> Vec<(IVec3, Block)> {
        let pending = std::mem::take(&mut self.touched);
        let mut ready = Vec::new();
        for (p, was) in pending {
            let radius = if is_log(was) { 6.5 } else { 1.0 };
            if self.area_loaded(p.x as f32 + 0.5, p.z as f32 + 0.5, radius) {
                ready.push((p, was));
            } else {
                self.touched.push((p, was));
            }
        }
        ready
    }

    /// Only called after the chunks and player/container state commit together.
    #[cfg(test)]
    pub fn checkpoint_committed(&mut self) {
        self.unloaded.clear();
        for s in self.chunks.values_mut() {
            s.modified = false;
        }
    }

    /// Surface height the generator gave a column (the world may have been dug since).
    pub fn ground_height(&self, x: i32, z: i32) -> i32 {
        ground_height(self.ver, self.seed, x, z)
    }

    pub fn is_loaded(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
    }

    /// Every chunk touched by a horizontal body or neighbourhood is available.
    pub fn area_loaded(&self, x: f32, z: f32, radius: f32) -> bool {
        let (x0, x1) = (((x - radius).floor() as i32) >> 4, ((x + radius).floor() as i32) >> 4);
        let (z0, z1) = (((z - radius).floor() as i32) >> 4, ((z + radius).floor() as i32) >> 4);
        (z0..=z1).all(|cz| (x0..=x1).all(|cx| self.chunks.contains_key(&(cx, cz))))
    }

    #[inline]
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> Block {
        if y < 0 {
            return BEDROCK;
        }
        if y >= CH {
            return AIR;
        }
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(s) => s.data.get(x & 15, y, z & 15),
            None => AIR,
        }
    }

    /// Pending lighting is unknown: spawning and crop growth wait for the worker.
    pub fn block_light(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        if !(0..CH).contains(&y) { return Some(0); }
        self.chunks.get(&(x >> 4, z >> 4))?.block_light.as_ref()
            .map(|light| light[bidx(x & 15, y, z & 15)])
    }

    /// Light sources within `range` blocks of `p`, nearest first.
    pub fn lights_near(&self, p: glam::Vec3, range: f32) -> Vec<(IVec3, Block, f32)> {
        let c = ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
        let r = (range / CW as f32).ceil() as i32;
        let mut v = Vec::new();
        for dz in -r..=r {
            for dx in -r..=r {
                let Some(s) = self.chunks.get(&(c.0 + dx, c.1 + dz)) else { continue };
                for &(q, b) in &s.lights {
                    let d = (q.as_vec3() + 0.5 - p).length();
                    if d < range {
                        v.push((q, b, d));
                    }
                }
            }
        }
        v.sort_by(|a, b| a.2.total_cmp(&b.2));
        v
    }

    /// True if a block of type `b` is within `r` blocks of `p`.
    pub fn block_near(&self, p: glam::Vec3, r: i32, b: Block) -> bool {
        let c = p.floor().as_ivec3();
        for y in (c.y - r).max(0)..=(c.y + r + 1).min(CH - 1) {
            for z in c.z - r..=c.z + r {
                for x in c.x - r..=c.x + r {
                    if self.get_block(x, y, z) == b {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// y of the highest block in a loaded column, ignoring plants and torches.
    pub fn surface_y(&self, x: i32, z: i32) -> Option<i32> {
        let s = self.chunks.get(&(x >> 4, z >> 4))?;
        (0..CH).rev().find(|&y| {
            let b = s.data.get(x & 15, y, z & 15);
            b != AIR && !is_torch(b) && !is_plant(b) && !is_ladder(b)
        })
    }

    fn invalidate(&mut self, k: Key, urgent: bool) {
        let Some(s) = self.chunks.get_mut(&k) else { return };
        s.version = s.version.wrapping_add(1);
        s.dirty = true;
        if urgent { self.edited.insert(k); }
    }

    /// Edits a block and lets the background workers remesh (for changes the player
    /// is not waiting on, such as crops growing).
    pub fn set_block_bg(&mut self, x: i32, y: i32, z: i32, b: Block) {
        if self.write_block(x, y, z, b) {
            self.remesh_edit(x, z, false);
        }
    }

    /// Writes a block without remeshing. False if the spot cannot be edited.
    fn write_block(&mut self, x: i32, y: i32, z: i32, b: Block) -> bool {
        if y < 1 || y >= CH || !crate::registry::valid(b) {
            return false;
        }
        let (lx, lz) = (x & 15, z & 15);
        let Some(s) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return false };
        if s.data.get(lx, y, lz) == BEDROCK {
            return false;
        }
        let was = s.data.get(lx, y, lz);
        if was == b { return false; }
        Arc::make_mut(&mut s.data).blocks[bidx(lx, y, lz)] = b;
        s.modified = true;
        let wp = IVec3::new(x, y, z);
        if self.touched.len() < 4096 {
            self.touched.push((wp, was));
        }
        s.lights.retain(|l| l.0 != wp);
        if is_light(b) {
            s.lights.push((wp, b));
        }
        let emission_changed=crate::lighting::emission(was)!=crate::lighting::emission(b);
        let transmission_changed=crate::lighting::transmission(was)!=crate::lighting::transmission(b);
        // A dark cell with no adjacent light capable of entering it cannot affect
        // propagation. Ordinary unlit building keeps the existing light volumes.
        let relight=emission_changed || (transmission_changed && (
            self.block_light(x,y,z).is_none_or(|l|l>0) ||
            [(1,0,0),(-1,0,0),(0,1,0),(0,-1,0),(0,0,1),(0,0,-1)].into_iter()
                .any(|(dx,dy,dz)|self.block_light(x+dx,y+dy,z+dz).is_none_or(|l|l>1))));
        if relight {
            for dz in -1..=1 {for dx in -1..=1 {if let Some(s)=self.chunks.get_mut(&((x>>4)+dx,(z>>4)+dz)) {s.block_light=None;}}}
        }
        true
    }

    /// Edits many blocks with one remesh per chunk touched (an explosion). Returns the
    /// edits that took (bedrock and unloaded places are left alone).
    pub fn set_many(&mut self, edits: &[(IVec3, Block)]) -> Vec<(IVec3, Block)> {
        let mut done = Vec::new();
        let mut keys: Vec<Key> = Vec::new();
        for &(p, b) in edits {
            let old = self.get_block(p.x, p.y, p.z);
            if self.write_block(p.x, p.y, p.z, b) {
                done.push((p, old));
                let xs: &[i32] = &[-1, 0, 1];
                let zs: &[i32] = &[-1, 0, 1];
                for &dz in zs {
                    for &dx in xs {
                        let k = ((p.x >> 4) + dx, (p.z >> 4) + dz);
                        if !keys.contains(&k) {
                            keys.push(k);
                        }
                    }
                }
            }
        }
        for k in keys {
            self.invalidate(k, true);
        }
        done
    }

    /// Edits a block and the one above it with a single remesh (both halves of a door).
    pub fn set_pair(&mut self, x: i32, y: i32, z: i32, lower: Block, upper: Block) -> bool {
        if y < 1 || y + 1 >= CH || !self.is_loaded(x, z)
            || self.get_block(x, y, z) == BEDROCK || self.get_block(x, y + 1, z) == BEDROCK
        {
            return false;
        }
        let lower = self.write_block(x, y, z, lower);
        let upper = self.write_block(x, y + 1, z, upper);
        if lower || upper { self.remesh_edit(x, z, true); }
        lower || upper
    }

    /// Edits take effect immediately; their meshes are coalesced on background workers.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, b: Block) -> bool {
        if !self.write_block(x, y, z, b) {
            return false;
        }
        self.remesh_edit(x, z, true);
        true
    }

    fn remesh_edit(&mut self, x: i32, z: i32, urgent: bool) {
        let k = (x >> 4, z >> 4);
        let relight=self.chunks.get(&k).is_none_or(|s|s.block_light.is_none());
        let (lx,lz)=(x&15,z&15);
        let xs: &[i32] = if relight {&[-1,0,1]} else if lx<=1 {&[0,-1]} else if lx>=14 {&[0,1]} else {&[0]};
        let zs: &[i32] = if relight {&[-1,0,1]} else if lz<=1 {&[0,-1]} else if lz>=14 {&[0,1]} else {&[0]};
        for &dz in zs {
            for &dx in xs {
                self.invalidate((k.0 + dx, k.1 + dz), urgent);
            }
        }
    }

    /// A small loaded world for tests: flat stone up to y = 9, plus what `extra` puts in.
    #[cfg(test)]
    pub fn flat(extra: impl Fn(i32, i32, i32) -> Option<Block>) -> World {
        let mut w = World::new(1, 1, 2, None);
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut blocks = vec![AIR; VOL].into_boxed_slice();
                for y in 0..CH {
                    for z in 0..CW {
                        for x in 0..CW {
                            blocks[bidx(x, y, z)] = extra(cx * CW + x, y, cz * CW + z).unwrap_or(if y <= 9 { STONE } else { AIR });
                        }
                    }
                }
                let slot = Slot { block_light: None, data: Arc::new(ChunkData { blocks }), mesh: None, lights: Vec::new(), dirty: false, pending: None, version: 0, modified: false };
                w.chunks.insert((cx, cz), slot);
            }
        }
        w
    }

    #[cfg(test)]
    pub(crate) fn bake_test_light(&mut self) {
        let keys:Vec<_>=self.chunks.keys().copied().collect();
        for key in keys {if let Some(n)=self.neighbours(key) {self.chunks.get_mut(&key).unwrap().block_light=Some(crate::lighting::Field::build(&n).center());}}
    }

    /// How open to the sky a cell is, as the mesher has it: 2 open, 1 beside open sky,
    /// 0 enclosed.
    pub fn sky_level(&self, x: i32, y: i32, z: i32) -> u8 {
        let open = |x: i32, z: i32| ((y + 1).max(1)..CH).all(|h| !roofs(self.get_block(x, h, z)));
        if open(x, z) {
            2
        } else if open(x - 1, z) || open(x + 1, z) || open(x, z - 1) || open(x, z + 1) {
            1
        } else {
            0
        }
    }

    /// Boxes of the shaped block `b` at a cell, in world coordinates.
    pub fn shape_at(&self, b: Block, x: i32, y: i32, z: i32) -> Boxes {
        let (mut v, n) = if is_torch(b) {
            let mut v = [[0.0f32; 6]; 5];
            v[0] = torch_box(b);
            (v, 1)
        } else {
            shape_boxes(b, |dx, dz| self.get_block(x + dx, y, z + dz))
        };
        let o = [x as f32, y as f32, z as f32];
        for bx in &mut v[..n] {
            for i in 0..6 {
                bx[i] += o[i % 3];
            }
        }
        (v, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused_world() -> World {
        let mut world = World::flat(|_, _, _| None);
        world.jobs.close();
        world.jobs = Arc::new(Jobs::default());
        world.max_in_flight = 4;
        world
    }

    #[test]
    fn material_edits_reuse_light_but_new_sources_invalidate_every_dependency() {
        let mut world=paused_world();
        let dark:Arc<[u8]>=vec![0;VOL].into();
        for slot in world.chunks.values_mut() {slot.block_light=Some(dark.clone());}
        assert!(world.set_block(8,9,8,DIRT));
        assert!(Arc::ptr_eq(world.chunks[&(0,0)].block_light.as_ref().unwrap(),&dark));
        assert!(world.queue_mesh((0,0),true));
        let Job::Mesh {neighbours,lighting,..}=world.jobs.pop().unwrap() else {panic!("mesh job")};
        assert!(lighting.is_some());
        let cached=mesh_cached(&neighbours,lighting);let fresh=mesh(&neighbours);
        assert_eq!(cached.verts,fresh.verts);assert_eq!(cached.block_light,fresh.block_light);
        world.set_block(8,12,8,PLANKS);
        assert!(Arc::ptr_eq(world.chunks[&(0,0)].block_light.as_ref().unwrap(),&dark));
        world.set_block(8,12,8,AIR);
        assert!(Arc::ptr_eq(world.chunks[&(0,0)].block_light.as_ref().unwrap(),&dark));
        world.set_block(8,12,8,TORCH);
        assert!(world.chunks.values().all(|s|s.block_light.is_none()));
        world.bake_test_light();
        world.set_block(10,12,8,PLANKS);
        assert!(world.chunks[&(0,0)].block_light.is_none(),"building across an illuminated path must rebuild light");
    }

    #[test]
    fn edits_are_immediate_and_mesh_jobs_coalesce_without_losing_border_updates() {
        let mut world = paused_world();
        let before = world.chunks[&(0, 0)].data.clone();
        assert!(world.set_block(15, 12, 15, PLANKS));
        assert_eq!(world.get_block(15, 12, 15), PLANKS);
        assert_eq!(before.get(15, 12, 15), AIR, "a running mesh keeps its immutable snapshot");
        for key in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert!(world.chunks[&key].dirty && world.edited.contains(&key));
        }
        assert_eq!(world.in_flight, 0, "editing does no synchronous mesh work");
        let version = world.chunks[&(0, 0)].version;
        assert!(!world.set_block(15, 12, 15, PLANKS));
        assert_eq!(world.chunks[&(0, 0)].version, version, "identical writes do no work");
        assert!(world.queue_mesh((0, 0), true));
        assert!(world.set_block(15, 12, 15, GLASS));
        assert!(world.queue_mesh((0, 0), true));
        assert_eq!(world.in_flight, 1, "the queued stale job was replaced");
        let Job::Mesh { key, ticket, version, neighbours, .. } = world.jobs.pop().unwrap() else { panic!("mesh job") };
        assert_eq!(neighbours[4].get(15, 12, 15), GLASS);
        world.accept(JobResult::Mesh(key, ticket, version, mesh(&neighbours)));
        let (_, ready) = &world.uploads[&(0, 0)];
        let expected = mesh(&world.neighbours((0, 0)).unwrap());
        assert_eq!(ready.verts, expected.verts);
        assert_eq!(ready.detail, expected.detail);
    }

    #[test]
    fn an_edit_during_meshing_discards_the_old_result_and_builds_the_latest_state() {
        let mut world = paused_world();
        world.set_block(8, 12, 8, PLANKS);
        assert!(world.queue_mesh((0, 0), true));
        let Job::Mesh { key, ticket, version, neighbours, .. } = world.jobs.pop().unwrap() else { panic!("mesh job") };
        world.set_block(8, 12, 8, LAMP);
        assert!(!world.queue_mesh(key, true), "already running work completes before rescheduling");
        world.accept(JobResult::Mesh(key, ticket, version, mesh(&neighbours)));
        world.in_flight -= 1;
        assert!(world.uploads.is_empty());
        assert!(world.chunks[&key].dirty && world.chunks[&key].pending.is_none());
        assert!(world.queue_mesh(key, true));
        let Job::Mesh { ticket, version, neighbours, .. } = world.jobs.pop().unwrap() else { panic!("mesh job") };
        assert_eq!(neighbours[4].get(8, 12, 8), LAMP);
        world.accept(JobResult::Mesh(key, ticket, version, mesh(&neighbours)));
        assert_eq!(world.uploads.len(), 1);
        assert_eq!(world.uploads[&key].0, world.chunks[&key].version);
    }

    #[test]
    fn old_mesh_tickets_cannot_replace_a_reloaded_chunk_or_its_pending_job() {
        let mut world = paused_world();
        let key = (0, 0);
        world.chunks.get_mut(&key).unwrap().pending = Some((20, 0));
        world.accept(JobResult::Mesh(key, 10, 0, mesh(&world.neighbours(key).unwrap())));
        assert_eq!(world.chunks[&key].pending, Some((20, 0)));
        assert!(world.uploads.is_empty());
    }

    #[test]
    fn worker_pickup_prioritizes_edits_and_tracks_the_current_player_position() {
        let world = paused_world();
        let jobs = Jobs::default();
        jobs.push(Job::Gen((0, 0), None));
        jobs.push(Job::Gen((10, 10), None));
        jobs.push(Job::Mesh { key: (5, 5), ticket: 1, version: 1, urgent: true, neighbours: world.neighbours((0, 0)).unwrap(), lighting:None });
        jobs.set_center((10, 10));
        assert!(matches!(jobs.pop(), Some(Job::Mesh { ticket: 1, .. })));
        assert!(matches!(jobs.pop(), Some(Job::Gen((10, 10), _))));
        assert!(matches!(jobs.pop(), Some(Job::Gen((0, 0), _))));
        let jobs = Arc::new(jobs);
        let worker = jobs.clone();
        let (done_tx, done_rx) = channel();
        let handle = std::thread::spawn(move || { done_tx.send(worker.pop().is_none()).unwrap(); });
        jobs.close();
        assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        handle.join().unwrap();
    }

    #[test]
    fn a_background_checkpoint_keeps_inventory_and_terrain_from_the_same_tick() {
        let root = crate::save::tests::TestDir::new("background-checkpoint");
        let mut world = World::flat(|x, y, z| (y == 10 && z == 1 && (x == 1 || x == 2)).then_some(LOG));
        let mut inv = crate::items::Inventory::new();
        world.set_block(1, 10, 1, AIR);
        inv.add(LOG, 1);
        let chunks = world.snapshot_chunks();
        let mut snapshot = crate::save::Snapshot::new(root.0.clone());
        snapshot.text("player.txt", inv.save_stacks());
        for (key, data) in &chunks { snapshot.chunk(chunk_path(Path::new(""), *key), data.clone()); }
        let writer = crate::save::Background::start(snapshot).unwrap();
        world.set_block(2, 10, 1, AIR);
        inv.add(LOG, 1);
        writer.wait().unwrap();
        world.snapshot_committed(&chunks);
        let (saved, _, _) = load_chunk(1, 1, Some(&root.0), (0, 0)).unwrap();
        assert_eq!((saved.get(1, 10, 1), saved.get(2, 10, 1)), (AIR, LOG));
        assert_eq!(std::fs::read_to_string(root.0.join("player.txt")).unwrap(), "slots 0:5:1:0\nheld 0 0 0\n");
        assert_eq!(inv.count(LOG), 2);
        assert!(world.chunks[&(0, 0)].modified, "newer edits remain dirty after the older snapshot commits");
        assert_eq!(world.snapshot_chunks().len(), 1);
    }

    #[test]
    fn completing_a_snapshot_cleans_only_unchanged_loaded_and_unloaded_chunks() {
        let mut world = paused_world();
        world.set_block(1, 10, 1, PLANKS);
        world.set_block(17, 10, 1, BRICK);
        let unloaded = world.chunks.remove(&(1, 0)).unwrap();
        world.unloaded.insert((1, 0), unloaded.data);
        let first = world.snapshot_chunks();
        world.set_block(1, 10, 1, GLASS);
        world.snapshot_committed(&first);
        assert!(world.unloaded.is_empty(), "unchanged unloaded data was saved");
        assert!(world.chunks[&(0, 0)].modified);
        let second = world.snapshot_chunks();
        assert_eq!(second.len(), 1);
        world.snapshot_committed(&second);
        assert!(!world.chunks[&(0, 0)].modified);
        assert!(world.snapshot_chunks().is_empty());
    }

    fn material_fixture() -> MeshData {
        let blocks = [STONE, GLASS, WATER, LAVA, LEAVES, SLAB, SLAB_TOP + 2, STAIR_BASE + 17, FENCE, PANE, CHEST, LOOT_CHEST,
            TORCH, WALL_TORCH, TALL_GRASS, WHEAT_2, LADDER_BASE + 1, TRAP_BASE + 4, DOOR_BASE + 37, SUNSHARD, GATE, WOOL + 8];
        let world = World::flat(|x, y, z| {
            if (0..CW).contains(&x) && (0..CW).contains(&z) && y == 12 {
                Some(blocks[(x as usize + z as usize * CW as usize) % blocks.len()])
            } else if x == 3 && y == CH - 1 && z == 3 {
                Some(LAVA)
            } else { None }
        });
        mesh(&world.neighbours((0, 0)).unwrap())
    }

    fn mesh_fingerprint(mesh: &MeshData) -> u64 {
        // Normalize the GPU byte representation back to light levels so the
        // existing geometry/light golden remains valid across packing changes.
        let level=|byte:u32| (0..16u8).find(|&l|crate::lighting::brightness(l)==byte).expect("valid vertex brightness") as u32;
        mesh.verts.iter().flat_map(|v|[v[0],(v[1]>>8)<<4|level(v[1]&255)])
            .chain(mesh.detail.chunks_exact(2).flat_map(|v|[v[0],(v[1]&((1<<21)-1))|level((v[1]>>21)&255)<<21]))
            .chain([mesh.opaque_quads, mesh.deep_quads, mesh.cut_quads, mesh.trans_quads, mesh.top.to_bits()])
            .fold(0xcbf2_9ce4_8422_2325, |hash, word| (hash ^ word as u64).wrapping_mul(0x100_0000_01b3))
    }

    #[test]
    fn material_geometry_lighting_and_fluid_surfaces_are_stable() {
        assert_eq!(mesh_fingerprint(&material_fixture()), 0x2ecb44ebf180e9af);
    }

    #[test]
    fn pending_simulation_edits_survive_checkpoints_and_wait_for_neighbours() {
        let root = crate::save::tests::TestDir::new("pending-updates");
        let mut world = World::flat(|_, _, _| None);
        let changes = vec![(IVec3::new(0, 10, 0), AIR), (IVec3::new(15, 12, 0), LOG)];
        world.touched = changes.clone();
        let checkpoint = crate::save::Checkpoint::begin(&root.0).unwrap();
        world.stage_chunks(&checkpoint.dir()).unwrap();
        checkpoint.commit().unwrap();
        let reloaded = World::new(1, 1, 3, Some(root.0.clone()));
        assert_eq!(reloaded.touched, changes);
        assert!(reloaded.load_error.is_none());
        let chunk = world.chunks.remove(&(1, 0)).unwrap();
        assert_eq!(world.take_ready_updates(), vec![changes[0]]);
        assert_eq!(world.touched, vec![changes[1]]);
        world.chunks.insert((1, 0), chunk);
        assert_eq!(world.take_ready_updates(), vec![changes[1]]);
        assert!(world.touched.is_empty());
    }

    #[test]
    fn legacy_chunks_upgrade_only_when_edited_and_preserve_extended_states() {
        let root=crate::save::tests::TestDir::new("lazy-chunk-upgrade");
        let path=chunk_path(&root.0,(0,0));
        std::fs::write(&path,vec![STONE as u8;VOL]).unwrap();
        let (mut data,dirty,saved)=load_chunk(1,3,Some(&root.0),(0,0)).unwrap();
        assert!(saved && !dirty,"loading an untouched legacy chunk must not schedule a rewrite");
        assert_eq!(std::fs::metadata(&path).unwrap().len(),VOL as u64);
        Arc::make_mut(&mut data).blocks[bidx(1,12,1)]=crate::building::STAIRS+31;
        let mut snapshot=crate::save::Snapshot::new(root.0.clone());
        snapshot.chunk(PathBuf::from("c.0.0.bin"),data);snapshot.commit().unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(crate::registry::MAGIC));
        let (loaded,dirty,_)=load_chunk(1,3,Some(&root.0),(0,0)).unwrap();
        assert!(!dirty);assert_eq!(loaded.get(1,12,1),crate::building::STAIRS+31);
    }

    #[test]
    fn saved_chunks_are_read_and_damaged_ones_are_never_regenerated() {
        let root = crate::save::tests::TestDir::new("chunks");
        let path = chunk_path(&root.0, (2, -3));
        let mut bytes = vec![AIR; VOL];
        bytes[bidx(1, CH - 1, 1)] = LAVA;
        std::fs::write(&path, crate::registry::encode(&ChunkData { blocks: bytes.clone().into_boxed_slice() })).unwrap();
        let (data, modified, saved) = load_chunk(1, 3, Some(&root.0), (2, -3)).unwrap();
        assert!(!modified);
        assert!(saved);
        assert_eq!(data.blocks.as_ref(), bytes.as_slice());
        assert!(chunk_lights((2, -3), &data).contains(&(IVec3::new(33, CH - 1, -47), LAVA)));

        for len in [0, 1, GVOL - 1, VOL - 1, VOL + 1] {
            let damaged = vec![STONE as u8; len];
            std::fs::write(&path, &damaged).unwrap();
            let error = load_chunk(1, 3, Some(&root.0), (2, -3)).err().expect("damaged chunks must fail loading");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert_eq!(std::fs::read(&path).unwrap(), damaged);
        }
        // An absent file is still new terrain, unlike a truncated saved chunk.
        assert!(load_chunk(1, 3, Some(&root.0), (3, -3)).is_ok());
    }

    #[test]
    fn edited_chunks_wait_for_the_player_checkpoint_even_after_unloading() {
        let root = crate::save::tests::TestDir::new("staged-chunks");
        let mut world = World::flat(|_, _, _| None);
        world.set_block_bg(1, 10, 1, PLANKS);
        world.set_block_bg(17, 10, 1, BRICK);
        // A modified chunk leaving view is retained until the next checkpoint.
        let unloaded = world.chunks.remove(&(1, 0)).unwrap();
        world.unloaded.insert((1, 0), unloaded.data);
        let checkpoint = crate::save::Checkpoint::begin(&root.0).unwrap();
        world.stage_chunks(&checkpoint.dir()).unwrap();
        std::fs::write(checkpoint.dir().join("player.txt"), "matching inventory\n").unwrap();
        assert!(!chunk_path(&root.0, (0, 0)).exists());
        assert!(!chunk_path(&root.0, (1, 0)).exists());
        assert!(world.chunks[&(0, 0)].modified);
        assert_eq!(world.unloaded.len(), 1);
        checkpoint.commit().unwrap();
        world.checkpoint_committed();
        assert!(world.unloaded.is_empty());
        assert!(!world.chunks[&(0, 0)].modified);
        for (key, block) in [((0, 0), PLANKS), ((1, 0), BRICK)] {
            let (data, _, _) = load_chunk(1, 1, Some(&root.0), key).unwrap();
            assert_eq!(data.get(1, 10, 1), block);
        }
    }

    #[test]
    fn lava_lights_at_the_height_limit_load_without_panicking() {
        let mut data = ChunkData { blocks: vec![AIR; VOL].into_boxed_slice() };
        data.blocks[bidx(1, CH - 1, 1)] = LAVA;
        data.blocks[bidx(1, CH - 2, 1)] = LAVA;
        data.blocks[bidx(5, 10, 5)] = LAVA;
        data.blocks[bidx(2, CH - 1, 2)] = TORCH;
        let lights = chunk_lights((-2, 3), &data);
        assert!(lights.contains(&(IVec3::new(-31, CH - 1, 49), LAVA)));
        assert!(!lights.contains(&(IVec3::new(-31, CH - 2, 49), LAVA)));
        assert!(lights.contains(&(IVec3::new(-27, 10, 53), LAVA)));
        assert!(lights.contains(&(IVec3::new(-30, CH - 1, 50), TORCH)));
    }

    /// An open hall in the slate near the middle of world 4242 (for screenshots).
    #[test]
    #[ignore]
    fn find_slate_hall() {
        for (cx, cz) in (0..400).map(|i| (i % 20 - 10, i / 20 - 10)) {
            let c = generate_for(3, 4242, cx, cz);
            let open = |x: i32, y: i32, z: i32| c.get(x, y, z) == AIR;
            for y in 14..LIFT - 6 {
                if (0..6).all(|k| open(8, y + k, 8)) && open(3, y + 2, 8) && open(13, y + 2, 8) && open(8, y + 2, 3) && open(8, y + 2, 13) && c.get(8, y - 1, 8) != AIR {
                    println!("HALL {} {} {}", cx * CW + 8, y, cz * CW + 8);
                    return;
                }
            }
        }
    }

    #[test]
    fn the_world_stands_on_deep_slate() {
        let (mut slate, mut open, mut lava, mut ore, mut total) = (0, 0, 0, 0, 0);
        for (cx, cz) in [(0, 0), (5, -3), (-7, 2), (11, 9), (-4, -12), (20, 1)] {
            let c = generate_for(3, 4242, cx, cz);
            assert_eq!(c.blocks.len(), VOL);
            for z in 0..CW {
                for x in 0..CW {
                    assert_eq!(c.get(x, 0, z), BEDROCK);
                    let h = ground_height(3, 4242, cx * CW + x, cz * CW + z);
                    assert!(h > LIFT + 2 && h < CH - 10, "{h}");
                    for y in 1..LIFT {
                        total += 1;
                        match c.get(x, y, z) {
                            SLATE => slate += 1,
                            AIR => open += 1,
                            LAVA => {
                                lava += 1;
                                assert!(y <= 10);
                            }
                            BEDROCK => assert!(y <= 3),
                            _ => ore += 1,
                        }
                    }
                    // no bedrock left where the old world's floor was
                    assert!(c.get(x, LIFT, z) != BEDROCK);
                }
            }
        }
        let f = |n: u32| n as f32 / total as f32;
        assert!(f(slate) > 0.5 && f(open) > 0.02 && f(open) < 0.4 && f(ore) > 0.01 && lava > 0, "slate {} open {} ore {} lava {lava}", f(slate), f(open), f(ore));
        // a chunk saved before the slate is lifted onto it
        let mut old = vec![STONE; GVOL];
        old[bidx(3, 0, 3)] = BEDROCK;
        old[bidx(3, 50, 3)] = PLANKS;
        let c = lift_saved(1, 0, 0, &old);
        assert_eq!((c.get(3, LIFT + 50, 3), c.get(3, LIFT, 3), c.get(3, 0, 3)), (PLANKS, SLATE, BEDROCK));
        // a vertex can name the top of the world
        let v = pack_xyz(16, 256, 16);
        assert_eq!(((v & 511) % 17, (v & 511) / 17, (v >> 9) & 511), (16, 16, 256));
    }

    /// The water faces of a mesh: (face, y of each corner as it is drawn).
    fn water_faces(m: &MeshData) -> Vec<(u32, [f32; 4])> {
        let first = (m.opaque_quads + m.cut_quads) as usize * 4;
        m.verts[first..first + m.trans_quads as usize * 4]
            .chunks_exact(4)
            .filter(|q| q[0][1] >> 8 == 12)
            .map(|q| ((q[0][0] >> 18) & 7, std::array::from_fn(|i| ((q[i][0] >> 9) & 511) as f32 - (3 - ((q[i][0] >> 28) & 3)) as f32 * 0.25)))
            .collect()
    }

    #[test]
    fn running_water_slopes_and_still_water_stays_flat() {
        // a still pool at y 10 on the -x side, running water out along +x, weaker
        // each step, under a stone roof at the far end
        let w = World::flat(|x, y, z| match (x, y) {
            (-3..=0, 10) if (-2..=2).contains(&z) => Some(WATER),
            (1..=6, 10) if z == 0 => Some(WATER_FLOW + (x - 1) as Block),
            (6, 11) if z == 0 => Some(STONE),
            _ => None,
        });
        let m = mesh(&w.neighbours((0, 0)).unwrap());
        let faces = water_faces(&m);
        let tops: Vec<[f32; 4]> = faces.iter().filter(|f| f.0 == 2).map(|f| f.1).collect();
        // the pool's surface is level, at the top of its cells
        assert!(tops.iter().any(|t| t.iter().all(|&y| y == 11.0)));
        // the run sinks as it goes: some corner sits low, none above the pool
        let low = tops.iter().flat_map(|t| t.iter()).fold(11.0f32, |a, &b| a.min(b));
        assert!(low <= 10.5 && tops.iter().flat_map(|t| t.iter()).all(|&y| y <= 11.0 && y > 10.0), "{tops:?}");
        // the weakest cell still shows its surface under the stone over it
        assert!(tops.len() >= 6, "{} tops", tops.len());
    }

    #[test]
    fn shaped_blocks_mesh_into_boxes() {
        // one of each shape standing on a stone floor in the middle chunk
        let shapes = [SLAB, SLAB_TOP + 1, STAIR_BASE + 2, STAIR_BASE + 13, FENCE, CHEST];
        let w = World::flat(|x, y, z| (y == 10 && z == 4 && x >= 2 && x % 2 == 0 && (x - 2) / 2 < 6).then(|| shapes[((x - 2) / 2) as usize]));
        let n = w.neighbours((0, 0)).unwrap();
        let m = mesh(&n);
        // slab 6 faces - 1 hidden by the floor, top slab 6, stairs 2 boxes, fence post, chest, ...
        assert_eq!(m.detail.len() % 8, 0);
        assert_eq!(m.detail.len() / 8, 5 + 6 + 11 + 11 + 5 + 5);
        assert!(m.top >= 11.0);
        // every vertex stays inside the chunk and names a real texture
        for v in m.detail.chunks_exact(2) {
            let (x, y, z) = (v[0] & 511, (v[0] >> 9) & 8191, (v[0] >> 22) & 511);
            assert!(x <= 256 && z <= 256 && (160..=184).contains(&y), "{x} {y} {z}");
            assert!((v[1] & 7) < 6 && (((v[1] >> 3) & 255) as usize) < crate::textures::LAYERS);
        }
        // the collision boxes: a fence is one and a half blocks high, a ladder is no obstacle
        assert_eq!(shape_boxes(FENCE, |_, _| AIR).0[0][4], 1.5);
        assert_eq!(shape_boxes(FENCE, |_, _| STONE).1, 5);
        // stairs rising to +z turn a corner to meet stairs set crosswise before or behind
        let a = STAIR_BASE + 3;
        let with = |x: i32, z: i32, b: Block| shape_boxes(a, move |dx, dz| if (dx, dz) == (x, z) { b } else { AIR });
        let half = [0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
        let outer = with(0, 1, STAIR_BASE + 1);
        assert_eq!((outer.1, outer.0[1]), (2, [0.5, 0.5, 0.5, 1.0, 1.0, 1.0]));
        let inner = with(0, -1, STAIR_BASE + 1);
        assert_eq!((inner.1, inner.0[1], inner.0[2]), (3, half, [0.5, 0.5, 0.0, 1.0, 1.0, 0.5]));
        // a straight run, stairs the other way up and other blocks leave it straight
        for (x, z, b) in [(1, 0, a), (0, 1, STAIR_BASE + 1 + STAIR_FLIP), (0, -1, STONE), (0, 1, a)] {
            let s = with(x, z, b);
            assert_eq!((s.1, s.0[1]), (2, half), "{x} {z} {b}");
        }
        // the sky reaches an open cell, half reaches one beside it, none under a wide roof
        let roofed = World::flat(|x, y, z| (y == 14 && (0..8).contains(&x) && (0..8).contains(&z)).then_some(STONE));
        assert_eq!([roofed.sky_level(12, 10, 4), roofed.sky_level(7, 10, 4), roofed.sky_level(4, 10, 4)], [2, 1, 0]);
        assert!(!is_solid(LADDER_BASE + 2) && is_solid(TRAP_BASE) && !is_opaque(SLAB) && !is_opaque(STAIR_BASE + 31));
        for b in 0..=255u16 {
            assert_eq!(is_shaped(b), b == PANE || is_slab(b) || is_fence(b) || b == CHEST || is_ladder(b) || is_trapdoor(b) || is_door(b) || is_stairs(b) || (TRAP_END..DOOR_BASE).contains(&b), "block {b}");
        }
    }

    /// Run by hand to look at many worlds at once:
    /// `CUBEWILD_MAP_DIR=/some/dir cargo test --release worldgen_survey -- --ignored --nocapture`
    /// prints one line of figures per seed and writes a top-down map of each.
    #[test]
    #[ignore]
    fn worldgen_survey() {
        let dir = std::env::var("CUBEWILD_MAP_DIR").ok();
        let seeds = [0u32, 1, 2, 3, 7, 42, 2026, 131, 1000, 65536, 123_456_789, 987_654_321, 555_555_555, 314_159_265, 271_828_182, 4_294_967_295, 808_080, 90_210, 77_777_777, 600_613];
        for seed in seeds {
            const R: i32 = 768;
            const STEP: i32 = 4;
            let n = (2 * R / STEP) as usize;
            let (mut land, mut high, mut snow, mut desert, mut total) = (0, 0, 0, 0, 0);
            let (mut lo, mut hi) = (GH, 0);
            let mut img = vec![0u8; n * n * 3];
            for (j, z) in (-R..R).step_by(STEP as usize).enumerate() {
                for (i, x) in (-R..R).step_by(STEP as usize).enumerate() {
                    let h = height_at(seed, x, z);
                    total += 1;
                    lo = lo.min(h);
                    hi = hi.max(h);
                    let d = h > GSEA + 1 && h < 82 && is_desert(seed, x, z);
                    let c: [u8; 3] = if h <= GSEA {
                        let k = (h - 3).clamp(0, 40) as u8;
                        [20, 40 + k * 2, 110 + k * 3]
                    } else if h <= GSEA + 1 || d {
                        [222, 206, 150]
                    } else if h >= 98 {
                        [245, 245, 250]
                    } else if h >= 86 {
                        [128, 128, 132]
                    } else {
                        let k = ((h - GSEA) * 2).clamp(0, 90) as u8;
                        [60 + k / 2, 150 - k, 50]
                    };
                    img[(j * n + i) * 3..][..3].copy_from_slice(&c);
                    land += (h > GSEA) as u32;
                    high += (h >= 86) as u32;
                    snow += (h >= 98) as u32;
                    desert += d as u32;
                }
            }
            let pct = |v: u32| v as f32 * 100.0 / total as f32;
            // a closer look at the 8 x 8 chunks around the origin
            let (mut trees, mut ore, mut cave, mut below, mut wet_holes, mut floating) = (0, [0u32; 4], 0u32, 0u32, 0, 0);
            let (mut band, mut lava, mut chests) = ([(0u32, 0u32); 4], 0u32, 0u32);
            for cz in -4..4 {
                for cx in -4..4 {
                    let c = generate(seed, cx, cz);
                    for z in 0..CW {
                        for x in 0..CW {
                            let h = height_at(seed, cx * CW + x, cz * CW + z);
                            for y in 1..GH - 1 {
                                let b = c.get(x, y, z);
                                let under = c.get(x, y - 1, z);
                                if y <= h {
                                    below += 1;
                                    cave += (b == AIR || b == LAVA) as u32;
                                    let k = (y / 12).min(3) as usize;
                                    band[k].0 += 1;
                                    band[k].1 += (b == AIR || b == LAVA) as u32;
                                }
                                lava += (b == LAVA) as u32;
                                chests += (b == LOOT_CHEST) as u32;
                                match b {
                                    LOG | BIRCH_LOG if under != LOG && under != BIRCH_LOG => {
                                        trees += 1;
                                        floating += (under == AIR) as u32;
                                    }
                                    COAL_ORE => ore[0] += 1,
                                    IRON_ORE => ore[1] += 1,
                                    GOLD_ORE => ore[2] += 1,
                                    CRYSTAL_ORE => ore[3] += 1,
                                    WATER => {
                                        // water with nothing to hold it in
                                        let open = under == AIR || (x > 0 && c.get(x - 1, y, z) == AIR && y < GSEA) || (x < CW - 1 && c.get(x + 1, y, z) == AIR && y < GSEA);
                                        wet_holes += open as u32;
                                    }
                                    b if is_plant(b) || b == CACTUS => floating += (under == AIR || under == WATER) as u32,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            println!(
                "seed {seed:>10}: land {:>5.1}%  mountain {:>4.1}%  snow {:>4.1}%  desert {:>4.1}%  height {lo}..{hi}  | near origin: trees {trees:>4}  coal {:>5} iron {:>4} gold {:>3} crystal {:>3}  caves {:.2}% (by depth {:.0}/{:.0}/{:.0}/{:.0}%)  lava {lava}  chests {chests}  open water {wet_holes}  floating {floating}",
                pct(land), pct(high), pct(snow), pct(desert), ore[0], ore[1], ore[2], ore[3], cave as f32 * 100.0 / below as f32,
                band[0].1 as f32 * 100.0 / band[0].0 as f32, band[1].1 as f32 * 100.0 / band[1].0 as f32, band[2].1 as f32 * 100.0 / band[2].0 as f32, band[3].1 as f32 * 100.0 / band[3].0 as f32
            );
            if let Some(d) = &dir {
                let mut out = format!("P6\n{n} {n}\n255\n").into_bytes();
                out.extend_from_slice(&img);
                std::fs::write(format!("{d}/map-{seed}.ppm"), out).unwrap();
            }
        }
    }

    #[test]
    fn worlds_differ_and_hold_together() {
        let seeds: Vec<u32> = (0..120u32).map(|i| hash2(i, 5, 9) % 1_000_000_000).chain([0, 1, 2026, u32::MAX]).collect();
        let (mut flat_top, mut snow) = (0u32, 0u32);
        let mut origin = std::collections::HashSet::new();
        for &seed in &seeds {
            let (mut land, mut total) = (0u32, 0u32);
            for z in (-512..512).step_by(16) {
                for x in (-512..512).step_by(16) {
                    let h = height_at(seed, x, z);
                    assert!((3..=GH - 12).contains(&h));
                    total += 1;
                    land += (h > GSEA) as u32;
                    snow += (h >= 98) as u32;
                    flat_top += (h == GH - 12) as u32;
                }
            }
            // no world is all sea or all land
            let share = land as f32 / total as f32;
            assert!((0.35..0.97).contains(&share), "seed {seed}: land {share}");
            origin.insert(height_at(seed, 0, 0));
            // the start is on level green land, not far from the middle
            let (x, h, z) = spawn_point(seed);
            assert_eq!(h, height_at(seed, x, z));
            assert!(h > GSEA + 2 && h < 78 && !is_desert(seed, x, z), "seed {seed}: spawn {x} {h} {z}");
            assert!(x.abs().max(z.abs()) <= 400, "seed {seed}: spawn {x} {z}");
        }
        // peaks are not sliced off at the build limit, and the middle of the world varies
        assert!(snow > 500 && flat_top == 0, "{flat_top} of {snow}");
        assert!(origin.len() > 20, "{}", origin.len());

        let (mut lava, mut chests, mut rock) = (0u32, 0u32, [(0u32, 0u32); 3]);
        for &seed in &seeds[..6] {
            // the same seed always gives the same chunk, another seed another one
            let (a, b) = (generate(seed, 0, 0), generate(seed, 0, 0));
            assert!(a.blocks == b.blocks);
            let other = seed ^ 0x5bd1_e995;
            let points = (-512..512).step_by(64).flat_map(|z| (-512..512).step_by(64).map(move |x| (x, z)));
            let same = points.filter(|&(x, z)| height_at(seed, x, z) == height_at(other, x, z)).count();
            assert!(same < 64, "seed {seed}: {same} of 256 heights alike");
            // nothing hangs in the air, no water stands beside a hole, and the player's
            // start is not buried
            for (cx, cz) in [(0, 0), (-1, 0), (3, -2), (20, 11)] {
                let c = generate(seed, cx, cz);
                for z in 0..CW {
                    for x in 0..CW {
                        let h = height_at(seed, cx * CW + x, cz * CW + z);
                        assert_eq!(c.get(x, 0, z), BEDROCK);
                        for y in 1..GH - 1 {
                            let (b, under) = (c.get(x, y, z), c.get(x, y - 1, z));
                            if is_plant(b) || b == CACTUS {
                                assert!(is_opaque(under) || under == CACTUS, "seed {seed}: block {b} on {under} at {x} {y} {z}");
                            }
                            if b == WATER {
                                assert!(y <= GSEA && under != AIR, "seed {seed}: water at {x} {y} {z}");
                            }
                            // lava lies only in the deepest caves, and cave chests stand on rock
                            if b == LAVA {
                                assert!(y <= LAVA_LEVEL && under != AIR, "seed {seed}: lava at {x} {y} {z}");
                                lava += 1;
                            }
                            if b == LOOT_CHEST {
                                assert!(is_opaque(under) && c.get(x, y + 1, z) == AIR && y > LAVA_LEVEL && y < h - 6, "seed {seed}: chest at {x} {y} {z}");
                                chests += 1;
                            }
                            // and the caves grow with depth
                            if y <= h && y < 48 {
                                let k = (y / 16) as usize;
                                rock[k].0 += 1;
                                rock[k].1 += (b == AIR || b == LAVA) as u32;
                            }
                            if (b == LOG || b == BIRCH_LOG) && y > h {
                                assert!(under != AIR && under != WATER, "seed {seed}: tree in the air at {x} {y} {z}");
                            }
                        }
                    }
                }
            }
        }
        let share = rock.map(|(all, open)| open as f32 / all as f32);
        assert!(lava > 0 && chests > 0, "lava {lava}, chests {chests}");
        assert!(share[0] > share[1] && share[1] > share[2] && share[0] > 0.08 && share[2] > 0.01, "{share:?}");
        assert!(!is_solid(LAVA) && !is_target(LAVA) && is_replaceable(LAVA) && is_shaped(LOOT_CHEST) && tex_for(LAVA, 2) == LAVA_LAYER);
    }

    /// `cargo test --release gen_speed -- --ignored --nocapture`: time to generate and mesh chunks.
    #[test]
    #[ignore]
    fn gen_speed() {
        for ver in [GEN_LATEST, crate::deep::DEEP_VER] {
            let t = std::time::Instant::now();
            let mut air = 0usize;
            for cz in 0..8 {
                for cx in 0..8 {
                    let chunk = std::hint::black_box(generate_for(ver, 2026, cx, cz));
                    air += chunk.blocks.iter().filter(|b| **b == AIR).count();
                }
            }
            println!("generator {ver}: {:.2} ms a chunk ({air} air blocks)", t.elapsed().as_secs_f64() * 1000.0 / 64.0);
            let n: [Arc<ChunkData>; 9] = std::array::from_fn(|i| Arc::new(generate_for(ver, 2026, (i % 3) as i32, (i / 3) as i32)));
            let mut times = Vec::new();
            let mut vertices = 0;
            for _ in 0..40 {
                let t = std::time::Instant::now();
                let m = std::hint::black_box(mesh(&n));
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                vertices += m.verts.len()*2 + m.detail.len();
            }
            times.sort_by(f64::total_cmp);
            println!("mesh {ver}: mean {:.2} ms, p95 {:.2} ms ({vertices} vertex words)", times.iter().sum::<f64>() / times.len() as f64, times[38]);
        }
        println!("material mesh fingerprint: {:016x}", mesh_fingerprint(&material_fixture()));
    }

    #[test]
    fn worldgen_stats() {
        let mut counts = [0usize; 256];
        let mut solid_cols = 0usize;
        let mut below = 0usize;
        for cz in -6..6 {
            for cx in 0..12 {
                let c = generate(2026, cx, cz);
                for z in 0..CW {
                    for x in 0..CW {
                        let h = height_at(2026, cx * CW + x, cz * CW + z);
                        solid_cols += 1;
                        for y in 1..=h {
                            below += 1;
                            counts[c.get(x, y, z) as usize] += 1;
                        }
                    }
                }
            }
        }
        println!("columns {solid_cols}, cells below surface {below}");
        for (b, n) in counts.iter().enumerate() {
            if *n > 0 {
                println!("block {b}: {n} ({:.2}%)", *n as f32 * 100.0 / below as f32);
            }
        }
        let air = counts[AIR as usize] as f32 / below as f32;
        assert!(air > 0.03 && air < 0.25, "cave fraction {air}");
    }
}
