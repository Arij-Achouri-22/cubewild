# Cubewild

A small open-world voxel sandbox in Rust (wgpu/Vulkan + winit): build with blocks, explore
an endless world of seas, deserts, jungles, snowfields and mountains, with caves and ores, hunt deer, boar and hare, and survive the
wolves and monsters that come out at night.

## Run

```sh
cargo build --release
./target/release/cubewild              # windowed (F11 toggles fullscreen)
./target/release/cubewild --fullscreen
```

Runs on Linux, Windows and macOS (Vulkan, DirectX 12 or Metal through wgpu). On Windows,
install Rust from <https://rustup.rs> (it offers to install the Visual Studio build tools it
needs), then in the project folder:

```bat
cargo build --release
target\release\cubewild.exe
```

## Worlds and game modes

The game opens on a title screen. **New survival world** and **New creative world** each
start a world with a seed of its own, so no two are alike; the worlds you have played are
listed below them, the latest first, to carry on with or delete. "Save and go to title" in
the pause menu brings you back.

- **Survival**: health, hunger, tools that wear out, blocks that take time to break and
  drop as items, and animals and monsters that can kill you.
- **Creative**: nothing hurts you and you never get hungry, monsters and wolves leave you
  alone, blocks break at once and drop nothing, placing a block does not use it up, tools
  do not wear, and you can fly (F, or Space twice). The inventory lists every item in
  place of the recipe book: click one for a full stack (Shift+click puts it in the pack),
  and click the list with a stack on the mouse to get rid of it.

A world keeps the mode it was made in.

## The world

A new world is laid out the way Minecraft 1.18+ lays out its own: slow maps of how far
inland, how eroded, how warm and how wet a place is shape the ground and pick its biome, so
the country changes as you travel (F3 names the biome you stand in).

- **Seas**: about three tenths of the world, from shelves to deep ocean, warm with sandy
  beds or frozen over with pack ice and icebergs. **Rivers** wind through every country
  down at sea level, and freeze in the cold. **Beaches** line the coasts.
- **Plains and meadows**: open grass with scattered trees and patches of red, yellow and
  blue flowers. **Forests** of oak and birch, with mushrooms, berries and ferns.
- **Taiga**: pine woods with ferns and berries; further into the cold, **snowy taiga** and
  **snowy plains**, where the grass lies under snow and still water is ice.
- **Desert**: sand over sandstone, cacti and dead bushes. **Badlands**: hot, rugged country
  that stands in terraces of banded terracotta over red sand.
- **Savanna**: dry yellow grass and flat-topped trees. **Jungle**: giant trees over thick
  undergrowth. **Swamp**: drowned flat land, dark grass, reeds and squat wide oaks.
- **Mountains** rise inland; it gets colder with height, so their slopes turn to pine
  woods, then bare rock, then snow (hot country keeps bare peaks).
- **Villages** are rare: you have to go and look for them. About one square of 640 blocks
  in ten holds one, on level plains, meadows, savanna, desert or snowfields, so the
  nearest is usually 400 to 1500 blocks from where you start. Each has a roofed well, a
  few houses built from what the country offers (some with a chest), wheat fields, lamp
  posts and gravel paths. **Villagers** live there: by day they stroll the paths and turn
  to look at you, at night they go home. They harm nobody; one that is killed stays gone.

Worlds made before biomes or villages existed keep their old terrain: a world stays with
the generator it was made with, so nothing already built is disturbed. Start a new world
to see the rest.

## Controls

| Key | Action |
| --- | --- |
| WASD / Space / Ctrl / Shift | move / jump / sprint / sneak (animals notice you later) |
| Hold left click | break block, or hit an animal in reach |
| Right click | place the selected block, eat the held food, open or close a door or trapdoor, open a chest, sleep in a bed, open a workbench or furnace |
| Q | drop one of the held item (Ctrl+Q drops the whole stack) |
| Middle click / P | select the block you look at; acquire it in creative |
| 1-9, mouse wheel | choose hotbar slot |
| E or Tab | inventory and crafting |
| F, or Space twice | fly, in creative worlds (Space up, Shift down) |
| F3 / F11 | debug overlay / fullscreen |
| Esc | pause menu (settings, controls, save and quit) |

