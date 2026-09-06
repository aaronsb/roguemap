//! Isometric rasteriser with a deferred lighting pass.
//!
//! Terrain is drawn by inverse projection: every screen cell walks down the
//! height column under it until it meets a tile top or a cliff face, so the
//! camera can sit at any angle. Cells on a boundary between two surfaces
//! are supersampled 2x3 and drawn with a sextant glyph and two colours.
//! Sprites are billboards drawn back to front with a depth test against the
//! terrain. A second pass lights every cell from ambient sky light, the sun
//! shadowed by drifting clouds, and point lights; overlays add precipitation
//! and the cloud layer.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use crate::biome::{self, BIOMES, MATERIALS, SPECIES};
use crate::canvas::{Canvas, Rgb};
use crate::map::{Map, Terrain, Tile, MAX_Z, SEA};
use crate::noise::{fbm, hash, smoothstep};
use crate::palette::{Palette, SEASON_NAMES};
use crate::settings::{Settings, AA, CLOUDS, ITEMS};
use crate::tileset::{Sprite, Tileset, ZOOMS};
use crate::world::{Light, World};

/// Background and glyph colours for a sprite's canopy and trunk parts.
struct SpriteColors {
    bg: Rgb,
    fg: Rgb,
    trunk_bg: Rgb,
    trunk_fg: Rgb,
}

const FACE_TOP: u8 = 0;
const FACE_RIGHT: u8 = 1;
const FACE_LEFT: u8 = 2;

#[derive(Clone, Copy)]
struct GCell {
    albedo: Rgb,
    ch: char,
    glyph: Rgb,
    wx: f32,
    wy: f32,
    wz: f32,
    face: u8,
    lit: bool,
    /// Distance toward the camera; larger is nearer.
    depth: f32,
}

const SKY_DEPTH: f32 = -1.0e9;

pub struct Camera {
    /// View angle in radians; pi/4 is the classic compass view.
    pub angle: f32,
    pub ox: f32,
    pub oy: f32,
    pub zoom: usize,
    pub hw: i32,
    pub hh: i32,
    /// Height of the point the screen centre was last aimed at, so zoom and
    /// rotation pivot about it.
    pub focus_z: f32,
}

impl Camera {
    pub fn new() -> Camera {
        Camera { angle: FRAC_PI_4, ox: 0.0, oy: 0.0, zoom: 0, hw: ZOOMS[0].0, hh: ZOOMS[0].1, focus_z: SEA as f32 }
    }

    /// Largest zoom at which the whole map fits the screen, else the smallest.
    pub fn fitting_zoom(map: &Map, sw: i32, sh: i32) -> usize {
        let n = map.w.max(map.h) as i32;
        ZOOMS
            .iter()
            .rposition(|&(hw, hh)| 2 * n * hw <= sw && 2 * n * hh + MAX_Z + 4 <= sh)
            .unwrap_or(0)
    }

    fn a(&self) -> f32 {
        self.hw as f32 * SQRT_2
    }

    pub fn b(&self) -> f32 {
        self.hh as f32 * SQRT_2
    }

    /// Screen position of a world point.
    pub fn project(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        (self.a() * (x * c - y * s) + self.ox, self.b() * (x * s + y * c) - z + self.oy)
    }

    /// Screen cell anchoring a tile: its centre projected and floored.
    pub fn project_tile(&self, mx: i32, my: i32, z: i32) -> (i32, i32) {
        let (sx, sy) = self.project(mx as f32 + 0.5, my as f32 + 0.5, z as f32);
        (sx.floor() as i32, sy.floor() as i32)
    }

    /// World point at height `z` under a screen position.
    pub fn unproject(&self, sx: f32, sy: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        let u = (sx - self.ox) / self.a();
        let v = (sy - self.oy + z) / self.b();
        (u * c + v * s, -u * s + v * c)
    }

    /// Ground point under a screen position.
    pub fn unproject_map(&self, sx: f32, sy: f32, _map: &Map) -> (f32, f32) {
        self.unproject(sx, sy, 0.0)
    }

