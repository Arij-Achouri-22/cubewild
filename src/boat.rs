//! Boats: placed on water from the item, ridden with WASD (W/S row, A/D turn), broken
//! back into the item with a few blows.

use crate::gfx::EntVert;
use crate::mobs::push_box;
use crate::player::{move_collide, ray_aabb, Input};
use crate::world::{is_water, World};
use glam::{Mat4, Vec3};
use std::path::Path;

/// half the width of the box a boat collides with, and its height
pub const HW: f32 = 0.55;
pub const H: f32 = 0.5;
/// blows it takes to break
const HP: f32 = 3.0;

pub struct Boat {
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
    /// rowing animation, and how hard the oars are working (0..1)
    row: f32,
    pull: f32,
    pub hp: f32,
    /// seconds of the shake after a blow
    hurt: f32,
    pub on_water: bool,
}

#[derive(Default)]
pub struct Boats {
    pub list: Vec<Boat>,
}

/// Where the boat's nose points.
pub fn forward(yaw: f32) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    Vec3::new(s, 0.0, c)
}

impl Boats {
    pub fn place(&mut self, pos: Vec3, yaw: f32) {
        self.list.push(Boat { pos, yaw, vel: Vec3::ZERO, row: 0.0, pull: 0.0, hp: HP, hurt: 0.0, on_water: false });
    }

