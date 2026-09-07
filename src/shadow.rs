//! Cast shadows (docs/structures.md): once per frame a mask in map space
//! over the visible tiles, four samples per tile and sixteen where props
//! cast, holding the height of
//! the highest sun ray any occluder blocks over each ground point. A point
//! below that height is in shadow. Occluders are terrain steeper than the
//! sun's ray, stack columns with their roofs, props and tree canopies, each
//! swept along the sun's ground direction by its height in metres times the
//! shadow length per metre, which is the cloud shadows' own factor.

use crate::biome::Prop;
use crate::blocks::Profile;
use crate::canvas::Rgb;
use crate::grid::HeightGrid;
use crate::map::{SEA, TILE_METRES};
use crate::render::Scene;

/// Samples per tile along each axis, and the finer grid the close zooms
/// use: a prop is a sub-tile thing, and a boulder's shadow is shorter than
/// one coarse sample, so where props cast the mask is laid twice as fine.
const RES: f32 = 2.0;
const CLOSE_RES: f32 = 4.0;
/// Longest sweep in tiles, whatever the sun does.
const MAX_SWEEP: f32 = 12.0;
/// Occluders below this height above their surroundings cast nothing.
const CLEAR: f32 = -1.0e9;
/// Shortest prop that casts, in metres: below this it is ground cover and
/// has no shadow to speak of.
const PROP_MIN_H: f32 = 0.25;

