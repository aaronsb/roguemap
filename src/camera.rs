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
//!
//! A positive field of view is a perspective view from an eye (stage 2 of
//! the ADR): the chase, shoulder and first-person modes each place the eye
//! from the character by a `Placement`, and the basis is the scale at the
//! character's depth, so level of detail and the sprite tiers keep one
//! answer per frame while every projection and ray comes from the eye.

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
/// a perspective ray has its own, the eye's line through the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub p0: (f32, f32),
    pub d: (f32, f32),
}

/// A perspective ray as the walk marches it by distance: the eye in tiles
/// and metres, and a unit direction in metres, `P(t) = eye + dir * t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeRay {
    pub eye: (f32, f32, f32),
    pub dir: (f32, f32, f32),
}

impl EyeRay {
    /// The ground point under the ray at `t` metres, in tiles.
    #[inline]
    pub fn ground(&self, t: f32) -> (f32, f32) {
        (self.eye.0 + self.dir.0 * t / TILE_METRES, self.eye.1 + self.dir.1 * t / TILE_METRES)
    }

    /// The ray's height at `t` metres.
    #[inline]
    pub fn height(&self, t: f32) -> f32 {
        self.eye.2 + self.dir.2 * t
    }

    /// Tiles the ground point moves per metre along the ray.
    #[inline]
    pub fn drift(&self) -> (f32, f32) {
        (self.dir.0 / TILE_METRES, self.dir.1 / TILE_METRES)
    }
}

/// How the camera is placed: the isometric presets, or a perspective eye
/// placed from the character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Isometric,
    /// Close behind and above the character, following them.
    Chase,
    /// Well back and high, off to one side, looking past the character's
    /// shoulder into the distance; the figure sits low and off-centre.
    Shoulder,
    /// At the character's eye; the character is not drawn.
    FirstPerson,
}

/// How a perspective mode places its eye from the point it is aimed at
/// (the character): the screen centre is that point pushed `ahead` metres
/// away from the camera along the ground and `lateral` metres to the
/// right, and the eye sits `distance` metres back from the centre along
/// the view direction, pitched `pitch` radians down. `fov` is the field of
/// view the mode defaults to; the `fov` settings row overrides it.
/// `visibility` scales the weather's visibility into the mode's fog
/// distance, since a narrow view into the distance wants a longer far
/// field than a close chase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub distance: f32,
    pub pitch: f32,
    pub fov: f32,
    pub lateral: f32,
    pub ahead: f32,
    pub visibility: f32,
}

impl Placement {
    /// The chase view: twelve metres back at thirty degrees, sixty degrees wide.
    pub const CHASE: Placement = Placement { distance: 12.0, pitch: 30.0 * DEG, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 1.0 };
    /// The over-the-shoulder view: thirty metres back at twenty degrees,
    /// the screen centre twelve metres past the character and three to the
    /// right, forty degrees wide so the far field reads at scale, and a
    /// fog distance half as long again.
    pub const SHOULDER: Placement = Placement { distance: 30.0, pitch: 20.0 * DEG, fov: 40.0 * DEG, lateral: 3.0, ahead: 12.0, visibility: 1.5 };
    /// First person: the eye itself, level, sixty degrees wide.
    pub const FIRST_PERSON: Placement = Placement { distance: 0.0, pitch: 0.0, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 1.0 };
}

const DEG: f32 = std::f32::consts::PI / 180.0;

#[derive(Clone, Copy)]
pub struct Camera {
    /// Yaw in radians; pi/4 is the classic compass view. Its sine and
    /// cosine are cached in `yaw`, so it is set through `set_angle`.
    angle: f32,
    yaw: (f32, f32),
    /// Pitch in radians above the horizon: positive looks down. The
    /// isometric constructor derives it from the basis; a camera built
    /// from a pitch (`Camera::orthographic`, the perspective modes)
    /// derives the basis from it.
    pub pitch: f32,
    /// Field of view in radians across the screen; zero is an orthographic
    /// view, and every isometric preset is one.
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
    mode: Mode,
    /// The perspective placement of the mode, with the field of view the
    /// `fov` settings row asked for in place of the preset's own.
    placement: Placement,
    /// The field of view the settings row asked for, if any, kept across a
    /// mode switch.
    fov_override: Option<f32>,
    /// The point a perspective camera was aimed at, in tiles and metres:
    /// the character, or wherever `look_at_point` was told to look.
    anchor: (f32, f32, f32),
    /// The point at the screen centre: the anchor pushed by the placement.
    target: (f32, f32, f32),
    /// The eye, in tiles and metres.
    eye: (f32, f32, f32),
    /// Focal length in columns: `(sw / 2) / tan(fov / 2)`.
    focal: f32,
    /// The screen the camera was last aimed on, so a mode switch can aim
    /// the new camera without being told again.
    screen: (i32, i32),
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

/// Where screen cells meet the cloud plane for one frame. Orthographic: a
/// ray from a virtual camera of height C metres through the ground point
/// under a cell, raised by the rows the cloud altitude H is worth, meets
/// the plane at altitude H at that point pulled toward the screen centre
/// by 1 - H/C, so panning moves clouds by C/(C - H) relative to the
/// ground. Perspective: the eye's own ray through the cell meets the plane
/// where it does, or not at all below the horizon.
pub struct CloudView {
    cx: f32,
    cy: f32,
    k: f32,
    rows: f32,
    /// A ray from the real eye rather than the virtual camera.
    eye: bool,
}

/// How far along a cloud ray the plane still counts, in metres: past it
/// the cells at the horizon would sample the field metres apart per cell
/// and read as noise.
const CLOUD_REACH: f32 = 2000.0;

impl CloudView {
    /// Cloud-plane point sampled at screen column `sx` (a cell's centre)
    /// and row `sy` (a cell's top edge); `None` where the cell's ray never
    /// meets the plane.
    pub fn sample(&self, cam: &Camera, sx: f32, sy: f32) -> Option<(f32, f32)> {
        if self.eye {
            let ray = cam.eye_ray(sx, sy);
            let dz = World::CLOUD_ALTITUDE - ray.eye.2;
            if ray.dir.2.abs() < 1e-6 || dz * ray.dir.2 <= 0.0 {
                return None;
            }
            let t = dz / ray.dir.2;
            return (t <= CLOUD_REACH).then(|| ray.ground(t));
        }
        let (gx, gy) = cam.unproject(sx, sy + self.rows, 0.0);
        Some((self.cx + (gx - self.cx) * self.k, self.cy + (gy - self.cy) * self.k))
    }
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

impl Camera {
    /// The camera modes the `camera` settings row offers, in its order:
    /// the isometric presets, then the perspective placements.
    pub const MODES: [&'static str; 4] = ["isometric", "chase", "shoulder", "first-person"];

    /// The height of a creature's eye as a fraction of its height: 1.7 m
    /// up a 2 m person.
    pub const EYE_HEIGHT: f32 = 0.85;

    /// The depth a first-person view states its scale at, in metres: who
    /// you face in an encounter.
    pub const FIRST_PERSON_DEPTH: f32 = 4.0;

    /// The screen a camera assumes until it is aimed on one.
    const DEFAULT_SCREEN: (i32, i32) = (120, 40);

    /// The pitch a perspective view may be turned to, either way.
    const PITCH_RANGE: (f32, f32) = (-80.0 * DEG, 85.0 * DEG);

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
        let mut cam = Camera {
            angle: 0.0,
            yaw: (0.0, 1.0),
            pitch: 0.0,
            fov: 0.0,
            ox: 0.0,
            oy: 0.0,
            zoom: 0,
            hw: 0,
            hh: 0,
            focus_z: SEA as f32,
            basis: Basis { cols: 1.0, rows: 1.0, rise: 1.0 },
            mode: Mode::Isometric,
            placement: Placement::CHASE,
            fov_override: None,
            anchor: (0.0, 0.0, 0.0),
            target: (0.0, 0.0, 0.0),
            eye: (0.0, 0.0, 0.0),
            focal: 0.0,
            screen: (0, 0),
        };
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
        let mut cam = Camera::isometric(Camera::nearest_zoom(basis.rise));
        cam.set_angle(yaw);
        cam.pitch = pitch;
        cam.basis = basis;
        cam
    }

