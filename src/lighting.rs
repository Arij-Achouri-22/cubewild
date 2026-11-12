//! Minecraft-style block light: levels 0..15, attenuated by each cell and stopped
//! by opaque blocks. Built with immutable chunk snapshots on mesh workers.
use crate::world::{self::*, ChunkData};
use std::sync::Arc;

const W: usize = 48;
const PLANE: usize = W * W;

pub fn emission(b: Block) -> u8 {
    match b {
        b if is_torch(b) => 14,
        LAMP | SUNSHARD => 15,
        b if is_lava(b) => 15,
        GLOWCAP => 12,
        b if is_gate(b) => 11,
        _ => 0,
    }
}

pub fn transmission(b: Block) -> u8 {
    if is_opaque(b) && !is_leaves(b) {
        0
    } else if is_water(b) || is_leaves(b) {
        2
    } else {
        1
    }
}

/// Vertex brightness is baked on workers; the GPU only normalizes a byte.
#[inline]
pub fn brightness(level: u8) -> u32 {
    const VALUES: [u8; 16] = [0, 11, 14, 18, 22, 27, 34, 43, 53, 67, 84, 104, 131, 163, 204, 255];
    VALUES[level as usize] as u32
}

pub struct Field {
    levels: Vec<u8>,
    y0: usize,
    cached: Option<Box<[Arc<[u8]>; 9]>>,
}

impl Field {
    pub fn cached(cached: Box<[Arc<[u8]>; 9]>) -> Self {
        Self { levels: Vec::new(), y0: 0, cached: Some(cached) }
    }

    pub fn build(n: &[Arc<ChunkData>; 9]) -> Self {
        static EMISSION: std::sync::LazyLock<[u8; crate::registry::LIMIT]> =
            std::sync::LazyLock::new(|| std::array::from_fn(|i| emission(i as Block)));
        let mut sources = Vec::new();
        for (k, c) in n.iter().enumerate() {
            for (i, &b) in c.blocks.iter().enumerate() {
                let light = EMISSION[b as usize];
                if light > 0 {
                    let x = k % 3 * 16 + (i & 15);
                    let z = k / 3 * 16 + ((i >> 4) & 15);
                    sources.push(((i >> 8) * PLANE + z * W + x, light));
                }
            }
        }
        if sources.is_empty() {
            return Self { levels: Vec::new(), y0: 0, cached: None };
        }
        static TRANSMISSION: std::sync::LazyLock<[u8; crate::registry::LIMIT]> =
            std::sync::LazyLock::new(|| std::array::from_fn(|i| transmission(i as Block)));
        let y0 = sources.iter().map(|&(i, _)| i / PLANE).min().unwrap().saturating_sub(14);
        let y1 = (sources.iter().map(|&(i, _)| i / PLANE).max().unwrap() + 15).min(CH as usize);
        let height = y1 - y0;
        let mut levels = vec![0; PLANE * height];
        let mut buckets: [Vec<usize>; 16] = std::array::from_fn(|_| Vec::new());
        for (i, light) in sources {
            let i = i - y0 * PLANE;
            levels[i] = light;
            buckets[light as usize].push(i);
        }
        let at = |i: usize| {
            let y = i / PLANE + y0;
            let z = i % PLANE / W;
            let x = i % W;
            n[z / 16 * 3 + x / 16].blocks[y * 256 + (z & 15) * 16 + (x & 15)]
        };
        for level in (2..=15).rev() {
            while let Some(i) = buckets[level].pop() {
                if levels[i] != level as u8 {
                    continue;
                }
                let (x, z, y) = (i % W, i % PLANE / W, i / PLANE);
                let candidates = [
                    (x > 0, i.wrapping_sub(1)),
                    (x + 1 < W, i + 1),
                    (z > 0, i.wrapping_sub(W)),
                    (z + 1 < W, i + W),
                    (y > 0, i.wrapping_sub(PLANE)),
                    (y + 1 < height, i + PLANE),
                ];
                for (inside, j) in candidates {
                    if !inside || levels[j] >= level as u8 - 1 {
                        continue;
                    }
                    let loss = TRANSMISSION[at(j) as usize];
                    if loss == 0 {
                        continue;
                    }
                    let next = (level as u8).saturating_sub(loss);
                    if next > levels[j] {
                        levels[j] = next;
                        if next > 1 {
                            buckets[next as usize].push(j);
                        }
                    }
                }
            }
        }
        Self { levels, y0, cached: None }
    }

    /// Coordinates relative to the middle chunk, including its neighbours.
    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> u8 {
        if let Some(n) = &self.cached {
            if !(0..CH).contains(&y) || !(-16..32).contains(&x) || !(-16..32).contains(&z) {
                return 0;
            }
            return n[(((z + 16) / 16) * 3 + (x + 16) / 16) as usize][(y * 256 + (z & 15) * 16 + (x & 15)) as usize];
        }
        if self.levels.is_empty() || !(0..CH).contains(&y) || !(-16..32).contains(&x) || !(-16..32).contains(&z) {
            return 0;
        }
        let y = y as usize;
        if y < self.y0 || y - self.y0 >= self.levels.len() / PLANE {
            return 0;
        }
        self.levels[(y - self.y0) * PLANE + (z + 16) as usize * W + (x + 16) as usize]
    }

    pub fn center(&self) -> Arc<[u8]> {
        if let Some(n) = &self.cached {
            return n[4].clone();
        }
        let mut out = vec![0; VOL];
        if !self.levels.is_empty() {
            for y in self.y0..self.y0 + self.levels.len() / PLANE {
                for z in 0..16 {
                    let start = (y - self.y0) * PLANE + (z + 16) * W + 16;
                    out[y * 256 + z * 16..y * 256 + z * 16 + 16].copy_from_slice(&self.levels[start..start + 16]);
                }
            }
        }
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(extra: impl Fn(i32, i32, i32) -> Option<Block>) -> Field {
        let w = World::flat(extra);
        let n = std::array::from_fn(|i| w.chunks[&((i % 3) as i32 - 1, (i / 3) as i32 - 1)].data.clone());
        Field::build(&n)
    }

    #[test]
    fn light_attenuates_and_respects_walls_and_chunk_edges() {
        let f = field(|x, y, z| {
            if x == -1 && y == 12 && z == 0 {
                Some(TORCH)
            } else if x == 1 && (10..CH).contains(&y) {
                Some(STONE)
            } else {
                None
            }
        });
        assert_eq!(f.at(-1, 12, 0), 14);
        assert_eq!(f.at(0, 12, 0), 13);
        assert_eq!(f.at(2, 12, 0), 0);
        assert_eq!(f.at(-1, CH, 0), 0);
    }

    #[test]
    fn removing_a_light_and_opening_a_wall_rebuilds_without_ghost_light() {
        let lit = field(|x, y, z| (x == 0 && y == 12 && z == 0).then_some(LAMP));
        assert_eq!(lit.at(1, 12, 0), 14);
        let dark = field(|_, _, _| None);
        assert_eq!(dark.at(1, 12, 0), 0);
        let glass = field(|x, y, z| {
            if x == 0 && y == 12 && z == 0 {
                Some(TORCH)
            } else if x == 1 && y == 12 {
                Some(GLASS)
            } else {
                None
            }
        });
        assert_eq!(glass.at(2, 12, 0), 12);
    }
}
