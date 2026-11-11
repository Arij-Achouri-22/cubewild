//! Villages: a well, a handful of houses and fields, and the paths between them, set down
//! here and there on level open country. They are rare: the world is cut into squares of
//! `SPACING` chunks, each square has one spot picked by the seed where a village may
//! stand, and it stands there only if the land suits it (the spread-out placement that
//! Horizon uses for its structures). Everything about a village follows from the seed and
//! its square, so any chunk can work out the part of it that falls inside its borders.

use crate::world::*;
use crate::worldgen::{self, Biome};
use glam::Vec3;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// one possible site in every square of this many chunks
pub const SPACING: i32 = 40;
/// sites keep at least this many chunks from the sites of the squares next to theirs
const SEPARATION: i32 = 12;
/// no part of a village lies further than this from its well
pub const REACH: i32 = 44;

#[derive(Clone, Copy)]
enum Roof {
    /// a gable of stairs cut from this material (see `SHAPE_FULL`)
    Stairs(u8),
    /// a stepped gable of whole blocks
    Blocks(Block),
    /// a flat roof with a rim
    Flat(Block, Block),
}

/// What a village is built from, by the country it stands in.
#[derive(Clone, Copy)]
struct Palette {
    wall: Block,
    post: Block,
    base: Block,
    floor: Block,
    roof: Roof,
    path: Block,
    fence: Block,
    /// added to a door's id: 0 oak, 32 birch
    door: Block,
    slab: Block,
}

fn palette(b: Biome, r: u32) -> Option<Palette> {
    Some(match b {
        Biome::Plains | Biome::Meadow => Palette {
            wall: PLANKS,
            post: LOG,
            base: COBBLE,
            floor: PLANKS,
            roof: Roof::Stairs(if r & 1 == 0 { 2 } else { 1 }),
            path: GRAVEL,
            fence: FENCE,
            door: 0,
            slab: SLAB,
        },
        Biome::Savanna => Palette {
            wall: TERRACOTTA_PALE,
            post: BIRCH_LOG,
            base: COBBLE,
            floor: BIRCH_PLANKS,
            roof: Roof::Blocks(THATCH),
            path: GRAVEL,
            fence: BIRCH_FENCE,
            door: 32,
            slab: SLAB + 1,
        },
        Biome::Desert => Palette {
            wall: SANDSTONE,
            post: TERRACOTTA,
            base: SANDSTONE,
            floor: TERRACOTTA_PALE,
            roof: Roof::Flat(SANDSTONE, TERRACOTTA),
            path: GRAVEL,
            fence: BIRCH_FENCE,
            door: 32,
            slab: SLAB + 1,
        },
        Biome::SnowyPlains => Palette {
            wall: LOG,
            post: STONE_BRICK,
            base: STONE_BRICK,
            floor: PLANKS,
            roof: Roof::Stairs(3),
            path: COBBLE,
            fence: FENCE,
            door: 0,
            slab: SLAB + 3,
        },
        _ => return None,
    })
}

/// A house or a field: its outer edge, the level of its floor, and the side its door (or
/// the end of its path) is on.
pub struct Lot {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
    /// y of the floor blocks
    pub g: i32,
    /// see `SIDE_DIR`
    pub side: u8,
    pub farm: bool,
    /// how many villagers live here
    pub beds: u8,
    /// picks the furniture
    r: u32,
}

impl Lot {
    /// the cell of the wall the door stands in
    pub fn door(&self) -> (i32, i32) {
        let (mx, mz) = ((self.x0 + self.x1) / 2, (self.z0 + self.z1) / 2);
        match self.side {
            0 => (self.x0, mz),
            1 => (self.x1, mz),
            2 => (mx, self.z0),
            _ => (mx, self.z1),
        }
    }
    fn along_x(&self) -> bool {
        self.x1 - self.x0 >= self.z1 - self.z0
    }
}

