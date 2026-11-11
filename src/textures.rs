//! Procedurally generated 16x16 block textures (no asset files).

pub const TEX: usize = 16;
/// first layer holding item and HUD icons
pub const ICON_BASE: usize = 96;
/// textures added after the icons: chest side, chest top, ladder, trapdoor
pub const EXTRA_BASE: usize = ICON_BASE + 103;
/// icons of the four slabs and two fences
pub const SHAPE_ICONS: usize = EXTRA_BASE + 4;
/// the eight door textures (they are drawn with the detailed shapes, which can name any
/// layer; the first 128 layers are kept for the blocks of the terrain mesh)
pub const DOOR_TEX: usize = SHAPE_ICONS + 6;
/// the player's arm in first person: skin, then sleeve
pub const ARM_TEX: usize = DOOR_TEX + 8;
pub const LAYERS: usize = ARM_TEX + 2;
/// lava, in the slot the doors used to start at
const LAVA_TEX: usize = 56;
/// block textures the slab / stair / fence icons are cut from
const SHAPE_TEX: [usize; 4] = [8, 44, 9, 15];

fn h(layer: u32, x: i32, y: i32) -> f32 {
    let mut v = layer.wrapping_mul(0x9e37_79b9)
        ^ (x as u32).wrapping_mul(0x85eb_ca6b)
        ^ (y as u32).wrapping_mul(0xc2b2_ae35).rotate_left(11);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2c1b_3c6d);
    v ^= v >> 12;
    v = v.wrapping_mul(0x297a_2d39);
    v ^= v >> 15;
    (v & 0xffff) as f32 / 65535.0
}

fn shade(c: [f32; 3], k: f32) -> [u8; 4] {
    let f = |v: f32| (v * k).clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2]), 255]
}

fn dirt(l: u32, x: i32, y: i32) -> [u8; 4] {
    let n = h(l, x, y);
    let k = if n > 0.9 { 0.72 } else { 0.88 + n * 0.24 };
    shade([126.0, 90.0, 60.0], k)
}

fn grass(l: u32, x: i32, y: i32) -> [u8; 4] {
    let n = h(l, x, y);
    let k = if n > 0.88 { 0.8 } else { 0.9 + n * 0.22 };
    shade([84.0, 148.0, 58.0], k)
}

const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// One pixel of a layer, for building meshes from the icons.
pub fn texel(layer: usize, x: i32, y: i32) -> [u8; 4] {
    pixel(layer, x, y)
}

