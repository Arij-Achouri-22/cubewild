//! Voxel world: block types, terrain generation, greedy meshing, chunk streaming.

use crate::gfx::{ChunkMesh, Gfx};
use glam::IVec3;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub const CW: i32 = 16; // chunk width (x and z)
pub const CH: i32 = 128; // chunk height
pub const SEA: i32 = 40;
const VOL: usize = (CW * CW * CH) as usize;

pub type Block = u8;
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
pub const SHAPE_FULL: [Block; 4] = [PLANKS, BIRCH_PLANKS, COBBLE, STONE_BRICK];

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
    (PANE..=CHEST).contains(&b) || b >= SLAB_TOP
}
#[inline]
pub fn is_slab(b: Block) -> bool {
    (SLAB..SLAB_END).contains(&b) || (SLAB_TOP..LADDER_BASE).contains(&b)
}
#[inline]
pub fn slab_top(b: Block) -> bool {
    b >= SLAB_TOP
}
/// index into `SHAPE_FULL`
#[inline]
pub fn slab_mat(b: Block) -> u8 {
    if b >= SLAB_TOP { b - SLAB_TOP } else { b - SLAB }
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
    b >= STAIR_BASE
}
#[inline]
pub fn stair_mat(b: Block) -> u8 {
    (b - STAIR_BASE) >> 3
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
pub fn door_wood(b: Block) -> u8 {
    (b - DOOR_BASE) >> 5
}
/// (side of the cell the door leaf stands on right now, side its hinge is at)
pub fn door_sides(b: Block) -> (u8, u8) {
    let d = b - DOOR_BASE;
    let closed = d & 3;
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
        let s = d & 3;
        let like = |o: Block| is_stairs(o) && (o - STAIR_BASE) & STAIR_FLIP == d & STAIR_FLIP;
        let side = |o: Block| (o - STAIR_BASE) & 3;
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
        v[0] = side_box(b - LADDER_BASE, 0.0, DOOR_THICK);
        1
    } else if is_trapdoor(b) {
        let d = b - TRAP_BASE;
        v[0] = if d & TRAP_OPEN != 0 {
            side_box(d & 3, 0.0, DOOR_THICK)
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
        v[0] = side_box(b - LADDER_BASE, 0.0625, 0.125);
        (v, 1)
    } else {
        shape_boxes(b, at)
    }
}

/// Small plants drawn as two crossed planes; you walk through them.
#[inline]
pub fn is_plant(b: Block) -> bool {
    (TALL_GRASS..=DEAD_BUSH).contains(&b) || (WHEAT_0..=WHEAT_3).contains(&b)
}
#[inline]
pub fn is_leaves(b: Block) -> bool {
    b == LEAVES || b == BIRCH_LEAVES || b == PINE_LEAVES
}
/// Blocks a new block may be placed into.
#[inline]
pub fn is_replaceable(b: Block) -> bool {
    b == AIR || b == WATER || b == LAVA || b == TALL_GRASS || b == DEAD_BUSH
}

#[inline]
pub fn is_opaque(b: Block) -> bool {
    b != AIR && b != WATER && b != GLASS && b != TORCH && !is_plant(b) && !is_shaped(b)
}
/// Blocks you collide with / can target.
#[inline]
pub fn is_solid(b: Block) -> bool {
    b != AIR && b != WATER && b != LAVA && b != TORCH && !is_plant(b) && !is_ladder(b)
}
/// Blocks the crosshair can select (torches are walk-through but breakable).
#[inline]
pub fn is_target(b: Block) -> bool {
    b != AIR && b != WATER && b != LAVA
}
/// Blocks that light their surroundings.
#[inline]
pub fn is_light(b: Block) -> bool {
    b == TORCH || b == LAMP
}

/// Texture layer for a block face (face: 0 +X, 1 -X, 2 +Y, 3 -Y, 4 +Z, 5 -Z).
#[inline]
pub fn tex_for(b: Block, face: usize) -> u32 {
    match b {
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
        WATER => 12,
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
        LAVA => LAVA_LAYER,
        CHEST | LOOT_CHEST => {
            if face == 2 || face == 3 {
                CHEST_TOP_LAYER
            } else {
                CHEST_LAYER
            }
        }
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
fn hash2(seed: u32, x: i32, z: i32) -> u32 {
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

fn fbm(seed: u32, mut x: f32, mut z: f32, oct: u32) -> f32 {
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

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
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
    (h as i32).clamp(3, CH - 12)
}

/// Where a new world's player starts: the nearest spot to the origin on fairly level
/// green land, clear of sea, desert and mountains. Returns the column and its height.
pub fn spawn_point(seed: u32) -> (i32, i32, i32) {
    let ok = |x: i32, z: i32, strict: bool| -> Option<i32> {
        let h = height_at(seed, x, z);
        if h <= SEA + 2 || h >= 78 {
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
    (0, height_at(seed, 0, 0).max(SEA), 0)
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
fn tunnel_fields(seed: u32, x: i32, y: i32, z: i32) -> (f32, f32) {
    // (off the lattice, like the terrain)
    let (xf, yf, zf) = (x as f32 + 16.3, y as f32 + 9.7, z as f32 + 26.9);
    (perlin3(seed.wrapping_add(8000), xf / 44.0, yf / 26.0, zf / 44.0), perlin3(seed.wrapping_add(8500), xf / 30.0, yf / 34.0, zf / 30.0))
}

/// Winding tunnels: the space where two noise fields are both near zero. Two networks
/// of them cross each other; the first widens into rooms here and there, the second is
/// finer and steeper, so it links the levels of the first. `a` and `c` are the values
/// of `tunnel_fields` at the cell.
fn in_tunnel(seed: u32, x: i32, y: i32, z: i32, a: f32, c: f32) -> bool {
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
fn cavern_field(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    perlin3(seed.wrapping_add(8300), (x as f32 + 5.9) / 58.0, (y as f32 + 3.1) / 22.0, (z as f32 + 12.7) / 58.0)
}

/// True if the cell lies in a cavern, given the cavern field's value there.
fn in_cavern(seed: u32, x: i32, y: i32, z: i32, hall: f32) -> bool {
    if hall < 0.12 || y < 2 || y > CAVERN_TOP {
        return false;
    }
    let deep = ((CAVERN_TOP - y) as f32 / 34.0).min(1.0);
    // (a finer field roughens the walls and leaves pillars standing)
    let rough = perlin3(seed.wrapping_add(8400), (x as f32 + 5.9) / 15.0, (y as f32 + 3.1) / 11.0, (z as f32 + 12.7) / 15.0);
    hall + rough * 0.22 > 0.47 - 0.16 * deep
}
/// no cavern reaches higher than this
const CAVERN_TOP: i32 = 44;

/// Where a chunk's cave chest stands, if it has one: on a cave floor of a column picked
/// by the seed, the deepest that is clear of the lava.
fn cave_chest(seed: u32, cx: i32, cz: i32, blocks: &[u8], h: impl Fn(i32, i32) -> i32) -> Option<(i32, i32, i32)> {
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
fn ore_at(seed: u32, x: i32, y: i32, z: i32) -> Block {
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

// ---------------------------------------------------------------- chunk data

#[derive(Clone)]
pub struct ChunkData {
    pub blocks: Box<[u8]>,
}

#[inline]
fn bidx(x: i32, y: i32, z: i32) -> usize {
    ((y * CW + z) * CW + x) as usize
}

impl ChunkData {
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        self.blocks[bidx(x, y, z)]
    }
}

pub fn generate(seed: u32, cx: i32, cz: i32) -> ChunkData {
    let mut blocks = vec![AIR; VOL].into_boxed_slice();
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
            let desert = h > SEA + 1 && h < 82 && is_desert(seed, wx, wz);
            // lowest neighbouring surface: caves stay clear of sea and lake beds
            let seabed = if h > SEA + 1 {
                SAND
            } else {
                let g = fbm(seed.wrapping_add(11000), wx as f32 / 38.0, wz as f32 / 38.0, 2);
                if g > 0.3 {
                    GRAVEL
                } else if g < -0.36 && h < SEA {
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
            let mut fields = [(0.0f32, 0.0f32); (CH / 2 + 1) as usize];
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
                } else if h <= SEA + 1 {
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
                if b != BEDROCK && !(y <= SEA + 1 && y >= low - 4) && (tunnel(y) || cavern(y)) {
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
            for y in (h + 1)..=SEA {
                blocks[bidx(x, y, z)] = WATER;
            }
            if desert && h + 4 < CH {
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
            if h <= SEA + 1 || h >= 86 {
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
                if x >= 0 && x < CW && z >= 0 && z < CW && y > 0 && y < CH {
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
            if h <= SEA + 1 || h >= 86 || blocks[bidx(x, h + 1, z)] != AIR {
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
                SAND if h > SEA + 2 && roll < 9 => DEAD_BUSH,
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
    /// packed quad vertices (4 per quad): opaque quads first, then transparent
    pub verts: Vec<u32>,
    pub opaque_quads: u32,
    /// how many of the opaque quads, the last ones, are buried: walls of caves well
    /// below the ground that no daylight reaches. From afar they cannot be seen from
    /// above ground, and need not be drawn.
    pub deep_quads: u32,
    /// plants: alpha-tested, drawn double sided
    pub cut_quads: u32,
    pub trans_quads: u32,
    /// shaped blocks (stairs, slabs, fences, doors...): quads of two words per vertex,
    /// x 9 bits | y 12 | z 9 in sixteenths of a block | sky 2, then face 3 | layer 8
    pub detail: Vec<u32>,
    /// highest y used by any vertex (for tight frustum culling)
    pub top: f32,
}

const PW: usize = (CW + 2) as usize;
const PH: usize = (CH + 2) as usize;

/// Greedy mesher with ambient occlusion. `n` is the 3x3 neighbourhood, index (dz+1)*3+(dx+1).
pub fn mesh(n: &[Arc<ChunkData>; 9]) -> MeshData {
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
    let mut deep: Vec<u32> = Vec::new();
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
    let dims: [i32; 3] = [CW, CH, CW];
    let pidx = |p: [i32; 3]| -> isize {
        (((p[1] + 1) as usize * PW + (p[2] + 1) as usize) * PW + (p[0] + 1) as usize) as isize
    };
    let at = |i: isize| -> Block { pad[i as usize] };

    let mut opaque: Vec<u32> = Vec::with_capacity(4096);
    let mut trans: Vec<u32> = Vec::new();
    let mut mask = vec![0u32; (CW * CH) as usize];

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
                    let m = &mut mask[vv * nu + uu];
                    *m = 0;
                    if b == AIR || b == TORCH || is_plant(b) || is_shaped(b) {
                        continue;
                    }
                    let q = i + sa;
                    let nb = at(q);
                    let op = is_opaque(b);
                    let visible = if op { !is_opaque(nb) } else { nb != b && !is_opaque(nb) };
                    if !visible {
                        continue;
                    }
                    let mut ao = 0xffu32;
                    if op {
                        let o = |i: isize| is_opaque(at(i)) as u32;
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
                    *m = 1 << 31 | (!op as u32) << 30 | sky << 16 | ao << 8 | tex_for(b, face);
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
                    let ao = (k >> 8) & 0xff;
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
                        quad[i] = p[0] | p[1] << 5
                            | p[2] << 13
                            | (face as u32) << 18
                            | (k & 127) << 21
                            | aoc[ci] << 28
                            | ((k >> 16) & 3) << 30;
                    }
                    let out = if k & (1 << 30) != 0 {
                        &mut trans
                    } else if (k >> 16) & 3 == 0 && high < buried_below {
                        &mut deep
                    } else {
                        &mut opaque
                    };
                    out.extend_from_slice(&quad);
                    uu += w;
                }
            }
        }
    }
    // plants: two crossed diagonal planes per block (face code 6)
    let mut cut: Vec<u32> = Vec::new();
    let mut detail: Vec<u32> = Vec::new();
    for y in 1..CH {
        for z in 0..CW {
            for x in 0..CW {
                let i = pidx([x, y, z]);
                let b = at(i);
                if b == PANE {
                    // a flat upright quad (face code 7) the shader moves to the middle of
                    // the cell: `code` 0 = towards +x, 2 = towards +z
                    let sky = sky_at(x, y, z);
                    let (x, y, z) = (x as u32, y as u32, z as u32);
                    let quad = |out: &mut Vec<u32>, code: u32| {
                        let tail = 7 << 18 | PANE_LAYER << 21 | code << 28 | sky << 30;
                        let corners = if code == 0 {
                            [(x, y, z), (x, y, z + 1), (x, y + 1, z + 1), (x, y + 1, z)]
                        } else {
                            [(x, y, z), (x + 1, y, z), (x + 1, y + 1, z), (x, y + 1, z)]
                        };
                        for (px, py, pz) in corners {
                            out.push(px | py << 5 | pz << 13 | tail);
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
                                let tail = f as u32 | tex_for(b, f) << 3;
                                for (px, py, pz) in corners {
                                    detail.push(px | py << 9 | pz << 21 | sky[f] << 30);
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
                let tail = 6 << 18 | tex_for(b, 0) << 21 | 3 << 28 | sky_at(x, y, z) << 30;
                let (x, y, z) = (x as u32, y as u32, z as u32);
                for (z0, z1) in [(z, z + 1), (z + 1, z)] {
                    for (px, py, pz) in [(x, y, z0), (x + 1, y, z1), (x + 1, y + 1, z1), (x, y + 1, z0)] {
                        cut.push(px | py << 5 | pz << 13 | tail);
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
    let top = opaque.iter().map(|v| (v >> 5) & 255).max().unwrap_or(0);
    let top = top.max(detail.chunks_exact(2).map(|v| (((v[0] >> 9) & 4095) + 15) / 16).max().unwrap_or(0)) as f32;
    MeshData { verts: opaque, opaque_quads: oq, deep_quads: dq, cut_quads: cq, trans_quads: tq, detail, top }
}

// ---------------------------------------------------------------- world / streaming

#[cfg(target_os = "linux")]
fn lower_thread_priority() {
    unsafe {
        let param = libc::sched_param { sched_priority: 0 };
        if libc::sched_setscheduler(0, libc::SCHED_IDLE, &param) != 0 {
            libc::setpriority(libc::PRIO_PROCESS, 0, 15);
        }
    }
}

#[cfg(target_os = "windows")]
fn lower_thread_priority() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_LOWEST: i32 = -2;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn lower_thread_priority() {}

enum Job {
    Gen(Key),
    /// write a modified chunk to disk off the render thread (slow HDD friendly)
    Save(PathBuf, Arc<ChunkData>),
    Mesh(Key, u32, Box<[Arc<ChunkData>; 9]>),
}
enum JobResult {
    Gen(Key, ChunkData, bool),
    Mesh(Key, u32, MeshData),
}

pub struct Slot {
    pub data: Arc<ChunkData>,
    pub mesh: Option<ChunkMesh>,
    /// world positions of the light-emitting blocks in this chunk
    pub lights: Vec<(IVec3, Block)>,
    dirty: bool,
    pending: bool,
    version: u32,
    modified: bool,
}

pub struct World {
    pub seed: u32,
    pub chunks: HashMap<Key, Slot, Fx>,
    gen_pending: HashSet<Key, Fx>,
    job_tx: Sender<Job>,
    res_rx: Receiver<JobResult>,
    in_flight: usize,
    max_in_flight: usize,
    pub radius: i32,
    /// chunk offsets sorted near -> far; bool = inside mesh radius
    pub spiral: Vec<(i32, i32, bool)>,
    uploads: VecDeque<(Key, u32, MeshData)>,
    save_dir: Option<PathBuf>,
    pending_saves: Arc<AtomicUsize>,
    tick: u32,
}

fn chunk_path(dir: &PathBuf, k: Key) -> PathBuf {
    dir.join(format!("c.{}.{}.bin", k.0, k.1))
}

impl World {
    pub fn new(seed: u32, radius: i32, save_dir: Option<PathBuf>) -> World {
        let (job_tx, job_rx) = channel::<Job>();
        let (res_tx, res_rx) = channel::<JobResult>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let pending_saves = Arc::new(AtomicUsize::new(0));
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        let workers = std::env::var("CUBEWILD_WORKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or((threads / 2).saturating_sub(1))
            .clamp(1, 8);
        for _ in 0..workers {
            let rx = job_rx.clone();
            let tx = res_tx.clone();
            let dir = save_dir.clone();
            let saves = pending_saves.clone();
            std::thread::spawn(move || {
                // Lowest priority: chunk work only runs on CPU time the render
                // thread and the rest of the desktop are not using.
                lower_thread_priority();
                loop {
                let job = match rx.lock().unwrap().recv() {
                    Ok(j) => j,
                    Err(_) => return,
                };
                let res = match job {
                    Job::Gen(k) => {
                        let saved = dir
                            .as_ref()
                            .and_then(|d| std::fs::read(chunk_path(d, k)).ok())
                            .filter(|b| b.len() == VOL);
                        match saved {
                            Some(b) => {
                                JobResult::Gen(k, ChunkData { blocks: b.into_boxed_slice() }, true)
                            }
                            None => JobResult::Gen(k, generate(seed, k.0, k.1), false),
                        }
                    }
                    Job::Mesh(k, ver, n) => JobResult::Mesh(k, ver, mesh(&n)),
                    Job::Save(path, data) => {
                        if let Err(e) = std::fs::write(&path, &data.blocks) {
                            eprintln!("cubewild: failed to save {}: {e}", path.display());
                        }
                        saves.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                };
                if tx.send(res).is_err() {
                    return;
                }
                }
            });
        }
        let mut w = World {
            seed,
            chunks: HashMap::default(),
            gen_pending: HashSet::default(),
            job_tx,
            res_rx,
            in_flight: 0,
            max_in_flight: workers * 3,
            radius,
            spiral: Vec::new(),
            uploads: VecDeque::new(),
            save_dir,
            pending_saves,
            tick: 0,
        };
        w.set_radius(radius);
        w
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
        let mut v: Vec<Arc<ChunkData>> = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                v.push(self.chunks.get(&(k.0 + dx, k.1 + dz))?.data.clone());
            }
        }
        let arr: [Arc<ChunkData>; 9] = v.try_into().ok()?;
        Some(Box::new(arr))
    }

    /// Streams chunks around `center` (chunk coords). Call once per frame.
    pub fn update(&mut self, gfx: &Gfx, center: Key) {
        self.tick = self.tick.wrapping_add(1);
        while let Ok(res) = self.res_rx.try_recv() {
            self.in_flight -= 1;
            match res {
                JobResult::Gen(k, data, modified) => {
                    self.gen_pending.remove(&k);
                    let mut lights = Vec::new();
                    {
                        for (i, &b) in data.blocks.iter().enumerate() {
                            let lava = b == LAVA && i & 0x33 == 0x11 && data.blocks[i + 256] != LAVA;
                            if (modified && is_light(b)) || lava {
                                let i = i as i32;
                                lights.push((
                                    IVec3::new(k.0 * CW + (i & 15), i >> 8, k.1 * CW + ((i >> 4) & 15)),
                                    b,
                                ));
                            }
                        }
                    }
                    self.chunks.insert(
                        k,
                        Slot {
                            data: Arc::new(data),
                            mesh: None,
                            lights,
                            dirty: true,
                            pending: false,
                            version: 0,
                            modified,
                        },
                    );
                }
                JobResult::Mesh(k, ver, m) => {
                    if let Some(s) = self.chunks.get_mut(&k) {
                        s.pending = false;
                        if s.version == ver {
                            self.uploads.push_back((k, ver, m));
                        }
                    }
                }
            }
        }
        // GPU uploads, a few per frame to keep frame times flat
        for _ in 0..2 {
            let Some((k, ver, m)) = self.uploads.pop_front() else { break };
            if let Some(s) = self.chunks.get_mut(&k) {
                if s.version == ver {
                    s.mesh = gfx.upload_mesh(&m);
                }
            }
        }
        // schedule nearest work first
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
                            self.job_tx.send(Job::Gen(k)).ok();
                            self.in_flight += 1;
                        }
                    }
                    Some(s) => {
                        if inner && s.dirty && !s.pending {
                            if let Some(n) = self.neighbours(k) {
                                let s = self.chunks.get_mut(&k).unwrap();
                                s.dirty = false;
                                s.pending = true;
                                self.job_tx.send(Job::Mesh(k, s.version, n)).ok();
                                self.in_flight += 1;
                            }
                        }
                    }
                }
            }
        }
        // unload far chunks
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
                    if let (true, Some(d)) = (s.modified, &self.save_dir) {
                        self.pending_saves.fetch_add(1, Ordering::SeqCst);
                        self.job_tx.send(Job::Save(chunk_path(d, k), s.data.clone())).ok();
                    }
                }
            }
        }
    }

    fn save_slot(&self, k: Key, s: &Slot) {
        if !s.modified {
            return;
        }
        if let Some(d) = &self.save_dir {
            if let Err(e) = std::fs::write(chunk_path(d, k), &s.data.blocks) {
                eprintln!("cubewild: failed to save chunk {k:?}: {e}");
            }
        }
    }

    pub fn save_all(&self) {
        for (k, s) in &self.chunks {
            self.save_slot(*k, s);
        }
        // let queued background saves reach the disk before the process exits
        let start = std::time::Instant::now();
        while self.pending_saves.load(Ordering::SeqCst) > 0 && start.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    pub fn is_loaded(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
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
            b != AIR && b != TORCH && !is_plant(b) && !is_ladder(b)
        })
    }

    fn remesh_now(&mut self, gfx: &Gfx, k: Key) {
        let n = self.neighbours(k);
        let Some(s) = self.chunks.get_mut(&k) else { return };
        s.version += 1; // invalidates any in-flight mesh of the old data
        match n {
            Some(n) => {
                s.mesh = gfx.upload_mesh(&mesh(&n));
                s.dirty = false;
            }
            None => s.dirty = true,
        }
    }

    /// Edits a block and lets the background workers remesh (for changes the player
    /// is not waiting on, such as crops growing).
    pub fn set_block_bg(&mut self, x: i32, y: i32, z: i32, b: Block) {
        if y < 1 || y >= CH {
            return;
        }
        let k = (x >> 4, z >> 4);
        let (lx, lz) = (x & 15, z & 15);
        let Some(s) = self.chunks.get_mut(&k) else { return };
        Arc::make_mut(&mut s.data).blocks[bidx(lx, y, lz)] = b;
        s.modified = true;
        s.version += 1;
        s.dirty = true;
    }

    /// Writes a block without remeshing. False if the spot cannot be edited.
    fn write_block(&mut self, x: i32, y: i32, z: i32, b: Block) -> bool {
        if y < 1 || y >= CH {
            return false;
        }
        let (lx, lz) = (x & 15, z & 15);
        let Some(s) = self.chunks.get_mut(&(x >> 4, z >> 4)) else { return false };
        if s.data.get(lx, y, lz) == BEDROCK {
            return false;
        }
        Arc::make_mut(&mut s.data).blocks[bidx(lx, y, lz)] = b;
        s.modified = true;
        let wp = IVec3::new(x, y, z);
        s.lights.retain(|l| l.0 != wp);
        if is_light(b) {
            s.lights.push((wp, b));
        }
        true
    }

    /// Edits a block and the one above it with a single remesh (both halves of a door).
    pub fn set_pair(&mut self, gfx: &Gfx, x: i32, y: i32, z: i32, lower: Block, upper: Block) -> bool {
        if y + 1 >= CH || !self.write_block(x, y, z, lower) {
            return false;
        }
        self.write_block(x, y + 1, z, upper);
        self.remesh_edit(gfx, x, z);
        true
    }

    /// Edits a block and remeshes the affected chunks immediately.
    pub fn set_block(&mut self, gfx: &Gfx, x: i32, y: i32, z: i32, b: Block) -> bool {
        if !self.write_block(x, y, z, b) {
            return false;
        }
        self.remesh_edit(gfx, x, z);
        true
    }

    fn remesh_edit(&mut self, gfx: &Gfx, x: i32, z: i32) {
        let k = (x >> 4, z >> 4);
        let (lx, lz) = (x & 15, z & 15);
        // sky exposure reaches two columns across a chunk border
        let xs: &[i32] = if lx <= 1 { &[0, -1] } else if lx >= 14 { &[0, 1] } else { &[0] };
        let zs: &[i32] = if lz <= 1 { &[0, -1] } else if lz >= 14 { &[0, 1] } else { &[0] };
        for &dz in zs {
            for &dx in xs {
                self.remesh_now(gfx, (k.0 + dx, k.1 + dz));
            }
        }
    }

    /// A small loaded world for tests: flat stone up to y = 9, plus what `extra` puts in.
    #[cfg(test)]
    pub fn flat(extra: impl Fn(i32, i32, i32) -> Option<Block>) -> World {
        let mut w = World::new(1, 2, None);
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
                let slot = Slot { data: Arc::new(ChunkData { blocks }), mesh: None, lights: Vec::new(), dirty: false, pending: false, version: 0, modified: false };
                w.chunks.insert((cx, cz), slot);
            }
        }
        w
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
        let (mut v, n) = shape_boxes(b, |dx, dz| self.get_block(x + dx, y, z + dz));
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
            let (x, y, z) = (v[0] & 511, (v[0] >> 9) & 4095, (v[0] >> 21) & 511);
            assert!(x <= 256 && z <= 256 && (160..=184).contains(&y), "{x} {y} {z}");
            assert!((v[1] & 7) < 6 && ((v[1] >> 3) as usize) < crate::textures::LAYERS);
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
        for b in 0..=255u8 {
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
            let (mut lo, mut hi) = (CH, 0);
            let mut img = vec![0u8; n * n * 3];
            for (j, z) in (-R..R).step_by(STEP as usize).enumerate() {
                for (i, x) in (-R..R).step_by(STEP as usize).enumerate() {
                    let h = height_at(seed, x, z);
                    total += 1;
                    lo = lo.min(h);
                    hi = hi.max(h);
                    let d = h > SEA + 1 && h < 82 && is_desert(seed, x, z);
                    let c: [u8; 3] = if h <= SEA {
                        let k = (h - 3).clamp(0, 40) as u8;
                        [20, 40 + k * 2, 110 + k * 3]
                    } else if h <= SEA + 1 || d {
                        [222, 206, 150]
                    } else if h >= 98 {
                        [245, 245, 250]
                    } else if h >= 86 {
                        [128, 128, 132]
                    } else {
                        let k = ((h - SEA) * 2).clamp(0, 90) as u8;
                        [60 + k / 2, 150 - k, 50]
                    };
                    img[(j * n + i) * 3..][..3].copy_from_slice(&c);
                    land += (h > SEA) as u32;
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
                            for y in 1..CH - 1 {
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
                                        let open = under == AIR || (x > 0 && c.get(x - 1, y, z) == AIR && y < SEA) || (x < CW - 1 && c.get(x + 1, y, z) == AIR && y < SEA);
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
                    assert!((3..=CH - 12).contains(&h));
                    total += 1;
                    land += (h > SEA) as u32;
                    snow += (h >= 98) as u32;
                    flat_top += (h == CH - 12) as u32;
                }
            }
            // no world is all sea or all land
            let share = land as f32 / total as f32;
            assert!((0.35..0.97).contains(&share), "seed {seed}: land {share}");
            origin.insert(height_at(seed, 0, 0));
            // the start is on level green land, not far from the middle
            let (x, h, z) = spawn_point(seed);
            assert_eq!(h, height_at(seed, x, z));
            assert!(h > SEA + 2 && h < 78 && !is_desert(seed, x, z), "seed {seed}: spawn {x} {h} {z}");
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
                        for y in 1..CH - 1 {
                            let (b, under) = (c.get(x, y, z), c.get(x, y - 1, z));
                            if is_plant(b) || b == CACTUS {
                                assert!(is_opaque(under) || under == CACTUS, "seed {seed}: block {b} on {under} at {x} {y} {z}");
                            }
                            if b == WATER {
                                assert!(y <= SEA && under != AIR, "seed {seed}: water at {x} {y} {z}");
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
        let t = std::time::Instant::now();
        let mut air = 0usize;
        for cz in 0..8 {
            for cx in 0..8 {
                air += generate(2026, cx, cz).blocks.iter().filter(|b| **b == AIR).count();
            }
        }
        println!("generate: {:.2} ms a chunk ({air})", t.elapsed().as_secs_f64() * 1000.0 / 64.0);
        let n: [Arc<ChunkData>; 9] = std::array::from_fn(|i| Arc::new(generate(2026, (i % 3) as i32, (i / 3) as i32)));
        let t = std::time::Instant::now();
        let mut quads = 0;
        for _ in 0..20 {
            let m = mesh(&n);
            quads += m.detail.len();
        }
        println!("mesh: {:.2} ms a chunk ({quads})", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
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
