//! The camera (ADR-007): a yaw and a pitch, a field of view, a scale and a
//! screen offset. It is the one place that maps the world to the screen:
//! projection takes a world point to a screen cell, unprojection walks back
//! to the ground at a chosen height, and the walk asks it for the ray
//! through a cell.
//!
//! A field of view of zero is an orthographic view, which projects through
//! a basis of three numbers: columns per tile across the screen, rows per
//! tile of ground depth toward the camera and rows per metre of height.
//! The isometric mode is a table (ADR-009): a zoom's columns per metre,
//! the tilt of the ground plane on screen, from 30 degrees to straight
//! down, and a relief, the factor height is drawn taller than the tilt
//! implies. Its scales are ADR-004's, each zoom an exact halving of the
//! next: `columns_per_metre` across the ground and `rows_per_metre` up
//! the screen. Heights project through the second, so a 2 m person is 12
//! rows at 1:1 and 1.5 at 1:8 at the floor tilt, and none at all straight
//! down, where `detail_rows` keeps the level of detail the zoom's.
//!
//! A positive field of view is a perspective view from an eye (stage 2 of
//! the ADR): the chase, shoulder and first-person modes each place the eye
//! from the character by a `Placement`, and the basis is the scale at the
//! character's depth, so level of detail and the sprite tiers keep one
//! answer per frame while every projection and ray comes from the eye.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

