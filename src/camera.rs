//! The isometric camera: a view angle, a screen offset and a tile footprint.
//! Projection maps a world point to a screen cell; unprojection walks back
//! to the ground at a chosen height.
//!
//! World positions are metres (ADR-004). A tile is `TILE_METRES` square and
//! each zoom is an exact halving of the next, so the scale of a zoom is two
//! numbers derived from its footprint: `columns_per_metre` across the
//! ground and `rows_per_metre` up the screen. Heights project through the
//! second, so a 2 m person is 12 rows at 1:1 and 1.5 at 1:8.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use crate::map::{Map, SEA, TILE_METRES};
use crate::tileset::{ZOOMS, ZOOM_NAMES, ZOOM_RATIOS};
use crate::world::World;

/// Rows a metre of height draws as at a footprint half width; see
/// `Camera::rows_per_metre`.
pub fn rows_per_metre_of(hw: i32) -> f32 {
    hw as f32 * 3.0 / 8.0
}

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

/// Where a tile sits on screen: its map position and drawn height, the
/// screen cell over its centre, and its depth for the sprite depth test.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub mx: i32,
    pub my: i32,
    /// Surface height the sprite stands on.
    pub z: f32,
    pub sx: i32,
    pub sy: i32,
    pub depth: f32,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

impl Camera {
    pub fn new() -> Camera {
        Camera { angle: FRAC_PI_4, ox: 0.0, oy: 0.0, zoom: 0, hw: ZOOMS[0].0, hh: ZOOMS[0].1, focus_z: SEA as f32 }
    }

    /// Largest zoom at which the whole map fits the screen, else the
    /// smallest. The relief of the map itself, not the world's ceiling,
    /// sets how many rows the terrain needs above its footprint.
    pub fn fitting_zoom(map: &Map, sw: i32, sh: i32) -> usize {
        let n = map.w.max(map.h) as i32;
        let relief = map.relief_ceiling() as f32;
        ZOOMS
            .iter()
            .rposition(|&(hw, hh)| 2 * n * hw <= sw && 2 * n * hh + (relief * rows_per_metre_of(hw)).ceil() as i32 + 4 <= sh)
            .unwrap_or(0)
    }

    pub fn a(&self) -> f32 {
        self.hw as f32 * SQRT_2
    }

    pub fn b(&self) -> f32 {
        self.hh as f32 * SQRT_2
    }

    /// Screen columns a metre of ground spans: the tile's own scale, since
    /// a tile is `TILE_METRES` square.
    pub fn columns_per_metre(&self) -> f32 {
        self.a() / TILE_METRES
    }

    /// Screen rows a metre of height draws as, from the same footprint:
    /// three rows per eight columns of half width, so a 2 m person is 12
    /// rows at 1:1 and halves with every zoom out. On 1:2 cells that is 96
    /// pixels of height against 90 pixels of ground per metre, so vertical
    /// and horizontal scale agree.
    pub fn rows_per_metre(&self) -> f32 {
        rows_per_metre_of(self.hw)
    }

    /// The zoom's name and the scale it draws at, for the HUD.
    pub fn zoom_name(&self) -> (&'static str, &'static str) {
        let i = self.zoom % ZOOMS.len();
        (ZOOM_NAMES[i], ZOOM_RATIOS[i])
    }

    /// The zoom the inset view shows while the main view is at `main`
    /// (ADR-004): the other end of the scale, biased toward the close view.
    /// Zoomed out at all — 1:2, 1:4 or 1:8 — the inset is 1:1; at 1:1 it is
    /// 1:8. The two views never share a level.
    pub fn inset_zoom(main: usize) -> usize {
        let close = ZOOMS.len() - 1;
        if main % ZOOMS.len() == close { 0 } else { close }
    }

