//! The per-frame grid the ray walk reads: every tile in view with its
//! smooth height, its stack resolved into a column with merged runs, and
//! the tree volumes whose footprint touches it. `Map::get` runs only while
//! the grid is built, so the walk itself never touches the chunk map.

use crate::blocks::{self, door_face, label_runs, merges, ridge_along_x, Column, Ground, Profile, Runs, Stack, NO_FACE};
use crate::map::{Tile, MAX_Z, SEA, TILE_METRES};
use crate::noise::{hash01, ifloor};
use crate::render::Scene;
use crate::volume::{size_scale, variant_scale, Volume};

/// Highest anything can reach above the ground: the tallest tree over the
/// highest terrain.
pub(crate) const TOP_CAP: f32 = MAX_Z as f32 + 40.0;
/// Tiles per block of the coarse ceiling grid.
const BLOCK: i32 = 16;
/// Ground margin around a volume's footprint so a segment between two
/// walk samples cannot cross it unregistered.
const VOLUME_MARGIN: f32 = 0.4;

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
    /// Range into the volume index.
    pub vol: (u32, u16),
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
    pub(crate) volumes: Vec<Volume>,
    vol_index: Vec<u32>,
    /// Coarse maxima of `top` per `BLOCK` x `BLOCK` tiles.
    blocks: Vec<f32>,
    bw: i32,
    /// Highest `top` anywhere in the grid.
    pub(crate) max_top: f32,
}

