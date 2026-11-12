//! Building states and container pairing. Every chest keeps its own 27 slots,
//! so joining or breaking a double chest never moves or duplicates its contents.
use crate::world::*;
use glam::IVec3;
use std::{collections::HashMap, io, path::Path};

pub const FENCE_GATE: Block = 288; // axis bit 0, open bit 1, birch bit 2
pub const SIGN: Block = 304; // four orientations
pub const SLABS: Block = 320; // four lower halves, then four upper halves
pub const STAIRS: Block = 352; // four materials, eight states each
pub fn is_fence_gate(b: Block) -> bool {
    (FENCE_GATE..FENCE_GATE + 8).contains(&b)
}
pub fn gate_open(b: Block) -> bool {
    is_fence_gate(b) && (b - FENCE_GATE) & 2 != 0
}
pub fn placed_gate(wood: Block, facing: Block) -> Block {
    // SIDE_DIR groups X directions first. The gate spans across the approach.
    FENCE_GATE + wood * 4 + ((facing / 2) ^ 1)
}
pub fn is_sign(b: Block) -> bool {
    (SIGN..SIGN + 4).contains(&b)
}
pub fn slab(mat: Block, top: bool) -> Block {
    if mat < 4 { if top { SLAB_TOP + mat } else { SLAB + mat } } else { SLABS + mat - 4 + if top { 4 } else { 0 } }
}
pub fn stairs(mat: Block, side: Block, top: bool) -> Block {
    if mat < 4 {
        STAIR_BASE + mat * 8 + side + if top { STAIR_FLIP } else { 0 }
    } else {
        STAIRS + (mat - 4) * 8 + side + if top { STAIR_FLIP } else { 0 }
    }
}

pub fn gate_boxes(b: Block, drawing: bool) -> Boxes {
    let mut v = [[0.0; 6]; 5];
    if gate_open(b) && !drawing {
        return (v, 0);
    }
    v[0] = [0.0, 0.0, 0.375, 0.1875, 1.0, 0.625];
    v[1] = [0.8125, 0.0, 0.375, 1.0, 1.0, 0.625];
    v[2] = if gate_open(b) {
        [0.0, 0.375, 0.375, 0.1875, 0.9375, 1.0]
    } else {
        [0.1875, 0.375, 0.4375, 0.8125, if drawing { 0.9375 } else { 1.5 }, 0.5625]
    };
    if (b - FENCE_GATE) & 1 != 0 {
        for bx in &mut v[..3] {
            bx.swap(0, 2);
            bx.swap(3, 5);
        }
    }
    (v, 3)
}

pub fn sign_boxes(b: Block) -> Boxes {
    let mut v = [[0.0; 6]; 5];
    v[0] = [0.4375, 0.0, 0.4375, 0.5625, 0.5, 0.5625];
    v[1] = [0.0625, 0.5, 0.4375, 0.9375, 1.0, 0.5625];
    if (b - SIGN) / 2 == 0 {
        for bx in &mut v[..2] {
            bx.swap(0, 2);
            bx.swap(3, 5);
        }
    }
    (v, 2)
}

fn adjacent(w: &World, p: IVec3) -> Vec<IVec3> {
    SIDE_DIR.iter().map(|&(x, z)| p + IVec3::new(x, 0, z)).filter(|p| w.get_block(p.x, p.y, p.z) == CHEST).collect()
}
pub fn chest_pair(w: &World, p: IVec3) -> Option<IVec3> {
    if w.get_block(p.x, p.y, p.z) != CHEST {
        return None;
    }
    let n = adjacent(w, p);
    if n.len() == 1 && adjacent(w, n[0]).len() == 1 { Some(n[0]) } else { None }
}
pub fn can_place_chest(w: &World, p: IVec3) -> bool {
    let n = adjacent(w, p);
    n.len() <= 1 && n.iter().all(|&q| adjacent(w, q).is_empty())
}
pub fn chest_halves(w: &World, p: IVec3) -> Vec<IVec3> {
    let mut positions = vec![p];
    if let Some(q) = chest_pair(w, p) {
        positions.push(q);
        positions.sort_by_key(|p| (p.x, p.y, p.z));
    }
    positions
}

