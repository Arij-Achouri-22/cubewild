//! Gameplay responsibilities extracted from the application event loop.

use super::*;

impl Game {
    pub(super) fn capture_checkpoint(&self) -> Option<SaveSnapshot> {
        let root = self.save_dir.as_ref()?;
        let mut files = save::Snapshot::new(root.clone());
        let p = &self.player;
        let mut s = format!("{} {} {} {} {}\n{}", p.pos.x, p.pos.y, p.pos.z, p.yaw, p.pitch, self.inv.save_stacks());
        s += &format!("\nsel {}\n", self.sel);
        s += &format!("stats {} {} {} {}\n", self.health, self.food, self.air, self.time);
        s += &format!("spawn {} {} {}\n", self.spawn.x, self.spawn.y, self.spawn.z);
        s += if self.mode == Mode::Creative { "mode creative\n" } else { "mode survival\n" };
        s += &format!("gen {}\n", self.over_ver);
        if self.world.is_deep() {
            s += "dim deep\n";
        }
        s += &format!("xp {} {}\n", self.level, self.xp);
        s += &format!("weather {} {} {}\n", self.weather.want, self.weather.timer, self.weather.storm as u8);
        files.text("player.txt", s);
        let gates: String = self.gates.iter().map(|g| format!("{} {} {} {} {}\n", g.deep as u8, g.at.x, g.at.y, g.at.z, g.axis)).collect();
        files.text("gates.txt", gates);
        files.text("trades.txt", trade::encode_uses(&self.trade_uses));
        // the rest belongs to the world the player is in
        let d = PathBuf::from(if self.world.is_deep() { "deep" } else { "" });
        let chunks = self.world.snapshot_chunks();
        for (key, data) in &chunks {
            files.chunk(d.join(format!("c.{}.{}.bin", key.0, key.1)), data.clone());
        }
        files.text(d.join("updates.txt"), self.world.encode_updates());
        files.text(d.join("drops.txt"), self.pickups.encode());
        files.text(d.join("chests.txt"), loot::encode_chests(&self.chests));
        files.text(d.join("signs.txt"), building::encode_signs(&self.signs));
        files.text(d.join("furnaces.txt"), furnace::encode(&self.furnaces));
        files.text(d.join("boats.txt"), self.boats.encode());
        files.text(d.join("animals.txt"), self.mobs.encode());
        files.text(d.join("fluids.txt"), self.fluids.encode());
        files.text(d.join("settle.txt"), self.settle.encode());
        Some(SaveSnapshot { files, chunks })
    }

