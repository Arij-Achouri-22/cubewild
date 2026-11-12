# Performance measurements

Measured on this PC: Intel Core i3-7020U (2 cores / 4 threads), Intel HD Graphics 620
(Vulkan), approximately 12 GB RAM, and a Toshiba MQ04ABF100 mechanical HDD.

## Survival systems update — 2026-11-12

Compared the completed items 1–7 against the exact committed executable at `28e222f`.
Four runs alternated baseline/final/baseline/final after compilation finished. Each used
seed 2026, native X11 fullscreen at **1366×768**, render scale 1, automatic scaling off,
VSync off, an 8-chunk view distance, 6 seconds of warmup and 12 measured seconds.
A stationary player repeatedly changed a block at a chunk corner six times per second.
Both versions ended with 101 visible chunks; the added crop/building geometry increased
visible quads from 149,488 to 149,774.

| Measurement | Baseline 1 | Final 1 | Baseline 2 | Final 2 |
| --- | ---: | ---: | ---: | ---: |
| Average uncapped FPS | 419.1 | 396.9 | 389.9 | 376.0 |
| 1% low FPS | 169.4 | 157.8 | 130.7 | 134.4 |
| Worst frame | 12.26 ms | 8.64 ms | 10.94 ms | 11.67 ms |
| Frames over 8.33 ms | 1 | 2 | 12 | 8 |
| First player chunk mesh | 421.7 ms | 421.6 ms | 391.4 ms | 571.9 ms |

The two-run mean average FPS is **4.5% lower** with the new systems, and mean 1% low
is 2.6% lower. Every measured final frame stayed below the 16.67 ms budget for 60 Hz.
These short runs show variation and a remaining rendering cost; they do not establish
a speedup. Early runs that overlapped compilation or used different resolutions were
discarded. No functional desktop tests ran alongside these measurements.

Flood lighting runs on mesh workers, reuses valid cached light for ordinary unlit
material edits, and crops propagation to the heights reachable from sources. Workers
bake a brightness byte into each vertex, avoiding GPU light-table lookup and the old
per-pixel source loop. Wider block IDs double raw chunk cell storage; untouched legacy
chunks are migrated only when edited so loading an old world does not rewrite it all.

The final **32-second HDD autosave** run used the same scene and settings, with
`TMPDIR` on `target/perf-tmp` on the mechanical drive. Snapshot capture took **0.17 ms**;
the save worker completed in **1,194.5 ms**. Average FPS was 415.8, 1% low was **166.5**,
the worst frame was **9.62 ms**, and four frames exceeded 8.33 ms. Saving therefore
remained asynchronous during this measured run.

The final cold CPU benchmark generated 64 chunks and meshed one neighbourhood 40 times
per dimension, rebuilding light each time:

| CPU work per chunk | Overworld | Emberdeep |
| --- | ---: | ---: |
| Generation mean | 7.52 ms | 2.19 ms |
| Cold mesh mean | 5.10 ms | 9.27 ms |
| Cold mesh p95 | 6.35 ms | 14.09 ms |

Cold lighting adds work compared with the old distance-based glow, especially around
the Emberdeep's many sources. These timings run on workers and do not model cached
material edits. The geometry/lighting fixture fingerprint is `2ecb44ebf180e9af`; its
test normalizes the GPU brightness packing so a storage-layout change cannot hide a
geometry or light-level regression. The full release suite passes with **108 passed,
9 ignored**, and the explicitly invoked generation/meshing benchmark also passes.

## Earlier frame-stall fixes (`28e222f`)

The baseline below is commit `0a6f637` with only the same benchmark instrumentation added.
The optimized executable in these original measurements is commit `28e222f`.
The later survival-system changes are measured separately above. These are individual
runs on the same machine; background activity and thermal conditions can affect timings.

### Repeated block edits

Both runs used seed 2026, an 8-chunk view distance, a 668×752 window, VSync off,
6 seconds of warmup and 12 seconds of measurement. The player stayed still and a block
at a chunk corner changed six times per second, affecting four meshes. Both rendered
73 chunks and 119,095 quads at the end of the run.

