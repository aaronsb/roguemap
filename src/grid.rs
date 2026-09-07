//! The per-frame grid the ray walk reads: every tile in view with its
//! smooth height, its stack resolved into a column with merged runs, and
//! the tree volumes whose footprint touches it. `Map::get` runs only while
//! the grid is built, so the walk itself never touches the chunk map.

use std::collections::HashMap;
use std::rc::Rc;

use crate::biome::Species;
use crate::blocks::{self, door_face, label_runs, merges, one_plot, ridge_along_x, Column, Ground, Profile, Runs, Stack, NO_FACE};
use crate::lsystem::{Growth, Placement, State, TreeModel};
use crate::map::{Fields, Tile, MAX_Z, SEA, TILE_METRES};
use crate::noise::{hash01, ifloor};
use crate::render::Scene;
use crate::volume::{instance, size_scale, stands_dead, variant_scale, Shape, Volume};

/// Highest anything can reach above the ground, in metres: the tallest tree
/// or roof over the highest terrain.
pub(crate) const TOP_CAP: f32 = MAX_Z as f32 + 40.0;
/// Metres a tree or a roof can add over the terrain under it; the view is
/// sized from the terrain's own ceiling plus this.
pub(crate) const CROWN_CAP: f32 = 40.0;
/// Tiles per block of the coarse ceiling grid.
const BLOCK: i32 = 8;
/// Ground margin around a volume's footprint so a segment between two
/// walk samples cannot cross it unregistered: the walk reads the volumes
/// of the lower sample's tile and tests them over the whole segment, so a
/// volume has to be registered a little beyond the ground it covers. The
/// step is shorter at the near zooms, where a tree is its own primitives
/// and the margin would otherwise be most of what a leaf cluster costs.
const VOLUME_MARGIN: f32 = 0.4;
const PRIMITIVE_MARGIN: f32 = 0.2;
/// Cells beyond the screen a tree may stand and still matter, through its
/// crown leaning in or its shadow falling across the edge.
const MARGIN: f32 = 64.0;
/// Buckets the volumes are counting-sorted into by their tops, so every
/// tile's list comes out tallest first without an n log n sort of the
/// hundred thousand primitives a stand of grown trees can carry.
const TOP_BUCKETS: usize = 2048;
/// Steps the foliage of a tree is quantised to for the model cache: a
/// grown model is one of nine states between bare and full, so a season
/// creeping forward does not rebuild every tree every frame.
const FOLIAGE_STEPS: f32 = 8.0;
/// Frames a grown model stays cached after the last frame that wanted it.
const MODEL_TTL: u32 = 4;
/// Models the cache holds before the oldest are dropped.
const MODEL_CAP: usize = 3000;

/// Which tree a cached model is: everything `TreeModel::build` reads, and
/// the zoom it was simplified for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct ModelKey {
    species: u8,
    seed: u32,
    /// Foliage in `0..=FOLIAGE_STEPS`.
    foliage: u8,
    dead: bool,
    detail: u8,
}

/// How fine the walk needs a grown tree at a zoom: leaf clusters closer
/// than `cell` metres are one blob on the screen and branches thinner than
/// `min_radius` are hidden under their own foliage, so the model the walk
/// tests is simplified to that (`TreeModel::simplify`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Detail {
    pub level: u8,
    pub cell: f32,
    pub min_radius: f32,
}

impl Detail {
    /// The detail a camera asks for: a lattice four rows of height across
    /// and a twig about a column thick. The level counts the zooms a model
    /// is grown at: 0 at the mid zoom, 1 near, 2 close.
    pub(crate) fn of(cam: &crate::camera::Camera) -> Detail {
        Detail::at(cam.rows_per_metre(), cam.columns_per_metre())
    }

    /// The same from a zoom's scale alone.
    pub(crate) fn at(rows: f32, cols: f32) -> Detail {
        let level = if rows >= 6.0 {
            2
        } else if rows >= 3.0 {
            1
        } else {
            0
        };
        Detail { level, cell: 4.0 / rows.max(0.1), min_radius: 1.1 / cols.max(0.1) }
    }
}

/// Grown L-system trees kept between frames (docs/lsystem.md). Growing one
/// is a string rewriting and a turtle walk; a stand of them every frame
/// would cost more than drawing them, so a model is built once per tree and
/// reused while the tree stays in view.
#[derive(Default)]
pub struct ModelCache {
    models: HashMap<ModelKey, (Rc<TreeModel>, u32)>,
    frame: u32,
}

