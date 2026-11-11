//! World generation, second version: the land is laid out the way Minecraft 1.18+ does
//! it. Slow noise maps (how far inland, how eroded, how warm, how wet) cover the world;
//! the first two shape the ground, and all of them together pick the biome of a column,
//! so seas, deserts, jungles and snowfields lie in broad regions that shade into each
//! other as you travel. Caves and ores are those of the first version (`world`).

use crate::world::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Biome {
    DeepOcean,
    Ocean,
    WarmOcean,
    FrozenOcean,
    River,
    FrozenRiver,
    Beach,
    Plains,
    Meadow,
    Forest,
    BirchForest,
    Taiga,
    SnowyTaiga,
    SnowyPlains,
    Desert,
    Badlands,
    Savanna,
    Jungle,
    Swamp,
    Peaks,
}
#[cfg(test)]
const BIOMES: usize = Biome::Peaks as usize + 1;

impl Biome {
    pub fn name(self) -> &'static str {
        match self {
            Biome::DeepOcean => "Deep ocean",
            Biome::Ocean => "Ocean",
            Biome::WarmOcean => "Warm ocean",
            Biome::FrozenOcean => "Frozen ocean",
            Biome::River => "River",
            Biome::FrozenRiver => "Frozen river",
            Biome::Beach => "Beach",
            Biome::Plains => "Plains",
            Biome::Meadow => "Meadow",
            Biome::Forest => "Forest",
            Biome::BirchForest => "Birch forest",
            Biome::Taiga => "Taiga",
            Biome::SnowyTaiga => "Snowy taiga",
            Biome::SnowyPlains => "Snowy plains",
            Biome::Desert => "Desert",
            Biome::Badlands => "Badlands",
            Biome::Savanna => "Savanna",
            Biome::Jungle => "Jungle",
            Biome::Swamp => "Swamp",
            Biome::Peaks => "Peaks",
        }
    }
    /// green country a new player can start in
    pub fn mild(self) -> bool {
        matches!(self, Biome::Plains | Biome::Meadow | Biome::Forest | Biome::BirchForest)
    }
}

/// What the generator knows about one column of the world.
#[derive(Clone, Copy)]
pub struct Col {
    /// y of the top solid block
    pub h: i32,
    pub biome: Biome,
    /// warmth at the surface, colder the higher the ground: about -2 (polar) .. 2 (torrid)
    pub temp: f32,
}

// the noise maps are divided by their spread, so that thresholds read in standard
// deviations: half of the world lies below 0, a sixth below -1 and a sixth above 1
const SIG2: f32 = 0.241;
const SIG3: f32 = 0.212;
const SIG4: f32 = 0.199;
const SIG5: f32 = 0.193;

/// below this warmth the ground is snowed over and still water freezes
const FROZEN: f32 = -1.0;
const COLD: f32 = -0.35;
const WARM: f32 = 0.45;
const HOT: f32 = 0.95;
/// how much colder a block higher up is, above `LAPSE_FROM`
const LAPSE: f32 = 0.032;
const LAPSE_FROM: f32 = 58.0;

