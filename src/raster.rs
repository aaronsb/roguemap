//! Terrain and geometry by inverse projection: the ray walk down the
//! continuous height field under each screen cell, testing the structure
//! columns and tree volumes in view on the way down (ADR-002), the surface
//! shading and texture, and sextant antialiasing along boundaries between
//! visibly different surfaces.

use crate::biome::{self, Form, MaterialRule};
use crate::blocks::{face_normal, Ground, Profile, Roof, NO_FACE};
use crate::camera::Camera;
use crate::canvas::Rgb;
use crate::grid::{Geo, HeightGrid, TOP_CAP};
use crate::map::{Terrain, Tile, FLOOR, SEA, TILE_METRES};
use crate::noise::{hash, iceil, ifloor, smoothstep};
use crate::palette::{surface_color, DIRT};
use crate::render::{GCell, Hit, HitKind, Renderer, Scene, FACE_LEFT, FACE_RIGHT, FACE_TOP, SKY_DEPTH};
use crate::volume::Part;

/// Detail octaves for a zoom: none at the overview, more up close. Keyed
/// off rows per metre, as the rest of the level of detail is (ADR-004).
pub(crate) fn detail_octaves(cam: &Camera) -> u32 {
    let rpm = cam.rows_per_metre();
    if rpm < 1.0 {
        0
    } else if rpm < 2.0 {
        1
    } else if rpm < 4.0 {
        2
    } else {
        3
    }
}

/// Surface kinds a top face can take besides water, for the per-tile colour
/// memo: `Terrain` less `Water`, indexed by the kind's discriminant less one.
pub(crate) const SURFACE_KINDS: usize = 5;

/// What the walk draws at a zoom (the level-of-detail table of ADR-002).
#[derive(Clone, Copy)]
pub(crate) struct Lod {
    /// Roof profiles; below this stacks are flat columns.
    profiles: bool,
    /// Trees as volumes; below this they are one-glyph billboards.
    pub(crate) volumes: bool,
    /// An L-system species draws from its grown model; below this it draws
    /// its habit's stand-in shape (docs/structures.md).
    pub(crate) model: bool,
    /// Window and door bands, roof glyphs and normal shading.
    bands: bool,
    /// Supersample the seams of crowns too; below this only the set's
    /// outline glyphs shape them.
    canopy_aa: bool,
    /// Cactus arms and the denser canopy glyphs.
    close: bool,
    bisections: u32,
}

/// The level of detail at a zoom, by rows per metre: 0.75 far, 1.5 mid,
/// 3 near, 6 close.
pub(crate) fn lod_of(rows_per_metre: f32) -> Lod {
    let rpm = rows_per_metre;
    Lod { profiles: rpm >= 1.5, volumes: rpm >= 1.5, model: rpm >= 1.5, bands: rpm >= 3.0, canopy_aa: rpm >= 6.0, close: rpm >= 6.0, bisections: if rpm >= 6.0 { 5 } else { 4 } }
}

fn lod(cam: &Camera) -> Lod {
    lod_of(cam.rows_per_metre())
}

/// Brightness of a tree cell on the seam behind a nearer tree
/// (`Renderer::outline_crowns`).
const CROWN_SEAM: f32 = 0.66;

/// The sun's ground direction, toward +x +y as the terrain shading has it.
const SUN_GROUND: (f32, f32) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);

/// Sun-facing factor of a normal, -1 away to 1 toward, from its ground
/// component.
fn sun_of(n: (f32, f32, f32)) -> f32 {
    let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt().max(1e-6);
    ((n.0 * SUN_GROUND.0 + n.1 * SUN_GROUND.1) / len).clamp(-1.0, 1.0)
}

/// Screen-horizontal component of a unit normal: positive points
/// screen-right.
fn screen_x_of(n: (f32, f32, f32), cam: &Camera) -> f32 {
    let (s, c) = cam.angle.sin_cos();
    let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt().max(1e-6);
    (n.0 * c - n.1 * s) / len
}

/// Grass lean field: a travelling wave whose speed and reach follow the wind.
fn grass_lean(x: f32, y: f32, t: f32, strength: f32) -> f32 {
    let speed = 0.3 + 2.5 * strength;
    let w = (t * speed + x * 0.09 + y * 0.18).sin() + 0.5 * (t * speed * 0.35 - x * 0.05 + y * 0.11).sin();
    w * (0.25 + 0.75 * strength)
}

/// Sextant glyph for a 2x3 bit pattern: bit `j * 2 + i` is column i, row j.
fn sextant(bits: u8) -> char {
    match bits {
        0 => ' ',
        63 => '█',
        21 => '▌',
        42 => '▐',
        b => {
            let mut i = b as u32 - 1;
            if b > 21 {
                i -= 1;
            }
            if b > 42 {
                i -= 1;
            }
            char::from_u32(0x1FB00 + i).unwrap_or('▒')
        }
    }
}

fn color_dist(a: Rgb, b: Rgb) -> i32 {
    (a.0 as i32 - b.0 as i32).abs() + (a.1 as i32 - b.1 as i32).abs() + (a.2 as i32 - b.2 as i32).abs()
}

/// Two-colour quantisation of a 2x3 supersample: the first sample and the
/// sample farthest from it, as a sextant glyph, its colour and the
/// background. `None` when the block is too uniform to be worth a glyph.
fn quantise(cols: &[Rgb; 6]) -> Option<(char, Rgb, Rgb)> {
    let a = cols[0];
    let b = *cols.iter().max_by_key(|c| color_dist(a, **c)).unwrap();
    if color_dist(a, b) < 24 {
        return None;
    }
    let mut bits = 0u8;
    for (n, c) in cols.iter().enumerate() {
        if color_dist(*c, a) <= color_dist(*c, b) {
            bits |= 1 << n;
        }
    }
    if bits == 0 || bits == 63 {
        return None;
    }
    Some((sextant(bits), a, b))
}

/// Hash of a ground point on a grid near cell resolution, so texture stays
/// put as the camera pans and thins consistently with zoom.
fn ground_hash(cam: &Camera, x: f32, y: f32, seed: u32) -> u64 {
    let qs = (2 * cam.hw) as f32;
    hash((x * qs).floor() as i64, (y * qs * 2.0).floor() as i64, seed as u64)
}

/// Texture glyph and its colour for a top-surface cell, or a blank cell in
/// the base colour.
fn texture(sc: &Scene, tile: &Tile, hit: &Hit, base: Rgb, sx: i32, sy: i32) -> (char, Rgb) {
    let (ts, pal, world, cam) = (sc.ts, &sc.pal, sc.world, sc.cam);
    let hv = ground_hash(cam, hit.x, hit.y, tile.seed);
    let r = (hv % 1000) as f32 / 1000.0;
    let snow = sc.snow_at(tile.temp);
    let b = tile.biome(sc.assets);
    let vig = biome::vigour(tile.temp as f32, world.season);
    let density_of = &sc.assets.surfaces.density;
    let ground_glyph = b.ground_glyph.lerp(pal.snow_glyph(), snow).lerp(base.scale(1.25), 1.0 - vig);
    let wind_strength = if cam.hw <= 2 { 0.0 } else { world.weather.wind };
    // Ground kinds: a paved tile carries the dirt pair, a tilled one the
    // dirt glyph in alternating rows.
    if let Some(st) = tile.stack {
        match st.kind(sc.assets).ground {
            Ground::Pave if r < sc.assets.surfaces.surface[DIRT].texture_density => return (ts.texture[DIRT][((hv >> 20) % 2) as usize], pal.surfaces[DIRT].glyph),
            Ground::Till if sy % 2 == 0 && r < 0.5 => return (ts.texture[DIRT][0], pal.surfaces[DIRT].glyph),
            Ground::Pave | Ground::Till => return (' ', base),
            _ => {}
        }
    }
    match tile.terrain {
        Terrain::Grass => {
            let density = (tile.grass as f32 * density_of.grass_per_level + density_of.grass_base) * (1.0 - snow);
            let (set, density) = if vig > 0.55 {
                (Some(&ts.cover[b.cover as usize]), density * (0.4 + 0.6 * vig))
            } else if vig > 0.15 {
                (None, density * 0.5 * (vig - 0.15) / 0.4 + 0.02)
            } else {
                (None, 0.0)
            };
            if r < density {
                let w = grass_lean(sx as f32, sy as f32, sc.t, wind_strength);
                let lean = if w < -0.35 {
                    0
                } else if w > 0.35 {
                    2
                } else {
                    1
                };
                let ch = match set {
                    Some(set) => set[lean],
                    None => ts.stubble[((hv >> 20) % 3) as usize],
                };
                return (ch, ground_glyph.scale(0.85 + ((hv >> 12) % 100) as f32 * 0.003));
            }
        }
        Terrain::Water => {
            let wave = sc.chop * smoothstep(6.0, 160.0, tile.body_size as f32);
            let phase = (sc.t * (0.6 + wave) + sx as f32 * 0.13 + sy as f32 * 0.37 + (hv >> 16) as f32 * 0.001).sin();
            let pond = tile.body_size < 60 && tile.z >= SEA - 4;
            if pond && vig > 0.3 && r < density_of.cattail + 0.06 * vig {
                let ch = ts.cattail[((hv >> 20) % 2) as usize];
                return (ch, Rgb(120, 140, 70).lerp(Rgb(150, 120, 60), 1.0 - vig));
            } else if r < 0.12 + 0.34 * wave {
                let ch = ts.water[if phase > 0.6 {
                    0
                } else if phase > 0.1 {
                    1
                } else if phase > -0.5 {
                    2
                } else {
                    3
                }];
                let crest = (0.15 + 0.85 * wave) * (0.55 + 0.45 * phase.max(0.0));
                return (ch, base.lerp(pal.water_glyph, crest));
            }
        }
        ground => {
            if let Some(i) = ground.surface() {
                if r < sc.assets.surfaces.surface[i].texture_density {
                    return (ts.texture[i][((hv >> 20) % 2) as usize], pal.surfaces[i].glyph);
                }
            }
        }
    }
    (' ', base)
}

