//! Height-mapped terrain, generated on demand in chunks from pure functions
//! of position and seed. An island map answers only inside its bounds; an
//! unbounded one goes on in every direction.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::noise::{fbm, hash, hash01};

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
    chunks: RefCell<HashMap<(i32, i32), Vec<Tile>>>,
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
        Some(chunk[(y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize])
    }

    /// Raw height at any position: rolling noise with a meandering river.
    fn height(&self, x: i32, y: i32) -> i32 {
        let n = fbm(x as f32 * 0.045, y as f32 * 0.045, self.seed, 4);
        let n = ((n - 0.5) * 1.7 + 0.5).clamp(0.0, 0.999);
        let mut z = (n.powf(1.15) * (MAX_Z as f32 + 1.0)) as i32;

        let s1 = hash01(1, 1, self.seed) * 6.28;
        let s2 = hash01(2, 2, self.seed) * 6.28;
        let xf = x as f32;
        let cy = self.h as f32 * 0.5 + 7.0 * (xf * 0.11 + s1).sin() + 3.0 * (xf * 0.29 + s2).sin();
        let d = (y as f32 - cy).abs();
        if d < 1.3 {
            z = SEA - 2;
        } else if d < 2.3 {
            z = z.min(SEA - 1);
        } else if d < 6.0 {
            z = z.min(SEA + (d - 2.3).floor() as i32);
        }
        z
    }

    fn is_water(&self, x: i32, y: i32) -> bool {
        self.height(x, y) < SEA
    }

    /// Classify one tile from the height field around it.
    fn tile(&self, x: i32, y: i32) -> Tile {
        let z = self.height(x, y);
        let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.is_water(x + dx, y + dy));
        let patch = fbm(x as f32 * 0.13, y as f32 * 0.13, self.seed ^ 0x51, 3);
        let terrain = if z < SEA {
            Terrain::Water
        } else if z == SEA || (z == SEA + 1 && near_water) {
            Terrain::Sand
        } else if z >= SNOW {
            Terrain::Snow
        } else if z >= SNOW - 2 {
            Terrain::Rock
        } else if patch > 0.70 {
            Terrain::Dirt
        } else {
            Terrain::Grass
        };

        let forest = fbm(x as f32 * 0.08, y as f32 * 0.08, self.seed ^ 0xF0, 3);
        let hv = hash(x as i64, y as i64, self.seed);
        let tree = if terrain == Terrain::Grass && z >= SEA + 2 && forest > 0.47 && (hv % 100) < 62 {
            Some(((hv >> 8) % 4) as u8)
        } else {
            None
        };
        let grass = (fbm(x as f32 * 0.15, y as f32 * 0.15, self.seed ^ 0xA7, 2) * 4.0) as u8;
        Tile { z, terrain, tree, grass: grass.min(3), seed: (hv >> 32) as u32, body: 0 }
    }

    fn generate_chunk(&self, cx: i32, cy: i32) -> Vec<Tile> {
        let (x0, y0) = (cx * CHUNK, cy * CHUNK);
        let mut tiles: Vec<Tile> = (0..CHUNK * CHUNK).map(|i| self.tile(x0 + i % CHUNK, y0 + i / CHUNK)).collect();
        self.label_water_bodies(&mut tiles, x0, y0);
        tiles
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
