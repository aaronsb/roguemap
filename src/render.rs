//! Isometric rasteriser with a deferred lighting pass.
//!
//! Tiles are drawn back to front into a G-buffer holding unlit colour, glyph,
//! world position and face. A second pass lights every cell from ambient sky
//! light, the sun (shadowed by drifting clouds), and point lights, then a
//! weather overlay adds precipitation.

use crate::canvas::{Canvas, Rgb};
use crate::map::{Map, Terrain, MAX_Z, SEA};
use crate::settings::{Settings, ITEMS};
use crate::noise::{fbm, hash, smoothstep};
use crate::palette::{Palette, SEASON_NAMES};
use crate::tileset::{Sprite, Tileset, ZOOMS};
use crate::world::{Weather, World};

/// Column span `[lo, lo + w)` of diamond row `dy` for a footprint, sampling
/// the rhombus |x|/hw + |y|/hh <= 1 at row centres.
fn row_span(dy: i32, hw: i32, hh: i32) -> (i32, i32) {
    let yc = dy as f32 + 0.5;
    let w = (2.0 * hw as f32 * (1.0 - yc.abs() / hh as f32)).round().max(0.0) as i32;
    (-(w / 2), w)
}

fn row_contains(dy: i32, dx: i32, hw: i32, hh: i32) -> bool {
    let (lo, w) = row_span(dy, hw, hh);
    dx >= lo && dx < lo + w
}

/// Bottom-most diamond row that contains column `dx`.
fn col_bottom(dx: i32, hw: i32, hh: i32) -> Option<i32> {
    (-hh..hh).rev().find(|&dy| row_contains(dy, dx, hw, hh))
}

/// Whether a neighbouring tile offset by `(ox, oy)` on screen covers the
/// cell at `(dx, dy)` relative to this tile, at equal height.
fn neighbour_covers(ox: i32, oy: i32, dx: i32, dy: i32, hw: i32, hh: i32) -> bool {
    let r = dy - oy;
    r >= -hh && r < hh && row_contains(r, dx - ox, hw, hh)
}

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
}

pub struct Camera {
    pub rot: u8,
    pub ox: i32,
    pub oy: i32,
    pub zoom: usize,
    pub hw: i32,
    pub hh: i32,
}

impl Camera {
    pub fn new() -> Camera {
        Camera { rot: 0, ox: 0, oy: 0, zoom: 0, hw: ZOOMS[0].0, hh: ZOOMS[0].1 }
    }