/// Where in a tile's volume list the last segment of a ray started
/// reading. A ray walks down, so the list can only be entered later and
/// later while it stays over one tile: remembering the place turns a binary
/// search per sample into a step or two.
#[derive(Clone, Copy)]
struct Scan {
    mx: i32,
    my: i32,
    at: usize,
}

/// The best geometry crossing found along one segment of a ray.
struct Candidate {
    z: f32,
    kind: HitKind,
    normal: (f32, f32, f32),
    which: u32,
    mx: i32,
    my: i32,
}

impl Renderer {
    fn grid(&self) -> &HeightGrid {
        self.heights.as_ref().expect("height grid built for the frame")
    }

    /// Height of the drawn surface at a ground point: the continuous field,
    /// flattened to sea level over water and to the pad under a flattened
    /// stack.
    fn surface_height(&self, sc: &Scene, x: f32, y: f32) -> f32 {
        let grid = self.grid();
        if let Some(g) = grid.geo(ifloor(x), ifloor(y)) {
            if g.flat {
                return g.base;
            }
        }
        self.field_height(sc, x, y)
    }

    /// The continuous field alone, for gradients.
    fn field_height(&self, _sc: &Scene, x: f32, y: f32) -> f32 {
        let grid = self.grid();
        let h = grid.sample(x, y) + grid.fields.detail(x, y);
        h.max(SEA as f32)
    }

    /// The profile a stack's roof is drawn with at this zoom.
    fn profile(sc: &Scene, kind: u8, lod: Lod) -> Profile {
        let b = &sc.assets.blocks[kind as usize % sc.assets.blocks.len()];
        Profile { roof: if lod.profiles { b.roof } else { Roof::Flat }, pitch: b.pitch, max_rise: b.max_rise }
    }

    /// Test one segment of the ray, `z` in `[lo, hi]`, against the volumes
    /// registered on the sample's tile and the columns of the tiles the
    /// segment can cross, keeping the highest crossing.
    #[allow(clippy::too_many_arguments)]
    fn geometry(&self, sc: &Scene, p0: (f32, f32), d: (f32, f32), lo: f32, hi: f32, cur: (i32, i32, &Geo), prev: Option<(i32, i32, &Geo)>, lod: Lod, scan: &mut Scan) -> Option<Candidate> {
        let grid = self.grid();
        let mut best: Option<Candidate> = None;
        let mut consider = |c: Candidate| {
            if best.as_ref().is_none_or(|b| c.z > b.z) {
                best = Some(c);
            }
        };
        let (mx, my, geo) = cur;
        if lod.volumes && geo.vol.1 > 0 && lo <= geo.top {
            // The segment's ground path is a short stroke; anything whose
            // own ground circle misses it cannot be met. Testing that from
            // the tile's index alone keeps a stand of grown trees out of
            // the cache: only the few volumes that survive are read.
            let floor = lo - grid.slack();
            // The stroke as its midpoint and half its run, so a volume is
            // rejected on its distance from the stroke itself rather than
            // from a circle around it: at the mid zoom a step runs a whole
            // tile of ground, and a circle that wide would pass most of a
            // stand to the solver.
            let mid = (p0.0 + d.0 * (lo + hi) * 0.5, p0.1 + d.1 * (lo + hi) * 0.5);
            let hv = (d.0 * 0.5 * (hi - lo), d.1 * 0.5 * (hi - lo));
            let hh = hv.0 * hv.0 + hv.1 * hv.1;
            // The distance test multiplies by the reciprocal, which can put
            // an entry a few ulps either side of where dividing would; the
            // solver is exact, so widening the circle by more than that
            // hands it the same entries and a few it rejects.
            let inv_hh = if hh > 1e-12 { 1.0 / hh } else { 0.0 };
            const SLOP: f32 = 1.0 + 1e-5;
            let list = grid.buckets(geo);
            let cut = hi + geo.vspan + grid.slack();
            let mut from = if scan.mx == mx && scan.my == my { scan.at } else { list.partition_point(|b| b.top > cut) };
            while from < list.len() && list[from].top > cut {
                from += 1;
            }
            *scan = Scan { mx, my, at: from };
            for b in &list[from..] {
                if b.top < floor {
                    break; // the rest are lower still
                }
                if b.h0 > hi {
                    continue; // it stands entirely above the segment
                }
                let (dx, dy) = (b.cx - mid.0, b.cy - mid.1);
                let t = ((dx * hv.0 + dy * hv.1) * inv_hh).clamp(-1.0, 1.0);
                let (ex, ey) = (dx - t * hv.0, dy - t * hv.1);
                if ex * ex + ey * ey > b.r2 * SLOP {
                    continue;
                }
                // A leaf cluster is solved from the index entry alone: a
                // stand of grown trees is tens of thousands of them, and
                // reading each one the walk rejects is what a forest cannot
                // afford. The volume is read only for what was met.
                let h = if b.cluster { crate::volume::ellipsoid_hit((p0.0 - b.cx, p0.1 - b.cy), d, b.r2, b.h0, b.top - b.h0, lo.max(b.h0), hi.min(b.top)) } else { grid.volume(b.v).hit(p0, d, lo, hi, lod.close) };
                let Some(h) = h else { continue };
                let (vi, v) = (b.v, grid.volume(b.v));
                // A stand-in crown is not solid: a sample inside one meets
                // foliage with the species' leaf density as its probability
                // and otherwise passes through, so a thin crown shows flecks
                // of what stands behind it (docs/structures.md). A grown
                // crown's holes are the gaps its grammar left between the
                // clusters, so each cluster is solid.
                if h.part == Part::Canopy && !v.dead && !b.cluster {
                    let density = sc.assets.species[v.species as usize % sc.assets.species.len()].leaf_density;
                    if !crate::volume::foliage_at(p0.0 + d.0 * h.z, p0.1 + d.1 * h.z, h.z, density) {
                        continue;
                    }
                }
                let kind = if h.part == Part::Canopy { HitKind::Canopy } else { HitKind::Trunk };
                consider(Candidate { z: h.z, kind, normal: h.normal, which: vi, mx: v.mx, my: v.my });
            }
        }
        let mut column = |tx: i32, ty: i32, g: &Geo| {
            let Some(st) = g.stack else { return };
            if st.levels == 0 || lo > g.top {
                return;
            }
            let col = grid.column(g, tx, ty, Self::profile(sc, st.kind, lod));
            if let Some(h) = col.hit(p0, d, lo, hi, lod.bisections) {
                if h.face != NO_FACE {
                    // Walls run into the ground; below the surface the
                    // terrain is what shows.
                    let (x, y) = (p0.0 + d.0 * h.z, p0.1 + d.1 * h.z);
                    if h.z < self.surface_height(sc, x, y) {
                        return;
                    }
                }
                let kind = if h.face == NO_FACE { HitKind::Roof } else { HitKind::Wall };
                consider(Candidate { z: h.z, kind, normal: h.normal, which: h.face as u32, mx: tx, my: ty });
            }
        };
        column(mx, my, geo);
        if let Some((px, py, pg)) = prev {
            if (px, py) != (mx, my) {
                column(px, py, pg);
                // A diagonal step can clip the corner of a third tile.
                if px != mx && py != my {
                    for (cx, cy) in [(px, my), (mx, py)] {
                        if let Some(g) = grid.geo(cx, cy) {
                            column(cx, cy, g);
                        }
                    }
                }
            }
        }
        best
    }

