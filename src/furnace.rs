//! Furnaces: fuel burns down, ore and food cook one at a time, and it all keeps going
//! while the player is away (as long as the furnace's place is remembered).

use crate::items::{self, Item, Stack};
use crate::world::*;
use glam::IVec3;
use std::collections::HashMap;
use std::path::Path;

/// slots of a furnace
pub const INPUT: usize = 0;
pub const FUEL: usize = 1;
pub const OUTPUT: usize = 2;
/// seconds to cook one item
pub const COOK: f32 = 6.0;

#[derive(Clone, Copy, Default)]
pub struct Furnace {
    pub slots: [Stack; 3],
    /// seconds of fire left, and how long the last piece of fuel lasted
    pub burn: f32,
    pub burn_max: f32,
    /// seconds the item in the input has been cooking
    pub cook: f32,
    /// experience earned by what it made, handed out when the output is taken
    pub xp: f32,
}

pub type Furnaces = HashMap<IVec3, Furnace>;

/// What an item cooks into, and how many.
pub fn smelt(i: Item) -> Option<(Item, u32)> {
    Some(match i {
        IRON_ORE => (items::IRON_INGOT, 1),
        GOLD_ORE => (items::GOLD_INGOT, 1),
        LOG | BIRCH_LOG => (items::COAL, 1),
        items::MEAT => (items::COOKED_MEAT, 1),
        items::RAW_FISH => (items::COOKED_FISH, 1),
        COBBLE => (STONE, 1),
        SAND | RED_SAND => (GLASS, 1),
        CLAY => (BRICK, 2),
        _ => return None,
    })
}

/// Experience a cooked item gives.
pub fn smelt_xp(i: Item) -> f32 {
    match i {
        items::GOLD_INGOT => 1.0,
        items::IRON_INGOT => 0.7,
        items::COOKED_MEAT | items::COOKED_FISH => 0.35,
        _ => 0.1,
    }
}

/// Seconds an item burns for (0 = it is no fuel).
pub fn fuel(i: Item) -> f32 {
    match i {
        items::COAL => 80.0,
        items::EMBER => 200.0,
        i if i == items::DEEP_ITEM + 6 || i == items::DEEP_ITEM + 8 => 15.0,
        LOG | BIRCH_LOG | PLANKS | BIRCH_PLANKS | WORKBENCH | CHEST | FENCE | BIRCH_FENCE => 15.0,
        items::LADDER | items::TRAPDOOR | items::OAK_DOOR | items::BIRCH_DOOR => 12.0,
        i if (SLAB..SLAB + 2).contains(&i) => 7.5,
        items::STICK => 5.0,
        items::FIBER | DEAD_BUSH => 2.5,
        i => match items::gear(i) {
            Some((_, items::Mat::Wood)) => 10.0,
            _ => 0.0,
        },
    }
}

impl Furnace {
    pub fn lit(&self) -> bool {
        self.burn > 0.0
    }

    fn can_cook(&self) -> bool {
        let Some((out, n)) = smelt(self.slots[INPUT].item).filter(|_| !self.slots[INPUT].is_empty()) else { return false };
        let o = self.slots[OUTPUT];
        o.is_empty() || (o.item == out && o.n + n <= items::max_stack(out))
    }

    /// Runs the furnace for `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let mut dt = dt;
        // (a long `dt` after loading is worked through in cook-sized steps)
        while dt > 0.0 {
            let step = dt.min(COOK);
            dt -= step;
            if self.burn <= 0.0 && self.can_cook() {
                let f = &mut self.slots[FUEL];
                let secs = fuel(f.item);
                if f.is_empty() || secs <= 0.0 {
                    self.cook = (self.cook - step * 2.0).max(0.0);
                    continue;
                }
                f.n -= 1;
                if f.n == 0 {
                    // a bucket of lava would leave its bucket; nothing here does
                    *f = Stack::default();
                }
                self.burn = secs;
                self.burn_max = secs;
            }
            if self.burn <= 0.0 {
                self.cook = (self.cook - step * 2.0).max(0.0);
                continue;
            }
            let burnt = step.min(self.burn);
            self.burn -= burnt;
            if self.can_cook() {
                self.cook += burnt;
                if self.cook >= COOK {
                    self.cook = 0.0;
                    let input = self.slots[INPUT].item;
                    let (out, n) = smelt(input).unwrap();
                    self.slots[INPUT].n -= 1;
                    if self.slots[INPUT].n == 0 {
                        self.slots[INPUT] = Stack::default();
                    }
                    let o = &mut self.slots[OUTPUT];
                    if o.is_empty() {
                        *o = Stack::new(out, n);
                    } else {
                        o.n += n;
                    }
                    self.xp += smelt_xp(out) * n as f32;
                }
            } else {
                self.cook = 0.0;
            }
        }
    }
}

pub fn save(furnaces: &Furnaces, path: &Path) {
    let mut s = String::new();
    for (p, f) in furnaces {
        if f.slots.iter().all(|st| st.is_empty()) && f.burn <= 0.0 {
            continue;
        }
        s += &format!("{} {} {} {} {} {} {}", p.x, p.y, p.z, f.burn, f.burn_max, f.cook, f.xp);
        for (i, st) in f.slots.iter().enumerate().filter(|(_, st)| !st.is_empty()) {
            s += &format!(" {}:{}:{}:{}", i, st.item, st.n, st.wear);
        }
        s.push('\n');
    }
    if let Err(e) = std::fs::write(path, s) {
        eprintln!("cubewild: failed to save furnaces: {e}");
    }
}

pub fn load(path: &Path) -> Furnaces {
    let mut all = Furnaces::default();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let mut parts = line.split_whitespace();
        let pos: Vec<i32> = parts.by_ref().take(3).filter_map(|t| t.parse().ok()).collect();
        let nums: Vec<f32> = parts.by_ref().take(4).filter_map(|t| t.parse().ok()).collect();
        if pos.len() < 3 || nums.len() < 4 {
            continue;
        }
        let mut f = Furnace { burn: nums[0].max(0.0), burn_max: nums[1].max(0.0), cook: nums[2].clamp(0.0, COOK), xp: nums[3].max(0.0), ..Default::default() };
        for t in parts {
            let v: Vec<u32> = t.split(':').filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 4 && (v[0] as usize) < 3 {
                if let Some(st) = crate::loot::parse_stack(v[1], v[2], v[3]) {
                    f.slots[v[0] as usize] = st;
                }
            }
        }
        all.insert(IVec3::new(pos[0], pos[1], pos[2]), f);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coal_cooks_ore_until_the_fire_dies() {
        let mut f = Furnace::default();
        f.slots[INPUT] = Stack::new(IRON_ORE, 20);
        f.slots[FUEL] = Stack::new(items::COAL, 1);
        f.tick(COOK * 5.0 + 0.1);
        assert_eq!(f.slots[OUTPUT].item, items::IRON_INGOT);
        assert_eq!(f.slots[OUTPUT].n, 5);
        assert!(f.slots[FUEL].is_empty() && f.lit());
        // a coal lasts 80 s: about 13 items
        f.tick(200.0);
        assert_eq!(f.slots[OUTPUT].n, (80.0 / COOK) as u32);
        assert!(!f.lit());
    }

    #[test]
    fn nothing_burns_without_something_to_cook() {
        let mut f = Furnace::default();
        f.slots[FUEL] = Stack::new(items::COAL, 3);
        f.tick(10.0);
        assert_eq!(f.slots[FUEL].n, 3);
        assert!(!f.lit());
    }
}
