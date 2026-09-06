//! Terrain by inverse projection: the ray walk down each height column, the
//! surface shading and texture, and sextant antialiasing along boundaries
//! between visibly different surfaces.

use crate::biome;
use crate::camera::Camera;
use crate::canvas::Rgb;
use crate::map::{Map, Terrain, Tile, MAX_Z, SEA};
use crate::noise::{hash, smoothstep};
use crate::palette::surface_color;
use crate::render::{GCell, Hit, Renderer, Scene, FACE_LEFT, FACE_RIGHT, FACE_TOP, SKY_DEPTH};

/// Detail octaves for a zoom: none at the overview, more up close.
pub(crate) fn detail_octaves(cam: &Camera) -> u32 {
    match cam.hw {
        0..=3 => 0,
        4..=6 => 1,
        7..=12 => 2,
        _ => 3,
    }
}

/// Smooth heights of the tiles in view, for cheap bilinear sampling of the
/// continuous field between tile centres.
pub(crate) struct HeightGrid {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    data: Vec<f32>,
}

impl HeightGrid {
    pub(crate) fn build(map: &Map, x0: i32, y0: i32, x1: i32, y1: i32) -> HeightGrid {
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut data = Vec::with_capacity((w * h) as usize);
        for y in y0..=y1 {
            for x in x0..=x1 {
                data.push(map.get(x, y).map(|t| t.hf).unwrap_or(0.0));
            }
        }
        HeightGrid { x0, y0, w, h, data }
    }

    fn at(&self, x: i32, y: i32) -> f32 {
        let (cx, cy) = ((x - self.x0).clamp(0, self.w - 1), (y - self.y0).clamp(0, self.h - 1));
        self.data[(cy * self.w + cx) as usize]
    }