    /// March down the surface under a screen position. Lowering the level
    /// moves the ground point away from the camera, so the first crossing
    /// found is the visible point. Each step tests the segment between two
    /// samples against the geometry in view, then the sample itself
    /// against the continuous field, so thin features cannot fall between
    /// samples; steep slopes shade as cliffs, with the face chosen by the
    /// gradient's direction.
    fn ray(&self, sc: &Scene, sx: f32, sy: f32) -> Option<Hit> {
        self.ray_from(sc, sx, sy, TOP_CAP)
    }

    /// The walk from no higher than `start`.
    fn ray_from(&self, sc: &Scene, sx: f32, sy: f32, start: f32) -> Option<Hit> {
        let cam = sc.cam;
        let grid = self.grid();
        let lod = lod(cam);
        let p0 = cam.unproject(sx, sy, 0.0);
        let (s, c) = cam.angle.sin_cos();
        let b = cam.b();
        // Tiles of ground the point moves per metre of height, so that the
        // screen position stays put: heights project through rows per metre.
        let rpm = cam.rows_per_metre();
        let d = (s * rpm / b, c * rpm / b);
        // Only the heights whose ground point lies in the grid can be met,
        // and outside them the walk would step through hundreds of metres of
        // air over a range it cannot see.
        let (zlo, zhi) = grid.z_span(p0, d)?;
        let top = grid.max_top.min(start).min(zhi);
        // Two screen rows per step, which is about a tile of ground at
        // every zoom: fine enough that a step cannot cross a terrace.
        let steps = (rpm / 2.0).ceil().max(1.0) as i32;
        let bottom = ((zlo.max(FLOOR)) * steps as f32).floor() as i32;
        let mut i = iceil(top * steps as f32);
        let mut z_prev = i as f32 / steps as f32;
        let mut prev: Option<(i32, i32, &Geo)> = None;
        let mut scan = Scan { mx: i32::MIN, my: i32::MIN, at: 0 };
        while i >= bottom {
            let zf = i as f32 / steps as f32;
            i -= 1;
            let (x, y) = (p0.0 + d.0 * zf, p0.1 + d.1 * zf);
            let (mx, my) = (ifloor(x), ifloor(y));
            // High over the coarse ceiling of the block it is crossing, the
            // walk has nothing to meet: drop to that ceiling, or to where the
            // path leaves the block, whichever comes first. A tile on the
            // map carries its block's ceiling; off the map the block grid
            // answers.
            let geo = grid.geo(mx, my);
            let ceiling = match geo {
                Some(g) => g.ceiling,
                None => grid.block_top(mx, my),
            };
            if zf > ceiling {
                let jump = ceiling.max(grid.block_exit(p0, d, mx, my));
                let next = iceil(jump * steps as f32);
                if next < i {
                    i = next;
                    prev = None;
                    z_prev = next as f32 / steps as f32;
                    continue;
                }
            }
            let Some(geo) = geo else {
                prev = None;
                z_prev = zf;
                continue;
            };
            if zf <= geo.top || prev.is_some_and(|(px, py, pg)| (px, py) != (mx, my) && zf <= pg.top) {
                if let Some(cand) = self.geometry(sc, p0, d, zf, z_prev, (mx, my, geo), prev, lod, &mut scan) {
                    return self.geometry_hit(sc, p0, d, cand);
                }
            }
            if zf <= geo.hmax {
                let h = self.surface_height(sc, x, y);
                if h >= zf {
                    let tile = *grid.tile(mx, my)?;
                    return Some(self.terrain_hit(sc, tile, mx, my, x, y, h, s, c));
                }
            }
            prev = Some((mx, my, geo));
            z_prev = zf;
        }
        None
    }

    /// A terrain crossing: gradient for shading and cliff faces.
    #[allow(clippy::too_many_arguments)]
    fn terrain_hit(&self, sc: &Scene, tile: Tile, mx: i32, my: i32, x: f32, y: f32, h: f32, s: f32, c: f32) -> Hit {
        let e = 0.25;
        // Metres of rise per metre of run, so the slope reads the same at
        // every zoom and the cliff threshold is an angle.
        let k = 1.0 / (2.0 * e * TILE_METRES);
        let gx = (self.field_height(sc, x + e, y) - self.field_height(sc, x - e, y)) * k;
        let gy = (self.field_height(sc, x, y + e) - self.field_height(sc, x, y - e)) * k;
        let slope = (gx * gx + gy * gy).sqrt();
        let sun = (-(gx + gy) * 0.5).clamp(-1.0, 1.0);
        /// Ground steeper than this is drawn as a cliff face: three metres
        /// of rise per two of run, steep enough that the sub-metre detail
        /// on flat ground never reaches it.
        const CLIFF: f32 = 1.5;
        let (face, below) = if slope < CLIFF {
            (FACE_TOP, 0)
        } else {
            let x_face = gx.abs() > gy.abs();
            let face = if (s * c > 0.0) == x_face { FACE_RIGHT } else { FACE_LEFT };
            (face, ((slope - CLIFF) * 1.5).ceil().clamp(1.0, 6.0) as i32)
        };
        Hit { tile, mx, my, x, y, h, face, below, sun, kind: HitKind::Terrain, which: 0, nsx: 0.0 }
    }

    /// A geometry crossing as a hit: walls take the screen side their face
    /// points to, everything else is a top.
    fn geometry_hit(&self, sc: &Scene, p0: (f32, f32), d: (f32, f32), cand: Candidate) -> Option<Hit> {
        let grid = self.grid();
        let tile = *grid.tile(cand.mx, cand.my)?;
        let (x, y) = (p0.0 + d.0 * cand.z, p0.1 + d.1 * cand.z);
        let face = if cand.kind == HitKind::Wall {
            if screen_x_of(cand.normal, sc.cam) > 0.0 {
                FACE_RIGHT
            } else {
                FACE_LEFT
            }
        } else {
            FACE_TOP
        };
        let sun = match cand.kind {
            HitKind::Wall => sun_of(cand.normal),
            HitKind::Roof => sun_of(cand.normal),
            HitKind::Canopy | HitKind::Trunk => {
                // Crowns take a little light from above as well.
                let n = cand.normal;
                let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt().max(1e-6);
                ((n.0 * 0.55 + n.1 * 0.55 + n.2 * 0.63) / len - 0.4).clamp(-1.0, 1.0)
            }
            HitKind::Terrain => 0.0,
        };
        let nsx = screen_x_of(cand.normal, sc.cam);
        Some(Hit { tile, mx: cand.mx, my: cand.my, x, y, h: cand.z, face, below: 0, sun, kind: cand.kind, which: cand.which, nsx })
    }

