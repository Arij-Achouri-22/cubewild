//! Player movement, AABB-vs-voxel physics (shared with animals), and ray casting.

use crate::world::{fluid_height, is_fence, is_lava, is_ladder, is_shaped, is_solid, is_target, is_torch, is_water, Block, World};
use glam::{IVec3, Vec3};

pub fn collides(world: &World, pos: Vec3, hw: f32, h: f32) -> bool {
    // Unknown terrain is a boundary until its chunk arrives, including in flight.
    if !world.area_loaded(pos.x, pos.z, hw) { return true; }
    let (x0, x1) = ((pos.x - hw).floor() as i32, (pos.x + hw).floor() as i32);
    let (y0, y1) = (pos.y.floor() as i32, (pos.y + h).floor() as i32);
    let (z0, z1) = ((pos.z - hw).floor() as i32, (pos.z + hw).floor() as i32);
    // one level further down: a fence stands half a block taller than its cell
    for y in y0 - 1..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                let b = world.get_block(x, y, z);
                if !is_solid(b) {
                    continue;
                }
                if !is_shaped(b) {
                    if y >= y0 {
                        return true;
                    }
                } else if (y >= y0 || is_fence(b)) && shape_overlaps(world, b, x, y, z, pos, hw, h) {
                    return true;
                }
            }
        }
    }
    false
}

/// True if a body of half-width `hw` and height `h` standing at `pos` reaches into a
/// shaped block (door, pane, slab, stairs, fence...).
fn shape_overlaps(world: &World, b: u8, x: i32, y: i32, z: i32, pos: Vec3, hw: f32, h: f32) -> bool {
    let (boxes, n) = world.shape_at(b, x, y, z);
    boxes[..n].iter().any(|q| {
        pos.x + hw > q[0] && pos.x - hw < q[3] && pos.y + h > q[1] && pos.y < q[4] && pos.z + hw > q[2] && pos.z - hw < q[5]
    })
}

