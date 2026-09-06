//! Full-screen world map: biomes plotted top-down at one of three extents,
//! with a cursor for teleporting.

use crate::biome;
use crate::canvas::{Canvas, Rgb};
use crate::frame::Rect;
use crate::input::{self, WORLDMAP};
use crate::map::{relief_fraction, Map, ROCK_Z, SEA};
use crate::palette::{Palette, ROCK, SAND};
use crate::ui::CHROME;
use crate::world::World;

/// Tiles per cell horizontally at each extent; vertically twice that, to
/// keep proportions on 1:2 cells.
pub const SCALES: [i32; 3] = [1, 4, 16];
pub const SCALE_NAMES: [&str; 3] = ["small", "medium", "large"];

pub struct WorldMap {
    pub scale: usize,
    /// Cursor in map coordinates.
    pub cursor: (i32, i32),
}

impl Default for WorldMap {
    fn default() -> WorldMap {
        WorldMap::new()
    }
}

impl WorldMap {
    pub fn new() -> WorldMap {
        WorldMap { scale: 1, cursor: (0, 0) }
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

    /// Step through the extents, wrapping.
    pub fn step_extent(&mut self, dir: i32) {
        let n = SCALES.len() as i32;
        self.scale = (self.scale as i32 + dir).rem_euclid(n) as usize;
    }

    /// Put the player on the nearest land to the cursor and return that
    /// tile.
    pub fn teleport(&self, map: &Map, world: &mut World) -> (i32, i32) {
        let (tx, ty) = map.nearest_land(self.cursor.0, self.cursor.1);
        if let Some(p) = world.player_mut() {
            p.mx = tx;
            p.my = ty;
        }
        (tx, ty)
    }

    /// The map keeps the spring palette in every season so the legend and
    /// the plot stay readable under winter snow.
    fn plot_palette(map: &Map) -> &Palette {
        &map.assets.surfaces.seasons[0].palette
    }

    /// Colour of one sampled tile: water by depth, snow and rock by height
    /// and cold, otherwise the biome's ground colour. Coarser than the
    /// scene's surface colour: no dirt, no shoreline sand, and a stronger
    /// height lift so relief reads at sixteen tiles per cell.
    fn sample(map: &Map, world: &World, x: i32, y: i32) -> Rgb {
        let plot = Self::plot_palette(map);
        let c = map.climate(x, y);
        if c.z < SEA {
            let depth = ((SEA - c.z) as f32 / -crate::map::FLOOR).clamp(0.0, 1.0);
            return plot.water_shallow.lerp(plot.water_deep, depth);
        }
        let mut col = biome::ground_color(&map.assets.biomes[c.biome], world.season);
        if c.z >= ROCK_Z {
            col = plot.surfaces[ROCK].color;
        } else if c.z == SEA {
            col = plot.surfaces[SAND].color;
        }
        let lift = 0.8 + 0.36 * relief_fraction((c.z - SEA) as f32);
        col.scale(lift).lerp(plot.snow(), world.snow_at(c.temp))
    }

    /// Draw the map over the frame's rectangle.
    pub fn draw(&self, cv: &mut Canvas, rect: Rect, map: &Map, world: &World, player: Option<(i32, i32)>) {
        self.plot(cv, rect, map, world, player);
        Self::crosshair(cv, rect);
        self.header(cv, rect, map);
        Self::legend(cv, rect, map, world);
    }

    /// Biome colours per cell, averaged over a 2x2 sub-sample at the coarse
    /// extents so they do not alias, with the player marked.
    fn plot(&self, cv: &mut Canvas, rect: Rect, map: &Map, world: &World, player: Option<(i32, i32)>) {
        let (w, h) = (rect.w, rect.h);
        let (sx, sy) = self.stride();
        let (cx, cy) = self.cursor;
        let x0 = cx - (w / 2) * sx;
        let y0 = cy - (h / 2) * sy;
        let void = Rgb(10, 12, 22);
        for row in 0..h {
            for col in 0..w {
                let (x, y) = (x0 + col * sx, y0 + row * sy);
                let c = if !map.contains(x, y) {
                    void
                } else if sx == 1 {
                    Self::sample(map, world, x, y)
                } else {
                    let (hx, hy) = (sx / 2, sy / 2);
                    let cs = [Self::sample(map, world, x, y), Self::sample(map, world, x + hx, y), Self::sample(map, world, x, y + hy), Self::sample(map, world, x + hx, y + hy)];
                    let avg = |f: fn(&Rgb) -> u8| (cs.iter().map(|c| f(c) as u32).sum::<u32>() / 4) as u8;
                    Rgb(avg(|c| c.0), avg(|c| c.1), avg(|c| c.2))
                };
                cv.put(rect.x + col, rect.y + row, ' ', c, c);
            }
        }
        if let Some((px, py)) = player {
            let (col, row) = ((px - x0) / sx, (py - y0) / sy);
            cv.glyph(rect.x + col, rect.y + row, '@', Rgb(255, 255, 255));
        }
    }

    /// Crosshair at the cursor, which is always the screen centre.
    fn crosshair(cv: &mut Canvas, rect: Rect) {
        let (ccol, crow) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
        let white = Rgb(255, 255, 255);
        for d in 2..5 {
            cv.glyph(ccol - d, crow, '─', white);
            cv.glyph(ccol + d, crow, '─', white);
            cv.glyph(ccol, crow - (d + 1) / 2, '│', white);
            cv.glyph(ccol, crow + (d + 1) / 2, '│', white);
        }
        cv.glyph(ccol, crow, '┼', white);
    }

    /// Extent, cursor position and the climate under it.
    fn header(&self, cv: &mut Canvas, rect: Rect, map: &Map) {
        let (sx, sy) = self.stride();
        let (cx, cy) = self.cursor;
        let assets = &map.assets;
        let c = map.climate(cx, cy);
        let b = &assets.biomes[c.biome];
        let lead = b.species.first().map(|&(sp, _)| assets.species[sp].name.as_str()).unwrap_or("none");
        let header = format!(
            " world map  {}  1 cell = {}x{} tiles  cursor {},{}  {} ({})  {:.0}C  precip {:.0}  z{}  trees: {}  builds: {} ",
            SCALE_NAMES[self.scale % SCALES.len()],
            sx,
            sy,
            cx,
            cy,
            b.name,
            b.koppen,
            c.temp,
            c.precip,
            c.z,
            lead,
            assets.materials[b.material].name
        );
        cv.text(rect.x, rect.y, &header, CHROME.text, CHROME.bar);
    }

    /// Biome swatches along the bottom row, then the key hints.
    fn legend(cv: &mut Canvas, rect: Rect, map: &Map, world: &World) {
        let (w, y) = (rect.w, rect.y + rect.h - 1);
        let mut x = 0;
        for b in &map.assets.biomes {
            let sw = biome::ground_color(b, world.season);
            cv.put(rect.x + x, y, ' ', sw, sw);
            cv.put(rect.x + x + 1, y, ' ', sw, sw);
            let label = format!(" {} ", b.koppen);
            cv.text(rect.x + x + 2, y, &label, CHROME.legend, CHROME.bar);
            x += 2 + label.len() as i32;
        }
        let hint = input::help_line(WORLDMAP, "  ");
        cv.text(rect.x + (w - hint.len() as i32).max(x), y, &hint, CHROME.dim, CHROME.bar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::map::{Terrain, Tile};

    #[test]
    fn teleport_lands_on_the_nearest_land_tile_to_the_cursor() {
        // A lake with two islets; the cursor sits nearer the western one.
        let map = Map::synthetic(24, 24, test_assets(), 0, |x, y| Tile::flat(if (x, y) == (4, 10) || (x, y) == (20, 10) { 6 } else { -4 }));
        let mut world = World::new(1);
        world.spawn_player(&map, 20, 10);
        let mut wm = WorldMap::new();
        wm.cursor = (9, 12);
        assert_eq!(map.get(9, 12).unwrap().terrain, Terrain::Water);
        assert_eq!(wm.teleport(&map, &mut world), (4, 10));
        assert_eq!(world.player().map(|p| (p.mx, p.my)), Some((4, 10)));
        wm.cursor = (20, 10);
        assert_eq!(wm.teleport(&map, &mut world), (20, 10), "a land cursor is the destination itself");
        assert_eq!(world.player().map(|p| (p.mx, p.my)), Some((20, 10)));
        // The map's own extents and cursor stride.
        assert_eq!(wm.stride(), (4, 8));
        wm.move_cursor(1, -1);
        assert_eq!(wm.cursor, (24, 2), "the cursor steps by one cell of the current extent");
        wm.step_extent(1);
        assert_eq!(wm.stride(), (16, 32));
        wm.step_extent(1);
        assert_eq!(wm.scale, 0, "extents wrap");
        assert_eq!(wm.teleport(&map, &mut World::new(1)), (20, 10), "with no player the destination is still reported");
    }
}
