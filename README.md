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
| Middle click | select the block you look at |
| 1-9, mouse wheel | choose hotbar slot |
| E or Tab | inventory and crafting |
| F, or Space twice | fly, in creative worlds (Space up, Shift down) |
| F3 / F11 | debug overlay / fullscreen |
| Esc | pause menu (settings, controls, save and quit) |

## Gathering and crafting

Broken blocks drop as items where they stood (stone drops cobblestone, grass drops dirt,
leaves sometimes drop a stick) and animals drop meat and hide where they fall; walk over
an item to pick it up. Items left lying vanish after five minutes. Placing a block uses one.
The inventory window has a 9-slot hotbar, a 27-slot backpack, four armour slots (head,
chest, legs, feet) beside a picture of your character, and a crafting area. Left click
picks a stack up and puts it down, right click takes half or drops one, Shift+click sends
a stack between hotbar and backpack (or puts armour on), and Sort tidies the backpack.
The recipe book on the left has tabs (Tools, Weapons, Armour, Utility, Building,
Materials, Food) and a "Can craft" filter; pick a recipe, then click the result slot to
take it (Shift+click crafts as many as you can). Hover anything for its details.

Recipes are in `src/items.rs`. Basic tools and torches can be made anywhere; a **workbench**
unlocks the furnace, bed, iron tools, crystal pickaxe and crystal lamp, and a **furnace**
(stand within a few steps of either) smelts iron ore, cooks meat, and burns logs into coal.

- **Ores** sit in underground stone: coal anywhere, iron below y 60 (needs a stone
  pickaxe), gold below y 34, crystal below y 22 (needs an iron pickaxe). The pockets
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
  light their surroundings, and a torch in your hand lights the way.
- **Tools** come in wood, stone, iron, gold (fast but brittle) and crystal: pickaxes,
  axes, shovels, hoes and swords. They wear out; a bar on the icon shows what is left.
  Also: shears, bucket (scoops and pours water), fishing rod (hold right click on water),
  bow and arrows, compass and clock (shown at the top of the screen while carried).
- **Farming**: till grass or dirt with a hoe, plant seeds (from grass tufts), wait for the
  wheat to turn gold, and bake bread.
- **The land**: oak, birch and pine trees, grass tufts (plant fibre), flowers, mushrooms and
  berry bushes (stew), gravel (flint) and clay (bricks) on the sea bed, gold deep down.
- **Armour**: leather, gold, iron and crystal pieces each absorb a share of the damage from
  bites and falls; a full crystal set absorbs 64%.
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
  Shift+click sends a stack across. A broken chest spills what it held.
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
- **Monsters** come out at night, never within 9 blocks of a torch or lamp, and are gone
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

Edited chunks, position, inventory, chests, dropped items, health and time of day are saved per world seed, and
settings separately:

| | Saves | Settings |
| --- | --- | --- |
| Linux | `~/.local/share/cubewild/world-<seed>/` | `~/.config/cubewild/settings.txt` |
| Windows | `%LOCALAPPDATA%\cubewild\world-<seed>\` | `%APPDATA%\cubewild\settings.txt` |
| macOS | `~/Library/Application Support/cubewild/` | same folder |

To move a world to another computer, copy its `world-<seed>` folder across.

## Frame rate

"Hold 120 FPS" in the settings (on by default) shortens the view distance a chunk at a time
when the frame rate falls under 120 and restores it when there is room again. It does
nothing on displays slower than 120 Hz with vsync on. "Render scale" draws the world at a
lower resolution and stretches it; it helps where pixel fill is the limit (it was not on
Intel HD 620, where geometry is).

## Environment variables

- `CUBEWILD_SEED=<n>` — skip the title screen and open the world of this seed (each seed
  has its own save); `CUBEWILD_MODE=creative|survival` sets its mode, and
  `CUBEWILD_NEW=creative|survival` starts a new world at once
- `CUBEWILD_DIST=<chunks>` — override the saved view distance
- `CUBEWILD_SCALE=<0.5..1>` — override the render scale
- `CUBEWILD_WORKERS=<n>` — background chunk threads (default: physical cores - 1)
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
- `src/gfx.rs` — renderer (one draw call per chunk, 4-byte packed vertices; buried cave
  walls of far chunks are skipped while you are above ground)
- `src/player.rs` — movement, collision, ray casting
- `src/mobs.rs` — animals, wolves, monsters and villagers
- `src/village.rs` — villages: where they stand, their layout, building them into chunks
- `src/path.rs` — path finding for hunting mobs (A* over walkable cells, one search a frame)
- `src/items.rs` — items, inventory, recipes, mining times
- `src/loot.rs` — items lying in the world, and what chests hold
- `src/ui.rs`, `src/textures.rs` — UI widgets and Noto Sans text, procedural block/item textures
