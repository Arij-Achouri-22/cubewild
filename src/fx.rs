//! Particles, weather and experience orbs: small things drawn as camera-facing quads into
//! the entity buffer, so they cost no draw call of their own.

use crate::gfx::{self, EntVert};
use crate::mobs::Rng;
use crate::world::{is_solid, World, WATER};
use glam::{IVec3, Vec3};

/// Hard cap: past this new particles push out the oldest.
const MAX_PARTICLES: usize = 900;

#[derive(Clone, Copy)]
struct P {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    max: f32,
    size: f32,
    color: [u8; 3],
    /// gravity in blocks/s², negative rises (smoke)
    grav: f32,
    glow: bool,
    /// shrinks as it dies instead of popping out
    shrink: bool,
}

#[derive(Default)]
pub struct Particles {
    list: Vec<P>,
}

impl Particles {
    fn push(&mut self, p: P) {
        if self.list.len() >= MAX_PARTICLES {
            self.list.swap_remove(0);
        }
        self.list.push(p);
    }

    /// Chips flying out of a broken block, or a few while it is being dug (`n` small).
    pub fn chips(&mut self, rng: &mut Rng, cell: IVec3, color: [u8; 3], n: usize) {
        for _ in 0..n {
            let off = Vec3::new(rng.f(), rng.f(), rng.f());
            let pos = cell.as_vec3() + off * 0.8 + 0.1;
            let vel = (off - 0.5) * 4.0 + Vec3::Y * rng.range(1.0, 3.0);
            let shade = rng.range(0.8, 1.1);
            let c = color.map(|v| (v as f32 * shade).min(255.0) as u8);
            self.push(P { pos, vel, life: rng.range(0.5, 1.0), max: 1.0, size: rng.range(0.06, 0.11), color: c, grav: 16.0, glow: false, shrink: false });
        }
    }

    /// Dust kicked up by feet: a landing, or a sprint.
    pub fn dust(&mut self, rng: &mut Rng, at: Vec3, color: [u8; 3], n: usize, spread: f32) {
        for _ in 0..n {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(0.2, 1.0) * spread;
            let vel = Vec3::new(a.cos() * r, rng.range(0.5, 1.6), a.sin() * r);
            self.push(P { pos: at + Vec3::new(a.cos() * 0.2, 0.05, a.sin() * 0.2), vel, life: rng.range(0.3, 0.6), max: 0.6, size: rng.range(0.05, 0.09), color, grav: 9.0, glow: false, shrink: true });
        }
    }

    /// Water thrown up where something falls in.
    pub fn splash(&mut self, rng: &mut Rng, at: Vec3, n: usize) {
        for _ in 0..n {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(0.5, 2.6);
            let vel = Vec3::new(a.cos() * r, rng.range(3.0, 6.5), a.sin() * r);
            let c = if rng.f() < 0.5 { [200, 225, 250] } else { [120, 170, 230] };
            self.push(P { pos: at, vel, life: rng.range(0.5, 0.9), max: 0.9, size: rng.range(0.05, 0.1), color: c, grav: 20.0, glow: false, shrink: true });
        }
    }