    /// Largest zoom at which the whole map fits the screen, else the smallest.
    pub fn fitting_zoom(map: &Map, sw: i32, sh: i32) -> usize {
        let n = map.w.max(map.h) as i32;
        ZOOMS
            .iter()
            .rposition(|&(hw, hh)| 2 * n * hw <= sw && 2 * n * hh + crate::map::MAX_Z + 4 <= sh)
            .unwrap_or(0)
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    pub fn set_zoom(&mut self, zoom: usize, map: &Map, sw: i32, sh: i32) {
        let (mx, my) = self.center_tile(map, sw, sh);
        self.zoom = zoom % ZOOMS.len();
        self.hw = ZOOMS[self.zoom].0;
        self.hh = ZOOMS[self.zoom].1;
        self.look_at(mx, my, map, sw, sh);
    }

    /// Map coordinates to view coordinates for the current rotation.
    pub fn to_view(&self, mx: i32, my: i32, map: &Map) -> (i32, i32) {
        let (w, h) = (map.w as i32, map.h as i32);
        match self.rot & 3 {
            0 => (mx, my),
            1 => (my, w - 1 - mx),
            2 => (w - 1 - mx, h - 1 - my),
            _ => (h - 1 - my, mx),
        }
    }

    /// Rotate a small offset from view space back into map space.
    fn offset_to_map(&self, u: f32, v: f32) -> (f32, f32) {
        match self.rot & 3 {
            0 => (u, v),
            1 => (-v, u),
            2 => (-u, -v),
            _ => (v, -u),
        }
    }

    pub fn view_dims(&self, map: &Map) -> (i32, i32) {
        if self.rot & 1 == 0 {
            (map.w as i32, map.h as i32)
        } else {
            (map.h as i32, map.w as i32)
        }
    }

    /// Screen position of a view-space tile at height `z`.
    pub fn project(&self, vx: i32, vy: i32, z: i32) -> (i32, i32) {
        ((vx - vy) * self.hw + self.ox, (vx + vy) * self.hh - z + self.oy)
    }

    /// View-space tile under a screen point, ignoring height.
    pub fn unproject(&self, sx: i32, sy: i32) -> (i32, i32) {
        let a = (sx - self.ox) as f32 / self.hw as f32;
        let b = (sy - self.oy) as f32 / self.hh as f32;
        (((a + b) / 2.0).round() as i32, ((b - a) / 2.0).round() as i32)
    }

    /// Map tile nearest the centre of the screen.
    pub fn center_tile(&self, map: &Map, sw: i32, sh: i32) -> (i32, i32) {
        let (mut vx, mut vy) = self.unproject(sw / 2, sh / 2);
        if map.bounded {
            let (vw, vh) = self.view_dims(map);
            vx = vx.clamp(0, vw - 1);
            vy = vy.clamp(0, vh - 1);
        }
        self.view_to_map(vx, vy, map)
    }

    /// Turn a step in view space into a step in map space.
    pub fn view_delta_to_map(&self, dvx: i32, dvy: i32) -> (i32, i32) {
        let (u, v) = self.offset_to_map(dvx as f32, dvy as f32);
        (u as i32, v as i32)
    }

    pub fn view_to_map(&self, vx: i32, vy: i32, map: &Map) -> (i32, i32) {
        let (w, h) = (map.w as i32, map.h as i32);
        match self.rot & 3 {
            0 => (vx, vy),
            1 => (w - 1 - vy, vx),
            2 => (w - 1 - vx, h - 1 - vy),
            _ => (vy, h - 1 - vx),
        }
    }

    /// Place map tile `(mx, my)` at the centre of the screen.
    pub fn look_at(&mut self, mx: i32, my: i32, map: &Map, sw: i32, sh: i32) {
        let (vx, vy) = self.to_view(mx, my, map);
        let z = map.get(mx, my).map(|t| t.draw_z()).unwrap_or(SEA);
        self.ox = 0;
        self.oy = 0;
        let (sx, sy) = self.project(vx, vy, z);
        self.ox = sw / 2 - sx;
        self.oy = sh / 2 - sy;
    }

    /// Rotate by quarter turns about whatever is at the screen centre.
    pub fn rotate(&mut self, steps: i32, map: &Map, sw: i32, sh: i32) {
        let (mx, my) = self.center_tile(map, sw, sh);
        self.rot = ((self.rot as i32 + steps).rem_euclid(4)) as u8;
        self.look_at(mx, my, map, sw, sh);
    }
}

pub struct Renderer {
    w: i32,
    h: i32,
    g: Vec<GCell>,
    pub show_hud: bool,
}

fn wind(x: f32, y: f32, t: f32) -> f32 {
    (t * 1.8 + x * 0.09 + y * 0.18).sin() + 0.5 * (t * 0.6 - x * 0.05 + y * 0.11).sin()
}

impl Renderer {
    pub fn new(w: i32, h: i32) -> Renderer {
        let sky = GCell { albedo: Rgb(0, 0, 0), ch: ' ', glyph: Rgb(0, 0, 0), wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false };
        Renderer { w, h, g: vec![sky; (w * h) as usize], show_hud: true }
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
    pub fn draw(&mut self, cv: &mut Canvas, map: &Map, ts: &Tileset, world: &World, cam: &Camera, settings: &Settings, t: f32) {
        let pal_player = SpriteColors { bg: Rgb(52, 74, 150), fg: Rgb(240, 214, 176), trunk_bg: Rgb(52, 74, 150), trunk_fg: Rgb(240, 214, 176) };
        let pal = Palette::for_season(world.season);
        let chop = world.choppiness(t);
        self.sky_pass(ts, world, t);
        // Walk every view-space tile whose footprint, walls or sprite can touch
        // the screen, back to front along diagonals of constant vx + vy.
        const SPRITE_H: i32 = 24;
        const SPRITE_W: i32 = 16;
        let (hw, hh) = (cam.hw, cam.hh);
        let s_min = (-cam.oy - 2 * hh - MAX_Z).div_euclid(hh) - 1;
        let s_max = (self.h - cam.oy + MAX_Z + hh + SPRITE_H).div_euclid(hh) + 1;
        let d_min = (-cam.ox - 2 * hw - SPRITE_W).div_euclid(hw) - 1;
        let d_max = (self.w - cam.ox + 2 * hw + SPRITE_W).div_euclid(hw) + 1;
        for sdiag in s_min..=s_max {
            for d in d_min..=d_max {
                if (sdiag + d) & 1 != 0 {
                    continue;
                }
                let (vx, vy) = ((sdiag + d) / 2, (sdiag - d) / 2);
                let (mx, my) = cam.view_to_map(vx, vy, map);
                let Some(tile) = map.get(mx, my) else { continue };
                let z = tile.draw_z();
                let (sx, sy) = cam.project(vx, vy, z);
                self.surface(map, &tile, mx, my, sx, sy, ts, &pal, cam, t, chop);
                self.walls(map, &tile, vx, vy, mx, my, sx, sy, cam, &pal, ts);
                if let Some(v) = tile.tree {
                    let set = ts.trees(cam.zoom);
                    let sprite = &set[v as usize % set.len()];
                    let colors = SpriteColors { bg: pal.canopy, fg: pal.canopy_glyph, trunk_bg: pal.trunk, trunk_fg: pal.trunk_glyph };
                    self.sprite(sprite, &tile, mx, my, sx, sy, &colors, true, t);
                }
                for e in &world.entities {
                    if e.mx == mx && e.my == my {
                        self.sprite(ts.player(cam.zoom), &tile, mx, my, sx, sy, &pal_player, false, t);
                    }
                }
            }
        }
        self.fires(map, world, cam, ts, t);
        self.light_pass(cv, world, t);
        self.weather_pass(cv, ts, world, t);
        if self.show_hud {
            self.hud(cv, ts, world, cam);
        }
        if settings.open {
            self.popover(cv, settings);
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
                let (ch, glyph) = if h % 89 == 0 {
                    let tw = 0.55 + 0.45 * (t * 1.7 + (h >> 20) as f32 * 0.01).sin();
                    let b = (tw * fade * 235.0) as u8;
                    (ts.star[((h >> 8) % 5 == 0) as usize], sky.lerp(Rgb(b, b, b.saturating_add(15)), fade))
                } else {
                    (' ', sky)
                };
                self.g[(y * self.w + x) as usize] =
                    GCell { albedo: sky, ch, glyph, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false };
            }
        }
    }

