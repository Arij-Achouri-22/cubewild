# Performance measurements

Measured on this PC: Intel Core i3-7020U (2 cores / 4 threads), Intel HD Graphics 620
(Vulkan), approximately 12 GB RAM, and a Toshiba MQ04ABF100 mechanical HDD.

The baseline is commit `0a6f637` with only the same benchmark instrumentation added.
The optimized executable uses the changes in this working tree. These are individual
runs on the same machine; background activity and thermal conditions can affect timings.

## Repeated block edits

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

## Autosaving on the HDD

`/tmp` is a RAM-backed filesystem on this PC, so the drive test instead set `TMPDIR` to
the ignored `target/perf-tmp` folder on `/dev/sda3` (ext4). During a 32-second measured
block-edit run, capturing the snapshot took **0.22 ms** and the asynchronous save
completed in **1,101.8 ms**. The run's worst frame was **6.40 ms**, with no frames over
8.33 ms, and its 1% low was **218.2 FPS**. The benchmark then waited and committed its
latest state before exiting.

## CPU generation and meshing

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

## Changes

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

## Repeat the checks

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
scale and settings consistent for comparisons. The test suite passes with **86 passed,
9 ignored**, including background-save failures, immutable snapshots, concurrent edits,
stale meshes and worker shutdown.

To measure the drive rather than a RAM-backed `/tmp`, first create `target/perf-tmp`
and set `TMPDIR` to its absolute path for the autosave command.
