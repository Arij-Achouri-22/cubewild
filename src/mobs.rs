//! Animals: huntable deer, boar and hare, and wolves that hunt you at night.
//! Monsters: zombies, skeleton archers, giant spiders and slimes that roam the dark.
//! Villagers: the peaceful people of the villages, who keep to their paths and houses.
//! Farm life: sheep to shear and dye, animals that follow their food and breed, wolves
//! tamed with bones into dogs. And the puffcap, a mushroom that creeps up and bursts.

use crate::gfx::{EntVert, EMISSIVE};
use crate::items::{self, Item, ARROW, BERRIES, BONE, COOKED_MEAT, FIBER, HIDE, IRON_INGOT, MEAT, SEEDS, SHEARS, WHEAT};
use crate::path::{settle, walk_line, Body, Finder, Probe};
use crate::player::{collides, move_step, ray_aabb, raycast_with};
use crate::village::{self, Village};
use crate::world::{is_fluid, is_lava, hash2, is_grassy, is_ladder, is_leaves, is_solid, World, AIR, CLAY, CLOTH, COLOURS, MUSHROOM, SEA, SNOW};
use glam::{IVec3, Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    pub fn f(&mut self) -> f32 {
        (self.next() & 0xff_ffff) as f32 / 16_777_216.0
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Deer,
    Boar,
    Hare,
    Wolf,
    Zombie,
    Skeleton,
    Spider,
    Slime,
    Villager,
    Sheep,
    Puffcap,
    /// the Emberdeep: scorchhounds hunt in packs across the floors
    Hound,
    /// emberwisps float in the air and throw fire
    Wisp,
    /// cinder brutes guard the ruined fortresses
    Brute,
}

/// Every kind, in the order they are saved by.
pub const KINDS: [Kind; 14] = [Kind::Deer, Kind::Boar, Kind::Hare, Kind::Wolf, Kind::Zombie, Kind::Skeleton, Kind::Spider, Kind::Slime, Kind::Villager, Kind::Sheep, Kind::Puffcap, Kind::Hound, Kind::Wisp, Kind::Brute];

/// seconds a puffcap's fuse burns before it bursts
pub const FUSE: f32 = 1.5;
/// seconds a baby takes to grow up
const GROW: f32 = 600.0;

impl Kind {
    pub fn is_monster(self) -> bool {
        matches!(self, Kind::Zombie | Kind::Skeleton | Kind::Spider | Kind::Slime | Kind::Puffcap | Kind::Hound | Kind::Wisp | Kind::Brute)
    }
    /// creatures of the Emberdeep: lava does not burn them
    pub fn fireproof(self) -> bool {
        matches!(self, Kind::Hound | Kind::Wisp | Kind::Brute)
    }
    /// What it eats: held, it lures the animal; fed, it breeds (or heals a dog).
    pub fn food(self) -> &'static [Item] {
        match self {
            Kind::Sheep | Kind::Deer => &[WHEAT],
            Kind::Boar => &[BERRIES, MUSHROOM as Item],
            Kind::Hare => &[SEEDS, BERRIES],
            Kind::Wolf => &[MEAT, COOKED_MEAT],
            _ => &[],
        }
    }
    /// the sun burns the undead
    fn undead(self) -> bool {
        matches!(self, Kind::Zombie | Kind::Skeleton)
    }
}

/// What a kill leaves behind: up to two stacks (a count of 0 means nothing).
pub type Drops = [(Item, u32); 2];

struct Spec {
    hw: f32,
    h: f32,
    hp: i32,
    walk: f32,
    run: f32,
    /// distance at which it bolts from the player (0 = only when hurt)
    wary: f32,
    meat: u32,
    hide: u32,
    /// how far it notices the player and gives chase (0 = never hunts)
    sight: f32,
    /// damage of its bite or blow
    dmg: f32,
}

fn spec(k: Kind) -> Spec {
    let sp = |hw, h, hp, walk, run, wary, meat, hide, sight, dmg| Spec { hw, h, hp, walk, run, wary, meat, hide, sight, dmg };
    match k {
        Kind::Deer => sp(0.32, 1.5, 3, 1.3, 6.9, 9.0, 3, 2, 0.0, 0.0),
        Kind::Boar => sp(0.36, 0.9, 4, 1.1, 5.2, 0.0, 2, 1, 0.0, 0.0),
        Kind::Hare => sp(0.18, 0.5, 1, 1.6, 7.4, 6.0, 1, 1, 0.0, 0.0),
        // slower than a walking player (4.4), and a bite costs one heart
        Kind::Wolf => sp(0.3, 0.85, 6, 1.5, 4.0, 0.0, 0, 1, 26.0, 2.0),
        Kind::Zombie => sp(0.3, 1.8, 10, 0.9, 3.0, 0.0, 0, 0, 22.0, 3.0),
        Kind::Skeleton => sp(0.25, 1.8, 7, 1.6, 3.2, 0.0, 0, 0, 20.0, 0.0),
        Kind::Spider => sp(0.4, 0.6, 5, 1.4, 5.6, 0.0, 0, 0, 16.0, 2.0),
        Kind::Slime => sp(0.4, 0.7, 5, 2.2, 3.6, 0.0, 0, 0, 14.0, 2.0),
        Kind::Villager => sp(0.3, 1.8, 8, 1.5, 4.2, 0.0, 0, 0, 0.0, 0.0),
        Kind::Sheep => sp(0.32, 1.25, 4, 1.1, 4.8, 0.0, 2, 0, 0.0, 0.0),
        // it does no biting: it bursts
        Kind::Puffcap => sp(0.3, 1.3, 8, 1.2, 3.4, 0.0, 0, 0, 18.0, 0.0),
        // a hound runs about as fast as you walk, and bites hard
        Kind::Hound => sp(0.3, 0.9, 8, 1.8, 4.6, 0.0, 1, 0, 26.0, 3.0),
        // (it throws fire instead of biting)
        Kind::Wisp => sp(0.3, 0.8, 6, 1.6, 3.4, 0.0, 0, 0, 28.0, 0.0),
        Kind::Brute => sp(0.48, 2.5, 34, 1.0, 2.7, 0.0, 0, 0, 18.0, 7.0),
    }
}

/// The wool item of a colour.
pub fn wool_item(c: u8) -> Item {
    if c == 0 { CLOTH as Item } else { items::WOOL_ITEM + c - 1 }
}

fn drops(m: &Mob, rng: &mut Rng) -> Drops {
    let k = m.kind;
    let s = spec(k);
    if m.baby < 1.0 {
        return [(0, 0), (0, 0)];
    }
    match k {
        Kind::Zombie => [(FIBER, 1 + rng.next() % 2), (IRON_INGOT, (rng.next() % 6 == 0) as u32)],
        Kind::Skeleton => [(ARROW, rng.next() % 3), (BONE, 1 + rng.next() % 2)],
        Kind::Sheep => [(MEAT, 1 + rng.next() % 2), (wool_item(m.wool), !m.sheared as u32)],
        Kind::Puffcap => [(MUSHROOM as Item, 1 + rng.next() % 2), (0, 0)],
        Kind::Spider => [(FIBER, 1 + rng.next() % 2), (0, 0)],
        Kind::Slime => [(CLAY as Item, 1 + rng.next() % 2), (0, 0)],
        Kind::Villager => [(0, 0), (0, 0)],
        Kind::Hound => [(MEAT, 1 + rng.next() % 2), (BONE, rng.next() % 2)],
        Kind::Wisp => [(items::EMBER, 1 + rng.next() % 2), (0, 0)],
        // a brute now and then carries a bar of pyre metal
        Kind::Brute => [(items::GOLD_INGOT, 1 + rng.next() % 3), (items::PYRE_INGOT, (rng.next() % 4 == 0) as u32)],
        _ => [(MEAT, s.meat), (HIDE, s.hide)],
    }
}

/// True if nothing solid lies on the straight line from `a` to `b` (an open doorway is
/// clear, a window pane is not).
fn clear_path(world: &World, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let len = d.length();
    len < 1e-3 || raycast_with(world, a, d / len, len, is_solid).is_none()
}

pub struct Mob {
    pub kind: Kind,
    pub pos: Vec3,
    vel: Vec3,
    yaw: f32,
    heading: f32,
    moving: bool,
    timer: f32,
    flee: f32,
    hurt: f32,
    hp: i32,
    phase: f32,
    dead: f32, // seconds since death, <0 = alive
    on_ground: bool,
    stuck: u8,
    /// seconds spent trying to move without getting anywhere
    stall: f32,
    grazing: bool,
    /// 0 = head up, 1 = head down at the grass (animated)
    graze: f32,
    hspeed: f32,
    age: f32,
    /// seconds until it can bite (or shoot) again
    bite: f32,
    /// seconds until a slime's next hop, or a spider's next leap
    hop: f32,
    /// seconds until the sun next scorches the undead
    burn: f32,
    /// cells still to walk through on the way to the player (used while `direct` is false)
    path: Vec<IVec3>,
    path_i: usize,
    /// seconds until the route is planned again
    repath: f32,
    /// the straight line to the player is walkable: no path needed
    direct: bool,
    /// seconds for which the straight line is not trusted (it got the mob stuck)
    distrust: f32,
    /// a skeleton's line of sight to the player, checked when it plans
    los: bool,
    /// a villager's village, and which of its homes is this one's
    village: Option<Arc<Village>>,
    slot: usize,
    /// where a villager is walking to
    goal: Option<Vec3>,
    /// a sheep's colour (see `COLOURS`), shorn or not, and seconds until it may grow back
    pub wool: u8,
    pub sheared: bool,
    regrow: f32,
    /// seconds left of looking for a mate, and until it can breed again
    pub love: f32,
    breed_cd: f32,
    /// 0 newborn .. 1 grown
    pub baby: f32,
    /// a dog: follows the player, fights monsters; sitting dogs stay put
    pub tame: bool,
    pub sitting: bool,
    /// saved with the world and never wanders off (pets, and animals that were bred)
    pub keep: bool,
    /// a puffcap's fuse: seconds burnt, below 0 when not lit
    pub fuse: f32,
}

/// What happened to the creatures this frame, for the game to act on.
pub enum MobEvent {
    Born { kind: Kind, pos: Vec3 },
    /// slain by something other than the player (a dog)
    Killed { kind: Kind, pos: Vec3, drops: Drops },
    /// a puffcap's fuse was lit
    Fuse { pos: Vec3 },
    Explode { pos: Vec3 },
}

/// What using an item on a creature did.
pub enum Use {
    Nothing,
    /// ate the item (a baby grows a little faster)
    Fed,
    /// ate it and is now looking for a mate
    Love,
    Sheared { item: Item, n: u32 },
    Dyed,
    Tamed,
    /// the bone was eaten but the wolf is not won over yet
    NotYet,
    Healed,
    Sit(bool),
    Trade,
}

