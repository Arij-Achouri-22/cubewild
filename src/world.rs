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
pub const LAST_BLOCK: Block = WHEAT_3;

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
    b == AIR || b == WATER || b == TALL_GRASS || b == DEAD_BUSH
}

#[inline]
pub fn is_opaque(b: Block) -> bool {
    b != AIR && b != WATER && b != GLASS && b != TORCH && !is_plant(b)
}
/// Blocks you collide with / can target.
#[inline]
pub fn is_solid(b: Block) -> bool {
    b != AIR && b != WATER && b != TORCH && !is_plant(b)
}
/// Blocks the crosshair can select (torches are walk-through but breakable).
#[inline]
pub fn is_target(b: Block) -> bool {
    b != AIR && b != WATER
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
        _ => 14,
    }
}

/// texture layer the shader renders at full brightness
pub const LAMP_LAYER: u32 = 27;

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
type Fx = BuildHasherDefault<FxHasher>;
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
    (h as i32).clamp(3, CH - 12)
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

/// Winding tunnels: the space where two noise fields are both near zero.
pub fn is_cave(seed: u32, x: i32, y: i32, z: i32) -> bool {
    if y < 2 {
        return false;
    }
    let (xf, yf, zf) = (x as f32, y as f32, z as f32);
    let a = perlin3(seed.wrapping_add(8000), xf / 44.0, yf / 26.0, zf / 44.0);
    if a.abs() > 0.1 {
        return false;
    }
    let b = perlin3(seed.wrapping_add(8100), xf / 44.0, yf / 26.0, zf / 44.0);
    // wider rooms here and there
    let room = perlin3(seed.wrapping_add(8200), xf / 90.0, yf / 60.0, zf / 90.0).max(0.0);
    a * a + b * b < 0.0034 + room * 0.007
}

/// Ore pockets inside stone, rarer and deeper for the better ones.
fn ore_at(seed: u32, x: i32, y: i32, z: i32) -> Block {
    let (xf, yf, zf) = (x as f32, y as f32, z as f32);
    let n = perlin3(seed.wrapping_add(9000), xf / 5.5, yf / 5.5, zf / 5.5);
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
                if b != BEDROCK && !(y <= SEA + 1 && y >= low - 4) && is_cave(seed, wx, y, wz) {
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
    /// plants: alpha-tested, drawn double sided
    pub cut_quads: u32,
    pub trans_quads: u32,
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
                if is_opaque(b) && !is_leaves(b) {
                    break;
                }
                y -= 1;
            }
            tops[((z + 2) * TW + x + 2) as usize] = y;
        }
    }
    let top_at = |x: i32, z: i32| tops[((z + 2) * TW + x + 2) as usize];
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
                    if b == AIR || b == TORCH || is_plant(b) {
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
                    let out = if k & (1 << 30) != 0 { &mut trans } else { &mut opaque };
                    for &ci in &order {
                        let mut p = [0u32; 3];
                        p[a] = ac as u32;
                        p[u] = corners[ci].0 as u32;
                        p[v] = corners[ci].1 as u32;
                        out.push(
                            p[0] | p[1] << 5
                                | p[2] << 13
                                | (face as u32) << 18
                                | (k & 127) << 21
                                | aoc[ci] << 28
                                | ((k >> 16) & 3) << 30,
                        );
                    }
                    uu += w;
                }
            }
        }
    }
    // plants: two crossed diagonal planes per block (face code 6)
    let mut cut: Vec<u32> = Vec::new();
    for y in 1..CH {
        for z in 0..CW {
            for x in 0..CW {
                let b = at(pidx([x, y, z]));
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
    let (oq, cq, tq) = ((opaque.len() / 4) as u32, (cut.len() / 4) as u32, (trans.len() / 4) as u32);
    opaque.extend_from_slice(&cut);
    opaque.extend_from_slice(&trans);
    let top = opaque.iter().map(|v| (v >> 5) & 255).max().unwrap_or(0) as f32;
    MeshData { verts: opaque, opaque_quads: oq, cut_quads: cq, trans_quads: tq, top }
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
                    if modified {
                        for (i, &b) in data.blocks.iter().enumerate() {
                            if is_light(b) {
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
            b != AIR && b != TORCH && !is_plant(b)
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

    /// Edits a block and remeshes the affected chunks immediately.
    pub fn set_block(&mut self, gfx: &Gfx, x: i32, y: i32, z: i32, b: Block) -> bool {
        if y < 1 || y >= CH {
            return false;
        }
        let k = (x >> 4, z >> 4);
        let (lx, lz) = (x & 15, z & 15);
        let Some(s) = self.chunks.get_mut(&k) else { return false };
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
        // sky exposure reaches two columns across a chunk border
        let xs: &[i32] = if lx <= 1 { &[0, -1] } else if lx >= 14 { &[0, 1] } else { &[0] };
        let zs: &[i32] = if lz <= 1 { &[0, -1] } else if lz >= 14 { &[0, 1] } else { &[0] };
        for &dz in zs {
            for &dx in xs {
                self.remesh_now(gfx, (k.0 + dx, k.1 + dz));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worldgen_stats() {
        let mut counts = [0usize; 32];
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
        assert!(air > 0.005 && air < 0.12, "cave fraction {air}");
    }
}