/// Base height of the ground for a value of the "how far inland" map: deep sea floor,
/// shelf, shore, lowland, highland.
fn continent(c: f32) -> f32 {
    const P: [(f32, f32); 10] = [(-2.4, 9.0), (-1.6, 15.0), (-1.0, 27.0), (-0.58, 35.5), (-0.45, 39.6), (-0.34, 42.6), (-0.1, 46.0), (0.5, 51.0), (1.2, 58.0), (2.2, 64.0)];
    if c <= P[0].0 {
        return P[0].1;
    }
    for w in P.windows(2) {
        if c < w[1].0 {
            return w[0].1 + (w[1].1 - w[0].1) * (c - w[0].0) / (w[1].0 - w[0].0);
        }
    }
    P[9].1
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The slow maps at a point: inland, erosion, warmth, wetness, rivers, ridges.
type Fields = [f32; 6];
/// They are worked out at every `GRID`th block and filled in between.
const GRID: i32 = 4;

fn fields(seed: u32, x: i32, z: i32) -> Fields {
    let (xf, zf) = (x as f32, z as f32);
    let n = |k: u32, scale: f32, oct: u32| fbm(seed.wrapping_add(k), xf / scale, zf / scale, oct);
    [
        // (shifted towards land: a little under three tenths of the world is sea)
        n(20_000, 900.0, 5) / SIG5 + 0.15,
        n(21_000, 520.0, 3) / SIG3,
        n(22_000, 700.0, 3) / SIG3,
        n(23_000, 460.0, 3) / SIG3,
        n(26_000, 430.0, 2),
        n(27_000, 190.0, 4) / SIG4,
    ]
}

/// The slow maps over a square of columns from (x0, z0), `w` on a side.
struct FieldGrid {
    gx: i32,
    gz: i32,
    n: i32,
    v: Vec<Fields>,
}

impl FieldGrid {
    fn new(seed: u32, x0: i32, z0: i32, w: i32) -> FieldGrid {
        let (gx, gz) = (x0.div_euclid(GRID), z0.div_euclid(GRID));
        let n = ((x0 + w - 1).div_euclid(GRID) - gx).max((z0 + w - 1).div_euclid(GRID) - gz) + 2;
        let v = (0..n * n).map(|i| fields(seed, (gx + i % n) * GRID, (gz + i / n) * GRID)).collect();
        FieldGrid { gx, gz, n, v }
    }
    fn at(&self, x: i32, z: i32) -> Fields {
        let (ix, iz) = (x.div_euclid(GRID) - self.gx, z.div_euclid(GRID) - self.gz);
        let (u, w) = (x.rem_euclid(GRID) as f32 / GRID as f32, z.rem_euclid(GRID) as f32 / GRID as f32);
        let p = |dx: i32, dz: i32| &self.v[((iz + dz) * self.n + ix + dx) as usize];
        let (a, b, c, d) = (p(0, 0), p(1, 0), p(0, 1), p(1, 1));
        std::array::from_fn(|k| lerp(lerp(a[k], b[k], u), lerp(c[k], d[k], u), w))
    }
}

pub fn column(seed: u32, x: i32, z: i32) -> Col {
    column_in(seed, x, z, &FieldGrid::new(seed, x, z, 1))
}

fn column_in(seed: u32, x: i32, z: i32, grid: &FieldGrid) -> Col {
    let (xf, zf) = (x as f32, z as f32);
    let n = |k: u32, scale: f32, oct: u32| fbm(seed.wrapping_add(k), xf / scale, zf / scale, oct);
    let [c, e, t, hu, rv, r] = grid.at(x, z);
    let hills = n(24_000, 95.0, 3) / SIG3;
    let detail = n(25_000, 26.0, 2) / SIG2;

    let mut h = continent(c);
    let land = smoothstep(-0.9, -0.3, c);
    let rough = smoothstep(0.7, -0.8, e);
    h += hills * (1.0 + land * (1.2 + 4.6 * rough)) + detail * 0.75;

    // mountain ranges: inland, where the land is little eroded
    let m = smoothstep(-0.3, 0.5, c) * smoothstep(-0.1, -1.1, e);
    if m > 0.0 {
        let ridge = (1.0 - r.abs() * 0.85).max(0.0);
        h += m * (9.0 + ridge * ridge * 64.0);
    }

    // swamps: warm, wet, worn flat country settles to the water line, half drowned
    let sw = smoothstep(0.25, 0.75, hu) * smoothstep(0.0, 0.6, e) * smoothstep(-0.5, -0.2, t) * smoothstep(1.1, 0.8, t) * smoothstep(-0.5, -0.42, c) * smoothstep(0.7, 0.3, c);
    if sw > 0.0 {
        let pool = fbm(seed.wrapping_add(28_000), xf / 13.0, zf / 13.0, 2) / SIG2;
        h = lerp(h, SEA as f32 + 0.45 + pool * 0.9, sw);
    }

    // badlands: hot, dry, rugged country stands in terraces
    let arid = smoothstep(HOT - 0.1, HOT + 0.1, t) * smoothstep(0.3, 0.1, hu);
    let bad = arid * smoothstep(-0.15, -0.4, e);
    if bad > 0.0 && h > 45.0 {
        let f = (h - 45.0) / 7.0;
        let step = 45.0 + 7.0 * (f.floor() + smoothstep(0.72, 1.0, f.fract())) + 3.0;
        h = lerp(h, step, bad);
    }

    // rivers follow the zero line of a map of their own, cutting down to sea level;
    // the valley is wider the higher the land it runs through
    let mut river = 9.0f32;
    if c > -0.75 {
        let rv = rv.abs();
        let tr = rv / (0.03 + (h - 44.0).max(0.0) * 0.0009);
        if tr < 1.0 {
            // (the high ranges are left whole)
            let k = smoothstep(0.24, 1.0, tr).max(smoothstep(0.45, 0.75, m));
            let bed = SEA as f32 - 3.4 + detail * 0.5;
            if h > bed {
                h = bed + (h - bed) * k;
            }
            river = tr;
        }
    }

    // the highest peaks level off gently instead of being cut flat at the build limit
    if h > 96.0 {
        h = 96.0 + 20.0 * (1.0 - (-(h - 96.0) / 20.0).exp());
    }
    let hi = (h as i32).clamp(3, CH - 12);

    let temp = t - (hi as f32 - LAPSE_FROM).max(0.0) * LAPSE;
    // (borders are frayed a little, block by block)
    let r = hash2(seed ^ 0x3b1e_55a7, x, z);
    let jt = temp + ((r & 255) as f32 / 255.0 - 0.5) * 0.14;
    let jh = hu + (((r >> 8) & 255) as f32 / 255.0 - 0.5) * 0.14;
    let biome = if hi < SEA {
        if sw > 0.5 {
            Biome::Swamp
        } else if river < 0.6 && c > -0.4 {
            if jt < FROZEN { Biome::FrozenRiver } else { Biome::River }
        } else if jt < FROZEN - 0.1 {
            Biome::FrozenOcean
        } else if hi < SEA - 15 {
            Biome::DeepOcean
        } else if jt > 0.6 {
            Biome::WarmOcean
        } else {
            Biome::Ocean
        }
    } else if sw > 0.5 {
        Biome::Swamp
    } else if hi <= SEA + 2 && c < -0.3 {
        Biome::Beach
    } else if hi as f32 > 86.0 + t * 9.0 + ((r >> 16) & 3) as f32 {
        Biome::Peaks
    } else if jt < FROZEN {
        if jh > 0.2 { Biome::SnowyTaiga } else { Biome::SnowyPlains }
    } else if jt < COLD {
        if jh > -0.35 { Biome::Taiga } else { Biome::Plains }
    } else if jt < WARM {
        if jh < -0.95 {
            Biome::Meadow
        } else if jh < -0.1 {
            Biome::Plains
        } else if jh < 0.6 {
            Biome::Forest
        } else if jh < 1.15 {
            Biome::BirchForest
        } else {
            Biome::Forest
        }
    } else if jt < HOT {
        if jh < 0.0 {
            Biome::Savanna
        } else if jh < 0.45 {
            Biome::Forest
        } else {
            Biome::Jungle
        }
    } else if jh < 0.2 {
        if bad > 0.5 { Biome::Badlands } else { Biome::Desert }
    } else {
        Biome::Jungle
    };
    Col { h: hi, biome, temp }
}

pub fn height_at(seed: u32, x: i32, z: i32) -> i32 {
    column(seed, x, z).h
}

/// The columns of a square from (x0, z0), `w` on a side, row by row.
pub(crate) fn columns(seed: u32, x0: i32, z0: i32, w: i32) -> Vec<Col> {
    let grid = FieldGrid::new(seed, x0, z0, w);
    (0..w * w).map(|i| column_in(seed, x0 + i % w, z0 + i / w, &grid)).collect()
}

/// Where a new world's player starts: the nearest spot to the origin on fairly level
/// green land of a mild biome. Returns the column and its height.
pub fn spawn_point(seed: u32) -> (i32, i32, i32) {
    let ok = |x: i32, z: i32, strict: bool| -> Option<i32> {
        let c = column(seed, x, z);
        if c.h <= SEA + 2 || c.h >= 78 {
            return None;
        }
        if strict {
            let level = [(6, 0), (-6, 0), (0, 6), (0, -6)].iter().all(|&(dx, dz)| (height_at(seed, x + dx, z + dz) - c.h).abs() <= 3);
            if !level || !c.biome.mild() {
                return None;
            }
        }
        Some(c.h)
    };
    // rings of points 16 apart, growing outwards; first the choosy pass, then any land
    for strict in [true, false] {
        for ring in 0..160i32 {
            for i in -ring..=ring {
                for (x, z) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                    if let Some(h) = ok(x * 16, z * 16, strict) {
                        return (x * 16, h, z * 16);
                    }
                }
            }
        }
    }
    (0, height_at(seed, 0, 0).max(SEA), 0)
}