    /// The ratio the inset draws at while the main view is at `main`.
    pub fn inset_ratio(main: usize) -> &'static str {
        ZOOM_RATIOS[Camera::inset_zoom(main)]
    }

    /// Screen position of a world point; `z` is metres.
    pub fn project(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        (self.a() * (x * c - y * s) + self.ox, self.b() * (x * s + y * c) - z * self.rows_per_metre() + self.oy)
    }

    /// Screen cell anchoring a tile: its centre projected and floored.
    pub fn project_tile(&self, mx: i32, my: i32, z: i32) -> (i32, i32) {
        let (sx, sy) = self.project(mx as f32 + 0.5, my as f32 + 0.5, z as f32);
        (sx.floor() as i32, sy.floor() as i32)
    }

    /// World point at height `z` metres under a screen position.
    pub fn unproject(&self, sx: f32, sy: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        let u = (sx - self.ox) / self.a();
        let v = (sy - self.oy + z * self.rows_per_metre()) / self.b();
        (u * c + v * s, -u * s + v * c)
    }

    /// Unit vector pointing toward the camera in map space.
    pub fn forward(&self) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        (s, c)
    }

    /// Depth of a tile's centre toward the camera, pushed to its near edge
    /// so a sprite standing on it sorts in front of the tile top.
    pub fn tile_depth(&self, mx: i32, my: i32) -> f32 {
        let (fx, fy) = self.forward();
        (mx as f32 + 0.5) * fx + (my as f32 + 0.5) * fy + 0.5 * (fx.abs() + fy.abs())
    }

    /// Where a tile at drawn height `z` sits on screen.
    pub fn anchor(&self, mx: i32, my: i32, z: i32) -> Anchor {
        self.anchor_f(mx, my, z as f32)
    }

    /// Anchor at a fractional surface height.
    pub fn anchor_f(&self, mx: i32, my: i32, z: f32) -> Anchor {
        let (sx, sy) = self.project(mx as f32 + 0.5, my as f32 + 0.5, z);
        Anchor { mx, my, z, sx: sx.floor() as i32, sy: sy.floor() as i32, depth: self.tile_depth(mx, my) }
    }

    /// Virtual camera altitude in metres for cloud parallax; higher when
    /// zoomed out.
    pub fn altitude(&self) -> f32 {
        match self.hw {
            0..=2 => 100.0,
            3..=4 => 130.0,
            _ => 240.0,
        }
    }

    /// Map tile nearest the centre of the screen at sea level.
    pub fn center_tile(&self, map: &Map, sw: i32, sh: i32) -> (i32, i32) {
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, SEA as f32);
        map.clamp(x.floor() as i32, y.floor() as i32)
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

    /// The map step a walk key makes: in screen space the figure moves that
    /// way on screen, which is a diagonal in map space; along the map axes
    /// the key is the step itself.
    pub fn walk_step(&self, screen_space: bool, dx: i32, dy: i32) -> (i32, i32) {
        if screen_space {
            self.screen_dir_to_map(dx, dy)
        } else {
            (dx, dy)
        }
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

    /// Slide the view by whole tile footprints: positive `dx` shows more of
    /// the map to the left, positive `dy` more above.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        self.ox += (dx * 2 * self.hw) as f32;
        self.oy += (dy * 2 * self.hh) as f32;
    }

    /// Recentre on the player when they leave the middle of the screen.
    pub fn follow(&mut self, world: &World, map: &Map, sw: i32, sh: i32) {
        let Some(p) = world.player() else { return };
        let z = map.get(p.mx, p.my).map(|t| t.draw_z()).unwrap_or(0);
        let (sx, sy) = self.project_tile(p.mx, p.my, z);
        if sx < sw / 5 || sx > sw * 4 / 5 || sy < sh / 5 || sy > sh * 4 / 5 {
            self.look_at(p.mx, p.my, map, sw, sh);
        }
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    pub fn set_zoom(&mut self, zoom: usize, sw: i32, sh: i32) {
        let z = self.focus_z;
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, z);
        self.zoom = zoom % ZOOMS.len();
        self.hw = ZOOMS[self.zoom].0;
        self.hh = ZOOMS[self.zoom].1;
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Step through the zoom levels, wrapping at either end.
    pub fn zoom_by(&mut self, steps: i32, sw: i32, sh: i32) {
        let n = ZOOMS.len() as i32;
        self.set_zoom((self.zoom as i32 + steps).rem_euclid(n) as usize, sw, sh);
    }

    /// Turn by an angle about whatever is at the screen centre.
    pub fn rotate_by(&mut self, radians: f32, sw: i32, sh: i32) {
        let z = self.focus_z;
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, z);
        self.angle = (self.angle + radians).rem_euclid(2.0 * std::f32::consts::PI);
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Rotate by quarter turns.
    pub fn rotate(&mut self, steps: i32, sw: i32, sh: i32) {
        self.rotate_by(steps as f32 * FRAC_PI_2, sw, sh);
    }

    pub fn degrees(&self) -> i32 {
        (self.angle.to_degrees().round() as i32).rem_euclid(360)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_unproject_round_trip() {
        for angle in [0.0, 0.3, FRAC_PI_4, 1.2, 2.5, 4.0] {
            for zoom in 0..ZOOMS.len() {
                let mut cam = Camera::new();
                cam.angle = angle;
                cam.set_zoom(zoom, 120, 40);
                cam.look_at_point(10.5, 7.5, 5.0, 120, 40);
                for &(x, y, z) in &[(0.0, 0.0, 0.0), (10.5, 7.5, 5.0), (-3.25, 12.0, 14.0), (40.0, -8.5, 3.0)] {
                    let (sx, sy) = cam.project(x, y, z);
                    let (bx, by) = cam.unproject(sx, sy, z);
                    assert!((bx - x).abs() < 1e-3 && (by - y).abs() < 1e-3, "angle {angle} zoom {zoom}: ({x}, {y}) -> ({bx}, {by})");
                }
            }
        }
    }

    #[test]
    fn screen_directions_give_the_eight_compass_steps() {
        // At the compass view with 2:1 tiles the eight screen directions
        // land on the eight distinct unit steps of the map.
        let mut cam = Camera::new();
        cam.set_zoom(0, 120, 40);
        let dirs = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
        let steps: Vec<(i32, i32)> = dirs.iter().map(|&(dx, dy)| cam.screen_dir_to_map(dx, dy)).collect();
        for (i, s) in steps.iter().enumerate() {
            assert!(s.0.abs() <= 1 && s.1.abs() <= 1 && *s != (0, 0), "{:?} -> {s:?}", dirs[i]);
            assert!(!steps[..i].contains(s), "{:?} repeats step {s:?}", dirs[i]);
        }
        assert_eq!(steps[0], (-1, -1), "screen up is map north-west");
        assert_eq!(steps[2], (1, -1), "screen right is map north-east");
        assert_eq!(steps[4], (1, 1));
        assert_eq!(steps[6], (-1, 1));
    }

    #[test]
    fn screen_space_steps_are_diagonal_and_map_axes_steps_cardinal() {
        let mut cam = Camera::new();
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                let (mx, my) = cam.walk_step(true, dx, dy);
                assert!(mx != 0 && my != 0, "zoom {zoom}: screen ({dx}, {dy}) -> ({mx}, {my}) is not a diagonal");
                assert_eq!(cam.walk_step(false, dx, dy), (dx, dy), "map axes keep the key's own step");
            }
        }
    }

    #[test]
    fn nearer_tiles_have_greater_depth_at_every_angle() {
        let tiles = [(0, 0), (3, 1), (-2, 5), (7, -4), (10, 10), (-6, -6)];
        let mut cam = Camera::new();
        for i in 0..64 {
            cam.angle = i as f32 * std::f32::consts::TAU / 64.0;
            let (fx, fy) = cam.forward();
            for &a in &tiles {
                for &b in &tiles {
                    let toward = (a.0 - b.0) as f32 * fx + (a.1 - b.1) as f32 * fy;
                    if toward.abs() < 1e-3 {
                        continue;
                    }
                    let nearer = cam.tile_depth(a.0, a.1) > cam.tile_depth(b.0, b.1);
                    assert_eq!(nearer, toward > 0.0, "angle {} tiles {a:?} {b:?}", cam.degrees());
                }
            }
        }
    }

    #[test]
    fn zoom_steps_wrap_both_ways() {
        let mut cam = Camera::new();
        cam.zoom_by(-1, 120, 40);
        assert_eq!(cam.zoom, ZOOMS.len() - 1);
        cam.zoom_by(1, 120, 40);
        assert_eq!(cam.zoom, 0);
    }

    #[test]
    fn each_zoom_is_an_exact_halving_of_the_next_in_rows_and_columns_per_metre() {
        // far 2x1 (1:8), mid 4x1 (1:4), near 8x2 (1:2), close 16x4 (1:1).
        let expected = [(0.75, 1.414), (1.5, 2.828), (3.0, 5.657), (6.0, 11.314)];
        assert_eq!(ZOOMS.len(), expected.len());
        let mut cam = Camera::new();
        for (zoom, (rows, columns)) in expected.iter().enumerate() {
            cam.set_zoom(zoom, 120, 40);
            assert_eq!(cam.rows_per_metre(), *rows, "zoom {zoom} rows per metre");
            assert!((cam.columns_per_metre() - columns).abs() < 1e-3, "zoom {zoom} columns per metre: {}", cam.columns_per_metre());
            if zoom > 0 {
                let (below_rows, below_columns) = expected[zoom - 1];
                assert_eq!(*rows, below_rows * 2.0, "zoom {zoom} is twice the zoom below");
                assert!((columns - below_columns * 2.0).abs() < 1e-3);
            }
        }
        assert_eq!(cam.zoom_name(), ("close", "1:1"));
    }

    #[test]
    fn the_inset_zoom_is_the_other_end_of_the_scale_biased_toward_the_close_view() {
        // Zoomed out at all — 1:2, 1:4 or 1:8 — the inset shows 1:1; only
        // at 1:1 does it show 1:8. The two views never share a level.
        let close = ZOOMS.len() - 1;
        for main in 0..ZOOMS.len() {
            let inset = Camera::inset_zoom(main);
            assert_eq!(inset, if main == close { 0 } else { close }, "main zoom {main}");
            assert_ne!(inset, main, "the two views never share a level");
        }
        let mut cam = Camera::new();
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            let (_, ratio) = cam.zoom_name();
            let want = if ratio == "1:1" { "1:8" } else { "1:1" };
            assert_eq!(Camera::inset_ratio(zoom), want, "main {ratio}");
        }
        assert_eq!([0, 1, 2, 3].map(Camera::inset_ratio), ["1:1", "1:1", "1:1", "1:8"]);
    }

    #[test]
    fn a_two_metre_person_is_twelve_rows_at_one_to_one_and_halves_with_every_zoom_out() {
        let mut cam = Camera::new();
        let person = 2.0;
        let rows: Vec<f32> = (0..ZOOMS.len())
            .map(|zoom| {
                cam.set_zoom(zoom, 120, 40);
                person * cam.rows_per_metre()
            })
            .collect();
        assert_eq!(rows, vec![1.5, 3.0, 6.0, 12.0]);
    }

    #[test]
    fn heights_project_through_rows_per_metre() {
        let mut cam = Camera::new();
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            cam.look_at_point(4.5, 4.5, 0.0, 120, 40);
            let ground = cam.project(4.5, 4.5, 0.0);
            for metres in [1.0, 2.0, 18.0, -3.0] {
                let up = cam.project(4.5, 4.5, metres);
                assert!((up.0 - ground.0).abs() < 1e-4, "zoom {zoom}: height does not move a point sideways");
                assert!(((ground.1 - up.1) - metres * cam.rows_per_metre()).abs() < 1e-3, "zoom {zoom}: {metres} m is {} rows", ground.1 - up.1);
                // And back: the ground point under that screen position at
                // the same height is where it started.
                let (bx, by) = cam.unproject(up.0, up.1, metres);
                assert!((bx - 4.5).abs() < 1e-3 && (by - 4.5).abs() < 1e-3, "zoom {zoom}: round trip at {metres} m");
            }
        }
    }

    #[test]
    fn one_keypress_is_one_tile_at_every_zoom_and_the_tile_is_the_footprint() {
        let mut cam = Camera::new();
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                for screen_space in [true, false] {
                    let (mx, my) = cam.walk_step(screen_space, dx, dy);
                    assert!(mx.abs() <= 1 && my.abs() <= 1 && (mx, my) != (0, 0), "zoom {zoom}: ({dx}, {dy}) is one tile, not ({mx}, {my})");
                    // One tile of the map is one tile footprint on screen,
                    // whatever the zoom: a block at 1:1, half a block at 1:2.
                    let from = cam.project(6.5, 6.5, 0.0);
                    let to = cam.project(6.5 + mx as f32, 6.5 + my as f32, 0.0);
                    let (ox, oy) = ((to.0 - from.0).abs(), (to.1 - from.1).abs());
                    let footprint = |v: f32, half: i32| (v - half as f32).abs() < 1e-3 || (v - 2.0 * half as f32).abs() < 1e-3 || v < 1e-3;
                    assert!(footprint(ox, cam.hw), "zoom {zoom}: {ox} columns is not a footprint of {}", cam.hw);
                    assert!(footprint(oy, cam.hh), "zoom {zoom}: {oy} rows is not a footprint of {}", cam.hh);
                    assert!(ox + oy > 1e-3);
                }
            }
        }
    }

    #[test]
    fn anchor_depth_matches_tile_depth() {
        let cam = Camera::new();
        let a = cam.anchor(3, 4, 5);
        assert_eq!(a.depth, cam.tile_depth(3, 4));
        assert_eq!((a.sx, a.sy), cam.project_tile(3, 4, 5));
    }
}
