//! The camera (ADR-007): a yaw and a pitch, a field of view, a scale and a
//! screen offset. It is the one place that maps the world to the screen:
//! projection takes a world point to a screen cell, unprojection walks back
//! to the ground at a chosen height, and the walk asks it for the ray
//! through a cell.
//!
//! A field of view of zero is an orthographic view, which projects through
//! a basis of three numbers: columns per tile across the screen, rows per
//! tile of ground depth toward the camera and rows per metre of height.
//! The isometric mode builds that basis from a footprint preset
//! (`Camera::isometric`), so its pitch is whatever the footprint implies —
//! 25.24 degrees above the horizon for the 4:1 footprints and 43.31 for
//! the far zoom's 2x1 — and its scales are ADR-004's, each zoom an exact
//! halving of the next: `columns_per_metre` across the ground and
//! `rows_per_metre` up the screen. Heights project through the second, so
//! a 2 m person is 12 rows at 1:1 and 1.5 at 1:8.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use crate::map::{Map, SEA, TILE_CM, TILE_METRES};
use crate::tileset::{ZOOMS, ZOOM_NAMES, ZOOM_RATIOS};
use crate::world::{Entity, World};

/// The three numbers an orthographic projection multiplies by: the scale
/// and the pitch in the form the formulas use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    /// Columns a tile spans across the screen: `columns * TILE_METRES`.
    pub cols: f32,
    /// Rows a tile of ground depth toward the camera spans:
    /// `rows * sin(pitch) * TILE_METRES`.
    pub rows: f32,
    /// Rows a metre of height spans: `rows * cos(pitch)`.
    pub rise: f32,
}

impl Basis {
    /// The pitch above the horizon this basis implies: a metre of ground
    /// depth is `rows / TILE_METRES` rows and a metre of height `rise`.
    pub fn pitch(&self) -> f32 {
        (self.rows / TILE_METRES).atan2(self.rise)
    }

    /// The scale this basis implies, per metre.
    pub fn scale(&self) -> Scale {
        Scale { columns: self.cols / TILE_METRES, rows: (self.rows / TILE_METRES).hypot(self.rise) }
    }
}

/// A camera's scale: columns per metre across the screen and rows per
/// metre along its vertical. `Camera::rows_per_metre` is the height
/// component of the second, `cos(pitch)` of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub columns: f32,
    pub rows: f32,
}

/// The ray through a screen cell, as the walk marches it: the ground
/// point at height zero and the tiles that point moves per metre of
/// height, `p(z) = p0 + d * z`. Every orthographic ray has the same drift;
/// a perspective ray (ADR-007 stage 2) has its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub p0: (f32, f32),
    pub d: (f32, f32),
}