pub struct Village {
    /// the square of the world it belongs to
    pub region: (i32, i32),
    /// the well
    pub x: i32,
    pub z: i32,
    pub g: i32,
    pal: Palette,
    pub lots: Vec<Lot>,
    /// (x, y of the ground, z)
    paths: Vec<(i32, i32, i32)>,
    lamps: Vec<(i32, i32, i32)>,
    /// where each villager lives: feet on the floor of its house
    pub homes: Vec<Vec3>,
    /// places to stroll to: feet on the paths and round the well
    pub spots: Vec<Vec3>,
}

/// The column a square's village would have its well in.
fn candidate(seed: u32, rx: i32, rz: i32) -> (i32, i32) {
    let r = hash2(seed ^ 0x5eed_b117, rx, rz);
    let span = (SPACING - SEPARATION) as u32;
    ((rx * SPACING + (r % span) as i32) * CW + 8, (rz * SPACING + ((r >> 12) % span) as i32) * CW + 8)
}

fn layout(seed: u32, rx: i32, rz: i32) -> Option<Village> {
    let (x, z) = candidate(seed, rx, rz);
    let c = worldgen::column(seed, x, z);
    let r = hash2(seed ^ 0x0a11_7a6e, rx, rz);
    let pal = palette(c.biome, r)?;
    if c.h <= SEA + 1 || c.h > 80 {
        return None;
    }
    // a quick look round before the ground is surveyed properly
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::TAU / 8.0;
        let n = worldgen::column(seed, x + (a.cos() * 22.0) as i32, z + (a.sin() * 22.0) as i32);
        if n.h <= SEA || (n.h - c.h).abs() > 6 {
            return None;
        }
    }
    const W: i32 = 2 * REACH + 1;
    let cols = worldgen::columns(seed, x - REACH, z - REACH, W);
    let hat = |px: i32, pz: i32| cols[((pz - z + REACH) * W + px - x + REACH) as usize].h;
    // level dry ground under a plot and the cells round it
    let fits = |x0: i32, z0: i32, x1: i32, z1: i32, g: i32| {
        (z0 - 1..=z1 + 1).all(|pz| {
            (x0 - 1..=x1 + 1).all(|px| {
                let h = hat(px, pz);
                h > SEA + 1 && (h - g).abs() <= 2
            })
        })
    };
    let g = c.h;
    if !fits(x - 3, z - 3, x + 3, z + 3, g) {
        return None;
    }
    // the plots stand in two rings round the well, turned towards it
    let mut taken: Vec<[i32; 4]> = vec![[x - 2, z - 2, x + 2, z + 2]];
    let mut lots: Vec<Lot> = Vec::new();
    let n = 6 + (r >> 8) % 4;
    let turn = ((r >> 12) & 255) as f32 / 256.0 * std::f32::consts::TAU;
    for i in 0..n {
        let q = hash2(r, i as i32, 17);
        let farm = i % 4 == 2;
        let (mut w, mut d) = match q % 10 {
            _ if farm => (9, 7),
            0..=3 => (5, 5),
            4..=7 => (7, 5),
            _ => (9, 7),
        };
        if (q >> 4) & 1 == 1 {
            (w, d) = (d, w);
        }
        for attempt in 0..5 {
            let q2 = hash2(q, attempt, 5);
            let ang = turn + (i as f32 + (q2 & 255) as f32 / 255.0 * 0.5 - 0.25) * std::f32::consts::TAU / n as f32;
            let dist = [12.0, 21.0][i as usize % 2] + attempt as f32 * 2.5 + ((q2 >> 8) & 3) as f32;
            let (hx, hz) = (x + (ang.cos() * dist).round() as i32, z + (ang.sin() * dist).round() as i32);
            let (x0, z0) = (hx - w / 2, hz - d / 2);
            let (x1, z1) = (x0 + w - 1, z0 + d - 1);
            if [x0 - x, x1 - x, z0 - z, z1 - z].iter().any(|v| v.abs() > REACH - 3) {
                continue;
            }
            let hg = hat(hx, hz);
            if !fits(x0, z0, x1, z1, hg) || taken.iter().any(|t| x0 - 2 <= t[2] && x1 + 2 >= t[0] && z0 - 2 <= t[3] && z1 + 2 >= t[1]) {
                continue;
            }
            let (dx, dz) = (hx - x, hz - z);
            let side = if dx.abs() >= dz.abs() { (dx < 0) as u8 } else { 2 + (dz < 0) as u8 };
            taken.push([x0, z0, x1, z1]);
            lots.push(Lot { x0, z0, x1, z1, g: hg, side, farm, beds: if farm { 0 } else { 1 + (w.max(d) > 5) as u8 }, r: q });
            break;
        }
    }
    if lots.iter().filter(|l| !l.farm).count() < 3 {
        return None;
    }

    // lamp posts at the corners of the square round the well
    let built = |px: i32, pz: i32| taken.iter().any(|t| px >= t[0] - 1 && px <= t[2] + 1 && pz >= t[1] - 1 && pz <= t[3] + 1);
    let lamps: Vec<(i32, i32, i32)> = [(-4, -4), (4, 4), (-4, 4), (4, -4)]
        .iter()
        .take(2 + (r >> 20) as usize % 3)
        .map(|&(dx, dz)| (x + dx, hat(x + dx, z + dz), z + dz))
        .filter(|&(px, h, pz)| h > SEA && !built(px, pz))
        .collect();
    // a path from every door to the well, two cells wide
    let mut paths: Vec<(i32, i32, i32)> = Vec::new();
    for l in &lots {
        let (dx, dz) = l.door();
        let dir = SIDE_DIR[l.side as usize];
        let (sx, sz) = (dx + dir.0 * 2, dz + dir.1 * 2);
        let (tx, tz) = (x - sx, z - sz);
        let steps = tx.abs().max(tz.abs()).max(1);
        let wide = if tx.abs() >= tz.abs() { (0, 1) } else { (1, 0) };
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let (px, pz) = (sx + (tx as f32 * t).round() as i32, sz + (tz as f32 * t).round() as i32);
            if (px - x).abs() <= 3 && (pz - z).abs() <= 3 {
                break;
            }
            for (qx, qz) in [(px, pz), (px + wide.0, pz + wide.1)] {
                let h = hat(qx, qz);
                if h > SEA && !built(qx, qz) && !lamps.iter().any(|l| l.0 == qx && l.2 == qz) {
                    paths.push((qx, h, qz));
                }
            }
        }
    }
    paths.sort_unstable();
    paths.dedup();

    let feet = |px: i32, y: i32, pz: i32| Vec3::new(px as f32 + 0.5, y as f32, pz as f32 + 0.5);
    let mut homes = Vec::new();
    for l in lots.iter().filter(|l| !l.farm) {
        let (mx, mz) = ((l.x0 + l.x1) / 2, (l.z0 + l.z1) / 2);
        for b in 0..l.beds as i32 {
            homes.push(if l.along_x() { feet(mx + b, l.g + 1, mz) } else { feet(mx, l.g + 1, mz + b) });
        }
    }
    let mut spots: Vec<Vec3> = paths.iter().map(|&(px, h, pz)| feet(px, h + 1, pz)).collect();
    for d in -2..=2 {
        for (dx, dz) in [(d, -2), (d, 2), (-2, d), (2, d)] {
            spots.push(feet(x + dx, g + 1, z + dz));
        }
    }
    Some(Village { region: (rx, rz), x, z, g, pal, lots, paths, lamps, homes, spots })
}

