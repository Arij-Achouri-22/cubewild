//! Items outside the inventory: those lying in the world (dropped by broken blocks, by
//! animals, by hand and by a dying player) and those stored in chests.

use crate::gfx::ItemVert;
use crate::items::{self, Inventory, Stack, CHEST_SLOTS};
use crate::mobs::Rng;
use crate::player::{collides, move_collide, PLAYER_H, PLAYER_HW};
use crate::world::*;
use glam::{IVec3, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// seconds an item lies in a loaded part of the world before it vanishes
pub const LIFE: f32 = 300.0;
/// the oldest items make room once this many lie around
const MAX: usize = 400;
const HW: f32 = 0.125;
const H: f32 = 0.25;

pub struct Pickup {
    pub pos: Vec3,
    vel: Vec3,
    pub stack: Stack,
    age: f32,
    /// seconds until the player can pick it up
    wait: f32,
    /// lying still: only checked now and then for the ground having gone
    rest: bool,
    check: f32,
    /// the cell its daylight was last read in, and what was read (see `World::sky_level`)
    cell: IVec3,
    sky: u8,
}

#[derive(Default)]
pub struct Pickups {
    pub list: Vec<Pickup>,
}

pub fn parse_stack(item: u32, n: u32, wear: u32) -> Option<Stack> {
    let item = items::Item::try_from(item).ok().filter(|&i| items::valid(i))?;
    (n > 0).then(|| Stack { item, n: n.min(items::max_stack(item)), wear: (wear & 65535).min(items::max_wear(item).saturating_sub(1) as u32) as u16, enchants: ((wear >> 16) as u8) & 63 })
}

impl Pickups {
    pub fn spawn(&mut self, pos: Vec3, vel: Vec3, stack: Stack, wait: f32) {
        if stack.is_empty() || !items::valid(stack.item) {
            return;
        }
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        self.list.push(Pickup { pos, vel, stack, age: 0.0, wait, rest: false, check: 0.0, cell: IVec3::MAX, sky: 2 });
    }

    /// Drops a stack at `pos` with a small random hop, as a broken block does.
    pub fn pop(&mut self, rng: &mut Rng, pos: Vec3, stack: Stack) {
        let vel = Vec3::new(rng.range(-1.3, 1.3), rng.range(2.4, 3.8), rng.range(-1.3, 1.3));
        self.spawn(pos - Vec3::Y * (H * 0.5), vel, stack, 0.5);
    }

    /// Moves the items and lets the player (`None` while dead) gather those in reach.
    /// What was gathered is added to `got`.
    pub fn update(&mut self, world: &World, player: Option<Vec3>, inv: &mut Inventory, got: &mut Vec<Stack>, dt: f32) {
        for i in 0..self.list.len() {
            let p = &mut self.list[i];
            let c = p.pos.floor().as_ivec3();
            // items in a part of the world that is not loaded wait there, without ageing
            if !world.is_loaded(c.x, c.z) {
                continue;
            }
            p.age += dt;
            p.wait -= dt;
            // daylight is read again on entering a cell, and now and then while lying still
            let cell = IVec3::new(c.x, (p.pos.y + 0.1).floor() as i32, c.z);
            if cell != p.cell || (p.rest && p.check <= dt) {
                p.cell = cell;
                p.sky = world.sky_level(cell.x, cell.y, cell.z);
            }
            if p.rest {
                p.check -= dt;
                if p.check <= 0.0 {
                    p.check = 0.4;
                    // (running water picks it up again)
                    let wet = crate::fluid::current(world, cell) != Vec3::ZERO;
                    p.rest = collides(world, p.pos - Vec3::Y * 0.05, HW, H) && !wet;
                }
            } else {
                let around = world.get_block(c.x, (p.pos.y + 0.1).floor() as i32, c.z);
                if is_lava(around) {
                    // lava destroys what falls into it
                    p.stack = Stack::default();
                    continue;
                }
                if is_water(around) {
                    // floats up and drifts with the water: to a stop in still water
                    let flow = crate::fluid::current(world, IVec3::new(c.x, (p.pos.y + 0.1).floor() as i32, c.z)) * 2.2;
                    p.vel.y = (p.vel.y + 9.0 * dt).min(1.0);
                    let k = 1.0 - (-dt * 3.0).exp();
                    p.vel.x += (flow.x - p.vel.x) * k;
                    p.vel.z += (flow.z - p.vel.z) * k;
                } else {
                    p.vel.y = (p.vel.y - 22.0 * dt).max(-30.0);
                }
                let (ground, _) = move_collide(world, &mut p.pos, &mut p.vel, HW, H, dt);
                if ground {
                    let k = (-dt * 10.0).exp();
                    p.vel.x *= k;
                    p.vel.z *= k;
                    if p.vel.x * p.vel.x + p.vel.z * p.vel.z < 0.01 && !is_water(around) {
                        p.vel = Vec3::ZERO;
                        p.rest = true;
                        p.check = 0.4;
                        self.merge(i);
                    }
                }
            }
            let p = &mut self.list[i];
            if let Some(pp) = player.filter(|_| p.wait <= 0.0 && !p.stack.is_empty()) {
                let d = p.pos - pp;
                let reach = PLAYER_HW + 1.0;
                if d.x.abs() < reach && d.z.abs() < reach && d.y > -0.5 - H && d.y < PLAYER_H + 0.5 {
                    let left = inv.pick_up(p.stack);
                    if left.n < p.stack.n {
                        got.push(Stack::new(p.stack.item, p.stack.n - left.n));
                    }
                    p.stack = left;
                }
            }
        }
        self.list.retain(|p| !p.stack.is_empty() && p.age < LIFE && p.pos.y > -20.0);
    }

    /// Adds the item that just came to rest to a like stack lying beside it.
    fn merge(&mut self, i: usize) {
        let st = self.list[i].stack;
        let max = items::max_stack(st.item);
        if max == 1 {
            return;
        }
        let pos = self.list[i].pos;
        let other = (0..self.list.len()).find(|&j| {
            let o = &self.list[j];
            j != i && o.rest && o.stack.item == st.item && !o.stack.is_empty() && o.stack.n + st.n <= max && (o.pos - pos).length_squared() < 0.8
        });
        if let Some(j) = other {
            self.list[j].stack.n += st.n;
            self.list[j].wait = self.list[j].wait.max(self.list[i].wait);
            self.list[j].age = self.list[j].age.min(self.list[i].age);
            self.list[i].stack = Stack::default();
        }
    }

    /// Small spinning models: a cube for full blocks, the icon for everything else.
    pub fn draw(&self, out: &mut Vec<ItemVert>, eye: Vec3, max_dist: f32) {
        const SHADE: [f32; 6] = [0.80, 0.80, 1.0, 0.50, 0.66, 0.66];
        // uv runs just inside the texture so the edge never wraps round
        const UV: [[f32; 2]; 4] = [[0.01, 0.99], [0.99, 0.99], [0.99, 0.01], [0.01, 0.01]];
        for p in &self.list {
            let rel = p.pos - eye;
            if rel.length_squared() > max_dist * max_dist {
                continue;
            }
            // each item starts its turn at a different angle
            let t = p.age * 1.8 + (p.pos.x + p.pos.z) * 3.0;
            let (s, c) = t.sin_cos();
            let item = p.stack.item;
            let sky = [0.2, 0.52, 1.0][p.sky.min(2) as usize];
            let block = items::block_of(item);
            let cube = items::is_block(item) && !is_plant(block) && !is_shaped(block) && block != TORCH && block != GLASS;
            for copy in 0..if p.stack.n > 1 { 2 } else { 1 } {
                let k = copy as f32;
                let mid = rel + Vec3::new(k * 0.09, 0.22 + (p.age * 2.4).sin() * 0.04 + k * 0.07, k * 0.09);
                let turn = |x: f32, y: f32, z: f32| mid + Vec3::new(x * c + z * s, y, z * c - x * s);
                if cube {
                    let h = 0.13;
                    let faces = [
                        [(h, -h, -h), (h, -h, h), (h, h, h), (h, h, -h)],
                        [(-h, -h, h), (-h, -h, -h), (-h, h, -h), (-h, h, h)],
                        [(-h, h, h), (h, h, h), (h, h, -h), (-h, h, -h)],
                        [(-h, -h, -h), (h, -h, -h), (h, -h, h), (-h, -h, h)],
                        [(h, -h, h), (-h, -h, h), (-h, h, h), (h, h, h)],
                        [(-h, -h, -h), (h, -h, -h), (h, h, -h), (-h, h, -h)],
                    ];
                    for (f, q) in faces.iter().enumerate() {
                        let layer = tex_for(block, f);
                        for (v, &(x, y, z)) in q.iter().enumerate() {
                            out.push(ItemVert { pos: turn(x, y, z).into(), uv: UV[v], layer, light: SHADE[f], sky });
                        }
                    }
                } else {
                    let h = 0.2;
                    let layer = items::icon(item);
                    for (v, (x, y)) in [(-h, -h), (h, -h), (h, h), (-h, h)].into_iter().enumerate() {
                        out.push(ItemVert { pos: turn(x, y + 0.05, 0.0).into(), uv: UV[v], layer, light: 1.0, sky });
                    }
                }
            }
        }
    }

    #[cfg(test)]
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.encode())
    }

    pub fn encode(&self) -> String {
        let mut s = String::new();
        for p in &self.list {
            s += &format!("{} {} {} {} {} {} {}\n", p.pos.x, p.pos.y, p.pos.z, p.stack.item, p.stack.n, p.stack.persist_wear(), p.age);
        }
        s
    }

    pub fn load(path: &Path) -> Pickups {
        let mut list = Vec::new();
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            let v: Vec<f32> = line.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if v.len() < 7 {
                continue;
            }
            if let Some(stack) = parse_stack(v[3] as u32, v[4] as u32, v[5] as u32) {
                list.push(Pickup { pos: Vec3::new(v[0], v[1], v[2]), vel: Vec3::ZERO, stack, age: v[6].max(0.0), wait: 0.5, rest: false, check: 0.0, cell: IVec3::MAX, sky: 2 });
            }
        }
        Pickups { list }
    }
}

