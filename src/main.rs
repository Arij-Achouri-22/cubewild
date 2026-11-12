//! Cubewild — a small open-world voxel sandbox: gather, craft, build, hunt.

mod audio;
mod boat;
mod deep;
mod furnace;
mod fluid;
mod fx;
mod settle;
mod save;
mod gfx;
mod items;
mod loot;
mod lighting;
mod registry;
mod equipment;
mod farming;
mod crafting;
mod controls;
mod building;
mod combat;
mod persistence;
mod inventory_ui;
mod mobs;
mod path;
mod player;
mod projectile;
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
/// Whether an item's name holds every word of a search (ignoring case).
fn search_hit(item: Item, query: &str) -> bool {
    let name = items::name(item).to_lowercase();
    query.to_lowercase().split_whitespace().all(|w| name.contains(w))
}

/// The inventory's search box: a magnifier, the text and a blinking caret while it has
/// the keyboard. True if the pointer is over it.
fn draw_search(ui: &mut Ui, font: &Font, x: f32, y: f32, w: f32, h: f32, text: &str, on: bool, blink: bool) -> bool {
    let s = ui.s;
    let hot = ui.hover(x, y, w, h);
    ui.rect(x, y, w, h, if on { PAPER_LIGHT } else if hot { [200, 190, 170, 255] } else { INSET });
    ui.outline(x, y, w, h, if on { 2.0 } else { 1.0 }, if on { INK } else { PAPER_DARK });
    // a magnifying glass: a ring and a handle
    let (gx, gy, r) = (x + 8.0 * s, y + h * 0.5 - 6.0 * s, (9.0 * s).round());
    let t = (2.0 * s).round().max(1.0);
    ui.outline(gx, gy, r, r, t, INK_DIM);
    for k in 0..3 {
        let o = k as f32 * t;
        ui.rect(gx + r - t + o, gy + r - t + o, t * 1.5, t * 1.5, INK_DIM);
    }
    let tx = gx + r + 10.0 * s;
    let ty = y + (h - font.height(SMALL)) * 0.5;
    if text.is_empty() && !on {
        ui.text(tx, ty, SMALL, "Search items  ( / )", INK_DIM);
    } else {
        let wide = ui.text(tx, ty, SMALL, text, INK);
        if on && blink {
            ui.rect(tx.round() + wide + 1.0, ty + 2.0, t, font.height(SMALL) - 4.0, INK);
        }
    }
    hot
}

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
    for enchant in equipment::Enchant::ALL { let level = enchant.level(st); if level > 0 { v.push(format!("{} {}", enchant.name(), level)); } }
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
    /// an emberwisp's fireball: flies straight, burns, bursts on what it hits
    fire: bool,
    damage: i32,
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
    bindings: controls::Bindings,
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
        let mut s = Settings { view_dist: 14, fov: 72.0, sens: 1.0, vsync: true, fullscreen: false, show_fps: true, render_scale: 1.0, auto_scale: true, volume: 0.8, music: 0.5, bobbing: true, bindings: controls::Bindings::default() };
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
                _ => { s.bindings.load_line(k,v); }
            }
        }
        s
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
    Sign,
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
    SaveSign,
    ToggleGrid,
    GridSlot(usize, bool),
    GridCraft(bool),
    DragVisit(controls::Slot),
    Bind(usize),
    ResetBindings,
    ToggleEquipment,
    Repair,
    Enchant(equipment::Enchant),
    ToggleCraftable,
    /// click on (true) or away from (false) the search box
    Search(bool),
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
    edit_tick: u32,
    terrain_ready: Option<f32>,
}

struct SaveSnapshot {
    files: save::Snapshot,
    chunks: Vec<(Key, Arc<ChunkData>)>,
}

struct PendingSave {
    writer: save::Background,
    chunks: Vec<(Key, Arc<ChunkData>)>,
    started: Instant,
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
    last_checkpoint: Instant,
    pending_save: Option<PendingSave>,
    save_error: Option<String>,
    load_notice: Option<String>,
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
    /// the inventory's search box: what is typed, and whether it has the keyboard
    search: String,
    search_on: bool,
    ctrl: bool,
    recipe_tab: Cat,
    recipe_sel: usize,
    recipe_craftable: bool,
    equipment_open: bool,
    grid_open: bool,
    rebinding: Option<usize>,
    drag: Option<controls::Drag>,
    shift: bool,
    arrows: Vec<Arrow>,
    combat: combat::State,
    /// items lying in the world
    pickups: Pickups,
    chests: Chests,
    signs: building::Signs,
    sign_edit: String,
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
    /// developer hook (CUBEWILD_TEST_FLUID): pour water and lava and drop sand nearby
    test_fluid: bool,
    /// where the test yard is and how long ago it was poured (a map of it is printed)
    test_fluid_at: Option<(IVec3, f32)>,
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
    /// running water and lava
    fluids: fluid::Fluids,
    /// falling sand and gravel, wilting leaves
    settle: settle::Settle,
    /// seconds to the next paddle stroke sound
    paddle_t: f32,
    /// the villager being traded with (their village square and home)
    trading: Option<((i32, i32), usize)>,
    /// offers used up today: (villager, offer) -> times made
    trade_uses: trade::Uses,
    /// the generator of the world above
    over_ver: u8,
    /// every Ember Gate there is, in both worlds
    gates: Vec<Gate>,
    /// seconds spent standing in a gate, and whether the player has to step out of it
    /// first (just arrived)
    gate_t: f32,
    gate_lock: bool,
    /// on the way through a gate: where to come out (a known gate, or a place to build
    /// one), waiting for the land there to load
    arrival: Option<(IVec3, Option<Gate>)>,
    /// the Emberdeep region the player is in, and its haze (eased)
    region: deep::Region,
    haze: [f32; 3],
    gate_hum_t: f32,
    /// seconds since the game started, for animations
    anim: f32,
    /// seconds until smouldering rock burns the feet again
    smoulder_t: f32,
    /// seconds until a cactus can prick again
    cactus_t: f32,
}

