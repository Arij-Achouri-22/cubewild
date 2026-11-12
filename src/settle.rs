//! The world settling after it is changed: sand and gravel with nothing under them
//! fall, leaves cut off from every trunk wilt away, and saplings grow into trees.
//! Like the fluids, this only looks at places that were edited (`World::touched`).

use crate::gfx::{Gfx, ItemVert};
use crate::mobs::Rng;
use crate::world::*;
use glam::{IVec3, Vec3};
use std::collections::{HashSet, VecDeque};
use std::path::Path;

/// Loose blocks: they fall when nothing holds them up.
pub fn loose(b: Block) -> bool {
    matches!(b, SAND | RED_SAND | GRAVEL)
}

/// What a loose block can rest on.
fn holds(b: Block) -> bool {
    is_solid(b)
}

/// A block on its way down.
pub struct Falling {
    /// the middle of its underside
    pub pos: Vec3,
    pub vy: f32,
    pub block: Block,
}

/// What came of the settling this frame, for the game to show and drop.
#[derive(Default)]
pub struct Settled {
    /// a falling block hit something it could not rest in: it drops as an item
    pub broke: Vec<(Vec3, Block)>,
    /// a falling block came down (for the thud)
    pub landed: Vec<(Vec3, Block)>,
    /// leaves that wilted away
    pub wilted: Vec<(IVec3, Block)>,
}

#[derive(Default)]
pub struct Settle {
    pub falling: Vec<Falling>,
    /// leaves waiting to see whether they still hang from a tree: (place, seconds left)
    wilt: VecDeque<(IVec3, f32)>,
    waiting: HashSet<IVec3>,
}

/// how far (in steps through leaves) a leaf can be from a trunk and live
const REACH: i32 = 6;

