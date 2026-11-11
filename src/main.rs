//! Cubewild — a small open-world voxel sandbox: gather, craft, build, hunt.

mod audio;
mod boat;
mod furnace;
mod fx;
mod gfx;
mod items;
mod loot;
mod mobs;
mod path;
mod player;
mod textures;
mod trade;
mod ui;
mod village;
mod world;
mod worldgen;

use gfx::{ChunkMesh, EntVert, Frame, Gfx, ItemVert};
use glam::{IVec3, Mat3, Mat4, Quat, Vec3, Vec4};
use items::{Cat, Inventory, Item, Stack, CATS, RECIPES};
use loot::{Chests, Pickups};
use mobs::{push_box, Mobs, Rng};
use player::{collides, raycast, raycast_with, Input, Player, PLAYER_H, PLAYER_HW};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use ui::{Font, Pointer, Ui, ACCENT, BODY, DIM, HEAD, SMALL, TEXT, TITLE};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};
use world::*;
use audio::{Audio, Snd};
use furnace::Furnaces;
use fx::{Orbs, Particles, Weather};

fn srgb(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| (v as f32 / 255.0).powf(2.2))
}

/// Right-handed perspective projection with 0..1 depth.
fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy * 0.5).tan();
    let r = far / (near - far);
    Mat4::from_cols(
        Vec4::new(f / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, f, 0.0, 0.0),
        Vec4::new(0.0, 0.0, r, -1.0),
        Vec4::new(0.0, 0.0, r * near, 0.0),
    )
}

/// Right-handed view matrix for a camera at the origin looking along `dir`.
fn look_to(dir: Vec3) -> Mat4 {
    let f = dir.normalize();
    let s = f.cross(Vec3::Y).normalize();
    let u = s.cross(f);
    Mat4::from_cols(
        Vec4::new(s.x, u.x, -f.x, 0.0),
        Vec4::new(s.y, u.y, -f.y, 0.0),
        Vec4::new(s.z, u.z, -f.z, 0.0),
        Vec4::W,
    )
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A camera-facing square in the sky pass.
fn push_sky_quad(out: &mut Vec<EntVert>, dir: Vec3, dist: f32, half: f32, color: [u8; 4]) {
    let r = if dir.y.abs() > 0.99 { Vec3::X } else { dir.cross(Vec3::Y).normalize() } * half;
    let u = r.cross(dir).normalize() * half;
    let c = dir * dist;
    let q = [c - r - u, c + r - u, c + r + u, c - r + u];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(EntVert { pos: q[i].into(), color });
    }
}

/// seconds for one full day and night
const DAY_SECS: f32 = 720.0;
const MAX_HEALTH: f32 = 20.0;
const MAX_FOOD: f32 = 20.0;
const MAX_AIR: f32 = 10.0;
/// recipe rows shown at once in the inventory
/// seconds one swing of the held item takes
const SWING: f32 = 0.28;
const RECIPE_COLS: usize = 5;
const RECIPE_ROWS: usize = 7;

// parchment palette of the inventory windows
const PAPER: [u8; 4] = [214, 204, 184, 255];
const PAPER_LIGHT: [u8; 4] = [246, 240, 226, 255];
const PAPER_DARK: [u8; 4] = [110, 98, 80, 255];
const INSET: [u8; 4] = [186, 175, 154, 255];
const INK: [u8; 4] = [50, 42, 34, 255];
const INK_DIM: [u8; 4] = [112, 100, 86, 255];
const INK_GOOD: [u8; 4] = [38, 122, 62, 255];
const INK_BAD: [u8; 4] = [176, 48, 40, 255];

/// An item icon with its count and, for worn tools, a durability bar.
fn draw_stack(ui: &mut Ui, x: f32, y: f32, size: f32, st: Stack) {
    if st.is_empty() {
        return;
    }
    let pad = (size * 0.16).round();
    ui.icon(x + pad, y + pad, size - 2.0 * pad, items::icon(st.item), 255);
    if st.n > 1 {
        let n = st.n.to_string();
        let f = ui.font;
        ui.label(x + size - f.width(SMALL, &n) - size * 0.1, y + size - f.height(SMALL) - size * 0.04, SMALL, &n, TEXT);
    }
    if let Some(c) = st.condition().filter(|_| st.wear > 0) {
        let (bx, by, bw, bh) = (x + pad, y + size - pad * 0.9, size - 2.0 * pad, (size * 0.06).round().max(2.0));
        ui.rect(bx, by, bw, bh, [10, 10, 12, 255]);
        let col = if c > 0.5 { [90, 210, 90, 255] } else if c > 0.2 { [236, 196, 60, 255] } else { [230, 70, 60, 255] };
        ui.rect(bx, by, (bw * c).max(1.0), bh, col);
    }
}

/// An inset inventory slot. `paper` slots sit in a window; the others float over the world.
fn draw_slot(ui: &mut Ui, x: f32, y: f32, size: f32, st: Stack, selected: bool, paper: bool) -> bool {
    let hot = paper && ui.hover(x, y, size, size);
    let (x0, y0, inner) = (x + 2.0, y + 2.0, size - 4.0);
    let (fill, dark, light) = if !paper {
        ([14, 16, 22, 190], [16, 18, 24, 255], [74, 84, 104, 255])
    } else if hot {
        ([190, 181, 162, 255], [92, 82, 66, 255], PAPER_LIGHT)
    } else {
        ([154, 142, 122, 255], [92, 82, 66, 255], PAPER_LIGHT)
    };
    ui.rect(x0, y0, inner, inner, fill);
    ui.rect(x0, y0, inner, 2.0, dark);
    ui.rect(x0, y0, 2.0, inner, dark);
    ui.rect(x0, y0 + inner - 2.0, inner, 2.0, light);
    ui.rect(x0 + inner - 2.0, y0, 2.0, inner, light);
    if selected {
        let c = if paper { INK } else { [255, 255, 255, 255] };
        ui.outline(x, y, size, size, (3.0 * ui.s).round().max(2.0), c);
    }
    draw_stack(ui, x, y, size, st);
    hot
}

/// A raised parchment window.
fn draw_window(ui: &mut Ui, x: f32, y: f32, w: f32, h: f32) {
    let t = (3.0 * ui.s).round().max(2.0);
    ui.rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, [30, 24, 18, 255]);
    ui.rect(x, y, w, h, PAPER);
    ui.rect(x, y, w, t, PAPER_LIGHT);
    ui.rect(x, y, t, h, PAPER_LIGHT);
    ui.rect(x, y + h - t, w, t, PAPER_DARK);
    ui.rect(x + w - t, y, t, h, PAPER_DARK);
}

/// Numbers worth knowing about an item, for its tooltip.
fn item_stats(st: Stack) -> String {
    let mut v: Vec<String> = Vec::new();
    let (dmg, _) = items::weapon(st.item);
    if dmg > 1 {
        v.push(format!("Damage {dmg}"));
    }
    if let Some((_, share)) = items::armor(st.item) {
        v.push(format!("Absorbs {:.0}% of damage", share * 100.0));
    }
    if items::food(st.item) > 0.0 {
        v.push(format!("Restores {} hunger", items::food(st.item)));
    }
    let max = items::max_wear(st.item);
    if max > 0 {
        v.push(format!("Durability {}/{}", max - st.wear.min(max), max));
    }
    v.join("    ")
}

/// The name, kind, description and numbers of the item under the mouse.
fn draw_tip(ui: &mut Ui, st: Stack, wf: f32, hf: f32) {
    let (font, s) = (ui.font, ui.s);
    let (px, py) = (ui.ptr.x, ui.ptr.y);
    let (name, kind, desc, stats) = (items::name(st.item), items::kind(st.item), items::desc(st.item), item_stats(st));
    let lines: Vec<(&str, usize, [u8; 4])> = [(name, BODY, ACCENT), (kind, SMALL, DIM), (desc, SMALL, TEXT), (stats.as_str(), SMALL, [150, 210, 255, 255])]
        .into_iter()
        .filter(|l| !l.0.is_empty())
        .collect();
    let tw = lines.iter().map(|l| font.width(l.1, l.0)).fold(0.0, f32::max) + 24.0 * s;
    let th = lines.iter().map(|l| font.height(l.1) + 3.0 * s).sum::<f32>() + 14.0 * s;
    let tx = (px + 18.0 * s).min(wf - tw - 4.0);
    let mut ty = (py + 20.0 * s).min(hf - th - 4.0);
    ui.rect(tx, ty, tw, th, [16, 14, 22, 246]);
    ui.outline(tx, ty, tw, th, 1.0, ACCENT);
    ty += 7.0 * s;
    for (text, style, col) in lines {
        ui.text(tx + 12.0 * s, ty, style, text, col);
        ty += font.height(style) + 3.0 * s;
    }
}

struct Arrow {
    pos: Vec3,
    vel: Vec3,
    life: f32,
    /// shot by a skeleton at the player
    hostile: bool,
}

// ---------------------------------------------------------------- settings

#[derive(Clone)]
struct Settings {
    view_dist: i32,
    fov: f32,
    sens: f32,
    vsync: bool,
    fullscreen: bool,
    show_fps: bool,
    /// share of the window resolution the world is drawn at
    render_scale: f32,
    /// shorten the view distance automatically to hold 120 fps
    auto_scale: bool,
    /// loudness of everything, and of the music on top of that (0..1)
    volume: f32,
    music: f32,
    /// the camera bobs with each step
    bobbing: bool,
}

/// Per-user folder for settings (`roaming`) or saves, following each platform's habit:
/// `%APPDATA%` / `%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on macOS,
/// the XDG folders elsewhere.
fn user_dir(config: bool) -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        if config { var("APPDATA") } else { var("LOCALAPPDATA").or_else(|| var("APPDATA")) }
            .or_else(|| var("USERPROFILE").map(|h| h.join("AppData").join(if config { "Roaming" } else { "Local" })))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if config {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    };
    Some(base?.join("cubewild"))
}

fn config_path() -> Option<PathBuf> {
    Some(user_dir(true)?.join("settings.txt"))
}

impl Settings {
    fn load() -> Settings {
        let mut s = Settings { view_dist: 14, fov: 72.0, sens: 1.0, vsync: true, fullscreen: false, show_fps: true, render_scale: 1.0, auto_scale: true, volume: 0.8, music: 0.5, bobbing: true };
        let text = config_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "view_dist" => s.view_dist = v.parse().unwrap_or(s.view_dist).clamp(4, 20),
                "fov" => s.fov = v.parse().unwrap_or(s.fov).clamp(55.0, 100.0),
                "sens" => s.sens = v.parse().unwrap_or(s.sens).clamp(0.2, 3.0),
                "vsync" => s.vsync = v == "true",
                "fullscreen" => s.fullscreen = v == "true",
                "show_fps" => s.show_fps = v == "true",
                "render_scale" => s.render_scale = v.parse().unwrap_or(1.0f32).clamp(0.5, 1.0),
                "auto_scale" => s.auto_scale = v == "true",
                "volume" => s.volume = v.parse().unwrap_or(s.volume).clamp(0.0, 1.0),
                "music" => s.music = v.parse().unwrap_or(s.music).clamp(0.0, 1.0),
                "bobbing" => s.bobbing = v == "true",
                _ => {}
            }
        }
        s
    }

    fn save(&self) {
        let Some(p) = config_path() else { return };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let text = format!(
            "view_dist={}\nfov={}\nsens={}\nvsync={}\nfullscreen={}\nshow_fps={}\nrender_scale={}\nauto_scale={}\nvolume={}\nmusic={}\nbobbing={}\n",
            self.view_dist, self.fov, self.sens, self.vsync, self.fullscreen, self.show_fps, self.render_scale, self.auto_scale, self.volume, self.music, self.bobbing
        );
        if let Err(e) = std::fs::write(&p, text) {
            eprintln!("cubewild: failed to save settings: {e}");
        }
    }
}

// ---------------------------------------------------------------- game

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    /// choose or start a world
    Title,
    Play,
    Pause,
    Settings,
    Controls,
    Inventory,
    /// the chest at `Game::chest_at` is open
    Chest,
    /// the furnace at `Game::chest_at` is open
    Furnace,
    /// trading with the villager `Game::trading`
    Trade,
    Dead,
}

enum Action {
    Goto(Screen),
    Quit,
    ViewDist(i32),
    Fov(f32),
    Sens(f32),
    ToggleVsync,
    ToggleFullscreen,
    ToggleFps,
    ToggleBob,
    Scale(f32),
    ToggleAuto,
    /// take the crafting result; true = craft as many as possible
    CraftHand(bool),
    ToggleCraftable,
    Tab(Cat),
    PickRecipe(usize),
    /// slot index, right button
    Slot(usize, bool),
    /// slot of the open chest, right button
    ChestSlot(usize, bool),
    /// slot of the open furnace, right button
    FurnaceSlot(usize, bool),
    /// make the villager's offer of this number
    Trade(usize),
    Volume(f32),
    Music(f32),
    /// throw the stack on the mouse out of the window
    DropHeld,
    Sort,
    Respawn,
    /// start a new world with a seed of its own
    NewWorld(Mode),
    /// play a saved world
    Open(u32),
    /// first click asks, the second deletes the saved world
    Delete(u32),
    /// save and go back to the title screen
    ToTitle,
    /// creative: take a full stack of an item (true = straight into the pack)
    Take(Item, bool),
}

struct Bench {
    duration: f32,
    t: f32,
    frames: Vec<f32>,
}

struct Game {
    window: Arc<Window>,
    gfx: Gfx,
    font: Rc<Font>,
    world: World,
    player: Player,
    mobs: Mobs,
    mode: Mode,
    /// saved worlds, for the title screen
    worlds: Vec<WorldInfo>,
    /// in the menus of the title screen, with no world entered yet
    at_title: bool,
    /// the world whose delete button was clicked once
    confirm_delete: Option<u32>,
    /// seconds left of the held item's swing
    swing: f32,
    /// the thick model of the held item's icon, rebuilt when the item changes
    hand_model: (Item, Vec<HandQuad>),
    /// seconds until lava burns the player again
    lava_t: f32,
    /// seconds since Space was last pressed (twice quickly toggles flight in creative)
    last_jump: f32,
    /// blocks broken just now drop nothing (creative)
    quiet: bool,
    inv: Inventory,
    settings: Settings,
    screen: Screen,
    started: bool,
    keys: HashSet<KeyCode>,
    buttons: [bool; 2],
    ptr: Pointer,
    sel: usize,
    mining: Option<(IVec3, f32)>,
    primary_cd: f32,
    place_cd: f32,
    hit_flash: f32,
    toast: (String, f32),
    rng: Rng,
    last: Instant,
    fps: f32,
    fps_acc: (f32, u32),
    worst_ms: f32,
    worst_acc: f32,
    debug: bool,
    ready: bool,
    drawn: (usize, u32),
    save_dir: Option<PathBuf>,
    bench: Option<Bench>,
    ents: Vec<EntVert>,
    sky: Vec<EntVert>,
    /// star directions and sizes
    stars: Vec<(Vec3, f32)>,
    health: f32,
    food: f32,
    air: f32,
    hurt_flash: f32,
    /// time of day, 0..1 (0 = sunrise, 0.25 = noon)
    time: f32,
    spawn: Vec3,
    /// after a respawn: wait for the chunk, then lift the player out of any blocks
    unstick: bool,
    recipe_scroll: usize,
    recipe_tab: Cat,
    recipe_sel: usize,
    recipe_craftable: bool,
    shift: bool,
    arrows: Vec<Arrow>,
    /// items lying in the world
    pickups: Pickups,
    chests: Chests,
    /// the chest whose window is open
    chest_at: IVec3,
    item_verts: Vec<ItemVert>,
    got: Vec<Stack>,
    /// what was picked up lately, shown as one running line, and seconds until it resets
    tally: (Vec<Stack>, f32),
    /// CUBEWILD_TEST_MOBS: line up one of each monster once the world is ready
    test_mobs: bool,
    /// CUBEWILD_TEST_HOUSE: build a hut with doors and windows; true = start inside it
    test_house: Option<bool>,
    /// water block being fished, seconds waited, seconds until the bite
    fishing: Option<(IVec3, f32, f32)>,
    grow_acc: f32,
    /// automatic view distance: current limit in chunks, timers, back-off, last change was a raise
    auto_cur: i32,
    auto_t: f32,
    auto_hold: f32,
    auto_backoff: f32,
    auto_raised: bool,
    audio: Audio,
    particles: Particles,
    weather: Weather,
    orbs: Orbs,
    furnaces: Furnaces,
    /// experience: whole levels, and the points towards the next
    level: u32,
    xp: u32,
    /// distance walked since the last footstep, and seconds to the next swim stroke
    step_acc: f32,
    swim_acc: f32,
    /// seconds to the next digging sound
    dig_acc: f32,
    was_water: bool,
    /// 0..1 how far the sprint has widened the view
    sprint_fov: f32,
    /// average colour of each block's texture, filled in as needed (alpha 0 = not yet)
    block_colors: Vec<[u8; 4]>,
    /// seconds to the next sound from an animal or monster nearby, and to the next puff
    /// of smoke from torches and furnaces
    ambient_t: f32,
    smoke_t: f32,
    landings: Vec<Vec3>,
    /// the biome the player stands in is looked up once a second
    biome_t: f32,
    /// 1 when the held item has just changed, falling to 0 as it comes up into view
    equip: f32,
    /// walking sway: phase (0..2, a step per unit) and strength
    bob: (f32, f32),
    /// the boat being ridden
    riding: Option<usize>,
    boats: boat::Boats,
    /// seconds to the next paddle stroke sound
    paddle_t: f32,
    /// the villager being traded with (their village square and home)
    trading: Option<((i32, i32), usize)>,
    /// offers used up today: (villager, offer) -> times made
    trade_uses: std::collections::HashMap<(((i32, i32), usize), usize), u32>,
}

fn data_dir(seed: u32) -> Option<PathBuf> {
    let dir = user_dir(false)?.join(format!("world-{seed}"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cubewild: cannot create {}: {e} - the world will not be saved", dir.display());
        return None;
    }
    Some(dir)
}

fn find_spawn(ver: u8, seed: u32) -> Vec3 {
    let (x, h, z) = spawn_for(ver, seed);
    Vec3::new(x as f32 + 0.5, h as f32 + 2.0, z as f32 + 0.5)
}

/// Survival, or building freely: no damage or hunger, flight, endless blocks.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Survival,
    Creative,
}

/// A saved world, as the title screen lists it.
struct WorldInfo {
    seed: u32,
    mode: Mode,
    when: SystemTime,
}

/// The worlds that have been played, the latest first.
fn list_worlds() -> Vec<WorldInfo> {
    let mut v = Vec::new();
    let entries = user_dir(false).and_then(|d| std::fs::read_dir(d).ok());
    for e in entries.into_iter().flatten().flatten() {
        let name = e.file_name();
        let Some(seed) = name.to_str().and_then(|n| n.strip_prefix("world-")).and_then(|n| n.parse::<u32>().ok()) else { continue };
        let file = e.path().join("player.txt");
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let mode = if text.lines().any(|l| l.trim() == "mode creative") { Mode::Creative } else { Mode::Survival };
        let when = std::fs::metadata(&file).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        v.push(WorldInfo { seed, mode, when });
    }
    v.sort_by(|a, b| b.when.cmp(&a.when));
    v
}

/// A seed no saved world uses yet, different on every call.
fn fresh_seed() -> u32 {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0x1234_5678, |d| d.as_nanos() as u64);
    let mut r = Rng((t ^ (std::process::id() as u64) << 40) | 1);
    for _ in 0..8 {
        r.next();
    }
    loop {
        let seed = r.next() % 1_000_000_000;
        let taken = user_dir(false).is_some_and(|d| d.join(format!("world-{seed}")).exists());
        if seed != 0 && !taken {
            return seed;
        }
    }
}

/// Everything that belongs to one world and its player.
struct Loaded {
    world: World,
    player: Player,
    inv: Inventory,
    sel: usize,
    health: f32,
    food: f32,
    air: f32,
    time: f32,
    spawn: Vec3,
    pickups: Pickups,
    chests: Chests,
    furnaces: Furnaces,
    mode: Mode,
    save_dir: Option<PathBuf>,
    level: u32,
    xp: u32,
    weather: Weather,
    boats: boat::Boats,
}

/// Opens the world of a seed: from its save if there is one (which also decides the
/// mode), otherwise new in `mode`. With `save` off nothing is read or written.
fn load_world(seed: u32, view_dist: i32, save: bool, mode: Mode) -> Loaded {
    let save_dir = if save { data_dir(seed) } else { None };
    let saved = save_dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("player.txt")).ok());
    // a world keeps the terrain generator it was made with: saves from before there
    // were two do not name one, and are of the first
    let ver = match &saved {
        Some(text) => text.lines().find_map(|l| l.strip_prefix("gen ")).and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(1).clamp(1, GEN_LATEST),
        None => std::env::var("CUBEWILD_GEN").ok().and_then(|v| v.parse::<u8>().ok()).unwrap_or(GEN_LATEST).clamp(1, GEN_LATEST),
    };
    let world = World::new(seed, ver, view_dist, save_dir.clone());
    let mut spawn = find_spawn(ver, seed);
    let mut player = Player::new(spawn);
    let mut inv = Inventory::new();
    let mut mode = mode;
    let mut sel = 0;
    let (mut health, mut food, mut air, mut time) = (MAX_HEALTH, MAX_FOOD, MAX_AIR, 0.04f32);
    let (mut level, mut xp) = (0u32, 0u32);
    let mut weather = Weather::default();
    if let Some(text) = saved {
        for (n, line) in text.lines().enumerate() {
            let mut parts = line.split_whitespace();
            if n == 0 {
                let v: Vec<f32> = parts.filter_map(|t| t.parse().ok()).collect();
                if v.len() >= 5 {
                    player.pos = Vec3::new(v[0], v[1], v[2]);
                    player.yaw = v[3];
                    player.pitch = v[4];
                }
                if v.len() >= 6 {
                    inv.add(items::MEAT, v[5] as u32); // save from before inventories existed
                }
                continue;
            }
            match parts.next() {
                Some("inv") => {
                    // save from before the slot inventory
                    for (id, c) in parts.filter_map(|t| t.split_once(':')) {
                        if let (Ok(id), Ok(c)) = (id.parse::<u8>(), c.parse::<u32>()) {
                            inv.add(id, c);
                        }
                    }
                }
                Some("hotbar") => {
                    if let Some(v) = parts.nth(9).and_then(|t| t.parse::<usize>().ok()) {
                        sel = v.min(8);
                    }
                }
                Some("slots") => {
                    for t in parts {
                        let v: Vec<u32> = t.split(':').filter_map(|x| x.parse().ok()).collect();
                        if v.len() >= 3 && (v[0] as usize) < items::SLOTS && v[1] < 256 && items::valid(v[1] as u8) && v[2] > 0 {
                            let item = v[1] as u8;
                            let slot = v[0] as usize;
                            let wear = v.get(3).copied().unwrap_or(0).min(items::max_wear(item).saturating_sub(1) as u32) as u16;
                            let st = Stack { item, n: v[2].min(items::max_stack(item)), wear };
                            // armour saved before there were four slots goes back in the bag
                            if slot < items::ARMOR || items::armor(item).is_some_and(|a| a.0 == slot - items::ARMOR) {
                                inv.slots[slot] = st;
                            } else {
                                inv.add(item, st.n);
                            }
                        }
                    }
                }
                Some("sel") => {
                    if let Some(v) = parts.next().and_then(|t| t.parse::<usize>().ok()) {
                        sel = v.min(8);
                    }
                }
                Some("stats") => {
                    let v: Vec<f32> = parts.filter_map(|t| t.parse().ok()).collect();
                    if v.len() >= 4 {
                        health = v[0].clamp(1.0, MAX_HEALTH);
                        food = v[1].clamp(0.0, MAX_FOOD);
                        air = v[2].clamp(0.0, MAX_AIR);
                        time = v[3].rem_euclid(1.0);
                    }
                }
                Some("mode") => {
                    mode = if parts.next() == Some("creative") { Mode::Creative } else { Mode::Survival };
                }
                Some("xp") => {
                    let v: Vec<u32> = parts.filter_map(|t| t.parse().ok()).collect();
                    if v.len() >= 2 {
                        (level, xp) = (v[0].min(10_000), v[1].min(fx::xp_to_next(v[0].min(10_000))));
                    }
                }
                Some("weather") => {
                    let v: Vec<f32> = parts.filter_map(|t| t.parse().ok()).collect();
                    if v.len() >= 3 {
                        weather.want = v[0].clamp(0.0, 1.0);
                        weather.rain = weather.want;
                        weather.timer = v[1].clamp(1.0, 3000.0);
                        weather.storm = v[2] > 0.5;
                    }
                }
                Some("spawn") => {
                    let v: Vec<f32> = parts.filter_map(|t| t.parse().ok()).collect();
                    if v.len() >= 3 {
                        spawn = Vec3::new(v[0], v[1], v[2]);
                    }
                }
                _ => {}
            }
        }
    }
    let pickups = save_dir.as_ref().map_or_else(Pickups::default, |d| Pickups::load(&d.join("drops.txt")));
    let chests = save_dir.as_ref().map_or_else(Chests::default, |d| loot::load_chests(&d.join("chests.txt")));
    let furnaces = save_dir.as_ref().map_or_else(Furnaces::default, |d| furnace::load(&d.join("furnaces.txt")));
    let boats = save_dir.as_ref().map_or_else(boat::Boats::default, |d| boat::Boats::load(&d.join("boats.txt")));
    Loaded { world, player, inv, sel, health, food, air, time, spawn, pickups, chests, furnaces, mode, save_dir, level, xp, weather, boats }
}

