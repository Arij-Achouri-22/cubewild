//! Items, the slot inventory, crafting recipes, block drops and mining times.

use crate::world::*;

/// Block items share their id with the block. Other items start at 64.
pub type Item = u8;
pub const STICK: Item = 64;
pub const MEAT: Item = 65;
pub const HIDE: Item = 66;
pub const WOOD_PICK: Item = 67;
pub const STONE_PICK: Item = 68;
pub const STONE_AXE: Item = 69;
pub const SPEAR: Item = 70;
pub const COAL: Item = 71;
pub const IRON_INGOT: Item = 72;
pub const CRYSTAL: Item = 73;
pub const COOKED_MEAT: Item = 74;
pub const IRON_PICK: Item = 75;
pub const IRON_AXE: Item = 76;
pub const IRON_SWORD: Item = 77;
pub const CRYSTAL_PICK: Item = 78;
pub const FLINT: Item = 79;
pub const GOLD_INGOT: Item = 80;
pub const FIBER: Item = 81;
pub const BERRIES: Item = 82;
pub const STEW: Item = 83;
pub const LEATHER_CHEST: Item = 84;
pub const IRON_CHEST: Item = 85;
pub const FLINT_KNIFE: Item = 86;
pub const GOLD_PICK: Item = 87;
pub const WOOD_AXE: Item = 88;
pub const GOLD_AXE: Item = 89;
pub const CRYSTAL_AXE: Item = 90;
pub const WOOD_SHOVEL: Item = 91;
pub const STONE_SHOVEL: Item = 92;
pub const IRON_SHOVEL: Item = 93;
pub const GOLD_SHOVEL: Item = 94;
pub const CRYSTAL_SHOVEL: Item = 95;
pub const WOOD_HOE: Item = 96;
pub const STONE_HOE: Item = 97;
pub const IRON_HOE: Item = 98;
pub const WOOD_SWORD: Item = 99;
pub const STONE_SWORD: Item = 100;
pub const GOLD_SWORD: Item = 101;
pub const CRYSTAL_SWORD: Item = 102;
pub const SHEARS: Item = 103;
pub const BUCKET: Item = 104;
pub const WATER_BUCKET: Item = 105;
pub const FISHING_ROD: Item = 106;
pub const RAW_FISH: Item = 107;
pub const COOKED_FISH: Item = 108;
pub const BOW: Item = 109;
pub const ARROW: Item = 110;
pub const COMPASS: Item = 111;
pub const CLOCK: Item = 112;
pub const SEEDS: Item = 113;
pub const WHEAT: Item = 114;
pub const BREAD: Item = 115;
pub const LEATHER_HELMET: Item = 116;
pub const LEATHER_LEGS: Item = 117;
pub const LEATHER_BOOTS: Item = 118;
pub const IRON_HELMET: Item = 119;
pub const IRON_LEGS: Item = 120;
pub const IRON_BOOTS: Item = 121;
pub const GOLD_HELMET: Item = 122;
pub const GOLD_CHEST: Item = 123;
pub const GOLD_LEGS: Item = 124;
pub const GOLD_BOOTS: Item = 125;
pub const CRYSTAL_HELMET: Item = 126;
pub const CRYSTAL_CHEST: Item = 127;
pub const CRYSTAL_LEGS: Item = 128;
pub const CRYSTAL_BOOTS: Item = 129;
pub const OAK_DOOR: Item = 130;
pub const BIRCH_DOOR: Item = 131;
pub const LADDER: Item = 132;
pub const TRAPDOOR: Item = 133;
/// oak, birch, cobblestone, stone brick (the order of `SHAPE_FULL`)
pub const STAIRS: Item = 134;
pub const BOAT: Item = 138;
/// dyes, one per colour of `COLOURS` (+ colour)
pub const DYE: Item = 139;
/// from skeletons: tames wolves, grinds into white dye
pub const BONE: Item = 155;
/// what villagers trade in
pub const GEM: Item = 156;
/// dyed wool (+ colour - 1; white wool is `CLOTH`)
pub const WOOL_ITEM: Item = 157;
/// the blocks of the Emberdeep (+ n of `world::CINDERSTONE` + n)
pub const DEEP_ITEM: Item = 172;
/// lights an Ember Gate
pub const STRIKER: Item = 184;
/// from ember ore: a fuel that burns a long time
pub const EMBER: Item = 185;
/// slate, fortress bricks (whole, cracked), pyre ore (+ n of `world::SLATE` + n)
pub const EXTRA_ITEM: Item = 186;
/// the rare metal of the Emberdeep, and the best gear there is
pub const PYRE_INGOT: Item = 190;
pub const PYRE_PICK: Item = 191;
pub const PYRE_AXE: Item = 192;
pub const PYRE_SHOVEL: Item = 193;
pub const PYRE_SWORD: Item = 194;
pub const PYRE_HELMET: Item = 195;
pub const PYRE_CHEST: Item = 196;
pub const PYRE_LEGS: Item = 197;
pub const PYRE_BOOTS: Item = 198;
/// oak, birch, pine, jungle, acacia saplings (+ n of `world::SAPLING` + n)
pub const SAPLING_ITEM: Item = 199;
pub const LAVA_BUCKET: Item = 204;
pub const ITEM_END: Item = 205;
/// slots in a chest
pub const CHEST_SLOTS: usize = 27;
/// first texture layer holding item icons
const ICON_BASE: u32 = crate::textures::ICON_BASE as u32;
pub const ICON_HEART: u32 = ICON_BASE + 100;
pub const ICON_FOOD: u32 = ICON_BASE + 101;
pub const ICON_AIR: u32 = ICON_BASE + 102;

pub fn is_block(i: Item) -> bool {
    (i != AIR && i < 64) || is_wool_item(i) || (DEEP_ITEM..DEEP_ITEM + DEEP_BLOCKS).contains(&i) || is_extra_item(i) || is_sapling_item(i)
}

pub fn is_sapling_item(i: Item) -> bool {
    (SAPLING_ITEM..SAPLING_ITEM + 5).contains(&i)
}

pub fn is_extra_item(i: Item) -> bool {
    (EXTRA_ITEM..EXTRA_ITEM + EXTRA_BLOCKS).contains(&i)
}

pub fn is_wool_item(i: Item) -> bool {
    (WOOL_ITEM..WOOL_ITEM + 15).contains(&i)
}

/// The block an item puts down (most block items are their own block).
pub fn block_of(i: Item) -> Block {
    if is_wool_item(i) {
        WOOL + (i - WOOL_ITEM)
    } else if (DEEP_ITEM..DEEP_ITEM + DEEP_BLOCKS).contains(&i) {
        CINDERSTONE + (i - DEEP_ITEM)
    } else if is_extra_item(i) {
        SLATE + (i - EXTRA_ITEM)
    } else if is_sapling_item(i) {
        SAPLING + (i - SAPLING_ITEM)
    } else {
        i
    }
}

/// The colour of a dye item.
pub fn dye_colour(i: Item) -> Option<u8> {
    (DYE..DYE + 16).contains(&i).then(|| i - DYE)
}

pub fn valid(i: Item) -> bool {
    (i >= GRASS && i <= LAST_BLOCK && i != WATER && i != BEDROCK && !(WHEAT_0..=WHEAT_3).contains(&i))
        || (ICE..=FLOWER_BLUE).contains(&i)
        || (64..ITEM_END).contains(&i)
}

