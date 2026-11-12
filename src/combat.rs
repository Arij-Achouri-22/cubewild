//! Combat timing and directional defence, kept separate from rendering.
use crate::items::{self, Item, Kind};
use glam::Vec3;

#[derive(Default)]
pub struct State {
    pub bow: Option<(usize, f32)>,
    pub blocking: f32,
}

pub fn attack_period(item: Item) -> f32 {
    match items::gear(item).map(|g| g.0) {
        Some(Kind::Sword) => 0.625,
        Some(Kind::Axe) => 1.0,
        _ => 0.4,
    }
}
pub fn bow_power(seconds: f32) -> f32 {
    let t = seconds.clamp(0.0, 1.0);
    ((t * t + 2.0 * t) / 3.0).min(1.0)
}
pub fn in_front(look: Vec3, from: Vec3) -> bool {
    let facing = Vec3::new(look.x, 0.0, look.z).normalize_or_zero();
    let source = Vec3::new(from.x, 0.0, from.z).normalize_or_zero();
    facing.dot(source) > 0.0
}

impl super::Game {
    pub(super) fn damage_from(&mut self, amount: f32, from: Vec3) {
        use super::*;
        if self.screen == Screen::Play
            && self.buttons[1]
            && self.inv.hot(self.sel) == items::SHIELD
            && self.combat.blocking >= 0.25
            && in_front(self.player.look_dir(), from - self.player.pos)
        {
            if equipment::damage(&mut self.inv.slots[self.sel], amount.ceil() as u16 + 1, self.rng.next()) {
                self.say("Your shield broke".into());
            }
            self.audio.play_var(Snd::ArrowHit, self.player.pos, 0.7);
        } else {
            self.strike(amount);
        }
    }

    pub(super) fn release_bow(&mut self) {
        use super::*;
        let Some((slot, time)) = self.combat.bow.take() else {
            return;
        };
        if time < 0.1 || self.screen != Screen::Play || self.sel != slot || self.inv.hot(slot) != items::BOW {
            return;
        }
        if self.mode != Mode::Creative && !self.inv.take(items::ARROW, 1) {
            self.say("No arrows".into());
            return;
        }
        let power = bow_power(time);
        let dir = self.player.look_dir();
        self.arrows.push(Arrow {
            pos: self.player.eye() + dir * 0.4,
            vel: dir * (40.0 * power) + self.player.vel * 0.5,
            life: 6.0,
            hostile: false,
            fire: false,
            damage: 1 + (power * 5.0).round() as i32,
        });
        self.audio.play(Snd::Bow, 0.8, 0.85 + power * 0.25);
        self.wear_held();
        self.swing = SWING;
        self.place_cd = 0.2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shield_only_blocks_the_front_and_bows_charge_monotonically() {
        assert!(in_front(Vec3::Z, Vec3::new(1.0, 8.0, 2.0)));
        assert!(!in_front(Vec3::Z, -Vec3::Z));
        assert!(!in_front(Vec3::Z, Vec3::ZERO));
        assert_eq!(bow_power(0.0), 0.0);
        assert_eq!(bow_power(2.0), 1.0);
        assert!(bow_power(0.25) < bow_power(0.75));
        assert!(attack_period(items::IRON_AXE) > attack_period(items::IRON_SWORD));
    }
}

use super::*;

impl Game {
    pub(super) fn wear_held(&mut self) {
        if self.mode == Mode::Creative {
            return;
        }
        let item = self.inv.hot(self.sel);
        if equipment::damage(&mut self.inv.slots[self.sel], 1, self.rng.next()) {
            self.say(format!("Your {} broke", items::name(item)));
        }
    }

    pub(super) fn loot(&mut self, drops: mobs::Drops, at: Vec3) {
        for (item, n) in drops {
            self.spill(at + Vec3::Y * 0.5, item, n);
        }
    }

    pub(super) fn die(&mut self) {
        self.health = 0.0;
        self.riding = None;
        let at = self.player.pos + Vec3::Y * 0.9;
        // some of the experience is left behind, the rest is lost
        let keep = (self.level * 4).min(60);
        self.orbs.spawn(&mut self.rng, at, keep);
        (self.level, self.xp) = (0, 0);
        let inv = std::mem::replace(&mut self.inv, Inventory::new());
        for st in inv.slots.into_iter().chain(inv.grid).chain([inv.held]) {
            self.pickups.pop(&mut self.rng, at, st);
        }
        self.goto(Screen::Dead);
    }

