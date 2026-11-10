//! Animals: huntable deer, boar and hare, and wolves that hunt you at night.

use crate::gfx::EntVert;
use crate::player::{collides, move_collide, ray_aabb};
use crate::world::{World, GRASS, SNOW, WATER};
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
}

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
}

fn spec(k: Kind) -> Spec {
    match k {
        Kind::Deer => Spec { hw: 0.32, h: 1.5, hp: 3, walk: 1.3, run: 6.9, wary: 9.0, meat: 3, hide: 2 },
        Kind::Boar => Spec { hw: 0.36, h: 0.9, hp: 4, walk: 1.1, run: 5.2, wary: 0.0, meat: 2, hide: 1 },
        Kind::Hare => Spec { hw: 0.18, h: 0.5, hp: 1, walk: 1.6, run: 7.4, wary: 6.0, meat: 1, hide: 1 },
        Kind::Wolf => Spec { hw: 0.3, h: 0.85, hp: 6, walk: 1.5, run: 5.7, wary: 0.0, meat: 0, hide: 1 },
    }
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
    /// seconds until a wolf can bite again
    bite: f32,
}

pub struct Mobs {
    pub list: Vec<Mob>,
    rng: Rng,
    spawn_timer: f32,
}

const MAX_MOBS: usize = 34;
const MAX_WOLVES: usize = 5;
/// how far a wolf notices the player
const WOLF_SIGHT: f32 = 26.0;

impl Mobs {
    pub fn new(seed: u64) -> Mobs {
        Mobs { list: Vec::new(), rng: Rng(seed | 1), spawn_timer: 0.0 }
    }

    fn try_spawn(&mut self, world: &World, player: Vec3, night: bool) {
        let ang = self.rng.range(0.0, std::f32::consts::TAU);
        let dist = self.rng.range(26.0, 72.0);
        let (bx, bz) = (player.x + ang.cos() * dist, player.z + ang.sin() * dist);
        let wolves = self.list.iter().filter(|m| m.kind == Kind::Wolf).count();
        let kind = if night && wolves < MAX_WOLVES && self.rng.next() % 3 == 0 {
            Kind::Wolf
        } else {
            match self.rng.next() % 10 {
                0..=3 => Kind::Deer,
                4..=6 => Kind::Boar,
                _ => Kind::Hare,
            }
        };
        let group = if kind == Kind::Wolf { 1 + self.rng.next() % 2 } else { 1 + self.rng.next() % 3 };
        for _ in 0..group {
            let x = (bx + self.rng.range(-4.0, 4.0)).floor() as i32;
            let z = (bz + self.rng.range(-4.0, 4.0)).floor() as i32;
            let Some(y) = world.surface_y(x, z) else { continue };
            let top = world.get_block(x, y, z);
            if top != GRASS && !(top == SNOW && kind == Kind::Hare) {
                continue;
            }
            let pos = Vec3::new(x as f32 + 0.5, y as f32 + 1.01, z as f32 + 0.5);
            let s = spec(kind);
            if collides(world, pos, s.hw, s.h) {
                continue;
            }
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
            });
        }
    }

    /// Advances every animal. Returns the damage wolves dealt to the player.
    pub fn update(&mut self, world: &World, player: Vec3, sneaking: bool, night: bool, dt: f32) -> f32 {
        let mut damage = 0.0;
        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn_timer = 0.6;
            if self.list.len() < MAX_MOBS {
                self.try_spawn(world, player, night);
            }
        }
        // wolves slink away at dawn once they are out of sight
        self.list.retain(|m| night || m.kind != Kind::Wolf || (m.pos - player).length_squared() < 34.0 * 34.0);
        // animals whose chunk got unloaded are removed rather than left frozen in place
        self.list.retain(|m| {
            m.dead < 1.3
                && (m.pos - player).length_squared() < 120.0 * 120.0
                && m.pos.y > -10.0
                && world.is_loaded(m.pos.x.floor() as i32, m.pos.z.floor() as i32)
        });
        let rng = &mut self.rng;
        for m in &mut self.list {
            let s = spec(m.kind);
            m.age += dt;
            m.hurt = (m.hurt - dt).max(0.0);
            let to_player = player - m.pos;
            let dist = to_player.length();
            let mut speed = 0.0;
            if m.dead >= 0.0 {
                m.dead += dt;
            } else {
                let wary = if sneaking { s.wary * 0.45 } else { s.wary };
                if dist < wary {
                    m.flee = m.flee.max(2.5);
                }
                m.bite -= dt;
                let hunting = m.kind == Kind::Wolf && m.flee <= 0.0 && dist < WOLF_SIGHT;
                if hunting {
                    m.heading = to_player.x.atan2(to_player.z);
                    m.grazing = false;
                    let flat = Vec3::new(to_player.x, 0.0, to_player.z).length();
                    if flat > 1.0 {
                        speed = s.run;
                    }
                    if flat < 1.5 && to_player.y.abs() < 1.9 && m.bite <= 0.0 {
                        m.bite = 1.2;
                        damage += 3.0;
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
                            m.grazing = !matches!(m.kind, Kind::Hare | Kind::Wolf) && rng.f() < 0.65;
                        }
                    }
                    if m.moving {
                        speed = s.walk;
                    }
                }
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
                if ground || in_water {
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
            if speed > 0.0 && hs < speed * 0.3 {
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
                let mut d = m.heading - m.yaw;
                d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                let rate = if speed > 0.0 { 9.0 } else { 1.6 };
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

    /// Damages an animal. Returns (meat, hides) if it died.
    pub fn hit(&mut self, i: usize, from: Vec3, damage: i32) -> Option<(u32, u32)> {
        let m = &mut self.list[i];
        let s = spec(m.kind);
        m.hp -= damage;
        m.hurt = 0.3;
        m.flee = if m.kind == Kind::Wolf { 0.7 } else { 6.0 };
        m.timer = 0.0;
        let mut away = m.pos - from;
        away.y = 0.0;
        let away = away.normalize_or_zero();
        m.vel += away * 6.0 + Vec3::new(0.0, 5.0, 0.0);
        if m.hp <= 0 {
            m.dead = 0.0;
            return Some((s.meat, s.hide));
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
            // head/neck group pivots down to graze, and bobs gently while idle
            let head_ang = m.graze * 1.35 + if alive { (m.age * 1.3).sin() * 0.05 * (1.0 - moving) } else { 0.0 };
            let head_pivot = match m.kind {
                Kind::Deer => Vec3::new(0.0, 1.12, 0.48),
                Kind::Boar => Vec3::new(0.0, 0.62, 0.46),
                Kind::Hare => Vec3::new(0.0, 0.32, 0.14),
                Kind::Wolf => Vec3::new(0.0, 0.66, 0.4),
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
            }
        }
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