Movement, jump, sprint, sneak, inventory, drop, flight and the keyboard pick-block shortcut
can be rebound in **Pause → Controls**. Click a binding and press its replacement; Escape
cancels. Mouse controls and menu shortcuts stay available.

## Gathering and crafting

Broken blocks drop as items where they stood (stone drops cobblestone, grass drops dirt,
leaves sometimes drop a stick) and animals drop meat and hide where they fall; walk over
an item to pick it up. Items left lying vanish after five minutes. Placing a block uses one.
The inventory window has a 9-slot hotbar, a 27-slot backpack, four armour slots (head,
chest, legs, feet) beside a picture of your character, and a crafting area. Left click
picks a stack up and puts it down, right click takes half or drops one, Shift+click sends
a stack between hotbar and backpack (or puts armour on), and Sort tidies the backpack.
Drag a carried stack with the left button to divide it evenly over slots, or with the
right button to put one in each slot. Items that do not fit stay on the cursor.
The recipe book on the left has tabs (Tools, Weapons, Armour, Utility, Building,
Materials, Food) and a "Can craft" filter; pick a recipe, then click the result slot to
take it (Shift+click crafts as many as you can). Hover anything for its details.
The **Grid** button switches to manual shaped crafting: 2×2 in the inventory, or 3×3
near a workbench. Recipes can be offset or mirrored. The recipe book remains available.
Closing the window returns grid ingredients to the bag, with excess dropped beside you;
an interrupted session also preserves the grid itself.

Recipes are in `src/items.rs`. Basic tools and torches can be made anywhere; a **workbench**
unlocks the furnace, bed, iron tools, crystal pickaxe and crystal lamp, and a **furnace**
(stand within a few steps of either) smelts iron ore, cooks meat, and burns logs into coal.

- **Ores** sit in underground stone: coal anywhere, iron below y 124 (needs a stone
  pickaxe), gold below y 98, crystal below y 86 (needs an iron pickaxe). The pockets
  grow towards the bottom of the world.
- **Caves**: two networks of tunnels wind through the rock and open onto hillsides, and
  below them lie caverns, wider and more frequent the further down. The deepest are
  flooded with **lava**, which glows, burns you (two hearts every half second) and
  destroys items that fall in. What daylight seeps in fades on the way down, so the
  depths are black without a torch. Monsters appear in the dark underground at any
  hour, more of them the deeper you are, and **chests** left on cave floors hold
  better loot the deeper they stand: torches and bread near the surface; iron, gold,
  crystal and finished tools and armour near the lava.
- **Light**: enclosed spaces are dark, and so is the night. Torches and crystal lamps
  illuminate through open cells, with levels from 0 to 15. Solid walls stop their light.
  Terrain, crop growth and Overworld monster spawning share these block-light values.
- **Tools** come in wood, stone, iron, gold (fast but brittle) and crystal: pickaxes,
  axes, shovels, hoes and swords. They wear out; a bar on the icon shows what is left.
  Also: shears, bucket (scoops and pours water), fishing rod (hold right click on water),
  bow and arrows, compass and clock (shown at the top of the screen while carried).
- **Farming**: till grass or dirt with a hoe, then plant wheat seeds, carrots or potatoes.
  Water within four blocks, including diagonals, hydrates soil; exposed rain does too.
  Crops need light level 9 or higher and grow faster on moist soil. Dry empty farmland
  eventually becomes dirt. Jumping on a field can trample it. Village fields supply the
  new crops. Harvested carrots and potatoes can be planted again; potatoes can also be baked.
- **The land**: oak, birch and pine trees, grass tufts (plant fibre), flowers, mushrooms and
  berry bushes (stew), gravel (flint) and clay (bricks) on the sea bed, gold deep down.