    pub(super) fn strike(&mut self, amount: f32) {
        self.hurt(amount * self.inv.armor_factor());
        for st in &mut self.inv.slots[items::ARMOR..] {
            equipment::damage(st, (amount / 4.0).floor().max(1.0) as u16, self.rng.next());
        }
    }

    pub(super) fn hurt(&mut self, amount: f32) {
        self.health -= amount;
        if amount >= 0.5 && self.hurt_flash < 0.2 {
            self.audio.play(Snd::Hurt, 0.9, 0.95 + self.rng.f() * 0.1);
        }
        self.hurt_flash = 0.35;
    }

    pub(super) fn gain_xp(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        self.xp += n;
        let pitch = 0.8 + self.rng.f() * 0.5;
        self.audio.play(Snd::Xp, 0.6, pitch);
        let mut up = false;
        while self.xp >= fx::xp_to_next(self.level) {
            self.xp -= fx::xp_to_next(self.level);
            self.level += 1;
            up = true;
        }
        if up {
            self.audio.play(Snd::LevelUp, 0.8, 1.0);
            self.say(format!("Level {}", self.level));
        }
    }

    pub(super) fn kill_xp(&mut self, kind: mobs::Kind, at: Vec3) {
        self.audio.play_var(Snd::Voice(kind, audio::Call::Death), at + Vec3::Y, 0.9);
        let n = match kind {
            mobs::Kind::Villager => 0,
            mobs::Kind::Hare => 1,
            mobs::Kind::Brute => 12,
            mobs::Kind::Wisp => 8,
            k if k.is_monster() => 5,
            mobs::Kind::Wolf => 3,
            _ => 1 + (self.rng.f() < 0.5) as u32 * 2,
        };
        if self.mode == Mode::Survival {
            self.orbs.spawn(&mut self.rng, at + Vec3::Y * 0.5, n);
        }
    }

    pub(super) fn mob_voice(kind: mobs::Kind) -> Snd {
        Snd::Voice(kind, audio::Call::Idle)
    }

    pub(super) fn mob_struck(&mut self, i: usize, at: Vec3) {
        let kind = self.mobs.list[i].kind;
        self.audio.play_var(Snd::Hit, at, 0.8);
        // the cry of the one hit (or its last, if the blow kills it: see `kill_xp`)
        if self.mobs.alive(i) {
            self.audio.play_var(Snd::Voice(kind, audio::Call::Hurt), at + Vec3::Y, 0.8);
        }
        self.particles.sparks(&mut self.rng, at + Vec3::Y * 0.7, 8);
    }