/// Plans a mob's way to `target`: straight if that works, otherwise round the obstacles.
fn plan(m: &mut Mob, s: &Spec, target: Vec3, drop: i32, probe: &mut Probe<'_>, finder: &mut Finder, searches: &mut i32, rng: &mut Rng) {
    let body = Body { tall: s.h.ceil() as i32, wide: s.hw * 2.0, drop };
    let mut get = |x, y, z| probe.get(x, y, z);
    // (standing on a slab counts as the cell above it)
    let cell = |p: Vec3| IVec3::new(p.x.floor() as i32, (p.y + 0.55).floor() as i32, p.z.floor() as i32);
    // (perched in the same cell as a door or window: start from beside it)
    let from = [(0.0, 0.0), (s.hw, 0.0), (-s.hw, 0.0), (0.0, s.hw), (0.0, -s.hw)]
        .into_iter()
        .find_map(|(ox, oz)| settle(&mut get, cell(m.pos + Vec3::new(ox, 0.0, oz)), body));
    match (from, settle(&mut get, cell(target), body)) {
        (Some(a), Some(b)) => {
            if m.distrust <= 0.0 && walk_line(&mut get, a, b, body) {
                m.direct = true;
                m.repath = rng.range(0.25, 0.4);
            } else if *searches > 0 {
                *searches -= 1;
                let reached = finder.find(&mut get, a, b, body, PATH_BUDGET, &mut m.path);
                m.path_i = 0;
                // a spider that finds no way round goes over instead
                m.direct = m.path.is_empty() || (!reached && m.kind == Kind::Spider);
                m.repath = rng.range(0.45, 0.75);
            }
        }
        // mid-air, or the target is: keep going as before
        _ => m.repath = 0.3,
    }
}

/// Turns a mob towards the next cell of its path. False when it has none left to follow
/// and heads straight for its target.
fn follow(m: &mut Mob, s: &Spec) -> bool {
    // wide bodies have to line up with a gap more exactly
    let near = (0.55 - s.hw).clamp(0.15, 0.3);
    while !m.direct && m.path_i < m.path.len() {
        let wp = m.path[m.path_i];
        let (dx, dz, dy) = (wp.x as f32 + 0.5 - m.pos.x, wp.z as f32 + 0.5 - m.pos.z, m.pos.y - wp.y as f32);
        if dx * dx + dz * dz < near * near && dy > -0.6 && dy < 1.3 {
            m.path_i += 1;
            continue;
        }
        m.heading = dx.atan2(dz);
        return true;
    }
    false
}

pub struct Mobs {
    pub list: Vec<Mob>,
    /// arrows loosed by skeletons this frame: (position, velocity)
    pub shots: Vec<(Vec3, Vec3)>,
    /// fireballs thrown by emberwisps this frame: (position, velocity)
    pub fireballs: Vec<(Vec3, Vec3)>,
    rng: Rng,
    spawn_timer: f32,
    finder: Finder,
    /// the player is left alone (a creative world)
    pub ignore: bool,
    /// villagers that were killed: (the square of their village, their home in it)
    gone: HashSet<((i32, i32), usize)>,
    /// each fortress's garrison: guards sent out so far, and seconds until it sends
    /// another once its first three are down (so a fortress can be cleared)
    guards: HashMap<(i32, i32), (u32, f32)>,
    /// what happened this frame (see `MobEvent`): the game takes them
    pub events: Vec<MobEvent>,
}

/// A wild sheep's coat: mostly white, now and then grey, black, brown or pink.
fn natural_wool(rng: &mut Rng) -> u8 {
    match rng.next() % 100 {
        0..=80 => 0,
        81..=85 => 7,
        86..=90 => 8,
        91..=95 => 15,
        96..=98 => 12,
        _ => 6,
    }
}

/// How a villager is dressed (and so what they trade in).
fn look(m: &Mob) -> u32 {
    hash2(m.village.as_ref().map_or(0, |v| v.region.0 as u32 ^ (v.region.1 as u32).rotate_left(16)), m.slot as i32, 7)
}

/// cells one route search may expand; with one search a frame this bounds the cost
const PATH_BUDGET: usize = 320;

const MAX_ANIMALS: usize = 12;
const MAX_WOLVES: usize = 4;
const MAX_MONSTERS: usize = 10;
/// monsters never appear this close to a torch or lamp
const SAFE_LIGHT: f32 = 9.0;

impl Mobs {
    pub fn new(seed: u64) -> Mobs {
        Mobs { list: Vec::new(), shots: Vec::new(), fireballs: Vec::new(), rng: Rng(seed | 1), spawn_timer: 0.0, finder: Finder::default(), ignore: false, gone: HashSet::new(), guards: HashMap::new(), events: Vec::new() }
    }

    /// The Emberdeep's creatures: hound packs on the floors, wisps in the air (more in
    /// the grove), and brutes in the fortresses. Light does not keep them away.
    fn spawn_deep(&mut self, world: &World, player: Vec3) {
        use crate::deep;
        let count = |me: &Mobs, k: Kind| me.list.iter().filter(|m| m.kind == k && m.dead < 0.0).count();
        // a fortress near the player keeps a few guards in its courtyard
        let (px, pz) = (player.x.floor() as i32, player.z.floor() as i32);
        for f in deep::forts_near(world.seed, px - 48, pz - 48, px + 48, pz + 48) {
            let guards = self.list.iter().filter(|m| m.kind == Kind::Brute && m.dead < 0.0 && f.inside(m.pos)).count();
            let (sent, wait) = self.guards.entry((f.x, f.z)).or_insert((0, 0.0));
            *wait -= 0.6;
            // three to begin with; after that one now and then
            let may = *sent < 3 || *wait <= 0.0;
            if guards < 3 && may && self.rng.f() < 0.3 {
                let p = f.yard() + Vec3::new(self.rng.range(-8.0, 8.0), 0.01, self.rng.range(-2.0, 2.0));
                if world.is_loaded(p.x.floor() as i32, p.z.floor() as i32) && (p - player).length() > 10.0 && !collides(world, p, 0.48, 2.5) {
                    let (sent, wait) = self.guards.get_mut(&(f.x, f.z)).unwrap();
                    *sent += 1;
                    *wait = 120.0;
                    self.add(Kind::Brute, p);
                }
            }
        }
        let monsters = self.list.iter().filter(|m| m.kind.is_monster() && m.kind != Kind::Brute && m.dead < 0.0).count();
        if monsters >= 9 || self.rng.f() > 0.35 {
            return;
        }
        let region = deep::region(world.seed, px, pz);
        let wisp = self.rng.f() < if region == deep::Region::Grove { 0.6 } else { 0.35 };
        let kind = if wisp && count(self, Kind::Wisp) < 4 { Kind::Wisp } else { Kind::Hound };
        for _ in 0..8 {
            let (dx, dz) = (self.rng.range(-26.0, 26.0), self.rng.range(-26.0, 26.0));
            if dx * dx + dz * dz < 14.0 * 14.0 {
                continue;
            }
            let (x, z) = ((player.x + dx).floor() as i32, (player.z + dz).floor() as i32);
            if !world.is_loaded(x, z) {
                continue;
            }
            let y0 = player.y.floor() as i32 + (self.rng.next() % 17) as i32 - 8;
            let floor = (0..10).map(|k| y0 - k).find(|&y| {
                y > deep::LAVA_SEA && world.get_block(x, y, z) == AIR && world.get_block(x, y + 1, z) == AIR && is_solid(world.get_block(x, y - 1, z))
            });
            let Some(y) = floor else { continue };
            let pos = Vec3::new(x as f32 + 0.5, y as f32 + 0.01, z as f32 + 0.5);
            if deep::forts_near(world.seed, x, z, x, z).iter().any(|f| f.covers(x, z)) {
                continue;
            }
            if kind == Kind::Hound {
                // hounds come in twos and threes
                let n = 2 + (self.rng.next() % 2) as usize;
                for k in 0..n {
                    let q = pos + Vec3::new((k as f32 - 1.0) * 0.9, 0.0, (k % 2) as f32 * 0.9);
                    if !collides(world, q, 0.3, 0.9) {
                        self.add(Kind::Hound, q);
                    }
                }
            } else {
                self.add(Kind::Wisp, pos + Vec3::Y * 2.0);
                if let Some(m) = self.list.last_mut() {
                    m.slot = (self.rng.next() % 2) as usize;
                }
            }
            break;
        }
    }

    /// Puts the people of the villages round the player into their houses: everyone who
    /// is not about already, and was not killed.
    fn populate(&mut self, world: &World, player: Vec3) {
        let (px, pz) = (player.x.floor() as i32, player.z.floor() as i32);
        for v in village::near_world(world.seed, px, pz, px, pz, 96 + village::REACH) {
            for (slot, &home) in v.homes.iter().enumerate() {
                let pos = home + Vec3::new(0.0, 0.01, 0.0);
                if (home - player).length_squared() > 96.0 * 96.0 || !world.is_loaded(home.x.floor() as i32, home.z.floor() as i32) {
                    continue;
                }
                let about = self.list.iter().any(|m| m.slot == slot && m.village.as_ref().is_some_and(|w| w.region == v.region));
                if about || self.gone.contains(&(v.region, slot)) || collides(world, pos, 0.3, 1.8) {
                    continue;
                }
                self.add(Kind::Villager, pos);
                if let Some(m) = self.list.last_mut() {
                    m.village = Some(v.clone());
                    m.slot = slot;
                }
            }
        }
    }

    fn try_spawn(&mut self, world: &World, player: Vec3, night: bool) {
        if world.is_deep() {
            self.spawn_deep(world, player);
            return;
        }
        if world.ver >= 3 {
            self.populate(world, player);
        }
        let ang = self.rng.range(0.0, std::f32::consts::TAU);
        let dist = self.rng.range(26.0, 72.0);
        let (bx, bz) = (player.x + ang.cos() * dist, player.z + ang.sin() * dist);
        let (mut animals, mut wolves, mut monsters) = (0, 0, 0);
        for m in self.list.iter().filter(|m| !m.keep) {
            match m.kind {
                Kind::Wolf => wolves += 1,
                Kind::Villager => {}
                k if k.is_monster() => monsters += 1,
                _ => animals += 1,
            }
        }
        // underground, where no daylight reaches, monsters turn up at any hour: more of
        // them, and sooner, the deeper the player is
        let (px, py, pz) = (player.x.floor() as i32, player.y.floor() as i32, player.z.floor() as i32);
        if world.surface_y(px, pz).is_some_and(|top| top - py > 6) && world.sky_level(px, py, pz) == 0 {
            let deep = ((SEA + 6 - py) as f32 / 36.0).clamp(0.0, 1.0);
            if monsters < 3 + (deep * 9.0) as usize && self.rng.f() < 0.25 + 0.6 * deep {
                let roll = self.rng.next() % 10;
                for _ in 0..8 {
                    let (dx, dz) = (self.rng.range(-22.0, 22.0), self.rng.range(-22.0, 22.0));
                    if dx * dx + dz * dz < 81.0 {
                        continue;
                    }
                    let (x, z) = ((player.x + dx).floor() as i32, (player.z + dz).floor() as i32);
                    // the floor of whatever space lies there, near the player's level
                    let y0 = py + (self.rng.next() % 13) as i32 - 6;
                    let floor = (0..6).map(|k| y0 - k).find(|&y| {
                        y > 1 && world.get_block(x, y, z) == AIR && world.get_block(x, y + 1, z) == AIR && is_solid(world.get_block(x, y - 1, z))
                    });
                    let Some(y) = floor else { continue };
                    let pos = Vec3::new(x as f32 + 0.5, y as f32 + 0.01, z as f32 + 0.5);
                    if world.sky_level(x, y, z) != 0 || !world.lights_near(pos, SAFE_LIGHT).is_empty() {
                        continue;
                    }
                    let kind = match roll {
                        0..=2 => Kind::Zombie,
                        3..=5 => Kind::Skeleton,
                        6..=7 => Kind::Spider,
                        8 => Kind::Slime,
                        _ => Kind::Puffcap,
                    };
                    self.add(kind, pos);
                    break;
                }
            }
            return;
        }
        let roll = self.rng.next() % 6;
        let kind = if night && monsters < MAX_MONSTERS && roll < 3 {
            match self.rng.next() % 10 {
                0..=2 => Kind::Zombie,
                3..=5 => Kind::Skeleton,
                6..=7 => Kind::Spider,
                8 => Kind::Slime,
                _ => Kind::Puffcap,
            }
        } else if night && wolves < MAX_WOLVES && roll == 3 {
            Kind::Wolf
        } else if animals < MAX_ANIMALS && roll >= 4 {
            // herds refill slowly, so hunted land stays quiet for a while
            match self.rng.next() % 10 {
                0..=2 => Kind::Deer,
                3..=4 => Kind::Boar,
                5..=7 => Kind::Sheep,
                _ => Kind::Hare,
            }
        } else {
            return;
        };
        let group = match kind {
            Kind::Zombie | Kind::Wolf | Kind::Deer | Kind::Hare => 1 + self.rng.next() % 2,
            Kind::Sheep => 2 + self.rng.next() % 2,
            _ => 1,
        };
        for _ in 0..group {
            let x = (bx + self.rng.range(-4.0, 4.0)).floor() as i32;
            let z = (bz + self.rng.range(-4.0, 4.0)).floor() as i32;
            let Some(y) = world.surface_y(x, z) else { continue };
            let top = world.get_block(x, y, z);
            let pos = Vec3::new(x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5);
            if kind.is_monster() {
                // any firm ground will do, as long as no light keeps them away
                if !is_solid(top) || is_leaves(top) || !world.lights_near(pos, SAFE_LIGHT).is_empty() {
                    continue;
                }
            } else if !is_grassy(top) && !(top == SNOW && kind == Kind::Hare) {
                continue;
            }
            let s = spec(kind);
            if collides(world, pos, s.hw, s.h) {
                continue;
            }
            self.add(kind, pos);
        }
    }

