//! Player movement, AABB-vs-voxel physics (shared with animals), and ray casting.

use crate::world::{is_solid, is_target, World, WATER};
use glam::{IVec3, Vec3};

pub fn collides(world: &World, pos: Vec3, hw: f32, h: f32) -> bool {
    let (x0, x1) = ((pos.x - hw).floor() as i32, (pos.x + hw).floor() as i32);
    let (y0, y1) = (pos.y.floor() as i32, (pos.y + h).floor() as i32);
    let (z0, z1) = ((pos.z - hw).floor() as i32, (pos.z + hw).floor() as i32);
    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                if is_solid(world.get_block(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Moves an AABB (feet-centre `pos`) by `vel * dt`, sliding along blocks.
/// Returns (on_ground, hit_wall).
pub fn move_collide(
    world: &World,
    pos: &mut Vec3,
    vel: &mut Vec3,
    hw: f32,
    h: f32,
    dt: f32,
) -> (bool, bool) {
    let d = *vel * dt;
    let steps = (d.abs().max_element() / 0.35).ceil().max(1.0) as i32;
    let sd = d / steps as f32;
    let (mut ground, mut wall) = (false, false);
    for _ in 0..steps {
        for axis in [1usize, 0, 2] {
            let delta = sd[axis];
            if delta == 0.0 {
                continue;
            }
            let old = pos[axis];
            pos[axis] = old + delta;
            if collides(world, *pos, hw, h) {
                // bisect to get flush against the surface
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..6 {
                    let mid = (lo + hi) * 0.5;
                    pos[axis] = old + delta * mid;
                    if collides(world, *pos, hw, h) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                pos[axis] = old + delta * lo;
                if axis == 1 {
                    if delta < 0.0 {
                        ground = true;
                    }
                } else {
                    wall = true;
                }
                vel[axis] = 0.0;
            }
        }
    }
    (ground, wall)
}

/// Voxel DDA. Returns (hit block, face normal, distance).
pub fn raycast(world: &World, o: Vec3, d: Vec3, max: f32) -> Option<(IVec3, IVec3, f32)> {
    raycast_with(world, o, d, max, is_target)
}

/// Like `raycast`, stopping at the first block for which `hit` is true.
pub fn raycast_with(world: &World, o: Vec3, d: Vec3, max: f32, hit: impl Fn(u8) -> bool) -> Option<(IVec3, IVec3, f32)> {
    let mut p = o.floor().as_ivec3();
    let step = IVec3::new(sgn(d.x), sgn(d.y), sgn(d.z));
    let inv = Vec3::new(1.0 / d.x.abs(), 1.0 / d.y.abs(), 1.0 / d.z.abs());
    let frac = |o: f32, p: i32, s: i32| if s > 0 { p as f32 + 1.0 - o } else { o - p as f32 };
    let mut t_max = Vec3::new(
        frac(o.x, p.x, step.x) * inv.x,
        frac(o.y, p.y, step.y) * inv.y,
        frac(o.z, p.z, step.z) * inv.z,
    );
    let mut normal = IVec3::ZERO;
    let mut t = 0.0;
    while t <= max {
        if hit(world.get_block(p.x, p.y, p.z)) {
            return Some((p, normal, t));
        }
        let axis = if t_max.x < t_max.y && t_max.x < t_max.z {
            0
        } else if t_max.y < t_max.z {
            1
        } else {
            2
        };
        t = t_max[axis];
        t_max[axis] += inv[axis];
        p[axis] += step[axis];
        normal = IVec3::ZERO;
        normal[axis] = -step[axis];
    }
    None
}

fn sgn(v: f32) -> i32 {
    if v > 0.0 {
        1
    } else {
        -1
    }
}

/// Ray vs axis-aligned box; returns entry distance.
pub fn ray_aabb(o: Vec3, d: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = Vec3::ONE / d;
    let t1 = (min - o) * inv;
    let t2 = (max - o) * inv;
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    if far >= near.max(0.0) {
        Some(near.max(0.0))
    } else {
        None
    }
}

#[derive(Default, Clone, Copy)]
pub struct Input {
    pub forward: f32,
    pub right: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

pub const PLAYER_HW: f32 = 0.3;
pub const PLAYER_H: f32 = 1.8;
const EYE: f32 = 1.62;

pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub flying: bool,
    pub in_water: bool,
    /// downward speed of this frame's landing (0 if none), for fall damage
    pub impact: f32,
}

impl Player {
    pub fn new(pos: Vec3) -> Player {
        Player {
            pos,
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            flying: false,
            in_water: false,
            impact: 0.0,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, EYE, 0.0)
    }

    pub fn look_dir(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, cy * cp)
    }

    pub fn eye_in_water(&self, world: &World) -> bool {
        let e = self.eye().floor().as_ivec3();
        world.get_block(e.x, e.y, e.z) == WATER
    }

    pub fn update(&mut self, world: &World, inp: &Input, dt: f32) {
        let (sy, cy) = self.yaw.sin_cos();
        let fwd = Vec3::new(sy, 0.0, cy);
        let right = Vec3::new(-cy, 0.0, sy);
        let mut wish = fwd * inp.forward + right * inp.right;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }
        let b = self.pos.floor().as_ivec3();
        self.in_water = world.get_block(b.x, (self.pos.y + 0.4).floor() as i32, b.z) == WATER;

        if self.flying {
            let speed = if inp.sprint { 28.0 } else { 11.0 };
            let mut target = wish * speed;
            target.y = (inp.jump as i32 - inp.sneak as i32) as f32 * speed * 0.7;
            let k = 1.0 - (-dt * 10.0).exp();
            self.vel += (target - self.vel) * k;
        } else if self.in_water {
            let target = wish * if inp.sprint { 3.6 } else { 2.6 };
            let k = 1.0 - (-dt * 6.0).exp();
            self.vel.x += (target.x - self.vel.x) * k;
            self.vel.z += (target.z - self.vel.z) * k;
            self.vel.y -= 9.0 * dt;
            if inp.jump {
                self.vel.y += 26.0 * dt;
            }
            self.vel.y = self.vel.y.clamp(-3.5, 4.0);
        } else {
            let speed = if inp.sneak {
                1.7
            } else if inp.sprint {
                6.6
            } else {
                4.4
            };
            let target = wish * speed;
            let k = 1.0 - (-dt * if self.on_ground { 14.0 } else { 3.5 }).exp();
            self.vel.x += (target.x - self.vel.x) * k;
            self.vel.z += (target.z - self.vel.z) * k;
            self.vel.y = (self.vel.y - 29.0 * dt).max(-55.0);
            if inp.jump && self.on_ground {
                self.vel.y = 8.8;
            }
        }
        let was_water = self.in_water;
        let fall = -self.vel.y;
        let (ground, wall) =
            move_collide(world, &mut self.pos, &mut self.vel, PLAYER_HW, PLAYER_H, dt);
        self.on_ground = ground;
        self.impact = if ground && !self.flying && !was_water { fall.max(0.0) } else { 0.0 };
        // hop out of water onto a bank
        if was_water && wall && inp.jump {
            self.vel.y = 6.5;
        }
        if self.flying && ground {
            self.flying = false;
        }
    }
}