impl Settle {
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut text = String::new();
        for f in &self.falling {
            text += &format!("fall {} {} {} {} {}\n", f.pos.x, f.pos.y, f.pos.z, f.vy, f.block);
        }
        for (p, timer) in &self.wilt {
            text += &format!("leaf {} {} {} {}\n", p.x, p.y, p.z, timer);
        }
        std::fs::write(path, text)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let mut settle = Self::default();
        let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid saved falling block or leaf decay");
        for line in crate::save::read_optional(path)?.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                [] => {}
                ["fall", x, y, z, vy, block] => {
                    let pos = Vec3::new(x.parse().map_err(|_| invalid())?, y.parse().map_err(|_| invalid())?, z.parse().map_err(|_| invalid())?);
                    let vy: f32 = vy.parse().map_err(|_| invalid())?;
                    let block = block.parse::<u8>().map_err(|_| invalid())?;
                    if !pos.is_finite() || !(1.0..=CH as f32).contains(&pos.y) || !vy.is_finite() || !(-40.0..=0.0).contains(&vy) || !loose(block) {
                        return Err(invalid());
                    }
                    settle.falling.push(Falling { pos, vy, block });
                }
                ["leaf", x, y, z, timer] => {
                    let p = IVec3::new(x.parse().map_err(|_| invalid())?, y.parse().map_err(|_| invalid())?, z.parse().map_err(|_| invalid())?);
                    let timer: f32 = timer.parse().map_err(|_| invalid())?;
                    if !(1..CH).contains(&p.y) || !timer.is_finite() || !(0.0..=7.0).contains(&timer) { return Err(invalid()); }
                    if settle.waiting.insert(p) {
                        settle.wilt.push_back((p, timer));
                    }
                }
                _ => return Err(invalid()),
            }
        }
        Ok(settle)
    }

    /// Runs the settling on: `touched` are the places edited since the last call, with
    /// what stood there before. `body` is where the player stands and how tall they are:
    /// a block coming down on them breaks rather than burying them.
    pub fn update(&mut self, world: &mut World, gfx: &Gfx, rng: &mut Rng, dt: f32, touched: &[(IVec3, Block)], body: (Vec3, f32)) -> Settled {
        let (out, edits) = self.advance(world, rng, dt, touched, body);
        if !edits.is_empty() {
            world.set_many(gfx, &edits);
        }
        out
    }

    fn advance(&mut self, world: &World, rng: &mut Rng, dt: f32, touched: &[(IVec3, Block)], body: (Vec3, f32)) -> (Settled, Vec<(IVec3, Block)>) {
        let mut out = Settled::default();
        let mut edits: Vec<(IVec3, Block)> = Vec::new();
        let get = |w: &World, c: IVec3| w.get_block(c.x, c.y, c.z);
        // loose blocks left hanging come loose; the leaves round a felled trunk start
        // to wonder whether they still hang from a tree
        let mut freed: Vec<IVec3> = Vec::new();
        for &(p, was) in touched {
            for c in [p, p + IVec3::Y] {
                let b = get(world, c);
                if loose(b) && !holds(get(world, c - IVec3::Y)) && !freed.contains(&c) && world.is_loaded(c.x, c.z) {
                    freed.push(c);
                }
            }
            if is_log(was) && !is_log(get(world, p)) {
                for dy in -REACH..=REACH {
                    for dz in -4..=4 {
                        for dx in -4..=4 {
                            let c = p + IVec3::new(dx, dy, dz);
                            if is_leaves(get(world, c)) && self.waiting.insert(c) {
                                self.wilt.push_back((c, rng.range(0.6, 7.0)));
                            }
                        }
                    }
                }
            }
        }
        for c in freed {
            let b = get(world, c);
            edits.push((c, AIR));
            self.falling.push(Falling { pos: c.as_vec3() + Vec3::new(0.5, 0.0, 0.5), vy: 0.0, block: b });
        }
        // falling blocks come down where something holds them, or break if what they
        // land in cannot be filled (a torch, a slab)
        self.falling.retain_mut(|f| {
            let (x, z) = (f.pos.x.floor() as i32, f.pos.z.floor() as i32);
            if !world.is_loaded(x, z) { return true; }
            f.vy = (f.vy - 30.0 * dt).max(-40.0);
            let from = f.pos.y;
            let to = from + f.vy * dt;
            if to < 1.0 {
                return false;
            }
            let top = (from - 1e-3).floor() as i32;
            let bottom = to.floor() as i32;
            for yc in (bottom..=top).rev() {
                if holds(world.get_block(x, yc, z)) {
                    let at = IVec3::new(x, yc + 1, z);
                    let here = world.get_block(at.x, at.y, at.z);
                    let mid = at.as_vec3() + 0.5;
                    let (bp, bh) = body;
                    let hw = crate::player::PLAYER_HW;
                    let on_player = (bp.x - mid.x).abs() < 0.5 + hw && (bp.z - mid.z).abs() < 0.5 + hw && bp.y < at.y as f32 + 1.0 && bp.y + bh > at.y as f32;
                    if is_replaceable(here) && !on_player && !edits.iter().any(|e| e.0 == at && e.1 != AIR) {
                        edits.push((at, f.block));
                        out.landed.push((mid, f.block));
                    } else {
                        out.broke.push((mid, f.block));
                    }
                    return false;
                }
            }
            f.pos.y = to;
            true
        });
        // leaves that waited their time: gone if no trunk holds them
        let mut gone = 0;
        let n = self.wilt.len();
        for _ in 0..n {
            let Some((c, t)) = self.wilt.pop_front() else { break };
            if !world.area_loaded(c.x as f32 + 0.5, c.z as f32 + 0.5, REACH as f32 + 0.5) {
                self.wilt.push_back((c, t));
                continue;
            }
            let t = t - dt;
            if t > 0.0 || gone >= 24 {
                self.wilt.push_back((c, t.max(0.0)));
                continue;
            }
            self.waiting.remove(&c);
            let b = get(world, c);
            if is_leaves(b) && world.is_loaded(c.x, c.z) && !held(world, c) {
                edits.push((c, AIR));
                out.wilted.push((c, b));
                gone += 1;
            }
        }
        (out, edits)
    }

    /// The falling blocks, as textured cubes.
    pub fn draw(&self, world: &World, out: &mut Vec<ItemVert>, eye: Vec3) {
        const SHADE: [f32; 6] = [0.80, 0.80, 1.0, 0.50, 0.66, 0.66];
        const UV: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for f in &self.falling {
            let c = f.pos.floor().as_ivec3();
            let sky = [0.2, 0.52, 1.0][world.sky_level(c.x, c.y, c.z).min(2) as usize];
            let o = f.pos - eye - Vec3::new(0.5, 0.0, 0.5);
            let faces = [
                [(1.0, 0.0, 0.0), (1.0, 0.0, 1.0), (1.0, 1.0, 1.0), (1.0, 1.0, 0.0)],
                [(0.0, 0.0, 1.0), (0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 1.0, 1.0)],
                [(0.0, 1.0, 1.0), (1.0, 1.0, 1.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)],
                [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 0.0, 1.0), (0.0, 0.0, 1.0)],
                [(1.0, 0.0, 1.0), (0.0, 0.0, 1.0), (0.0, 1.0, 1.0), (1.0, 1.0, 1.0)],
                [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)],
            ];
            for (fi, q) in faces.iter().enumerate() {
                let layer = tex_for(f.block, fi);
                for (v, &(x, y, z)) in q.iter().enumerate() {
                    out.push(ItemVert { pos: (o + Vec3::new(x, y, z)).into(), uv: UV[v], layer, light: SHADE[fi], sky });
                }
            }
        }
    }
}

