//! Screen-space overlays drawn after lighting: precipitation and the cloud
//! layer seen from above at the smallest zooms.

use crate::canvas::{Canvas, Rgb};
use crate::noise::{hash, smoothstep};
use crate::render::{Renderer, Scene};
use crate::world::World;

impl Renderer {
    /// Cloud layer seen from above at the two smallest zooms. Each screen
    /// cell samples the cloud field at the point a ray from a virtual camera
    /// of height C meets the cloud plane at altitude H: the ground point under
    /// the cell, raised H rows, pulled toward the screen centre by 1 - H/C.
    /// Panning therefore moves clouds by C/(C - H) relative to the ground.
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
        let altitude = World::CLOUD_ALTITUDE;
        let c = cam.altitude();
        let k = 1.0 - altitude / c;
        let (cx, cy) = cam.unproject(self.w as f32 / 2.0, self.h as f32 / 2.0, 0.0);
        let th = world.cloud_threshold();
        let light = 0.3 + 0.7 * world.skylight();
        let sunlit = Rgb(238, 240, 246).scale(light);
        let shaded = Rgb(190, 196, 212).scale(light);
        let rows = altitude * cam.hh as f32 / 2.0;
        for y in 0..self.h {
            for x in 0..self.w {
                let (gx, gy) = cam.unproject(x as f32 + 0.5, y as f32 + rows, 0.0);
                let (wx, wy) = (cx + (gx - cx) * k, cy + (gy - cy) * k);
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
