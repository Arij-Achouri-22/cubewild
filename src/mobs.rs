//! Animals: huntable deer, boar and hare, and wolves that hunt you at night.
//! Monsters: zombies, skeleton archers, giant spiders and slimes that roam the dark.

use crate::gfx::{EntVert, EMISSIVE};
use crate::items::{Item, ARROW, FIBER, FLINT, HIDE, IRON_INGOT, MEAT};
use crate::player::{collides, move_collide, ray_aabb};
use crate::world::{is_leaves, is_solid, World, CLAY, CLOTH, GRASS, SNOW, WATER};
use glam::{Mat4, Vec3};

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

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Deer,
    Boar,
    Hare,
    Wolf,
    Zombie,
    Skeleton,
    Spider,
    Slime,
}

impl Kind {
    pub fn is_monster(self) -> bool {
        matches!(self, Kind::Zombie | Kind::Skeleton | Kind::Spider | Kind::Slime)
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
        Kind::Wolf => sp(0.3, 0.85, 6, 1.5, 5.7, 0.0, 0, 1, 26.0, 3.0),
        Kind::Zombie => sp(0.3, 1.8, 10, 0.9, 3.0, 0.0, 0, 0, 22.0, 3.0),
        Kind::Skeleton => sp(0.25, 1.8, 7, 1.6, 3.2, 0.0, 0, 0, 20.0, 0.0),
        Kind::Spider => sp(0.45, 0.6, 5, 1.4, 5.6, 0.0, 0, 0, 16.0, 2.0),
        Kind::Slime => sp(0.4, 0.7, 5, 2.2, 3.6, 0.0, 0, 0, 14.0, 2.0),
    }
}

fn drops(k: Kind, rng: &mut Rng) -> Drops {
    let s = spec(k);
    match k {
        Kind::Zombie => [(CLOTH as Item, 1), (IRON_INGOT, (rng.next() % 6 == 0) as u32)],
        Kind::Skeleton => [(ARROW, 1 + rng.next() % 3), (FLINT, (rng.next() % 3 == 0) as u32)],
        Kind::Spider => [(FIBER, 1 + rng.next() % 2), (0, 0)],
        Kind::Slime => [(CLAY as Item, 1 + rng.next() % 2), (0, 0)],
        _ => [(MEAT, s.meat), (HIDE, s.hide)],
    }
}

/// True if nothing solid lies on the straight line from `a` to `b`.
fn clear_path(world: &World, a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let n = (d.length() * 2.0).ceil() as i32;
    (1..n).all(|i| {
        let p = (a + d * (i as f32 / n as f32)).floor().as_ivec3();
        !is_solid(world.get_block(p.x, p.y, p.z))
    })
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
}

pub struct Mobs {
    pub list: Vec<Mob>,
    /// arrows loosed by skeletons this frame: (position, velocity)
    pub shots: Vec<(Vec3, Vec3)>,
    rng: Rng,
    spawn_timer: f32,
}

const MAX_ANIMALS: usize = 12;
const MAX_WOLVES: usize = 4;
const MAX_MONSTERS: usize = 10;
/// monsters never appear this close to a torch or lamp
const SAFE_LIGHT: f32 = 9.0;

impl Mobs {
    pub fn new(seed: u64) -> Mobs {
        Mobs { list: Vec::new(), shots: Vec::new(), rng: Rng(seed | 1), spawn_timer: 0.0 }
    }

    fn try_spawn(&mut self, world: &World, player: Vec3, night: bool) {
        let ang = self.rng.range(0.0, std::f32::consts::TAU);
        let dist = self.rng.range(26.0, 72.0);
        let (bx, bz) = (player.x + ang.cos() * dist, player.z + ang.sin() * dist);
        let (mut animals, mut wolves, mut monsters) = (0, 0, 0);
        for m in &self.list {
            match m.kind {
                Kind::Wolf => wolves += 1,
                k if k.is_monster() => monsters += 1,
                _ => animals += 1,
            }
        }
        let roll = self.rng.next() % 6;
        let kind = if night && monsters < MAX_MONSTERS && roll < 3 {
            match self.rng.next() % 10 {
                0..=3 => Kind::Zombie,
                4..=6 => Kind::Skeleton,
                7..=8 => Kind::Spider,
                _ => Kind::Slime,
            }
        } else if night && wolves < MAX_WOLVES && roll == 3 {
            Kind::Wolf
        } else if animals < MAX_ANIMALS && roll >= 4 {
            // herds refill slowly, so hunted land stays quiet for a while
            match self.rng.next() % 10 {
                0..=3 => Kind::Deer,
                4..=6 => Kind::Boar,
                _ => Kind::Hare,
            }
        } else {
            return;
        };
        let group = match kind {
            Kind::Zombie | Kind::Wolf | Kind::Deer | Kind::Hare => 1 + self.rng.next() % 2,
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
            } else if top != GRASS && !(top == SNOW && kind == Kind::Hare) {
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
        for (i, kind) in [Kind::Zombie, Kind::Skeleton, Kind::Spider, Kind::Slime].into_iter().enumerate() {
            let (x, z) = ((p.x + i as f32 * 2.0 - 3.0).floor() as i32, (p.z + 7.0).floor() as i32);
            if let Some(y) = world.surface_y(x, z) {
                self.add(kind, Vec3::new(x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5));
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
            });
        }
    }