    pub(super) fn explode(&mut self, at: Vec3, radius: f32) {
        let c = at.floor().as_ivec3();
        let r = radius.ceil() as i32;
        let mut edits = Vec::new();
        for dy in -r..=r {
            for dz in -r..=r {
                for dx in -r..=r {
                    let p = c + IVec3::new(dx, dy, dz);
                    let d = (p.as_vec3() + 0.5 - at).length();
                    // a ragged edge, not a perfect ball
                    if d > radius * (0.7 + 0.3 * self.rng.f()) {
                        continue;
                    }
                    let b = self.world.get_block(p.x, p.y, p.z);
                    // (chests and furnaces are sturdy enough to keep what they hold)
                    if is_fluid(b) || matches!(b, AIR | BEDROCK | CHEST | LOOT_CHEST | FURNACE) {
                        continue;
                    }
                    edits.push((p, AIR));
                }
            }
        }
        let broken = self.world.set_many(&edits);
        let cells: Vec<IVec3> = broken.iter().map(|b| b.0).collect();
        self.check_gates(&cells);
        self.weather.dirty();
        for (p, b) in broken {
            // a third of what breaks is left to pick up
            if self.rng.f() < 0.33 && self.mode == Mode::Survival {
                let drop = items::drop_for(b, AIR);
                if drop != AIR {
                    self.pickups.pop(&mut self.rng, p.as_vec3() + 0.5, Stack::new(drop, 1));
                }
            }
            if self.rng.f() < 0.15 {
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, 3);
            }
        }
        for _ in 0..40 {
            let o = Vec3::new(self.rng.range(-1.5, 1.5), self.rng.range(-0.5, 1.5), self.rng.range(-1.5, 1.5));
            let dark = self.rng.f() < 0.5;
            self.particles.smoke(&mut self.rng, at + o, dark);
        }
        for _ in 0..16 {
            let o = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(0.0, 1.0), self.rng.range(-1.0, 1.0));
            self.particles.ember(&mut self.rng, at + o);
        }
        self.particles.sparks(&mut self.rng, at, 24);
        self.audio.play_at(Snd::Boom, at, 1.4, 0.9 + self.rng.f() * 0.2);
        // the blast: worse the closer you are, softened by armour; it throws you back
        let reach = radius * 2.0;
        let to = self.player.pos + Vec3::Y * 0.9 - at;
        let d = to.length();
        if d < reach && self.mode == Mode::Survival && self.screen != Screen::Dead {
            let k = 1.0 - d / reach;
            self.damage_from(16.0 * k, at);
            self.player.vel += to.normalize_or(Vec3::Y) * 12.0 * k + Vec3::Y * 5.0 * k;
            if self.health <= 0.0 {
                self.die();
            }
        }
        for i in 0..self.mobs.list.len() {
            let m = &self.mobs.list[i];
            let d = (m.pos + Vec3::Y * 0.5 - at).length();
            if !self.mobs.alive(i) || d >= reach {
                continue;
            }
            let (kind, pos) = (m.kind, m.pos);
            let dmg = (14.0 * (1.0 - d / reach)).ceil() as i32;
            if let Some(drops) = self.mobs.hit(i, at, dmg) {
                self.loot(drops, pos);
                self.kill_xp(kind, pos);
            }
        }
    }

    pub(super) fn survive(&mut self, input: &Input, dt: f32) {
        if self.player.impact > 13.5 {
            let d = (self.player.impact - 13.5) * 0.95;
            self.hurt(d);
        }
        if self.player.impact > 6.0 && self.mode == Mode::Survival {
            let p = (self.player.pos - Vec3::Y * 0.1).floor().as_ivec3();
            if farming::moisture(self.world.get_block(p.x, p.y, p.z)).is_some() {
                self.world.set_block(p.x, p.y, p.z, DIRT);
                let b = self.world.get_block(p.x, p.y + 1, p.z);
                if let Some((item, count)) = farming::harvest(b) {
                    self.world.set_block(p.x, p.y + 1, p.z, AIR);
                    self.spill(p.as_vec3() + Vec3::Y, item, count);
                }
            }
        }
        // smouldering rock burns the feet, unless you step carefully
        let under = (self.player.pos - Vec3::Y * 0.1).floor().as_ivec3();
        self.smoulder_t -= dt;
        if self.player.on_ground && !input.sneak && self.world.get_block(under.x, under.y, under.z) == SMOULDER && self.smoulder_t <= 0.0 {
            self.smoulder_t = 0.8;
            self.hurt(1.0);
        }
        // cactus pricks whoever brushes against it or stands on it
        self.cactus_t -= dt;
        if self.cactus_t <= 0.0 {
            let (p, r) = (self.player.pos, PLAYER_HW + 0.05);
            let at = |x: f32, y: f32, z: f32| self.world.get_block(x.floor() as i32, y.floor() as i32, z.floor() as i32) == CACTUS;
            let side =
                [(-r, -r), (r, -r), (-r, r), (r, r)].iter().any(|&(dx, dz)| at(p.x + dx, p.y + 0.3, p.z + dz) || at(p.x + dx, p.y + 1.2, p.z + dz));
            if side || (self.player.on_ground && at(p.x, p.y - 0.1, p.z)) {
                self.cactus_t = 0.5;
                self.hurt(1.0);
            }
        }
        // lava: two hearts every half second, whatever is worn
        self.lava_t -= dt;
        if self.player.in_lava && self.lava_t <= 0.0 {
            self.lava_t = 0.5;
            self.hurt(4.0);
        }
        if self.player.eye_in_water(&self.world) {
            self.air -= dt;
            if self.air <= -1.0 {
                self.air = 0.0;
                self.hurt(2.0);
            }
        } else {
            self.air = (self.air + dt * 4.0).min(MAX_AIR);
        }
        let moving = input.forward != 0.0 || input.right != 0.0;
        let effort = if !moving {
            0.0
        } else if input.sprint {
            0.035
        } else {
            0.006
        };
        self.food = (self.food - dt * (0.011 + effort)).max(0.0);
        if self.food > 13.0 && self.health < MAX_HEALTH {
            self.health = (self.health + 0.45 * dt).min(MAX_HEALTH);
            self.food -= 0.14 * dt;
        }
        if self.food <= 0.0 && self.health > 4.0 {
            self.health -= 0.25 * dt;
        }
    }

    pub(super) fn primary(&mut self, dt: f32) {
        if self.combat.blocking > 0.0 || self.combat.bow.is_some() {
            return;
        }
        let (o, d) = (self.player.eye(), self.player.look_dir());
        let tool = self.inv.hot(self.sel);
        let block = self.target();
        let (damage, reach) = items::weapon(tool);
        if let Some((i, t)) = self.boats.pick(o, d, reach) {
            let nearer = block.map_or(true, |b| t < b.2) && self.mobs.pick(o, d, reach).map_or(true, |m| t < m.1);
            if nearer {
                self.mining = None;
                if self.primary_cd <= 0.0 {
                    self.primary_cd = combat::attack_period(self.inv.hot(self.sel));
                    self.swing = SWING;
                    let at = self.boats.list[i].pos;
                    self.audio.play_var(Snd::Dig(audio::Surface::Wood), at, 0.8);
                    let creative = self.mode == Mode::Creative;
                    if self.boats.hit(i, if creative { 9.0 } else { 1.0 }) {
                        self.boats.list.remove(i);
                        self.riding = match self.riding {
                            Some(r) if r == i => None,
                            Some(r) if r > i => Some(r - 1),
                            r => r,
                        };
                        self.audio.play_var(Snd::Break(audio::Surface::Wood), at, 0.9);
                        self.particles.chips(&mut self.rng, at.floor().as_ivec3(), [168, 124, 72], 14);
                        if !creative {
                            self.spill(at + Vec3::Y * 0.4, items::BOAT, 1);
                        }
                    }
                }
                return;
            }
        }
        if let Some((i, t)) = self.mobs.pick(o, d, reach) {
            if block.map_or(true, |b| t < b.2) {
                self.mining = None;
                if self.primary_cd <= 0.0 {
                    self.primary_cd = combat::attack_period(self.inv.hot(self.sel));
                    self.hit_flash = 0.18;
                    self.swing = SWING;
                    let at = self.mobs.list[i].pos;
                    let kind = self.mobs.list[i].kind;
                    self.mob_struck(i, at);
                    let dead = self.mobs.hit(i, self.player.pos, damage);
                    self.wear_held();
                    if let Some(drops) = dead {
                        self.loot(drops, at);
                        self.kill_xp(kind, at);
                    }
                }
                return;
            }
        }
        let Some((p, _, _)) = block else {
            self.mining = None;
            return;
        };
        let b = self.world.get_block(p.x, p.y, p.z);
        if b == BEDROCK {
            self.mining = None;
            return;
        }
        let progress = match self.mining {
            Some((mp, t)) if mp == p => t + dt,
            _ => 0.0,
        };
        if self.swing <= 0.0 {
            self.swing = SWING;
        }
        if self.mode == Mode::Survival {
            self.dig_acc -= dt;
            if self.dig_acc <= 0.0 {
                self.dig_acc = 0.24;
                let c = p.as_vec3() + 0.5;
                self.audio.play_var(Snd::Dig(audio::surface(b)), c, 0.7);
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, 2);
            }
        }
        // creative breaks at once, a few blocks a second, and the blocks drop nothing
        let creative = self.mode == Mode::Creative;
        if creative && self.primary_cd > 0.0 {
            return;
        }
        if creative
            || progress
                >= items::break_time(b, tool)
                    / if items::break_time(b, tool) < items::break_time(b, AIR) {
                        equipment::mining_multiplier(self.inv.slots[self.sel])
                    } else {
                        1.0
                    }
        {
            self.mining = None;
            self.primary_cd = if creative { 0.22 } else { 0.0 };
            if self.world.set_block(p.x, p.y, p.z, AIR) {
                self.check_gates(&[p]);
                self.quiet = creative;
                self.dig_acc = 0.0;
                self.weather.dirty();
                // what it gives falls where the block was
                let drop = items::drop_for(b, tool);
                let at = p.as_vec3() + 0.5;
                self.audio.play_var(Snd::Break(audio::surface(b)), at, 0.9);
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, if is_torch(b) || is_plant(b) { 5 } else { 14 });
                if !creative && drop != AIR {
                    let xp = match b {
                        COAL_ORE => (self.rng.f() < 0.6) as u32,
                        CRYSTAL_ORE => 3 + (self.rng.f() * 4.0) as u32,
                        _ => 0,
                    };
                    self.orbs.spawn(&mut self.rng, at, xp);
                }
                if building::is_sign(b) {
                    self.signs.remove(&p);
                }
                if b == FURNACE {
                    // a broken furnace spills what it held
                    if let Some(f) = self.furnaces.remove(&p) {
                        for st in f.slots {
                            self.pickups.pop(&mut self.rng, at, st);
                        }
                        let xp = f.xp as u32;
                        self.orbs.spawn(&mut self.rng, at, xp);
                    }
                }
                match b {
                    GRAVEL if self.rng.f() < 0.25 => self.spill(at, items::FLINT, 1),
                    b if is_tuft(b) && tool != items::SHEARS => {
                        if self.rng.f() < 0.6 {
                            self.spill(at, items::FIBER, 1);
                        }
                        if self.rng.f() < 0.3 {
                            self.spill(at, items::SEEDS, 1);
                        }
                    }
                    b if farming::crop(b).is_some_and(|c| c.0 != WHEAT_0) => {
                        let (item, count) = farming::harvest(b).unwrap();
                        self.spill(at, item, count);
                    }
                    WHEAT_3 => {
                        self.spill(at, items::WHEAT, 1);
                        let seeds = 1 + (self.rng.f() < 0.5) as u32;
                        self.spill(at, items::SEEDS, seeds);
                    }
                    BERRY_BUSH => self.spill(at, drop, 2),
                    _ => self.spill(at, drop, 1),
                }
                if is_leaves(b) && self.rng.f() < 0.3 {
                    self.spill(at, items::STICK, 1);
                }
                if is_leaves(b) && tool != items::SHEARS && self.rng.f() < 0.06 {
                    self.spill(at, items::item_of(sapling_of(b)), 1);
                }
                if b == CHEST || b == LOOT_CHEST {
                    // a broken chest spills what it held
                    let held =
                        if b == LOOT_CHEST { Some(loot::roll_chest(self.world.seed, p, self.world.is_deep())) } else { self.chests.remove(&p) };
                    for st in held.into_iter().flatten() {
                        self.pickups.pop(&mut self.rng, at, st);
                    }
                }
                if drop == AIR && items::tier_needed(b) > 0 && !creative {
                    let need = if items::tier_needed(b) >= 3 { "an iron" } else { "a stone" };
                    self.say(format!("{} needs {need} pickaxe or better", items::name(b)));
                }
                if !is_plant(b) && !is_torch(b) {
                    self.wear_held();
                }
                if is_door(b) {
                    // the other half goes with it
                    let y = if door_upper(b) { p.y - 1 } else { p.y + 1 };
                    if is_door(self.world.get_block(p.x, y, p.z)) {
                        self.world.set_block(p.x, y, p.z, AIR);
                    }
                }
                // doors, torches and plants standing on the block come loose
                let above = self.world.get_block(p.x, p.y + 1, p.z);
                let up = at + Vec3::Y;
                if is_door(above) && !door_upper(above) {
                    if self.world.set_pair(p.x, p.y + 1, p.z, AIR, AIR) {
                        self.spill(up, items::door_item(above), 1);
                    }
                } else if (above == TORCH || is_plant(above)) && self.world.set_block(p.x, p.y + 1, p.z, AIR) {
                    self.spill(up, items::drop_for(above, tool), 1);
                    // (a stand of reeds falls whole)
                    let mut y = p.y + 2;
                    while above == REEDS && self.world.get_block(p.x, y, p.z) == REEDS && self.world.set_block(p.x, y, p.z, AIR) {
                        self.spill(up, items::FIBER, 1);
                        y += 1;
                    }
                }
                // and so do ladders and torches hanging on it
                for (s, (dx, dz)) in SIDE_DIR.into_iter().enumerate() {
                    let c = IVec3::new(p.x - dx, p.y, p.z - dz);
                    let hung = self.world.get_block(c.x, c.y, c.z);
                    let item = if hung == LADDER_BASE + s as Block {
                        items::LADDER
                    } else if hung == WALL_TORCH + s as Block {
                        TORCH
                    } else {
                        continue;
                    };
                    if self.world.set_block(c.x, c.y, c.z, AIR) {
                        self.spill(c.as_vec3() + 0.5, item, 1);
                    }
                }
                self.quiet = false;
            }
        } else {
            self.mining = Some((p, progress));
        }
    }
}