pub type Signs = HashMap<IVec3, String>;
pub fn encode_signs(signs: &Signs) -> String {
    signs.iter().map(|(p, t)| format!("{} {} {} {}\n", p.x, p.y, p.z, t)).collect()
}
pub fn load_signs(path: &Path) -> io::Result<Signs> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Signs::new()),
        Err(e) => return Err(e),
    };
    let mut signs = Signs::new();
    for line in text.lines() {
        let mut parts = line.splitn(4, ' ');
        let parsed = (|| {
            Some((IVec3::new(parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?), parts.next().unwrap_or("")))
        })();
        let Some((p, t)) = parsed else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "damaged sign data"));
        };
        if t.len() > 60 || !t.is_ascii() || t.bytes().any(|b| b < 32) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "damaged sign text"));
        }
        signs.insert(p, t.to_string());
    }
    Ok(signs)
}

impl super::Game {
    pub(super) fn chest_stack(&self, i: usize) -> Option<crate::items::Stack> {
        let halves = chest_halves(&self.world, self.chest_at);
        self.chests.get(halves.get(i / crate::items::CHEST_SLOTS)?)?.get(i % crate::items::CHEST_SLOTS).copied()
    }
    pub(super) fn set_chest_stack(&mut self, i: usize, st: crate::items::Stack) {
        let halves = chest_halves(&self.world, self.chest_at);
        if let Some(p) = halves.get(i / crate::items::CHEST_SLOTS) {
            self.chests.entry(*p).or_insert([crate::items::Stack::default(); crate::items::CHEST_SLOTS])[i % crate::items::CHEST_SLOTS] = st;
        }
    }
    pub(super) fn stash_chest(&mut self, mut st: crate::items::Stack) -> crate::items::Stack {
        for p in chest_halves(&self.world, self.chest_at) {
            st = crate::items::stash_in(self.chests.entry(p).or_insert([crate::items::Stack::default(); crate::items::CHEST_SLOTS]), st);
        }
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_is_symmetric_and_cannot_make_a_triple_chest() {
        let mut w = World::flat(|x, y, z| (y == 10 && z == 0 && (x == 0 || x == 1)).then_some(CHEST));
        let a = IVec3::new(0, 10, 0);
        let b = IVec3::new(1, 10, 0);
        assert_eq!(chest_pair(&w, a), Some(b));
        assert_eq!(chest_halves(&w, a), chest_halves(&w, b));
        assert!(!can_place_chest(&w, IVec3::new(2, 10, 0)));
        w.set_block(0, 10, 0, AIR);
        assert_eq!(chest_pair(&w, b), None);
    }
    #[test]
    fn open_gates_allow_passage_and_signs_round_trip() {
        assert!(gate_boxes(FENCE_GATE, false).1 > 0);
        assert_eq!(gate_boxes(FENCE_GATE + 2, false).1, 0);
        let root = crate::save::tests::TestDir::new("signs");
        let path = root.0.join("signs.txt");
        let signs = Signs::from([(IVec3::new(-2, 150, 3), "Home and farm".into())]);
        std::fs::write(&path, encode_signs(&signs)).unwrap();
        assert_eq!(load_signs(&path).unwrap(), signs);
    }

    #[test]
    fn gates_and_sign_boards_face_the_placement_approach() {
        for facing in 0..4 {
            let axis = if facing < 2 { 0 } else { 2 };
            for wood in 0..2 {
                let (posts, _) = gate_boxes(placed_gate(wood, facing), false);
                assert_eq!((posts[0][axis], posts[0][axis + 3]), (0.375, 0.625));
            }
            let (boards, _) = sign_boxes(SIGN + facing);
            assert_eq!(boards[1][axis + 3] - boards[1][axis], 0.125);
        }
    }
}

use super::*;

impl Game {
    pub(super) fn fits(&self, q: IVec3, b: Block) -> bool {
        if !is_solid(b) {
            return true;
        }
        let (boxes, n) = self.world.shape_at(b, q.x, q.y, q.z);
        let (lo, hi) =
            (self.player.pos - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW), self.player.pos + Vec3::new(PLAYER_HW, self.player.height(), PLAYER_HW));
        boxes[..n].iter().all(|bx| {
            let (min, max) = (Vec3::new(bx[0], bx[1], bx[2]), Vec3::new(bx[3], bx[4], bx[5]));
            !(hi.cmpgt(min).all() && lo.cmplt(max).all()) && !self.mobs.blocks_box(min, max)
        })
    }