    /// Unlit colour of a top surface at a fractional ground point: the
    /// continuous fields decide water, sand, earth and rock through the tile,
    /// and the field's slope shades it toward or away from the sun.
    fn field_color(&self, sc: &Scene, hit: &Hit) -> Rgb {
        let h = hit.h;
        let grid = self.grid();
        let kind = sc.map.surface_kind(h, hit.tile.temp as f32, || self.beach(sc, hit.x, hit.y), || self.shore(sc, hit.x, hit.y), || grid.fields.patch(hit.x, hit.y));
        let mut t = hit.tile;
        t.terrain = kind;
        let mut c = if kind == Terrain::Water {
            t.z = (h.floor() as i32).min(SEA - 1);
            surface_color(&t, &sc.pal, sc.world, sc.assets)
        } else {
            // The tile's colour for this kind, worked out once per frame:
            // every sub-ray through the cell asks for it again.
            let slot = grid.index(hit.mx, hit.my).and_then(|i| self.colors.get(i * SURFACE_KINDS + kind as usize - 1));
            match slot.map(|s| s.get()) {
                Some(v) if v != u32::MAX => Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
                _ => {
                    let c = surface_color(&t, &sc.pal, sc.world, sc.assets);
                    if let Some(s) = slot {
                        s.set(((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32);
                    }
                    c
                }
            }
        };
        if let Some(st) = hit.tile.stack {
            let b = st.kind(sc.assets);
            if b.ground == Ground::Pave && kind != Terrain::Water {
                let snow = sc.snow_at(hit.tile.temp);
                c = self.material(sc, &hit.tile, st.kind).wall.lerp(sc.pal.snow(), snow);
            }
        }
        if kind != Terrain::Water {
            // Slope shading toward or away from the sun.
            c = c.scale(1.0 + 0.18 * hit.sun * sc.daylight);
        }
        c
    }

    /// Whether a ground point lies on or beside sand or water, as
    /// `Map::beach` answers it, from the frame's grid wherever the tiles are
    /// in it.
    fn beach(&self, sc: &Scene, xf: f32, yf: f32) -> bool {
        let (x, y) = (xf.floor() as i32, yf.floor() as i32);
        let sandy = |x: i32, y: i32| self.tile_at(sc, x, y).map(|t| matches!(t.terrain, Terrain::Sand | Terrain::Water)).unwrap_or(false);
        sandy(x, y) || [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| sandy(x + dx, y + dy))
    }

    /// Whether a ground point lies within a tile of water, as `Map::shore`
    /// answers it, from the frame's grid wherever the tiles are in it.
    fn shore(&self, sc: &Scene, xf: f32, yf: f32) -> bool {
        let (x, y) = (xf.floor() as i32, yf.floor() as i32);
        self.tile_at(sc, x, y).map(|t| t.near_water || t.terrain == Terrain::Water).unwrap_or(false)
            || [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| self.tile_at(sc, x + dx, y + dy).map(|t| t.terrain == Terrain::Water).unwrap_or(false))
    }

    /// A tree's own canopy colour: its species' colour, brightened or
    /// dimmed by a sixth and nudged toward yellow or blue, so a stand is
    /// not one flat green (ADR-002's volumes, ADR-004's scale).
    fn canopy_tint(&self, c: Rgb, v: &crate::volume::Volume) -> Rgb {
        let i = v.instance;
        let hue = 1.0 + 0.14 * i.hue;
        Rgb((c.0 as f32 * i.tint * hue).clamp(0.0, 255.0) as u8, (c.1 as f32 * i.tint).clamp(0.0, 255.0) as u8, (c.2 as f32 * i.tint / hue).clamp(0.0, 255.0) as u8)
    }

    /// How many other crowns stand over a canopy point, up to three: where
    /// crowns meet, the surface is deep in the canopy and lit less.
    fn crowns_over(&self, hit: &Hit) -> f32 {
        let grid = self.grid();
        let Some(g) = grid.geo(hit.mx, hit.my) else { return 0.0 };
        let mut n = 0.0;
        for (vi, v) in grid.volumes_at(g) {
            if vi == hit.which || v.top() < hit.h - 0.5 || !matches!(v.shape, crate::volume::Shape::Cone | crate::volume::Shape::Ellipsoid | crate::volume::Shape::Dome) {
                continue;
            }
            let (dx, dy) = (hit.x - v.cx, hit.y - v.cy);
            if dx * dx + dy * dy < v.radius * v.radius {
                n += 1.0;
                if n >= 3.0 {
                    break;
                }
            }
        }
        n
    }

    /// The material a stack on a tile is built of.
    fn material<'a>(&self, sc: &Scene<'a>, tile: &Tile, kind: u8) -> &'a crate::biome::Material {
        let b = &sc.assets.blocks[kind as usize % sc.assets.blocks.len()];
        match b.material {
            MaterialRule::Local => tile.material(sc.assets),
            MaterialRule::Fixed(i) => &sc.assets.materials[i % sc.assets.materials.len()],
        }
    }

    /// Unlit colour of a hit: the surface, a cliff face below it that keeps
    /// the surface tone for shallow steps and turns to earth deeper, or the
    /// geometry's own colour.
    fn hit_color(&self, sc: &Scene, hit: &Hit) -> Rgb {
        let (world, pal) = (sc.world, &sc.pal);
        let daylight = sc.daylight;
        let snow = sc.snow_at(hit.tile.temp);
        match hit.kind {
            HitKind::Terrain => {
                let surface = self.field_color(sc, hit);
                if hit.face == FACE_TOP {
                    return surface;
                }
                let earth = ((hit.below - 1) as f32 / 4.0).min(1.0);
                let depth_shade = 1.0 - (hit.below as f32 * 0.02).min(0.25);
                surface.scale(0.82).lerp(pal.dirt(), earth).scale(depth_shade)
            }
            HitKind::Wall => {
                let kind = hit.tile.stack.map(|s| s.kind).unwrap_or(0);
                self.material(sc, &hit.tile, kind).wall.scale(1.0 + 0.25 * hit.sun * daylight)
            }
            HitKind::Roof => {
                let kind = hit.tile.stack.map(|s| s.kind).unwrap_or(0);
                let b = &sc.assets.blocks[kind as usize % sc.assets.blocks.len()];
                let mat = self.material(sc, &hit.tile, kind);
                mat.roof.lerp(pal.snow(), snow * 0.8 * b.physical.snow_cover).scale(1.0 + 0.3 * hit.sun * daylight)
            }
            HitKind::Canopy => {
                let v = &self.grid().volumes[hit.which as usize];
                let sp = &sc.assets.species[v.species as usize % sc.assets.species.len()];
                let live = biome::seasonal(&sp.canopy, world.season).lerp(pal.snow(), snow * 0.7 * sp.physical.snow_cover);
                // A snag has no foliage: grey-brown wood where the crown was.
                let base = if v.dead { pal.trunk.lerp(Rgb(146, 138, 124), 0.45) } else { self.canopy_tint(live, v) };
                // The crown's own relief: the sun side lightens toward the
                // tip, and the deeper into a crown a point is the less light
                // reaches it. For a stand-in that depth is how many other
                // crowns stand over the point; a grown tree's own clusters
                // shade each other, so it takes the depth below its crown's
                // top instead.
                let up = ((hit.h - v.crown.0) / v.crown.1.max(1e-3)).clamp(0.0, 1.0);
                let crowded = if v.shape == crate::volume::Shape::Cluster { 1.0 - up } else { self.crowns_over(hit) };
                base.scale((1.0 + 0.45 * hit.sun * daylight) * (0.9 + 0.22 * up) * (1.0 - 0.13 * crowded))
            }
            HitKind::Trunk => {
                let v = &self.grid().volumes[hit.which as usize];
                let bark = if v.dead { pal.trunk.lerp(Rgb(150, 146, 138), 0.55).scale(0.85) } else { pal.trunk };
                // A branch at an angle is round in the light; a stand-in's
                // trunk is a column and keeps its flat colour.
                if v.shape == crate::volume::Shape::Branch {
                    bark.scale(1.0 + 0.3 * hit.sun * daylight)
                } else {
                    bark
                }
            }
        }
    }

    /// Terrain and geometry by inverse projection, with edge supersampling.
    pub(crate) fn terrain_pass(&mut self, sc: &Scene, aa: bool) {
        let hits = self.raycast(sc);
        self.shade_hits(sc, &hits);
        self.outline_crowns(sc, &hits);
        if aa {
            self.antialias_edges(sc, &hits);
        }
    }