    pub(super) fn finish_autosave(&mut self) -> std::io::Result<()> {
        if let Some(pending) = self.pending_save.take() {
            pending.writer.wait()?;
            self.world.snapshot_committed(&pending.chunks);
            if std::env::var_os("CUBEWILD_PROF").is_some() {
                println!("autosave completed in {:.1}ms on the save worker", pending.started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        Ok(())
    }

    pub(super) fn start_autosave(&mut self) -> bool {
        if self.pending_save.is_some() {
            return true;
        }
        let started = Instant::now();
        let Some(snapshot) = self.capture_checkpoint() else { return true };
        self.last_checkpoint = Instant::now();
        match save::Background::start(snapshot.files) {
            Ok(writer) => {
                self.pending_save = Some(PendingSave { writer, chunks: snapshot.chunks, started });
                if std::env::var_os("CUBEWILD_PROF").is_some() {
                    println!("autosave capture {:.2}ms", started.elapsed().as_secs_f64() * 1000.0);
                }
                true
            }
            Err(e) => {
                self.report_save_error(e);
                false
            }
        }
    }

    pub(super) fn poll_autosave(&mut self) -> bool {
        if self.pending_save.as_ref().is_some_and(|p| p.writer.is_finished()) {
            self.last_checkpoint = Instant::now();
            if let Err(e) = self.finish_autosave() {
                self.report_save_error(e);
                return false;
            }
            self.save_error = None;
        }
        true
    }

    pub(super) fn write_checkpoint(&mut self) -> std::io::Result<()> {
        // A manual retry also recovers a failed older journal before writing the
        // current state. There is never more than one writer for this world.
        let _ = self.finish_autosave();
        if let Some(snapshot) = self.capture_checkpoint() {
            snapshot.files.commit()?;
            self.world.snapshot_committed(&snapshot.chunks);
        }
        Ok(())
    }

    pub(super) fn report_save_error(&mut self, error: std::io::Error) {
        let message = format!("Save failed: {error}. Your world is still open; retry saving before leaving.");
        eprintln!("cubewild: {message}");
        self.save_error = Some(message);
    }

    pub(super) fn checkpoint(&mut self) -> bool {
        let result = self.write_checkpoint();
        self.last_checkpoint = Instant::now();
        match result {
            Ok(()) => {
                self.save_error = None;
                true
            }
            Err(e) => {
                self.report_save_error(e);
                false
            }
        }
    }

    pub(super) fn install(&mut self, l: Loaded) {
        // Entering another world already saved; deleting the current one must also
        // wait for any writer before its directory can be removed.
        let _ = self.finish_autosave();
        self.world = l.world;
        self.player = l.player;
        self.inv = l.inv;
        self.grid_open=self.inv.grid.iter().any(|s|!s.is_empty());
        self.equipment_open=false;self.drag=None;self.rebinding=None;
        self.sel = l.sel;
        (self.health, self.food, self.air, self.time) = (l.health, l.food, l.air, l.time);
        self.spawn = l.spawn;
        self.pickups = l.pickups;
        self.chests = l.chests;
        self.signs = l.signs;
        self.sign_edit.clear();
        self.furnaces = l.furnaces;
        (self.level, self.xp) = (l.level, l.xp);
        self.weather = l.weather;
        self.boats = l.boats;
        self.fluids = l.fluids;
        self.settle = l.settle;
        self.trade_uses = l.trade_uses;
        self.riding = None;
        self.orbs = Orbs::default();
        self.particles = Particles::default();
        self.mode = l.mode;
        self.save_dir = l.save_dir;
        self.last_checkpoint = Instant::now();
        self.save_error = None;
        self.load_notice = None;
        self.mobs = l.mobs;
        self.mobs.ignore = self.mode == Mode::Creative;
        self.trading = None;
        self.over_ver = l.over_ver;
        self.gates = load_gates(&self.save_dir);
        self.arrival = None;
        (self.gate_t, self.gate_lock) = (0.0, false);
        if self.world.is_deep() {
            self.unstick = false;
        }
        self.arrows.clear();
        self.combat = combat::State::default();
        self.mining = None;
        self.fishing = None;
        self.ready = false;
        self.unstick = true;
        self.started = false;
        self.toast.1 = 0.0;
        self.tally.1 = 0.0;
        self.auto_cur = 20;
        self.apply_dist();
    }

    pub(super) fn switch_world(&mut self, deep: bool) -> bool {
        if !self.checkpoint() {
            return false;
        }
        let ver = if deep { deep::DEEP_VER } else { self.over_ver };
        let dir = dim_dir(&self.save_dir, deep);
        let seed = self.world.seed;
        let radius = if deep { self.settings.view_dist.min(DEEP_DIST) } else { self.settings.view_dist };
        let world = World::new(seed, ver, radius, dir.clone());
        if let Some(error) = &world.load_error {
            self.load_notice = Some(format!("Could not open the destination world: {error}"));
            return false;
        }
        let simulation = match load_simulation(seed, deep, &dir) {
            Ok(simulation) => simulation,
            Err(error) => {
                self.load_notice = Some(format!("Could not open the destination world: {error}"));
                return false;
            }
        };
        let signs = match dir.as_ref().map(|d| building::load_signs(&d.join("signs.txt"))).transpose() {
            Ok(signs) => signs.unwrap_or_default(),
            Err(error) => {
                self.load_notice = Some(format!("Could not open destination sign data: {error}"));
                return false;
            }
        };
        self.world = world;
        self.load_notice = None;
        self.pickups = dir.as_ref().map_or_else(Pickups::default, |d| Pickups::load(&d.join("drops.txt")));
        self.chests = dir.as_ref().map_or_else(Chests::default, |d| loot::load_chests(&d.join("chests.txt")));
        self.furnaces = dir.as_ref().map_or_else(Furnaces::default, |d| furnace::load(&d.join("furnaces.txt")));
        self.signs = signs;
        self.boats = dir.as_ref().map_or_else(boat::Boats::default, |d| boat::Boats::load(&d.join("boats.txt")));
        (self.mobs, self.fluids, self.settle) = simulation;
        self.mobs.ignore = self.mode == Mode::Creative;
        self.riding = None;
        self.trading = None;
        self.fishing = None;
        self.mining = None;
        self.arrows.clear();
        self.combat = combat::State::default();
        self.orbs = Orbs::default();
        self.particles = Particles::default();
        self.weather.dirty();
        self.ready = false;
        self.unstick = false;
        self.auto_cur = 20;
        self.apply_dist();
        true
    }

    pub(super) fn cross_gate(&mut self) {
        let deep = !self.world.is_deep();
        let p = self.player.pos.floor().as_ivec3();
        let target = if deep {
            IVec3::new(p.x.div_euclid(deep::SCALE), (p.y - LIFT).clamp(deep::LAVA_SEA + 6, 96), p.z.div_euclid(deep::SCALE))
        } else {
            IVec3::new(p.x * deep::SCALE, p.y + LIFT, p.z * deep::SCALE)
        };
        let reach = if deep { 16 } else { 128 };
        let known = self
            .gates
            .iter()
            .filter(|g| g.deep == deep && (g.at.x - target.x).abs() <= reach && (g.at.z - target.z).abs() <= reach)
            .min_by_key(|g| (g.at - target).length_squared())
            .copied();
        self.audio.play(Snd::Travel, 1.0, 1.0);
        if !self.switch_world(deep) {
            self.goto(Screen::Pause);
            return;
        }
        self.player.pos = target.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
        self.player.vel = Vec3::ZERO;
        self.arrival = Some((target, known));
        self.gate_lock = true;
        self.gate_t = 0.0;
        self.say(if deep { "The Emberdeep".into() } else { "Back in the world above".into() });
    }

    pub(super) fn arrive(&mut self) {
        let Some((target, known)) = self.arrival else { return };
        let r = 2 * CW;
        if [(-r, -r), (r, -r), (-r, r), (r, r), (0, 0)].iter().any(|d| !self.world.is_loaded(target.x + d.0, target.z + d.1)) {
            return;
        }
        self.arrival = None;
        // developer hook: a place to look from on the far side (for screenshots)
        if let Some(c) = std::env::var("CUBEWILD_FAR_AT")
            .ok()
            .map(|v| v.split(',').filter_map(|t| t.trim().parse::<f32>().ok()).collect::<Vec<_>>())
            .filter(|c| c.len() == 3)
        {
            self.player.pos = Vec3::new(c[0], c[1], c[2]);
            self.player.flying = true;
            self.ready = true;
            return;
        }
        if let Some(g) = known.filter(|g| is_gate(self.world.get_block(g.at.x, g.at.y, g.at.z))) {
            let side = if g.axis == 0 { Vec3::X } else { Vec3::Z };
            self.player.pos = g.at.as_vec3() + Vec3::new(0.5, 0.0, 0.5) + side * 0.5;
            self.ready = true;
            return;
        }
        // build a gate: on a floor near the target, with room in front and behind
        let (fx, fy, fz) = if self.world.is_deep() {
            deep::find_floor(self.world.seed, target.x, target.z).unwrap_or((target.x, target.y - 1, target.z))
        } else {
            let y = self.world.surface_y(target.x, target.z).unwrap_or(SEA);
            (target.x, y.clamp(4, CH - 8), target.z)
        };
        let axis = 0u8;
        let mut edits = Vec::new();
        for dz in -1..=1i32 {
            for dx in -1..=2i32 {
                for dy in 0..=4i32 {
                    let c = IVec3::new(fx + dx, fy + dy, fz + dz);
                    let frame = dz == 0 && (dy == 0 || dy == 4 || dx == -1 || dx == 2);
                    let veil = dz == 0 && !frame;
                    let b = if frame {
                        CINDERSTONE
                    } else if veil {
                        GATE + axis as Block
                    } else if dy == 0 {
                        // a ledge to step out onto, if there is no floor
                        let cur = self.world.get_block(c.x, c.y, c.z);
                        if is_solid(cur) { continue } else { CINDERSTONE }
                    } else {
                        AIR
                    };
                    edits.push((c, b));
                }
            }
        }
        self.world.set_many(&edits);
        let at = IVec3::new(fx, fy + 1, fz);
        self.gates.push(Gate { deep: self.world.is_deep(), at, axis });
        self.player.pos = at.as_vec3() + Vec3::new(1.0, 0.0, 0.5);
        if std::env::var("CUBEWILD_FAR_AT").as_deref() == Ok("gate") {
            // developer hook: step back from the new gate to look at it
            self.player.pos += Vec3::new(0.0, 0.6, -3.5);
            self.player.flying = true;
            self.gate_lock = true;
        }
        self.ready = true;
    }
}
