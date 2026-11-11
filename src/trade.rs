//! What villagers trade, by what they do (read from how they are dressed). Gems are
//! the money: villagers buy what you gather for them, and sell finished goods.

use crate::items::*;
use crate::world::*;

pub struct Offer {
    /// what the player gives (the second may be nothing: `(AIR, 0)`)
    pub give: [(Item, u32); 2],
    pub get: (Item, u32),
    /// times it can be made before the villager runs out (they restock each morning)
    pub uses: u32,
}

const fn o(give: (Item, u32), get: (Item, u32), uses: u32) -> Offer {
    Offer { give: [give, (AIR, 0)], get, uses }
}
const fn o2(give: (Item, u32), more: (Item, u32), get: (Item, u32), uses: u32) -> Offer {
    Offer { give: [give, more], get, uses }
}

/// The trades, in the order of the villagers' tunic colours.
pub const PROFESSIONS: [&str; 6] = ["Hunter", "Scholar", "Farmer", "Smith", "Weaver", "Mason"];

const HUNTER: &[Offer] = &[
        o((MEAT, 10), (GEM, 1), 12),
        o((HIDE, 6), (GEM, 1), 12),
        o((BONE, 8), (GEM, 1), 8),
        o((GEM, 1), (COOKED_MEAT, 5), 10),
        o((GEM, 2), (ARROW, 12), 8),
        o((GEM, 4), (BOW, 1), 3),
        o((GEM, 3), (LEATHER_CHEST, 1), 3),
];
const SCHOLAR: &[Offer] = &[
        o((GLASS, 6), (GEM, 1), 10),
        o((GEM, 1), (TORCH, 8), 12),
        o((GEM, 2), (LAMP, 1), 6),
        o((GEM, 3), (COMPASS, 1), 3),
        o((GEM, 3), (CLOCK, 1), 3),
        o((GEM, 2), (BOAT, 1), 3),
];
const FARMER: &[Offer] = &[
        o((WHEAT, 18), (GEM, 1), 16),
        o((BERRIES, 16), (GEM, 1), 12),
        o((GEM, 1), (BREAD, 4), 12),
        o((GEM, 1), (SEEDS, 12), 8),
        o((GEM, 2), (STEW, 2), 6),
];
const SMITH: &[Offer] = &[
        o((COAL, 14), (GEM, 1), 16),
        o((IRON_INGOT, 4), (GEM, 1), 12),
        o((GEM, 6), (IRON_PICK, 1), 3),
        o((GEM, 5), (IRON_SWORD, 1), 3),
        o((GEM, 4), (IRON_HELMET, 1), 3),
        o((GEM, 9), (IRON_CHEST, 1), 3),
        o2((GEM, 14), (IRON_INGOT, 3), (CRYSTAL_PICK, 1), 1),
];
const WEAVER: &[Offer] = &[
        o((CLOTH, 12), (GEM, 1), 16),
        o((GEM, 2), (SHEARS, 1), 3),
        o((GEM, 1), (DYE + 14, 4), 8),
        o((GEM, 1), (DYE + 11, 4), 8),
        o((GEM, 1), (DYE + 4, 4), 8),
        o((GEM, 2), (BED, 1), 4),
];
const MASON: &[Offer] = &[
        o((CLAY, 10), (GEM, 1), 12),
        o((COBBLE, 32), (GEM, 1), 16),
        o((GEM, 1), (STONE_BRICK, 8), 12),
        o((GEM, 1), (BRICK, 8), 12),
        o((GEM, 1), (TERRACOTTA, 8), 12),
        o((GEM, 1), (GLASS, 6), 8),
];

pub fn offers(profession: u32) -> &'static [Offer] {
    match profession {
        0 => HUNTER,
        1 => SCHOLAR,
        2 => FARMER,
        3 => SMITH,
        4 => WEAVER,
        _ => MASON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_trade_deals_in_real_items() {
        for p in 0..6 {
            for of in offers(p) {
                assert!(valid(of.give[0].0) && of.give[0].1 > 0 && valid(of.get.0) && of.get.1 > 0 && of.uses > 0);
                assert!(of.give[1].1 == 0 || valid(of.give[1].0));
                assert!(of.get.1 <= max_stack(of.get.0));
            }
        }
    }
}