    /// Drawn height of the tile at a view position; off-map counts as ground.
    fn view_z(&self, map: &Map, cam: &Camera, vx: i32, vy: i32) -> i32 {
        let (mx, my) = cam.view_to_map(vx, vy, map);
        map.get(mx, my).map(|t| t.draw_z()).unwrap_or(0)
    }

    fn base_color(tile: &crate::map::Tile, pal: &Palette) -> Rgb {
        let lift = 0.86 + tile.draw_z() as f32 * 0.018;
        match tile.terrain {
            Terrain::Water => {
                let depth = ((SEA - tile.z) as f32 / 3.0).clamp(0.0, 1.0);
                pal.water_shallow.lerp(pal.water_deep, depth)
            }
            Terrain::Sand => pal.sand.scale(lift),
            Terrain::Grass => pal.grass.scale(lift),
            Terrain::Dirt => pal.dirt.scale(lift),
            Terrain::Rock => pal.rock.scale(lift),
            Terrain::Snow => pal.snow,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn surface(
        &mut self,
        _map: &Map,
        tile: &crate::map::Tile,
        mx: i32,
        my: i32,
        sx: i32,
        sy: i32,
        ts: &Tileset,
        pal: &Palette,
        cam: &Camera,
        t: f32,
        chop: f32,
    ) {
        let base = Self::base_color(tile, pal);
        // Wave visibility: weather roughness scaled by body size, so ponds lie flat.
        let wave = chop * smoothstep(6.0, 160.0, tile.body as f32);
        let z = tile.draw_z() as f32;
        let (thw, thh) = (cam.hw, cam.hh);
        for dy in -thh..thh {
            let (lo, w) = row_span(dy, thw, thh);
            for dx in lo..lo + w {
                let (x, y) = (sx + dx, sy + dy);
                let hv = hash(mx as i64 * 8 + dx as i64, my as i64 * 8 + dy as i64, tile.seed as u64);
                let r = (hv % 1000) as f32 / 1000.0;
                let u = (dx as f32 / thw as f32 + dy as f32 / thh as f32) * 0.5;
                let v = (dy as f32 / thh as f32 - dx as f32 / thw as f32) * 0.5;
                let (du, dv) = cam.offset_to_map(u, v);
                let (wx, wy) = (mx as f32 + du, my as f32 + dv);
                let mut ch = ' ';
                let mut glyph = base;
                match tile.terrain {
                    Terrain::Grass => {
                        if r < tile.grass as f32 * 0.14 + 0.04 {
                            let w = wind((x) as f32, (y) as f32, t);
                            ch = ts.grass[if w < -0.35 { 0 } else if w > 0.35 { 2 } else { 1 }];
                            let vary = 0.85 + ((hv >> 12) % 100) as f32 * 0.003;
                            glyph = pal.grass_glyph.scale(vary);
                        }
                    }
                    Terrain::Water => {
                        let phase = (t * (0.6 + wave) + x as f32 * 0.13 + y as f32 * 0.37 + (hv >> 16) as f32 * 0.001).sin();
                        if r < 0.12 + 0.34 * wave {
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
                if let Some(c) = self.cell(x, y) {
                    *c = GCell { albedo: base, ch, glyph, wx, wy, wz: z, face: FACE_TOP, lit: true };
                }
            }
        }
    }

    /// Cliff faces. For each column the neighbour directly below is found
    /// geometrically, and the height difference to it is drawn as wall rows.
    #[allow(clippy::too_many_arguments)]
    fn walls(&mut self, map: &Map, tile: &crate::map::Tile, vx: i32, vy: i32, mx: i32, my: i32, sx: i32, sy: i32, cam: &Camera, pal: &Palette, ts: &Tileset) {
        let z = tile.draw_z();
        let (thw, thh) = (cam.hw, cam.hh);
        let right = z - self.view_z(map, cam, vx + 1, vy);
        let left = z - self.view_z(map, cam, vx, vy + 1);
        let front = z - self.view_z(map, cam, vx + 1, vy + 1);
        if right <= 0 && left <= 0 && front <= 0 {
            return;
        }
        let surface = Self::base_color(tile, pal);
        let (mx, my) = (mx as f32, my as f32);
        let (lo, w) = (-thh..thh).map(|dy| row_span(dy, thw, thh)).min_by_key(|&(lo, _)| lo).unwrap_or((0, 0));
        let max_w = (-thh..thh).map(|dy| row_span(dy, thw, thh).1).max().unwrap_or(0);
        let _ = w;
        for dx in lo..lo + max_w {
            let Some(bot) = col_bottom(dx, thw, thh) else { continue };
            let below = bot + 1;
            let (d, face) = if neighbour_covers(thw, thh, dx, below, thw, thh) {
                (right, FACE_RIGHT)
            } else if neighbour_covers(-thw, thh, dx, below, thw, thh) {
                (left, FACE_LEFT)
            } else if neighbour_covers(0, 2 * thh, dx, below, thw, thh) {
                (front, if dx >= 0 { FACE_RIGHT } else { FACE_LEFT })
            } else {
                continue;
            };
            for k in 1..=d {
                // Shallow steps keep the surface tone so contours read as
                // shading bands; deeper rows turn to exposed earth.
                let earth = ((k - 1) as f32 / 4.0).min(1.0);
                let depth_shade = 1.0 - (k as f32 * 0.02).min(0.25);
                let albedo = surface.scale(0.82).lerp(pal.dirt, earth).scale(depth_shade);
                let wz = (z - k) as f32;
                let ch = ts.wall[(face - 1) as usize];
                let glyph = albedo.scale(if face == FACE_RIGHT { 1.18 } else { 0.8 });
                if let Some(c) = self.cell(sx + dx, sy + bot + k) {
                    *c = GCell { albedo, ch, glyph, wx: mx, wy: my, wz, face, lit: true };
                }
            }
        }
    }

    /// Draw a billboard anchored so its bottom row sits on the tile's centre
    /// row. With `sway` the canopy leans with the wind, more at the top.
    #[allow(clippy::too_many_arguments)]
    fn sprite(&mut self, sp: &Sprite, tile: &crate::map::Tile, mx: i32, my: i32, sx: i32, sy: i32, col: &SpriteColors, sway: bool, t: f32) {
        let n = sp.rows.len() as i32;
        let phase = (tile.seed % 628) as f32 * 0.01;
        let lean = if sway { (t * 1.3 + phase).sin() * (1.0 + n as f32 * 0.08) } else { 0.0 };
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
                    *cell = GCell { albedo: bg, ch, glyph: fg, wx: mx as f32, wy: my as f32, wz, face: FACE_TOP, lit: true };
                }
            }
        }
    }

    fn fires(&mut self, map: &Map, world: &World, cam: &Camera, ts: &Tileset, t: f32) {
        for l in &world.lights {
            let (vx, vy) = cam.to_view(l.mx, l.my, map);
            let (sx, sy) = cam.project(vx, vy, l.z);
            let frame = ((t * 9.0) as usize + (l.mx as usize)) % 3;
            let flick = 0.8 + 0.2 * (t * 17.0 + l.mx as f32).sin();
            let hot = Rgb((255.0 * flick) as u8, (190.0 * flick) as u8, (70.0 * flick) as u8);
            let ember = Rgb(140, 50, 20);
            for (i, dx) in (-1..=1).enumerate() {
                let ch = ts.flame[(frame + i) % 3];
                let y = if dx == 0 { sy - 1 } else { sy };
                if let Some(c) = self.cell(sx + dx, y) {
                    let bg = c.albedo;
                    *c = GCell { albedo: bg, ch, glyph: hot, wx: l.mx as f32, wy: l.my as f32, wz: l.z as f32, face: 0, lit: false };
                }
            }
            for dx in -1..=0 {
                if let Some(c) = self.cell(sx + dx, sy + 1) {
                    *c = GCell { albedo: ember, ch: ' ', glyph: ember, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false };
                }
            }
        }
    }

    fn light_pass(&self, cv: &mut Canvas, world: &World, t: f32) {
        let amb = world.ambient();
        let sun = world.sun();
        let sunny = sun[0] + sun[1] + sun[2] > 0.01;
        let cloud_th = world.cloud_threshold();
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
                    let cloud = fbm(g.wx * 0.07 + t * 0.30, g.wy * 0.07 + t * 0.11, 0xC10D, 3);
                    let shadow = smoothstep(cloud_th, cloud_th + 0.10, cloud);
                    let s = fk * (1.0 - 0.72 * shadow);
                    l = [l[0] + sun[0] * s, l[1] + sun[1] * s, l[2] + sun[2] * s];
                }
                for (li, light) in world.lights.iter().enumerate() {
                    let dx = g.wx - light.mx as f32;
                    let dy = g.wy - light.my as f32;
                    let dz = (g.wz - light.z as f32) * 0.5;
                    let d = (dx * dx + dy * dy + dz * dz).sqrt();
                    if d >= light.radius {
                        continue;
                    }
                    let mut f = (1.0 - d / light.radius).powi(2) * 2.2;
                    if light.flicker {
                        f *= 0.78 + 0.22 * (t * 11.0 + li as f32 * 1.7).sin() * (t * 5.3).cos().abs();
                    }
                    l = [l[0] + light.color[0] * f, l[1] + light.color[1] * f, l[2] + light.color[2] * f];
                }
                cv.put(x, y, g.ch, mul(g.glyph, l), mul(g.albedo, l));
            }
        }
    }

