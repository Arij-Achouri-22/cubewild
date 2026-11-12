//! Flowing water and lava. A still block (`WATER`, `LAVA`) is a source; it pours
//! straight down if it can, and otherwise runs out sideways, one level weaker each
//! block (water seven blocks, lava three) as `WATER_FLOW` / `LAVA_FLOW` blocks. Flowing
//! blocks with nothing feeding them dry up again. Only places that changed are looked
//! at: `World` notes every edit, and this works through those and what they touch.
//! Water that meets lava hardens it: a source into cinderstone (as with a bucket),
//! running lava into cobblestone; lava that runs into water turns to stone.

use crate::world::*;
use glam::IVec3;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// how far each runs from its source
const WATER_RUN: u8 = 7;
const LAVA_RUN: u8 = 3;
/// cells looked at per step, per fluid (the rest wait for the next step)
const BUDGET: usize = 320;

const SIDES: [IVec3; 4] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
const AROUND: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y, IVec3::NEG_Y];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Water,
    Lava,
}

impl Kind {
    fn is(self, b: Block) -> bool {
        match self {
            Kind::Water => is_water(b),
            Kind::Lava => is_lava(b),
        }
    }
    fn run(self) -> u8 {
        match self {
            Kind::Water => WATER_RUN,
            Kind::Lava => LAVA_RUN,
        }
    }
    /// The flowing block of a level (1 ..= run).
    fn flow(self, level: u8) -> Block {
        match self {
            Kind::Water => WATER_FLOW + (WATER_RUN - level) as Block,
            Kind::Lava => LAVA_FLOW + (LAVA_RUN - level) as Block,
        }
    }
}

/// How strong a block of the fluid is: a source is one past the longest run, flowing
/// blocks count down to 1, anything else 0.
pub fn level(kind: Kind, b: Block) -> u8 {
    match kind {
        Kind::Water if b == WATER => WATER_RUN + 1,
        Kind::Water if (WATER_FLOW..WATER_FLOW_END).contains(&b) => WATER_RUN - (b - WATER_FLOW) as u8,
        Kind::Lava if b == LAVA => LAVA_RUN + 1,
        Kind::Lava if (LAVA_FLOW..LAVA_FLOW_END).contains(&b) => LAVA_RUN - (b - LAVA_FLOW) as u8,
        _ => 0,
    }
}

/// A fluid washes these away as it runs into them.
fn open(b: Block) -> bool {
    b == AIR || is_tuft(b) || b == DEAD_BUSH || (is_plant(b) && b != CACTUS) || is_torch(b)
}

#[derive(Default)]
pub struct Fluids {
    water: VecDeque<IVec3>,
    lava: VecDeque<IVec3>,
    queued: HashSet<(IVec3, bool)>,
    water_t: f32,
    lava_t: f32,
    waking: VecDeque<Wake>,
    waking_keys: HashSet<Key>,
}

struct Wake {
    key: Key,
    data: Arc<ChunkData>,
    next: usize,
}

impl Fluids {
    fn push(&mut self, p: IVec3, lava: bool) {
        if !(1..CH).contains(&p.y) { return; }
        if self.queued.insert((p, lava)) {
            if lava { self.lava.push_back(p) } else { self.water.push_back(p) }
        }
    }