| Measurement | Baseline | Optimized |
| --- | ---: | ---: |
| Average uncapped FPS | 506.9 | 619.2 |
| 1% low FPS | 36.0 | 226.6 |
| Worst frame | 62.37 ms | 6.15 ms |
| Frames over 8.33 ms | 72 | 0 |
| First player chunk mesh | 267.8 ms | 159.1 ms |

The first-mesh measurement covers the player's chunk; filling the entire view distance
takes longer. This stress test measures editing stalls, rather than promising the same
FPS in every scene.

An ordinary fly-through at the same resolution and distance improved from 743.8 to
786.1 average uncapped FPS, with 1% low improving from 230.6 to 258.8 FPS. Both runs
finished with 60 visible chunks and 49,784 quads.

### Autosaving on the HDD

`/tmp` is a RAM-backed filesystem on this PC, so the drive test instead set `TMPDIR` to
the ignored `target/perf-tmp` folder on `/dev/sda3` (ext4). During a 32-second measured
block-edit run, capturing the snapshot took **0.22 ms** and the asynchronous save
completed in **1,101.8 ms**. The run's worst frame was **6.40 ms**, with no frames over
8.33 ms, and its 1% low was **218.2 FPS**. The benchmark then waited and committed its
latest state before exiting.

### CPU generation and meshing

The corrected `gen_speed` benchmark uses full-height chunks from the current generators.
Its old implementation incorrectly passed legacy half-height chunks to the modern
mesher. Generation samples 64 chunks; meshing samples 40 builds of the same neighbourhood.

| CPU work per chunk | Baseline mean | Optimized mean |
| --- | ---: | ---: |
| Overworld generation | 7.07 ms | 6.60 ms |
| Overworld mesh | 4.89 ms | 2.85 ms |
| Emberdeep generation | 1.81 ms | 1.78 ms |
| Emberdeep mesh | 4.62 ms | 2.39 ms |

Generation rules are unchanged. The main generation improvement comes from worker
utilization and scheduling; the small single-thread generation difference can be noise.
The mixed-material mesh fixture retains its original fingerprint, `67ff6749e9ace203`,
covering geometry, ambient occlusion, sky lighting, transparency and fluid surfaces.

### Changes

- Mining, placement, fluids and bulk edits update block data immediately and invalidate
  meshes for background work. Repeated edits are coalesced; identical writes do no work.
- Workers select recent edits and nearby terrain ahead of distant queued work. Defaults
  use two workers on this CPU, with modestly reduced priority instead of idle scheduling.
- Mesh job tickets and chunk versions reject results for obsolete data or unloaded chunks.
- Block classification is cached for the mesher's inner loops.
- GPU uploads prioritize recent edits, reuse buffers and run within a short frame budget.
- Saved-fluid activation is processed in bounded batches.
- Autosaves and menu checkpoints capture immutable state and perform disk work on a
  separate thread. Later edits stay dirty; quitting and world changes wait and save the
  latest state. Recovery journals and backups remain in use.

### Repeat the checks

```sh
cargo test --offline --release
cargo test --offline --release gen_speed -- --ignored --nocapture
cargo build --offline --release
CUBEWILD_BENCH=12 CUBEWILD_DIST=8 target/release/cubewild
CUBEWILD_BENCH=12 CUBEWILD_DIST=8 CUBEWILD_BENCH_EDITS=1 CUBEWILD_BENCH_TURN=0 target/release/cubewild
CUBEWILD_BENCH=32 CUBEWILD_DIST=8 CUBEWILD_BENCH_EDITS=1 CUBEWILD_BENCH_TURN=0 CUBEWILD_BENCH_AUTOSAVE=1 CUBEWILD_PROF=1 target/release/cubewild
```

Normal benchmarks do not save a world. The autosave benchmark creates a separate
temporary save folder and prints its location. Keep window size, view distance, render
scale and settings consistent for comparisons. At `28e222f`, the test suite passed with
**86 passed, 9 ignored**, including background-save failures, immutable snapshots, concurrent edits,
stale meshes and worker shutdown.

To measure the drive rather than a RAM-backed `/tmp`, first create `target/perf-tmp`
and set `TMPDIR` to its absolute path for the autosave command.