- **Armour**: leather, gold, iron and crystal pieces each absorb a share of the damage from
  attacks; a full crystal set absorbs 64%. Armour wears when it absorbs attacks. Falling,
  drowning, hunger and lava follow their own damage rules.
- **Doors and windows**: at a workbench, six planks make three doors (oak or birch) and
  six glass make sixteen window panes. A door stands two blocks tall on the edge of its
  block nearest to you; right click opens and closes it (sneak to place a block against
  it instead), and two doors side by side hinge on opposite sides. Panes are thin and
  join up with the walls and panes beside them. Monsters cannot open doors or see
  through closed ones, and arrows stop at doors and panes.
- **Stairs, slabs, fences**: at a workbench, six planks, cobblestones or stone bricks make
  four stairs, three make six slabs, and four planks with two sticks make three fences.
  You walk up stairs and slabs without jumping, and stairs set crosswise to each other
  turn the corner. Point at the upper half of a block's side
  (or at its underside) to place them upside down or as the upper half; two slabs of a
  kind in one cell make a full block again. Fences join each other and solid blocks, and
  are too high to jump over, for you and for monsters.
- **Ladders and trapdoors**: seven sticks make three ladders, six planks two trapdoors.
  A ladder hangs on the side of a solid block: walk into it to climb, sneak to hold on,
  let go to slide down. Monsters that walk into a ladder climb it too, though they do
  not look for one. A trapdoor opens and closes with a right click.
- **Chests**: eight planks make a chest holding 27 stacks. Right click to open it;
  Shift+click sends a stack across. Two adjacent chests join into a 54-slot large chest;
  either half opens it. A third chest cannot join the pair. Leave room above the lid.
  Breaking a half spills only that half's contents.
- **Survival**: you have health and hunger. Eat meat (cooked is far better) to stay fed;
  a full stomach heals you. Long falls, drowning, starving and wolf bites hurt. When you
  die, everything you carry and wear drops where you fell and you wake at your spawn
  point: go back for it before it vanishes (the five minutes only run while you are near).
- **In your hand**: your arms show at the bottom of the view. A tool stands up out of
  your right fist by its handle, with thickness; a block rests on the fist. The arm
  swings when you hit, dig or place.
- **Day and night** take 12 minutes. Wolves spawn at night and hunt you; they are a
  little slower than your walk, and a bite costs one heart. Right click a bed
  to set your spawn point and, at night, sleep until morning.
- **Monsters** come out at night in places with no block light, and are gone
  by morning: sunlight burns zombies and skeletons, and the rest slip away once out of
  sight. Sneaking shortens the distance at which they notice you. Hunters find their way
  to you: round walls, through doorways and open doors, up steps and down into pits and
  caves. Walls with a closed door keep them out; spiders and slimes are too wide for the
  gap an open door leaves, but spiders climb.
  - **Zombies** shamble at you in ones and twos and hit hard. They drop cloth and, now
    and then, an iron ingot.
  - **Skeletons** keep their distance and shoot arrows; break their line of sight or
    close in. They drop arrows and sometimes flint.
  - **Giant spiders** are fast, climb walls and pounce. They drop plant fibre.
  - **Slimes** hop after you. They drop clay.

- **More building pieces**: brick, sandstone, slate and cinderwood have matching stairs
  and slabs. Oak and birch fence gates open with right click and keep animals enclosed
  when shut. Signs hold four short lines: place one to write it, look at it to read,
  and right click to edit. Enter saves the text; Escape cancels.
- **Combat**: hold right click with a shield to raise it and block attacks from the front;
  it takes a quarter second to raise and wears when hit. Hold right click with a bow,
  then release to shoot; a full draw takes one second. Changing slots cancels the draw.
  The bar below the crosshair shows attack readiness. Enemies signal an approaching
  strike, giving you time to block or move out of reach.