    #[cfg(test)]
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.encode())
    }

    pub fn encode(&self) -> String {
        let mut text = format!("timers {} {}\n", self.water_t, self.lava_t);
        for (name, queue) in [("water", &self.water), ("lava", &self.lava)] {
            for p in queue {
                text += &format!("{name} {} {} {}\n", p.x, p.y, p.z);
            }
        }
        text
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let mut fluids = Self::default();
        let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid saved fluid state");
        for line in crate::save::read_optional(path)?.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                [] => {}
                ["timers", water, lava] => {
                    let water: f32 = water.parse().map_err(|_| invalid())?;
                    let lava: f32 = lava.parse().map_err(|_| invalid())?;
                    if !water.is_finite() || !lava.is_finite() || !(0.0..=1.0).contains(&water) || !(0.0..=1.0).contains(&lava) {
                        return Err(invalid());
                    }
                    (fluids.water_t, fluids.lava_t) = (water, lava);
                }
                [kind @ ("water" | "lava"), x, y, z] => {
                    let p = IVec3::new(x.parse().map_err(|_| invalid())?, y.parse().map_err(|_| invalid())?, z.parse().map_err(|_| invalid())?);
                    if !(1..CH).contains(&p.y) { return Err(invalid()); }
                    fluids.push(p, *kind == "lava");
                }
                _ => return Err(invalid()),
            }
        }
        Ok(fluids)
    }

    /// Older saves have no queue. Resume flows and exposed sources in saved chunks;
    /// untouched generated lakes do not need a simulation job for every water cell.
    pub fn chunk_loaded(&mut self, world: &World, key: Key, saved: bool) {
        if !saved || self.waking_keys.contains(&key) { return; }
        let Some(chunk) = world.chunks.get(&key) else { return };
        self.waking_keys.insert(key);
        self.waking.push_back(Wake { key, data: chunk.data.clone(), next: 0 });
    }

    /// Resuming a saved sea or lava pool must not scan several full chunks in one
    /// frame. The chunk snapshot stays alive while its cells are checked in batches.
    fn wake(&mut self, world: &World, budget: usize) {
        let start = Instant::now();
        let mut left = budget;
        let mut plan = Vec::new();
        while left > 0 {
            let Some(mut wake) = self.waking.pop_front() else { break };
            if !world.chunks.contains_key(&wake.key) {
                self.waking_keys.remove(&wake.key);
                continue;
            }
            let key = wake.key;
            let end = (wake.next + left).min(wake.data.blocks.len());
            while wake.next < end {
                if left % 128 == 0 && start.elapsed() >= Duration::from_micros(500) {
                    self.waking.push_front(wake);
                    return;
                }
                let i = wake.next;
                let b = wake.data.blocks[i];
                wake.next += 1;
                left -= 1;
                let kind = if is_water(b) { Kind::Water } else if is_lava(b) { Kind::Lava } else { continue };
                let p = IVec3::new(key.0 * CW + (i as i32 & 15), i as i32 >> 8, key.1 * CW + ((i as i32 >> 4) & 15));
                let source = b == WATER || b == LAVA;
                plan.clear();
                if source && world.area_loaded(p.x as f32 + 0.5, p.z as f32 + 0.5, 1.0) {
                    think(&|c| world.get_block(c.x, c.y, c.z), kind, p, &mut plan);
                    if plan.is_empty() { continue; }
                }
                self.push(p, kind == Kind::Lava);
            }
            if wake.next < wake.data.blocks.len() {
                self.waking.push_front(wake);
            } else {
                self.waking_keys.remove(&key);
            }
        }
    }

    /// Takes the edits the world noted: each edited place and its neighbours are looked
    /// at again by whichever fluid is there or beside it.
    fn gather(&mut self, world: &World, touched: &[(IVec3, Block)]) {
        for &(p, _) in touched {
            let (mut w, mut l) = (false, false);
            for c in std::iter::once(p).chain(AROUND.iter().map(|&d| p + d)) {
                let b = world.get_block(c.x, c.y, c.z);
                w |= is_water(b);
                l |= is_lava(b);
            }
            if w || l {
                for c in std::iter::once(p).chain(AROUND.iter().map(|&d| p + d)) {
                    let b = world.get_block(c.x, c.y, c.z);
                    if w && (is_water(b) || open(b)) {
                        self.push(c, false);
                    }
                    if l && (is_lava(b) || open(b)) {
                        self.push(c, true);
                    }
                }
            }
        }
    }

    /// Runs the fluids on. `touched` are the places edited since the last call. Returns
    /// what running water washed away (torches, plants, crops), to be dropped as items.
    pub fn update(&mut self, world: &mut World, dt: f32, touched: &[(IVec3, Block)]) -> Vec<(IVec3, Block)> {
        self.wake(world, 4096);
        self.gather(world, touched);
        self.water_t += dt;
        self.lava_t += dt;
        // water runs quickly; lava slowly, if less so in the heat of the Emberdeep
        let lava_tick = if world.is_deep() { 0.5 } else { 1.0 };
        let mut edits: HashMap<IVec3, Block> = HashMap::new();
        if self.water_t >= 0.25 {
            self.water_t = 0.0;
            self.step(world, Kind::Water, &mut edits);
        }
        if self.lava_t >= lava_tick {
            self.lava_t = 0.0;
            self.step(world, Kind::Lava, &mut edits);
        }
        let mut washed = Vec::new();
        if !edits.is_empty() {
            let edits: Vec<(IVec3, Block)> = edits.into_iter().collect();
            for &(p, b) in &edits {
                let was = world.get_block(p.x, p.y, p.z);
                // (lava burns what it runs over)
                if was != AIR && !is_fluid(was) && open(was) && is_water(b) {
                    washed.push((p, was));
                }
            }
            world.set_many(&edits);
        }
        washed
    }

    /// One step of a fluid: every waiting cell decides what it becomes from the world as
    /// it stands; the changes go in together afterwards.
    fn step(&mut self, world: &World, kind: Kind, edits: &mut HashMap<IVec3, Block>) {
        let lava = kind == Kind::Lava;
        let n = if lava { self.lava.len() } else { self.water.len() }.min(BUDGET);
        let mut plan: Vec<(IVec3, Block)> = Vec::new();
        for _ in 0..n {
            let p = if lava { self.lava.pop_front() } else { self.water.pop_front() };
            let Some(p) = p else { break };
            self.queued.remove(&(p, lava));
            if !world.area_loaded(p.x as f32 + 0.5, p.z as f32 + 0.5, 1.0) {
                self.push(p, lava);
                continue;
            }
            think(&|c: IVec3| world.get_block(c.x, c.y, c.z), kind, p, &mut plan);
        }
        for (p, b) in plan {
            merge_plan(edits, kind, p, b);
        }
    }
}

