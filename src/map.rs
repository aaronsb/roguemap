//! Height-mapped terrain, generated on demand in chunks from pure functions
//! of position and seed. An island map answers only inside its bounds; an
//! unbounded one goes on in every direction.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::biome::{self, Biome, BuildingKind, Material, Species, BIOMES, BUILDINGS, HOUSE, MATERIALS, SPECIES, STONE};
use crate::noise::{fbm, hash};

/// Tiles below this height are water; the water surface is drawn at this level.
pub const SEA: i32 = 3;
/// Height from which terrain is bare rock; the treeline sits a little below
/// it and buildings up there are built of stone.
pub const ALPINE_Z: i32 = 12;
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

impl Terrain {
    /// Index into the plain-surface tables for sand, dirt, rock and snow;
    /// water and grass have bespoke rules.
    pub fn surface(self) -> Option<usize> {
        match self {
            Terrain::Sand => Some(0),
            Terrain::Dirt => Some(1),
            Terrain::Rock => Some(2),
            Terrain::Snow => Some(3),
            Terrain::Water | Terrain::Grass => None,
        }
    }
}

/// A tree standing on a tile.
#[derive(Clone, Copy, Debug)]
pub struct Flora {
    /// Index into the species table.
    pub species: u8,
    /// Sprite variant, 0..=3.
    pub variant: u8,
}

impl Flora {
    pub fn species(&self) -> &'static Species {
        &SPECIES[self.species as usize % SPECIES.len()]
    }
}

/// A building standing on a tile.
#[derive(Clone, Copy, Debug)]
pub struct Structure {
    /// Index into the building table.
    pub kind: u8,
    /// Sprite variant, 0..=3.
    pub variant: u8,
}

impl Structure {
    pub fn kind(&self) -> &'static BuildingKind {
        &BUILDINGS[self.kind as usize % BUILDINGS.len()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub z: i32,
    pub terrain: Terrain,
    pub tree: Option<Flora>,
    /// Grass tuft density, 0..=3.
    pub grass: u8,
    /// Per-tile random seed for stable texture and sway phase.
    pub seed: u32,
    /// For water: tiles in the connected body, capped. Small ponds stay calm.
    pub body_size: u16,
    /// Index into the biome table.
    pub biome: u8,
    pub building: Option<Structure>,
    /// Index into the material table: what is built here is made of this.
    pub material: u8,
    /// Annual mean temperature, degrees Celsius.
    pub temp: i8,
    /// Whether a cardinal neighbour is water.
    pub near_water: bool,
    /// Smooth height at the tile centre, before flooring.
    pub hf: f32,
}

impl Tile {
    /// Height the top surface is drawn at; water is flattened to sea level.
    pub fn draw_z(&self) -> i32 {
        self.z.max(SEA)
    }

    pub fn biome(&self) -> &'static Biome {
        &BIOMES[self.biome as usize % BIOMES.len()]
    }

    /// The species of the tree here, if any. Read by the asset pass's
    /// flora tables.
    #[allow(dead_code)]
    pub fn species(&self) -> Option<&'static Species> {
        self.tree.map(|f| f.species())
    }

    pub fn material(&self) -> &'static Material {
        &MATERIALS[self.material as usize % MATERIALS.len()]
    }
}

/// Climate at a tile: smooth and floored height, annual mean temperature,
/// precipitation and the biome they classify to.
#[derive(Clone, Copy, Debug)]
pub struct Climate {
    pub hf: f32,
    pub z: i32,
    pub temp: f32,
    pub precip: f32,
    pub biome: usize,
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

/// Terrain kind from the height and climate at a tile.
fn terrain_for(z: i32, temp: f32, near_water: bool, patch: f32) -> Terrain {
    if z < SEA {
        Terrain::Water
    } else if z == SEA || (z == SEA + 1 && near_water) {
        Terrain::Sand
    } else if temp <= -16.0 {
        Terrain::Snow
    } else if z >= ALPINE_Z - 2 {
        Terrain::Rock
    } else if patch > 0.72 {
        Terrain::Dirt
    } else {
        Terrain::Grass
    }
}

/// Local building material: stone on rock and near the alpine zone,
/// otherwise the biome's.
fn material_for(terrain: Terrain, z: i32, biome: &Biome) -> usize {
    if terrain == Terrain::Rock || z >= ALPINE_Z - 3 {
        STONE
    } else {
        biome.material
    }
}

impl Map {
    pub fn new(w: usize, h: usize, seed: u64) -> Map {
        Map { w, h, seed, bounded: true, chunks: RefCell::new(HashMap::new()) }
    }

