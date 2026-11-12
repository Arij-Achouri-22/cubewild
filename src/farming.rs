//! Bounded random crop ticks. Water reaches four cells (including diagonals),
//! moisture lasts through short dry spells, and crops need light level nine.
use crate::items::{self, Item};
use crate::world::*;
use glam::IVec3;

pub const WET_FARMLAND: Block = 258; // moisture 1..7
pub const CARROTS: Block = 272; // four growth stages
pub const POTATOES: Block = 276;

pub fn moisture(b: Block) -> Option<u8> {
    if b == FARMLAND {
        Some(0)
    } else if (WET_FARMLAND..WET_FARMLAND + 7).contains(&b) {
        Some((b - WET_FARMLAND + 1) as u8)
    } else {
        None
    }
}
pub fn crop(b: Block) -> Option<(Block, u8)> {
    if (WHEAT_0..=WHEAT_3).contains(&b) {
        Some((WHEAT_0, (b - WHEAT_0) as u8))
    } else if (CARROTS..CARROTS + 4).contains(&b) {
        Some((CARROTS, (b - CARROTS) as u8))
    } else if (POTATOES..POTATOES + 4).contains(&b) {
        Some((POTATOES, (b - POTATOES) as u8))
    } else {
        None
    }
}
pub fn plant(item: Item) -> Option<Block> {
    match item {
        items::SEEDS => Some(WHEAT_0),
        items::CARROT => Some(CARROTS),
        items::POTATO => Some(POTATOES),
        _ => None,
    }
}
pub fn harvest(b: Block) -> Option<(Item, u32)> {
    let (base, stage) = crop(b)?;
    let item = if base == WHEAT_0 {
        if stage == 3 { items::WHEAT } else { items::SEEDS }
    } else if base == CARROTS {
        items::CARROT
    } else {
        items::POTATO
    };
    Some((item, if stage == 3 && base != WHEAT_0 { 3 } else { 1 }))
}
pub fn irrigated(w: &World, p: IVec3) -> bool {
    (p.z - 4..=p.z + 4).any(|z| (p.x - 4..=p.x + 4).any(|x| is_water(w.get_block(x, p.y, z)) || is_water(w.get_block(x, p.y + 1, z))))
}

pub fn tick(w: &mut World, p: IVec3, daylight: f32, rain: bool, roll: u32) {
    let b = w.get_block(p.x, p.y, p.z);
    if let Some(m) = moisture(b) {
        // Unknown borders must not dry a field whose irrigation is still loading.
        if !w.area_loaded(p.x as f32 + 0.5, p.z as f32 + 0.5, 4.5) {
            return;
        }
        let wet = irrigated(w, p) || (rain && w.sky_level(p.x, p.y + 1, p.z) > 0);
        let above = w.get_block(p.x, p.y + 1, p.z);
        let next = if is_solid(above) && !is_shaped(above) {
            DIRT
        } else if wet {
            WET_FARMLAND + 6
        } else if m > 1 {
            WET_FARMLAND + m as Block - 2
        } else if m == 1 || crop(above).is_some() {
            FARMLAND
        } else {
            DIRT
        };
        w.set_block_bg(p.x, p.y, p.z, next);
    } else if let Some((_, stage)) = crop(b) {
        if moisture(w.get_block(p.x, p.y - 1, p.z)).is_none() {
            return;
        }
        let Some(block) = w.block_light(p.x, p.y, p.z) else {
            return;
        };
        let sky = if w.sky_level(p.x, p.y, p.z) > 0 { (daylight * 15.0).round() as u8 } else { 0 };
        if stage < 3 && block.max(sky) >= 9 {
            let moist = moisture(w.get_block(p.x, p.y - 1, p.z)).unwrap_or(0) > 0;
            if roll % if moist { 2 } else { 6 } == 0 {
                w.set_block_bg(p.x, p.y, p.z, b + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn irrigation_reaches_four_blocks_and_dry_planted_soil_survives() {
        let mut w = World::flat(|x, y, z| {
            if (x, y, z) == (4, 10, 4) {
                Some(WATER)
            } else if (x, y, z) == (0, 10, 0) {
                Some(FARMLAND)
            } else {
                None
            }
        });
        assert!(irrigated(&w, IVec3::new(0, 10, 0)));
        assert!(!irrigated(&w, IVec3::new(-1, 10, 0)));
        tick(&mut w, IVec3::new(0, 10, 0), 1.0, false, 0);
        assert_eq!(moisture(w.get_block(0, 10, 0)), Some(7));
        w.set_block(4, 10, 4, AIR);
        w.set_block(0, 11, 0, CARROTS);
        for _ in 0..8 {
            tick(&mut w, IVec3::new(0, 10, 0), 1.0, false, 0);
        }
        assert_eq!(w.get_block(0, 10, 0), FARMLAND);
        w.set_block(0, 11, 0, AIR);
        tick(&mut w, IVec3::new(0, 10, 0), 1.0, false, 0);
        assert_eq!(w.get_block(0, 10, 0), DIRT);
    }

    #[test]
    fn crops_need_light_and_moisture_changes_the_growth_rate() {
        let mut w = World::flat(|x, y, z| match (x, y, z) {
            (0, 10, 0) => Some(WET_FARMLAND + 6),
            (0, 11, 0) => Some(CARROTS),
            _ => None,
        });
        w.bake_test_light();
        tick(&mut w, IVec3::new(0, 11, 0), 0.0, false, 0);
        assert_eq!(w.get_block(0, 11, 0), CARROTS, "night cannot grow an unlit crop");
        tick(&mut w, IVec3::new(0, 11, 0), 1.0, false, 0);
        assert_eq!(w.get_block(0, 11, 0), CARROTS + 1);
        w.set_block(0, 10, 0, FARMLAND);
        w.bake_test_light();
        tick(&mut w, IVec3::new(0, 11, 0), 1.0, false, 2);
        assert_eq!(w.get_block(0, 11, 0), CARROTS + 1, "dry soil grows less often");
        w.set_block(0, 10, 0, WET_FARMLAND + 6);
        w.bake_test_light();
        tick(&mut w, IVec3::new(0, 11, 0), 1.0, false, 2);
        assert_eq!(w.get_block(0, 11, 0), CARROTS + 2);
    }

    #[test]
    fn torchlight_grows_crops_in_a_roofed_room() {
        let mut w = World::flat(|x, y, z| {
            if y == 14 {
                Some(STONE)
            } else if (x, y, z) == (0, 10, 0) {
                Some(WET_FARMLAND + 6)
            } else if (x, y, z) == (0, 11, 0) {
                Some(POTATOES)
            } else if (x, y, z) == (1, 11, 0) {
                Some(TORCH)
            } else {
                None
            }
        });
        w.bake_test_light();
        tick(&mut w, IVec3::new(0, 11, 0), 0.0, false, 0);
        assert_eq!(w.get_block(0, 11, 0), POTATOES + 1);
    }
}
