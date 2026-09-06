//! Screen-space overlays drawn after lighting: precipitation and the cloud
//! layer seen from above at the smallest zooms.

use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::noise::{hash, smoothstep};
use crate::render::{Renderer, Scene};
use crate::world::World;

/// Where screen cells meet the cloud plane for one frame: a ray from a
/// virtual camera of height C metres through the ground point under a cell,
/// raised by the rows the cloud altitude H is worth, meets the plane at
/// altitude H at that point pulled toward the screen centre by 1 - H/C.
/// Panning therefore moves clouds by C/(C - H) relative to the ground.
pub(crate) struct CloudView {
    cx: f32,
    cy: f32,
    k: f32,
    rows: f32,
}

impl CloudView {
    pub(crate) fn new(cam: &Camera, w: i32, h: i32) -> CloudView {
        let altitude = World::CLOUD_ALTITUDE;
        let c = cam.altitude();
        let k = 1.0 - altitude / c;
        let (cx, cy) = cam.unproject(w as f32 / 2.0, h as f32 / 2.0, 0.0);
        let rows = altitude * cam.rows_per_metre();
        CloudView { cx, cy, k, rows }
    }

    /// Cloud-plane point sampled at screen column `sx` (a cell's centre)
    /// and row `sy` (a cell's top edge).
    pub(crate) fn sample(&self, cam: &Camera, sx: f32, sy: f32) -> (f32, f32) {
        let (gx, gy) = cam.unproject(sx, sy + self.rows, 0.0);
        (self.cx + (gx - self.cx) * self.k, self.cy + (gy - self.cy) * self.k)
    }
}

impl Renderer {
    /// Cloud layer seen from above at the two smallest zooms; see
    /// `CloudView` for where each cell samples the field.
    pub(crate) fn cloud_pass(&self, cv: &mut Canvas, sc: &Scene) {
        let (ts, world, cam) = (sc.ts, sc.world, sc.cam);
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
        let view = CloudView::new(cam, self.w, self.h);
        let th = world.cloud_threshold();
        let light = 0.3 + 0.7 * world.skylight();
        let sunlit = Rgb(238, 240, 246).scale(light);
        let shaded = Rgb(190, 196, 212).scale(light);
        for y in 0..self.h {
            for x in 0..self.w {
                let (wx, wy) = view.sample(cam, x as f32 + 0.5, y as f32);
                let d = world.cloud_density(wx, wy);
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
    pub(crate) fn weather_pass(&self, cv: &mut Canvas, sc: &Scene) {
        let (map, ts, world, cam, t) = (sc.map, sc.ts, sc.world, sc.cam, sc.t);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::map::{Map, Tile};
    use crate::tileset::Tileset;

    #[test]
    fn cloud_sample_moves_by_c_over_c_minus_h_per_tile_of_pan() {
        let (w, h) = (120, 40);
        for zoom in 0..2 {
            let mut cam = Camera::new();
            cam.set_zoom(zoom, w, h);
            cam.look_at_point(0.0, 0.0, 0.0, w, h);
            let ratio = cam.altitude() / (cam.altitude() - World::CLOUD_ALTITUDE);
            let (sx, sy) = (33.5, 12.0);
            let before = CloudView::new(&cam, w, h).sample(&cam, sx, sy);
            let ground_before = cam.unproject(sx, sy, 0.0);
            cam.pan(1, 0);
            let view = CloudView::new(&cam, w, h);
            // The ground under a cell has moved by one tile footprint, 2hw cells.
            let cells = (2 * cam.hw) as f32;
            let ground_after = cam.unproject(sx + cells, sy, 0.0);
            assert!((ground_after.0 - ground_before.0).abs() < 1e-3 && (ground_after.1 - ground_before.1).abs() < 1e-3);
            // The cloud point that was under the cell is now C/(C - H) times as far along.
            let after = view.sample(&cam, sx + cells * ratio, sy);
            assert!((after.0 - before.0).abs() < 1e-3 && (after.1 - before.1).abs() < 1e-3, "zoom {zoom}: {before:?} vs {after:?}");
            let ground_speed = view.sample(&cam, sx + cells, sy);
            assert!((ground_speed.0 - before.0).abs() > 0.05, "zoom {zoom}: clouds move faster than the ground (ratio {ratio})");
        }
    }

    /// Count the precipitation glyphs of each kind on a canvas.
    fn count(cv: &Canvas, ts: &Tileset) -> (usize, usize) {
        let snow = cv.cells.iter().filter(|c| ts.snowflake.contains(&c.ch)).count();
        let rain = cv.cells.iter().filter(|c| c.ch == ts.rain).count();
        (snow, rain)
    }

    #[test]
    fn precipitation_kind_follows_the_temperature_at_the_screen_centre() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        assert!(!ts.snowflake.contains(&ts.rain) && ts.rain != ' ' && !ts.snowflake.contains(&' '));
        let (w, h) = (120, 40);
        for (temp, expect_snow) in [(-20i8, true), (25, false)] {
            let map = Map::synthetic(32, 32, assets.clone(), 0, move |_, _| {
                let mut t = Tile::flat(5);
                t.temp = temp;
                t
            });
            let mut world = World::new(1);
            world.weather.precip = 1.0;
            world.weather.wind = 0.0;
            let mut cam = Camera::new();
            cam.set_zoom(2, w, h);
            cam.look_at(16, 16, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 1.0);
            let r = Renderer::new(w, h);
            let mut cv = Canvas::new(w as u16, h as u16);
            r.weather_pass(&mut cv, &sc);
            let (snow, rain) = count(&cv, ts);
            if expect_snow {
                assert!(snow > 0 && rain == 0, "{temp}C: {snow} snowflakes, {rain} rain");
            } else {
                assert!(rain > 0 && snow == 0, "{temp}C: {snow} snowflakes, {rain} rain");
            }
            world.weather.precip = 0.0;
            let mut dry = Canvas::new(w as u16, h as u16);
            r.weather_pass(&mut dry, &Scene::new(&map, ts, &world, &cam, 1.0));
            assert_eq!(count(&dry, ts), (0, 0), "no precipitation draws nothing");
        }
    }
}