/// The village of a square of the world, if one stands there. Worked out once and kept.
fn get(seed: u32, rx: i32, rz: i32) -> Option<Arc<Village>> {
    static CACHE: OnceLock<Mutex<HashMap<(u32, i32, i32), Option<Arc<Village>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap().get(&(seed, rx, rz)) {
        return v.clone();
    }
    let v = layout(seed, rx, rz).map(Arc::new);
    cache.lock().unwrap().insert((seed, rx, rz), v.clone());
    v
}

/// The villages whose well lies within `pad` blocks of a rectangle of columns.
pub fn near(seed: u32, x0: i32, z0: i32, x1: i32, z1: i32, pad: i32) -> Vec<Arc<Village>> {
    const RS: i32 = SPACING * CW;
    let mut out = Vec::new();
    for rz in (z0 - pad).div_euclid(RS)..=(z1 + pad).div_euclid(RS) {
        for rx in (x0 - pad).div_euclid(RS)..=(x1 + pad).div_euclid(RS) {
            let (x, z) = candidate(seed, rx, rz);
            if x < x0 - pad || x > x1 + pad || z < z0 - pad || z > z1 + pad {
                continue;
            }
            if let Some(v) = get(seed, rx, rz) {
                out.push(v);
            }
        }
    }
    out
}