pub(crate) struct ShadowMask {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// Samples per tile of this frame's mask.
    res: f32,
    top: Vec<f32>,
    /// How much of the sun the occluder over each sample stops, 0..1: a
    /// crown stops its species' leaf density, so a thin tree throws a light
    /// shadow (docs/structures.md, "Porous canopies"). Terrain and walls
    /// stop all of it.
    opacity: Vec<f32>,
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
        let lod = crate::raster::lod_of(sc.cam.detail_rows());
        let (volumes, props) = (lod.volumes || sc.cam.is_perspective(), lod.prop_shadows);
        let res = if props { CLOSE_RES } else { RES };
        let (w, h) = (((x1 - x0 + 1) as f32 * res) as i32, ((y1 - y0 + 1) as f32 * res) as i32);
        let u = world.shadow_dir();
        let mut mask = ShadowMask { x0, y0, w, h, res, top: vec![CLEAR; (w * h) as usize], opacity: vec![0.0; (w * h) as usize], u, k };
        for (mx, my, t, g) in grid.cells() {
            let (cx, cy) = (mx as f32 + 0.5, my as f32 + 0.5);
            // Terrain: a tile whose ground drops faster than the sun's ray
            // along the shadow direction shades what lies below it.
            let here = grid.sample(cx, cy).max(SEA as f32);
            let ahead = grid.sample(cx + u.0, cy + u.1).max(SEA as f32);
            if (here - ahead) * k > 2.0 {
                mask.stamp((cx, cy), 0.6, 0.0, (k * (here - ahead) + 1.5).min(MAX_SWEEP), here, 1.0, None);
            }
            // Props: a boulder or a tent is a solid thing a metre or two
            // high and its `size` says so, so it lays a short shadow of its
            // own through the same sweep. Only at the close zooms: at the
            // overview a prop is one glyph and a metre is under a row.
            if props {
                crate::sprites::scatter(sc, mx, my, t, |x, y, pi| mask.prop(&sc.assets.props[pi], x, y, g.base));
            }
            if let Some(st) = g.stack {
                if st.levels > 0 {
                    let b = &sc.assets.blocks[st.kind as usize % sc.assets.blocks.len()];
                    let profile = Profile { roof: b.roof, pitch: b.pitch, max_rise: b.max_rise };
                    let top = g.zs + 0.5 * profile.peak(g.runs);
                    let len = (k * (top - g.base) + 1.5).min(MAX_SWEEP);
                    mask.stamp((cx, cy), 0.72, 0.0, len, top, 1.0, Some((mx as f32, my as f32, 1.0, 1.0)));
                }
            }
        }
        if props {
            // Hand-placed props stand where someone put them rather than on
            // a tile's own geometry, so their ground comes from the field.
            for pl in sc.world.placed.iter().filter(|pl| pl.prop < sc.assets.props.len()) {
                mask.prop(&sc.assets.props[pl.prop], pl.x, pl.y, grid.sample(pl.x, pl.y).max(SEA as f32));
            }
        }
        if volumes {
            // One sweep per tree, from its stand-in crown: the branches and
            // clusters of a grown tree are the same crown seen closer, and
            // a hundred thousand discs would cost more than the frame.
            for v in grid.tree_crowns() {
                let len = (k * (v.top() - v.ground) + 1.5).min(MAX_SWEEP);
                let r = v.radius;
                let sp = &sc.assets.species[v.species as usize % sc.assets.species.len()];
                let opacity = if v.dead { 0.45 } else { sp.leaf_density };
                mask.stamp((v.cx, v.cy), r, k * (v.h0 - v.ground), len, v.top(), opacity, Some((v.cx - r, v.cy - r, 2.0 * r, 2.0 * r)));
            }
        }
        Some(mask)
    }

    /// Sweep one prop standing at a ground point: a disc the size of its
    /// footprint, from its top, with `size` in metres deciding both the
    /// height of the ray and how far down-sun it reaches. A prop is a
    /// sub-tile thing, so a boulder's whole shadow is a sample or two past
    /// its own footprint, and one narrower than the mask can hold throws
    /// nothing.
    fn prop(&mut self, p: &Prop, x: f32, y: f32, ground: f32) {
        let (w, d, h) = (p.size[0] / TILE_METRES, p.size[1] / TILE_METRES, p.size[2]);
        if h < PROP_MIN_H {
            return; // a patch of moss is ground cover, not a thing standing on it
        }
        // A prop drawn as a bare glyph over the ground — no fill colour — is
        // a thin thing: a tuft of grass or a stand of reeds stops half the
        // sun, the way a sparse crown does. A solid one stops all of it.
        let opacity = if p.color == Rgb(0, 0, 0) { 0.5 } else { 1.0 };
        let r = w.max(d) / 2.0;
        self.stamp((x, y), r, 0.0, (self.k * h + r).min(MAX_SWEEP), ground + h, opacity, Some((x - w / 2.0, y - d / 2.0, w, d)));
    }

    /// Sweep a disc of radius `r` at `c` along the shadow direction from
    /// `t0` to `t1` tiles, recording the height of the ray from `top` that
    /// enters the disc's column. Samples inside `exclude` (x, y, w, h), the
    /// occluder's own footprint, are left alone so it does not shade itself.
    #[allow(clippy::too_many_arguments)]
    fn stamp(&mut self, c: (f32, f32), r: f32, t0: f32, t1: f32, top: f32, opacity: f32, exclude: Option<(f32, f32, f32, f32)>) {
        if t0 >= t1 {
            return; // a sweep that starts past MAX_SWEEP falls beyond the cap and marks nothing
        }
        let (ux, uy) = self.u;
        let (px, py) = (-uy, ux);
        // Bounding box of the swept disc.
        let (ax, ay) = (c.0 + ux * t0, c.1 + uy * t0);
        let (bx, by) = (c.0 + ux * t1, c.1 + uy * t1);
        let (minx, maxx) = (ax.min(bx) - r, ax.max(bx) + r);
        let (miny, maxy) = (ay.min(by) - r, ay.max(by) + r);
        let i0 = (((minx - self.x0 as f32) * self.res).floor() as i32).max(0);
        let i1 = (((maxx - self.x0 as f32) * self.res).ceil() as i32).min(self.w - 1);
        let j0 = (((miny - self.y0 as f32) * self.res).floor() as i32).max(0);
        let j1 = (((maxy - self.y0 as f32) * self.res).ceil() as i32).min(self.h - 1);
        let r2 = r * r;
        for j in j0..=j1 {
            let qy = self.y0 as f32 + (j as f32 + 0.5) / self.res;
            for i in i0..=i1 {
                let qx = self.x0 as f32 + (i as f32 + 0.5) / self.res;
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
                let at = (j * self.w + i) as usize;
                // The highest ray decides how far up the shadow reaches;
                // the densest occluder over the point decides how dark it
                // is, so two thin crowns shade more than one.
                self.top[at] = self.top[at].max(ray);
                self.opacity[at] = self.opacity[at].max(opacity);
            }
        }
    }

    /// Shadow at a world point, 0 lit to 1 shaded, bilinear over the four
    /// samples around it.
    pub(crate) fn factor(&self, wx: f32, wy: f32, wz: f32) -> f32 {
        let fx = (wx - self.x0 as f32) * self.res - 0.5;
        let fy = (wy - self.y0 as f32) * self.res - 0.5;
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - ix, fy - iy);
        let (ix, iy) = (ix as i32, iy as i32);
        // Coverage and opacity are blended apart: how much of the sample is
        // in shadow at all, and how much sun the thing casting it stops.
        let cell = |x: i32, y: i32| -> (f32, f32) {
            let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
            let i = (y * self.w + x) as usize;
            ((wz < self.top[i] - 0.02) as u8 as f32, self.opacity[i])
        };
        let lerp = |a: (f32, f32), b: (f32, f32), t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        let a = lerp(cell(ix, iy), cell(ix + 1, iy), tx);
        let b = lerp(cell(ix, iy + 1), cell(ix + 1, iy + 1), tx);
        let (cover, opacity) = lerp(a, b, ty);
        // Soft, but not a tile-wide ramp: the edge is tightened to the
        // middle of the blend.
        crate::noise::smoothstep(0.2, 0.8, cover) * opacity
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
    use crate::assets::{test_assets, Assets};
    use crate::biome::Cover;
    use crate::blocks::Stack;
    use crate::camera::Camera;
    use crate::map::{Map, Terrain, Tile};
    use crate::render::Renderer;
    use crate::tileset::Tileset;
    use crate::world::{PlacedProp, World};

    /// A tile of a plain the scattered props leave alone: sand under a cover
    /// none of them asks for, since they want grass, rock, dirt or the
    /// waterline. A test on one sees only the occluders it places itself.
    fn bare(assets: &Assets, z: i32) -> Tile {
        let biome = assets.biomes.iter().position(|b| b.cover != Cover::Dry).expect("a biome that is not dry country");
        Tile { terrain: Terrain::Sand, biome: biome as u8, ..Tile::flat(z) }
    }

    /// A sweep whose start is past its end marks nothing. The crown sweep
    /// caps its end at `MAX_SWEEP` and takes its start from the crown's
    /// underside, so a crown high enough over ground at a low sun hands
    /// `stamp` an interval that runs backwards.
    #[test]
    fn a_sweep_that_starts_past_its_end_marks_nothing() {
        let (w, h) = (16, 16);
        let mut mask = ShadowMask { x0: 0, y0: 0, w, h, res: RES, top: vec![CLEAR; (w * h) as usize], opacity: vec![0.0; (w * h) as usize], u: (1.0, 0.0), k: 1.8 };
        mask.stamp((4.0, 4.0), 1.0, MAX_SWEEP + 0.78, MAX_SWEEP, 20.0, 1.0, None);
        assert!(mask.top.iter().all(|&t| t == CLEAR), "an inverted sweep wrote a ray");
        assert!(mask.opacity.iter().all(|&o| o == 0.0), "an inverted sweep wrote an opacity");
        // The interval the owner's crash carried, to the bit.
        mask.stamp((4.0, 4.0), 1.0, 12.783108, 12.0, 20.0, 1.0, None);
        assert!(mask.top.iter().all(|&t| t == CLEAR));
    }

    #[test]
    fn a_single_column_shades_the_ground_down_sun_for_its_height_times_the_factor() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        // A flat plain with one three-level tower at (10, 10).
        let tower = assets.blocks.iter().position(|b| b.name == "tower").expect("a tower kind");
        let plain = bare(&assets, 5);
        let map = Map::synthetic(32, 32, assets.clone(), 0, move |x, y| {
            let mut t = plain;
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
        let grid = HeightGrid::build(&sc, x0, y0, x1, y1, w, h, &mut crate::grid::ModelCache::new());
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

    #[test]
    fn a_prop_shades_down_sun_by_its_height_at_the_close_zooms_and_nothing_at_the_overview() {
        let assets = test_assets();
        let ts = &Tileset::all(&assets)[0];
        let plain = bare(&assets, 5);
        let map = Map::synthetic(32, 32, assets.clone(), 0, move |_, _| plain);
        let prop = |name: &str| assets.props.iter().position(|p| p.name == name).unwrap_or_else(|| panic!("a {name} prop"));
        let (reeds, boulder) = (prop("reeds"), prop("boulder"));
        let (tall, short) = (assets.props[reeds].size[2], assets.props[boulder].size[2]);
        assert!(tall > 2.0 * short - 0.1, "the reeds stand about twice the boulder: {tall} m and {short} m");
        let mut world = World::new(1);
        world.tod = 15.0;
        world.placed.push(PlacedProp { x: 12.5, y: 12.5, prop: reeds });
        world.placed.push(PlacedProp { x: 17.5, y: 17.5, prop: boulder });
        let (w, h) = (120, 40);
        let r = Renderer::new(w, h);
        let ground = 5.5;
        let mask_at = |zoom: usize| {
            let mut cam = Camera::new();
            cam.set_zoom(zoom, w, h);
            cam.look_at(15, 15, &map, w, h);
            let sc = Scene::new(&map, ts, &world, &cam, 0.0);
            let (x0, y0, x1, y1) = r.visible_bounds(&cam, 40.0);
            let grid = HeightGrid::build(&sc, x0, y0, x1, y1, w, h, &mut crate::grid::ModelCache::new());
            ShadowMask::build(&sc, &grid).expect("the sun is up")
        };
        let mask = mask_at(2);
        let k = world.shadow_per_metre();
        let (ux, uy) = mask.u;
        let along = |c: (f32, f32), t: f32| (c.0 + ux * t, c.1 + uy * t);
        // The reeds shade the ground down-sun out to their height times the
        // per-unit length, and not beyond it or on the sun side.
        let (sx, sy) = along((12.5, 12.5), tall * k * 0.5);
        assert!(mask.factor(sx, sy, ground) > 0.2, "half way along the reeds' shadow the ground is dark");
        let (sx, sy) = along((12.5, 12.5), tall * k + 1.0);
        assert_eq!(mask.factor(sx, sy, ground), 0.0, "past {} tiles down-sun the ground is lit", tall * k);
        let (sx, sy) = along((12.5, 12.5), -1.0);
        assert_eq!(mask.factor(sx, sy, ground), 0.0, "the sun side is lit");
        // Height decides the reach: the boulder shades the ground just past
        // its own footprint, and not as far as a thing twice as tall does.
        let (sx, sy) = along((17.5, 17.5), short * k + 0.1);
        assert!(mask.factor(sx, sy, ground) > 0.2, "the boulder shades the ground beside it");
        let (sx, sy) = along((17.5, 17.5), tall * k);
        assert_eq!(mask.factor(sx, sy, ground), 0.0, "and no further than its own height carries");
        let (sx, sy) = along((12.5, 12.5), tall * k);
        assert!(mask.factor(sx, sy, ground) > 0.2, "where the reeds, twice as tall, still reach");
        // At the overview a prop is one glyph and throws nothing.
        let far = mask_at(0);
        for t in [0.25, 0.5, 1.0] {
            let (sx, sy) = along((12.5, 12.5), tall * k * t);
            assert_eq!(far.factor(sx, sy, ground), 0.0, "no prop shadow at 1:8");
        }
    }
}