    /// Puts one of every monster in a row in front of `p` (CUBEWILD_TEST_MOBS).
    pub fn spawn_test(&mut self, world: &World, p: Vec3) {
        for (i, kind) in [Kind::Zombie, Kind::Skeleton, Kind::Spider, Kind::Slime, Kind::Puffcap].into_iter().enumerate() {
            let (x, z) = ((p.x + i as f32 * 2.2 - 4.4).floor() as i32, (p.z + 14.0).floor() as i32);
            if let Some(y) = world.surface_y(x, z) {
                self.add(kind, Vec3::new(x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5));
            }
        }
        // and the farm: a white sheep, a red one, a shorn one, a lamb, a sheep in love,
        // and a sitting dog
        for i in 0..6 {
            let (x, z) = ((p.x + i as f32 * 1.9 - 4.75).floor() as i32, (p.z + 9.0).floor() as i32);
            let Some(y) = world.surface_y(x, z) else { continue };
            let kind = if i == 5 { Kind::Wolf } else { Kind::Sheep };
            self.add(kind, Vec3::new(x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5));
            let m = self.list.last_mut().unwrap();
            m.yaw = std::f32::consts::PI;
            m.heading = m.yaw;
            m.timer = 1000.0;
            m.keep = true;
            match i {
                0 => m.wool = 0,
                1 => m.wool = 14,
                2 => m.sheared = true,
                3 => m.baby = 0.0,
                4 => (m.love, m.wool) = (1000.0, 11),
                _ => (m.tame, m.sitting) = (true, true),
            }
        }
    }

    fn add(&mut self, kind: Kind, pos: Vec3) {
        let s = spec(kind);
        {
            let heading = self.rng.range(0.0, std::f32::consts::TAU);
            self.list.push(Mob {
                kind,
                pos,
                vel: Vec3::ZERO,
                yaw: heading,
                heading,
                moving: false,
                timer: self.rng.range(0.5, 3.0),
                flee: 0.0,
                hurt: 0.0,
                hp: s.hp,
                phase: 0.0,
                dead: -1.0,
                on_ground: false,
                stuck: 0,
                stall: 0.0,
                grazing: false,
                graze: 0.0,
                hspeed: 0.0,
                age: 0.0,
                bite: 1.0,
                hop: 0.0,
                burn: self.rng.range(0.5, 1.5),
                path: Vec::new(),
                path_i: 0,
                repath: self.rng.range(0.0, 0.5),
                direct: true,
                distrust: 0.0,
                los: true,
                village: None,
                slot: 0,
                goal: None,
                wool: if kind == Kind::Sheep { natural_wool(&mut self.rng) } else { 0 },
                sheared: false,
                regrow: 0.0,
                love: 0.0,
                breed_cd: 0.0,
                baby: 1.0,
                tame: false,
                sitting: false,
                keep: false,
                fuse: -1.0,
            });
        }
    }

    /// Puts a creature into the world and returns its index (for babies and loading).
    pub fn spawn(&mut self, kind: Kind, pos: Vec3) -> usize {
        self.add(kind, pos);
        self.list.len() - 1
    }

    /// Keeps a villager standing where they are for a moment (while trading).
    pub fn hold(&mut self, i: usize) {
        let m = &mut self.list[i];
        m.goal = None;
        m.timer = m.timer.max(2.0);
    }

    /// The villager's home: its village's square and its house in it.
    pub fn villager_id(&self, i: usize) -> Option<((i32, i32), usize)> {
        let m = self.list.get(i)?;
        (m.kind == Kind::Villager).then(|| m.village.as_ref().map(|v| (v.region, m.slot))).flatten()
    }

    /// A villager's trade, read from how they are dressed (0..6, see `trade`).
    pub fn profession(&self, i: usize) -> u32 {
        let m = &self.list[i];
        look(m) % 6
    }

    /// Uses `item` on creature `i`: feeding, shearing, dyeing, taming, telling a dog to sit.
    pub fn interact(&mut self, i: usize, item: Item) -> Use {
        let rng = &mut self.rng;
        let m = &mut self.list[i];
        if m.dead >= 0.0 {
            return Use::Nothing;
        }
        if m.kind == Kind::Villager {
            return Use::Trade;
        }
        if m.kind == Kind::Sheep {
            if item == SHEARS && !m.sheared && m.baby >= 1.0 {
                m.sheared = true;
                m.regrow = rng.range(40.0, 90.0);
                return Use::Sheared { item: wool_item(m.wool), n: 1 + rng.next() % 3 };
            }
            if let Some(c) = items::dye_colour(item) {
                if c != m.wool {
                    m.wool = c;
                    return Use::Dyed;
                }
                return Use::Nothing;
            }
        }
        if m.kind == Kind::Wolf && !m.tame {
            if item == BONE {
                if rng.next() % 3 == 0 {
                    m.tame = true;
                    m.keep = true;
                    m.flee = 0.0;
                    m.sitting = false;
                    return Use::Tamed;
                }
                return Use::NotYet;
            }
            return Use::Nothing;
        }
        if m.kind.food().contains(&item) {
            let max = spec(m.kind).hp;
            if m.tame && m.hp < max {
                m.hp = (m.hp + 4).min(max);
                return Use::Healed;
            }
            if m.baby < 1.0 {
                m.baby = (m.baby + 0.1).min(1.0);
                return Use::Fed;
            }
            if m.breed_cd <= 0.0 && m.love <= 0.0 {
                m.love = 25.0;
                m.keep = true;
                return Use::Love;
            }
            return Use::Nothing;
        }
        if m.tame {
            m.sitting = !m.sitting;
            return Use::Sit(m.sitting);
        }
        Use::Nothing
    }