    /// Bilinear sample between tile centres.
    fn sample(&self, xf: f32, yf: f32) -> f32 {
        let (gx, gy) = (xf - 0.5, yf - 0.5);
        let (ix, iy) = (gx.floor(), gy.floor());
        let (fx, fy) = (gx - ix, gy - iy);
        let (ix, iy) = (ix as i32, iy as i32);
        let a = self.at(ix, iy);
        let b = self.at(ix + 1, iy);
        let c = self.at(ix, iy + 1);
        let d = self.at(ix + 1, iy + 1);
        let top = a + (b - a) * fx;
        let bot = c + (d - c) * fx;
        top + (bot - top) * fy
    }
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

/// Texture glyph and its colour for a top-surface cell, or a blank cell in
/// the base colour. Placement is keyed to a ground grid near cell
/// resolution, so it stays put as the camera pans and thins consistently
/// with zoom.
fn texture(sc: &Scene, tile: &Tile, hit: &Hit, base: Rgb, sx: i32, sy: i32) -> (char, Rgb) {
    let (ts, pal, world, cam) = (sc.ts, &sc.pal, sc.world, sc.cam);
    let qs = (2 * cam.hw) as f32;
    let hv = hash((hit.x * qs).floor() as i64, (hit.y * qs * 2.0).floor() as i64, tile.seed as u64);
    let r = (hv % 1000) as f32 / 1000.0;
    let snow = world.snow_at(tile.temp as f32);
    let b = tile.biome(sc.assets);
    let vig = biome::vigour(tile.temp as f32, world.season);
    let density_of = &sc.assets.surfaces.density;
    let ground_glyph = b.ground_glyph.lerp(pal.snow_glyph(), snow).lerp(base.scale(1.25), 1.0 - vig);
    let wind_strength = if cam.hw <= 2 { 0.0 } else { world.weather.wind };
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
                let lean = if w < -0.35 { 0 } else if w > 0.35 { 2 } else { 1 };
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
            let pond = tile.body_size < 60 && tile.z >= SEA - 1;
            if pond && vig > 0.3 && r < density_of.cattail + 0.06 * vig {
                let ch = ts.cattail[((hv >> 20) % 2) as usize];
                return (ch, Rgb(120, 140, 70).lerp(Rgb(150, 120, 60), 1.0 - vig));
            } else if r < 0.12 + 0.34 * wave {
                let ch = ts.water[if phase > 0.6 { 0 } else if phase > 0.1 { 1 } else if phase > -0.5 { 2 } else { 3 }];
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

impl Renderer {
    /// Height of the drawn surface at a ground point: the continuous field,
    /// flattened to sea level over water.
    fn surface_height(&self, sc: &Scene, x: f32, y: f32) -> f32 {
        let grid = self.heights.as_ref().expect("height grid built for the frame");
        let h = grid.sample(x, y) + sc.map.detail(x, y, detail_octaves(sc.cam));
        h.max(SEA as f32)
    }

    /// March down the surface under a screen position. Lowering the level
    /// moves the ground point away from the camera, so the first sample at
    /// or below the surface is the visible point. The surface is the
    /// continuous field, so there are no terraces; steep slopes shade as
    /// cliffs, with the face chosen by the gradient's direction.
    fn ray(&self, sc: &Scene, sx: f32, sy: f32) -> Option<Hit> {
        let (map, cam) = (sc.map, sc.cam);
        let (x0, y0) = cam.unproject(sx, sy, 0.0);
        let (x1, y1) = cam.unproject(sx, sy, MAX_Z as f32);
        let mut top = 0;
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            let (mx, my) = map.clamp(x.floor() as i32, y.floor() as i32);
            top = top.max(map.ceiling(mx, my));
        }
        let top = (top + 1).min(MAX_Z + 1);
        let steps = (2.0 / cam.b()).ceil().max(1.0) as i32;
        let (s, c) = cam.angle.sin_cos();
        let mut i = top * steps;
        while i >= 0 {
            let zf = i as f32 / steps as f32;
            i -= 1;
            let (x, y) = cam.unproject(sx, sy, zf);
            let (mx, my) = (x.floor() as i32, y.floor() as i32);
            let Some(tile) = map.get(mx, my) else { continue };
            let h = self.surface_height(sc, x, y);
            if h < zf {
                continue;
            }
            // Gradient of the surface for shading and cliff faces.
            let e = 0.25;
            let gx = (self.surface_height(sc, x + e, y) - self.surface_height(sc, x - e, y)) / (2.0 * e);
            let gy = (self.surface_height(sc, x, y + e) - self.surface_height(sc, x, y - e)) / (2.0 * e);
            let slope = (gx * gx + gy * gy).sqrt();
            let sun = (-(gx + gy) * 0.5).clamp(-1.0, 1.0);
            const CLIFF: f32 = 1.6;
            let (face, below) = if slope < CLIFF {
                (FACE_TOP, 0)
            } else {
                let x_face = gx.abs() > gy.abs();
                let face = if (s * c > 0.0) == x_face { FACE_RIGHT } else { FACE_LEFT };
                (face, ((slope - CLIFF) * 1.5).ceil().clamp(1.0, 6.0) as i32)
            };
            let z = zf.floor() as i32;
            return Some(Hit { tile, mx, my, x, y, h, z, face, below, sun });
        }
        None
    }

    /// Unlit colour of a top surface at a fractional ground point: the
    /// continuous fields decide water, sand, earth and rock through the tile,
    /// and the field's slope shades it toward or away from the sun.
    fn field_color(&self, sc: &Scene, hit: &Hit) -> Rgb {
        let grid = self.heights.as_ref().expect("height grid built for the frame");
        let h = grid.sample(hit.x, hit.y) + sc.map.detail(hit.x, hit.y, detail_octaves(sc.cam));
        let kind = sc.map.surface_at(hit.x, hit.y, h, hit.tile.temp as f32);
        let mut t = hit.tile;
        t.terrain = kind;
        if kind == Terrain::Water {
            t.z = (h.floor() as i32).min(SEA - 1);
        }
        let mut c = surface_color(&t, &sc.pal, sc.world, sc.assets);
        if kind != Terrain::Water {
            // Slope shading toward or away from the sun.
            c = c.scale(1.0 + 0.18 * hit.sun * sc.world.daylight());
        }
        c
    }

    /// Unlit colour of a hit: the surface, or a cliff face below it that
    /// keeps the surface tone for shallow steps and turns to earth deeper.
    fn hit_color(&self, sc: &Scene, hit: &Hit) -> Rgb {
        let surface = self.field_color(sc, hit);
        if hit.face == FACE_TOP {
            return surface;
        }
        let earth = ((hit.below - 1) as f32 / 4.0).min(1.0);
        let depth_shade = 1.0 - (hit.below as f32 * 0.02).min(0.25);
        surface.scale(0.82).lerp(sc.pal.dirt(), earth).scale(depth_shade)
    }

    /// Terrain by inverse projection, with edge supersampling.
    pub(crate) fn terrain_pass(&mut self, sc: &Scene, aa: bool) {
        let hits = self.raycast(sc);
        self.shade_hits(sc, &hits);
        if aa {
            self.antialias_edges(sc);
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
                    Some(hh) => ((hh.mx as u64) << 40) ^ ((hh.my as u64 & 0xFFFFF) << 8) ^ hh.face as u64 ^ 1 << 4,
                    None => 0,
                };
                hits[i] = hit;
            }
        }
        hits
    }