    /// Advances every animal and monster. Returns the damage they dealt to the player
    /// up close; arrows shot by skeletons are left in `shots`.
    pub fn update(&mut self, world: &World, player: Vec3, sneaking: bool, night: bool, dt: f32) -> f32 {
        let mut damage = 0.0;
        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn_timer = 0.6;
            self.try_spawn(world, player, night);
        }
        // wolves and monsters slink away at dawn once they are out of sight
        self.list.retain(|m| {
            night || !(m.kind == Kind::Wolf || m.kind.is_monster()) || (m.pos - player).length_squared() < 34.0 * 34.0
        });
        // animals whose chunk got unloaded are removed rather than left frozen in place
        self.list.retain(|m| {
            m.dead < 1.3
                && (m.pos - player).length_squared() < 120.0 * 120.0
                && m.pos.y > -10.0
                && world.is_loaded(m.pos.x.floor() as i32, m.pos.z.floor() as i32)
        });
        let rng = &mut self.rng;
        let shots = &mut self.shots;
        for m in &mut self.list {
            let s = spec(m.kind);
            m.age += dt;
            m.hurt = (m.hurt - dt).max(0.0);
            let to_player = player - m.pos;
            let dist = to_player.length();
            let mut speed = 0.0;
            // direction to look in when it differs from the direction of travel
            let mut face: Option<f32> = None;
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
                let sight = if sneaking && m.kind.is_monster() { s.sight * 0.6 } else { s.sight };
                let hunting = m.flee <= 0.0 && dist < sight;
                if hunting {
                    let toward = to_player.x.atan2(to_player.z);
                    m.heading = toward;
                    m.grazing = false;
                    let flat = Vec3::new(to_player.x, 0.0, to_player.z).length();
                    match m.kind {
                        Kind::Skeleton => {
                            // keeps its distance and shoots
                            face = Some(toward);
                            if flat > 13.0 {
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
                        Kind::Spider => {
                            if flat > 1.0 {
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
                            if flat > 1.0 {
                                speed = s.run;
                            }
                        }
                    }
                    if s.dmg > 0.0 && flat < 1.5 && to_player.y.abs() < 1.9 && m.bite <= 0.0 {
                        m.bite = 1.2;
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
            let in_water = world.get_block(b.x, (m.pos.y + s.h * 0.5).floor() as i32, b.z) == WATER;
            if in_water {
                m.vel.y = (m.vel.y + 14.0 * dt).min(1.6);
            } else {
                m.vel.y = (m.vel.y - 29.0 * dt).max(-50.0);
            }
            let (ground, wall) = move_collide(world, &mut m.pos, &mut m.vel, s.hw, s.h, dt);
            m.on_ground = ground;
            if wall && speed > 0.0 {
                if m.kind == Kind::Spider {
                    m.vel.y = 4.0; // spiders climb straight up walls
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
        m.hp -= damage;
        m.hurt = 0.3;
        // prey bolts, a wolf backs off for a moment, monsters just keep coming
        m.flee = match m.kind {
            Kind::Wolf => 0.7,
            k if k.is_monster() => 0.0,
            _ => 6.0,
        };
        m.timer = 0.0;
        let mut away = m.pos - from;
        away.y = 0.0;
        let away = away.normalize_or_zero();
        m.vel += away * 6.0 + Vec3::new(0.0, 5.0, 0.0);
        if m.hp <= 0 {
            m.dead = 0.0;
            return Some(drops(m.kind, &mut self.rng));
        }
        None
    }

    /// True if a block at integer coords would overlap an animal.
    pub fn blocks_cell(&self, x: i32, y: i32, z: i32) -> bool {
        let (min, max) = (Vec3::new(x as f32, y as f32, z as f32), Vec3::new(x as f32 + 1.0, y as f32 + 1.0, z as f32 + 1.0));
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
            let mut base = Mat4::from_translation(rel + Vec3::new(0.0, hop, 0.0)) * Mat4::from_rotation_y(m.yaw);
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
            if m.kind.is_monster() {
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
                    part(&head, [-0.13, 0.76, 0.715], [-0.07, 0.81, 0.725], [255, 214, 70]); // eyes
                    part(&head, [0.07, 0.76, 0.715], [0.13, 0.81, 0.725], [255, 214, 70]);
                    drop(part);
                    legs(out, 0.14, 0.34, 0.44, 0.06, dark);
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