    /// Sparks off a blow that lands.
    pub fn sparks(&mut self, rng: &mut Rng, at: Vec3, n: usize) {
        for _ in 0..n {
            let d = Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.2, 1.0), rng.range(-1.0, 1.0)).normalize_or(Vec3::Y);
            let c = if rng.f() < 0.5 { [255, 240, 200] } else { [255, 200, 120] };
            self.push(P { pos: at, vel: d * rng.range(2.0, 5.0), life: rng.range(0.25, 0.45), max: 0.45, size: rng.range(0.04, 0.07), color: c, grav: 6.0, glow: true, shrink: true });
        }
    }

    /// A wisp of smoke (torches, a burning furnace, lava).
    pub fn smoke(&mut self, rng: &mut Rng, at: Vec3, dark: bool) {
        let g = if dark { rng.range(50.0, 80.0) } else { rng.range(150.0, 190.0) } as u8;
        let vel = Vec3::new(rng.range(-0.15, 0.15), rng.range(0.4, 0.8), rng.range(-0.15, 0.15));
        self.push(P { pos: at, vel, life: rng.range(1.0, 1.8), max: 1.8, size: rng.range(0.06, 0.1), color: [g, g, g], grav: -0.3, glow: false, shrink: true });
    }

    /// A flame lick or an ember (glowing).
    pub fn ember(&mut self, rng: &mut Rng, at: Vec3) {
        let vel = Vec3::new(rng.range(-0.1, 0.1), rng.range(0.2, 0.5), rng.range(-0.1, 0.1));
        self.push(P { pos: at, vel, life: rng.range(0.25, 0.5), max: 0.5, size: rng.range(0.04, 0.07), color: [255, 170, 60], grav: -0.5, glow: true, shrink: true });
    }

    /// A drop of rain landing.
    pub fn drip(&mut self, rng: &mut Rng, at: Vec3) {
        for _ in 0..2 {
            let vel = Vec3::new(rng.range(-0.8, 0.8), rng.range(1.0, 2.0), rng.range(-0.8, 0.8));
            self.push(P { pos: at, vel, life: 0.25, max: 0.25, size: 0.04, color: [170, 195, 230], grav: 14.0, glow: false, shrink: true });
        }
    }

    pub fn update(&mut self, world: &World, dt: f32) {
        self.list.retain_mut(|p| {
            p.life -= dt;
            if p.life <= 0.0 {
                return false;
            }
            p.vel.y -= p.grav * dt;
            let next = p.pos + p.vel * dt;
            let c = next.floor().as_ivec3();
            if p.grav > 0.0 && is_solid(world.get_block(c.x, c.y, c.z)) {
                // came to rest on (or against) a block
                if p.vel.y < 0.0 && p.pos.y >= c.y as f32 + 0.99 {
                    p.pos.y = c.y as f32 + 1.0 + p.size * 0.5;
                }
                p.vel = Vec3::ZERO;
                p.grav = 0.0;
            } else {
                p.pos = next;
                p.vel *= 1.0 - (dt * 1.2).min(0.5);
            }
            true
        });
    }

    pub fn draw(&self, out: &mut Vec<EntVert>, eye: Vec3, right: Vec3, up: Vec3) {
        for p in &self.list {
            let rel = p.pos - eye;
            if rel.length_squared() > 64.0 * 64.0 {
                continue;
            }
            let s = if p.shrink { p.size * (p.life / p.max).clamp(0.25, 1.0) } else { p.size };
            quad(out, rel, right * s, up * s, p.color, if p.glow { gfx::EMISSIVE } else { 255 });
        }
    }
}

/// A flat square at `c` spanning `r` and `u` either way (both half-extents).
#[inline]
pub fn quad(out: &mut Vec<EntVert>, c: Vec3, r: Vec3, u: Vec3, color: [u8; 3], alpha: u8) {
    let col = [color[0], color[1], color[2], alpha];
    let q = [c - r - u, c + r - u, c + r + u, c - r + u];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(EntVert { pos: q[i].into(), color: col });
    }
}

// --- weather -------------------------------------------------------------------------

/// Rain comes and goes on its own; now and then it is a thunderstorm.
pub struct Weather {
    /// how hard it is raining now, 0..1 (eased towards `want`)
    pub rain: f32,
    pub want: f32,
    pub storm: bool,
    /// seconds until the weather changes
    pub timer: f32,
    /// bright flash of lightning, fading
    pub flash: f32,
    bolt_in: f32,
    /// thunder heard after its flash: seconds until it arrives
    pub thunder_in: Option<f32>,
    /// the top of each column near the player, for where rain stops
    tops: Vec<i32>,
    tops_at: Option<IVec3>,
    pub snowy: bool,
    pub dry: bool,
    /// seconds, for the falling rain
    clock: f32,
}

impl Default for Weather {
    fn default() -> Self {
        Weather { rain: 0.0, want: 0.0, storm: false, timer: 600.0, flash: 0.0, bolt_in: 20.0, thunder_in: None, tops: Vec::new(), tops_at: None, snowy: false, dry: false, clock: 0.0 }
    }
}

const RAIN_R: i32 = 14;