    /// Where one tree stands in front of another, the cells of the tree
    /// behind along the seam are darkened, so every crown carries an
    /// outline against its neighbours and a stand reads as trees rather
    /// than as one canopy. Crown seams are never supersampled
    /// (`antialias_edges`), so this is what separates two crowns of one
    /// species in the same light.
    fn outline_crowns(&mut self, sc: &Scene, hits: &[Option<Hit>]) {
        let (w, h) = (self.w, self.h);
        let cam = sc.cam;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let Some(hit) = hits[i] else { continue };
                if !hit.kind.is_tree() {
                    continue;
                }
                let depth = cam.tile_depth(hit.mx, hit.my);
                let behind = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        return false;
                    }
                    hits[(ny * w + nx) as usize].is_some_and(|n| n.kind.is_tree() && (n.mx, n.my) != (hit.mx, hit.my) && cam.tile_depth(n.mx, n.my) > depth)
                });
                if behind {
                    let cell = &mut self.g[i];
                    cell.albedo = cell.albedo.scale(CROWN_SEAM);
                    cell.glyph = cell.glyph.scale(CROWN_SEAM);
                }
            }
        }
    }

    /// One ray per cell, recording each cell's surface identity for edge
    /// detection.
    fn raycast(&mut self, sc: &Scene) -> Vec<Option<Hit>> {
        let (w, h) = (self.w, self.h);
        let mut hits: Vec<Option<Hit>> = vec![None; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let hit = self.ray(sc, x as f32 + 0.5, y as f32 + 0.5);
                self.ids[i] = match &hit {
                    Some(hh) => ((hh.mx as u64) << 40) ^ ((hh.my as u64 & 0xFFFFF) << 8) ^ hh.face as u64 ^ (1 << 4) ^ ((hh.kind as u64) << 5),
                    None => 0,
                };
                hits[i] = hit;
            }
        }
        hits
    }

    /// Colour and texture every cell whose ray hit something.
    fn shade_hits(&mut self, sc: &Scene, hits: &[Option<Hit>]) {
        let (fx, fy) = sc.cam.forward();
        let (w, h) = (self.w, self.h);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let Some(hit) = hits[i] else { continue };
                let (albedo, ch, glyph) = self.shade(sc, &hit, x, y);
                let depth = hit.x * fx + hit.y * fy;
                self.g[i] = GCell { albedo, ch, glyph, wx: hit.x, wy: hit.y, wz: hit.h, face: hit.face, lit: true, depth };
            }
        }
    }

    /// Supersample cells on a boundary between visibly different surfaces
    /// and draw them as a sextant glyph in two colours. Crown seams are
    /// left to the outline glyphs at the middle zooms, where a forest
    /// would otherwise supersample most of the screen.
    fn antialias_edges(&mut self, sc: &Scene, hits: &[Option<Hit>]) {
        let (fx, fy) = sc.cam.forward();
        let (w, h) = (self.w, self.h);
        let lod = lod(sc.cam);
        let (canopy_aa, grown) = (lod.canopy_aa, lod.model);
        let is_crown = |id: u64| HitKind::from_id(id).is_some_and(HitKind::is_tree);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let id = self.ids[i];
                if !canopy_aa && is_crown(id) {
                    continue;
                }
                // Only boundaries between visibly different colours are worth
                // supersampling; seams inside a meadow are not. The sub-rays
                // need not start above what the cell and its neighbours met.
                let here = self.g[i].albedo;
                let mut start = hits[i].map(|hh| hh.h).unwrap_or(0.0);
                let mut edge = false;
                for &(dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let n = (ny * w + nx) as usize;
                    if let Some(hh) = hits[n] {
                        start = start.max(hh.h);
                    }
                    // Two crowns meeting is not a boundary worth six rays
                    // once trees are grown geometry: the branches and leaf
                    // clusters already cut the outline at cell resolution,
                    // and in a stand that seam is most of the screen. What
                    // is still worth it is the stand's edge against the sky,
                    // the ground or a wall.
                    let seam = is_crown(id) && is_crown(self.ids[n]);
                    if self.ids[n] != id && color_dist(self.g[n].albedo, here) >= 24 && (canopy_aa || !is_crown(self.ids[n])) && !(grown && seam) {
                        edge = true;
                    }
                }
                if !edge {
                    continue;
                }
                let Some((hh, cols)) = self.supersample(sc, x, y, start + 4.0) else { continue };
                // A cell whose centre missed but whose edge touches terrain
                // takes that terrain's position and lighting.
                if self.g[i].depth == SKY_DEPTH {
                    let albedo = self.hit_color(sc, &hh);
                    self.g[i] = GCell { albedo, ch: ' ', glyph: albedo, wx: hh.x, wy: hh.y, wz: hh.h, face: hh.face, lit: true, depth: hh.x * fx + hh.y * fy };
                }
                let Some((ch, glyph, albedo)) = quantise(&cols) else { continue };
                let cell = &mut self.g[i];
                cell.ch = ch;
                cell.glyph = glyph;
                cell.albedo = albedo;
            }
        }
    }

    /// Six unlit colours from a 2x3 grid of rays through one cell, plus the
    /// first hit; `None` when no ray met anything. The rays start no
    /// higher than `start`.
    fn supersample(&self, sc: &Scene, x: i32, y: i32, start: f32) -> Option<(Hit, [Rgb; 6])> {
        let mut cols = [Rgb(0, 0, 0); 6];
        let mut first: Option<Hit> = None;
        for j in 0..3 {
            for k in 0..2 {
                let sx = x as f32 + (k as f32 + 0.5) / 2.0;
                let sy = y as f32 + (j as f32 + 0.5) / 3.0;
                cols[j * 2 + k] = match self.ray_from(sc, sx, sy, start) {
                    Some(hh) => {
                        first.get_or_insert(hh);
                        self.hit_color(sc, &hh)
                    }
                    None => sc.world.sky(),
                };
            }
        }
        first.map(|hh| (hh, cols))
    }

    /// Unlit colour and texture glyph for a hit.
    fn shade(&self, sc: &Scene, hit: &Hit, sx: i32, sy: i32) -> (Rgb, char, Rgb) {
        let base = self.hit_color(sc, hit);
        let (ts, pal, cam) = (sc.ts, &sc.pal, sc.cam);
        let lod = lod(cam);
        match hit.kind {
            HitKind::Terrain => {
                if hit.face != FACE_TOP {
                    let ch = ts.wall[(hit.face - 1) as usize];
                    let glyph = base.scale(if hit.face == FACE_RIGHT { 1.18 } else { 0.8 });
                    return (base, ch, glyph);
                }
                // Texture follows the continuous surface kind, not the tile's.
                let mut tile = hit.tile;
                tile.terrain = sc.map.surface_at(hit.x, hit.y, hit.h, tile.temp as f32);
                let (ch, glyph) = texture(sc, &tile, hit, base, sx, sy);
                (base, ch, glyph)
            }
            HitKind::Wall => {
                let kind = hit.tile.stack.map(|s| s.kind).unwrap_or(0);
                let mat = self.material(sc, &hit.tile, kind);
                let mut ch = ts.wall[(hit.face - 1) as usize];
                let mut glyph = mat.wall_glyph;
                if lod.bands {
                    if let Some((band_ch, band_glyph)) = self.wall_band(sc, hit, kind) {
                        ch = band_ch;
                        glyph = band_glyph;
                    }
                }
                (base, ch, glyph)
            }
            HitKind::Roof => {
                let kind = hit.tile.stack.map(|s| s.kind).unwrap_or(0);
                let mat = self.material(sc, &hit.tile, kind);
                if lod.bands && ground_hash(cam, hit.x, hit.y, hit.tile.seed) % 100 < 35 {
                    return (base, ts.art.roof_fill, mat.roof_glyph);
                }
                (base, ' ', base)
            }
            HitKind::Canopy => {
                let v = &self.grid().volumes[hit.which as usize];
                let sp = &sc.assets.species[v.species as usize % sc.assets.species.len()];
                let snow = sc.world.snow_at(hit.tile.temp as f32);
                let live = biome::seasonal(&sp.canopy_glyph, sc.world.season).lerp(pal.snow_glyph(), snow * 0.5);
                let glyph = if v.dead { pal.trunk_glyph } else { self.canopy_tint(live, v) };
                let art = &ts.art;
                // The set's outline where the crown turns away sideways.
                if hit.nsx.abs() > 0.7 && sp.form != Form::Cactus {
                    let ch = match (sp.form, hit.nsx > 0.0) {
                        (Form::Pine, false) => art.pine_l,
                        (Form::Pine, true) => art.pine_r,
                        (_, false) => art.round_mid[0],
                        (_, true) => art.round_mid[2],
                    };
                    return (base, ch, glyph);
                }
                let hv = ground_hash(cam, hit.x, hit.y, hit.tile.seed);
                let density = if lod.close { 45 } else { 30 };
                if hv % 100 < density {
                    let ch = match sp.form {
                        Form::Pine => art.pine_fill[((hv >> 20) % 2) as usize],
                        Form::Cactus => art.cactus,
                        _ => art.round_mid[1],
                    };
                    return (base, ch, glyph);
                }
                (base, ' ', base)
            }
            HitKind::Trunk => (base, ts.art.trunk[0].chars().next().unwrap_or(' '), pal.trunk_glyph),
        }
    }

    /// Window and door glyphs on a wall at the close zooms: windows in the
    /// kind's band of each level at its pitch along the face, the door in
    /// the ground band of the door face.
    fn wall_band(&self, sc: &Scene, hit: &Hit, kind: u8) -> Option<(char, Rgb)> {
        let grid = self.grid();
        let g = grid.geo(hit.mx, hit.my)?;
        let b = &sc.assets.blocks[kind as usize % sc.assets.blocks.len()];
        let (ts, world, cam) = (sc.ts, sc.world, sc.cam);
        let face = hit.which as u8;
        let n = face_normal(face);
        // Position along the face within the tile and along the merged run.
        let (local, run) = if n.0 != 0.0 { (hit.y - hit.my as f32, g.runs.ky as f32) } else { (hit.x - hit.mx as f32, g.runs.kx as f32) };
        let along = run + local;
        let hz = (hit.h - g.base).max(0.0);
        let lh = b.level_height.max(1e-3);
        let level = (hz / lh).floor();
        let within = hz / lh - level;
        let mat = self.material(sc, &hit.tile, kind);
        if b.door && face == g.door && level < 0.5 && within < 0.5 {
            let half = if cam.hw >= 16 { 1.0 / cam.hw as f32 } else { 0.6 / cam.hw as f32 };
            if (local - 0.5).abs() < half {
                return Some((ts.art.door, mat.wall_glyph.scale(0.7)));
            }
        }
        if let Some([lo, hi]) = b.windows {
            let pitch = b.window_pitch.max(1e-3) / crate::map::TILE_METRES;
            let phase = (along / pitch).fract();
            // A window is about a cell and a half wide whatever the zoom.
            let half = (0.75 / cam.hw as f32).max(0.06);
            if within >= lo && within < hi && (phase - 0.5).abs() * pitch < half {
                let night = 1.0 - world.skylight();
                let lit = b.light.map(|i| sc.assets.lights[i].color).map(|c| Rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
                let color = match lit {
                    Some(c) if night > 0.05 => mat.wall_glyph.lerp(c, night),
                    _ => mat.wall_glyph.lerp(Rgb(40, 48, 70), 0.5),
                };
                return Some((ts.art.window, color));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::blocks::Stack;
    use crate::map::{Flora, Map};
    use crate::tileset::Tileset;
    use crate::world::World;
    use std::f32::consts::{FRAC_PI_4, PI};

    #[test]
    fn sextants_are_a_bijection_onto_the_block_plus_the_half_blocks() {
        assert_eq!(sextant(0), ' ');
        assert_eq!(sextant(63), '█');
        assert_eq!(sextant(21), '▌', "left column is the left half block");
        assert_eq!(sextant(42), '▐', "right column is the right half block");
        let mut seen: Vec<u32> = (1..63u8).filter(|&b| b != 21 && b != 42).map(|b| sextant(b) as u32).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, (0x1FB00..=0x1FB3B).collect::<Vec<u32>>(), "every other pattern is one sextant glyph");
    }

    #[test]
    fn quantising_six_identical_colours_yields_no_glyph() {
        let flat = [Rgb(10, 10, 10); 6];
        assert!(quantise(&flat).is_none());
        let nearly = [Rgb(10, 10, 10), Rgb(12, 10, 10), Rgb(10, 14, 10), Rgb(10, 10, 15), Rgb(11, 11, 11), Rgb(10, 10, 10)];
        assert!(quantise(&nearly).is_none(), "differences under the threshold are not worth a glyph");
    }

    #[test]
    fn two_colours_split_three_and_three_yield_the_expected_pattern() {
        let (a, b) = (Rgb(10, 10, 10), Rgb(200, 200, 200));
        // Top row and middle-left take the first colour: sextants 1, 2, 3.
        assert_eq!(quantise(&[a, a, a, b, b, b]), Some(('\u{1FB06}', a, b)));
        // The left column alone is the half block.
        assert_eq!(quantise(&[a, b, a, b, a, b]), Some(('▌', a, b)));
        // The first sample always keeps its colour as the glyph colour.
        assert_eq!(quantise(&[b, b, b, a, a, a]), Some(('\u{1FB06}', b, a)));
    }

    /// A renderer with the height grid built for the view, as `draw` does.
    fn prepared(sc: &Scene, w: i32, h: i32) -> Renderer {
        let mut r = Renderer::new(w, h);
        let (x0, y0, x1, y1) = r.visible_bounds(sc.cam, 40.0);
        r.heights = Some(HeightGrid::build(sc, x0, y0, x1, y1, w, h, &mut crate::grid::ModelCache::new()));
        r
    }

    /// The projected corners of the square `(x, y)..(x + n, y + n)` at
    /// height `z`, in ring order.
    fn square(cam: &Camera, x: f32, y: f32, n: f32, z: f32) -> [(f32, f32); 4] {
        [cam.project(x, y, z), cam.project(x + n, y, z), cam.project(x + n, y + n, z), cam.project(x, y + n, z)]
    }

    fn corner_by(corners: &[(f32, f32); 4], pick: fn(f32, f32) -> bool) -> usize {
        let mut best = 0;
        for i in 1..4 {
            if pick(corners[i].1, corners[best].1) {
                best = i;
            }
        }
        best
    }

    #[test]
    fn flat_field_rays_hit_the_top_at_the_field_height_under_every_cell() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let world = World::new(1);
        let mut map = Map::synthetic(8, 8, assets, 3, |_, _| Tile::flat(6));
        map.bounded = false;
        let (w, h) = (120, 40);
        // No sub-tile relief at the overview zoom: the surface is exactly flat.
        for (zoom, angle) in [(0, FRAC_PI_4), (0, 1.1), (0, 3.0), (0, 5.2)] {
            let mut cam = Camera::new();
            cam.angle = angle;
            cam.set_zoom(zoom, w, h);
            cam.look_at(0, 0, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let r = prepared(&sc, w, h);
            for y in 0..h {
                for x in 0..w {
                    let hit = r.ray(&sc, x as f32 + 0.5, y as f32 + 0.5).unwrap_or_else(|| panic!("zoom {zoom}: no hit at ({x}, {y})"));
                    assert_eq!((hit.face, hit.h.floor() as i32, hit.tile.z, hit.below), (FACE_TOP, 6, 6, 0), "zoom {zoom} cell ({x}, {y})");
                    assert!((hit.h - 6.5).abs() < 1e-4 && hit.sun.abs() < 1e-6, "zoom {zoom} cell ({x}, {y}): h {} sun {}", hit.h, hit.sun);
                    assert_eq!((hit.x.floor() as i32, hit.y.floor() as i32), (hit.mx, hit.my), "the ground point lies in the tile it hit");
                    assert_eq!(hit.kind, HitKind::Terrain);
                }
            }
        }
        // Up close the field carries relief under a metre, so every hit stays
        // within that of the tile height.
        let mut cam = Camera::new();
        cam.set_zoom(3, w, h);
        cam.look_at(0, 0, &map, w, h);
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        let r = prepared(&sc, w, h);
        for y in 0..h {
            for x in 0..w {
                let hit = r.ray(&sc, x as f32 + 0.5, y as f32 + 0.5).unwrap_or_else(|| panic!("close: no hit at ({x}, {y})"));
                assert!((hit.h - 6.5).abs() < 0.5 && hit.tile.z == 6, "close cell ({x}, {y}): h {}", hit.h);
            }
        }
    }

    #[test]
    fn front_slopes_of_a_raised_plateau_are_cliff_hits_on_the_predicted_side() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let world = World::new(1);
        // A 5x5 plateau seven metres over a plain: its sides fall from the
        // edge tiles to the neighbouring tile centres, which at a metre a
        // row and more is a cliff.
        let map = Map::synthetic(16, 16, assets, 0, |x, y| Tile::flat(if (3..=7).contains(&x) && (3..=7).contains(&y) { 10 } else { 3 }));
        let (w, h) = (120, 40);
        let (mid_x, mid_y, top_z) = (5.5, 5.5, 10.5);
        for angle in [FRAC_PI_4, FRAC_PI_4 + 0.3, 3.0 * FRAC_PI_4 - 0.2, PI + 0.4, 5.0 * FRAC_PI_4 + 0.1, 7.0 * FRAC_PI_4 - 0.25, 1.0, 5.6] {
            let mut cam = Camera::new();
            cam.angle = angle;
            cam.set_zoom(1, w, h);
            cam.look_at(5, 5, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let r = prepared(&sc, w, h);
            let centre = cam.project(mid_x, mid_y, top_z);
            let (s, c) = angle.sin_cos();
            // Half way down the middle of each side that faces the camera:
            // the sides are at +x and +y toward the camera's own direction,
            // and the slope falls seven metres over the tile beyond the
            // plateau's edge.
            let half = 0.45;
            let edge = 2.0; // tile centres: the middle of the plateau to its edge tile
            let sides = [((edge + half) * s.signum(), 0.0, s.abs()), (0.0, (edge + half) * c.signum(), c.abs())];
            let mut probes = 0;
            for (dx, dy, facing) in sides {
                if facing < 0.3 {
                    continue; // this side is nearly edge-on
                }
                let (px, py) = cam.project(mid_x + dx, mid_y + dy, top_z - half * 7.0);
                if (px - centre.0).abs() < 2.0 {
                    continue; // its slope points neither way on screen
                }
                let expected = if px > centre.0 { FACE_RIGHT } else { FACE_LEFT };
                let hit = r.ray(&sc, px, py).unwrap_or_else(|| panic!("angle {angle}: no hit on the plateau's side"));
                assert_eq!(hit.face, expected, "angle {angle}: the side at screen x {px:.1} from the middle at {:.1}", centre.0);
                assert!(hit.below >= 1 && hit.h > 3.0 && hit.h < top_z + 0.5, "angle {angle}: a cliff hit partway down (h {})", hit.h);
                probes += 1;
            }
            assert!(probes > 0, "angle {angle}: at least one side faces the camera");
            // Over the middle of the plateau the walk reaches its top.
            let top = r.ray(&sc, centre.0, centre.1 + 0.5).unwrap_or_else(|| panic!("angle {angle}: no hit over the plateau"));
            assert_eq!(top.face, FACE_TOP, "angle {angle}: the middle of the plateau is a top");
            assert!(top.h > 9.5 && top.h < top_z + 0.5, "angle {angle}: at the plateau's height (h {})", top.h);
        }
    }

    #[test]
    fn island_far_edge_is_sky_and_near_edge_is_plinth() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let world = World::new(1);
        let map = Map::synthetic(8, 8, assets, 0, |_, _| Tile::flat(5));
        let (w, h) = (120, 60);
        let plateau = 5.5;
        for angle in [FRAC_PI_4, FRAC_PI_4 + 0.4, 3.0 * FRAC_PI_4, 4.2] {
            let mut cam = Camera::new();
            cam.angle = angle;
            cam.set_zoom(1, w, h);
            cam.look_at(4, 4, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let r = prepared(&sc, w, h);
            // Above the far edge of the plateau there is nothing.
            let top = square(&cam, 0.0, 0.0, 8.0, plateau);
            let far = corner_by(&top, |a, b| a < b);
            for n in [(far + 1) % 4, (far + 3) % 4] {
                let (sx, sy) = ((top[far].0 + top[n].0) / 2.0, (top[far].1 + top[n].1) / 2.0 - 1.5);
                assert!(r.ray(&sc, sx, sy).is_none(), "angle {angle}: above the far edge is sky");
            }
            // The edge falls from the plateau to sea level as a cliff, and
            // below the waterline the plinth carries on down to the sea bed.
            let sea = square(&cam, 0.0, 0.0, 8.0, SEA as f32);
            let near = corner_by(&sea, |a, b| a > b);
            let rpm = cam.rows_per_metre();
            for n in [(near + 1) % 4, (near + 3) % 4] {
                let (sx, sy) = ((sea[near].0 + sea[n].0) / 2.0, (sea[near].1 + sea[n].1) / 2.0);
                // Down this column: the island's side is met as a cliff face
                // above the waterline, and below it the plinth is a wall on an
                // edge tile, until the walk runs out under the sea bed.
                let face = (0..(plateau * rpm).ceil() as i32).map(|i| r.ray(&sc, sx, sy - i as f32)).find(|hit| hit.is_some_and(|h| h.face != FACE_TOP)).flatten();
                let cliff = face.unwrap_or_else(|| panic!("angle {angle}: no cliff on the island's side"));
                assert!(cliff.h > SEA as f32 && cliff.h <= plateau + 0.5, "angle {angle}: the side is a cliff (h {})", cliff.h);
                let deep = (SEA as f32 - crate::map::FLOOR) * rpm;
                let hit = (1..deep as i32).map(|i| r.ray(&sc, sx, sy + i as f32)).find(|hit| hit.is_some_and(|h| h.face != FACE_TOP)).flatten().unwrap_or_else(|| panic!("angle {angle}: no plinth below the near edge"));
                assert!(hit.below >= 1 && hit.h > SEA as f32 && hit.h <= plateau + 0.5, "angle {angle}: the plinth is a wall hit (h {})", hit.h);
                let edge = |v: f32| !(0.5..=7.5).contains(&v);
                assert!(edge(hit.x) || edge(hit.y), "angle {angle}: on the island's edge ({:.2}, {:.2})", hit.x, hit.y);
                assert!(map.get(hit.mx, hit.my).is_some(), "angle {angle}: the plinth belongs to an island tile");
                // The plinth ends at the sea bed: that many metres below the
                // waterline, in rows.
                let below = (SEA as f32 - crate::map::FLOOR) * rpm + 4.0;
                assert!(r.ray(&sc, sx, sy + below).is_none(), "angle {angle}: below the plinth is sky again");
            }
        }
    }

    #[test]
    fn a_stack_is_met_as_roof_and_walls_above_the_ground_and_a_tree_as_a_crown_over_its_trunk() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let world = World::new(1);
        let house = assets.blocks.iter().position(|b| b.name == "house").unwrap();
        let pine = assets.species.iter().position(|s| s.name == "pine").unwrap();
        // The camera looks from +x +y: the tree at (2, 6) stands to one side
        // of the house at (5, 5), so neither hides the other and both are on
        // screen at every zoom.
        let map = Map::synthetic(16, 16, assets.clone(), 0, move |x, y| {
            let mut t = Tile::flat(5);
            if (x, y) == (5, 5) {
                t.stack = Some(Stack { kind: house as u8, levels: 2 });
            }
            if (x, y) == (2, 6) {
                t.tree = Some(Flora { species: pine as u8, variant: 2 });
                // A seed of zero rolls a standing snag; this pine is alive.
                t.seed = 7;
            }
            t
        });
        let (w, h) = (120, 40);
        let lh = assets.blocks[house].level_height;
        for zoom in [1usize, 2, 3] {
            let mut cam = Camera::new();
            cam.set_zoom(zoom, w, h);
            cam.look_at(4, 6, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let r = prepared(&sc, w, h);
            // Straight over the tile centre at the eaves height and above:
            // the roof, higher than the eaves and no higher than the peak.
            let (px, py) = cam.project(5.5, 5.5, 5.5 + 2.0 * lh);
            let roof = r.ray(&sc, px, py).unwrap_or_else(|| panic!("zoom {zoom}: no hit over the house"));
            assert_eq!(roof.kind, HitKind::Roof, "zoom {zoom}: {:?}", roof.kind);
            assert_eq!((roof.mx, roof.my), (5, 5));
            assert!(roof.h >= 5.5 + 2.0 * lh - 1e-3 && roof.h <= 5.5 + 2.0 * lh + assets.blocks[house].max_rise + 1e-3, "zoom {zoom}: roof at {}", roof.h);
            // The near corner of the column, a unit below the eaves, is a wall.
            let (fx, fy) = cam.forward();
            let (cx, cy) = (5.5 + 0.49 * fx.signum(), 5.5 + 0.49 * fy.signum());
            let (px, py) = cam.project(cx, cy, 5.5 + 2.0 * lh - 1.0);
            let wall = r.ray(&sc, px, py).unwrap_or_else(|| panic!("zoom {zoom}: no hit on the wall"));
            assert_eq!(wall.kind, HitKind::Wall, "zoom {zoom}: {:?} at h {}", wall.kind, wall.h);
            assert!(wall.face != FACE_TOP && wall.h > 5.5 && wall.h < 5.5 + 2.0 * lh, "zoom {zoom}: wall at {}", wall.h);
            // The tree: grown from its habit at every one of these zooms,
            // so what a ray meets is whichever of foliage and wood comes
            // first. Down the leader a metre and a half under the top it
            // is the tree; on the flank facing the camera half way up it
            // is the tree wherever the tiers leave no gap for the ray to
            // slip through; and four tiles off it is plain ground.
            let v = *r.grid().tree_crowns().find(|v| (v.mx, v.my) == (2, 6)).expect("the pine has a crown");
            let (fx, fy) = cam.forward();
            let (px, py) = cam.project(v.cx, v.cy, v.top() - 1.5);
            let leader = r.ray(&sc, px, py).unwrap_or_else(|| panic!("zoom {zoom}: no hit down the leader"));
            assert!(leader.kind.is_tree(), "zoom {zoom}: {:?}", leader.kind);
            assert!((leader.h - (v.top() - 1.5)).abs() < 2.0, "zoom {zoom}: the leader is met near the top ({} vs {})", leader.h, v.top());
            let half = v.h0 + 0.5 * v.height;
            let (px, py) = cam.project(v.cx + 0.5 * v.radius * fx, v.cy + 0.5 * v.radius * fy, half);
            if let Some(crown) = r.ray(&sc, px, py).filter(|h| h.kind.is_tree()) {
                assert!((crown.h - half).abs() < 1.5, "zoom {zoom}: the flank is met half way up ({} vs {})", crown.h, half);
            }
            let (px, py) = cam.project(v.cx + 4.0, v.cy - 1.0, 5.5);
            let ground = r.ray(&sc, px, py).unwrap();
            assert_eq!(ground.kind, HitKind::Terrain, "zoom {zoom}: four tiles from the trunk is ground");
        }
        // At the overview trees are billboards: no volumes are built.
        let mut cam = Camera::new();
        cam.set_zoom(0, w, h);
        cam.look_at(4, 6, &map, w, h);
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        let r = prepared(&sc, w, h);
        assert!(r.grid().volumes.is_empty());
        let (px, py) = cam.project(5.5, 5.5, 5.5 + 2.0 * lh);
        assert_eq!(r.ray(&sc, px, py).map(|h| h.kind), Some(HitKind::Roof), "the column still stands at the overview");
    }

    /// One flat map carrying a single tree of `name` on a tile of `temp`
    /// degrees, for the tests below.
    fn one_tree(assets: &std::rc::Rc<crate::assets::Assets>, name: &str, temp: i8) -> Map {
        let sp = assets.species.iter().position(|s| s.name == name).unwrap_or_else(|| panic!("a {name} in the set"));
        Map::synthetic(16, 16, assets.clone(), 0, move |x, y| {
            let mut t = Tile::flat(5);
            t.temp = temp;
            // A seed of zero rolls a standing snag, which has no foliage
            // whatever the season; this one is alive.
            t.seed = 7;
            if (x, y) == (8, 8) {
                t.tree = Some(Flora { species: sp as u8, variant: 2 });
            }
            t
        })
    }

    /// The grid over a tree at a zoom, with the season the world is in.
    fn tree_grid(map: &Map, ts: &Tileset, season: f32, zoom: usize) -> (Renderer, (i32, i32, i32, i32)) {
        let mut world = World::new(1);
        world.season = season;
        let (w, h) = (120, 40);
        let mut cam = Camera::new();
        cam.set_zoom(zoom, w, h);
        cam.look_at(8, 8, map, w, h);
        let sc = Scene::new(map, ts, &world, &cam, 0.0);
        let r = prepared(&sc, w, h);
        let b = r.grid().bounds();
        (r, b)
    }

    #[test]
    fn a_grown_species_buckets_its_branches_and_leaf_clusters_into_the_grid() {
        use crate::volume::Shape;
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let map = one_tree(&assets, "oak", 12);
        // At the mid zoom the oak is already its model, at the coarse
        // level: fewer primitives than the near zoom draws, and the
        // stand-in beside them for the shadow mask.
        let (r, _) = tree_grid(&map, ts, 1.0, 1);
        let mid = r.grid().volumes.len();
        assert!(mid > 10, "a grown oak at the mid zoom: {mid} volumes");
        assert_eq!(r.grid().volumes[r.grid().crowns[0] as usize].shape, Shape::Ellipsoid);
        // At the near zoom it is its model in more detail: branches and
        // leaf clusters, one stand-in kept for the shadow mask, and every
        // primitive bucketed into the tiles its footprint covers.
        let (r, (x0, y0, x1, y1)) = tree_grid(&map, ts, 1.0, 2);
        assert!(r.grid().volumes.len() > mid, "the near zoom keeps more of the model than the mid one: {} vs {mid}", r.grid().volumes.len());
        let grid = r.grid();
        let kinds = |k: Shape| grid.volumes.iter().filter(|v| v.shape == k).count();
        assert_eq!(grid.crowns.len(), 1, "one tree, one crown for the shadow mask");
        assert_eq!(grid.volumes[grid.crowns[0] as usize].shape, Shape::Ellipsoid);
        assert!(kinds(Shape::Branch) > 3, "the model's branches: {}", kinds(Shape::Branch));
        assert!(kinds(Shape::Cluster) > 10, "the model's leaf clusters: {}", kinds(Shape::Cluster));
        assert_eq!(kinds(Shape::Branch) + kinds(Shape::Cluster) + 1, grid.volumes.len());
        // Every bucket entry describes the volume it points at, and the
        // trunk's own tile carries both wood and foliage.
        let mut on_trunk = (0, 0);
        let mut entries = 0;
        for my in y0..=y1 {
            for mx in x0..=x1 {
                let Some(g) = grid.geo(mx, my) else { continue };
                for b in grid.buckets(g) {
                    let v = grid.volume(b.v);
                    assert_eq!((b.top, b.h0), (v.top(), v.h0));
                    entries += 1;
                    if (mx, my) == (8, 8) {
                        match v.shape {
                            Shape::Branch => on_trunk.0 += 1,
                            Shape::Cluster => on_trunk.1 += 1,
                            other => panic!("{other:?} bucketed with a grown tree"),
                        }
                    }
                }
            }
        }
        assert!(entries > grid.volumes.len(), "a primitive covers at least its own tile");
        assert!(on_trunk.0 > 0 && on_trunk.1 > 0, "the trunk's tile carries wood and foliage: {on_trunk:?}");
        // A tile well clear of the crown carries nothing.
        let far = grid.geo(8 - 8, 8).expect("a tile eight over");
        assert_eq!(far.vol.1, 0);
    }

    #[test]
    fn a_model_is_grown_from_the_mid_zoom_up_and_its_detail_follows_the_zoom() {
        use crate::grid::Detail;
        // Rows per metre at the four zooms: the overview draws billboards,
        // every other zoom the grown model.
        assert!(!lod_of(0.75).volumes && !lod_of(0.75).model);
        assert!(lod_of(1.5).volumes && lod_of(1.5).model);
        assert!(lod_of(3.0).model && lod_of(6.0).model);
        // The detail level counts the zooms a model is grown at, and the
        // lattice and the twig cut halve with each zoom in.
        let (mid, near, close) = (Detail::at(1.5, 2.83), Detail::at(3.0, 5.66), Detail::at(6.0, 11.31));
        assert_eq!((mid.level, near.level, close.level), (0, 1, 2));
        assert!((mid.cell / near.cell - 2.0).abs() < 1e-3 && (near.cell / close.cell - 2.0).abs() < 1e-3);
        assert!((mid.min_radius / near.min_radius - 2.0).abs() < 1e-2 && (near.min_radius / close.min_radius - 2.0).abs() < 1e-2);
        assert!(mid.cell > 2.0 && close.cell < 1.0, "about four rows of height: {} m at 1:4, {} m at 1:1", mid.cell, close.cell);
    }

    #[test]
    fn a_crown_behind_a_nearer_tree_is_darkened_along_the_seam() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let pine = assets.species.iter().position(|s| s.name == "pine").unwrap();
        // Two live pines four tiles apart on the camera's forward diagonal,
        // so the nearer one stands in front of the lower half of the other
        // on screen and the far crown shows over it.
        let map = Map::synthetic(16, 16, assets.clone(), 0, move |x, y| {
            let mut t = Tile::flat(5);
            t.seed = 7;
            if (x, y) == (8, 8) || (x, y) == (12, 12) {
                t.tree = Some(Flora { species: pine as u8, variant: 2 });
            }
            t
        });
        let world = World::new(1);
        let (w, h) = (120, 120);
        let mut cam = Camera::new();
        cam.set_zoom(1, w, h);
        cam.look_at(8, 8, &map, w, h);
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        let mut r = prepared(&sc, w, h);
        r.terrain_pass(&sc, false);
        let (near, far) = ((12, 12), (8, 8));
        assert!(cam.tile_depth(near.0, near.1) > cam.tile_depth(far.0, far.1));
        let hit_at = |x: i32, y: i32| r.ray(&sc, x as f32 + 0.5, y as f32 + 0.5).filter(|hh| hh.kind.is_tree()).map(|hh| ((hh.mx, hh.my), hh));
        let (mut seam, mut plain) = (0, 0);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let Some((tree, hh)) = hit_at(x, y) else { continue };
                if tree != far {
                    continue;
                }
                let beside: Vec<_> = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter_map(|&(dx, dy)| hit_at(x + dx, y + dy).map(|(t, _)| t)).collect();
                let base = r.hit_color(&sc, &hh);
                let cell = r.g[(y * w + x) as usize];
                if beside.contains(&near) {
                    seam += 1;
                    assert_eq!(cell.albedo, base.scale(CROWN_SEAM), "a far-tree cell beside the near tree is darkened at ({x}, {y})");
                } else if beside.len() == 4 && beside.iter().all(|t| *t == far) {
                    plain += 1;
                    assert_eq!(cell.albedo, base, "a cell inside the far crown keeps its colour at ({x}, {y})");
                }
            }
        }
        assert!(seam > 3 && plain > 3, "the seam runs through the frame: {seam} seam cells, {plain} inside");
    }

    #[test]
    fn a_bare_tree_leaves_only_branches_on_the_walk() {
        use crate::volume::Shape;
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        // A cold winter: an oak sheds, so its foliage is nothing.
        let map = one_tree(&assets, "oak", 0);
        let oak = assets.species.iter().find(|s| s.name == "oak").unwrap();
        assert!(oak.sheds);
        assert_eq!(oak.foliage(0.0, 3.0), 0.0, "an oak in a cold winter is bare");
        let (r, _) = tree_grid(&map, ts, 3.0, 2);
        let grid = r.grid();
        assert!(grid.volumes.iter().filter(|v| v.shape == Shape::Branch).count() > 10, "a bare tree keeps every twig");
        assert_eq!(grid.volumes.iter().filter(|v| v.shape == Shape::Cluster).count(), 0, "no foliage in winter");
        // In summer the same tree is in leaf, and an evergreen beside it
        // keeps its foliage whatever the season.
        let (r, _) = tree_grid(&map, ts, 1.0, 2);
        assert!(r.grid().volumes.iter().any(|v| v.shape == Shape::Cluster));
        let spruce = one_tree(&assets, "spruce", 0);
        let (r, _) = tree_grid(&spruce, ts, 3.0, 2);
        assert!(r.grid().volumes.iter().any(|v| v.shape == Shape::Cluster), "an evergreen keeps its crown in winter");
    }
}