/// Builds into a chunk the part of any village that falls inside it. `h` gives the height
/// of the ground of the chunk's own columns.
pub fn stamp(seed: u32, cx: i32, cz: i32, blocks: &mut [u8], h: &dyn Fn(i32, i32) -> i32) {
    let (ox, oz) = (cx * CW, cz * CW);
    for v in near(seed, ox, oz, ox + CW - 1, oz + CW - 1, REACH) {
        v.build(&mut Edit { blocks, ox, oz }, h);
    }
}

/// Writes blocks by world position into one chunk; whatever lies outside it is dropped.
struct Edit<'a> {
    blocks: &'a mut [u8],
    ox: i32,
    oz: i32,
}

impl Edit<'_> {
    fn inside(&self, x: i32, z: i32) -> bool {
        (self.ox..self.ox + CW).contains(&x) && (self.oz..self.oz + CW).contains(&z)
    }
    fn set(&mut self, x: i32, y: i32, z: i32, b: Block) {
        if self.inside(x, z) && (1..CH).contains(&y) {
            self.blocks[bidx(x - self.ox, y, z - self.oz)] = b;
        }
    }
    fn get(&self, x: i32, y: i32, z: i32) -> Block {
        if self.inside(x, z) && (0..CH).contains(&y) { self.blocks[bidx(x - self.ox, y, z - self.oz)] } else { AIR }
    }
    /// Makes a column end in `top` at level `g`: open above, packed with `fill` below,
    /// down to firm ground.
    fn level(&mut self, x: i32, z: i32, g: i32, fill: Block, top: Block) {
        if !self.inside(x, z) {
            return;
        }
        for y in g + 1..=g + 14 {
            self.set(x, y, z, AIR);
        }
        self.set(x, g, z, top);
        let mut y = g - 1;
        while y > 0 && !(is_opaque(self.get(x, y, z)) && !is_leaves(self.get(x, y, z))) {
            self.set(x, y, z, fill);
            y -= 1;
        }
    }
    /// Cuts a column down to level `g` where the ground stands higher, keeping its turf.
    fn trim(&mut self, x: i32, z: i32, g: i32) {
        let mut top = None;
        for y in (g + 1..=g + 14).rev() {
            let b = self.get(x, y, z);
            if top.is_none() && is_opaque(b) && !is_leaves(b) && b != LOG && b != BIRCH_LOG {
                top = Some(b);
            }
            self.set(x, y, z, AIR);
        }
        if let Some(b) = top {
            self.set(x, g, z, b);
        }
    }
}