impl Weather {
    /// Advances the weather. Returns true on the frame a bolt of lightning strikes.
    pub fn update(&mut self, rng: &mut Rng, dt: f32) -> bool {
        self.clock = (self.clock + dt) % 3600.0;
        self.timer -= dt;
        if self.timer <= 0.0 {
            if self.want > 0.0 {
                // clear for ten to twenty-five minutes
                self.want = 0.0;
                self.storm = false;
                self.timer = rng.range(600.0, 1500.0);
            } else {
                // rain for three to eight
                self.want = rng.range(0.6, 1.0);
                self.storm = rng.f() < 0.3;
                self.timer = rng.range(180.0, 480.0);
            }
        }
        let k = (dt / 12.0).min(1.0);
        self.rain += (self.want - self.rain) * k;
        if self.rain < 0.002 && self.want == 0.0 {
            self.rain = 0.0;
        }
        self.flash = (self.flash - dt * 2.5).max(0.0);
        if let Some(t) = &mut self.thunder_in {
            *t -= dt;
        }
        if self.storm && self.rain > 0.5 {
            self.bolt_in -= dt;
            if self.bolt_in <= 0.0 {
                self.bolt_in = rng.range(8.0, 30.0);
                self.flash = 1.0;
                self.thunder_in = Some(rng.range(0.2, 3.0));
                return true;
            }
        }
        false
    }

    /// Thunder whose time has come.
    pub fn thunder_now(&mut self) -> bool {
        if self.thunder_in.is_some_and(|t| t <= 0.0) {
            self.thunder_in = None;
            return true;
        }
        false
    }

    /// Re-reads the column tops when the player moves to another block.
    pub fn track(&mut self, world: &World, eye: Vec3) {
        let c = eye.floor().as_ivec3();
        if self.tops_at == Some(c) && !self.tops.is_empty() {
            return;
        }
        self.tops_at = Some(c);
        let n = (2 * RAIN_R + 1) as usize;
        self.tops.clear();
        self.tops.reserve(n * n);
        for dz in -RAIN_R..=RAIN_R {
            for dx in -RAIN_R..=RAIN_R {
                self.tops.push(world.surface_y(c.x + dx, c.z + dz).unwrap_or(i32::MAX));
            }
        }
    }

    /// Forget the tops (a block was placed or broken).
    pub fn dirty(&mut self) {
        self.tops_at = None;
    }

    /// Whether rain falls on this spot (no roof above it).
    pub fn open_sky(&self, at: Vec3) -> bool {
        let Some(c) = self.tops_at else { return false };
        let (dx, dz) = (at.x.floor() as i32 - c.x, at.z.floor() as i32 - c.z);
        if dx.abs() > RAIN_R || dz.abs() > RAIN_R || self.tops.is_empty() {
            return false;
        }
        let top = self.tops[((dz + RAIN_R) * (2 * RAIN_R + 1) + dx + RAIN_R) as usize];
        at.y >= top as f32 + 1.0
    }

    /// Streaks of rain (or flakes of snow) in the columns around the eye, and the spots
    /// where drops land (for splashes).
    pub fn draw(&self, out: &mut Vec<EntVert>, eye: Vec3, landings: &mut Vec<Vec3>) {
        let time = self.clock;
        if self.rain < 0.02 || self.dry || self.tops.is_empty() {
            return;
        }
        let Some(c) = self.tops_at else { return };
        let n = 2 * RAIN_R + 1;
        let alpha = (self.rain * if self.snowy { 230.0 } else { 150.0 }) as u8;
        for dz in -RAIN_R..=RAIN_R {
            for dx in -RAIN_R..=RAIN_R {
                let d2 = dx * dx + dz * dz;
                if d2 > RAIN_R * RAIN_R {
                    continue;
                }
                let (x, z) = (c.x + dx, c.z + dz);
                let h = hash(x, z);
                // fewer columns when it rains lightly, and fewer far off
                if (h & 255) as f32 / 255.0 > self.rain * (1.0 - d2 as f32 / (RAIN_R * RAIN_R) as f32 * 0.4) {
                    continue;
                }
                let top = self.tops[((dz + RAIN_R) * n + dx + RAIN_R) as usize];
                if top == i32::MAX {
                    continue;
                }
                let floor = top as f32 + 1.0;
                let span = 24.0;
                let ceil = eye.y + 14.0;
                for k in 0..2u32 {
                    let hk = h.wrapping_mul(2654435761).wrapping_add(k * 977);
                    let fx = x as f32 + 0.15 + (hk & 1023) as f32 / 1023.0 * 0.7;
                    let fz = z as f32 + 0.15 + ((hk >> 10) & 1023) as f32 / 1023.0 * 0.7;
                    let (speed, len) = if self.snowy { (2.2, 0.09) } else { (17.0, 0.55) };
                    let phase = ((hk >> 20) & 1023) as f32 / 1023.0;
                    let y = ceil - ((time * speed / span + phase).fract()) * span;
                    if y < floor {
                        // this drop has just landed
                        if !self.snowy && y > floor - speed * 0.02 && (eye.y - floor).abs() < 12.0 {
                            landings.push(Vec3::new(fx, floor, fz));
                        }
                        continue;
                    }
                    let mut p = Vec3::new(fx, y, fz) - eye;
                    if self.snowy {
                        // flakes drift
                        p.x += (time * 0.9 + phase * 6.0).sin() * 0.3;
                        p.z += (time * 0.7 + phase * 4.0).cos() * 0.3;
                        let side = Vec3::new(-p.z, 0.0, p.x).normalize_or(Vec3::X) * len;
                        quad(out, p, side, Vec3::Y * len, [240, 244, 250], alpha);
                    } else {
                        // a thin streak that always faces the eye, leaning with the wind
                        let side = Vec3::new(-p.z, 0.0, p.x).normalize_or(Vec3::X) * 0.012;
                        let up = Vec3::new(0.05, 1.0, 0.0) * len;
                        quad(out, p, side, up, [170, 190, 220], alpha);
                    }
                }
            }
        }
    }
}