pub struct Camera {
    /// Yaw in radians; pi/4 is the classic compass view. Its sine and
    /// cosine are cached in `yaw`, so it is set through `set_angle`.
    angle: f32,
    yaw: (f32, f32),
    /// Pitch in radians above the horizon. The isometric constructor
    /// derives it from the basis; a camera built from a pitch
    /// (`Camera::orthographic`) derives the basis from it.
    pub pitch: f32,
    /// Field of view in radians across the screen; zero is an orthographic
    /// view, which is every camera until ADR-007 stage 2.
    pub fov: f32,
    pub ox: f32,
    pub oy: f32,
    /// The zoom preset: its index in `ZOOMS` and its footprint. A camera
    /// built from a scale rather than a preset carries the preset nearest
    /// its rows per metre, for what is keyed by zoom.
    pub zoom: usize,
    pub hw: i32,
    pub hh: i32,
    /// Height of the point the screen centre was last aimed at, so zoom and
    /// rotation pivot about it.
    pub focus_z: f32,
    basis: Basis,
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

/// Where screen cells meet the cloud plane for one frame: a ray from a
/// virtual camera of height C metres through the ground point under a cell,
/// raised by the rows the cloud altitude H is worth, meets the plane at
/// altitude H at that point pulled toward the screen centre by 1 - H/C.
/// Panning therefore moves clouds by C/(C - H) relative to the ground.
/// The virtual camera becomes the real eye in ADR-007 stage 2.
pub struct CloudView {
    cx: f32,
    cy: f32,
    k: f32,
    rows: f32,
}

impl CloudView {
    /// Cloud-plane point sampled at screen column `sx` (a cell's centre)
    /// and row `sy` (a cell's top edge).
    pub fn sample(&self, cam: &Camera, sx: f32, sy: f32) -> (f32, f32) {
        let (gx, gy) = cam.unproject(sx, sy + self.rows, 0.0);
        (self.cx + (gx - self.cx) * self.k, self.cy + (gy - self.cy) * self.k)
    }
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

impl Camera {
    /// The camera modes the `camera` settings row offers, in its order.
    /// `perspective` joins in ADR-007 stage 2.
    pub const MODES: [&'static str; 1] = ["isometric"];

    /// The far preset at the compass view.
    pub fn new() -> Camera {
        Camera::isometric(0)
    }

    /// The isometric mode: the orthographic camera a footprint preset
    /// implies, at the compass view, with no offset. The basis is the
    /// preset's own numbers — `hw * sqrt 2` columns and `hh * sqrt 2` rows
    /// per tile, `3 hw / 8` rows per metre — and the pitch is what they
    /// imply.
    pub fn isometric(zoom: usize) -> Camera {
        let mut cam = Camera { angle: 0.0, yaw: (0.0, 1.0), pitch: 0.0, fov: 0.0, ox: 0.0, oy: 0.0, zoom: 0, hw: 0, hh: 0, focus_z: SEA as f32, basis: Basis { cols: 1.0, rows: 1.0, rise: 1.0 } };
        cam.set_angle(FRAC_PI_4);
        cam.preset(zoom);
        cam
    }

    /// An orthographic camera from a yaw, a pitch and a scale, at no
    /// offset: the general form the isometric presets are instances of.
    /// It carries the preset nearest its rows per metre for what is keyed
    /// by zoom.
    pub fn orthographic(yaw: f32, pitch: f32, scale: Scale) -> Camera {
        let (s, c) = pitch.sin_cos();
        let basis = Basis { cols: scale.columns * TILE_METRES, rows: scale.rows * s * TILE_METRES, rise: scale.rows * c };
        let nearest = (0..ZOOMS.len()).min_by(|&i, &j| {
            let d = |z: usize| (Camera::isometric(z).rows_per_metre() - basis.rise).abs();
            d(i).total_cmp(&d(j))
        });
        let mut cam = Camera::isometric(nearest.unwrap_or(0));
        cam.set_angle(yaw);
        cam.pitch = pitch;
        cam.basis = basis;
        cam
    }

    /// Switch to a footprint preset in place: its index, footprint, basis
    /// and pitch. The offset is left to the caller.
    fn preset(&mut self, zoom: usize) {
        self.zoom = zoom % ZOOMS.len();
        (self.hw, self.hh) = ZOOMS[self.zoom];
        self.basis = Basis { cols: self.hw as f32 * SQRT_2, rows: self.hh as f32 * SQRT_2, rise: self.hw as f32 * 3.0 / 8.0 };
        self.pitch = self.basis.pitch();
    }

    /// Largest zoom at which the whole map fits the screen, else the
    /// smallest. The relief of the map itself, not the world's ceiling,
    /// sets how many rows the terrain needs above its footprint.
    pub fn fitting_zoom(map: &Map, sw: i32, sh: i32) -> usize {
        let n = map.w.max(map.h) as i32;
        let relief = map.relief_ceiling() as f32;
        (0..ZOOMS.len())
            .rev()
            .find(|&z| {
                let cam = Camera::isometric(z);
                let (fw, fh) = cam.footprint();
                n * fw <= sw && n * fh + (relief * cam.rows_per_metre()).ceil() as i32 + 4 <= sh
            })
            .unwrap_or(0)
    }

    pub fn angle(&self) -> f32 {
        self.angle
    }

    /// Set the yaw, caching its sine and cosine for every projection.
    pub fn set_angle(&mut self, radians: f32) {
        self.angle = radians;
        self.yaw = radians.sin_cos();
    }

    /// The orthographic basis: the numbers the projection multiplies by.
    pub fn basis(&self) -> Basis {
        self.basis
    }

    /// The scale per metre, across the screen and along its vertical.
    pub fn scale(&self) -> Scale {
        self.basis.scale()
    }

    /// Columns a tile spans across the screen.
    pub fn a(&self) -> f32 {
        self.basis.cols
    }

    /// Rows a tile of ground depth toward the camera spans.
    pub fn b(&self) -> f32 {
        self.basis.rows
    }

    /// Screen columns a metre of ground spans: the tile's own scale, since
    /// a tile is `TILE_METRES` square.
    pub fn columns_per_metre(&self) -> f32 {
        self.a() / TILE_METRES
    }

    /// Screen rows a metre of height draws as. In the isometric mode that
    /// is three rows per eight columns of half width, so a 2 m person is
    /// 12 rows at 1:1 and halves with every zoom out. On 1:2 cells that is
    /// 96 pixels of height against 90 pixels of ground per metre, so
    /// vertical and horizontal scale agree. Level of detail and the sprite
    /// tiers key off it.
    pub fn rows_per_metre(&self) -> f32 {
        self.basis.rise
    }

    /// The columns and rows a tile's footprint spans at the compass view:
    /// the cells the view slides by on a pan, and the lattice the ground
    /// texture hashes on.
    pub fn footprint(&self) -> (i32, i32) {
        (2 * self.hw, 2 * self.hh)
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
        if main % ZOOMS.len() == close {
            0
        } else {
            close
        }
    }

    /// The ratio the inset draws at while the main view is at `main`.
    pub fn inset_ratio(main: usize) -> &'static str {
        ZOOM_RATIOS[Camera::inset_zoom(main)]
    }