impl Village {
    fn build(&self, e: &mut Edit, h: &dyn Fn(i32, i32) -> i32) {
        let p = &self.pal;
        for &(x, y, z) in &self.paths {
            // (the ground is as the generator left it, unless a tree stands here)
            if e.inside(x, z) && h(x - e.ox, z - e.oz) == y && is_opaque(e.get(x, y, z)) {
                e.set(x, y, z, p.path);
                if is_plant(e.get(x, y + 1, z)) || e.get(x, y + 1, z) == CACTUS {
                    for dy in 1..=4 {
                        if is_plant(e.get(x, y + dy, z)) || e.get(x, y + dy, z) == CACTUS {
                            e.set(x, y + dy, z, AIR);
                        }
                    }
                }
            }
        }
        self.well(e);
        for l in &self.lots {
            if l.farm {
                self.farm(e, l);
            } else {
                self.house(e, l);
            }
        }
        for &(x, y, z) in &self.lamps {
            if e.inside(x, z) && h(x - e.ox, z - e.oz) == y {
                for dy in 1..=6 {
                    e.set(x, y + dy, z, AIR);
                }
                e.set(x, y + 1, z, p.fence);
                e.set(x, y + 2, z, p.fence);
                e.set(x, y + 3, z, LAMP);
            }
        }
    }

    /// A paved square with a roofed well in the middle.
    fn well(&self, e: &mut Edit) {
        let (p, x, z, g) = (&self.pal, self.x, self.z, self.g);
        for dz in -3i32..=3 {
            for dx in -3i32..=3 {
                let rim = dx.abs() == 3 || dz.abs() == 3;
                e.level(x + dx, z + dz, g, p.base, if rim { p.path } else { p.base });
                if dx.abs() <= 1 && dz.abs() <= 1 {
                    let mid = dx == 0 && dz == 0;
                    e.set(x + dx, g + 1, z + dz, if mid { WATER } else { p.base });
                    if dx != 0 && dz != 0 {
                        e.set(x + dx, g + 2, z + dz, p.fence);
                        e.set(x + dx, g + 3, z + dz, p.fence);
                    }
                    e.set(x + dx, g + 4, z + dz, p.slab);
                }
            }
        }
        e.set(x, g, z, WATER);
        e.set(x, g - 1, z, WATER);
        e.set(x, g - 2, z, p.base);
    }

    /// A field of wheat with a ditch down the middle, edged with timber.
    fn farm(&self, e: &mut Edit, l: &Lot) {
        let p = &self.pal;
        for z in l.z0 - 1..=l.z1 + 1 {
            for x in l.x0 - 1..=l.x1 + 1 {
                if x < l.x0 || x > l.x1 || z < l.z0 || z > l.z1 {
                    e.trim(x, z, l.g);
                } else if x == l.x0 || x == l.x1 || z == l.z0 || z == l.z1 {
                    e.level(x, z, l.g, p.base, p.post);
                } else if if l.along_x() { z == (l.z0 + l.z1) / 2 } else { x == (l.x0 + l.x1) / 2 } {
                    e.level(x, z, l.g, DIRT, WATER);
                } else {
                    e.level(x, z, l.g, DIRT, FARMLAND);
                    e.set(x, l.g + 1, z, WHEAT_0 + (hash2(l.r, x, z) % 4) as Block);
                }
            }
        }
    }

