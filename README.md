# Cubewild

A small open-world voxel sandbox in Rust (wgpu/Vulkan + winit): build with blocks, explore
endless terrain with caves, ores and deserts, hunt deer, boar and hare, and survive the
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

## Controls

| Key | Action |
| --- | --- |
| WASD / Space / Ctrl / Shift | move / jump / sprint / sneak (animals notice you later) |
| Hold left click | break block, or hit an animal in reach |
| Right click | place the selected block, eat the held food, sleep in a bed, open a workbench or furnace |
| Middle click | select the block you look at |
| 1-9, mouse wheel | choose hotbar slot |
| E or Tab | inventory and crafting |
| F | fly (Space up, Shift down) |
| F3 / F11 | debug overlay / fullscreen |
| Esc | pause menu (settings, controls, save and quit) |

## Gathering and crafting

Broken blocks go into your inventory (stone drops cobblestone, grass drops dirt, leaves
sometimes drop a stick); placing a block uses one. Animals drop meat and hide.
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
  pickaxe), crystal below y 22 (needs an iron pickaxe). Caves wind through the rock.
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
- **Survival**: you have health and hunger. Eat meat (cooked is far better) to stay fed;
  a full stomach heals you. Long falls, drowning, starving and wolf bites hurt. When you
  die you keep your items and wake at your spawn point.
- **Day and night** take 12 minutes. Wolves spawn at night and hunt you. Right click a bed
  to set your spawn point and, at night, sleep until morning.
- **Monsters** come out at night, never within 9 blocks of a torch or lamp, and are gone
  by morning: sunlight burns zombies and skeletons, and the rest slip away once out of
  sight. Sneaking shortens the distance at which they notice you.
  - **Zombies** shamble at you in ones and twos and hit hard. They drop cloth and, now
    and then, an iron ingot.
  - **Skeletons** keep their distance and shoot arrows; break their line of sight or
    close in. They drop arrows and sometimes flint.
  - **Giant spiders** are fast, climb walls and pounce. They drop plant fibre.
  - **Slimes** hop after you. They drop clay.

Edited chunks, position, inventory, health and time of day are saved per world seed, and
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

- `CUBEWILD_SEED=<n>` — world seed (default 2026; each seed has its own save)
- `CUBEWILD_DIST=<chunks>` — override the saved view distance
- `CUBEWILD_SCALE=<0.5..1>` — override the render scale
- `CUBEWILD_WORKERS=<n>` — background chunk threads (default: physical cores - 1)
- `CUBEWILD_BENCH=<seconds>` — automated fly-through with vsync off; prints average fps,
  1% low and worst frame. `CUBEWILD_BENCH_TURN` sets the turn rate, `CUBEWILD_PROF=1`
  logs slow frames.
- `CUBEWILD_SCREEN=settings|controls|inventory`, `CUBEWILD_TEST_ITEMS=1` — open a menu at
  start / add sample items (for checking the UI); `CUBEWILD_TIME=<0..1>` sets the time of
  day (0 sunrise, 0.25 noon, 0.5 sunset)

## Layout

- `src/world.rs` — blocks, terrain, caves and ores, greedy mesher with AO and sky exposure, chunk streaming
- `src/gfx.rs` — renderer (one draw call per chunk, 4-byte packed vertices)
- `src/player.rs` — movement, collision, ray casting
- `src/mobs.rs` — animals, wolves and monsters
- `src/items.rs` — items, inventory, recipes, mining times
- `src/ui.rs`, `src/textures.rs` — UI widgets and Noto Sans text, procedural block/item textures