#[inline]
fn hash(x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^ (h >> 16)
}

// --- experience ----------------------------------------------------------------------

pub struct Orb {
    pos: Vec3,
    vel: Vec3,
    pub value: u32,
    age: f32,
}

#[derive(Default)]
pub struct Orbs {
    pub list: Vec<Orb>,
}

impl Orbs {
    /// Scatters `total` experience as a few orbs.
    pub fn spawn(&mut self, rng: &mut Rng, at: Vec3, mut total: u32) {
        while total > 0 && self.list.len() < 300 {
            let v = match total {
                0..=2 => total,
                3..=10 => 3,
                11..=30 => 7,
                _ => 17,
            };
            total -= v;
            let vel = Vec3::new(rng.range(-1.5, 1.5), rng.range(2.5, 4.5), rng.range(-1.5, 1.5));
            self.list.push(Orb { pos: at, vel, value: v, age: rng.range(0.0, 3.0) });
        }
    }

    /// Orbs float to a player within reach; returns the experience picked up.
    pub fn update(&mut self, world: &World, player: Option<Vec3>, dt: f32) -> u32 {
        let mut got = 0;
        self.list.retain_mut(|o| {
            o.age += dt;
            if o.age > 300.0 {
                return false;
            }
            let mut pull = false;
            if let Some(p) = player {
                let to = p + Vec3::Y * 0.8 - o.pos;
                let d = to.length();
                if d < 0.7 && o.age > 0.6 {
                    got += o.value;
                    return false;
                }
                if d < 7.0 && o.age > 0.4 {
                    pull = true;
                    let k = 1.0 - d / 7.0;
                    o.vel += to / d.max(0.1) * (28.0 * k * k + 4.0) * dt;
                    o.vel *= 1.0 - (dt * 3.0).min(0.6);
                }
            }
            if !pull {
                o.vel.y -= 12.0 * dt;
                o.vel.x *= 1.0 - (dt * 2.0).min(0.5);
                o.vel.z *= 1.0 - (dt * 2.0).min(0.5);
            }
            let next = o.pos + o.vel * dt;
            let c = next.floor().as_ivec3();
            let b = world.get_block(c.x, c.y, c.z);
            if !pull && is_solid(b) {
                o.vel = Vec3::ZERO;
            } else {
                o.pos = next;
                if b == WATER {
                    o.vel.y = o.vel.y.max(0.8);
                }
            }
            true
        });
        got
    }

    pub fn draw(&self, out: &mut Vec<EntVert>, eye: Vec3, right: Vec3, up: Vec3) {
        for o in &self.list {
            let rel = o.pos - eye + Vec3::Y * (0.12 + (o.age * 3.0).sin() * 0.05);
            if rel.length_squared() > 48.0 * 48.0 {
                continue;
            }
            let s = 0.07 + (o.value as f32).sqrt() * 0.02;
            let t = (o.age * 4.0).sin() * 0.5 + 0.5;
            let c = [(140.0 + 110.0 * t) as u8, 250, (60.0 + 40.0 * t) as u8];
            quad(out, rel, right * s, up * s, c, gfx::EMISSIVE);
            quad(out, rel - (right.cross(up)) * 0.01, right * s * 0.55, up * s * 0.55, [255, 255, 210], gfx::EMISSIVE);
        }
    }
}

/// Experience needed to go from `level` to the next.
pub fn xp_to_next(level: u32) -> u32 {
    10 + 4 * level
}