/// What each chest in the world holds, by the block's position.
pub type Chests = HashMap<IVec3, [Stack; CHEST_SLOTS]>;

/// What a chest found in a cave holds: always the same for a world and a place, and
/// better the deeper it stands (torches and food near the surface; iron, gold, crystal
/// and finished gear near the lava).
pub fn roll_chest(seed: u32, at: IVec3, emberdeep: bool) -> [Stack; CHEST_SLOTS] {
    use items::*;
    let mut rng = Rng(((seed as u64) << 32 | (at.x as u32 as u64) << 8) ^ (at.z as u32 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ ((at.y as u64) << 52) | 1);
    for _ in 0..6 {
        rng.next();
    }
    if emberdeep {
        return fortress_chest(&mut rng);
    }
    // 0 = just under the surface .. 1 = at the lava
    let deep = ((SEA - at.y) as f32 / (SEA - LIFT) as f32).clamp(0.0, 1.0);
    // (item, most at once, chance at the top, chance at the bottom)
    let table: [(Item, u32, f32, f32); 17] = [
        (TORCH, 8, 0.9, 0.7),
        (BREAD, 3, 0.6, 0.5),
        (COAL, 8, 0.7, 0.6),
        (ARROW, 12, 0.4, 0.5),
        (STICK, 6, 0.4, 0.1),
        (COOKED_MEAT, 3, 0.3, 0.5),
        (IRON_INGOT, 5, 0.35, 0.9),
        (GOLD_INGOT, 4, 0.1, 0.7),
        (CRYSTAL, 3, 0.0, 0.6),
        (IRON_PICK, 1, 0.08, 0.3),
        (IRON_SWORD, 1, 0.06, 0.3),
        (BOW, 1, 0.08, 0.25),
        (IRON_HELMET, 1, 0.03, 0.25),
        (IRON_CHEST, 1, 0.02, 0.2),
        (IRON_BOOTS, 1, 0.03, 0.25),
        (CRYSTAL_PICK, 1, 0.0, 0.1),
        (CRYSTAL_SWORD, 1, 0.0, 0.1),
    ];
    let mut slots = [Stack::default(); CHEST_SLOTS];
    for (item, most, top, bottom) in table {
        if rng.f() < top + (bottom - top) * deep {
            // more of it the deeper, in a slot of its own somewhere in the chest
            let n = (1 + (rng.f() * (most as f32 - 1.0) * (0.4 + 0.6 * deep)).round() as u32).min(max_stack(item));
            let free: Vec<usize> = (0..CHEST_SLOTS).filter(|&i| slots[i].is_empty()).collect();
            slots[free[rng.next() as usize % free.len()]] = Stack::new(item, n);
        }
    }
    slots
}

/// The hoard of a ruined fortress in the Emberdeep: gold and embers, and the only pyre
/// metal that is not dug out by the lava.
fn fortress_chest(rng: &mut Rng) -> [Stack; CHEST_SLOTS] {
    use items::*;
    // (item, most at once, chance)
    let table: [(Item, u32, f32); 14] = [
        (GOLD_INGOT, 6, 0.75),
        (EMBER, 5, 0.6),
        (IRON_INGOT, 6, 0.5),
        (CRYSTAL, 4, 0.45),
        (PYRE_INGOT, 2, 0.35),
        (GEM, 5, 0.4),
        (COOKED_MEAT, 4, 0.4),
        (ARROW, 16, 0.35),
        (STRIKER, 1, 0.2),
        (EXTRA_ITEM + 3, 2, 0.2),
        (GOLD_SWORD, 1, 0.15),
        (CRYSTAL_HELMET, 1, 0.1),
        (CRYSTAL_SWORD, 1, 0.1),
        (PYRE_BOOTS, 1, 0.03),
    ];
    let mut slots = [Stack::default(); CHEST_SLOTS];
    for (item, most, chance) in table {
        if rng.f() < chance {
            let n = (1 + (rng.f() * (most as f32 - 1.0)).round() as u32).min(max_stack(item));
            let free: Vec<usize> = (0..CHEST_SLOTS).filter(|&i| slots[i].is_empty()).collect();
            slots[free[rng.next() as usize % free.len()]] = Stack::new(item, n);
        }
    }
    slots
}

#[cfg(test)]
pub fn save_chests(chests: &Chests, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, encode_chests(chests))
}