    pub(super) fn hiss(&mut self, at: Vec3) {
        self.audio.play_var(Snd::Fizz, at, 1.0);
        for _ in 0..10 {
            self.particles.smoke(&mut self.rng, at, false);
        }
    }

    pub(super) fn light_gate(&mut self, p: IVec3, n: IVec3) {
        let at = (p + n).as_vec3() + 0.5;
        self.particles.sparks(&mut self.rng, at, 8);
        self.audio.play_var(Snd::Dig(audio::Surface::Stone), at, 0.6);
        self.wear_held();
        if self.world.get_block(p.x, p.y, p.z) != CINDERSTONE {
            return;
        }
        let q = p + n;
        let found = [0u8, 1].into_iter().find_map(|axis| frame_opening(&self.world, q, axis).map(|o| (axis, o)));
        let Some((axis, (lo, hi))) = found else {
            self.say("An Ember Gate needs a closed cinderstone frame (an opening 2 wide and 3 high, up to 10)".into());
            return;
        };
        let mut edits = Vec::new();
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    edits.push((IVec3::new(x, y, z), GATE + axis as Block));
                }
            }
        }
        self.world.set_many(&edits);
        self.gates.push(Gate { deep: self.world.is_deep(), at: lo, axis });
        let mid = (lo.as_vec3() + hi.as_vec3()) * 0.5 + 0.5;
        self.audio.play_at(Snd::GateOpen, mid, 1.2, 1.0);
        for _ in 0..30 {
            let o = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(-1.5, 1.5), self.rng.range(-1.0, 1.0));
            self.particles.ember(&mut self.rng, mid + o);
        }
        self.say("The Ember Gate opens. Stand in it to cross".into());
    }

    pub(super) fn check_gates(&mut self, cells: &[IVec3]) {
        let mut out: Vec<IVec3> = Vec::new();
        for &c in cells {
            for d in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z, IVec3::ZERO] {
                let s = c + d;
                if !is_gate(self.world.get_block(s.x, s.y, s.z)) || out.contains(&s) {
                    continue;
                }
                // every veil cell joined to this one goes
                let mut todo = vec![s];
                out.push(s);
                while let Some(g) = todo.pop() {
                    for d in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                        let n = g + d;
                        if out.len() < 200 && is_gate(self.world.get_block(n.x, n.y, n.z)) && !out.contains(&n) {
                            out.push(n);
                            todo.push(n);
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            return;
        }
        let deep = self.world.is_deep();
        self.gates.retain(|g| g.deep != deep || !out.contains(&g.at));
        let edits: Vec<(IVec3, Block)> = out.iter().map(|&c| (c, AIR)).collect();
        self.world.set_many(&edits);
        let mid = out[0].as_vec3() + 0.5;
        self.audio.play_at(Snd::Fizz, mid, 1.0, 0.6);
    }

    pub(super) fn place(&mut self) {
        if self.combat.bow.is_some() {
            return;
        }
        self.place_cd = 0.2;
        let item = self.inv.hot(self.sel);
        if item != AIR {
            self.swing = SWING;
        }
        let target = self.target();
        // use the held item on a creature in reach: feed, shear, dye, tame, trade
        if let Some((i, t)) = self.mobs.pick(self.player.eye(), self.player.look_dir(), 4.0) {
            if target.map_or(true, |b| t < b.2) && self.use_on_mob(i, item) {
                self.place_cd = 0.3;
                return;
            }
        }
        // get into a boat in reach (unless a block is nearer)
        if self.riding.is_none() {
            if let Some((i, t)) = self.boats.pick(self.player.eye(), self.player.look_dir(), 4.0) {
                if target.map_or(true, |b| t < b.2) {
                    self.riding = Some(i);
                    self.player.vel = Vec3::ZERO;
                    self.player.swimming = false;
                    self.place_cd = 0.4;
                    self.say("Shift to get out".into());
                    return;
                }
            }
        }
        if item == items::BOAT {
            // on water if the crosshair finds some, otherwise on top of the block aimed at
            let yaw = self.player.yaw;
            let at = if let Some(w) = self.water_target(6.0) {
                // (the top of the water: climb out of any water above the cell aimed at)
                let mut y = w.y;
                while self.world.get_block(w.x, y + 1, w.z) == WATER {
                    y += 1;
                }
                Some(Vec3::new(w.x as f32 + 0.5, y as f32 + 0.9, w.z as f32 + 0.5))
            } else {
                target.filter(|t| t.1.y == 1).map(|(p, _, _)| p.as_vec3() + Vec3::new(0.5, 1.0, 0.5))
            };
            match at {
                Some(at) if !collides(&self.world, at + Vec3::Y * 0.05, boat::HW, boat::H) => {
                    self.boats.place(at, yaw);
                    self.spend();
                    self.audio.play_var(Snd::Splash, at, 0.4);
                }
                _ => self.say("A boat goes on water, or on top of a block".into()),
            }
            return;
        }
        // sneak to place a block against a door, chest or trapdoor instead of using it
        let sneak = self.settings.bindings.pressed(6, &self.keys);
        // use the block you are looking at
        if let Some((p, _, _)) = target {
            match self.world.get_block(p.x, p.y, p.z) {
                b @ (CHEST | LOOT_CHEST) if !sneak => {
                    self.chest_at = p;
                    if b == LOOT_CHEST {
                        // found in a cave and opened for the first time: from now on an
                        // ordinary chest holding what was left in it
                        self.chests.insert(p, loot::roll_chest(self.world.seed, p, self.world.is_deep()));
                        self.world.set_block(p.x, p.y, p.z, CHEST);
                        self.say("You found a chest".into());
                    }
                    self.chests.entry(p).or_insert([Stack::default(); items::CHEST_SLOTS]);
                    let halves = building::chest_halves(&self.world, p);
                    if halves.iter().any(|p| is_opaque(self.world.get_block(p.x, p.y + 1, p.z))) {
                        self.say("The chest needs room above its lid".into());
                        return;
                    }
                    self.chest_at = halves[0];
                    self.audio.play_var(Snd::Chest, p.as_vec3() + 0.5, 0.8);
                    self.goto(Screen::Chest);
                    return;
                }
                b if building::is_sign(b) && !sneak => {
                    self.chest_at = p;
                    self.sign_edit = self.signs.get(&p).cloned().unwrap_or_default();
                    self.goto(Screen::Sign);
                    return;
                }
                b if building::is_fence_gate(b) && !sneak => {
                    self.place_cd = 0.25;
                    let turned = b ^ 2;
                    if self.fits(p, turned) {
                        self.world.set_block(p.x, p.y, p.z, turned);
                        self.audio.play_var(Snd::Door, p.as_vec3() + 0.5, 0.7);
                    }
                    return;
                }
                b if is_trapdoor(b) && !sneak => {
                    self.place_cd = 0.25;
                    let turned = TRAP_BASE + ((b - TRAP_BASE) ^ TRAP_OPEN);
                    if !self.fits(p, turned) {
                        self.say("Something is in the way of the trapdoor".into());
                    } else if self.world.set_block(p.x, p.y, p.z, turned) {
                        self.audio.play_var(Snd::Door, p.as_vec3() + 0.5, 0.7);
                    }
                    return;
                }
                BED if self.world.is_deep() => {
                    // no sleeping down here
                    self.place_cd = 0.6;
                    self.world.set_block(p.x, p.y, p.z, AIR);
                    self.explode(p.as_vec3() + 0.5, 3.0);
                    self.say("Beds don't work in the Emberdeep".into());
                    return;
                }
                BED => {
                    self.place_cd = 0.6;
                    self.spawn = p.as_vec3() + Vec3::new(0.5, 1.01, 0.5);
                    if self.sun().1 < 0.35 {
                        self.time = 0.015;
                        self.trade_uses.clear();
                        self.say("You slept until morning. Spawn point set".into());
                    } else {
                        self.say("Spawn point set. You can only sleep at night".into());
                    }
                    return;
                }
                b @ (ANVIL | ENCHANTING_TABLE | WORKBENCH) if !sneak => {
                    self.equipment_open = b != WORKBENCH;
                    self.goto(Screen::Inventory);
                    return;
                }
                FURNACE if !sneak => {
                    self.chest_at = p;
                    self.furnaces.entry(p).or_default();
                    self.goto(Screen::Furnace);
                    return;
                }
                b if is_door(b) && !sneak => {
                    self.toggle_door(p, b);
                    return;
                }
                _ => {}
            }
        }
        if let Some(crop) = farming::plant(item) {
            if let Some((p, n, _)) = target {
                if n.y == 1 && farming::moisture(self.world.get_block(p.x, p.y, p.z)).is_some() && self.world.get_block(p.x, p.y + 1, p.z) == AIR {
                    if self.world.set_block(p.x, p.y + 1, p.z, crop) {
                        self.spend();
                    }
                    return;
                }
            }
        }
        let bite = items::food(item);
        if bite > 0.0 {
            if self.food < MAX_FOOD - 0.5 {
                self.spend();
                self.food = (self.food + bite).min(MAX_FOOD);
                self.place_cd = 0.4;
                let pitch = 0.9 + self.rng.f() * 0.2;
                self.audio.play(Snd::Eat, 0.7, pitch);
                self.say(format!("Ate {}", items::name(item)));
            }
            return;
        }
        match items::gear(item) {
            Some((items::Kind::Hoe, _)) => {
                // till the ground
                if let Some((p, _, _)) = target {
                    let above = self.world.get_block(p.x, p.y + 1, p.z);
                    let soil = {
                        let b = self.world.get_block(p.x, p.y, p.z);
                        b == DIRT || is_grassy(b)
                    };
                    if soil && (above == AIR || is_tuft(above)) {
                        if above != AIR {
                            self.world.set_block(p.x, p.y + 1, p.z, AIR);
                        }
                        if self.world.set_block(p.x, p.y, p.z, FARMLAND) {
                            self.wear_held();
                        }
                    }
                }
                return;
            }
            _ => {}
        }
        match item {
            items::SEEDS => {
                if let Some((p, n, _)) = target {
                    if n.y == 1 && self.world.get_block(p.x, p.y, p.z) == FARMLAND && self.world.get_block(p.x, p.y + 1, p.z) == AIR {
                        if self.world.set_block(p.x, p.y + 1, p.z, WHEAT_0) {
                            self.spend();
                        }
                    } else {
                        self.say("Seeds need farmland: till grass or dirt with a hoe".into());
                    }
                }
                return;
            }
            items::BUCKET => {
                // scoops up the still water or lava it is pointed at (not running water)
                let hit = raycast_with(&self.world, self.player.eye(), self.player.look_dir(), 6.0, |b| b == WATER || b == LAVA || is_solid(b));
                if let Some((w, _, _)) = hit {
                    let b = self.world.get_block(w.x, w.y, w.z);
                    let full = if b == WATER {
                        items::WATER_BUCKET
                    } else if b == LAVA {
                        items::LAVA_BUCKET
                    } else {
                        return;
                    };
                    if self.world.set_block(w.x, w.y, w.z, AIR) {
                        self.audio.play_var(if b == WATER { Snd::Swim } else { Snd::Fizz }, w.as_vec3() + 0.5, 0.9);
                        self.inv.slots[self.sel] = Stack::new(full, 1);
                    }
                }
                return;
            }
            items::LAVA_BUCKET => {
                if let Some((p, n, _)) = target {
                    let q = p + n;
                    let here = q.as_vec3() + 0.5;
                    let was = self.world.get_block(q.x, q.y, q.z);
                    if is_water(was) {
                        // poured into water it hardens at once
                        if self.world.set_block(q.x, q.y, q.z, if was == WATER { CINDERSTONE } else { COBBLE }) {
                            self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                            self.hiss(here);
                        }
                    } else if is_replaceable(was) && self.world.set_block(q.x, q.y, q.z, LAVA) {
                        self.audio.play_var(Snd::Fizz, here, 0.6);
                        self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                    }
                }
                return;
            }
            items::WATER_BUCKET => {
                if let Some((p, n, _)) = target {
                    let q = p + n;
                    let here = q.as_vec3() + 0.5;
                    if self.world.is_deep() {
                        // the heat down here boils it away at once
                        self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                        self.audio.play_var(Snd::Fizz, here, 1.0);
                        for _ in 0..12 {
                            self.particles.smoke(&mut self.rng, here, false);
                        }
                        self.say("The water boils away".into());
                        return;
                    }
                    let was = self.world.get_block(q.x, q.y, q.z);
                    if was == LAVA {
                        // water poured on lava: it hardens into cinderstone
                        if self.world.set_block(q.x, q.y, q.z, CINDERSTONE) {
                            self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                            self.hiss(here);
                        }
                    } else if is_replaceable(was) && self.world.set_block(q.x, q.y, q.z, WATER) {
                        self.audio.play_var(Snd::Splash, here, 0.5);
                        self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                        // and the lava it touches hardens
                        let mut hard = Vec::new();
                        for d in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::NEG_Y, IVec3::Y] {
                            let c = q + d;
                            if self.world.get_block(c.x, c.y, c.z) == LAVA {
                                hard.push((c, CINDERSTONE));
                            }
                        }
                        if !hard.is_empty() {
                            self.world.set_many(&hard);
                            self.hiss(here);
                        }
                    }
                }
                return;
            }
            items::STRIKER => {
                if let Some((p, n, _)) = target {
                    self.light_gate(p, n);
                }
                return;
            }
            items::SHIELD => return,
            items::BOW => {
                if self.combat.bow.is_none() {
                    self.combat.bow = Some((self.sel, 0.0));
                }
                return;
            }

            _ => {}
        }
        if matches!(item, items::OAK_DOOR | items::BIRCH_DOOR) {
            self.place_door(target, item);
            return;
        }
        let Some((p, n, t)) = target else { return };
        let q = p + n;
        let clicked = self.world.get_block(p.x, p.y, p.z);
        let d = self.player.look_dir();
        // the side of the cell you look towards, and the side the clicked wall is on
        let facing: Block = if d.x.abs() > d.z.abs() { (d.x > 0.0) as Block } else { 2 + (d.z > 0.0) as Block };
        let wall: Block = if n.x == 1 {
            0
        } else if n.x == -1 {
            1
        } else if n.z == 1 {
            2
        } else {
            3
        };
        // pointing at the upper half of a side, or at an underside, places the high variant
        let high = n.y == -1 || (n.y == 0 && (self.player.eye() + d * t).y - p.y as f32 > 0.5);
        let (at, block): (IVec3, Block) = match item {
            items::OAK_GATE | items::BIRCH_GATE => (q, building::placed_gate(item - items::OAK_GATE, facing)),
            items::SIGN_ITEM => (q, building::SIGN + facing),
            items::LADDER => {
                if n.y != 0 || !is_opaque(clicked) {
                    self.say("A ladder hangs on the side of a solid block".into());
                    return;
                }
                (q, LADDER_BASE + wall)
            }
            TORCH => {
                // on top of a block it stands, on a side it hangs from the wall
                let ok = is_solid(clicked) && !is_door(clicked) && !is_ladder(clicked) && !is_trapdoor(clicked) && !is_fence(clicked)
                    || (is_fence(clicked) && n.y == 1);
                if n.y == -1 || !ok || (n.y == 0 && !is_opaque(clicked)) {
                    self.say("A torch stands on a block or hangs on its side".into());
                    return;
                }
                (q, if n.y == 1 { TORCH } else { WALL_TORCH + wall })
            }
            items::TRAPDOOR => (q, TRAP_BASE + if n.y == 0 { wall } else { facing } + if high { TRAP_TOP } else { 0 }),
            i if items::stair_material(i).is_some() => (q, building::stairs(items::stair_material(i).unwrap(), facing, high)),
            i if items::slab_material(i).is_some() => {
                // two halves of a kind make a full block again
                let mat = items::slab_material(i).unwrap();
                let full = SHAPE_FULL[mat as usize];
                let same = |b: Block| is_slab(b) && slab_mat(b) == mat;
                if same(clicked) && n.y == if slab_top(clicked) { -1 } else { 1 } {
                    (p, full)
                } else if same(self.world.get_block(q.x, q.y, q.z)) {
                    (q, full)
                } else {
                    (q, building::slab(mat, high))
                }
            }
            i if items::is_sapling_item(i) => {
                let soil = clicked == DIRT || clicked == FARMLAND || is_grassy(clicked);
                if n.y != 1 || !soil {
                    self.say("A sapling needs grass or dirt to grow in".into());
                    return;
                }
                (q, items::block_of(i))
            }
            i if items::is_block(i) => (q, items::block_of(i)),
            _ => return,
        };
        if block == CHEST && !building::can_place_chest(&self.world, at) {
            self.say("A double chest can have only two halves".into());
            return;
        }
        let cur = self.world.get_block(at.x, at.y, at.z);
        // (a slab is only ever replaced by the full block its second half makes)
        let joins = is_slab(cur) && items::slab_material(item).is_some() && block == SHAPE_FULL[slab_mat(cur) as usize];
        if !is_replaceable(cur) && !joins {
            return;
        }
        if !self.fits(at, block) {
            return;
        }
        if self.world.set_block(at.x, at.y, at.z, block) {
            self.spend();
            if building::is_sign(block) {
                self.chest_at = at;
                self.sign_edit = String::new();
                self.goto(Screen::Sign);
            }
            self.weather.dirty();
            self.audio.play_var(Snd::Place(audio::surface(block)), at.as_vec3() + 0.5, 0.8);
        }
    }

    pub(super) fn door_blocked(&self, p: IVec3, b: Block) -> bool {
        let q = shape_boxes(b, |_, _| AIR).0[0];
        let min = p.as_vec3() + Vec3::new(q[0], 0.0, q[2]);
        let max = p.as_vec3() + Vec3::new(q[3], 2.0, q[5]);
        let (lo, hi) =
            (self.player.pos - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW), self.player.pos + Vec3::new(PLAYER_HW, self.player.height(), PLAYER_HW));
        (hi.cmpgt(min).all() && lo.cmplt(max).all()) || self.mobs.blocks_box(min, max)
    }

    pub(super) fn toggle_door(&mut self, p: IVec3, b: Block) {
        self.place_cd = 0.25;
        let y = if door_upper(b) { p.y - 1 } else { p.y };
        let (lower, upper) = (self.world.get_block(p.x, y, p.z), self.world.get_block(p.x, y + 1, p.z));
        if !is_door(lower) || !is_door(upper) {
            return;
        }
        let (lower, upper) = (lower ^ DOOR_OPEN, upper ^ DOOR_OPEN);
        if self.door_blocked(IVec3::new(p.x, y, p.z), lower) {
            self.say("Something is in the way of the door".into());
            return;
        }
        if self.world.set_pair(p.x, y, p.z, lower, upper) {
            self.audio.play_var(Snd::Door, p.as_vec3() + 0.5, 0.75);
        }
    }

    pub(super) fn place_door(&mut self, target: Option<(IVec3, IVec3, f32)>, item: Item) {
        let Some((p, n, _)) = target else { return };
        let q = p + n;
        let at = |w: &World, dx: i32, dy: i32, dz: i32| w.get_block(q.x + dx, q.y + dy, q.z + dz);
        let full = |b: Block| is_solid(b) && !is_shaped(b);
        let w = &self.world;
        if !is_replaceable(at(w, 0, 0, 0)) || !is_replaceable(at(w, 0, 1, 0)) || !full(at(w, 0, -1, 0)) {
            self.say("A door needs firm ground and two blocks of room".into());
            return;
        }
        let d = self.player.look_dir();
        let side = if d.x.abs() > d.z.abs() { (d.x < 0.0) as u8 } else { 2 + (d.z < 0.0) as u8 };
        // the hinge goes on the left, unless that would leave it hanging in the open
        // or there is a door there already (so two doors meet in the middle)
        let beside = |s: u8| at(w, SIDE_DIR[s as usize].0, 0, SIDE_DIR[s as usize].1);
        let (left, right) = (beside(SIDE_LEFT[side as usize]), beside(SIDE_RIGHT[side as usize]));
        let hinge_right = !is_door(right) && (is_door(left) || (full(right) && !full(left)));
        let lower = DOOR_BASE + side as Block + if hinge_right { DOOR_HINGE_RIGHT } else { 0 } + (item - items::OAK_DOOR) * 32;
        if self.door_blocked(q, lower) {
            return;
        }
        if self.world.set_pair(q.x, q.y, q.z, lower, lower + DOOR_UPPER) {
            self.spend();
        }
    }
}