fn pixel(layer: usize, x: i32, y: i32) -> [u8; 4] {
    if layer == LAVA_TEX {
        // slow bright veins in dark red crust
        let n = h(56, x / 2, y / 2) * 0.6 + h(57, x, y) * 0.4;
        let vein = ((x as f32 * 0.55 + (y as f32 * 0.7).sin() * 1.6).sin() * 0.5 + 0.5) * 0.6 + n * 0.4;
        return if vein > 0.62 {
            shade([255.0, 214.0, 90.0], 0.92 + n * 0.08)
        } else if vein > 0.4 {
            shade([246.0, 128.0, 30.0], 0.85 + n * 0.15)
        } else {
            shade([196.0, 62.0, 18.0], 0.8 + n * 0.2)
        };
    }
    if layer >= ARM_TEX {
        let n = h(layer as u32, x, y);
        let c = if layer == ARM_TEX { [226.0, 176.0, 134.0] } else { [176.0, 92.0, 52.0] };
        return shade(c, 0.93 + n * 0.07);
    }
    if (57..ICON_BASE).contains(&layer) {
        return biome_pixel(layer, x, y);
    }
    // (doors keep the numbering their drawing code was written for)
    let layer = if layer >= DOOR_TEX { layer - DOOR_TEX + 56 } else { layer };
    let l = layer as u32;
    let n = h(l, x, y);
    match layer {
        0 => grass(l, x, y),
        1 => {
            let edge = 2 + (h(l + 50, x, 0) * 3.0) as i32;
            if y < edge {
                grass(0, x, y)
            } else {
                dirt(2, x, y)
            }
        }
        2 => dirt(l, x, y),
        3 => {
            let blotch = h(l + 9, x / 4, y / 3);
            shade([128.0, 128.0, 130.0], 0.84 + blotch * 0.16 + n * 0.1)
        }
        4 => shade([222.0, 206.0, 152.0], 0.93 + n * 0.1),
        5 => {
            let col = h(l, x, 0);
            let k = 0.8 + col * 0.25 + n * 0.08;
            shade([104.0, 78.0, 46.0], k)
        }
        6 => {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let d = dx.abs().max(dy.abs());
            if d > 6.6 {
                shade([104.0, 78.0, 46.0], 0.85 + n * 0.1)
            } else if (d as i32) % 2 == 0 {
                shade([184.0, 148.0, 92.0], 0.95 + n * 0.08)
            } else {
                shade([160.0, 124.0, 74.0], 0.95 + n * 0.08)
            }
        }
        7 => {
            let k = if n > 0.8 { 0.62 } else if n < 0.15 { 1.18 } else { 0.86 + n * 0.2 };
            shade([52.0, 118.0, 42.0], k)
        }
        8 => {
            let row = y / 4;
            let seam = (x + row * 5) % 16 == 0;
            let k = if y % 4 == 3 || seam { 0.72 } else { 0.92 + h(l, x / 3, row) * 0.12 + n * 0.05 };
            shade([178.0, 140.0, 86.0], k)
        }
        9 => {
            // cobble: jittered cell pattern with dark gaps
            let mut best = (99.0f32, 99.0f32, 0.0f32);
            for cy in -1i32..5 {
                for cx in -1i32..5 {
                    let (wx, wy) = (cx.rem_euclid(4), cy.rem_euclid(4));
                    let px = cx as f32 * 4.0 + 1.0 + h(l + 1, wx, wy) * 2.5;
                    let py = cy as f32 * 4.0 + 1.0 + h(l + 2, wx, wy) * 2.5;
                    let d = (x as f32 - px).abs().max((y as f32 - py).abs() * 1.15);
                    if d < best.0 {
                        best = (d, best.0, h(l + 3, wx, wy));
                    } else if d < best.1 {
                        best.1 = d;
                    }
                }
            }
            let k = if best.1 - best.0 < 0.9 { 0.55 } else { 0.8 + best.2 * 0.3 + n * 0.06 };
            shade([122.0, 122.0, 124.0], k)
        }
        10 => {
            let row = y / 4;
            let mortar = y % 4 == 3 || (x + if row % 2 == 0 { 0 } else { 4 }) % 8 == 7;
            if mortar {
                shade([188.0, 182.0, 170.0], 0.9 + n * 0.1)
            } else {
                let b = h(l, (x + if row % 2 == 0 { 0 } else { 4 }) / 8, row);
                shade([156.0, 74.0, 56.0], 0.85 + b * 0.2 + n * 0.08)
            }
        }
        11 => shade([238.0, 243.0, 250.0], 0.96 + n * 0.04),
        12 => {
            let mut c = shade([44.0, 104.0, 196.0], 0.94 + n * 0.1);
            c[3] = 165;
            c
        }
        13 => {
            if x == 0 || y == 0 || x == 15 || y == 15 {
                [214, 232, 240, 255]
            } else if (x - y == 3 || x - y == 4) && (3..9).contains(&x) {
                [235, 245, 250, 150]
            } else {
                [190, 220, 235, 38]
            }
        }
        14 => {
            let k = if n > 0.7 { 0.6 } else { 0.9 + n * 0.3 };
            shade([58.0, 58.0, 64.0], k)
        }
        15 => {
            // stone bricks: large staggered blocks
            let row = y / 8;
            let xo = (x + if row % 2 == 0 { 0 } else { 8 }) % 16;
            if y % 8 == 7 || xo == 15 {
                shade([84.0, 84.0, 88.0], 0.9 + n * 0.1)
            } else {
                let b = h(l, (x + if row % 2 == 0 { 0 } else { 8 }) / 16, row);
                let top = if y % 8 == 0 || xo == 0 { 1.1 } else { 1.0 };
                shade([140.0, 140.0, 144.0], (0.88 + b * 0.1 + n * 0.08) * top)
            }
        }
        16 => {
            // leather: stitched panels
            let stitch = (x % 8 == 0 && y % 2 == 0) || (y % 8 == 0 && x % 2 == 0);
            let k = if stitch { 0.62 } else { 0.9 + n * 0.14 };
            shade([150.0, 98.0, 58.0], k)
        }
        17 | 18 | 19 | 37 => {
            // ores: stone speckled with nuggets
            let blotch = h(3 + 9, x / 4, y / 3);
            let nug = h(l + 5, (x + 1) / 2, (y + (x / 2) % 2) / 2) > 0.7 && x > 0 && y > 0 && x < 15 && y < 15;
            if nug {
                let c = match layer {
                    17 => [38.0, 38.0, 42.0],
                    18 => [204.0, 152.0, 110.0],
                    37 => [250.0, 210.0, 60.0],
                    _ => [104.0, 226.0, 232.0],
                };
                shade(c, 0.85 + n * 0.3)
            } else {
                shade([128.0, 128.0, 130.0], 0.84 + blotch * 0.16 + n * 0.1)
            }
        }
        20 => {
            let k = if y % 5 == 4 { 0.84 } else if y == 0 { 1.04 } else { 0.94 + h(l, x / 5, y / 5) * 0.06 + n * 0.05 };
            shade([214.0, 194.0, 138.0], k)
        }
        21 => {
            let k = if x % 4 == 1 { 0.7 } else { 0.92 + n * 0.14 };
            if x % 4 == 3 && (y + x) % 5 == 0 {
                [226, 230, 196, 255]
            } else {
                shade([66.0, 132.0, 58.0], k)
            }
        }
        22 => {
            let d = (x as f32 - 7.5).abs().max((y as f32 - 7.5).abs());
            shade([76.0, 146.0, 64.0], if d > 6.6 { 0.7 } else if d < 1.6 { 1.12 } else { 0.94 + n * 0.1 })
        }
        23 => {
            // workbench top: planks with a framed grid
            let frame = x == 0 || y == 0 || x == 15 || y == 15;
            let grid = x == 5 || x == 10 || y == 5 || y == 10;
            shade([178.0, 140.0, 86.0], if frame { 0.55 } else if grid { 0.74 } else { 0.96 + n * 0.08 })
        }
        24 => {
            let frame = x == 0 || x == 15 || y == 15 || y == 0;
            if frame || y == 5 {
                shade([104.0, 78.0, 46.0], 0.82 + n * 0.1)
            } else if y < 5 {
                shade([178.0, 140.0, 86.0], 0.98 + n * 0.08)
            } else if matches!((x, y), (4, 8..=12) | (3..=5, 8) | (10..=12, 8..=9) | (11, 10..=12)) {
                shade([150.0, 150.0, 156.0], 0.95) // hanging tools
            } else {
                shade([150.0, 114.0, 68.0], 0.92 + n * 0.1)
            }
        }
        25 | 26 => {
            // furnace: stone casing, the front has a firebox
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            if layer == 25 && (4..=11).contains(&x) && (8..=13).contains(&y) {
                if y >= 11 {
                    shade([246.0, 150.0, 40.0], 0.8 + n * 0.4)
                } else {
                    shade([30.0, 26.0, 26.0], 1.0)
                }
            } else if layer == 25 && (3..=12).contains(&x) && (7..=14).contains(&y) {
                shade([70.0, 70.0, 74.0], 1.0)
            } else if layer == 25 && (5..=10).contains(&x) && (3..=4).contains(&y) {
                shade([44.0, 44.0, 48.0], 1.0)
            } else {
                shade([112.0, 112.0, 116.0], if edge { 0.72 } else { 0.92 + n * 0.12 })
            }
        }
        27 => {
            let d = (x as f32 - 7.5).abs().max((y as f32 - 7.5).abs());
            if d > 6.6 || x == 7 || x == 8 || y == 7 || y == 8 {
                shade([120.0, 96.0, 60.0], 1.0)
            } else {
                shade([255.0, 232.0, 150.0], 0.92 + n * 0.08)
            }
        }
        28 => {
            if y < 5 {
                if x == 0 || x == 15 || y == 0 { shade([200.0, 200.0, 204.0], 1.0) } else { shade([240.0, 240.0, 236.0], 0.96 + n * 0.04) }
            } else if y == 5 {
                shade([236.0, 226.0, 214.0], 1.0)
            } else {
                shade([176.0, 52.0, 48.0], if x == 0 || x == 15 || y == 15 { 0.75 } else { 0.92 + n * 0.12 })
            }
        }
        29 => {
            if y < 5 {
                shade([176.0, 52.0, 48.0], if y == 4 { 0.72 } else { 0.9 + n * 0.12 })
            } else if x < 2 || x > 13 || y < 9 {
                shade([150.0, 114.0, 68.0], if y == 5 || y == 8 { 0.74 } else { 0.9 + n * 0.1 })
            } else {
                shade([92.0, 68.0, 42.0], 0.8 + n * 0.1)
            }
        }
        30 => {
            // torch icon
            if (7..=8).contains(&x) && (6..=14).contains(&y) {
                shade([122.0, 86.0, 48.0], 0.9 + n * 0.12)
            } else if (6..=9).contains(&x) && (2..=5).contains(&y) && !((x == 6 || x == 9) && y == 2) {
                if (7..=8).contains(&x) && y >= 4 { [255, 240, 170, 255] } else { [250, 160, 40, 255] }
            } else {
                [0, 0, 0, 0]
            }
        }
        31 => {
            // birch bark: pale with dark dashes
            if h(l, x / 3, y) > 0.84 {
                shade([56.0, 54.0, 50.0], 1.0)
            } else {
                shade([228.0, 226.0, 214.0], 0.9 + h(l, x, 0) * 0.08 + n * 0.05)
            }
        }
        32 => {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let d = dx.abs().max(dy.abs());
            if d > 6.6 {
                shade([228.0, 226.0, 214.0], 0.88 + n * 0.1)
            } else if (d as i32) % 2 == 0 {
                shade([214.0, 190.0, 138.0], 0.95 + n * 0.08)
            } else {
                shade([194.0, 168.0, 116.0], 0.95 + n * 0.08)
            }
        }
        33 | 34 => {
            let k = if n > 0.8 { 0.62 } else if n < 0.15 { 1.18 } else { 0.86 + n * 0.2 };
            shade(if layer == 33 { [116.0, 164.0, 62.0] } else { [30.0, 82.0, 52.0] }, k)
        }
        35 => {
            let c = h(l, (x + (y / 2) % 2) / 2, y / 2);
            shade([134.0, 130.0, 126.0], 0.68 + c * 0.44 + n * 0.08)
        }
        36 => shade([158.0, 166.0, 180.0], if y % 6 == 5 { 0.9 } else { 0.96 + n * 0.06 }),
        38 => {
            // grass tuft
            let top = 15 - (4.0 + h(l, x, 0) * 10.0) as i32;
            if x % 3 != 1 && y >= top && h(l + 1, x, 1) > 0.25 {
                shade([78.0, 150.0, 56.0], 0.78 + h(l, x, 2) * 0.3 + (15 - y) as f32 * 0.02)
            } else {
                CLEAR
            }
        }
        39 | 40 => {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 5.0);
            let d = dx * dx + dy * dy;
            if d < 1.6 {
                if layer == 39 { [250, 214, 70, 255] } else { [226, 130, 40, 255] }
            } else if d < 11.0 && !(dx.abs() > 2.0 && dy.abs() > 2.0) {
                shade(if layer == 39 { [214.0, 44.0, 52.0] } else { [248.0, 214.0, 62.0] }, 0.88 + n * 0.2)
            } else if ((7..=8).contains(&x) && y >= 8) || matches!((x, y), (5, 11) | (6, 11) | (6, 12) | (9, 10) | (10, 10) | (9, 11)) {
                shade([62.0, 132.0, 52.0], 0.9 + n * 0.15)
            } else {
                CLEAR
            }
        }
        41 => {
            let half = match y {
                5 => 2,
                6 => 4,
                7 | 8 => 5,
                _ => 0,
            };
            if half > 0 && (x as f32 - 7.5).abs() < half as f32 {
                if matches!((x, y), (6, 6) | (9, 7) | (4, 8) | (11, 8) | (7, 5)) { [244, 240, 230, 255] } else { shade([184.0, 62.0, 46.0], 0.9 + n * 0.16) }
            } else if (6..=9).contains(&x) && (9..=15).contains(&y) && !((x == 6 || x == 9) && y < 13) {
                shade([232.0, 222.0, 200.0], 0.9 + n * 0.1)
            } else {
                CLEAR
            }
        }
        42 => {
            let (dx, dy) = ((x as f32 - 7.5) / 7.8, (y as f32 - 9.0) / 6.8);
            if dx * dx + dy * dy < 1.0 && n > 0.1 {
                if h(l + 3, x / 2, y / 2) > 0.78 && y > 4 {
                    shade([206.0, 40.0, 76.0], 0.9 + n * 0.2)
                } else {
                    shade([48.0, 112.0, 48.0], 0.74 + n * 0.4)
                }
            } else {
                CLEAR
            }
        }
        43 => {
            let t = 13 - y;
            let twig = ((7..=8).contains(&x) && y >= 8)
                || (t >= 0 && t <= 8 && (x == 7 - t || x == 8 + t))
                || (y >= 3 && y <= 8 && (x == 4 + (8 - y) / 2 || x == 11 - (8 - y) / 2));
            if twig { shade([124.0, 92.0, 56.0], 0.85 + n * 0.25) } else { CLEAR }
        }
        44 => {
            let row = y / 4;
            let seam = (x + row * 5) % 16 == 0;
            let k = if y % 4 == 3 || seam { 0.76 } else { 0.92 + h(l, x / 3, row) * 0.1 + n * 0.05 };
            shade([218.0, 200.0, 152.0], k)
        }
        45 | 46 => {
            // metal blocks: bevelled plate
            let c = if layer == 45 { [248.0, 208.0, 66.0] } else { [210.0, 212.0, 220.0] };
            let k = if x == 0 || y == 0 {
                1.12
            } else if x == 15 || y == 15 {
                0.68
            } else if (x == 3 || x == 12) && (y == 3 || y == 12) {
                0.7
            } else if x - y == 2 || x - y == -6 {
                1.06
            } else {
                0.9 + n * 0.08
            };
            shade(c, k)
        }
        47 => {
            if h(l + 7, x / 3, y / 3) > 0.52 && n > 0.2 {
                shade([70.0, 122.0, 56.0], 0.8 + n * 0.3)
            } else {
                pixel(9, x, y)
            }
        }
        48 => shade([234.0, 230.0, 218.0], if (x + y) % 2 == 0 { 0.9 } else { 1.0 } * if x % 8 == 0 || y % 8 == 0 { 0.9 } else { 1.0 }),
        49 => {
            let k = if y % 5 == 4 { 0.7 } else { 0.84 + h(l, x, y / 5) * 0.26 };
            shade([208.0, 176.0, 84.0], k)
        }
        50 => {
            // tilled soil: dark furrows
            let k = if x % 4 == 0 { 0.5 } else if x % 4 == 1 { 0.66 } else { 0.76 + n * 0.14 };
            shade([126.0, 90.0, 60.0], k)
        }
        51..=54 => {
            // wheat, taller and yellower with each stage
            let stage = (layer - 51) as i32;
            let top = 12 - stage * 3 - (h(l, x, 0) * 2.0) as i32;
            if x % 2 == 0 && y >= top && h(51, x, 1) > 0.2 {
                if stage == 3 && y < top + 4 {
                    shade([226.0, 186.0, 70.0], 0.85 + n * 0.25)
                } else {
                    let ripe = stage as f32 / 3.0;
                    shade([90.0 + 110.0 * ripe, 160.0 + 10.0 * ripe, 56.0], 0.8 + n * 0.25)
                }
            } else {
                CLEAR
            }
        }
        55 => {
            // window pane: clear glass in a slim wooden frame
            if x == 0 || y == 0 || x == 15 || y == 15 {
                shade([150.0, 112.0, 68.0], 0.9 + n * 0.14)
            } else if (x - y == 3 || x - y == 4) && (3..10).contains(&x) {
                [235, 245, 250, 150]
            } else {
                [190, 220, 235, 34]
            }
        }
        56..=63 => {
            // doors: 56 oak top, 57 oak bottom, 58/59 birch; 60..63 the same mirrored
            let k = layer - 56;
            let x = if k >= 4 { 15 - x } else { x };
            let wood = if k % 4 < 2 { [178.0, 140.0, 86.0] } else { [218.0, 200.0, 152.0] };
            let grain = 0.94 + h(56 + (k % 4) as u32, x / 2, y) * 0.1;
            let frame = x == 0 || x == 15;
            if k % 2 == 0 {
                // top half: four small lights
                let glass = matches!(x, 3..=6 | 9..=12) && matches!(y, 3..=6 | 8..=11);
                let rim = matches!(x, 2..=13) && matches!(y, 2..=12);
                if glass {
                    CLEAR
                } else if frame || y == 0 {
                    shade(wood, 0.66)
                } else if rim {
                    shade(wood, 0.78)
                } else {
                    shade(wood, grain)
                }
            } else {
                let panel = matches!(x, 3..=12) && matches!(y, 5..=12);
                let edge = panel && (x == 3 || x == 12 || y == 5 || y == 12);
                if matches!(x, 12 | 13) && matches!(y, 1 | 2) {
                    [58, 56, 62, 255] // knob
                } else if frame || y == 15 {
                    shade(wood, 0.66)
                } else if edge {
                    shade(wood, 0.72)
                } else if panel {
                    shade(wood, grain * 0.9)
                } else {
                    shade(wood, grain)
                }
            }
        }
        l if l >= EXTRA_BASE => extra_pixel(l - EXTRA_BASE, x, y),
        l if l >= ICON_BASE => item_pixel(l - ICON_BASE, x, y),
        _ => [0, 0, 0, 0],
    }
}