    /// Unit vector pointing toward the camera in map space.
    pub fn forward(&self) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        (s, c)
    }

    /// Virtual camera height in height units for parallax; higher when
    /// zoomed out.
    pub fn height(&self) -> f32 {
        match self.hw {
            0..=2 => 48.0,
            3 => 64.0,
            _ => 120.0,
        }
    }

    /// Map tile nearest the centre of the screen at sea level.
    pub fn center_tile(&self, map: &Map, sw: i32, sh: i32) -> (i32, i32) {
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, SEA as f32);
        let (mut mx, mut my) = (x.floor() as i32, y.floor() as i32);
        if map.bounded {
            mx = mx.clamp(0, map.w as i32 - 1);
            my = my.clamp(0, map.h as i32 - 1);
        }
        (mx, my)
    }

    /// Map step for a screen direction: the inverse projection of the
    /// direction, scaled so its larger component is one tile, rounded.
    pub fn screen_dir_to_map(&self, dx: i32, dy: i32) -> (i32, i32) {
        let (s, c) = self.angle.sin_cos();
        let u = dx as f32 / self.a();
        let v = dy as f32 / self.b();
        let (x, y) = (u * c + v * s, -u * s + v * c);
        let m = x.abs().max(y.abs()).max(1e-6);
        ((x / m).round() as i32, (y / m).round() as i32)
    }

    /// Place a world point at the centre of the screen.
    pub fn look_at_point(&mut self, x: f32, y: f32, z: f32, sw: i32, sh: i32) {
        self.ox = 0.0;
        self.oy = 0.0;
        self.focus_z = z;
        let (sx, sy) = self.project(x, y, z);
        self.ox = (sw as f32 / 2.0 - sx).round();
        self.oy = (sh as f32 / 2.0 - sy).round();
    }

    /// Place map tile `(mx, my)` at the centre of the screen.
    pub fn look_at(&mut self, mx: i32, my: i32, map: &Map, sw: i32, sh: i32) {
        let z = map.get(mx, my).map(|t| t.draw_z()).unwrap_or(SEA);
        self.look_at_point(mx as f32 + 0.5, my as f32 + 0.5, z as f32, sw, sh);
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    pub fn set_zoom(&mut self, zoom: usize, map: &Map, sw: i32, sh: i32) {
        let z = self.focus_z;
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, z);
        self.zoom = zoom % ZOOMS.len();
        self.hw = ZOOMS[self.zoom].0;
        self.hh = ZOOMS[self.zoom].1;
        let _ = map;
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Turn by an angle about whatever is at the screen centre.
    pub fn rotate_by(&mut self, radians: f32, sw: i32, sh: i32) {
        let z = self.focus_z;
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, z);
        self.angle = (self.angle + radians).rem_euclid(2.0 * std::f32::consts::PI);
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Rotate by quarter turns.
    pub fn rotate(&mut self, steps: i32, _map: &Map, sw: i32, sh: i32) {
        self.rotate_by(steps as f32 * FRAC_PI_2, sw, sh);
    }

    pub fn degrees(&self) -> i32 {
        (self.angle.to_degrees().round() as i32).rem_euclid(360)
    }
}

/// What a screen cell's ray met in the height column.
#[derive(Clone, Copy)]
struct Hit {
    tile: Tile,
    mx: i32,
    my: i32,
    x: f32,
    y: f32,
    z: i32,
    face: u8,
    /// Rows below the top edge for a wall hit.
    below: i32,
}

pub struct Renderer {
    w: i32,
    h: i32,
    g: Vec<GCell>,
    /// Per-cell surface identity from the centre sample, for edge detection.
    ids: Vec<u64>,
    /// Lights discovered while drawing this frame, such as lit windows.
    frame_lights: Vec<Light>,
    pub show_hud: bool,
}