/// One face of a held item's model: corners in the model's own space, texture
/// coordinates, texture layer and a fixed shade.
struct HandQuad {
    p: [Vec3; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    light: f32,
}

/// uv runs just inside the texture so the edge never wraps round
const QUAD_UV: [[f32; 2]; 4] = [[0.01, 0.99], [0.99, 0.99], [0.99, 0.01], [0.01, 0.01]];
/// the faces of the unit cube, in the order of `tex_for`
const CUBE_FACES: [[(f32, f32, f32); 4]; 6] = [
    [(1.0, 0.0, 0.0), (1.0, 0.0, 1.0), (1.0, 1.0, 1.0), (1.0, 1.0, 0.0)],
    [(0.0, 0.0, 1.0), (0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 1.0, 1.0)],
    [(0.0, 1.0, 1.0), (1.0, 1.0, 1.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)],
    [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 0.0, 1.0), (0.0, 0.0, 1.0)],
    [(1.0, 0.0, 1.0), (0.0, 0.0, 1.0), (0.0, 1.0, 1.0), (1.0, 1.0, 1.0)],
    [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)],
];
const FACE_SHADE: [f32; 6] = [0.80, 0.80, 1.0, 0.50, 0.66, 0.66];

/// True for items held as a small block rather than as their icon.
fn held_as_cube(item: Item) -> bool {
    let b = items::block_of(item);
    items::is_block(item) && !is_plant(b) && !is_shaped(b) && b != TORCH && b != GLASS
}

/// An item's icon given thickness, a sixteenth of its width: the picture on both sides,
/// and a rim round every pixel that borders an empty one.
fn icon_model(item: Item) -> Vec<HandQuad> {
    let layer = items::icon(item);
    let n = textures::TEX as i32;
    let solid = |x: i32, y: i32| (0..n).contains(&x) && (0..n).contains(&y) && textures::texel(layer as usize, x, y)[3] >= 128;
    let t = 0.5 / n as f32;
    let mut v = Vec::new();
    for (z, light) in [(t, 1.0), (-t, 0.8)] {
        let p = [Vec3::new(0.0, 0.0, z), Vec3::new(1.0, 0.0, z), Vec3::new(1.0, 1.0, z), Vec3::new(0.0, 1.0, z)];
        v.push(HandQuad { p, uv: QUAD_UV, layer, light });
    }
    for py in 0..n {
        for px in 0..n {
            if !solid(px, py) {
                continue;
            }
            let f = |i: i32| i as f32 / n as f32;
            // (the picture's first row is its top)
            let (x0, x1, y0, y1) = (f(px), f(px + 1), 1.0 - f(py + 1), 1.0 - f(py));
            let uv = [[(px as f32 + 0.5) / n as f32, (py as f32 + 0.5) / n as f32]; 4];
            let mut rim = |p: [(f32, f32, f32); 4], light: f32| {
                v.push(HandQuad { p: p.map(|(x, y, z)| Vec3::new(x, y, z)), uv, layer, light });
            };
            if !solid(px - 1, py) {
                rim([(x0, y0, -t), (x0, y0, t), (x0, y1, t), (x0, y1, -t)], 0.7);
            }
            if !solid(px + 1, py) {
                rim([(x1, y0, -t), (x1, y0, t), (x1, y1, t), (x1, y1, -t)], 0.7);
            }
            if !solid(px, py - 1) {
                rim([(x0, y1, -t), (x1, y1, -t), (x1, y1, t), (x0, y1, t)], 0.9);
            }
            if !solid(px, py + 1) {
                rim([(x0, y0, -t), (x1, y0, -t), (x1, y0, t), (x0, y0, t)], 0.55);
            }
        }
    }
    v
}

/// The player's arms at the bottom of the view and the selected item in the right fist:
/// a tool stands up out of the fist by its handle, other things are held upright by
/// their lower edge, a block rests on the fist. `swing` runs from 1 to 0 through a
/// stroke; `model` is `icon_model` of the item (unused for blocks).
/// How the first-person hand moves this frame: the swing (0..1 through a stroke), the
/// dip of an item being taken out (1 = just changed, 0 = settled), and the sway of
/// walking (phase, 0..1 strength).
#[derive(Clone, Copy, Default)]
struct HandPose {
    swing: f32,
    equip: f32,
    bob: f32,
    bob_amp: f32,
}

/// Where a held item's own x and y axes point in view space: its diagonal (handle to tip
/// of a tool icon) along `d`, its face turned towards `n`.
fn item_basis(d: Vec3, n: Vec3) -> (Vec3, Vec3, Vec3) {
    let d = d.normalize();
    let n = (n - d * n.dot(d)).normalize();
    let e = d.cross(n);
    let s = std::f32::consts::FRAC_1_SQRT_2;
    ((d + e) * s, (d - e) * s, n)
}

/// The first-person view of the right hand. With nothing held it is the bare arm,
/// reaching up from the lower right of the screen; anything held is shown on its own
/// there instead (a tool or weapon diagonal, its head up and inwards; a block as a
/// small cube turned to show three faces). `cam` is the camera's bobbing offset, which
/// the hand follows.
fn push_hand(out: &mut Vec<ItemVert>, dir: Vec3, item: Item, model: &[HandQuad], pose: HandPose, sky: f32, cam: Vec3) {
    use std::f32::consts::PI;
    // the view's own axes: x to the right, y up, z back towards the viewer
    let right = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let view = Mat3::from_cols(right, right.cross(dir), -dir);
    let put = |p: Vec3| view * p + cam;
    let mut cube = |m: Mat4, layer: &dyn Fn(usize) -> u32| {
        for (f, q) in CUBE_FACES.iter().enumerate() {
            for (i, &(x, y, z)) in q.iter().enumerate() {
                let pos = put(m.transform_point3(Vec3::new(x, y, z)));
                out.push(ItemVert { pos: pos.into(), uv: QUAD_UV[i], layer: layer(f), light: FACE_SHADE[f], sky });
            }
        }
    };
    // walking sways the hand in a small arc; taking out an item raises it from below
    let sway = Vec3::new((pose.bob * PI).sin() * 0.022, -(pose.bob * PI).sin().abs() * 0.026, 0.0) * pose.bob_amp;
    let rest = Vec3::new(0.0, -0.55 * pose.equip * pose.equip, 0.0) + sway;
    // a stroke: forward, down and in towards the middle of the screen, then back
    let t = pose.swing.clamp(0.0, 1.0);
    let k = (t * PI).sin();
    let k2 = (t.sqrt() * PI).sin();
    let reach = Vec3::new(-0.24 * k2, 0.1 * (t.sqrt() * PI * 2.0).sin() - 0.06 * k, -0.14 * k);
    let stroke = |pivot: Vec3| {
        Mat4::from_translation(pivot + rest + reach) * Mat4::from_rotation_y(0.55 * k2) * Mat4::from_rotation_x(-0.9 * k) * Mat4::from_rotation_z(0.25 * k) * Mat4::from_translation(-pivot)
    };
    if item == AIR {
        // the bare arm: thick and square, its fist low on the right
        let (shoulder, fist) = (Vec3::new(0.98, -1.2, -0.42), Vec3::new(0.5, -0.4, -1.0));
        let m = stroke(fist);
        let w = fist - shoulder;
        // turned so its top and inner side face the eye
        let u0 = w.cross(Vec3::new(-0.5, 0.35, 1.0)).normalize();
        let v0 = u0.cross(w).normalize();
        // (a quarter turn about its length, so two faces show and the arm reads as a block)
        let (u, v) = ((u0 + v0) * std::f32::consts::FRAC_1_SQRT_2, (v0 - u0) * std::f32::consts::FRAC_1_SQRT_2);
        let r = 0.12;
        for (from, to, layer) in [(0.0, 0.5, textures::ARM_TEX as u32 + 1), (0.5, 1.0, textures::ARM_TEX as u32)] {
            let o = shoulder + w * from - (u + v) * r;
            let part = Mat4::from_cols((u * 2.0 * r).extend(0.0), (w * (to - from)).extend(0.0), (v * 2.0 * r).extend(0.0), o.extend(1.0));
            cube(m * part, &|_| layer);
        }
        return;
    }
    if held_as_cube(item) {
        // a small block, turned so its top and two sides show
        let at = Vec3::new(0.6, -0.5, -1.05);
        let m = stroke(at) * Mat4::from_translation(at) * Mat4::from_rotation_x(0.5) * Mat4::from_rotation_y(0.78) * Mat4::from_scale(Vec3::splat(0.3)) * Mat4::from_translation(Vec3::splat(-0.5));
        cube(m, &|f| tex_for(items::block_of(item), f));
        return;
    }
    // icons: tools and weapons are drawn handle at the lower left, head at the upper
    // right; they are held by the handle with the head up, leaning in and away. Other
    // things are held upright by the middle of their base, turned a little.
    let tool = items::max_wear(item) > 0 && items::armor(item).is_none();
    let (grip, size, at, d, n) = if tool {
        (Vec3::new(0.22, 0.22, 0.0), 0.6, Vec3::new(0.62, -0.6, -1.0), Vec3::new(-0.22, 0.9, -0.38), Vec3::new(-0.8, 0.1, 0.6))
    } else {
        (Vec3::new(0.5, 0.0, 0.0), 0.42, Vec3::new(0.6, -0.56, -1.0), Vec3::new(0.5, 0.82, -0.25), Vec3::new(-0.4, 0.1, 0.9))
    };
    let (ax, ay, az) = item_basis(d, n);
    let place = Mat4::from_cols((ax * size).extend(0.0), (ay * size).extend(0.0), (az * size).extend(0.0), at.extend(1.0));
    let m = stroke(at) * place * Mat4::from_translation(-grip);
    for q in model {
        for i in 0..4 {
            out.push(ItemVert { pos: put(m.transform_point3(q.p[i])).into(), uv: q.uv[i], layer: q.layer, light: q.light, sky });
        }
    }
}

fn ui_scale(h: u32) -> f32 {
    (h as f32 / 1080.0).clamp(0.6, 2.0)
}

impl Game {
    fn new(window: Arc<Window>) -> Game {
        let env = |k: &str| std::env::var(k).ok();
        let bench = env("CUBEWILD_BENCH").and_then(|v| v.parse::<f32>().ok()).map(|d| Bench {
            duration: d,
            t: 0.0,
            frames: Vec::new(),
        });
        let mut settings = Settings::load();
        if let Some(d) = env("CUBEWILD_DIST").and_then(|v| v.parse().ok()) {
            settings.view_dist = d;
        }
        // a seed given by hand goes straight to that world; otherwise the title screen
        // opens over the world played last, or over a new one that is not saved yet
        let env_seed: Option<u32> = env("CUBEWILD_SEED").and_then(|v| v.parse().ok());
        let env_mode = env("CUBEWILD_MODE").map(|v| if v == "creative" { Mode::Creative } else { Mode::Survival });
        let worlds = list_worlds();
        let titled = env_seed.is_none() && bench.is_none();
        let (seed, save) = match (env_seed, worlds.first()) {
            (Some(s), _) => (s, bench.is_none()),
            (None, _) if bench.is_some() => (2026, false),
            (None, Some(w)) => (w.seed, true),
            (None, None) => (fresh_seed(), false),
        };

        if settings.fullscreen || std::env::args().any(|a| a == "--fullscreen") {
            settings.fullscreen = true;
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let mut gfx = pollster::block_on(Gfx::new(window.clone(), bench.is_none() && settings.vsync));
        gfx.set_scale(env("CUBEWILD_SCALE").and_then(|v| v.parse().ok()).unwrap_or(settings.render_scale));
        println!("cubewild: rendering on {}", gfx.adapter_name);
        let (font, atlas) = Font::bake(ui_scale(gfx.size().1));
        gfx.upload_font(&atlas);

        let Loaded { world, mut player, mut inv, sel, health, food, air, mut time, spawn, pickups, chests, furnaces, mut mode, save_dir, level, xp, mut weather, boats } = load_world(seed, settings.view_dist, save, Mode::Survival);
        // CUBEWILD_RAIN=rain|storm|clear sets the weather (for testing)
        match env("CUBEWILD_RAIN").as_deref() {
            Some("clear") => (weather.want, weather.rain, weather.storm) = (0.0, 0.0, false),
            Some(w) => (weather.want, weather.rain, weather.storm, weather.timer) = (1.0, 1.0, w == "storm", 3000.0),
            None => {}
        }
        let audio = Audio::new(if bench.is_some() { 0.0 } else { settings.volume }, settings.music);
        if let Some(m) = env_mode {
            mode = m;
        }
        if let Some(t) = env("CUBEWILD_TIME").and_then(|v| v.parse::<f32>().ok()) {
            time = t.rem_euclid(1.0);
        }
        if bench.is_some() {
            player.flying = true;
            player.pitch = -0.25;
        }
        // developer hooks for checking the menus without playing
        if env("CUBEWILD_TEST_ITEMS").is_some() {
            for (i, n) in [(CHEST, 2), (items::LADDER, 8), (items::TRAPDOOR, 4), (items::STAIRS + 2, 12), (SLAB, 12), (FENCE, 12), (TORCH, 16), (items::OAK_DOOR, 6), (items::BIRCH_DOOR, 3), (PANE, 32), (LOG, 12), (PLANKS, 7), (COBBLE, 5), (DIRT, 31), (SAND, 1), (items::STICK, 3), (items::MEAT, 4), (items::HIDE, 3), (items::STONE_PICK, 1), (items::SPEAR, 1), (LEAVES, 9), (items::STONE_AXE, 1), (items::WOOD_PICK, 1), (GLASS, 2), (STONE_BRICK, 8), (LEATHER, 1), (BRICK, 4), (items::COAL, 9), (IRON_ORE, 6), (items::IRON_INGOT, 5), (items::CRYSTAL, 4), (items::COOKED_MEAT, 3), (WORKBENCH, 1), (FURNACE, 1), (BED, 1), (LAMP, 2), (items::IRON_PICK, 1), (items::IRON_SWORD, 1), (CACTUS, 2), (SANDSTONE, 6), (BIRCH_LOG, 8), (GRAVEL, 7), (CLAY, 6), (GOLD_ORE, 4), (items::FIBER, 12), (items::BERRIES, 5), (MUSHROOM, 4), (FLOWER_RED, 3), (items::FLINT, 2), (items::LEATHER_CHEST, 1), (items::IRON_CHEST, 1), (items::IRON_HELMET, 1), (items::GOLD_LEGS, 1), (items::CRYSTAL_BOOTS, 1), (items::BOW, 1), (items::ARROW, 24), (items::FISHING_ROD, 1), (items::BUCKET, 1), (items::IRON_HOE, 1), (items::SEEDS, 9), (items::IRON_SHOVEL, 1), (items::SHEARS, 1), (items::COMPASS, 1), (items::CLOCK, 1), (items::WHEAT, 6), (items::CRYSTAL_SWORD, 1), (items::GOLD_INGOT, 8), (items::STEW, 1), (THATCH, 6)] {
                inv.add(i, n);
            }
        }
        if let Some(v) = env("CUBEWILD_AT") {
            let c: Vec<f32> = v.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            if c.len() == 3 {
                player.pos = Vec3::new(c[0], c[1], c[2]);
            }
            if let Some(a) = env("CUBEWILD_LOOK").and_then(|v| v.split_once(',').map(|(y, p)| (y.parse::<f32>(), p.parse::<f32>()))) {
                (player.yaw, player.pitch) = (a.0.unwrap_or(0.0), a.1.unwrap_or(0.0));
            }
        }
        if let Some(item) = env("CUBEWILD_HOLD").and_then(|v| v.parse::<u8>().ok()).filter(|&i| items::valid(i)) {
            inv.slots[sel] = Stack::new(item, 1);
        }
        let first_screen = match env("CUBEWILD_SCREEN").as_deref() {
            Some("settings") => Screen::Settings,
            Some("controls") => Screen::Controls,
            Some("inventory") => Screen::Inventory,
            Some("play" | "chest" | "furnace") => Screen::Play,
            _ if bench.is_some() => Screen::Play,
            _ if titled => Screen::Title,
            _ => Screen::Pause,
        };
        let mut mobs = Mobs::new(seed as u64 * 7919 + 17);
        mobs.ignore = mode == Mode::Creative;
        if let Some(d) = &save_dir {
            mobs.load(&d.join("animals.txt"));
        }
        let mut game = Game {
            window,
            gfx,
            font: Rc::new(font),
            world,
            player,
            mobs,
            mode,
            worlds,
            at_title: first_screen == Screen::Title,
            confirm_delete: None,
            swing: 0.0,
            hand_model: (AIR, Vec::new()),
            lava_t: 0.0,
            last_jump: -1.0,
            quiet: false,
            inv,
            settings,
            screen: first_screen,
            started: false,
            keys: HashSet::new(),
            buttons: [false; 2],
            ptr: Pointer::default(),
            sel,
            mining: None,
            primary_cd: 0.0,
            place_cd: 0.0,
            hit_flash: 0.0,
            toast: (String::new(), 0.0),
            rng: Rng(0x9e37_79b9_7f4a_7c15),
            last: Instant::now(),
            fps: 0.0,
            fps_acc: (0.0, 0),
            worst_ms: 0.0,
            worst_acc: 0.0,
            debug: false,
            ready: false,
            drawn: (0, 0),
            save_dir,
            bench,
            ents: Vec::new(),
            sky: Vec::new(),
            stars: {
                let mut r = Rng(0x5eed_1234_abcd_ef01);
                (0..260)
                    .map(|_| {
                        let (a, y) = (r.range(0.0, std::f32::consts::TAU), r.range(-1.0, 1.0));
                        let k = (1.0 - y * y).sqrt();
                        (Vec3::new(a.cos() * k, y, a.sin() * k), r.range(0.1, 0.26))
                    })
                    .collect()
            },
            health,
            food,
            air,
            hurt_flash: 0.0,
            time,
            spawn,
            unstick: true,
            recipe_scroll: 0,
            recipe_tab: Cat::All,
            recipe_sel: 0,
            recipe_craftable: false,
            shift: false,
            arrows: Vec::new(),
            pickups,
            chests,
            chest_at: IVec3::ZERO,
            item_verts: Vec::new(),
            got: Vec::new(),
            tally: (Vec::new(), 0.0),
            test_mobs: env("CUBEWILD_TEST_MOBS").is_some(),
            test_house: env("CUBEWILD_TEST_HOUSE").map(|v| v == "in"),
            fishing: None,
            grow_acc: 0.0,
            auto_cur: 20,
            auto_t: -4.0,
            auto_hold: 0.0,
            auto_backoff: 25.0,
            auto_raised: false,
            audio,
            particles: Particles::default(),
            weather,
            orbs: Orbs::default(),
            furnaces,
            level,
            xp,
            step_acc: 0.0,
            swim_acc: 0.0,
            dig_acc: 0.0,
            was_water: false,
            sprint_fov: 0.0,
            block_colors: vec![[0; 4]; 256],
            ambient_t: 3.0,
            smoke_t: 0.0,
            landings: Vec::new(),
            biome_t: 0.0,
            equip: 0.0,
            bob: (0.0, 0.0),
            riding: None,
            boats,
            paddle_t: 0.0,
            trading: None,
            trade_uses: Default::default(),
        };
        // developer hook: start a new world at once, as the title screen's buttons do
        if let Some(v) = env("CUBEWILD_NEW") {
            game.enter(fresh_seed(), if v == "creative" { Mode::Creative } else { Mode::Survival });
        }
        if env("CUBEWILD_AT").is_some() {
            game.unstick = false;
        }
        game
    }

    /// Puts a loaded world in place of the current one.
    fn install(&mut self, l: Loaded) {
        let seed = l.world.seed;
        self.world = l.world;
        self.player = l.player;
        self.inv = l.inv;
        self.sel = l.sel;
        (self.health, self.food, self.air, self.time) = (l.health, l.food, l.air, l.time);
        self.spawn = l.spawn;
        self.pickups = l.pickups;
        self.chests = l.chests;
        self.furnaces = l.furnaces;
        (self.level, self.xp) = (l.level, l.xp);
        self.weather = l.weather;
        self.boats = l.boats;
        self.riding = None;
        self.orbs = Orbs::default();
        self.particles = Particles::default();
        self.mode = l.mode;
        self.save_dir = l.save_dir;
        self.mobs = Mobs::new(seed as u64 * 7919 + 17);
        self.mobs.ignore = self.mode == Mode::Creative;
        if let Some(d) = &self.save_dir {
            self.mobs.load(&d.join("animals.txt"));
        }
        self.trading = None;
        self.arrows.clear();
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

    /// Saves the current world and enters another: the saved one of that seed, or a
    /// new one in `mode`.
    fn enter(&mut self, seed: u32, mode: Mode) {
        self.save();
        self.install(load_world(seed, self.settings.view_dist, true, mode));
        self.save_player();
        self.goto(Screen::Play);
    }

    /// Removes a saved world from the disk.
    fn delete_world(&mut self, seed: u32) {
        if seed == self.world.seed {
            // step out of it first: a new world that is not saved yet takes its place
            self.world.save_all();
            self.install(load_world(fresh_seed(), self.settings.view_dist, false, Mode::Survival));
        }
        if let Some(dir) = user_dir(false).map(|d| d.join(format!("world-{seed}"))) {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                eprintln!("cubewild: cannot delete {}: {e}", dir.display());
            }
        }
        self.worlds = list_worlds();
    }

    fn save_player(&self) {
        let Some(d) = &self.save_dir else { return };
        let p = &self.player;
        let mut s = format!("{} {} {} {} {}\nslots", p.pos.x, p.pos.y, p.pos.z, p.yaw, p.pitch);
        let mut inv_slots = self.inv.slots.to_vec();
        if !self.inv.held.is_empty() {
            // a stack still on the mouse is saved in the first free slot
            if let Some(free) = inv_slots[..items::BAG_END].iter_mut().find(|s| s.is_empty()) {
                *free = self.inv.held;
            }
        }
        for (i, st) in inv_slots.iter().enumerate().filter(|(_, st)| !st.is_empty()) {
            s += &format!(" {}:{}:{}:{}", i, st.item, st.n, st.wear);
        }
        s += &format!("\nsel {}\n", self.sel);
        s += &format!("stats {} {} {} {}\n", self.health, self.food, self.air, self.time);
        s += &format!("spawn {} {} {}\n", self.spawn.x, self.spawn.y, self.spawn.z);
        s += if self.mode == Mode::Creative { "mode creative\n" } else { "mode survival\n" };
        s += &format!("gen {}\n", self.world.ver);
        s += &format!("xp {} {}\n", self.level, self.xp);
        s += &format!("weather {} {} {}\n", self.weather.want, self.weather.timer, self.weather.storm as u8);
        if let Err(e) = std::fs::write(d.join("player.txt"), s) {
            eprintln!("cubewild: failed to save player: {e}");
        }
        self.pickups.save(&d.join("drops.txt"));
        loot::save_chests(&self.chests, &d.join("chests.txt"));
        furnace::save(&self.furnaces, &d.join("furnaces.txt"));
        self.boats.save(&d.join("boats.txt"));
        self.mobs.save(&d.join("animals.txt"));
    }

    fn save(&self) {
        self.world.save_all();
        self.save_player();
        self.settings.save();
    }

    fn goto(&mut self, screen: Screen) {
        // from the title screen, the other menus lead back to it
        let screen = if screen == Screen::Pause && self.at_title { Screen::Title } else { screen };
        if screen == Screen::Play {
            let ok = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
            if !ok && self.bench.is_none() {
                return;
            }
            self.window.set_cursor_visible(false);
            self.started = true;
            self.at_title = false;
            self.auto_t = -3.0;
        } else {
            self.window.set_cursor_grab(CursorGrabMode::None).ok();
            self.window.set_cursor_visible(true);
            self.buttons = [false; 2];
            self.keys.clear();
            if matches!(self.screen, Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade) {
                // what was on the mouse goes back in the pack, or on the ground
                self.inv.stow_held();
                let left = std::mem::take(&mut self.inv.held);
                self.throw(left);
            }
            self.mining = None;
            if matches!(self.screen, Screen::Play | Screen::Chest | Screen::Furnace) {
                self.save_player();
            }
        }
        if self.screen == Screen::Settings && screen != Screen::Settings {
            self.settings.save();
        }
        self.screen = screen;
    }

    fn target(&self) -> Option<(IVec3, IVec3, f32)> {
        raycast(&self.world, self.player.eye(), self.player.look_dir(), 6.0)
    }

    /// The water block in the crosshair, if nothing solid is in the way.
    fn water_target(&self, reach: f32) -> Option<IVec3> {
        let hit = raycast_with(&self.world, self.player.eye(), self.player.look_dir(), reach, |b| b == WATER || is_solid(b))?;
        (self.world.get_block(hit.0.x, hit.0.y, hit.0.z) == WATER).then_some(hit.0)
    }

    /// Uses up one of the held item, except in creative.
    fn spend(&mut self) {
        if self.mode == Mode::Survival {
            self.inv.take_slot(self.sel);
        }
    }

    /// Spends one use of the held tool and reports it if it breaks.
    fn wear_held(&mut self) {
        if self.mode == Mode::Creative {
            return;
        }
        let item = self.inv.hot(self.sel);
        if self.inv.wear_tool(self.sel) {
            self.say(format!("Your {} broke", items::name(item)));
        }
    }

    /// What a slain animal leaves behind, lying where it fell.
    fn loot(&mut self, drops: mobs::Drops, at: Vec3) {
        for (item, n) in drops {
            self.spill(at + Vec3::Y * 0.5, item, n);
        }
    }

    /// Drops items on the ground at `at` with a small hop.
    fn spill(&mut self, at: Vec3, item: Item, n: u32) {
        if self.quiet {
            return;
        }
        self.pickups.pop(&mut self.rng, at, Stack::new(item, n));
    }

    /// Throws a stack out in front of the player.
    fn throw(&mut self, st: Stack) {
        let d = self.player.look_dir();
        self.pickups.spawn(self.player.eye() - Vec3::Y * 0.3 + d * 0.3, d * 6.0 + Vec3::Y * 1.5, st, 1.5);
    }

    /// Q: drops one of the held item, or the whole stack.
    fn drop_selected(&mut self, all: bool) {
        let st = self.inv.slots[self.sel];
        if st.is_empty() {
            return;
        }
        if all {
            self.inv.slots[self.sel] = Stack::default();
            self.throw(st);
        } else {
            self.inv.take_slot(self.sel);
            self.throw(Stack { n: 1, ..st });
        }
    }

    /// Everything carried and worn falls to the ground where the player died.
    fn die(&mut self) {
        self.health = 0.0;
        self.riding = None;
        let at = self.player.pos + Vec3::Y * 0.9;
        // some of the experience is left behind, the rest is lost
        let keep = (self.level * 4).min(60);
        self.orbs.spawn(&mut self.rng, at, keep);
        (self.level, self.xp) = (0, 0);
        let inv = std::mem::replace(&mut self.inv, Inventory::new());
        for st in inv.slots.into_iter().chain([inv.held]) {
            self.pickups.pop(&mut self.rng, at, st);
        }
        self.goto(Screen::Dead);
    }

    /// True if block `b` put at `q` would not cut into the player or a mob.
    fn fits(&self, q: IVec3, b: Block) -> bool {
        if !is_solid(b) {
            return true;
        }
        let (boxes, n) = self.world.shape_at(b, q.x, q.y, q.z);
        let (lo, hi) = (self.player.pos - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW), self.player.pos + Vec3::new(PLAYER_HW, self.player.height(), PLAYER_HW));
        boxes[..n].iter().all(|bx| {
            let (min, max) = (Vec3::new(bx[0], bx[1], bx[2]), Vec3::new(bx[3], bx[4], bx[5]));
            !(hi.cmpgt(min).all() && lo.cmplt(max).all()) && !self.mobs.blocks_box(min, max)
        })
    }