    fn weather_pass(&self, cv: &mut Canvas, ts: &Tileset, world: &World, t: f32) {
        let (w, h) = (self.w, self.h);
        match world.weather {
            Weather::Clear => {}
            Weather::Rain => {
                let n = (w * h / 28).max(1);
                for i in 0..n {
                    let hv = hash(i as i64, 7, 0xA1);
                    let speed = 26.0 + (hv % 10) as f32;
                    let x = (hv % w as u64) as i32;
                    let y0 = ((hv >> 24) % h as u64) as f32;
                    let y = ((y0 + t * speed) % h as f32) as i32;
                    cv.glyph(x, y, ts.rain, Rgb(150, 170, 205));
                }
            }
            Weather::Snow => {
                let n = (w * h / 40).max(1);
                for i in 0..n {
                    let hv = hash(i as i64, 9, 0xA2);
                    let speed = 3.5 + (hv % 5) as f32 * 0.6;
                    let x0 = (hv % w as u64) as f32;
                    let y0 = ((hv >> 24) % h as u64) as f32;
                    let drift = (t * 0.9 + (hv >> 40) as f32 * 0.01).sin() * 2.5;
                    let x = ((x0 + drift).rem_euclid(w as f32)) as i32;
                    let y = ((y0 + t * speed) % h as f32) as i32;
                    let ch = ts.snowflake[((hv >> 16) % 3 == 0) as usize];
                    cv.glyph(x, y, ch, Rgb(235, 240, 250));
                }
            }
        }
    }

    fn hud(&self, cv: &mut Canvas, ts: &Tileset, world: &World, cam: &Camera) {
        let s = world.season.rem_euclid(4.0);
        let line = format!(
            " roguemap  rot {}  zoom {}  {} ({:.2})  {:02}:{:02}{}  {}  glyphs:{}  lights:{} ",
            cam.rot,
            cam.zoom,
            SEASON_NAMES[s.floor() as usize % 4],
            s,
            world.tod.floor() as i32,
            ((world.tod.fract()) * 60.0) as i32,
            if world.auto_time { "" } else { " (paused)" },
            world.weather.name(),
            ts.name,
            world.lights.len(),
        );
        let help = " tab settings  wasd/hjkl walk  arrows pan  c centre  r/R rotate  z/Z zoom  v fill  g glyphs  [ ] season  , . time  p pause  W weather  f fire  F clear  H hud  q quit ";
        cv.text(0, 0, &line, Rgb(220, 220, 230), Rgb(30, 32, 44));
        cv.text(0, self.h - 1, help, Rgb(160, 160, 176), Rgb(30, 32, 44));
    }
}