    fn house(&self, e: &mut Edit, l: &Lot) {
        let p = &self.pal;
        let (x0, z0, x1, z1) = (l.x0, l.z0, l.x1, l.z1);
        let y0 = l.g + 1;
        let (doorx, doorz) = l.door();
        let dir = SIDE_DIR[l.side as usize];
        for z in z0 - 1..=z1 + 1 {
            for x in x0 - 1..=x1 + 1 {
                if x < x0 || x > x1 || z < z0 || z > z1 {
                    e.trim(x, z, l.g);
                } else {
                    e.level(x, z, l.g, p.base, p.floor);
                }
            }
        }
        // a step up to the door where the ground lies lower
        e.level(doorx + dir.0, doorz + dir.1, l.g, p.base, p.path);

        // walls with a post at each corner, a window in every other cell
        for z in z0..=z1 {
            for x in x0..=x1 {
                let (ex, ez) = (x == x0 || x == x1, z == z0 || z == z1);
                if !ex && !ez {
                    continue;
                }
                // how far along its wall the cell is, and how far from the door
                let (i, from_door) = if ex { (z - z0, (z - doorz).abs()) } else { (x - x0, (x - doorx).abs()) };
                let door_wall = match l.side {
                    0 => x == x0,
                    1 => x == x1,
                    2 => z == z0,
                    _ => z == z1,
                };
                let window = !(ex && ez) && i % 2 == 0 && (!door_wall || from_door >= 2);
                for dy in 0..3 {
                    let b = if ex && ez {
                        p.post
                    } else if window && dy == 1 {
                        PANE
                    } else {
                        p.wall
                    };
                    e.set(x, y0 + dy, z, b);
                }
            }
        }
        // the door stands open, so that the people of the house can come and go
        let door = DOOR_BASE + l.side + DOOR_OPEN + p.door;
        e.set(doorx, y0, doorz, door);
        e.set(doorx, y0 + 1, doorz, door + DOOR_UPPER);

        match p.roof {
            Roof::Flat(roof, rim) => {
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        let (ex, ez) = (x == x0 || x == x1, z == z0 || z == z1);
                        e.set(x, y0 + 3, z, if ex || ez { rim } else { roof });
                        if ex && ez {
                            e.set(x, y0 + 4, z, rim);
                        }
                    }
                }
            }
            Roof::Stairs(_) | Roof::Blocks(_) => {
                // a gable along the longer side: `a` runs along the ridge, `b` across it
                let along_x = l.along_x();
                let (a0, a1, b0, b1) = if along_x { (x0, x1, z0, z1) } else { (z0, z1, x0, x1) };
                let mut put = |a: i32, y: i32, b: i32, blk: Block| if along_x { e.set(a, y, b, blk) } else { e.set(b, y, a, blk) };
                // (the block of the lower row, of the upper row, and of a ridge between them)
                let (low, high, ridge) = match p.roof {
                    Roof::Stairs(mat) => {
                        let s = STAIR_BASE + (mat << 3);
                        if along_x { (s + 3, s + 2, SLAB + mat) } else { (s + 1, s, SLAB + mat) }
                    }
                    Roof::Blocks(b) => (b, b, b),
                    Roof::Flat(..) => unreachable!(),
                };
                for k in 0.. {
                    let (lo, hi, y) = (b0 - 1 + k, b1 + 1 - k, y0 + 2 + k);
                    if lo > hi {
                        break;
                    }
                    for a in a0 - 1..=a1 + 1 {
                        if lo == hi {
                            put(a, y, lo, ridge);
                        } else {
                            put(a, y, lo, low);
                            put(a, y, hi, high);
                        }
                    }
                    if k >= 1 {
                        for b in lo + 1..hi {
                            put(a0, y, b, p.wall);
                            put(a1, y, b, p.wall);
                        }
                    }
                }
            }
        }

        // furniture in the corners: beds at the back, a light and a chest or a bench by the door
        let mut corners = [(x0 + 1, z0 + 1), (x1 - 1, z0 + 1), (x0 + 1, z1 - 1), (x1 - 1, z1 - 1)];
        corners.sort_by_key(|c| (c.0 - doorx).abs() + (c.1 - doorz).abs());
        let [near_a, near_b, far_a, far_b] = corners;
        e.set(near_a.0, y0, near_a.1, TORCH);
        e.set(near_b.0, y0, near_b.1, match (l.r >> 8) % 6 {
            0 | 1 => LOOT_CHEST,
            2 | 3 => WORKBENCH,
            _ => FURNACE,
        });
        e.set(far_a.0, y0, far_a.1, BED);
        if l.beds > 1 {
            e.set(far_b.0, y0, far_b.1, BED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// every village of a seed whose well lies within `r` blocks of the origin
    fn all(seed: u32, r: i32) -> Vec<Arc<Village>> {
        near(seed, -r, -r, r, r, 0)
    }

    #[test]
    fn villages_stand_whole() {
        let mut found = 0;
        for seed in [1u32, 42, 2026, 987_654_321] {
            for v in all(seed, 5000) {
                found += 1;
                let homes = v.lots.iter().filter(|l| !l.farm).count();
                assert!(homes >= 3 && v.homes.len() >= homes && !v.spots.is_empty());
                // every chunk it touches, generated on its own, fits the others
                let at = |x: i32, y: i32, z: i32| worldgen::generate(seed, x >> 4, z >> 4, true).get(x & 15, y, z & 15);
                assert_eq!(at(v.x, v.g + 1, v.z), WATER, "seed {seed}: the well of {:?}", v.region);
                for l in v.lots.iter().filter(|l| !l.farm) {
                    assert!((l.x0 - v.x).abs().max((l.x1 - v.x).abs()).max((l.z0 - v.z).abs()).max((l.z1 - v.z).abs()) < REACH - 1);
                    let (dx, dz) = l.door();
                    assert!(is_door(at(dx, l.g + 1, dz)) && door_open(at(dx, l.g + 1, dz)) && door_upper(at(dx, l.g + 2, dz)));
                    // the four corners stand on something, the roof is on, and the way in is clear
                    let ch = worldgen::generate(seed, l.x0 >> 4, l.z0 >> 4, true);
                    let (cx, cz) = (l.x0 & 15, l.z0 & 15);
                    assert!((l.g..=l.g + 3).all(|y| is_opaque(ch.get(cx, y, cz))), "seed {seed}: a corner hangs at {} {}", l.x0, l.z0);
                    let (mx, mz) = ((l.x0 + l.x1) / 2, (l.z0 + l.z1) / 2);
                    assert!((l.g + 4..l.g + 9).any(|y| at(mx, y, mz) != AIR), "seed {seed}: no roof at {mx} {mz}");
                    let (fx, fz) = (dx + SIDE_DIR[l.side as usize].0, dz + SIDE_DIR[l.side as usize].1);
                    assert!(is_solid(at(fx, l.g, fz)) && at(fx, l.g + 1, fz) == AIR && at(fx, l.g + 2, fz) == AIR);
                }
                for h in &v.homes {
                    let (x, y, z) = (h.x.floor() as i32, h.y as i32, h.z.floor() as i32);
                    assert!(at(x, y, z) == AIR && at(x, y + 1, z) == AIR && is_opaque(at(x, y - 1, z)), "seed {seed}: no room at home {x} {y} {z}");
                }
                if found > 12 {
                    break;
                }
            }
        }
        assert!(found >= 4, "only {found} villages");
    }

    /// `cargo test --release village_survey -- --ignored --nocapture`: how many villages a
    /// world holds, and how far the nearest is from where the player starts.
    #[test]
    #[ignore]
    fn village_survey() {
        const R: i32 = 8000;
        for seed in [0u32, 1, 2, 7, 42, 2026, 131, 65536, 123_456_789, 987_654_321, 808_080, 600_613] {
            let (sx, _, sz) = worldgen::spawn_point(seed);
            let vs = all(seed, R);
            let (people, houses): (usize, usize) = vs.iter().fold((0, 0), |a, v| (a.0 + v.homes.len(), a.1 + v.lots.len()));
            let nearest = vs.iter().map(|v| (((v.x - sx) as f32).hypot((v.z - sz) as f32), v.x, v.z)).min_by(|a, b| a.0.total_cmp(&b.0));
            let squares = (2 * R / (SPACING * CW)).pow(2);
            println!(
                "seed {seed:>10}: {:3} villages in {squares} squares ({:.1} lots, {:.1} people each)  nearest to the start {:?}",
                vs.len(),
                houses as f32 / vs.len().max(1) as f32,
                people as f32 / vs.len().max(1) as f32,
                nearest.map(|n| (n.0 as i32, n.1, n.2)),
            );
        }
    }
}