/// Grass lean field: a travelling wave whose speed and reach follow the wind.
fn wind(x: f32, y: f32, t: f32, strength: f32) -> f32 {
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

impl Renderer {
    pub fn new(w: i32, h: i32) -> Renderer {
        let sky = GCell { albedo: Rgb(0, 0, 0), ch: ' ', glyph: Rgb(0, 0, 0), wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
        Renderer { w, h, g: vec![sky; (w * h) as usize], ids: vec![0; (w * h) as usize], frame_lights: Vec::new(), show_hud: true }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        *self = Renderer { show_hud: self.show_hud, ..Renderer::new(w, h) };
    }

    #[inline]
    fn cell(&mut self, x: i32, y: i32) -> Option<&mut GCell> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some(&mut self.g[(y * self.w + x) as usize])
        }
    }

    /// Draw one frame into `cv`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, cv: &mut Canvas, map: &Map, ts: &Tileset, world: &World, cam: &Camera, settings: &Settings, t: f32) {
        let pal = Palette::for_season(world.season);
        let chop = world.choppiness();
        self.frame_lights.clear();
        self.sky_pass(ts, world, t);
        self.terrain_pass(map, ts, &pal, world, cam, t, chop, settings.get(AA) == 0 && ts.antialias);
        self.sprite_pass(map, ts, &pal, world, cam, t);
        self.fires(map, world, cam, ts, t);
        self.light_pass(cv, world, t);
        // Precipitation falls beneath the cloud layer, so it is drawn first.
        self.weather_pass(cv, ts, world, map, cam, t);
        if settings.get(CLOUDS) == 0 {
            self.cloud_pass(cv, ts, world, map, cam);
        }
        if self.show_hud {
            self.hud(cv, ts, world, cam, map);
        }
        if settings.open {
            self.popover(cv, settings);
        }
    }

    /// Walk down the height column under a screen position. Lowering the
    /// level moves the ground point away from the camera, so a column is
    /// entered through its front face. Levels are subdivided so the ground
    /// point never moves more than half a tile between samples.
    fn ray(map: &Map, cam: &Camera, sx: f32, sy: f32) -> Option<Hit> {
        // Nothing stands above the ceiling of the chunks the walk can cross;
        // the segment is shorter than a chunk, so its bounding corners cover
        // every chunk it touches.
        let (x0, y0) = cam.unproject(sx, sy, 0.0);
        let (x1, y1) = cam.unproject(sx, sy, MAX_Z as f32);
        let clamp = |x: f32, y: f32| -> (i32, i32) {
            let (mut mx, mut my) = (x.floor() as i32, y.floor() as i32);
            if map.bounded {
                mx = mx.clamp(0, map.w as i32 - 1);
                my = my.clamp(0, map.h as i32 - 1);
            }
            (mx, my)
        };
        let mut top = 0;
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            let (mx, my) = clamp(x, y);
            top = top.max(map.ceiling(mx, my));
        }
        let top = top.min(MAX_Z);
        let steps = (2.0 / cam.b()).ceil().max(1.0) as i32;
        let (s, c) = cam.angle.sin_cos();
        let mut i = top * steps;
        while i >= 0 {
            let zf = i as f32 / steps as f32;
            i -= 1;
            let (x, y) = cam.unproject(sx, sy, zf);
            let (mx, my) = (x.floor() as i32, y.floor() as i32);
            let Some(tile) = map.get(mx, my) else { continue };
            let dz = tile.draw_z();
            let dzf = dz as f32;
            if dzf < zf {
                continue;
            }
            let z = zf.floor() as i32;
            if (dzf - zf).abs() < 1e-4 {
                return Some(Hit { tile, mx, my, x, y, z, face: FACE_TOP, below: 0 });
            }
            // Entered through a side: the slab crossed last along the ray is
            // the face, then the axis face is mapped to the screen side it
            // shows on for this angle.
            let (px, py) = cam.unproject(sx, sy, dzf);
            let tx = ((px - (mx as f32 + 0.5)).abs() - 0.5) / s.abs().max(1e-6);
            let ty = ((py - (my as f32 + 0.5)).abs() - 0.5) / c.abs().max(1e-6);
            let x_face = tx > ty;
            let face = if (s * c > 0.0) == x_face { FACE_RIGHT } else { FACE_LEFT };
            let below = (dzf - zf).ceil().max(1.0) as i32;
            return Some(Hit { tile, mx, my, x, y, z, face, below });
        }
        None
    }

    /// Unlit surface colour: water by depth, ground by biome and season, and
    /// a snow blend wherever the seasonal temperature is below freezing.
    fn base_color(tile: &Tile, pal: &Palette, world: &World) -> Rgb {
        let season = world.season;
        let lift = (0.86 + tile.draw_z() as f32 * 0.018) * (1.0 - 0.28 * world.wet_at(tile.temp as f32));
        let b = &BIOMES[tile.biome as usize % BIOMES.len()];
        let c = match tile.terrain {
            Terrain::Water => {
                let depth = ((SEA - tile.z) as f32 / 3.0).clamp(0.0, 1.0);
                return pal.water_shallow.lerp(pal.water_deep, depth);
            }
            Terrain::Sand => pal.sand.scale(lift),
            Terrain::Grass => {
                // Seasonal biomes go dormant as vigour drops.
                let g = biome::ground_color(b, season);
                let dormant = Rgb(142, 126, 92);
                let k = if b.seasonal { (1.0 - biome::vigour(tile.temp as f32, season)) * 0.85 } else { 0.0 };
                g.lerp(dormant, k).scale(lift)
            }
            Terrain::Dirt => pal.dirt.scale(lift),
            Terrain::Rock => pal.rock.scale(lift),
            Terrain::Snow => pal.snow,
        };
        c.lerp(pal.snow, world.snow_at(tile.temp as f32))
    }

    /// Unlit colour of a hit: the surface, or a cliff face below it that
    /// keeps the surface tone for shallow steps and turns to earth deeper.
    fn hit_color(hit: &Hit, pal: &Palette, world: &World) -> Rgb {
        let surface = Self::base_color(&hit.tile, pal, world);
        if hit.face == FACE_TOP {
            return surface;
        }
        let earth = ((hit.below - 1) as f32 / 4.0).min(1.0);
        let depth_shade = 1.0 - (hit.below as f32 * 0.02).min(0.25);
        surface.scale(0.82).lerp(pal.dirt, earth).scale(depth_shade)
    }

    /// Terrain by inverse projection, with edge supersampling.
    #[allow(clippy::too_many_arguments)]
    fn terrain_pass(&mut self, map: &Map, ts: &Tileset, pal: &Palette, world: &World, cam: &Camera, t: f32, chop: f32, aa: bool) {
        let (fx, fy) = cam.forward();
        let (w, h) = (self.w, self.h);
        let mut hits: Vec<Option<Hit>> = vec![None; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let hit = Self::ray(map, cam, x as f32 + 0.5, y as f32 + 0.5);
                self.ids[i] = match &hit {
                    Some(hh) => ((hh.mx as u64) << 40) ^ ((hh.my as u64 & 0xFFFFF) << 8) ^ hh.face as u64 ^ 1 << 4,
                    None => 0,
                };
                hits[i] = hit;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let Some(hit) = hits[i] else { continue };
                let (albedo, ch, glyph) = self.shade(&hit, ts, pal, world, cam, t, chop, x, y);
                let depth = hit.x * fx + hit.y * fy;
                self.g[i] = GCell { albedo, ch, glyph, wx: hit.x, wy: hit.y, wz: hit.z as f32, face: hit.face, lit: true, depth };
            }
        }
        if !aa {
            return;
        }
        // Boundary cells: any neighbour with a different surface identity.
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
                let mut cols = [Rgb(0, 0, 0); 6];
                let mut first: Option<Hit> = None;
                for j in 0..3 {
                    for k in 0..2 {
                        let sx = x as f32 + (k as f32 + 0.5) / 2.0;
                        let sy = y as f32 + (j as f32 + 0.5) / 3.0;
                        cols[j * 2 + k] = match Self::ray(map, cam, sx, sy) {
                            Some(hh) => {
                                first.get_or_insert(hh);
                                Self::hit_color(&hh, pal, world)
                            }
                            None => world.sky(),
                        };
                    }
                }
                let Some(hh) = first else { continue };
                // A cell whose centre missed but whose edge touches terrain
                // takes that terrain's position and lighting.
                if self.g[i].depth == SKY_DEPTH {
                    let albedo = Self::hit_color(&hh, pal, world);
                    self.g[i] = GCell { albedo, ch: ' ', glyph: albedo, wx: hh.x, wy: hh.y, wz: hh.z as f32, face: hh.face, lit: true, depth: hh.x * fx + hh.y * fy };
                }
                // Two-colour quantisation: the first sample and its farthest.
                let a = cols[0];
                let b = *cols.iter().max_by_key(|c| color_dist(a, **c)).unwrap();
                if color_dist(a, b) < 24 {
                    continue;
                }
                let mut bits = 0u8;
                for (n, c) in cols.iter().enumerate() {
                    if color_dist(*c, a) <= color_dist(*c, b) {
                        bits |= 1 << n;
                    }
                }
                if bits == 0 || bits == 63 {
                    continue;
                }
                let cell = &mut self.g[i];
                cell.ch = sextant(bits);
                cell.glyph = a;
                cell.albedo = b;
            }
        }
    }

    /// Unlit colour and texture glyph for a hit.
    #[allow(clippy::too_many_arguments)]
    fn shade(&self, hit: &Hit, ts: &Tileset, pal: &Palette, world: &World, cam: &Camera, t: f32, chop: f32, sx: i32, sy: i32) -> (Rgb, char, Rgb) {
        let base = Self::hit_color(hit, pal, world);
        if hit.face != FACE_TOP {
            let ch = ts.wall[(hit.face - 1) as usize];
            let glyph = base.scale(if hit.face == FACE_RIGHT { 1.18 } else { 0.8 });
            return (base, ch, glyph);
        }
        let tile = &hit.tile;
        // Texture placement keyed to a ground grid near cell resolution, so it
        // stays put as the camera pans and thins consistently with zoom.
        let qs = (2 * cam.hw) as f32;
        let hv = hash((hit.x * qs).floor() as i64, (hit.y * qs * 2.0).floor() as i64, tile.seed as u64);
        let r = (hv % 1000) as f32 / 1000.0;
        let snow = world.snow_at(tile.temp as f32);
        let biome_ = &BIOMES[tile.biome as usize % BIOMES.len()];
        let vig = biome::vigour(tile.temp as f32, world.season);
        let ground_glyph = biome_.ground_glyph.lerp(pal.snow_glyph, snow).lerp(base.scale(1.25), 1.0 - vig);
        let wind_strength = if cam.hw <= 2 { 0.0 } else { world.weather.wind };
        let mut ch = ' ';
        let mut glyph = base;
        match tile.terrain {
            Terrain::Grass => {
                let density = (tile.grass as f32 * 0.14 + 0.04) * (1.0 - snow);
                let (set, density) = if vig > 0.55 {
                    (Some(&ts.cover[biome_.cover]), density * (0.4 + 0.6 * vig))
                } else if vig > 0.15 {
                    (None, density * 0.5 * (vig - 0.15) / 0.4 + 0.02)
                } else {
                    (None, 0.0)
                };
                if r < density {
                    let w = wind(sx as f32, sy as f32, t, wind_strength);
                    let lean = if w < -0.35 { 0 } else if w > 0.35 { 2 } else { 1 };
                    ch = match set {
                        Some(set) => set[lean],
                        None => ts.stubble[((hv >> 20) % 3) as usize],
                    };
                    glyph = ground_glyph.scale(0.85 + ((hv >> 12) % 100) as f32 * 0.003);
                }
            }
            Terrain::Water => {
                let wave = chop * smoothstep(6.0, 160.0, tile.body as f32);
                let phase = (t * (0.6 + wave) + sx as f32 * 0.13 + sy as f32 * 0.37 + (hv >> 16) as f32 * 0.001).sin();
                let pond = tile.body < 60 && tile.z >= SEA - 1;
                if pond && vig > 0.3 && r < 0.10 + 0.06 * vig {
                    ch = ts.cattail[((hv >> 20) % 2) as usize];
                    glyph = Rgb(120, 140, 70).lerp(Rgb(150, 120, 60), 1.0 - vig);
                } else if r < 0.12 + 0.34 * wave {
                    ch = ts.water[if phase > 0.6 { 0 } else if phase > 0.1 { 1 } else if phase > -0.5 { 2 } else { 3 }];
                    let crest = (0.15 + 0.85 * wave) * (0.55 + 0.45 * phase.max(0.0));
                    glyph = base.lerp(pal.water_glyph, crest);
                }
            }
            Terrain::Sand => {
                if r < 0.14 {
                    ch = ts.sand[((hv >> 20) % 2) as usize];
                    glyph = pal.sand_glyph;
                }
            }
            Terrain::Dirt => {
                if r < 0.22 {
                    ch = ts.dirt[((hv >> 20) % 2) as usize];
                    glyph = pal.dirt_glyph;
                }
            }
            Terrain::Rock => {
                if r < 0.24 {
                    ch = ts.rock[((hv >> 20) % 2) as usize];
                    glyph = pal.rock_glyph;
                }
            }
            Terrain::Snow => {
                if r < 0.18 {
                    ch = ts.snow[((hv >> 20) % 2) as usize];
                    glyph = pal.snow_glyph;
                }
            }
        }
        (base, ch, glyph)
    }

    /// Map-space bounding box of everything that can appear on screen.
    fn visible_bounds(&self, cam: &Camera) -> (i32, i32, i32, i32) {
        let corners = [(0.0, 0.0), (self.w as f32, 0.0), (0.0, self.h as f32), (self.w as f32, self.h as f32)];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(sx, sy) in &corners {
            for z in [0.0, (MAX_Z + 24) as f32] {
                let (x, y) = cam.unproject(sx, sy, z);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        // Widest sprite art is about 16 columns either side of its tile.
        let m = (16.0 / cam.a()).ceil() as i32 + 2;
        (x0.floor() as i32 - m, y0.floor() as i32 - m, x1.ceil() as i32 + m, y1.ceil() as i32 + m)
    }

    /// Trees, buildings and entities, back to front with a depth test.
    fn sprite_pass(&mut self, map: &Map, ts: &Tileset, pal: &Palette, world: &World, cam: &Camera, t: f32) {
        let (fx, fy) = cam.forward();
        let (x0, y0, x1, y1) = self.visible_bounds(cam);
        let night = 1.0 - world.skylight();
        let pal_player = SpriteColors { bg: Rgb(52, 74, 150), fg: Rgb(240, 214, 176), trunk_bg: Rgb(52, 74, 150), trunk_fg: Rgb(240, 214, 176) };
        let mut items: Vec<(f32, i32, i32, Tile)> = Vec::new();
        for my in y0..=y1 {
            for mx in x0..=x1 {
                let Some(tile) = map.get(mx, my) else { continue };
                let has_entity = world.entities.iter().any(|e| e.mx == mx && e.my == my);
                if tile.tree.is_none() && tile.building.is_none() && !has_entity {
                    continue;
                }
                let depth = (mx as f32 + 0.5) * fx + (my as f32 + 0.5) * fy + 0.5 * (fx.abs() + fy.abs());
                items.push((depth, mx, my, tile));
            }
        }
        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (depth, mx, my, tile) in items {
            let (sx, sy) = cam.project_tile(mx, my, tile.draw_z());
            if sx < -40 || sx > self.w + 40 || sy < -40 || sy > self.h + 40 {
                continue;
            }
            if let Some(v) = tile.tree {
                let sp = &SPECIES[tile.species as usize % SPECIES.len()];
                let set = ts.trees(cam.zoom, sp.form);
                // Variants come in a smaller pair then a larger pair; species
                // scale chooses the pair.
                let pair = if sp.scale < 0.9 { 0 } else if sp.scale > 1.2 { 2 } else { (v as usize / 2 % 2) * 2 };
                let sprite = &set[(pair + v as usize % 2) % set.len()];
                let snow = world.snow_at(tile.temp as f32) * 0.7;
                let colors = SpriteColors {
                    bg: biome::seasonal(&sp.canopy, world.season).lerp(pal.snow, snow),
                    fg: biome::seasonal(&sp.canopy_glyph, world.season).lerp(pal.snow_glyph, snow * 0.5),
                    trunk_bg: pal.trunk,
                    trunk_fg: pal.trunk_glyph,
                };
                let gust = if cam.hw <= 2 { 0.0 } else { world.gust(mx as f32, my as f32, t) };
                self.sprite(sprite, &tile, mx, my, sx, sy, &colors, gust, t, depth);
            }
            if let Some(v) = tile.building {
                let mat_ix = if matches!(tile.terrain, Terrain::Rock) || tile.z >= crate::map::SNOW - 3 {
                    biome::STONE
                } else {
                    BIOMES[tile.biome as usize % BIOMES.len()].material
                };
                let mat = &MATERIALS[mat_ix];
                let set = ts.houses(cam.zoom);
                let sprite = &set[v as usize % set.len()];
                let snow = world.snow_at(tile.temp as f32) * 0.8;
                let colors = SpriteColors { bg: mat.roof.lerp(pal.snow, snow), fg: mat.roof_glyph, trunk_bg: mat.wall, trunk_fg: mat.wall_glyph };
                self.sprite(sprite, &tile, mx, my, sx, sy, &colors, 0.0, t, depth);
                if night > 0.05 {
                    self.frame_lights.push(Light { mx, my, z: tile.draw_z(), color: [1.0 * night, 0.75 * night, 0.4 * night], radius: 4.0, intensity: 0.8, flicker: false });
                }
            }
            for e in &world.entities {
                if e.mx == mx && e.my == my {
                    self.sprite(ts.player(cam.zoom), &tile, mx, my, sx, sy, &pal_player, 0.0, t, depth + 0.01);
                }
            }
        }
    }

    /// Draw a billboard anchored so its bottom row sits on the tile's centre
    /// row. `gust` in 0..1 leans the canopy with the wind, more at the top.
    /// Cells already holding nearer terrain or sprites are left alone.
    #[allow(clippy::too_many_arguments)]
    fn sprite(&mut self, sp: &Sprite, tile: &Tile, mx: i32, my: i32, sx: i32, sy: i32, col: &SpriteColors, gust: f32, t: f32, depth: f32) {
        let n = sp.rows.len() as i32;
        let phase = (tile.seed % 628) as f32 * 0.01;
        let lean = if gust > 0.0 { (t * (0.8 + 1.2 * gust) + phase).sin() * gust * (1.0 + n as f32 * 0.08) } else { 0.0 };
        for (r, row) in sp.rows.iter().enumerate() {
            let r = r as i32;
            let height = n - 1 - r;
            let off = (lean * height as f32 / (n - 1).max(1) as f32).round() as i32;
            let y = sy - height;
            let trunk = r as usize >= sp.rows.len() - sp.trunk_rows;
            let (bg, fg) = if trunk { (col.trunk_bg, col.trunk_fg) } else { (col.bg, col.fg) };
            for (c, ch) in row.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let x = sx + c as i32 - sp.center + if trunk { 0 } else { off };
                let wz = tile.draw_z() as f32 + height as f32;
                if let Some(cell) = self.cell(x, y) {
                    if cell.depth > depth {
                        continue;
                    }
                    *cell = GCell { albedo: bg, ch, glyph: fg, wx: mx as f32, wy: my as f32, wz, face: FACE_TOP, lit: true, depth };
                }
            }
        }
    }

    fn fires(&mut self, map: &Map, world: &World, cam: &Camera, ts: &Tileset, t: f32) {
        let (fx, fy) = cam.forward();
        for l in &world.lights {
            let _ = map;
            let (sx, sy) = cam.project_tile(l.mx, l.my, l.z);
            let depth = (l.mx as f32 + 0.5) * fx + (l.my as f32 + 0.5) * fy + 0.5 * (fx.abs() + fy.abs());
            let frame = ((t * 9.0) as usize + (l.mx as usize)) % 3;
            let flick = 0.8 + 0.2 * (t * 17.0 + l.mx as f32).sin();
            let hot = Rgb((255.0 * flick) as u8, (190.0 * flick) as u8, (70.0 * flick) as u8);
            let ember = Rgb(140, 50, 20);
            for (i, dx) in (-1..=1).enumerate() {
                let ch = ts.flame[(frame + i) % 3];
                let y = if dx == 0 { sy - 1 } else { sy };
                if let Some(c) = self.cell(sx + dx, y) {
                    let bg = c.albedo;
                    *c = GCell { albedo: bg, ch, glyph: hot, wx: l.mx as f32, wy: l.my as f32, wz: l.z as f32, face: 0, lit: false, depth };
                }
            }
            for dx in -1..=0 {
                if let Some(c) = self.cell(sx + dx, sy + 1) {
                    *c = GCell { albedo: ember, ch: ' ', glyph: ember, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth };
                }
            }
        }
    }

    /// Modal settings window, one row per table item.
    fn popover(&self, cv: &mut Canvas, settings: &Settings) {
        let name_w = ITEMS.iter().map(|i| i.name.len()).max().unwrap_or(8);
        let val_w = ITEMS.iter().flat_map(|i| i.values.iter().map(|v| v.len())).max().unwrap_or(8);
        let hint = " up/down select   left/right change   esc close ";
        let inner = (name_w + val_w + 11).max(hint.len());
        let rows = ITEMS.len() as i32 + 4;
        let x0 = (self.w - inner as i32 - 2) / 2;
        let y0 = (self.h - rows) / 2;
        let (fg, bg, dim) = (Rgb(225, 225, 235), Rgb(28, 30, 44), Rgb(140, 140, 160));
        let row = |body: &str| format!("│{:<w$}│", body, w = inner);
        cv.text(x0, y0, &format!("┌{}┐", "─".repeat(inner)), dim, bg);
        cv.text(x0 + 2, y0, " settings ", fg, bg);
        cv.text(x0, y0 + 1, &row(""), dim, bg);
        for (i, item) in ITEMS.iter().enumerate() {
            let selected = i == settings.cursor;
            let body = format!(
                "  {:<nw$}   {} {:^vw$} {}",
                item.name,
                if selected { '<' } else { ' ' },
                settings.label(i),
                if selected { '>' } else { ' ' },
                nw = name_w,
                vw = val_w
            );
            let (lf, lb) = if selected { (bg, Rgb(200, 200, 215)) } else { (fg, bg) };
            let y = y0 + 2 + i as i32;
            cv.text(x0, y, &row(&body), lf, lb);
            cv.put(x0, y, '│', dim, bg);
            cv.put(x0 + inner as i32 + 1, y, '│', dim, bg);
        }
        cv.text(x0, y0 + rows - 2, &row(hint), dim, bg);
        cv.text(x0, y0 + rows - 1, &format!("└{}┘", "─".repeat(inner)), dim, bg);
    }

    fn sky_pass(&mut self, ts: &Tileset, world: &World, t: f32) {
        let sky = world.sky();
        let fade = 1.0 - world.skylight() * 0.92;
        for y in 0..self.h {
            for x in 0..self.w {
                let h = hash(x as i64, y as i64, 0xBEEF);
                let (ch, glyph) = if h.is_multiple_of(89) {
                    let tw = 0.55 + 0.45 * (t * 1.7 + (h >> 20) as f32 * 0.01).sin();
                    let b = (tw * fade * 235.0) as u8;
                    (ts.star[(h >> 8).is_multiple_of(5) as usize], sky.lerp(Rgb(b, b, b.saturating_add(15)), fade))
                } else {
                    (' ', sky)
                };
                self.g[(y * self.w + x) as usize] =
                    GCell { albedo: sky, ch, glyph, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
            }
        }
    }

    fn light_pass(&self, cv: &mut Canvas, world: &World, t: f32) {
        let amb = world.ambient();
        let sun = world.sun();
        let sunny = sun[0] + sun[1] + sun[2] > 0.01;
        let cloud_th = world.cloud_threshold();
        let (shadow_dx, shadow_dy) = world.shadow_shift();
        let face_k = [1.0f32, 0.78, 0.5];
        let mul = |c: Rgb, l: [f32; 3]| -> Rgb {
            Rgb(
                (c.0 as f32 * l[0]).clamp(0.0, 255.0) as u8,
                (c.1 as f32 * l[1]).clamp(0.0, 255.0) as u8,
                (c.2 as f32 * l[2]).clamp(0.0, 255.0) as u8,
            )
        };
        for y in 0..self.h {
            for x in 0..self.w {
                let g = self.g[(y * self.w + x) as usize];
                if !g.lit {
                    cv.put(x, y, g.ch, g.glyph, g.albedo);
                    continue;
                }
                let mut l = amb;
                let fk = face_k[g.face as usize];
                let amb_face = 0.75 + 0.25 * fk;
                l = [l[0] * amb_face, l[1] * amb_face, l[2] * amb_face];
                if sunny {
                    let (ox, oy) = world.cloud_offset;
                    let cloud = fbm((g.wx + ox + shadow_dx) * 0.07, (g.wy + oy + shadow_dy) * 0.07, 0xC10D, 3);
                    let shadow = smoothstep(cloud_th, cloud_th + 0.10, cloud);
                    let s = fk * (1.0 - 0.72 * shadow);
                    l = [l[0] + sun[0] * s, l[1] + sun[1] * s, l[2] + sun[2] * s];
                }
                let mut pl = [0.0f32; 3];
                for (li, light) in world.lights.iter().chain(self.frame_lights.iter()).enumerate() {
                    let dx = g.wx - light.mx as f32;
                    let dy = g.wy - light.my as f32;
                    let dz = (g.wz - light.z as f32) * 0.5;
                    let d = (dx * dx + dy * dy + dz * dz).sqrt();
                    if d >= light.radius {
                        continue;
                    }
                    let mut f = (1.0 - d / light.radius).powi(2) * light.intensity;
                    if light.flicker {
                        f *= 0.78 + 0.22 * (t * 11.0 + li as f32 * 1.7).sin() * (t * 5.3).cos().abs();
                    }
                    pl = [pl[0] + light.color[0] * f, pl[1] + light.color[1] * f, pl[2] + light.color[2] * f];
                }
                // Soft knee so clustered lights saturate instead of blowing out.
                let knee = |v: f32| 1.6 * (1.0 - (-v / 1.6).exp());
                l = [l[0] + knee(pl[0]), l[1] + knee(pl[1]), l[2] + knee(pl[2])];
                cv.put(x, y, g.ch, mul(g.glyph, l), mul(g.albedo, l));
            }
        }
    }

    /// Cloud layer seen from above at the two smallest zooms. Each screen
    /// cell samples the cloud field at the point a ray from a virtual camera
    /// of height C meets the cloud plane at altitude H: the ground point under
    /// the cell, raised H rows, pulled toward the screen centre by 1 - H/C.
    /// Panning therefore moves clouds by C/(C - H) relative to the ground.
    fn cloud_pass(&self, cv: &mut Canvas, ts: &Tileset, world: &World, map: &Map, cam: &Camera) {
        if cam.hw > 3 {
            return;
        }
        let cover = world.weather.cover;
        // Light cover reads as clouds above the land; heavy cover has already
        // dimmed the whole scene, so the layer fades out toward overcast.
        let strength = smoothstep(0.03, 0.15, cover) * (1.0 - smoothstep(0.7, 0.9, cover));
        if strength <= 0.0 {
            return;
        }
        let altitude = World::CLOUD_ALTITUDE;
        let c = cam.height();
        let k = 1.0 - altitude / c;
        let (cx, cy) = cam.unproject_map(self.w as f32 / 2.0, self.h as f32 / 2.0, map);
        let (ox, oy) = world.cloud_offset;
        let th = world.cloud_threshold();
        let light = 0.3 + 0.7 * world.skylight();
        let sunlit = Rgb(238, 240, 246).scale(light);
        let shaded = Rgb(190, 196, 212).scale(light);
        let rows = altitude * cam.hh as f32 / 2.0;
        for y in 0..self.h {
            for x in 0..self.w {
                let (gx, gy) = cam.unproject_map(x as f32 + 0.5, y as f32 + rows, map);
                let (wx, wy) = (cx + (gx - cx) * k, cy + (gy - cy) * k);
                let d = fbm((wx + ox) * 0.07, (wy + oy) * 0.07, 0xC10D, 3);
                let a = smoothstep(th, th + 0.16, d) * strength;
                if a < 0.08 {
                    continue;
                }
                // Thick centres are bright; edges take the shaded tone.
                let core = smoothstep(th + 0.1, th + 0.3, d);
                let col = shaded.lerp(sunlit, core);
                let i = (y * self.w + x) as usize;
                let cell = cv.cells[i];
                if a > 0.6 {
                    cv.put(x, y, ' ', col, cell.bg.lerp(col, a));
                } else {
                    cv.put(x, y, ts.wall[1], col, cell.bg.lerp(col, a * 0.6));
                }
            }
        }
    }

    /// Precipitation overlay: intensity from the weather, kind from the
    /// temperature at the screen centre.
    fn weather_pass(&self, cv: &mut Canvas, ts: &Tileset, world: &World, map: &Map, cam: &Camera, t: f32) {
        let (w, h) = (self.w, self.h);
        let p = world.weather.precip;
        if p < 0.02 {
            return;
        }
        let (mx, my) = cam.center_tile(map, w, h);
        let temp = map.get(mx, my).map(|t| t.temp as f32).unwrap_or(10.0);
        let wind = world.weather.wind;
        let across = world.weather.wind_dir.cos();
        if world.snowing_at(temp) {
            let n = ((w * h) as f32 / 40.0 * p) as i32;
            for i in 0..n {
                let hv = hash(i as i64, 9, 0xA2);
                let speed = 3.5 + (hv % 5) as f32 * 0.6;
                let x0 = (hv % w as u64) as f32;
                let y0 = ((hv >> 24) % h as u64) as f32;
                let drift = (t * 0.9 + (hv >> 40) as f32 * 0.01).sin() * 2.5 + t * wind * 6.0 * across;
                let x = ((x0 + drift).rem_euclid(w as f32)) as i32;
                let y = ((y0 + t * speed) % h as f32) as i32;
                let ch = ts.snowflake[(hv >> 16).is_multiple_of(3) as usize];
                cv.glyph(x, y, ch, Rgb(235, 240, 250));
            }
        } else {
            let n = ((w * h) as f32 / 28.0 * p) as i32;
            for i in 0..n {
                let hv = hash(i as i64, 7, 0xA1);
                let speed = 26.0 + (hv % 10) as f32;
                let x0 = (hv % w as u64) as f32;
                let y0 = ((hv >> 24) % h as u64) as f32;
                let x = ((x0 + t * wind * 8.0 * across).rem_euclid(w as f32)) as i32;
                let y = ((y0 + t * speed) % h as f32) as i32;
                cv.glyph(x, y, ts.rain, Rgb(150, 170, 205));
            }
        }
    }

    fn hud(&self, cv: &mut Canvas, ts: &Tileset, world: &World, cam: &Camera, map: &Map) {
        let s = world.season.rem_euclid(4.0);
        let here = world
            .entities
            .first()
            .and_then(|e| map.get(e.mx, e.my))
            .map(|t| {
                let b = &BIOMES[t.biome as usize % BIOMES.len()];
                format!("{} ({}) {}C z{}", b.name, b.koppen, t.temp, t.z)
            })
            .unwrap_or_default();
        let wx = &world.weather;
        let weather = format!("cloud {:.0}% wind {:.0}% precip {:.0}%", wx.cover * 100.0, wx.wind * 100.0, wx.precip * 100.0);
        let line = format!(
            " roguemap  {}deg  zoom {}  {} ({:.2})  {:02}:{:02}{}  {}  glyphs:{}  lights:{}  {} ",
            cam.degrees(),
            cam.zoom,
            SEASON_NAMES[s.floor() as usize % 4],
            s,
            world.tod.floor() as i32,
            ((world.tod.fract()) * 60.0) as i32,
            if world.auto_time { "" } else { " (paused)" },
            weather,
            ts.name,
            world.lights.len() + self.frame_lights.len(),
            here,
        );
        let help = " tab settings  m world map  wasd/hjkl walk  arrows pan  c centre  r/R ( ) rotate  z/Z zoom  v fill  g glyphs  [ ] season  , . time  p pause  W weather  f fire  F clear  H hud  q quit ";
        cv.text(0, 0, &line, Rgb(220, 220, 230), Rgb(30, 32, 44));
        cv.text(0, self.h - 1, help, Rgb(160, 160, 176), Rgb(30, 32, 44));
    }
}