pub fn name(i: Item) -> &'static str {
    match i {
        GRASS => "Grass",
        ICE => "Ice",
        RED_SAND => "Red Sand",
        TERRACOTTA => "Terracotta",
        TERRACOTTA_PALE => "Pale Terracotta",
        TERRACOTTA_DARK => "Dark Terracotta",
        FLOWER_BLUE => "Blue Flower",
        DIRT => "Dirt",
        STONE => "Stone",
        SAND => "Sand",
        LOG => "Log",
        LEAVES => "Leaves",
        PLANKS => "Planks",
        COBBLE => "Cobblestone",
        BRICK => "Bricks",
        SNOW => "Snow",
        GLASS => "Glass",
        STONE_BRICK => "Stone Bricks",
        LEATHER => "Leather Block",
        COAL_ORE => "Coal Ore",
        IRON_ORE => "Iron Ore",
        CRYSTAL_ORE => "Crystal Ore",
        SANDSTONE => "Sandstone",
        CACTUS => "Cactus",
        WORKBENCH => "Workbench",
        FURNACE => "Furnace",
        TORCH => "Torch",
        LAMP => "Crystal Lamp",
        BED => "Bed",
        BIRCH_LOG => "Birch Log",
        BIRCH_LEAVES => "Birch Leaves",
        PINE_LEAVES => "Pine Needles",
        GRAVEL => "Gravel",
        CLAY => "Clay",
        GOLD_ORE => "Gold Ore",
        TALL_GRASS => "Grass Tuft",
        FLOWER_RED => "Red Flower",
        FLOWER_YELLOW => "Yellow Flower",
        MUSHROOM => "Mushroom",
        BERRY_BUSH => "Berry Bush",
        DEAD_BUSH => "Dry Bush",
        BIRCH_PLANKS => "Birch Planks",
        GOLD_BLOCK => "Gold Block",
        IRON_BLOCK => "Iron Block",
        MOSSY_COBBLE => "Mossy Cobblestone",
        CLOTH => "White Wool",
        THATCH => "Thatch",
        FARMLAND => "Farmland",
        PANE => "Window Pane",
        SLAB => "Oak Slab",
        51 => "Birch Slab",
        52 => "Cobblestone Slab",
        53 => "Stone Brick Slab",
        FENCE => "Oak Fence",
        BIRCH_FENCE => "Birch Fence",
        CHEST => "Chest",
        LADDER => "Ladder",
        TRAPDOOR => "Trapdoor",
        STAIRS => "Oak Stairs",
        135 => "Birch Stairs",
        136 => "Cobblestone Stairs",
        137 => "Stone Brick Stairs",
        OAK_DOOR => "Oak Door",
        BIRCH_DOOR => "Birch Door",
        STICK => "Stick",
        MEAT => "Raw Meat",
        HIDE => "Hide",
        WOOD_PICK => "Wooden Pickaxe",
        STONE_PICK => "Stone Pickaxe",
        STONE_AXE => "Stone Axe",
        SPEAR => "Spear",
        COAL => "Coal",
        IRON_INGOT => "Iron Ingot",
        CRYSTAL => "Crystal",
        COOKED_MEAT => "Cooked Meat",
        IRON_PICK => "Iron Pickaxe",
        IRON_AXE => "Iron Axe",
        IRON_SWORD => "Iron Sword",
        CRYSTAL_PICK => "Crystal Pickaxe",
        FLINT => "Flint",
        GOLD_INGOT => "Gold Ingot",
        FIBER => "Plant Fibre",
        BERRIES => "Berries",
        STEW => "Forest Stew",
        LEATHER_CHEST => "Leather Tunic",
        IRON_CHEST => "Iron Chestplate",
        FLINT_KNIFE => "Flint Knife",
        GOLD_PICK => "Golden Pickaxe",
        WOOD_AXE => "Wooden Axe",
        GOLD_AXE => "Golden Axe",
        CRYSTAL_AXE => "Crystal Axe",
        WOOD_SHOVEL => "Wooden Shovel",
        STONE_SHOVEL => "Stone Shovel",
        IRON_SHOVEL => "Iron Shovel",
        GOLD_SHOVEL => "Golden Shovel",
        CRYSTAL_SHOVEL => "Crystal Shovel",
        WOOD_HOE => "Wooden Hoe",
        STONE_HOE => "Stone Hoe",
        IRON_HOE => "Iron Hoe",
        WOOD_SWORD => "Wooden Sword",
        STONE_SWORD => "Stone Sword",
        GOLD_SWORD => "Golden Sword",
        CRYSTAL_SWORD => "Crystal Sword",
        SHEARS => "Shears",
        BUCKET => "Bucket",
        WATER_BUCKET => "Water Bucket",
        BOAT => "Boat",
        BONE => "Bone",
        STRIKER => "Fire Striker",
        EMBER => "Ember",
        i if i == DEEP_ITEM => "Cinderstone",
        i if i == DEEP_ITEM + 1 => "Ashstone",
        i if i == DEEP_ITEM + 2 => "Ash",
        i if i == DEEP_ITEM + 3 => "Scoria",
        i if i == DEEP_ITEM + 4 => "Smoulder Rock",
        i if i == DEEP_ITEM + 5 => "Ember Moss",
        i if i == DEEP_ITEM + 6 => "Cinderwood",
        i if i == DEEP_ITEM + 7 => "Glowcap",
        i if i == DEEP_ITEM + 8 => "Cinder Planks",
        i if i == DEEP_ITEM + 9 => "Sunshard",
        i if i == DEEP_ITEM + 10 => "Ember Ore",
        i if i == DEEP_ITEM + 11 => "Ashstone Bricks",
        GEM => "Gem",
        i if i == EXTRA_ITEM => "Deep Slate",
        i if i == EXTRA_ITEM + 1 => "Fortress Bricks",
        i if i == EXTRA_ITEM + 2 => "Cracked Fortress Bricks",
        i if i == EXTRA_ITEM + 3 => "Pyre Ore",
        PYRE_INGOT => "Pyre Ingot",
        i if i == SAPLING_ITEM => "Oak Sapling",
        i if i == SAPLING_ITEM + 1 => "Birch Sapling",
        i if i == SAPLING_ITEM + 2 => "Pine Sapling",
        i if i == SAPLING_ITEM + 3 => "Jungle Sapling",
        i if i == SAPLING_ITEM + 4 => "Acacia Sapling",
        LAVA_BUCKET => "Lava Bucket",
        PYRE_PICK => "Pyre Pickaxe",
        PYRE_AXE => "Pyre Axe",
        PYRE_SHOVEL => "Pyre Shovel",
        PYRE_SWORD => "Pyre Sword",
        PYRE_HELMET => "Pyre Helmet",
        PYRE_CHEST => "Pyre Chestplate",
        PYRE_LEGS => "Pyre Leggings",
        PYRE_BOOTS => "Pyre Boots",
        i if i == DYE + 0 => "White Dye",
        i if i == DYE + 1 => "Orange Dye",
        i if i == DYE + 2 => "Magenta Dye",
        i if i == DYE + 3 => "Light Blue Dye",
        i if i == DYE + 4 => "Yellow Dye",
        i if i == DYE + 5 => "Lime Dye",
        i if i == DYE + 6 => "Pink Dye",
        i if i == DYE + 7 => "Grey Dye",
        i if i == DYE + 8 => "Light Grey Dye",
        i if i == DYE + 9 => "Cyan Dye",
        i if i == DYE + 10 => "Purple Dye",
        i if i == DYE + 11 => "Blue Dye",
        i if i == DYE + 12 => "Brown Dye",
        i if i == DYE + 13 => "Green Dye",
        i if i == DYE + 14 => "Red Dye",
        i if i == DYE + 15 => "Black Dye",
        i if i == WOOL_ITEM + 0 => "Orange Wool",
        i if i == WOOL_ITEM + 1 => "Magenta Wool",
        i if i == WOOL_ITEM + 2 => "Light Blue Wool",
        i if i == WOOL_ITEM + 3 => "Yellow Wool",
        i if i == WOOL_ITEM + 4 => "Lime Wool",
        i if i == WOOL_ITEM + 5 => "Pink Wool",
        i if i == WOOL_ITEM + 6 => "Grey Wool",
        i if i == WOOL_ITEM + 7 => "Light Grey Wool",
        i if i == WOOL_ITEM + 8 => "Cyan Wool",
        i if i == WOOL_ITEM + 9 => "Purple Wool",
        i if i == WOOL_ITEM + 10 => "Blue Wool",
        i if i == WOOL_ITEM + 11 => "Brown Wool",
        i if i == WOOL_ITEM + 12 => "Green Wool",
        i if i == WOOL_ITEM + 13 => "Red Wool",
        i if i == WOOL_ITEM + 14 => "Black Wool",
        FISHING_ROD => "Fishing Rod",
        RAW_FISH => "Raw Fish",
        COOKED_FISH => "Cooked Fish",
        BOW => "Bow",
        ARROW => "Arrow",
        COMPASS => "Compass",
        CLOCK => "Clock",
        SEEDS => "Seeds",
        WHEAT => "Wheat",
        BREAD => "Bread",
        LEATHER_HELMET => "Leather Cap",
        LEATHER_LEGS => "Leather Trousers",
        LEATHER_BOOTS => "Leather Boots",
        IRON_HELMET => "Iron Helmet",
        IRON_LEGS => "Iron Leggings",
        IRON_BOOTS => "Iron Boots",
        GOLD_HELMET => "Golden Helmet",
        GOLD_CHEST => "Golden Chestplate",
        GOLD_LEGS => "Golden Leggings",
        GOLD_BOOTS => "Golden Boots",
        CRYSTAL_HELMET => "Crystal Helmet",
        CRYSTAL_CHEST => "Crystal Chestplate",
        CRYSTAL_LEGS => "Crystal Leggings",
        CRYSTAL_BOOTS => "Crystal Boots",
        _ => "?",
    }
}