/// Blocks of the biomes (layers 57 up to the icons; `world::tex_for` has the list).
fn biome_pixel(layer: usize, x: i32, y: i32) -> [u8; 4] {
    let l = layer as u32;
    let n = h(l, x, y);
    let tint = |c: [f32; 3]| {
        let k = if n > 0.88 { 0.8 } else { 0.9 + n * 0.22 };
        shade(c, k)
    };
    // a side of grassy soil: a fringe of `c` over dirt
    let side = |c: [f32; 3]| {
        let edge = 2 + (h(l + 50, x, 0) * 3.0) as i32;
        if y < edge { tint(c) } else { dirt(2, x, y) }
    };
    let leaf = |c: [f32; 3]| {
        let k = if n > 0.8 { 0.62 } else if n < 0.15 { 1.18 } else { 0.86 + n * 0.2 };
        shade(c, k)
    };
    const DRY: [f32; 3] = [176.0, 166.0, 78.0];
    const LUSH: [f32; 3] = [58.0, 172.0, 44.0];
    const SWAMP: [f32; 3] = [86.0, 112.0, 62.0];
    match layer {
        // ice: pale blue with fine cracks
        57 => {
            let crack = (x + y * 3) % 11 == 0 && h(l + 1, x / 3, y / 3) > 0.4 || (x * 2 + 15 - y) % 13 == 0 && h(l + 2, x / 4, y / 2) > 0.55;
            if crack { [214, 236, 250, 255] } else { shade([150.0, 196.0, 240.0], 0.94 + n * 0.08) }
        }
        58 => shade([196.0, 106.0, 50.0], 0.93 + n * 0.1),
        59 | 60 | 61 => {
            let c = match layer {
                59 => [164.0, 88.0, 56.0],
                60 => [214.0, 182.0, 158.0],
                _ => [104.0, 62.0, 48.0],
            };
            // fired clay: smooth, in faint level layers
            shade(c, 0.9 + h(l, x / 5, y / 2) * 0.1 + n * 0.05)
        }
        62 => {
            // snow lying over the edge of the soil
            let edge = 3 + (h(l + 50, x, 0) * 3.0) as i32;
            if y < edge { shade([238.0, 243.0, 250.0], 0.95 + n * 0.05) } else { dirt(2, x, y) }
        }
        63 => tint(DRY),
        64 => side(DRY),
        65 => tint(LUSH),
        66 => side(LUSH),
        67 => tint(SWAMP),
        68 => side(SWAMP),
        69 => leaf([36.0, 136.0, 30.0]),
        70 => leaf([118.0, 142.0, 48.0]),
        71 => {
            // needles with snow lying on them
            if h(l + 4, x / 3, y / 2) > 0.62 || (n > 0.9) { shade([226.0, 234.0, 244.0], 0.93 + n * 0.07) } else { shade([40.0, 92.0, 62.0], 0.84 + n * 0.22) }
        }
        72 => {
            // fern: fronds arching out from the middle
            let dx = (x as f32 - 7.5).abs();
            let arch = 15.0 - (dx * 1.9 - dx * dx * 0.2) - 2.0;
            let frond = dx < 7.0 && (y as f32) >= arch - 1.5 && (y as f32) <= arch + 1.5 + (7.0 - dx) * 0.7 && (x + y) % 3 != 0;
            if frond || ((7..=8).contains(&x) && y >= 6) { shade([44.0, 128.0, 52.0], 0.8 + n * 0.35) } else { CLEAR }
        }
        73 => {
            // reeds: a few upright canes with joints
            let cane = matches!(x, 2 | 3 | 7 | 8 | 12 | 13) && (y as f32) >= h(l + 1, x / 5, 0) * 4.0;
            if !cane {
                CLEAR
            } else if y % 5 == (x / 5) % 5 {
                shade([86.0, 120.0, 52.0], 0.8)
            } else {
                shade([128.0, 176.0, 78.0], if x % 5 == 2 { 0.86 } else { 1.0 } + n * 0.08)
            }
        }
        74 => {
            // a blue flower
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 5.0);
            let d = dx * dx + dy * dy;
            if d < 1.6 {
                [250, 226, 110, 255]
            } else if d < 10.0 && !(dx.abs() > 2.0 && dy.abs() > 2.0) {
                shade([70.0, 110.0, 226.0], 0.88 + n * 0.2)
            } else if ((7..=8).contains(&x) && y >= 8) || matches!((x, y), (5, 12) | (6, 12) | (6, 13) | (9, 10) | (10, 10) | (9, 11)) {
                shade([62.0, 132.0, 52.0], 0.9 + n * 0.15)
            } else {
                CLEAR
            }
        }
        75 => {
            // dry grass tuft
            let top = 15 - (6.0 + h(l, x, 0) * 9.0) as i32;
            if x % 3 != 2 && y >= top && h(l + 1, x, 1) > 0.2 {
                shade([198.0, 178.0, 84.0], 0.8 + h(l, x, 2) * 0.28 + (15 - y) as f32 * 0.015)
            } else {
                CLEAR
            }
        }
        _ => CLEAR,
    }
}

