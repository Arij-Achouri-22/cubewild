//! Persistent gameplay bindings. Menu shortcuts remain available while rebinding.
use std::collections::HashSet;
use winit::keyboard::KeyCode;

pub const ACTIONS: [(&str, KeyCode); 11] = [
    ("Forward", KeyCode::KeyW),
    ("Back", KeyCode::KeyS),
    ("Left", KeyCode::KeyA),
    ("Right", KeyCode::KeyD),
    ("Jump", KeyCode::Space),
    ("Sprint", KeyCode::ControlLeft),
    ("Sneak", KeyCode::ShiftLeft),
    ("Inventory", KeyCode::KeyE),
    ("Drop", KeyCode::KeyQ),
    ("Fly", KeyCode::KeyF),
    ("Pick block", KeyCode::KeyP),
];

#[derive(Clone)]
pub struct Bindings(pub [KeyCode; 11]);
impl Default for Bindings {
    fn default() -> Self {
        Self(ACTIONS.map(|a| a.1))
    }
}
impl Bindings {
    pub fn pressed(&self, action: usize, held: &HashSet<KeyCode>) -> bool {
        held.contains(&self.0[action])
    }
    pub fn set(&mut self, action: usize, key: KeyCode) {
        if action >= self.0.len() {
            return;
        }
        if let Some(other) = self.0.iter().position(|&k| k == key) {
            self.0.swap(action, other);
        }
        self.0[action] = key;
    }
    pub fn load_line(&mut self, k: &str, v: &str) -> bool {
        let Some(index) = k.strip_prefix("bind_").and_then(|n| n.parse::<usize>().ok()).filter(|&i| i < 11) else {
            return false;
        };
        let Some(key) = parse(v) else {
            return false;
        };
        self.set(index, key);
        true
    }
    pub fn encode(&self) -> String {
        self.0.iter().enumerate().map(|(i, k)| format!("bind_{i}={k:?}\n")).collect()
    }
}
pub fn parse(s: &str) -> Option<KeyCode> {
    use KeyCode::*;
    let keys = [
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Space,
        ControlLeft,
        ControlRight,
        ShiftLeft,
        ShiftRight,
        AltLeft,
        AltRight,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Enter,
        Backspace,
        Backquote,
        Minus,
        Equal,
        BracketLeft,
        BracketRight,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
        Backslash,
        Home,
        End,
        PageUp,
        PageDown,
        Insert,
        Delete,
    ];
    keys.into_iter().find(|k| format!("{k:?}") == s)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Player(usize),
    Grid(usize),
    Chest(usize),
    Furnace(usize),
}
pub struct Drag {
    pub right: bool,
    pub slots: Vec<Slot>,
}

impl super::Game {
    pub(super) fn pick_block(&mut self) {
        use super::*;
        let Some((p, _, _)) = self.target() else {
            return;
        };
        let item = items::item_of(self.world.get_block(p.x, p.y, p.z));
        if !items::valid(item) {
            return;
        }
        if let Some(i) = (0..items::HOTBAR).find(|&i| self.inv.hot(i) == item && !self.inv.slots[i].is_empty()) {
            self.sel = i;
        } else if let Some(i) = (items::HOTBAR..items::BAG_END).find(|&i| self.inv.slots[i].item == item && !self.inv.slots[i].is_empty()) {
            self.inv.slots.swap(i, self.sel);
        } else if self.mode == Mode::Creative {
            self.inv.slots[self.sel] = Stack::new(item, items::max_stack(item));
        }
    }