/// Texture-array layer used as the item's icon.
pub fn icon(i: Item) -> u32 {
    if (SLAB..=BIRCH_FENCE).contains(&i) {
        // a picture of the shape rather than a face of it
        crate::textures::SHAPE_ICONS as u32 + (i - SLAB) as u32
    } else if i == LADDER {
        LADDER_LAYER
    } else if i == TRAPDOOR {
        TRAP_LAYER
    } else if i == STRIKER {
        ICON_BASE + 93
    } else if i == EMBER {
        ICON_BASE + 94
    } else if (PYRE_INGOT..SAPLING_ITEM).contains(&i) {
        crate::textures::PYRE_ICONS as u32 + (i - PYRE_INGOT) as u32
    } else if i == LAVA_BUCKET {
        crate::textures::LAVA_BUCKET_ICON as u32
    } else if is_wool_item(i) || is_extra_item(i) || is_sapling_item(i) || (DEEP_ITEM..DEEP_ITEM + DEEP_BLOCKS).contains(&i) {
        tex_for(block_of(i), if block_of(i) == EMBER_MOSS { 2 } else { 0 })
    } else if is_block(i) {
        tex_for(i, if i == FURNACE { 4 } else { 0 })
    } else {
        ICON_BASE + (i - 64) as u32
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mat {
    Wood,
    Stone,
    Iron,
    Gold,
    Crystal,
    Leather,
    Pyre,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Pick,
    Axe,
    Shovel,
    Hoe,
    Sword,
    Helmet,
    Chest,
    Legs,
    Boots,
}

/// Tools, weapons and armour that come in several materials.
pub fn gear(i: Item) -> Option<(Kind, Mat)> {
    Some(match i {
        WOOD_PICK => (Kind::Pick, Mat::Wood),
        WOOD_AXE => (Kind::Axe, Mat::Wood),
        WOOD_SHOVEL => (Kind::Shovel, Mat::Wood),
        WOOD_HOE => (Kind::Hoe, Mat::Wood),
        WOOD_SWORD => (Kind::Sword, Mat::Wood),
        STONE_PICK => (Kind::Pick, Mat::Stone),
        STONE_AXE => (Kind::Axe, Mat::Stone),
        STONE_SHOVEL => (Kind::Shovel, Mat::Stone),
        STONE_HOE => (Kind::Hoe, Mat::Stone),
        STONE_SWORD => (Kind::Sword, Mat::Stone),
        IRON_PICK => (Kind::Pick, Mat::Iron),
        IRON_AXE => (Kind::Axe, Mat::Iron),
        IRON_SHOVEL => (Kind::Shovel, Mat::Iron),
        IRON_HOE => (Kind::Hoe, Mat::Iron),
        IRON_SWORD => (Kind::Sword, Mat::Iron),
        GOLD_PICK => (Kind::Pick, Mat::Gold),
        GOLD_AXE => (Kind::Axe, Mat::Gold),
        GOLD_SHOVEL => (Kind::Shovel, Mat::Gold),
        GOLD_SWORD => (Kind::Sword, Mat::Gold),
        CRYSTAL_PICK => (Kind::Pick, Mat::Crystal),
        CRYSTAL_AXE => (Kind::Axe, Mat::Crystal),
        CRYSTAL_SHOVEL => (Kind::Shovel, Mat::Crystal),
        CRYSTAL_SWORD => (Kind::Sword, Mat::Crystal),
        LEATHER_HELMET => (Kind::Helmet, Mat::Leather),
        LEATHER_CHEST => (Kind::Chest, Mat::Leather),
        LEATHER_LEGS => (Kind::Legs, Mat::Leather),
        LEATHER_BOOTS => (Kind::Boots, Mat::Leather),
        IRON_HELMET => (Kind::Helmet, Mat::Iron),
        IRON_CHEST => (Kind::Chest, Mat::Iron),
        IRON_LEGS => (Kind::Legs, Mat::Iron),
        IRON_BOOTS => (Kind::Boots, Mat::Iron),
        GOLD_HELMET => (Kind::Helmet, Mat::Gold),
        GOLD_CHEST => (Kind::Chest, Mat::Gold),
        GOLD_LEGS => (Kind::Legs, Mat::Gold),
        GOLD_BOOTS => (Kind::Boots, Mat::Gold),
        CRYSTAL_HELMET => (Kind::Helmet, Mat::Crystal),
        CRYSTAL_CHEST => (Kind::Chest, Mat::Crystal),
        CRYSTAL_LEGS => (Kind::Legs, Mat::Crystal),
        CRYSTAL_BOOTS => (Kind::Boots, Mat::Crystal),
        PYRE_PICK => (Kind::Pick, Mat::Pyre),
        PYRE_AXE => (Kind::Axe, Mat::Pyre),
        PYRE_SHOVEL => (Kind::Shovel, Mat::Pyre),
        PYRE_SWORD => (Kind::Sword, Mat::Pyre),
        PYRE_HELMET => (Kind::Helmet, Mat::Pyre),
        PYRE_CHEST => (Kind::Chest, Mat::Pyre),
        PYRE_LEGS => (Kind::Legs, Mat::Pyre),
        PYRE_BOOTS => (Kind::Boots, Mat::Pyre),
        _ => return None,
    })
}

fn tool(i: Item, kind: Kind) -> Option<Mat> {
    gear(i).filter(|g| g.0 == kind).map(|g| g.1)
}

/// Share of the bare-hand time a tool of this material needs.
fn speed(m: Mat) -> f32 {
    match m {
        Mat::Wood | Mat::Leather => 0.31,
        Mat::Stone => 0.135,
        Mat::Iron => 0.085,
        Mat::Gold => 0.06,
        Mat::Crystal => 0.054,
        Mat::Pyre => 0.038,
    }
}

/// Pickaxe strength: 0 none, 1 wood and gold, 2 stone, 3 iron, 4 crystal.
fn pick_tier(i: Item) -> u8 {
    match tool(i, Kind::Pick) {
        Some(Mat::Wood | Mat::Gold) => 1,
        Some(Mat::Stone) => 2,
        Some(Mat::Iron) => 3,
        Some(Mat::Crystal) => 4,
        Some(Mat::Pyre) => 5,
        _ => 0,
    }
}

/// How many uses an item survives (0 = never wears out).
pub fn max_wear(i: Item) -> u16 {
    if i == STRIKER {
        return 64;
    }
    match gear(i) {
        Some((Kind::Pick | Kind::Axe | Kind::Shovel | Kind::Hoe | Kind::Sword, m)) => match m {
            Mat::Wood | Mat::Leather => 60,
            Mat::Stone => 130,
            Mat::Iron => 250,
            Mat::Gold => 40,
            Mat::Crystal => 900,
            Mat::Pyre => 2000,
        },
        Some(_) => 0,
        None => match i {
            SPEAR => 80,
            FLINT_KNIFE => 50,
            SHEARS => 200,
            FISHING_ROD => 64,
            BOW => 200,
            _ => 0,
        },
    }
}

/// Hunger restored by eating an item (0 = not food).
pub fn food(i: Item) -> f32 {
    match i {
        MEAT | RAW_FISH => 2.5,
        BERRIES => 1.5,
        BREAD => 5.0,
        COOKED_FISH => 6.0,
        COOKED_MEAT => 7.0,
        STEW => 9.0,
        _ => 0.0,
    }
}

/// Armour slot (0 head .. 3 feet) and the share of damage the piece absorbs.
pub fn armor(i: Item) -> Option<(usize, f32)> {
    let (kind, mat) = gear(i)?;
    let (slot, share) = match kind {
        Kind::Helmet => (0, 0.15),
        Kind::Chest => (1, 0.4),
        Kind::Legs => (2, 0.3),
        Kind::Boots => (3, 0.15),
        _ => return None,
    };
    let set = match mat {
        Mat::Leather => 0.25,
        Mat::Gold => 0.38,
        Mat::Iron => 0.5,
        Mat::Pyre => 0.8,
        _ => 0.64,
    };
    Some((slot, share * set))
}

pub fn max_stack(i: Item) -> u32 {
    if gear(i).is_some() || max_wear(i) > 0 || matches!(i, BED | BUCKET | WATER_BUCKET | LAVA_BUCKET | COMPASS | CLOCK | BOAT) {
        1
    } else {
        99
    }
}

/// Short type label shown in tooltips.
pub fn kind(i: Item) -> &'static str {
    match gear(i) {
        Some((Kind::Sword, _)) => "Weapon",
        Some((Kind::Helmet | Kind::Chest | Kind::Legs | Kind::Boots, _)) => "Armour",
        Some(_) => "Tool",
        None => match i {
            SPEAR | FLINT_KNIFE | BOW | ARROW => "Weapon",
            SHEARS | BUCKET | WATER_BUCKET | LAVA_BUCKET | FISHING_ROD | COMPASS | CLOCK | BOAT => "Tool",
            i if is_sapling_item(i) => "Plant",
            WORKBENCH | FURNACE | BED | TORCH | LAMP | OAK_DOOR | BIRCH_DOOR | CHEST | LADDER | TRAPDOOR => "Utility block",
            STAIRS..=137 => "Block",
            i if food(i) > 0.0 => "Food",
            i if is_plant(i) => "Plant",
            i if is_block(i) => "Block",
            _ => "Material",
        },
    }
}

/// One line of help for the tooltip.
pub fn desc(i: Item) -> &'static str {
    match gear(i) {
        Some((Kind::Pick, _)) => return "Breaks stone and ore. Better ones mine rarer ores.",
        Some((Kind::Axe, _)) => return "Chops logs and planks quickly. A fair weapon too.",
        Some((Kind::Shovel, _)) => return "Digs dirt, sand, gravel, clay and snow quickly.",
        Some((Kind::Hoe, _)) => return "Right click grass or dirt to till it for seeds.",
        Some((Kind::Sword, _)) => return "Made for fighting.",
        Some(_) => return "Wear it in its armour slot to take less damage.",
        None => {}
    }
    match i {
        GRASS | DIRT => "Soft ground. A shovel digs it fastest.",
        ICE => "Frozen water from the cold seas and rivers.",
        RED_SAND => "The rust-coloured sand of the badlands.",
        TERRACOTTA | TERRACOTTA_PALE | TERRACOTTA_DARK => "Hard baked clay from the banded cliffs of the badlands.",
        STONE => "Smooth stone, smelted from cobblestone.",
        SAND => "Smelt it into glass or pack it into sandstone.",
        LOG | BIRCH_LOG => "Craft into planks, or cook it into coal in a furnace.",
        LEAVES | BIRCH_LEAVES | PINE_LEAVES => "Light foliage. Sometimes hides a stick.",
        PLANKS | BIRCH_PLANKS => "The base of most early crafting.",
        COBBLE => "Rough stone for tools, furnaces and walls.",
        BRICK => "Fired clay. A sturdy building block.",
        SNOW => "Packed snow from the mountain tops.",
        GLASS => "Lets you see out while keeping wolves out.",
        STONE_BRICK | SANDSTONE | MOSSY_COBBLE | THATCH | CLOTH | LEATHER => "A decorative building block.",
        COAL_ORE => "Drops coal when mined.",
        IRON_ORE => "Needs a stone pickaxe. Smelt it in a furnace.",
        CRYSTAL_ORE => "Deep underground. Needs an iron pickaxe.",
        GOLD_ORE => "Deep underground. Needs an iron pickaxe.",
        CACTUS => "A prickly desert plant.",
        WORKBENCH => "Stand near it to craft advanced recipes.",
        FURNACE => "Smelts ore and cooks food. Needs fuel: coal, wood or sticks.",
        TORCH => "Lights the dark. Glows in your hand too.",
        LAMP => "A bright, steady light.",
        BED => "Right click to set your spawn and sleep through the night.",
        GRAVEL => "Loose stones from the sea bed. May hide flint.",
        CLAY => "Found under water. Fire it into bricks.",
        TALL_GRASS => "Gives plant fibre and sometimes seeds when cut.",
        FLOWER_RED | FLOWER_YELLOW | FLOWER_BLUE => "Brightens up a home.",
        MUSHROOM => "Grows in the woods. Good in a stew.",
        BERRY_BUSH => "Break it to gather berries.",
        DEAD_BUSH => "Dry twigs. Breaks into sticks.",
        GOLD_BLOCK | IRON_BLOCK => "Solid metal. Shows off your wealth.",
        FARMLAND => "Tilled soil. Plant seeds on it.",
        PANE => "A thin window that joins the walls beside it.",
        OAK_DOOR | BIRCH_DOOR => "Right click to open or close. Monsters cannot open it.",
        SLAB..=53 => "Half a block high. Two of a kind make a full block again.",
        FENCE | BIRCH_FENCE => "Too high to jump: keeps animals in and monsters out.",
        CHEST => "Right click to store 27 stacks. Break it to spill them.",
        LADDER => "Hangs on a wall. Walk into it to climb, sneak to hold on.",
        TRAPDOOR => "A hatch for a hole in the floor. Right click to open or close.",
        STAIRS..=137 => "Walk up without jumping. Place high for upside down.",
        STICK => "Handles for tools, torches and weapons.",
        MEAT | RAW_FISH => "Restores a little hunger. Better cooked.",
        COOKED_MEAT | COOKED_FISH => "A hearty meal.",
        BERRIES => "A quick snack.",
        BREAD => "Baked from three wheat. Filling.",
        STEW => "The best meal there is.",
        HIDE => "Animal skin for beds, leather and armour.",
        SPEAR => "A long reach keeps wolves at a distance.",
        FLINT_KNIFE => "Cheap and quick to make.",
        COAL => "Fuel for the furnace and for torches.",
        IRON_INGOT => "Forge it into tools, weapons and armour.",
        GOLD_INGOT => "Soft and shiny. Golden tools are fast but brittle.",
        CRYSTAL => "A rare gem for lamps and the best gear.",
        FLINT => "A sharp stone for knives and arrowheads.",
        FIBER => "For cloth, thatch, bowstrings and fishing line.",
        SHEARS => "Snips leaves and grass in an instant, and keeps the grass.",
        BUCKET => "Right click water or lava to scoop it up.",
        WATER_BUCKET => "Right click to pour the water out.",
        BOAT => "Right click water to put it in. Right click it to get in, Shift to get out.",
        BONE => "Wolves love them. Also grinds into white dye.",
        STRIKER => "Right click a finished cinderstone frame to open an Ember Gate.",
        EMBER => "Burns in a furnace for a very long time.",
        i if i == DEEP_ITEM => "Forms where water meets lava. Build an Ember Gate's frame of it.",
        i if i == DEEP_ITEM + 9 => "Glowing crystal from the roof of the Emberdeep.",
        i if i == DEEP_ITEM + 2 => "Soft ash. It slows you down.",
        i if i == DEEP_ITEM + 4 => "Hot to stand on.",
        GEM => "Villagers trade in these.",
        i if i == EXTRA_ITEM => "The dark rock far below the old bedrock. Rich in ore.",
        i if i == EXTRA_ITEM + 1 || i == EXTRA_ITEM + 2 => "From the ruined fortresses of the Emberdeep.",
        i if i == EXTRA_ITEM + 3 => "Rare, deep in the Emberdeep by the lava. Needs a crystal pickaxe. Smelt it.",
        PYRE_INGOT => "The rarest metal. Forge it into the best gear there is.",
        i if is_sapling_item(i) => "Plant it on grass or dirt and in time a tree grows.",
        LAVA_BUCKET => "Right click to pour it out. Burns in a furnace for a very long time.",
        i if dye_colour(i).is_some() => "Dyes wool and sheep. Mix two to get a new colour.",
        i if is_wool_item(i) => "Soft, coloured building block. Shear sheep or dye white wool.",
        FISHING_ROD => "Hold right click on water and wait for a bite.",
        BOW => "Right click to shoot an arrow.",
        ARROW => "Ammunition for the bow.",
        COMPASS => "Carry it to see your heading and the way to your spawn.",
        CLOCK => "Carry it to see the time of day.",
        SEEDS => "Plant on farmland. Grows into wheat.",
        WHEAT => "Three make a loaf of bread.",
        _ => "",
    }
}

