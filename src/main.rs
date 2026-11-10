//! Cubewild — a small open-world voxel sandbox: gather, craft, build, hunt.

mod gfx;
mod items;
mod mobs;
mod player;
mod textures;
mod ui;
mod world;

use gfx::{ChunkMesh, EntVert, Frame, Gfx};
use glam::{IVec3, Mat3, Mat4, Quat, Vec3, Vec4};
use items::{Cat, Inventory, Item, Stack, CATS, RECIPES};
use mobs::{push_box, Mobs, Rng};
use player::{collides, raycast, raycast_with, Input, Player, PLAYER_H, PLAYER_HW};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use ui::{Font, Pointer, Ui, ACCENT, BODY, DIM, HEAD, SMALL, TEXT, TITLE};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};
use world::*;

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

struct Arrow {
    pos: Vec3,
    vel: Vec3,
    life: f32,
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
        let mut s = Settings { view_dist: 14, fov: 72.0, sens: 1.0, vsync: true, fullscreen: false, show_fps: true, render_scale: 1.0, auto_scale: true };
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
            "view_dist={}\nfov={}\nsens={}\nvsync={}\nfullscreen={}\nshow_fps={}\nrender_scale={}\nauto_scale={}\n",
            self.view_dist, self.fov, self.sens, self.vsync, self.fullscreen, self.show_fps, self.render_scale, self.auto_scale
        );
        if let Err(e) = std::fs::write(&p, text) {
            eprintln!("cubewild: failed to save settings: {e}");
        }
    }
}

// ---------------------------------------------------------------- game

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Play,
    Pause,
    Settings,
    Controls,
    Inventory,
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
    Scale(f32),
    ToggleAuto,
    /// take the crafting result; true = craft as many as possible
    CraftHand(bool),
    ToggleCraftable,
    Tab(Cat),
    PickRecipe(usize),
    /// slot index, right button
    Slot(usize, bool),
    Sort,
    Respawn,
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
    /// water block being fished, seconds waited, seconds until the bite
    fishing: Option<(IVec3, f32, f32)>,
    grow_acc: f32,
    /// automatic view distance: current limit in chunks, timers, back-off, last change was a raise
    auto_cur: i32,
    auto_t: f32,
    auto_hold: f32,
    auto_backoff: f32,
    auto_raised: bool,
}