    /// A perspective camera with its eye at a point, looking along `yaw`
    /// pitched `pitch` radians down, `fov` radians wide: the general form
    /// the chase, shoulder and first-person presets are instances of. The
    /// eye is the aimed point, as in the first-person mode; the screen is
    /// the default until the camera is aimed on one.
    pub fn perspective(eye: (f32, f32, f32), yaw: f32, pitch: f32, fov: f32) -> Camera {
        let mut cam = Camera::in_placement(Mode::FirstPerson, Placement { pitch, fov, ..Placement::FIRST_PERSON }, yaw);
        cam.anchor = eye;
        cam.aim();
        cam
    }

    /// The chase preset behind the character at the compass view.
    pub fn chase(yaw: f32) -> Camera {
        Camera::in_placement(Mode::Chase, Placement::CHASE, yaw)
    }

    /// The over-the-shoulder preset.
    pub fn shoulder(yaw: f32) -> Camera {
        Camera::in_placement(Mode::Shoulder, Placement::SHOULDER, yaw)
    }

    /// The first-person preset at the character's eye.
    pub fn first_person(yaw: f32) -> Camera {
        Camera::in_placement(Mode::FirstPerson, Placement::FIRST_PERSON, yaw)
    }

    fn in_placement(mode: Mode, placement: Placement, yaw: f32) -> Camera {
        let mut cam = Camera::isometric(ZOOMS.len() - 1);
        cam.mode = mode;
        cam.placement = placement;
        cam.pitch = placement.pitch;
        cam.fov = placement.fov;
        cam.screen = Camera::DEFAULT_SCREEN;
        cam.set_angle(yaw);
        cam.aim();
        cam
    }

    /// The placement a mode names; the isometric mode has none.
    fn placement_of(mode: Mode) -> Placement {
        match mode {
            Mode::Isometric | Mode::Chase => Placement::CHASE,
            Mode::Shoulder => Placement::SHOULDER,
            Mode::FirstPerson => Placement::FIRST_PERSON,
        }
    }

    /// This view in another mode (an index into `MODES`): the yaw, the
    /// aimed point, the field-of-view override and the screen carry over,
    /// and an isometric camera keeps its zoom, so switching there and
    /// back lands where it was. Aimed on the screen it last knew.
    pub fn in_mode(&self, index: usize) -> Camera {
        let mode = match index % Camera::MODES.len() {
            0 => Mode::Isometric,
            1 => Mode::Chase,
            2 => Mode::Shoulder,
            _ => Mode::FirstPerson,
        };
        if mode == self.mode {
            return *self;
        }
        let mut cam = if mode == Mode::Isometric { Camera::isometric(self.zoom) } else { Camera::in_placement(mode, Camera::placement_of(mode), self.angle) };
        cam.set_angle(self.angle);
        cam.fov_override = self.fov_override;
        cam.apply_fov();
        cam.screen = self.screen;
        cam.anchor = self.anchor;
        if mode == Mode::Isometric {
            if self.screen != (0, 0) {
                cam.look_at_point(self.anchor.0, self.anchor.1, self.anchor.2, self.screen.0, self.screen.1);
            }
        } else {
            cam.aim();
        }
        cam
    }

    /// The mode's name, as the `camera` settings row shows it.
    pub fn mode_name(&self) -> &'static str {
        Camera::MODES[self.mode_index()]
    }