    /// Whether a position exists on this map.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        !self.bounded || (x >= 0 && y >= 0 && x < self.w as i32 && y < self.h as i32)
    }

    /// The nearest position that exists on this map.
    pub fn clamp(&self, x: i32, y: i32) -> (i32, i32) {
        if self.bounded {
            (x.clamp(0, self.w as i32 - 1), y.clamp(0, self.h as i32 - 1))
        } else {
            (x, y)
        }
    }

    /// Run `f` on the chunk containing a position, generating it on first
    /// touch.
    fn with_chunk<R>(&self, x: i32, y: i32, f: impl FnOnce(&Chunk) -> R) -> R {
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        let mut chunks = self.chunks.borrow_mut();
        let chunk = chunks.entry(key).or_insert_with(|| self.generate_chunk(key.0, key.1));
        f(chunk)
    }

    /// The tile at a position, generating its chunk on first touch.
    pub fn get(&self, x: i32, y: i32) -> Option<Tile> {
        if !self.contains(x, y) {
            return None;
        }
        Some(self.with_chunk(x, y, |c| c.tiles[(y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize]))
    }

    /// Tallest drawn height in the chunk containing a position, generating
    /// it if needed; zero outside a bounded map.
    pub fn ceiling(&self, x: i32, y: i32) -> i32 {
        if !self.contains(x, y) {
            return 0;
        }
        self.with_chunk(x, y, |c| c.max_z)
    }

    /// Nearest land tile to a position, searching outward in rings.
    pub fn nearest_land(&self, cx: i32, cy: i32) -> (i32, i32) {
        for r in 0..64i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    if let Some(t) = self.get(cx + dx, cy + dy) {
                        if t.terrain != Terrain::Water {
                            return (cx + dx, cy + dy);
                        }
                    }
                }
            }
        }
        (cx, cy)
    }

    /// Smooth height at a fractional position, in height units. A slow
    /// continental field sets oceans, plains and ranges over hundreds of
    /// tiles; local noise adds hills; rivers follow the mid-level contour of
    /// another slow field. Tile heights are this field floored at the tile
    /// centre, so the column geometry is a terrace of it.
    pub fn height_smooth(&self, xf: f32, yf: f32) -> f32 {
        let local = fbm(xf * 0.045, yf * 0.045, self.seed, 4);
        let n = self.control_height(xf, yf) * 0.62 + local * 0.38;
        let n = ((n - 0.5) * 1.6 + 0.5).clamp(0.0, 0.999);
        let mut z = n.powf(1.15) * (MAX_Z as f32 + 1.0);

        if z > SEA as f32 + 1.0 && z < (ALPINE_Z - 2) as f32 {
            let r = crate::noise::value(xf * 0.012 + 5.0, yf * 0.012 + 9.0, self.seed ^ 0x51E);
            let d = (r - 0.5).abs();
            if d < 0.008 {
                z = SEA as f32 - 1.5;
            } else if d < 0.016 {
                z = z.min(SEA as f32 - 0.5);
            } else if d < 0.045 {
                z = z.min(SEA as f32 + (d - 0.016) * 90.0 + 0.5);
            }
        }
        z
    }

    /// The coarse control layer for height in `[0, 1]`: the overall shape of
    /// the world at a resolution of hundreds of tiles. Everything finer is
    /// procedural detail filled in below it. Today it is a slow noise; an
    /// authored or edited control grid can replace it without touching the
    /// detail layers.
    pub fn control_height(&self, xf: f32, yf: f32) -> f32 {
        let c = fbm(xf * 0.0035 + 17.0, yf * 0.0035, self.seed ^ 0xC0, 3);
        ((c - 0.5) * 2.0 + 0.5).clamp(0.0, 1.0)
    }

    /// Fine relief added to the smooth field at close zooms: fractal detail
    /// under one height unit, so it never changes a tile's terrace but gives
    /// slopes, shorelines and patches sub-tile shape.
    pub fn detail(&self, xf: f32, yf: f32, octaves: u32) -> f32 {
        if octaves == 0 {
            return 0.0;
        }
        (fbm(xf * 0.6 + 3.0, yf * 0.6 + 1.0, self.seed ^ 0xD7, octaves) - 0.5) * 0.9
    }

    /// Tile height: the smooth field floored at the tile centre.
    pub fn height(&self, x: i32, y: i32) -> i32 {
        self.height_smooth(x as f32 + 0.5, y as f32 + 0.5).floor() as i32
    }

    /// Dirt patch field, above 0.72 is bare earth.
    pub fn patch(&self, xf: f32, yf: f32) -> f32 {
        fbm(xf * 0.13, yf * 0.13, self.seed ^ 0x51, 3)
    }

    /// Surface kind at a fractional position from the continuous fields.
    pub fn surface_at(&self, xf: f32, yf: f32, h: f32, temp: f32) -> Terrain {
        if h < SEA as f32 {
            Terrain::Water
        } else if h < SEA as f32 + 1.3 {
            Terrain::Sand
        } else if temp <= -16.0 {
            Terrain::Snow
        } else if h >= (ALPINE_Z - 2) as f32 {
            Terrain::Rock
        } else if self.patch(xf, yf) > 0.72 {
            Terrain::Dirt
        } else {
            Terrain::Grass
        }
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

    /// Height and climate at a tile, without generating its chunk.
    pub fn climate(&self, x: i32, y: i32) -> Climate {
        let hf = self.height_smooth(x as f32 + 0.5, y as f32 + 0.5);
        let z = hf.floor() as i32;
        let temp = self.temperature(x, y, z);
        let precip = self.precipitation(x, y);
        Climate { hf, z, temp, precip, biome: biome::classify(temp, precip) }
    }

    /// Whether a building stands at a tile, from the settlement field and
    /// the kind's placement rule.
    fn place_building(&self, x: i32, y: i32, hv: u64, terrain: Terrain, z: i32) -> Option<Structure> {
        let kind = &BUILDINGS[HOUSE];
        let settle = fbm(x as f32 * 0.05 + 7.0, y as f32 * 0.05, self.seed ^ 0xB1, 2);
        if kind.terrain.contains(&terrain) && z > SEA && settle > kind.settle_min && (hv >> 40) % 100 < kind.chance {
            Some(Structure { kind: HOUSE as u8, variant: ((hv >> 48) % 4) as u8 })
        } else {
            None
        }
    }

    /// Classify one tile from the height field and climate around it.
    fn tile(&self, x: i32, y: i32) -> Tile {
        let climate = self.climate(x, y);
        let z = climate.z;
        let temp = climate.temp;
        let biome = &BIOMES[climate.biome];
        let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.is_water(x + dx, y + dy));
        let patch = self.patch(x as f32 + 0.5, y as f32 + 0.5);
        let terrain = terrain_for(z, temp, near_water, patch);

        let hv = hash(x as i64, y as i64, self.seed);
        let forest = fbm(x as f32 * 0.08, y as f32 * 0.08, self.seed ^ 0xF0, 3);
        let density = biome.tree_density * crate::noise::smoothstep(0.3, 0.7, forest);
        let tree = if terrain == Terrain::Grass && z > SEA && !biome.species.is_empty() && (hv % 1000) as f32 / 1000.0 < density {
            Some(Flora { species: biome::weighted_pick(biome.species, hv >> 16) as u8, variant: ((hv >> 8) % 4) as u8 })
        } else {
            None
        };
        let building = if tree.is_none() { self.place_building(x, y, hv, terrain, z) } else { None };

        let grass = ((fbm(x as f32 * 0.15, y as f32 * 0.15, self.seed ^ 0xA7, 2) * 4.0) as u8).min(biome.grass);
        Tile {
            z,
            terrain,
            tree,
            grass,
            seed: (hv >> 32) as u32,
            body_size: 0,
            biome: climate.biome as u8,
            building,
            material: material_for(terrain, z, biome) as u8,
            temp: temp.round().clamp(-60.0, 60.0) as i8,
            near_water,
            hf: climate.hf,
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
                    if !self.contains(p.0, p.1) {
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
                    tiles[j].body_size = size;
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

    #[test]
    fn bounds_helpers_follow_bounded() {
        let mut m = Map::new(8, 8, 3);
        assert!(m.contains(0, 0) && m.contains(7, 7));
        assert!(!m.contains(-1, 0) && !m.contains(8, 0));
        assert_eq!(m.clamp(-5, 20), (0, 7));
        m.bounded = false;
        assert!(m.contains(-100, 100));
        assert_eq!(m.clamp(-5, 20), (-5, 20));
    }

    #[test]
    fn climate_agrees_with_tiles() {
        let m = Map::new(32, 32, 7);
        for &(x, y) in &[(0, 0), (31, 31), (5, 17), (16, 16)] {
            let c = m.climate(x, y);
            let t = m.get(x, y).unwrap();
            assert_eq!(c.z, t.z);
            assert_eq!(c.biome, t.biome as usize);
            assert_eq!(c.hf, t.hf);
        }
    }

    #[test]
    fn nearest_land_is_never_water() {
        let m = Map::new(32, 32, 7);
        let (x, y) = m.nearest_land(0, 0);
        assert_ne!(m.get(x, y).unwrap().terrain, Terrain::Water);
    }
}