impl HeightGrid {
    /// Build the grid over the tile range for the frame.
    pub(crate) fn build(sc: &Scene, x0: i32, y0: i32, x1: i32, y1: i32) -> HeightGrid {
        let (map, assets, world, cam) = (sc.map, sc.assets, sc.world, sc.cam);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let n = (w * h) as usize;
        let mut data = Vec::with_capacity(n);
        let mut tiles = Vec::with_capacity(n);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let t = map.get(x, y);
                data.push(t.map(|t| t.hf).unwrap_or(0.0));
                tiles.push(t);
            }
        }
        let empty = Geo { stack: None, runs: Runs::SINGLE, open: 0, door: NO_FACE, base: 0.0, flat: false, zs: 0.0, top: 0.0, hmax: 0.0, vol: (0, 0) };
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
            let runs = label_runs(w as usize, |i| merges(stack_at(x0 + i as i32, y), stack_at(x0 + i as i32 + 1, y), kind_merges));
            for (i, (n, k)) in runs.into_iter().enumerate() {
                let g = &mut geo[at(x0 + i as i32, y)];
                g.runs.nx = n;
                g.runs.kx = k;
            }
        }
        for x in x0..=x1 {
            let runs = label_runs(h as usize, |j| merges(stack_at(x, y0 + j as i32), stack_at(x, y0 + j as i32 + 1), kind_merges));
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
        let mut volumes: Vec<Volume> = Vec::new();
        let mut counts = vec![0u32; n];
        let mut spans: Vec<(i32, i32, i32, i32)> = Vec::new();
        if cam.hw > 3 {
            let octaves = crate::raster::detail_octaves(cam);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = at(x, y);
                    let Some(t) = tiles[i] else { continue };
                    let Some(flora) = t.tree else { continue };
                    let sp = flora.species(assets);
                    let (shape, dims) = sp.volume();
                    let s = size_scale(sp.size_class) * variant_scale(flora.variant);
                    let seed = t.seed as u64;
                    let jx = (hash01(x as i64, y as i64, seed ^ 0x11) - 0.5) * 0.3;
                    let jy = (hash01(x as i64, y as i64, seed ^ 0x22) - 0.5) * 0.3;
                    let (cx, cy) = (x as f32 + 0.5 + jx, y as f32 + 0.5 + jy);
                    let ground = (t.hf + map.detail(cx, cy, octaves)).max(SEA as f32);
                    let gust = world.gust(x as f32, y as f32, sc.t);
                    let shear = if gust > 0.0 {
                        let phase = (t.seed % 628) as f32 * 0.01;
                        let lean = 0.12 * gust * (sc.t * (0.8 + 1.2 * gust) + phase).sin();
                        (world.weather.wind_dir.cos() * lean, world.weather.wind_dir.sin() * lean)
                    } else {
                        (0.0, 0.0)
                    };
                    let v = Volume {
                        shape,
                        cx,
                        cy,
                        ground,
                        h0: ground + dims.trunk * s,
                        height: dims.height * s,
                        radius: dims.radius * s / TILE_METRES,
                        trunk_radius: dims.trunk_radius * s / TILE_METRES,
                        shear,
                        species: flora.species,
                        mx: x,
                        my: y,
                    };
                    let (fx0, fy0, fx1, fy1) = v.footprint();
                    let span = (
                        ((fx0 - VOLUME_MARGIN).floor() as i32).max(x0),
                        ((fy0 - VOLUME_MARGIN).floor() as i32).max(y0),
                        ((fx1 + VOLUME_MARGIN).floor() as i32).min(x1),
                        ((fy1 + VOLUME_MARGIN).floor() as i32).min(y1),
                    );
                    for ty in span.1..=span.3 {
                        for tx in span.0..=span.2 {
                            let j = at(tx, ty);
                            counts[j] += 1;
                            geo[j].top = geo[j].top.max(v.top());
                        }
                    }
                    volumes.push(v);
                    spans.push(span);
                }
            }
        }
        let mut vol_index = vec![0u32; counts.iter().sum::<u32>() as usize];
        let mut start = 0u32;
        for (g, c) in geo.iter_mut().zip(counts.iter()) {
            g.vol = (start, *c as u16);
            start += c;
        }
        let mut fill = vec![0u32; n];
        for (vi, span) in spans.iter().enumerate() {
            for ty in span.1..=span.3 {
                for tx in span.0..=span.2 {
                    let j = at(tx, ty);
                    vol_index[(geo[j].vol.0 + fill[j]) as usize] = vi as u32;
                    fill[j] += 1;
                }
            }
        }
        // Tallest first, so a walk can stop testing once the rest are
        // below its segment.
        for g in &geo {
            let (s, l) = (g.vol.0 as usize, g.vol.1 as usize);
            if l > 1 {
                vol_index[s..s + l].sort_by(|a, b| volumes[*b as usize].top().partial_cmp(&volumes[*a as usize].top()).unwrap_or(std::cmp::Ordering::Equal));
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
        HeightGrid { x0, y0, w, h, data, tiles, geo, volumes, vol_index, blocks, bw, max_top }
    }

    #[inline]
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            None
        } else {
            Some(((y - self.y0) * self.w + (x - self.x0)) as usize)
        }
    }

    #[inline]
    fn at(&self, x: i32, y: i32) -> f32 {
        let (cx, cy) = ((x - self.x0).clamp(0, self.w - 1), (y - self.y0).clamp(0, self.h - 1));
        self.data[(cy * self.w + cx) as usize]
    }

    /// Bilinear sample of the smooth height between tile centres.
    pub(crate) fn sample(&self, xf: f32, yf: f32) -> f32 {
        let (gx, gy) = (xf - 0.5, yf - 0.5);
        let (ix, iy) = (ifloor(gx), ifloor(gy));
        let (fx, fy) = (gx - ix as f32, gy - iy as f32);
        let a = self.at(ix, iy);
        let b = self.at(ix + 1, iy);
        let c = self.at(ix, iy + 1);
        let d = self.at(ix + 1, iy + 1);
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
        let i = self.index(x, y)?;
        self.tiles[i].as_ref()?;
        Some(&self.geo[i])
    }

    /// The volumes registered on a tile, tallest first, with their indices.
    #[inline]
    pub(crate) fn volumes_at(&self, g: &Geo) -> impl Iterator<Item = (u32, &Volume)> {
        self.vol_index[g.vol.0 as usize..(g.vol.0 + g.vol.1 as u32) as usize].iter().map(move |&i| (i, &self.volumes[i as usize]))
    }

    /// The column of a tile with a stack of one level or more.
    pub(crate) fn column(&self, g: &Geo, mx: i32, my: i32, profile: Profile) -> Column {
        Column { mx, my, zs: g.zs, profile, runs: g.runs }
    }

    /// Where a ray may start: the coarse ceiling over the tiles its ground
    /// path can cross from `z = 0` up to the grid's highest top.
    pub(crate) fn top_for_ray(&self, x0: f32, y0: f32, d: (f32, f32)) -> f32 {
        let z1 = self.max_top;
        let (xa, ya) = (x0, y0);
        let (xb, yb) = (x0 + d.0 * z1, y0 + d.1 * z1);
        let bx = |x: f32| ((ifloor(x) - self.x0).div_euclid(BLOCK)).clamp(0, self.bw - 1);
        let by = |y: f32| ((ifloor(y) - self.y0).div_euclid(BLOCK)).clamp(0, (self.h + BLOCK - 1) / BLOCK - 1);
        let (bx0, bx1) = (bx(xa.min(xb)), bx(xa.max(xb)));
        let (by0, by1) = (by(ya.min(yb)), by(ya.max(yb)));
        let mut top = 0.0f32;
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                top = top.max(self.blocks[(y * self.bw + x) as usize]);
            }
        }
        top
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