/// Chest, ladder and trapdoor textures, and the icons of slabs and fences.
fn extra_pixel(k: usize, x: i32, y: i32) -> [u8; 4] {
    let n = h(700 + k as u32, x, y);
    let wood = [178.0, 140.0, 86.0];
    match k {
        0 => {
            // chest side: boards, a lid seam and an iron latch
            // (the chest is drawn a pixel narrower than its cell and two shorter)
            if matches!(x, 7 | 8) && matches!(y, 5..=8) {
                [196, 198, 206, 255]
            } else if x <= 1 || x >= 14 || y <= 2 || y >= 15 {
                shade([112.0, 80.0, 44.0], 0.9 + n * 0.1)
            } else if y == 6 {
                shade([112.0, 80.0, 44.0], 0.7)
            } else {
                shade([164.0, 118.0, 62.0], 0.9 + h(700, x / 4, 0) * 0.12 + n * 0.06)
            }
        }
        1 => {
            // chest lid
            if x <= 1 || x >= 14 || y <= 1 || y >= 14 {
                shade([112.0, 80.0, 44.0], 0.9 + n * 0.1)
            } else {
                shade([164.0, 118.0, 62.0], 0.92 + h(701, 0, y / 4) * 0.12 + n * 0.06)
            }
        }
        2 => {
            // ladder: two rails and four rungs
            if matches!(x, 2 | 3 | 12 | 13) {
                shade([150.0, 110.0, 62.0], 0.88 + n * 0.16)
            } else if y % 4 == 1 && (2..=13).contains(&x) {
                shade([170.0, 128.0, 74.0], 0.9 + n * 0.14)
            } else if y % 4 == 2 && (4..=11).contains(&x) {
                shade([120.0, 86.0, 48.0], 0.9)
            } else {
                CLEAR
            }
        }
        3 => {
            // trapdoor: a framed hatch with slits and iron bands
            let frame = x <= 1 || x >= 14 || y <= 1 || y >= 14;
            if !frame && matches!(x, 4 | 5 | 10 | 11) && matches!(y, 4..=6 | 9..=11) {
                CLEAR
            } else if frame && (matches!(x, 3 | 12) || matches!(y, 3 | 12)) {
                [92, 94, 104, 255]
            } else if frame {
                shade(wood, 0.7 + n * 0.08)
            } else {
                shade(wood, 0.9 + h(703, 0, y / 3) * 0.1 + n * 0.06)
            }
        }
        4..=7 => {
            // slab: the lower half of the block, with a lit top edge
            match y {
                0..=6 => CLEAR,
                7..=9 => lighten(pixel(SHAPE_TEX[k - 4], x, y - 7), 1.18),
                _ => darken(pixel(SHAPE_TEX[k - 4], x, y), 0.86),
            }
        }
        _ => {
            // fence: two posts and two rails
            let t = pixel(SHAPE_TEX[k - 8], x, y);
            if matches!(x, 2..=4 | 11..=13) && y >= 1 {
                t
            } else if matches!(y, 4 | 5 | 10 | 11) {
                darken(t, 0.82)
            } else {
                CLEAR
            }
        }
    }
}