pub fn encode_chests(chests: &Chests) -> String {
    let mut s = String::new();
    for (p, slots) in chests {
        if slots.iter().all(|st| st.is_empty()) {
            continue;
        }
        s += &format!("{} {} {}", p.x, p.y, p.z);
        for (i, st) in slots.iter().enumerate().filter(|(_, st)| !st.is_empty()) {
            s += &format!(" {}:{}:{}:{}", i, st.item, st.n, st.persist_wear());
        }
        s.push('\n');
    }
    s
}

pub fn load_chests(path: &Path) -> Chests {
    let mut chests = Chests::default();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let mut parts = line.split_whitespace();
        let pos: Vec<i32> = parts.by_ref().take(3).filter_map(|t| t.parse().ok()).collect();
        if pos.len() < 3 {
            continue;
        }
        let mut slots = [Stack::default(); CHEST_SLOTS];
        for t in parts {
            let v: Vec<u32> = t.split(':').filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 4 && (v[0] as usize) < CHEST_SLOTS {
                if let Some(st) = parse_stack(v[1], v[2], v[3]) {
                    slots[v[0] as usize] = st;
                }
            }
        }
        chests.insert(IVec3::new(pos[0], pos[1], pos[2]), slots);
    }
    chests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cave_chests_hold_more_the_deeper_they_stand() {
        let worth = |st: &Stack| -> u32 {
            st.n * match st.item {
                items::CRYSTAL | items::CRYSTAL_PICK | items::CRYSTAL_SWORD => 30,
                items::GOLD_INGOT => 12,
                items::IRON_INGOT => 6,
                i if items::max_wear(i) > 0 => 20,
                _ => 1,
            }
        };
        let (mut top, mut bottom) = (0, 0);
        for i in 0..200 {
            let (high, low) = (IVec3::new(i * 7, SEA, -i * 3), IVec3::new(i * 7, LIFT + 8, -i * 3));
            // the same chest always holds the same things
            assert!(roll_chest(77, high, false) == roll_chest(77, high, false));
            let (a, b) = (roll_chest(77, high, false), roll_chest(77, low, false));
            for st in a.iter().chain(b.iter()).filter(|st| !st.is_empty()) {
                assert!(items::valid(st.item) && st.n >= 1 && st.n <= items::max_stack(st.item));
            }
            assert!(!a.iter().any(|st| st.item == items::CRYSTAL));
            top += a.iter().map(worth).sum::<u32>();
            bottom += b.iter().map(worth).sum::<u32>();
        }
        assert!(top > 0 && bottom > top * 3, "{top} {bottom}");
        assert!(roll_chest(77, IVec3::new(1, 20, 1), false) != roll_chest(78, IVec3::new(1, 20, 1), false));
    }

    #[test]
    fn dropped_items_fall_rest_merge_and_are_picked_up() {
        let w = World::flat(|_, _, _| None);
        let mut drops = Pickups::default();
        let mut rng = Rng(7);
        let mut inv = Inventory::new();
        let mut got = Vec::new();
        for _ in 0..3 {
            drops.pop(&mut rng, Vec3::new(0.5, 12.5, 0.5), Stack::new(COBBLE, 2));
        }
        drops.pop(&mut rng, Vec3::new(0.5, 12.5, 0.5), Stack { item: items::IRON_PICK, n: 1, wear: 5, enchants: 0 });
        for _ in 0..600 {
            drops.update(&w, None, &mut inv, &mut got, 1.0 / 120.0);
        }
        // nobody there: they lie on the ground, like stacks merged where they landed together
        assert!(got.is_empty() && inv.is_empty());
        assert!(drops.list.iter().all(|p| p.rest && (p.pos.y - 10.0).abs() < 0.01), "all at rest on the floor");
        assert_eq!(drops.list.iter().filter(|p| p.stack.item == COBBLE).map(|p| p.stack.n).sum::<u32>(), 6);
        // a player far away gathers nothing, one standing there gathers it all, wear included
        drops.update(&w, Some(Vec3::new(9.5, 10.0, 0.5)), &mut inv, &mut got, 1.0 / 120.0);
        assert!(inv.is_empty());
        drops.update(&w, Some(Vec3::new(0.5, 10.0, 0.5)), &mut inv, &mut got, 1.0 / 120.0);
        assert!(drops.list.is_empty());
        assert_eq!(inv.count(COBBLE), 6);
        assert!(inv.slots.contains(&Stack { item: items::IRON_PICK, n: 1, wear: 5, enchants: 0 }));
        assert_eq!(got.iter().map(|s| s.n).sum::<u32>(), 7);
        // after five minutes untouched an item is gone
        drops.spawn(Vec3::new(0.5, 10.0, 0.5), Vec3::ZERO, Stack::new(DIRT, 1), 0.0);
        for _ in 0..(LIFE as usize + 2) * 20 {
            drops.update(&w, None, &mut inv, &mut got, 0.05);
        }
        assert!(drops.list.is_empty());
        // the textured models: a cube (6 quads) for a block, two flat icons for a stack of sticks
        drops.spawn(Vec3::new(0.5, 10.0, 0.5), Vec3::ZERO, Stack::new(DIRT, 1), 0.0);
        drops.spawn(Vec3::new(3.5, 10.0, 0.5), Vec3::ZERO, Stack::new(items::STICK, 4), 0.0);
        let mut verts = Vec::new();
        drops.draw(&mut verts, Vec3::new(0.0, 11.0, 0.0), 40.0);
        assert_eq!(verts.len(), (6 + 2) * 4);
    }

    #[test]
    fn chests_and_dropped_items_survive_a_save() {
        let dir = std::env::temp_dir().join(format!("cubewild-loot-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut chests = Chests::default();
        let mut slots = [Stack::default(); CHEST_SLOTS];
        slots[0] = Stack::new(COBBLE, 64);
        slots[26] = Stack { item: items::IRON_PICK, n: 1, wear: 12, enchants: 0 };
        chests.insert(IVec3::new(-5, 70, 12), slots);
        chests.insert(IVec3::new(1, 2, 3), [Stack::default(); CHEST_SLOTS]);
        save_chests(&chests, &dir.join("chests.txt")).unwrap();
        let back = load_chests(&dir.join("chests.txt"));
        assert_eq!(back.len(), 1, "empty chests are not written");
        assert_eq!(back[&IVec3::new(-5, 70, 12)], slots);

        let mut drops = Pickups::default();
        drops.spawn(Vec3::new(1.5, 64.0, -2.5), Vec3::ZERO, Stack { item: items::IRON_PICK, n: 1, wear: 3, enchants: 0 }, 0.0);
        drops.spawn(Vec3::ZERO, Vec3::ZERO, Stack::default(), 0.0);
        assert_eq!(drops.list.len(), 1, "nothing is dropped for an empty stack");
        drops.save(&dir.join("drops.txt")).unwrap();
        let back = Pickups::load(&dir.join("drops.txt"));
        assert_eq!((back.list.len(), back.list[0].stack, back.list[0].pos), (1, drops.list[0].stack, drops.list[0].pos));
        std::fs::remove_dir_all(&dir).ok();
    }
}
