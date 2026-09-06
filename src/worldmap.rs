//! Full-screen world map: biomes plotted top-down at one of three extents,
//! with a cursor for teleporting.

use crate::biome::{self, BIOMES};
use crate::canvas::{Canvas, Rgb};
use crate::map::{Map, SEA, SNOW};
use crate::world::World;

/// Tiles per cell horizontally at each extent; vertically twice that, to
/// keep proportions on 1:2 cells.
pub const SCALES: [i32; 3] = [1, 4, 16];
pub const SCALE_NAMES: [&str; 3] = ["small", "medium", "large"];

pub struct WorldMap {
    pub open: bool,
    pub scale: usize,
    /// Cursor in map coordinates.
    pub cursor: (i32, i32),
}

impl WorldMap {
    pub fn new() -> WorldMap {
        WorldMap { open: false, scale: 1, cursor: (0, 0) }
    }

    pub fn stride(&self) -> (i32, i32) {
        let s = SCALES[self.scale % SCALES.len()];
        (s, s * 2)
    }

    pub fn move_cursor(&mut self, dx: i32, dy: i32) {
        let (sx, sy) = self.stride();
        self.cursor.0 += dx * sx;
        self.cursor.1 += dy * sy;
    }

    /// Colour of one sampled tile: water by depth, snow and rock by height
    /// and cold, otherwise the biome's ground colour.
    fn sample(map: &Map, world: &World, x: i32, y: i32) -> Rgb {
        let z = map.height(x, y);
        if z < SEA {
            let depth = ((SEA - z) as f32 / 3.0).clamp(0.0, 1.0);
            return Rgb(46, 128, 176).lerp(Rgb(16, 48, 104), depth);
        }
        let temp = map.temperature(x, y, z);
        let precip = map.precipitation(x, y);
        let b = &BIOMES[biome::classify(temp, precip)];
        let mut c = biome::ground_color(b, world.season);
        if z >= SNOW - 2 {
            c = Rgb(124, 124, 130);
        } else if z == SEA {
            c = Rgb(196, 180, 130);
        }
        let lift = 0.8 + (z - SEA) as f32 * 0.03;
        c.scale(lift).lerp(Rgb(228, 232, 240), world.snow_at(temp))
    }

    /// Draw the map over the whole canvas.
    pub fn draw(&self, cv: &mut Canvas, map: &Map, world: &World, player: Option<(i32, i32)>) {
        let (w, h) = (cv.w, cv.h);
        let (sx, sy) = self.stride();
        let (cx, cy) = self.cursor;
        let x0 = cx - (w / 2) * sx;
        let y0 = cy - (h / 2) * sy;
        let void = Rgb(10, 12, 22);
        for row in 0..h {
            for col in 0..w {
                let (x, y) = (x0 + col * sx, y0 + row * sy);
                let inside = !map.bounded || (x >= 0 && y >= 0 && x < map.w as i32 && y < map.h as i32);
                let c = if !inside {
                    void
                } else if sx == 1 {
                    Self::sample(map, world, x, y)
                } else {
                    // Average a 2x2 sub-sample so coarse extents do not alias.
                    let (hx, hy) = (sx / 2, sy / 2);
                    let cs = [
                        Self::sample(map, world, x, y),
                        Self::sample(map, world, x + hx, y),
                        Self::sample(map, world, x, y + hy),
                        Self::sample(map, world, x + hx, y + hy),
                    ];
                    let avg = |f: fn(&Rgb) -> u8| (cs.iter().map(|c| f(c) as u32).sum::<u32>() / 4) as u8;
                    Rgb(avg(|c| c.0), avg(|c| c.1), avg(|c| c.2))
                };
                cv.put(col, row, ' ', c, c);
            }
        }
        if let Some((px, py)) = player {
            let (col, row) = ((px - x0) / sx, (py - y0) / sy);
            cv.glyph(col, row, '@', Rgb(255, 255, 255));
        }
        // Crosshair at the cursor.
        let (ccol, crow) = (w / 2, h / 2);
        let white = Rgb(255, 255, 255);
        for d in 2..5 {
            cv.glyph(ccol - d, crow, '─', white);
            cv.glyph(ccol + d, crow, '─', white);
            cv.glyph(ccol, crow - (d + 1) / 2, '│', white);
            cv.glyph(ccol, crow + (d + 1) / 2, '│', white);
        }
        cv.glyph(ccol, crow, '┼', white);

        // Header and legend.
        let z = map.height(cx, cy);
        let temp = map.temperature(cx, cy, z);
        let precip = map.precipitation(cx, cy);
        let b = &BIOMES[biome::classify(temp, precip)];
        let header = format!(
            " world map  {}  1 cell = {}x{} tiles  cursor {},{}  {} ({})  {:.0}C  precip {:.0}  z{} ",
            SCALE_NAMES[self.scale % SCALES.len()],
            sx,
            sy,
            cx,
            cy,
            b.name,
            b.koppen,
            temp,
            precip,
            z
        );
        cv.text(0, 0, &header, Rgb(220, 220, 230), Rgb(30, 32, 44));
        let mut x = 0;
        for b in BIOMES {
            let sw = biome::ground_color(b, world.season);
            cv.put(x, h - 1, ' ', sw, sw);
            cv.put(x + 1, h - 1, ' ', sw, sw);
            let label = format!(" {} ", b.koppen);
            cv.text(x + 2, h - 1, &label, Rgb(200, 200, 210), Rgb(30, 32, 44));
            x += 2 + label.len() as i32;
        }
        let hint = " arrows move  z extent  enter teleport  m/esc close ";
        cv.text((w - hint.len() as i32).max(x), h - 1, hint, Rgb(160, 160, 176), Rgb(30, 32, 44));
    }
}
