//! Height-mapped terrain generation.

use crate::noise::{fbm, hash, hash01};

/// Tiles below this height are water; the water surface is drawn at this level.
pub const SEA: i32 = 3;
/// Tiles at or above this height are snow-covered rock.
pub const SNOW: i32 = 12;
pub const MAX_Z: i32 = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Terrain {
    Water,
    Sand,
    Grass,
    Dirt,
    Rock,
    Snow,
}

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub z: i32,
    pub terrain: Terrain,
    /// Tree sprite variant, if a tree stands here.
    pub tree: Option<u8>,
    /// Grass tuft density, 0..=3.
    pub grass: u8,
    /// Per-tile random seed for stable texture and sway phase.
    pub seed: u32,
    /// For water: tiles in the connected body, capped. Small ponds stay calm.
    pub body: u16,
}

impl Tile {
    /// Height the top surface is drawn at; water is flattened to sea level.
    pub fn draw_z(&self) -> i32 {
        self.z.max(SEA)
    }
}

pub struct Map {
    pub w: usize,
    pub h: usize,
    pub tiles: Vec<Tile>,
}

impl Map {
    pub fn get(&self, x: i32, y: i32) -> Option<&Tile> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            None
        } else {
            Some(&self.tiles[y as usize * self.w + x as usize])
        }
    }

    /// Generate rolling terrain with a meandering river and forests.
    pub fn generate(w: usize, h: usize, seed: u64) -> Map {
        let mut z = vec![0i32; w * h];
        for y in 0..h {
            for x in 0..w {
                let n = fbm(x as f32 * 0.045, y as f32 * 0.045, seed, 4);
                let n = ((n - 0.5) * 1.7 + 0.5).clamp(0.0, 0.999);
                z[y * w + x] = (n.powf(1.15) * (MAX_Z as f32 + 1.0)) as i32;
            }
        }

        // River: a meandering channel carved to below sea level with sloped banks.
        let s1 = hash01(1, 1, seed) * 6.28;
        let s2 = hash01(2, 2, seed) * 6.28;
        for x in 0..w {
            let xf = x as f32;
            let cy = h as f32 * 0.5 + 7.0 * (xf * 0.11 + s1).sin() + 3.0 * (xf * 0.29 + s2).sin();
            for y in 0..h {
                let d = (y as f32 - cy).abs();
                let i = y * w + x;
                if d < 1.3 {
                    z[i] = SEA - 2;
                } else if d < 2.3 {
                    z[i] = z[i].min(SEA - 1);
                } else if d < 6.0 {
                    z[i] = z[i].min(SEA + (d - 2.3).floor() as i32);
                }
            }
        }

        let at = |x: i32, y: i32| -> i32 {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                SEA
            } else {
                z[y as usize * w + x as usize]
            }
        };

        let mut tiles = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let zz = z[y * w + x];
                let (xi, yi) = (x as i32, y as i32);
                let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| at(xi + dx, yi + dy) < SEA);
                let patch = fbm(x as f32 * 0.13, y as f32 * 0.13, seed ^ 0x51, 3);
                let terrain = if zz < SEA {
                    Terrain::Water
                } else if zz == SEA || (zz == SEA + 1 && near_water) {
                    Terrain::Sand
                } else if zz >= SNOW {
                    Terrain::Snow
                } else if zz >= SNOW - 2 {
                    Terrain::Rock
                } else if patch > 0.70 {
                    Terrain::Dirt
                } else {
                    Terrain::Grass
                };

                let forest = fbm(x as f32 * 0.08, y as f32 * 0.08, seed ^ 0xF0, 3);
                let hv = hash(xi as i64, yi as i64, seed);
                let tree = if terrain == Terrain::Grass
                    && zz >= SEA + 2
                    && forest > 0.47
                    && (hv % 100) < 62
                {
                    Some(((hv >> 8) % 4) as u8)
                } else {
                    None
                };
                let grass = (fbm(x as f32 * 0.15, y as f32 * 0.15, seed ^ 0xA7, 2) * 4.0) as u8;
                tiles.push(Tile { z: zz, terrain, tree, grass: grass.min(3), seed: (hv >> 32) as u32, body: 0 });
            }
        }
        let mut map = Map { w, h, tiles };
        map.label_water_bodies();
        map
    }

    /// Flood-fill connected water and record each body's size on its tiles.
    fn label_water_bodies(&mut self) {
        let (w, h) = (self.w, self.h);
        let mut seen = vec![false; w * h];
        for start in 0..w * h {
            if seen[start] || self.tiles[start].terrain != Terrain::Water {
                continue;
            }
            let mut stack = vec![start];
            let mut members = Vec::new();
            seen[start] = true;
            while let Some(i) = stack.pop() {
                members.push(i);
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && self.tiles[j].terrain == Terrain::Water {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            let size = members.len().min(u16::MAX as usize) as u16;
            for i in members {
                self.tiles[i].body = size;
            }
        }
    }
}