/// The part of its cell a block fills, in world coordinates, for the selection outline.
pub fn block_bounds(world: &World, p: IVec3) -> (Vec3, Vec3) {
    let b = world.get_block(p.x, p.y, p.z);
    if !is_shaped(b) {
        return (p.as_vec3(), p.as_vec3() + Vec3::ONE);
    }
    let (boxes, n) = world.shape_at(b, p.x, p.y, p.z);
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    for q in &boxes[..n] {
        lo = lo.min(Vec3::new(q[0], q[1], q[2]));
        hi = hi.max(Vec3::new(q[3], q[4], q[5]));
    }
    (lo, hi)
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

/// height a walker steps up without jumping: slabs and stairs
pub const STEP: f32 = 0.6;

/// `move_collide` for walkers: one that is on the ground (`grounded`) and runs into a
/// ledge no higher than `STEP` walks up onto it.
pub fn move_step(world: &World, pos: &mut Vec3, vel: &mut Vec3, hw: f32, h: f32, dt: f32, grounded: bool) -> (bool, bool) {
    let (p0, v0) = (*pos, *vel);
    let (ground, wall) = move_collide(world, pos, vel, hw, h, dt);
    if !wall || !(grounded || ground) || v0.y > 0.5 {
        return (ground, wall);
    }
    // the same move again from a step higher, then back down onto whatever is there
    let mut p = p0 + Vec3::Y * STEP;
    if collides(world, p, hw, h) {
        return (ground, wall);
    }
    let mut v = Vec3::new(v0.x, 0.0, v0.z);
    let (_, wall2) = move_collide(world, &mut p, &mut v, hw, h, dt);
    let mut down = Vec3::new(0.0, -(STEP + 0.05) / dt, 0.0);
    let (landed, _) = move_collide(world, &mut p, &mut down, hw, h, dt);
    let gain = |q: Vec3| (q.x - p0.x).powi(2) + (q.z - p0.z).powi(2);
    if !landed || p.y < p0.y + 0.05 || gain(p) <= gain(*pos) + 1e-6 {
        return (ground, wall);
    }
    *pos = p;
    *vel = Vec3::new(v.x, 0.0, v.z);
    (true, wall2)
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
        let b = world.get_block(p.x, p.y, p.z);
        if hit(b) {
            if !is_shaped(b) && !is_torch(b) {
                return Some((p, normal, t));
            }
            // doors, slabs and the like are only hit where they actually are: you can
            // aim past them (and the face hit is that of the box, not of the cell)
            let (boxes, n) = world.shape_at(b, p.x, p.y, p.z);
            let mut near = (f32::MAX, normal);
            for q in &boxes[..n] {
                let (lo, hi) = (Vec3::new(q[0], q[1], q[2]), Vec3::new(q[3], q[4].min(p.y as f32 + 1.0), q[5]));
                if let Some(t) = ray_aabb(o, d, lo, hi).filter(|&t| t < near.0) {
                    let h = o + d * t;
                    let mut nn = normal;
                    for a in 0..3 {
                        if (h[a] - lo[a]).abs() < 1e-3 || (h[a] - hi[a]).abs() < 1e-3 {
                            nn = IVec3::ZERO;
                            nn[a] = -step[a];
                        }
                    }
                    near = (t, nn);
                }
            }
            if near.0 <= max {
                return Some((p, near.1, near.0));
            }
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
/// lying flat while swimming fast (or crawling where there is no room to stand)
pub const SWIM_H: f32 = 0.6;
const SWIM_EYE: f32 = 0.42;

#[derive(Clone)]
pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub flying: bool,
    /// wading or swimming, in water or in lava
    pub in_water: bool,
    pub in_lava: bool,
    /// downward speed of this frame's landing (0 if none), for fall damage
    pub impact: f32,
    /// ran into something on the last move (walking into a ladder climbs it)
    wall: bool,
    /// how far the eye still lags behind after stepping up a ledge
    lift: f32,
    /// sprint-swimming: lying flat, moving where you look
    pub swimming: bool,
    /// eye height, eased between standing and lying
    eye_h: f32,
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
            in_lava: false,
            impact: 0.0,
            wall: false,
            lift: 0.0,
            swimming: false,
            eye_h: EYE,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, self.eye_h - self.lift, 0.0)
    }

    /// Height of the body: less while lying flat.
    pub fn height(&self) -> f32 {
        if self.swimming { SWIM_H } else { PLAYER_H }
    }

    pub fn look_dir(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, cy * cp)
    }

    pub fn eye_in_lava(&self, world: &World) -> bool {
        self.eye_under(world, is_lava)
    }

    pub fn eye_in_water(&self, world: &World) -> bool {
        self.eye_under(world, is_water)
    }

    /// The eyes are under the surface of a fluid (running water is shallow: its surface
    /// stands lower in the cell).
    fn eye_under(&self, world: &World, fluid: fn(Block) -> bool) -> bool {
        let eye = self.eye();
        let e = eye.floor().as_ivec3();
        let b = world.get_block(e.x, e.y, e.z);
        fluid(b) && (fluid(world.get_block(e.x, e.y + 1, e.z)) || eye.y - e.y as f32 <= fluid_height(b) - 0.06)
    }

    pub fn update(&mut self, world: &World, inp: &Input, dt: f32) {
        if !world.area_loaded(self.pos.x, self.pos.z, PLAYER_HW) {
            self.vel = Vec3::ZERO;
            self.impact = 0.0;
            return;
        }
        let (sy, cy) = self.yaw.sin_cos();
        let fwd = Vec3::new(sy, 0.0, cy);
        let right = Vec3::new(-cy, 0.0, sy);
        let mut wish = fwd * inp.forward + right * inp.right;
        if wish.length_squared() > 1.0 {
            wish = wish.normalize();
        }
        let b = self.pos.floor().as_ivec3();
        let around = world.get_block(b.x, (self.pos.y + if self.swimming { 0.3 } else { 0.4 }).floor() as i32, b.z);
        self.in_lava = is_lava(around);
        self.in_water = is_water(around) || self.in_lava;
        // running water carries you along
        let flow = if self.in_water && !self.flying {
            crate::fluid::current(world, IVec3::new(b.x, (self.pos.y + 0.4).floor() as i32, b.z)) * if self.in_lava { 0.8 } else { 2.4 }
        } else {
            Vec3::ZERO
        };
        // Ctrl + forward in water: lie flat and swim. Coming out of it needs room to stand.
        let want = self.in_water && !self.in_lava && !self.flying && inp.sprint && inp.forward > 0.0;
        if want {
            self.swimming = true;
        } else if self.swimming && !collides(world, self.pos, PLAYER_HW, PLAYER_H) {
            self.swimming = false;
        }
        let eye_want = if self.swimming { SWIM_EYE } else { EYE };
        self.eye_h += (eye_want - self.eye_h) * (1.0 - (-dt * 10.0).exp());

        if self.flying {
            let speed = if inp.sprint { 28.0 } else { 11.0 };
            let mut target = wish * speed;
            target.y = (inp.jump as i32 - inp.sneak as i32) as f32 * speed * 0.7;
            let k = 1.0 - (-dt * 10.0).exp();
            self.vel += (target - self.vel) * k;
        } else if self.swimming && self.in_water {
            // swim the way you look: down to dive, up to rise
            let look = self.look_dir();
            let mut target = look * inp.forward.max(0.0) * 5.4 + right * inp.right * 2.0 + flow;
            if inp.jump {
                target.y = target.y.max(2.5);
            }
            let k = 1.0 - (-dt * 4.0).exp();
            self.vel += (target - self.vel) * k;
        } else if self.in_water {
            // (lava is thick: half the pace of water)
            let target = wish * if inp.sprint { 3.6 } else { 2.6 } * if self.in_lava { 0.5 } else { 1.0 } + flow;
            let k = 1.0 - (-dt * 6.0).exp();
            self.vel.x += (target.x - self.vel.x) * k;
            self.vel.z += (target.z - self.vel.z) * k;
            self.vel.y -= 9.0 * dt;
            if inp.jump {
                self.vel.y += 26.0 * dt;
            }
            self.vel.y = self.vel.y.clamp(-3.5, 4.0);
        } else {
            let speed = if self.swimming {
                // crawling through a gap too low to stand in
                1.3
            } else if inp.sneak {
                1.7
            } else if inp.sprint {
                6.6
            } else {
                4.4
            };
            // ash is heavy going
            let feet = world.get_block(b.x, (self.pos.y - 0.1).floor() as i32, b.z);
            let speed = if feet == crate::world::ASH { speed * 0.6 } else { speed };
            let target = wish * speed;
            let k = 1.0 - (-dt * if self.on_ground { 14.0 } else { 3.5 }).exp();
            self.vel.x += (target.x - self.vel.x) * k;
            self.vel.z += (target.z - self.vel.z) * k;
            if inp.sneak && self.on_ground {
                // sneaking never steps off an edge: a move that would leave nothing
                // under the feet is held back, one axis at a time
                let held = |p: Vec3| collides(world, p - Vec3::Y * 0.6, PLAYER_HW, 0.55);
                if !held(self.pos + Vec3::new(self.vel.x * dt, 0.0, 0.0)) {
                    self.vel.x = 0.0;
                }
                if !held(self.pos + Vec3::new(0.0, 0.0, self.vel.z * dt)) {
                    self.vel.z = 0.0;
                }
            }
            self.vel.y = (self.vel.y - 29.0 * dt).max(-55.0);
            if inp.jump && self.on_ground {
                self.vel.y = 8.8;
            }
            if is_ladder(world.get_block(b.x, (self.pos.y + 0.1).floor() as i32, b.z)) {
                // on a ladder: slide down slowly, hold on while sneaking, and climb by
                // walking into the wall behind it (or jumping)
                self.vel.y = self.vel.y.max(if inp.sneak { 0.0 } else { -2.6 });
                if inp.jump || (self.wall && wish != Vec3::ZERO) {
                    self.vel.y = 2.6;
                }
            }
        }
        let was_water = self.in_water;
        let fall = -self.vel.y;
        let y0 = self.pos.y;
        let walking = self.on_ground && !self.flying;
        let h = self.height();
        let (ground, wall) = move_step(world, &mut self.pos, &mut self.vel, PLAYER_HW, h, dt, walking);
        // the eye follows a step up smoothly instead of jumping with the feet
        if walking && ground && self.pos.y > y0 + 0.05 {
            self.lift = (self.lift + self.pos.y - y0).min(1.0);
        }
        self.lift *= (-dt * 14.0).exp();
        self.wall = wall;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::*;

    #[test]
    fn walking_and_flying_stop_at_unloaded_chunks_and_resume_when_loaded() {
        for (start_x, direction, missing) in [(14.5, 1.0, (1, 0)), (1.5, -1.0, (-1, 0))] {
            for flying in [false, true] {
                let mut world = World::flat(|_, _, _| None);
                let chunk = world.chunks.remove(&missing).unwrap();
                let y = if flying { 20.0 } else { 10.0 };
                let mut p = Player::new(Vec3::new(start_x, y, 0.5));
                p.flying = flying;
                p.yaw = direction * std::f32::consts::FRAC_PI_2;
                let inp = Input { forward: 1.0, sprint: true, ..Default::default() };
                for _ in 0..240 { p.update(&world, &inp, 1.0 / 120.0); }
                assert!(world.area_loaded(p.pos.x, p.pos.z, PLAYER_HW), "{flying}: {}", p.pos);
                assert!((p.pos.y - y).abs() < 0.01, "no fall at an unloaded boundary: {}", p.pos);
                assert!(p.impact < 1.0);
                let boundary = p.pos.x;
                world.chunks.insert(missing, chunk);
                for _ in 0..30 { p.update(&world, &inp, 1.0 / 120.0); }
                assert!((p.pos.x - boundary) * direction > 0.5, "movement resumes: {}", p.pos);
            }
        }
    }

    #[test]
    fn a_player_whose_chunk_is_missing_waits_without_fall_damage() {
        let mut world = World::flat(|_, _, _| None);
        world.chunks.remove(&(0, 0));
        let pos = Vec3::new(0.5, 30.0, 0.5);
        let mut p = Player::new(pos);
        p.vel = Vec3::new(12.0, -55.0, 0.0);
        for _ in 0..60 { p.update(&world, &Input::default(), 0.05); }
        assert_eq!(p.pos, pos);
        assert_eq!(p.vel, Vec3::ZERO);
        assert_eq!(p.impact, 0.0);
    }

    /// Walks a player east from (1.5, 10, 0.5) for `secs` and returns where it ends up.
    fn walk(world: &World, secs: f32, jump: bool) -> Vec3 {
        let mut p = Player::new(Vec3::new(1.5, 10.0, 0.5));
        p.yaw = std::f32::consts::FRAC_PI_2;
        let inp = Input { forward: 1.0, jump, ..Default::default() };
        for _ in 0..(secs * 120.0) as usize {
            p.update(world, &inp, 1.0 / 120.0);
        }
        p.pos
    }

    #[test]
    fn stairs_and_slabs_are_walked_up_without_jumping() {
        // a block-high terrace from x = 4 on
        let terrace = |front: Option<Block>| World::flat(move |x, y, _| if y == 10 && x >= 4 { Some(STONE) } else if y == 10 && x == 3 { front } else { None });
        let blocked = walk(&terrace(None), 2.0, false);
        assert!(blocked.x < 3.71 && blocked.y < 10.01, "a full block stops a walker: {blocked}");
        // stairs with the high step to the east, then a slab in front of the terrace
        for front in [STAIR_BASE + 1 + 16, SLAB + 2] {
            let up = walk(&terrace(Some(front)), 2.0, false);
            assert!(up.x > 5.0 && (up.y - 11.0).abs() < 0.01, "walks up over block {front}: {up}");
        }
        // an upper slab is a wall with a gap under it too low to enter
        let low = walk(&terrace(Some(SLAB_TOP)), 2.0, false);
        assert!(low.x < 3.0, "{low}");
        // standing heights
        let w = terrace(Some(SLAB));
        assert!(collides(&w, Vec3::new(3.5, 10.49, 0.5), 0.3, 1.8) && !collides(&w, Vec3::new(3.5, 10.5, 0.5), 0.3, 1.8));
    }

    #[test]
    fn sprint_swimming_lies_flat_and_dives_where_you_look() {
        // deep water from y 2 to 20
        let w = World::flat(|_, y, _| (2..=20).contains(&y).then_some(crate::world::WATER));
        let mut p = Player::new(Vec3::new(1.5, 15.0, 0.5));
        (p.yaw, p.pitch) = (std::f32::consts::FRAC_PI_2, -0.6);
        let inp = Input { forward: 1.0, sprint: true, ..Default::default() };
        for _ in 0..120 {
            p.update(&w, &inp, 1.0 / 120.0);
        }
        assert!(p.swimming && p.height() < 1.0);
        assert!(p.pos.y < 13.5 && p.pos.x > 4.0, "dives ahead: {}", p.pos);
        assert!((p.eye().y - p.pos.y) < 0.6, "the eye is low while lying flat");
        // letting go of Ctrl stands back up in open water
        for _ in 0..120 {
            p.update(&w, &Input::default(), 1.0 / 120.0);
        }
        assert!(!p.swimming);
    }

    #[test]
    fn sneaking_stops_at_an_edge() {
        // a ledge: ground up to x = 4, a drop beyond
        let w = World::flat(move |x, y, _| (y <= 9 && x >= 4).then_some(AIR));
        let run = |sneak: bool| {
            let mut p = Player::new(Vec3::new(1.5, 10.0, 0.5));
            p.yaw = std::f32::consts::FRAC_PI_2;
            let inp = Input { forward: 1.0, sneak, ..Default::default() };
            for _ in 0..(4.0 * 120.0) as usize {
                p.update(&w, &inp, 1.0 / 120.0);
            }
            p.pos
        };
        let careful = run(true);
        assert!(careful.y > 9.99 && careful.x < 4.0 + PLAYER_HW + 0.01, "{careful}");
        assert!(run(false).y < 9.0);
    }

    #[test]
    fn a_fence_cannot_be_jumped_but_a_block_can() {
        let line = |b: Block| World::flat(move |x, y, _| (y == 10 && x == 4).then_some(b));
        let over = walk(&line(STONE), 3.0, true);
        assert!(over.x > 6.0, "{over}");
        let held = walk(&line(FENCE), 3.0, true);
        assert!(held.x < 4.2, "{held}");
        // it is only as thick as its post and rails, though
        assert!(held.x > 4.0);
    }

    #[test]
    fn ladders_are_climbed_by_walking_into_them() {
        // a cliff six high from x = 4 on, with a ladder up its west face
        let wall = |ladder: bool| {
            World::flat(move |x, y, z| match (x, y) {
                (4.., 10..=15) => Some(STONE),
                (3, 10..=15) if ladder && z == 0 => Some(LADDER_BASE + 1),
                _ => None,
            })
        };
        assert!(walk(&wall(false), 5.0, false).y < 10.01);
        let top = walk(&wall(true), 5.0, false);
        assert!(top.x > 4.0 && (top.y - 16.0).abs() < 0.01, "{top}");
        // letting go half way up slides down gently: no fall damage
        let w = wall(true);
        let mut p = Player::new(Vec3::new(3.5, 14.0, 0.5));
        let mut worst = 0.0f32;
        for _ in 0..600 {
            p.update(&w, &Input::default(), 1.0 / 120.0);
            worst = worst.max(p.impact);
        }
        assert!(p.on_ground && worst < 3.0, "{worst}");
        // the target outline and the crosshair find the ladder, not the cell around it
        let hit = raycast(&w, Vec3::new(1.5, 11.5, 0.5), Vec3::X, 6.0).unwrap();
        assert_eq!(hit.0, IVec3::new(3, 11, 0));
        assert!((hit.2 - (2.5 - DOOR_THICK)).abs() < 1e-3);
        // looking down at a slab hits its top face, half way down the cell
        let w = World::flat(|x, y, z| (x == 2 && y == 10 && z == 0).then_some(SLAB));
        let hit = raycast(&w, Vec3::new(2.5, 12.0, 0.5), Vec3::NEG_Y, 6.0).unwrap();
        assert_eq!((hit.0, hit.1), (IVec3::new(2, 10, 0), IVec3::Y));
        assert!((hit.2 - 1.5).abs() < 1e-3);
    }
}