fn data_dir(seed: u32) -> Option<PathBuf> {
    let dir = user_dir(false)?.join(format!("world-{seed}"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cubewild: cannot create {}: {e} - the world will not be saved", dir.display());
        return None;
    }
    Some(dir)
}

fn find_spawn(seed: u32) -> Vec3 {
    for i in 0..400 {
        let x = i * 9;
        let h = height_at(seed, x, 0);
        if h > SEA + 2 && h < 78 {
            return Vec3::new(x as f32 + 0.5, h as f32 + 2.0, 0.5);
        }
    }
    Vec3::new(0.5, 100.0, 0.5)
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
        let seed = env("CUBEWILD_SEED").and_then(|v| v.parse().ok()).unwrap_or(2026);
        let save_dir = if bench.is_some() { None } else { data_dir(seed) };

        if settings.fullscreen || std::env::args().any(|a| a == "--fullscreen") {
            settings.fullscreen = true;
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let mut gfx = pollster::block_on(Gfx::new(window.clone(), bench.is_none() && settings.vsync));
        gfx.set_scale(env("CUBEWILD_SCALE").and_then(|v| v.parse().ok()).unwrap_or(settings.render_scale));
        println!("cubewild: rendering on {}", gfx.adapter_name);
        let (font, atlas) = Font::bake(ui_scale(gfx.size().1));
        gfx.upload_font(&atlas);

        let world = World::new(seed, settings.view_dist, save_dir.clone());
        let mut spawn = find_spawn(seed);
        let mut player = Player::new(spawn);
        let mut inv = Inventory::new();
        let mut sel = 0;
        let (mut health, mut food, mut air, mut time) = (MAX_HEALTH, MAX_FOOD, MAX_AIR, 0.04f32);
        if let Some(text) = save_dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("player.txt")).ok()) {
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
        if let Some(t) = env("CUBEWILD_TIME").and_then(|v| v.parse::<f32>().ok()) {
            time = t.rem_euclid(1.0);
        }
        if bench.is_some() {
            player.flying = true;
            player.pitch = -0.25;
        }
        // developer hooks for checking the menus without playing
        if env("CUBEWILD_TEST_ITEMS").is_some() {
            for (i, n) in [(TORCH, 16), (LOG, 12), (PLANKS, 7), (COBBLE, 5), (DIRT, 31), (SAND, 1), (items::STICK, 3), (items::MEAT, 4), (items::HIDE, 3), (items::STONE_PICK, 1), (items::SPEAR, 1), (LEAVES, 9), (items::STONE_AXE, 1), (items::WOOD_PICK, 1), (GLASS, 2), (STONE_BRICK, 8), (LEATHER, 1), (BRICK, 4), (items::COAL, 9), (IRON_ORE, 6), (items::IRON_INGOT, 5), (items::CRYSTAL, 4), (items::COOKED_MEAT, 3), (WORKBENCH, 1), (FURNACE, 1), (BED, 1), (LAMP, 2), (items::IRON_PICK, 1), (items::IRON_SWORD, 1), (CACTUS, 2), (SANDSTONE, 6), (BIRCH_LOG, 8), (GRAVEL, 7), (CLAY, 6), (GOLD_ORE, 4), (items::FIBER, 12), (items::BERRIES, 5), (MUSHROOM, 4), (FLOWER_RED, 3), (items::FLINT, 2), (items::LEATHER_CHEST, 1), (items::IRON_CHEST, 1), (items::IRON_HELMET, 1), (items::GOLD_LEGS, 1), (items::CRYSTAL_BOOTS, 1), (items::BOW, 1), (items::ARROW, 24), (items::FISHING_ROD, 1), (items::BUCKET, 1), (items::IRON_HOE, 1), (items::SEEDS, 9), (items::IRON_SHOVEL, 1), (items::SHEARS, 1), (items::COMPASS, 1), (items::CLOCK, 1), (items::WHEAT, 6), (items::CRYSTAL_SWORD, 1), (items::GOLD_INGOT, 8), (items::STEW, 1), (THATCH, 6)] {
                inv.add(i, n);
            }
        }
        let first_screen = match env("CUBEWILD_SCREEN").as_deref() {
            Some("settings") => Screen::Settings,
            Some("controls") => Screen::Controls,
            Some("inventory") => Screen::Inventory,
            _ if bench.is_some() => Screen::Play,
            _ => Screen::Pause,
        };
        Game {
            window,
            gfx,
            font: Rc::new(font),
            world,
            player,
            mobs: Mobs::new(seed as u64 * 7919 + 17),
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
            unstick: false,
            recipe_scroll: 0,
            recipe_tab: Cat::All,
            recipe_sel: 0,
            recipe_craftable: false,
            shift: false,
            arrows: Vec::new(),
            fishing: None,
            grow_acc: 0.0,
            auto_cur: 20,
            auto_t: -4.0,
            auto_hold: 0.0,
            auto_backoff: 25.0,
            auto_raised: false,
        }
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
        if let Err(e) = std::fs::write(d.join("player.txt"), s) {
            eprintln!("cubewild: failed to save player: {e}");
        }
    }

    fn save(&self) {
        self.world.save_all();
        self.save_player();
        self.settings.save();
    }

    fn goto(&mut self, screen: Screen) {
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
            self.auto_t = -3.0;
        } else {
            self.window.set_cursor_grab(CursorGrabMode::None).ok();
            self.window.set_cursor_visible(true);
            self.buttons = [false; 2];
            self.keys.clear();
            if self.screen == Screen::Inventory {
                self.inv.stow_held();
            }
            self.mining = None;
            if self.screen == Screen::Play {
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

    /// Spends one use of the held tool and reports it if it breaks.
    fn wear_held(&mut self) {
        let item = self.inv.hot(self.sel);
        if self.inv.wear_tool(self.sel) {
            self.say(format!("Your {} broke", items::name(item)));
        }
    }

    fn loot(&mut self, meat: u32, hide: u32) {
        self.give(items::MEAT, meat);
        self.give(items::HIDE, hide);
        self.say(if meat > 0 { format!("+{meat} Raw Meat   +{hide} Hide") } else { format!("+{hide} Hide") });
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
        self.toast = (msg, 2.4);
    }

    /// Adds items to the inventory, warning when it is full.
    fn give(&mut self, item: Item, n: u32) {
        if self.inv.add(item, n) > 0 {
            self.say("Inventory full".into());
        }
    }

    /// Damage from blows and falls, softened by armour.
    fn strike(&mut self, amount: f32) {
        self.hurt(amount * self.inv.armor_factor());
    }

    fn hurt(&mut self, amount: f32) {
        self.health -= amount;
        self.hurt_flash = 0.35;
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

    /// Hunger, healing, fall damage and drowning.
    fn survive(&mut self, input: &Input, dt: f32) {
        if self.player.impact > 13.5 {
            let d = (self.player.impact - 13.5) * 0.95;
            self.strike(d);
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
        if let Some((i, t)) = self.mobs.pick(o, d, reach) {
            if block.map_or(true, |b| t < b.2) {
                self.mining = None;
                if self.primary_cd <= 0.0 {
                    self.primary_cd = 0.4;
                    self.hit_flash = 0.18;
                    let dead = self.mobs.hit(i, self.player.pos, damage);
                    self.wear_held();
                    if let Some((meat, hide)) = dead {
                        self.loot(meat, hide);
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
        if progress >= items::break_time(b, tool) {
            self.mining = None;
            if self.world.set_block(&self.gfx, p.x, p.y, p.z, AIR) {
                let drop = items::drop_for(b, tool);
                match b {
                    GRAVEL if self.rng.f() < 0.25 => self.give(items::FLINT, 1),
                    TALL_GRASS if tool != items::SHEARS => {
                        if self.rng.f() < 0.6 {
                            self.give(items::FIBER, 1);
                        }
                        if self.rng.f() < 0.3 {
                            self.give(items::SEEDS, 1);
                        }
                    }
                    WHEAT_3 => {
                        self.give(items::WHEAT, 1);
                        let seeds = 1 + (self.rng.f() < 0.5) as u32;
                        self.give(items::SEEDS, seeds);
                    }
                    BERRY_BUSH => self.give(drop, 2),
                    _ => self.give(drop, 1),
                }
                if is_leaves(b) && self.rng.f() < 0.3 {
                    self.give(items::STICK, 1);
                }
                if drop == AIR && items::tier_needed(b) > 0 {
                    let need = if items::tier_needed(b) >= 3 { "an iron" } else { "a stone" };
                    self.say(format!("{} needs {need} pickaxe or better", items::name(b)));
                }
                if !is_plant(b) && b != TORCH {
                    self.wear_held();
                }
                // torches and plants standing on the block come loose
                let above = self.world.get_block(p.x, p.y + 1, p.z);
                if (above == TORCH || is_plant(above)) && self.world.set_block(&self.gfx, p.x, p.y + 1, p.z, AIR) {
                    self.give(items::drop_for(above, tool), 1);
                }
            }
        } else {
            self.mining = Some((p, progress));
        }
    }

    fn place(&mut self) {
        self.place_cd = 0.2;
        let item = self.inv.hot(self.sel);
        let target = self.target();
        // use the block you are looking at
        if let Some((p, _, _)) = target {
            match self.world.get_block(p.x, p.y, p.z) {
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
                WORKBENCH | FURNACE => {
                    self.goto(Screen::Inventory);
                    return;
                }
                _ => {}
            }
        }
        let bite = items::food(item);
        if bite > 0.0 {
            if self.food < MAX_FOOD - 0.5 {
                self.inv.take_slot(self.sel);
                self.food = (self.food + bite).min(MAX_FOOD);
                self.place_cd = 0.4;
                self.say(format!("Ate {}", items::name(item)));
            }
            return;
        }
        match items::gear(item) {
            Some((items::Kind::Hoe, _)) => {
                // till the ground
                if let Some((p, _, _)) = target {
                    let above = self.world.get_block(p.x, p.y + 1, p.z);
                    let soil = matches!(self.world.get_block(p.x, p.y, p.z), GRASS | DIRT);
                    if soil && (above == AIR || above == TALL_GRASS) {
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
                            self.inv.take_slot(self.sel);
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
                        self.inv.slots[self.sel] = Stack::new(items::WATER_BUCKET, 1);
                    }
                }
                return;
            }
            items::WATER_BUCKET => {
                if let Some((p, n, _)) = target {
                    let q = p + n;
                    if is_replaceable(self.world.get_block(q.x, q.y, q.z)) && self.world.set_block(&self.gfx, q.x, q.y, q.z, WATER) {
                        self.inv.slots[self.sel] = Stack::new(items::BUCKET, 1);
                    }
                }
                return;
            }
            items::BOW => {
                if self.inv.take(items::ARROW, 1) {
                    let dir = self.player.look_dir();
                    self.arrows.push(Arrow { pos: self.player.eye() + dir * 0.4, vel: dir * 40.0 + self.player.vel * 0.5, life: 6.0 });
                    self.place_cd = 0.7;
                    self.wear_held();
                } else {
                    self.say("No arrows".into());
                }
                return;
            }
            _ => {}
        }
        if !items::is_block(item) {
            return;
        }
        let Some((p, n, _)) = target else { return };
        let q = p + n;
        let cur = self.world.get_block(q.x, q.y, q.z);
        if !is_replaceable(cur) {
            return;
        }
        let pp = self.player.pos;
        let overlaps_player = pp.x + PLAYER_HW > q.x as f32
            && pp.x - PLAYER_HW < q.x as f32 + 1.0
            && pp.z + PLAYER_HW > q.z as f32
            && pp.z - PLAYER_HW < q.z as f32 + 1.0
            && pp.y + PLAYER_H > q.y as f32
            && pp.y < q.y as f32 + 1.0;
        if item != TORCH && !is_plant(item) && (overlaps_player || self.mobs.blocks_cell(q.x, q.y, q.z)) {
            return;
        }
        if self.world.set_block(&self.gfx, q.x, q.y, q.z, item) {
            self.inv.take_slot(self.sel);
        }
    }

    fn key_pressed(&mut self, code: KeyCode) {
        match code {
            KeyCode::Escape => match self.screen {
                Screen::Play => self.goto(Screen::Pause),
                Screen::Pause | Screen::Inventory => self.goto(Screen::Play),
                Screen::Dead => {}
                _ => self.goto(Screen::Pause),
            },
            KeyCode::KeyE | KeyCode::Tab => match self.screen {
                Screen::Play => self.goto(Screen::Inventory),
                Screen::Inventory => self.goto(Screen::Play),
                _ => {}
            },
            KeyCode::KeyF if self.screen == Screen::Play => self.player.flying = !self.player.flying,
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
                } else if self.shift {
                    self.inv.quick_move(i);
                } else {
                    self.inv.click(i);
                }
            }
            Action::Sort => self.inv.sort_bag(),
            Action::Respawn => self.respawn(),
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
        self.world.update(&self.gfx, pc);
        let t_world = t0.elapsed().as_secs_f32() * 1000.0;
        self.ready = self.ready
            || self.world.is_loaded(self.player.pos.x.floor() as i32, self.player.pos.z.floor() as i32);
        if self.ready && self.unstick {
            self.unstick = false;
            while collides(&self.world, self.player.pos, PLAYER_HW, PLAYER_H) && self.player.pos.y < CH as f32 {
                self.player.pos.y += 1.0;
            }
        }

        let playing = self.screen == Screen::Play;
        let k = |c: KeyCode| playing && self.keys.contains(&c);
        let mut input = Input {
            forward: (k(KeyCode::KeyW) as i32 - k(KeyCode::KeyS) as i32) as f32,
            right: (k(KeyCode::KeyD) as i32 - k(KeyCode::KeyA) as i32) as f32,
            jump: k(KeyCode::Space),
            sneak: k(KeyCode::ShiftLeft),
            sprint: k(KeyCode::ControlLeft),
        };
        if let Some(b) = &mut self.bench {
            b.t += real_dt;
            input = Input { forward: 1.0, ..Default::default() };
            self.player.flying = true;
            self.player.yaw +=
                std::env::var("CUBEWILD_BENCH_TURN").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.12) * dt;
            let floor = height_at(self.world.seed, self.player.pos.x as i32, self.player.pos.z as i32) as f32 + 14.0;
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
        let simulate = self.ready && matches!(self.screen, Screen::Play | Screen::Inventory);
        if simulate {
            self.time = (self.time + dt / DAY_SECS).rem_euclid(1.0);
            self.player.update(&self.world, &input, dt);
            let night = self.sun().1 < 0.3;
            let bites = self.mobs.update(&self.world, self.player.pos, input.sneak && !self.player.flying, night, dt);
            if self.bench.is_none() {
                if bites > 0.0 {
                    self.strike(bites);
                }
                self.survive(&input, dt);
                if self.health <= 0.0 {
                    self.health = 0.0;
                    self.goto(Screen::Dead);
                }
            }
        }
        let playing = self.screen == Screen::Play;
        let (sun_dir, daylight) = self.sun();
        if simulate {
            // arrows fly in an arc and stick the first animal or block they meet
            let mut arrows = std::mem::take(&mut self.arrows);
            arrows.retain_mut(|a| {
                a.vel.y -= 20.0 * dt;
                a.life -= dt;
                let step = a.vel * dt;
                let len = step.length().max(1e-4);
                if let Some((i, _)) = self.mobs.pick(a.pos, step / len, len) {
                    if let Some((meat, hide)) = self.mobs.hit(i, a.pos - step, 5) {
                        self.loot(meat, hide);
                    }
                    self.hit_flash = 0.18;
                    return false;
                }
                a.pos += step;
                let c = a.pos.floor().as_ivec3();
                a.life > 0.0 && !is_solid(self.world.get_block(c.x, c.y, c.z))
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
        let proj = perspective(self.settings.fov.to_radians(), w as f32 / h as f32, 0.08, view_dist * 1.6 + 64.0);
        let vp = proj * look_to(dir);
        let (r0, r1, r2, r3) = (vp.row(0), vp.row(1), vp.row(2), vp.row(3));
        let planes: [Vec4; 5] = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2];

        let mut chunks: Vec<(&ChunkMesh, [f32; 3], bool)> = Vec::with_capacity(512);
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
                quads += mesh.quads();
                // distant plants are only a few pixels: skip their draw calls
                chunks.push((mesh, min.into(), dx * dx + dz * dz <= 64));
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
                let c = rel + Vec3::new(0.5, 0.7, 0.5);
                lights.push([c.x, c.y, c.z, if b == LAMP { 15.0 } else { 11.0 }]);
            }
            if b == TORCH {
                let m = Mat4::from_translation(rel);
                push_box(&mut ents, &m, Vec3::new(0.44, 0.0, 0.44), Vec3::new(0.56, 0.58, 0.56), [150, 106, 58], gfx::EMISSIVE);
                push_box(&mut ents, &m, Vec3::new(0.41, 0.58, 0.41), Vec3::new(0.59, 0.76, 0.59), [255, 200, 80], gfx::EMISSIVE);
            }
        }
        let mut sky = std::mem::take(&mut self.sky);
        sky.clear();
        if !underwater {
            let night = 1.0 - smooth(0.15, 0.6, daylight);
            if night > 0.02 {
                let rot = Mat3::from_rotation_z(self.time * std::f32::consts::TAU);
                for &(d, size) in &self.stars {
                    let d = rot * d;
                    if d.y > -0.05 {
                        push_sky_quad(&mut sky, d, 60.0, size, [235, 240, 255, (night * 230.0) as u8]);
                    }
                }
            }
            if sun_dir.y > -0.2 {
                push_sky_quad(&mut sky, sun_dir, 60.0, 7.0, [255, 236, 170, 70]);
                push_sky_quad(&mut sky, sun_dir, 60.0, 4.4, [255, 246, 214, 255]);
            }
            if sun_dir.y < 0.2 {
                push_sky_quad(&mut sky, -sun_dir, 60.0, 3.4, [226, 232, 244, 255]);
                push_sky_quad(&mut sky, (-sun_dir + Vec3::new(0.012, 0.012, 0.0)).normalize(), 59.0, 1.0, [196, 204, 222, 255]);
            }
        }
        let target = if playing { raycast(&self.world, eye, dir, 6.0) } else { None };
        if let Some((p, _, _)) = target {
            // wireframe around the targeted block
            let m = Mat4::from_translation(p.as_vec3() - eye);
            let (a, b, t) = (-0.004f32, 1.004f32, 0.012f32);
            for (u, v) in [(a, a), (b, a), (a, b), (b, b)] {
                push_box(&mut ents, &m, Vec3::new(a, u - t, v - t), Vec3::new(b, u + t, v + t), [10, 10, 10], 200);
                push_box(&mut ents, &m, Vec3::new(u - t, a, v - t), Vec3::new(u + t, b, v + t), [10, 10, 10], 200);
                push_box(&mut ents, &m, Vec3::new(u - t, v - t, a), Vec3::new(u + t, v + t, b), [10, 10, 10], 200);
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
        if self.ready && self.screen != Screen::Inventory {
            for i in 0..9 {
                draw_slot(&mut ui, hx + i as f32 * slot, hy, slot, self.inv.slots[i], i == self.sel, false);
            }
            // health on the left, hunger on the right, air above hunger while diving
            let isz = (22.0 * s).round();
            let iy = hy - isz - 6.0 * s;
            for i in 0..10 {
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
            if self.started && self.inv.is_empty() && playing {
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
                format!("Worst frame  {:.1} ms", self.worst_ms),
                format!("View {} chunks   drawn {}   quads {}", self.world.radius, self.drawn.0, self.drawn.1),
                format!("Animals {}   {}", self.mobs.list.len(), self.gfx.adapter_name),
                {
                    let hours = (self.time * 24.0 + 6.0).rem_euclid(24.0);
                    format!("Time {:02}:{:02}   health {:.1}   food {:.1}", hours as u32, (hours.fract() * 60.0) as u32, self.health, self.food)
                },
            ];
            for (i, l) in lines.iter().enumerate() {
                ui.label(12.0 * s, 8.0 * s + (i as f32 + 1.0) * (font.height(SMALL) + 2.0), SMALL, l, [225, 230, 236, 255]);
            }
        }

        if !self.ready {
            ui.rect(0.0, 0.0, wf, hf, [16, 22, 30, 255]);
            ui.text_centered(cx, cy - font.height(HEAD), HEAD, "Loading world...", TEXT);
        } else if self.bench.is_none() {
            let bh = (50.0 * s).round();
            let gap = (12.0 * s).round();
            match self.screen {
                Screen::Play => {}
                Screen::Dead => {
                    ui.rect(0.0, 0.0, wf, hf, [70, 8, 8, 150]);
                    let bw = (300.0 * s).round();
                    ui.text_centered(cx, cy - 150.0 * s, TITLE, "You died", TEXT);
                    ui.text_centered(cx, cy - 150.0 * s + font.height(TITLE) + 4.0 * s, BODY, "You keep your items and wake at your spawn point", DIM);
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
                    let ph = (4.0 * (bh + gap) + 170.0 * s).round();
                    let (px, py) = ((cx - pw * 0.5).round(), (cy - ph * 0.5).round());
                    ui.rect(px, py, pw, ph, [20, 24, 32, 238]);
                    ui.outline(px, py, pw, ph, (2.0 * s).round().max(1.0), [78, 90, 110, 255]);
                    ui.text_centered(cx, py + 18.0 * s, TITLE, "Cubewild", TEXT);
                    let sub = if self.started { "Game paused" } else { "Gather, craft, build, hunt" };
                    ui.text_centered(cx, py + 18.0 * s + font.height(TITLE), SMALL, sub, DIM);
                    let bx = px + 40.0 * s;
                    let bw = pw - 80.0 * s;
                    let mut y = py + 140.0 * s;
                    let first = if self.started { "Resume" } else { "Play" };
                    for (label, act) in [
                        (first, Action::Goto(Screen::Play)),
                        ("Settings", Action::Goto(Screen::Settings)),
                        ("Controls", Action::Goto(Screen::Controls)),
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
                    let (pw, ph) = ((700.0 * s).round(), (650.0 * s).round());
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
                    let tw = (64.0 * s).round();
                    let tx = px + pw - 32.0 * s - tw;
                    for (name, hint, on, act) in [
                        ("Vsync", "Off = uncapped frame rate, more heat", st.vsync, Action::ToggleVsync),
                        ("Fullscreen", "Also toggled with F11", st.fullscreen, Action::ToggleFullscreen),
                        ("Hold 120 FPS", "Shortens the view distance automatically when needed", st.auto_scale, Action::ToggleAuto),
                        ("Show FPS", "Frame counter in the corner", st.show_fps, Action::ToggleFps),
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
                        ("Right click", "Place block / eat / use bed, workbench, furnace"),
                        ("Middle click", "Select the block you look at"),
                        ("1 - 9, mouse wheel", "Choose hotbar slot"),
                        ("E or Tab", "Inventory and crafting"),
                        ("In the inventory", "Click moves stacks, right click splits, Shift+click sends across"),
                        ("Right click with a tool", "Hoe tills, bow shoots, rod fishes, bucket scoops"),
                        ("F", "Toggle flying"),
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
                    } else if let Some(st) = tip {
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
                    } else if let Some(text) = tip_text {
                        let tw = font.width(SMALL, text) + 20.0 * s;
                        let th = font.height(SMALL) + 12.0 * s;
                        ui.rect(px + 14.0 * s, py + 18.0 * s, tw, th, [16, 14, 22, 246]);
                        ui.outline(px + 14.0 * s, py + 18.0 * s, tw, th, 1.0, ACCENT);
                        ui.text(px + 24.0 * s, py + 24.0 * s, SMALL, text, TEXT);
                    }
                }
            }
        }

        let (fog_color, fog_start, fog_end) = if underwater {
            (mix3(srgb([3, 8, 20]), srgb([24, 70, 140]), daylight), 0.0, 26.0)
        } else {
            let base = mix3(srgb([6, 9, 20]), srgb([142, 196, 238]), daylight);
            // warm horizon while the sun is low
            let dusk = (1.0 - (sun_dir.y / 0.3).powi(2)).max(0.0) * 0.5;
            (mix3(base, srgb([232, 142, 92]), dusk), view_dist * 0.62, view_dist - 6.0)
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
            lights: &lights,
            sky: &sky,
            ents: &ents,
            ui: &ui.verts,
        });
        drop(chunks);
        self.ents = ents;
        self.sky = sky;
        let t_render = t1.elapsed().as_secs_f32() * 1000.0;
        if std::env::var_os("CUBEWILD_PROF").is_some()
            && real_dt > 0.009
            && self.bench.as_ref().map_or(false, |b| b.t > 6.0)
        {
            println!("slow frame {:.1}ms: world {:.2} build {:.2} render {:.2}", real_dt * 1000.0, t_world, t_build, t_render);
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
                            let b = g.world.get_block(p.x, p.y, p.z);
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
                    let max = (RECIPES.len() / RECIPE_COLS) as i32;
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
