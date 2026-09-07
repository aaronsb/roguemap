//! Height-mapped terrain, generated on demand in chunks from pure functions
//! of position and seed. An island map answers only inside its bounds; an
//! unbounded one goes on in every direction.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::assets::Assets;
use crate::biome::{self, Biome, Block, Material, Species};
use crate::blocks::Stack;
use crate::noise::{fbm, hash, FbmCache};
use crate::volume::{instance, size_scale, variant_scale};

/// Sea level: heights are metres above it, so the shoreline is zero.
pub const SEA: i32 = 0;
/// The sea bed and the island's plinth: the walk stops here.
pub const FLOOR: f32 = -12.0;
/// Metres of water under the shoreline per unit of the generator's field.
const DEPTH: f32 = 4.0;
/// Height from which terrain is bare rock.
pub const ROCK_Z: i32 = 51;
/// Height from which what is built is built of stone.
pub const STONE_Z: i32 = 40;
// There is no treeline constant. Height cools the air by `LAPSE`, the
// climate picks the biome, and the biome's `tree_density` in
// assets/biomes.toml says how many trees stand there: tundra and ice cap
// carry almost none, so the trees stop where the table says they do.
/// Ceiling of the relief: the top of a range.
pub const MAX_Z: i32 = RELIEF as i32;
/// Metres from the shoreline to the top of a range (ADR-004).
pub const RELIEF: f32 = 120.0;
/// How the generator's field becomes metres: relief grows as this power of
/// the field above the shoreline, so a valley floor is a metre or two over
/// the water and only a range reaches `RELIEF`.
const RELIEF_EXP: f32 = 1.6;
/// The generator's field: unitless, `SEA_UNITS` at the shoreline and
/// `FIELD_UNITS` at the top. Rivers, lakes and the biome bands are shaped
/// in it, then `relief` turns it into metres.
const FIELD_UNITS: f32 = 15.0;
const SEA_UNITS: f32 = 3.0;
/// Field value where bare rock starts; `ROCK_Z` is its height in metres.
const ROCK_UNITS: f32 = 10.0;
/// Degrees the air cools between the shoreline and the top of a range. The
/// world's 120 m of relief stands in for a continent's, so the lapse rate
/// follows `relief_fraction` rather than a real rate per metre, and the
/// treeline, the snow line and the biome bands sit where they did.
const LAPSE: f32 = 26.4;
/// Side of a tile in metres. Every size in the tables is in metres, and a
/// metre draws as `Camera::rows_per_metre` rows (ADR-004).
pub const TILE_METRES: f32 = 2.0;
/// The same side in centimetres, the unit entity positions are held in
/// (ADR-006); the tile a position falls in is `cm.div_euclid(TILE_CM)`.
pub const TILE_CM: i32 = (TILE_METRES * 100.0) as i32;
/// How far inland a beach reaches: sand at sea level needs water within
/// this many tiles, so a low plain away from any water is grass.
const SHORE_TILES: i32 = 3;

/// Metres of height for a value of the generator's field: linear down to
/// the sea bed, and a rising curve above the shoreline so lowlands are
/// gentle and ranges reach `RELIEF`.
pub fn relief(units: f32) -> f32 {
    if units <= SEA_UNITS {
        (units - SEA_UNITS) * DEPTH
    } else {
        RELIEF * ((units - SEA_UNITS) / (FIELD_UNITS - SEA_UNITS)).clamp(0.0, 1.0).powf(RELIEF_EXP)
    }
}

/// Where a height stands in the world's relief, 0 at the shoreline and 1 at
/// the top of a range: `relief` inverted. Height bands that were linear in
/// the generator's field — the lapse rate, the world map's shading — are
/// linear in this.
pub fn relief_fraction(metres: f32) -> f32 {
    (metres.max(0.0) / RELIEF).powf(1.0 / RELIEF_EXP)
}

const CHUNK: i32 = 32;
/// Tiles of a settlement plot: at most one building stands in each, with a
/// tile of margin, so buildings of neighbouring plots never touch.
const PLOT: i32 = 16;
/// Tiles of cleared ground around a building: no tree grows against a
/// wall, so a house in a wood stands in its own clearing.
const CLEARING: i32 = 2;
/// Water bodies larger than this are treated as open water.
const BODY_CAP: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    pub fn species<'a>(&self, assets: &'a Assets) -> &'a Species {
        &assets.species[self.species as usize % assets.species.len()]
    }
}

impl Stack {
    pub fn kind<'a>(&self, assets: &'a Assets) -> &'a Block {
        &assets.blocks[self.kind as usize % assets.blocks.len()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    /// Gameplay height: the smooth field floored to whole metres, the
    /// terrace the tile stands on.
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
    /// The block geometry stacked here, if any (ADR-002).
    pub stack: Option<Stack>,
    /// Index into the material table: what is built here is made of this.
    pub material: u8,
    /// Annual mean temperature, degrees Celsius.
    pub temp: i8,
    /// Whether a cardinal neighbour is water.
    pub near_water: bool,
    /// Smooth height in metres at the tile centre, before flooring.
    pub hf: f32,
}

impl Tile {
    /// Height the top surface is drawn at; water is flattened to sea level.
    pub fn draw_z(&self) -> i32 {
        self.z.max(SEA)
    }