/// Two plans for one place: rock made where the fluids met stays; otherwise the stronger
/// fluid wins (so a cell that would dry up stays wet if a neighbour feeds it).
fn merge_plan(edits: &mut HashMap<IVec3, Block>, kind: Kind, p: IVec3, b: Block) {
    let hard = |x: Block| x != AIR && !is_fluid(x);
    match edits.get(&p) {
        Some(&e) if hard(e) => {}
        Some(&e) if !(hard(b) || level(kind, b) > level(kind, e) || !kind.is(e)) => {}
        _ => {
            edits.insert(p, b);
        }
    }
}

/// How strongly the fluid in a cell runs out sideways (its level), or 0 if it does not:
/// running fluid falls rather than spreading while it can, and joins more of its kind
/// below it; a still block spreads unless it can fall.
fn sideways(at: &dyn Fn(IVec3) -> Block, kind: Kind, c: IVec3) -> u8 {
    let lvl = level(kind, at(c));
    if lvl == 0 {
        return 0;
    }
    let under = at(c - IVec3::Y);
    if open(under) || meet(kind, under).is_some() || (kind.is(under) && lvl <= kind.run()) {
        return 0;
    }
    lvl
}

/// What a cell of the fluid does: dries up, changes level, or runs on.
fn think(at: &dyn Fn(IVec3) -> Block, kind: Kind, p: IVec3, plan: &mut Vec<(IVec3, Block)>) {
    let b = at(p);
    let run = kind.run();
    let mut lvl = level(kind, b);
    if lvl == 0 {
        // an empty cell: fill it if a neighbour feeds it (the neighbours also spread
        // into it themselves; this catches a cell freed beside still fluid)
        if !open(b) {
            return;
        }
        let feed = if kind.is(at(p + IVec3::Y)) { run } else { SIDES.iter().map(|&d| sideways(at, kind, p + d)).max().unwrap_or(0).saturating_sub(1) };
        if feed > 0 {
            plan.push((p, kind.flow(feed.min(run))));
        }
        return;
    }
    if lvl <= run {
        // flowing: as strong as what feeds it, gone if nothing does
        let from_above = kind.is(at(p + IVec3::Y));
        let side = SIDES.iter().map(|&d| sideways(at, kind, p + d)).max().unwrap_or(0);
        let mut want = if from_above { run } else { side.saturating_sub(1) };
        // two water sources side by side over something firm make a third
        if kind == Kind::Water && !from_above {
            let sources = SIDES.iter().filter(|&&d| at(p + d) == WATER).count();
            let under = at(p - IVec3::Y);
            if sources >= 2 && (under == WATER || is_solid(under)) {
                plan.push((p, WATER));
                return;
            }
        }
        want = want.min(run);
        if want == 0 {
            plan.push((p, AIR));
            return;
        }
        if want != lvl {
            plan.push((p, kind.flow(want)));
            return;
        }
        lvl = want;
    }
    // run on: down first, sideways only when it cannot fall
    let below = p - IVec3::Y;
    let under = at(below);
    if let Some(hard) = meet(kind, under) {
        plan.push((below, hard));
        return;
    }
    if open(under) && below.y >= 1 {
        plan.push((below, kind.flow(run)));
        return;
    }
    if kind.is(under) && level(kind, under) < run {
        // falling onto a thin run of its own kind tops it up
        plan.push((below, kind.flow(run)));
        return;
    }
    if kind.is(under) && lvl <= run {
        // running water that lands in water joins it: only a still block spreads out
        // over the top of more of its kind
        return;
    }
    if lvl <= 1 {
        return;
    }
    let next = lvl.min(run + 1) - 1;
    for d in SIDES {
        let c = p + d;
        let nb = at(c);
        if let Some(hard) = meet(kind, nb) {
            plan.push((c, hard));
        } else if open(nb) || (kind.is(nb) && level(kind, nb) < next) {
            plan.push((c, kind.flow(next)));
        }
    }
}