    /// The nearest boat along a ray, and the distance to it.
    pub fn pick(&self, o: Vec3, d: Vec3, reach: f32) -> Option<(usize, f32)> {
        self.list
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let (lo, hi) = (b.pos - Vec3::new(HW, 0.0, HW), b.pos + Vec3::new(HW, H, HW));
                ray_aabb(o, d, lo, hi).filter(|&t| t <= reach).map(|t| (i, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// A blow: true if it broke.
    pub fn hit(&mut self, i: usize, damage: f32) -> bool {
        let b = &mut self.list[i];
        b.hp -= damage;
        b.hurt = 0.3;
        b.hp <= 0.0
    }

    /// Moves the boats near `near`. The ridden one (if any) takes the rider's input and
    /// returns how much it turned, so the rider's view can turn with it.
    pub fn update(&mut self, world: &World, near: Vec3, ridden: Option<(usize, &Input)>, dt: f32) -> f32 {
        let mut turned = 0.0;
        for (i, b) in self.list.iter_mut().enumerate() {
            if b.pos.distance_squared(near) > 96.0 * 96.0 || !world.is_loaded(b.pos.x.floor() as i32, b.pos.z.floor() as i32) {
                continue;
            }
            b.hurt = (b.hurt - dt).max(0.0);
            let at = |y: f32| world.get_block(b.pos.x.floor() as i32, y.floor() as i32, b.pos.z.floor() as i32);
            let sunk = is_water(at(b.pos.y + 0.05));
            let resting = is_water(at(b.pos.y - 0.25));
            b.on_water = sunk || resting;
            if sunk {
                // pushed up to the surface
                b.vel.y = (b.vel.y + 18.0 * dt).min(3.0);
            } else if resting {
                // floating: the hull sits a little into the water
                let top = (b.pos.y - 0.25).floor() + 1.0 - 0.12;
                b.vel.y = (top - b.pos.y) * 8.0;
            } else {
                b.vel.y = (b.vel.y - 22.0 * dt).max(-30.0);
            }
            let fwd = forward(b.yaw);
            let mut pull = 0.0;
            if let Some((r, inp)) = ridden.filter(|r| r.0 == i) {
                let _ = r;
                let rate = 1.9 * if inp.forward < 0.0 { -1.0 } else { 1.0 };
                let turn = -inp.right * rate * dt;
                b.yaw += turn;
                turned = turn;
                let (accel, top) = if b.on_water { (7.0, 8.0) } else { (3.0, 1.2) };
                let thrust = if inp.forward > 0.0 { inp.forward } else { inp.forward * 0.5 };
                let flat = Vec3::new(b.vel.x, 0.0, b.vel.z);
                let along = flat.dot(fwd);
                if thrust != 0.0 && (along * thrust.signum()) < top * thrust.abs() {
                    b.vel += fwd * thrust * accel * dt;
                }
                pull = (thrust.abs() + inp.right.abs() * 0.6).min(1.0);
            }
            b.pull += (pull - b.pull) * (1.0 - (-dt * 6.0).exp());
            b.row = (b.row + b.pull * dt * 1.4) % 1.0;
            // water lets a boat glide, land drags it; the keel stops it sliding sideways
            let drag = if b.on_water { 0.7 } else { 6.0 };
            let side = Vec3::new(fwd.z, 0.0, -fwd.x);
            let (along, across) = (b.vel.dot(fwd), b.vel.dot(side));
            let along = along * (-dt * drag).exp();
            let across = across * (-dt * (drag + 4.0)).exp();
            b.vel = fwd * along + side * across + Vec3::Y * b.vel.y;
            move_collide(world, &mut b.pos, &mut b.vel, HW, H, dt);
        }
        turned
    }

    pub fn draw(&self, out: &mut Vec<EntVert>, eye: Vec3) {
        const WOOD: [u8; 3] = [168, 124, 72];
        const DARK: [u8; 3] = [120, 84, 48];
        for b in &self.list {
            if b.pos.distance_squared(eye) > 110.0 * 110.0 {
                continue;
            }
            let shake = if b.hurt > 0.0 { (b.hurt * 60.0).sin() * 0.12 * b.hurt / 0.3 } else { 0.0 };
            let m = Mat4::from_translation(b.pos - eye) * Mat4::from_rotation_y(b.yaw) * Mat4::from_rotation_z(shake);
            let bx = |out: &mut Vec<EntVert>, a: [f32; 3], c: [f32; 3], col: [u8; 3]| push_box(out, &m, Vec3::from(a), Vec3::from(c), col, 255);
            let (w, l, t) = (0.55, 0.85, 0.09);
            // hull: floor, sides, bow and stern, and a seat across the middle
            bx(out, [-w, 0.0, -l], [w, 0.1, l], DARK);
            bx(out, [-w, 0.1, -l], [-w + t, 0.5, l], WOOD);
            bx(out, [w - t, 0.1, -l], [w, 0.5, l], WOOD);
            bx(out, [-w + t, 0.1, l - t], [w - t, 0.5, l], WOOD);
            bx(out, [-w + t, 0.1, -l], [w - t, 0.45, -l + t], WOOD);
            bx(out, [-w + t, 0.28, -0.12], [w - t, 0.34, 0.12], DARK);
            // oars, swinging as they row
            let a = (b.row * std::f32::consts::TAU).sin() * 0.6 * b.pull;
            for side in [-1.0f32, 1.0] {
                let oar = m * Mat4::from_translation(Vec3::new(side * (w + 0.02), 0.45, 0.05)) * Mat4::from_rotation_x(a) * Mat4::from_rotation_z(side * 0.5);
                push_box(out, &oar, Vec3::new(-0.03, -0.03, -0.03), Vec3::new(0.03, 0.03, 0.03) + Vec3::new(side * 0.0, 0.0, 0.0), DARK, 255);
                push_box(out, &oar, Vec3::new(if side < 0.0 { -0.9 } else { 0.0 }, -0.025, -0.025), Vec3::new(if side < 0.0 { 0.0 } else { 0.9 }, 0.025, 0.025), WOOD, 255);
                let blade = if side < 0.0 { -0.9 } else { 0.62 };
                push_box(out, &oar, Vec3::new(blade, -0.015, -0.1), Vec3::new(blade + 0.28, 0.015, 0.1), WOOD, 255);
            }
        }
    }

    pub fn encode(&self) -> String {
        self.list.iter().map(|b| format!("{} {} {} {}\n", b.pos.x, b.pos.y, b.pos.z, b.yaw)).collect()
    }

    pub fn load(path: &Path) -> Boats {
        let mut boats = Boats::default();
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let v: Vec<f32> = line.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if v.len() >= 4 && v.iter().all(|x| x.is_finite()) {
                boats.place(Vec3::new(v[0], v[1], v[2]), v[3]);
            }
        }
        boats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boat_floats_and_rows_forward() {
        // a lake: water from y 6 to 9 everywhere
        let w = World::flat(|_, y, _| (6..=9).contains(&y).then_some(crate::world::WATER));
        let mut boats = Boats::default();
        boats.place(Vec3::new(0.5, 12.0, 0.5), 0.0);
        let still = Input::default();
        for _ in 0..240 {
            boats.update(&w, Vec3::ZERO, Some((0, &still)), 1.0 / 120.0);
        }
        let b = &boats.list[0];
        assert!(b.on_water && (b.pos.y - 9.88).abs() < 0.1, "floats at the surface: {}", b.pos);
        let row = Input { forward: 1.0, ..Default::default() };
        for _ in 0..240 {
            boats.update(&w, Vec3::ZERO, Some((0, &row)), 1.0 / 120.0);
        }
        let b = &boats.list[0];
        assert!(b.pos.z > 5.0 && (b.pos.y - 9.88).abs() < 0.1, "rows ahead: {}", b.pos);
    }
}