    /// The creatures worth keeping, one per line: kind, place, facing and their state.
    pub fn save(&self, path: &std::path::Path) {
        let mut out = String::new();
        for m in self.list.iter().filter(|m| m.keep && m.dead < 0.0) {
            let k = KINDS.iter().position(|&k| k == m.kind).unwrap_or(0);
            out += &format!("{} {} {} {} {} {} {} {} {} {} {}\n", k, m.pos.x, m.pos.y, m.pos.z, m.yaw, m.wool, m.sheared as u8, m.tame as u8, m.sitting as u8, m.baby, m.hp);
        }
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("cubewild: failed to save animals: {e}");
        }
    }

    pub fn load(&mut self, path: &std::path::Path) {
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let v: Vec<f32> = line.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if v.len() < 11 || !v.iter().all(|x| x.is_finite()) {
                continue;
            }
            let Some(&kind) = KINDS.get(v[0] as usize) else { continue };
            let i = self.spawn(kind, Vec3::new(v[1], v[2], v[3]));
            let m = &mut self.list[i];
            m.yaw = v[4];
            m.heading = v[4];
            m.wool = (v[5] as u8).min(15);
            m.sheared = v[6] > 0.5;
            m.tame = v[7] > 0.5;
            m.sitting = v[8] > 0.5;
            m.baby = v[9].clamp(0.0, 1.0);
            m.hp = (v[10] as i32).clamp(1, spec(kind).hp);
            m.keep = true;
        }
    }

    /// Advances every animal and monster. Returns the damage they dealt to the player
    /// up close; arrows shot by skeletons are left in `shots`.
    /// `lure` is the item in the player's hand: animals follow their food.
    pub fn update(&mut self, world: &World, player: Vec3, sneaking: bool, night: bool, lure: Item, dt: f32) -> f32 {
        let mut damage = 0.0;
        // (in the Emberdeep it is always night: its creatures never slink off)
        let night = night || world.is_deep();
        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn_timer = 0.6;
            self.try_spawn(world, player, night);
        }
        // wolves and monsters slink away at dawn once they are out of sight
        // (kept animals and pets stay)
        self.list.retain(|m| {
            m.keep || night || !(m.kind == Kind::Wolf || m.kind.is_monster()) || (m.pos - player).length_squared() < 34.0 * 34.0
        });
        // animals whose chunk got unloaded are removed rather than left frozen in place
        // (kept ones wait there, frozen, for the player to come back)
        self.list.retain(|m| {
            m.dead < 1.3
                && m.pos.y > -10.0
                && (m.keep || ((m.pos - player).length_squared() < 120.0 * 120.0 && world.is_loaded(m.pos.x.floor() as i32, m.pos.z.floor() as i32)))
        });
        // before anyone moves: who each animal in love is heading for, and which
        // monster each dog is after
        let n = self.list.len();
        let mut mate: Vec<Option<usize>> = vec![None; n];
        let mut prey: Vec<Option<usize>> = vec![None; n];
        for i in 0..n {
            let a = &self.list[i];
            if a.dead >= 0.0 {
                continue;
            }
            if a.love > 0.0 && a.baby >= 1.0 {
                mate[i] = (0..n)
                    .filter(|&j| j != i)
                    .filter(|&j| {
                        let b = &self.list[j];
                        b.kind == a.kind && b.love > 0.0 && b.baby >= 1.0 && b.dead < 0.0 && (b.pos - a.pos).length_squared() < 12.0 * 12.0
                    })
                    .min_by(|&x, &y| (self.list[x].pos - a.pos).length_squared().total_cmp(&(self.list[y].pos - a.pos).length_squared()));
            }
            if a.tame && !a.sitting {
                prey[i] = (0..n)
                    .filter(|&j| {
                        let b = &self.list[j];
                        b.kind.is_monster() && b.dead < 0.0 && (b.pos - a.pos).length_squared() < 12.0 * 12.0 && (b.pos - player).length_squared() < 20.0 * 20.0
                    })
                    .min_by(|&x, &y| (self.list[x].pos - a.pos).length_squared().total_cmp(&(self.list[y].pos - a.pos).length_squared()));
            }
        }
        let targets: Vec<Option<Vec3>> = (0..n).map(|i| mate[i].or(prey[i]).map(|j| self.list[j].pos)).collect();
        let mut births: Vec<(usize, usize)> = Vec::new();
        let mut bites: Vec<(usize, Vec3)> = Vec::new();
        let events = &mut self.events;
        let ignore = self.ignore;
        let rng = &mut self.rng;
        let shots = &mut self.shots;
        let fireballs = &mut self.fireballs;
        let finder = &mut self.finder;
        let mut probe = Probe::new(world);
        // route searches allowed this frame; the others wait their turn
        let mut searches = 1;
        for (idx, m) in self.list.iter_mut().enumerate() {
            let s = spec(m.kind);
            let to_player = player - m.pos;
            let dist = to_player.length();
            // a dog left far behind catches up
            if m.tame && !m.sitting && m.dead < 0.0 && dist > 28.0 {
                let back = Vec3::new(-to_player.x, 0.0, -to_player.z).normalize_or(Vec3::X) * 2.0;
                if !collides(world, player + back, s.hw, s.h) {
                    m.pos = player + back;
                    m.vel = Vec3::ZERO;
                }
            }
            if m.keep && !world.is_loaded(m.pos.x.floor() as i32, m.pos.z.floor() as i32) {
                continue;
            }
            let to_player = player - m.pos;
            let dist = to_player.length();
            m.age += dt;
            m.hurt = (m.hurt - dt).max(0.0);
            m.love = (m.love - dt).max(0.0);
            m.breed_cd -= dt;
            if m.baby < 1.0 {
                m.baby = (m.baby + dt / GROW).min(1.0);
            }
            if m.sheared {
                // grazing grows the wool back
                m.regrow -= dt;
                if m.regrow <= 0.0 && m.grazing {
                    m.sheared = false;
                }
            }
            let mut speed = 0.0;
            // direction to look in when it differs from the direction of travel
            let mut face: Option<f32> = None;
            let mut hunting = false;
            if m.dead < 0.0 && m.kind.undead() && !night {
                m.burn -= dt;
                if m.burn <= 0.0 {
                    m.burn = 1.0;
                    let (x, z) = (m.pos.x.floor() as i32, m.pos.z.floor() as i32);
                    if world.surface_y(x, z).map_or(true, |y| (y as f32) < m.pos.y) {
                        m.hp -= 2;
                        m.hurt = 0.3;
                        if m.hp <= 0 {
                            m.dead = 0.0;
                        }
                    }
                }
            }
            if m.dead >= 0.0 {
                m.dead += dt;
            } else {
                let wary = if sneaking { s.wary * 0.45 } else { s.wary };
                if dist < wary {
                    m.flee = m.flee.max(2.5);
                }
                m.bite -= dt;
                m.hop -= dt;
                // monsters are easier to slip past when sneaking; a wolf's nose is not fooled
                let sight = if ignore {
                    0.0
                } else if sneaking && m.kind.is_monster() {
                    s.sight * 0.6
                } else {
                    s.sight
                };
                hunting = m.flee <= 0.0 && dist < sight && !m.tame;
                let lured = !m.kind.is_monster() && !(m.kind == Kind::Wolf && !m.tame) && m.kind.food().contains(&lure) && dist < 10.0 && m.flee <= 0.0;
                if m.tame && m.sitting {
                    // sits where it was told, watching the player
                    face = Some(to_player.x.atan2(to_player.z));
                } else if let (Some(t), true) = (targets[idx], m.love > 0.0 || m.tame) {
                    // off to its mate, or after a monster
                    let d = t - m.pos;
                    let flat = Vec3::new(d.x, 0.0, d.z).length();
                    m.heading = d.x.atan2(d.z);
                    if m.love > 0.0 {
                        speed = if flat > 1.1 { s.walk } else { 0.0 };
                        if flat < 1.3 {
                            births.push((idx, mate[idx].unwrap()));
                        }
                    } else {
                        speed = if flat > 1.0 { s.run } else { 0.0 };
                        if flat < 1.6 && d.y.abs() < 1.8 && m.bite <= 0.0 {
                            m.bite = 0.9;
                            bites.push((prey[idx].unwrap(), m.pos));
                        }
                    }
                } else if m.tame {
                    // a dog keeps close to the player
                    m.heading = to_player.x.atan2(to_player.z);
                    if dist > 3.5 {
                        speed = if dist > 9.0 { s.run } else { s.walk * 1.6 };
                        m.repath -= dt;
                        m.distrust -= dt;
                        if m.repath <= 0.0 {
                            plan(m, &s, player, 3, &mut probe, finder, &mut searches, rng);
                        }
                        follow(m, &s);
                    } else {
                        face = Some(m.heading);
                    }
                } else if lured && m.love <= 0.0 {
                    // follows the food in the player's hand, and waits in front of them
                    m.grazing = false;
                    m.heading = to_player.x.atan2(to_player.z);
                    if dist > 2.4 {
                        speed = s.walk * 1.3;
                    } else {
                        face = Some(m.heading);
                    }
                } else if m.kind == Kind::Villager && m.flee <= 0.0 {
                    // home for the night; by day, now standing about, now off to another
                    // part of the village
                    let home = m.village.as_ref().map_or(m.pos, |v| v.homes[m.slot]);
                    m.timer -= dt;
                    if night {
                        if m.goal != Some(home) {
                            m.goal = Some(home);
                            m.repath = 0.0;
                        }
                    } else if m.timer <= 0.0 {
                        m.goal = match &m.village {
                            Some(v) if rng.f() < 0.7 => Some(v.spots[rng.next() as usize % v.spots.len()]),
                            _ => None,
                        };
                        m.timer = if m.goal.is_some() { rng.range(10.0, 24.0) } else { rng.range(2.0, 7.0) };
                        m.repath = 0.0;
                    }
                    if let Some(g) = m.goal {
                        let (dx, dz) = (g.x - m.pos.x, g.z - m.pos.z);
                        if dx * dx + dz * dz < 0.5 {
                            if !night {
                                m.goal = None;
                                m.timer = rng.range(2.0, 7.0);
                            }
                        } else {
                            m.repath -= dt;
                            m.distrust -= dt;
                            if m.repath <= 0.0 {
                                // (aimed a little high: the ground may have been built up)
                                plan(m, &s, g + Vec3::new(0.0, 1.2, 0.0), 2, &mut probe, finder, &mut searches, rng);
                            }
                            m.heading = dx.atan2(dz);
                            follow(m, &s);
                            speed = s.walk;
                        }
                    }
                    // turns to whoever comes up to it
                    if speed == 0.0 && dist < 5.0 {
                        face = Some(to_player.x.atan2(to_player.z));
                    }
                } else if hunting {
                    let toward = to_player.x.atan2(to_player.z);
                    m.grazing = false;
                    let flat = Vec3::new(to_player.x, 0.0, to_player.z).length();
                    // plan the way there: straight if that works, otherwise round the obstacles
                    m.repath -= dt;
                    m.distrust -= dt;
                    if m.repath <= 0.0 {
                        if m.kind == Kind::Wisp {
                            // it flies: no route on foot, only whether it can see you
                            m.repath = rng.range(0.3, 0.5);
                            m.direct = true;
                            m.path.clear();
                        } else {
                            plan(m, &s, player, 4, &mut probe, finder, &mut searches, rng);
                        }
                        if matches!(m.kind, Kind::Skeleton | Kind::Wisp) {
                            m.los = clear_path(world, m.pos + Vec3::new(0.0, 1.35, 0.0), player + Vec3::new(0.0, 1.2, 0.0));
                        }
                    }
                    // head for the next cell of the path, or straight at the player
                    m.heading = toward;
                    let pathing = follow(m, &s);
                    // stop at arm's length, but keep coming while the player is on another
                    // level: down in a hole, or where the path still has a way to go
                    let chase = flat > 1.0 || (to_player.y.abs() > 1.5 && (pathing || to_player.y < 0.0));
                    match m.kind {
                        Kind::Skeleton => {
                            // keeps its distance and shoots; comes round when it cannot see you
                            face = Some(toward);
                            if flat > 13.0 || (!m.los && chase) {
                                speed = s.run;
                            } else if flat < 6.0 {
                                m.heading = toward + std::f32::consts::PI;
                                speed = s.walk;
                            }
                            if m.bite <= 0.0 {
                                m.bite = rng.range(2.0, 3.0);
                                let from = m.pos + Vec3::new(toward.sin() * 0.45, 1.35, toward.cos() * 0.45);
                                let aim = player + Vec3::new(0.0, 1.2, 0.0);
                                if clear_path(world, from, aim) {
                                    // lob it so gravity brings it down on the target
                                    let t = ((aim - from).length() / 24.0).max(0.05);
                                    let jitter = Vec3::new(rng.range(-0.9, 0.9), rng.range(-0.6, 0.6), rng.range(-0.9, 0.9));
                                    shots.push((from, (aim - from) / t + Vec3::Y * (10.0 * t) + jitter));
                                }
                            }
                        }
                        Kind::Puffcap => {
                            // creeps close, then its fuse burns; it gives up if you get away
                            let close = flat < 2.6 && to_player.y.abs() < 2.0;
                            if m.fuse >= 0.0 {
                                if dist > 5.5 {
                                    m.fuse = -1.0;
                                } else {
                                    m.fuse += dt;
                                    face = Some(toward);
                                    if m.fuse >= FUSE {
                                        events.push(MobEvent::Explode { pos: m.pos + Vec3::Y * 0.6 });
                                        m.dead = 1.3; // gone in the blast, leaving nothing
                                        m.hp = 0;
                                    }
                                }
                            } else if close {
                                m.fuse = 0.0;
                                events.push(MobEvent::Fuse { pos: m.pos });
                            } else if chase {
                                speed = s.run;
                            }
                        }
                        Kind::Wisp => {
                            // drifts at a distance and throws fire; closes in when it
                            // cannot see you
                            face = Some(toward);
                            if flat > 12.0 || (!m.los && chase) {
                                speed = s.run;
                            } else if flat < 5.0 {
                                m.heading = toward + std::f32::consts::PI;
                                speed = s.walk;
                            } else {
                                // circles slowly round
                                m.heading = toward + std::f32::consts::FRAC_PI_2 * if m.slot % 2 == 0 { 1.0 } else { -1.0 };
                                speed = s.walk * 0.6;
                            }
                            if m.bite <= 0.0 {
                                m.bite = rng.range(2.6, 3.8);
                                let from = m.pos + Vec3::new(toward.sin() * 0.4, 0.45, toward.cos() * 0.4);
                                let aim = player + Vec3::new(0.0, 1.0, 0.0);
                                if clear_path(world, from, aim) {
                                    let jitter = Vec3::new(rng.range(-0.6, 0.6), rng.range(-0.3, 0.3), rng.range(-0.6, 0.6));
                                    fireballs.push((from, (aim - from).normalize_or(Vec3::Z) * 15.0 + jitter));
                                }
                            }
                        }
                        Kind::Hound => {
                            if chase {
                                speed = s.run;
                            }
                            // leaps the last few blocks
                            if m.on_ground && m.hop <= 0.0 && flat > 2.0 && flat < 5.0 {
                                m.hop = rng.range(1.8, 2.6);
                                m.vel.y = 6.0;
                                m.vel.x += toward.sin() * 6.5;
                                m.vel.z += toward.cos() * 6.5;
                            }
                        }
                        Kind::Spider => {
                            if chase {
                                speed = s.run;
                            }
                            // pounces from a few blocks away
                            if m.on_ground && m.hop <= 0.0 && flat > 2.5 && flat < 6.0 {
                                m.hop = 2.5;
                                m.vel.y = 7.2;
                                m.vel.x += toward.sin() * 5.0;
                                m.vel.z += toward.cos() * 5.0;
                            }
                        }
                        _ => {
                            if chase {
                                speed = s.run;
                            }
                        }
                    }
                    // (a brute's club reaches further, and higher)
                    let (reach, up) = if m.kind == Kind::Brute { (2.3, 2.6) } else { (1.5, 1.9) };
                    if s.dmg > 0.0 && flat < reach && to_player.y.abs() < up && m.bite <= 0.0 {
                        m.bite = if m.kind == Kind::Brute { 1.8 } else { 1.2 };
                        damage += s.dmg;
                    }
                } else if m.flee > 0.0 {
                    m.flee -= dt;
                    m.timer -= dt;
                    if m.timer <= 0.0 {
                        m.timer = rng.range(0.3, 0.8);
                        m.heading = (-to_player.x).atan2(-to_player.z) + rng.range(-0.6, 0.6);
                    }
                    speed = s.run;
                } else {
                    m.timer -= dt;
                    if m.timer <= 0.0 {
                        // never stand idle twice in a row, and keep pauses short
                        m.moving = !m.moving || rng.f() < 0.45;
                        m.heading = rng.range(0.0, std::f32::consts::TAU);
                        if m.moving {
                            m.timer = rng.range(2.0, 6.0);
                            m.grazing = false;
                        } else {
                            m.timer = rng.range(1.2, 3.2);
                            m.grazing = matches!(m.kind, Kind::Deer | Kind::Boar) && rng.f() < 0.65;
                        }
                    }
                    if m.moving {
                        speed = s.walk;
                    }
                }
            }
            // slimes only travel in hops: they gather on the ground, then spring
            let mut going = speed > 0.0;
            if m.kind == Kind::Slime && m.dead < 0.0 {
                if m.on_ground {
                    if going && m.hop <= 0.0 {
                        m.hop = rng.range(0.7, 1.1);
                        m.vel.y = 8.4;
                    } else {
                        speed = 0.0;
                    }
                }
                going = false; // its pauses are not a sign of being stuck
            }
            let target = Vec3::new(m.heading.sin(), 0.0, m.heading.cos()) * speed;
            let k = 1.0 - (-dt * 8.0).exp();
            m.vel.x += (target.x - m.vel.x) * k;
            m.vel.z += (target.z - m.vel.z) * k;

            let b = m.pos.floor().as_ivec3();
            let around = world.get_block(b.x, (m.pos.y + s.h * 0.5).floor() as i32, b.z);
            let in_water = is_fluid(around);
            if is_lava(around) && m.dead < 0.0 && !m.kind.fireproof() {
                // lava burns whatever blunders into it
                m.burn -= dt;
                if m.burn <= 0.0 {
                    m.burn = 0.5;
                    m.hp -= 2;
                    m.hurt = 0.3;
                    if m.hp <= 0 {
                        m.dead = 0.0;
                    }
                }
            }
            if in_water && m.kind != Kind::Wisp {
                // carried along by running water
                let flow = crate::fluid::current(world, IVec3::new(b.x, (m.pos.y + s.h * 0.5).floor() as i32, b.z));
                m.vel.x += flow.x * 14.0 * dt;
                m.vel.z += flow.z * 14.0 * dt;
            }
            if m.kind == Kind::Wisp && m.dead < 0.0 {
                // it flies: hovers a little above the player's head, or above the floor
                // when idle, bobbing
                let floor = (0..10).map(|k| m.pos.y.floor() as i32 - k).find(|&y| is_solid(world.get_block(b.x, y, b.z))).map_or(m.pos.y - 3.0, |y| y as f32 + 1.0);
                let want = if hunting { player.y + 2.4 } else { floor + 2.6 } + (m.age * 1.3).sin() * 0.4;
                m.vel.y += ((want - m.pos.y).clamp(-2.0, 2.0) * 1.6 - m.vel.y) * (1.0 - (-dt * 3.0).exp());
            } else if in_water {
                m.vel.y = (m.vel.y + 14.0 * dt).min(1.6);
            } else {
                m.vel.y = (m.vel.y - 29.0 * dt).max(-50.0);
            }
            // on a ladder a mob slides down slowly, and climbs when it walks into the wall
            let ladder = !in_water && is_ladder(world.get_block(b.x, (m.pos.y + 0.1).floor() as i32, b.z));
            if ladder {
                m.vel.y = m.vel.y.max(-2.6);
            }
            let (ground, wall) = move_step(world, &mut m.pos, &mut m.vel, s.hw, s.h, dt, m.on_ground);
            m.on_ground = ground;
            if wall && speed > 0.0 {
                if ladder {
                    m.vel.y = 2.6;
                } else if m.kind == Kind::Spider && (m.direct || !hunting) {
                    m.vel.y = 4.0; // spiders climb straight up walls (unless a path leads round)
                } else if m.kind == Kind::Wisp {
                    m.vel.y = 3.0; // floats up over it
                } else if ground || in_water {
                    m.vel.y = 8.2; // hop up one block
                    m.stuck = m.stuck.saturating_add(1);
                }
                if m.stuck > 3 {
                    m.stuck = 0;
                    m.heading += rng.range(1.2, 2.6);
                }
            } else if ground {
                m.stuck = 0;
            }
            let hs = Vec3::new(m.vel.x, 0.0, m.vel.z).length();
            m.hspeed = hs;
            // trying to walk but going nowhere (wall, pit, tree): turn around
            if going && hs < speed * 0.3 {
                m.stall += dt;
                if m.stall > 0.7 {
                    m.stall = 0.0;
                    m.heading += rng.range(1.6, 4.6);
                    if hunting || m.goal.is_some() {
                        // the straight line snagged on something: search for a way round
                        m.distrust = 4.0;
                        m.repath = 0.0;
                    }
                }
            } else {
                m.stall = 0.0;
            }
            // face the direction of travel; idle animals slowly look around
            if m.dead < 0.0 && !(speed == 0.0 && m.grazing) {
                let mut d = face.unwrap_or(m.heading) - m.yaw;
                d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let rate = if speed > 0.0 || face.is_some() { 9.0 } else { 1.6 };
                m.yaw += d * (1.0 - (-dt * rate).exp());
            }
            let graze_target = if speed == 0.0 && m.grazing && m.dead < 0.0 { 1.0 } else { 0.0 };
            m.graze += (graze_target - m.graze) * (1.0 - (-dt * 5.0).exp());
            m.phase += hs * dt * 4.2;
        }
        // pairs that met make a baby between them
        for (a, b) in births {
            if self.list[a].love <= 0.0 || self.list[b].love <= 0.0 {
                continue;
            }
            let (kind, pos) = (self.list[a].kind, (self.list[a].pos + self.list[b].pos) * 0.5);
            let (wool, tame) = (if self.rng.f() < 0.5 { self.list[a].wool } else { self.list[b].wool }, self.list[a].tame);
            for k in [a, b] {
                let m = &mut self.list[k];
                m.love = 0.0;
                m.breed_cd = 240.0;
                m.keep = true;
            }
            let c = self.spawn(kind, pos);
            let m = &mut self.list[c];
            m.baby = 0.0;
            m.keep = true;
            m.wool = wool;
            m.tame = tame;
            self.events.push(MobEvent::Born { kind, pos });
        }
        // dogs' bites
        for (t, from) in bites {
            if t < self.list.len() && self.list[t].dead < 0.0 {
                let (kind, pos) = (self.list[t].kind, self.list[t].pos);
                if let Some(drops) = self.hit(t, from, 3) {
                    self.events.push(MobEvent::Killed { kind, pos, drops });
                }
            }
        }
        damage
    }

    /// Nearest living animal hit by a ray, within `max`.
    pub fn pick(&self, o: Vec3, d: Vec3, max: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in self.list.iter().enumerate() {
            if m.dead >= 0.0 {
                continue;
            }
            let s = spec(m.kind);
            let pad = 0.12;
            let min = m.pos - Vec3::new(s.hw + pad, 0.0, s.hw + pad);
            let max_c = m.pos + Vec3::new(s.hw + pad, s.h + pad, s.hw + pad);
            if let Some(t) = ray_aabb(o, d, min, max_c) {
                if t <= max && best.map_or(true, |b| t < b.1) {
                    best = Some((i, t));
                }
            }
        }
        best
    }

    /// Damages an animal or monster. Returns what it drops if it died.
    pub fn hit(&mut self, i: usize, from: Vec3, damage: i32) -> Option<Drops> {
        let m = &mut self.list[i];
        if let (Some(v), true) = (&m.village, m.hp > 0 && m.hp <= damage) {
            self.gone.insert((v.region, m.slot));
        }
        m.goal = None;
        m.hp -= damage;
        m.hurt = 0.3;
        // prey bolts, a wolf backs off for a moment, monsters just keep coming
        m.flee = match m.kind {
            Kind::Wolf if m.tame => 0.0,
            Kind::Wolf => 0.7,
            k if k.is_monster() => 0.0,
            _ => 6.0,
        };
        m.timer = 0.0;
        let mut away = m.pos - from;
        away.y = 0.0;
        let away = away.normalize_or_zero();
        m.vel += away * 6.0 + Vec3::new(0.0, 5.0, 0.0);
        m.fuse = -1.0;
        m.sitting = false;
        if m.hp <= 0 {
            m.dead = 0.0;
            return Some(drops(m, &mut self.rng));
        }
        None
    }

    /// Not dead (or dying).
    pub fn alive(&self, i: usize) -> bool {
        self.list[i].dead < 0.0
    }

    /// True if the box overlaps an animal or monster.
    pub fn blocks_box(&self, min: Vec3, max: Vec3) -> bool {
        self.list.iter().any(|m| {
            let s = spec(m.kind);
            m.pos.x + s.hw > min.x
                && m.pos.x - s.hw < max.x
                && m.pos.z + s.hw > min.z
                && m.pos.z - s.hw < max.z
                && m.pos.y + s.h > min.y
                && m.pos.y < max.y
        })
    }

    pub fn draw(&self, out: &mut Vec<EntVert>, cam: Vec3, max_dist: f32) {
        for m in &self.list {
            let rel = m.pos - cam;
            if rel.length_squared() > max_dist * max_dist {
                continue;
            }
            let alive = m.dead < 0.0;
            let moving = (m.hspeed / 1.2).min(1.0);
            // hares bound along; everything breathes a little so nothing looks frozen
            let hop = if m.kind == Kind::Hare && alive { (m.phase * 1.4).sin().abs() * 0.16 * moving } else { 0.0 };
            let breathe = if alive { (m.age * 2.2).sin() * 0.012 } else { 0.0 };
            // babies are small; a sitting dog sinks onto its haunches
            let grown = 0.5 + 0.5 * m.baby;
            let sit = if m.sitting { -0.14 } else { 0.0 };
            let mut base = Mat4::from_translation(rel + Vec3::new(0.0, hop + sit, 0.0)) * Mat4::from_rotation_y(m.yaw) * Mat4::from_scale(Vec3::splat(grown));
            if m.love > 0.0 && alive {
                // a pair of little hearts over its head
                let h = spec(m.kind).h * grown + 0.3 + (m.age * 2.0).sin() * 0.06;
                let hm = Mat4::from_translation(rel + Vec3::Y * h) * Mat4::from_rotation_y(m.age * 1.5);
                push_box(out, &hm, Vec3::new(-0.09, 0.0, -0.03), Vec3::new(-0.01, 0.09, 0.03), [230, 40, 70], EMISSIVE);
                push_box(out, &hm, Vec3::new(0.01, 0.0, -0.03), Vec3::new(0.09, 0.09, 0.03), [230, 40, 70], EMISSIVE);
                push_box(out, &hm, Vec3::new(-0.06, -0.07, -0.03), Vec3::new(0.06, 0.02, 0.03), [230, 40, 70], EMISSIVE);
            }
            if m.dead >= 0.0 {
                let a = (m.dead / 0.35).min(1.0) * std::f32::consts::FRAC_PI_2;
                base = base * Mat4::from_rotation_z(a);
            }
            let red = m.hurt > 0.0 || m.dead >= 0.0;
            let tint = |c: [u8; 3]| -> [u8; 3] {
                if red {
                    [c[0].saturating_add(90), c[1] / 2, c[2] / 2]
                } else {
                    c
                }
            };
            let swing = m.phase.sin() * if alive { 0.7 * moving } else { 0.0 };
            if m.kind.is_monster() || m.kind == Kind::Villager {
                draw_monster(out, m, &base, swing, &tint);
                continue;
            }
            // head/neck group pivots down to graze, and bobs gently while idle
            let head_ang = m.graze * 1.35 + if alive { (m.age * 1.3).sin() * 0.05 * (1.0 - moving) } else { 0.0 };
            let head_pivot = match m.kind {
                Kind::Deer => Vec3::new(0.0, 1.12, 0.48),
                Kind::Boar => Vec3::new(0.0, 0.62, 0.46),
                Kind::Hare => Vec3::new(0.0, 0.32, 0.14),
                Kind::Wolf => Vec3::new(0.0, 0.66, 0.4),
                Kind::Sheep => Vec3::new(0.0, 0.84, 0.44),
                _ => Vec3::ZERO,
            };
            let head = base * Mat4::from_translation(head_pivot) * Mat4::from_rotation_x(head_ang) * Mat4::from_translation(-head_pivot);
            let body = base * Mat4::from_translation(Vec3::new(0.0, breathe, 0.0));
            let mut part = |m: &Mat4, min: [f32; 3], max: [f32; 3], c: [u8; 3]| {
                push_box(out, m, min.into(), max.into(), tint(c), 255);
            };
            // (x half-offset, z offset, length, width, colour) for four legs
            let legs = |out: &mut Vec<EntVert>, lx: f32, lz: f32, len: f32, w: f32, c: [u8; 3]| {
                for (i, (sx, sz)) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
                    .into_iter()
                    .enumerate()
                {
                    let ang = if i == 0 || i == 3 { swing } else { -swing };
                    let mm = base
                        * Mat4::from_translation(Vec3::new(sx * lx, len, sz * lz))
                        * Mat4::from_rotation_x(ang);
                    push_box(
                        out,
                        &mm,
                        Vec3::new(-w, -len, -w),
                        Vec3::new(w, 0.0, w),
                        tint(c),
                        255,
                    );
                }
            };
            match m.kind {
                Kind::Deer => {
                    let (c, dark, pale) = ([156, 108, 62], [112, 76, 44], [232, 222, 200]);
                    part(&body, [-0.25, 0.72, -0.58], [0.25, 1.22, 0.58], c);
                    part(&body, [-0.07, 0.98, -0.68], [0.07, 1.16, -0.58], pale); // tail
                    part(&head, [-0.13, 1.05, 0.42], [0.13, 1.5, 0.66], c); // neck
                    part(&head, [-0.16, 1.36, 0.5], [0.16, 1.68, 0.96], c); // head
                    part(&head, [-0.09, 1.38, 0.96], [0.09, 1.52, 1.04], dark); // nose
                    part(&head, [-0.2, 1.68, 0.56], [-0.14, 1.98, 0.62], pale); // antlers
                    part(&head, [0.14, 1.68, 0.56], [0.2, 1.98, 0.62], pale);
                    part(&head, [-0.34, 1.86, 0.56], [-0.2, 1.92, 0.62], pale);
                    part(&head, [0.2, 1.86, 0.56], [0.34, 1.92, 0.62], pale);
                    drop(part);
                    legs(out, 0.17, 0.44, 0.74, 0.065, dark);
                }
                Kind::Boar => {
                    let (c, dark, pale) = ([78, 60, 48], [52, 40, 32], [236, 230, 214]);
                    part(&body, [-0.3, 0.34, -0.52], [0.3, 0.86, 0.5], c);
                    part(&body, [-0.1, 0.86, -0.4], [0.1, 0.94, 0.4], dark); // bristle ridge
                    let head = base * Mat4::from_translation(head_pivot) * Mat4::from_rotation_x(head_ang * 0.45) * Mat4::from_translation(-head_pivot);
                    part(&head, [-0.23, 0.36, 0.5], [0.23, 0.8, 0.86], c); // head
                    part(&head, [-0.11, 0.4, 0.86], [0.11, 0.58, 0.99], [150, 104, 100]); // snout
                    part(&head, [-0.2, 0.44, 0.84], [-0.15, 0.62, 0.89], pale); // tusks
                    part(&head, [0.15, 0.44, 0.84], [0.2, 0.62, 0.89], pale);
                    drop(part);
                    legs(out, 0.2, 0.36, 0.36, 0.08, dark);
                }
                Kind::Hare => {
                    let (c, dark, pale) = ([176, 160, 134], [138, 122, 100], [244, 240, 232]);
                    part(&body, [-0.13, 0.1, -0.22], [0.13, 0.36, 0.2], c);
                    part(&body, [-0.05, 0.2, -0.29], [0.05, 0.3, -0.22], pale); // tail
                    part(&head, [-0.1, 0.28, 0.14], [0.1, 0.5, 0.38], c); // head
                    // ears twitch
                    let tw = (m.age * 3.1).sin().max(0.0) * 0.03;
                    part(&head, [-0.09, 0.5, 0.18 - tw], [-0.03, 0.78, 0.24 - tw], dark);
                    part(&head, [0.03, 0.5, 0.18], [0.09, 0.78, 0.24], dark);
                    drop(part);
                    legs(out, 0.09, 0.14, 0.12, 0.04, dark);
                }
                Kind::Wolf => {
                    let (c, dark, pale) = ([112, 116, 124], [70, 74, 82], [206, 208, 212]);
                    part(&body, [-0.2, 0.42, -0.5], [0.2, 0.8, 0.42], c);
                    part(&body, [-0.22, 0.46, 0.14], [0.22, 0.86, 0.44], pale); // ruff
                    part(&body, [-0.06, 0.5, -0.86], [0.06, 0.64, -0.5], dark); // tail
                    let head = base * Mat4::from_translation(head_pivot) * Mat4::from_rotation_x(head_ang * 0.3) * Mat4::from_translation(-head_pivot);
                    part(&head, [-0.17, 0.56, 0.4], [0.17, 0.9, 0.72], c); // head
                    part(&head, [-0.09, 0.56, 0.72], [0.09, 0.72, 0.94], pale); // muzzle
                    part(&head, [-0.05, 0.68, 0.9], [0.05, 0.74, 0.96], [20, 20, 22]); // nose
                    part(&head, [-0.16, 0.9, 0.44], [-0.07, 1.04, 0.52], dark); // ears
                    part(&head, [0.07, 0.9, 0.44], [0.16, 1.04, 0.52], dark);
                    // (a dog's eyes are warm brown, and it wears a red collar)
                    let eye = if m.tame { [90, 60, 30] } else { [255, 214, 70] };
                    part(&head, [-0.13, 0.76, 0.715], [-0.07, 0.81, 0.725], eye); // eyes
                    part(&head, [0.07, 0.76, 0.715], [0.13, 0.81, 0.725], eye);
                    if m.tame {
                        part(&body, [-0.18, 0.56, 0.36], [0.18, 0.84, 0.46], [196, 36, 36]);
                    }
                    drop(part);
                    legs(out, 0.14, 0.34, 0.44, 0.06, dark);
                }
                Kind::Sheep => {
                    let wool = COLOURS[m.wool as usize];
                    let (face, dark) = ([214, 196, 172], [74, 62, 54]);
                    if m.sheared {
                        part(&body, [-0.2, 0.5, -0.46], [0.2, 0.86, 0.44], [222, 206, 186]);
                    } else {
                        // a thick coat, a little uneven on top
                        part(&body, [-0.3, 0.42, -0.56], [0.3, 1.02, 0.52], wool);
                        part(&body, [-0.24, 1.02, -0.4], [0.24, 1.08, 0.3], wool.map(|v| (v as u16 * 9 / 10) as u8));
                    }
                    part(&head, [-0.15, 0.7, 0.46], [0.15, 1.02, 0.8], face); // head
                    part(&head, [-0.08, 0.72, 0.8], [0.08, 0.84, 0.84], dark); // muzzle
                    part(&head, [-0.13, 0.86, 0.79], [-0.07, 0.91, 0.805], [30, 26, 24]); // eyes
                    part(&head, [0.07, 0.86, 0.79], [0.13, 0.91, 0.805], [30, 26, 24]);
                    part(&head, [-0.25, 0.88, 0.52], [-0.15, 0.94, 0.62], face); // ears
                    part(&head, [0.15, 0.88, 0.52], [0.25, 0.94, 0.62], face);
                    if !m.sheared {
                        part(&head, [-0.16, 0.96, 0.46], [0.16, 1.06, 0.66], wool); // tuft
                    }
                    drop(part);
                    legs(out, 0.18, 0.36, 0.46, 0.07, if m.sheared { face } else { dark });
                }
                _ => {}
            }
        }
    }
}