use crate::map::{Map, SEA, TILE_CM, TILE_METRES};
use crate::tileset::{ZOOMS, ZOOM_NAMES, ZOOM_RATIOS};
use crate::world::{Entity, Facing, World};

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
    /// The pitch above the horizon the exaggeration implies, taking the
    /// relief for foreshortening: `atan(tan(tilt) / relief)`, 25.24
    /// degrees at the floor tilt, the number ADR-007 pinned. The ground
    /// plane's own angle is `Camera::tilt`.
    pub fn apparent_pitch(&self) -> f32 {
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
    /// Pitch in radians above the horizon: positive looks down. An
    /// isometric camera's pitch is its tilt; a perspective mode's is its
    /// placement's, turned by `pitch_by`.
    pub pitch: f32,
    /// The table's tilt (ADR-009): the angle of the ground plane on
    /// screen, in radians, within `TILT_RANGE`.
    tilt: f32,
    /// How much taller than the tilt implies height is drawn; it
    /// multiplies `cos(tilt)` and so vanishes at the plan view.
    relief: f32,
    /// Rows a metre draws as at the floor tilt: the basis's `rise` there,
    /// and the same number at every tilt (`detail_rows`).
    detail: f32,
    /// Field of view in radians across the screen; zero is an orthographic
    /// view, and every isometric preset is one.
    pub fov: f32,
    pub ox: f32,
    pub oy: f32,
    /// The zoom preset, an index into `ZOOMS`. A camera built from a
    /// scale rather than a preset carries the preset nearest its detail
    /// scale, for what is keyed by zoom.
    pub zoom: usize,
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

    /// A cell's width over its height: the canonical font's 8 pixels
    /// over 16.
    pub const CELL_ASPECT: f32 = 0.5;

    /// The tilt the table may be set to: the isometric look at the floor,
    /// straight down at the top.
    pub const TILT_RANGE: (f32, f32) = (30.0 * DEG, 90.0 * DEG);

    /// The relief the table draws with: `sqrt 1.5`, the exaggeration the
    /// footprint presets always had.
    pub const RELIEF: f32 = 1.224_744_9;

    /// The far preset at the compass view.
    pub fn new() -> Camera {
        Camera::isometric(0)
    }

    /// The isometric mode: the table at a zoom preset, at the compass
    /// view and the floor tilt, with no offset.
    pub fn isometric(zoom: usize) -> Camera {
        let mut cam = Camera {
            angle: 0.0,
            yaw: (0.0, 1.0),
            pitch: 0.0,
            tilt: Camera::TILT_RANGE.0,
            relief: Camera::RELIEF,
            detail: 1.0,
            fov: 0.0,
            ox: 0.0,
            oy: 0.0,
            zoom: 0,
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

    /// An orthographic camera from a yaw, a tilt, a relief and columns per
    /// metre, at no offset: the general form the isometric presets are
    /// instances of. It carries the preset nearest its detail scale for
    /// what is keyed by zoom, whatever its tilt.
    pub fn orthographic(yaw: f32, tilt: f32, relief: f32, columns: f32) -> Camera {
        let detail = Camera::table_basis(columns, Camera::TILT_RANGE.0, relief).rise;
        let mut cam = Camera::isometric(Camera::nearest_zoom(detail));
        cam.set_angle(yaw);
        cam.tilt = tilt;
        cam.relief = relief;
        cam.pitch = tilt;
        cam.basis = Camera::table_basis(columns, tilt, relief);
        cam.detail = detail;
        cam
    }

    /// The basis of a table (ADR-009): `columns` per metre across the
    /// screen, the ground plane tilted `tilt` radians toward the viewer,
    /// and height `relief` times taller than the tilt implies.
    fn table_basis(columns: f32, tilt: f32, relief: f32) -> Basis {
        let (s, c) = tilt.sin_cos();
        let cols = columns * TILE_METRES;
        Basis { cols, rows: cols * Camera::CELL_ASPECT * s, rise: relief * columns * Camera::CELL_ASPECT * c }
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

    /// The preset whose detail scale is nearest `detail`: the rows a metre
    /// draws as at the floor tilt, which no tilt moves.
    fn nearest_zoom(detail: f32) -> usize {
        let nearest = (0..ZOOMS.len()).min_by(|&i, &j| {
            let d = |z: usize| (Camera::isometric(z).detail_rows() - detail).abs();
            d(i).total_cmp(&d(j))
        });
        nearest.unwrap_or(0)
    }

    /// Switch to a zoom preset in place: its index, basis and detail
    /// scale. The offset is left to the caller.
    fn preset(&mut self, zoom: usize) {
        self.zoom = zoom % ZOOMS.len();
        let columns = ZOOMS[self.zoom];
        self.basis = Camera::table_basis(columns, self.tilt, self.relief);
        self.detail = Camera::table_basis(columns, Camera::TILT_RANGE.0, self.relief).rise;
        self.pitch = self.tilt;
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
        self.detail = self.basis.rise;
        self.zoom = Camera::nearest_zoom(self.detail);
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

    /// The table's tilt in radians: the angle of the ground plane on
    /// screen, 30 degrees at the floor and 90 straight down.
    pub fn tilt(&self) -> f32 {
        self.tilt
    }

    /// The relief: how much taller than the tilt implies height is drawn.
    pub fn relief(&self) -> f32 {
        self.relief
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

    /// Screen rows a metre of height displaces a thing up the screen: the
    /// basis's `rise`. At the floor tilt a 2 m person is 12 rows at 1:1
    /// and halves with every zoom out, 96 pixels of height against 90
    /// pixels of ground per metre; straight down it is zero. The
    /// projection and the screen-extent culls read it; what a thing is
    /// drawn as reads `detail_rows`. In perspective it is the scale at
    /// the character's depth.
    pub fn rows_per_metre(&self) -> f32 {
        self.basis.rise
    }

    /// Rows a metre of an upright thing facing the viewer draws as: the
    /// zoom's `rows_per_metre` at the floor tilt, to the bit, at every
    /// tilt. The level of detail, the sprite tiers and the walk's samples
    /// per metre key off it, so a plan view at 1:1 resolves the ground as
    /// finely as a tilted one and a person seen from overhead is still a
    /// person-sized billboard. In perspective it is the scale at the
    /// character's depth; `detail_rows_at` gives any other point's.
    pub fn detail_rows(&self) -> f32 {
        self.detail
    }

    /// `detail_rows` at a world point: the camera's in an orthographic
    /// view, and in perspective the scale at the point's own depth, so a
    /// sprite picks the tier its distance implies.
    pub fn detail_rows_at(&self, x: f32, y: f32, z: f32) -> f32 {
        if !self.is_perspective() {
            return self.detail;
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

    /// The columns and rows a tile spans at the compass view, at least
    /// one cell each: the cells the view slides by on a pan, and the
    /// lattice the ground texture hashes on. Under an eye a tile spans a
    /// different count at every depth, so the footprint there is the
    /// scale the view is stated at, the carried preset's: one answer per
    /// frame, quantised, so the ground texture stays put as the character
    /// moves.
    pub fn footprint(&self) -> (i32, i32) {
        if self.is_perspective() {
            return Camera::isometric(self.zoom).footprint();
        }
        let cells = |n: f32| (n * SQRT_2).round().max(1.0) as i32;
        (cells(self.basis.cols), cells(self.basis.rows))
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
    /// orthographic modes; higher when zoomed out, whatever the tilt. A
    /// perspective view has a real eye and does not use it.
    pub fn altitude(&self) -> f32 {
        match self.zoom {
            0 => 100.0,
            1 => 130.0,
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

    /// The centimetre step of one screen cell (ADR-006) for a figure at
    /// `from` (centimetres) standing at height `z`: a column for left and
    /// right, a row for up and down, at this zoom. The keys no longer
    /// step by cell — a press sets a heading and the figure walks
    /// (ADR-008, `heading`) — but this is still the camera's account of
    /// what a cell covers, which the snapshot's `player_dx` and the
    /// ADR-006 tests use. In screen space the step goes to the centre of the
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

    /// The unit map vector a walk key means (ADR-008): in screen space the
    /// ground under the pressed screen direction, a diagonal at the
    /// compass view and forward or sideways from an eye; along the map
    /// axes, the axis the key names.
    pub fn heading(&self, screen_space: bool, dx: i32, dy: i32) -> (f32, f32) {
        let (x, y) = if screen_space { self.ground_vector(dx as f32, dy as f32) } else { (dx as f32, dy as f32) };
        let len = x.hypot(y).max(1e-6);
        (x / len, y / len)
    }

    /// The heading of several direction keys held at once (ADR-008): the
    /// normalised sum of what each one means on its own, so two
    /// perpendicular keys are a unit diagonal and two opposite ones are
    /// `None` — nothing held and nothing to walk toward.
    pub fn held_heading(&self, screen_space: bool, dirs: impl IntoIterator<Item = (i32, i32)>) -> Option<(f32, f32)> {
        let (mut x, mut y) = (0.0, 0.0);
        let mut add = |dx: i32, dy: i32| {
            let (hx, hy) = self.heading(screen_space, dx, dy);
            x += hx;
            y += hy;
        };
        // Each key counts as the unit heading of each axis it names, so a
        // diagonal key is the two keys it stands for and no key weighs
        // more for the ground its screen axis covers.
        for (dx, dy) in dirs {
            if dx != 0 {
                add(dx.signum(), 0);
            }
            if dy != 0 {
                add(0, dy.signum());
            }
        }
        let len = x.hypot(y);
        // Two opposite keys leave a sum that is zero but for rounding.
        (len > 1e-3).then(|| (x / len, y / len))
    }

    /// Which way a figure walking along a map heading faces on screen:
    /// `None` when it walks straight toward or away from the camera, so
    /// the figure keeps the facing it had.
    pub fn facing_of(&self, dir: (f32, f32)) -> Option<Facing> {
        let (sx, _) = self.project_vector(dir, 0.0);
        if sx.abs() < 1e-3 {
            None
        } else if sx > 0.0 {
            Some(Facing::Right)
        } else {
            Some(Facing::Left)
        }
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

    /// The point this camera aims at to look at an entity: its point at
    /// the drawn height of its tile in the isometric mode; the middle of
    /// the creature standing on the ground under it for the chase and
    /// shoulder views; its eye, `EYE_HEIGHT` of its height over the
    /// ground, for the first-person view.
    pub fn entity_point(&self, e: &Entity, map: &Map) -> (f32, f32, f32) {
        let (x, y) = e.pos();
        if self.is_perspective() {
            let creature = &map.assets.creatures[e.kind as usize % map.assets.creatures.len()];
            let up = if self.mode == Mode::FirstPerson { Camera::EYE_HEIGHT } else { 0.5 };
            return (x, y, map.ground_at(x, y) + up * creature.size[2]);
        }
        let z = map.get(e.mx(), e.my()).map(|t| t.draw_z()).unwrap_or(SEA);
        (x, y, z as f32)
    }

    /// Place an entity's point (`entity_point`) at the centre of the
    /// screen in one jump.
    pub fn look_at_entity(&mut self, e: &Entity, map: &Map, sw: i32, sh: i32) {
        let (x, y, z) = self.entity_point(e, map);
        self.look_at_point(x, y, z, sw, sh);
    }

    /// The fraction of the remaining offset to the figure the camera
    /// closes each tick of `follow` (ADR-008).
    pub const EASE: f32 = 0.3;

    /// One tick of following the player (ADR-008): move `EASE` of what is
    /// left toward them and report whether the view has settled. In the
    /// isometric mode the figure has a dead zone, the middle third of the
    /// screen each way, inside which the view does not move; outside it
    /// the offset eases by whole cells, never less than one while any
    /// remains, until the figure is back inside. The chase and shoulder
    /// views ease the aimed point toward the character; the first-person
    /// view is the character's eye and snaps to it.
    pub fn follow(&mut self, world: &World, map: &Map, sw: i32, sh: i32) -> bool {
        let Some(p) = world.player() else { return true };
        let (x, y, z) = self.entity_point(p, map);
        if self.is_perspective() {
            let (ax, ay, az) = self.anchor;
            let (dx, dy, dz) = (x - ax, y - ay, z - az);
            let left = (dx * TILE_METRES).hypot(dy * TILE_METRES).hypot(dz);
            if self.mode == Mode::FirstPerson || left < 0.005 {
                self.look_at_point(x, y, z, sw, sh);
                return true;
            }
            self.look_at_point(ax + dx * Camera::EASE, ay + dy * Camera::EASE, az + dz * Camera::EASE, sw, sh);
            return false;
        }
        let (sx, sy) = self.project(x, y, z);
        let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
        let need = |s: i32, n: i32| {
            let (lo, hi) = (n / 3, n - n / 3);
            if s < lo {
                lo - s
            } else if s > hi {
                hi - s
            } else {
                0
            }
        };
        let (nx, ny) = (need(sx, sw), need(sy, sh));
        if (nx, ny) == (0, 0) {
            return true;
        }
        let step = |n: i32| {
            let s = (n as f32 * Camera::EASE).round() as i32;
            if s == 0 && n != 0 {
                n.signum()
            } else {
                s
            }
        };
        self.ox += step(nx) as f32;
        self.oy += step(ny) as f32;
        false
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

    /// The footprint presets of ADR-004, half width and half height in
    /// cells, which the table reproduces at the floor tilt: the far one's
    /// rows are the whole cell it was drawn in, and the table halves them.
    const FOOTPRINTS: [(i32, i32); 4] = [(2, 1), (4, 1), (8, 2), (16, 4)];

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
            let (hw, hh) = FOOTPRINTS[cam.zoom];
            Old { angle: cam.angle(), ox: cam.ox, oy: cam.oy, hw, hh }
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
            for zoom in 0..ZOOMS.len() {
                cam.set_zoom(zoom, 168, 71);
                cam.look_at_point(10.5, 7.5, 5.0, 168, 71);
                let old = Old::of(&cam);
                let (a, b, rpm) = old.scales();
                assert_eq!((cam.a(), cam.rows_per_metre()), (a, rpm));
                if zoom == 0 {
                    // The far zoom's rows are half the mid zoom's (ADR-009),
                    // not the whole cell the footprint drew.
                    assert_eq!(cam.b(), Camera::isometric(1).b() / 2.0);
                    continue;
                }
                assert_eq!(cam.b(), b);
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
    fn the_table_at_the_floor_tilt_is_the_footprint_preset_to_the_bit() {
        // The basis from the tilt, the relief and the zoom's columns per
        // metre (ADR-009) is the footprint's own numbers — `hw sqrt 2`,
        // `hh sqrt 2`, `3 hw / 8` — bit for bit at 1:4, 1:2 and 1:1, and
        // the far zoom's rise too: sin(30) is exactly 0.5 in f32 and the
        // relief is sqrt 1.5. The detail scale is that rise.
        assert_eq!(Camera::RELIEF, 1.5f32.sqrt());
        assert_eq!(Camera::TILT_RANGE.0.sin(), 0.5);
        for (zoom, &(hw, hh)) in FOOTPRINTS.iter().enumerate() {
            let cam = Camera::isometric(zoom);
            let old = Old { angle: 0.0, ox: 0.0, oy: 0.0, hw, hh };
            let (a, b, rpm) = old.scales();
            assert_eq!((cam.a(), cam.rows_per_metre(), cam.detail_rows()), (a, rpm, rpm), "zoom {zoom}");
            if zoom > 0 {
                assert_eq!(cam.b(), b, "zoom {zoom}");
            }
            assert_eq!((cam.tilt(), cam.pitch, cam.relief()), (Camera::TILT_RANGE.0, Camera::TILT_RANGE.0, Camera::RELIEF));
            assert_eq!(cam.fov, 0.0, "every preset is orthographic");
            // The general form built from the preset's own numbers is the
            // preset again and carries it.
            let general = Camera::orthographic(cam.angle(), cam.tilt(), cam.relief(), cam.columns_per_metre());
            assert_eq!((general.basis(), general.detail_rows(), general.zoom), (cam.basis(), rpm, zoom), "zoom {zoom}");
        }
        // The far zoom is a true half of the mid zoom: its rows halve and
        // its rise is unchanged.
        let (far, mid) = (Camera::isometric(0), Camera::isometric(1));
        assert_eq!(far.b(), mid.b() / 2.0);
        assert_eq!(far.rows_per_metre(), 0.75);
        // The apparent pitch is the number ADR-007 pinned, the tilt taken
        // for foreshortening with the relief: 25.24 degrees at every zoom.
        for zoom in 0..ZOOMS.len() {
            let cam = Camera::isometric(zoom);
            assert!((cam.basis().apparent_pitch().to_degrees() - 25.24).abs() < 0.01, "zoom {zoom}");
            assert!((cam.basis().apparent_pitch() - (Camera::TILT_RANGE.0.tan() / Camera::RELIEF).atan()).abs() < 1e-6);
        }
        // A top-down view has all its rows in ground depth and none in
        // height; a view along the ground the reverse, with no relief.
        let down = Camera::orthographic(0.0, FRAC_PI_2, Camera::RELIEF, 4.0);
        assert!((down.b() - 2.0 * TILE_METRES).abs() < 1e-6 && down.rows_per_metre().abs() < 1e-6);
        assert_eq!(down.detail_rows(), Camera::table_basis(4.0, Camera::TILT_RANGE.0, Camera::RELIEF).rise, "the detail scale does not tilt");
        let along = Camera::orthographic(0.0, 0.0, 1.0, 4.0);
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
        // A tile at the compass view: (4, 1) far, (8, 2) mid, (16, 4)
        // near, (32, 8) close.
        let mut cam = Camera::new();
        for (zoom, (fw, fh)) in [(4, 1), (8, 2), (16, 4), (32, 8)].into_iter().enumerate() {
            cam.set_zoom(zoom, 120, 40);
            assert_eq!(cam.footprint(), (fw, fh), "zoom {zoom}");
            let (ox, oy) = (cam.ox, cam.oy);
            cam.pan(1, -2);
            assert_eq!((cam.ox - ox, cam.oy - oy), (fw as f32, (-2 * fh) as f32));
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
    fn screen_axes_give_the_four_diagonal_compass_steps_at_every_zoom() {
        // At the compass view the four screen axes land on the four
        // diagonal unit steps of the map, whatever the zoom: the tilt is
        // one angle at every zoom (ADR-009), so the far zoom no longer has
        // the 45-degree diamond that once told the screen diagonals apart.
        let mut cam = Camera::new();
        for zoom in 0..ZOOMS.len() {
            cam.set_zoom(zoom, 120, 40);
            let dirs = [(0, -1), (1, 0), (0, 1), (-1, 0)];
            let steps: Vec<(i32, i32)> = dirs.iter().map(|&(dx, dy)| cam.screen_dir_to_map(dx, dy)).collect();
            assert_eq!(steps, [(-1, -1), (1, -1), (1, 1), (-1, 1)], "zoom {zoom}: up is north-west, right north-east");
        }
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
        // far 1:8, mid 1:4, near 1:2, close 1:1.
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
        // The ground under one column is TILE_CM / cols and under one row
        // TILE_CM / rows: 8.8, 17.7, 35.4, 70.7 cm and 35.4, 70.7, 141,
        // 283 cm from close to far, each an exact halving of the zoom
        // below (ADR-006 as corrected by ADR-009).
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
        assert_eq!(rounded, [(71, 283), (35, 141), (18, 71), (9, 35)], "far, mid, near, close");
        for zoom in 1..ZOOMS.len() {
            let ((c, r), (c0, r0)) = (steps[zoom], steps[zoom - 1]);
            assert!((c * 2.0 - c0).abs() < 1e-3, "zoom {zoom}: a column is half the zoom below");
            assert!((r * 2.0 - r0).abs() < 1e-3, "zoom {zoom}: and so is a row");
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
            assert_eq!(cam.detail_rows(), cam.rows_per_metre(), "{}: from an eye the detail scale is the height scale", cam.mode_name());
            assert!((cam.detail_rows_at(cx, cy, cz) - cam.rows_per_metre()).abs() < 1e-3);
            let far = (cx + (cx - ex), cy + (cy - ey), cz + (cz - ez));
            assert!((cam.detail_rows_at(far.0, far.1, far.2) * 2.0 - cam.rows_per_metre()).abs() < 1e-2, "{}: twice the depth is half the rows", cam.mode_name());
            // Level of detail keys off the stated scale, and the preset
            // carried is the one nearest it.
            let nearest = Camera::nearest_zoom(cam.detail_rows());
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
    fn a_heading_is_the_ground_under_a_key_and_faces_the_way_it_goes() {
        use std::f32::consts::FRAC_1_SQRT_2;
        let cam = Camera::isometric(3);
        // Screen space at the compass view: right is the map diagonal
        // (1, -1) and up is (-1, -1), each a unit vector.
        let (x, y) = cam.heading(true, 1, 0);
        assert!((x - FRAC_1_SQRT_2).abs() < 1e-5 && (y + FRAC_1_SQRT_2).abs() < 1e-5, "{x}, {y}");
        let (x, y) = cam.heading(true, 0, -1);
        assert!((x + FRAC_1_SQRT_2).abs() < 1e-5 && (y + FRAC_1_SQRT_2).abs() < 1e-5, "{x}, {y}");
        // Along the map axes the key names the axis whatever the yaw.
        let mut turned = cam;
        turned.set_angle(1.0);
        assert_eq!(turned.heading(false, 0, 1), (0.0, 1.0));
        assert_eq!(turned.heading(false, -1, 0), (-1.0, 0.0));
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (x, y) = turned.heading(true, dx, dy);
            assert!((x.hypot(y) - 1.0).abs() < 1e-5, "a unit heading for {dx}, {dy}");
        }
        // Facing is the screen-space sign of the heading; straight toward
        // or away from the camera keeps whatever it was.
        assert_eq!(cam.facing_of(cam.heading(true, 1, 0)), Some(Facing::Right));
        assert_eq!(cam.facing_of(cam.heading(true, -1, 0)), Some(Facing::Left));
        assert_eq!(cam.facing_of(cam.heading(true, 0, 1)), None);
        assert_eq!(cam.facing_of(cam.heading(true, 0, -1)), None);
        assert_eq!(turned.facing_of((1.0, 0.0)), Some(Facing::Right), "east at a yaw of one radian is still rightward");
        // From an eye, up walks away from the camera and right faces right.
        let chase = Camera::chase(FRAC_PI_4);
        let (fx, fy) = chase.forward();
        let (x, y) = chase.heading(true, 0, -1);
        assert!((x + fx).abs() < 1e-4 && (y + fy).abs() < 1e-4, "up is away: {x}, {y} against forward {fx}, {fy}");
        assert_eq!(chase.facing_of(chase.heading(true, 1, 0)), Some(Facing::Right));
        assert_eq!(chase.facing_of(chase.heading(true, 0, -1)), None);
    }

    #[test]
    fn two_keys_held_are_one_diagonal_heading_and_two_opposite_ones_are_none() {
        use std::f32::consts::FRAC_1_SQRT_2;
        let iso = Camera::isometric(3);
        let chase = Camera::chase(FRAC_PI_4);
        for cam in [iso, chase] {
            for screen_space in [true, false] {
                // One key is what that key means on its own.
                let (x, y) = cam.held_heading(screen_space, [(1, 0)]).expect("one key walks");
                let (hx, hy) = cam.heading(screen_space, 1, 0);
                assert!((x - hx).abs() < 1e-6 && (y - hy).abs() < 1e-6, "{x}, {y} against {hx}, {hy}");
                assert_eq!(cam.held_heading(screen_space, []), None, "nothing held is nothing to walk toward");
                // Two perpendicular keys are the unit vector half way
                // between them: the diagonal of the two headings.
                let (ax, ay) = cam.heading(screen_space, 0, -1);
                let (bx, by) = cam.heading(screen_space, 1, 0);
                let (x, y) = cam.held_heading(screen_space, [(0, -1), (1, 0)]).expect("two keys walk");
                assert!((x.hypot(y) - 1.0).abs() < 1e-5, "a unit heading: {x}, {y}");
                assert!((x - (ax + bx) * FRAC_1_SQRT_2).abs() < 1e-5 && (y - (ay + by) * FRAC_1_SQRT_2).abs() < 1e-5, "half way between the two: {x}, {y}");
                // Opposite keys cancel, whichever pair and however many.
                assert_eq!(cam.held_heading(screen_space, [(1, 0), (-1, 0)]), None);
                assert_eq!(cam.held_heading(screen_space, [(0, 1), (0, -1)]), None);
                assert_eq!(cam.held_heading(screen_space, [(1, 1), (-1, -1), (1, 0), (-1, 0)]), None);
                // A diagonal key alone is that same diagonal.
                let one = cam.held_heading(screen_space, [(1, -1)]).expect("a diagonal key walks");
                let two = cam.held_heading(screen_space, [(0, -1), (1, 0)]).expect("two keys walk");
                assert!((one.0 - two.0).abs() < 1e-5 && (one.1 - two.1).abs() < 1e-5, "{one:?} against {two:?}");
            }
        }
    }

    #[test]
    fn the_camera_settles_on_the_figure_and_rests_inside_the_dead_zone() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(64, 64, assets, 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 32, 32);
        let (sw, sh) = (120, 40);
        let mut cam = Camera::isometric(3);
        cam.look_at_entity(world.player().unwrap(), &map, sw, sh);
        let (ox, oy) = (cam.ox, cam.oy);
        // A metre to screen-right is eleven columns of a hundred and
        // twenty: inside the middle third, so the view does not move.
        assert!(world.try_move(&map, 71, -71));
        assert!(cam.follow(&world, &map, sw, sh), "settled");
        assert_eq!((cam.ox, cam.oy), (ox, oy), "the ground did not scroll");
        // Four tiles that way is off the screen: the view eases after the
        // figure by whole cells, a fraction of what is left each tick and
        // never less than one, until the figure is back inside the zone.
        world.player_mut().unwrap().set_tile(36, 28);
        let figure = |cam: &Camera| {
            let (x, y, z) = cam.entity_point(world.player().unwrap(), &map);
            let (sx, sy) = cam.project(x, y, z);
            (sx.floor() as i32, sy.floor() as i32)
        };
        assert!(figure(&cam).0 > sw, "off the right edge at column {}", figure(&cam).0);
        let (mut ticks, mut last) = (0, cam.ox);
        while !cam.follow(&world, &map, sw, sh) {
            ticks += 1;
            assert!(cam.ox < last && cam.ox.fract() == 0.0, "whole cells, closing in: {} after {last}", cam.ox);
            assert_eq!(cam.oy, oy, "no vertical offset to close");
            last = cam.ox;
            assert!(ticks < 60, "never settles");
        }
        let (sx, sy) = figure(&cam);
        assert!(sx >= sw / 3 && sx <= sw - sw / 3 && sy >= sh / 3 && sy <= sh - sh / 3, "back inside the zone at {sx}, {sy}");
        assert!(ticks > 3, "eased over several ticks rather than one jump: {ticks}");
        assert!(cam.follow(&world, &map, sw, sh) && cam.ox == last, "and rests there");
        // A chase view closes EASE of the gap to the character each tick
        // and converges; the first-person eye snaps.
        let p = *world.player().unwrap();
        let mut chase = Camera::chase(FRAC_PI_4);
        chase.look_at_entity(&p, &map, sw, sh);
        let was = chase.entity_point(&p, &map);
        assert!(world.try_move(&map, 200, 0));
        let gap = |cam: &Camera| {
            let (x, y, z) = cam.entity_point(world.player().unwrap(), &map);
            let (ax, ay, az) = cam.anchor_point();
            ((x - ax) * TILE_METRES).hypot((y - ay) * TILE_METRES).hypot(z - az)
        };
        let g0 = gap(&chase);
        let now = chase.entity_point(world.player().unwrap(), &map);
        let moved = ((now.0 - was.0) * TILE_METRES).hypot((now.1 - was.1) * TILE_METRES).hypot(now.2 - was.2);
        assert!((g0 - moved).abs() < 1e-3 && g0 >= 2.0, "the whole step behind, two metres and the ground's rise: {g0} of {moved}");
        assert!(!chase.follow(&world, &map, sw, sh));
        assert!((gap(&chase) - g0 * (1.0 - Camera::EASE)).abs() < 1e-4, "{} of {g0} left", gap(&chase));
        let mut n = 0;
        while !chase.follow(&world, &map, sw, sh) {
            n += 1;
            assert!(n < 100, "never converges");
        }
        assert!(gap(&chase) < 1e-3 && n > 3, "on the character after {n} more ticks, {} m off", gap(&chase));
        let mut fp = Camera::first_person(FRAC_PI_4);
        fp.look_at_entity(&p, &map, sw, sh);
        assert!(fp.follow(&world, &map, sw, sh) && gap(&fp) < 1e-5, "the eye is the character's in one tick");
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