fn darken(c: [u8; 4], k: f32) -> [u8; 4] {
    [(c[0] as f32 * k) as u8, (c[1] as f32 * k) as u8, (c[2] as f32 * k) as u8, c[3]]
}

fn lighten(c: [u8; 4], k: f32) -> [u8; 4] {
    [(c[0] as f32 * k).min(255.0) as u8, (c[1] as f32 * k).min(255.0) as u8, (c[2] as f32 * k).min(255.0) as u8, c[3]]
}

/// 16x16 icons for non-block items (transparent background).
fn item_pixel(item: usize, x: i32, y: i32) -> [u8; 4] {
    const NONE: [u8; 4] = [0, 0, 0, 0];
    let n = h(900 + item as u32, x, y);
    let wood = |k: f32| shade([122.0, 86.0, 48.0], k + n * 0.12);
    let stone = |k: f32| shade([150.0, 150.0, 156.0], k + n * 0.12);
    let plank = |k: f32| shade([186.0, 146.0, 90.0], k + n * 0.1);
    let iron = |k: f32| shade([216.0, 216.0, 222.0], k + n * 0.08);
    let crystal = |k: f32| shade([104.0, 222.0, 232.0], k + n * 0.1);
    // diagonal handle from bottom-left to top-right
    let handle = |lo: i32, hi: i32| (x + y == 15 || x + y == 16) && x >= lo && x <= hi;
    let pick = [
        (6, 2), (7, 2), (8, 2), (9, 2), (7, 3), (8, 3), (9, 3), (10, 3), (11, 3), (10, 4),
        (11, 4), (12, 4), (11, 5), (12, 5), (12, 6), (13, 6), (12, 7), (13, 7), (12, 8), (13, 8), (13, 9),
    ];
    // tools and armour that come in several materials share a shape
    let metal = |m: u8, k: f32| -> [u8; 4] {
        let c = match m {
            0 => [186.0, 146.0, 90.0],
            1 => [150.0, 150.0, 156.0],
            2 => [216.0, 216.0, 222.0],
            3 => [250.0, 208.0, 66.0],
            4 => [104.0, 222.0, 232.0],
            _ => [150.0, 98.0, 58.0],
        };
        shade(c, k + n * 0.1)
    };
    // (shape, material): 0 pick 1 axe 2 shovel 3 hoe 4 sword 5 helmet 6 chest 7 legs 8 boots
    let gear: Option<(u8, u8)> = match item {
        23 => Some((0, 3)),
        24 => Some((1, 0)),
        25 => Some((1, 3)),
        26 => Some((1, 4)),
        27..=31 => Some((2, (item - 27) as u8)),
        32..=34 => Some((3, (item - 32) as u8)),
        35 => Some((4, 0)),
        36 => Some((4, 1)),
        37 => Some((4, 3)),
        38 => Some((4, 4)),
        52 => Some((5, 5)),
        53 => Some((7, 5)),
        54 => Some((8, 5)),
        55 => Some((5, 2)),
        56 => Some((7, 2)),
        57 => Some((8, 2)),
        58..=61 => Some((5 + (item - 58) as u8, 3)),
        62..=65 => Some((5 + (item - 62) as u8, 4)),
        _ => None,
    };
    if let Some((shape, m)) = gear {
        let dgl = x + y;
        return match shape {
            0 => {
                if pick.contains(&(x, y)) { metal(m, 0.95) } else if handle(2, 11) { wood(0.9) } else { NONE }
            }
            1 => {
                let head = match y {
                    2 => (8..=11).contains(&x),
                    3 | 4 => (7..=13).contains(&x),
                    5 => (8..=13).contains(&x),
                    6 => (9..=13).contains(&x),
                    7 => (10..=12).contains(&x),
                    _ => false,
                };
                if head { metal(m, if x >= 12 || y <= 2 { 1.08 } else { 0.9 }) } else if handle(2, 11) { wood(0.9) } else { NONE }
            }
            2 => {
                // shovel: rounded blade at the top right
                let (dx, dy) = (x as f32 - 11.0, y as f32 - 4.0);
                if dx * dx + dy * dy < 9.5 && dgl <= 17 { metal(m, if dgl <= 14 { 1.08 } else { 0.9 }) } else if handle(2, 10) { wood(0.9) } else { NONE }
            }
            3 => {
                // hoe: a short blade bent back from the tip
                if matches!((x, y), (9, 2) | (10, 2) | (11, 2) | (12, 2) | (9, 3) | (10, 3) | (11, 3) | (12, 3) | (13, 3) | (8, 3) | (8, 4) | (9, 4)) {
                    metal(m, if y == 2 { 1.08 } else { 0.9 })
                } else if handle(2, 12) {
                    wood(0.9)
                } else {
                    NONE
                }
            }
            4 => {
                if (dgl == 15 || dgl == 16) && x >= 6 {
                    metal(m, if dgl == 15 { 1.1 } else { 0.86 })
                } else if (y - x == 3 || y - x == 4) && (3..=7).contains(&x) {
                    shade([96.0, 80.0, 60.0], 1.0)
                } else if (dgl == 15 || dgl == 16) && x >= 2 {
                    wood(0.9)
                } else {
                    NONE
                }
            }
            5 => {
                // helmet: dome with an open face
                let dome = (3..=11).contains(&y) && (3..=12).contains(&x) && !(y == 3 && (x == 3 || x == 12));
                let face = y >= 8 && (6..=9).contains(&x);
                if dome && !face { metal(m, if y == 3 || x == 3 { 1.08 } else if x == 12 || y == 11 { 0.72 } else { 0.92 }) } else { NONE }
            }
            6 => {
                let body = (5..=13).contains(&y) && (4..=11).contains(&x);
                let shoulders = (2..=6).contains(&y) && (2..=13).contains(&x) && !((6..=9).contains(&x) && y <= 3);
                if body || shoulders {
                    let edge = x == 2 || x == 13 || y == 13 || (y == 6 && (x < 4 || x > 11));
                    metal(m, if edge { 0.7 } else if x == 7 || x == 8 { 1.08 } else { 0.92 })
                } else {
                    NONE
                }
            }
            7 => {
                // leggings: a belt and two legs
                let belt = (2..=4).contains(&y) && (3..=12).contains(&x);
                let legs = (5..=13).contains(&y) && ((3..=6).contains(&x) || (9..=12).contains(&x));
                if belt || legs { metal(m, if y == 2 { 1.08 } else if y == 4 || x == 6 || x == 12 { 0.74 } else { 0.92 }) } else { NONE }
            }
            _ => {
                // boots: two shafts with toes
                let shaft = (4..=12).contains(&y) && ((2..=5).contains(&x) || (9..=12).contains(&x));
                let toe = (10..=12).contains(&y) && ((6..=7).contains(&x) || (13..=14).contains(&x));
                if shaft || toe { metal(m, if y == 4 { 1.08 } else if y == 12 { 0.68 } else { 0.92 }) } else { NONE }
            }
        };
    }
    match item {
        39 => {
            // shears: two crossed blades with ring handles
            if (x == y || x + y == 15) && y <= 9 {
                iron(if x == y { 1.05 } else { 0.86 })
            } else if matches!((x, y), (4, 10) | (3, 11) | (5, 11) | (3, 12) | (5, 12) | (4, 13) | (11, 10) | (10, 11) | (12, 11) | (10, 12) | (12, 12) | (11, 13)) {
                shade([170.0, 60.0, 50.0], 1.0)
            } else {
                NONE
            }
        }
        40 | 41 => {
            // bucket, empty or full of water
            let w = if y <= 5 { 5 } else { 5 - (y - 5) / 3 };
            let inside = (4..=13).contains(&y) && (x as f32 - 7.5).abs() <= w as f32;
            if inside {
                if y == 4 || y == 5 {
                    if item == 41 && (x as f32 - 7.5).abs() < w as f32 - 0.6 { shade([56.0, 120.0, 214.0], 0.95 + n * 0.1) } else if (x as f32 - 7.5).abs() < w as f32 - 0.6 { shade([70.0, 72.0, 80.0], 1.0) } else { iron(1.05) }
                } else {
                    iron(if x <= 4 { 1.05 } else if x >= 11 { 0.7 } else { 0.88 })
                }
            } else if (y == 2 || y == 3) && (x == 3 || x == 12) || (y == 1 && (4..=11).contains(&x)) {
                iron(0.7)
            } else {
                NONE
            }
        }
        42 => {
            // fishing rod: a long pole with a line and float
            if x + y == 15 && x >= 2 {
                wood(0.9)
            } else if x == 13 && (3..=9).contains(&y) {
                [230, 230, 236, 255]
            } else if (12..=14).contains(&x) && (10..=11).contains(&y) {
                if y == 10 { [220, 50, 50, 255] } else { [240, 240, 240, 255] }
            } else {
                NONE
            }
        }
        43 | 44 => {
            // fish, raw or cooked
            let (dx, dy) = ((x as f32 - 8.5) / 5.6, (y as f32 - 8.0) / 3.4);
            let tail = (1..=3).contains(&x) && (y - 8).abs() <= 4 - x.min(4) + 1;
            if dx * dx + dy * dy < 1.0 || tail {
                if (x, y) == (11, 7) {
                    [20, 20, 24, 255]
                } else if item == 43 {
                    shade([110.0, 150.0, 190.0], if y > 8 { 1.15 } else { 0.85 } + n * 0.12)
                } else {
                    shade([200.0, 136.0, 72.0], if (x + y) % 4 == 0 { 0.7 } else { 0.95 } + n * 0.1)
                }
            } else {
                NONE
            }
        }
        45 => {
            // bow: curved limb and a straight string
            let (dx, dy) = (x as f32 - 3.0, y as f32 - 12.0);
            let d = (dx * dx + dy * dy).sqrt();
            if (d - 10.0).abs() < 0.9 && x >= 3 && y <= 12 {
                wood(0.9)
            } else if x + y == 15 && (3..=12).contains(&x) {
                [230, 230, 236, 255]
            } else {
                NONE
            }
        }
        46 => {
            // arrow
            let dgl = x + y;
            if matches!((x, y), (13, 1) | (14, 1) | (12, 2) | (13, 2) | (14, 2) | (12, 3) | (13, 3)) {
                shade([96.0, 100.0, 114.0], 1.0)
            } else if dgl == 15 && (4..=12).contains(&x) {
                wood(0.9)
            } else if (dgl == 14 || dgl == 16) && (2..=4).contains(&x) || (dgl == 15 && x < 4 && x >= 2) {
                [236, 236, 240, 255]
            } else {
                NONE
            }
        }
        47 | 48 => {
            // compass and clock: a round dial
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let d = dx * dx + dy * dy;
            if d < 42.0 && d >= 26.0 {
                if item == 47 { iron(0.85) } else { shade([250.0, 208.0, 66.0], 0.9 + n * 0.1) }
            } else if d < 26.0 {
                if item == 47 {
                    if x + y == 15 && y < 8 { [220, 50, 50, 255] } else if x + y == 15 { [236, 236, 240, 255] } else { [40, 44, 56, 255] }
                } else if (x == 7 && (4..=8).contains(&y)) || (y == 8 && (7..=10).contains(&x)) {
                    [40, 40, 46, 255]
                } else {
                    [240, 238, 226, 255]
                }
            } else {
                NONE
            }
        }
        49 => {
            // seeds
            if matches!((x, y), (4, 5) | (5, 5) | (4, 6) | (9, 4) | (10, 4) | (10, 5) | (7, 8) | (8, 8) | (7, 9) | (11, 10) | (12, 10) | (11, 11) | (4, 11) | (5, 11) | (5, 12)) {
                shade([150.0, 170.0, 70.0], 0.8 + n * 0.4)
            } else {
                NONE
            }
        }
        50 => {
            // wheat sheaf
            if (y == 9 || y == 10) && (5..=10).contains(&x) {
                shade([122.0, 86.0, 48.0], 0.9)
            } else if (2..=14).contains(&y) && (4..=11).contains(&x) && (x + (y - 9).abs() / 3) % 2 == 0 {
                shade([226.0, 186.0, 70.0], if y < 6 { 1.0 } else { 0.8 } + n * 0.2)
            } else {
                NONE
            }
        }
        51 => {
            // loaf of bread
            let (dx, dy) = ((x as f32 - 7.5) / 6.8, (y as f32 - 8.5) / 4.0);
            if dx * dx + dy * dy < 1.0 {
                if y > 10 { shade([150.0, 96.0, 44.0], 0.9) } else if (x + y) % 5 == 0 && y < 9 { shade([236.0, 200.0, 130.0], 1.0) } else { shade([204.0, 146.0, 72.0], 0.92 + n * 0.12) }
            } else {
                NONE
            }
        }
        0 => {
            if handle(3, 12) { wood(0.9) } else { NONE }
        }
        1 => {
            // meat: red cut with a bone end
            let (dx, dy) = ((x as f32 - 6.5) / 5.2, (y as f32 - 9.0) / 4.2);
            let d = dx * dx + dy * dy;
            if matches!((x, y), (12, 3) | (13, 3) | (12, 4) | (13, 2) | (11, 4) | (11, 5) | (14, 3)) {
                [240, 234, 220, 255]
            } else if d < 1.0 {
                if d > 0.72 { shade([150.0, 44.0, 44.0], 1.0) } else if n > 0.82 { [236, 190, 180, 255] } else { shade([204.0, 70.0, 66.0], 0.92 + n * 0.14) }
            } else {
                NONE
            }
        }
        2 => {
            // hide: a stretched pelt
            let inside = (3..=12).contains(&x) && (2..=13).contains(&y);
            let notch = ((x == 3 || x == 12) && (6..=9).contains(&y)) || ((y == 2 || y == 13) && (6..=9).contains(&x));
            let leg = matches!((x, y), (2, 2) | (13, 2) | (2, 13) | (13, 13) | (2, 3) | (13, 3) | (2, 12) | (13, 12));
            if (inside && !notch) || leg {
                let edge = x == 3 || x == 12 || y == 2 || y == 13 || leg;
                shade([176.0, 128.0, 80.0], if edge { 0.72 } else { 0.92 + n * 0.16 })
            } else {
                NONE
            }
        }
        3 | 4 | 11 | 14 => {
            if pick.contains(&(x, y)) {
                match item {
                    3 => plank(0.95),
                    4 => stone(0.92),
                    11 => iron(1.0),
                    _ => crystal(1.0),
                }
            } else if handle(2, 11) {
                wood(0.9)
            } else {
                NONE
            }
        }
        5 | 12 => {
            let stone = |k: f32| if item == 12 { iron(k) } else { stone(k) };
            let head = match y {
                2 => (8..=11).contains(&x),
                3 | 4 => (7..=13).contains(&x),
                5 => (8..=13).contains(&x),
                6 => (9..=13).contains(&x),
                7 => (10..=12).contains(&x),
                _ => false,
            };
            if head { stone(if x >= 12 || y <= 2 { 1.08 } else { 0.9 }) } else if handle(2, 11) { wood(0.9) } else { NONE }
        }
        6 => {
            // spear
            let tip = matches!((x, y), (13, 1) | (14, 1) | (12, 2) | (13, 2) | (14, 2) | (11, 3) | (12, 3) | (13, 3) | (11, 4) | (12, 4));
            if tip { stone(1.0) } else if handle(1, 11) && x + y == 15 { wood(0.9) } else { NONE }
        }
        7 | 9 | 15 => {
            // coal lump / crystal shard: a faceted blob
            let (dx, dy) = ((x as f32 - 7.5) / 5.4, (y as f32 - 8.0) / 4.8);
            let d = dx.abs() + dy.abs() * 0.9 + (dx * dy) * 0.6;
            if d < 1.0 {
                let facet = if (x + y) % 5 == 0 || x - y == 2 { 1.35 } else { 1.0 };
                if item == 7 {
                    shade([44.0, 44.0, 50.0], (0.8 + n * 0.4) * facet)
                } else if item == 15 {
                    if dx + dy > 0.5 { NONE } else { shade([84.0, 88.0, 100.0], (0.7 + n * 0.3) * facet) }
                } else {
                    shade([96.0, 214.0, 226.0], (0.82 + n * 0.2) * if d > 0.74 { 0.7 } else { facet.min(1.2) })
                }
            } else {
                NONE
            }
        }
        8 | 16 => {
            let iron = |k: f32| if item == 16 { shade([250.0, 206.0, 64.0], k + n * 0.08) } else { iron(k) };
            // ingot
            let inside = (5..=11).contains(&y) && x >= 2 + (11 - y) / 3 && x <= 13 - (11 - y) / 3;
            if inside {
                iron(if y == 5 { 1.12 } else if y >= 10 { 0.74 } else { 0.94 })
            } else {
                NONE
            }
        }
        10 => {
            // cooked meat
            let (dx, dy) = ((x as f32 - 6.5) / 5.2, (y as f32 - 9.0) / 4.2);
            let d = dx * dx + dy * dy;
            if matches!((x, y), (12, 3) | (13, 3) | (12, 4) | (13, 2) | (11, 4) | (11, 5) | (14, 3)) {
                [240, 234, 220, 255]
            } else if d < 1.0 {
                if d > 0.72 { shade([92.0, 52.0, 28.0], 1.0) } else if (x + y * 2) % 5 == 0 { shade([104.0, 60.0, 30.0], 1.0) } else { shade([166.0, 100.0, 50.0], 0.92 + n * 0.14) }
            } else {
                NONE
            }
        }
        13 => {
            // sword: blade along the diagonal, guard and grip
            let dgl = x + y;
            if (dgl == 15 || dgl == 16) && x >= 6 {
                iron(if dgl == 15 { 1.1 } else { 0.88 })
            } else if (y - x == 3 || y - x == 4) && (3..=7).contains(&x) {
                shade([96.0, 80.0, 60.0], 1.0)
            } else if (dgl == 15 || dgl == 16) && x >= 2 {
                wood(0.9)
            } else {
                NONE
            }
        }
        17 => {
            // fibre bundle
            if (3..=13).contains(&y) && (4..=11).contains(&x) && (y == 8 || y == 9) {
                shade([122.0, 86.0, 48.0], 0.9)
            } else if (2..=14).contains(&y) && (x + (y - 8).abs() / 3) % 3 == 1 && (3..=12).contains(&x) {
                shade([156.0, 178.0, 84.0], 0.8 + n * 0.3)
            } else {
                NONE
            }
        }
        18 => {
            // berries
            let b = |cx: f32, cy: f32| (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2) < 7.5;
            if matches!((x, y), (8, 2) | (9, 2) | (10, 3) | (9, 3) | (8, 4) | (7, 3)) {
                shade([62.0, 132.0, 52.0], 1.0)
            } else if b(5.0, 8.0) || b(10.5, 8.5) || b(7.5, 12.0) {
                if matches!((x, y), (4, 7) | (10, 7) | (7, 11)) { [255, 170, 190, 255] } else { shade([206.0, 40.0, 76.0], 0.86 + n * 0.2) }
            } else {
                NONE
            }
        }
        19 => {
            // stew in a bowl
            let (dx, dy) = ((x as f32 - 7.5) / 6.6, (y as f32 - 7.0) / 6.4);
            if y >= 7 && dx * dx + dy * dy < 1.0 {
                if y == 7 || (y == 8 && x.abs_diff(7) < 5) {
                    if (x + y) % 4 == 0 { shade([184.0, 62.0, 46.0], 1.0) } else { shade([196.0, 150.0, 90.0], 0.9 + n * 0.15) }
                } else {
                    wood(if y > 11 { 0.72 } else { 0.92 })
                }
            } else {
                NONE
            }
        }
        20 | 21 => {
            // body armour
            let body = (5..=13).contains(&y) && (4..=11).contains(&x);
            let shoulders = (2..=6).contains(&y) && (2..=13).contains(&x) && !((6..=9).contains(&x) && y <= 3);
            if body || shoulders {
                let edge = x == 2 || x == 13 || y == 13 || (y == 6 && (x < 4 || x > 11));
                let k = if edge { 0.7 } else if x == 7 || x == 8 { 1.08 } else { 0.92 };
                if item == 20 { shade([150.0, 98.0, 58.0], k + n * 0.1) } else { iron(k) }
            } else {
                NONE
            }
        }
        22 => {
            // flint knife
            let dgl = x + y;
            if (dgl == 15 || dgl == 16) && x >= 8 && x <= 13 {
                shade([96.0, 100.0, 114.0], if dgl == 15 { 1.15 } else { 0.85 })
            } else if (dgl == 15 || dgl == 16) && x >= 4 {
                wood(0.9)
            } else {
                NONE
            }
        }
        66 | 67 => {
            // door
            let c = if item == 66 { [178.0, 140.0, 86.0] } else { [218.0, 200.0, 152.0] };
            if !(4..=11).contains(&x) || !(1..=14).contains(&y) {
                NONE
            } else if x == 4 || x == 11 || y == 1 || y == 14 {
                shade(c, 0.62)
            } else if matches!(x, 6..=9) && matches!(y, 3..=6) {
                [176, 214, 236, 255]
            } else if x == 10 && y == 8 {
                [58, 56, 62, 255]
            } else if y == 9 || (y > 9 && (x == 6 || x == 9) && y < 13) {
                shade(c, 0.78)
            } else {
                shade(c, 0.92 + n * 0.1)
            }
        }
        70..=73 => {
            // stairs seen from the side: three steps of the block's own texture
            let t = pixel(SHAPE_TEX[item - 70], x, y);
            let step = 15 - (x / 5).min(2) * 5 - 5;
            if y < step {
                NONE
            } else if y == step || (x % 5 == 0 && x > 0 && x < 15 && y < step + 5) {
                lighten(t, 1.15)
            } else {
                darken(t, 0.88)
            }
        }
        100 => {
            // heart
            let (fx, fy) = ((x as f32 - 7.5) / 6.6, (9.2 - y as f32) / 6.6);
            let v = fx * fx + fy * fy - 1.0;
            if v * v * v - fx * fx * fy * fy * fy < 0.0 {
                if (x == 4 || x == 5) && (y == 4 || y == 5) { [255, 190, 190, 255] } else { shade([222.0, 44.0, 52.0], 0.94 + n * 0.1) }
            } else {
                NONE
            }
        }
        101 => {
            // drumstick
            let (dx, dy) = (x as f32 - 9.5, y as f32 - 6.0);
            if dx * dx + dy * dy < 20.0 {
                shade([190.0, 116.0, 58.0], if dx * dx + dy * dy > 13.0 { 0.72 } else { 0.94 + n * 0.12 })
            } else if (x + y == 16 || x + y == 17) && (2..=6).contains(&x) || matches!((x, y), (1, 13) | (2, 15) | (1, 14) | (3, 15)) {
                [240, 234, 220, 255]
            } else {
                NONE
            }
        }
        102 => {
            // air bubble
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let d = dx * dx + dy * dy;
            if d < 36.0 && d > 20.0 {
                [200, 230, 255, 255]
            } else if d <= 20.0 {
                if (x == 5 || x == 6) && y == 5 { [255, 255, 255, 255] } else { [120, 180, 240, 150] }
            } else {
                NONE
            }
        }
        _ => NONE,
    }
}

