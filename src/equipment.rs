//! Equipment progression and transactions, independent of rendering and menus.
use crate::items::{self::*, Kind, Mat};

#[derive(Clone, Copy, Debug)]
pub enum Enchant {
    Efficiency,
    Unbreaking,
    Protection,
}

impl Enchant {
    pub const ALL: [Self; 3] = [Self::Efficiency, Self::Unbreaking, Self::Protection];
    pub fn name(self) -> &'static str {
        match self {
            Self::Efficiency => "Efficiency",
            Self::Unbreaking => "Unbreaking",
            Self::Protection => "Protection",
        }
    }
    fn shift(self) -> u8 {
        match self {
            Self::Efficiency => 0,
            Self::Unbreaking => 2,
            Self::Protection => 4,
        }
    }
    pub fn level(self, st: Stack) -> u8 {
        (st.enchants >> self.shift()) & 3
    }
    pub fn accepts(self, item: Item) -> bool {
        match self {
            Self::Efficiency => matches!(gear(item), Some((Kind::Pick | Kind::Axe | Kind::Shovel | Kind::Hoe, _))),
            Self::Unbreaking => max_wear(item) > 0,
            Self::Protection => armor(item).is_some(),
        }
    }
}

pub fn repair_material(item: Item) -> Option<Item> {
    Some(match gear(item).map(|g| g.1) {
        Some(Mat::Wood) => crate::world::PLANKS,
        Some(Mat::Leather) => crate::world::LEATHER,
        Some(Mat::Stone) => crate::world::COBBLE,
        Some(Mat::Iron) => IRON_INGOT,
        Some(Mat::Gold) => GOLD_INGOT,
        Some(Mat::Crystal) => CRYSTAL,
        Some(Mat::Pyre) => PYRE_INGOT,
        None => match item {
            BOW | FISHING_ROD => FIBER,
            SHEARS | STRIKER => IRON_INGOT,
            SHIELD => crate::world::PLANKS,
            _ => return None,
        },
    })
}

/// An anvil repairs a quarter of maximum durability per material and level.
pub fn repair(inv: &mut Inventory, levels: &mut u32, creative: bool) -> Result<(), &'static str> {
    let st = inv.held;
    let material = repair_material(st.item).ok_or("Put damaged equipment on the cursor")?;
    if st.is_empty() || st.wear == 0 {
        return Err("This equipment needs no repair");
    }
    if !creative && (*levels < 1 || inv.count(material) < 1) {
        return Err("Repair needs one level and one matching material");
    }
    if !creative {
        inv.take(material, 1);
        *levels -= 1;
    }
    inv.held.wear = st.wear.saturating_sub(max_wear(st.item).div_ceil(4));
    Ok(())
}

pub fn enchant(inv: &mut Inventory, levels: &mut u32, kind: Enchant, creative: bool) -> Result<(), &'static str> {
    let st = inv.held;
    let old = kind.level(st);
    if st.is_empty() || !kind.accepts(st.item) {
        return Err("This enchantment does not fit that equipment");
    }
    if old >= 3 {
        return Err("This enchantment is already level III");
    }
    let cost = (old as u32 + 1) * 3;
    if !creative && (*levels < cost || inv.count(CRYSTAL) < 1) {
        return Err("Enchanting needs crystal and 3, 6 or 9 levels");
    }
    if !creative {
        inv.take(CRYSTAL, 1);
        *levels -= cost;
    }
    inv.held.enchants = (st.enchants & !(3 << kind.shift())) | ((old + 1) << kind.shift());
    Ok(())
}

pub fn mining_multiplier(st: Stack) -> f32 {
    1.0 + Enchant::Efficiency.level(st) as f32 * 0.45
}

/// Unbreaking keeps the familiar 1/(level+1) chance to spend a use.
pub fn damage(st: &mut Stack, uses: u16, roll: u32) -> bool {
    if st.is_empty() || max_wear(st.item) == 0 {
        return false;
    }
    if roll % (Enchant::Unbreaking.level(*st) as u32 + 1) != 0 {
        return false;
    }
    st.wear = st.wear.saturating_add(uses);
    if st.wear >= max_wear(st.item) {
        *st = Stack::default();
        return true;
    }
    false
}

/// Upgrading crystal gear to pyre retains its enchantments and damage.
pub fn craft_output(recipe: &Recipe, inputs: &[Stack]) -> Stack {
    let mut out = Stack::new(recipe.out.0, recipe.out.1);
    if gear(out.item).is_some() {
        if let Some(st) = inputs.iter().find(|s| !s.is_empty() && gear(s.item).is_some() && recipe.ins.iter().any(|&(i, _)| i == s.item)) {
            out.enchants = st.enchants;
            out.wear = st.wear.min(max_wear(out.item).saturating_sub(1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_and_enchant_preserve_metadata_and_fail_atomically() {
        let mut inv = Inventory::new();
        inv.held = Stack { wear: 100, ..Stack::new(IRON_PICK, 1) };
        let mut levels = 10;
        assert!(repair(&mut inv, &mut levels, false).is_err());
        assert_eq!((inv.held.wear, levels), (100, 10));
        inv.add(IRON_INGOT, 1);
        inv.add(CRYSTAL, 1);
        enchant(&mut inv, &mut levels, Enchant::Efficiency, false).unwrap();
        repair(&mut inv, &mut levels, false).unwrap();
        assert_eq!((inv.held.wear, levels, Enchant::Efficiency.level(inv.held)), (37, 6, 1));
        let encoded = inv.held.persist_wear();
        assert_eq!(crate::loot::parse_stack(IRON_PICK as u32, 1, encoded).unwrap(), inv.held);
        assert!(enchant(&mut inv, &mut levels, Enchant::Protection, false).is_err());
    }

    #[test]
    fn armour_and_shields_wear_and_unbreaking_can_prevent_wear() {
        for item in [IRON_CHEST, SHIELD] {
            let mut st = Stack::new(item, 1);
            st.enchants = 3 << 2;
            assert!(!damage(&mut st, 1, 1));
            assert_eq!(st.wear, 0);
            assert!(!damage(&mut st, 1, 0));
            assert_eq!(st.wear, 1);
            st.wear = max_wear(item) - 1;
            assert!(damage(&mut st, 1, 0));
            assert!(st.is_empty());
        }
    }

    #[test]
    fn upgrading_keeps_damage_and_enchantments_in_both_crafting_modes() {
        let recipe = RECIPES.iter().find(|r| r.out.0 == PYRE_PICK).unwrap();
        let source = Stack { wear: 72, enchants: 5, ..Stack::new(CRYSTAL_PICK, 1) };
        let mut inv = Inventory::new();
        inv.slots[0] = source;
        inv.add(PYRE_INGOT, 3);
        assert!(inv.craft(recipe));
        let made = inv.slots.iter().find(|s| s.item == PYRE_PICK).unwrap();
        assert_eq!((made.wear, made.enchants), (72, 5));
        let mut inv = Inventory::new();
        inv.grid[0] = source;
        for i in 1..4 {
            inv.grid[i] = Stack::new(PYRE_INGOT, 1);
        }
        assert!(crate::crafting::craft(&mut inv, 3, true, false));
        assert_eq!((inv.held.wear, inv.held.enchants), (72, 5));
    }
}