    pub fn biome<'a>(&self, assets: &'a Assets) -> &'a Biome {
        &assets.biomes[self.biome as usize % assets.biomes.len()]
    }

    /// The species of the tree here, if any.
    pub fn species<'a>(&self, assets: &'a Assets) -> Option<&'a Species> {
        self.tree.map(|f| f.species(assets))
    }

    pub fn material<'a>(&self, assets: &'a Assets) -> &'a Material {
        &assets.materials[self.material as usize % assets.materials.len()]
    }

    /// Highest solid over the tile: the terrain top, the stack's column
    /// and roof, or the tree's crown.
    pub fn top(&self, assets: &Assets) -> f32 {
        let ground = self.hf.max(SEA as f32);
        let mut top = ground;
        if let Some(st) = self.stack {
            let b = st.kind(assets);
            top = top.max(ground + st.levels as f32 * b.level_height + b.max_rise);
        }
        if let Some(f) = self.tree {
            let sp = f.species(assets);
            let (_, d) = sp.volume();
            let s = size_scale(sp.size_class) * variant_scale(f.variant) * instance(self.seed).height;
            top = top.max(ground + 0.5 + (d.trunk + d.height) * s);
        }
        top
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

/// A small flat map for previews and tests (ADR-003): every tile is at one
/// height, in one biome, of one terrain, with no generated trees or
/// buildings. Subjects are placed on it with `Map::set_tile`.
#[derive(Clone, Debug, PartialEq)]
pub struct FixtureSpec {
    pub w: usize,
    pub h: usize,
    /// Tile height in metres; the smooth field sits half a metre above it.
    pub z: i32,
    /// Index into the biome table.
    pub biome: usize,
    pub terrain: Terrain,
    /// Annual mean temperature, degrees Celsius.
    pub temp: i8,
    /// Grass tuft density, 0..=3.
    pub grass: u8,
    /// Index into the material table.
    pub material: usize,
}

pub struct Map {
    pub w: usize,
    pub h: usize,
    pub seed: u64,
    /// When true, only tiles inside `w` x `h` exist.
    pub bounded: bool,
    /// The tables tiles index into.
    pub assets: Rc<Assets>,
    /// Material index of stone, for the highlands.
    stone: usize,
    /// When set, the continuous fields are flat and every tile is the
    /// fixture's; the generator is bypassed.
    fixture: Option<FixtureSpec>,
    chunks: RefCell<HashMap<(i32, i32), Chunk>>,
    /// Stacks placed by hand or by the settlement system, over the
    /// generated ones.
    stacks: RefCell<HashMap<(i32, i32), Option<Stack>>>,
}

/// A building the generator puts on a plot: a rectangle of tiles in
/// tiles, all carrying one stack, which the geometry merges into one
/// building (ADR-002).
#[derive(Clone, Copy, Debug)]
struct Building {
    x0: i32,
    y0: i32,
    w: i32,
    d: i32,
    stack: Stack,
}

/// The continuous fields over a tile range for one frame (`Map::fields`):
/// the same values `Map::detail` and `Map::patch` give, from lattices
/// hashed once.
pub struct Fields {
    octaves: u32,
    detail: FbmCache,
    patch: FbmCache,
}

impl Fields {
    /// What `Map::detail` gives at this point for the octaves the fields
    /// were built with.
    #[inline]
    pub fn detail(&self, xf: f32, yf: f32) -> f32 {
        if self.octaves == 0 {
            return 0.0;
        }
        (self.detail.fbm(xf * 0.6 + 3.0, yf * 0.6 + 1.0) - 0.5) * 0.9
    }

    /// What `Map::patch` gives at this point.
    #[inline]
    pub fn patch(&self, xf: f32, yf: f32) -> f32 {
        self.patch.fbm(xf * 0.13, yf * 0.13)
    }
}

/// A generated block of tiles with the ceiling over everything in it.
struct Chunk {
    tiles: Vec<Tile>,
    max_z: i32,
}

/// Terrain kind from the height in metres and the climate at a tile. Sand
/// is a shore terrain, so both of its bands ask how far the water is: the
/// lowest one is beach out to `SHORE_TILES`, a metre up only next to the
/// water. A basin that happens to sit at sea level inland is not sand.
fn terrain_for(z: i32, temp: f32, near_water: bool, shore: bool, patch: f32) -> Terrain {
    if z < SEA {
        Terrain::Water
    } else if (z == SEA && shore) || (z == SEA + 1 && near_water) {
        Terrain::Sand
    } else if temp <= -16.0 {
        Terrain::Snow
    } else if z >= ROCK_Z {
        Terrain::Rock
    } else if patch > 0.72 {
        Terrain::Dirt
    } else {
        Terrain::Grass
    }
}

/// Local building material: stone on rock and on high ground, otherwise
/// the biome's.
fn material_for(terrain: Terrain, z: i32, biome: &Biome, stone: usize) -> usize {
    if terrain == Terrain::Rock || z >= STONE_Z {
        stone
    } else {
        biome.material
    }
}

impl Map {
    pub fn new(w: usize, h: usize, seed: u64, assets: Rc<Assets>) -> Map {
        let stone = assets.material_index("stone").expect("the loader requires a stone material");
        Map { w, h, seed, bounded: true, assets, stone, fixture: None, chunks: RefCell::new(HashMap::new()), stacks: RefCell::new(HashMap::new()) }
    }

    /// A bounded flat map from a fixture spec; see `FixtureSpec`.
    pub fn fixture(spec: FixtureSpec, assets: Rc<Assets>) -> Map {
        let mut m = Map::new(spec.w, spec.h, 1, assets);
        m.fixture = Some(spec);
        m
    }

    /// Whether this map is a fixture rather than generated terrain.
    pub fn is_fixture(&self) -> bool {
        self.fixture.is_some()
    }

    /// Replace the tile at a position, generating its chunk first. Off-map
    /// positions are ignored. The chunk ceiling grows to cover the tile.
    pub fn set_tile(&mut self, x: i32, y: i32, tile: Tile) {
        if !self.contains(x, y) {
            return;
        }
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        let mut chunks = self.chunks.borrow_mut();
        let chunk = chunks.entry(key).or_insert_with(|| self.generate_chunk(key.0, key.1));
        chunk.tiles[(y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize] = tile;
        chunk.max_z = chunk.max_z.max(tile.top(&self.assets).ceil() as i32);
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

    /// The tiles of a range, row-major, generating each chunk once: the
    /// per-frame grid reads thousands of tiles, and going chunk by chunk
    /// keeps that to one lookup per chunk rather than one per tile.
    pub fn tiles_in(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<Option<Tile>> {
        let (w, h) = ((x1 - x0 + 1).max(0), (y1 - y0 + 1).max(0));
        let mut out = vec![None; (w * h) as usize];
        for cy in y0.div_euclid(CHUNK)..=y1.div_euclid(CHUNK) {
            for cx in x0.div_euclid(CHUNK)..=x1.div_euclid(CHUNK) {
                let (bx, by) = (cx * CHUNK, cy * CHUNK);
                if self.bounded && (bx + CHUNK <= 0 || by + CHUNK <= 0 || bx >= self.w as i32 || by >= self.h as i32) {
                    continue;
                }
                let mut chunks = self.chunks.borrow_mut();
                let chunk = chunks.entry((cx, cy)).or_insert_with(|| self.generate_chunk(cx, cy));
                for ty in y0.max(by)..=y1.min(by + CHUNK - 1) {
                    for tx in x0.max(bx)..=x1.min(bx + CHUNK - 1) {
                        if !self.contains(tx, ty) {
                            continue;
                        }
                        out[((ty - y0) * w + (tx - x0)) as usize] = Some(chunk.tiles[((ty - by) * CHUNK + (tx - bx)) as usize]);
                    }
                }
            }
        }
        out
    }

    /// Ceiling over everything in the chunk containing a position (terrain,
    /// stacks and crowns), generating it if needed; zero outside a bounded
    /// map.
    pub fn ceiling(&self, x: i32, y: i32) -> i32 {
        if !self.contains(x, y) {
            return 0;
        }
        self.with_chunk(x, y, |c| c.max_z)
    }

    /// Highest anything reaches over the map's own tiles, in metres, from
    /// the chunk ceilings. The view uses it to know how many rows the
    /// terrain needs above its footprint.
    pub fn relief_ceiling(&self) -> i32 {
        let step = |n: usize| {
            let last = n as i32 - 1;
            (0..n as i32).step_by(CHUNK as usize).chain(std::iter::once(last)).collect::<Vec<i32>>()
        };
        let (xs, ys) = (step(self.w), step(self.h));
        ys.iter().flat_map(|y| xs.iter().map(move |x| (*x, *y))).map(|(x, y)| self.ceiling(x, y)).max().unwrap_or(1).max(1)
    }

    /// Place or clear a stack on a tile, over what the generator put there,
    /// and lift the chunk's ceiling for it. Off the map nothing happens.
    pub fn set_stack(&self, x: i32, y: i32, stack: Option<Stack>) {
        if !self.contains(x, y) {
            return;
        }
        self.stacks.borrow_mut().insert((x, y), stack);
        let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK));
        let mut chunks = self.chunks.borrow_mut();
        let chunk = chunks.entry(key).or_insert_with(|| self.generate_chunk(key.0, key.1));
        let i = (y.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize;
        chunk.tiles[i].stack = stack;
        chunk.max_z = chunk.max_z.max(chunk.tiles[i].top(&self.assets).ceil() as i32);
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

    /// Smooth height at a fractional position, in metres. A slow
    /// continental field sets oceans, plains and ranges over hundreds of
    /// tiles; local noise adds hills; rivers follow the mid-level contour of
    /// another slow field. The shaping happens in the generator's own units;
    /// `relief` turns the result into metres. Tile heights are this field
    /// floored at the tile centre, so the column geometry is a terrace of it
    /// one metre at a time.
    pub fn height_smooth(&self, xf: f32, yf: f32) -> f32 {
        if let Some(f) = &self.fixture {
            return f.z as f32 + 0.5;
        }
        relief(self.field(xf, yf))
    }

    /// The generator's height field in its own units, before `relief`.
    fn field(&self, xf: f32, yf: f32) -> f32 {
        let local = fbm(xf * 0.045, yf * 0.045, self.seed, 4);
        let n = self.control_height(xf, yf) * 0.62 + local * 0.38;
        let n = ((n - 0.5) * 1.6 + 0.5).clamp(0.0, 0.999);
        let mut z = n.powf(1.15) * FIELD_UNITS;

        // Rivers are cut in the field, between the shore and the highlands.
        if z > SEA_UNITS + 1.0 && z < ROCK_UNITS {
            let r = crate::noise::value(xf * 0.012 + 5.0, yf * 0.012 + 9.0, self.seed ^ 0x51E);
            let d = (r - 0.5).abs();
            if d < 0.008 {
                z = SEA_UNITS - 1.5;
            } else if d < 0.016 {
                z = z.min(SEA_UNITS - 0.5);
            } else if d < 0.045 {
                z = z.min(SEA_UNITS + (d - 0.016) * 90.0 + 0.5);
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
    /// under a metre, so it never changes a tile's terrace but gives
    /// slopes, shorelines and patches sub-tile shape.
    pub fn detail(&self, xf: f32, yf: f32, octaves: u32) -> f32 {
        if octaves == 0 || self.fixture.is_some() {
            return 0.0;
        }
        (fbm(xf * 0.6 + 3.0, yf * 0.6 + 1.0, self.seed ^ 0xD7, octaves) - 0.5) * 0.9
    }

    /// The continuous fields the walk samples, with their noise lattices
    /// hashed once over the tile range `x0..=x1` by `y0..=y1` for a frame:
    /// `detail` and the dirt `patch` behind `surface_at`, sampled a hundred
    /// thousand times a frame, come out the same to the bit.
    pub fn fields(&self, x0: i32, y0: i32, x1: i32, y1: i32, octaves: u32) -> Fields {
        let (fx0, fy0, fx1, fy1) = (x0 as f32 - 2.0, y0 as f32 - 2.0, x1 as f32 + 3.0, y1 as f32 + 3.0);
        let octaves = if self.fixture.is_some() { 0 } else { octaves };
        let detail = FbmCache::new(fx0 * 0.6 + 3.0, fy0 * 0.6 + 1.0, fx1 * 0.6 + 3.0, fy1 * 0.6 + 1.0, self.seed ^ 0xD7, octaves);
        let patch = FbmCache::new(fx0 * 0.13, fy0 * 0.13, fx1 * 0.13, fy1 * 0.13, self.seed ^ 0x51, 3);
        Fields { octaves, detail, patch }
    }

    /// Tile height: the smooth field floored at the tile centre.
    pub fn height(&self, x: i32, y: i32) -> i32 {
        self.height_smooth(x as f32 + 0.5, y as f32 + 0.5).floor() as i32
    }

    /// Height a figure stands at over a point of the map (ADR-006): the
    /// smooth field between tile centres, the bilinear of the four nearest
    /// tiles' `hf`, never below sea level. At a tile's centre it is the
    /// tile's own `hf`; past the map's edge the nearest tile's. The
    /// sub-tile detail the walk adds to the drawn surface is not applied.
    pub fn ground_at(&self, x: f32, y: f32) -> f32 {
        let (gx, gy) = (x - 0.5, y - 0.5);
        let (ix, iy) = (gx.floor() as i32, gy.floor() as i32);
        let (fx, fy) = (gx - ix as f32, gy - iy as f32);
        let corners = [(ix, iy), (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1)].map(|(mx, my)| self.get(mx, my).map(|t| t.hf));
        let fallback = corners.iter().flatten().next().copied().unwrap_or(SEA as f32);
        let [a, b, c, d] = corners.map(|h| h.unwrap_or(fallback));
        let top = a + (b - a) * fx;
        let bot = c + (d - c) * fx;
        (top + (bot - top) * fy).max(SEA as f32)
    }

    /// Whether a tile is the site of its own neighbourhood: no tile within
    /// `radius` tiles has a higher draw. Trees are placed on these sites, so
    /// crowns of the species' spacing never overlap however dense the
    /// forest field is, and the gaps between them are trunk and grass.
    fn stands_clear(&self, x: i32, y: i32, radius: f32) -> bool {
        let draw = |px: i32, py: i32| hash(px as i64, py as i64, self.seed ^ 0x7EE5);
        let r = radius.clamp(0.5, 6.0);
        let n = r.floor() as i32;
        let mine = draw(x, y);
        for dy in -n..=n {
            for dx in -n..=n {
                if (dx, dy) == (0, 0) || (dx * dx + dy * dy) as f32 > r * r {
                    continue;
                }
                if draw(x + dx, y + dy) > mine {
                    return false;
                }
            }
        }
        true
    }

    /// Dirt patch field, above 0.72 is bare earth.
    pub fn patch(&self, xf: f32, yf: f32) -> f32 {
        fbm(xf * 0.13, yf * 0.13, self.seed ^ 0x51, 3)
    }

    /// Whether a position lies on or beside a beach: a tile of sand within
    /// a step. The tiles already carry the shore test
    /// (`terrain_for`), so the continuous surface asks them rather than
    /// walking the field again, and the sand at sea level stops where the
    /// beach does instead of pooling in an inland basin.
    fn beach(&self, xf: f32, yf: f32) -> bool {
        let (x, y) = (xf.floor() as i32, yf.floor() as i32);
        let sandy = |x: i32, y: i32| self.get(x, y).map(|t| t.terrain == Terrain::Sand).unwrap_or(false);
        sandy(x, y) || [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| sandy(x + dx, y + dy))
    }

    /// Whether the tile under a position is water, which is what tells the
    /// sea from the sand where the drawn surface sits at the sea-level
    /// clamp.
    fn wet(&self, xf: f32, yf: f32) -> bool {
        self.get(xf.floor() as i32, yf.floor() as i32).map(|t| t.terrain == Terrain::Water).unwrap_or(false)
    }

    /// Whether a position lies within a tile of water, for beaches.
    fn shore(&self, xf: f32, yf: f32) -> bool {
        let (x, y) = (xf.floor() as i32, yf.floor() as i32);
        self.get(x, y).map(|t| t.near_water || t.terrain == Terrain::Water).unwrap_or(false)
            || [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.get(x + dx, y + dy).map(|t| t.terrain == Terrain::Water).unwrap_or(false))
    }

    /// Surface kind at a fractional position from the continuous fields.
    pub fn surface_at(&self, xf: f32, yf: f32, h: f32, temp: f32) -> Terrain {
        self.surface_kind(h, temp, self.wet(xf, yf), || self.beach(xf, yf), || self.shore(xf, yf), || self.patch(xf, yf))
    }

    /// The surface kind from a height and the climate, asking for the
    /// beach, the shore and the dirt patch only where they decide: the walk answers
    /// them from its frame grid and cached fields (`Fields`), the map from
    /// its chunks. `wet` is whether the tile under the point is water: the
    /// drawn surface is flattened to sea level over water, so at the clamp
    /// the height alone cannot tell the sea from the beach beside it.
    pub fn surface_kind(&self, h: f32, temp: f32, wet: bool, beach: impl FnOnce() -> bool, shore: impl FnOnce() -> bool, patch: impl FnOnce() -> f32) -> Terrain {
        if let Some(f) = &self.fixture {
            return f.terrain;
        }
        if h < SEA as f32 || (wet && h <= SEA as f32) {
            Terrain::Water
        } else if (h < SEA as f32 + 0.45 && beach()) || (h < SEA as f32 + 1.3 && shore()) {
            Terrain::Sand
        } else if temp <= -16.0 {
            Terrain::Snow
        } else if h >= ROCK_Z as f32 {
            Terrain::Rock
        } else if patch() > 0.72 {
            Terrain::Dirt
        } else {
            Terrain::Grass
        }
    }

    fn is_water(&self, x: i32, y: i32) -> bool {
        self.height(x, y) < SEA
    }

    /// Whether water lies within `SHORE_TILES` of a tile, the width of a
    /// beach. Heights come from the field rather than from tiles, so this
    /// stays a pure function of position and seed and generates no chunk.
    fn near_shore(&self, x: i32, y: i32) -> bool {
        let r = SHORE_TILES;
        (-r..=r).any(|dy| (-r..=r).any(|dx| dx * dx + dy * dy <= r * r && self.is_water(x + dx, y + dy)))
    }

    /// Annual mean temperature: a slow latitude-like field, minus the lapse
    /// rate with height, so the same region cools going uphill. The lapse
    /// follows the relief curve, so the bands sit where they did before
    /// heights became metres (`LAPSE`).
    pub fn temperature(&self, x: i32, y: i32, z: i32) -> f32 {
        let lat = fbm(x as f32 * 0.0022, y as f32 * 0.0022, self.seed ^ 0x7E, 2) * 2.0 - 1.0;
        let local = fbm(x as f32 * 0.02, y as f32 * 0.02, self.seed ^ 0x7F, 2) * 2.0 - 1.0;
        26.0 - 36.0 * (lat + 1.0) * 0.5 + local * 3.0 - LAPSE * relief_fraction((z.max(SEA) - SEA) as f32)
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
        if let Some(f) = &self.fixture {
            return Climate { hf, z, temp: f.temp as f32, precip: 50.0, biome: f.biome };
        }
        let temp = self.temperature(x, y, z);
        let precip = self.precipitation(x, y);
        let biome = self.assets.koppen[biome::classify(temp, precip)];
        Climate { hf, z, temp, precip, biome }
    }

    /// The settlement field at a ground point: how built-up the country is
    /// there, in 0..1. It runs over tens of tiles, so a village is a
    /// cluster of plots rather than one lone house.
    fn settlement(&self, xf: f32, yf: f32) -> f32 {
        fbm(xf * 0.03 + 7.0, yf * 0.03, self.seed ^ 0xB1, 2)
    }

    /// The building of a settlement plot, a pure function of the plot and
    /// the seed: the first block kind, in table order, whose rule passes
    /// the settlement field and its own roll, sized from its footprint
    /// range and set inside the plot's margin so two buildings never touch
    /// and merge into one. `None` where nothing is built.
    fn plot_building(&self, px: i32, py: i32) -> Option<Building> {
        let hv = hash(px as i64, py as i64, self.seed ^ 0xB1D6);
        let (cx, cy) = (px * PLOT + PLOT / 2, py * PLOT + PLOT / 2);
        let settle = self.settlement(cx as f32, cy as f32);
        let (kind, b) = self.assets.blocks.iter().enumerate().find(|(i, k)| {
            let roll = hash(px as i64, py as i64, self.seed ^ (0x5E77 + *i as u64)) % 100;
            k.chance > 0 && settle > k.settle_min && roll < k.chance
        })?;
        let span = |lo: u8, hi: u8, bits: u32| lo as i32 + ((hv >> bits) % (hi as u64 - lo as u64 + 1)) as i32;
        let [[wmin, dmin], [wmax, dmax]] = b.footprint;
        let (w, d) = (span(wmin, wmax, 8), span(dmin, dmax, 12));
        // Inside the plot with a tile of margin all round.
        let (room_x, room_y) = (PLOT - w - 2, PLOT - d - 2);
        if room_x < 1 || room_y < 1 {
            return None;
        }
        let x0 = px * PLOT + 1 + ((hv >> 16) % room_x as u64) as i32;
        let y0 = py * PLOT + 1 + ((hv >> 24) % room_y as u64) as i32;
        // The plot must be land the kind stands on, and flat enough that one
        // roof covers it: every corner within two metres of the first tile.
        if !b.terrain.contains(&self.terrain_at(x0, y0)) {
            return None;
        }
        let base = self.height(x0, y0);
        if base <= SEA {
            return None;
        }
        for (cx, cy) in [(x0 + w - 1, y0), (x0, y0 + d - 1), (x0 + w - 1, y0 + d - 1), (x0 + w / 2, y0 + d / 2)] {
            if (self.height(cx, cy) - base).abs() > 2 {
                return None;
            }
        }
        // Deeper into the settlement, taller.
        let [lo, hi] = b.levels;
        let deep = ((settle - b.settle_min) * 6.0).clamp(0.0, 1.0);
        let roll = ((hv >> 48) % 100) as f32 / 100.0;
        let span = (hi - lo) as u64 + 1;
        let extra = ((roll * (0.4 + deep)) * span as f32) as u64;
        let levels = lo + extra.min(span - 1) as u8;
        Some(Building { x0, y0, w, d, stack: Stack { kind: kind as u8, levels } })
    }

    /// Terrain at a tile without generating its chunk, as `tile` classifies
    /// it: the settlement generator asks before any tile exists.
    fn terrain_at(&self, x: i32, y: i32) -> Terrain {
        let c = self.climate(x, y);
        let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.is_water(x + dx, y + dy));
        let shore = c.z == SEA && self.near_shore(x, y);
        terrain_for(c.z, c.temp, near_water, shore, self.patch(x as f32 + 0.5, y as f32 + 0.5))
    }

    /// Paint the buildings of every plot the chunk touches onto its tiles.
    /// A building stands on whole tiles of one kind and level count, so the
    /// geometry merges it into one house with one roof and one door, and
    /// its ground is cleared of trees.
    fn build_plots(&self, tiles: &mut [Tile], x0: i32, y0: i32) {
        let (px0, py0) = ((x0 - PLOT).div_euclid(PLOT), (y0 - PLOT).div_euclid(PLOT));
        let (px1, py1) = ((x0 + CHUNK).div_euclid(PLOT), (y0 + CHUNK).div_euclid(PLOT));
        for py in py0..=py1 {
            for px in px0..=px1 {
                let Some(b) = self.plot_building(px, py) else { continue };
                // The building stands in a clearing: its own tiles carry the
                // stack, and the ring around it is cleared of trees, so a
                // house in a wood is not buried in one.
                for ty in b.y0 - CLEARING..b.y0 + b.d + CLEARING {
                    for tx in b.x0 - CLEARING..b.x0 + b.w + CLEARING {
                        let (lx, ly) = (tx - x0, ty - y0);
                        if lx < 0 || ly < 0 || lx >= CHUNK || ly >= CHUNK {
                            continue;
                        }
                        let t = &mut tiles[(ly * CHUNK + lx) as usize];
                        if t.terrain == Terrain::Water {
                            continue;
                        }
                        t.tree = None;
                        let inside = (b.x0..b.x0 + b.w).contains(&tx) && (b.y0..b.y0 + b.d).contains(&ty);
                        if inside {
                            t.stack = Some(b.stack);
                        }
                    }
                }
            }
        }
    }

    /// Classify one tile from the height field and climate around it.    /// Classify one tile from the height field and climate around it.
    fn tile(&self, x: i32, y: i32) -> Tile {
        if let Some(f) = &self.fixture {
            let hv = hash(x as i64, y as i64, self.seed);
            return Tile {
                z: f.z,
                terrain: f.terrain,
                tree: None,
                grass: f.grass,
                seed: (hv >> 32) as u32,
                body_size: 0,
                biome: f.biome as u8,
                stack: None,
                material: f.material as u8,
                temp: f.temp,
                near_water: false,
                hf: f.z as f32 + 0.5,
            };
        }
        let climate = self.climate(x, y);
        let z = climate.z;
        let temp = climate.temp;
        let biome = &self.assets.biomes[climate.biome];
        let near_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.is_water(x + dx, y + dy));
        let patch = self.patch(x as f32 + 0.5, y as f32 + 0.5);
        let shore = z == SEA && self.near_shore(x, y);
        let terrain = terrain_for(z, temp, near_water, shore, patch);

        let hv = hash(x as i64, y as i64, self.seed);
        let forest = fbm(x as f32 * 0.08, y as f32 * 0.08, self.seed ^ 0xF0, 3);
        let density = biome.tree_density * crate::noise::smoothstep(0.3, 0.7, forest);
        // A tree needs room for its crown: the sites are the tiles that win
        // their species' spacing, and the biome's density is the fraction of
        // those that carry one.
        let tree = if terrain == Terrain::Grass && z > SEA && !biome.species.is_empty() {
            let si = biome::weighted_pick(&biome.species, hv >> 16);
            let spacing = self.assets.species[si].spacing() * biome.spacing / TILE_METRES;
            if self.stands_clear(x, y, spacing) && (hv % 1000) as f32 / 1000.0 < density {
                Some(Flora { species: si as u8, variant: ((hv >> 8) % 4) as u8 })
            } else {
                None
            }
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
            body_size: 0,
            biome: climate.biome as u8,
            stack: None,
            material: material_for(terrain, z, biome, self.stone) as u8,
            temp: temp.round().clamp(-60.0, 60.0) as i8,
            near_water,
            hf: climate.hf,
        }
    }

    fn generate_chunk(&self, cx: i32, cy: i32) -> Chunk {
        let (x0, y0) = (cx * CHUNK, cy * CHUNK);
        let mut tiles: Vec<Tile> = (0..CHUNK * CHUNK).map(|i| self.tile(x0 + i % CHUNK, y0 + i / CHUNK)).collect();
        self.label_water_bodies(&mut tiles, x0, y0);
        if self.fixture.is_none() {
            self.build_plots(&mut tiles, x0, y0);
        }
        let placed = self.stacks.borrow();
        if !placed.is_empty() {
            for (i, t) in tiles.iter_mut().enumerate() {
                if let Some(st) = placed.get(&(x0 + i as i32 % CHUNK, y0 + i as i32 / CHUNK)) {
                    t.stack = *st;
                }
            }
        }
        Chunk { max_z: Self::ceiling_of(&tiles, &self.assets), tiles }
    }

    /// The ceiling over a chunk's tiles.
    fn ceiling_of(tiles: &[Tile], assets: &Assets) -> i32 {
        tiles.iter().map(|t| t.top(assets).ceil() as i32).max().unwrap_or(0)
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
impl Tile {
    /// A bare tile at height `z` for synthetic test maps: water below sea
    /// level, otherwise plain grass of the first biome, at 15 degrees.
    pub(crate) fn flat(z: i32) -> Tile {
        Tile { z, terrain: if z < SEA { Terrain::Water } else { Terrain::Grass }, tree: None, grass: 0, seed: 0, body_size: 0, biome: 0, stack: None, material: 0, temp: 15, near_water: false, hf: z as f32 + 0.5 }
    }
}

#[cfg(test)]
impl Map {
    /// A bounded `w` x `h` map whose tiles come from `tile` instead of the
    /// noise fields. Chunks within `radius` chunks of the origin are filled
    /// so an unbounded view over them never reaches the generator.
    pub(crate) fn synthetic(w: usize, h: usize, assets: Rc<Assets>, radius: i32, tile: impl Fn(i32, i32) -> Tile) -> Map {
        let map = Map::new(w, h, 0, assets);
        for cy in -radius..=radius {
            for cx in -radius..=radius {
                let (x0, y0) = (cx * CHUNK, cy * CHUNK);
                let tiles: Vec<Tile> = (0..CHUNK * CHUNK).map(|i| tile(x0 + i % CHUNK, y0 + i / CHUNK)).collect();
                let max_z = Map::ceiling_of(&tiles, &map.assets);
                map.chunks.borrow_mut().insert((cx, cy), Chunk { tiles, max_z });
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    #[test]
    fn synthetic_tiles_come_from_the_closure() {
        let m = Map::synthetic(8, 8, test_assets(), 1, |x, y| Tile::flat(if (x, y) == (3, 4) { 9 } else { -2 }));
        assert_eq!(m.get(3, 4).unwrap().z, 9);
        assert_eq!(m.get(0, 0).unwrap().terrain, Terrain::Water, "below sea level is water");
        assert_eq!(m.ceiling(0, 0), 10, "the ceiling rounds the smooth height 9.5 up");
        assert!(m.get(8, 0).is_none());
    }

    #[test]
    fn tiles_ignore_the_order_chunks_are_touched_in() {
        let assets = test_assets();
        let mut a = Map::new(8, 8, 7, assets.clone());
        let mut b = Map::new(8, 8, 7, assets);
        a.bounded = false;
        b.bounded = false;
        let probes = [(-40, -40), (0, 0), (33, -2), (70, 70), (-1, 31), (31, -1)];
        for &(x, y) in &probes {
            a.get(x, y);
        }
        for &(x, y) in probes.iter().rev() {
            b.get(x, y);
        }
        for y in (-48..80).step_by(5) {
            for x in (-48..80).step_by(7) {
                let (ta, tb) = (a.get(x, y).unwrap(), b.get(x, y).unwrap());
                assert_eq!((ta.z, ta.terrain, ta.biome, ta.body_size), (tb.z, tb.terrain, tb.biome, tb.body_size), "({x}, {y})");
                assert!(ta.tree.map(|f| (f.species, f.variant)) == tb.tree.map(|f| (f.species, f.variant)), "({x}, {y}) tree");
            }
        }
    }

    #[test]
    fn nearest_land_is_the_closest_land_tile_to_a_water_cursor() {
        let m = Map::synthetic(16, 16, test_assets(), 0, |x, y| Tile::flat(if (x, y) == (10, 6) { 5 } else { -2 }));
        assert_eq!(m.nearest_land(2, 2), (10, 6));
        assert_eq!(m.nearest_land(15, 15), (10, 6));
        assert_eq!(m.nearest_land(10, 6), (10, 6), "a land cursor is its own nearest land");
    }

    #[test]
    fn tiles_are_deterministic_and_chunk_independent() {
        let assets = test_assets();
        let a = Map::new(32, 32, 7, assets.clone());
        let mut b = Map::new(32, 32, 7, assets);
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
    fn ceiling_bounds_every_tile_with_its_stack_and_crown() {
        let mut m = Map::new(8, 8, 7, test_assets());
        m.bounded = false;
        let (mut stacks, mut trees) = (0, 0);
        for y in -40..40 {
            for x in -40..40 {
                let t = m.get(x, y).unwrap();
                assert!(t.draw_z() <= m.ceiling(x, y));
                assert!(t.top(&m.assets) <= m.ceiling(x, y) as f32, "({x}, {y}): top {} over ceiling {}", t.top(&m.assets), m.ceiling(x, y));
                stacks += t.stack.is_some() as u32;
                trees += t.tree.is_some() as u32;
                if let Some(st) = t.stack {
                    assert!(t.top(&m.assets) >= t.hf.max(SEA as f32) + st.levels as f32 * st.kind(&m.assets).level_height, "a stack lifts the top");
                }
            }
        }
        assert!(stacks > 0 && trees > 0, "the sample holds stacks ({stacks}) and trees ({trees})");
    }

    #[test]
    fn placed_stacks_override_the_generator_and_lift_the_ceiling() {
        let m = Map::new(32, 32, 7, test_assets());
        let (x, y) = m.nearest_land(16, 16);
        let before = m.ceiling(x, y);
        m.set_stack(x, y, Some(Stack { kind: 0, levels: 3 }));
        let t = m.get(x, y).unwrap();
        assert_eq!(t.stack, Some(Stack { kind: 0, levels: 3 }));
        assert!(m.ceiling(x, y) >= before && m.ceiling(x, y) as f32 >= t.top(&m.assets));
        m.set_stack(x, y, None);
        assert_eq!(m.get(x, y).unwrap().stack, None);
        // An override made before the chunk exists is applied when it is
        // generated.
        let far = Map::new(64, 64, 7, test_assets());
        far.set_stack(40, 40, Some(Stack { kind: 0, levels: 2 }));
        assert_eq!(far.get(40, 40).unwrap().stack, Some(Stack { kind: 0, levels: 2 }));
        far.set_stack(-1, -1, Some(Stack { kind: 0, levels: 1 }));
        assert!(far.get(-1, -1).is_none(), "off a bounded map nothing is placed");
    }

    #[test]
    fn bounds_helpers_follow_bounded() {
        let mut m = Map::new(8, 8, 3, test_assets());
        assert!(m.contains(0, 0) && m.contains(7, 7));
        assert!(!m.contains(-1, 0) && !m.contains(8, 0));
        assert_eq!(m.clamp(-5, 20), (0, 7));
        m.bounded = false;
        assert!(m.contains(-100, 100));
        assert_eq!(m.clamp(-5, 20), (-5, 20));
    }

    #[test]
    fn climate_agrees_with_tiles() {
        let m = Map::new(32, 32, 7, test_assets());
        for &(x, y) in &[(0, 0), (31, 31), (5, 17), (16, 16)] {
            let c = m.climate(x, y);
            let t = m.get(x, y).unwrap();
            assert_eq!(c.z, t.z);
            assert_eq!(c.biome, t.biome as usize);
            assert_eq!(c.hf, t.hf);
        }
    }

    #[test]
    fn fixture_is_flat_and_takes_placed_tiles() {
        let assets = test_assets();
        let spec = FixtureSpec { w: 12, h: 12, z: 6, biome: 5, terrain: Terrain::Grass, temp: 12, grass: 2, material: 1 };
        let mut m = Map::fixture(spec, assets);
        assert!(m.is_fixture());
        for (x, y) in [(0, 0), (11, 11), (6, 6)] {
            let t = m.get(x, y).unwrap();
            assert_eq!((t.z, t.terrain, t.biome, t.material, t.temp, t.grass), (6, Terrain::Grass, 5, 1, 12, 2));
            assert!(t.tree.is_none() && t.stack.is_none());
            assert_eq!(t.hf, 6.5);
            assert_eq!(m.height(x, y), 6);
            assert_eq!(m.climate(x, y).biome, 5);
        }
        assert_eq!(m.detail(3.3, 4.4, 3), 0.0);
        assert_eq!(m.fields(0, 0, 8, 8, 3).detail(3.3, 4.4), 0.0, "nor from the cached fields");
        assert_eq!(m.surface_at(3.3, 4.4, 0.0, -30.0), Terrain::Grass);
        assert!(m.get(12, 0).is_none());
        let mut t = m.get(6, 6).unwrap();
        t.tree = Some(Flora { species: 2, variant: 1 });
        t.z = 9;
        m.set_tile(6, 6, t);
        assert_eq!(m.get(6, 6).unwrap().tree.map(|f| f.species), Some(2));
        assert!(m.ceiling(6, 6) as f32 >= t.top(&m.assets), "the ceiling covers the placed tree's crown");
        m.set_tile(40, 40, t);
        assert!(m.get(40, 40).is_none());
    }

    #[test]
    fn cached_fields_are_the_map_fields_to_the_bit() {
        let m = Map::new(32, 32, 7, test_assets());
        let f = m.fields(100, -50, 140, -10, 3);
        for i in 0..2000 {
            // Inside the tile range and beyond it.
            let (x, y) = (95.0 + i as f32 * 0.027, -55.0 + (i as f32 * 0.019).sin() * 30.0);
            assert_eq!(f.detail(x, y).to_bits(), m.detail(x, y, 3).to_bits(), "detail at ({x}, {y})");
            assert_eq!(f.patch(x, y).to_bits(), m.patch(x, y).to_bits(), "patch at ({x}, {y})");
        }
        assert_eq!(m.fields(0, 0, 8, 8, 0).detail(3.3, 4.4), 0.0, "no octaves at the overview, no detail");
    }

    #[test]
    fn relief_puts_a_valley_floor_a_few_metres_over_the_water_and_a_range_at_the_ceiling() {
        // The generator's field becomes metres through `relief` (ADR-004):
        // the shoreline is zero, the sea bed FLOOR, a valley floor a metre
        // or two up and the top of a range RELIEF.
        assert_eq!(relief(SEA_UNITS), 0.0);
        assert_eq!(relief(0.0), FLOOR);
        assert_eq!(relief(FIELD_UNITS), RELIEF);
        let valley = relief(SEA_UNITS + 1.0);
        assert!((1.0..4.0).contains(&valley), "a valley floor is a few metres over the water: {valley}");
        assert!(relief(ROCK_UNITS).round() as i32 == ROCK_Z, "bare rock starts at ROCK_Z: {}", relief(ROCK_UNITS));
        // The curve rises, and `relief_fraction` inverts it.
        let mut last = FLOOR;
        for i in 0..=60 {
            let u = i as f32 * 0.25;
            let m = relief(u);
            assert!(m >= last, "the curve rises at {u}");
            last = m;
            if u > SEA_UNITS {
                let back = relief_fraction(m) * (FIELD_UNITS - SEA_UNITS) + SEA_UNITS;
                assert!((back - u).abs() < 0.01, "relief_fraction inverts relief at {u}: {back}");
            }
        }
    }

    #[test]
    fn generated_heights_are_metres_within_the_relief() {
        let mut m = Map::new(8, 8, 7, test_assets());
        m.bounded = false;
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for y in (-400..400).step_by(37) {
            for x in (-400..400).step_by(31) {
                let h = m.height_smooth(x as f32, y as f32);
                lowest = lowest.min(h);
                highest = highest.max(h);
                assert!((FLOOR..=RELIEF).contains(&h), "({x}, {y}): {h} m is outside the relief");
                assert_eq!(m.height(x, y), m.height_smooth(x as f32 + 0.5, y as f32 + 0.5).floor() as i32, "the tile terraces at whole metres");
            }
        }
        assert!(lowest < 0.0 && highest > 40.0, "the sample holds sea bed and highlands: {lowest} to {highest}");
    }

    #[test]
    fn the_generator_builds_whole_buildings_on_plots() {
        // Every generated building is a rectangle of one kind and one level
        // count, so the geometry merges it into one house with one roof,
        // and two buildings never touch.
        let mut m = Map::new(8, 8, 7, test_assets());
        m.bounded = false;
        let mut plots = 0;
        let mut seen: Vec<(i32, i32, i32, i32)> = Vec::new();
        for py in -12..12 {
            for px in -12..12 {
                let Some(b) = m.plot_building(px, py) else { continue };
                plots += 1;
                let kind = &m.assets.blocks[b.stack.kind as usize];
                let [[wmin, dmin], [wmax, dmax]] = kind.footprint;
                assert!((wmin as i32..=wmax as i32).contains(&b.w) && (dmin as i32..=dmax as i32).contains(&b.d), "{} is {}x{} tiles", kind.name, b.w, b.d);
                assert!((kind.levels[0]..=kind.levels[1]).contains(&b.stack.levels), "{}: {} levels", kind.name, b.stack.levels);
                // Inside its own plot with a tile of margin.
                assert!(b.x0 > px * PLOT && b.x0 + b.w < (px + 1) * PLOT, "{} runs into the next plot", kind.name);
                assert!(b.y0 > py * PLOT && b.y0 + b.d < (py + 1) * PLOT);
                seen.push((b.x0, b.y0, b.w, b.d));
                if plots <= 40 {
                    let c = m.climate(b.x0, b.y0);
                    eprintln!("plot ({px}, {py}): {} {}x{} at ({}, {}) levels {} biome {}", kind.name, b.w, b.d, b.x0, b.y0, b.stack.levels, m.assets.biomes[c.biome].name);
                }
            }
        }
        assert!(plots > 10, "the sample holds buildings ({plots} plots)");
        // Every tile of a building carries its stack, and the tiles around
        // it are clear, so nothing merges across the gap.
        let (x0, y0, w, d) = seen[0];
        for y in y0 - 1..y0 + d + 1 {
            for x in x0 - 1..x0 + w + 1 {
                let inside = (x0..x0 + w).contains(&x) && (y0..y0 + d).contains(&y);
                let t = m.get(x, y).unwrap();
                if inside {
                    assert!(t.stack.is_some() || t.terrain == Terrain::Water, "({x}, {y}) is part of the building");
                    assert!(t.tree.is_none(), "a building clears its ground");
                } else {
                    assert!(t.stack.is_none(), "({x}, {y}) is the margin around the building");
                }
            }
        }
    }

    #[test]
    fn nearest_land_is_never_water() {
        let m = Map::new(32, 32, 7, test_assets());
        let (x, y) = m.nearest_land(0, 0);
        assert_ne!(m.get(x, y).unwrap().terrain, Terrain::Water);
    }

    #[test]
    fn sand_is_a_shore_terrain_and_an_inland_basin_at_sea_level_is_not_a_beach() {
        let mut m = Map::new(32, 32, 7, test_assets());
        m.bounded = false;
        let water_within = |m: &Map, x: i32, y: i32, r: i32| (-r..=r).any(|dy| (-r..=r).any(|dx| m.get(x + dx, y + dy).map(|t| t.terrain == Terrain::Water).unwrap_or(false)));
        // A plain of seed 7 that sits exactly at sea level with no water for
        // twice the beach band around it: grass, not sand, at the tile and
        // at any point on it.
        let (x, y) = (208, -488);
        let t = m.get(x, y).unwrap();
        assert_eq!(t.z, SEA, "the basin is at sea level");
        assert!(!water_within(&m, x, y, 2 * SHORE_TILES), "and has no water anywhere near it");
        assert_ne!(t.terrain, Terrain::Sand, "so it is not a beach");
        assert_ne!(m.surface_at(x as f32 + 0.5, y as f32 + 0.5, t.hf.max(SEA as f32), t.temp as f32), Terrain::Sand, "and the continuous surface agrees");
        // The shore of a lake, at the same height, still is.
        let (bx, by) = (-205, 284);
        let b = m.get(bx, by).unwrap();
        assert_eq!(b.z, SEA);
        assert!(water_within(&m, bx, by, SHORE_TILES), "this one has water within the band");
        assert_eq!(b.terrain, Terrain::Sand, "so it is a beach");
        assert_eq!(m.surface_at(bx as f32 + 0.5, by as f32 + 0.5, b.hf.max(SEA as f32), b.temp as f32), Terrain::Sand);
        // The lake itself is water, not the sand band: the drawn surface is
        // flattened to sea level over it, so the height alone cannot say.
        let r = SHORE_TILES;
        let (wx, wy) = (-r..=r).flat_map(|dy| (-r..=r).map(move |dx| (bx + dx, by + dy))).find(|&(x, y)| m.get(x, y).unwrap().terrain == Terrain::Water).expect("the lake is within the band");
        let l = m.get(wx, wy).unwrap();
        assert_eq!(m.surface_at(wx as f32 + 0.5, wy as f32 + 0.5, l.hf.max(SEA as f32), l.temp as f32), Terrain::Water);
    }
}