fn data_dir(seed: u32) -> std::io::Result<PathBuf> {
    let dir = user_dir(false).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "cannot find the save folder"))?.join(format!("world-{seed}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Moves a world saved before the deep slate up by `LIFT`, once, after copying the
/// folder aside (`world-N.before-slate`). Its chunks are lifted as they load (see
/// `world::lift_saved`); here go the positions in the text files of the world above.
fn migrate(dir: &std::path::Path) {
    let mark = dir.join("height.txt");
    if mark.exists() {
        return;
    }
    if dir.join("player.txt").exists() {
        let backup = dir.with_file_name(format!("{}.before-slate", dir.file_name().and_then(|n| n.to_str()).unwrap_or("world")));
        if !backup.exists() {
            if let Err(e) = copy_dir(dir, &backup) {
                eprintln!("cubewild: cannot back up {}: {e} - not moving it", dir.display());
                return;
            }
        }
        let shift = |name: &str, col: usize, keep: &dyn Fn(&[&str]) -> bool| {
            let path = dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else { return };
            let out: String = text
                .lines()
                .map(|l| {
                    let mut t: Vec<String> = l.split(' ').map(str::to_string).collect();
                    let words: Vec<&str> = l.split(' ').collect();
                    if keep(&words) {
                        if let Some(v) = t.get(col).and_then(|v| v.parse::<f64>().ok()) {
                            t[col] = format!("{}", v + LIFT as f64);
                        }
                    }
                    t.join(" ") + "\n"
                })
                .collect();
            let _ = std::fs::write(&path, out);
        };
        let player = std::fs::read_to_string(dir.join("player.txt")).unwrap_or_default();
        let in_deep = player.lines().any(|l| l.trim() == "dim deep");
        // the player's place (unless they are in the Emberdeep) and their spawn
        shift("player.txt", 1, &|w: &[&str]| !in_deep && w.len() >= 5 && w[0].parse::<f32>().is_ok());
        shift("player.txt", 2, &|w: &[&str]| w.first() == Some(&"spawn"));
        // gates of the world above (the first number says which world)
        shift("gates.txt", 2, &|w: &[&str]| w.first() == Some(&"0"));
        for name in ["animals.txt"] {
            shift(name, 2, &|_| true);
        }
        for name in ["boats.txt", "chests.txt", "drops.txt", "furnaces.txt"] {
            shift(name, 1, &|_| true);
        }
        eprintln!("cubewild: moved {} onto the deep slate (a copy is in {})", dir.display(), backup.display());
    }
    let _ = std::fs::write(mark, "256\n");
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// View distance in the Emberdeep, whatever the setting: its fog is thick, and a world
/// of caverns is all walls to draw.
const DEEP_DIST: i32 = 8;

/// Where a dimension keeps its things: the world's folder above, its `deep` folder below.
fn dim_dir(save_dir: &Option<PathBuf>, deep: bool) -> Option<PathBuf> {
    let d = save_dir.as_ref()?;
    if !deep {
        return Some(d.clone());
    }
    Some(d.join("deep"))
}

/// The open space of a frame of cinderstone, in the plane of `axis` (0 = the frame
/// runs along x, 1 = along z), starting at the empty cell `q`. None unless it is
/// a closed rectangle, 2 to 10 wide and 3 to 10 high.
fn frame_opening(world: &World, q: IVec3, axis: u8) -> Option<(IVec3, IVec3)> {
    let side = if axis == 0 { IVec3::X } else { IVec3::Z };
    let mut seen = vec![q];
    let mut todo = vec![q];
    while let Some(c) = todo.pop() {
        for d in [side, -side, IVec3::Y, IVec3::NEG_Y] {
            let n = c + d;
            if seen.contains(&n) {
                continue;
            }
            match world.get_block(n.x, n.y, n.z) {
                CINDERSTONE => {}
                b if b == AIR || is_tuft(b) => {
                    seen.push(n);
                    if seen.len() > 100 {
                        return None;
                    }
                    todo.push(n);
                }
                _ => return None,
            }
        }
    }
    let lo = seen.iter().fold(q, |a, &b| a.min(b));
    let hi = seen.iter().fold(q, |a, &b| a.max(b));
    let size = hi - lo + IVec3::ONE;
    let (w, h) = (if axis == 0 { size.x } else { size.z }, size.y);
    ((2..=10).contains(&w) && (3..=10).contains(&h) && seen.len() as i32 == w * h).then_some((lo, hi))
}

/// An Ember Gate: the lowest corner of its veil, which way it faces, in which world.
#[derive(Clone, Copy, PartialEq)]
struct Gate {
    deep: bool,
    at: IVec3,
    axis: u8,
}

fn load_gates(save_dir: &Option<PathBuf>) -> Vec<Gate> {
    let Some(d) = save_dir else { return Vec::new() };
    std::fs::read_to_string(d.join("gates.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let v: Vec<i32> = l.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            (v.len() >= 5).then(|| Gate { deep: v[0] != 0, at: IVec3::new(v[1], v[2], v[3]), axis: (v[4] & 1) as u8 })
        })
        .collect()
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
        if let Err(e) = save::recover(&e.path()) {
            eprintln!("cubewild: cannot recover world {seed}: {e}");
        }
        let file = e.path().join("player.txt");
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            // An interrupted first checkpoint still needs to appear in the list.
            Err(_) if e.path().join(".checkpoint-pending").exists() => String::new(),
            Err(_) => continue,
        };
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
    mobs: Mobs,
    fluids: fluid::Fluids,
    settle: settle::Settle,
    trade_uses: trade::Uses,
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
    signs: building::Signs,
    furnaces: Furnaces,
    mode: Mode,
    save_dir: Option<PathBuf>,
    level: u32,
    xp: u32,
    weather: Weather,
    boats: boat::Boats,
    /// the generator of the world above (the Emberdeep has its own)
    over_ver: u8,
}

/// Opens the world of a seed: from its save if there is one (which also decides the
/// mode), otherwise new in `mode`. With `save` off nothing is read or written.
fn load_world(seed: u32, view_dist: i32, save: bool, mode: Mode) -> std::io::Result<Loaded> {
    let save_dir = if save { Some(data_dir(seed)?) } else { None };
    load_world_at(seed, view_dist, save_dir, mode)
}

fn load_world_at(seed: u32, view_dist: i32, save_dir: Option<PathBuf>, mode: Mode) -> std::io::Result<Loaded> {
    if let Some(d) = &save_dir {
        save::recover(d)?;
        migrate(d);
    }
    let saved = match save_dir.as_ref().map(|d| std::fs::read_to_string(d.join("player.txt"))) {
        Some(Ok(text)) => Some(text),
        None => None,
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Some(Err(e)) => return Err(e),
    };
    // a world keeps the terrain generator it was made with: saves from before there
    // were two do not name one, and are of the first
    let ver = match &saved {
        Some(text) => text.lines().find_map(|l| l.strip_prefix("gen ")).and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(1).clamp(1, GEN_LATEST),
        None => std::env::var("CUBEWILD_GEN").ok().and_then(|v| v.parse::<u8>().ok()).unwrap_or(GEN_LATEST).clamp(1, GEN_LATEST),
    };
    // a player who saved in the Emberdeep comes back there
    let in_deep = saved.as_ref().is_some_and(|t| t.lines().any(|l| l.trim() == "dim deep"));
    let dim_dir = dim_dir(&save_dir, in_deep);
    let world = if in_deep { World::new(seed, deep::DEEP_VER, view_dist.min(DEEP_DIST), dim_dir.clone()) } else { World::new(seed, ver, view_dist, save_dir.clone()) };
    if let Some(error) = &world.load_error {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error.clone()));
    }
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
                        if let (Ok(id), Ok(c)) = (id.parse::<Item>(), c.parse::<u32>()) {
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
                        if v.len() >= 3 && (v[0] as usize) < items::SLOTS && v[1] < items::ITEM_END as u32 && items::valid(v[1] as Item) && v[2] > 0 {
                            let item = v[1] as Item;
                            let slot = v[0] as usize;
                            let st = loot::parse_stack(item as u32, v[2], v.get(3).copied().unwrap_or(0)).unwrap_or_default();
                            // armour saved before there were four slots goes back in the bag
                            if slot < items::ARMOR || items::armor(item).is_some_and(|a| a.0 == slot - items::ARMOR) {
                                inv.slots[slot] = st;
                            } else {
                                inv.add(item, st.n);
                            }
                        }
                    }
                }
                Some("grid") => {
                    for t in parts { let v: Vec<u32> = t.split(':').filter_map(|x|x.parse().ok()).collect();
                        if v.len()==4 && v[0]<9 { inv.grid[v[0] as usize]=loot::parse_stack(v[1],v[2],v[3]).unwrap_or_default(); }
                    }
                }
                Some("held") => {
                    let v: Vec<u32> = parts.filter_map(|t| t.parse().ok()).collect();
                    if v.len() == 3 {
                        inv.held = loot::parse_stack(v[0], v[1], v[2]).unwrap_or_default();
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
    let pickups = dim_dir.as_ref().map_or_else(Pickups::default, |d| Pickups::load(&d.join("drops.txt")));
    let chests = dim_dir.as_ref().map_or_else(Chests::default, |d| loot::load_chests(&d.join("chests.txt")));
    let furnaces = dim_dir.as_ref().map_or_else(Furnaces::default, |d| furnace::load(&d.join("furnaces.txt")));
    let signs = dim_dir.as_ref().map(|d|building::load_signs(&d.join("signs.txt"))).transpose()?.unwrap_or_default();
    let boats = dim_dir.as_ref().map_or_else(boat::Boats::default, |d| boat::Boats::load(&d.join("boats.txt")));
    let (mobs, fluids, settle) = load_simulation(seed, in_deep, &dim_dir)?;
    let trade_uses = save_dir.as_ref().map(|d| trade::load_uses(&d.join("trades.txt"))).transpose()?.unwrap_or_default();
    Ok(Loaded { world, mobs, fluids, settle, trade_uses, player, inv, sel, health, food, air, time, spawn, pickups, chests, signs, furnaces, mode, save_dir, level, xp, weather, boats, over_ver: ver })
}

fn load_simulation(seed: u32, deep: bool, dir: &Option<PathBuf>) -> std::io::Result<(Mobs, fluid::Fluids, settle::Settle)> {
    let mut mobs = Mobs::new(seed as u64 * 7919 + if deep { 31 } else { 17 });
    let Some(dir) = dir else { return Ok((mobs, fluid::Fluids::default(), settle::Settle::default())) };
    mobs.load(&dir.join("animals.txt"))?;
    Ok((mobs, fluid::Fluids::load(&dir.join("fluids.txt"))?, settle::Settle::load(&dir.join("settle.txt"))?))
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
            edit_tick: 0,
            terrain_ready: None,
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

        let mut startup_error = None;
        let loaded = if bench.is_some() && env("CUBEWILD_BENCH_AUTOSAVE").is_some() {
            let dir = std::env::temp_dir().join(format!("cubewild-bench-save-{}", std::process::id()));
            println!("benchmark save folder: {}", dir.display());
            std::fs::create_dir_all(&dir).and_then(|_| load_world_at(seed, settings.view_dist, Some(dir), Mode::Survival))
        } else {
            load_world(seed, settings.view_dist, save, Mode::Survival)
        }.unwrap_or_else(|e| {
            startup_error = Some(format!("Could not open world {seed}: {e}"));
            load_world(seed, settings.view_dist, false, Mode::Survival).expect("an unsaved world needs no disk access")
        });
        let Loaded { world, mut mobs, fluids, settle, trade_uses, mut player, mut inv, sel, health, food, air, mut time, spawn, pickups, chests, signs, furnaces, mut mode, save_dir, level, xp, mut weather, boats, over_ver } = loaded;
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
        if let Some(item) = env("CUBEWILD_HOLD").and_then(|v| v.parse::<Item>().ok()).filter(|&i| items::valid(i)) {
            inv.slots[sel] = Stack::new(item, 1);
        }
        let first_screen = match env("CUBEWILD_SCREEN").as_deref() {
            _ if startup_error.is_some() => Screen::Title,
            Some("settings") => Screen::Settings,
            Some("controls") => Screen::Controls,
            Some("inventory") => Screen::Inventory,
            Some("play" | "chest" | "furnace") => Screen::Play,
            _ if bench.is_some() => Screen::Play,
            _ if titled => Screen::Title,
            _ => Screen::Pause,
        };
        mobs.ignore = mode == Mode::Creative;
        let grid_open=inv.grid.iter().any(|s| !s.is_empty());
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
            debug: env("CUBEWILD_DEBUG").is_some(),
            ready: false,
            drawn: (0, 0),
            save_dir,
            last_checkpoint: Instant::now(),
            pending_save: None,
            save_error: None,
            load_notice: startup_error,
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
            search: String::new(),
            search_on: false,
            ctrl: false,
            recipe_tab: Cat::All,
            recipe_sel: 0,
            recipe_craftable: false,
            equipment_open: false,
            grid_open,
            rebinding: None,
            drag: None,
            shift: false,
            arrows: Vec::new(),
            combat: combat::State::default(),
            pickups,
            chests,
            signs,
            sign_edit: String::new(),
            chest_at: IVec3::ZERO,
            item_verts: Vec::new(),
            got: Vec::new(),
            tally: (Vec::new(), 0.0),
            test_mobs: env("CUBEWILD_TEST_MOBS").is_some(),
            test_house: env("CUBEWILD_TEST_HOUSE").map(|v| v == "in"),
            test_fluid: env("CUBEWILD_TEST_FLUID").is_some(),
            test_fluid_at: None,
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
            block_colors: vec![[0; 4]; registry::LIMIT],
            ambient_t: 3.0,
            smoke_t: 0.0,
            landings: Vec::new(),
            biome_t: 0.0,
            equip: 0.0,
            bob: (0.0, 0.0),
            riding: None,
            boats,
            fluids,
            settle,
            paddle_t: 0.0,
            trading: None,
            trade_uses,
            over_ver,
            gates: Vec::new(),
            gate_t: 0.0,
            gate_lock: false,
            arrival: None,
            region: deep::Region::Wastes,
            haze: [0.3, 0.1, 0.06],
            gate_hum_t: 0.0,
            anim: 0.0,
            smoulder_t: 0.0,
            cactus_t: 0.0,
        };
        game.gates = load_gates(&game.save_dir);
        // developer hook: CUBEWILD_DIM=deep goes straight down into the Emberdeep (as if
        // through a gate at the spawn point)
        if env("CUBEWILD_DIM").as_deref() == Some("deep") && !game.world.is_deep() {
            game.cross_gate();
        }
        // developer hook: start a new world at once, as the title screen's buttons do
        if let Some(v) = env("CUBEWILD_NEW") {
            game.enter(fresh_seed(), if v == "creative" { Mode::Creative } else { Mode::Survival });
        }
        if env("CUBEWILD_AT").is_some() {
            game.unstick = false;
        }
        // developer hook: something typed in the inventory's search box
        if let Some(v) = env("CUBEWILD_SEARCH") {
            game.search = v;
        }
        game
    }

    /// Saves the current world and enters another: the saved one of that seed, or a
    /// new one in `mode`.
    fn enter(&mut self, seed: u32, mode: Mode) {
        if !self.save() {
            return;
        }
        let loaded = match load_world(seed, self.settings.view_dist, true, mode) {
            Ok(loaded) => loaded,
            Err(e) => {
                self.load_notice = Some(format!("Could not open world {seed}: {e}"));
                return;
            }
        };
        self.install(loaded);
        self.start_autosave();
        self.goto(Screen::Play);
    }

    /// Removes a saved world from the disk.
    fn delete_world(&mut self, seed: u32) {
        if seed == self.world.seed {
            // step out of it first: a new world that is not saved yet takes its place
            self.install(load_world(fresh_seed(), self.settings.view_dist, false, Mode::Survival).expect("an unsaved world needs no disk access"));
        }
        if let Some(dir) = user_dir(false).map(|d| d.join(format!("world-{seed}"))) {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                eprintln!("cubewild: cannot delete {}: {e}", dir.display());
            }
        }
        self.worlds = list_worlds();
    }

    fn save(&mut self) -> bool {
        self.settings.save();
        self.checkpoint()
    }

    fn goto(&mut self, screen: Screen) {
        // from the title screen, the other menus lead back to it
        let screen = if screen == Screen::Pause && self.at_title { Screen::Title } else { screen };
        if screen != Screen::Inventory {
            self.search.clear();
            self.search_on = false;
        }
        if screen == Screen::Play {
            if self.world.load_error.is_some() || (self.save_error.is_some() && !self.checkpoint()) {
                return;
            }
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
            self.combat=combat::State::default();
            self.keys.clear();
            self.mining = None;
        }
        if self.screen != screen && matches!(self.screen, Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign) {
            // Leaving any item screen returns the mouse stack, including to Play.
            self.inv.stow_held();
            let grid = std::mem::take(&mut self.inv.grid);
            for st in grid { self.throw(st); }
            self.drag = None;
            let left = std::mem::take(&mut self.inv.held);
            self.throw(left);
        }
        if screen != Screen::Play && matches!(self.screen, Screen::Play | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign) {
            self.start_autosave();
        }
        if matches!(self.screen, Screen::Settings | Screen::Controls) && self.screen != screen {
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

    /// View distance in use: the setting, or less while the frame rate needs help.
    fn apply_dist(&mut self) {
        let d = if self.settings.auto_scale { self.settings.view_dist.min(self.auto_cur) } else { self.settings.view_dist };
        let d = if self.world.is_deep() { d.min(DEEP_DIST) } else { d };
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
        if self.world.is_deep() {
            // whoever dies below wakes up at home
            if !self.switch_world(false) {
                return;
            }
            self.arrival = None;
        }
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
        if !pl.on_ground || !is_target(ground) || is_water(ground) {
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
        let deep = self.world.is_deep();
        let wet = if self.weather.dry || self.weather.snowy || deep { 0.0 } else { self.weather.rain };
        let outside = self.weather.open_sky(eye);
        let rain = wet * if outside { 1.0 } else if sky > 0 { 0.45 } else { 0.0 };
        let cave = if deep { 0.9 } else if sky == 0 { smooth(LIFT as f32 + 56.0, LIFT as f32 + 20.0, eye.y) * 0.8 } else { 0.0 };
        if deep {
            // ash and embers drifting in the warm air
            for _ in 0..2 {
                if self.rng.f() < dt * 6.0 {
                    let o = Vec3::new(self.rng.range(-10.0, 10.0), self.rng.range(-3.0, 6.0), self.rng.range(-10.0, 10.0));
                    if self.rng.f() < 0.3 {
                        self.particles.ember(&mut self.rng, eye + o);
                    } else {
                        let dark = self.rng.f() < 0.5;
                        self.particles.smoke(&mut self.rng, eye + o, dark);
                    }
                }
            }
        }
        let under = if self.player.eye_in_water(&self.world) { 1.0 } else { 0.0 };
        self.audio.beds(rain, cave, under);
    }

    /// A stone yard in front of the player (towards +z) with water poured from a post, lava
    /// on the ground, sand left hanging in the air and a sapling (CUBEWILD_TEST_FLUID).
    /// False while the chunks are still loading.
    fn pour_test_fluids(&mut self) -> bool {
        let c = self.player.pos.floor().as_ivec3();
        let Some(g) = self.world.surface_y(c.x, c.z) else { return false };
        if [(-9, -2), (9, -2), (-9, 16), (9, 16)].iter().any(|d| !self.world.is_loaded(c.x + d.0, c.z + d.1)) {
            return false;
        }
        let mut edits = Vec::new();
        for dz in -2..16 {
            for dx in -9..=9 {
                for y in g - 2..g + 9 {
                    edits.push((IVec3::new(c.x + dx, y, c.z + dz), if y <= g { STONE } else { AIR }));
                }
            }
        }
        let at = |dx: i32, dy: i32, dz: i32| IVec3::new(c.x + dx, g + dy, c.z + dz);
        edits.push((at(-4, 1, 9), STONE));
        edits.push((at(-4, 2, 9), WATER));
        edits.push((at(5, 1, 10), LAVA));
        for dy in 4..7 {
            edits.push((at(1, dy, 5), if dy == 5 { GRAVEL } else { SAND }));
        }
        edits.push((at(-1, 0, 4), GRASS));
        edits.push((at(-1, 1, 4), SAPLING));
        self.world.set_many(&edits);
        self.player.pos.y = (g + 1) as f32;
        self.test_fluid_at = Some((IVec3::new(c.x, g, c.z), 0.0));
        true
    }

    /// The test yard as text, a layer at a time (CUBEWILD_TEST_FLUID): W still water,
    /// 7..1 running water, L still lava, 3..1 running lava, S sand, G gravel, T trunk,
    /// * leaves, # stone, . air.
    fn print_test_yard(&self, o: IVec3) {
        let mut out = format!("TEST YARD at {o} ({} falling)\n", self.settle.falling.len());
        for dy in 1..=7 {
            out += &format!("y+{dy}\n");
            for dz in -2..16 {
                for dx in -9..=9 {
                    let b = self.world.get_block(o.x + dx, o.y + dy, o.z + dz);
                    out.push(match b {
                        WATER => 'W',
                        LAVA => 'L',
                        b if (WATER_FLOW..WATER_FLOW_END).contains(&b) => (b'7' - (b - WATER_FLOW) as u8) as char,
                        b if (LAVA_FLOW..LAVA_FLOW_END).contains(&b) => (b'3' - (b - LAVA_FLOW) as u8) as char,
                        SAND => 'S',
                        GRAVEL => 'G',
                        b if is_log(b) => 'T',
                        b if is_leaves(b) => '*',
                        b if is_sapling(b) => 'p',
                        AIR => '.',
                        _ => '#',
                    });
                }
                out.push('\n');
            }
        }
        eprint!("{out}");
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
        for (i, st) in [Stack::new(COBBLE, 64), Stack::new(items::COAL, 12), Stack { item: items::IRON_PICK, n: 1, wear: 40, enchants: 0 }, Stack::new(items::BREAD, 3)].into_iter().enumerate() {
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
        self.world.set_block(ox + 3, y0 + 2, oz + 3, LAMP);
        // torches on the west and north walls
        self.world.set_block(ox + 1, y0 + 2, oz + 2, WALL_TORCH);
        self.world.set_block(ox + 3, y0 + 2, oz + 1, WALL_TORCH + 2);
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
        self.anim = (self.anim + real_dt) % 3600.0;
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
        if !self.poll_autosave() {
            self.goto(Screen::Pause);
        }
        let pc = ((self.player.pos.x.floor() as i32) >> 4, (self.player.pos.z.floor() as i32) >> 4);
        let t0 = Instant::now();
        if self.screen != Screen::Title {
            self.world.update(&self.gfx, pc);
        }
        for (key, saved) in std::mem::take(&mut self.world.activated) {
            self.fluids.chunk_loaded(&self.world, key, saved);
        }
        if self.world.load_error.is_some() && matches!(self.screen, Screen::Play | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign) {
            self.goto(Screen::Pause);
        }
        self.last_jump += real_dt;
        self.swing -= dt;
        let t_world = t0.elapsed().as_secs_f32() * 1000.0;
        if self.arrival.is_some() {
            self.arrive();
        }
        self.ready = self.arrival.is_none()
            && self.world.area_loaded(self.player.pos.x, self.player.pos.z, PLAYER_HW);
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
        let mut input = Input {
            forward: (self.settings.bindings.pressed(0,&self.keys) as i32 - self.settings.bindings.pressed(1,&self.keys) as i32) as f32,
            right: (self.settings.bindings.pressed(3,&self.keys) as i32 - self.settings.bindings.pressed(2,&self.keys) as i32) as f32,
            jump: self.settings.bindings.pressed(4,&self.keys),
            sneak: self.settings.bindings.pressed(6,&self.keys),
            // too hungry to run
            sprint: self.settings.bindings.pressed(5,&self.keys) && (self.food > 6.0 || self.mode == Mode::Creative),
        };
        if !playing {input=Input::default();}
        if playing && self.buttons[1] && self.inv.hot(self.sel)==items::SHIELD {
            self.combat.blocking+=dt;input.sneak=true;input.sprint=false;
        } else {self.combat.blocking=0.0;}
        if let Some((slot,time))=&mut self.combat.bow {
            if playing && self.buttons[1] && *slot==self.sel && self.inv.hot(self.sel)==items::BOW {*time=(*time+dt).min(1.0);input.sneak=true;input.sprint=false;}
            else {self.combat.bow=None;}
        }
        if let Some(b) = &mut self.bench {
            b.t += real_dt;
            if b.terrain_ready.is_none() && self.world.chunks.get(&pc).is_some_and(|s| s.mesh.is_some()) {
                b.terrain_ready = Some(b.t);
            }
            let edit_bench = std::env::var_os("CUBEWILD_BENCH_EDITS").is_some();
            input = Input { forward: if edit_bench { 0.0 } else { 1.0 }, ..Default::default() };
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
                    "BENCH {w}x{h} dist={} frames={} avg_fps={:.1} low1pct_fps={:.1} worst_ms={:.2} frames_over_8.3ms={} chunks_drawn={} quads={} terrain_ready_ms={:.1} chunks_loaded={} chunks_meshed={}",
                    self.world.radius,
                    f.len(),
                    1.0 / avg,
                    1.0 / low_avg,
                    f[0] * 1000.0,
                    over,
                    self.drawn.0,
                    self.drawn.1,
                    b.terrain_ready.unwrap_or(-0.001) * 1000.0,
                    self.world.chunks.len(),
                    self.world.chunks.values().filter(|s| s.mesh.is_some()).count(),
                );
                if self.save_dir.is_some() { self.checkpoint(); }
                el.exit();
                return;
            }
        }
        // the world keeps running while the inventory is open, but pauses in the menus
        let simulate = self.ready && matches!(self.screen, Screen::Play | Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign);
        if simulate {
            let was = self.time;
            self.time = (self.time + dt / DAY_SECS).rem_euclid(1.0);
            if self.time < was {
                // a new day: the villagers have restocked
                self.trade_uses.clear();
            }
            // what the edits since the last frame set going: running water and lava,
            // falling sand and gravel, wilting leaves
            let touched = self.world.take_ready_updates();
            let creative = self.mode == Mode::Creative;
            for (p, b) in self.fluids.update(&mut self.world, dt, &touched) {
                // (running water carries off torches and plants, which drop)
                let drop = items::drop_for(b, AIR);
                if drop != AIR && !creative {
                    self.pickups.pop(&mut self.rng, p.as_vec3() + 0.5, Stack::new(drop, 1));
                }
            }
            let body = (self.player.pos, self.player.height());
            let settled = self.settle.update(&mut self.world, &mut self.rng, dt, &touched, body);
            for (at, b) in settled.landed {
                self.audio.play_var(Snd::Place(audio::surface(b)), at, 0.9);
            }
            for (at, b) in settled.broke {
                if !creative {
                    self.pickups.pop(&mut self.rng, at, Stack::new(items::item_of(b), 1));
                }
            }
            for (c, b) in settled.wilted {
                let at = c.as_vec3() + 0.5;
                let col = self.block_color(b);
                self.particles.chips(&mut self.rng, c, col, 4);
                if !creative {
                    if self.rng.f() < 0.06 {
                        self.pickups.pop(&mut self.rng, at, Stack::new(items::item_of(sapling_of(b)), 1));
                    } else if self.rng.f() < 0.03 {
                        self.pickups.pop(&mut self.rng, at, Stack::new(items::STICK, 1));
                    }
                }
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
            if self.test_fluid && self.pour_test_fluids() {
                self.test_fluid = false;
            }
            if let Some((o, t)) = self.test_fluid_at {
                self.test_fluid_at = Some((o, t + dt));
                if t + dt >= 10.0 {
                    self.test_fluid_at = None;
                    self.print_test_yard(o);
                }
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
            let _bites = self.mobs.update(&self.world, self.player.pos, input.sneak && !self.player.flying, night, lure, dt);
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
                self.arrows.push(Arrow { pos, vel, life: 5.0, hostile: true, fire: false, damage: 3 });
            }
            for (pos, vel) in self.mobs.fireballs.drain(..) {
                self.arrows.push(Arrow { pos, vel, life: 4.0, hostile: true, fire: true, damage: 4 });
                self.audio.play_var(Snd::Fizz, pos, 0.7);
            }
            if self.mode == Mode::Creative {
                (self.health, self.food, self.air) = (MAX_HEALTH, MAX_FOOD, MAX_AIR);
            } else if self.bench.is_none() {
                let attacks=std::mem::take(&mut self.mobs.attacks);
                for (from,amount) in attacks {self.damage_from(amount,from);}
                self.survive(&input, dt);
                if self.health <= 0.0 {
                    self.die();
                }
            }
            // standing in an Ember Gate: a few seconds of shimmer, then across
            let pc = self.player.pos.floor().as_ivec3();
            let in_gate = (0..2).any(|k| is_gate(self.world.get_block(pc.x, pc.y + k, pc.z)));
            if in_gate && self.riding.is_none() {
                if !self.gate_lock {
                    self.gate_t += dt;
                    self.gate_hum_t -= dt;
                    if self.gate_hum_t <= 0.0 {
                        self.gate_hum_t = 1.0;
                        self.audio.play(Snd::GateHum, 0.5 + self.gate_t * 0.15, 0.8 + self.gate_t * 0.15);
                    }
                    let need = if self.mode == Mode::Creative { 0.8 } else { 3.0 };
                    if self.gate_t >= need {
                        self.cross_gate();
                    }
                }
            } else {
                self.gate_lock = false;
                self.gate_t = (self.gate_t - dt * 2.0).max(0.0);
                self.gate_hum_t = 0.0;
            }
            // furnaces burn on wherever they are
            for f in self.furnaces.values_mut() {
                if f.lit() || f.cook > 0.0 || (!f.slots[furnace::INPUT].is_empty() && !f.slots[furnace::FUEL].is_empty()) {
                    f.tick(dt);
                }
            }
            // weather (none in the Emberdeep: there the region sets the haze)
            self.biome_t -= dt;
            let deep = self.world.is_deep();
            if self.biome_t <= 0.0 {
                self.biome_t = 1.0;
                if deep {
                    self.region = deep::region(self.world.seed, self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32);
                } else if self.world.ver >= 2 {
                    use worldgen::Biome as B;
                    let b = worldgen::column(self.world.seed, self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32).biome;
                    self.weather.snowy = matches!(b, B::SnowyTaiga | B::SnowyPlains | B::FrozenOcean | B::FrozenRiver | B::Peaks);
                    self.weather.dry = matches!(b, B::Desert | B::Badlands | B::Savanna);
                }
            }
            if !deep && self.weather.update(&mut self.rng, dt) {
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
        let deep = self.world.is_deep();
        let overcast = if deep { 0.0 } else { self.weather.rain };
        let daylight = if deep { 0.0 } else { (daylight * (1.0 - 0.45 * overcast) + self.weather.flash * 0.7).min(1.0) };
        if simulate {
            // arrows fly in an arc and stick the first animal or block they meet
            let mut arrows = std::mem::take(&mut self.arrows);
            let mut bursts: Vec<Vec3> = Vec::new();
            arrows.retain_mut(|a| {
                if !a.fire {
                    a.vel.y -= 20.0 * dt;
                }
                a.life -= dt;
                let step = a.vel * dt;
                if let Some((hit, impact)) = projectile::trace(&self.world, &self.mobs, &self.player, a.hostile, a.pos, step) {
                    match hit {
                        projectile::Hit::Player => {
                            if self.bench.is_none() {
                                self.damage_from(a.damage as f32,a.pos);
                            }
                            if a.fire {
                                bursts.push(impact);
                            }
                        }
                        projectile::Hit::Mob(i) => {
                            let at = self.mobs.list[i].pos;
                            let kind = self.mobs.list[i].kind;
                            self.mob_struck(i, at);
                            if let Some(drops) = self.mobs.hit(i, a.pos, a.damage) {
                                self.loot(drops, at);
                                self.kill_xp(kind, at);
                            }
                            self.hit_flash = 0.18;
                        }
                        projectile::Hit::Block => {
                            if a.fire {
                                bursts.push(impact);
                            } else {
                                self.audio.play_var(Snd::ArrowHit, impact, 0.6);
                            }
                        }
                    }
                    return false;
                }
                a.pos += step;
                if a.fire && a.life <= 0.0 {
                    bursts.push(a.pos);
                }
                a.life > 0.0
            });
            self.arrows = arrows;
            for at in bursts {
                // a fireball bursts in a puff of sparks and smoke
                self.audio.play_var(Snd::Fizz, at, 1.0);
                self.particles.dust(&mut self.rng, at, [255, 90, 40], 14, 0.6);
                for _ in 0..6 {
                    self.particles.smoke(&mut self.rng, at, false);
                }
            }
            // crops ripen a stage at a time, at random
            self.grow_acc += dt * 600.0;
            while self.grow_acc >= 1.0 {
                self.grow_acc -= 1.0;
                let p = self.player.pos + Vec3::new(self.rng.range(-24.0, 24.0), self.rng.range(-8.0, 8.0), self.rng.range(-24.0, 24.0));
                let c = p.floor().as_ivec3();
                let b = self.world.get_block(c.x, c.y, c.z);
                if farming::crop(b).is_some() || farming::moisture(b).is_some() {
                    farming::tick(&mut self.world, c, daylight, self.weather.rain > 0.25, self.rng.next());
                } else if is_sapling(b) && self.rng.f() < 0.35 {
                    // a sapling grows into its tree, if there is room
                    if let Some(tree) = settle::grow(&self.world, c, b, &mut self.rng) {
                        self.world.set_many(&tree);
                    }
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

        if simulate && self.arrival.is_none() && self.save_dir.is_some() && self.pending_save.is_none()
            && self.last_checkpoint.elapsed().as_secs() >= 30 && !self.start_autosave()
        {
            self.goto(Screen::Pause);
        }

        // Repeatable stress test for editing a corner shared by four chunk meshes.
        if let Some(bench) = &mut self.bench {
            if std::env::var_os("CUBEWILD_BENCH_EDITS").is_some() && bench.t > 6.0 && self.ready {
                let tick = ((bench.t - 6.0) * 6.0) as u32;
                if tick != bench.edit_tick {
                    bench.edit_tick = tick;
                    let p = self.player.pos.floor().as_ivec3();
                    let (x, z) = ((p.x >> 4) * CW, (p.z >> 4) * CW);
                    let y = (p.y + 3).clamp(1, CH - 2);
                    self.world.set_block(x, y, z, if tick % 2 == 0 { AIR } else { PLANKS });
                }
            }
        }

        let t_sim = t0.elapsed().as_secs_f32() * 1000.0 - t_world;

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
        for a in self.arrows.iter().filter(|a| a.fire) {
            // a fireball: a glowing core in a ragged shell of flame
            let spin = Mat4::from_translation(a.pos - eye) * Mat4::from_rotation_y(self.anim * 9.0) * Mat4::from_rotation_x(self.anim * 7.0);
            push_box(&mut ents, &spin, Vec3::splat(-0.12), Vec3::splat(0.12), [255, 230, 150], gfx::EMISSIVE);
            push_box(&mut ents, &spin, Vec3::new(-0.22, -0.16, -0.16), Vec3::new(0.22, 0.16, 0.16), [240, 70, 30], gfx::EMISSIVE);
            push_box(&mut ents, &spin, Vec3::new(-0.16, -0.22, -0.16), Vec3::new(0.16, 0.22, 0.16), [200, 40, 30], 200);
        }
        for a in self.arrows.iter().filter(|a| !a.fire) {
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
        let mut gate_lights: Vec<Vec3> = Vec::new();
        // (every pixel weighs every light: below, the warm glow lights the caverns and lava
        // shines by itself, so only a few lights are needed)
        let max_lights = if deep { 6 } else { gfx::MAX_LIGHTS };
        let clock = self.anim;
        for (q, b, _) in self.world.lights_near(eye, 46.0) {
            let rel = q.as_vec3() - eye;
            if is_gate(b) {
                // the veil: a black sheet with crimson light welling up through drifting
                // smoke, drawn from both sides
                let (u, v) = if b == GATE { (Vec3::X * 0.5, Vec3::Y * 0.5) } else { (Vec3::Z * 0.5, Vec3::Y * 0.5) };
                let c = rel + Vec3::splat(0.5);
                let across = if b == GATE { Vec3::Z } else { Vec3::X };
                let along = if b == GATE { q.x } else { q.z } as f32;
                fx::quad(&mut ents, c, u, v, [14, 4, 6], 235);
                // a 3 x 3 weave of glow and smoke cells, each its own swirl of the flow
                for j in 0..3 {
                    for i in 0..3 {
                        let (fu, fv) = ((i as f32 - 1.0) / 3.0, (j as f32 - 1.0) / 3.0);
                        let (px, py) = (along + 0.5 + fu, q.y as f32 + 0.5 + fv);
                        let swirl = (px * 1.7 + clock * 0.9 + (py * 1.3 - clock * 0.6).sin() * 1.4).sin() * (py * 1.9 - clock * 1.3 + (px * 0.8).cos()).cos();
                        let w = swirl * 0.5 + 0.5;
                        let mid = c + u * (fu * 2.0) + v * (fv * 2.0);
                        let (su, sv) = (u / 3.0, v / 3.0);
                        // (on both faces of the black core, so it looks the same from either side)
                        for side in [-1.0f32, 1.0] {
                            if w > 0.47 {
                                // crimson glow, hottest where the flow gathers
                                let k = ((w - 0.47) / 0.53).min(1.0);
                                let col = [(84.0 + 140.0 * k * k) as u8, (4.0 + 26.0 * k * k * k) as u8, (12.0 + 16.0 * k) as u8];
                                fx::quad(&mut ents, mid + across * (0.03 * side), su, sv, col, gfx::EMISSIVE);
                            } else {
                                // black smoke drifting over it
                                let k = 1.0 - w / 0.47;
                                fx::quad(&mut ents, mid + across * (0.05 * side), su, sv, [(34.0 + 26.0 * (1.0 - k)) as u8, 5, 9], (120.0 + 100.0 * k) as u8);
                            }
                        }
                    }
                }
                if lights.len() < max_lights && !gate_lights.iter().any(|g| g.distance(c) < 2.6) {
                    gate_lights.push(c);
                    lights.push([c.x, c.y, c.z, 9.0]);
                }
                continue;
            }
            if lights.len() < max_lights && !(deep && b == LAVA) {
                let c = match b {
                    b if (WALL_TORCH..WALL_TORCH_END).contains(&b) => {
                        let (dx, dz) = SIDE_DIR[(b - WALL_TORCH) as usize];
                        rel + Vec3::new(0.5 + dx as f32 * 0.2, 0.85, 0.5 + dz as f32 * 0.2)
                    }
                    _ => rel + Vec3::new(0.5, 0.7, 0.5),
                };
                lights.push([c.x, c.y, c.z, match b {
                    LAMP => 15.0,
                    SUNSHARD | GLOWCAP => 8.0,
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
        let mut landings = std::mem::take(&mut self.landings);
        landings.clear();
        if !deep {
            self.weather.track(&self.world, eye);
            self.weather.draw(&mut ents, eye, &mut landings);
        }
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
        self.settle.draw(&self.world, &mut item_verts, eye);
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
        if !underwater && !deep {
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
        if self.gate_t > 0.0 || self.arrival.is_some() {
            // the gate's glow fills the view as it takes you
            let k = if self.arrival.is_some() { 1.0 } else { (self.gate_t / 3.0).min(1.0) };
            let w = (self.anim * 3.0).sin() * 0.5 + 0.5;
            // (crimson light, darkening to black as it takes hold)
            let deep_k = k * k;
            ui.rect(0.0, 0.0, wf, hf, [((150.0 + 60.0 * w) * (1.0 - 0.75 * deep_k)) as u8, (10.0 + 12.0 * w) as u8, (18.0 + 8.0 * w) as u8, (k * 220.0) as u8]);
            if self.arrival.is_some() {
                ui.label_centered(cx, cy, BODY, "Crossing...", TEXT);
            }
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
        if self.ready && !matches!(self.screen, Screen::Inventory | Screen::Chest | Screen::Furnace | Screen::Trade | Screen::Sign) {
            for i in 0..9 {
                draw_slot(&mut ui, hx + i as f32 * slot, hy, slot, self.inv.slots[i], i == self.sel, false);
            }
            if self.screen==Screen::Play {
                let ready=1.0-(self.primary_cd/combat::attack_period(self.inv.hot(self.sel))).clamp(0.0,1.0);
                ui.rect(cx-20.0*s,cy+18.0*s,40.0*s,3.0*s,[40,40,40,200]);
                ui.rect(cx-20.0*s,cy+18.0*s,40.0*s*ready,3.0*s,[230,230,230,230]);
                if let Some((_,t))=self.combat.bow {ui.rect(cx-20.0*s,cy+24.0*s,40.0*s*combat::bow_power(t),4.0*s,[110,190,255,255]);}
                if self.combat.blocking>=0.25 {ui.label_centered(cx,cy+30.0*s,SMALL,"Blocking",TEXT);}
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
            if deep && (self.inv.has(items::CLOCK) || self.inv.has(items::COMPASS)) {
                // no sun, no north: the needles spin
                info.push("The needles spin wildly".into());
            } else if self.inv.has(items::CLOCK) {
                let hours = (self.time * 24.0 + 6.0).rem_euclid(24.0);
                info.push(format!("{:02}:{:02}", hours as u32, (hours.fract() * 60.0) as u32));
            }
            if self.inv.has(items::COMPASS) && !deep {
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
                if deep {
                    format!("Emberdeep  {}", self.region.name())
                } else if self.world.ver >= 2 {
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
                    let row=34.0*s;
                    let pw=700.0*s; let ph=650.0*s;
                    let px=cx-pw*0.5; let py=cy-ph*0.5;
                    let top=ui.panel(px,py,pw,ph,"Controls");
                    ui.text(px+24.0*s,top+12.0*s,SMALL,"Click a binding, then press a key. Esc cancels.",DIM);
                    for (i,(name,_)) in controls::ACTIONS.iter().enumerate() {
                        let y=top+42.0*s+i as f32*row;
                        ui.text(px+24.0*s,y+5.0*s,BODY,name,TEXT);
                        let label=if self.rebinding==Some(i) {"Press a key...".into()} else {format!("{:?}",self.settings.bindings.0[i])};
                        if ui.button(px+400.0*s,y,270.0*s,30.0*s,&label,true) {actions.push(Action::Bind(i));}
                    }
                    let y=top+42.0*s+11.0*row;
                    for (i,line) in ["Left click attacks / mines; right click uses / charges a bow", "Middle click picks blocks. Shift+click moves stacks.", "Left drag divides stacks; right drag places one per slot.", "1-9 / wheel select slots. F3 debug. F11 fullscreen. Esc pause."].into_iter().enumerate() {
                        ui.text(px+24.0*s,y+i as f32*24.0*s,SMALL,line,DIM);
                    }
                    if ui.button(cx-210.0*s,py+ph-48.0*s,200.0*s,32.0*s,"Reset bindings",true) {actions.push(Action::ResetBindings);}
                    if ui.button(cx+10.0*s,py+ph-48.0*s,200.0*s,32.0*s,"Back",true) {actions.push(Action::Goto(Screen::Pause));}
                }
                Screen::Inventory => self.draw_inventory(&mut ui,&font,cx,cy,wf,hf,&mut actions),
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
                            if self.drag.is_some() { actions.push(Action::DragVisit(controls::Slot::Player(i))); }
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
                            if self.drag.is_some() { actions.push(Action::DragVisit(controls::Slot::Furnace(i))); }
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
                            if self.drag.is_some() { actions.push(Action::DragVisit(controls::Slot::Player(i))); }
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
                Screen::Sign => {
                    ui.rect(0.0,0.0,wf,hf,[6,10,16,160]);
                    let pw=600.0*s;let ph=330.0*s;let px=cx-pw*0.5;let py=cy-ph*0.5;
                    let y=ui.panel(px,py,pw,ph,"Edit sign");
                    ui.text(px+24.0*s,y+16.0*s,SMALL,"Type up to 60 characters. Enter saves; Esc cancels.",DIM);
                    for (i,line) in self.sign_edit.as_bytes().chunks(15).enumerate() {ui.text_centered(cx,y+(60.0+i as f32*28.0)*s,BODY,std::str::from_utf8(line).unwrap_or(""),ACCENT);}
                    if ui.button(cx-100.0*s,py+ph-54.0*s,200.0*s,32.0*s,"Done",true) {actions.push(Action::SaveSign);}
                }
                Screen::Chest => {
                    ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
                    let slot = (58.0 * s).round();
                    let pad = (20.0 * s).round();
                    let head = (46.0 * s).round();
                    let mw = 9.0 * slot + 2.0 * pad;
                    let halves=building::chest_halves(&self.world,self.chest_at);
                    let rows=halves.len()*3;
                    let mh = 2.0 * head + (rows+4) as f32 * slot + 10.0*s + pad;
                    let (mx, wy) = ((cx - mw * 0.5).round(), ((hf - mh) * 0.5).round());
                    draw_window(&mut ui, mx, wy, mw, mh);
                    ui.text(mx + pad, wy + 11.0 * s, HEAD, if rows==6 {"Large Chest"} else {"Chest"}, INK);
                    ui.text_right(mx + mw - pad, wy + 18.0 * s, SMALL, "Shift+click sends a stack across", INK_DIM);
                    let mut tip: Option<Stack> = None;
                    let chest:Vec<Stack>=(0..halves.len()*items::CHEST_SLOTS).map(|i|self.chest_stack(i).unwrap_or_default()).collect();
                    for (i, &st) in chest.iter().enumerate() {
                        let (x, y) = (mx + pad + (i % 9) as f32 * slot, wy + head + (i / 9) as f32 * slot);
                        if draw_slot(&mut ui, x, y, slot, st, false, true) {
                            if self.drag.is_some() { actions.push(Action::DragVisit(controls::Slot::Chest(i))); }
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
                    let iy = wy + head + rows as f32 * slot;
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
                            if self.drag.is_some() { actions.push(Action::DragVisit(controls::Slot::Player(i))); }
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

        if self.screen==Screen::Play {
            if let Some((p,_,_))=self.target() {if building::is_sign(self.world.get_block(p.x,p.y,p.z)) {if let Some(text)=self.signs.get(&p) {
                for (i,line) in text.as_bytes().chunks(15).enumerate() {ui.label_centered(cx,cy+70.0*s+i as f32*25.0*s,BODY,std::str::from_utf8(line).unwrap_or(""),TEXT);}
            }}}
        }
        if let Some(message) = self.save_error.as_deref().or(self.world.load_error.as_deref()).or(self.load_notice.as_deref()) {
            let mut lines = vec![String::new()];
            for word in message.split_whitespace() {
                let line = lines.last_mut().unwrap();
                if !line.is_empty() && font.width(SMALL, &format!("{line} {word}")) > wf - 40.0 * s {
                    lines.push(word.to_string());
                } else {
                    if !line.is_empty() { line.push(' '); }
                    line.push_str(word);
                }
            }
            let line_h = font.height(SMALL) + 4.0 * s;
            let top = hf - lines.len() as f32 * line_h - 20.0 * s;
            ui.rect(0.0, top, wf, hf - top, [31, 8, 8, 245]);
            for (i, line) in lines.iter().enumerate() {
                ui.label_centered(cx, top + 10.0 * s + i as f32 * line_h, SMALL, line, [255, 184, 158, 255]);
            }
        }

        // the Emberdeep's haze takes the colour of the region
        let want = srgb(self.region.fog());
        let k = 1.0 - (-dt * 1.5).exp();
        for i in 0..3 {
            self.haze[i] += (want[i] - self.haze[i]) * k;
        }
        let (fog_color, fog_start, fog_end) = if deep && !underwater {
            (self.haze, 10.0, (view_dist - 6.0).min(84.0))
        } else if underwater {
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
        let t_build = t0.elapsed().as_secs_f32() * 1000.0 - t_world - t_sim;
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
            ent_light: if deep { 0.62 } else { (0.3 + 0.7 * (0.13 + 0.87 * daylight)) * if eye_sky == 0 { 0.22 + 0.78 * smooth(LIFT as f32 + 12.0, LIFT as f32 + 44.0, eye.y) } else { 1.0 } },
            ambient: if deep { 0.42 } else { 0.0 },
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
        let work_ms = t0.elapsed().as_secs_f32() * 1000.0;
        if std::env::var_os("CUBEWILD_PROF").is_some()
            && work_ms > if self.bench.is_some() { 9.0 } else { 22.0 }
            && self.bench.as_ref().is_none_or(|b| b.t > 6.0)
        {
            println!("slow frame work {work_ms:.1}ms: world {t_world:.2} simulation {t_sim:.2} build {t_build:.2} render {t_render:.2}");
        }

        // a click that did something clicks
        if (self.ptr.clicked || self.ptr.rclicked) && !actions.is_empty() {
            self.audio.play(Snd::Click, 0.6, 1.0);
        }
        let drag_released=self.ptr.released || self.ptr.rreleased;
        self.ptr.released=false; self.ptr.rreleased=false;
        self.ptr.clicked = false;
        self.ptr.rclicked = false;
        for a in actions {
            if self.apply(a) {
                if self.save() {
                    el.exit();
                }
                return;
            }
        }
        if drag_released {self.finish_drag();}
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
                if g.save() {
                    el.exit();
                } else {
                    g.goto(Screen::Pause);
                }
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
                if g.screen==Screen::Sign && event.state==ElementState::Pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape)=>g.goto(Screen::Play),
                        PhysicalKey::Code(KeyCode::Enter)=>{g.apply(Action::SaveSign);},
                        PhysicalKey::Code(KeyCode::Backspace)=>{g.sign_edit.pop();},
                        _=>if let Some(t)=event.text.as_deref() {for c in t.chars().filter(|c|c.is_ascii() && !c.is_ascii_control()) {if g.sign_edit.len()<60 {g.sign_edit.push(c);}}},
                    }
                    return;
                }
                if g.screen == Screen::Inventory && event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        let ctrl = g.ctrl;
                        if !g.search_on && (code == KeyCode::Slash || (code == KeyCode::KeyF && ctrl)) {
                            g.search_on = true;
                            return;
                        }
                        if g.search_on && g.type_search(code, event.text.as_deref()) {
                            return;
                        }
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                            g.shift = true;
                        }
                        if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight) {
                            g.ctrl = true;
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
                        if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight) {
                            g.ctrl = false;
                        }
                        g.keys.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                if g.screen != Screen::Play {
                    if down && !g.shift && !g.inv.held.is_empty() && matches!(button,MouseButton::Left|MouseButton::Right) {
                        g.drag=Some(controls::Drag {right:button==MouseButton::Right,slots:Vec::new()});
                    }
                    if button == MouseButton::Left {
                        g.ptr.down = down;
                        if !down { g.ptr.released=true; }
                        if down {
                            g.ptr.clicked = true;
                        }
                    } else if button == MouseButton::Right {
                        g.ptr.rdown=down;
                        if down {g.ptr.rclicked=true;} else {g.ptr.rreleased=true;}
                    }
                    return;
                }
                match button {
                    MouseButton::Left => g.buttons[0] = down,
                    MouseButton::Right => {
                        g.buttons[1] = down;
                        if !down {g.release_bow();}
                        if down {
                            g.place();
                        }
                    }
                    MouseButton::Middle if down => {
                        g.pick_block();
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

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn a_full_inventory_and_cursor_stack_survive_a_checkpoint_and_reload() {
        let root = save::tests::TestDir::new("held-stack");
        std::fs::write(root.0.join("height.txt"), "256\n").unwrap();
        for held in [Stack::new(PLANKS, 99), Stack { item: items::IRON_PICK, n: 1, wear: 37, enchants: 0 }] {
            let mut inv = Inventory::new();
            inv.slots[..items::BAG_END].fill(Stack::new(DIRT, 99));
            inv.held = held;
            let checkpoint = save::Checkpoint::begin(&root.0).unwrap();
            std::fs::write(checkpoint.dir().join("player.txt"), format!("0.5 150 0.5 0 0\n{}gen 3\n", inv.save_stacks())).unwrap();
            checkpoint.commit().unwrap();
            let loaded = load_world_at(1, 3, Some(root.0.clone()), Mode::Survival).unwrap();
            assert_eq!(loaded.inv.slots, inv.slots);
            assert_eq!(loaded.inv.held, held);
        }
    }

    #[test]
    fn crafting_grid_and_enchanted_cursor_survive_a_full_inventory_checkpoint() {
        let root=save::tests::TestDir::new("grid-equipment");
        std::fs::write(root.0.join("height.txt"),"256\n").unwrap();
        let mut inv=Inventory::new();inv.slots[..items::BAG_END].fill(Stack::new(DIRT,99));
        inv.grid[0]=Stack::new(LOG,7);
        inv.grid[4]=Stack {wear:43,enchants:9,..Stack::new(items::IRON_PICK,1)};
        inv.held=Stack {wear:22,enchants:8,..Stack::new(items::SHIELD,1)};
        let checkpoint=save::Checkpoint::begin(&root.0).unwrap();
        std::fs::write(checkpoint.dir().join("player.txt"),format!("0.5 150 0.5 0 0\n{}gen 3\n",inv.save_stacks())).unwrap();
        checkpoint.commit().unwrap();
        let loaded=load_world_at(1,3,Some(root.0.clone()),Mode::Survival).unwrap();
        assert_eq!(loaded.inv.slots,inv.slots);assert_eq!(loaded.inv.held,inv.held);assert_eq!(loaded.inv.grid,inv.grid);
    }

    #[test]
    fn player_saves_without_a_cursor_record_still_load() {
        let root = save::tests::TestDir::new("old-inventory");
        std::fs::write(root.0.join("height.txt"), "256\n").unwrap();
        std::fs::write(root.0.join("player.txt"), "0.5 150 0.5 0 0\nslots 0:7:12:0\ngen 3\n").unwrap();
        let loaded = load_world_at(1, 3, Some(root.0.clone()), Mode::Survival).unwrap();
        assert_eq!(loaded.inv.slots[0], Stack::new(PLANKS, 12));
        assert!(loaded.inv.held.is_empty());
    }

    #[test]
    fn dimension_simulation_and_world_trade_limits_survive_checkpoints() {
        let root = save::tests::TestDir::new("dimension-state");
        std::fs::write(root.0.join("height.txt"), "256\n").unwrap();
        let uses = trade::Uses::from([((((-4, 7), 2), 0), 12)]);
        for (deep, block, y) in [(false, SAND, 20.0), (true, GRAVEL, 45.0)] {
            let checkpoint = save::Checkpoint::begin(&root.0).unwrap();
            let d = checkpoint.dir();
            trade::save_uses(&uses, &d.join("trades.txt")).unwrap();
            let dim = if deep { "dim deep\n" } else { "" };
            std::fs::write(d.join("player.txt"), format!("0.5 {y} 0.5 0 0\ngen 3\n{dim}")).unwrap();
            let d = if deep { d.join("deep") } else { d };
            let mut world = World::flat(|_, _, _| None);
            world.touched.push((IVec3::new(1, y as i32, 1), LOG));
            world.stage_chunks(&d).unwrap();
            std::fs::write(d.join("fluids.txt"), "timers 0.12 0.4\nwater 1 10 1\nlava 3 11 3\n").unwrap();
            let mut settle = settle::Settle::default();
            settle.falling.push(settle::Falling { pos: Vec3::new(1.5, y, 1.5), vy: -5.0, block });
            settle.save(&d.join("settle.txt")).unwrap();
            checkpoint.commit().unwrap();
            let loaded = load_world_at(1, 3, Some(root.0.clone()), Mode::Survival).unwrap();
            assert_eq!(loaded.world.is_deep(), deep);
            assert_eq!(loaded.settle.falling.len(), 1);
            assert_eq!(loaded.settle.falling[0].block, block);
            assert_eq!(loaded.trade_uses, uses);
            assert_eq!(loaded.world.touched, vec![(IVec3::new(1, y as i32, 1), LOG)]);
            let copy = root.0.join("loaded-fluids.txt");
            loaded.fluids.save(&copy).unwrap();
            assert_eq!(std::fs::read_to_string(copy).unwrap(), "timers 0.12 0.4\nwater 1 10 1\nlava 3 11 3\n");
        }
        let (_, _, above) = load_simulation(1, false, &Some(root.0.clone())).unwrap();
        assert_eq!(above.falling[0].block, SAND, "the other dimension keeps its own simulation");
        let other = load_world_at(2, 3, None, Mode::Survival).unwrap();
        assert!(other.trade_uses.is_empty(), "trade limits belong to their world");
    }

    #[test]
    fn damaged_simulation_state_prevents_loading_and_is_kept_for_recovery() {
        let root = save::tests::TestDir::new("damaged-simulation");
        std::fs::write(root.0.join("height.txt"), "256\n").unwrap();
        std::fs::write(root.0.join("player.txt"), "0.5 150 0.5 0 0\ngen 3\n").unwrap();
        let text = "timers NaN 0\nwater 1 10 1\n";
        std::fs::write(root.0.join("fluids.txt"), text).unwrap();
        assert!(load_world_at(1, 3, Some(root.0.clone()), Mode::Survival).is_err());
        assert_eq!(std::fs::read_to_string(root.0.join("fluids.txt")).unwrap(), text);
    }

    /// A cinderstone frame in the x-y plane at z = 0, its opening `w` wide and `h` high
    /// from (0, 10); `gap` leaves one block of a side out.
    fn framed(w: i32, h: i32, gap: bool) -> World {
        World::flat(move |x, y, z| {
            let side = x == -1 || x == w;
            let edge = y == 9 || y == 10 + h;
            let inside_x = (-1..=w).contains(&x);
            let inside_y = (9..=10 + h).contains(&y);
            if z == 0 && inside_x && inside_y && (side || edge) && !(gap && x == w && y == 11) {
                Some(CINDERSTONE)
            } else if y >= 10 {
                Some(AIR)
            } else {
                None
            }
        })
    }

    #[test]
    fn a_closed_frame_has_an_opening_and_a_broken_one_does_not() {
        let w = framed(2, 3, false);
        let (lo, hi) = frame_opening(&w, IVec3::new(0, 10, 0), 0).expect("a 2x3 opening");
        assert_eq!((lo, hi), (IVec3::new(0, 10, 0), IVec3::new(1, 12, 0)));
        // the same frame seen the other way round is no frame
        assert!(frame_opening(&w, IVec3::new(0, 10, 0), 1).is_none());
        // a gap in the side lets the opening leak out
        assert!(frame_opening(&framed(2, 3, true), IVec3::new(0, 10, 0), 0).is_none());
        // bigger frames work, too small ones do not
        assert!(frame_opening(&framed(5, 7, false), IVec3::new(2, 12, 0), 0).is_some());
        assert!(frame_opening(&framed(1, 3, false), IVec3::new(0, 10, 0), 0).is_none());
    }

    #[test]
    fn an_old_save_is_moved_up_once_and_kept_aside() {
        let root = std::env::temp_dir().join(format!("cubewild-migrate-{}", std::process::id()));
        let dir = root.join("world-77");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(dir.join("deep")).unwrap();
        let w = |f: &str, t: &str| std::fs::write(dir.join(f), t).unwrap();
        w("player.txt", "10.5 65 -3.25 1 0\nslots 0:3:5:0\nspawn -63.5 78 0.5\ngen 3\n");
        w("gates.txt", "0 -96 74 10 0\n1 -6 88 -2 0\n");
        w("chests.txt", "-890 12 4160 0:23:7:0\n");
        w("animals.txt", "3 1.5 70 2.5 0 0 0 1 0 1 6\n");
        std::fs::write(dir.join("deep").join("chests.txt"), "5 40 5 0:23:7:0\n").unwrap();
        migrate(&dir);
        let r = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        assert!(r("player.txt").starts_with("10.5 129 -3.25 1 0\n"), "{}", r("player.txt"));
        assert!(r("player.txt").contains("spawn -63.5 142 0.5"));
        assert_eq!(r("gates.txt"), "0 -96 138 10 0\n1 -6 88 -2 0\n");
        assert!(r("chests.txt").starts_with("-890 76 4160 "));
        assert!(r("animals.txt").starts_with("3 1.5 134 2.5"));
        // the Emberdeep keeps its heights; a copy of the old world is kept aside
        assert_eq!(std::fs::read_to_string(dir.join("deep").join("chests.txt")).unwrap(), "5 40 5 0:23:7:0\n");
        assert!(root.join("world-77.before-slate").join("player.txt").exists());
        // and it happens only once
        migrate(&dir);
        assert!(r("player.txt").starts_with("10.5 129 "));
        // a player who saved in the Emberdeep keeps their place there
        let dir2 = root.join("world-78");
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(dir2.join("player.txt"), "1 50 1 0 0\nspawn 0 70 0\ndim deep\n").unwrap();
        migrate(&dir2);
        let p = std::fs::read_to_string(dir2.join("player.txt")).unwrap();
        assert!(p.starts_with("1 50 1 0 0\n") && p.contains("spawn 0 134 0"), "{p}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