    /// The inset's camera: this view's heading at the other end of the
    /// zoom scale, for the caller to aim at the player.
    pub fn inset(&self) -> Camera {
        let mut cam = Camera::isometric(Camera::inset_zoom(self.zoom));
        cam.set_angle(self.angle);
        cam
    }

    /// Screen position of a world point; `z` is metres.
    pub fn project(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.yaw;
        (self.a() * (x * c - y * s) + self.ox, self.b() * (x * s + y * c) - z * self.rows_per_metre() + self.oy)
    }

    /// Screen displacement, in columns and rows, of a world displacement
    /// of `run` tiles of ground and `rise` metres of height.
    pub fn project_vector(&self, run: (f32, f32), rise: f32) -> (f32, f32) {
        let (s, c) = self.yaw;
        (self.a() * (run.0 * c - run.1 * s), self.b() * (run.0 * s + run.1 * c) - rise * self.rows_per_metre())
    }

    /// Screen cell anchoring a tile: its centre projected and floored.
    pub fn project_tile(&self, mx: i32, my: i32, z: i32) -> (i32, i32) {
        let (sx, sy) = self.project(mx as f32 + 0.5, my as f32 + 0.5, z as f32);
        (sx.floor() as i32, sy.floor() as i32)
    }

    /// World point at height `z` metres under a screen position.
    pub fn unproject(&self, sx: f32, sy: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.yaw;
        let u = (sx - self.ox) / self.a();
        let v = (sy - self.oy + z * self.rows_per_metre()) / self.b();
        (u * c + v * s, -u * s + v * c)
    }

    /// The ray through a screen position: the ground point under it at
    /// height zero and the drift of that point per metre of height, which
    /// is exactly what keeps the screen position fixed as the walk
    /// descends. An orthographic view's rays are parallel, so the drift
    /// is the camera's; a perspective view's come from the eye.
    pub fn ray(&self, sx: f32, sy: f32) -> Ray {
        let p0 = self.unproject(sx, sy, 0.0);
        let (s, c) = self.yaw;
        let b = self.b();
        let rpm = self.rows_per_metre();
        Ray { p0, d: (s * rpm / b, c * rpm / b) }
    }

    /// Unit vector pointing toward the camera in map space.
    pub fn forward(&self) -> (f32, f32) {
        self.yaw
    }

    /// Unit vector pointing screen-right in map space.
    pub fn right(&self) -> (f32, f32) {
        let (s, c) = self.yaw;
        (c, -s)
    }

    /// Depth of a map point toward the camera, for the sprite sort.
    pub fn depth(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = self.yaw;
        x * fx + y * fy
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
        self.anchor_at(mx as f32 + 0.5, my as f32 + 0.5, z)
    }

    /// Anchor a point of the map at a surface height: its cell is the
    /// point projected and floored, and its depth is its tile's, so a
    /// figure anywhere in a tile sorts in front of that tile's ground.
    pub fn anchor_at(&self, x: f32, y: f32, z: f32) -> Anchor {
        let (mx, my) = (x.floor() as i32, y.floor() as i32);
        let (sx, sy) = self.project(x, y, z);
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

    /// Where the screen cells of a `w` by `h` view meet the cloud plane
    /// this frame; see `CloudView`.
    pub fn cloud_view(&self, w: i32, h: i32) -> CloudView {
        let altitude = World::CLOUD_ALTITUDE;
        let c = self.altitude();
        let k = 1.0 - altitude / c;
        let (cx, cy) = self.unproject(w as f32 / 2.0, h as f32 / 2.0, 0.0);
        let rows = altitude * self.rows_per_metre();
        CloudView { cx, cy, k, rows }
    }

    /// Map tile nearest the centre of the screen at sea level.
    pub fn center_tile(&self, map: &Map, sw: i32, sh: i32) -> (i32, i32) {
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, SEA as f32);
        map.clamp(x.floor() as i32, y.floor() as i32)
    }