    /// Colour and texture every cell whose ray hit terrain.
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
    /// and draw them as a sextant glyph in two colours.
    fn antialias_edges(&mut self, sc: &Scene) {
        let (fx, fy) = sc.cam.forward();
        let (w, h) = (self.w, self.h);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let id = self.ids[i];
                // Only boundaries between visibly different colours are worth
                // supersampling; seams inside a meadow are not.
                let here = self.g[i].albedo;
                let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        return false;
                    }
                    let n = (ny * w + nx) as usize;
                    self.ids[n] != id && color_dist(self.g[n].albedo, here) >= 24
                });
                if !edge {
                    continue;
                }
                let Some((hh, cols)) = self.supersample(sc, x, y) else { continue };
                // A cell whose centre missed but whose edge touches terrain
                // takes that terrain's position and lighting.
                if self.g[i].depth == SKY_DEPTH {
                    let albedo = self.hit_color(sc, &hh);
                    self.g[i] = GCell { albedo, ch: ' ', glyph: albedo, wx: hh.x, wy: hh.y, wz: hh.z as f32, face: hh.face, lit: true, depth: hh.x * fx + hh.y * fy };
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
    /// first hit; `None` when no ray met terrain.
    fn supersample(&self, sc: &Scene, x: i32, y: i32) -> Option<(Hit, [Rgb; 6])> {
        let mut cols = [Rgb(0, 0, 0); 6];
        let mut first: Option<Hit> = None;
        for j in 0..3 {
            for k in 0..2 {
                let sx = x as f32 + (k as f32 + 0.5) / 2.0;
                let sy = y as f32 + (j as f32 + 0.5) / 3.0;
                cols[j * 2 + k] = match self.ray(sc, sx, sy) {
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
        if hit.face != FACE_TOP {
            let ch = sc.ts.wall[(hit.face - 1) as usize];
            let glyph = base.scale(if hit.face == FACE_RIGHT { 1.18 } else { 0.8 });
            return (base, ch, glyph);
        }
        // Texture follows the continuous surface kind, not the tile's.
        let mut tile = hit.tile;
        if let Some(grid) = &self.heights {
            let h = grid.sample(hit.x, hit.y) + sc.map.detail(hit.x, hit.y, detail_octaves(sc.cam));
            tile.terrain = sc.map.surface_at(hit.x, hit.y, h, tile.temp as f32);
        }
        let (ch, glyph) = texture(sc, &tile, hit, base, sx, sy);
        (base, ch, glyph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
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
    fn prepared(map: &Map, cam: &Camera, w: i32, h: i32) -> Renderer {
        let mut r = Renderer::new(w, h);
        let (x0, y0, x1, y1) = r.visible_bounds(cam);
        r.heights = Some(HeightGrid::build(map, x0, y0, x1, y1));
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
        // No sub-tile relief at the overview zooms: the surface is exactly flat.
        for (zoom, angle) in [(0, FRAC_PI_4), (1, 1.1), (0, 3.0), (1, 5.2)] {
            let mut cam = Camera::new();
            cam.angle = angle;
            cam.set_zoom(zoom, w, h);
            cam.look_at(0, 0, &map, w, h);
            let r = prepared(&map, &cam, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            for y in 0..h {
                for x in 0..w {
                    let hit = r.ray(&sc, x as f32 + 0.5, y as f32 + 0.5).unwrap_or_else(|| panic!("zoom {zoom}: no hit at ({x}, {y})"));
                    assert_eq!((hit.face, hit.z, hit.tile.z, hit.below), (FACE_TOP, 6, 6, 0), "zoom {zoom} cell ({x}, {y})");
                    assert!((hit.h - 6.5).abs() < 1e-4 && hit.sun.abs() < 1e-6, "zoom {zoom} cell ({x}, {y}): h {} sun {}", hit.h, hit.sun);
                    assert_eq!((hit.x.floor() as i32, hit.y.floor() as i32), (hit.mx, hit.my), "the ground point lies in the tile it hit");
                }
            }
        }
        // Up close the field carries relief under one height unit, so every
        // hit stays within that of the tile height.
        let mut cam = Camera::new();
        cam.set_zoom(4, w, h);
        cam.look_at(0, 0, &map, w, h);
        let r = prepared(&map, &cam, w, h);
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        for y in 0..h {
            for x in 0..w {
                let hit = r.ray(&sc, x as f32 + 0.5, y as f32 + 0.5).unwrap_or_else(|| panic!("zoom 4: no hit at ({x}, {y})"));
                assert!((hit.h - 6.5).abs() < 0.5 && hit.z == 6 && hit.tile.z == 6, "zoom 4 cell ({x}, {y}): h {} z {}", hit.h, hit.z);
            }
        }
    }

    #[test]
    fn front_slopes_of_a_raised_tile_are_cliff_hits_on_the_predicted_side() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let world = World::new(1);
        // One tile seven units above a plain: the field makes it a peak whose
        // slopes fall to the neighbouring tile centres.
        let map = Map::synthetic(16, 16, assets, 0, |x, y| Tile::flat(if (x, y) == (5, 5) { 10 } else { 3 }));
        let (w, h) = (120, 40);
        let (peak_x, peak_y, peak_z) = (5.5, 5.5, 10.5);
        for angle in [FRAC_PI_4, FRAC_PI_4 + 0.3, 3.0 * FRAC_PI_4 - 0.2, PI + 0.4, 5.0 * FRAC_PI_4 + 0.1, 7.0 * FRAC_PI_4 - 0.25, 1.0, 5.6] {
            let mut cam = Camera::new();
            cam.angle = angle;
            cam.set_zoom(6, w, h);
            cam.look_at(5, 5, &map, w, h);
            let r = prepared(&map, &cam, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let (s, c) = angle.sin_cos();
            let peak = cam.project(peak_x, peak_y, peak_z);
            // A point partway down each slope that faces the camera.
            let slopes = [(0.4 * s.signum(), 0.0, s.abs()), (0.0, 0.4 * c.signum(), c.abs())];
            for (dx, dy, facing) in slopes {
                if facing < 0.3 {
                    continue; // this slope is nearly edge-on
                }
                let (px, py) = cam.project(peak_x + dx, peak_y + dy, peak_z - 0.4 * 7.0);
                let expected = if px > peak.0 + 1.0 {
                    FACE_RIGHT
                } else if px < peak.0 - 1.0 {
                    FACE_LEFT
                } else {
                    continue;
                };
                let hit = r.ray(&sc, px, py).unwrap_or_else(|| panic!("angle {angle}: no hit on the slope"));
                assert_eq!((hit.mx, hit.my), (5, 5), "angle {angle}: the slope belongs to the raised tile");
                assert_eq!(hit.face, expected, "angle {angle}: slope toward screen x {px:.1} from the peak at {:.1}", peak.0);
                assert!(hit.below >= 1 && hit.h > 3.5 && hit.h < peak_z, "angle {angle}: a cliff hit partway down (h {})", hit.h);
            }
            // Just below the apex on the camera's side the walk reaches the
            // top of the raised tile. (At the apex pixel itself the height
            // steps of the walk can carry the ray over the point onto the
            // far slope, which is the walk's own sampling, not geometry.)
            let (px, py) = cam.project(peak_x + 0.2 * s, peak_y + 0.2 * c, peak_z - 7.0 * 0.2 * (s.abs() + c.abs()));
            let near_top = r.ray(&sc, px, py).unwrap_or_else(|| panic!("angle {angle}: no hit below the apex"));
            assert_eq!((near_top.mx, near_top.my), (5, 5), "angle {angle}: below the apex is the raised tile");
            assert!(near_top.h > 9.0 && near_top.h < peak_z + 0.5, "angle {angle}: near the top (h {})", near_top.h);
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
            cam.set_zoom(3, w, h);
            cam.look_at(4, 4, &map, w, h);
            let r = prepared(&map, &cam, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            // Above the far edge of the plateau there is nothing.
            let top = square(&cam, 0.0, 0.0, 8.0, plateau);
            let far = corner_by(&top, |a, b| a < b);
            for n in [(far + 1) % 4, (far + 3) % 4] {
                let (sx, sy) = ((top[far].0 + top[n].0) / 2.0, (top[far].1 + top[n].1) / 2.0 - 1.5);
                assert!(r.ray(&sc, sx, sy).is_none(), "angle {angle}: above the far edge is sky");
            }
            // The edge slopes from the plateau to sea level as a cliff, and
            // below the waterline the plinth carries on down to height zero.
            let sea = square(&cam, 0.0, 0.0, 8.0, SEA as f32);
            let near = corner_by(&sea, |a, b| a > b);
            for n in [(near + 1) % 4, (near + 3) % 4] {
                let (sx, sy) = ((sea[near].0 + sea[n].0) / 2.0, (sea[near].1 + sea[n].1) / 2.0);
                let cliff = r.ray(&sc, sx, sy - 1.0).unwrap_or_else(|| panic!("angle {angle}: no hit on the island's side"));
                assert!(cliff.face != FACE_TOP && cliff.h > SEA as f32 && cliff.h < plateau, "angle {angle}: the side is a cliff (h {})", cliff.h);
                let hit = r.ray(&sc, sx, sy + 1.5).unwrap_or_else(|| panic!("angle {angle}: no plinth below the near edge"));
                assert!(hit.face != FACE_TOP && hit.z < SEA && hit.below >= 1, "angle {angle}: the plinth is a wall hit below sea level (z {})", hit.z);
                assert!(map.get(hit.mx, hit.my).is_some(), "angle {angle}: the plinth belongs to an island tile");
                assert!(r.ray(&sc, sx, sy + 8.0).is_none(), "angle {angle}: below the plinth is sky again");
            }
        }
    }
}