/// Layers of the badlands, by height.
fn strata(y: i32) -> Block {
    const S: [Block; 14] = [
        TERRACOTTA, TERRACOTTA, TERRACOTTA_PALE, TERRACOTTA, TERRACOTTA_DARK, TERRACOTTA, TERRACOTTA, RED_SAND, TERRACOTTA, TERRACOTTA_DARK, TERRACOTTA_PALE,
        TERRACOTTA, TERRACOTTA, TERRACOTTA_DARK,
    ];
    S[y.rem_euclid(14) as usize]
}

#[derive(Clone, Copy, PartialEq)]
enum Tree {
    Oak,
    Birch,
    Pine,
    SnowyPine,
    Acacia,
    Jungle,
    Bush,
    Swamp,
}

/// Makes a chunk; `villages` adds those of generator 3 (see `GEN_LATEST`).
pub fn generate(seed: u32, cx: i32, cz: i32, villages: bool) -> ChunkData {
    let mut blocks = vec![AIR; VOL].into_boxed_slice();
    // columns of the chunk plus an apron: 3 for the trees that reach across the border,
    // and one more for the slope under them
    const A: i32 = 4;
    const T: i32 = A - 1;
    const HW: i32 = CW + 2 * A;
    let grid = FieldGrid::new(seed, cx * CW - A, cz * CW - A, HW);
    let cols: Vec<Col> = (0..HW * HW).map(|i| column_in(seed, cx * CW + i % HW - A, cz * CW + i / HW - A, &grid)).collect();
    let col = |x: i32, z: i32| cols[((z + A) * HW + x + A) as usize];
    let hget = |x: i32, z: i32| col(x, z).h;
    let frozen = |x: i32, z: i32| {
        let r = hash2(seed ^ 0x1ce0_f00d, cx * CW + x, cz * CW + z);
        col(x, z).temp + ((r & 255) as f32 / 255.0 - 0.5) * 0.1 < FROZEN
    };

    // (top block, the block under it, how many blocks down that goes) of a column
    let surface = |x: i32, z: i32| -> (Block, Block, i32) {
        let c = col(x, z);
        let h = c.h;
        let (wx, wz) = (cx * CW + x, cz * CW + z);
        if h <= SEA + 1 {
            // beds and shores
            let g = fbm(seed.wrapping_add(11000), wx as f32 / 38.0, wz as f32 / 38.0, 2);
            return match c.biome {
                Biome::Swamp if h >= SEA => (SWAMP_GRASS, DIRT, 3),
                Biome::Swamp => {
                    if g < -0.2 {
                        (CLAY, DIRT, 3)
                    } else {
                        (DIRT, DIRT, 3)
                    }
                }
                Biome::Jungle if h >= SEA => (LUSH_GRASS, DIRT, 3),
                Biome::Badlands => (RED_SAND, RED_SAND, 3),
                Biome::WarmOcean | Biome::Desert | Biome::Beach => (SAND, SAND, 3),
                Biome::FrozenOcean | Biome::FrozenRiver if g > -0.1 => (GRAVEL, GRAVEL, 3),
                _ => {
                    let b = if g > 0.3 {
                        GRAVEL
                    } else if g < -0.36 && h < SEA {
                        CLAY
                    } else {
                        SAND
                    };
                    (b, b, 3)
                }
            };
        }
        let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|&(dx, dz)| (hget(x + dx, z + dz) - h).abs()).max().unwrap_or(0);
        let cold = frozen(x, z);
        match c.biome {
            Biome::Peaks => {
                if cold && steep < 6 {
                    (SNOW, SNOW, 2)
                } else {
                    (STONE, STONE, 0)
                }
            }
            Biome::Desert => (SAND, SAND, 3),
            Biome::Beach => {
                if steep >= 3 {
                    (STONE, STONE, 0)
                } else {
                    (SAND, SAND, 3)
                }
            }
            Biome::Badlands => {
                if steep < 2 {
                    (RED_SAND, RED_SAND, 2)
                } else {
                    (STONE, STONE, 0)
                }
            }
            // cliffs show their rock
            _ if steep >= 4 && h > 60 => (STONE, STONE, 0),
            _ if cold => (SNOWY_GRASS, DIRT, 4),
            Biome::Savanna => (DRY_GRASS, DIRT, 4),
            Biome::Jungle => (LUSH_GRASS, DIRT, 4),
            Biome::Swamp => (SWAMP_GRASS, DIRT, 4),
            _ => (GRASS, DIRT, 4),
        }
    };

    for z in 0..CW {
        for x in 0..CW {
            let c = col(x, z);
            let h = c.h;
            let (wx, wz) = (cx * CW + x, cz * CW + z);
            let (top, under, depth) = surface(x, z);
            // the bands of the badlands sag and rise a little across the country
            let band = if c.biome == Biome::Badlands { (fbm(seed.wrapping_add(13000), wx as f32 / 70.0, wz as f32 / 70.0, 2) * 9.0) as i32 } else { 0 };
            // lowest neighbouring surface: caves stay clear of sea and lake beds
            let low = h.min(hget(x - 1, z)).min(hget(x + 1, z)).min(hget(x, z - 1)).min(hget(x, z + 1));
            // the cavern field up the column, at y = 0, 4, 8, ...
            let mut halls = [0.0f32; (CAVERN_TOP / 4 + 2) as usize];
            for (k, v) in halls.iter_mut().enumerate() {
                *v = cavern_field(seed, wx, k as i32 * 4, wz);
            }
            // and the tunnel fields at y = 0, 2, 4, ...
            let mut fields = [(0.0f32, 0.0f32); (CH / 2 + 1) as usize];
            for (k, v) in fields.iter_mut().enumerate().take((h / 2 + 2) as usize) {
                *v = tunnel_fields(seed, wx, k as i32 * 2, wz);
            }
            let tunnel = |y: i32| {
                let (p, q) = (fields[(y / 2) as usize], fields[(y / 2 + y % 2) as usize]);
                in_tunnel(seed, wx, y, wz, (p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5)
            };
            let cavern = |y: i32| {
                let (k, t) = ((y / 4) as usize, (y % 4) as f32 * 0.25);
                y < low - 7 && y <= CAVERN_TOP && in_cavern(seed, wx, y, wz, halls[k] + (halls[k + 1] - halls[k]) * t)
            };
            for y in 0..=h {
                let mut b = if y == 0 {
                    BEDROCK
                } else if y == h {
                    top
                } else if y > h - depth {
                    under
                } else if c.biome == Biome::Desert && y > h - 7 {
                    SANDSTONE
                } else {
                    STONE
                };
                if c.biome == Biome::Badlands && b == STONE && y > h - 18 && h > SEA + 1 {
                    b = strata(y + band);
                }
                // caves; the caverns keep a roof of rock under the land and the sea bed
                if b != BEDROCK && !(y <= SEA + 1 && y >= low - 4) && (tunnel(y) || cavern(y)) {
                    if y <= LAVA_LEVEL {
                        blocks[bidx(x, y, z)] = LAVA;
                    }
                    continue;
                }
                if b == STONE && y < h - 3 {
                    b = ore_at(seed, wx, y, wz);
                }
                blocks[bidx(x, y, z)] = b;
            }
            for y in (h + 1)..=SEA {
                blocks[bidx(x, y, z)] = WATER;
            }
            if h < SEA && frozen(x, z) {
                if c.biome == Biome::FrozenOcean || c.biome == Biome::DeepOcean || c.biome == Biome::Ocean {
                    // pack ice with open leads, and here and there an iceberg
                    let f = fbm(seed.wrapping_add(14000), wx as f32 / 34.0, wz as f32 / 34.0, 3) / SIG3;
                    if f > -0.5 {
                        blocks[bidx(x, SEA, z)] = ICE;
                    }
                    if f > 1.35 && h < SEA - 4 {
                        let up = ((f - 1.35) * 14.0) as i32;
                        for y in SEA - 2 - up / 2..=SEA + up {
                            blocks[bidx(x, y.max(h + 1), z)] = if y == SEA + up && up > 0 { SNOW } else { ICE };
                        }
                    }
                } else {
                    blocks[bidx(x, SEA, z)] = ICE;
                }
            }
        }
    }

    if let Some((x, y, z)) = cave_chest(seed, cx, cz, &blocks, &hget) {
        blocks[bidx(x, y, z)] = LOOT_CHEST;
    }

    // trees
    for tz in -T..CW + T {
        for tx in -T..CW + T {
            let c = col(tx, tz);
            let h = c.h;
            let (base, min) = match c.biome {
                Biome::Plains => (0.0012, 0.0),
                Biome::Meadow | Biome::SnowyPlains => (0.0006, 0.0),
                Biome::Forest | Biome::BirchForest => (0.014, 0.085),
                Biome::Taiga | Biome::SnowyTaiga => (0.012, 0.075),
                Biome::Savanna => (0.003, 0.008),
                Biome::Jungle => (0.035, 0.09),
                Biome::Swamp => (0.014, 0.0),
                _ => continue,
            };
            let (wx, wz) = (cx * CW + tx, cz * CW + tz);
            let r = hash2(seed ^ 0x9e37_79b9, wx, wz);
            let roll = ((r & 0xffff) as f32) / 65536.0;
            if roll >= base + min * 0.6 {
                continue;
            }
            let forest = fbm(seed.wrapping_add(5000), wx as f32 / 110.0, wz as f32 / 110.0, 2);
            if roll >= base + (forest + 0.1).clamp(0.0, 0.6) * min {
                continue;
            }
            let wading = c.biome == Biome::Swamp && h >= SEA - 1;
            if !(wading || is_grassy(surface(tx, tz).0)) || is_cave(seed, wx, h, wz) {
                continue;
            }
            let cold = frozen(tx, tz);
            let pick = (r >> 20) % 100;
            let kind = match c.biome {
                Biome::Taiga | Biome::SnowyTaiga | Biome::SnowyPlains => {
                    if cold { Tree::SnowyPine } else { Tree::Pine }
                }
                Biome::BirchForest => {
                    if pick < 88 { Tree::Birch } else { Tree::Oak }
                }
                Biome::Savanna => Tree::Acacia,
                Biome::Jungle => {
                    if pick < 42 { Tree::Jungle } else { Tree::Bush }
                }
                Biome::Swamp => Tree::Swamp,
                _ => {
                    if pick < 22 { Tree::Birch } else { Tree::Oak }
                }
            };
            let mut put = |x: i32, y: i32, z: i32, b: Block, force: bool| {
                if x >= 0 && x < CW && z >= 0 && z < CW && y > 0 && y < CH {
                    let i = bidx(x, y, z);
                    if force || blocks[i] == AIR {
                        blocks[i] = b;
                    }
                }
            };
            let pick = ((r >> 16) % 16) as i32;
            match kind {
                Tree::Pine | Tree::SnowyPine => {
                    // a tall trunk with tiers of needles narrowing to a point
                    let leaves = if kind == Tree::Pine { PINE_LEAVES } else { SNOWY_LEAVES };
                    let th = 6 + pick % 3;
                    for dy in 2..=th + 1 {
                        let rad: i32 = if dy == th + 1 {
                            0
                        } else if dy == th {
                            1
                        } else {
                            1 + (th - dy) % 2
                        };
                        for dz in -rad..=rad {
                            for dx in -rad..=rad {
                                if rad > 0 && dx.abs() == rad && dz.abs() == rad {
                                    continue;
                                }
                                put(tx + dx, h + dy, tz + dz, leaves, false);
                            }
                        }
                    }
                    for dy in 1..=th {
                        put(tx, h + dy, tz, LOG, true);
                    }
                }
                Tree::Oak | Tree::Birch => {
                    let birch = kind == Tree::Birch;
                    let (log, leaves) = if birch { (BIRCH_LOG, BIRCH_LEAVES) } else { (LOG, LEAVES) };
                    let th = 4 + pick % 3 + birch as i32;
                    for dy in (th - 2)..=(th + 1) {
                        let rad: i32 = if dy <= th - 1 { 2 } else { 1 };
                        for dz in -rad..=rad {
                            for dx in -rad..=rad {
                                if dx.abs() == rad && dz.abs() == rad {
                                    let keep = rad == 2 && dy == th - 2 && hash2(seed ^ 77, wx + dx * 7, wz + dz * 13 + dy) & 1 == 0;
                                    if !keep {
                                        continue;
                                    }
                                }
                                put(tx + dx, h + dy, tz + dz, leaves, false);
                            }
                        }
                    }
                    for dy in 1..=th {
                        put(tx, h + dy, tz, log, true);
                    }
                }
                Tree::Acacia => {
                    // a bare trunk under a wide flat crown
                    let th = 4 + pick % 3;
                    for dz in -3i32..=3 {
                        for dx in -3i32..=3 {
                            let d = dx * dx + dz * dz;
                            if d <= 10 {
                                put(tx + dx, h + th, tz + dz, ACACIA_LEAVES, false);
                            }
                            if d <= 2 {
                                put(tx + dx, h + th + 1, tz + dz, ACACIA_LEAVES, false);
                            }
                        }
                    }
                    for dy in 1..=th {
                        put(tx, h + dy, tz, LOG, true);
                    }
                }
                Tree::Jungle => {
                    // a giant: a long trunk and a broad rounded crown
                    let th = 8 + pick % 7;
                    for (dy, lim) in [(-2, 5), (-1, 10), (0, 10), (1, 5), (2, 1)] {
                        for dz in -3i32..=3 {
                            for dx in -3i32..=3 {
                                if dx * dx + dz * dz <= lim {
                                    put(tx + dx, h + th + dy, tz + dz, JUNGLE_LEAVES, false);
                                }
                            }
                        }
                    }
                    for dy in 1..=th {
                        put(tx, h + dy, tz, LOG, true);
                    }
                }
                Tree::Bush => {
                    for (dy, lim) in [(1, 2), (2, 1)] {
                        for dz in -1i32..=1 {
                            for dx in -1i32..=1 {
                                if dx * dx + dz * dz <= lim {
                                    put(tx + dx, h + dy, tz + dz, JUNGLE_LEAVES, false);
                                }
                            }
                        }
                    }
                    put(tx, h + 1, tz, LOG, true);
                }
                Tree::Swamp => {
                    // a squat oak with a wide drooping crown, often standing in the water
                    let base = h.max(SEA);
                    let th = 4 + pick % 2;
                    for (dy, lim) in [(-1, 10), (0, 10), (1, 4), (2, 1)] {
                        for dz in -3i32..=3 {
                            for dx in -3i32..=3 {
                                if dx * dx + dz * dz <= lim {
                                    put(tx + dx, base + th + dy, tz + dz, LEAVES, false);
                                }
                            }
                        }
                    }
                    for y in h + 1..=base + th {
                        put(tx, y, tz, LOG, true);
                    }
                }
            }
        }
    }

    // ground plants, each biome its own
    for z in 0..CW {
        for x in 0..CW {
            let c = col(x, z);
            let h = c.h;
            if h < SEA || blocks[bidx(x, h + 1, z)] != AIR {
                continue;
            }
            let (wx, wz) = (cx * CW + x, cz * CW + z);
            let r = hash2(seed ^ 0x71a2_55ed, wx, wz);
            let roll = (r % 1000) as i32;
            let ground = blocks[bidx(x, h, z)];
            // reeds line the water's edge
            if h <= SEA + 1 && (is_grassy(ground) || ground == SAND || ground == DIRT) && !frozen(x, z) {
                let wet = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| hget(x + dx, z + dz) < SEA);
                let share = if c.biome == Biome::Swamp { 300 } else { 110 };
                if wet && ((r >> 12) % 1000) < share {
                    for dy in 1..=1 + ((r >> 24) % 3) as i32 {
                        blocks[bidx(x, h + dy, z)] = REEDS;
                    }
                    continue;
                }
            }
            if h <= SEA + 1 && c.biome != Biome::Swamp {
                continue;
            }
            let mut plant = AIR;
            if is_grassy(ground) {
                // (cumulative shares out of 1000)
                let table: &[(i32, Block)] = match c.biome {
                    _ if ground == SNOWY_GRASS => &[(12, TALL_GRASS)],
                    Biome::Plains => &[(200, TALL_GRASS)],
                    Biome::Meadow => &[(260, TALL_GRASS)],
                    Biome::Forest | Biome::BirchForest => &[(110, TALL_GRASS), (124, MUSHROOM), (138, BERRY_BUSH), (160, FERN)],
                    Biome::Taiga => &[(120, FERN), (160, TALL_GRASS), (184, BERRY_BUSH), (200, MUSHROOM)],
                    Biome::Savanna => &[(200, DRY_TUFT)],
                    Biome::Jungle => &[(120, FERN), (190, TALL_GRASS), (196, FLOWER_RED), (202, FLOWER_BLUE)],
                    Biome::Swamp => &[(130, TALL_GRASS), (160, MUSHROOM), (180, FLOWER_BLUE), (200, FERN)],
                    _ => &[(60, TALL_GRASS)],
                };
                plant = table.iter().find(|e| roll < e.0).map_or(AIR, |e| e.1);
                // flowers grow in patches, a kind to each
                if plant == AIR && matches!(c.biome, Biome::Plains | Biome::Meadow | Biome::Forest | Biome::BirchForest) && ground == GRASS {
                    let patch = fbm(seed.wrapping_add(12000), wx as f32 / 28.0, wz as f32 / 28.0, 2);
                    let thick = if c.biome == Biome::Meadow { 0.12 } else { 0.3 };
                    let share = if patch.abs() > thick { if c.biome == Biome::Meadow { 190 } else { 80 } } else { 3 };
                    if roll >= 900 - share && roll < 900 {
                        let kind = fbm(seed.wrapping_add(12500), wx as f32 / 90.0, wz as f32 / 90.0, 1);
                        plant = if patch < 0.0 {
                            FLOWER_YELLOW
                        } else if kind > 0.0 {
                            FLOWER_RED
                        } else {
                            FLOWER_BLUE
                        };
                    }
                }
            } else if ground == SAND && c.biome == Biome::Desert {
                if roll < 9 {
                    plant = DEAD_BUSH;
                } else if roll < 14 && h + 4 < CH {
                    for dy in 1..=(2 + (r >> 9) % 2) as i32 {
                        blocks[bidx(x, h + dy, z)] = CACTUS;
                    }
                }
            } else if ground == RED_SAND {
                if roll < 16 {
                    plant = DEAD_BUSH;
                } else if roll < 19 {
                    blocks[bidx(x, h + 1, z)] = CACTUS;
                    blocks[bidx(x, h + 2, z)] = CACTUS;
                }
            }
            if plant != AIR {
                blocks[bidx(x, h + 1, z)] = plant;
            }
        }
    }
    if villages {
        crate::village::stamp(seed, cx, cz, &mut blocks, &hget);
    }
    ChunkData { blocks }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colour(b: Biome) -> [u8; 3] {
        match b {
            Biome::DeepOcean => [16, 40, 120],
            Biome::Ocean => [30, 70, 170],
            Biome::WarmOcean => [40, 150, 200],
            Biome::FrozenOcean => [170, 200, 240],
            Biome::River => [60, 110, 220],
            Biome::FrozenRiver => [150, 180, 250],
            Biome::Beach => [240, 230, 160],
            Biome::Plains => [130, 190, 80],
            Biome::Meadow => [170, 215, 110],
            Biome::Forest => [50, 130, 50],
            Biome::BirchForest => [100, 160, 70],
            Biome::Taiga => [30, 90, 70],
            Biome::SnowyTaiga => [170, 200, 190],
            Biome::SnowyPlains => [245, 245, 250],
            Biome::Desert => [225, 200, 120],
            Biome::Badlands => [190, 90, 40],
            Biome::Savanna => [180, 170, 70],
            Biome::Jungle => [20, 180, 20],
            Biome::Swamp => [70, 90, 60],
            Biome::Peaks => [130, 130, 135],
        }
    }

    #[test]
    fn biomes_hold_together() {
        let mut seen = [0u32; BIOMES];
        let (mut ice, mut reeds, mut trees) = (0u32, 0u32, 0u32);
        for seed in [0u32, 1, 42, 2026, 987_654_321, u32::MAX] {
            let (mut land, mut total) = (0u32, 0u32);
            for z in (-2048..2048).step_by(32) {
                for x in (-2048..2048).step_by(32) {
                    let c = column(seed, x, z);
                    assert!((3..=CH - 12).contains(&c.h));
                    seen[c.biome as usize] += 1;
                    total += 1;
                    land += (c.h >= SEA) as u32;
                }
            }
            let share = land as f32 / total as f32;
            assert!((0.5..0.9).contains(&share), "seed {seed}: land {share}");
            // the start is on level green land
            let (x, h, z) = spawn_point(seed);
            let c = column(seed, x, z);
            assert!(c.h == h && h > SEA + 2 && h < 78 && c.biome.mild(), "seed {seed}: spawn {x} {h} {z} {:?}", c.biome);
            assert!(x.abs().max(z.abs()) <= 1200, "seed {seed}: spawn {x} {z}");
            // chunks are the same every time, and nothing in them hangs in the air
            for k in 0..40 {
                let r = hash2(seed, k, 3);
                let (cx, cz) = ((r % 256) as i32 - 128, ((r >> 8) % 256) as i32 - 128);
                let ch = generate(seed, cx, cz, true);
                // (a village may hold water above the sea: its well, the ditches of its fields)
                let village = !crate::village::near(seed, cx * CW, cz * CW, cx * CW + 15, cz * CW + 15, crate::village::REACH).is_empty();
                if k == 0 {
                    assert!(ch.blocks == generate(seed, cx, cz, true).blocks);
                }
                for z in 0..CW {
                    for x in 0..CW {
                        let h = height_at(seed, cx * CW + x, cz * CW + z);
                        assert_eq!(ch.get(x, 0, z), BEDROCK);
                        for y in 1..CH - 1 {
                            let (b, under) = (ch.get(x, y, z), ch.get(x, y - 1, z));
                            if is_plant(b) || b == CACTUS {
                                assert!(is_opaque(under) || under == CACTUS || (b == REEDS && under == REEDS), "seed {seed}: block {b} on {under} at {x} {y} {z} in {cx} {cz}");
                            }
                            if b == WATER || b == ICE {
                                assert!(under != AIR && (y <= SEA || b == ICE || village), "seed {seed}: water at {x} {y} {z}");
                            }
                            if (b == LOG || b == BIRCH_LOG) && y == h + 1 {
                                assert!(under != AIR && under != WATER, "seed {seed}: tree in the air at {x} {y} {z}");
                                trees += 1;
                            }
                            ice += (b == ICE) as u32;
                            reeds += (b == REEDS) as u32;
                        }
                    }
                }
            }
        }
        // every biome turns up
        assert!(seen.iter().all(|&n| n > 20), "{seen:?}");
        assert!(ice > 0 && reeds > 0 && trees > 100, "ice {ice} reeds {reeds} trees {trees}");
        // the new blocks behave like their kin
        assert!(is_plant(FERN) && is_plant(REEDS) && is_plant(DRY_TUFT) && is_plant(FLOWER_BLUE) && !is_plant(ICE) && !is_plant(SNOWY_GRASS));
        assert!(is_leaves(JUNGLE_LEAVES) && is_leaves(SNOWY_LEAVES) && is_opaque(ICE) && is_opaque(SWAMP_GRASS) && !is_shaped(DRY_TUFT) && !is_shaped(ICE));
        for b in ICE..=DRY_TUFT {
            assert!((tex_for(b, 0) as usize) < crate::textures::ICON_BASE && tex_for(b, 0) != 14, "block {b}");
            let it = crate::items::item_of(b);
            assert!(crate::items::valid(it), "block {b} gives {it}");
            let d = crate::items::drop_for(b, AIR);
            assert!(d == AIR || crate::items::valid(d), "block {b} drops {d}");
        }
    }

    /// `cargo test --release find_biomes -- --ignored --nocapture`: a spot well inside each
    /// biome of a seed (CUBEWILD_SEED, default 42), nearest the origin, to go and look at.
    #[test]
    #[ignore]
    fn find_biomes() {
        let seed = std::env::var("CUBEWILD_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(42u32);
        let mut found = [None; BIOMES];
        for ring in 0..200i32 {
            for i in -ring..=ring {
                for (x, z) in [(i * 24, -ring * 24), (i * 24, ring * 24), (-ring * 24, i * 24), (ring * 24, i * 24)] {
                    let c = column(seed, x, z);
                    if found[c.biome as usize].is_none() && [(30, 0), (-30, 0), (0, 30), (0, -30)].iter().all(|&(dx, dz)| column(seed, x + dx, z + dz).biome == c.biome) {
                        found[c.biome as usize] = Some((x, c.h, z));
                    }
                }
            }
        }
        for (b, f) in found.iter().enumerate() {
            if let Some((x, h, z)) = f {
                println!("SPOT {:?} {x} {h} {z}", unsafe { std::mem::transmute::<u8, Biome>(b as u8) });
            }
        }
    }

    /// `cargo test --release gen2_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn gen2_speed() {
        let t = std::time::Instant::now();
        let mut air = 0usize;
        for cz in 0..8 {
            for cx in 0..8 {
                air += generate(2026, cx * 9, cz * 9, true).blocks.iter().filter(|b| **b == AIR).count();
            }
        }
        println!("generate v2: {:.2} ms a chunk ({air})", t.elapsed().as_secs_f64() * 1000.0 / 64.0);
        let t = std::time::Instant::now();
        let mut hs = 0i64;
        for i in 0..64 * 576 {
            hs += column(2026, i % 997, i / 31).h as i64;
        }
        println!("columns alone, one at a time: {:.2} ms a chunk ({hs})", t.elapsed().as_secs_f64() * 1000.0 / 64.0);
        let t = std::time::Instant::now();
        for cz in 0..8 {
            for cx in 0..8 {
                air += crate::world::generate(2026, cx * 9, cz * 9).blocks.len();
            }
        }
        println!("generate v1, same chunks: {:.2} ms a chunk ({air})", t.elapsed().as_secs_f64() * 1000.0 / 64.0);
    }

    /// Run by hand to look at many worlds at once:
    /// `CUBEWILD_MAP_DIR=/some/dir cargo test --release biome_survey -- --ignored --nocapture`
    /// prints the share of each biome and writes a map of each seed, 4096 blocks across.
    #[test]
    #[ignore]
    fn biome_survey() {
        let dir = std::env::var("CUBEWILD_MAP_DIR").ok();
        // the spread of the noise the thresholds are written against
        for (oct, sig) in [(2, SIG2), (3, SIG3), (4, SIG4), (5, SIG5)] {
            let (mut sum, mut n) = (0.0f64, 0);
            for i in 0..200_000 {
                let v = fbm(i / 1000 + 5, (i % 449) as f32 * 3.17, (i % 1000) as f32 * 2.31, oct) as f64;
                sum += v * v;
                n += 1;
            }
            println!("fbm {oct} octaves: sigma {:.3} (constant {sig})", (sum / n as f64).sqrt());
        }
        let mut all = [0u32; BIOMES];
        for seed in [0u32, 1, 2, 7, 42, 2026, 131, 65536, 123_456_789, 987_654_321, 808_080, 600_613] {
            const R: i32 = 2048;
            const STEP: i32 = 8;
            let n = (2 * R / STEP) as usize;
            let mut img = vec![0u8; n * n * 3];
            let mut share = [0u32; BIOMES];
            let (mut lo, mut hi) = (CH, 0);
            for (j, z) in (-R..R).step_by(STEP as usize).enumerate() {
                for (i, x) in (-R..R).step_by(STEP as usize).enumerate() {
                    let c = column(seed, x, z);
                    share[c.biome as usize] += 1;
                    lo = lo.min(c.h);
                    hi = hi.max(c.h);
                    // shaded by height
                    let k = if c.h < SEA { 0.75 + (c.h - 10).max(0) as f32 / 120.0 } else { 0.8 + (c.h - SEA) as f32 / 150.0 };
                    let rgb = colour(c.biome).map(|v| (v as f32 * k).min(255.0) as u8);
                    img[(j * n + i) * 3..][..3].copy_from_slice(&rgb);
                }
            }
            let (sx, _, sz) = spawn_point(seed);
            let (i, j) = (((sx + R) / STEP) as usize, ((sz + R) / STEP) as usize);
            for d in 0..5 {
                for (a, b) in [(i + d, j), (i - d, j), (i, j + d), (i, j - d)] {
                    img[(b * n + a) * 3..][..3].copy_from_slice(&[255, 0, 255]);
                }
            }
            let total = (n * n) as f32;
            let line: Vec<String> = (0..BIOMES).filter(|&b| share[b] > 0).map(|b| format!("{:?} {:.0}", unsafe { std::mem::transmute::<u8, Biome>(b as u8) }, share[b] as f32 * 100.0 / total)).collect();
            println!("seed {seed:>10}: height {lo}..{hi}  spawn {sx} {sz}  {}", line.join("  "));
            for b in 0..BIOMES {
                all[b] += share[b];
            }
            if let Some(d) = &dir {
                let mut out = format!("P6\n{n} {n}\n255\n").into_bytes();
                out.extend_from_slice(&img);
                std::fs::write(format!("{d}/biomes-{seed}.ppm"), out).unwrap();
            }
        }
        let total: u32 = all.iter().sum();
        for b in 0..BIOMES {
            println!("{:>14?} {:5.1}%", unsafe { std::mem::transmute::<u8, Biome>(b as u8) }, all[b] as f32 * 100.0 / total as f32);
        }
    }
}
