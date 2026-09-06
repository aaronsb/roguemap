//! Cast shadows (docs/structures.md): once per frame a mask in map space
//! over the visible tiles, four samples per tile, holding the height of
//! the highest sun ray any occluder blocks over each ground point. A point
//! below that height is in shadow. Occluders are terrain steeper than the
//! sun's ray, stack columns with their roofs, and tree canopies, each
//! swept along the sun's ground direction by its height in metres times the
//! shadow length per metre, which is the cloud shadows' own factor.

use crate::blocks::Profile;
use crate::grid::HeightGrid;
use crate::map::SEA;
use crate::render::Scene;

/// Samples per tile along each axis.
const RES: f32 = 2.0;
/// Longest sweep in tiles, whatever the sun does.
const MAX_SWEEP: f32 = 12.0;
/// Occluders below this height above their surroundings cast nothing.
const CLEAR: f32 = -1.0e9;

pub(crate) struct ShadowMask {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    top: Vec<f32>,
    /// Unit ground direction shadows fall along.
    pub(crate) u: (f32, f32),
    /// Tiles of shadow per metre of occluder height.
    pub(crate) k: f32,
}

impl ShadowMask {
    /// Build the mask for the frame, or `None` when there is no sun.
    pub(crate) fn build(sc: &Scene, grid: &HeightGrid) -> Option<ShadowMask> {
        let world = sc.world;
        if world.daylight() <= 0.0 {
            return None;
        }
        let k = world.shadow_per_metre();
        if k < 0.05 {
            return None; // the sun is overhead: nothing reaches past its own footprint
        }
        let (x0, y0, x1, y1) = grid.bounds();
        let (w, h) = (((x1 - x0 + 1) as f32 * RES) as i32, ((y1 - y0 + 1) as f32 * RES) as i32);
        let u = world.shadow_dir();
        let mut mask = ShadowMask { x0, y0, w, h, top: vec![CLEAR; (w * h) as usize], u, k };
        let volumes = crate::raster::lod_of(sc.cam.rows_per_metre()).volumes;
        for (mx, my, _, g) in grid.cells() {
            let (cx, cy) = (mx as f32 + 0.5, my as f32 + 0.5);
            // Terrain: a tile whose ground drops faster than the sun's ray
            // along the shadow direction shades what lies below it.
            let here = grid.sample(cx, cy).max(SEA as f32);
            let ahead = grid.sample(cx + u.0, cy + u.1).max(SEA as f32);
            if (here - ahead) * k > 2.0 {
                mask.stamp((cx, cy), 0.6, 0.0, (k * (here - ahead) + 1.5).min(MAX_SWEEP), here, None);
            }
            if let Some(st) = g.stack {
                if st.levels > 0 {
                    let b = &sc.assets.blocks[st.kind as usize % sc.assets.blocks.len()];
                    let profile = Profile { roof: b.roof, pitch: b.pitch, max_rise: b.max_rise };
                    let top = g.zs + 0.5 * profile.peak(g.runs);
                    let len = (k * (top - g.base) + 1.5).min(MAX_SWEEP);
                    mask.stamp((cx, cy), 0.72, 0.0, len, top, Some((mx as f32, my as f32, 1.0, 1.0)));
                }
            }
        }
        if volumes {
            for v in &grid.volumes {
                let len = (k * (v.top() - v.ground) + 1.5).min(MAX_SWEEP);
                let r = v.radius;
                mask.stamp((v.cx, v.cy), r, k * (v.h0 - v.ground), len, v.top(), Some((v.cx - r, v.cy - r, 2.0 * r, 2.0 * r)));
            }
        }
        Some(mask)
    }

    /// Sweep a disc of radius `r` at `c` along the shadow direction from
    /// `t0` to `t1` tiles, recording the height of the ray from `top` that
    /// enters the disc's column. Samples inside `exclude` (x, y, w, h), the
    /// occluder's own footprint, are left alone so it does not shade itself.
    fn stamp(&mut self, c: (f32, f32), r: f32, t0: f32, t1: f32, top: f32, exclude: Option<(f32, f32, f32, f32)>) {
        let (ux, uy) = self.u;
        let (px, py) = (-uy, ux);
        // Bounding box of the swept disc.
        let (ax, ay) = (c.0 + ux * t0, c.1 + uy * t0);
        let (bx, by) = (c.0 + ux * t1, c.1 + uy * t1);
        let (minx, maxx) = (ax.min(bx) - r, ax.max(bx) + r);
        let (miny, maxy) = (ay.min(by) - r, ay.max(by) + r);
        let i0 = (((minx - self.x0 as f32) * RES).floor() as i32).max(0);
        let i1 = (((maxx - self.x0 as f32) * RES).ceil() as i32).min(self.w - 1);
        let j0 = (((miny - self.y0 as f32) * RES).floor() as i32).max(0);
        let j1 = (((maxy - self.y0 as f32) * RES).ceil() as i32).min(self.h - 1);
        let r2 = r * r;
        for j in j0..=j1 {
            let qy = self.y0 as f32 + (j as f32 + 0.5) / RES;
            for i in i0..=i1 {
                let qx = self.x0 as f32 + (i as f32 + 0.5) / RES;
                if let Some((ex, ey, ew, eh)) = exclude {
                    if qx >= ex && qx < ex + ew && qy >= ey && qy < ey + eh {
                        continue;
                    }
                }
                let (dx, dy) = (qx - c.0, qy - c.1);
                let t = dx * ux + dy * uy;
                let lat = dx * px + dy * py;
                if lat.abs() > r {
                    continue;
                }
                let tc = t.clamp(t0, t1);
                if (t - tc) * (t - tc) + lat * lat > r2 {
                    continue;
                }
                // The sun ray through the top of the occluder reaches this
                // point after it leaves the occluder's column.
                let entry = (t - (r2 - lat * lat).max(0.0).sqrt()).max(0.0);
                let ray = top - entry / self.k;
                let slot = &mut self.top[(j * self.w + i) as usize];
                if ray > *slot {
                    *slot = ray;
                }
            }
        }
    }