impl ModelCache {
    pub fn new() -> ModelCache {
        ModelCache::default()
    }

    /// Start a frame: nothing is evicted until it ends.
    fn begin(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// The model of one instance, grown if this frame is the first to want
    /// it. `foliage` is already quantised to `FOLIAGE_STEPS`.
    fn model(&mut self, index: u8, sp: &Species, seed: u32, foliage: u8, dead: bool, detail: Detail) -> Option<Rc<TreeModel>> {
        let key = ModelKey { species: index, seed, foliage, dead, detail: detail.level };
        let frame = self.frame;
        if let Some(entry) = self.models.get_mut(&key) {
            entry.1 = frame;
            return Some(entry.0.clone());
        }
        let state = if dead { State::Dead } else { State::Alive };
        let growth = Growth { foliage: foliage as f32 / FOLIAGE_STEPS, state };
        let model = Rc::new(sp.tree_model(seed as u64, growth)?.simplify(detail.cell, detail.min_radius));
        self.models.insert(key, (model.clone(), frame));
        Some(model)
    }

    /// Drop what this frame did not want, once it has gone out of view.
    fn sweep(&mut self) {
        let (frame, ttl) = (self.frame, MODEL_TTL);
        self.models.retain(|_, (_, used)| frame.wrapping_sub(*used) <= ttl);
        if self.models.len() > MODEL_CAP {
            let mut ages: Vec<u32> = self.models.values().map(|(_, u)| frame.wrapping_sub(*u)).collect();
            ages.sort_unstable();
            let cut = ages[MODEL_CAP];
            self.models.retain(|_, (_, used)| frame.wrapping_sub(*used) < cut);
        }
    }
}

/// What stands on a tile, resolved for the walk.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geo {
    pub stack: Option<Stack>,
    pub runs: Runs,
    /// Open face bits (`1 << face`): faces not shared with a merged tile.
    pub open: u8,
    /// The door face, or `NO_FACE`.
    pub door: u8,
    /// Ground under the stack, and whether the tile is flattened to it.
    pub base: f32,
    pub flat: bool,
    /// Eaves height of the column; `base` when there is none.
    pub zs: f32,
    /// Highest solid over this tile: terrain bound, column and roof, or
    /// any canopy reaching over it.
    pub top: f32,
    /// Upper bound of the terrain surface in the 3x3 around the tile.
    pub hmax: f32,
    /// Whether the tile is on the map at all; the walk asks at every
    /// sample, and this keeps it out of the tile array.
    pub on_map: bool,
    /// The coarse ceiling of the block the tile is in (`block_top`), here
    /// so the walk's sample reads one record.
    pub ceiling: f32,
    /// Range into the volume index.
    pub vol: (u32, u32),
    /// Tallest volume on the tile measured base to top: how far back from
    /// the height a segment reaches the index must be read, since the
    /// entries are ordered by their tops.
    pub vspan: f32,
}

/// One entry of a tile's volume list. The heights sit beside the index so
/// the walk can drop everything outside its segment without touching the
/// volume itself: a stand of grown trees puts a couple of hundred branches
/// and leaf clusters on a tile, and reading each of them to find the dozen
/// in range is what a forest cannot afford.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bucket {
    pub top: f32,
    pub h0: f32,
    /// Ground circle of the volume, from `Volume::reach`, its radius
    /// squared for the walk's distance test.
    pub cx: f32,
    pub cy: f32,
    pub r2: f32,
    pub v: u32,
    /// A leaf cluster: an upright ellipsoid the walk can solve from this
    /// entry alone, without reading the volume.
    pub cluster: bool,
}

