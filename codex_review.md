# Codex review — Cubewild

Reviewed commit: `0089b73`

**Implementation update — 2026-11-12:** Reliability findings **1–8** below are now addressed. The first pass fixed crafting, height-limit lava, coordinated recoverable checkpoints, and cursor-stack loss. The second pass persists fluid queues, falling blocks, leaf decay and pending edits; resolves the nearest projectile collision; blocks movement into unloaded chunks and prioritizes nearby terrain; and persists villager deaths, breeding cooldowns and daily trade limits. Added **31 regression tests** across both passes; `cargo test --offline --release` passes with **76 passed, 9 ignored**. The feature roadmap remains pending. The original review follows unchanged.

**Cubewild already has enough features to become a small Minecraft-style game. The biggest improvements now are reliability, consistent world rules, and stronger progression.**

I reviewed the source in `Projects/cubewild`, including gameplay, saving, inventory, world generation, rendering, and mobs. **No project code or settings changed during the review.** The freshly compiled tests passed: **45 passed, 9 ignored**. Two additional checks in `/tmp` reproduced crafting problems. This was a source review, not an interactive visual playtest.

You already have a substantial foundation: survival and creative modes, mining, crafting, furnaces, equipment tiers, biomes, caves, villages, trading, breeding, taming, boats, weather, flowing fluids, and the Emberdeep dimension.

These are the bugs I would address first, ordered roughly by impact. The crafting issues were reproduced directly; the others were identified by tracing the code.

1. **High — Crafting can consume materials without giving the result.**  
   With a full inventory and a different item on the cursor, crafting removes ingredients and silently discards output that cannot fit. I reproduced a log disappearing without producing planks. Matching cursor stacks can also exceed their limit: I reproduced **103 planks in a stack limited to 99**. Crafting should check output capacity before consuming anything.  
   [items.rs](src/items.rs), line 1083.

2. **High — Lava at the height limit can crash a saved world on loading.**  
   Chunk loading checks the block above lava using `data.blocks[i + 256]` without checking the upper boundary. Lava at `y = 255`, at qualifying positions such as local `x = 1, z = 1`, accesses beyond the chunk array. A player can create this through ordinary placement.  
   [world.rs](src/world.rs), line 1953.

3. **High — Saving can leave inventory and terrain out of sync after a crash.**  
   Opening menus saves player/container state without necessarily saving the corresponding chunk edits. Files are overwritten individually, and there is no periodic, consistent world checkpoint. A crash can therefore preserve collected items while restoring the blocks they came from. Truncated chunk files are also treated as missing and regenerated. Add atomic file replacement, coordinated checkpoints, backups, and visible save failures.  
   [main.rs](src/main.rs), line 1383; [world.rs](src/world.rs), line 1857.

4. **High — Closing the game can lose the stack on your cursor.**  
   The save routine preserves that stack only if an inventory slot is empty. Taking items from a chest while your bag is full, then closing the window, can lose them. Returning from inventory to gameplay also skips the intended cursor-stowing logic. Save the cursor explicitly or reliably return/drop its contents.  
   [main.rs](src/main.rs), lines 1388 and 1436.

5. **Medium — Fluid movement and falling blocks do not survive reloads correctly.**  
   Fluid queues are cleared and are not reconstructed when chunks load. Saving during a flow can leave water or lava frozen until another nearby edit wakes it. Falling sand/gravel is removed from the terrain but its falling entity is not saved, so it can disappear when leaving the world mid-fall. Pending leaf decay has similar persistence gaps.  
   [fluid.rs](src/fluid.rs), line 78; [settle.rs](src/settle.rs), line 52; [main.rs](src/main.rs), line 1815.

6. **Medium — Projectiles can damage targets through walls.**  
   Each projectile checks for an entity hit before checking terrain. If one movement step crosses both a wall and a target behind it, the target takes damage first. This is especially relevant to thin doors and panes. Calculate both hit distances and resolve the nearest collision.  
   [main.rs](src/main.rs), line 3646.

7. **Medium — Players can fall into terrain that has not loaded yet.**  
   Missing chunks return `AIR`, and player readiness remains true after initial loading. If movement outruns chunk generation, collision sees empty space where terrain should be. Protect movement at unloaded boundaries and prioritize loading around the player.  
   [world.rs](src/world.rs), line 2076; [main.rs](src/main.rs), line 3353.