    /// Shadow at a world point, 0 lit to 1 shaded, bilinear over the four
    /// samples around it.
    pub(crate) fn factor(&self, wx: f32, wy: f32, wz: f32) -> f32 {
        let fx = (wx - self.x0 as f32) * RES - 0.5;
        let fy = (wy - self.y0 as f32) * RES - 0.5;
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i32, iy as i32);
        let at = |x: i32, y: i32| -> f32 {
            let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
            if wz < self.top[(y * self.w + x) as usize] - 0.02 {
                1.0
            } else {
                0.0
            }
        };
        let a = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * tx;
        let b = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * tx;
        // Soft, but not a tile-wide ramp: the edge is tightened to the
        // middle of the blend.
        crate::noise::smoothstep(0.2, 0.8, a + (b - a) * ty)
    }

    /// Shadow at a surface point, looked up a little way along the sun ray
    /// so a point on the sunlit side of its own occluder stays lit.
    pub(crate) fn at_surface(&self, wx: f32, wy: f32, wz: f32) -> f32 {
        let step = 0.2;
        self.factor(wx - self.u.0 * step, wy - self.u.1 * step, wz + step / self.k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::blocks::Stack;
    use crate::camera::Camera;
    use crate::map::{Map, Tile};
    use crate::render::Renderer;
    use crate::tileset::Tileset;
    use crate::world::World;

    #[test]
    fn a_single_column_shades_the_ground_down_sun_for_its_height_times_the_factor() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        // A flat plain with one three-level tower at (10, 10).
        let tower = assets.blocks.iter().position(|b| b.name == "tower").expect("a tower kind");
        let map = Map::synthetic(32, 32, assets.clone(), 0, move |x, y| {
            let mut t = Tile::flat(5);
            if (x, y) == (10, 10) {
                t.stack = Some(Stack { kind: tower as u8, levels: 3 });
            }
            t
        });
        let mut world = World::new(1);
        world.tod = 15.0; // elevation sin(3 pi / 4): about 0.707
        let (w, h) = (120, 40);
        let mut cam = Camera::new();
        cam.set_zoom(2, w, h);
        cam.look_at(10, 10, &map, w, h);
        let r = Renderer::new(w, h);
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        let (x0, y0, x1, y1) = r.visible_bounds(&cam, 40.0);
        let grid = HeightGrid::build(&sc, x0, y0, x1, y1, w, h);
        let mask = ShadowMask::build(&sc, &grid).expect("the sun is up");
        let k = world.shadow_per_metre();
        assert!((k - 0.5).abs() < 0.02, "at 15:00 a metre of height throws half a tile: {k}");
        let (ux, uy) = mask.u;
        assert!(ux < 0.0 && uy < 0.0, "shadows fall toward -x, -y like the cloud shadows: {:?}", mask.u);
        let ground = 5.5;
        let height = 3.0 * assets.blocks[tower].level_height;
        let reach = height * k;
        // Just outside the footprint on the shadow side the ground is dark;
        // beyond the reach it is lit; on the sun side it is lit.
        let along = |t: f32| (10.5 + ux * t, 10.5 + uy * t);
        let (sx, sy) = along(1.0);
        assert!(mask.factor(sx, sy, ground) > 0.9, "one tile down-sun is shaded");
        let (sx, sy) = along(reach + 0.4);
        assert!(mask.factor(sx, sy, ground) > 0.9, "the sweep reaches height times the factor ({reach} tiles)");
        let (sx, sy) = along(reach + 3.0);
        assert!(mask.factor(sx, sy, ground) < 0.1, "beyond the reach the ground is lit");
        let (sx, sy) = along(-1.5);
        assert_eq!(mask.factor(sx, sy, ground), 0.0, "the sun side is lit");
        // Height matters: a point at the column's top height in the shadow
        // band is above the ray and lit; the roof itself is not shaded.
        let (sx, sy) = along(1.0);
        assert_eq!(mask.factor(sx, sy, ground + height + 0.5), 0.0);
        assert_eq!(mask.at_surface(10.5, 10.5, ground + height), 0.0, "the roof is not in its own shadow");
        // Sideways, half a tile past the footprint edge, the mask is clear.
        let (px, py) = (-uy, ux);
        assert_eq!(mask.factor(10.5 + px * 1.8, 10.5 + py * 1.8, ground), 0.0);
        // At night there is no mask.
        world.tod = 1.0;
        let sc = Scene::new(&map, ts, &world, &cam, 0.0);
        assert!(ShadowMask::build(&sc, &grid).is_none());
    }
}
