# Cubewild development priorities

Cubewild is a small Minecraft-style clone. Keep new gameplay, recipes, placement rules
and controls familiar to Minecraft players. Check Minecraft's behavior when designing
changes; keep deliberate simplifications small and explain them in the player guide.

Keep the game fast and smooth on the i3-7020U / Intel HD 620 described in PERFORMANCE.md.
Keep expensive generation, lighting, meshing and world I/O off the render thread during
ordinary play, bound work per frame, reuse valid results, and discard stale background results. Judge performance by
frame times and 1% lows as well as average FPS. Compare identical scenes, resolutions,
render scales and settings, with builds finished before benchmarking.

Watch for bugs throughout implementation, especially item conservation, equipment
metadata, container transfers, world transitions, unloaded chunks and save recovery.
Preserve existing worlds. Add meaningful regression coverage for gameplay and
persistence changes, run the appropriate tests, and verify affected desktop flows.
Use isolated temporary worlds for destructive playtests and benchmark saves.

See DEVELOPMENT.md for the Minecraft references and implemented survival systems,
and PERFORMANCE.md for reproducible release benchmarks.
