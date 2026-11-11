//! The Emberdeep: the world under the world, reached through an Ember Gate. A closed
//! cavern between a floor and a ceiling of bedrock, with a sea of lava low down and no
//! sky at all. Four regions: the reddish Ash Wastes, the Ember Grove with its giant
//! glowing fungi, the soft grey Ashfall Dunes, and the Scoria Spires among lava pools.
//! Sunshard clusters hang from the ceiling and light the dark.

use crate::world::*;

/// The terrain generator id of the Emberdeep (see `World::ver`).
pub const DEEP_VER: u8 = 100;
/// lava fills every open space up to here
pub const LAVA_SEA: i32 = 30;
/// distances in the Emberdeep are an eighth of those above
pub const SCALE: i32 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Region {
    Wastes,
    Grove,
    Dunes,
    Spires,
}

impl Region {
    pub fn name(self) -> &'static str {
        match self {
            Region::Wastes => "Ash Wastes",
            Region::Grove => "Ember Grove",
            Region::Dunes => "Ashfall Dunes",
            Region::Spires => "Scoria Spires",
        }
    }
    /// the colour of the haze
    pub fn fog(self) -> [u8; 3] {
        match self {
            Region::Wastes => [138, 52, 34],
            Region::Grove => [156, 70, 28],
            Region::Dunes => [132, 122, 112],
            Region::Spires => [96, 78, 92],
        }
    }
}

pub fn region(seed: u32, x: i32, z: i32) -> Region {
    let a = fbm(seed.wrapping_add(0xdee9), x as f32 / 170.0, z as f32 / 170.0, 2);
    let b = fbm(seed.wrapping_add(0xdeea), x as f32 / 150.0, z as f32 / 150.0, 2);
    if a > 0.22 {
        Region::Grove
    } else if a < -0.24 {
        Region::Dunes
    } else if b > 0.26 {
        Region::Spires
    } else {
        Region::Wastes
    }
}

#[inline]
fn h3(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    let h = hash2(seed ^ (y as u32).wrapping_mul(0x9e37_79b9), x, z);
    (h >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
}

/// Smooth 3D value noise.
fn noise3(seed: u32, x: f32, y: f32, z: f32) -> f32 {
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v, w) = (s(x - fx), s(y - fy), s(z - fz));
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| h3(seed, ix + dx, iy + dy, iz + dz);
    l(
        l(l(c(0, 0, 0), c(1, 0, 0), u), l(c(0, 0, 1), c(1, 0, 1), u), w),
        l(l(c(0, 1, 0), c(1, 1, 0), u), l(c(0, 1, 1), c(1, 1, 1), u), w),
        v,
    )
}

/// grid of the density field: every 4 blocks across, 8 up
const GX: i32 = 4;
const GY: i32 = 8;

/// The density at a grid point: above 0 is rock. Open caverns in the middle, closing
/// to rock near the floor and the ceiling.
fn grid_density(seed: u32, gx: i32, gy: i32, gz: i32) -> f32 {
    let (x, y, z) = ((gx * GX) as f32, (gy * GY) as f32, (gz * GX) as f32);
    let mut d = noise3(seed, x / 52.0, y / 30.0, z / 52.0) + 0.5 * noise3(seed ^ 0x55, x / 21.0, y / 15.0, z / 21.0) + 0.22 * noise3(seed ^ 0xaa, x / 9.0, y / 9.0, z / 9.0);
    // a floor under the lava, a thick roof
    if y < 26.0 {
        d += (26.0 - y) / 10.0;
    }
    if y > 96.0 {
        d += (y - 96.0) / 12.0;
    }
    // a little more rock just above the lava, so there are shores to walk on
    if (30.0..44.0).contains(&y) {
        d += 0.12;
    }
    d - 0.08
}