/// The way the fluid in a cell pushes what is in it: towards weaker neighbours, and down
/// where it falls. Still water among still water pushes nothing. Length 0..1.
pub fn current(world: &World, c: IVec3) -> glam::Vec3 {
    let at = |p: IVec3| world.get_block(p.x, p.y, p.z);
    let b = at(c);
    let kind = if is_water(b) {
        Kind::Water
    } else if is_lava(b) {
        Kind::Lava
    } else {
        return glam::Vec3::ZERO;
    };
    let lvl = level(kind, b) as f32;
    let mut v = glam::Vec3::ZERO;
    for d in SIDES {
        let nb = at(c + d);
        let diff = if kind.is(nb) {
            lvl - level(kind, nb) as f32
        } else if open(nb) && lvl <= kind.run() as f32 {
            // running off an edge
            lvl
        } else {
            0.0
        };
        v += d.as_vec3() * diff;
    }
    let mut v = v.normalize_or_zero();
    if lvl <= kind.run() as f32 && kind.is(at(c + IVec3::Y)) {
        // a falling column bears down
        v.y -= 1.0;
        v = v.normalize_or_zero();
    }
    v
}

/// What the fluid makes of the other one when it runs into it, if it does.
fn meet(kind: Kind, other: Block) -> Option<Block> {
    match kind {
        Kind::Water if other == LAVA => Some(CINDERSTONE),
        Kind::Water if is_lava(other) => Some(COBBLE),
        Kind::Lava if is_water(other) => Some(if other == WATER { STONE } else { COBBLE }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn saved_fluid_work_keeps_its_timing_and_resumes_after_chunks_load() {
        let root = crate::save::tests::TestDir::new("fluid-resume");
        let path = root.0.join("fluids.txt");
        for (kind, block) in [(Kind::Water, WATER), (Kind::Lava, LAVA)] {
            let mut world = World::flat(|x, y, z| (x == 0 && y == 10 && z == 0).then_some(block));
            let mut fluids = Fluids { water_t: 0.12, lava_t: 0.42, ..Default::default() };
            fluids.gather(&world, &[(IVec3::new(0, 10, 0), AIR)]);
            fluids.save(&path).unwrap();
            let mut resumed = Fluids::load(&path).unwrap();
            assert_eq!(resumed.water, fluids.water);
            assert_eq!(resumed.lava, fluids.lava);
            assert_eq!((resumed.water_t, resumed.lava_t), (0.12, 0.42));
            let unloaded = std::mem::take(&mut world.chunks);
            let mut edits = HashMap::new();
            resumed.step(&world, kind, &mut edits);
            assert!(edits.is_empty());
            assert!(!resumed.queued.is_empty(), "work waits instead of being discarded");
            world.chunks = unloaded;
            resumed.step(&world, kind, &mut edits);
            assert_eq!(edits[&IVec3::new(1, 10, 0)], kind.flow(kind.run()));
        }
    }

    #[test]
    fn old_saved_flows_and_exposed_sources_wake_on_chunk_load() {
        let world = World::flat(|x, y, z| {
            if y == 10 && z == 0 { match x { 2 => Some(WATER), 3 => Some(WATER_FLOW), 6 => Some(LAVA), _ => None } } else { None }
        });
        let mut fluids = Fluids::default();
        fluids.chunk_loaded(&world, (0, 0), false);
        assert!(fluids.queued.is_empty(), "generated lakes need no full scan");
        fluids.chunk_loaded(&world, (0, 0), true);
        while !fluids.waking.is_empty() { fluids.wake(&world, VOL); }
        let mut edits = HashMap::new();
        fluids.step(&world, Kind::Water, &mut edits);
        fluids.step(&world, Kind::Lava, &mut edits);
        assert!(is_water(edits[&IVec3::new(4, 10, 0)]));
        assert!(is_lava(edits[&IVec3::new(7, 10, 0)]));
    }

    #[test]
    fn saved_chunks_wake_in_bounded_batches_and_can_be_requeued_after_unloading() {
        let mut world = World::flat(|_, y, _| (y == 10).then_some(WATER));
        let mut fluids = Fluids::default();
        fluids.chunk_loaded(&world, (0, 0), true);
        fluids.chunk_loaded(&world, (0, 0), true);
        assert_eq!(fluids.waking.len(), 1);
        assert_eq!(fluids.waking[0].next, 0, "loading only enqueues a snapshot");
        fluids.wake(&world, 128);
        assert!(fluids.waking[0].next <= 128);
        let chunk = world.chunks.remove(&(0, 0)).unwrap();
        fluids.wake(&world, 128);
        assert!(fluids.waking.is_empty() && fluids.waking_keys.is_empty());
        world.chunks.insert((0, 0), chunk);
        fluids.chunk_loaded(&world, (0, 0), true);
        assert_eq!(fluids.waking.len(), 1);
    }

    #[test]
    fn invalid_fluid_state_is_reported_instead_of_reset() {
        let root = crate::save::tests::TestDir::new("invalid-fluids");
        let path = root.0.join("fluids.txt");
        assert!(Fluids::load(&path).unwrap().queued.is_empty());
        for text in ["timers NaN 0\n", "water 0 256 0\n", "lava 0 10\n"] {
            std::fs::write(&path, text).unwrap();
            assert!(Fluids::load(&path).is_err());
        }
    }

    /// Runs the rules on a world-free overlay of `base` for a number of steps, starting
    /// from `todo` (as `Fluids::step` does on the real world).
    fn simulate(base: &dyn Fn(IVec3) -> Block, over: &mut HashMap<IVec3, Block>, kind: Kind, mut todo: Vec<IVec3>, steps: usize) {
        for _ in 0..steps {
            let mut plan = Vec::new();
            {
                let get = |c: IVec3| over.get(&c).copied().unwrap_or_else(|| base(c));
                for &p in &todo {
                    think(&get, kind, p, &mut plan);
                }
            }
            todo.clear();
            let mut merged = HashMap::new();
            for (p, b) in plan {
                merge_plan(&mut merged, kind, p, b);
            }
            for (p, b) in merged {
                let now = over.get(&p).copied().unwrap_or_else(|| base(p));
                if now != b {
                    over.insert(p, b);
                    todo.push(p);
                    todo.extend(AROUND.iter().map(|&d| p + d));
                }
            }
            todo.sort_by_key(|p| (p.x, p.y, p.z));
            todo.dedup();
        }
    }

    /// stone up to y = 9, air above, and a pit two deep at x = 2
    fn ground(c: IVec3) -> Block {
        if c.y <= 9 && !(c.x == 2 && c.z == 0 && c.y >= 8) {
            STONE
        } else {
            AIR
        }
    }

    #[test]
    fn a_source_runs_out_and_stops() {
        let flat = |c: IVec3| if c.y <= 9 { STONE } else { AIR };
        let mut over = HashMap::new();
        over.insert(IVec3::new(0, 10, 0), WATER);
        simulate(&flat, &mut over, Kind::Water, vec![IVec3::new(0, 10, 0)], 40);
        let at = |x, z| over.get(&IVec3::new(x, 10, z)).copied().unwrap_or(AIR);
        assert_eq!(level(Kind::Water, at(1, 0)), 7);
        assert_eq!(level(Kind::Water, at(7, 0)), 1);
        assert_eq!(at(8, 0), AIR);
        let mut over = HashMap::new();
        over.insert(IVec3::new(0, 10, 0), LAVA);
        simulate(&flat, &mut over, Kind::Lava, vec![IVec3::new(0, 10, 0)], 20);
        assert_eq!(level(Kind::Lava, over.get(&IVec3::new(3, 10, 0)).copied().unwrap_or(AIR)), 1);
        assert_eq!(over.get(&IVec3::new(4, 10, 0)).copied().unwrap_or(AIR), AIR);
    }

    #[test]
    fn it_pours_into_a_hole_and_dries_when_cut_off() {
        let mut over = HashMap::new();
        over.insert(IVec3::new(0, 10, 0), WATER);
        simulate(&ground, &mut over, Kind::Water, vec![IVec3::new(0, 10, 0)], 30);
        assert!(is_water(over[&IVec3::new(2, 8, 0)]), "the pit fills");
        // the source is taken away: everything it fed runs dry
        over.insert(IVec3::new(0, 10, 0), AIR);
        let todo: Vec<IVec3> = over.keys().copied().collect();
        simulate(&ground, &mut over, Kind::Water, todo, 40);
        let wet: Vec<_> = over.iter().filter(|e| is_water(*e.1)).collect();
        assert!(wet.is_empty(), "{wet:?}");
    }

    #[test]
    fn water_poured_from_a_post_does_not_hang_in_the_air() {
        // a source on a post one block high: it spills over the edges and spreads on
        // the ground, but nothing stays up beside the source but what falls from it
        let base = |c: IVec3| if c.y <= 9 || c == IVec3::new(0, 10, 0) { STONE } else { AIR };
        let mut over = HashMap::new();
        over.insert(IVec3::new(0, 11, 0), WATER);
        simulate(&base, &mut over, Kind::Water, vec![IVec3::new(0, 11, 0)], 60);
        let up: Vec<_> = over.iter().filter(|(p, b)| p.y == 11 && is_water(**b)).map(|e| *e.0).collect();
        assert!(up.len() <= 5, "{up:?}");
        let ground = over.iter().filter(|(p, b)| p.y == 10 && is_water(**b)).count();
        assert!(ground > 40, "{ground}");
    }

    #[test]
    fn a_freed_cell_beside_still_water_fills() {
        // a lake at y 10, a stone wall at x = 1 that is mined away
        let lake = |c: IVec3| if c.y <= 9 { STONE } else if c.y == 10 && c.x <= 0 { WATER } else { AIR };
        let mut over = HashMap::new();
        simulate(&lake, &mut over, Kind::Water, vec![IVec3::new(1, 10, 0), IVec3::new(0, 10, 0)], 10);
        assert!(is_water(over[&IVec3::new(1, 10, 0)]));
        assert!(is_water(over[&IVec3::new(3, 10, 0)]));
    }

    #[test]
    fn running_water_pushes_downstream_and_still_water_does_not() {
        // a source at x = 0 running out along +x over flat stone
        let mut w = World::flat(|x, y, _| (y == 10).then_some(match x {
            0 => WATER,
            1 => WATER_FLOW,
            2 => WATER_FLOW + 1,
            _ => AIR,
        }));
        let _ = &mut w;
        let v = current(&w, IVec3::new(1, 10, 0));
        assert!(v.x > 0.9 && v.y.abs() < 1e-3, "{v}");
        let lake = World::flat(|_, y, _| (y == 10 || y == 11).then_some(WATER));
        assert_eq!(current(&lake, IVec3::new(5, 10, 5)), glam::Vec3::ZERO);
    }

    #[test]
    fn water_on_lava_hardens_it() {
        assert_eq!(meet(Kind::Water, LAVA), Some(CINDERSTONE));
        assert_eq!(meet(Kind::Lava, WATER), Some(STONE));
        assert_eq!(meet(Kind::Water, STONE), None);
        // lava running into a lake
        let base = |c: IVec3| if c.y <= 9 { STONE } else if c.y == 10 && c.x >= 2 { WATER } else { AIR };
        let mut over = HashMap::new();
        over.insert(IVec3::new(0, 10, 0), LAVA);
        simulate(&base, &mut over, Kind::Lava, vec![IVec3::new(0, 10, 0)], 10);
        assert_eq!(over[&IVec3::new(2, 10, 0)], STONE);
    }
}
