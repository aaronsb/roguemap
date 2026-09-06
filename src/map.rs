//! Height-mapped terrain, generated on demand in chunks from pure functions
//! of position and seed. An island map answers only inside its bounds; an
//! unbounded one goes on in every direction.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::biome::{self, BIOMES};
use crate::noise::{fbm, hash};

/// Tiles below this height are water; the water surface is drawn at this level.
pub const SEA: i32 = 3;
/// Tiles at or above this height are snow-covered rock.
pub const SNOW: i32 = 12;
pub const MAX_Z: i32 = 14;

const CHUNK: i32 = 32;
/// Water bodies larger than this are treated as open water.
const BODY_CAP: usize = 200;

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
    /// Index into the biome table.
    pub biome: u8,
    /// Species index for the tree here, if any.
    pub species: u8,
    /// Building sprite variant, if a building stands here.
    pub building: Option<u8>,
    /// Annual mean temperature, degrees Celsius.
    pub temp: i8,
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
    pub seed: u64,
    /// When true, only tiles inside `w` x `h` exist.
    pub bounded: bool,
    chunks: RefCell<HashMap<(i32, i32), Chunk>>,
}

/// A generated block of tiles with its tallest drawn height.
struct Chunk {
    tiles: Vec<Tile>,
    max_z: i32,
}

impl Map {
    pub fn new(w: usize, h: usize, seed: u64) -> Map {
        Map { w, h, seed, bounded: true, chunks: RefCell::new(HashMap::new()) }
    }

    /// The tile at a position, generating its chunk on first touch.
    pub fn get(&self, x: i32, y: i32) -> Option<Tile> {
        if self.bounded && (x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32) {
            return None;
        }
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        let mut chunks = self.chunks.borrow_mut();
        let chunk = chunks.entry(key).or_insert_with(|| self.generate_chunk(key.0, key.1));
        Some(chunk.tiles[(y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize])
    }

    /// Tallest drawn height in the chunk containing a position, generating
    /// it if needed; zero outside a bounded map.
    pub fn ceiling(&self, x: i32, y: i32) -> i32 {
        if self.bounded && (x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32) {
            return 0;
        }
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        let mut chunks = self.chunks.borrow_mut();
        chunks.entry(key).or_insert_with(|| self.generate_chunk(key.0, key.1)).max_z
    }

    /// Raw height at any position. A slow continental field sets oceans,
    /// plains and ranges over hundreds of tiles; local noise adds hills.
    /// Rivers follow the mid-level contour of another slow field.
    pub fn height(&self, x: i32, y: i32) -> i32 {
        let (xf, yf) = (x as f32, y as f32);
        let continental = fbm(xf * 0.0035 + 17.0, yf * 0.0035, self.seed ^ 0xC0, 3);
        let continental = ((continental - 0.5) * 2.0 + 0.5).clamp(0.0, 1.0);
        let local = fbm(xf * 0.045, yf * 0.045, self.seed, 4);
        let n = continental * 0.62 + local * 0.38;
        let n = ((n - 0.5) * 1.6 + 0.5).clamp(0.0, 0.999);
        let mut z = (n.powf(1.15) * (MAX_Z as f32 + 1.0)) as i32;

        if z > SEA && z < SNOW - 2 {
            let r = crate::noise::value(xf * 0.012 + 5.0, yf * 0.012 + 9.0, self.seed ^ 0x51E);
            let d = (r - 0.5).abs();
            if d < 0.008 {
                z = SEA - 2;
            } else if d < 0.016 {
                z = z.min(SEA - 1);
            } else if d < 0.045 {
                z = z.min(SEA + ((d - 0.016) * 90.0).floor() as i32);
            }
        }
        z
    }

    fn is_water(&self, x: i32, y: i32) -> bool {
        self.height(x, y) < SEA
    }

    /// Annual mean temperature: a slow latitude-like field, minus a lapse
    /// rate with height, so the same region cools going uphill.
    pub fn temperature(&self, x: i32, y: i32, z: i32) -> f32 {
        let lat = fbm(x as f32 * 0.0022, y as f32 * 0.0022, self.seed ^ 0x7E, 2) * 2.0 - 1.0;
        let local = fbm(x as f32 * 0.02, y as f32 * 0.02, self.seed ^ 0x7F, 2) * 2.0 - 1.0;
        26.0 - 36.0 * (lat + 1.0) * 0.5 + local * 3.0 - (z.max(SEA) - SEA) as f32 * 2.2
    }

    /// Precipitation on a 0..100 scale from a slow moisture field.
    pub fn precipitation(&self, x: i32, y: i32) -> f32 {
        let big = fbm(x as f32 * 0.003 + 31.0, y as f32 * 0.003, self.seed ^ 0x9A, 3);
        let local = fbm(x as f32 * 0.03, y as f32 * 0.03, self.seed ^ 0x9B, 2);
        ((big * 0.8 + local * 0.2 - 0.5) * 1.6 + 0.5).clamp(0.0, 1.0) * 100.0
    }

    /// Classify one tile from the height field and climate around it.
    fn tile(&self, x: i32, y: i32) -> Tile {
        let z = self.height(x, y);
        let temp = self.temperature(x, y, z);
        let precip = self.precipitation(x, y);
        let biome_ix = biome::classify(temp, precip);
        let biome = &BIOMES[biome_ix];
        let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.is_water(x + dx, y + dy));
        let patch = fbm(x as f32 * 0.13, y as f32 * 0.13, self.seed ^ 0x51, 3);
        let terrain = if z < SEA {
            Terrain::Water
        } else if z == SEA || (z == SEA + 1 && near_water) {
            Terrain::Sand
        } else if temp <= -16.0 {
            Terrain::Snow
        } else if z >= SNOW - 2 {
            Terrain::Rock
        } else if patch > 0.72 {
            Terrain::Dirt
        } else {
            Terrain::Grass
        };

        let hv = hash(x as i64, y as i64, self.seed);
        let forest = fbm(x as f32 * 0.08, y as f32 * 0.08, self.seed ^ 0xF0, 3);
        let density = biome.tree_density * crate::noise::smoothstep(0.3, 0.7, forest);
        let mut species = 0u8;
        let tree = if terrain == Terrain::Grass && z >= SEA + 1 && !biome.species.is_empty() && (hv % 1000) as f32 / 1000.0 < density {
            let total: u32 = biome.species.iter().map(|&(_, w)| w as u32).sum();
            let mut pick = ((hv >> 16) % total.max(1) as u64) as u32;
            for &(sp, w) in biome.species {
                if pick < w as u32 {
                    species = sp as u8;
                    break;
                }
                pick -= w as u32;
            }
            Some(((hv >> 8) % 4) as u8)
        } else {
            None
        };

        let settle = fbm(x as f32 * 0.05 + 7.0, y as f32 * 0.05, self.seed ^ 0xB1, 2);
        let building = if tree.is_none()
            && matches!(terrain, Terrain::Grass | Terrain::Dirt | Terrain::Sand)
            && z >= SEA + 1
            && settle > 0.64
            && (hv >> 40) % 100 < 14
        {
            Some(((hv >> 48) % 4) as u8)
        } else {
            None
        };

        let grass = ((fbm(x as f32 * 0.15, y as f32 * 0.15, self.seed ^ 0xA7, 2) * 4.0) as u8).min(biome.grass);
        Tile {
            z,
            terrain,
            tree,
            grass,
            seed: (hv >> 32) as u32,
            body: 0,
            biome: biome_ix as u8,
            species,
            building,
            temp: temp.round().clamp(-60.0, 60.0) as i8,
        }
    }