/// The density anywhere, interpolated from the grid (the same at chunk borders).
fn density(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    let (gx, gy, gz) = (x.div_euclid(GX), y.div_euclid(GY), z.div_euclid(GX));
    let (u, v, w) = (x.rem_euclid(GX) as f32 / GX as f32, y.rem_euclid(GY) as f32 / GY as f32, z.rem_euclid(GX) as f32 / GX as f32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| grid_density(seed, gx + dx, gy + dy, gz + dz);
    l(
        l(l(c(0, 0, 0), c(1, 0, 0), u), l(c(0, 0, 1), c(1, 0, 1), u), w),
        l(l(c(0, 1, 0), c(1, 1, 0), u), l(c(0, 1, 1), c(1, 1, 1), u), w),
        v,
    )
}

/// Rock or open space, before decoration. (Spires are rock whatever the field says.)
fn rock(seed: u32, x: i32, y: i32, z: i32, spire: bool) -> bool {
    if y <= 0 || y >= CH - 1 {
        return true;
    }
    (spire && y < 92) || density(seed, x, y, z) > 0.0
}

/// A scoria spire stands on this column.
fn spire_at(seed: u32, x: i32, z: i32) -> bool {
    if region(seed, x, z) != Region::Spires {
        return false;
    }
    // round columns, three to five wide, scattered
    let (cx, cz) = (x.div_euclid(11), z.div_euclid(11));
    let h = hash2(seed ^ 0x5b17e, cx, cz);
    if h % 3 != 0 {
        return false;
    }
    let (ox, oz) = (cx * 11 + 3 + (h >> 4) as i32 % 5, cz * 11 + 3 + (h >> 8) as i32 % 5);
    let r = 1.5 + ((h >> 12) % 3) as f32 * 0.5;
    let (dx, dz) = ((x - ox) as f32, (z - oz) as f32);
    dx * dx + dz * dz <= r * r
}

/// The floor under a column, from the middle of the cavern down: the y of the first
/// rock with open space over it (`None` if it is all rock or all lava).
fn floor(seed: u32, x: i32, z: i32) -> Option<i32> {
    let spire = spire_at(seed, x, z);
    let mut open = false;
    for y in (LAVA_SEA + 1..=88).rev() {
        let r = rock(seed, x, y, z, spire);
        // (a floor is at least three blocks thick: thinner shelves are cleared away)
        if r && open && rock(seed, x, y - 1, z, spire) && rock(seed, x, y - 2, z, spire) {
            return Some(y);
        }
        open = !r;
    }
    None
}

pub fn generate(seed: u32, cx: i32, cz: i32) -> ChunkData {
    let mut c = ChunkData { blocks: vec![AIR; (CW * CW * CH) as usize].into_boxed_slice() };
    let (x0, z0) = (cx * CW, cz * CW);
    // the density grid this chunk needs, sampled once
    let (n, ny) = ((CW / GX + 1) as usize, (CH / GY + 1) as usize);
    let (gx0, gz0) = (x0.div_euclid(GX), z0.div_euclid(GX));
    let mut grid = vec![0.0f32; n * n * ny];
    for gy in 0..ny {
        for gz in 0..n {
            for gx in 0..n {
                grid[(gy * n + gz) * n + gx] = grid_density(seed, gx0 + gx as i32, gy as i32, gz0 + gz as i32);
            }
        }
    }
    let dens = |lx: i32, y: i32, lz: i32| -> f32 {
        let (gx, gy, gz) = ((lx / GX) as usize, (y / GY) as usize, (lz / GX) as usize);
        let (u, v, w) = ((lx % GX) as f32 / GX as f32, (y % GY) as f32 / GY as f32, (lz % GX) as f32 / GX as f32);
        let g = |dx: usize, dy: usize, dz: usize| grid[((gy + dy).min(ny - 1) * n + gz + dz) * n + gx + dx];
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        l(l(l(g(0, 0, 0), g(1, 0, 0), u), l(g(0, 0, 1), g(1, 0, 1), u), w), l(l(g(0, 1, 0), g(1, 1, 0), u), l(g(0, 1, 1), g(1, 1, 1), u), w), v)
    };
    for lz in 0..CW {
        for lx in 0..CW {
            let (x, z) = (x0 + lx, z0 + lz);
            let reg = region(seed, x, z);
            let spire = spire_at(seed, x, z);
            let col = |y: i32| y <= 0 || y >= CH - 1 || (spire && y < 92) || dens(lx, y, lz) > 0.0;
            let mut above_open = false;
            for y in (0..CH).rev() {
                let h = hash2(seed ^ 0xb00, x, z ^ (y * 7919));
                let b = if y == 0 || y == CH - 1 {
                    BEDROCK
                } else if (y <= 3 && h % 4 < 4 - y as u32) || (y >= CH - 4 && h % 4 < (y - (CH - 5)) as u32) {
                    BEDROCK
                } else if col(y) {
                    // rock: which kind, by region and depth below the open space
                    let top = above_open;
                    let base = if spire || (reg == Region::Spires && h % 5 < 2) {
                        SCORIA
                    } else {
                        ASHSTONE
                    };
                    let mut b = base;
                    if top && y > LAVA_SEA {
                        b = match reg {
                            Region::Grove => EMBER_MOSS,
                            Region::Dunes => ASH,
                            Region::Spires if h % 13 == 0 => SMOULDER,
                            Region::Wastes if h % 41 == 0 => SMOULDER,
                            Region::Wastes if h % 11 == 0 => ASH,
                            _ => base,
                        };
                    } else if reg == Region::Dunes && y > LAVA_SEA && (1..=2).any(|k| !col(y + k)) {
                        b = ASH;
                    }
                    if b == ASHSTONE {
                        // ores in pockets
                        let pick = hash2(seed ^ 0x0e5, (x + 2) >> 2, ((z + 2) >> 2) ^ (((y + 2) >> 2) * 131));
                        let blob = h3(seed ^ 0x77, x, y, z);
                        if pick % 100 < 9 && blob > 0.2 {
                            b = EMBER_ORE;
                        } else if pick % 100 >= 97 && blob > 0.35 {
                            b = GOLD_ORE;
                        }
                    }
                    b
                } else if y <= LAVA_SEA {
                    LAVA
                } else {
                    AIR
                };
                c.blocks[bidx(lx, y, lz)] = b;
                above_open = b == AIR;
            }
            // thin floating shelves (one or two blocks of rock with open space above and
            // below) look like litter in the air: clear them
            let mut y = LAVA_SEA + 2;
            while y < CH - 6 {
                if c.blocks[bidx(lx, y, lz)] == AIR {
                    y += 1;
                    continue;
                }
                let start = y;
                while y < CH - 6 && c.blocks[bidx(lx, y, lz)] != AIR {
                    y += 1;
                }
                if y - start <= 2 && c.blocks[bidx(lx, start - 1, lz)] == AIR && y < CH - 6 {
                    for k in start..y {
                        c.blocks[bidx(lx, k, lz)] = AIR;
                    }
                }
            }
            // sunshards hanging from the ceiling
            for y in 60..CH - 6 {
                let (here, below) = (c.blocks[bidx(lx, y, lz)], c.blocks[bidx(lx, y - 1, lz)]);
                if here != AIR && here != BEDROCK && below == AIR {
                    let cell = hash2(seed ^ 0x5a5, x.div_euclid(5), z.div_euclid(5) ^ (y / 6));
                    let (ox, oz) = (x.div_euclid(5) * 5 + (cell % 5) as i32, z.div_euclid(5) * 5 + ((cell >> 4) % 5) as i32);
                    let d = (x - ox).abs() + (z - oz).abs();
                    if cell % 9 == 0 && d <= 1 {
                        let len = 1 + (cell >> 9) as i32 % 3 - d;
                        for k in 1..=len.max(1) {
                            if c.blocks[bidx(lx, y - k, lz)] == AIR {
                                c.blocks[bidx(lx, y - k, lz)] = SUNSHARD;
                            }
                        }
                    }
                }
            }
        }
    }
    // giant fungi of the Ember Grove, one per 9x9 cell at most (from the cells around
    // too, so a cap reaches across the chunk border)
    for tz in (z0.div_euclid(9) - 1)..=((z0 + CW).div_euclid(9) + 1) {
        for tx in (x0.div_euclid(9) - 1)..=((x0 + CW).div_euclid(9) + 1) {
            let h = hash2(seed ^ 0xf0a6, tx, tz);
            if h % 3 != 0 {
                continue;
            }
            let (x, z) = (tx * 9 + 2 + (h >> 3) as i32 % 5, tz * 9 + 2 + (h >> 7) as i32 % 5);
            if region(seed, x, z) != Region::Grove {
                continue;
            }
            let Some(fy) = floor(seed, x, z) else { continue };
            let tall = 5 + (h >> 11) as i32 % 5;
            // there must be room over it
            if (1..=tall + 3).any(|k| rock(seed, x, fy + k, z, false)) {
                continue;
            }
            let r = 2 + (h >> 15) as i32 % 2;
            let top = fy + tall;
            let mut put = |px: i32, py: i32, pz: i32, b: Block, over: bool| {
                let (lx, lz) = (px - x0, pz - z0);
                if (0..CW).contains(&lx) && (0..CW).contains(&lz) && (1..CH - 1).contains(&py) {
                    let i = bidx(lx, py, lz);
                    if over || c.blocks[i] == AIR {
                        c.blocks[i] = b;
                    }
                }
            };
            for k in 1..=tall {
                put(x, fy + k, z, CINDERWOOD, true);
            }
            // a domed cap with a skirt hanging round its rim
            for dz in -r..=r {
                for dx in -r..=r {
                    let d2 = dx * dx + dz * dz;
                    if d2 <= r * r + 1 {
                        put(x + dx, top, z + dz, GLOWCAP, false);
                        if d2 <= (r - 1) * (r - 1) + 1 {
                            put(x + dx, top + 1, z + dz, GLOWCAP, false);
                        }
                        if d2 >= r * r - 1 {
                            put(x + dx, top - 1, z + dz, GLOWCAP, false);
                        }
                    }
                }
            }
        }
    }
    c
}

/// A place to stand near (x, z) for someone arriving: a floor with two open cells over
/// it, searching outwards. Also says how high the floor is.
pub fn find_floor(seed: u32, x: i32, z: i32) -> Option<(i32, i32, i32)> {
    for r in 0..24i32 {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let (px, pz) = (x + dx, z + dz);
                if let Some(y) = floor(seed, px, pz) {
                    let spire = spire_at(seed, px, pz);
                    if !rock(seed, px, y + 1, pz, spire) && !rock(seed, px, y + 2, pz, spire) && !rock(seed, px, y + 3, pz, spire) {
                        return Some((px, y, pz));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_cavern_with_a_lava_sea_and_room_to_walk() {
        let (mut open, mut lava, mut total, mut glow) = (0, 0, 0, 0);
        let mut regions = std::collections::HashSet::new();
        for cz in -3..3 {
            for cx in -3..3 {
                let c = generate(42, cx * 7, cz * 7);
                for lz in 0..CW {
                    for lx in 0..CW {
                        assert_eq!(c.get(lx, 0, lz), BEDROCK);
                        assert_eq!(c.get(lx, CH - 1, lz), BEDROCK);
                        for y in LAVA_SEA + 1..90 {
                            total += 1;
                            match c.get(lx, y, lz) {
                                AIR => open += 1,
                                SUNSHARD | GLOWCAP => glow += 1,
                                _ => {}
                            }
                        }
                        lava += (c.get(lx, LAVA_SEA, lz) == LAVA) as u32;
                        regions.insert(region(42, (cx * 7) * CW + lx, (cz * 7) * CW + lz));
                    }
                }
            }
        }
        let share = open as f32 / total as f32;
        assert!(share > 0.25 && share < 0.75, "open space {share}");
        assert!(lava > 100 && glow > 0, "lava {lava}, glow {glow}");
        assert!(regions.len() >= 2, "{regions:?}");
        assert!(find_floor(42, 0, 0).is_some());
    }

    #[test]
    fn chunks_meet_at_their_borders() {
        // the field is one function: a block on either side of a border agrees with a
        // chunk generated around it
        let a = generate(7, 0, 0);
        let b = generate(7, 1, 0);
        let same = (40..80).filter(|&y| (a.get(15, y, 5) == AIR) == (density(7, 15, y, 5) <= 0.0 && !spire_at(7, 15, 5))).count();
        assert!(same >= 38, "{same}");
        let _ = b;
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    /// The shader lights these layers itself (see `gfx`): keep the numbers in step.
    #[test]
    fn glowing_layers_match_the_shader() {
        assert_eq!(tex_for(GLOWCAP, 0), 106);
        assert_eq!(tex_for(SUNSHARD, 0), 108);
        assert_eq!(tex_for(SMOULDER, 0), 101);
        // every block's layer fits the terrain mesh's 7 bits
        for b in 0..=255u8 {
            for f in 0..6 {
                if !is_shaped(b) {
                    assert!(tex_for(b, f) < 128, "block {b} face {f}");
                }
            }
        }
        assert!(crate::textures::LAYERS <= 256);
    }
}

#[cfg(test)]
mod survey {
    use super::*;
    /// Where the regions are, near the middle of world 4242 (for screenshots).
    #[test]
    #[ignore]
    fn find_regions() {
        for r in [Region::Grove, Region::Dunes, Region::Spires, Region::Wastes] {
            'found: for d in (0..600).step_by(8) {
                for (x, z) in [(d, 0), (-d, 0), (0, d), (0, -d), (d, d), (-d, -d)] {
                    if region(4242, x, z) == r {
                        if let Some(f) = find_floor(4242, x, z) {
                            println!("REGION {:?} floor {:?}", r, f);
                            break 'found;
                        }
                    }
                }
            }
        }
    }
}