/// True if a trunk is within reach of a leaf, through leaves.
fn held(world: &World, start: IVec3) -> bool {
    const SIDES: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
    let mut seen: HashSet<IVec3> = HashSet::from([start]);
    let mut todo = VecDeque::from([(start, 0)]);
    while let Some((c, d)) = todo.pop_front() {
        for s in SIDES {
            let n = c + s;
            let b = world.get_block(n.x, n.y, n.z);
            if is_log(b) {
                return true;
            }
            if d + 1 < REACH && is_leaves(b) && seen.len() < 600 && seen.insert(n) {
                todo.push_back((n, d + 1));
            }
        }
    }
    false
}

/// The tree a sapling at `at` grows into, if there is room for it: the blocks to set.
pub fn grow(world: &World, at: IVec3, sapling: Block, rng: &mut Rng) -> Option<Vec<(IVec3, Block)>> {
    tree(&|c: IVec3| world.get_block(c.x, c.y, c.z), at, sapling, rng)
}

fn tree(get: &dyn Fn(IVec3) -> Block, at: IVec3, sapling: Block, rng: &mut Rng) -> Option<Vec<(IVec3, Block)>> {
    let ground = at.y - 1;
    let (x, z) = (at.x, at.z);
    let soil = get(IVec3::new(x, ground, z));
    if !(soil == DIRT || soil == FARMLAND || is_grassy(soil)) {
        return None;
    }
    let kind = sapling - SAPLING;
    let pick = (rng.next() % 16) as i32;
    let mut trunk: Vec<(IVec3, Block)> = Vec::new();
    let mut crown: Vec<(IVec3, Block)> = Vec::new();
    let mut leaf = |dx: i32, dy: i32, dz: i32, b: Block| crown.push((IVec3::new(x + dx, ground + dy, z + dz), b));
    let (log, th) = match kind {
        // oak and birch: a round crown
        0 | 1 => {
            let birch = kind == 1;
            let leaves = if birch { BIRCH_LEAVES } else { LEAVES };
            let th = 4 + pick % 3 + birch as i32;
            for dy in (th - 2)..=(th + 1) {
                let rad: i32 = if dy <= th - 1 { 2 } else { 1 };
                for dz in -rad..=rad {
                    for dx in -rad..=rad {
                        if dx.abs() == rad && dz.abs() == rad && !(rad == 2 && dy == th - 2 && (rng.next() & 1) == 0) {
                            continue;
                        }
                        leaf(dx, dy, dz, leaves);
                    }
                }
            }
            (if birch { BIRCH_LOG } else { LOG }, th)
        }
        // pine: tiers narrowing to a point
        2 => {
            let th = 6 + pick % 3;
            for dy in 2..=th + 1 {
                let rad: i32 = if dy == th + 1 { 0 } else if dy == th { 1 } else { 1 + (th - dy) % 2 };
                for dz in -rad..=rad {
                    for dx in -rad..=rad {
                        if rad > 0 && dx.abs() == rad && dz.abs() == rad {
                            continue;
                        }
                        leaf(dx, dy, dz, PINE_LEAVES);
                    }
                }
            }
            (LOG, th)
        }
        // jungle: a tall trunk and a broad crown
        3 => {
            let th = 8 + pick % 5;
            for (dy, lim) in [(-2, 5), (-1, 10), (0, 10), (1, 5), (2, 1)] {
                for dz in -3i32..=3 {
                    for dx in -3i32..=3 {
                        if dx * dx + dz * dz <= lim {
                            leaf(dx, th + dy, dz, JUNGLE_LEAVES);
                        }
                    }
                }
            }
            (LOG, th)
        }
        // acacia: a bare trunk under a wide flat crown
        _ => {
            let th = 4 + pick % 3;
            for dz in -3i32..=3 {
                for dx in -3i32..=3 {
                    let d = dx * dx + dz * dz;
                    if d <= 10 {
                        leaf(dx, th, dz, ACACIA_LEAVES);
                    }
                    if d <= 2 {
                        leaf(dx, th + 1, dz, ACACIA_LEAVES);
                    }
                }
            }
            (LOG, th)
        }
    };
    // the trunk needs a clear way up; leaves only fill empty space
    for dy in 1..=th {
        let c = IVec3::new(x, ground + dy, z);
        let b = get(c);
        if !(b == AIR || c == at || is_tuft(b) || is_leaves(b)) || c.y >= CH - 1 {
            return None;
        }
        trunk.push((c, log));
    }
    let mut out = trunk;
    for (c, b) in crown {
        let here = get(c);
        if (here == AIR || is_tuft(here)) && c.y < CH - 1 && !out.iter().any(|e| e.0 == c) {
            out.push((c, b));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falling_blocks_and_leaf_decay_resume_from_their_saved_state() {
        let root = crate::save::tests::TestDir::new("settle-resume");
        let path = root.0.join("settle.txt");
        let leaf = IVec3::new(2, 14, 2);
        let mut world = World::flat(|x, y, z| (IVec3::new(x, y, z) == leaf).then_some(LEAVES));
        let mut settle = Settle::default();
        settle.falling.push(Falling { pos: Vec3::new(1.5, 12.5, 1.5), vy: -8.0, block: GRAVEL });
        settle.waiting.insert(leaf);
        settle.wilt.push_back((leaf, 0.12));
        settle.save(&path).unwrap();
        let mut resumed = Settle::load(&path).unwrap();
        let mut rng = Rng(1);
        let body = (Vec3::new(20.0, 10.0, 20.0), 1.8);
        let unloaded = std::mem::take(&mut world.chunks);
        let (_, edits) = resumed.advance(&world, &mut rng, 1.0, &[], body);
        assert!(edits.is_empty());
        assert_eq!(resumed.falling[0].pos.y, 12.5);
        assert_eq!(resumed.falling[0].vy, -8.0);
        assert_eq!(resumed.wilt.front(), Some(&(leaf, 0.12)));
        world.chunks = unloaded;
        let mut landed = 0;
        let mut wilted = 0;
        for _ in 0..20 {
            let (out, edits) = resumed.advance(&world, &mut rng, 0.05, &[], body);
            landed += out.landed.len();
            wilted += out.wilted.len();
            for (p, b) in edits { world.set_block_bg(p.x, p.y, p.z, b); }
        }
        assert_eq!((landed, wilted), (1, 1));
        assert!(resumed.falling.is_empty() && resumed.wilt.is_empty());
        assert_eq!(world.get_block(1, 10, 1), GRAVEL);
        assert_eq!(world.get_block(leaf.x, leaf.y, leaf.z), AIR);
    }

    #[test]
    fn leaves_wait_for_a_trunk_across_an_unloaded_boundary() {
        let mut world = World::flat(|x, y, z| {
            if y == 12 && z == 0 { match x { 15 => Some(LEAVES), 16 => Some(LOG), _ => None } } else { None }
        });
        let leaf = IVec3::new(15, 12, 0);
        let mut settle = Settle::default();
        settle.waiting.insert(leaf);
        settle.wilt.push_back((leaf, 0.0));
        let chunk = world.chunks.remove(&(1, 0)).unwrap();
        let body = (Vec3::splat(25.0), 1.8);
        let (out, _) = settle.advance(&world, &mut Rng(1), 1.0, &[], body);
        assert!(out.wilted.is_empty() && settle.waiting.contains(&leaf));
        world.chunks.insert((1, 0), chunk);
        let (out, _) = settle.advance(&world, &mut Rng(1), 0.05, &[], body);
        assert!(out.wilted.is_empty());
        assert!(settle.waiting.is_empty(), "the loaded trunk keeps the leaf alive");
    }

    #[test]
    fn saplings_grow_into_their_trees_where_there_is_room() {
        let ground = |c: IVec3| if c.y < 10 { DIRT } else if c.y == 10 { GRASS } else { AIR };
        let mut rng = Rng(7);
        for k in 0..5 {
            let at = IVec3::new(0, 11, 0);
            let t = tree(&ground, at, SAPLING + k, &mut rng).expect("room to grow");
            let logs = t.iter().filter(|e| is_log(e.1)).count();
            let leaves = t.iter().filter(|e| is_leaves(e.1)).count();
            assert!(logs >= 4 && leaves >= 12, "sapling {k}: {logs} logs {leaves} leaves");
            assert!(t.iter().any(|e| e.0 == at && is_log(e.1)), "the sapling becomes the trunk");
            // every leaf hangs within reach of the trunk
            assert!(t.iter().filter(|e| is_leaves(e.1)).all(|e| (e.0.x.abs() + e.0.z.abs()) <= 4));
        }
        // a roof right over it: no tree
        let roofed = |c: IVec3| if c.y <= 10 { GRASS } else if c.y == 13 { STONE } else { AIR };
        assert!(tree(&roofed, IVec3::new(0, 11, 0), SAPLING, &mut rng).is_none());
        // nor on stone
        let stone = |c: IVec3| if c.y <= 10 { STONE } else { AIR };
        assert!(tree(&stone, IVec3::new(0, 11, 0), SAPLING, &mut rng).is_none());
    }

    #[test]
    fn leaves_live_only_near_a_trunk() {
        // a trunk at x = 0 up to y 14, leaves round its top, one stray leaf far off
        let tree = |cut: bool| {
            World::flat(move |x, y, z| {
                if x == 0 && z == 0 && (10..=14).contains(&y) {
                    (!cut || y != 14).then_some(LOG)
                } else if (x - 0).abs() <= 2 && z.abs() <= 2 && (13..=15).contains(&y) {
                    Some(LEAVES)
                } else if x == 12 && z == 0 && y == 12 {
                    Some(LEAVES)
                } else {
                    None
                }
            })
        };
        let w = tree(false);
        assert!(held(&w, IVec3::new(2, 15, 2)));
        assert!(!held(&w, IVec3::new(12, 12, 0)));
        // with the top log gone the crown still touches the trunk below it
        assert!(held(&tree(true), IVec3::new(2, 15, 2)));
    }

    #[test]
    fn sand_and_gravel_fall_and_nothing_else_does() {
        assert!(loose(SAND) && loose(GRAVEL) && loose(RED_SAND));
        assert!(!loose(DIRT) && !loose(STONE) && !loose(ASH));
        assert!(holds(STONE) && holds(SLAB) && !holds(AIR) && !holds(WATER) && !holds(TORCH) && !holds(TALL_GRASS));
    }
}