/// Recipe book tabs.
#[derive(Clone, Copy, PartialEq)]
pub enum Cat {
    All,
    Tools,
    Combat,
    Armour,
    Utility,
    Building,
    Materials,
    Food,
}

/// (tab, label, icon item)
pub const CATS: [(Cat, &str, Item); 8] = [
    (Cat::All, "All recipes", WORKBENCH),
    (Cat::Tools, "Tools", STONE_PICK),
    (Cat::Combat, "Weapons", IRON_SWORD),
    (Cat::Armour, "Armour", IRON_CHEST),
    (Cat::Utility, "Utility", TORCH),
    (Cat::Building, "Building blocks", BRICK),
    (Cat::Materials, "Materials", IRON_INGOT),
    (Cat::Food, "Food", COOKED_MEAT),
];

pub struct Recipe {
    pub out: (Item, u32),
    pub ins: &'static [(Item, u32)],
    /// block that must stand within a few steps (AIR = craft anywhere)
    pub station: Block,
    pub cat: Cat,
}

const fn r(cat: Cat, out: (Item, u32), ins: &'static [(Item, u32)], station: Block) -> Recipe {
    Recipe { out, ins, station, cat }
}

pub const RECIPES: &[Recipe] = &[
    r(Cat::Materials, (PLANKS, 4), &[(LOG, 1)], AIR),
    r(Cat::Materials, (BIRCH_PLANKS, 4), &[(BIRCH_LOG, 1)], AIR),
    r(Cat::Materials, (STICK, 4), &[(PLANKS, 2)], AIR),
    r(Cat::Materials, (STICK, 4), &[(BIRCH_PLANKS, 2)], AIR),
    r(Cat::Materials, (FLINT, 1), &[(GRAVEL, 3)], AIR),
    r(Cat::Utility, (WORKBENCH, 1), &[(PLANKS, 4)], AIR),
    r(Cat::Utility, (TORCH, 4), &[(COAL, 1), (STICK, 1)], AIR),
    r(Cat::Utility, (FURNACE, 1), &[(COBBLE, 8)], WORKBENCH),
    r(Cat::Utility, (BED, 1), &[(PLANKS, 3), (HIDE, 3)], WORKBENCH),
    r(Cat::Utility, (LAMP, 1), &[(CRYSTAL, 1), (GLASS, 1)], WORKBENCH),
    r(Cat::Utility, (CHEST, 1), &[(PLANKS, 8)], WORKBENCH),
    r(Cat::Utility, (CHEST, 1), &[(BIRCH_PLANKS, 8)], WORKBENCH),
    r(Cat::Utility, (LADDER, 3), &[(STICK, 7)], WORKBENCH),
    r(Cat::Utility, (TRAPDOOR, 2), &[(PLANKS, 6)], WORKBENCH),
    r(Cat::Utility, (BUCKET, 1), &[(IRON_INGOT, 3)], WORKBENCH),
    r(Cat::Utility, (BOAT, 1), &[(PLANKS, 5)], WORKBENCH),
    r(Cat::Utility, (STRIKER, 1), &[(FLINT, 1), (IRON_INGOT, 1)], AIR),
    r(Cat::Materials, (DEEP_ITEM + 8, 4), &[(DEEP_ITEM + 6, 1)], AIR),
    r(Cat::Materials, (STICK, 4), &[(DEEP_ITEM + 8, 2)], AIR),
    r(Cat::Building, (DEEP_ITEM + 11, 4), &[(DEEP_ITEM + 1, 4)], WORKBENCH),
    r(Cat::Utility, (LAMP, 1), &[(DEEP_ITEM + 9, 2), (GLASS, 1)], WORKBENCH),
    r(Cat::Materials, (DYE + 14, 2), &[(FLOWER_RED, 1)], AIR),
    r(Cat::Materials, (DYE + 4, 2), &[(FLOWER_YELLOW, 1)], AIR),
    r(Cat::Materials, (DYE + 11, 2), &[(FLOWER_BLUE, 1)], AIR),
    r(Cat::Materials, (DYE + 15, 1), &[(COAL, 1)], AIR),
    r(Cat::Materials, (DYE + 0, 3), &[(BONE, 1)], AIR),
    r(Cat::Materials, (DYE + 13, 1), &[(CACTUS, 1)], AIR),
    r(Cat::Materials, (DYE + 12, 1), &[(MUSHROOM, 1)], AIR),
    r(Cat::Materials, (DYE + 1, 2), &[(DYE + 14, 1), (DYE + 4, 1)], AIR),
    r(Cat::Materials, (DYE + 10, 2), &[(DYE + 14, 1), (DYE + 11, 1)], AIR),
    r(Cat::Materials, (DYE + 6, 2), &[(DYE + 14, 1), (DYE + 0, 1)], AIR),
    r(Cat::Materials, (DYE + 3, 2), &[(DYE + 11, 1), (DYE + 0, 1)], AIR),
    r(Cat::Materials, (DYE + 5, 2), &[(DYE + 13, 1), (DYE + 0, 1)], AIR),
    r(Cat::Materials, (DYE + 9, 2), &[(DYE + 11, 1), (DYE + 13, 1)], AIR),
    r(Cat::Materials, (DYE + 7, 2), &[(DYE + 15, 1), (DYE + 0, 1)], AIR),
    r(Cat::Materials, (DYE + 8, 2), &[(DYE + 7, 1), (DYE + 0, 1)], AIR),
    r(Cat::Materials, (DYE + 2, 2), &[(DYE + 10, 1), (DYE + 6, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 0, 1), &[(CLOTH, 1), (DYE + 1, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 1, 1), &[(CLOTH, 1), (DYE + 2, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 2, 1), &[(CLOTH, 1), (DYE + 3, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 3, 1), &[(CLOTH, 1), (DYE + 4, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 4, 1), &[(CLOTH, 1), (DYE + 5, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 5, 1), &[(CLOTH, 1), (DYE + 6, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 6, 1), &[(CLOTH, 1), (DYE + 7, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 7, 1), &[(CLOTH, 1), (DYE + 8, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 8, 1), &[(CLOTH, 1), (DYE + 9, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 9, 1), &[(CLOTH, 1), (DYE + 10, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 10, 1), &[(CLOTH, 1), (DYE + 11, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 11, 1), &[(CLOTH, 1), (DYE + 12, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 12, 1), &[(CLOTH, 1), (DYE + 13, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 13, 1), &[(CLOTH, 1), (DYE + 14, 1)], AIR),
    r(Cat::Building, (WOOL_ITEM + 14, 1), &[(CLOTH, 1), (DYE + 15, 1)], AIR),
    r(Cat::Utility, (BOAT, 1), &[(BIRCH_PLANKS, 5)], WORKBENCH),
    r(Cat::Utility, (COMPASS, 1), &[(IRON_INGOT, 4), (CRYSTAL, 1)], WORKBENCH),
    r(Cat::Utility, (CLOCK, 1), &[(GOLD_INGOT, 4), (CRYSTAL, 1)], WORKBENCH),
    r(Cat::Tools, (WOOD_PICK, 1), &[(PLANKS, 3), (STICK, 2)], AIR),
    r(Cat::Tools, (STONE_PICK, 1), &[(COBBLE, 3), (STICK, 2)], AIR),
    r(Cat::Tools, (IRON_PICK, 1), &[(IRON_INGOT, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (GOLD_PICK, 1), &[(GOLD_INGOT, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (CRYSTAL_PICK, 1), &[(CRYSTAL, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (WOOD_AXE, 1), &[(PLANKS, 3), (STICK, 2)], AIR),
    r(Cat::Tools, (STONE_AXE, 1), &[(COBBLE, 3), (STICK, 2)], AIR),
    r(Cat::Tools, (IRON_AXE, 1), &[(IRON_INGOT, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (GOLD_AXE, 1), &[(GOLD_INGOT, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (CRYSTAL_AXE, 1), &[(CRYSTAL, 3), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (WOOD_SHOVEL, 1), &[(PLANKS, 1), (STICK, 2)], AIR),
    r(Cat::Tools, (STONE_SHOVEL, 1), &[(COBBLE, 1), (STICK, 2)], AIR),
    r(Cat::Tools, (IRON_SHOVEL, 1), &[(IRON_INGOT, 1), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (GOLD_SHOVEL, 1), &[(GOLD_INGOT, 1), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (CRYSTAL_SHOVEL, 1), &[(CRYSTAL, 1), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (WOOD_HOE, 1), &[(PLANKS, 2), (STICK, 2)], AIR),
    r(Cat::Tools, (STONE_HOE, 1), &[(COBBLE, 2), (STICK, 2)], AIR),
    r(Cat::Tools, (IRON_HOE, 1), &[(IRON_INGOT, 2), (STICK, 2)], WORKBENCH),
    r(Cat::Tools, (SHEARS, 1), &[(IRON_INGOT, 2)], WORKBENCH),
    r(Cat::Tools, (FISHING_ROD, 1), &[(STICK, 3), (FIBER, 2)], AIR),
    r(Cat::Combat, (FLINT_KNIFE, 1), &[(FLINT, 1), (STICK, 1)], AIR),
    r(Cat::Combat, (SPEAR, 1), &[(COBBLE, 1), (STICK, 2)], AIR),
    r(Cat::Combat, (WOOD_SWORD, 1), &[(PLANKS, 2), (STICK, 1)], AIR),
    r(Cat::Combat, (STONE_SWORD, 1), &[(COBBLE, 2), (STICK, 1)], AIR),
    r(Cat::Combat, (IRON_SWORD, 1), &[(IRON_INGOT, 2), (STICK, 1)], WORKBENCH),
    r(Cat::Combat, (GOLD_SWORD, 1), &[(GOLD_INGOT, 2), (STICK, 1)], WORKBENCH),
    r(Cat::Combat, (CRYSTAL_SWORD, 1), &[(CRYSTAL, 2), (STICK, 1)], WORKBENCH),
    r(Cat::Combat, (BOW, 1), &[(STICK, 3), (FIBER, 3)], AIR),
    r(Cat::Combat, (ARROW, 4), &[(FLINT, 1), (STICK, 1)], AIR),
    r(Cat::Armour, (LEATHER_HELMET, 1), &[(HIDE, 5)], WORKBENCH),
    r(Cat::Armour, (LEATHER_CHEST, 1), &[(HIDE, 8)], WORKBENCH),
    r(Cat::Armour, (LEATHER_LEGS, 1), &[(HIDE, 7)], WORKBENCH),
    r(Cat::Armour, (LEATHER_BOOTS, 1), &[(HIDE, 4)], WORKBENCH),
    r(Cat::Armour, (IRON_HELMET, 1), &[(IRON_INGOT, 5)], WORKBENCH),
    r(Cat::Armour, (IRON_CHEST, 1), &[(IRON_INGOT, 8)], WORKBENCH),
    r(Cat::Armour, (IRON_LEGS, 1), &[(IRON_INGOT, 7)], WORKBENCH),
    r(Cat::Armour, (IRON_BOOTS, 1), &[(IRON_INGOT, 4)], WORKBENCH),
    r(Cat::Armour, (GOLD_HELMET, 1), &[(GOLD_INGOT, 5)], WORKBENCH),
    r(Cat::Armour, (GOLD_CHEST, 1), &[(GOLD_INGOT, 8)], WORKBENCH),
    r(Cat::Armour, (GOLD_LEGS, 1), &[(GOLD_INGOT, 7)], WORKBENCH),
    r(Cat::Armour, (GOLD_BOOTS, 1), &[(GOLD_INGOT, 4)], WORKBENCH),
    r(Cat::Armour, (CRYSTAL_HELMET, 1), &[(CRYSTAL, 5)], WORKBENCH),
    r(Cat::Armour, (CRYSTAL_CHEST, 1), &[(CRYSTAL, 8)], WORKBENCH),
    r(Cat::Armour, (CRYSTAL_LEGS, 1), &[(CRYSTAL, 7)], WORKBENCH),
    r(Cat::Armour, (CRYSTAL_BOOTS, 1), &[(CRYSTAL, 4)], WORKBENCH),
    // pyre gear is crystal gear forged over with the rare metal
    r(Cat::Armour, (PYRE_HELMET, 1), &[(CRYSTAL_HELMET, 1), (PYRE_INGOT, 3)], WORKBENCH),
    r(Cat::Armour, (PYRE_CHEST, 1), &[(CRYSTAL_CHEST, 1), (PYRE_INGOT, 5)], WORKBENCH),
    r(Cat::Armour, (PYRE_LEGS, 1), &[(CRYSTAL_LEGS, 1), (PYRE_INGOT, 4)], WORKBENCH),
    r(Cat::Armour, (PYRE_BOOTS, 1), &[(CRYSTAL_BOOTS, 1), (PYRE_INGOT, 2)], WORKBENCH),
    r(Cat::Tools, (PYRE_PICK, 1), &[(CRYSTAL_PICK, 1), (PYRE_INGOT, 3)], WORKBENCH),
    r(Cat::Tools, (PYRE_AXE, 1), &[(CRYSTAL_AXE, 1), (PYRE_INGOT, 3)], WORKBENCH),
    r(Cat::Tools, (PYRE_SHOVEL, 1), &[(CRYSTAL_SHOVEL, 1), (PYRE_INGOT, 1)], WORKBENCH),
    r(Cat::Combat, (PYRE_SWORD, 1), &[(CRYSTAL_SWORD, 1), (PYRE_INGOT, 2)], WORKBENCH),
    r(Cat::Building, (EXTRA_ITEM + 1, 4), &[(EXTRA_ITEM + 2, 4)], WORKBENCH),
    r(Cat::Food, (BREAD, 1), &[(WHEAT, 3)], AIR),
    r(Cat::Food, (STEW, 1), &[(MUSHROOM, 2), (BERRIES, 2)], AIR),
    r(Cat::Building, (STONE_BRICK, 4), &[(COBBLE, 4)], AIR),
    r(Cat::Building, (MOSSY_COBBLE, 2), &[(COBBLE, 2), (FIBER, 2)], AIR),
    r(Cat::Building, (SANDSTONE, 2), &[(SAND, 4)], AIR),
    r(Cat::Building, (THATCH, 2), &[(FIBER, 3)], AIR),
    r(Cat::Building, (CLOTH, 1), &[(FIBER, 4)], AIR),
    r(Cat::Building, (LEATHER, 1), &[(HIDE, 2)], AIR),
    r(Cat::Building, (PANE, 16), &[(GLASS, 6)], WORKBENCH),
    r(Cat::Building, (OAK_DOOR, 3), &[(PLANKS, 6)], WORKBENCH),
    r(Cat::Building, (BIRCH_DOOR, 3), &[(BIRCH_PLANKS, 6)], WORKBENCH),
    r(Cat::Building, (STAIRS, 4), &[(PLANKS, 6)], WORKBENCH),
    r(Cat::Building, (STAIRS + 1, 4), &[(BIRCH_PLANKS, 6)], WORKBENCH),
    r(Cat::Building, (STAIRS + 2, 4), &[(COBBLE, 6)], WORKBENCH),
    r(Cat::Building, (STAIRS + 3, 4), &[(STONE_BRICK, 6)], WORKBENCH),
    r(Cat::Building, (SLAB, 6), &[(PLANKS, 3)], WORKBENCH),
    r(Cat::Building, (SLAB + 1, 6), &[(BIRCH_PLANKS, 3)], WORKBENCH),
    r(Cat::Building, (SLAB + 2, 6), &[(COBBLE, 3)], WORKBENCH),
    r(Cat::Building, (SLAB + 3, 6), &[(STONE_BRICK, 3)], WORKBENCH),
    r(Cat::Building, (FENCE, 3), &[(PLANKS, 4), (STICK, 2)], WORKBENCH),
    r(Cat::Building, (BIRCH_FENCE, 3), &[(BIRCH_PLANKS, 4), (STICK, 2)], WORKBENCH),
    r(Cat::Building, (IRON_BLOCK, 1), &[(IRON_INGOT, 4)], WORKBENCH),
    r(Cat::Building, (GOLD_BLOCK, 1), &[(GOLD_INGOT, 4)], WORKBENCH),
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stack {
    pub item: Item,
    pub n: u32,
    /// uses spent, for tools
    pub wear: u16,
}

impl Stack {
    pub fn new(item: Item, n: u32) -> Stack {
        Stack { item, n, wear: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Remaining durability, 0..1 (None for items that never wear).
    pub fn condition(&self) -> Option<f32> {
        let max = max_wear(self.item);
        (max > 0 && !self.is_empty()).then(|| 1.0 - self.wear as f32 / max as f32)
    }
}

/// Left click on a slot with `held` on the mouse: pick up, put down, merge or swap.
/// `accepts` is false where the slot will not take the held item.
pub fn click_stack(held: &mut Stack, slot: &mut Stack, accepts: bool) {
    let s = *slot;
    if held.is_empty() {
        *held = s;
        *slot = Stack::default();
    } else if !accepts {
    } else if !s.is_empty() && s.item == held.item && max_stack(s.item) > 1 {
        let k = held.n.min(max_stack(s.item).saturating_sub(s.n));
        slot.n += k;
        *held = if held.n > k { Stack::new(held.item, held.n - k) } else { Stack::default() };
    } else {
        *slot = *held;
        *held = s;
    }
}

/// Right click on a slot: take half of it, or put down one of the held stack.
pub fn right_click_stack(held: &mut Stack, slot: &mut Stack, accepts: bool) {
    let s = *slot;
    if held.is_empty() {
        if !s.is_empty() {
            let k = (s.n + 1) / 2;
            *held = Stack { n: k, ..s };
            *slot = if s.n > k { Stack { n: s.n - k, ..s } } else { Stack::default() };
        }
    } else if max_stack(held.item) == 1 {
        if s.is_empty() {
            click_stack(held, slot, accepts);
        }
    } else if accepts && (s.is_empty() || (s.item == held.item && s.n < max_stack(s.item))) {
        *slot = Stack { item: held.item, n: s.n + 1, wear: s.wear };
        held.n -= 1;
        if held.n == 0 {
            *held = Stack::default();
        }
    }
}

/// Moves a whole stack (keeping its wear) into `slots`: tops up matching stacks, then
/// fills empty slots. Returns what did not fit.
pub fn stash_in(slots: &mut [Stack], st: Stack) -> Stack {
    if st.is_empty() {
        return st;
    }
    let max = max_stack(st.item);
    let mut n = st.n;
    if max > 1 {
        for s in slots.iter_mut().filter(|s| !s.is_empty() && s.item == st.item) {
            let k = n.min(max.saturating_sub(s.n));
            s.n += k;
            n -= k;
        }
    }
    for s in slots.iter_mut().filter(|s| s.is_empty()) {
        if n == 0 {
            break;
        }
        let k = n.min(max);
        *s = Stack { item: st.item, n: k, wear: st.wear };
        n -= k;
    }
    if n > 0 { Stack { n, ..st } } else { Stack::default() }
}

/// slots 0..9 are the hotbar, 9..36 the backpack, 36..40 armour (head, chest, legs, feet)
pub const HOTBAR: usize = 9;
pub const BAG_END: usize = 36;
pub const ARMOR: usize = 36;
pub const SLOTS: usize = 40;

pub struct Inventory {
    pub slots: [Stack; SLOTS],
    /// the stack picked up with the mouse in the inventory screen
    pub held: Stack,
}

impl Inventory {
    pub fn new() -> Inventory {
        Inventory { slots: [Stack::default(); SLOTS], held: Stack::default() }
    }

    /// Item in a hotbar slot (AIR if empty).
    pub fn hot(&self, slot: usize) -> Item {
        self.slots[slot].item
    }

    /// Share of damage that gets through the armour worn.
    pub fn armor_factor(&self) -> f32 {
        1.0 - self.slots[ARMOR..].iter().filter_map(|s| armor(s.item)).map(|a| a.1).sum::<f32>()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_empty())
    }

    /// Total carried in the hotbar and backpack.
    pub fn count(&self, i: Item) -> u32 {
        self.slots[..BAG_END].iter().filter(|s| s.item == i).map(|s| s.n).sum()
    }

    /// True if the item is anywhere on the player.
    pub fn has(&self, i: Item) -> bool {
        self.count(i) > 0
    }

    fn put(&mut self, slot: usize, i: Item, n: u32) -> u32 {
        let s = &mut self.slots[slot];
        if s.is_empty() || s.item == i {
            let k = n.min(max_stack(i).saturating_sub(s.n));
            if k > 0 {
                *s = Stack { item: i, n: s.n + k, wear: s.wear };
            }
            n - k
        } else {
            n
        }
    }

    /// Adds to the slots in `range`: tops up matching stacks, then fills empty slots.
    fn add_to(&mut self, range: std::ops::Range<usize>, i: Item, mut n: u32) -> u32 {
        for slot in range.clone() {
            if n > 0 && self.slots[slot].item == i && !self.slots[slot].is_empty() {
                n = self.put(slot, i, n);
            }
        }
        for slot in range {
            if n > 0 && self.slots[slot].is_empty() {
                n = self.put(slot, i, n);
            }
        }
        n
    }

    /// Moves a whole stack (keeping its wear) into `range`. Returns what did not fit.
    fn stash(&mut self, range: std::ops::Range<usize>, st: Stack) -> Stack {
        stash_in(&mut self.slots[range], st)
    }

    /// Picks a stack up into the hotbar and backpack. Returns what did not fit.
    pub fn pick_up(&mut self, st: Stack) -> Stack {
        if valid(st.item) { self.stash(0..BAG_END, st) } else { Stack::default() }
    }

    /// Adds items. Returns how many did not fit.
    pub fn add(&mut self, i: Item, n: u32) -> u32 {
        if !valid(i) || n == 0 {
            return 0;
        }
        self.add_to(0..BAG_END, i, n)
    }

    /// Removes `n` of an item, emptying the backpack before the hotbar.
    pub fn take(&mut self, i: Item, mut n: u32) -> bool {
        if self.count(i) < n {
            return false;
        }
        for slot in (0..BAG_END).rev() {
            let s = &mut self.slots[slot];
            if s.item == i && n > 0 {
                let k = n.min(s.n);
                s.n -= k;
                n -= k;
                if s.n == 0 {
                    *s = Stack::default();
                }
            }
        }
        true
    }

    /// Removes one item from a specific slot.
    pub fn take_slot(&mut self, slot: usize) -> bool {
        let s = &mut self.slots[slot];
        if s.is_empty() {
            return false;
        }
        s.n -= 1;
        if s.n == 0 {
            *s = Stack::default();
        }
        true
    }

    /// Spends one use of the tool in `slot`. Returns true if it just broke.
    pub fn wear_tool(&mut self, slot: usize) -> bool {
        let s = &mut self.slots[slot];
        let max = max_wear(s.item);
        if max == 0 || s.is_empty() {
            return false;
        }
        s.wear += 1;
        if s.wear >= max {
            *s = Stack::default();
            return true;
        }
        false
    }

    pub fn can_craft(&self, r: &Recipe) -> bool {
        r.ins.iter().all(|&(i, n)| self.count(i) >= n)
    }

    /// Crafts once into the inventory. Returns false if ingredients are missing.
    pub fn craft(&mut self, r: &Recipe) -> bool {
        if !self.can_craft(r) {
            return false;
        }
        for &(i, n) in r.ins {
            self.take(i, n);
        }
        let left = self.add(r.out.0, r.out.1);
        if left > 0 {
            // no room: hand the rest to the mouse, or give up on it
            if self.held.is_empty() {
                self.held = Stack::new(r.out.0, left);
            } else if self.held.item == r.out.0 {
                self.held.n += left;
            }
        }
        true
    }

    /// Crafts once onto the mouse, like taking the result out of a crafting grid.
    pub fn craft_to_hand(&mut self, r: &Recipe) -> bool {
        let fits = self.held.is_empty()
            || (self.held.item == r.out.0 && self.held.n + r.out.1 <= max_stack(r.out.0));
        if !fits {
            return self.craft(r);
        }
        if !self.can_craft(r) {
            return false;
        }
        for &(i, n) in r.ins {
            self.take(i, n);
        }
        self.held = Stack::new(r.out.0, self.held.n + r.out.1);
        true
    }

    fn accepts(slot: usize, i: Item) -> bool {
        slot < ARMOR || armor(i).is_some_and(|a| a.0 == slot - ARMOR)
    }

    /// Left click: pick up, drop, merge or swap the held stack.
    pub fn click(&mut self, slot: usize) {
        let ok = Self::accepts(slot, self.held.item);
        click_stack(&mut self.held, &mut self.slots[slot], ok);
    }

    /// Right click: take half a stack, or drop a single held item.
    pub fn right_click(&mut self, slot: usize) {
        let ok = Self::accepts(slot, self.held.item);
        right_click_stack(&mut self.held, &mut self.slots[slot], ok);
    }

    /// Shift click: send a stack to the other half of the inventory (armour goes on).
    pub fn quick_move(&mut self, slot: usize) {
        let s = self.slots[slot];
        if s.is_empty() {
            return;
        }
        self.slots[slot] = Stack::default();
        let worn = armor(s.item).map(|a| ARMOR + a.0).filter(|&a| slot < ARMOR && self.slots[a].is_empty());
        let left = if let Some(a) = worn {
            self.slots[a] = s;
            Stack::default()
        } else if slot < HOTBAR {
            self.stash(HOTBAR..BAG_END, s)
        } else if slot < BAG_END {
            self.stash(0..HOTBAR, s)
        } else {
            self.stash(0..BAG_END, s)
        };
        if !left.is_empty() {
            self.slots[slot] = left;
        }
    }

    /// Puts the mouse stack back when the inventory closes.
    pub fn stow_held(&mut self) {
        self.held = self.stash(0..BAG_END, self.held);
    }

    /// Merges and orders the backpack: blocks first, then tools and materials.
    pub fn sort_bag(&mut self) {
        let mut all: Vec<Stack> = self.slots[HOTBAR..BAG_END].iter().copied().filter(|s| !s.is_empty()).collect();
        all.sort_by_key(|s| (s.item, s.wear));
        for s in &mut self.slots[HOTBAR..BAG_END] {
            *s = Stack::default();
        }
        for s in all {
            self.stash(HOTBAR..BAG_END, s);
        }
    }
}

/// The item that places (and drops from) a door block.
pub fn door_item(b: Block) -> Item {
    OAK_DOOR + door_wood(b)
}

/// The item a block in the world stands for (middle click, and what it drops).
pub fn item_of(b: Block) -> Item {
    match b {
        b if is_door(b) => door_item(b),
        b if is_slab(b) => SLAB + slab_mat(b),
        b if is_stairs(b) => STAIRS + stair_mat(b),
        b if is_ladder(b) => LADDER,
        b if is_trapdoor(b) => TRAPDOOR,
        b if is_torch(b) => TORCH,
        b if (WOOL..WOOL_END).contains(&b) => WOOL_ITEM + (b - WOOL),
        b if is_deep_block(b) => DEEP_ITEM + (b - CINDERSTONE),
        b if is_extra_block(b) => EXTRA_ITEM + (b - SLATE),
        b if is_sapling(b) => SAPLING_ITEM + (b - SAPLING),
        b if is_gate(b) => AIR,
        LOOT_CHEST => CHEST,
        b if is_grassy(b) => GRASS,
        JUNGLE_LEAVES | ACACIA_LEAVES => LEAVES,
        SNOWY_LEAVES => PINE_LEAVES,
        FERN | DRY_TUFT => TALL_GRASS,
        REEDS => FIBER,
        b => b,
    }
}

/// Pickaxe tier a block needs before it drops anything.
pub fn tier_needed(b: Block) -> u8 {
    match b {
        IRON_ORE => 2,
        CRYSTAL_ORE | GOLD_ORE => 3,
        // (gates are built of something only the best pick cuts)
        CINDERSTONE | PYRE_ORE => 4,
        _ => 0,
    }
}

/// What a block gives when broken with `tool` (AIR = nothing).
pub fn drop_for(b: Block, tool: Item) -> Item {
    if pick_tier(tool) < tier_needed(b) {
        return AIR;
    }
    match b {
        STONE => COBBLE,
        FARMLAND => DIRT,
        b if is_grassy(b) => DIRT,
        REEDS => FIBER,
        COAL_ORE => COAL,
        EMBER_ORE => EMBER,
        CRYSTAL_ORE => CRYSTAL,
        BERRY_BUSH => BERRIES,
        DEAD_BUSH => STICK,
        b if is_tuft(b) => {
            if tool == SHEARS {
                TALL_GRASS
            } else {
                FIBER
            }
        }
        WHEAT_0 | WHEAT_1 | WHEAT_2 => SEEDS,
        WHEAT_3 => WHEAT,
        b if is_fluid(b) => AIR,
        BEDROCK | AIR => AIR,
        b => item_of(b),
    }
}

/// Seconds of holding the button needed to break `b` with `tool` in hand.
pub fn break_time(b: Block, tool: Item) -> f32 {
    let with = |kind: Kind, base: f32, gain: f32| match self::tool(tool, kind) {
        Some(m) => (base * speed(m) * gain).max(0.06),
        None => base,
    };
    match b {
        CINDERSTONE => with(Kind::Pick, 26.0, 1.0),
        SLATE => with(Kind::Pick, 3.6, 1.0),
        FORT_BRICK | FORT_CRACKED => with(Kind::Pick, 3.2, 1.0),
        PYRE_ORE => with(Kind::Pick, 9.0, 1.0),
        ASHSTONE | EMBER_MOSS | SMOULDER => with(Kind::Pick, 1.3, 1.0),
        SCORIA | EMBER_ORE | ASHSTONE_BRICK => with(Kind::Pick, 2.6, 1.0),
        ASH => with(Kind::Shovel, 0.5, 2.0),
        CINDERWOOD | CINDER_PLANKS => with(Kind::Axe, 1.1, 2.0),
        GLOWCAP => 0.4,
        SUNSHARD => 0.3,
        STONE | COBBLE | BRICK | STONE_BRICK | SANDSTONE | FURNACE | COAL_ORE | IRON_ORE | CRYSTAL_ORE | GOLD_ORE
        | MOSSY_COBBLE | IRON_BLOCK | GOLD_BLOCK => {
            let hard = match b {
                IRON_ORE | IRON_BLOCK | GOLD_BLOCK => 1.5,
                CRYSTAL_ORE | GOLD_ORE => 2.2,
                SANDSTONE => 0.7,
                _ => 1.0,
            };
            with(Kind::Pick, 2.6 * hard, 1.0)
        }
        LOG | PLANKS | WORKBENCH | BED | BIRCH_LOG | BIRCH_PLANKS => with(Kind::Axe, 1.1, 2.0),
        b if is_door(b) || is_trapdoor(b) || b == CHEST || b == LOOT_CHEST => with(Kind::Axe, 1.5, 2.0),
        b if is_slab(b) || is_stairs(b) => {
            let mat = if is_slab(b) { slab_mat(b) } else { stair_mat(b) };
            if mat < 2 { with(Kind::Axe, 1.1, 2.0) } else { with(Kind::Pick, 2.6, 1.0) }
        }
        FENCE | BIRCH_FENCE => with(Kind::Axe, 1.1, 2.0),
        b if is_ladder(b) => 0.3,
        DIRT | SAND | GRAVEL | CLAY | SNOW | FARMLAND | RED_SAND => with(Kind::Shovel, 0.5, 2.0),
        b if is_grassy(b) => with(Kind::Shovel, 0.5, 2.0),
        TERRACOTTA | TERRACOTTA_PALE | TERRACOTTA_DARK => with(Kind::Pick, 1.6, 1.0),
        ICE => with(Kind::Pick, 0.6, 1.0),
        b if is_leaves(b) || is_wool(b) || b == THATCH => {
            if tool == SHEARS {
                0.04
            } else {
                0.18
            }
        }
        GLASS | LAMP | PANE => 0.15,
        b if is_torch(b) => 0.05,
        b if is_plant(b) => 0.05,
        _ => 0.3,
    }
}

/// (damage, reach) of a melee hit with `tool` in hand.
pub fn weapon(tool: Item) -> (i32, f32) {
    match gear(tool) {
        Some((Kind::Sword, m)) => (
            match m {
                Mat::Stone => 4,
                Mat::Iron => 5,
                Mat::Crystal => 7,
                Mat::Pyre => 9,
                _ => 3,
            },
            4.6,
        ),
        Some((Kind::Axe, m)) => (
            match m {
                Mat::Iron => 3,
                Mat::Crystal => 4,
                Mat::Pyre => 6,
                _ => 2,
            },
            4.3,
        ),
        _ => match tool {
            SPEAR => (2, 5.6),
            FLINT_KNIFE => (2, 4.3),
            _ => (1, 4.3),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_stack_split_and_move() {
        let mut inv = Inventory::new();
        assert_eq!(inv.add(COBBLE, 150), 0);
        assert_eq!((inv.slots[0].n, inv.slots[1].n), (99, 51));
        assert_eq!(inv.count(COBBLE), 150);
        // tools do not stack
        inv.add(IRON_PICK, 2);
        assert_eq!((inv.slots[2].n, inv.slots[3].n), (1, 1));
        // right click takes half, left click drops it elsewhere
        inv.right_click(1);
        assert_eq!((inv.held.n, inv.slots[1].n), (26, 25));
        inv.click(20);
        assert!(inv.held.is_empty());
        assert_eq!(inv.slots[20], Stack::new(COBBLE, 26));
        // merging overflows back onto the mouse
        inv.click(20);
        inv.click(0);
        assert_eq!((inv.slots[0].n, inv.held.n), (99, 26));
        inv.stow_held();
        assert_eq!(inv.count(COBBLE), 150);
        // the armour slot only takes armour
        inv.click(2);
        inv.click(ARMOR);
        assert_eq!(inv.held.item, IRON_PICK);
        inv.stow_held();
        inv.add(IRON_CHEST, 1);
        let at = inv.slots.iter().position(|s| s.item == IRON_CHEST).unwrap();
        inv.quick_move(at);
        assert_eq!(inv.slots[ARMOR + 1].item, IRON_CHEST);
        assert!((inv.armor_factor() - 0.8).abs() < 0.001);
        // taking drains the backpack before the hotbar; crafting consumes and produces
        assert!(inv.take(COBBLE, 60));
        assert_eq!(inv.slots[0].n, 90);
        inv.add(STICK, 2);
        let r = RECIPES.iter().find(|r| r.out.0 == STONE_PICK).unwrap();
        assert!(inv.craft(r));
        assert_eq!((inv.count(COBBLE), inv.count(STICK), inv.count(STONE_PICK)), (87, 0, 1));
        // tools wear out and break
        let at = inv.slots.iter().position(|s| s.item == STONE_PICK).unwrap();
        for _ in 0..max_wear(STONE_PICK) - 1 {
            assert!(!inv.wear_tool(at));
        }
        assert!(inv.wear_tool(at));
        assert_eq!(inv.count(STONE_PICK), 0);
        inv.sort_bag();
        assert_eq!(inv.count(COBBLE), 87);
    }

    #[test]
    fn chest_slots_and_pick_up() {
        let mut inv = Inventory::new();
        let mut chest = [Stack::default(); CHEST_SLOTS];
        inv.add(COBBLE, 70);
        // carry a stack across with the mouse, then split one off with a right click
        inv.click(0);
        click_stack(&mut inv.held, &mut chest[3], true);
        assert!(inv.held.is_empty());
        assert_eq!(chest[3], Stack::new(COBBLE, 70));
        right_click_stack(&mut inv.held, &mut chest[3], true);
        assert_eq!((inv.held.n, chest[3].n), (35, 35));
        right_click_stack(&mut inv.held, &mut chest[4], true);
        assert_eq!((inv.held.n, chest[4].n), (34, 1));
        inv.stow_held();
        // shift-click style moves keep a tool's wear
        let pick = Stack { item: IRON_PICK, n: 1, wear: 9 };
        assert!(stash_in(&mut chest, pick).is_empty());
        assert!(chest.contains(&pick));
        assert_eq!(stash_in(&mut chest, Stack::new(COBBLE, 100)).n, 0);
        assert_eq!(chest.iter().filter(|s| s.item == COBBLE).map(|s| s.n).sum::<u32>(), 136);
        // a full inventory hands back what it cannot take
        let mut full = Inventory::new();
        for s in &mut full.slots[..BAG_END] {
            *s = Stack::new(DIRT, 99);
        }
        full.slots[5].n = 90;
        assert_eq!(full.pick_up(Stack::new(DIRT, 20)).n, 11);
        assert_eq!(full.pick_up(pick), pick);
    }

    #[test]
    fn shaped_blocks_drop_their_items() {
        for (b, item) in [(SLAB_TOP + 2, SLAB + 2), (STAIR_BASE + 8 + 5, STAIRS + 1), (LADDER_BASE + 3, LADDER), (TRAP_BASE + 13, TRAPDOOR), (DOOR_BASE + 40, BIRCH_DOOR), (CHEST, CHEST)] {
            assert_eq!(drop_for(b, AIR), item);
            assert!(valid(item) && name(item) != "?" && !desc(item).is_empty());
        }
    }

    #[test]
    fn every_recipe_item_is_named() {
        for r in RECIPES {
            for &(i, _) in r.ins.iter().chain([&r.out]) {
                assert!(valid(i) && name(i) != "?", "item {i}");
            }
        }
    }
}