/// Builds a monster out of boxes. `base` places it in the world, +z is its front.
fn draw_monster(out: &mut Vec<EntVert>, m: &Mob, base: &Mat4, swing: f32, tint: &dyn Fn([u8; 3]) -> [u8; 3]) {
    const O: u8 = 255;
    let alive = m.dead < 0.0;
    let mut part = |mm: &Mat4, min: [f32; 3], max: [f32; 3], c: [u8; 3], alpha: u8| {
        push_box(out, mm, min.into(), max.into(), tint(c), alpha);
    };
    // a limb hanging from `pivot`, swung by `ang` about the x axis
    let limb = |pivot: [f32; 3], ang: f32| *base * Mat4::from_translation(pivot.into()) * Mat4::from_rotation_x(ang);
    let sway = if alive { (m.age * 1.7).sin() * 0.06 } else { 0.0 };
    match m.kind {
        Kind::Zombie => {
            let (skin, rags, trousers) = ([108, 142, 104], [96, 74, 60], [58, 62, 84]);
            part(base, [-0.25, 0.75, -0.13], [0.25, 1.4, 0.13], rags, O);
            part(base, [-0.25, 0.75, -0.14], [0.08, 0.95, 0.14], skin, O); // torn shirt
            let head = *base * Mat4::from_translation(Vec3::new(0.0, 1.4, 0.0)) * Mat4::from_rotation_z(0.16 + sway);
            part(&head, [-0.2, 0.0, -0.2], [0.2, 0.4, 0.2], skin, O);
            part(&head, [-0.2, 0.3, -0.21], [0.2, 0.41, 0.1], [44, 52, 40], O); // matted hair
            part(&head, [-0.1, 0.08, 0.2], [0.1, 0.13, 0.21], [40, 30, 30], O); // mouth
            for sx in [-1.0f32, 1.0] {
                part(&head, [sx * 0.095 - 0.045, 0.2, 0.2], [sx * 0.095 + 0.045, 0.26, 0.21], [255, 70, 50], EMISSIVE);
                part(&limb([sx * 0.11, 0.75, 0.0], sx * swing), [-0.1, -0.75, -0.1], [0.1, 0.0, 0.1], trousers, O);
                // arms held out in front, one drooping a little lower
                let droop = -1.45 + sway + if sx > 0.0 { 0.22 } else { 0.0 };
                part(&limb([sx * 0.33, 1.32, 0.0], droop - sx * swing * 0.25), [-0.08, -0.62, -0.08], [0.08, 0.0, 0.08], skin, O);
            }
        }
        Kind::Skeleton => {
            let (bone, shade, wood) = ([226, 222, 204], [150, 146, 132], [122, 86, 48]);
            part(base, [-0.04, 0.75, -0.04], [0.04, 1.42, 0.04], shade, O); // spine
            part(base, [-0.16, 0.72, -0.07], [0.16, 0.82, 0.07], bone, O); // hips
            for i in 0..3 {
                let y = 0.98 + i as f32 * 0.14;
                part(base, [-0.19, y, -0.09], [0.19, y + 0.07, 0.09], bone, O); // ribs
            }
            part(base, [-0.18, 1.42, -0.18], [0.18, 1.78, 0.18], bone, O); // skull
            part(base, [-0.1, 1.44, 0.18], [0.1, 1.5, 0.19], shade, O); // teeth
            for sx in [-1.0f32, 1.0] {
                part(base, [sx * 0.1 - 0.045, 1.58, 0.18], [sx * 0.1 + 0.045, 1.67, 0.19], [24, 24, 30], O); // sockets
                part(&limb([sx * 0.1, 0.75, 0.0], sx * swing), [-0.045, -0.75, -0.045], [0.045, 0.0, 0.045], bone, O);
            }
            // bow arm straight out, the other drawn back to the jaw
            let bow_arm = limb([0.24, 1.36, 0.0], -1.5 + sway);
            part(&bow_arm, [-0.04, -0.6, -0.04], [0.04, 0.0, 0.04], bone, O);
            part(&bow_arm, [-0.02, -0.64, -0.5], [0.02, -0.58, 0.5], wood, O); // bow
            part(&limb([-0.24, 1.36, 0.0], -0.9 + sway), [-0.04, -0.55, -0.04], [0.04, 0.0, 0.04], bone, O);
        }
        Kind::Spider => {
            let (c, dark, mark) = ([52, 44, 50], [30, 26, 30], [150, 60, 40]);
            let bob = if alive { (m.age * 3.0).sin() * 0.015 } else { 0.0 };
            let body = *base * Mat4::from_translation(Vec3::new(0.0, bob, 0.0));
            part(&body, [-0.3, 0.24, -0.78], [0.3, 0.64, -0.12], c, O); // abdomen
            part(&body, [-0.1, 0.64, -0.6], [0.1, 0.66, -0.3], mark, O);
            part(&body, [-0.2, 0.24, -0.12], [0.2, 0.52, 0.36], dark, O); // head
            for sx in [-1.0f32, 1.0] {
                part(&body, [sx * 0.06 - 0.03, 0.4, 0.36], [sx * 0.06 + 0.03, 0.46, 0.37], [255, 40, 40], EMISSIVE);
                part(&body, [sx * 0.14 - 0.02, 0.36, 0.36], [sx * 0.14 + 0.02, 0.4, 0.37], [255, 40, 40], EMISSIVE);
                part(&body, [sx * 0.08 - 0.03, 0.2, 0.36], [sx * 0.08 + 0.03, 0.3, 0.42], [200, 190, 170], O); // fangs
                for i in 0..4 {
                    // four legs a side, fanned out, alternate ones stepping together
                    let spread = (i as f32 - 1.5) * 0.42;
                    let step = if i % 2 == 0 { swing } else { -swing } * 0.5;
                    let leg = body
                        * Mat4::from_translation(Vec3::new(sx * 0.18, 0.42, 0.12 - i as f32 * 0.08))
                        * Mat4::from_rotation_y(sx * (spread + step))
                        * Mat4::from_rotation_z(-sx * 0.48);
                    let (a, b) = if sx > 0.0 { (0.0, 0.86) } else { (-0.86, 0.0) };
                    part(&leg, [a, -0.035, -0.035], [b, 0.035, 0.035], dark, O);
                }
            }
        }
        Kind::Slime => {
            // stretches as it leaves the ground, flattens as it lands
            let sy = if alive { (1.0 + m.vel.y * 0.035).clamp(0.72, 1.3) + (m.age * 2.4).sin() * 0.03 } else { 0.6 };
            let sxz = 1.0 / sy.sqrt();
            let body = *base * Mat4::from_scale(Vec3::new(sxz, sy, sxz));
            part(&body, [-0.4, 0.0, -0.4], [0.4, 0.7, 0.4], [74, 168, 150], O);
            part(&body, [-0.3, 0.7, -0.3], [0.3, 0.74, 0.3], [120, 206, 186], O); // sheen
            part(&body, [-0.16, 0.14, 0.4], [0.16, 0.2, 0.41], [24, 70, 62], O); // mouth
            for sx in [-1.0f32, 1.0] {
                part(&body, [sx * 0.17 - 0.07, 0.36, 0.4], [sx * 0.17 + 0.07, 0.52, 0.41], [20, 44, 44], O);
            }
        }
        Kind::Puffcap => {
            // a stout mushroom on stubby feet; when its fuse burns it swells and flashes
            let lit = m.fuse >= 0.0;
            let swell = if lit { 1.0 + 0.28 * (m.fuse / FUSE).min(1.0) } else { 1.0 };
            let flash = lit && (m.fuse * m.fuse * 14.0).sin() > 0.0;
            let w = |c: [u8; 3]| if flash { [255, 255, 255] } else { c };
            let (stalk, cap, spot, gill) = ([226, 214, 186], [186, 44, 38], [246, 240, 228], [180, 150, 120]);
            let body = *base * Mat4::from_translation(Vec3::new(0.0, 0.2, 0.0)) * Mat4::from_scale(Vec3::new(swell, 1.0 + (swell - 1.0) * 0.5, swell)) * Mat4::from_translation(Vec3::new(0.0, -0.2, 0.0));
            part(&body, [-0.2, 0.18, -0.2], [0.2, 0.86, 0.2], w(stalk), O); // stalk
            part(&body, [-0.46, 0.84, -0.46], [0.46, 0.92, 0.46], w(gill), O); // gills
            part(&body, [-0.48, 0.92, -0.48], [0.48, 1.22, 0.48], w(cap), O); // cap
            part(&body, [-0.34, 1.22, -0.34], [0.34, 1.3, 0.34], w(cap), O);
            for (x, z) in [(-0.3, 0.2), (0.22, -0.28), (0.05, 0.36), (-0.36, -0.22), (0.38, 0.12)] {
                part(&body, [x - 0.07, 1.205, z - 0.07], [x + 0.07, 1.31, z + 0.07], w(spot), O);
            }
            // a little sleepy face on the stalk
            for sx in [-1.0f32, 1.0] {
                part(&body, [sx * 0.08 - 0.03, 0.6, 0.2], [sx * 0.08 + 0.03, 0.66, 0.21], [30, 24, 20], O);
                part(&limb([sx * 0.11, 0.2, 0.0], sx * swing * 0.6), [-0.08, -0.2, -0.09], [0.08, 0.0, 0.11], w(stalk), O);
            }
            part(&body, [-0.05, 0.5, 0.2], [0.05, 0.53, 0.21], [90, 50, 40], O);
        }
        Kind::Hound => {
            // a lean black hound, cracked through with fire, a flame for a tail
            let (hide, dark, fire, hot) = ([46, 36, 36], [28, 22, 24], [255, 110, 30], [255, 214, 120]);
            let pant = if alive { (m.age * 6.0).sin() * 0.015 } else { 0.0 };
            let body = *base * Mat4::from_translation(Vec3::new(0.0, pant, 0.0));
            part(&body, [-0.2, 0.44, -0.5], [0.2, 0.78, 0.4], hide, O);
            part(&body, [-0.22, 0.46, 0.1], [0.22, 0.84, 0.42], dark, O); // shoulders
            // embers along the spine, flickering
            for i in 0..4 {
                let z = -0.42 + i as f32 * 0.2;
                let f = if alive { ((m.age * 7.0 + i as f32 * 1.7).sin() * 0.5 + 0.5) * 0.06 } else { 0.0 };
                part(&body, [-0.05, 0.78, z], [0.05, 0.84 + f, z + 0.12], fire, EMISSIVE);
            }
            part(&body, [-0.21, 0.55, -0.2], [-0.2, 0.62, 0.2], fire, EMISSIVE); // cracks in the flank
            part(&body, [0.2, 0.6, -0.35], [0.21, 0.66, 0.0], fire, EMISSIVE);
            let head = body * Mat4::from_translation(Vec3::new(0.0, 0.66, 0.4)) * Mat4::from_rotation_x(sway * 0.5) * Mat4::from_translation(Vec3::new(0.0, -0.66, -0.4));
            part(&head, [-0.16, 0.54, 0.38], [0.16, 0.86, 0.7], hide, O);
            part(&head, [-0.09, 0.54, 0.7], [0.09, 0.7, 0.9], dark, O); // snout
            part(&head, [-0.08, 0.5, 0.72], [0.08, 0.54, 0.86], fire, EMISSIVE); // a glowing jaw
            for sx in [-1.0f32, 1.0] {
                part(&head, [sx * 0.09 - 0.035, 0.74, 0.7], [sx * 0.09 + 0.035, 0.79, 0.71], hot, EMISSIVE);
                part(&head, [sx * 0.11 - 0.04, 0.86, 0.44], [sx * 0.11 + 0.04, 1.0, 0.52], dark, O); // ears
            }
            // the tail: a lick of flame that waves
            let wave = if alive { (m.age * 5.0).sin() * 0.25 } else { 0.0 };
            let tail = body * Mat4::from_translation(Vec3::new(0.0, 0.72, -0.5)) * Mat4::from_rotation_y(wave) * Mat4::from_rotation_x(-0.6);
            part(&tail, [-0.05, -0.05, -0.36], [0.05, 0.05, 0.0], fire, EMISSIVE);
            part(&tail, [-0.03, -0.03, -0.46], [0.03, 0.03, -0.3], hot, EMISSIVE);
            for (i, (sx, sz)) in [(1.0f32, 1.0f32), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].into_iter().enumerate() {
                let ang = if i == 0 || i == 3 { swing } else { -swing };
                part(&limb([sx * 0.13, 0.46, sz * 0.32], ang), [-0.06, -0.46, -0.06], [0.06, 0.0, 0.06], dark, O);
            }
        }
        Kind::Wisp => {
            // a knot of fire: a white-hot heart in a shell of flame, sparks circling it
            let t = m.age;
            let core = *base * Mat4::from_translation(Vec3::new(0.0, 0.42, 0.0));
            let spin = core * Mat4::from_rotation_y(t * 2.3) * Mat4::from_rotation_x(t * 1.7);
            part(&core, [-0.15, -0.15, -0.15], [0.15, 0.15, 0.15], [255, 236, 170], EMISSIVE);
            part(&spin, [-0.26, -0.2, -0.2], [0.26, 0.2, 0.2], [240, 90, 30], EMISSIVE);
            part(&spin, [-0.2, -0.26, -0.2], [0.2, 0.26, 0.2], [200, 50, 30], 190);
            // two dark eyes on the heart
            for sx in [-1.0f32, 1.0] {
                part(&core, [sx * 0.06 - 0.025, 0.0, 0.15], [sx * 0.06 + 0.025, 0.07, 0.16], [40, 10, 10], O);
            }
            if alive {
                for i in 0..4 {
                    let a = t * 3.2 + i as f32 * std::f32::consts::FRAC_PI_2;
                    let p = Vec3::new(a.cos() * 0.42, (t * 2.0 + i as f32).sin() * 0.15, a.sin() * 0.42);
                    let e = core * Mat4::from_translation(p);
                    part(&e, [-0.05, -0.05, -0.05], [0.05, 0.05, 0.05], [255, 160, 60], EMISSIVE);
                }
                // flames licking up from the top
                let lick = (t * 9.0).sin() * 0.06;
                part(&core, [-0.08, 0.18, -0.08], [0.08, 0.38 + lick, 0.08], [255, 130, 40], EMISSIVE);
            }
        }
        Kind::Brute => {
            // a hulk of cinder and brick with a fire burning in its chest, dragging a club
            let (rock, plate, fire, wood) = ([60, 50, 52], [74, 40, 38], [255, 110, 40], [96, 66, 44]);
            let breathe = if alive { (m.age * 1.4).sin() * 0.02 } else { 0.0 };
            let body = *base * Mat4::from_translation(Vec3::new(0.0, breathe, 0.0));
            part(&body, [-0.44, 0.95, -0.3], [0.44, 1.95, 0.3], rock, O); // torso
            part(&body, [-0.48, 1.6, -0.34], [0.48, 2.05, 0.3], plate, O); // shoulders
            part(&body, [-0.16, 1.25, 0.3], [0.16, 1.6, 0.33], fire, EMISSIVE); // the fire in its chest
            part(&body, [-0.3, 0.95, 0.3], [0.3, 1.1, 0.32], plate, O);
            let head = body * Mat4::from_translation(Vec3::new(0.0, 1.95, 0.05)) * Mat4::from_rotation_x(0.25 + sway);
            part(&head, [-0.2, 0.0, -0.12], [0.2, 0.36, 0.26], rock, O);
            for sx in [-1.0f32, 1.0] {
                part(&head, [sx * 0.08 - 0.04, 0.16, 0.26], [sx * 0.08 + 0.04, 0.22, 0.27], [255, 180, 60], EMISSIVE);
                part(&head, [sx * 0.2 - 0.04, 0.26, 0.0], [sx * 0.2 + 0.04, 0.5, 0.08], [200, 190, 170], O); // horns
                part(&limb([sx * 0.22, 0.95, 0.0], sx * swing * 0.6), [-0.15, -0.95, -0.16], [0.15, 0.0, 0.16], rock, O);
            }
            // arms: the right drags a club; both swing up when it strikes
            let strike = if alive && m.bite > 1.3 { (m.bite - 1.3) * 3.0 } else { 0.0 };
            part(&limb([-0.58, 1.9, 0.0], -swing * 0.4 - strike), [-0.13, -1.25, -0.13], [0.13, 0.0, 0.13], plate, O);
            let arm = limb([0.58, 1.9, 0.0], swing * 0.4 - strike * 1.2);
            part(&arm, [-0.13, -1.25, -0.13], [0.13, 0.0, 0.13], plate, O);
            part(&arm, [-0.06, -1.95, -0.06], [0.06, -1.1, 0.06], wood, O);
            part(&arm, [-0.17, -2.3, -0.17], [0.17, -1.85, 0.17], [70, 66, 74], O);
        }
        Kind::Villager => {
            // dressed each in their own colours, some in a straw hat, some in a cap
            let look = look(m);
            const TUNICS: [[u8; 3]; 6] = [[156, 62, 52], [58, 92, 150], [84, 124, 62], [190, 140, 56], [112, 70, 120], [200, 188, 160]];
            const SKINS: [[u8; 3]; 4] = [[232, 190, 152], [204, 156, 112], [160, 112, 76], [116, 78, 54]];
            const HAIRS: [[u8; 3]; 4] = [[52, 38, 28], [110, 72, 38], [176, 140, 76], [150, 150, 150]];
            let tunic = TUNICS[(look % 6) as usize];
            let skin = SKINS[((look >> 4) % 4) as usize];
            let hair = HAIRS[((look >> 8) % 4) as usize];
            let shade = |c: [u8; 3]| c.map(|v| (v as u16 * 7 / 10) as u8);
            let (trousers, straw) = ([92, 72, 54], [216, 188, 112]);
            part(base, [-0.24, 0.74, -0.14], [0.24, 1.38, 0.14], tunic, O);
            part(base, [-0.25, 0.56, -0.15], [0.25, 0.76, 0.15], shade(tunic), O); // skirt of the tunic
            part(base, [-0.255, 0.78, -0.155], [0.255, 0.85, 0.155], [70, 50, 34], O); // belt
            let nod = if alive { (m.age * 0.9 + m.slot as f32).sin() * 0.04 } else { 0.0 };
            let head = *base * Mat4::from_translation(Vec3::new(0.0, 1.38, 0.0)) * Mat4::from_rotation_x(nod);
            part(&head, [-0.19, 0.0, -0.19], [0.19, 0.38, 0.19], skin, O);
            part(&head, [-0.2, 0.12, -0.2], [0.2, 0.39, -0.06], hair, O); // hair down the back of the head
            part(&head, [-0.07, 0.07, 0.19], [0.07, 0.1, 0.2], shade(skin), O); // mouth
            if (look >> 12) & 1 == 0 {
                part(&head, [-0.36, 0.34, -0.36], [0.36, 0.38, 0.36], straw, O); // brim
                part(&head, [-0.2, 0.38, -0.2], [0.2, 0.5, 0.2], straw, O);
                part(&head, [-0.205, 0.38, -0.205], [0.205, 0.42, 0.205], shade(tunic), O); // band
            } else {
                part(&head, [-0.2, 0.3, -0.2], [0.2, 0.42, 0.2], shade(tunic), O); // cap
            }
            for sx in [-1.0f32, 1.0] {
                part(&head, [sx * 0.09 - 0.035, 0.19, 0.19], [sx * 0.09 + 0.035, 0.25, 0.2], [40, 32, 30], O); // eyes
                part(&limb([sx * 0.11, 0.75, 0.0], sx * swing), [-0.1, -0.75, -0.1], [0.1, 0.0, 0.1], trousers, O);
                // arms at the sides, swinging against the legs: a sleeve, and a hand below it
                let arm = limb([sx * 0.32, 1.34, 0.0], -sx * swing * 0.8 + sway * 0.3);
                part(&arm, [-0.08, -0.42, -0.08], [0.08, 0.0, 0.08], tunic, O);
                part(&arm, [-0.07, -0.62, -0.07], [0.07, -0.42, 0.07], skin, O);
            }
        }
        _ => {}
    }
}

