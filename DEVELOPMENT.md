# Survival improvement plan

Cubewild is a small Minecraft-style game. Prefer familiar Minecraft rules and controls,
scaled to this game's content. Keep gameplay responsive on the i3-7020U / Intel HD 620:
batch edits, bound simulation work, and keep generation, meshing and disk I/O off the
render thread. Preserve existing worlds and item metadata through every transition.

Baseline: `28e222f` (last night's work), 86 tests passed / 9 ignored.

- [x] 1. Shared, occluded block lighting for terrain, spawning and farming.
- [x] 2. Armour durability, XP repairs and basic enchantments.
- [x] 3. Hydrated farmland, light-dependent growth and two additional crops.
- [x] 4. Crafting grids, stack dragging, creative pick block and remappable controls.
- [x] 5. Fence gates, double chests, signs and matching stairs/slabs.
- [x] 6. Shields, attack cooldown feedback, charged bows and enemy attack cues.
- [x] 7. Extract gameplay responsibilities; version block storage and widen IDs.

For each step: exercise gameplay boundaries and persistence, run the relevant tests,
then run the complete release suite. At the end compare generation/meshing and real
frame-time benchmarks against the baseline, including repeated edits and autosaving.
Report measured limits honestly; compilation alone does not verify interactive play.

## Minecraft references

- [Controls](https://www.minecraft.net/en-us/article/minecraft-controls)
- [Farmland: water within four blocks, light and trampling](https://www.minecraft.net/en-us/article/block-week-farmland)
- [Shields: use to block attacks from the front](https://www.minecraft.net/en-us/article/taking-inventory--shield)
- [Overworld monsters require complete darkness](https://www.minecraft.net/en-us/article/new-world-generation-java-available-testing)
- [Fence gates and their recipe](https://www.minecraft.net/en-us/article/fence-gate)
- [Enchantments](https://learn.microsoft.com/en-us/minecraft/creator/documents/introtoenchantments)

## Implementation

All seven requested systems are implemented. Lighting, farming, crafting, equipment,
controls, building interactions, combat, persistence and inventory UI have dedicated
modules; `main.rs` is approximately 3,800 lines rather than 5,580.

New chunks use a checksummed `CWB2` header, format and registry versions, a cell count,
and little-endian 16-bit IDs. Existing raw byte chunks and half-height worlds remain
readable. Full-height byte chunks upgrade when edited; untouched terrain is not rewritten.
Height migrations and edited chunks commit through coordinated checkpoints. Permanent ID families live
in `src/registry.rs`; old ID meanings are never reassigned. Terrain materials use a
separate 16-bit texture index, removing the old seven-bit limit.

Static light is built on workers, cropped vertically to the region sources can reach.
Mesh jobs reuse current light where transmission and emission did not change. Changing
walls or emitters invalidates neighbouring light and meshes; stale jobs cannot publish
results. Workers bake brightness into a vertex byte; the GPU only normalizes it,
with no light-table lookup or per-pixel source loop.

The XP systems use existing Cubewild materials and three selectable enchantment tiers.
Signs have four 15-character lines and display their text when targeted. Double chests
retain two independent 27-slot stores so splitting them preserves both halves.

## Validation

`cargo test --offline --release`: **108 passed, 9 ignored**, with no compiler warnings.
The new coverage exercises wall and border lighting, stale/cached light, crop hydration
and darkness, equipment costs/metadata/upgrades, shaped crafting, full-inventory grid
checkpoints, stack distribution, binding persistence, container pairing, sign persistence,
gate orientation, combat warnings, directional defence and versioned chunk corruption.

Automated desktop flows passed in isolated temporary worlds, with assertions against
their saved state: 2×2 and 3×3 crafting, even stack dragging, XP repair/enchanting,
large-chest shift transfer, sign editing, charged-bow release and cancellation, shield
raising, and remapping/reloading controls. Screenshots were inspected and revealed a
texture-layer routing bug and small-window layout issues, both fixed. Performance runs
are separate from these functional tests and from compilation.

Final matched performance results are recorded in `PERFORMANCE.md`.