    /// The mode's index into `MODES`.
    pub fn mode_index(&self) -> usize {
        match self.mode {
            Mode::Isometric => 0,
            Mode::Chase => 1,
            Mode::Shoulder => 2,
            Mode::FirstPerson => 3,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether the view is from an eye rather than an orthographic basis.
    pub fn is_perspective(&self) -> bool {
        self.fov > 0.0
    }

    /// Whether the character is the eye and so not drawn.
    pub fn hides_player(&self) -> bool {
        self.mode == Mode::FirstPerson
    }

    /// The field of view in degrees the `fov` settings row asks for, or
    /// `None` for the preset's own. An orthographic view ignores it.
    pub fn set_fov_override(&mut self, degrees: Option<f32>) {
        self.fov_override = degrees.map(|d| d * DEG);
        self.apply_fov();
    }

    /// The field of view in degrees, zero for an orthographic view.
    pub fn fov_degrees(&self) -> f32 {
        self.fov / DEG
    }

    fn apply_fov(&mut self) {
        if self.mode == Mode::Isometric {
            return;
        }
        self.fov = self.fov_override.unwrap_or(self.placement.fov).clamp(10.0 * DEG, 150.0 * DEG);
        self.aim();
    }

    /// The preset whose rows per metre are nearest `rise`.
    fn nearest_zoom(rise: f32) -> usize {
        let nearest = (0..ZOOMS.len()).min_by(|&i, &j| {
            let d = |z: usize| (Camera::isometric(z).rows_per_metre() - rise).abs();
            d(i).total_cmp(&d(j))
        });
        nearest.unwrap_or(0)
    }

    /// Switch to a footprint preset in place: its index, footprint, basis
    /// and pitch. The offset is left to the caller.
    fn preset(&mut self, zoom: usize) {
        self.zoom = zoom % ZOOMS.len();
        (self.hw, self.hh) = ZOOMS[self.zoom];
        self.basis = Basis { cols: self.hw as f32 * SQRT_2, rows: self.hh as f32 * SQRT_2, rise: self.hw as f32 * 3.0 / 8.0 };
        self.pitch = self.basis.pitch();
    }

    /// The perspective modes' scale factor the preset carries: the far
    /// field's scale is the weather's visibility times this.
    pub fn visibility_scale(&self) -> f32 {
        if self.is_perspective() {
            self.placement.visibility
        } else {
            1.0
        }
    }

    /// The chase distance, eye to screen centre, in metres; zero at the eye.
    pub fn distance(&self) -> f32 {
        self.placement.distance
    }

    /// Rows per unit of tangent up the screen, `focal / 2`: the rows a
    /// metre of height spans at a metre's depth. Zero for an orthographic
    /// view.
    pub fn focal_rows(&self) -> f32 {
        self.focal / 2.0
    }

    /// How far a view looks before the frame's own fog distance is known:
    /// the weather's clear-day visibility scaled by the mode, for the
    /// callers that size a box without a scene.
    pub fn far_reach(&self) -> f32 {
        World::CLEAR_VISIBILITY * self.visibility_scale()
    }

    /// The depth the scale is stated at: the character's, or in first
    /// person a conversational few metres.
    fn depth_ref(&self) -> f32 {
        if self.placement.distance <= 0.0 {
            return Camera::FIRST_PERSON_DEPTH;
        }
        let (d, _, _) = self.view_axes();
        let v = ((self.anchor.0 - self.eye.0) * TILE_METRES, (self.anchor.1 - self.eye.1) * TILE_METRES, self.anchor.2 - self.eye.2);
        (v.0 * d.0 + v.1 * d.1 + v.2 * d.2).max(0.5)
    }

    /// Place the eye and the screen from the anchor, the placement, the
    /// yaw, the pitch and the screen: the perspective camera's one setup.
    fn aim(&mut self) {
        if self.mode == Mode::Isometric {
            return;
        }
        let (sw, sh) = if self.screen == (0, 0) { Camera::DEFAULT_SCREEN } else { self.screen };
        self.ox = sw as f32 / 2.0;
        self.oy = sh as f32 / 2.0;
        self.focal = (sw as f32 / 2.0) / (self.fov / 2.0).tan();
        let (s, c) = self.yaw;
        let (sp, cp) = self.pitch.sin_cos();
        let p = self.placement;
        // The screen centre: the anchor pushed away along the ground and
        // to the right by the placement, in tiles.
        let (rx, ry) = self.right();
        let (ax, ay, az) = self.anchor;
        self.target = (ax + (rx * p.lateral - s * p.ahead) / TILE_METRES, ay + (ry * p.lateral - c * p.ahead) / TILE_METRES, az);
        let (tx, ty, tz) = self.target;
        self.eye = (tx + s * cp * p.distance / TILE_METRES, ty + c * cp * p.distance / TILE_METRES, tz + sp * p.distance);
        self.focus_z = tz;
        let depth = self.depth_ref();
        let rows = self.focal / 2.0 / depth;
        self.basis = Basis { cols: self.focal / depth * TILE_METRES, rows: rows * sp * TILE_METRES, rise: rows * cp };
        self.zoom = Camera::nearest_zoom(self.basis.rise);
        (self.hw, self.hh) = ZOOMS[self.zoom];
    }

    /// The view axes in metres: the direction the eye looks along, screen
    /// right and screen up.
    #[inline]
    fn view_axes(&self) -> (V3, V3, V3) {
        let (s, c) = self.yaw;
        let (sp, cp) = self.pitch.sin_cos();
        ((-s * cp, -c * cp, -sp), (c, -s, 0.0), (-s * sp, -c * sp, cp))
    }

    /// The eye in tiles and metres; the orthographic modes have none and
    /// answer with the focus point.
    pub fn eye(&self) -> (f32, f32, f32) {
        self.eye
    }

    /// The point the camera was aimed at.
    pub fn anchor_point(&self) -> (f32, f32, f32) {
        self.anchor
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

    /// Set the yaw, caching its sine and cosine for every projection. A
    /// perspective eye moves with it, about the point it is aimed at.
    pub fn set_angle(&mut self, radians: f32) {
        self.angle = radians;
        self.yaw = radians.sin_cos();
        if self.is_perspective() {
            self.aim();
        }
    }

    /// Turn a perspective view up or down by an angle, within its range;
    /// an orthographic view's pitch is its preset's and does not move.
    pub fn pitch_by(&mut self, radians: f32) {
        if !self.is_perspective() {
            return;
        }
        self.pitch = (self.pitch + radians).clamp(Camera::PITCH_RANGE.0, Camera::PITCH_RANGE.1);
        self.aim();
    }

    /// The pitch in whole degrees, positive looking down.
    pub fn pitch_degrees(&self) -> i32 {
        (self.pitch / DEG).round() as i32
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
    /// a tile is `TILE_METRES` square. In perspective, at the character's
    /// depth.
    pub fn columns_per_metre(&self) -> f32 {
        self.a() / TILE_METRES
    }

    /// Screen rows a metre of height draws as. In the isometric mode that
    /// is three rows per eight columns of half width, so a 2 m person is
    /// 12 rows at 1:1 and halves with every zoom out. On 1:2 cells that is
    /// 96 pixels of height against 90 pixels of ground per metre, so
    /// vertical and horizontal scale agree. Level of detail and the sprite
    /// tiers key off it. In perspective it is the scale at the character's
    /// depth; `rows_per_metre_at` gives any other point's.
    pub fn rows_per_metre(&self) -> f32 {
        self.basis.rise
    }

    /// Rows a metre of height draws as at a world point: the camera's
    /// scale in an orthographic view, and in perspective the scale at the
    /// point's own depth, so a sprite picks the tier its distance implies.
    pub fn rows_per_metre_at(&self, x: f32, y: f32, z: f32) -> f32 {
        if !self.is_perspective() {
            return self.rows_per_metre();
        }
        let depth = self.view_depth(x, y, z).max(0.5);
        self.focal / 2.0 * self.pitch.cos() / depth
    }

    /// Metres along the view direction from the eye to a point.
    #[inline]
    fn view_depth(&self, x: f32, y: f32, z: f32) -> f32 {
        let (d, _, _) = self.view_axes();
        let v = ((x - self.eye.0) * TILE_METRES, (y - self.eye.1) * TILE_METRES, z - self.eye.2);
        v.0 * d.0 + v.1 * d.1 + v.2 * d.2
    }

    /// Where the fog is measured from on a `sw` by `sh` screen: the eye,
    /// or in an orthographic view the point under the screen centre.
    pub fn fog_origin(&self, sw: i32, sh: i32) -> (f32, f32, f32) {
        if self.is_perspective() {
            return self.eye;
        }
        let (x, y) = self.focus(sw, sh);
        (x, y, self.focus_z)
    }

    /// Metres of fog between the origin and a point: the distance from
    /// the eye, or in an orthographic view the depth along the view past
    /// the origin, negative in front of it.
    pub fn fog_depth(&self, origin: (f32, f32, f32), x: f32, y: f32, z: f32) -> f32 {
        let v = ((x - origin.0) * TILE_METRES, (y - origin.1) * TILE_METRES, z - origin.2);
        if self.is_perspective() {
            return (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt();
        }
        let (d, _, _) = self.view_axes();
        v.0 * d.0 + v.1 * d.1 + v.2 * d.2
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

    /// What the status line says of the view: the zoom's ratio and name,
    /// or the perspective mode with its distance, field of view and pitch.
    pub fn view_label(&self) -> String {
        if !self.is_perspective() {
            let (name, ratio) = self.zoom_name();
            return format!("{ratio} {name}");
        }
        let distance = if self.placement.distance > 0.0 { format!(" {:.0}m", self.placement.distance) } else { String::new() };
        format!("{}{distance} fov {:.0} pitch {}", self.mode_name(), self.fov_degrees(), self.pitch_degrees())
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
    /// zoom scale, for the caller to aim at the player. A perspective
    /// view's inset is the isometric view at the other end from the
    /// preset its scale is nearest.
    pub fn inset(&self) -> Camera {
        let mut cam = Camera::isometric(Camera::inset_zoom(self.zoom));
        cam.set_angle(self.angle);
        cam
    }

    /// Screen position of a world point; `z` is metres. A perspective view
    /// puts a point behind the eye far off screen.
    pub fn project(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        if self.is_perspective() {
            let (d, r, u) = self.view_axes();
            let v = ((x - self.eye.0) * TILE_METRES, (y - self.eye.1) * TILE_METRES, z - self.eye.2);
            let depth = v.0 * d.0 + v.1 * d.1 + v.2 * d.2;
            if depth < 0.05 {
                return (-1.0e5, -1.0e5);
            }
            let sx = self.ox + self.focal * (v.0 * r.0 + v.1 * r.1) / depth;
            let sy = self.oy - self.focal / 2.0 * (v.0 * u.0 + v.1 * u.1 + v.2 * u.2) / depth;
            return (sx, sy);
        }
        let (s, c) = self.yaw;
        (self.a() * (x * c - y * s) + self.ox, self.b() * (x * s + y * c) - z * self.rows_per_metre() + self.oy)
    }

    /// Screen displacement, in columns and rows, of a world displacement
    /// of `run` tiles of ground and `rise` metres of height; in
    /// perspective, at the screen centre.
    pub fn project_vector(&self, run: (f32, f32), rise: f32) -> (f32, f32) {
        if self.is_perspective() {
            let (tx, ty, tz) = self.target;
            let (ax, ay) = self.project(tx, ty, tz);
            let (bx, by) = self.project(tx + run.0, ty + run.1, tz + rise);
            return (bx - ax, by - ay);
        }
        let (s, c) = self.yaw;
        (self.a() * (run.0 * c - run.1 * s), self.b() * (run.0 * s + run.1 * c) - rise * self.rows_per_metre())
    }

    /// Screen cell anchoring a tile: its centre projected and floored.
    pub fn project_tile(&self, mx: i32, my: i32, z: i32) -> (i32, i32) {
        let (sx, sy) = self.project(mx as f32 + 0.5, my as f32 + 0.5, z as f32);
        (sx.floor() as i32, sy.floor() as i32)
    }

    /// World point at height `z` metres under a screen position. In
    /// perspective, where the cell's ray meets that height ahead of the
    /// eye, or, for a ray that never does, its point far along.
    pub fn unproject(&self, sx: f32, sy: f32, z: f32) -> (f32, f32) {
        if self.is_perspective() {
            let ray = self.eye_ray(sx, sy);
            let t = if ray.dir.2.abs() > 1e-6 { (z - ray.eye.2) / ray.dir.2 } else { -1.0 };
            return ray.ground(if t > 0.0 { t } else { UNPROJECT_FALLBACK });
        }
        let (s, c) = self.yaw;
        let u = (sx - self.ox) / self.a();
        let v = (sy - self.oy + z * self.rows_per_metre()) / self.b();
        (u * c + v * s, -u * s + v * c)
    }

    /// The ray through a screen position: the ground point under it at
    /// height zero and the drift of that point per metre of height, which
    /// is exactly what keeps the screen position fixed as the walk
    /// descends. An orthographic view's rays are parallel, so the drift
    /// is the camera's; a perspective view's come from the eye, and a
    /// ray within a whisker of level is tilted that whisker so the form
    /// stays finite.
    pub fn ray(&self, sx: f32, sy: f32) -> Ray {
        if self.is_perspective() {
            let ray = self.eye_ray(sx, sy);
            let dz = Camera::level_guard(ray.dir.2);
            let (gx, gy) = ray.drift();
            let (ex, ey, ez) = ray.eye;
            return Ray { p0: (ex - gx * ez / dz, ey - gy * ez / dz), d: (gx / dz, gy / dz) };
        }
        let p0 = self.unproject(sx, sy, 0.0);
        let (s, c) = self.yaw;
        let b = self.b();
        let rpm = self.rows_per_metre();
        Ray { p0, d: (s * rpm / b, c * rpm / b) }
    }

    /// A vertical component no nearer level than `LEVEL`, keeping its sign.
    #[inline]
    pub fn level_guard(dz: f32) -> f32 {
        if dz.abs() >= LEVEL {
            dz
        } else if dz < 0.0 {
            -LEVEL
        } else {
            LEVEL
        }
    }

    /// The eye's ray through a screen position, as a unit direction in
    /// metres: what the perspective walk marches. An orthographic view
    /// answers with a ray from far along its own drift, for the callers
    /// that do not ask which they have.
    pub fn eye_ray(&self, sx: f32, sy: f32) -> EyeRay {
        if !self.is_perspective() {
            let Ray { p0, d } = self.ray(sx, sy);
            let (s, c) = self.yaw;
            let (sp, cp) = self.pitch.sin_cos();
            let far = 400.0;
            return EyeRay { eye: (p0.0 + d.0 * far * sp, p0.1 + d.1 * far * sp, far * sp), dir: (-s * cp, -c * cp, -sp) };
        }
        let (d, r, u) = self.view_axes();
        let ku = (sx - self.ox) / self.focal;
        let kv = -(sy - self.oy) / (self.focal / 2.0);
        let dir = (d.0 + r.0 * ku + u.0 * kv, d.1 + r.1 * ku + u.1 * kv, d.2 + r.2 * ku + u.2 * kv);
        let len = (dir.0 * dir.0 + dir.1 * dir.1 + dir.2 * dir.2).sqrt().max(1e-6);
        EyeRay { eye: self.eye, dir: (dir.0 / len, dir.1 / len, dir.2 / len) }
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

    /// Virtual camera altitude in metres for cloud parallax in the
    /// orthographic modes; higher when zoomed out. A perspective view has
    /// a real eye and does not use it.
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
        if self.is_perspective() {
            return CloudView { cx: 0.0, cy: 0.0, k: 1.0, rows: 0.0, eye: true };
        }
        let altitude = World::CLOUD_ALTITUDE;
        let c = self.altitude();
        let k = 1.0 - altitude / c;
        let (cx, cy) = self.unproject(w as f32 / 2.0, h as f32 / 2.0, 0.0);
        let rows = altitude * self.rows_per_metre();
        CloudView { cx, cy, k, rows, eye: false }
    }

    /// The map-space box the view can reach on a `w`-column screen looking
    /// `far` metres from the eye: the ground under the frustum, as the
    /// extremes of its yaw range at that distance. Orthographic views do
    /// not have one and answer `None`.
    pub fn reach(&self, w: i32, far: f32) -> Option<(f32, f32, f32, f32)> {
        if !self.is_perspective() {
            return None;
        }
        // The ground directions the screen's columns look along span the
        // view yaw either side by the half field of view; the extreme
        // reach on each map axis is the farthest that arc goes that way.
        let (d, _, _) = self.view_axes();
        let theta = d.1.atan2(d.0);
        let half = ((w as f32 / 2.0) / self.focal).atan();
        let extreme = |axis: f32| -> f32 {
            // The largest cos(phi - axis) over phi in [theta - half, theta + half].
            let rel = (theta - axis + std::f32::consts::PI).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI;
            if rel.abs() <= half {
                1.0
            } else {
                (rel.abs() - half).cos().max(0.0)
            }
        };
        let r = far / TILE_METRES;
        let (ex, ey) = (self.eye.0, self.eye.1);
        let (px, nx) = (extreme(0.0), extreme(std::f32::consts::PI));
        let (py, ny) = (extreme(FRAC_PI_2), extreme(-FRAC_PI_2));
        Some((ex - r * nx, ey - r * ny, ex + r * px, ey + r * py))
    }

    /// Map tile nearest the centre of the screen at sea level; in
    /// perspective the tile under the screen centre's target.
    pub fn center_tile(&self, map: &Map, sw: i32, sh: i32) -> (i32, i32) {
        if self.is_perspective() {
            return map.clamp(self.target.0.floor() as i32, self.target.1.floor() as i32);
        }
        let (x, y) = self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, SEA as f32);
        map.clamp(x.floor() as i32, y.floor() as i32)
    }

    /// The target: the map point under the centre of a `sw` by `sh` screen
    /// at the height the view was last aimed at, which zoom and rotation
    /// pivot about. A perspective view pivots about the point it was
    /// aimed at.
    pub fn focus(&self, sw: i32, sh: i32) -> (f32, f32) {
        if self.is_perspective() {
            return (self.anchor.0, self.anchor.1);
        }
        self.unproject(sw as f32 / 2.0, sh as f32 / 2.0, self.focus_z)
    }

    /// The ground under a screen displacement, in tiles: the inverse
    /// projection of `dx` columns and `dy` rows at a fixed height. In
    /// perspective, at the character's depth, where a row is two columns.
    pub fn ground_vector(&self, dx: f32, dy: f32) -> (f32, f32) {
        let (s, c) = self.yaw;
        if self.is_perspective() {
            let (u, v) = (dx / self.a(), 2.0 * dy / self.a());
            return (u * c + v * s, -u * s + v * c);
        }
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
    /// is the ground length of the cell along the axis the key names. A
    /// perspective view steps the ground a cell covers at the character's
    /// depth, a row counting as two columns; walking by heading is its
    /// own decision (ADR-007).
    pub fn cell_step(&self, screen_space: bool, dx: i32, dy: i32, from: (i32, i32), z: f32) -> (i32, i32) {
        let cm = |tiles: f32| (tiles * TILE_CM as f32).round() as i32;
        if self.is_perspective() {
            if !screen_space {
                return (dx * cm(1.0 / self.a()), dy * cm(2.0 / self.a()));
            }
            let (gx, gy) = self.ground_vector(dx as f32, dy as f32);
            return (cm(gx), cm(gy));
        }
        if !screen_space {
            return (dx * cm(1.0 / self.a()), dy * cm(1.0 / self.b()));
        }
        let (sx, sy) = self.project(from.0 as f32 / TILE_CM as f32, from.1 as f32 / TILE_CM as f32, z);
        let (tx, ty) = self.unproject(sx.floor() + 0.5 + dx as f32, sy.floor() + 0.5 + dy as f32, z);
        (cm(tx) - from.0, cm(ty) - from.1)
    }

    /// Place a world point at the centre of the screen; a perspective
    /// view aims itself at it by its placement.
    pub fn look_at_point(&mut self, x: f32, y: f32, z: f32, sw: i32, sh: i32) {
        self.anchor = (x, y, z);
        self.screen = (sw, sh);
        if self.is_perspective() {
            self.aim();
            return;
        }
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
    /// the map to the left, positive `dy` more above. A perspective view
    /// moves the point it is aimed at by a tile that way.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        if self.is_perspective() {
            let (rx, ry) = self.right();
            let (fx, fy) = self.forward();
            let (ax, ay, az) = self.anchor;
            self.anchor = (ax - rx * dx as f32 - fx * dy as f32, ay - ry * dx as f32 - fy * dy as f32, az);
            self.aim();
            return;
        }
        let (fw, fh) = self.footprint();
        self.ox += (dx * fw) as f32;
        self.oy += (dy * fh) as f32;
    }

    /// Place an entity's point at the centre of the screen, at the drawn
    /// height of its tile. The chase and shoulder views aim at the middle
    /// of the creature standing on the ground under it; the first-person
    /// view puts the eye at the creature's eye, `EYE_HEIGHT` of its height
    /// over the ground.
    pub fn look_at_entity(&mut self, e: &Entity, map: &Map, sw: i32, sh: i32) {
        let (x, y) = e.pos();
        if self.is_perspective() {
            let creature = &map.assets.creatures[e.kind as usize % map.assets.creatures.len()];
            let up = if self.mode == Mode::FirstPerson { Camera::EYE_HEIGHT } else { 0.5 };
            self.look_at_point(x, y, map.ground_at(x, y) + up * creature.size[2], sw, sh);
            return;
        }
        let z = map.get(e.mx(), e.my()).map(|t| t.draw_z()).unwrap_or(SEA);
        self.look_at_point(x, y, z as f32, sw, sh);
    }

    /// Recentre on the player when they leave the middle of the screen; a
    /// perspective view follows them every frame.
    pub fn follow(&mut self, world: &World, map: &Map, sw: i32, sh: i32) {
        let Some(p) = world.player() else { return };
        if self.is_perspective() {
            self.look_at_entity(p, map, sw, sh);
            return;
        }
        let z = map.get(p.mx(), p.my()).map(|t| t.draw_z()).unwrap_or(0);
        let (x, y) = p.pos();
        let (sx, sy) = self.project(x, y, z as f32);
        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
        if sx < sw / 5 || sx > sw * 4 / 5 || sy < sh / 5 || sy > sh * 4 / 5 {
            self.look_at_entity(p, map, sw, sh);
        }
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    /// A perspective view has no footprint and is left alone.
    pub fn set_zoom(&mut self, zoom: usize, sw: i32, sh: i32) {
        if self.is_perspective() {
            return;
        }
        let z = self.focus_z;
        let (x, y) = self.focus(sw, sh);
        self.preset(zoom);
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Step through the zoom levels, wrapping at either end. A chase or
    /// shoulder view halves or doubles its distance instead, between three
    /// metres and sixty-four; a first-person view has nothing to zoom.
    pub fn zoom_by(&mut self, steps: i32, sw: i32, sh: i32) {
        if self.is_perspective() {
            if self.placement.distance > 0.0 {
                let factor = 2.0f32.powi(-steps);
                self.placement.distance = (self.placement.distance * factor).clamp(3.0, 64.0);
                self.aim();
            }
            return;
        }
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

    /// Snap to the next compass view: the diagonal headings 45, 135, 225
    /// and 315 degrees, `steps` of them on from the current yaw. From a
    /// compass view that is a quarter turn; from between two it is the
    /// nearer one that way.
    pub fn rotate(&mut self, steps: i32, sw: i32, sh: i32) {
        let k = (self.angle - FRAC_PI_4) / FRAC_PI_2;
        let next = if steps > 0 { (k + 1e-4).floor() + steps as f32 } else { (k - 1e-4).ceil() + steps as f32 };
        let want = FRAC_PI_4 + next * FRAC_PI_2;
        self.rotate_by(want - self.angle, sw, sh);
    }

    pub fn degrees(&self) -> i32 {
        (self.angle.to_degrees().round() as i32).rem_euclid(360)
    }
}

/// A vector in metres: the view axes and the eye's ray direction.
type V3 = (f32, f32, f32);

/// Metres along a perspective ray that never meets the height asked for,
/// standing in for the point under it.
const UNPROJECT_FALLBACK: f32 = 400.0;

/// The least vertical component a perspective ray keeps in its height
/// form: a whisker of tilt, a tenth of a metre over a hundred, so that a
/// level ray's drift per metre of height is large but finite.
const LEVEL: f32 = 1e-3;

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
            let before = cam.cloud_view(w, h).sample(&cam, sx, sy).unwrap();
            let ground_before = cam.unproject(sx, sy, 0.0);
            cam.pan(1, 0);
            let view = cam.cloud_view(w, h);
            // The ground under a cell has moved by one tile footprint.
            let cells = cam.footprint().0 as f32;
            let ground_after = cam.unproject(sx + cells, sy, 0.0);
            assert!((ground_after.0 - ground_before.0).abs() < 1e-3 && (ground_after.1 - ground_before.1).abs() < 1e-3);
            // The cloud point that was under the cell is now C/(C - H) times as far along.
            let after = view.sample(&cam, sx + cells * ratio, sy).unwrap();
            assert!((after.0 - before.0).abs() < 1e-3 && (after.1 - before.1).abs() < 1e-3, "zoom {zoom}: {before:?} vs {after:?}");
            let ground_speed = view.sample(&cam, sx + cells, sy).unwrap();
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

    /// The three perspective presets aimed at a point on a 120x40 screen.
    fn perspective_cameras() -> Vec<Camera> {
        [Camera::chase(1.2), Camera::shoulder(0.3), Camera::first_person(4.0)]
            .into_iter()
            .map(|mut cam| {
                cam.look_at_point(20.5, 14.5, 6.0, 120, 40);
                cam
            })
            .collect()
    }

    #[test]
    fn a_perspective_ray_passes_through_the_eye_and_its_cell() {
        // The eye's ray through a cell starts at the eye, and every point
        // along it projects back to that cell; the height form the walk's
        // geometry takes is the same line, its ground point at the eye's
        // height being the eye.
        for cam in perspective_cameras() {
            assert!(cam.is_perspective() && cam.fov > 0.0);
            let (ex, ey, ez) = cam.eye();
            for (sx, sy) in [(60.5, 20.5), (0.5, 0.5), (119.5, 39.5), (30.5, 5.5), (90.5, 33.5)] {
                let ray = cam.eye_ray(sx, sy);
                assert_eq!(ray.eye, cam.eye());
                let len = (ray.dir.0 * ray.dir.0 + ray.dir.1 * ray.dir.1 + ray.dir.2 * ray.dir.2).sqrt();
                assert!((len - 1.0).abs() < 1e-4, "{} at ({sx}, {sy}): a unit direction", cam.mode_name());
                for t in [1.0, 7.5, 40.0] {
                    let (x, y) = ray.ground(t);
                    let (px, py) = cam.project(x, y, ray.height(t));
                    assert!((px - sx).abs() < 1e-2 && (py - sy).abs() < 1e-2, "{} at {t} m: ({px}, {py}) for ({sx}, {sy})", cam.mode_name());
                }
                let Ray { p0, d } = cam.ray(sx, sy);
                let (gx, gy) = (p0.0 + d.0 * ez, p0.1 + d.1 * ez);
                assert!((gx - ex).abs() < 1e-2 && (gy - ey).abs() < 1e-2, "{}: the height form passes through the eye at ({gx}, {gy}) vs ({ex}, {ey})", cam.mode_name());
            }
            // The centre cell looks straight along the view: its ray is the
            // view direction, and the screen centre's target lies on it.
            let centre = cam.eye_ray(cam.ox, cam.oy);
            let (tx, ty, tz) = cam.target;
            let t = (tz - ez) / centre.dir.2.min(-1e-6);
            if cam.distance() > 0.0 {
                let (x, y) = centre.ground(t);
                assert!((x - tx).abs() < 1e-3 && (y - ty).abs() < 1e-3 && (t - cam.distance()).abs() < 1e-3, "{}: the target is {t} m down the centre ray", cam.mode_name());
            }
        }
        // A ray within the level guard keeps a finite drift.
        let cam = Camera::perspective((3.0, 4.0, 5.0), 0.7, 0.0, 60.0 * DEG);
        let level = cam.ray(cam.ox, cam.oy);
        assert!(level.d.0.is_finite() && level.d.1.is_finite() && level.d.0.abs() < 1.0e4);
        assert_eq!(Camera::level_guard(0.0), LEVEL);
        assert_eq!(Camera::level_guard(-1.0e-5), -LEVEL);
        assert_eq!(Camera::level_guard(0.5), 0.5);
    }

    #[test]
    fn the_scale_is_stated_at_the_characters_depth() {
        // A metre of height at the aimed point spans `rows_per_metre` rows
        // and a metre of ground across it `columns_per_metre` columns; a
        // point twice as far spans half as many, and the scale at the aimed
        // point is the camera's own.
        for cam in perspective_cameras() {
            let (ax, ay, az) = cam.anchor_point();
            // The scale is the on-axis differential at the character's
            // depth: a short symmetric span on the centre ray at that depth
            // measures it (a metre's top is nearer the eye, and a figure
            // off the axis foreshortens by its offset besides).
            let depth = if cam.distance() > 0.0 { cam.view_depth(ax, ay, az) } else { Camera::FIRST_PERSON_DEPTH };
            let (ex, ey, ez) = cam.eye();
            let (d, _, _) = cam.view_axes();
            let (cx, cy, cz) = (ex + d.0 * depth / TILE_METRES, ey + d.1 * depth / TILE_METRES, ez + d.2 * depth);
            let (_, sy0) = cam.project(cx, cy, cz - 0.05);
            let (_, sy1) = cam.project(cx, cy, cz + 0.05);
            assert!(((sy0 - sy1) * 10.0 - cam.rows_per_metre()).abs() < 0.05, "{}: a metre is {} rows, stated {}", cam.mode_name(), (sy0 - sy1) * 10.0, cam.rows_per_metre());
            let (rx, ry) = cam.right();
            let (sx0, _) = cam.project(cx, cy, cz);
            let (sx1, _) = cam.project(cx + rx / TILE_METRES, cy + ry / TILE_METRES, cz);
            assert!(((sx1 - sx0) - cam.columns_per_metre()).abs() < 0.05, "{}: a metre across is {} columns, stated {}", cam.mode_name(), sx1 - sx0, cam.columns_per_metre());
            assert!((cam.rows_per_metre_at(cx, cy, cz) - cam.rows_per_metre()).abs() < 1e-3);
            let far = (cx + (cx - ex), cy + (cy - ey), cz + (cz - ez));
            assert!((cam.rows_per_metre_at(far.0, far.1, far.2) * 2.0 - cam.rows_per_metre()).abs() < 1e-2, "{}: twice the depth is half the rows", cam.mode_name());
            // Level of detail keys off the stated scale, and the preset
            // carried is the one nearest it.
            let nearest = Camera::nearest_zoom(cam.rows_per_metre());
            assert_eq!(cam.zoom, nearest);
        }
        // First person states its scale a few metres out.
        let fp = &perspective_cameras()[2];
        assert!((fp.rows_per_metre() - fp.focal_rows() / Camera::FIRST_PERSON_DEPTH).abs() < 1e-3);
        // The chase view is a 2 m person at about the rows of the near zoom
        // on a 120x40 screen, and the first person view many more.
        let chase = &perspective_cameras()[0];
        assert!(chase.rows_per_metre() * 2.0 > 6.0 && chase.rows_per_metre() * 2.0 < 12.0, "{}", chase.rows_per_metre() * 2.0);
        assert!(fp.rows_per_metre() > chase.rows_per_metre());
    }

    #[test]
    fn the_first_person_eye_sits_at_the_creatures_eye_height_and_the_chase_eye_behind() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(16, 16, assets.clone(), 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 8, 8);
        let p = world.player().unwrap();
        let (x, y) = p.pos();
        let person = assets.creatures[p.kind as usize].size[2];
        let ground = map.ground_at(x, y);
        let mut fp = Camera::first_person(FRAC_PI_4);
        fp.look_at_entity(p, &map, 120, 40);
        let eye = fp.eye();
        assert!((eye.0 - x).abs() < 1e-4 && (eye.1 - y).abs() < 1e-4, "the eye stands where the character does");
        assert!((eye.2 - (ground + Camera::EYE_HEIGHT * person)).abs() < 1e-4, "eye at {} over ground {ground} for a {person} m person", eye.2);
        assert!((person - 2.0).abs() < 1e-6 && (eye.2 - ground - 1.7).abs() < 1e-4, "1.7 m up a 2 m person");
        assert!(fp.hides_player(), "the character is the eye and is not drawn");
        // Following puts the eye back on the character every frame.
        fp.pan(3, 0);
        assert_ne!(fp.eye(), eye);
        fp.follow(&world, &map, 120, 40);
        assert_eq!(fp.eye(), eye);
        // The chase eye is the placement's distance behind and above the
        // middle of the character, along the heading.
        let mut chase = Camera::chase(FRAC_PI_4);
        chase.look_at_entity(p, &map, 120, 40);
        assert!(!chase.hides_player());
        let (ex, ey, ez) = chase.eye();
        let (fx, fy) = chase.forward();
        let (dx, dy) = ((ex - x) * TILE_METRES, (ey - y) * TILE_METRES);
        let back = (dx * dx + dy * dy).sqrt();
        let up = ez - (ground + 0.5 * person);
        assert!((back.hypot(up) - Placement::CHASE.distance).abs() < 1e-3, "{back} m back and {up} m up");
        assert!((up / back - Placement::CHASE.pitch.tan()).abs() < 1e-3, "at the placement's pitch");
        assert!((dx / back - fx).abs() < 1e-4 && (dy / back - fy).abs() < 1e-4, "behind along the heading");
        // The shoulder view puts the character low and to one side of the
        // screen, looking past them.
        let mut sh = Camera::shoulder(FRAC_PI_4);
        sh.look_at_entity(p, &map, 168, 71);
        let (sx, sy) = sh.project(x, y, ground + 0.5 * person);
        assert!(sx < 168.0 * 0.45 && sx > 168.0 * 0.1, "off to the left: column {sx}");
        assert!(sy > 71.0 * 0.6 && sy < 71.0 * 0.95, "in the lower third: row {sy}");
        // Every mode in the list builds, and switching keeps the yaw and
        // the aimed point.
        for (i, name) in Camera::MODES.iter().enumerate() {
            let cam = chase.in_mode(i);
            assert_eq!(cam.mode_name(), *name);
            assert_eq!(cam.mode_index(), i);
            assert_eq!(cam.angle(), chase.angle());
            assert_eq!(cam.anchor_point(), chase.anchor_point());
            assert_eq!(cam.is_perspective(), i != 0);
        }
        let back_to_iso = chase.in_mode(0);
        assert_eq!(back_to_iso.zoom, chase.in_mode(0).in_mode(1).in_mode(0).zoom);
    }

    #[test]
    fn a_perspective_view_pitches_within_its_range_and_the_zoom_keys_step_the_distance() {
        let mut cam = Camera::chase(0.0);
        cam.look_at_point(4.0, 4.0, 0.0, 120, 40);
        let d0 = cam.distance();
        cam.zoom_by(1, 120, 40);
        assert!((cam.distance() * 2.0 - d0).abs() < 1e-4, "zooming in halves the chase distance");
        cam.zoom_by(-2, 120, 40);
        assert!((cam.distance() - 2.0 * d0).abs() < 1e-4, "and out doubles it");
        for _ in 0..8 {
            cam.zoom_by(-1, 120, 40);
        }
        assert_eq!(cam.distance(), 64.0, "no farther than sixty-four metres");
        cam.pitch_by(1.0);
        assert!(cam.pitch <= Camera::PITCH_RANGE.1 + 1e-6);
        cam.pitch_by(-10.0);
        assert!((cam.pitch - Camera::PITCH_RANGE.0).abs() < 1e-6, "no lower than the range's foot");
        let mut iso = Camera::new();
        let before = iso.pitch;
        iso.pitch_by(0.3);
        assert_eq!(iso.pitch, before, "an isometric pitch is its preset's");
        // The field of view override stands across a mode switch and is
        // dropped by `None`.
        cam.set_fov_override(Some(90.0));
        assert!((cam.fov_degrees() - 90.0).abs() < 1e-4);
        let sh = cam.in_mode(2);
        assert!((sh.fov_degrees() - 90.0).abs() < 1e-4, "the override carries over");
        let mut sh = sh;
        sh.set_fov_override(None);
        assert!((sh.fov - Placement::SHOULDER.fov).abs() < 1e-6, "the preset's own field of view is back");
    }

    #[test]
    fn the_compass_snap_turns_to_the_next_diagonal_heading() {
        let mut cam = Camera::new();
        cam.rotate(1, 120, 40);
        assert_eq!(cam.degrees(), 135);
        cam.rotate(-1, 120, 40);
        assert_eq!(cam.degrees(), 45);
        cam.rotate_by(0.1, 120, 40);
        cam.rotate(1, 120, 40);
        assert_eq!(cam.degrees(), 135, "from between two compass views the snap goes to the next");
        cam.rotate_by(-0.1, 120, 40);
        cam.rotate(-1, 120, 40);
        assert_eq!(cam.degrees(), 45);
        cam.rotate(-1, 120, 40);
        assert_eq!(cam.degrees(), 315);
    }
}