    /// View distance in use: the setting, or less while the frame rate needs help.
    fn apply_dist(&mut self) {
        let d = if self.settings.auto_scale { self.settings.view_dist.min(self.auto_cur) } else { self.settings.view_dist };
        if d != self.world.radius {
            self.world.set_radius(d);
        }
    }

    /// Trades view distance for frame rate so the game holds 120 fps where the display allows it.
    /// (Measured on integrated graphics: the cost is in geometry, not in pixels.)
    fn auto_dist(&mut self, dt: f32) {
        self.auto_t += dt;
        self.auto_hold -= dt;
        if self.auto_t < 1.5 || self.fps < 1.0 {
            return;
        }
        self.auto_t = 0.0;
        let hz = self.window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).map_or(60.0, |m| m as f32 / 1000.0);
        let cap = if self.settings.vsync { hz } else { f32::INFINITY };
        if cap < 119.0 {
            return; // the display cannot show 120 fps: nothing to gain
        }
        let cur = self.world.radius;
        if self.fps < 117.0 && cur > 7 {
            self.auto_cur = cur - 1;
            // a raise that had to be undone is retried less and less often
            self.auto_backoff = if self.auto_raised { (self.auto_backoff * 2.0).min(300.0) } else { 20.0 };
            self.auto_hold = self.auto_backoff;
            self.auto_raised = false;
            self.apply_dist();
        } else if self.auto_hold <= 0.0 && cur < self.settings.view_dist && self.fps > (cap * 0.985).min(150.0) {
            self.auto_cur = cur + 1;
            self.auto_hold = 6.0;
            self.auto_raised = true;
            self.apply_dist();
        }
    }

    fn say(&mut self, msg: String) {
        self.tally.1 = 0.0;
        self.toast = (msg, 2.4);
    }

    /// Adds items to the inventory; what does not fit lands at the player's feet.
    fn give(&mut self, item: Item, n: u32) {
        let left = self.inv.add(item, n);
        if left > 0 {
            self.say("Inventory full".into());
            self.pickups.spawn(self.player.pos + Vec3::Y * 0.5, Vec3::ZERO, Stack::new(item, left), 1.5);
        }
    }

    /// Damage from blows and falls, softened by armour.
    fn strike(&mut self, amount: f32) {
        self.hurt(amount * self.inv.armor_factor());
    }

    fn hurt(&mut self, amount: f32) {
        self.health -= amount;
        if amount >= 0.5 && self.hurt_flash < 0.2 {
            self.audio.play(Snd::Hurt, 0.9, 0.95 + self.rng.f() * 0.1);
        }
        self.hurt_flash = 0.35;
    }

    /// The average colour of a block's texture (for the chips it throws when dug).
    fn block_color(&mut self, b: Block) -> [u8; 3] {
        let c = self.block_colors[b as usize];
        if c[3] != 0 {
            return [c[0], c[1], c[2]];
        }
        let layer = tex_for(b, 0) as usize;
        let n = textures::TEX as i32;
        let (mut sum, mut k) = ([0u32; 3], 0u32);
        for y in 0..n {
            for x in 0..n {
                let t = textures::texel(layer, x, y);
                if t[3] >= 128 {
                    for i in 0..3 {
                        sum[i] += t[i] as u32;
                    }
                    k += 1;
                }
            }
        }
        let c = if k == 0 { [128, 128, 128] } else { sum.map(|v| (v / k) as u8) };
        self.block_colors[b as usize] = [c[0], c[1], c[2], 255];
        c
    }

    /// Gains experience, a level at a time.
    fn gain_xp(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        self.xp += n;
        let pitch = 0.8 + self.rng.f() * 0.5;
        self.audio.play(Snd::Xp, 0.6, pitch);
        let mut up = false;
        while self.xp >= fx::xp_to_next(self.level) {
            self.xp -= fx::xp_to_next(self.level);
            self.level += 1;
            up = true;
        }
        if up {
            self.audio.play(Snd::LevelUp, 0.8, 1.0);
            self.say(format!("Level {}", self.level));
        }
    }

    /// Experience a slain creature leaves.
    fn kill_xp(&mut self, kind: mobs::Kind, at: Vec3) {
        self.audio.play_var(Snd::Voice(kind, audio::Call::Death), at + Vec3::Y, 0.9);
        let n = match kind {
            mobs::Kind::Villager => 0,
            mobs::Kind::Hare => 1,
            k if k.is_monster() => 5,
            mobs::Kind::Wolf => 3,
            _ => 1 + (self.rng.f() < 0.5) as u32 * 2,
        };
        if self.mode == Mode::Survival {
            self.orbs.spawn(&mut self.rng, at + Vec3::Y * 0.5, n);
        }
    }

    /// The everyday call of a creature.
    fn mob_voice(kind: mobs::Kind) -> Snd {
        Snd::Voice(kind, audio::Call::Idle)
    }

    /// A blow lands on creature `i`: sound and sparks.
    fn mob_struck(&mut self, i: usize, at: Vec3) {
        let kind = self.mobs.list[i].kind;
        self.audio.play_var(Snd::Hit, at, 0.8);
        // the cry of the one hit (or its last, if the blow kills it: see `kill_xp`)
        if self.mobs.alive(i) {
            self.audio.play_var(Snd::Voice(kind, audio::Call::Hurt), at + Vec3::Y, 0.8);
        }
        self.particles.sparks(&mut self.rng, at + Vec3::Y * 0.7, 8);
    }

    /// Direction of the sun and the resulting sky brightness (0 night .. 1 day).
    fn sun(&self) -> (Vec3, f32) {
        let a = self.time * std::f32::consts::TAU;
        let d = Vec3::new(a.cos(), a.sin(), 0.32).normalize();
        (d, smooth(-0.2, 0.16, d.y))
    }

    fn near_station(&self, b: Block) -> bool {
        b == AIR || self.world.block_near(self.player.pos + Vec3::Y, 4, b)
    }

    fn respawn(&mut self) {
        self.riding = None;
        self.player.pos = self.spawn;
        self.player.vel = Vec3::ZERO;
        self.player.flying = false;
        self.health = MAX_HEALTH;
        self.food = MAX_FOOD;
        self.air = MAX_AIR;
        self.ready = false;
        self.unstick = true;
        self.goto(Screen::Play);
    }

    /// Uses the held item on creature `i`. False if that did nothing (so a block can
    /// still be placed past it).
    fn use_on_mob(&mut self, i: usize, item: Item) -> bool {
        let (kind, at) = (self.mobs.list[i].kind, self.mobs.list[i].pos);
        let head = at + Vec3::Y;
        match self.mobs.interact(i, item) {
            mobs::Use::Nothing => return false,
            mobs::Use::Fed => {
                self.spend();
                self.audio.play_var(Snd::Eat, head, 0.7);
            }
            mobs::Use::Love => {
                self.spend();
                self.audio.play_var(Snd::Eat, head, 0.7);
                self.audio.play_var(Snd::Voice(kind, audio::Call::Idle), head, 0.8);
            }
            mobs::Use::Healed => {
                self.spend();
                self.audio.play_var(Snd::Bark, head, 0.8);
            }
            mobs::Use::Dyed => {
                self.spend();
                self.audio.play_var(Snd::Pop, head, 0.6);
            }
            mobs::Use::Sheared { item, n } => {
                self.audio.play_var(Snd::Shear, head, 0.9);
                self.spill(head, item, n);
                self.wear_held();
            }
            mobs::Use::Tamed => {
                self.spend();
                self.audio.play_var(Snd::Bark, head, 1.0);
                for _ in 0..6 {
                    self.particles.sparks(&mut self.rng, head, 2);
                }
                self.say("Tamed! Right click your dog to make it sit or follow".into());
            }
            mobs::Use::NotYet => {
                self.spend();
                for _ in 0..5 {
                    self.particles.smoke(&mut self.rng, head + Vec3::Y * 0.3, false);
                }
            }
            mobs::Use::Sit(sitting) => self.say(if sitting { "Your dog sits and waits" } else { "Your dog follows you" }.into()),
            mobs::Use::Trade => {
                if let Some(id) = self.mobs.villager_id(i) {
                    self.trading = Some(id);
                    self.audio.play_var(Snd::Voice(kind, audio::Call::Idle), head, 0.9);
                    self.goto(Screen::Trade);
                }
            }
        }
        true
    }

    /// The villager being traded with: their index now, if they are still close by.
    fn trader(&self) -> Option<usize> {
        let id = self.trading?;
        (0..self.mobs.list.len()).find(|&i| self.mobs.villager_id(i) == Some(id) && self.mobs.alive(i) && self.mobs.list[i].pos.distance(self.player.pos) < 8.0)
    }

    /// Makes offer `k` of the villager traded with, if the player has what it costs.
    fn trade(&mut self, k: usize) {
        let (Some(i), Some(id)) = (self.trader(), self.trading) else { return };
        let Some(of) = trade::offers(self.mobs.profession(i)).get(k) else { return };
        let used = self.trade_uses.get(&(id, k)).copied().unwrap_or(0);
        if used >= of.uses {
            self.say("Sold out until tomorrow".into());
            return;
        }
        if of.give.iter().any(|&(it, n)| n > 0 && self.inv.count(it) < n) {
            self.say("You don't have enough for that".into());
            return;
        }
        for &(it, n) in &of.give {
            if n > 0 {
                self.inv.take(it, n);
            }
        }
        self.give(of.get.0, of.get.1);
        self.trade_uses.insert((id, k), used + 1);
        let at = self.mobs.list[i].pos;
        self.audio.play_var(Snd::Voice(mobs::Kind::Villager, audio::Call::Idle), at + Vec3::Y, 0.8);
        self.audio.play(Snd::Pop, 0.6, 1.2);
        let xp = 1 + (self.rng.f() < 0.5) as u32;
        self.gain_xp(xp);
    }

    /// A puffcap bursts: a crater of broken blocks (a few of them drop), and a blast
    /// that hurts and throws whatever is near.
    fn explode(&mut self, at: Vec3, radius: f32) {
        let c = at.floor().as_ivec3();
        let r = radius.ceil() as i32;
        let mut edits = Vec::new();
        for dy in -r..=r {
            for dz in -r..=r {
                for dx in -r..=r {
                    let p = c + IVec3::new(dx, dy, dz);
                    let d = (p.as_vec3() + 0.5 - at).length();
                    // a ragged edge, not a perfect ball
                    if d > radius * (0.7 + 0.3 * self.rng.f()) {
                        continue;
                    }
                    let b = self.world.get_block(p.x, p.y, p.z);
                    // (chests and furnaces are sturdy enough to keep what they hold)
                    if matches!(b, AIR | BEDROCK | WATER | LAVA | CHEST | LOOT_CHEST | FURNACE) {
                        continue;
                    }
                    edits.push((p, AIR));
                }
            }
        }
        let broken = self.world.set_many(&self.gfx, &edits);
        self.weather.dirty();
        for (p, b) in broken {
            // a third of what breaks is left to pick up
            if self.rng.f() < 0.33 && self.mode == Mode::Survival {
                let drop = items::drop_for(b, AIR);
                if drop != AIR {
                    self.pickups.pop(&mut self.rng, p.as_vec3() + 0.5, Stack::new(drop, 1));
                }
            }
            if self.rng.f() < 0.15 {
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, 3);
            }
        }
        for _ in 0..40 {
            let o = Vec3::new(self.rng.range(-1.5, 1.5), self.rng.range(-0.5, 1.5), self.rng.range(-1.5, 1.5));
            let dark = self.rng.f() < 0.5;
            self.particles.smoke(&mut self.rng, at + o, dark);
        }
        for _ in 0..16 {
            let o = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(0.0, 1.0), self.rng.range(-1.0, 1.0));
            self.particles.ember(&mut self.rng, at + o);
        }
        self.particles.sparks(&mut self.rng, at, 24);
        self.audio.play_at(Snd::Boom, at, 1.4, 0.9 + self.rng.f() * 0.2);
        // the blast: worse the closer you are, softened by armour; it throws you back
        let reach = radius * 2.0;
        let to = self.player.pos + Vec3::Y * 0.9 - at;
        let d = to.length();
        if d < reach && self.mode == Mode::Survival && self.screen != Screen::Dead {
            let k = 1.0 - d / reach;
            self.strike(16.0 * k);
            self.player.vel += to.normalize_or(Vec3::Y) * 12.0 * k + Vec3::Y * 5.0 * k;
            if self.health <= 0.0 {
                self.die();
            }
        }
        for i in 0..self.mobs.list.len() {
            let m = &self.mobs.list[i];
            let d = (m.pos + Vec3::Y * 0.5 - at).length();
            if !self.mobs.alive(i) || d >= reach {
                continue;
            }
            let (kind, pos) = (m.kind, m.pos);
            let dmg = (14.0 * (1.0 - d / reach)).ceil() as i32;
            if let Some(drops) = self.mobs.hit(i, at, dmg) {
                self.loot(drops, pos);
                self.kill_xp(kind, pos);
            }
        }
    }

    /// Steps out of the boat: on top of it if there is room, else beside it.
    fn get_out(&mut self) {
        let Some(i) = self.riding.take() else { return };
        let b = self.boats.list[i].pos;
        let fwd = boat::forward(self.boats.list[i].yaw);
        let side = Vec3::new(fwd.z, 0.0, -fwd.x);
        let spots = [b + Vec3::Y * boat::H, b + side * 1.2 + Vec3::Y * 0.6, b - side * 1.2 + Vec3::Y * 0.6, b + fwd * 1.5 + Vec3::Y * 0.6, b - fwd * 1.5 + Vec3::Y * 0.6];
        let spot = spots.into_iter().find(|&p| !collides(&self.world, p, PLAYER_HW, PLAYER_H)).unwrap_or(b + Vec3::Y * 1.0);
        self.player.pos = spot;
        self.player.vel = Vec3::ZERO;
        self.place_cd = 0.3;
    }

    /// Footsteps, landings, splashes and swimming strokes.
    fn feet(&mut self, input: &Input, before: Vec3, fall_speed: f32, dt: f32) {
        let pl = self.player.clone();
        let under = (pl.pos - Vec3::Y * 0.2).floor().as_ivec3();
        let mut ground = self.world.get_block(under.x, under.y, under.z);
        if !is_target(ground) {
            ground = self.world.get_block(under.x, under.y - 1, under.z);
        }
        let feet = pl.pos;
        let in_water = pl.in_water && !pl.in_lava;
        // falling into water
        if in_water && !self.was_water && fall_speed > 4.0 {
            let surf = Vec3::new(feet.x, feet.y.floor() + 0.95, feet.z);
            self.audio.play_var(Snd::Splash, surf, (fall_speed / 14.0).clamp(0.4, 1.0));
            self.particles.splash(&mut self.rng, surf, (fall_speed * 2.0).min(40.0) as usize);
        }
        self.was_water = in_water;
        if pl.flying {
            return;
        }
        let moved = Vec3::new(feet.x - before.x, 0.0, feet.z - before.z).length();
        if pl.in_water {
            if input.forward != 0.0 || input.right != 0.0 || input.jump {
                self.swim_acc -= dt;
                if self.swim_acc <= 0.0 {
                    self.swim_acc = 0.9;
                    self.audio.play_var(Snd::Swim, feet, 0.5);
                }
            }
            return;
        }
        // a hard landing kicks up dust
        if pl.impact > 6.0 && is_target(ground) {
            self.audio.play_var(Snd::Step(audio::surface(ground)), feet, (pl.impact / 12.0).clamp(0.6, 1.2));
            let col = self.block_color(ground);
            let n = (pl.impact * 1.5).min(24.0) as usize;
            self.particles.dust(&mut self.rng, feet, col, n, 1.6);
            self.step_acc = 0.0;
        }
        if !pl.on_ground || !is_target(ground) || ground == WATER {
            return;
        }
        self.step_acc += moved;
        let stride = if input.sneak { 1.2 } else { 1.9 };
        if self.step_acc >= stride {
            self.step_acc = 0.0;
            let vol = if input.sneak { 0.25 } else if input.sprint { 0.75 } else { 0.55 };
            self.audio.play_var(Snd::Step(audio::surface(ground)), feet, vol);
            if input.sprint {
                let col = self.block_color(ground);
                self.particles.dust(&mut self.rng, feet, col, 3, 0.8);
            }
        }
    }

    /// Calls of creatures around, lava bubbling, rain, cave wind; smoke from torches
    /// and furnaces.
    fn ambience(&mut self, night: bool, dt: f32) {
        let eye = self.player.eye();
        self.audio.listen(eye, self.player.yaw);
        self.ambient_t -= dt;
        if self.ambient_t <= 0.0 {
            self.ambient_t = self.rng.range(1.5, 4.0);
            // one creature in earshot speaks up (monsters more often at night)
            let near: Vec<usize> = (0..self.mobs.list.len()).filter(|&i| self.mobs.list[i].pos.distance(eye) < 24.0 && self.mobs.alive(i)).collect();
            if !near.is_empty() {
                let i = near[(self.rng.f() * near.len() as f32) as usize % near.len()];
                let m = &self.mobs.list[i];
                let (kind, at) = (m.kind, m.pos + Vec3::Y);
                let chance = if kind.is_monster() { if night { 0.8 } else { 0.5 } } else { 0.45 };
                if self.rng.f() < chance {
                    let pitch = 0.85 + self.rng.f() * 0.3;
                    self.audio.play_at(Self::mob_voice(kind), at, 0.8, pitch);
                }
            }
        }
        // smoke from torches and lit furnaces, and lava popping
        self.smoke_t -= dt;
        if self.smoke_t <= 0.0 {
            self.smoke_t = 0.12;
            let lights = self.world.lights_near(eye, 16.0);
            for (q, b, _) in lights.into_iter().take(8) {
                if self.rng.f() > 0.35 {
                    continue;
                }
                match b {
                    TORCH => {
                        let top = q.as_vec3() + Vec3::new(0.5, 0.8, 0.5);
                        self.particles.smoke(&mut self.rng, top, true);
                    }
                    b if is_torch(b) => {
                        let (dx, dz) = SIDE_DIR[(b - WALL_TORCH) as usize];
                        let top = q.as_vec3() + Vec3::new(0.5 + dx as f32 * 0.16, 0.98, 0.5 + dz as f32 * 0.16);
                        self.particles.smoke(&mut self.rng, top, true);
                    }
                    LAVA if self.rng.f() < 0.15 => {
                        let top = q.as_vec3() + Vec3::new(self.rng.f(), 1.0, self.rng.f());
                        self.particles.ember(&mut self.rng, top);
                        self.audio.play_var(Snd::Fizz, top, 0.5);
                    }
                    _ => {}
                }
            }
            let lit: Vec<IVec3> = self.furnaces.iter().filter(|(p, f)| f.lit() && p.as_vec3().distance(eye) < 20.0).map(|(p, _)| *p).collect();
            for p in lit {
                if self.rng.f() < 0.4 {
                    let c = p.as_vec3() + Vec3::new(self.rng.range(0.3, 0.7), 1.05, self.rng.range(0.3, 0.7));
                    self.particles.smoke(&mut self.rng, c, true);
                }
                if self.rng.f() < 0.12 {
                    self.audio.play_var(Snd::Crackle, p.as_vec3() + 0.5, 0.7);
                }
            }
        }
        // backgrounds: rain (muffled under a roof), wind in caves, the hush under water
        let eye_cell = eye.floor().as_ivec3();
        let sky = self.world.sky_level(eye_cell.x, eye_cell.y, eye_cell.z);
        let wet = if self.weather.dry || self.weather.snowy { 0.0 } else { self.weather.rain };
        let outside = self.weather.open_sky(eye);
        let rain = wet * if outside { 1.0 } else if sky > 0 { 0.45 } else { 0.0 };
        let cave = if sky == 0 { smooth(56.0, 20.0, eye.y) * 0.8 } else { 0.0 };
        let under = if self.player.eye_in_water(&self.world) { 1.0 } else { 0.0 };
        self.audio.beds(rain, cave, under);
    }

    /// Hunger, healing, fall damage and drowning.
    fn survive(&mut self, input: &Input, dt: f32) {
        if self.player.impact > 13.5 {
            let d = (self.player.impact - 13.5) * 0.95;
            self.strike(d);
        }
        // lava: two hearts every half second, whatever is worn
        self.lava_t -= dt;
        if self.player.in_lava && self.lava_t <= 0.0 {
            self.lava_t = 0.5;
            self.hurt(4.0);
        }
        if self.player.eye_in_water(&self.world) {
            self.air -= dt;
            if self.air <= -1.0 {
                self.air = 0.0;
                self.hurt(2.0);
            }
        } else {
            self.air = (self.air + dt * 4.0).min(MAX_AIR);
        }
        let moving = input.forward != 0.0 || input.right != 0.0;
        let effort = if !moving {
            0.0
        } else if input.sprint {
            0.035
        } else {
            0.006
        };
        self.food = (self.food - dt * (0.011 + effort)).max(0.0);
        if self.food > 13.0 && self.health < MAX_HEALTH {
            self.health = (self.health + 0.45 * dt).min(MAX_HEALTH);
            self.food -= 0.14 * dt;
        }
        if self.food <= 0.0 && self.health > 4.0 {
            self.health -= 0.25 * dt;
        }
    }

    /// Left button held: hit an animal in reach, otherwise keep mining the targeted block.
    fn primary(&mut self, dt: f32) {
        let (o, d) = (self.player.eye(), self.player.look_dir());
        let tool = self.inv.hot(self.sel);
        let block = self.target();
        let (damage, reach) = items::weapon(tool);
        if let Some((i, t)) = self.boats.pick(o, d, reach) {
            let nearer = block.map_or(true, |b| t < b.2) && self.mobs.pick(o, d, reach).map_or(true, |m| t < m.1);
            if nearer {
                self.mining = None;
                if self.primary_cd <= 0.0 {
                    self.primary_cd = 0.4;
                    self.swing = SWING;
                    let at = self.boats.list[i].pos;
                    self.audio.play_var(Snd::Dig(audio::Surface::Wood), at, 0.8);
                    let creative = self.mode == Mode::Creative;
                    if self.boats.hit(i, if creative { 9.0 } else { 1.0 }) {
                        self.boats.list.remove(i);
                        self.riding = match self.riding {
                            Some(r) if r == i => None,
                            Some(r) if r > i => Some(r - 1),
                            r => r,
                        };
                        self.audio.play_var(Snd::Break(audio::Surface::Wood), at, 0.9);
                        self.particles.chips(&mut self.rng, at.floor().as_ivec3(), [168, 124, 72], 14);
                        if !creative {
                            self.spill(at + Vec3::Y * 0.4, items::BOAT, 1);
                        }
                    }
                }
                return;
            }
        }
        if let Some((i, t)) = self.mobs.pick(o, d, reach) {
            if block.map_or(true, |b| t < b.2) {
                self.mining = None;
                if self.primary_cd <= 0.0 {
                    self.primary_cd = 0.4;
                    self.hit_flash = 0.18;
                    self.swing = SWING;
                    let at = self.mobs.list[i].pos;
                    let kind = self.mobs.list[i].kind;
                    self.mob_struck(i, at);
                    let dead = self.mobs.hit(i, self.player.pos, damage);
                    self.wear_held();
                    if let Some(drops) = dead {
                        self.loot(drops, at);
                        self.kill_xp(kind, at);
                    }
                }
                return;
            }
        }
        let Some((p, _, _)) = block else {
            self.mining = None;
            return;
        };
        let b = self.world.get_block(p.x, p.y, p.z);
        if b == BEDROCK {
            self.mining = None;
            return;
        }
        let progress = match self.mining {
            Some((mp, t)) if mp == p => t + dt,
            _ => 0.0,
        };
        if self.swing <= 0.0 {
            self.swing = SWING;
        }
        if self.mode == Mode::Survival {
            self.dig_acc -= dt;
            if self.dig_acc <= 0.0 {
                self.dig_acc = 0.24;
                let c = p.as_vec3() + 0.5;
                self.audio.play_var(Snd::Dig(audio::surface(b)), c, 0.7);
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, 2);
            }
        }
        // creative breaks at once, a few blocks a second, and the blocks drop nothing
        let creative = self.mode == Mode::Creative;
        if creative && self.primary_cd > 0.0 {
            return;
        }
        if creative || progress >= items::break_time(b, tool) {
            self.mining = None;
            self.primary_cd = if creative { 0.22 } else { 0.0 };
            if self.world.set_block(&self.gfx, p.x, p.y, p.z, AIR) {
                self.quiet = creative;
                self.dig_acc = 0.0;
                self.weather.dirty();
                // what it gives falls where the block was
                let drop = items::drop_for(b, tool);
                let at = p.as_vec3() + 0.5;
                self.audio.play_var(Snd::Break(audio::surface(b)), at, 0.9);
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, p, col, if is_torch(b) || is_plant(b) { 5 } else { 14 });
                if !creative && drop != AIR {
                    let xp = match b {
                        COAL_ORE => (self.rng.f() < 0.6) as u32,
                        CRYSTAL_ORE => 3 + (self.rng.f() * 4.0) as u32,
                        _ => 0,
                    };
                    self.orbs.spawn(&mut self.rng, at, xp);
                }
                if b == FURNACE {
                    // a broken furnace spills what it held
                    if let Some(f) = self.furnaces.remove(&p) {
                        for st in f.slots {
                            self.pickups.pop(&mut self.rng, at, st);
                        }
                        let xp = f.xp as u32;
                        self.orbs.spawn(&mut self.rng, at, xp);
                    }
                }
                match b {
                    GRAVEL if self.rng.f() < 0.25 => self.spill(at, items::FLINT, 1),
                    b if is_tuft(b) && tool != items::SHEARS => {
                        if self.rng.f() < 0.6 {
                            self.spill(at, items::FIBER, 1);
                        }
                        if self.rng.f() < 0.3 {
                            self.spill(at, items::SEEDS, 1);
                        }
                    }
                    WHEAT_3 => {
                        self.spill(at, items::WHEAT, 1);
                        let seeds = 1 + (self.rng.f() < 0.5) as u32;
                        self.spill(at, items::SEEDS, seeds);
                    }
                    BERRY_BUSH => self.spill(at, drop, 2),
                    _ => self.spill(at, drop, 1),
                }
                if is_leaves(b) && self.rng.f() < 0.3 {
                    self.spill(at, items::STICK, 1);
                }
                if b == CHEST || b == LOOT_CHEST {
                    // a broken chest spills what it held
                    let held = if b == LOOT_CHEST { Some(loot::roll_chest(self.world.seed, p)) } else { self.chests.remove(&p) };
                    for st in held.into_iter().flatten() {
                        self.pickups.pop(&mut self.rng, at, st);
                    }
                }
                if drop == AIR && items::tier_needed(b) > 0 && !creative {
                    let need = if items::tier_needed(b) >= 3 { "an iron" } else { "a stone" };
                    self.say(format!("{} needs {need} pickaxe or better", items::name(b)));
                }
                if !is_plant(b) && !is_torch(b) {
                    self.wear_held();
                }
                if is_door(b) {
                    // the other half goes with it
                    let y = if door_upper(b) { p.y - 1 } else { p.y + 1 };
                    if is_door(self.world.get_block(p.x, y, p.z)) {
                        self.world.set_block(&self.gfx, p.x, y, p.z, AIR);
                    }
                }
                // doors, torches and plants standing on the block come loose
                let above = self.world.get_block(p.x, p.y + 1, p.z);
                let up = at + Vec3::Y;
                if is_door(above) && !door_upper(above) {
                    if self.world.set_pair(&self.gfx, p.x, p.y + 1, p.z, AIR, AIR) {
                        self.spill(up, items::door_item(above), 1);
                    }
                } else if (above == TORCH || is_plant(above)) && self.world.set_block(&self.gfx, p.x, p.y + 1, p.z, AIR) {
                    self.spill(up, items::drop_for(above, tool), 1);
                    // (a stand of reeds falls whole)
                    let mut y = p.y + 2;
                    while above == REEDS && self.world.get_block(p.x, y, p.z) == REEDS && self.world.set_block(&self.gfx, p.x, y, p.z, AIR) {
                        self.spill(up, items::FIBER, 1);
                        y += 1;
                    }
                }
                // and so do ladders and torches hanging on it
                for (s, (dx, dz)) in SIDE_DIR.into_iter().enumerate() {
                    let c = IVec3::new(p.x - dx, p.y, p.z - dz);
                    let hung = self.world.get_block(c.x, c.y, c.z);
                    let item = if hung == LADDER_BASE + s as u8 {
                        items::LADDER
                    } else if hung == WALL_TORCH + s as u8 {
                        TORCH
                    } else {
                        continue;
                    };
                    if self.world.set_block(&self.gfx, c.x, c.y, c.z, AIR) {
                        self.spill(c.as_vec3() + 0.5, item, 1);
                    }
                }
                self.quiet = false;
            }
        } else {
            self.mining = Some((p, progress));
        }
    }

    fn place(&mut self) {
        self.place_cd = 0.2;
        let item = self.inv.hot(self.sel);
        if item != AIR {
            self.swing = SWING;
        }
        let target = self.target();
        // use the held item on a creature in reach: feed, shear, dye, tame, trade
        if let Some((i, t)) = self.mobs.pick(self.player.eye(), self.player.look_dir(), 4.0) {
            if target.map_or(true, |b| t < b.2) && self.use_on_mob(i, item) {
                self.place_cd = 0.3;
                return;
            }
        }
        // get into a boat in reach (unless a block is nearer)
        if self.riding.is_none() {
            if let Some((i, t)) = self.boats.pick(self.player.eye(), self.player.look_dir(), 4.0) {
                if target.map_or(true, |b| t < b.2) {
                    self.riding = Some(i);
                    self.player.vel = Vec3::ZERO;
                    self.player.swimming = false;
                    self.place_cd = 0.4;
                    self.say("Shift to get out".into());
                    return;
                }
            }
        }
        if item == items::BOAT {
            // on water if the crosshair finds some, otherwise on top of the block aimed at
            let yaw = self.player.yaw;
            let at = if let Some(w) = self.water_target(6.0) {
                // (the top of the water: climb out of any water above the cell aimed at)
                let mut y = w.y;
                while self.world.get_block(w.x, y + 1, w.z) == WATER {
                    y += 1;
                }
                Some(Vec3::new(w.x as f32 + 0.5, y as f32 + 0.9, w.z as f32 + 0.5))
            } else {
                target.filter(|t| t.1.y == 1).map(|(p, _, _)| p.as_vec3() + Vec3::new(0.5, 1.0, 0.5))
            };
            match at {
                Some(at) if !collides(&self.world, at + Vec3::Y * 0.05, boat::HW, boat::H) => {
                    self.boats.place(at, yaw);
                    self.spend();
                    self.audio.play_var(Snd::Splash, at, 0.4);
                }
                _ => self.say("A boat goes on water, or on top of a block".into()),
            }
            return;
        }
        // sneak to place a block against a door, chest or trapdoor instead of using it
        let sneak = self.keys.contains(&KeyCode::ShiftLeft);
        // use the block you are looking at
        if let Some((p, _, _)) = target {
            match self.world.get_block(p.x, p.y, p.z) {
                b @ (CHEST | LOOT_CHEST) if !sneak => {
                    self.chest_at = p;
                    if b == LOOT_CHEST {
                        // found in a cave and opened for the first time: from now on an
                        // ordinary chest holding what was left in it
                        self.chests.insert(p, loot::roll_chest(self.world.seed, p));
                        self.world.set_block(&self.gfx, p.x, p.y, p.z, CHEST);
                        self.say("You found a chest".into());
                    }
                    self.chests.entry(p).or_insert([Stack::default(); items::CHEST_SLOTS]);
                    self.audio.play_var(Snd::Chest, p.as_vec3() + 0.5, 0.8);
                    self.goto(Screen::Chest);
                    return;
                }
                b if is_trapdoor(b) && !sneak => {
                    self.place_cd = 0.25;
                    let turned = TRAP_BASE + ((b - TRAP_BASE) ^ TRAP_OPEN);
                    if !self.fits(p, turned) {
                        self.say("Something is in the way of the trapdoor".into());
                    } else if self.world.set_block(&self.gfx, p.x, p.y, p.z, turned) {
                        self.audio.play_var(Snd::Door, p.as_vec3() + 0.5, 0.7);
                    }
                    return;
                }
                BED => {
                    self.place_cd = 0.6;
                    self.spawn = p.as_vec3() + Vec3::new(0.5, 1.01, 0.5);
                    if self.sun().1 < 0.35 {
                        self.time = 0.015;
                        self.say("You slept until morning. Spawn point set".into());
                    } else {
                        self.say("Spawn point set. You can only sleep at night".into());
                    }
                    return;
                }
                WORKBENCH => {
                    self.goto(Screen::Inventory);
                    return;
                }
                FURNACE if !sneak => {
                    self.chest_at = p;
                    self.furnaces.entry(p).or_default();
                    self.goto(Screen::Furnace);
                    return;
                }
                b if is_door(b) && !sneak => {
                    self.toggle_door(p, b);
                    return;
                }
                _ => {}
            }
        }
        let bite = items::food(item);
        if bite > 0.0 {
            if self.food < MAX_FOOD - 0.5 {
                self.spend();
                self.food = (self.food + bite).min(MAX_FOOD);
                self.place_cd = 0.4;
                let pitch = 0.9 + self.rng.f() * 0.2;
                self.audio.play(Snd::Eat, 0.7, pitch);
                self.say(format!("Ate {}", items::name(item)));
            }
            return;
        }
        match items::gear(item) {
            Some((items::Kind::Hoe, _)) => {
                // till the ground
                if let Some((p, _, _)) = target {
                    let above = self.world.get_block(p.x, p.y + 1, p.z);
                    let soil = { let b = self.world.get_block(p.x, p.y, p.z); b == DIRT || is_grassy(b) };
                    if soil && (above == AIR || is_tuft(above)) {
                        if above != AIR {
                            self.world.set_block(&self.gfx, p.x, p.y + 1, p.z, AIR);
                        }
                        if self.world.set_block(&self.gfx, p.x, p.y, p.z, FARMLAND) {
                            self.wear_held();
                        }
                    }
                }
                return;
            }
            _ => {}
        }
        match item {
            items::SEEDS => {
                if let Some((p, n, _)) = target {
                    if n.y == 1 && self.world.get_block(p.x, p.y, p.z) == FARMLAND && self.world.get_block(p.x, p.y + 1, p.z) == AIR {
                        if self.world.set_block(&self.gfx, p.x, p.y + 1, p.z, WHEAT_0) {
                            self.spend();
                        }
                    } else {
                        self.say("Seeds need farmland: till grass or dirt with a hoe".into());
                    }
                }
                return;
            }
            items::BUCKET => {
                if let Some(w) = self.water_target(6.0) {
                    if self.world.set_block(&self.gfx, w.x, w.y, w.z, AIR) {
                        self.audio.play_var(Snd::Swim, w.as_vec3() + 0.5, 0.9);
                        self.inv.slots[self.sel] = Stack::new(items::WATER_BUCKET, 1);
                    }
                }
                return;
            }
            items::WATER_BUCKET => {
                if let Some((p, n, _)) = target {
                    let q = p + n;
                    if is_replaceable(self.world.get_block(q.x, q.y, q.z)) && self.world.set_block(&self.gfx, q.x, q.y, q.z, WATER) {
                        self.audio.play_var(Snd::Splash, q.as_vec3() + 0.5, 0.5);
                        self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                    }
                }
                return;
            }
            items::BOW => {
                if self.inv.take(items::ARROW, 1) {
                    let dir = self.player.look_dir();
                    self.arrows.push(Arrow { pos: self.player.eye() + dir * 0.4, vel: dir * 40.0 + self.player.vel * 0.5, life: 6.0, hostile: false });
                    self.place_cd = 0.7;
                    self.audio.play(Snd::Bow, 0.7, 0.95 + self.rng.f() * 0.1);
                    self.wear_held();
                } else {
                    self.say("No arrows".into());
                }
                return;
            }
            _ => {}
        }
        if matches!(item, items::OAK_DOOR | items::BIRCH_DOOR) {
            self.place_door(target, item);
            return;
        }
        let Some((p, n, t)) = target else { return };
        let q = p + n;
        let clicked = self.world.get_block(p.x, p.y, p.z);
        let d = self.player.look_dir();
        // the side of the cell you look towards, and the side the clicked wall is on
        let facing: u8 = if d.x.abs() > d.z.abs() { (d.x > 0.0) as u8 } else { 2 + (d.z > 0.0) as u8 };
        let wall: u8 = if n.x == 1 { 0 } else if n.x == -1 { 1 } else if n.z == 1 { 2 } else { 3 };
        // pointing at the upper half of a side, or at an underside, places the high variant
        let high = n.y == -1 || (n.y == 0 && (self.player.eye() + d * t).y - p.y as f32 > 0.5);
        let (at, block): (IVec3, Block) = match item {
            items::LADDER => {
                if n.y != 0 || !is_opaque(clicked) {
                    self.say("A ladder hangs on the side of a solid block".into());
                    return;
                }
                (q, LADDER_BASE + wall)
            }
            TORCH => {
                // on top of a block it stands, on a side it hangs from the wall
                let ok = is_solid(clicked) && !is_door(clicked) && !is_ladder(clicked) && !is_trapdoor(clicked) && !is_fence(clicked) || (is_fence(clicked) && n.y == 1);
                if n.y == -1 || !ok || (n.y == 0 && !is_opaque(clicked)) {
                    self.say("A torch stands on a block or hangs on its side".into());
                    return;
                }
                (q, if n.y == 1 { TORCH } else { WALL_TORCH + wall })
            }
            items::TRAPDOOR => (q, TRAP_BASE + if n.y == 0 { wall } else { facing } + if high { TRAP_TOP } else { 0 }),
            i if i >= items::STAIRS && i < items::STAIRS + 4 => (q, STAIR_BASE + facing + if high { STAIR_FLIP } else { 0 } + (i - items::STAIRS) * 8),
            i if (SLAB..SLAB_END).contains(&i) => {
                // two halves of a kind make a full block again
                let mat = i - SLAB;
                let full = SHAPE_FULL[mat as usize];
                let same = |b: Block| is_slab(b) && slab_mat(b) == mat;
                if same(clicked) && n.y == if slab_top(clicked) { -1 } else { 1 } {
                    (p, full)
                } else if same(self.world.get_block(q.x, q.y, q.z)) {
                    (q, full)
                } else {
                    (q, if high { SLAB_TOP + mat } else { i })
                }
            }
            i if items::is_block(i) => (q, items::block_of(i)),
            _ => return,
        };
        let cur = self.world.get_block(at.x, at.y, at.z);
        // (a slab is only ever replaced by the full block its second half makes)
        let joins = is_slab(cur) && (SLAB..SLAB_END).contains(&item) && block == SHAPE_FULL[slab_mat(cur) as usize];
        if !is_replaceable(cur) && !joins {
            return;
        }
        if !self.fits(at, block) {
            return;
        }
        if self.world.set_block(&self.gfx, at.x, at.y, at.z, block) {
            self.spend();
            self.weather.dirty();
            self.audio.play_var(Snd::Place(audio::surface(block)), at.as_vec3() + 0.5, 0.8);
        }
    }

    /// A plank hut around the player: a closed oak door to the west, an open birch door to
    /// the east, windows north and west, on a levelled yard (CUBEWILD_TEST_HOUSE).
    /// Returns false while the chunks it stands on are still loading.
    fn build_test_house(&mut self, inside: bool) -> bool {
        let c = self.player.pos.floor().as_ivec3();
        let Some(g) = self.world.surface_y(c.x, c.z) else { return false };
        if [(-7, -7), (7, -7), (-7, 7), (7, 7)].iter().any(|d| !self.world.is_loaded(c.x + d.0, c.z + d.1)) {
            return false;
        }
        let (ox, oz, y0) = (c.x - 3, c.z - 3, g + 1);
        for dz in -3..10 {
            for dx in -3..10 {
                let hut = (0..7).contains(&dx) && (0..7).contains(&dz);
                let edge = dx == 0 || dx == 6 || dz == 0 || dz == 6;
                for y in g - 3..y0 + 12 {
                    let b = match y - y0 {
                        i32::MIN..=-1 => COBBLE,
                        0..=2 if hut && edge => PLANKS,
                        3 if hut => BIRCH_PLANKS,
                        _ => AIR,
                    };
                    self.world.set_block_bg(ox + dx, y, oz + dz, b);
                }
            }
        }
        for i in 2..=4 {
            self.world.set_block_bg(ox + i, y0 + 1, oz + 6, PANE);
        }
        self.world.set_block_bg(ox, y0 + 1, oz + 1, PANE);
        self.world.set_block_bg(ox, y0 + 1, oz + 5, PANE);
        let (west, east) = (DOOR_BASE, DOOR_BASE + 1 + 32 + DOOR_OPEN);
        for (x, door) in [(ox, west), (ox + 6, east)] {
            self.world.set_block_bg(x, y0, oz + 3, door);
            self.world.set_block_bg(x, y0 + 1, oz + 3, door + DOOR_UPPER);
        }
        // one of each new shape: outside along the west wall, and inside by the east door
        for (dx, dy, dz, b) in [
            (-1, 0, 5, STAIR_BASE + 1 + 16),
            (-1, 0, 6, STAIR_BASE + 1 + STAIR_FLIP),
            (-1, 0, 4, SLAB + 2),
            // these two turn the stairs by the wall into an inner corner, and the first
            // of them into an outer one
            (-2, 0, 5, STAIR_BASE + 3 + 16),
            (-2, 0, 6, STAIR_BASE + 16),
            (-1, 2, 3, SLAB_TOP),
            (-2, 0, 0, FENCE),
            (-2, 0, 1, FENCE),
            (-1, 0, 0, FENCE),
            (-1, 0, 2, LADDER_BASE + 1),
            (-1, 1, 2, LADDER_BASE + 1),
            (-1, 2, 2, LADDER_BASE + 1),
            (5, 0, 1, CHEST),
            (5, 0, 2, TRAP_BASE),
            (5, 0, 4, TRAP_BASE + 1 + TRAP_OPEN),
            (5, 0, 5, SLAB),
        ] {
            self.world.set_block_bg(ox + dx, y0 + dy, oz + dz, b);
        }
        let chest = IVec3::new(ox + 5, y0, oz + 1);
        let mut slots = [Stack::default(); items::CHEST_SLOTS];
        for (i, st) in [Stack::new(COBBLE, 64), Stack::new(items::COAL, 12), Stack { item: items::IRON_PICK, n: 1, wear: 40 }, Stack::new(items::BREAD, 3)].into_iter().enumerate() {
            slots[i * 4] = st;
        }
        self.chests.insert(chest, slots);
        if std::env::var("CUBEWILD_SCREEN").as_deref() == Ok("chest") {
            self.chest_at = chest;
            self.goto(Screen::Chest);
        }
        if std::env::var("CUBEWILD_SCREEN").as_deref() == Ok("furnace") {
            // a furnace part way through a batch of iron (its window only: no block)
            let at = chest + IVec3::new(0, 40, 0);
            let mut f = furnace::Furnace { burn: 50.0, burn_max: 80.0, cook: 3.0, ..Default::default() };
            f.slots = [Stack::new(IRON_ORE, 12), Stack::new(items::COAL, 3), Stack::new(items::IRON_INGOT, 4)];
            self.furnaces.insert(at, f);
            self.chest_at = at;
            self.goto(Screen::Furnace);
        }
        // a few things lying in the yard, out of reach of the starting spot
        for (i, st) in [Stack::new(COBBLE, 5), Stack::new(PLANKS, 1), Stack::new(items::IRON_PICK, 1), Stack::new(items::STICK, 3), Stack::new(items::STAIRS, 2)].into_iter().enumerate() {
            self.pickups.spawn(Vec3::new(ox as f32 - 0.6, y0 as f32 + 0.5, oz as f32 + 2.6 + i as f32 * 0.45), Vec3::ZERO, st, 0.0);
        }
        self.world.set_block(&self.gfx, ox + 3, y0 + 2, oz + 3, LAMP);
        // torches on the west and north walls
        self.world.set_block(&self.gfx, ox + 1, y0 + 2, oz + 2, WALL_TORCH);
        self.world.set_block(&self.gfx, ox + 3, y0 + 2, oz + 1, WALL_TORCH + 2);
        let p = &mut self.player;
        if inside {
            (p.pos, p.yaw, p.pitch) = (Vec3::new(ox as f32 + 2.5, y0 as f32 + 0.01, oz as f32 + 3.5), 1.2, 0.0);
        } else {
            (p.pos, p.yaw, p.pitch) = (Vec3::new(ox as f32 - 2.6, y0 as f32 + 0.01, oz as f32 + 2.2), 1.35, -0.05);
        }
        if std::env::var_os("CUBEWILD_TEST_BOAT").is_some() {
            // a boat on the yard in front of the hut
            self.boats.place(Vec3::new(ox as f32 - 0.5, y0 as f32, oz as f32 + 4.5), 0.6);
        }
                if let Some((y, t)) = std::env::var("CUBEWILD_LOOK").ok().and_then(|v| v.split_once(',').and_then(|(y, t)| Some((y.parse().ok()?, t.parse().ok()?)))) {
            (p.yaw, p.pitch) = (y, t);
        }
        true
    }

    /// True if the leaf of door block `b`, two cells tall from `p`, would cut into the
    /// player or a mob.
    fn door_blocked(&self, p: IVec3, b: Block) -> bool {
        let q = shape_boxes(b, |_, _| AIR).0[0];
        let min = p.as_vec3() + Vec3::new(q[0], 0.0, q[2]);
        let max = p.as_vec3() + Vec3::new(q[3], 2.0, q[5]);
        let (lo, hi) = (self.player.pos - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW), self.player.pos + Vec3::new(PLAYER_HW, self.player.height(), PLAYER_HW));
        (hi.cmpgt(min).all() && lo.cmplt(max).all()) || self.mobs.blocks_box(min, max)
    }

    /// Opens or closes the door one of whose halves is `b` at `p`.
    fn toggle_door(&mut self, p: IVec3, b: Block) {
        self.place_cd = 0.25;
        let y = if door_upper(b) { p.y - 1 } else { p.y };
        let (lower, upper) = (self.world.get_block(p.x, y, p.z), self.world.get_block(p.x, y + 1, p.z));
        if !is_door(lower) || !is_door(upper) {
            return;
        }
        let (lower, upper) = (lower ^ DOOR_OPEN, upper ^ DOOR_OPEN);
        if self.door_blocked(IVec3::new(p.x, y, p.z), lower) {
            self.say("Something is in the way of the door".into());
            return;
        }
        if self.world.set_pair(&self.gfx, p.x, y, p.z, lower, upper) {
            self.audio.play_var(Snd::Door, p.as_vec3() + 0.5, 0.75);
        }
    }

    /// Stands a door on the block you point at, on the edge of its cell nearest to you.
    fn place_door(&mut self, target: Option<(IVec3, IVec3, f32)>, item: Item) {
        let Some((p, n, _)) = target else { return };
        let q = p + n;
        let at = |w: &World, dx: i32, dy: i32, dz: i32| w.get_block(q.x + dx, q.y + dy, q.z + dz);
        let full = |b: Block| is_solid(b) && !is_shaped(b);
        let w = &self.world;
        if !is_replaceable(at(w, 0, 0, 0)) || !is_replaceable(at(w, 0, 1, 0)) || !full(at(w, 0, -1, 0)) {
            self.say("A door needs firm ground and two blocks of room".into());
            return;
        }
        let d = self.player.look_dir();
        let side = if d.x.abs() > d.z.abs() { (d.x < 0.0) as u8 } else { 2 + (d.z < 0.0) as u8 };
        // the hinge goes on the left, unless that would leave it hanging in the open
        // or there is a door there already (so two doors meet in the middle)
        let beside = |s: u8| at(w, SIDE_DIR[s as usize].0, 0, SIDE_DIR[s as usize].1);
        let (left, right) = (beside(SIDE_LEFT[side as usize]), beside(SIDE_RIGHT[side as usize]));
        let hinge_right = !is_door(right) && (is_door(left) || (full(right) && !full(left)));
        let lower = DOOR_BASE + side + if hinge_right { DOOR_HINGE_RIGHT } else { 0 } + (item - items::OAK_DOOR) * 32;
        if self.door_blocked(q, lower) {
            return;
        }
        if self.world.set_pair(&self.gfx, q.x, q.y, q.z, lower, lower + DOOR_UPPER) {
            self.spend();
        }
    }

    fn key_pressed(&mut self, code: KeyCode) {
        match code {
            KeyCode::Escape => match self.screen {
                Screen::Play => self.goto(Screen::Pause),
                Screen::Pause | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade => self.goto(Screen::Play),
                Screen::Dead | Screen::Title => {}
                _ => self.goto(Screen::Pause),
            },
            KeyCode::KeyE | KeyCode::Tab => match self.screen {
                Screen::Play => self.goto(Screen::Inventory),
                Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade => self.goto(Screen::Play),
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

    /// Applies a UI action. Returns true if the game should quit.
    fn apply(&mut self, a: Action) -> bool {
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
                self.window.set_fullscreen(if self.settings.fullscreen {
                    Some(Fullscreen::Borderless(None))
                } else {
                    None
                });
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
                if right {
                    self.inv.right_click(i);
                } else if !self.shift {
                    self.inv.click(i);
                } else if self.screen == Screen::Chest {
                    // shift-click sends the stack into the chest
                    if let Some(chest) = self.chests.get_mut(&self.chest_at) {
                        self.inv.slots[i] = items::stash_in(chest, self.inv.slots[i]);
                    }
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
                if let Some(chest) = self.chests.get_mut(&self.chest_at) {
                    if right {
                        items::right_click_stack(&mut self.inv.held, &mut chest[i], true);
                    } else if self.shift {
                        chest[i] = self.inv.pick_up(chest[i]);
                    } else {
                        items::click_stack(&mut self.inv.held, &mut chest[i], true);
                    }
                }
            }
            Action::FurnaceSlot(i, right) => {
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
                if seed == self.world.seed && self.save_dir.is_some() {
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
                self.save();
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

    fn resize(&mut self, w: u32, h: u32) {
        self.gfx.resize(w, h);
        let s = ui_scale(self.gfx.size().1);
        if (s - self.font.scale).abs() > 0.01 {
            let (font, atlas) = Font::bake(s);
            self.gfx.upload_font(&atlas);
            self.font = Rc::new(font);
        }
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let real_dt = (now - self.last).as_secs_f32();
        self.last = now;
        let dt = real_dt.min(0.05);
        self.fps_acc.0 += real_dt;
        self.fps_acc.1 += 1;
        self.worst_acc = self.worst_acc.max(real_dt * 1000.0);
        if self.fps_acc.0 >= 0.5 {
            self.fps = self.fps_acc.1 as f32 / self.fps_acc.0;
            self.worst_ms = self.worst_acc;
            self.fps_acc = (0.0, 0);
            self.worst_acc = 0.0;
        }

        // ---- simulate
        let pc = ((self.player.pos.x.floor() as i32) >> 4, (self.player.pos.z.floor() as i32) >> 4);
        let t0 = Instant::now();
        if self.screen != Screen::Title {
            self.world.update(&self.gfx, pc);
        }
        self.last_jump += real_dt;
        self.swing -= dt;
        let t_world = t0.elapsed().as_secs_f32() * 1000.0;
        self.ready = self.ready
            || self.world.is_loaded(self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32);
        if self.ready && self.unstick {
            self.unstick = false;
            if collides(&self.world, self.player.pos, PLAYER_HW, PLAYER_H) {
                // buried, most likely in a tree that stands on the spot: step aside to the
                // nearest open ground, or failing that climb out on top
                let (px, pz) = (self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32);
                let rings = (1..=8i32).flat_map(|r| (-r..=r).flat_map(move |i| [(i, -r), (i, r), (-r, i), (r, i)]));
                let open = rings.map(|(dx, dz)| (px + dx, pz + dz)).find_map(|(x, z)| {
                    let h = self.world.ground_height(x, z);
                    let p = Vec3::new(x as f32 + 0.5, h as f32 + 1.0, z as f32 + 0.5);
                    let ok = h > SEA && self.world.is_loaded(x, z) && is_solid(self.world.get_block(x, h, z)) && !collides(&self.world, p, PLAYER_HW, PLAYER_H);
                    ok.then_some(p)
                });
                match open {
                    Some(p) => self.player.pos = p,
                    None => {
                        while collides(&self.world, self.player.pos, PLAYER_HW, PLAYER_H) && self.player.pos.y < CH as f32 {
                            self.player.pos.y += 1.0;
                        }
                    }
                }
            }
        }

        let playing = self.screen == Screen::Play;
        let k = |c: KeyCode| playing && self.keys.contains(&c);
        let mut input = Input {
            forward: (k(KeyCode::KeyW) as i32 - k(KeyCode::KeyS) as i32) as f32,
            right: (k(KeyCode::KeyD) as i32 - k(KeyCode::KeyA) as i32) as f32,
            jump: k(KeyCode::Space),
            sneak: k(KeyCode::ShiftLeft),
            // too hungry to run
            sprint: k(KeyCode::ControlLeft) && (self.food > 6.0 || self.mode == Mode::Creative),
        };
        if let Some(b) = &mut self.bench {
            b.t += real_dt;
            input = Input { forward: 1.0, ..Default::default() };
            self.player.flying = true;
            self.player.yaw +=
                std::env::var("CUBEWILD_BENCH_TURN").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.12) * dt;
            let floor = self.world.ground_height(self.player.pos.x as i32, self.player.pos.z as i32) as f32 + 14.0;
            self.player.pos.y = self.player.pos.y.max(floor);
            if b.t > 6.0 {
                b.frames.push(real_dt);
            }
            if b.t > 6.0 + b.duration {
                let mut f = b.frames.clone();
                f.sort_by(|a, b| b.partial_cmp(a).unwrap());
                let avg = f.iter().sum::<f32>() / f.len() as f32;
                let low = &f[..(f.len() / 100).max(1)];
                let low_avg = low.iter().sum::<f32>() / low.len() as f32;
                let over = f.iter().filter(|t| **t > 1.0 / 120.0).count();
                let (w, h) = self.gfx.size();
                println!(
                    "BENCH {w}x{h} dist={} frames={} avg_fps={:.1} low1pct_fps={:.1} worst_ms={:.2} frames_over_8.3ms={} chunks_drawn={} quads={}",
                    self.world.radius,
                    f.len(),
                    1.0 / avg,
                    1.0 / low_avg,
                    f[0] * 1000.0,
                    over,
                    self.drawn.0,
                    self.drawn.1
                );
                el.exit();
                return;
            }
        }
        // the world keeps running while the inventory is open, but pauses in the menus
        let simulate = self.ready && matches!(self.screen, Screen::Play | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade);
        if simulate {
            let was = self.time;
            self.time = (self.time + dt / DAY_SECS).rem_euclid(1.0);
            if self.time < was {
                // a new day: the villagers have restocked
                self.trade_uses.clear();
            }
            let fall_speed = -self.player.vel.y;
            let before = self.player.pos;
            let ridden = self.riding.map(|i| (i, &input));
            let turned = self.boats.update(&self.world, self.player.pos, ridden, dt);
            if let Some(i) = self.riding {
                // sitting in the boat: the view turns with it, Shift gets out
                let b = &self.boats.list[i];
                let (bpos, bvel, on_water) = (b.pos, b.vel, b.on_water);
                self.player.yaw += turned;
                self.player.pos = bpos + Vec3::Y * 0.05 - Vec3::Y * 0.6;
                self.player.vel = bvel;
                self.player.on_ground = false;
                self.player.impact = 0.0;
                let moving = Vec3::new(bvel.x, 0.0, bvel.z).length() > 0.8;
                self.paddle_t -= dt;
                if on_water && moving && self.paddle_t <= 0.0 {
                    self.paddle_t = 0.8;
                    self.audio.play_var(Snd::Swim, bpos, 0.35);
                }
                if input.sneak {
                    self.get_out();
                }
            } else {
                self.player.update(&self.world, &input, dt);
                self.feet(&input, before, fall_speed, dt);
            }
            // (a thunderstorm is as dark as night to the monsters)
            let night = self.sun().1 < 0.3 || (self.weather.storm && self.weather.rain > 0.5);
            if self.test_house.is_some_and(|inside| self.build_test_house(inside)) {
                self.test_house = None;
            }
            if self.test_house.is_none() && std::mem::take(&mut self.test_mobs) {
                self.mobs.spawn_test(&self.world, self.player.pos);
            }
            if self.screen == Screen::Trade {
                if let Some(i) = self.trader() {
                    self.mobs.hold(i);
                }
            }
            let lure = self.inv.hot(self.sel);
            let bites = self.mobs.update(&self.world, self.player.pos, input.sneak && !self.player.flying, night, lure, dt);
            for ev in std::mem::take(&mut self.mobs.events) {
                match ev {
                    mobs::MobEvent::Born { kind, pos } => {
                        self.audio.play_var(Snd::Voice(kind, audio::Call::Idle), pos + Vec3::Y, 0.8);
                        let xp = 1 + (self.rng.f() * 4.0) as u32;
                        self.orbs.spawn(&mut self.rng, pos + Vec3::Y * 0.5, xp);
                    }
                    mobs::MobEvent::Killed { kind, pos, drops } => {
                        self.loot(drops, pos);
                        self.kill_xp(kind, pos);
                    }
                    mobs::MobEvent::Fuse { pos } => self.audio.play_at(Snd::Fuse, pos + Vec3::Y, 1.0, 1.0),
                    mobs::MobEvent::Explode { pos } => self.explode(pos, 3.2),
                }
            }
            for (pos, vel) in self.mobs.shots.drain(..) {
                self.arrows.push(Arrow { pos, vel, life: 5.0, hostile: true });
            }
            if self.mode == Mode::Creative {
                (self.health, self.food, self.air) = (MAX_HEALTH, MAX_FOOD, MAX_AIR);
            } else if self.bench.is_none() {
                if bites > 0.0 {
                    self.strike(bites);
                }
                self.survive(&input, dt);
                if self.health <= 0.0 {
                    self.die();
                }
            }
            // furnaces burn on wherever they are
            for f in self.furnaces.values_mut() {
                if f.lit() || f.cook > 0.0 || (!f.slots[furnace::INPUT].is_empty() && !f.slots[furnace::FUEL].is_empty()) {
                    f.tick(dt);
                }
            }
            // weather
            self.biome_t -= dt;
            if self.biome_t <= 0.0 {
                self.biome_t = 1.0;
                if self.world.ver >= 2 {
                    use worldgen::Biome as B;
                    let b = worldgen::column(self.world.seed, self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32).biome;
                    self.weather.snowy = matches!(b, B::SnowyTaiga | B::SnowyPlains | B::FrozenOcean | B::FrozenRiver | B::Peaks);
                    self.weather.dry = matches!(b, B::Desert | B::Badlands | B::Savanna);
                }
            }
            if self.weather.update(&mut self.rng, dt) {
                self.say("Lightning!".into());
            }
            if self.weather.thunder_now() {
                let pitch = 0.85 + self.rng.f() * 0.3;
                self.audio.play(Snd::Thunder, 1.0, pitch);
            }
            self.particles.update(&self.world, dt);
            // experience floats to the player
            let who = (self.bench.is_none() && self.screen != Screen::Dead && self.mode == Mode::Survival).then_some(self.player.pos);
            let xp = self.orbs.update(&self.world, who, dt);
            self.gain_xp(xp);
            self.ambience(night, dt);
            // items on the ground settle, and those in reach are gathered
            let mut got = std::mem::take(&mut self.got);
            got.clear();
            let who = (self.bench.is_none() && self.screen != Screen::Dead).then_some(self.player.pos);
            self.pickups.update(&self.world, who, &mut self.inv, &mut got, dt);
            if !got.is_empty() {
                let pitch = 0.8 + self.rng.f() * 0.6;
                self.audio.play(Snd::Pop, 0.5, pitch);
                // one line that counts up, the newest kind first, not a message per item
                if self.tally.1 <= 0.0 {
                    self.tally.0.clear();
                }
                for st in &got {
                    let n = match self.tally.0.iter().position(|t| t.item == st.item) {
                        Some(i) => self.tally.0.remove(i).n,
                        None => 0,
                    };
                    self.tally.0.insert(0, Stack::new(st.item, n + st.n));
                }
                self.tally.0.truncate(4);
                self.tally.1 = 2.4;
                let text: Vec<String> = self.tally.0.iter().map(|st| format!("+{} {}", st.n, items::name(st.item))).collect();
                self.toast = (text.join("   "), 2.4);
            }
            self.got = got;
        }
        let playing = self.screen == Screen::Play;
        let (sun_dir, daylight) = self.sun();
        // clouds dim the day, and lightning lights it up for a moment
        let overcast = self.weather.rain;
        let daylight = (daylight * (1.0 - 0.45 * overcast) + self.weather.flash * 0.7).min(1.0);
        if simulate {
            // arrows fly in an arc and stick the first animal or block they meet
            let mut arrows = std::mem::take(&mut self.arrows);
            arrows.retain_mut(|a| {
                a.vel.y -= 20.0 * dt;
                a.life -= dt;
                let step = a.vel * dt;
                let len = step.length().max(1e-4);
                if a.hostile {
                    // a skeleton's arrow: only the player is in danger
                    let p = self.player.pos;
                    let (min, max) = (p - Vec3::new(PLAYER_HW, 0.0, PLAYER_HW), p + Vec3::new(PLAYER_HW, self.player.height(), PLAYER_HW));
                    if player::ray_aabb(a.pos, step / len, min, max).is_some_and(|t| t <= len) {
                        if self.bench.is_none() {
                            self.strike(3.0);
                        }
                        return false;
                    }
                } else if let Some((i, _)) = self.mobs.pick(a.pos, step / len, len) {
                    let at = self.mobs.list[i].pos;
                    let kind = self.mobs.list[i].kind;
                    self.mob_struck(i, at);
                    if let Some(drops) = self.mobs.hit(i, a.pos - step, 5) {
                        self.loot(drops, at);
                        self.kill_xp(kind, at);
                    }
                    self.hit_flash = 0.18;
                    return false;
                }
                // stopped by blocks, closed doors and window panes, but not by an open doorway
                let blocked = raycast_with(&self.world, a.pos, step / len, len, is_solid).is_some();
                if blocked {
                    self.audio.play_var(Snd::ArrowHit, a.pos, 0.6);
                }
                a.pos += step;
                a.life > 0.0 && !blocked
            });
            self.arrows = arrows;
            // crops ripen a stage at a time, at random
            self.grow_acc += dt * 600.0;
            while self.grow_acc >= 1.0 {
                self.grow_acc -= 1.0;
                let p = self.player.pos + Vec3::new(self.rng.range(-24.0, 24.0), self.rng.range(-8.0, 8.0), self.rng.range(-24.0, 24.0));
                let c = p.floor().as_ivec3();
                let b = self.world.get_block(c.x, c.y, c.z);
                if (WHEAT_0..WHEAT_3).contains(&b) {
                    self.world.set_block_bg(c.x, c.y, c.z, b + 1);
                }
            }
        }
        if self.settings.auto_scale && playing && self.ready && self.bench.is_none() {
            self.auto_dist(real_dt);
        }
        // fishing: keep the line in the water until something bites
        let rod = playing && self.buttons[1] && self.inv.hot(self.sel) == items::FISHING_ROD;
        match (rod.then(|| self.water_target(9.0)).flatten(), self.fishing) {
            (Some(w), Some((at, t, need))) if at == w => {
                if t + dt >= need {
                    self.fishing = None;
                    self.give(items::RAW_FISH, 1);
                    self.say("You caught a fish".into());
                    self.wear_held();
                } else {
                    self.fishing = Some((at, t + dt, need));
                }
            }
            (Some(w), _) => self.fishing = Some((w, 0.0, self.rng.range(3.0, 8.0))),
            (None, _) => self.fishing = None,
        }

        self.primary_cd -= dt;
        self.place_cd -= dt;
        self.hit_flash -= dt;
        self.hurt_flash -= dt;
        self.toast.1 -= dt;
        self.tally.1 -= dt;
        if playing && self.bench.is_none() {
            if self.buttons[0] {
                self.primary(dt);
            } else {
                self.mining = None;
            }
            if self.buttons[1] && self.place_cd <= 0.0 {
                self.place();
            }
        }

        // ---- build the frame
        let (w, h) = self.gfx.size();
        let eye = self.player.eye();
        let dir = self.player.look_dir();
        let underwater = self.player.eye_in_water(&self.world);
        let view_dist = self.world.radius as f32 * CW as f32;
        // sprinting widens the view a little
        let running = input.sprint && self.riding.is_none() && (input.forward != 0.0 || input.right != 0.0) && !self.player.in_water && self.player.vel.length() > 5.0;
        let k = 1.0 - (-dt * 10.0).exp();
        self.sprint_fov += (running as u8 as f32 - self.sprint_fov) * k;
        let fov = self.settings.fov * (1.0 + 0.12 * self.sprint_fov);
        let proj = perspective(fov.to_radians(), w as f32 / h as f32, 0.08, view_dist * 1.6 + 64.0);
        // view bobbing: the camera rises and falls a little with each step
        self.equip = (self.equip - dt * 5.0).max(0.0);
        let pl = &self.player;
        let speed = Vec3::new(pl.vel.x, 0.0, pl.vel.z).length();
        let walking = pl.on_ground && !pl.flying && !pl.in_water && self.riding.is_none() && playing;
        let want = if walking { (speed / 4.4).min(1.3) } else { 0.0 };
        self.bob.1 += (want - self.bob.1) * (1.0 - (-dt * 8.0).exp());
        if walking {
            self.bob.0 = (self.bob.0 + speed * dt * 0.52) % 2.0;
        }
        let cam_bob = if self.settings.bobbing {
            let ph = self.bob.0 * std::f32::consts::PI;
            let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
            (side * ph.sin() * 0.035 - Vec3::Y * ph.sin().abs() * 0.055) * self.bob.1
        } else {
            Vec3::ZERO
        };
        if !self.settings.bobbing {
            self.bob.1 = 0.0;
        }
        let vp = proj * look_to(dir) * Mat4::from_translation(-cam_bob);
        let (r0, r1, r2, r3) = (vp.row(0), vp.row(1), vp.row(2), vp.row(3));
        let planes: [Vec4; 5] = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2];

        // from under a roof of rock every cave wall is drawn; from above ground only those nearby
        let eye_cell = eye.floor().as_ivec3();
        let eye_sky = self.world.sky_level(eye_cell.x, eye_cell.y, eye_cell.z).min(2);
        let mut chunks: Vec<(&ChunkMesh, [f32; 3], bool, bool)> = Vec::with_capacity(512);
        let mut quads = 0;
        for &(dx, dz, inner) in &self.world.spiral {
            if !inner {
                continue;
            }
            let key = (pc.0 + dx, pc.1 + dz);
            let Some(mesh) = self.world.chunks.get(&key).and_then(|s| s.mesh.as_ref()) else { continue };
            let min = Vec3::new((key.0 * CW) as f32 - eye.x, -eye.y, (key.1 * CW) as f32 - eye.z);
            let max = min + Vec3::new(CW as f32, mesh.top, CW as f32);
            let visible = planes.iter().all(|p| {
                let v = Vec3::new(
                    if p.x > 0.0 { max.x } else { min.x },
                    if p.y > 0.0 { max.y } else { min.y },
                    if p.z > 0.0 { max.z } else { min.z },
                );
                p.x * v.x + p.y * v.y + p.z * v.z + p.w >= 0.0
            });
            if visible {
                let deep = eye_sky == 0 || dx * dx + dz * dz <= 16;
                quads += mesh.quads(deep);
                // distant plants are only a few pixels: skip their draw calls
                chunks.push((mesh, min.into(), dx * dx + dz * dz <= 64, deep));
            }
        }
        self.drawn = (chunks.len(), quads);

        let mut ents = std::mem::take(&mut self.ents);
        ents.clear();
        self.mobs.draw(&mut ents, eye, view_dist.min(110.0));
        for a in &self.arrows {
            let m = Mat4::from_rotation_translation(Quat::from_rotation_arc(Vec3::Z, a.vel.normalize_or(Vec3::Z)), a.pos - eye);
            push_box(&mut ents, &m, Vec3::new(-0.025, -0.025, -0.5), Vec3::new(0.025, 0.025, 0.3), [150, 110, 64], 255);
            push_box(&mut ents, &m, Vec3::new(-0.05, -0.05, 0.3), Vec3::new(0.05, 0.05, 0.42), [90, 94, 106], 255);
        }
        if let Some((w, t, _)) = self.fishing {
            // the float bobs on the water
            let bob = (t * 3.0).sin() * 0.04;
            let m = Mat4::from_translation(w.as_vec3() - eye + Vec3::new(0.5, 0.86 + bob, 0.5));
            push_box(&mut ents, &m, Vec3::splat(-0.07), Vec3::new(0.07, 0.0, 0.07), [240, 240, 240], 255);
            push_box(&mut ents, &m, Vec3::new(-0.07, 0.0, -0.07), Vec3::splat(0.07), [220, 50, 50], 255);
        }
        // torches are little glowing models; the nearest lights also shine on the terrain
        let mut lights: Vec<[f32; 4]> = Vec::with_capacity(gfx::MAX_LIGHTS);
        let held = self.inv.hot(self.sel);
        if held == TORCH || held == LAMP {
            lights.push([0.0, -0.35, 0.0, 10.0]);
        }
        for (q, b, _) in self.world.lights_near(eye, 46.0) {
            let rel = q.as_vec3() - eye;
            if lights.len() < gfx::MAX_LIGHTS {
                let c = match b {
                    b if (WALL_TORCH..WALL_TORCH_END).contains(&b) => {
                        let (dx, dz) = SIDE_DIR[(b - WALL_TORCH) as usize];
                        rel + Vec3::new(0.5 + dx as f32 * 0.2, 0.85, 0.5 + dz as f32 * 0.2)
                    }
                    _ => rel + Vec3::new(0.5, 0.7, 0.5),
                };
                lights.push([c.x, c.y, c.z, match b {
                    LAMP => 15.0,
                    LAVA => 10.0,
                    _ => 11.0,
                }]);
            }
            if is_torch(b) {
                // a wall torch leans out from the wall it hangs on
                let m = if b == TORCH {
                    Mat4::from_translation(rel + Vec3::new(0.5, 0.0, 0.5))
                } else {
                    let (dx, dz) = SIDE_DIR[(b - WALL_TORCH) as usize];
                    let foot = rel + Vec3::new(0.5 + dx as f32 * 0.44, 0.2, 0.5 + dz as f32 * 0.44);
                    // tilt the top away from the wall
                    let axis = Vec3::new(dz as f32, 0.0, -dx as f32);
                    Mat4::from_translation(foot) * Mat4::from_axis_angle(axis, -0.42)
                };
                push_box(&mut ents, &m, Vec3::new(-0.06, 0.0, -0.06), Vec3::new(0.06, 0.58, 0.06), [150, 106, 58], gfx::EMISSIVE);
                push_box(&mut ents, &m, Vec3::new(-0.09, 0.58, -0.09), Vec3::new(0.09, 0.76, 0.09), [255, 200, 80], gfx::EMISSIVE);
            }
        }
        // lit furnaces glow
        for (p, f) in &self.furnaces {
            let rel = p.as_vec3() - eye + Vec3::new(0.5, 0.6, 0.5);
            if f.lit() && lights.len() < gfx::MAX_LIGHTS && rel.length() < 40.0 {
                lights.push([rel.x, rel.y, rel.z, 9.0]);
            }
        }
        let right = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(dir);
        self.boats.draw(&mut ents, eye);
        self.particles.draw(&mut ents, eye, right, up);
        self.orbs.draw(&mut ents, eye, right, up);
        self.weather.track(&self.world, eye);
        let mut landings = std::mem::take(&mut self.landings);
        landings.clear();
        self.weather.draw(&mut ents, eye, &mut landings);
        if simulate {
            for &l in &landings {
                if self.rng.f() < 0.12 {
                    self.particles.drip(&mut self.rng, l);
                }
            }
        }
        self.landings = landings;
        let mut item_verts = std::mem::take(&mut self.item_verts);
        item_verts.clear();
        self.pickups.draw(&mut item_verts, eye, view_dist.min(56.0));
        let hand_from = item_verts.len();
        if playing && self.bench.is_none() {
            if self.hand_model.0 != held {
                self.hand_model = (held, if held == AIR || held_as_cube(held) { Vec::new() } else { icon_model(held) });
                self.equip = 1.0;
            }
            let pose = HandPose {
                swing: if self.swing > 0.0 { 1.0 - self.swing / SWING } else { 0.0 },
                equip: self.equip,
                bob: self.bob.0,
                bob_amp: self.bob.1,
            };
            push_hand(&mut item_verts, dir, held, &self.hand_model.1, pose, [0.2, 0.52, 1.0][eye_sky as usize], cam_bob);
        }
        let mut sky = std::mem::take(&mut self.sky);
        sky.clear();
        if !underwater {
            let night = (1.0 - smooth(0.15, 0.6, daylight)) * (1.0 - overcast);
            if night > 0.02 {
                let rot = Mat3::from_rotation_z(self.time * std::f32::consts::TAU);
                for &(d, size) in &self.stars {
                    let d = rot * d;
                    if d.y > -0.05 {
                        push_sky_quad(&mut sky, d, 60.0, size, [235, 240, 255, (night * 230.0) as u8]);
                    }
                }
            }
            if sun_dir.y > -0.2 && overcast < 0.7 {
                push_sky_quad(&mut sky, sun_dir, 60.0, 7.0, [255, 236, 170, 70]);
                push_sky_quad(&mut sky, sun_dir, 60.0, 4.4, [255, 246, 214, 255]);
            }
            if sun_dir.y < 0.2 && overcast < 0.7 {
                push_sky_quad(&mut sky, -sun_dir, 60.0, 3.4, [226, 232, 244, 255]);
                push_sky_quad(&mut sky, (-sun_dir + Vec3::new(0.012, 0.012, 0.0)).normalize(), 59.0, 1.0, [196, 204, 222, 255]);
            }
        }
        let target = if playing { raycast(&self.world, eye, dir, 6.0) } else { None };
        if let Some((p, _, _)) = target {
            // wireframe around the targeted block
            // (doors and panes get a box as thin as they are)
            let (lo, hi) = player::block_bounds(&self.world, p);
            let (a, b, t) = (lo - eye - 0.004, hi - eye + 0.004, Vec3::splat(0.012));
            let m = Mat4::IDENTITY;
            for (i, j) in [(a, a), (b, a), (a, b), (b, b)] {
                for (c0, c1) in [(Vec3::new(a.x, i.y, j.z), Vec3::new(b.x, i.y, j.z)), (Vec3::new(i.x, a.y, j.z), Vec3::new(i.x, b.y, j.z)), (Vec3::new(i.x, j.y, a.z), Vec3::new(i.x, j.y, b.z))] {
                    push_box(&mut ents, &m, c0 - t, c1 + t, [10, 10, 10], 200);
                }
            }
        }

        // ---- HUD and menus
        let font = self.font.clone();
        let mut ui = Ui::new(w, h, &font, self.ptr);
        let mut actions: Vec<Action> = Vec::new();
        let (wf, hf) = (w as f32, h as f32);
        let s = ui.s;
        let (cx, cy) = ((wf * 0.5).round(), (hf * 0.5).round());
        if underwater {
            ui.rect(0.0, 0.0, wf, hf, [20, 70, 160, 70]);
        }
        if self.player.eye_in_lava(&self.world) {
            ui.rect(0.0, 0.0, wf, hf, [236, 96, 20, 215]);
        }
        if self.hurt_flash > 0.0 {
            ui.rect(0.0, 0.0, wf, hf, [200, 20, 20, (self.hurt_flash / 0.35 * 80.0) as u8]);
        }
        if playing {
            let t = (2.0 * s).round().max(1.0);
            let cross = if self.hit_flash > 0.0 { [255, 70, 60, 255] } else { [255, 255, 255, 215] };
            ui.rect(cx - 9.0 * s, cy - t * 0.5, 18.0 * s, t, cross);
            ui.rect(cx - t * 0.5, cy - 9.0 * s, t, 18.0 * s, cross);
            if let Some((p, prog)) = self.mining {
                let b = self.world.get_block(p.x, p.y, p.z);
                let total = items::break_time(b, self.inv.hot(self.sel));
                if total > 0.2 {
                    let bw = 90.0 * s;
                    ui.rect(cx - bw * 0.5 - 2.0, cy + 22.0 * s - 2.0, bw + 4.0, 8.0 * s + 4.0, [0, 0, 0, 150]);
                    ui.rect(cx - bw * 0.5, cy + 22.0 * s, bw * (prog / total).min(1.0), 8.0 * s, ACCENT);
                }
            }
        }

        // hotbar
        let slot = (58.0 * s).round();
        let hx = (cx - slot * 4.5).round();
        let hy = (hf - slot - 12.0 * s).round();
        if self.ready && !matches!(self.screen, Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade) {
            for i in 0..9 {
                draw_slot(&mut ui, hx + i as f32 * slot, hy, slot, self.inv.slots[i], i == self.sel, false);
            }
            // experience: a thin bar over the hotbar, the level in its middle
            let survival = self.mode == Mode::Survival;
            let xp_h = if survival { (7.0 * s).round().max(3.0) } else { 0.0 };
            if survival {
                let (bx, bw, by) = (hx + 2.0 * s, slot * 9.0 - 4.0 * s, hy - xp_h - 5.0 * s);
                ui.rect(bx - 1.0, by - 1.0, bw + 2.0, xp_h + 2.0, [0, 0, 0, 170]);
                ui.rect(bx, by, bw, xp_h, [34, 44, 30, 230]);
                let share = self.xp as f32 / fx::xp_to_next(self.level) as f32;
                ui.rect(bx, by, bw * share.min(1.0), xp_h, [128, 230, 70, 255]);
                if self.level > 0 {
                    let t = self.level.to_string();
                    let th = font.height(SMALL);
                    ui.label_centered(cx, by - th * 0.5 - 2.0 * s, SMALL, &t, [150, 250, 90, 255]);
                }
            }
            // health on the left, hunger on the right, air above hunger while diving
            let isz = (22.0 * s).round();
            let iy = hy - isz - 6.0 * s - if survival { xp_h + 8.0 * s } else { 0.0 };
            // (a creative world has neither)
            for i in 0..if self.mode == Mode::Survival { 10 } else { 0 } {
                let x = hx + i as f32 * (isz + 1.0);
                ui.icon(x, iy, isz, items::ICON_HEART, 45);
                ui.icon_part(x, iy, isz, items::ICON_HEART, (self.health - i as f32 * 2.0) / 2.0);
                let x = hx + 9.0 * slot - (i + 1) as f32 * (isz + 1.0);
                ui.icon(x, iy, isz, items::ICON_FOOD, 45);
                ui.icon_part(x, iy, isz, items::ICON_FOOD, (self.food - i as f32 * 2.0) / 2.0);
                if self.air < MAX_AIR - 0.05 && (self.air / MAX_AIR * 10.0).ceil() as i32 > i {
                    ui.icon(x, iy - isz - 3.0 * s, isz, items::ICON_AIR, 255);
                }
            }
            let held = self.inv.hot(self.sel);
            let mut above = iy - font.height(BODY) - 6.0 * s - if self.air < MAX_AIR - 0.05 { isz } else { 0.0 };
            if held != AIR {
                ui.label_centered(cx, above, BODY, items::name(held), TEXT);
                above -= font.height(BODY) + 2.0 * s;
            }
            if self.toast.1 > 0.0 {
                let a = (self.toast.1 * 2.0).min(1.0);
                let mut c = ACCENT;
                c[3] = (a * 255.0) as u8;
                ui.label_centered(cx, above, BODY, &self.toast.0, c);
                above -= font.height(BODY) + 2.0 * s;
            }
            if self.player.flying && playing {
                ui.label_centered(cx, above, SMALL, "Flying", [200, 230, 255, 255]);
                above -= font.height(SMALL) + 2.0 * s;
            }
            if self.fishing.is_some() {
                ui.label_centered(cx, above, SMALL, "Waiting for a bite...", [200, 230, 255, 255]);
            }
            // instruments work from anywhere in the pack
            let mut info: Vec<String> = Vec::new();
            if self.inv.has(items::CLOCK) {
                let hours = (self.time * 24.0 + 6.0).rem_euclid(24.0);
                info.push(format!("{:02}:{:02}", hours as u32, (hours.fract() * 60.0) as u32));
            }
            if self.inv.has(items::COMPASS) {
                let (sy, cy) = self.player.yaw.sin_cos();
                let bearing = sy.atan2(-cy).to_degrees().rem_euclid(360.0);
                let names = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
                let to = self.spawn - self.player.pos;
                let rel = (to.x * -cy + to.z * sy).atan2(to.x * sy + to.z * cy).to_degrees().rem_euclid(360.0);
                let sides = ["ahead", "ahead right", "right", "behind right", "behind", "behind left", "left", "ahead left"];
                info.push(format!(
                    "{}   Spawn {:.0} m {}",
                    names[((bearing + 22.5) / 45.0) as usize % 8],
                    Vec3::new(to.x, 0.0, to.z).length(),
                    sides[((rel + 22.5) / 45.0) as usize % 8]
                ));
            }
            if !info.is_empty() && playing {
                ui.label_centered(cx, 14.0 * s, BODY, &info.join("      "), TEXT);
            }
            if self.started && self.inv.is_empty() && playing && self.mode == Mode::Survival {
                ui.label_centered(cx, 60.0 * s, BODY, "Hold left click on a tree to gather wood, then press E to craft", TEXT);
            }
        }
        if self.settings.show_fps || self.debug {
            ui.label(12.0 * s, 8.0 * s, SMALL, &format!("{:.0} FPS", self.fps), TEXT);
        }
        if self.debug {
            let p = self.player.pos;
            let lines = [
                format!("Position  {:.1}, {:.1}, {:.1}", p.x, p.y, p.z),
                if self.world.ver >= 2 {
                    format!("Biome  {}", worldgen::column(self.world.seed, p.x.floor() as i32, p.z.floor() as i32).biome.name())
                } else {
                    "Biome  - (a world from before there were biomes)".to_string()
                },
                format!("Worst frame  {:.1} ms", self.worst_ms),
                format!("View {} chunks   drawn {}   quads {}", self.world.radius, self.drawn.0, self.drawn.1),
                {
                    let monsters = self.mobs.list.iter().filter(|m| m.kind.is_monster()).count();
                    let villagers = self.mobs.list.iter().filter(|m| m.kind == mobs::Kind::Villager).count();
                    format!("Animals {}   Monsters {}   Villagers {}   {}", self.mobs.list.len() - monsters - villagers, monsters, villagers, self.gfx.adapter_name)
                },
                {
                    let hours = (self.time * 24.0 + 6.0).rem_euclid(24.0);
                    format!("Time {:02}:{:02}   health {:.1}   food {:.1}", hours as u32, (hours.fract() * 60.0) as u32, self.health, self.food)
                },
            ];
            for (i, l) in lines.iter().enumerate() {
                ui.label(12.0 * s, 8.0 * s + (i as f32 + 1.0) * (font.height(SMALL) + 2.0), SMALL, l, [225, 230, 236, 255]);
            }
        }

        if self.screen == Screen::Title {
            ui.rect(0.0, 0.0, wf, hf, [16, 22, 30, 255]);
            let bh = (50.0 * s).round();
            let gap = (12.0 * s).round();
            let rows = self.worlds.len().min(5);
            let pw = (620.0 * s).round();
            let list_h = if rows > 0 { 34.0 * s + rows as f32 * (bh + gap) } else { 0.0 };
            let ph = (150.0 * s + 2.0 * (bh + gap) + list_h + 24.0 * s).round();
            let (px, py) = ((cx - pw * 0.5).round(), (cy - ph * 0.5).round());
            ui.rect(px, py, pw, ph, [20, 24, 32, 255]);
            ui.outline(px, py, pw, ph, (2.0 * s).round().max(1.0), [78, 90, 110, 255]);
            ui.text_centered(cx, py + 18.0 * s, TITLE, "Cubewild", TEXT);
            ui.text_centered(cx, py + 18.0 * s + font.height(TITLE), SMALL, "Every new world is a different one", DIM);
            let bx = px + 40.0 * s;
            let bw = pw - 80.0 * s;
            let half = ((bw - gap) * 0.5).round();
            let mut y = py + 140.0 * s;
            if ui.button(bx, y, half, bh, "New survival world", true) {
                actions.push(Action::NewWorld(Mode::Survival));
            }
            if ui.button(bx + half + gap, y, half, bh, "New creative world", true) {
                actions.push(Action::NewWorld(Mode::Creative));
            }
            y += bh + gap;
            if rows > 0 {
                ui.text(bx, y + 6.0 * s, SMALL, "Your worlds, the latest first", DIM);
                y += 34.0 * s;
                let dw = (120.0 * s).round();
                for w in self.worlds.iter().take(rows) {
                    let kind = if w.mode == Mode::Creative { "Creative" } else { "Survival" };
                    if ui.button(bx, y, bw - dw - gap, bh, &format!("World {}  -  {kind}", w.seed), true) {
                        actions.push(Action::Open(w.seed));
                    }
                    let sure = self.confirm_delete == Some(w.seed);
                    if ui.button(bx + bw - dw, y, dw, bh, if sure { "Sure?" } else { "Delete" }, true) {
                        actions.push(Action::Delete(w.seed));
                    }
                    y += bh + gap;
                }
            }
            if ui.button(bx, y, half, bh, "Settings", true) {
                actions.push(Action::Goto(Screen::Settings));
            }
            if ui.button(bx + half + gap, y, half, bh, "Quit", true) {
                actions.push(Action::Quit);
            }
        } else if !self.ready {
            ui.rect(0.0, 0.0, wf, hf, [16, 22, 30, 255]);
            ui.text_centered(cx, cy - font.height(HEAD), HEAD, "Loading world...", TEXT);
        } else if self.bench.is_none() {
            let bh = (50.0 * s).round();
            let gap = (12.0 * s).round();
            match self.screen {
                Screen::Play | Screen::Title => {}
                Screen::Dead => {
                    ui.rect(0.0, 0.0, wf, hf, [70, 8, 8, 150]);
                    let bw = (300.0 * s).round();
                    ui.text_centered(cx, cy - 150.0 * s, TITLE, "You died", TEXT);
                    ui.text_centered(cx, cy - 150.0 * s + font.height(TITLE) + 4.0 * s, BODY, "Your items lie where you fell, and vanish after five minutes there", DIM);
                    if ui.button(cx - bw * 0.5, cy + 10.0 * s, bw, bh, "Respawn", true) {
                        actions.push(Action::Respawn);
                    }
                    if ui.button(cx - bw * 0.5, cy + 10.0 * s + bh + gap, bw, bh, "Save and quit", true) {
                        actions.push(Action::Quit);
                    }
                }
                Screen::Pause => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 150]);
                    let pw = (440.0 * s).round();
                    let ph = (5.0 * (bh + gap) + 170.0 * s).round();
                    let (px, py) = ((cx - pw * 0.5).round(), (cy - ph * 0.5).round());
                    ui.rect(px, py, pw, ph, [20, 24, 32, 238]);
                    ui.outline(px, py, pw, ph, (2.0 * s).round().max(1.0), [78, 90, 110, 255]);
                    ui.text_centered(cx, py + 18.0 * s, TITLE, "Cubewild", TEXT);
                    let kind = if self.mode == Mode::Creative { "Creative" } else { "Survival" };
                    let sub = format!("{kind} world {}", self.world.seed);
                    ui.text_centered(cx, py + 18.0 * s + font.height(TITLE), SMALL, &sub, DIM);
                    let bx = px + 40.0 * s;
                    let bw = pw - 80.0 * s;
                    let mut y = py + 140.0 * s;
                    let first = if self.started { "Resume" } else { "Play" };
                    for (label, act) in [
                        (first, Action::Goto(Screen::Play)),
                        ("Settings", Action::Goto(Screen::Settings)),
                        ("Controls", Action::Goto(Screen::Controls)),
                        ("Save and go to title", Action::ToTitle),
                        ("Save and quit", Action::Quit),
                    ] {
                        if ui.button(bx, y, bw, bh, label, true) {
                            actions.push(act);
                        }
                        y += bh + gap;
                    }
                }
                Screen::Settings => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 150]);
                    let (pw, ph) = ((700.0 * s).round(), (830.0 * s).round());
                    let (px, py) = ((cx - pw * 0.5).round(), (cy - ph * 0.5).round());
                    let top = ui.panel(px, py, pw, ph, "Settings");
                    let row = (60.0 * s).round();
                    let lx = px + 32.0 * s;
                    let ctl_w = (240.0 * s).round();
                    let ctl_x = px + pw - 32.0 * s - ctl_w;
                    let ctl_h = (26.0 * s).round();
                    let st = self.settings.clone();
                    let mut y = top + 14.0 * s;
                    let row_label = |ui: &mut Ui, y: f32, right: f32, name: &str, hint: &str, value: &str| {
                        ui.text(lx, y + 4.0 * s, BODY, name, TEXT);
                        ui.text(lx, y + 4.0 * s + font.height(BODY) - 3.0 * s, SMALL, hint, DIM);
                        ui.text_right(right - 18.0 * s, y + 14.0 * s, BODY, value, ACCENT);
                    };
                    row_label(&mut ui, y, ctl_x, "View distance", "Lower it if the frame rate drops", &if self.world.radius < st.view_dist { format!("{} chunks (auto {})", st.view_dist, self.world.radius) } else { format!("{} chunks", st.view_dist) });
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, (st.view_dist - 4) as f32 / 16.0) {
                        let d = 4 + (t * 16.0).round() as i32;
                        if d != st.view_dist {
                            actions.push(Action::ViewDist(d));
                        }
                    }
                    y += row;
                    row_label(&mut ui, y, ctl_x, "Field of view", "How wide the camera sees", &format!("{:.0}", st.fov));
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, (st.fov - 55.0) / 45.0) {
                        actions.push(Action::Fov((55.0 + t * 45.0).round()));
                    }
                    y += row;
                    let scale_text = format!("{:.0}%", st.render_scale * 100.0);
                    row_label(&mut ui, y, ctl_x, "Render scale", "Lower can help at high resolutions. The HUD stays sharp", &scale_text);
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, (st.render_scale - 0.5) / 0.5) {
                        let v = 0.5 + (t * 10.0).round() * 0.05;
                        if (v - st.render_scale).abs() > 0.001 {
                            actions.push(Action::Scale(v));
                        }
                    }
                    y += row;
                    row_label(&mut ui, y, ctl_x, "Mouse sensitivity", "Camera turn speed", &format!("{:.1}x", st.sens));
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, (st.sens - 0.2) / 2.8) {
                        actions.push(Action::Sens(((0.2 + t * 2.8) * 10.0).round() / 10.0));
                    }
                    y += row;
                    row_label(&mut ui, y, ctl_x, "Volume", "All sound", &format!("{:.0}%", st.volume * 100.0));
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, st.volume) {
                        actions.push(Action::Volume((t * 20.0).round() / 20.0));
                    }
                    y += row;
                    row_label(&mut ui, y, ctl_x, "Music", "Quiet tunes now and then", &format!("{:.0}%", st.music * 100.0));
                    if let Some(t) = ui.slider(ctl_x, y + 16.0 * s, ctl_w, ctl_h, st.music) {
                        actions.push(Action::Music((t * 20.0).round() / 20.0));
                    }
                    y += row;
                    let tw = (64.0 * s).round();
                    let tx = px + pw - 32.0 * s - tw;
                    for (name, hint, on, act) in [
                        ("Vsync", "Off = uncapped frame rate, more heat", st.vsync, Action::ToggleVsync),
                        ("Fullscreen", "Also toggled with F11", st.fullscreen, Action::ToggleFullscreen),
                        ("Hold 120 FPS", "Shortens the view distance automatically when needed", st.auto_scale, Action::ToggleAuto),
                        ("Show FPS", "Frame counter in the corner", st.show_fps, Action::ToggleFps),
                        ("View bobbing", "The camera sways as you walk", st.bobbing, Action::ToggleBob),
                    ] {
                        row_label(&mut ui, y, tx, name, hint, if on { "On" } else { "Off" });
                        if ui.toggle(tx, y + 14.0 * s, tw, (30.0 * s).round(), on) {
                            actions.push(act);
                        }
                        y += row;
                    }
                    let bw = (200.0 * s).round();
                    if ui.button(cx - bw * 0.5, py + ph - bh - 22.0 * s, bw, bh, "Back", true) {
                        actions.push(Action::Goto(Screen::Pause));
                    }
                }
                Screen::Controls => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 150]);
                    let rows = [
                        ("W A S D", "Move"),
                        ("Space", "Jump / swim up"),
                        ("Ctrl", "Sprint"),
                        ("Shift", "Sneak (animals notice you later)"),
                        ("Hold left click", "Break block / hit animal"),
                        ("Right click", "Place block / eat / open door, trapdoor, chest / use bed, workbench"),
                        ("Q", "Drop the held item (Ctrl+Q drops the whole stack)"),
                        ("Middle click", "Select the block you look at"),
                        ("1 - 9, mouse wheel", "Choose hotbar slot"),
                        ("E or Tab", "Inventory and crafting"),
                        ("In the inventory", "Click moves stacks, right click splits, Shift+click sends across"),
                        ("Right click with a tool", "Hoe tills, bow shoots, rod fishes, bucket scoops"),
                        ("F, or Space twice", "Fly (creative worlds)"),
                        ("F3", "Debug info"),
                        ("F11", "Fullscreen"),
                        ("Esc", "Pause menu"),
                    ];
                    let row = (font.height(BODY) + 12.0 * s).round();
                    let (pw, ph) = ((700.0 * s).round(), (rows.len() as f32 * row + 180.0 * s).round());
                    let (px, py) = ((cx - pw * 0.5).round(), (cy - ph * 0.5).round());
                    let top = ui.panel(px, py, pw, ph, "Controls");
                    let mut y = top + 18.0 * s;
                    for (i, (key, what)) in rows.iter().enumerate() {
                        if i % 2 == 0 {
                            ui.rect(px + 16.0 * s, y - 5.0 * s, pw - 32.0 * s, row, [255, 255, 255, 10]);
                        }
                        ui.text(px + 32.0 * s, y, BODY, key, ACCENT);
                        ui.text(px + 290.0 * s, y, BODY, what, TEXT);
                        y += row;
                    }
                    let bw = (200.0 * s).round();
                    if ui.button(cx - bw * 0.5, py + ph - bh - 22.0 * s, bw, bh, "Back", true) {
                        actions.push(Action::Goto(Screen::Pause));
                    }
                }
                Screen::Inventory => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
                    let slot = (58.0 * s).round();
                    let pad = (20.0 * s).round();
                    let head = (46.0 * s).round();
                    let mw = 9.0 * slot + 2.0 * pad;
                    let top_h = 4.0 * slot;
                    let mh = head + top_h + 14.0 * s + 3.0 * slot + 10.0 * s + slot + pad;
                    let bw = RECIPE_COLS as f32 * slot + 2.0 * pad + 8.0 * s;
                    let gap = (10.0 * s).round();
                    let bx = (cx - (bw + gap + mw) * 0.5).round();
                    let mx = bx + bw + gap;
                    let wy = ((hf - mh) * 0.5).round();
                    let mut tip: Option<Stack> = None;
                    let mut tip_text: Option<&str> = None;
                    let stations = [(WORKBENCH, self.near_station(WORKBENCH)), (FURNACE, self.near_station(FURNACE))];
                    let reachable = |b: Block| b == AIR || stations.iter().any(|&(st, near)| st == b && near);

                    if self.mode == Mode::Creative {
                        // ================= every item, free to take =================
                        draw_window(&mut ui, bx, wy, bw, mh);
                        ui.text(bx + pad, wy + 11.0 * s, HEAD, "All items", INK);
                        let list: Vec<Item> = (1..items::ITEM_END).filter(|&i| items::valid(i)).collect();
                        let gy = wy + head;
                        let rows_fit = (((wy + mh - pad - gy) / slot).floor() as usize).max(1);
                        let total_rows = (list.len() + RECIPE_COLS - 1) / RECIPE_COLS;
                        let scroll = self.recipe_scroll.min(total_rows.saturating_sub(rows_fit));
                        ui.rect(bx + pad - 2.0, gy - 2.0, RECIPE_COLS as f32 * slot + 4.0, rows_fit as f32 * slot + 4.0, INSET);
                        for (k, &item) in list.iter().skip(scroll * RECIPE_COLS).take(RECIPE_COLS * rows_fit).enumerate() {
                            let (x, y) = (bx + pad + (k % RECIPE_COLS) as f32 * slot, gy + (k / RECIPE_COLS) as f32 * slot);
                            if draw_slot(&mut ui, x, y, slot, Stack::new(item, 1), false, true) {
                                tip = Some(Stack::new(item, 1));
                                if ui.ptr.clicked {
                                    actions.push(Action::Take(item, self.shift));
                                }
                            }
                        }
                        if total_rows > rows_fit {
                            let track_h = rows_fit as f32 * slot;
                            let bar_h = track_h * rows_fit as f32 / total_rows as f32;
                            let bar_y = gy + (track_h - bar_h) * scroll as f32 / (total_rows - rows_fit) as f32;
                            let sx = bx + pad + RECIPE_COLS as f32 * slot + 5.0 * s;
                            ui.rect(sx, gy, 5.0 * s, track_h, INSET);
                            ui.rect(sx, bar_y, 5.0 * s, bar_h, PAPER_DARK);
                        }
                    } else {
                    // ================= recipe book =================
                    draw_window(&mut ui, bx, wy, bw, mh);
                    ui.text(bx + pad, wy + 11.0 * s, HEAD, "Recipes", INK);
                    {
                        // "craftable only" switch in the title row
                        let label = "Can craft";
                        let bsz = (18.0 * s).round();
                        let lw = font.width(SMALL, label);
                        let (tx, ty) = (bx + bw - pad - lw - bsz - 8.0 * s, wy + 16.0 * s);
                        let hot = ui.hover(tx, ty - 3.0, lw + bsz + 8.0 * s, bsz + 6.0);
                        ui.rect(tx, ty, bsz, bsz, [92, 82, 66, 255]);
                        ui.rect(tx + 2.0, ty + 2.0, bsz - 4.0, bsz - 4.0, if self.recipe_craftable { INK_GOOD } else { INSET });
                        ui.text(tx + bsz + 8.0 * s, ty - 1.0, SMALL, label, if hot { INK } else { INK_DIM });
                        if hot && ui.ptr.clicked {
                            actions.push(Action::ToggleCraftable);
                        }
                    }
                    let tab_w = ((bw - 2.0 * pad) / CATS.len() as f32).floor();
                    let tab_h = (38.0 * s).round();
                    let ty = wy + head;
                    for (i, &(cat, label, icon)) in CATS.iter().enumerate() {
                        let x = bx + pad + i as f32 * tab_w;
                        let w = tab_w - 2.0;
                        let on = cat == self.recipe_tab;
                        let hot = ui.hover(x, ty, w, tab_h);
                        ui.rect(x, ty, w, tab_h, if on { PAPER_LIGHT } else if hot { [200, 190, 170, 255] } else { INSET });
                        ui.outline(x, ty, w, tab_h, 1.0, PAPER_DARK);
                        if on {
                            ui.rect(x, ty + tab_h - (3.0 * s).round(), w, (3.0 * s).round(), INK);
                        }
                        let isz = (24.0 * s).round();
                        ui.icon(x + (w - isz) * 0.5, ty + (tab_h - isz) * 0.5 - 1.0, isz, items::icon(icon), if on || hot { 255 } else { 190 });
                        if hot {
                            tip_text = Some(label);
                            if ui.ptr.clicked {
                                actions.push(Action::Tab(cat));
                            }
                        }
                    }
                    let list: Vec<usize> = (0..RECIPES.len())
                        .filter(|&i| self.recipe_tab == Cat::All || RECIPES[i].cat == self.recipe_tab)
                        .filter(|&i| !self.recipe_craftable || (self.inv.can_craft(&RECIPES[i]) && reachable(RECIPES[i].station)))
                        .collect();
                    let gy = ty + tab_h + 8.0 * s;
                    let rows_fit = (((wy + mh - pad - gy) / slot).floor() as usize).clamp(1, RECIPE_ROWS);
                    let total_rows = (list.len() + RECIPE_COLS - 1) / RECIPE_COLS;
                    let scroll = self.recipe_scroll.min(total_rows.saturating_sub(rows_fit));
                    ui.rect(bx + pad - 2.0, gy - 2.0, RECIPE_COLS as f32 * slot + 4.0, rows_fit as f32 * slot + 4.0, INSET);
                    if list.is_empty() {
                        ui.text(bx + pad + 10.0 * s, gy + 12.0 * s, SMALL, "Nothing you can craft here yet.", INK_DIM);
                    }
                    for (k, &ri) in list.iter().skip(scroll * RECIPE_COLS).take(RECIPE_COLS * rows_fit).enumerate() {
                        let r = &RECIPES[ri];
                        let (x, y) = (bx + pad + (k % RECIPE_COLS) as f32 * slot, gy + (k / RECIPE_COLS) as f32 * slot);
                        let can = self.inv.can_craft(r) && reachable(r.station);
                        let picked = ri == self.recipe_sel;
                        let hot = ui.hover(x, y, slot, slot);
                        let fill = if hot {
                            [226, 218, 200, 255]
                        } else if can {
                            [202, 208, 170, 255]
                        } else {
                            [150, 140, 124, 255]
                        };
                        ui.rect(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0, fill);
                        ui.outline(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0, 1.0, if can { INK_GOOD } else { PAPER_DARK });
                        if picked {
                            ui.outline(x, y, slot, slot, (3.0 * s).round().max(2.0), INK);
                        }
                        let ip = (slot * 0.18).round();
                        ui.icon(x + ip, y + ip, slot - 2.0 * ip, items::icon(r.out.0), if can { 255 } else { 120 });
                        if r.out.1 > 1 {
                            let n = r.out.1.to_string();
                            ui.label(x + slot - font.width(SMALL, &n) - 6.0 * s, y + slot - font.height(SMALL) - 2.0, SMALL, &n, TEXT);
                        }
                        if hot {
                            tip = Some(Stack::new(r.out.0, 1));
                            if ui.ptr.clicked {
                                actions.push(Action::PickRecipe(ri));
                            }
                        }
                    }
                    if total_rows > rows_fit {
                        let track_h = rows_fit as f32 * slot;
                        let bar_h = track_h * rows_fit as f32 / total_rows as f32;
                        let bar_y = gy + (track_h - bar_h) * scroll as f32 / (total_rows - rows_fit) as f32;
                        let sx = bx + pad + RECIPE_COLS as f32 * slot + 5.0 * s;
                        ui.rect(sx, gy, 5.0 * s, track_h, INSET);
                        ui.rect(sx, bar_y, 5.0 * s, bar_h, PAPER_DARK);
                    }

                    }
                    // ================= inventory window =================
                    draw_window(&mut ui, mx, wy, mw, mh);
                    ui.text(mx + pad, wy + 11.0 * s, HEAD, "Inventory", INK);
                    {
                        // vital signs and the sort button share the title row
                        let sort_w = (64.0 * s).round();
                        let sort_h = (26.0 * s).round();
                        let (sx, sy) = (mx + mw - pad - sort_w, wy + 12.0 * s);
                        let hot = ui.hover(sx, sy, sort_w, sort_h);
                        ui.rect(sx, sy, sort_w, sort_h, if hot { PAPER_LIGHT } else { INSET });
                        ui.outline(sx, sy, sort_w, sort_h, 1.0, PAPER_DARK);
                        ui.text_centered(sx + sort_w * 0.5, sy + (sort_h - font.height(SMALL)) * 0.5, SMALL, "Sort", INK);
                        if hot && ui.ptr.clicked {
                            actions.push(Action::Sort);
                        }
                        let isz = font.height(SMALL).round();
                        let mut x = sx - 14.0 * s;
                        let armour = format!("Armour {:.0}%", (1.0 - self.inv.armor_factor()) * 100.0);
                        for (icon, text) in [
                            (0, armour),
                            (items::ICON_FOOD, format!("{:.0}", self.food.ceil())),
                            (items::ICON_HEART, format!("{:.0}", self.health.ceil())),
                        ] {
                            x -= font.width(SMALL, &text);
                            ui.text(x, sy + (sort_h - isz) * 0.5, SMALL, &text, INK);
                            if icon != 0 {
                                x -= isz + 4.0 * s;
                                ui.icon(x, sy + (sort_h - isz) * 0.5, isz, icon, 255);
                            }
                            x -= 14.0 * s;
                        }
                    }
                    let mut clicks: Vec<Action> = Vec::new();
                    let slot_at = |ui: &mut Ui, tip: &mut Option<Stack>, clicks: &mut Vec<Action>, idx: usize, x: f32, y: f32, selected: bool| {
                        let st = self.inv.slots[idx];
                        if draw_slot(ui, x, y, slot, st, selected, true) {
                            if !st.is_empty() {
                                *tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                clicks.push(Action::Slot(idx, false));
                            } else if ui.ptr.rclicked {
                                clicks.push(Action::Slot(idx, true));
                            }
                        }
                    };
                    // armour column: an empty slot shows the outline of what goes in it
                    let ty = wy + head;
                    let ghosts = [items::IRON_HELMET, items::IRON_CHEST, items::IRON_LEGS, items::IRON_BOOTS];
                    for i in 0..4 {
                        let (x, y) = (mx + pad, ty + i as f32 * slot);
                        let empty = self.inv.slots[items::ARMOR + i].is_empty();
                        slot_at(&mut ui, &mut tip, &mut clicks, items::ARMOR + i, x, y, false);
                        if empty {
                            let ip = (slot * 0.2).round();
                            ui.icon(x + ip, y + ip, slot - 2.0 * ip, items::icon(ghosts[i]), 70);
                        }
                    }
                    // the player, dressed in whatever is worn
                    let fx = mx + pad + slot + 8.0 * s;
                    let fw = (150.0 * s).round();
                    ui.rect(fx, ty + 2.0, fw, top_h - 4.0, [58, 52, 46, 255]);
                    ui.outline(fx, ty + 2.0, fw, top_h - 4.0, 2.0, [92, 82, 66, 255]);
                    {
                        let u = ((top_h - 4.0) / 19.0).floor();
                        let (ox, oy) = ((fx + fw * 0.5).round(), ty + 2.0 + ((top_h - 4.0) - 16.0 * u) * 0.5);
                        let worn = |i: usize| -> Option<[u8; 4]> {
                            items::gear(self.inv.slots[items::ARMOR + i].item).map(|(_, m)| match m {
                                items::Mat::Leather => [150, 98, 58, 255],
                                items::Mat::Iron => [216, 216, 222, 255],
                                items::Mat::Gold => [250, 208, 66, 255],
                                _ => [104, 222, 232, 255],
                            })
                        };
                        let dark = |c: [u8; 4]| [(c[0] as f32 * 0.72) as u8, (c[1] as f32 * 0.72) as u8, (c[2] as f32 * 0.72) as u8, 255];
                        let (skin, shirt, trousers, shoes) = ([226, 176, 134, 255], [176, 92, 52, 255], [104, 94, 76, 255], [52, 44, 38, 255]);
                        // head
                        ui.rect(ox - 2.0 * u, oy, 4.0 * u, 4.0 * u, skin);
                        ui.rect(ox - 2.0 * u, oy, 4.0 * u, u, [38, 32, 30, 255]);
                        ui.rect(ox - 1.2 * u, oy + 1.8 * u, 0.7 * u, 0.7 * u, [40, 40, 60, 255]);
                        ui.rect(ox + 0.5 * u, oy + 1.8 * u, 0.7 * u, 0.7 * u, [40, 40, 60, 255]);
                        if let Some(c) = worn(0) {
                            ui.rect(ox - 2.2 * u, oy - 0.2 * u, 4.4 * u, 1.6 * u, c);
                            ui.rect(ox - 2.2 * u, oy + 1.4 * u, 0.7 * u, 2.2 * u, dark(c));
                            ui.rect(ox + 1.5 * u, oy + 1.4 * u, 0.7 * u, 2.2 * u, dark(c));
                        }
                        // body and arms
                        let by = oy + 4.0 * u;
                        let top_c = worn(1).unwrap_or(shirt);
                        ui.rect(ox - 2.0 * u, by, 4.0 * u, 6.0 * u, top_c);
                        ui.rect(ox - 4.0 * u, by, 2.0 * u, 6.0 * u, dark(top_c));
                        ui.rect(ox + 2.0 * u, by, 2.0 * u, 6.0 * u, dark(top_c));
                        if worn(1).is_none() {
                            ui.rect(ox - 4.0 * u, by + 2.0 * u, 2.0 * u, 4.0 * u, skin);
                            ui.rect(ox + 2.0 * u, by + 2.0 * u, 2.0 * u, 4.0 * u, skin);
                        } else {
                            ui.rect(ox - 4.0 * u, by + 5.0 * u, 2.0 * u, u, skin);
                            ui.rect(ox + 2.0 * u, by + 5.0 * u, 2.0 * u, u, skin);
                        }
                        // legs and feet
                        let ly = by + 6.0 * u;
                        let leg_c = worn(2).unwrap_or(trousers);
                        ui.rect(ox - 2.0 * u, ly, 2.0 * u - 1.0, 6.0 * u, leg_c);
                        ui.rect(ox + 1.0, ly, 2.0 * u - 1.0, 6.0 * u, dark(leg_c));
                        let foot_c = worn(3).unwrap_or(shoes);
                        let fh = if worn(3).is_some() { 2.0 * u } else { u };
                        ui.rect(ox - 2.0 * u, ly + 6.0 * u - fh, 2.0 * u - 1.0, fh, foot_c);
                        ui.rect(ox + 1.0, ly + 6.0 * u - fh, 2.0 * u - 1.0, fh, dark(foot_c));
                    }
                    // crafting: the picked recipe's ingredients, an arrow, and the result to take
                    let kx = fx + fw + 14.0 * s;
                    let kw = mx + mw - pad - kx;
                    let r = &RECIPES[self.recipe_sel.min(RECIPES.len() - 1)];
                    let at_station = reachable(r.station);
                    let can = self.inv.can_craft(r) && at_station;
                    ui.text(kx, ty + 2.0 * s, SMALL, "Crafting", INK_DIM);
                    let ing = (50.0 * s).round();
                    let iy = ty + 26.0 * s;
                    for k in 0..4 {
                        let (x, y) = (kx + (k % 2) as f32 * ing, iy + (k / 2) as f32 * ing);
                        match r.ins.get(k) {
                            Some(&(item, need)) => {
                                let have = self.inv.count(item);
                                let hot = draw_slot(&mut ui, x, y, ing, Stack::new(item, need), false, true);
                                ui.outline(x + 1.0, y + 1.0, ing - 2.0, ing - 2.0, 2.0, if have >= need { INK_GOOD } else { INK_BAD });
                                if hot {
                                    tip = Some(Stack::new(item, 1));
                                }
                            }
                            None => {
                                draw_slot(&mut ui, x, y, ing, Stack::default(), false, true);
                            }
                        }
                    }
                    // arrow
                    let (ax, ay) = (kx + 2.0 * ing + 10.0 * s, iy + ing);
                    let ac = if can { INK } else { [150, 140, 124, 255] };
                    ui.rect(ax, ay - 3.0 * s, 20.0 * s, 6.0 * s, ac);
                    for k in 0..5 {
                        let h = (5 - k) as f32 * 4.0 * s;
                        ui.rect(ax + 20.0 * s + k as f32 * 3.0 * s, ay - h * 0.5, 3.0 * s, h, ac);
                    }
                    // result
                    let big = (66.0 * s).round();
                    let (ox, oy) = ((kx + kw - big).min(ax + 46.0 * s), iy + ing - big * 0.5);
                    let out = Stack::new(r.out.0, r.out.1);
                    let hot = ui.hover(ox, oy, big, big);
                    draw_slot(&mut ui, ox, oy, big, Stack::default(), false, true);
                    if can {
                        ui.outline(ox, oy, big, big, (3.0 * s).round().max(2.0), INK_GOOD);
                        draw_stack(&mut ui, ox, oy, big, out);
                    } else {
                        let ip = (big * 0.16).round();
                        ui.icon(ox + ip, oy + ip, big - 2.0 * ip, items::icon(r.out.0), 110);
                    }
                    if hot {
                        tip = Some(out);
                        if can && ui.ptr.clicked {
                            clicks.push(Action::CraftHand(self.shift));
                        }
                    }
                    let mut ly = iy + 2.0 * ing + 8.0 * s;
                    ui.text(kx, ly, BODY, items::name(r.out.0), INK);
                    ly += font.height(BODY) + 1.0 * s;
                    let missing: Vec<String> = r
                        .ins
                        .iter()
                        .filter(|&&(i, n)| self.inv.count(i) < n)
                        .map(|&(i, n)| format!("{} {}", n - self.inv.count(i), items::name(i)))
                        .collect();
                    let (line, col) = if !at_station {
                        (format!("Stand near a {}", items::name(r.station)), INK_BAD)
                    } else if !missing.is_empty() {
                        (format!("Need {}", missing.join(", ")), INK_BAD)
                    } else if r.station != AIR {
                        (format!("Ready at the {}", items::name(r.station)), INK_GOOD)
                    } else {
                        ("Ready to craft by hand".to_string(), INK_GOOD)
                    };
                    // long lines shrink to what fits
                    let mut line = line;
                    while font.width(SMALL, &line) > kw && line.len() > 4 {
                        line.truncate(line.len() - 4);
                        line.push_str("...");
                    }
                    ui.text(kx, ly, SMALL, &line, col);
                    ly += font.height(SMALL) + 2.0 * s;
                    ui.text(kx, ly, SMALL, "Click the result to take it", INK_DIM);
                    ui.text(kx, ly + font.height(SMALL), SMALL, "Shift+click crafts all you can", INK_DIM);

                    // backpack and hotbar
                    let gy = ty + top_h + 14.0 * s;
                    for i in items::HOTBAR..items::BAG_END {
                        let k = i - items::HOTBAR;
                        slot_at(&mut ui, &mut tip, &mut clicks, i, mx + pad + (k % 9) as f32 * slot, gy + (k / 9) as f32 * slot, false);
                    }
                    let hy2 = gy + 3.0 * slot + 10.0 * s;
                    for i in 0..items::HOTBAR {
                        slot_at(&mut ui, &mut tip, &mut clicks, i, mx + pad + i as f32 * slot, hy2, i == self.sel);
                    }
                    actions.append(&mut clicks);
                    if self.toast.1 > 0.0 {
                        ui.label_centered(cx, wy + mh + 10.0 * s, BODY, &self.toast.0, ACCENT);
                    }

                    // the stack on the mouse, or a tooltip for what is under it
                    let held = self.inv.held;
                    let (px, py) = (ui.ptr.x, ui.ptr.y);
                    if !held.is_empty() {
                        draw_stack(&mut ui, px - slot * 0.5, py - slot * 0.5, slot, held);
                        // clicking outside the windows throws it on the ground
                        if ui.ptr.clicked && !ui.hover(bx, wy, bw + gap + mw, mh) {
                            actions.push(Action::DropHeld);
                        }
                    } else if let Some(st) = tip {
                        draw_tip(&mut ui, st, wf, hf);
                    } else if let Some(text) = tip_text {
                        let tw = font.width(SMALL, text) + 20.0 * s;
                        let th = font.height(SMALL) + 12.0 * s;
                        ui.rect(px + 14.0 * s, py + 18.0 * s, tw, th, [16, 14, 22, 246]);
                        ui.outline(px + 14.0 * s, py + 18.0 * s, tw, th, 1.0, ACCENT);
                        ui.text(px + 24.0 * s, py + 24.0 * s, SMALL, text, TEXT);
                    }
                }
                Screen::Trade => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
                    let slot = (58.0 * s).round();
                    let pad = (20.0 * s).round();
                    let head = (46.0 * s).round();
                    let trader = self.trader();
                    let prof = trader.map_or(0, |i| self.mobs.profession(i));
                    let offers = trade::offers(prof);
                    let row = (slot * 0.95).round();
                    let mw = 9.0 * slot + 2.0 * pad;
                    let top_h = offers.len() as f32 * row + 8.0 * s;
                    let mh = 2.0 * head + top_h + 3.0 * slot + 10.0 * s + slot + pad;
                    let (mx, wy) = ((cx - mw * 0.5).round(), ((hf - mh) * 0.5).round());
                    draw_window(&mut ui, mx, wy, mw, mh);
                    ui.text(mx + pad, wy + 11.0 * s, HEAD, trade::PROFESSIONS[prof as usize], INK);
                    let gems = self.inv.count(items::GEM);
                    ui.text_right(mx + mw - pad, wy + 18.0 * s, SMALL, &format!("You have {gems} gems   ·   restocks each morning"), INK_DIM);
                    let mut tip: Option<Stack> = None;
                    let id = self.trading;
                    for (k, of) in offers.iter().enumerate() {
                        let y = wy + head + k as f32 * row;
                        let small = (slot * 0.82).round();
                        let mut x = mx + pad;
                        for &(it, n) in of.give.iter().filter(|g| g.1 > 0) {
                            let st = Stack::new(it, n);
                            if draw_slot(&mut ui, x, y, small, st, false, true) {
                                tip = Some(st);
                            }
                            x += small + 4.0 * s;
                        }
                        // an arrow, then what it buys
                        let ax = mx + pad + 2.0 * (small + 4.0 * s) + 8.0 * s;
                        let ay = y + small * 0.5;
                        ui.rect(ax, ay - 3.0 * s, slot * 0.7, 6.0 * s, INK_DIM);
                        ui.rect(ax + slot * 0.7, ay - 9.0 * s, 8.0 * s, 18.0 * s, INK_DIM);
                        let gx = ax + slot * 0.7 + 22.0 * s;
                        let st = Stack::new(of.get.0, of.get.1);
                        if draw_slot(&mut ui, gx, y, small, st, false, true) {
                            tip = Some(st);
                        }
                        let used = id.and_then(|id| self.trade_uses.get(&(id, k)).copied()).unwrap_or(0);
                        let left = of.uses.saturating_sub(used);
                        let can = trader.is_some() && left > 0 && of.give.iter().all(|&(it, n)| n == 0 || self.inv.count(it) >= n);
                        let label = if left == 0 { "Sold out".to_string() } else { format!("{left} left") };
                        let lx = gx + small + 16.0 * s;
                        ui.text(lx, y + small * 0.5 - font.height(SMALL) * 0.5, SMALL, &label, INK_DIM);
                        let (bw, bh) = ((150.0 * s).round(), (small * 0.75).round());
                        if ui.button(mx + mw - pad - bw, y + (small - bh) * 0.5, bw, bh, "Trade", can) {
                            actions.push(Action::Trade(k));
                        }
                    }
                    let iy2 = wy + head + top_h;
                    ui.text(mx + pad, iy2 + 11.0 * s, HEAD, "Inventory", INK);
                    let gy = iy2 + head;
                    for i in 0..items::BAG_END {
                        let k = i.wrapping_sub(items::HOTBAR);
                        let (x, y) = if i < items::HOTBAR {
                            (mx + pad + i as f32 * slot, gy + 3.0 * slot + 10.0 * s)
                        } else {
                            (mx + pad + (k % 9) as f32 * slot, gy + (k / 9) as f32 * slot)
                        };
                        let st = self.inv.slots[i];
                        if draw_slot(&mut ui, x, y, slot, st, i == self.sel, true) {
                            if !st.is_empty() {
                                tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                actions.push(Action::Slot(i, false));
                            } else if ui.ptr.rclicked {
                                actions.push(Action::Slot(i, true));
                            }
                        }
                    }
                    let held = self.inv.held;
                    if !held.is_empty() {
                        let (px, py) = (ui.ptr.x, ui.ptr.y);
                        draw_stack(&mut ui, px - slot * 0.5, py - slot * 0.5, slot, held);
                        if ui.ptr.clicked && !ui.hover(mx, wy, mw, mh) {
                            actions.push(Action::DropHeld);
                        }
                    } else if let Some(st) = tip {
                        draw_tip(&mut ui, st, wf, hf);
                    }
                    if trader.is_none() {
                        // they walked off (or worse)
                        actions.push(Action::Goto(Screen::Play));
                    }
                }
                Screen::Furnace => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
                    let slot = (58.0 * s).round();
                    let pad = (20.0 * s).round();
                    let head = (46.0 * s).round();
                    let mw = 9.0 * slot + 2.0 * pad;
                    let top_h = 3.0 * slot;
                    let mh = 2.0 * head + top_h + 3.0 * slot + 10.0 * s + slot + pad;
                    let (mx, wy) = ((cx - mw * 0.5).round(), ((hf - mh) * 0.5).round());
                    draw_window(&mut ui, mx, wy, mw, mh);
                    ui.text(mx + pad, wy + 11.0 * s, HEAD, "Furnace", INK);
                    ui.text_right(mx + mw - pad, wy + 18.0 * s, SMALL, "Ore or food on top, fuel below", INK_DIM);
                    let mut tip: Option<Stack> = None;
                    let f = self.furnaces.get(&self.chest_at).copied().unwrap_or_default();
                    // input over the fire over the fuel, an arrow, then the output
                    let col = mx + mw * 0.5 - slot * 2.2;
                    let (ix, iy) = (col, wy + head);
                    let (fx_, fy) = (col, wy + head + 2.0 * slot);
                    let (ox, oy) = (mx + mw * 0.5 + slot * 1.2, wy + head + slot);
                    let places = [(ix, iy), (fx_, fy), (ox, oy)];
                    for (i, &(x, y)) in places.iter().enumerate() {
                        let st = f.slots[i];
                        if draw_slot(&mut ui, x, y, slot, st, false, true) {
                            if !st.is_empty() {
                                tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                actions.push(Action::FurnaceSlot(i, false));
                            } else if ui.ptr.rclicked {
                                actions.push(Action::FurnaceSlot(i, true));
                            }
                        }
                    }
                    // the fire: how much of the last piece of fuel is left
                    let (fw, fh) = (slot * 0.4, slot * 0.7);
                    let (fxx, fyy) = (col + (slot - fw) * 0.5, wy + head + slot + (slot - fh) * 0.5);
                    ui.rect(fxx, fyy, fw, fh, [70, 60, 50, 255]);
                    if f.lit() && f.burn_max > 0.0 {
                        let k = (f.burn / f.burn_max).clamp(0.0, 1.0);
                        ui.rect(fxx, fyy + fh * (1.0 - k), fw, fh * k, [255, 150, 40, 255]);
                        ui.rect(fxx + fw * 0.25, fyy + fh * (1.0 - k * 0.7), fw * 0.5, fh * k * 0.7, [255, 220, 110, 255]);
                    }
                    // the arrow: how far the item has cooked
                    let (aw, ah) = (slot * 1.6, slot * 0.3);
                    let (ax, ay) = (col + slot * 1.3, oy + (slot - ah) * 0.5);
                    ui.rect(ax, ay, aw, ah, [70, 60, 50, 255]);
                    ui.rect(ax, ay, aw * (f.cook / furnace::COOK).clamp(0.0, 1.0), ah, [240, 240, 240, 255]);
                    ui.rect(ax + aw, ay - ah * 0.6, ah * 0.9, ah * 2.2, [70, 60, 50, 255]);
                    let iy2 = wy + head + top_h;
                    ui.text(mx + pad, iy2 + 11.0 * s, HEAD, "Inventory", INK);
                    let gy = iy2 + head;
                    for i in 0..items::BAG_END {
                        let k = i.wrapping_sub(items::HOTBAR);
                        let (x, y) = if i < items::HOTBAR {
                            (mx + pad + i as f32 * slot, gy + 3.0 * slot + 10.0 * s)
                        } else {
                            (mx + pad + (k % 9) as f32 * slot, gy + (k / 9) as f32 * slot)
                        };
                        let st = self.inv.slots[i];
                        if draw_slot(&mut ui, x, y, slot, st, i == self.sel, true) {
                            if !st.is_empty() {
                                tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                actions.push(Action::Slot(i, false));
                            } else if ui.ptr.rclicked {
                                actions.push(Action::Slot(i, true));
                            }
                        }
                    }
                    let held = self.inv.held;
                    if !held.is_empty() {
                        let (px, py) = (ui.ptr.x, ui.ptr.y);
                        draw_stack(&mut ui, px - slot * 0.5, py - slot * 0.5, slot, held);
                        if ui.ptr.clicked && !ui.hover(mx, wy, mw, mh) {
                            actions.push(Action::DropHeld);
                        }
                    } else if let Some(st) = tip {
                        draw_tip(&mut ui, st, wf, hf);
                    }
                }
                Screen::Chest => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
                    let slot = (58.0 * s).round();
                    let pad = (20.0 * s).round();
                    let head = (46.0 * s).round();
                    let mw = 9.0 * slot + 2.0 * pad;
                    let mh = 2.0 * head + 6.0 * slot + 10.0 * s + slot + pad;
                    let (mx, wy) = ((cx - mw * 0.5).round(), ((hf - mh) * 0.5).round());
                    draw_window(&mut ui, mx, wy, mw, mh);
                    ui.text(mx + pad, wy + 11.0 * s, HEAD, "Chest", INK);
                    ui.text_right(mx + mw - pad, wy + 18.0 * s, SMALL, "Shift+click sends a stack across", INK_DIM);
                    let mut tip: Option<Stack> = None;
                    let empty = [Stack::default(); items::CHEST_SLOTS];
                    let chest = self.chests.get(&self.chest_at).unwrap_or(&empty);
                    for (i, &st) in chest.iter().enumerate() {
                        let (x, y) = (mx + pad + (i % 9) as f32 * slot, wy + head + (i / 9) as f32 * slot);
                        if draw_slot(&mut ui, x, y, slot, st, false, true) {
                            if !st.is_empty() {
                                tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                actions.push(Action::ChestSlot(i, false));
                            } else if ui.ptr.rclicked {
                                actions.push(Action::ChestSlot(i, true));
                            }
                        }
                    }
                    let iy = wy + head + 3.0 * slot;
                    ui.text(mx + pad, iy + 11.0 * s, HEAD, "Inventory", INK);
                    let gy = iy + head;
                    for i in 0..items::BAG_END {
                        let k = i.wrapping_sub(items::HOTBAR);
                        let (x, y) = if i < items::HOTBAR {
                            (mx + pad + i as f32 * slot, gy + 3.0 * slot + 10.0 * s)
                        } else {
                            (mx + pad + (k % 9) as f32 * slot, gy + (k / 9) as f32 * slot)
                        };
                        let st = self.inv.slots[i];
                        if draw_slot(&mut ui, x, y, slot, st, i == self.sel, true) {
                            if !st.is_empty() {
                                tip = Some(st);
                            }
                            if ui.ptr.clicked {
                                actions.push(Action::Slot(i, false));
                            } else if ui.ptr.rclicked {
                                actions.push(Action::Slot(i, true));
                            }
                        }
                    }
                    let held = self.inv.held;
                    if !held.is_empty() {
                        let (px, py) = (ui.ptr.x, ui.ptr.y);
                        draw_stack(&mut ui, px - slot * 0.5, py - slot * 0.5, slot, held);
                        if ui.ptr.clicked && !ui.hover(mx, wy, mw, mh) {
                            actions.push(Action::DropHeld);
                        }
                    } else if let Some(st) = tip {
                        draw_tip(&mut ui, st, wf, hf);
                    }
                }
            }
        }

        let (fog_color, fog_start, fog_end) = if underwater {
            (mix3(srgb([3, 8, 20]), srgb([24, 70, 140]), daylight), 0.0, 26.0)
        } else {
            let base = mix3(srgb([6, 9, 20]), srgb([142, 196, 238]), daylight);
            // grey under rain clouds
            let grey = srgb([118, 124, 134]);
            let base = mix3(base, mix3(srgb([6, 8, 12]), grey, daylight), overcast * 0.85);
            // warm horizon while the sun is low
            let dusk = (1.0 - (sun_dir.y / 0.3).powi(2)).max(0.0) * 0.5;
            let dusk = dusk * (1.0 - overcast);
            let near = 1.0 - 0.35 * overcast;
            (mix3(base, srgb([232, 142, 92]), dusk), view_dist * 0.62 * near, (view_dist - 6.0) * (1.0 - 0.2 * overcast))
        };
        let t_build = t0.elapsed().as_secs_f32() * 1000.0 - t_world;
        let t1 = Instant::now();
        self.gfx.render(&Frame {
            view_proj: vp,
            fog_color,
            fog_start,
            fog_end,
            chunks: &chunks,
            // a little moonlight so nights stay playable
            daylight: 0.13 + 0.87 * daylight,
            // (under a roof of rock, animals and monsters are as dim as their surroundings)
            ent_light: (0.3 + 0.7 * (0.13 + 0.87 * daylight)) * if eye_sky == 0 { 0.22 + 0.78 * smooth(12.0, 44.0, eye.y) } else { 1.0 },
            eye_y: eye.y,
            lights: &lights,
            sky: &sky,
            ents: &ents,
            items: &item_verts,
            hand: item_verts.len() - hand_from,
            ui: &ui.verts,
        });
        drop(chunks);
        self.ents = ents;
        self.item_verts = item_verts;
        self.sky = sky;
        let t_render = t1.elapsed().as_secs_f32() * 1000.0;
        if std::env::var_os("CUBEWILD_PROF").is_some()
            && real_dt > 0.009
            && self.bench.as_ref().map_or(false, |b| b.t > 6.0)
        {
            println!("slow frame {:.1}ms: world {:.2} build {:.2} render {:.2}", real_dt * 1000.0, t_world, t_build, t_render);
        }

        // a click that did something clicks
        if (self.ptr.clicked || self.ptr.rclicked) && !actions.is_empty() {
            self.audio.play(Snd::Click, 0.6, 1.0);
        }
        self.ptr.clicked = false;
        self.ptr.rclicked = false;
        for a in actions {
            if self.apply(a) {
                self.save();
                el.exit();
                return;
            }
        }
    }
}