    /// The target: the map point under the centre of a `sw` by `sh` screen
    /// at the height the view was last aimed at, which zoom and rotation
    /// pivot about.
    pub fn focus(&self, sw: i32, sh: i32) -> (f32, f32) {
        self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, self.focus_z)
    }

    /// The ground under a screen displacement, in tiles: the inverse
    /// projection of `dx` columns and `dy` rows at a fixed height.
    pub fn ground_vector(&self, dx: f32, dy: f32) -> (f32, f32) {
        let (s, c) = self.yaw;
        let u = dx / self.a();
        let v = dy / self.b();
        (u * c + v * s, -u * s + v * c)
    }

    /// Map step for a screen direction: the inverse projection of the
    /// direction, scaled so its larger component is one tile, rounded.
    pub fn screen_dir_to_map(&self, dx: i32, dy: i32) -> (i32, i32) {
        let (x, y) = self.ground_vector(dx as f32, dy as f32);
        let m = x.abs().max(y.abs()).max(1e-6);
        ((x / m).round() as i32, (y / m).round() as i32)
    }

    /// The centimetre step one keypress makes (ADR-006) for a figure at
    /// `from` (centimetres) standing at height `z`: one screen cell in the
    /// pressed direction — a column for left and right, a row for up and
    /// down — at this zoom, so a press moves the figure one visible cell
    /// and no more. In screen space the step goes to the centre of the
    /// next cell: the figure is drawn at its point floored to a cell, so
    /// from a cell centre that is the ground under one cell (8.8 cm a
    /// column at 1:1, 71 cm at 1:8), and from the exact boundary a spawn,
    /// a teleport or `c` leaves it on, half a cell or one and a half, after
    /// which every press lands within the rounding of a centimetre of a
    /// cell centre and never flips a boundary. Along the map axes the step
    /// is the ground length of the cell along the axis the key names.
    pub fn cell_step(&self, screen_space: bool, dx: i32, dy: i32, from: (i32, i32), z: f32) -> (i32, i32) {
        let cm = |tiles: f32| (tiles * TILE_CM as f32).round() as i32;
        if !screen_space {
            return (dx * cm(1.0 / self.a()), dy * cm(1.0 / self.b()));
        }
        let (sx, sy) = self.project(from.0 as f32 / TILE_CM as f32, from.1 as f32 / TILE_CM as f32, z);
        let (tx, ty) = self.unproject(sx.floor() + 0.5 + dx as f32, sy.floor() + 0.5 + dy as f32, z);
        (cm(tx) - from.0, cm(ty) - from.1)
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
        let (fw, fh) = self.footprint();
        self.ox += (dx * fw) as f32;
        self.oy += (dy * fh) as f32;
    }

    /// Place an entity's point at the centre of the screen, at the drawn
    /// height of its tile.
    pub fn look_at_entity(&mut self, e: &Entity, map: &Map, sw: i32, sh: i32) {
        let z = map.get(e.mx(), e.my()).map(|t| t.draw_z()).unwrap_or(SEA);
        let (x, y) = e.pos();
        self.look_at_point(x, y, z as f32, sw, sh);
    }

