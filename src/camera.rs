//! The isometric camera: a view angle, a screen offset and a tile footprint.
//! Projection maps a world point to a screen cell; unprojection walks back
//! to the ground at a chosen height.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use crate::map::{Map, MAX_Z, SEA};
use crate::tileset::ZOOMS;
use crate::world::World;

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
    pub z: i32,
    pub sx: i32,
    pub sy: i32,
    pub depth: f32,
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

    pub fn a(&self) -> f32 {
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
        let (sx, sy) = self.project_tile(mx, my, z);
        Anchor { mx, my, z, sx, sy, depth: self.tile_depth(mx, my) }
    }

    /// Virtual camera altitude in height units for cloud parallax; higher
    /// when zoomed out.
    pub fn altitude(&self) -> f32 {
        match self.hw {
            0..=2 => 48.0,
            3 => 64.0,
            _ => 120.0,
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
    fn zoom_steps_wrap_both_ways() {
        let mut cam = Camera::new();
        cam.zoom_by(-1, 120, 40);
        assert_eq!(cam.zoom, ZOOMS.len() - 1);
        cam.zoom_by(1, 120, 40);
        assert_eq!(cam.zoom, 0);
    }

    #[test]
    fn anchor_depth_matches_tile_depth() {
        let cam = Camera::new();
        let a = cam.anchor(3, 4, 5);
        assert_eq!(a.depth, cam.tile_depth(3, 4));
        assert_eq!((a.sx, a.sy), cam.project_tile(3, 4, 5));
    }
}