8. **Medium — NPC consequences and cooldowns are not fully persistent.**  
   Killed-villager records are held only in memory, so villagers can return after reloading or changing dimensions. Animal breeding cooldowns are omitted from saves, and restarting the application resets trade usage limits. These allow players to bypass intended restrictions and make villages feel inconsistent.  
   [mobs.rs](src/mobs.rs), lines 311 and 714; [main.rs](src/main.rs), line 2002.

**The largest gameplay weakness is that several existing systems need deeper connections.** These are design limitations, rather than necessarily bugs:

- **XP has no spending mechanism.** You gain levels, but there is no enchanting or repair system that makes those levels useful.
- **Armour never wears out.** Once players obtain strong armour, an important reason to keep gathering materials disappears. Tools already have durability; armour does not. [items.rs](src/items.rs), line 496.
- **Farming has few meaningful decisions.** Wheat advances randomly near the player without checking hydration or light. Irrigation, lighting, crop variety, and soil conditions would make farms worth designing. [main.rs](src/main.rs), line 3692.
- **Lighting does not represent the world consistently.** Torch illumination is distance-based, so it passes through walls. Mob spawning separately checks proximity to light sources. A shared system for light propagation would improve caves, houses, farming, and spawn prevention together. [gfx.rs](src/gfx.rs), line 202.
- **Discoverability trails implementation.** The README omits several major features already present, and its height information predates the terrain lift. Players need an updated guide and a small set of optional progression hints.

For the Minecraft feel, I would make the first session follow a clear sequence: **gather wood → craft tools → build shelter → establish food → mine better materials → explore the Emberdeep**. Minecraft's own beginner guidance emphasizes gathering, crafting, and shelter before night; that is a useful foundation for your onboarding. [Minecraft's first-day guide](https://www.minecraft.net/en-us/article/how-survive-your-first-day)

I would build the next features in this order:

| Priority | Feature | A manageable first version |
|---|---|---|
| 1 | Reliable survival and building | Fix the bugs above; add regression coverage for saving, full inventories, projectiles, and chunk boundaries. |
| 2 | Familiar crafting and controls | Optional 2×2 inventory and 3×3 workbench grids alongside the recipe book; draggable stack distribution; creative middle-click acquisition; remappable controls. |
| 3 | Useful XP and equipment progression | Repair station, armour durability, and three simple enchantments: mining speed, durability, and protection. |
| 4 | Better homes and farms | Fence gates, double chests, signs, more matching stairs/slabs, irrigated farmland, and two additional crops. |
| 5 | More deliberate combat | Shield blocking, a visible attack cooldown, charged bow shots, and clearer enemy attack cues. |
| 6 | A small automation system | Wire, lever, button, pressure plate, lamp, and powered door; add item transfer later. |
| 7 | A reason to finish an expedition | One distinctive fortress encounter in the existing Emberdeep, with a unique reward and a clear preparation path. |

For appearance, the most promising changes from the rendering code are **textured mob skins, consistent material palettes, animated water/lava, and better local lighting**. Terrain already uses procedural 16×16 textures, while mobs largely use solid-colour boxes. Giving creatures comparable surface detail would help the world feel more cohesive. I would assess the results through screenshots and playtesting before deciding whether shadows or more expensive effects are worthwhile.  
[textures.rs](src/textures.rs), line 1; [mobs.rs](src/mobs.rs), line 1324.

There are also three engineering weaknesses worth addressing as those features are developed:

- **`main.rs` has grown beyond 5,000 lines.** Saving, combat, interactions, menus, and simulation share one large state object. Extracting those responsibilities would make bugs around screen changes and world transitions easier to prevent.
- **Block and texture IDs are tightly constrained.** Block types and states share a byte, while terrain textures use seven packed bits. More wood variants, machinery, and block states will strain this layout. Plan a versioned block registry and save format before a large content expansion.
- **Edits rebuild chunk meshes synchronously.** Fluids, explosions, and construction can trigger substantial work on the render thread. Profile these cases, then batch edits and use the existing background workers more consistently. [world.rs](src/world.rs), line 2133.

I would defer multiplayer, additional dimensions, and a large machinery catalogue until a player can build a base, run a farm, complete an expedition, and repeatedly save/reload without losing items or changing the world's behaviour.