    /// Recentre on the player when they leave the middle of the screen.
    pub fn follow(&mut self, world: &World, map: &Map, sw: i32, sh: i32) {
        let Some(p) = world.player() else { return };
        let z = map.get(p.mx(), p.my()).map(|t| t.draw_z()).unwrap_or(0);
        let (x, y) = p.pos();
        let (sx, sy) = self.project(x, y, z as f32);
        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
        if sx < sw / 5 || sx > sw * 4 / 5 || sy < sh / 5 || sy > sh * 4 / 5 {
            self.look_at_entity(p, map, sw, sh);
        }
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    pub fn set_zoom(&mut self, zoom: usize, sw: i32, sh: i32) {
        let z = self.focus_z;
        let (x, y) = self.focus(sw, sh);
        self.preset(zoom);
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
        let (x, y) = self.focus(sw, sh);
        self.set_angle((self.angle + radians).rem_euclid(2.0 * std::f32::consts::PI));
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

    /// The projection as it was before ADR-007: the footprint's numbers
    /// worked out on every call. The general path must give these bits.
    struct Old {
        angle: f32,
        ox: f32,
        oy: f32,
        hw: i32,
        hh: i32,
    }

    impl Old {
        fn of(cam: &Camera) -> Old {
            Old { angle: cam.angle(), ox: cam.ox, oy: cam.oy, hw: cam.hw, hh: cam.hh }
        }

        fn scales(&self) -> (f32, f32, f32) {
            (self.hw as f32 * SQRT_2, self.hh as f32 * SQRT_2, self.hw as f32 * 3.0 / 8.0)
        }

        fn project(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
            let (s, c) = self.angle.sin_cos();
            let (a, b, rpm) = self.scales();
            (a * (x * c - y * s) + self.ox, b * (x * s + y * c) - z * rpm + self.oy)
        }

        fn unproject(&self, sx: f32, sy: f32, z: f32) -> (f32, f32) {
            let (s, c) = self.angle.sin_cos();
            let (a, b, rpm) = self.scales();
            let u = (sx - self.ox) / a;
            let v = (sy - self.oy + z * rpm) / b;
            (u * c + v * s, -u * s + v * c)
        }
    }

    const ANGLES: [f32; 7] = [0.0, 0.3, FRAC_PI_4, 1.2, 2.5, 4.0, 5.9];

    #[test]
    fn the_general_projection_gives_the_old_formulas_bits_at_every_zoom_and_angle() {
        // A lattice of points in a box a hundred tiles across and the
        // world's whole vertical band, projected and unprojected through
        // the basis, is the old arithmetic to the bit.
        let mut cam = Camera::new();
        for angle in ANGLES {
            cam.set_angle(angle);
            for (zoom, &(hw, hh)) in ZOOMS.iter().enumerate() {
                cam.set_zoom(zoom, 168, 71);
                cam.look_at_point(10.5, 7.5, 5.0, 168, 71);
                let old = Old::of(&cam);
                assert_eq!((cam.a(), cam.b(), cam.rows_per_metre()), old.scales());
                assert_eq!((old.hw, old.hh), (hw, hh));
                for i in -10..=10 {
                    for j in -10..=10 {
                        for z in [-12.0, 0.0, 0.37, 5.0, 17.5, 120.0] {
                            let (x, y) = (i as f32 * 5.25, j as f32 * 4.75);
                            assert_eq!(cam.project(x, y, z), old.project(x, y, z), "project at angle {angle} zoom {zoom}: ({x}, {y}, {z})");
                            let (sx, sy) = (i as f32 * 8.5 + 0.5, j as f32 * 3.5 + 0.5);
                            assert_eq!(cam.unproject(sx, sy, z), old.unproject(sx, sy, z), "unproject at angle {angle} zoom {zoom}: ({sx}, {sy}, {z})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_ray_is_the_old_walks_ray_to_the_bit() {
        let mut cam = Camera::new();
        for angle in ANGLES {
            cam.set_angle(angle);
            for zoom in 0..ZOOMS.len() {
                cam.set_zoom(zoom, 120, 40);
                cam.look_at_point(3.5, 9.5, 2.0, 120, 40);
                let (s, c) = angle.sin_cos();
                let (b, rpm) = (cam.b(), cam.rows_per_metre());
                for (sx, sy) in [(0.5, 0.5), (60.5, 20.5), (119.5, 39.5), (17.5, 2.5)] {
                    let ray = cam.ray(sx, sy);
                    assert_eq!(ray.p0, cam.unproject(sx, sy, 0.0));
                    assert_eq!(ray.d, (s * rpm / b, c * rpm / b), "angle {angle} zoom {zoom}");
                    // Every point of the ray projects back to the cell.
                    for z in [0.0, 7.0, 40.0] {
                        let (px, py) = cam.project(ray.p0.0 + ray.d.0 * z, ray.p0.1 + ray.d.1 * z, z);
                        assert!((px - sx).abs() < 1e-3 && (py - sy).abs() < 1e-3, "angle {angle} zoom {zoom} at {z} m: ({px}, {py}) for ({sx}, {sy})");
                    }
                }
            }
        }
    }

    #[test]
    fn the_isometric_pitch_is_what_the_footprint_implies() {
        // tan(pitch) = 4 sqrt 2 hh / (3 hw): 25.24 degrees for the three
        // 4:1 footprints and 43.31 for the far zoom's 2x1, which has always
        // been the steeper view. Building the general orthographic camera
        // from that pitch and the preset's scale gives the preset's basis
        // back, within a few ulps.
        let four_to_one = (SQRT_2 / 3.0).atan();
        let two_to_one = (2.0 * SQRT_2 / 3.0).atan();
        assert!((four_to_one.to_degrees() - 25.24).abs() < 0.01 && (two_to_one.to_degrees() - 43.31).abs() < 0.01);
        for (zoom, &(hw, hh)) in ZOOMS.iter().enumerate() {
            let cam = Camera::isometric(zoom);
            let want = (4.0 * SQRT_2 * hh as f32 / (3.0 * hw as f32)).atan();
            assert!((cam.pitch - want).abs() < 1e-6, "zoom {zoom}: pitch {} for {hw}x{hh}", cam.pitch.to_degrees());
            assert!((cam.pitch - if hw == 2 * hh { two_to_one } else { four_to_one }).abs() < 1e-6, "zoom {zoom}");
            assert_eq!(cam.fov, 0.0, "every preset is orthographic");
            let general = Camera::orthographic(cam.angle(), cam.pitch, cam.scale());
            let (got, want) = (general.basis(), cam.basis());
            assert!((got.cols - want.cols).abs() < 1e-5 && (got.rows - want.rows).abs() < 1e-5 && (got.rise - want.rise).abs() < 1e-5, "zoom {zoom}: {got:?} vs {want:?}");
            assert_eq!((general.zoom, general.hw, general.hh), (zoom, hw, hh), "the general camera carries the nearest preset");
            assert!((general.pitch - cam.pitch).abs() < 1e-6 && (general.scale().rows - cam.scale().rows).abs() < 1e-5);
        }
        // A top-down view has all its rows in ground depth and none in
        // height; a view along the ground the reverse.
        let down = Camera::orthographic(0.0, FRAC_PI_2, Scale { columns: 4.0, rows: 2.0 });
        assert!((down.b() - 2.0 * TILE_METRES).abs() < 1e-6 && down.rows_per_metre().abs() < 1e-6);
        let along = Camera::orthographic(0.0, 0.0, Scale { columns: 4.0, rows: 2.0 });
        assert!(along.b().abs() < 1e-6 && (along.rows_per_metre() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_inset_camera_keeps_the_heading_at_the_other_end_of_the_scale() {
        let mut main = Camera::new();
        main.set_angle(1.2);
        for zoom in 0..ZOOMS.len() {
            main.set_zoom(zoom, 120, 40);
            let inset = main.inset();
            assert_eq!(inset.zoom, Camera::inset_zoom(zoom));
            assert_eq!(inset.angle(), 1.2);
            assert_eq!(inset.forward(), main.forward());
        }
    }

    #[test]
    fn screen_vectors_and_depth_come_from_the_yaw() {
        let mut cam = Camera::new();
        for angle in ANGLES {
            cam.set_angle(angle);
            cam.set_zoom(2, 120, 40);
            let (s, c) = angle.sin_cos();
            assert_eq!(cam.forward(), (s, c));
            assert_eq!(cam.right(), (c, -s));
            assert_eq!(cam.depth(3.5, -2.25), 3.5 * s + -2.25 * c);
            // A world displacement projects as the difference of two
            // projections, and a unit of ground toward the camera is b
            // rows down the screen.
            let (dx, dy) = cam.project_vector((1.5, -0.5), 2.0);
            let (ax, ay) = cam.project(1.5, -0.5, 2.0);
            let (ox, oy) = cam.project(0.0, 0.0, 0.0);
            assert!((dx - (ax - ox)).abs() < 1e-4 && (dy - (ay - oy)).abs() < 1e-4, "angle {angle}");
            let toward = cam.project_vector(cam.forward(), 0.0);
            assert!(toward.0.abs() < 1e-4 && (toward.1 - cam.b()).abs() < 1e-4, "angle {angle}: {toward:?}");
            let across = cam.project_vector(cam.right(), 0.0);
            assert!((across.0 - cam.a()).abs() < 1e-4 && across.1.abs() < 1e-4, "angle {angle}: {across:?}");
        }
    }

    #[test]
    fn the_footprint_is_the_cells_a_pan_slides_by() {
        let mut cam = Camera::new();
        for (zoom, &(hw, hh)) in ZOOMS.iter().enumerate() {
            cam.set_zoom(zoom, 120, 40);
            assert_eq!(cam.footprint(), (2 * hw, 2 * hh));
            let (ox, oy) = (cam.ox, cam.oy);
            cam.pan(1, -2);
            assert_eq!((cam.ox - ox, cam.oy - oy), ((2 * hw) as f32, (-4 * hh) as f32));
        }
    }

    #[test]
    fn cloud_sample_moves_by_c_over_c_minus_h_per_tile_of_pan() {
        let (w, h) = (120, 40);
        for zoom in 0..2 {
            let mut cam = Camera::new();
            cam.set_zoom(zoom, w, h);
            cam.look_at_point(0.0, 0.0, 0.0, w, h);
            let ratio = cam.altitude() / (cam.altitude() - World::CLOUD_ALTITUDE);
            let (sx, sy) = (33.5, 12.0);
            let before = cam.cloud_view(w, h).sample(&cam, sx, sy);
            let ground_before = cam.unproject(sx, sy, 0.0);
            cam.pan(1, 0);
            let view = cam.cloud_view(w, h);
            // The ground under a cell has moved by one tile footprint.
            let cells = cam.footprint().0 as f32;
            let ground_after = cam.unproject(sx + cells, sy, 0.0);
            assert!((ground_after.0 - ground_before.0).abs() < 1e-3 && (ground_after.1 - ground_before.1).abs() < 1e-3);
            // The cloud point that was under the cell is now C/(C - H) times as far along.
            let after = view.sample(&cam, sx + cells * ratio, sy);
            assert!((after.0 - before.0).abs() < 1e-3 && (after.1 - before.1).abs() < 1e-3, "zoom {zoom}: {before:?} vs {after:?}");
            let ground_speed = view.sample(&cam, sx + cells, sy);
            assert!((ground_speed.0 - before.0).abs() > 0.05, "zoom {zoom}: clouds move faster than the ground (ratio {ratio})");
        }
    }

    #[test]
    fn project_and_unproject_round_trip() {
        for angle in [0.0, 0.3, FRAC_PI_4, 1.2, 2.5, 4.0] {
            for zoom in 0..ZOOMS.len() {
                let mut cam = Camera::new();
                cam.set_angle(angle);
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
                let (x, y) = cam.cell_step(true, dx, dy, (1300, 1300), 0.0);
                assert!(x != 0 && y != 0, "zoom {zoom}: screen ({dx}, {dy}) -> ({x}, {y}) cm is not a diagonal");
                let (x, y) = cam.cell_step(false, dx, dy, (1300, 1300), 0.0);
                assert!((x == 0) != (y == 0), "zoom {zoom}: map axes ({dx}, {dy}) -> ({x}, {y}) cm is not cardinal");
                assert_eq!((x.signum(), y.signum()), (dx, dy), "map axes keep the key's own direction");
            }
        }
    }

    #[test]
    fn nearer_tiles_have_greater_depth_at_every_angle() {
        let tiles = [(0, 0), (3, 1), (-2, 5), (7, -4), (10, 10), (-6, -6)];
        let mut cam = Camera::new();
        for i in 0..64 {
            cam.set_angle(i as f32 * std::f32::consts::TAU / 64.0);
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
            assert_eq!(cam.scale().columns, cam.columns_per_metre());
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

    /// The screen cell a centimetre position is drawn in at height `z`.
    fn cell_of(cam: &Camera, x_cm: i32, y_cm: i32, z: f32) -> (i32, i32) {
        let (sx, sy) = cam.project(x_cm as f32 / TILE_CM as f32, y_cm as f32 / TILE_CM as f32, z);
        (sx.floor() as i32, sy.floor() as i32)
    }

    #[test]
    fn one_keypress_is_one_screen_cell_at_every_zoom_and_angle() {
        // A press moves the figure's cell one column sideways or one row up
        // or down the screen, whatever the zoom, however the camera is
        // turned and wherever the figure stands (ADR-006): from the exact
        // cell boundary a spawn leaves it on, from odd centimetres, at any
        // height, and on through a run of eight.
        let mut cam = Camera::new();
        for angle in [FRAC_PI_4, 0.0, 0.3, 1.2, 2.5, 4.0] {
            cam.set_angle(angle);
            for zoom in 0..ZOOMS.len() {
                cam.set_zoom(zoom, 120, 40);
                cam.look_at_point(6.5, 6.5, 5.0, 120, 40);
                for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                    for (start, z) in [((1300, 1300), 5.0), ((1300, 1300), 0.0), ((1337, 1201), 5.0), ((1313, 1300), 17.5), ((1300, 1319), 5.0)] {
                        let (mut px, mut py) = start;
                        for press in 1..=8 {
                            let (x, y) = cam.cell_step(true, dx, dy, (px, py), z);
                            assert!((x, y) != (0, 0), "zoom {zoom} angle {angle}: a press moves");
                            let before = cell_of(&cam, px, py, z);
                            (px, py) = (px + x, py + y);
                            let after = cell_of(&cam, px, py, z);
                            assert_eq!((after.0 - before.0, after.1 - before.1), (dx, dy), "zoom {zoom} angle {angle}: press {press} of ({dx}, {dy}) from {start:?} at {z} m");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_cell_step_halves_with_every_zoom_in_and_is_the_stated_table() {
        // The ground under one column is TILE_CM / (hw * sqrt 2) and under
        // one row TILE_CM / (hh * sqrt 2): 8.8, 17.7, 35.4, 70.7 cm and
        // 35.4, 70.7, 141, 141 cm from close to far. Columns halve exactly
        // between zooms; rows halve wherever the half height does, and far
        // and mid share a half height of one.
        let mut cam = Camera::new();
        let mut steps = Vec::new();
        let from = (1300, 1300);
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            let col = TILE_CM as f32 / cam.a();
            let row = TILE_CM as f32 / cam.b();
            steps.push((col, row));
            // Map axes: the rounded lengths along the axis the key names.
            assert_eq!(cam.cell_step(false, 1, 0, from, 0.0), (col.round() as i32, 0), "zoom {zoom}");
            assert_eq!(cam.cell_step(false, 0, -1, from, 0.0), (0, -(row.round() as i32)), "zoom {zoom}");
            // Screen space at the compass view, from a cell centre (the
            // second press): the same length within the rounding, diagonal.
            for ((dx, dy), len, signs) in [((1, 0), col, (1, -1)), ((0, 1), row, (1, 1))] {
                let (x, y) = cam.cell_step(true, dx, dy, from, 0.0);
                let centred = (from.0 + x, from.1 + y);
                let (x, y) = cam.cell_step(true, dx, dy, centred, 0.0);
                let diagonal = (x.signum(), y.signum()) == signs && (x.abs() - y.abs()).abs() <= 1;
                assert!((((x * x + y * y) as f32).sqrt() - len).abs() < 2.0 && diagonal, "zoom {zoom}: a cell ({dx}, {dy}) is ({x}, {y}) cm, not {len}");
            }
        }
        let rounded: Vec<(i32, i32)> = steps.iter().map(|(c, r)| (c.round() as i32, r.round() as i32)).collect();
        assert_eq!(rounded, [(71, 141), (35, 141), (18, 71), (9, 35)], "far, mid, near, close");
        for zoom in 1..ZOOMS.len() {
            let ((c, r), (c0, r0)) = (steps[zoom], steps[zoom - 1]);
            assert!((c * 2.0 - c0).abs() < 1e-3, "zoom {zoom}: a column is half the zoom below");
            let (hh, hh0) = (ZOOMS[zoom].1, ZOOMS[zoom - 1].1);
            assert!((r * hh as f32 / hh0 as f32 - r0).abs() < 1e-3, "zoom {zoom}: a row follows the half height");
        }
        // From a tile centre the figure sits on a cell boundary in both
        // axes, so the first press lands it at the centre of the next cell,
        // within the rounding of a centimetre; after it every press is one
        // cell of ground.
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            let (x, y) = cam.cell_step(true, 1, 0, from, 0.0);
            let (sx, sy) = cam.project((from.0 + x) as f32 / TILE_CM as f32, (from.1 + y) as f32 / TILE_CM as f32, 0.0);
            let (cols_per_cm, rows_per_cm) = (cam.a() / TILE_CM as f32, cam.b() / TILE_CM as f32);
            assert!((sx.rem_euclid(1.0) - 0.5).abs() <= cols_per_cm && (sy.rem_euclid(1.0) - 0.5).abs() <= rows_per_cm, "zoom {zoom}: the first press lands at ({sx}, {sy}), not at a cell centre");
        }
    }

    #[test]
    fn a_fractional_anchor_is_its_tiles_depth_and_the_centre_anchor_is_the_tile_anchor() {
        let mut cam = Camera::new();
        cam.set_zoom(3, 120, 40);
        let centre = cam.anchor_f(3, 4, 5.0);
        let same = cam.anchor_at(3.5, 4.5, 5.0);
        assert_eq!((same.sx, same.sy, same.depth, same.mx, same.my), (centre.sx, centre.sy, centre.depth, 3, 4));
        let corner = cam.anchor_at(3.98, 4.02, 5.0);
        assert_eq!((corner.mx, corner.my, corner.depth), (3, 4, centre.depth), "a point in the tile keeps the tile's depth");
        assert_ne!((corner.sx, corner.sy), (centre.sx, centre.sy), "but is drawn where it stands");
    }

    #[test]
    fn anchor_depth_matches_tile_depth() {
        let cam = Camera::new();
        let a = cam.anchor(3, 4, 5);
        assert_eq!(a.depth, cam.tile_depth(3, 4));
        assert_eq!((a.sx, a.sy), cam.project_tile(3, 4, 5));
    }
}