    pub(super) fn finish_drag(&mut self) {
        use super::*;
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.slots.len() == 1 {
            self.apply(match drag.slots[0] {
                self::Slot::Player(i) => Action::Slot(i, drag.right),
                self::Slot::Grid(i) => Action::GridSlot(i, drag.right),
                self::Slot::Chest(i) => Action::ChestSlot(i, drag.right),
                self::Slot::Furnace(i) => Action::FurnaceSlot(i, drag.right),
            });
            return;
        }
        let item = self.inv.held.item;
        let mut selected = Vec::new();
        let mut stacks = Vec::new();
        for slot in drag.slots {
            let value = match slot {
                self::Slot::Player(i) if i < items::SLOTS && (i < items::ARMOR || items::armor(item).is_some_and(|a| a.0 == i - items::ARMOR)) => {
                    Some(self.inv.slots[i])
                }
                self::Slot::Grid(i) if i < 9 => Some(self.inv.grid[i]),
                self::Slot::Chest(i) => self.chest_stack(i),
                self::Slot::Furnace(i)
                    if (i == furnace::INPUT && furnace::smelt(item).is_some()) || (i == furnace::FUEL && furnace::fuel(item) > 0.0) =>
                {
                    self.furnaces.get(&self.chest_at).map(|f| f.slots[i])
                }
                _ => None,
            };
            if let Some(st) = value {
                selected.push(slot);
                stacks.push(st);
            }
        }
        let indices: Vec<usize> = (0..stacks.len()).collect();
        crafting::distribute(&mut self.inv.held, &mut stacks, &indices, drag.right);
        for (slot, st) in selected.into_iter().zip(stacks) {
            match slot {
                self::Slot::Player(i) => self.inv.slots[i] = st,
                self::Slot::Grid(i) => self.inv.grid[i] = st,
                self::Slot::Chest(i) => self.set_chest_stack(i, st),
                self::Slot::Furnace(i) => {
                    if let Some(f) = self.furnaces.get_mut(&self.chest_at) {
                        f.slots[i] = st;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bindings_swap_conflicts_and_round_trip() {
        let mut b = Bindings::default();
        b.set(0, KeyCode::KeyS);
        assert_eq!((b.0[0], b.0[1]), (KeyCode::KeyS, KeyCode::KeyW));
        let mut restored = Bindings::default();
        for line in b.encode().lines() {
            let (k, v) = line.split_once('=').unwrap();
            assert!(restored.load_line(k, v));
        }
        assert_eq!(restored.0, b.0);
    }
}

use super::*;

impl Game {
    pub(super) fn type_search(&mut self, code: KeyCode, text: Option<&str>) -> bool {
        match code {
            KeyCode::Escape if !self.search.is_empty() => self.search.clear(),
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter => self.search_on = false,
            KeyCode::Backspace => {
                self.search.pop();
            }
            // (Shift and Ctrl still reach the game, for shift-clicks)
            KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::ControlRight => return false,
            _ => {
                // (the font has the printable ASCII letters only)
                let t: String = text.unwrap_or("").chars().filter(|c| c.is_ascii_graphic() || *c == ' ').collect();
                if self.search.chars().count() + t.chars().count() <= 24 {
                    self.search += &t;
                }
            }
        }
        self.recipe_scroll = 0;
        true
    }

    pub(super) fn key_pressed(&mut self, mut code: KeyCode) {
        if let Some(index) = self.rebinding.take() {
            if code != KeyCode::Escape {
                if controls::parse(&format!("{code:?}")).is_some() {
                    self.settings.bindings.set(index, code);
                    self.settings.save();
                } else {
                    self.say("That key is reserved for menus".into());
                }
            }
            return;
        }
        if code == self.settings.bindings.0[10] && self.screen == Screen::Play {
            self.pick_block();
            return;
        }
        if matches!(self.screen, Screen::Play | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade) {
            if let Some(i) = self.settings.bindings.0.iter().position(|&k| k == code) {
                code = controls::ACTIONS[i].1;
            } else if [KeyCode::KeyE, KeyCode::KeyQ, KeyCode::KeyF, KeyCode::Space].contains(&code) {
                return;
            }
        }
        match code {
            KeyCode::Escape => match self.screen {
                Screen::Play => self.goto(Screen::Pause),
                Screen::Pause | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign => self.goto(Screen::Play),
                Screen::Dead | Screen::Title => {}
                _ => self.goto(Screen::Pause),
            },
            KeyCode::KeyE | KeyCode::Tab => match self.screen {
                Screen::Play => self.goto(Screen::Inventory),
                Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign => self.goto(Screen::Play),
                _ => {}
            },
            KeyCode::KeyQ if self.screen == Screen::Play => self.drop_selected(self.keys.contains(&KeyCode::ControlLeft)),
            KeyCode::KeyF if self.screen == Screen::Play => {
                if self.mode == Mode::Creative {
                    self.player.flying = !self.player.flying;
                } else {
                    self.say("Flying needs a creative world".into());
                }
            }
            KeyCode::Space if self.screen == Screen::Play && self.mode == Mode::Creative => {
                // Space twice in a row takes off and lands
                if self.last_jump < 0.3 {
                    self.player.flying = !self.player.flying;
                    self.last_jump = 1.0;
                } else {
                    self.last_jump = 0.0;
                }
            }
            KeyCode::F3 => self.debug = !self.debug,
            KeyCode::F11 => {
                self.apply(Action::ToggleFullscreen);
            }
            KeyCode::Digit1 => self.sel = 0,
            KeyCode::Digit2 => self.sel = 1,
            KeyCode::Digit3 => self.sel = 2,
            KeyCode::Digit4 => self.sel = 3,
            KeyCode::Digit5 => self.sel = 4,
            KeyCode::Digit6 => self.sel = 5,
            KeyCode::Digit7 => self.sel = 6,
            KeyCode::Digit8 => self.sel = 7,
            KeyCode::Digit9 => self.sel = 8,
            _ => {}
        }
    }

    pub(super) fn apply(&mut self, a: Action) -> bool {
        match a {
            Action::Goto(s) => self.goto(s),
            Action::Quit => return true,
            Action::ViewDist(d) => {
                self.settings.view_dist = d.clamp(4, 20);
                self.auto_cur = 20;
                self.apply_dist();
            }
            Action::Fov(f) => self.settings.fov = f,
            Action::Sens(s) => self.settings.sens = s,
            Action::ToggleVsync => {
                self.settings.vsync = !self.settings.vsync;
                self.gfx.set_vsync(self.settings.vsync);
            }
            Action::ToggleFullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
                self.window.set_fullscreen(if self.settings.fullscreen { Some(Fullscreen::Borderless(None)) } else { None });
            }
            Action::ToggleFps => self.settings.show_fps = !self.settings.show_fps,
            Action::ToggleBob => self.settings.bobbing = !self.settings.bobbing,
            Action::Scale(v) => {
                self.settings.render_scale = v;
                self.gfx.set_scale(v);
            }
            Action::ToggleAuto => {
                self.settings.auto_scale = !self.settings.auto_scale;
                self.auto_cur = 20;
                self.apply_dist();
            }
            Action::SaveSign => {
                if building::is_sign(self.world.get_block(self.chest_at.x, self.chest_at.y, self.chest_at.z)) {
                    self.signs.insert(self.chest_at, self.sign_edit.clone());
                }
                self.goto(Screen::Play);
            }
            Action::Bind(i) => self.rebinding = Some(i),
            Action::ResetBindings => {
                self.settings.bindings = controls::Bindings::default();
                self.settings.save();
            }
            Action::ToggleGrid => {
                self.grid_open = !self.grid_open;
                self.equipment_open = false;
            }
            Action::DragVisit(slot) => {
                if let Some(drag) = &mut self.drag {
                    if !drag.slots.contains(&slot) {
                        drag.slots.push(slot);
                    }
                }
            }
            Action::GridSlot(i, right) => {
                if let Some(drag) = &mut self.drag {
                    let slot = controls::Slot::Grid(i);
                    if !drag.slots.contains(&slot) {
                        drag.slots.push(slot);
                    }
                } else if self.shift {
                    self.inv.grid[i] = self.inv.pick_up(self.inv.grid[i]);
                } else if right {
                    items::right_click_stack(&mut self.inv.held, &mut self.inv.grid[i], true);
                } else {
                    items::click_stack(&mut self.inv.held, &mut self.inv.grid[i], true);
                }
            }
            Action::GridCraft(all) => {
                let bench = self.near_station(WORKBENCH);
                let size = if bench { 3 } else { 2 };
                let times = if all { 64 } else { 1 };
                for _ in 0..times {
                    if !crafting::craft(&mut self.inv, size, bench, all) {
                        break;
                    }
                }
            }
            Action::ToggleEquipment => {
                self.equipment_open = !self.equipment_open;
                self.grid_open = false;
            }
            Action::Repair => {
                if self.near_station(ANVIL) {
                    let fraction = self.xp as f32 / fx::xp_to_next(self.level) as f32;
                    let result = equipment::repair(&mut self.inv, &mut self.level, self.mode == Mode::Creative);
                    if result.is_ok() {
                        self.xp = (fraction * fx::xp_to_next(self.level) as f32) as u32;
                    }
                    self.say(result.err().unwrap_or("Equipment repaired").into());
                }
            }
            Action::Enchant(kind) => {
                if self.near_station(ENCHANTING_TABLE) {
                    let fraction = self.xp as f32 / fx::xp_to_next(self.level) as f32;
                    let result = equipment::enchant(&mut self.inv, &mut self.level, kind, self.mode == Mode::Creative);
                    if result.is_ok() {
                        self.xp = (fraction * fx::xp_to_next(self.level) as f32) as u32;
                    }
                    self.say(result.err().unwrap_or("Equipment enchanted").into());
                }
            }
            Action::CraftHand(all) => {
                let r = &RECIPES[self.recipe_sel.min(RECIPES.len() - 1)];
                if self.near_station(r.station) {
                    if all {
                        let mut made = 0;
                        while made < 64 && self.inv.craft(r) {
                            made += 1;
                        }
                    } else {
                        self.inv.craft_to_hand(r);
                    }
                }
            }
            Action::Search(on) => self.search_on = on,
            Action::ToggleCraftable => {
                self.recipe_craftable = !self.recipe_craftable;
                self.recipe_scroll = 0;
            }
            Action::Tab(c) => {
                self.recipe_tab = c;
                self.recipe_scroll = 0;
            }
            Action::PickRecipe(i) => self.recipe_sel = i,
            Action::Slot(i, right) => {
                if let Some(drag) = &mut self.drag {
                    let slot = controls::Slot::Player(i);
                    if !drag.slots.contains(&slot) {
                        drag.slots.push(slot);
                    }
                    return false;
                }
                if right {
                    self.inv.right_click(i);
                } else if !self.shift {
                    self.inv.click(i);
                } else if self.screen == Screen::Chest {
                    // shift-click sends the stack into the chest
                    self.inv.slots[i] = self.stash_chest(self.inv.slots[i]);
                } else if self.screen == Screen::Furnace {
                    // shift-click sends ore and food to be cooked, and fuel to the fire
                    let st = self.inv.slots[i];
                    let to = if furnace::smelt(st.item).is_some() {
                        Some(furnace::INPUT)
                    } else if furnace::fuel(st.item) > 0.0 {
                        Some(furnace::FUEL)
                    } else {
                        None
                    };
                    match (to, self.furnaces.get_mut(&self.chest_at)) {
                        (Some(k), Some(f)) => self.inv.slots[i] = items::stash_in(&mut f.slots[k..k + 1], st),
                        _ => self.inv.quick_move(i),
                    }
                } else {
                    self.inv.quick_move(i);
                }
            }
            Action::ChestSlot(i, right) => {
                if let Some(drag) = &mut self.drag {
                    let slot = controls::Slot::Chest(i);
                    if !drag.slots.contains(&slot) {
                        drag.slots.push(slot);
                    }
                    return false;
                }
                if let Some(mut st) = self.chest_stack(i) {
                    if right {
                        items::right_click_stack(&mut self.inv.held, &mut st, true);
                    } else if self.shift {
                        st = self.inv.pick_up(st);
                    } else {
                        items::click_stack(&mut self.inv.held, &mut st, true);
                    }
                    self.set_chest_stack(i, st);
                }
            }

            Action::FurnaceSlot(i, right) => {
                if let Some(drag) = &mut self.drag {
                    let slot = controls::Slot::Furnace(i);
                    if !drag.slots.contains(&slot) {
                        drag.slots.push(slot);
                    }
                    return false;
                }
                let mut xp = 0.0;
                if let Some(f) = self.furnaces.get_mut(&self.chest_at) {
                    let held = &mut self.inv.held;
                    if i == furnace::OUTPUT {
                        // the output can only be taken
                        let out = f.slots[i];
                        let before = out.n;
                        if self.shift {
                            f.slots[i] = self.inv.pick_up(out);
                        } else if held.is_empty() {
                            *held = out;
                            f.slots[i] = Stack::default();
                        } else if held.item == out.item {
                            let k = out.n.min(items::max_stack(out.item).saturating_sub(held.n));
                            held.n += k;
                            f.slots[i].n -= k;
                            if f.slots[i].n == 0 {
                                f.slots[i] = Stack::default();
                            }
                        }
                        if f.slots[i].n < before || (before > 0 && f.slots[i].is_empty()) {
                            xp = std::mem::take(&mut f.xp);
                        }
                    } else {
                        let accepts = if i == furnace::INPUT { furnace::smelt(held.item).is_some() } else { furnace::fuel(held.item) > 0.0 };
                        if right {
                            items::right_click_stack(held, &mut f.slots[i], accepts);
                        } else if self.shift {
                            f.slots[i] = self.inv.pick_up(f.slots[i]);
                        } else {
                            items::click_stack(held, &mut f.slots[i], accepts);
                        }
                    }
                }
                // (a fraction of a point is a chance of one)
                let whole = xp.floor() as u32 + (self.rng.f() < xp.fract()) as u32;
                if self.mode == Mode::Survival {
                    self.gain_xp(whole);
                }
            }
            Action::Trade(k) => self.trade(k),
            Action::Volume(v) => {
                self.settings.volume = v;
                self.audio.set_volume(v, self.settings.music);
            }
            Action::Music(v) => {
                self.settings.music = v;
                self.audio.set_volume(self.settings.volume, v);
            }
            Action::DropHeld => {
                let st = std::mem::take(&mut self.inv.held);
                self.throw(st);
            }
            Action::Sort => self.inv.sort_bag(),
            Action::Respawn => self.respawn(),
            Action::NewWorld(mode) => self.enter(fresh_seed(), mode),
            Action::Open(seed) => {
                if seed == self.world.seed && self.save_dir.is_some() && self.world.load_error.is_none() {
                    self.goto(Screen::Play);
                } else {
                    self.enter(seed, Mode::Survival);
                }
            }
            Action::Delete(seed) => {
                if self.confirm_delete == Some(seed) {
                    self.confirm_delete = None;
                    self.delete_world(seed);
                } else {
                    self.confirm_delete = Some(seed);
                }
            }
            Action::ToTitle => {
                if !self.save() {
                    return false;
                }
                self.worlds = list_worlds();
                self.confirm_delete = None;
                self.at_title = true;
                self.goto(Screen::Title);
            }
            Action::Take(item, to_pack) => {
                // a stack on the mouse is put away for good, as in a bin
                if !self.inv.held.is_empty() {
                    self.inv.held = Stack::default();
                } else if to_pack {
                    self.inv.add(item, items::max_stack(item));
                } else {
                    self.inv.held = Stack::new(item, items::max_stack(item));
                }
            }
        }
        false
    }
}

impl Settings {
    pub(super) fn save(&self) {
        let Some(p) = config_path() else { return };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let text = format!(
            "view_dist={}\nfov={}\nsens={}\nvsync={}\nfullscreen={}\nshow_fps={}\nrender_scale={}\nauto_scale={}\nvolume={}\nmusic={}\nbobbing={}\n",
            self.view_dist,
            self.fov,
            self.sens,
            self.vsync,
            self.fullscreen,
            self.show_fps,
            self.render_scale,
            self.auto_scale,
            self.volume,
            self.music,
            self.bobbing
        );
        if let Err(e) = std::fs::write(&p, text + &self.bindings.encode()) {
            eprintln!("cubewild: failed to save settings: {e}");
        }
    }
}