/// Emits an oriented box as 12 triangles with simple directional shading.
pub fn push_box(out: &mut Vec<EntVert>, m: &Mat4, min: Vec3, max: Vec3, c: [u8; 3], alpha: u8) {
    let p = |x: f32, y: f32, z: f32| m.transform_point3(Vec3::new(x, y, z));
    let (a, b) = (min, max);
    let faces: [([Vec3; 4], Vec3); 6] = [
        ([p(b.x, a.y, a.z), p(b.x, b.y, a.z), p(b.x, b.y, b.z), p(b.x, a.y, b.z)], Vec3::X),
        ([p(a.x, a.y, a.z), p(a.x, a.y, b.z), p(a.x, b.y, b.z), p(a.x, b.y, a.z)], Vec3::NEG_X),
        ([p(a.x, b.y, a.z), p(a.x, b.y, b.z), p(b.x, b.y, b.z), p(b.x, b.y, a.z)], Vec3::Y),
        ([p(a.x, a.y, a.z), p(b.x, a.y, a.z), p(b.x, a.y, b.z), p(a.x, a.y, b.z)], Vec3::NEG_Y),
        ([p(a.x, a.y, b.z), p(b.x, a.y, b.z), p(b.x, b.y, b.z), p(a.x, b.y, b.z)], Vec3::Z),
        ([p(a.x, a.y, a.z), p(a.x, b.y, a.z), p(b.x, b.y, a.z), p(b.x, a.y, a.z)], Vec3::NEG_Z),
    ];
    for (q, n) in faces {
        let n = m.transform_vector3(n);
        let k = 0.74 + 0.26 * n.y + 0.07 * n.x;
        let col = [
            (c[0] as f32 * k).min(255.0) as u8,
            (c[1] as f32 * k).min(255.0) as u8,
            (c[2] as f32 * k).min(255.0) as u8,
            alpha,
        ];
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(EntVert { pos: q[i].into(), color: col });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> World {
        World::flat(|_, y, _| (y == 9).then_some(crate::world::GRASS))
    }

    #[test]
    fn two_animals_in_love_make_a_baby() {
        let w = field();
        let mut mobs = Mobs::new(5);
        mobs.ignore = true;
        let a = mobs.spawn(Kind::Sheep, Vec3::new(2.5, 10.0, 2.5));
        let b = mobs.spawn(Kind::Sheep, Vec3::new(5.5, 10.0, 2.5));
        assert!(matches!(mobs.interact(a, WHEAT), Use::Love));
        assert!(matches!(mobs.interact(b, WHEAT), Use::Love));
        let player = Vec3::new(30.0, 10.0, 30.0);
        for _ in 0..(8.0 * 60.0) as usize {
            mobs.update(&w, player, false, false, AIR, 1.0 / 60.0);
            if mobs.list.iter().any(|m| m.baby < 1.0) {
                break;
            }
        }
        let baby = mobs.list.iter().find(|m| m.baby < 1.0).expect("a lamb is born");
        assert!(baby.kind == Kind::Sheep && baby.keep);
        assert!(mobs.events.iter().any(|e| matches!(e, MobEvent::Born { kind: Kind::Sheep, .. })));
        // the parents cannot breed again straight away
        assert!(matches!(mobs.interact(a, WHEAT), Use::Nothing));
    }

    #[test]
    fn a_shorn_sheep_gives_its_colour_and_a_dyed_one_changes() {
        let mut mobs = Mobs::new(9);
        let i = mobs.spawn(Kind::Sheep, Vec3::new(0.5, 10.0, 0.5));
        assert!(matches!(mobs.interact(i, items::DYE + 14), Use::Dyed));
        match mobs.interact(i, SHEARS) {
            Use::Sheared { item, n } => assert!(item == items::WOOL_ITEM + 13 && (1..=3).contains(&n)),
            _ => panic!("shears shear"),
        }
        assert!(matches!(mobs.interact(i, SHEARS), Use::Nothing));
    }

    #[test]
    fn a_puffcap_next_to_the_player_bursts() {
        let w = field();
        let mut mobs = Mobs::new(3);
        mobs.spawn(Kind::Puffcap, Vec3::new(4.5, 10.0, 2.5));
        let player = Vec3::new(2.5, 10.0, 2.5);
        let mut lit = false;
        for _ in 0..(4.0 * 60.0) as usize {
            mobs.update(&w, player, false, true, AIR, 1.0 / 60.0);
            lit |= mobs.events.iter().any(|e| matches!(e, MobEvent::Fuse { .. }));
            if mobs.events.iter().any(|e| matches!(e, MobEvent::Explode { .. })) {
                assert!(lit, "the fuse burns first");
                return;
            }
            mobs.events.clear();
        }
        panic!("no burst");
    }
}