#[derive(Default)]
struct App {
    game: Option<Game>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Cubewild")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        self.game = Some(Game::new(window));
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = &mut self.game else { return };
        match event {
            WindowEvent::CloseRequested => {
                g.save();
                el.exit();
            }
            WindowEvent::Resized(sz) => g.resize(sz.width, sz.height),
            WindowEvent::Focused(false) => {
                if g.screen == Screen::Play && g.bench.is_none() {
                    g.goto(Screen::Pause);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                g.ptr.x = position.x as f32;
                g.ptr.y = position.y as f32;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                            g.shift = true;
                        }
                        if !event.repeat && g.bench.is_none() {
                            g.key_pressed(code);
                        }
                        if g.screen == Screen::Play {
                            g.keys.insert(code);
                        }
                    } else {
                        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                            g.shift = false;
                        }
                        g.keys.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                if g.screen != Screen::Play {
                    if button == MouseButton::Left {
                        g.ptr.down = down;
                        if down {
                            g.ptr.clicked = true;
                        }
                    } else if button == MouseButton::Right && down {
                        g.ptr.rclicked = true;
                    }
                    return;
                }
                match button {
                    MouseButton::Left => g.buttons[0] = down,
                    MouseButton::Right => {
                        g.buttons[1] = down;
                        if down {
                            g.place();
                        }
                    }
                    MouseButton::Middle if down => {
                        if let Some((p, _, _)) = g.target() {
                            let b = items::item_of(g.world.get_block(p.x, p.y, p.z));
                            if let Some(slot) = (0..items::HOTBAR).find(|&i| g.inv.hot(i) == b) {
                                g.sel = slot;
                            } else if let Some(from) = (items::HOTBAR..items::BAG_END).find(|&i| g.inv.slots[i].item == b) {
                                g.inv.slots.swap(from, g.sel);
                            }
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if y != 0.0 && g.screen == Screen::Play {
                    g.sel = (g.sel as i32 + if y < 0.0 { 1 } else { -1 }).rem_euclid(9) as usize;
                } else if y != 0.0 && g.screen == Screen::Inventory {
                    let max = if g.mode == Mode::Creative { items::ITEM_END as usize } else { RECIPES.len() } as i32 / RECIPE_COLS as i32;
                    g.recipe_scroll = (g.recipe_scroll as i32 + if y < 0.0 { 1 } else { -1 }).clamp(0, max) as usize;
                }
            }
            WindowEvent::RedrawRequested => g.frame(el),
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        let Some(g) = &mut self.game else { return };
        if let DeviceEvent::MouseMotion { delta } = event {
            if g.screen == Screen::Play && g.bench.is_none() {
                let k = 0.0022 * g.settings.sens;
                g.player.yaw -= delta.0 as f32 * k;
                g.player.pitch = (g.player.pitch - delta.1 as f32 * k).clamp(-1.55, 1.55);
            }
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = &self.game {
            g.window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run");
}