/// Returns one RGBA8 buffer per mip level; each holds all layers back to back.
pub fn build() -> Vec<(u32, Vec<u8>)> {
    let mut level: Vec<Vec<[u8; 4]>> = (0..LAYERS)
        .map(|l| {
            (0..TEX * TEX)
                .map(|i| pixel(l, (i % TEX) as i32, (i / TEX) as i32))
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    let mut size = TEX;
    loop {
        let mut bytes = Vec::with_capacity(LAYERS * size * size * 4);
        for l in &level {
            for p in l {
                bytes.extend_from_slice(p);
            }
        }
        out.push((size as u32, bytes));
        if size == 1 {
            break;
        }
        let half = size / 2;
        level = level
            .iter()
            .map(|l| {
                (0..half * half)
                    .map(|i| {
                        let (x, y) = (i % half * 2, i / half * 2);
                        let mut acc = [0u32; 4];
                        for (ox, oy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let p = l[(y + oy) * size + x + ox];
                            for c in 0..4 {
                                acc[c] += p[c] as u32;
                            }
                        }
                        [
                            (acc[0] / 4) as u8,
                            (acc[1] / 4) as u8,
                            (acc[2] / 4) as u8,
                            (acc[3] / 4) as u8,
                        ]
                    })
                    .collect()
            })
            .collect();
        size = half;
    }
    out
}