## Repairing and enchanting

Craft an **anvil** from three iron blocks and four ingots at a workbench. Stand nearby,
open the inventory and choose **Equipment**, then pick damaged gear onto the cursor.
Each repair costs one level and one matching material and restores a quarter of maximum
durability. Right clicking the anvil opens this panel directly.

An **enchanting table** costs four slate, two crystals and three plant fibre. Its Equipment
panel offers **Efficiency** for mining tools, **Unbreaking** for durable equipment and
**Protection** for armour. Each goes up to level III and costs a crystal plus 3, 6 or 9
experience levels. Repairs and pyre upgrades preserve enchantments. These are deliberately
small versions of Minecraft's equipment systems, using Cubewild's existing materials.

Edited chunks, position, inventory (including the cursor and crafting grid), equipment,
signs, chests, dropped items, health and time of day are saved per world seed, and
settings separately:

| | Saves | Settings |
| --- | --- | --- |
| Linux | `~/.local/share/cubewild/world-<seed>/` | `~/.config/cubewild/settings.txt` |
| Windows | `%LOCALAPPDATA%\cubewild\world-<seed>\` | `%APPDATA%\cubewild\settings.txt` |
| macOS | `~/Library/Application Support/cubewild/` | same folder |

To move a world to another computer, copy its `world-<seed>` folder across.

World checkpoints run in the background every 30 seconds while the world is running and
when opening menus. Changing dimensions, saving to title, or quitting waits for the save
worker and commits the latest state before leaving. Terrain, inventory and container
contents commit together; an interrupted commit finishes before the world opens again.
The `.checkpoint-backup` folder keeps the previous versions of files replaced by the
latest checkpoint. Save failures stay visible and keep the world open so you can retry.
A damaged saved chunk pauses play and reports the error instead of regenerating over
your buildings.

Water and lava resume flowing after reloading. Falling sand and gravel, pending leaf
decay, and edits waiting for the next simulation tick are saved with their dimension;
they wait when their terrain is unloaded. Movement stops at unloaded chunk boundaries
until terrain arrives. Villager deaths, animal breeding cooldowns, and daily trade limits
also survive reloading; villagers restock at the next dawn, including after sleeping.

## Frame rate

"Hold 120 FPS" in the settings (on by default) shortens the view distance a chunk at a time
when the frame rate falls under 120 and restores it when there is room again. It does
nothing on displays slower than 120 Hz with vsync on. "Render scale" draws the world at a
lower resolution and stretches it; it helps where pixel fill is the limit (it was not on
Intel HD 620, where geometry is).

Block edits change collision and gameplay immediately, then workers rebuild their
meshes. Edits to the same chunk are combined, recent edits take priority over distant
terrain, and obsolete results are discarded. GPU uploads have a short time budget and
reuse existing buffers. Loading saved fluids is spread over frames, and autosave disk
writes run on a separate thread so a slow drive does not pause ordinary play.

For repeatable performance checks, use the release executable:

```sh
CUBEWILD_BENCH=12 CUBEWILD_DIST=8 target/release/cubewild
CUBEWILD_BENCH=12 CUBEWILD_DIST=8 CUBEWILD_BENCH_EDITS=1 CUBEWILD_BENCH_TURN=0 target/release/cubewild
cargo test --release gen_speed -- --ignored --nocapture
```

The first run flies through new terrain; the second repeatedly edits a corner shared
by four chunk meshes. Both print average FPS, 1% low, worst frame, and time to the first
player chunk mesh. They do not save a world. To exercise a real autosave, also set
`CUBEWILD_BENCH_AUTOSAVE=1` with a duration of at least 32 seconds; this creates a separate
temporary save folder and prints its path. `CUBEWILD_PROF=1` logs slow frame work and
autosave capture/completion times.

On an i3-7020U with Intel HD 620, the repeated-edit test at view distance 8 improved
from a 62.4 ms worst frame to 6.2 ms. See [PERFORMANCE.md](PERFORMANCE.md) for the test
conditions, measurements and HDD autosave verification.

## Environment variables

- `CUBEWILD_SEED=<n>` — skip the title screen and open the world of this seed (each seed
  has its own save); `CUBEWILD_MODE=creative|survival` sets its mode, and
  `CUBEWILD_NEW=creative|survival` starts a new world at once
- `CUBEWILD_DIST=<chunks>` — override the saved view distance
- `CUBEWILD_SCALE=<0.5..1>` — override the render scale
- `CUBEWILD_WORKERS=<1..8>` — background chunk threads (default: half the available
  hardware threads, limited to 1–4; two workers on a four-thread CPU)
- `CUBEWILD_BENCH=<seconds>` — automated fly-through with vsync off; prints average fps,
  1% low and worst frame. `CUBEWILD_BENCH_TURN` sets the turn rate, `CUBEWILD_PROF=1`
  logs slow frames.
- `CUBEWILD_SCREEN=settings|controls|inventory|play|chest`, `CUBEWILD_TEST_ITEMS=1` — open a menu
  at start (or go straight into the game) / add sample items (for checking the UI);
  `CUBEWILD_TIME=<0..1>` sets the time of day (0 sunrise, 0.25 noon, 0.5 sunset);
  `CUBEWILD_HOLD=<item id>` puts an item in the selected slot;
  `CUBEWILD_AT=<x,y,z>` and `CUBEWILD_LOOK=<yaw,pitch>` place the player
- `CUBEWILD_TEST_HOUSE=in|out`, `CUBEWILD_TEST_MOBS=1` — build a hut with doors, windows
  and one of each shaped block (`CUBEWILD_SCREEN=chest` opens its chest)
  around the player (start inside or outside it) / line up one of each monster nearby.
  These change the world: use them with a spare `CUBEWILD_SEED`

## Checking world generation

`cargo test --release` generates 124 worlds and checks that each has land and sea, a
start on level green land near the middle, no peaks cut flat, nothing floating, caves
that grow with depth, lava only at the bottom and cave chests standing on rock. To
look at worlds yourself:

```sh
CUBEWILD_MAP_DIR=/some/folder cargo test --release worldgen_survey -- --ignored --nocapture
```

prints land, mountain, desert, ore and cave figures for twenty seeds and writes a
top-down map of each (`map-<seed>.ppm`).

`cargo test --release village_survey -- --ignored --nocapture` counts the villages of
twelve seeds and prints where the one nearest the start is (F3 shows your position).

## Layout

- `src/world.rs` — blocks, terrain, caves and ores, where a new world's player starts, greedy mesher with AO and sky exposure, chunk streaming
- `src/gfx.rs` — renderer (one draw call per chunk, 8-byte packed terrain vertices; buried cave
  walls of far chunks are skipped while you are above ground)
- `src/player.rs` — movement, collision, ray casting
- `src/projectile.rs` — nearest collision of arrows and fireballs with terrain and bodies
- `src/mobs.rs` — animals, wolves, monsters and villagers
- `src/village.rs` — villages: where they stand, their layout, building them into chunks
- `src/path.rs` — path finding for hunting mobs (A* over walkable cells, one search a frame)
- `src/items.rs` — items, inventory, recipes, mining times
- `src/lighting.rs` — cached block light, propagated on mesh workers
- `src/equipment.rs`, `src/farming.rs`, `src/crafting.rs` — equipment progression, random farm ticks and shaped crafting
- `src/building.rs`, `src/combat.rs`, `src/controls.rs` — building interactions, combat and configurable input
- `src/persistence.rs`, `src/registry.rs` — coordinated game snapshots and versioned 16-bit block storage
- `src/inventory_ui.rs` — inventory, crafting and equipment screens
- `src/loot.rs` — items lying in the world, and what chests hold
- `src/ui.rs`, `src/textures.rs` — UI widgets and Noto Sans text, procedural block/item textures