    fn generate_chunk(&self, cx: i32, cy: i32) -> Chunk {
        let (x0, y0) = (cx * CHUNK, cy * CHUNK);
        let mut tiles: Vec<Tile> = (0..CHUNK * CHUNK).map(|i| self.tile(x0 + i % CHUNK, y0 + i / CHUNK)).collect();
        self.label_water_bodies(&mut tiles, x0, y0);
        let max_z = tiles.iter().map(|t| t.draw_z()).max().unwrap_or(0);
        Chunk { tiles, max_z }
    }

    /// Flood-fill each water body touching the chunk, capped, and record the
    /// size on the chunk's member tiles.
    fn label_water_bodies(&self, tiles: &mut [Tile], x0: i32, y0: i32) {
        let mut done = vec![false; tiles.len()];
        for i in 0..tiles.len() {
            if done[i] || tiles[i].terrain != Terrain::Water {
                continue;
            }
            let start = (x0 + i as i32 % CHUNK, y0 + i as i32 / CHUNK);
            let mut seen: HashSet<(i32, i32)> = HashSet::new();
            let mut queue = VecDeque::new();
            seen.insert(start);
            queue.push_back(start);
            while let Some((x, y)) = queue.pop_front() {
                if seen.len() >= BODY_CAP {
                    break;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let p = (x + dx, y + dy);
                    if self.bounded && (p.0 < 0 || p.1 < 0 || p.0 >= self.w as i32 || p.1 >= self.h as i32) {
                        continue;
                    }
                    if !seen.contains(&p) && self.is_water(p.0, p.1) {
                        seen.insert(p);
                        queue.push_back(p);
                    }
                }
            }
            let size = seen.len() as u16;
            for (x, y) in seen {
                let (lx, ly) = (x - x0, y - y0);
                if lx >= 0 && ly >= 0 && lx < CHUNK && ly < CHUNK {
                    let j = (ly * CHUNK + lx) as usize;
                    tiles[j].body = size;
                    done[j] = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_deterministic_and_chunk_independent() {
        let a = Map::new(32, 32, 7);
        let mut b = Map::new(32, 32, 7);
        b.bounded = false;
        for &(x, y) in &[(0, 0), (31, 31), (5, 17), (16, 16)] {
            let ta = a.get(x, y).unwrap();
            let tb = b.get(x, y).unwrap();
            assert_eq!(ta.z, tb.z);
            assert_eq!(ta.terrain, tb.terrain);
            assert_eq!(ta.biome, tb.biome);
        }
        assert!(a.get(-1, 0).is_none());
        assert!(b.get(-1, 0).is_some());
    }

    #[test]
    fn ceiling_bounds_every_tile() {
        let mut m = Map::new(8, 8, 3);
        m.bounded = false;
        for y in -40..40 {
            for x in -40..40 {
                assert!(m.get(x, y).unwrap().draw_z() <= m.ceiling(x, y));
            }
        }
    }
}