/// Smooth heights, tiles and geometry of the tiles in view.
pub(crate) struct HeightGrid {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    data: Vec<f32>,
    tiles: Vec<Option<Tile>>,
    geo: Vec<Geo>,
    /// Every volume the frame carries: one stand-in per tree, or, where a
    /// grown species draws from its model, that model's branches and leaf
    /// clusters and the stand-in the shadow mask still sweeps.
    pub(crate) volumes: Vec<Volume>,
    /// One index into `volumes` per tree, at its stand-in: what casts a
    /// shadow and what a crown counts as standing over it.
    pub(crate) crowns: Vec<u32>,
    vol_index: Vec<Bucket>,
    /// How far out of order the counting sort by tops can leave the index,
    /// in metres: the walk widens its search by this and loses nothing.
    slack: f32,
    /// Coarse maxima of `top` per `BLOCK` x `BLOCK` tiles.
    blocks: Vec<f32>,
    bw: i32,
    /// Highest `top` anywhere in the grid.
    pub(crate) max_top: f32,
    /// The detail and patch fields over the grid, hashed once.
    pub(crate) fields: Fields,
}

/// The volumes' indices, tallest first, by counting sort on their tops:
/// linear in the number of volumes, where sorting each tile's list is not.
/// The buckets are about a finger's width of height apart over whatever
/// range the frame spans, which is finer than the walk's own step, so the
/// order is exact wherever it matters.
fn top_order(volumes: &[Volume]) -> (Vec<u32>, f32) {
    let mut order = vec![0u32; volumes.len()];
    if volumes.is_empty() {
        return (order, 0.0);
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for v in volumes {
        lo = lo.min(v.top());
        hi = hi.max(v.top());
    }
    let span = (hi - lo).max(1e-3);
    let k = TOP_BUCKETS as f32 / span;
    let bucket = |v: &Volume| (TOP_BUCKETS - 1).saturating_sub((((v.top() - lo) * k) as usize).min(TOP_BUCKETS - 1));
    let mut counts = vec![0u32; TOP_BUCKETS + 1];
    for v in volumes {
        counts[bucket(v)] += 1;
    }
    let mut start = 0u32;
    for c in counts.iter_mut() {
        let n = *c;
        *c = start;
        start += n;
    }
    for (i, v) in volumes.iter().enumerate() {
        let b = bucket(v);
        order[counts[b] as usize] = i as u32;
        counts[b] += 1;
    }
    (order, span / TOP_BUCKETS as f32)
}

impl HeightGrid {
    /// Build the grid over the tile range for the frame, for a screen of
    /// `sw` x `sh` cells: trees far off that screen are not made into
    /// volumes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(sc: &Scene, x0: i32, y0: i32, x1: i32, y1: i32, sw: i32, sh: i32, cache: &mut ModelCache) -> HeightGrid {
        let (map, assets, world, cam) = (sc.map, sc.assets, sc.world, sc.cam);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let n = (w * h) as usize;
        let tiles = map.tiles_in(x0, y0, x1, y1);
        let data: Vec<f32> = tiles.iter().map(|t| t.map(|t| t.hf).unwrap_or(0.0)).collect();
        let octaves = crate::raster::detail_octaves(cam);
        let fields = map.fields(x0, y0, x1, y1, octaves);
        let empty = Geo { stack: None, runs: Runs::SINGLE, open: 0, door: NO_FACE, base: 0.0, flat: false, zs: 0.0, top: 0.0, hmax: 0.0, on_map: false, ceiling: 0.0, vol: (0, 0), vspan: 0.0 };
        let mut geo = vec![empty; n];
        let at = |x: i32, y: i32| ((y - y0) * w + (x - x0)) as usize;

        // Terrain bound and stacks.
        let kind_merges = |k: u8| assets.blocks[k as usize % assets.blocks.len()].merge;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = at(x, y);
                let mut hmax = SEA as f32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = ((x + dx).clamp(x0, x1), (y + dy).clamp(y0, y1));
                        hmax = hmax.max(data[at(nx, ny)]);
                    }
                }
                let g = &mut geo[i];
                g.hmax = hmax + 0.5;
                g.top = g.hmax;
                let Some(t) = tiles[i] else { continue };
                g.on_map = true;
                g.base = t.hf.max(SEA as f32);
                g.zs = g.base;
                g.stack = t.stack;
                if let Some(st) = t.stack {
                    let b = &assets.blocks[st.kind as usize % assets.blocks.len()];
                    g.flat = b.ground == Ground::Flatten && st.levels > 0;
                    g.zs = g.base + st.levels as f32 * b.level_height;
                }
            }
        }
        // Merged runs along each axis, then the open faces and the door.
        let stack_at = |x: i32, y: i32| -> Option<Stack> { (x >= x0 && x <= x1 && y >= y0 && y <= y1).then(|| tiles[at(x, y)].and_then(|t| t.stack)).flatten() };
        for y in y0..=y1 {
            let runs = label_runs(w as usize, |i| one_plot(stack_at(x0 + i as i32, y), stack_at(x0 + i as i32 + 1, y), kind_merges));
            for (i, (n, k)) in runs.into_iter().enumerate() {
                let g = &mut geo[at(x0 + i as i32, y)];
                g.runs.nx = n;
                g.runs.kx = k;
            }
        }
        for x in x0..=x1 {
            let runs = label_runs(h as usize, |j| one_plot(stack_at(x, y0 + j as i32), stack_at(x, y0 + j as i32 + 1), kind_merges));
            for (j, (n, k)) in runs.into_iter().enumerate() {
                let g = &mut geo[at(x, y0 + j as i32)];
                g.runs.ny = n;
                g.runs.ky = k;
            }
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let i = at(x, y);
                let Some(st) = geo[i].stack else { continue };
                if st.levels == 0 {
                    continue;
                }
                let b = &assets.blocks[st.kind as usize % assets.blocks.len()];
                let runs = geo[i].runs;
                let first_seed = tiles[at(x - runs.kx as i32, y)].map(|t| t.seed).unwrap_or(0);
                let ridge_x = ridge_along_x(runs.nx, runs.ny, first_seed);
                let mut open = 0u8;
                let mut road = 0u8;
                for (face, dx, dy) in [(blocks::FACE_PX, 1, 0), (blocks::FACE_NX, -1, 0), (blocks::FACE_PY, 0, 1), (blocks::FACE_NY, 0, -1)] {
                    let n = stack_at(x + dx, y + dy);
                    if !merges(Some(st), n, kind_merges) {
                        open |= 1 << face;
                    }
                    if n.is_some_and(|n| assets.blocks[n.kind as usize % assets.blocks.len()].ground == Ground::Pave) {
                        road |= 1 << face;
                    }
                }
                let seed = tiles[i].map(|t| t.seed).unwrap_or(0);
                let g = &mut geo[i];
                g.runs.ridge_x = ridge_x;
                g.open = open;
                g.door = if b.door { door_face(open, road, seed) } else { NO_FACE };
                let profile = Profile { roof: b.roof, pitch: b.pitch, max_rise: b.max_rise };
                g.top = g.top.max(g.zs + profile.peak(g.runs));
            }
        }

        // Tree volumes, registered on every tile their footprint touches.
        // A grown species at the near zooms registers its model's branches
        // and leaf clusters instead of its stand-in, which is kept for the
        // shadow mask alone (docs/structures.md, "Volumes are stand-ins").
        let mut volumes: Vec<Volume> = Vec::new();
        let mut crowns: Vec<u32> = Vec::new();
        // The per-tile count, ceiling and volume span are gathered in one
        // small record apart from `geo`, so registering a hundred thousand
        // primitives touches twelve bytes a tile and not a whole row of it.
        let mut regs: Vec<(u32, f32, f32)> = geo.iter().map(|g| (0, g.top, 0.0)).collect();
        /// A volume the walk never meets: only the shadow mask sweeps it.
        const UNREGISTERED: (i32, i32, i32, i32) = (0, 0, -1, -1);
        let mut spans: Vec<(i32, i32, i32, i32)> = Vec::new();
        let lod = crate::raster::lod_of(cam.rows_per_metre());
        let detail = Detail::of(cam);
        cache.begin();
        if lod.volumes {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = at(x, y);
                    let Some(t) = tiles[i] else { continue };
                    let Some(flora) = t.tree else { continue };
                    let sp = flora.species(assets);
                    let (shape, dims) = sp.volume();
                    // The species' size, its class and variant, and this
                    // tree's own quarter either way.
                    let inst = instance(t.seed);
                    let kind = size_scale(sp.size_class) * variant_scale(flora.variant);
                    let s = kind * inst.height;
                    let sr = kind * inst.radius;
                    let seed = t.seed as u64;
                    let jx = (hash01(x as i64, y as i64, seed ^ 0x11) - 0.5) * 0.3;
                    let jy = (hash01(x as i64, y as i64, seed ^ 0x22) - 0.5) * 0.3;
                    let (cx, cy) = (x as f32 + 0.5 + jx, y as f32 + 0.5 + jy);
                    let ground = (t.hf + fields.detail(cx, cy)).max(SEA as f32);
                    // A tree whose crown cannot reach the screen, and whose
                    // shadow cannot either, is not worth a volume.
                    let (sx, sy) = cam.project(cx, cy, ground);
                    let crown = (dims.trunk + dims.height) * s * cam.rows_per_metre();
                    let wide = dims.radius * s / TILE_METRES * cam.a() + MARGIN;
                    if sx + wide < -MARGIN || sx - wide > sw as f32 + MARGIN || sy + MARGIN < 0.0 || sy - crown - MARGIN > sh as f32 {
                        continue;
                    }
                    let gust = world.gust(x as f32, y as f32, sc.t);
                    let shear = if gust > 0.0 {
                        let phase = (t.seed % 628) as f32 * 0.01;
                        let lean = 0.12 * gust * (sc.t * (0.8 + 1.2 * gust) + phase).sin();
                        (world.weather.wind_dir.cos() * lean, world.weather.wind_dir.sin() * lean)
                    } else {
                        (0.0, 0.0)
                    };
                    let dead = stands_dead(t.seed, sp.dead_chance);
                    let stand_in = Volume {
                        shape,
                        cx,
                        cy,
                        ground,
                        h0: ground + dims.trunk * s,
                        height: dims.height * s,
                        radius: dims.radius * sr / TILE_METRES,
                        trunk_radius: dims.trunk_radius * s / TILE_METRES,
                        run: (0.0, 0.0),
                        crown: (ground + dims.trunk * s, dims.height * s),
                        shear,
                        species: flora.species,
                        instance: inst,
                        dead,
                        mx: x,
                        my: y,
                    };
                    // The model, if this species grows one and the zoom is
                    // near enough to read it.
                    let model = if lod.model && sp.lsystem.is_some() {
                        let foliage = if dead { 0.0 } else { sp.foliage(t.temp as f32, world.season) };
                        let steps = (foliage.clamp(0.0, 1.0) * FOLIAGE_STEPS).round() as u8;
                        cache.model(flora.species, sp, t.seed, steps, dead, detail)
                    } else {
                        None
                    };
                    crowns.push(volumes.len() as u32);
                    let total = (dims.trunk + dims.height) * s;
                    match model {
                        Some(m) => {
                            volumes.push(stand_in);
                            spans.push(UNREGISTERED);
                            let at_ = Placement {
                                species: flora.species,
                                mx: x,
                                my: y,
                                cx,
                                cy,
                                ground,
                                spread: sr,
                                height: s,
                                crown: (stand_in.h0, stand_in.height),
                                // The stand-in shears over its crown; a
                                // grown tree leans over its whole height.
                                shear: (shear.0 / total.max(1e-3), shear.1 / total.max(1e-3)),
                                instance: inst,
                            };
                            m.place(&at_, &mut volumes);
                        }
                        None => volumes.push(stand_in),
                    }
                    for v in &volumes[spans.len()..] {
                        let (fx0, fy0, fx1, fy1) = v.footprint();
                        let m = if matches!(v.shape, Shape::Branch | Shape::Cluster) { PRIMITIVE_MARGIN } else { VOLUME_MARGIN };
                        let span = (((fx0 - m).floor() as i32).max(x0), ((fy0 - m).floor() as i32).max(y0), ((fx1 + m).floor() as i32).min(x1), ((fy1 + m).floor() as i32).min(y1));
                        let (vtop, vspan) = (v.top(), v.top() - v.h0);
                        for ty in span.1..=span.3 {
                            let row = (ty - y0) * w - x0;
                            for tx in span.0..=span.2 {
                                let j = (row + tx) as usize;
                                let r = &mut regs[j];
                                r.0 += 1;
                                r.1 = r.1.max(vtop);
                                r.2 = r.2.max(vspan);
                            }
                        }
                        spans.push(span);
                    }
                }
            }
        }
        cache.sweep();
        let mut vol_index = vec![Bucket { top: 0.0, h0: 0.0, cx: 0.0, cy: 0.0, r2: 0.0, v: 0, cluster: false }; regs.iter().map(|r| r.0).sum::<u32>() as usize];
        let mut start = 0u32;
        let mut starts = vec![0u32; n];
        for (i, g) in geo.iter_mut().enumerate() {
            let (count, top, vspan) = regs[i];
            g.vol = (start, count);
            g.top = top;
            g.vspan = vspan;
            starts[i] = start;
            start += count;
        }
        // Tallest first, so a walk can stop testing once the rest are below
        // its segment. A stand of grown trees is a hundred thousand
        // primitives, too many to sort per tile, so the volumes are
        // counting-sorted by their tops once and the tile lists are filled
        // in that order.
        let (order, slack) = top_order(&volumes);
        for vi in order {
            let span = spans[vi as usize];
            let v = &volumes[vi as usize];
            let (rx, ry, rr) = v.reach();
            let entry = Bucket { top: v.top(), h0: v.h0, cx: rx, cy: ry, r2: rr * rr, v: vi, cluster: v.shape == Shape::Cluster };
            for ty in span.1..=span.3 {
                let row = (ty - y0) * w - x0;
                for tx in span.0..=span.2 {
                    let j = (row + tx) as usize;
                    vol_index[starts[j] as usize] = entry;
                    starts[j] += 1;
                }
            }
        }

        // Coarse ceilings.
        let (bw, bh) = ((w + BLOCK - 1) / BLOCK, (h + BLOCK - 1) / BLOCK);
        let mut blocks = vec![0.0f32; (bw * bh) as usize];
        let mut max_top = 0.0f32;
        for y in 0..h {
            for x in 0..w {
                let t = geo[(y * w + x) as usize].top.min(TOP_CAP);
                let b = &mut blocks[((y / BLOCK) * bw + x / BLOCK) as usize];
                *b = b.max(t);
                max_top = max_top.max(t);
            }
        }
        for y in 0..h {
            for x in 0..w {
                geo[(y * w + x) as usize].ceiling = blocks[((y / BLOCK) * bw + x / BLOCK) as usize];
            }
        }
        HeightGrid { x0, y0, w, h, data, tiles, geo, volumes, crowns, vol_index, slack, blocks, bw, max_top, fields }
    }

    /// A tile's index into the grid's row-major arrays, if it is in the
    /// grid.
    #[inline]
    pub(crate) fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            None
        } else {
            Some(((y - self.y0) * self.w + (x - self.x0)) as usize)
        }
    }

    /// Tiles in the grid, on the map or not.
    pub(crate) fn len(&self) -> usize {
        (self.w * self.h) as usize
    }

    /// Bilinear sample of the smooth height between tile centres, the
    /// corners clamped to the grid.
    #[inline]
    pub(crate) fn sample(&self, xf: f32, yf: f32) -> f32 {
        let (gx, gy) = (xf - 0.5, yf - 0.5);
        let (ix, iy) = (ifloor(gx), ifloor(gy));
        let (fx, fy) = (gx - ix as f32, gy - iy as f32);
        let (cx0, cx1) = ((ix - self.x0).clamp(0, self.w - 1) as usize, (ix + 1 - self.x0).clamp(0, self.w - 1) as usize);
        let (cy0, cy1) = ((iy - self.y0).clamp(0, self.h - 1) as usize, (iy + 1 - self.y0).clamp(0, self.h - 1) as usize);
        let (r0, r1) = (&self.data[cy0 * self.w as usize..], &self.data[cy1 * self.w as usize..]);
        let (a, b, c, d) = (r0[cx0], r0[cx1], r1[cx0], r1[cx1]);
        let top = a + (b - a) * fx;
        let bot = c + (d - c) * fx;
        top + (bot - top) * fy
    }

    /// The tile at a position, if it is in the grid and on the map.
    #[inline]
    pub(crate) fn tile(&self, x: i32, y: i32) -> Option<&Tile> {
        self.index(x, y).and_then(|i| self.tiles[i].as_ref())
    }

    /// The geometry of a tile that is in the grid and on the map.
    #[inline]
    pub(crate) fn geo(&self, x: i32, y: i32) -> Option<&Geo> {
        let g = &self.geo[self.index(x, y)?];
        g.on_map.then_some(g)
    }

    /// A tile's whole volume list, ordered by top.
    #[inline]
    pub(crate) fn buckets(&self, g: &Geo) -> &[Bucket] {
        &self.vol_index[g.vol.0 as usize..(g.vol.0 + g.vol.1) as usize]
    }

    /// How far below a segment's foot the index must still be read, since
    /// the counting sort leaves it that far out of order.
    #[inline]
    pub(crate) fn slack(&self) -> f32 {
        self.slack
    }

    /// One volume by its index.
    #[inline]
    pub(crate) fn volume(&self, i: u32) -> &Volume {
        &self.volumes[i as usize]
    }

    /// Every volume registered on a tile, tallest first.
    #[inline]
    pub(crate) fn volumes_at(&self, g: &Geo) -> impl Iterator<Item = (u32, &Volume)> {
        self.vol_index[g.vol.0 as usize..(g.vol.0 + g.vol.1) as usize].iter().map(move |b| (b.v, &self.volumes[b.v as usize]))
    }

    /// The stand-in of every tree in the frame: one volume per tree,
    /// whether or not the walk draws that tree from a grown model.
    pub(crate) fn tree_crowns(&self) -> impl Iterator<Item = &Volume> {
        self.crowns.iter().map(move |&i| &self.volumes[i as usize])
    }

    /// The column of a tile with a stack of one level or more.
    pub(crate) fn column(&self, g: &Geo, mx: i32, my: i32, profile: Profile) -> Column {
        Column { mx, my, zs: g.zs, profile, runs: g.runs }
    }

    /// Coarse ceiling over the `BLOCK` x `BLOCK` tiles around a ground
    /// point: nothing in that block reaches higher.
    #[inline]
    pub(crate) fn block_top(&self, x: i32, y: i32) -> f32 {
        let bx = (x - self.x0).div_euclid(BLOCK).clamp(0, self.bw - 1);
        let bh = (self.h + BLOCK - 1) / BLOCK;
        let by = (y - self.y0).div_euclid(BLOCK).clamp(0, bh - 1);
        self.blocks[(by * self.bw + bx) as usize]
    }

    /// The lowest height at which a ray's path is still over the block
    /// holding `(x, y)`: below it the walk has moved on to another block, so
    /// that block's ceiling no longer covers what is skipped.
    #[inline]
    pub(crate) fn block_exit(&self, p0: (f32, f32), d: (f32, f32), x: i32, y: i32) -> f32 {
        let bx = (x - self.x0).div_euclid(BLOCK) * BLOCK + self.x0;
        let by = (y - self.y0).div_euclid(BLOCK) * BLOCK + self.y0;
        let mut lo = f32::MIN;
        for (v0, dv, m0) in [(p0.0, d.0, bx as f32), (p0.1, d.1, by as f32)] {
            if dv.abs() < 1e-9 {
                continue;
            }
            // Descending, the point moves against `d`, so a positive drift
            // leaves through the block's low edge: one division per axis.
            let edge = if dv > 0.0 { m0 } else { m0 + BLOCK as f32 };
            lo = lo.max((edge - v0) / dv);
        }
        lo
    }

    /// The heights whose ground point lies inside the grid, for a ray's
    /// path `p(z) = p0 + d z`; `None` when the path never enters it. The
    /// walk has nothing to meet outside this span, so a range hundreds of
    /// metres up costs nothing where it cannot be seen.
    pub(crate) fn z_span(&self, p0: (f32, f32), d: (f32, f32)) -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (f32::MIN, f32::MAX);
        let box_ = [(p0.0, d.0, self.x0 as f32, (self.x0 + self.w) as f32), (p0.1, d.1, self.y0 as f32, (self.y0 + self.h) as f32)];
        for (v0, dv, m0, m1) in box_ {
            if dv.abs() < 1e-9 {
                if v0 < m0 || v0 > m1 {
                    return None;
                }
                continue;
            }
            let (t0, t1) = ((m0 - v0) / dv, (m1 - v0) / dv);
            let (a, b) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
            lo = lo.max(a);
            hi = hi.min(b);
        }
        (lo <= hi).then_some((lo, hi))
    }

    /// The tile range the grid covers.
    pub(crate) fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.x0, self.y0, self.x0 + self.w - 1, self.y0 + self.h - 1)
    }

    /// Every tile with its geometry, row-major.
    pub(crate) fn cells(&self) -> impl Iterator<Item = (i32, i32, &Tile, &Geo)> {
        self.tiles.iter().zip(self.geo.iter()).enumerate().filter_map(move |(i, (t, g))| {
            let t = t.as_ref()?;
            Some((self.x0 + i as i32 % self.w, self.y0 + i as i32 / self.w, t, g))
        })
    }
}
