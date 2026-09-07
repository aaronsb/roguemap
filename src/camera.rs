//! The camera (ADR-007): a yaw and a pitch, a field of view, a scale and a
//! screen offset. It is the one place that maps the world to the screen:
//! projection takes a world point to a screen cell, unprojection walks back
//! to the ground at a chosen height, and the walk asks it for the ray
//! through a cell.
//!
//! An orthographic projection (ADR-010) projects through a basis of three
//! numbers: columns per tile across the screen, rows per tile of ground
//! depth toward the camera and rows per metre of height. The table
//! (ADR-009) is a zoom's columns per metre, the pitch of the ground plane
//! on screen, from 30 degrees to straight down, and a relief, the factor
//! height is drawn taller than the pitch implies. Its scales are
//! ADR-004's, each zoom an exact halving of the next: `columns_per_metre`
//! across the ground and `rows_per_metre` up the screen. Heights project
//! through the second, so a 2 m person is 12 rows at 1:1 and 1.5 at 1:8
//! at the floor tilt, and none at all straight down, where `detail_rows`
//! keeps the level of detail the zoom's.
//!
//! A perspective projection is a view from an eye: the chase, shoulder
//! and first-person modes each place the eye from the character by a
//! `Placement`, and the basis is the scale at the character's depth, so
//! level of detail and the sprite tiers keep one answer per frame while
//! every projection and ray comes from the eye. The detail scale drops
//! the pitch's cosine, which is the projection's foreshortening and not
//! the thing's own size.

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
    /// relief for foreshortening: `atan(tan(pitch) / relief)`, 25.24
    /// degrees at the floor tilt, the number ADR-007 pinned. The ground
    /// plane's own angle is the camera's `pitch`.
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

/// How the camera is placed: the table's presets, or a perspective eye
/// placed from the character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The ground under the screen centre, at a zoom's scale (ADR-009).
    Table,
    /// Close behind and above the character, following them.
    Chase,
    /// Well back and high, off to one side, looking past the character's
    /// shoulder into the distance; the figure sits low and off-centre.
    Shoulder,
    /// At the character's eye; the character is not drawn.
    FirstPerson,
    /// Detached, flown by the player (ADR-009): the anchor is the eye
    /// itself, and the character stands where it was left.
    Free,
}

/// How a vantage's three numbers become a basis (ADR-010): the scale at
/// the reference distance with the distance dropped, or an eye at that
/// distance and a field of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Orthographic,
    Perspective,
}

impl Projection {
    /// The `projection` settings row's values, in its order: the first
    /// asks the vantage for the projection it has always had, and the two
    /// after it override that.
    pub const NAMES: [&'static str; 3] = ["vantage", "orthographic", "perspective"];
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
    /// The table (ADR-010): the ground under the screen centre, with no
    /// push and no distance of its own — a zoom fixes the scale and the
    /// distance falls out of it, `focal / columns`. The floor tilt is
    /// where it starts and sixty degrees is what an eye reads it through.
    /// Its far field is twice the weather's, since at 1:8 the eye stands
    /// seventy-three metres from the point it looks at and a clear noon
    /// fades the ground at a hundred and six.
    pub const TABLE: Placement = Placement { distance: 0.0, pitch: 30.0 * DEG, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 2.0 };
    /// The chase view: twelve metres back at thirty degrees, sixty degrees wide.
    pub const CHASE: Placement = Placement { distance: 12.0, pitch: 30.0 * DEG, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 1.0 };
    /// The over-the-shoulder view: thirty metres back at twenty degrees,
    /// the screen centre twelve metres past the character and three to the
    /// right, forty degrees wide so the far field reads at scale, and a
    /// fog distance half as long again.
    pub const SHOULDER: Placement = Placement { distance: 30.0, pitch: 20.0 * DEG, fov: 40.0 * DEG, lateral: 3.0, ahead: 12.0, visibility: 1.5 };
    /// First person: the eye itself, level, sixty degrees wide.
    pub const FIRST_PERSON: Placement = Placement { distance: 0.0, pitch: 0.0, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 1.0 };
    /// The free camera: the eye itself, at whatever pitch the view it was
    /// entered from had, sixty degrees wide.
    pub const FREE: Placement = Placement { distance: 0.0, pitch: 0.0, fov: 60.0 * DEG, lateral: 0.0, ahead: 0.0, visibility: 1.0 };
}

const DEG: f32 = std::f32::consts::PI / 180.0;

#[derive(Clone, Copy)]
pub struct Camera {
    /// Yaw in radians; pi/4 is the classic compass view. Its sine and
    /// cosine are cached in `yaw`, so it is set through `set_angle`.
    angle: f32,
    yaw: (f32, f32),
    /// Pitch in radians above the horizon: positive looks down. It is the
    /// angle of the table's ground plane on screen (ADR-009) and the
    /// direction a perspective eye looks along, within the range its
    /// projection allows.
    pub pitch: f32,
    /// The angle the table was last left at, kept through a mode round
    /// trip the way `zoom` is (ADR-009): `pitch` is what the camera is
    /// doing now, and this is what the vantage was set to.
    table_pitch: f32,
    /// How much taller than the pitch implies height is drawn; it
    /// multiplies `cos(pitch)` and so vanishes at the plan view.
    relief: f32,
    /// Rows a metre draws as at the floor tilt: the basis's `rise` there,
    /// and the same number at every tilt (`detail_rows`).
    detail: f32,
    /// The cells a tile spans (`footprint`), kept with the basis: the
    /// ground texture hashes on it once a shaded cell, so it is two
    /// integers rather than a table basis per call.
    foot: (i32, i32),
    /// How the basis is built (ADR-010).
    projection: Projection,
    /// Field of view in radians across the screen; the orthographic table
    /// has none.
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
    /// Whether that eye stands above the cloud plane, which says on which
    /// side of it a hit has to be for the crossing to come first.
    above: bool,
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

    /// Whether a surface met at height `wz` lies past the cloud plane, so
    /// the crossing the cell samples is in front of it (ADR-010). Height
    /// runs one way along a ray, so that is exactly when the hit is on the
    /// far side of the plane from the eye: from an overview above the
    /// plane, ground below it; from an eye under it, a peak through it.
    pub fn beyond_plane(&self, wz: f32) -> bool {
        if self.above {
            wz < World::CLOUD_ALTITUDE
        } else {
            wz > World::CLOUD_ALTITUDE
        }
    }
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

impl Camera {
    /// The camera modes the `camera` settings row offers, in its order:
    /// the table's presets, then the perspective placements.
    pub const MODES: [&'static str; 5] = ["table", "chase", "shoulder", "first-person", "free"];

    /// The height of a creature's eye as a fraction of its height: 1.7 m
    /// up a 2 m person.
    pub const EYE_HEIGHT: f32 = 0.85;

    /// The depth a first-person view states its scale at, in metres: who
    /// you face in an encounter.
    pub const FIRST_PERSON_DEPTH: f32 = 4.0;

    /// The screen a camera assumes until it is aimed on one.
    const DEFAULT_SCREEN: (i32, i32) = (120, 40);

    /// The pitch a perspective view may be turned to: the full sphere,
    /// since the eye is a point and its ground plane never degenerates.
    const PITCH_RANGE: (f32, f32) = (-90.0 * DEG, 90.0 * DEG);

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
        Camera::table(0)
    }

    /// The table at a zoom preset, at the compass view and the floor
    /// tilt, with no offset.
    pub fn table(zoom: usize) -> Camera {
        let mut cam = Camera {
            angle: 0.0,
            yaw: (0.0, 1.0),
            pitch: Camera::TILT_RANGE.0,
            table_pitch: Camera::TILT_RANGE.0,
            relief: Camera::RELIEF,
            detail: 1.0,
            foot: (1, 1),
            projection: Projection::Orthographic,
            fov: 0.0,
            ox: 0.0,
            oy: 0.0,
            zoom: 0,
            focus_z: SEA as f32,
            basis: Basis { cols: 1.0, rows: 1.0, rise: 1.0 },
            mode: Mode::Table,
            placement: Placement::TABLE,
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
    /// metre, at no offset: the general form the table's presets are
    /// instances of. It carries the preset nearest its detail scale for
    /// what is keyed by zoom, whatever its tilt.
    pub fn orthographic(yaw: f32, tilt: f32, relief: f32, columns: f32) -> Camera {
        let detail = Camera::table_basis(columns, Camera::TILT_RANGE.0, relief).rise;
        let mut cam = Camera::table(Camera::nearest_zoom(detail));
        cam.set_angle(yaw);
        cam.relief = relief;
        cam.pitch = tilt;
        cam.table_pitch = tilt;
        cam.basis = Camera::table_basis(columns, tilt, relief);
        cam.detail = detail;
        cam.foot = Camera::cells_of(&cam.basis);
        cam
    }

    /// The basis of a table (ADR-009): `columns` per metre across the
    /// screen, the ground plane tilted `pitch` radians toward the viewer,
    /// and height `relief` times taller than the pitch implies.
    fn table_basis(columns: f32, pitch: f32, relief: f32) -> Basis {
        // cos(90 degrees) in f32 is a negative remainder; straight down,
        // height displaces nothing.
        let (s, c) = if pitch >= Camera::TILT_RANGE.1 { (1.0, 0.0) } else { pitch.sin_cos() };
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
        let mut cam = Camera::table(ZOOMS.len() - 1);
        cam.mode = mode;
        cam.placement = placement;
        cam.pitch = placement.pitch;
        cam.projection = Projection::Perspective;
        cam.fov = placement.fov;
        cam.screen = Camera::DEFAULT_SCREEN;
        cam.set_angle(yaw);
        cam.aim();
        cam
    }

    /// The placement a mode names.
    fn placement_of(mode: Mode) -> Placement {
        match mode {
            Mode::Table => Placement::TABLE,
            Mode::Chase => Placement::CHASE,
            Mode::Shoulder => Placement::SHOULDER,
            Mode::FirstPerson => Placement::FIRST_PERSON,
            Mode::Free => Placement::FREE,
        }
    }

    /// This view in another mode (an index into `MODES`): the yaw, the
    /// aimed point, the field-of-view override and the screen carry over.
    /// A placement comes up at its own angle; the table comes up at its
    /// zoom and the angle it was left at, so switching there and back
    /// lands where it was (ADR-009). Aimed on the screen it last knew.
    /// The free camera's aimed point is its eye, so leaving it aims at
    /// what the eye looks at (`free_target`) instead.
    pub fn in_mode(&self, index: usize) -> Camera {
        let mode = match index % Camera::MODES.len() {
            0 => Mode::Table,
            1 => Mode::Chase,
            2 => Mode::Shoulder,
            3 => Mode::FirstPerson,
            _ => Mode::Free,
        };
        if mode == self.mode {
            return *self;
        }
        if mode == Mode::Free {
            return self.free_from();
        }
        let point = if self.mode == Mode::Free { self.free_target() } else { self.anchor };
        let mut cam = if mode == Mode::Table { Camera::table(self.zoom) } else { Camera::in_placement(mode, Camera::placement_of(mode), self.angle) };
        cam.table_pitch = self.table_pitch;
        cam.set_angle(self.angle);
        cam.fov_override = self.fov_override;
        cam.apply_fov();
        cam.screen = self.screen;
        cam.anchor = point;
        if mode == Mode::Table {
            cam.pitch = cam.table_pitch.clamp(Camera::TILT_RANGE.0, Camera::TILT_RANGE.1);
            cam.preset(self.zoom);
            if self.screen != (0, 0) {
                cam.look_at_point(point.0, point.1, point.2, self.screen.0, self.screen.1);
            }
        } else {
            cam.aim();
        }
        cam
    }

    /// The free camera entered from this view (ADR-009): its eye is where
    /// this view's is, so the switch does not jump. A perspective view
    /// lends the eye and the pitch it has; the table lends the point under
    /// the screen centre, pushed back along the yaw and up by the pitch at
    /// the shoulder view's thirty metres and looked at down the pitch.
    fn free_from(&self) -> Camera {
        let mut cam = Camera::in_placement(Mode::Free, Placement::FREE, self.angle);
        cam.table_pitch = self.table_pitch;
        cam.fov_override = self.fov_override;
        if self.screen != (0, 0) {
            cam.screen = self.screen;
        }
        if self.is_perspective() {
            cam.pitch = self.pitch;
            cam.anchor = self.eye;
        } else {
            let (sw, sh) = cam.screen;
            let (x, y) = self.focus(sw, sh);
            let (s, c) = self.yaw;
            let (sp, cp) = self.pitch.sin_cos();
            let back = Placement::SHOULDER.distance;
            cam.pitch = self.pitch;
            cam.anchor = (x + s * cp * back / TILE_METRES, y + c * cp * back / TILE_METRES, self.focus_z + sp * back);
        }
        cam.apply_fov();
        cam
    }

    /// What the free eye looks at: the point `Placement::SHOULDER`'s
    /// thirty metres down its own view, which is `free_from`'s push
    /// undone. Entered and left with no key between, the table comes back
    /// to the point it was aimed at; flown, the eye leaves the table
    /// aimed at the ground ahead of it.
    fn free_target(&self) -> (f32, f32, f32) {
        let (d, _, _) = self.view_axes();
        let (ex, ey, ez) = self.eye;
        let ahead = Placement::SHOULDER.distance;
        (ex + d.0 * ahead / TILE_METRES, ey + d.1 * ahead / TILE_METRES, ez + d.2 * ahead)
    }

    /// Fly the eye (ADR-009): `forward` metres along the view direction,
    /// its pitch and all, and `right` metres across it. The free camera's
    /// anchor is its eye, so the view goes with it.
    pub fn fly(&mut self, forward: f32, right: f32) {
        let (d, r, _) = self.view_axes();
        let (ax, ay, az) = self.anchor;
        self.anchor = (ax + (d.0 * forward + r.0 * right) / TILE_METRES, ay + (d.1 * forward + r.1 * right) / TILE_METRES, az + d.2 * forward);
        self.aim();
    }

    /// Whether the walk keys address the character (ADR-009), which is
    /// also when the view follows them; the free camera flies the eye.
    pub fn addresses_character(&self) -> bool {
        self.mode != Mode::Free
    }

    /// Whether the vantage is placed from the character (ADR-010): the
    /// four placements stand at a distance from it and hold it at the
    /// screen centre, so they aim at the character and track it every
    /// tick. The table is aimed at the ground under the screen centre
    /// under either projection, and follows by its dead zone. The
    /// projection is a separate question, and asking it here is what
    /// gave the orthographic chase no character to stand on.
    pub fn placed_on_character(&self) -> bool {
        self.mode != Mode::Table
    }

    /// The mode's name, as the `camera` settings row shows it.
    pub fn mode_name(&self) -> &'static str {
        Camera::MODES[self.mode_index()]
    }

    /// The mode's index into `MODES`.
    pub fn mode_index(&self) -> usize {
        match self.mode {
            Mode::Table => 0,
            Mode::Chase => 1,
            Mode::Shoulder => 2,
            Mode::FirstPerson => 3,
            Mode::Free => 4,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether the view is from an eye rather than an orthographic basis.
    pub fn is_perspective(&self) -> bool {
        self.projection == Projection::Perspective
    }

    /// The projection a vantage has of its own (ADR-010): the table states
    /// a scale on the screen, a placement puts an eye at a distance.
    fn projection_of(mode: Mode) -> Projection {
        match mode {
            Mode::Table => Projection::Orthographic,
            _ => Projection::Perspective,
        }
    }

    /// Whether the `projection` row means anything here. A vantage whose
    /// eye is its own anchor has no distance, and an orthographic
    /// projection is the one that drops the distance, so it has nothing
    /// left to say.
    fn takes_projection(&self) -> bool {
        !matches!(self.mode, Mode::FirstPerson | Mode::Free)
    }

    /// Whether this is the orthographic table: the one of the eight
    /// combinations whose basis is a zoom's and whose screen offset the
    /// pan owns, so its setup is `preset` and not `aim`.
    fn is_orthographic_table(&self) -> bool {
        self.mode == Mode::Table && !self.is_perspective()
    }

    /// The screen the camera was last aimed on, or the one it assumes
    /// until it is aimed at all.
    fn screen_or_default(&self) -> (i32, i32) {
        if self.screen == (0, 0) {
            Camera::DEFAULT_SCREEN
        } else {
            self.screen
        }
    }

    /// This view under another projection (an index into
    /// `Projection::NAMES`, whose first value is the vantage's own): the
    /// yaw, the anchor, the zoom, the angle and the field-of-view
    /// override carry over, and the angle is clamped into the range the
    /// projection it lands in allows. `first-person` and `free` come back
    /// unchanged. `Settings::apply` calls it after `in_mode`, since a
    /// vantage switch rebuilds the camera.
    pub fn in_projection(&self, index: usize) -> Camera {
        let want = match index % Projection::NAMES.len() {
            1 => Projection::Orthographic,
            2 => Projection::Perspective,
            _ => Camera::projection_of(self.mode),
        };
        if want == self.projection || !self.takes_projection() {
            return *self;
        }
        let mut cam = *self;
        if cam.is_orthographic_table() {
            // The table's anchor is the ground under the screen centre,
            // which the pan moves without telling it.
            let (sw, sh) = cam.screen_or_default();
            let (x, y) = cam.focus(sw, sh);
            cam.anchor = (x, y, cam.focus_z);
        }
        cam.projection = want;
        let (lo, hi) = cam.pitch_range();
        cam.pitch = cam.pitch.clamp(lo, hi);
        if cam.mode == Mode::Table {
            cam.table_pitch = cam.pitch;
        }
        if cam.is_orthographic_table() {
            cam.preset(cam.zoom);
            if cam.screen != (0, 0) {
                let (x, y, z) = cam.anchor;
                cam.look_at_point(x, y, z, cam.screen.0, cam.screen.1);
            }
            return cam;
        }
        cam.apply_fov();
        cam
    }

    /// The range of angles above the ground the projection allows: the
    /// table's, where a shallower plane degenerates toward a line, or the
    /// full sphere from an eye.
    fn pitch_range(&self) -> (f32, f32) {
        match self.projection {
            Projection::Orthographic => Camera::TILT_RANGE,
            Projection::Perspective => Camera::PITCH_RANGE,
        }
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
        // The orthographic table is the one combination that reads no
        // field of view: an orthographic placement turns its distance
        // into a scale through the focal length (ADR-010).
        if self.is_orthographic_table() {
            return;
        }
        self.fov = self.fov_override.unwrap_or(self.placement.fov).clamp(10.0 * DEG, 150.0 * DEG);
        self.aim();
    }

    /// The preset whose detail scale is nearest `detail`: the rows a metre
    /// draws as at the floor tilt, which no tilt moves.
    fn nearest_zoom(detail: f32) -> usize {
        let nearest = (0..ZOOMS.len()).min_by(|&i, &j| {
            let d = |z: usize| (Camera::table(z).detail_rows() - detail).abs();
            d(i).total_cmp(&d(j))
        });
        nearest.unwrap_or(0)
    }

    /// Switch to a zoom preset in place: its index, basis and detail
    /// scale. The offset is left to the caller. Under perspective the
    /// zoom names a distance instead, `focal / columns`, so `aim` does it.
    fn preset(&mut self, zoom: usize) {
        self.zoom = zoom % ZOOMS.len();
        if self.is_perspective() {
            self.aim();
            return;
        }
        let columns = ZOOMS[self.zoom];
        self.basis = Camera::table_basis(columns, self.pitch, self.relief);
        self.detail = Camera::table_basis(columns, Camera::TILT_RANGE.0, self.relief).rise;
        self.foot = Camera::cells_of(&self.basis);
    }

    /// The scale factor a view from an eye carries: the far field's scale
    /// is the weather's visibility times this. An orthographic view has no
    /// far field to end and takes the weather's own.
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
    /// person a conversational few metres. The table states a scale
    /// instead and the distance falls out of it through the identity
    /// `columns per metre * metres of depth = focal` (ADR-010).
    fn depth_ref(&self) -> f32 {
        if self.mode == Mode::Table {
            return self.focal / ZOOMS[self.zoom % ZOOMS.len()];
        }
        if self.placement.distance <= 0.0 {
            return Camera::FIRST_PERSON_DEPTH;
        }
        let (d, _, _) = self.view_axes();
        let v = ((self.anchor.0 - self.eye.0) * TILE_METRES, (self.anchor.1 - self.eye.1) * TILE_METRES, self.anchor.2 - self.eye.2);
        (v.0 * d.0 + v.1 * d.1 + v.2 * d.2).max(0.5)
    }

    /// Place the eye and the screen from the anchor, the placement, the
    /// yaw, the pitch and the screen: every vantage's setup but the
    /// orthographic table's, whose basis is its zoom's and whose offset
    /// the pan owns. A projection reads the vantage's anchor, view
    /// direction and reference distance and turns them into a basis
    /// (ADR-010): perspective stands the eye at that distance and centres
    /// the screen on it; orthographic drops the distance, takes the scale
    /// `focal / depth` it implies and slides the projected target to the
    /// centre.
    fn aim(&mut self) {
        if self.is_orthographic_table() {
            return;
        }
        let (sw, sh) = self.screen_or_default();
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
        let back = if self.mode == Mode::Table { self.depth_ref() } else { p.distance };
        self.eye = (tx + s * cp * back / TILE_METRES, ty + c * cp * back / TILE_METRES, tz + sp * back);
        self.focus_z = tz;
        let depth = self.depth_ref();
        let rows = self.focal / 2.0 / depth;
        if self.mode == Mode::Table {
            self.detail = Camera::table_basis(ZOOMS[self.zoom], Camera::TILT_RANGE.0, self.relief).rise;
        } else {
            self.detail = rows;
            self.zoom = Camera::nearest_zoom(self.detail);
        }
        if self.is_perspective() {
            self.ox = sw as f32 / 2.0;
            self.oy = sh as f32 / 2.0;
            self.basis = Basis { cols: self.focal / depth * TILE_METRES, rows: rows * sp * TILE_METRES, rise: rows * cp };
            self.foot = Camera::cells_of(&Camera::table_basis(ZOOMS[self.zoom], Camera::TILT_RANGE.0, Camera::RELIEF));
            return;
        }
        self.basis = Camera::table_basis(self.focal / depth, self.pitch, self.relief);
        self.foot = Camera::cells_of(&self.basis);
        self.ox = 0.0;
        self.oy = 0.0;
        let (px, py) = self.project(tx, ty, tz);
        self.ox = (sw as f32 / 2.0 - px).round();
        self.oy = (sh as f32 / 2.0 - py).round();
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
                let cam = Camera::table(z);
                let (fw, fh) = cam.footprint();
                n * fw <= sw && n * fh + (relief * cam.rows_per_metre()).ceil() as i32 + 4 <= sh
            })
            .unwrap_or(0)
    }

    pub fn angle(&self) -> f32 {
        self.angle
    }

    /// Set the yaw, caching its sine and cosine for every projection. The
    /// orthographic table reads it in `project`; every other vantage
    /// swings its eye about the point it is aimed at, and an orthographic
    /// placement's offset has to follow the swing.
    pub fn set_angle(&mut self, radians: f32) {
        self.angle = radians;
        self.yaw = radians.sin_cos();
        if !self.is_orthographic_table() {
            self.aim();
        }
    }

    /// Turn the view up or down by an angle, within its range.
    pub fn pitch_by(&mut self, radians: f32) {
        self.set_pitch(self.pitch + radians);
    }

    /// Set the angle above the ground in radians, clamped to the range the
    /// projection allows. The table tilts about whatever is at the screen
    /// centre, keeps it there and remembers the angle for a switch back;
    /// a perspective view swings its eye about the point it is aimed at.
    pub fn set_pitch(&mut self, radians: f32) {
        let (lo, hi) = self.pitch_range();
        self.pitch = radians.clamp(lo, hi);
        if self.mode == Mode::Table {
            self.table_pitch = self.pitch;
        }
        if !self.is_orthographic_table() {
            self.aim();
            return;
        }
        let (sw, sh) = self.screen;
        let z = self.focus_z;
        let (x, y) = self.focus(sw, sh);
        self.preset(self.zoom);
        if self.screen != (0, 0) {
            self.look_at_point(x, y, z, sw, sh);
        }
    }

    /// The pitch in whole degrees, positive looking down.
    pub fn pitch_degrees(&self) -> i32 {
        (self.pitch / DEG).round() as i32
    }

    /// The relief: how much taller than the pitch implies height is drawn.
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
    /// vantage's own scale, which neither the angle nor the projection
    /// moves. The table's is the zoom's `rows_per_metre` at the floor
    /// tilt, to the bit; a placement's is `focal_rows` over the depth its
    /// scale is stated at, with no foreshortening, since the cosine in
    /// `rows_per_metre` is the projection's and not the thing's. The
    /// level of detail, the sprite tiers and the walk's samples per metre
    /// key off it, so a plan view at 1:1 resolves the ground as finely as
    /// a tilted one and a person seen from overhead is still a
    /// person-sized billboard. `detail_rows_at` gives another point's.
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
        self.focal / 2.0 / depth
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
    /// scale the view is stated at, the carried preset's at the floor
    /// tilt: one answer per frame, quantised, so the ground texture stays
    /// put as the character moves. Kept with the basis, since the raster
    /// asks once a shaded ground cell.
    pub fn footprint(&self) -> (i32, i32) {
        self.foot
    }

    /// The cells a basis spans at the compass view.
    fn cells_of(basis: &Basis) -> (i32, i32) {
        let cells = |n: f32| (n * SQRT_2).round().max(1.0) as i32;
        (cells(basis.cols), cells(basis.rows))
    }

    /// The zoom's name and the scale it draws at, for the HUD.
    pub fn zoom_name(&self) -> (&'static str, &'static str) {
        let i = self.zoom % ZOOMS.len();
        (ZOOM_NAMES[i], ZOOM_RATIOS[i])
    }

    /// What the status line says of the view: the table's zoom ratio and
    /// name with the angle when it is off the floor, or a placement with
    /// its distance, field of view and pitch. The projection is named
    /// only where the row overrides the vantage's own (ADR-010), so at
    /// the default every character of the line stays where it was.
    pub fn view_label(&self) -> String {
        let angle = if (self.pitch - Camera::TILT_RANGE.0).abs() > 1e-6 { format!(" {}deg", self.pitch_degrees()) } else { String::new() };
        let overridden = self.projection != Camera::projection_of(self.mode);
        if self.mode == Mode::Table {
            let (name, ratio) = self.zoom_name();
            let projection = if overridden { " persp" } else { "" };
            return format!("{ratio} {name}{angle}{projection}");
        }
        let distance = if self.placement.distance > 0.0 { format!(" {:.0}m", self.placement.distance) } else { String::new() };
        if overridden {
            return format!("{}{distance} ortho{angle}", self.mode_name());
        }
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

    /// The inset's camera: this view's heading and angle at the other end
    /// of the zoom scale, for the caller to aim at the player, so the one
    /// thing that differs between the panes is the scale. It is an
    /// orthographic table whatever the main view is, and takes the main
    /// view's angle only from another orthographic table: a pane that
    /// also changed projection would compare two things at once.
    pub fn inset(&self) -> Camera {
        let mut cam = Camera::table(Camera::inset_zoom(self.zoom));
        if self.is_orthographic_table() {
            cam.pitch = self.pitch;
            cam.table_pitch = self.pitch;
            cam.preset(cam.zoom);
        }
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
            if depth < NEAR_DEPTH {
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
    /// that do not ask which they have: down it lies `ray`'s ground point
    /// and along it `ray`'s drift, so its direction is the basis's
    /// `apparent_pitch` rather than the table's tilt.
    pub fn eye_ray(&self, sx: f32, sy: f32) -> EyeRay {
        if !self.is_perspective() {
            let Ray { p0, d } = self.ray(sx, sy);
            let (s, c) = self.yaw;
            let (sp, cp) = self.basis.apparent_pitch().sin_cos();
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
            return CloudView { cx: 0.0, cy: 0.0, k: 1.0, rows: 0.0, eye: true, above: self.eye.2 >= World::CLOUD_ALTITUDE };
        }
        let altitude = World::CLOUD_ALTITUDE;
        let c = self.altitude();
        let k = 1.0 - altitude / c;
        let (cx, cy) = self.unproject(w as f32 / 2.0, h as f32 / 2.0, 0.0);
        let rows = altitude * self.rows_per_metre();
        CloudView { cx, cy, k, rows, eye: false, above: true }
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
    /// pivot about. Every vantage but the orthographic table pivots about
    /// the point it was aimed at.
    pub fn focus(&self, sw: i32, sh: i32) -> (f32, f32) {
        if !self.is_orthographic_table() {
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

    /// The ground displacement, in tiles, that carries the point
    /// `(x, y, z)` `dx` columns and `dy` rows across the screen under a
    /// perspective view. The camera slides along the ground with the
    /// anchor, so the point's depth moves with it and the solve is at the
    /// point's own depth: `ground_vector` answers for a direction at the
    /// view's reference depth and is a different question. `None` where
    /// no ground move does it — a point at the eye, a row past the
    /// horizon, or a move that would carry the point behind the eye.
    fn ground_shift(&self, x: f32, y: f32, z: f32, dx: f32, dy: f32) -> Option<(f32, f32)> {
        let (d, _, _) = self.view_axes();
        let v = ((x - self.eye.0) * TILE_METRES, (y - self.eye.1) * TILE_METRES, z - self.eye.2);
        let depth = v.0 * d.0 + v.1 * d.1 + v.2 * d.2;
        if depth < NEAR_DEPTH {
            return None;
        }
        let (sp, cp) = self.pitch.sin_cos();
        let (sx, sy) = self.project(x, y, z);
        // Where the point is asked to land, measured from the centre.
        let (col, row) = (sx + dx - self.ox, sy + dy - self.oy);
        // Sliding the camera along the ground under its own view moves
        // the point up the screen toward the horizon, the row
        // `-focal / 2 * tan(pitch)`, and no distance reaches it.
        let den = self.focal / 2.0 * sp + row * cp;
        if den.abs() < 1e-3 {
            return None;
        }
        let along = depth * dy / den;
        if depth - along * cp < NEAR_DEPTH {
            return None;
        }
        let across = (col * along * cp - depth * dx) / self.focal;
        let (s, c) = self.yaw;
        Some(((across * c - along * s) / TILE_METRES, (-across * s - along * c) / TILE_METRES))
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

    /// Forward and right for the fly keys down (ADR-009): the normalised
    /// sum of what each one names, as `held_heading` is for the walk, so
    /// two keys fly a diagonal at one key's pace and two opposite ones
    /// are `None`. The keys name a direction on screen; what it is in the
    /// world is the view's own axes, which `fly` applies.
    pub fn held_fly(dirs: impl IntoIterator<Item = (i32, i32)>) -> Option<(f32, f32)> {
        let (mut forward, mut right) = (0.0f32, 0.0f32);
        for (dx, dy) in dirs {
            forward -= dy.signum() as f32;
            right += dx.signum() as f32;
        }
        let len = forward.hypot(right);
        (len > 1e-3).then(|| (forward / len, right / len))
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

    /// Place a world point at the centre of the screen; every vantage but
    /// the orthographic table aims itself at it by its placement.
    pub fn look_at_point(&mut self, x: f32, y: f32, z: f32, sw: i32, sh: i32) {
        self.anchor = (x, y, z);
        self.screen = (sw, sh);
        if !self.is_orthographic_table() {
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
    /// the map to the left, positive `dy` more above. Only the
    /// orthographic table has an offset to slide; every other vantage
    /// moves the point it is aimed at by a tile that way.
    pub fn pan(&mut self, dx: i32, dy: i32) {
        if self.is_orthographic_table() {
            let (fw, fh) = self.footprint();
            self.ox += (dx * fw) as f32;
            self.oy += (dy * fh) as f32;
            return;
        }
        let (rx, ry) = self.right();
        let (fx, fy) = self.forward();
        let (ax, ay, az) = self.anchor;
        self.anchor = (ax - rx * dx as f32 - fx * dy as f32, ay - ry * dx as f32 - fy * dy as f32, az);
        self.aim();
    }

    /// The point this camera aims at to look at an entity: its point at
    /// the drawn height of its tile on the table, which is the height
    /// the table draws that tile at under either projection; the middle
    /// of the creature standing on the ground under it for the chase and
    /// shoulder views; its eye, `EYE_HEIGHT` of its height over the
    /// ground, for the first-person view. The vantage answers it: a
    /// placement aims at the creature whichever projection draws it, so
    /// an orthographic chase glides over the ground the figure walks on
    /// rather than stepping by whole metres from tile top to tile top.
    pub fn entity_point(&self, e: &Entity, map: &Map) -> (f32, f32, f32) {
        let (x, y) = e.pos();
        if self.placed_on_character() {
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

    /// The cells a figure at screen position `s` owes the dead zone on a
    /// screen `n` cells across: none within the middle third, and the
    /// signed count back to the nearer edge outside it. The position is
    /// clamped before it is floored, so a projection that answers with
    /// its off-screen sentinel or with a point a long way out asks for a
    /// bounded step rather than saturating the cast and overflowing the
    /// subtraction.
    fn dead_zone_step(s: f32, n: i32) -> i32 {
        let (lo, hi) = (n / 3, n - n / 3);
        let s = s.clamp(-1.0e6, 1.0e6).floor() as i32;
        if s < lo {
            lo - s
        } else if s > hi {
            hi - s
        } else {
            0
        }
    }

    /// One tick of following the player (ADR-008): move `EASE` of what is
    /// left toward them and report whether the view has settled. A free
    /// camera follows nobody and is settled where it is (ADR-009). In the
    /// table the figure has a dead zone, the middle third of the
    /// screen each way, inside which the view does not move; outside it
    /// the view eases by whole cells, never less than one while any
    /// remains, until the figure is back inside — spent into the offset
    /// under orthographic and into the anchor, through the ground under
    /// that many cells, under perspective. The chase and shoulder views
    /// ease the aimed point toward the character; the first-person view
    /// is the character's eye and snaps to it.
    pub fn follow(&mut self, world: &World, map: &Map, sw: i32, sh: i32) -> bool {
        if !self.addresses_character() {
            return true;
        }
        let Some(p) = world.player() else { return true };
        let (x, y, z) = self.entity_point(p, map);
        if self.mode != Mode::Table {
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
        // A figure at the eye has no screen place to measure a dead zone
        // from — `project` puts one there far off screen — so the anchor
        // eases toward it until the projection can put it somewhere.
        if self.is_perspective() && self.view_depth(x, y, z) < NEAR_DEPTH {
            let (ax, ay, az) = self.anchor;
            self.look_at_point(ax + (x - ax) * Camera::EASE, ay + (y - ay) * Camera::EASE, az + (z - az) * Camera::EASE, sw, sh);
            return false;
        }
        let (sx, sy) = self.project(x, y, z);
        let (nx, ny) = (Camera::dead_zone_step(sx, sw), Camera::dead_zone_step(sy, sh));
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
        let (dx, dy) = (step(nx), step(ny));
        if self.is_perspective() {
            // The offset is the eye's own place, so the cells go into the
            // anchor instead: the ground move that carries the figure
            // those cells at its own depth. A row near the horizon buys
            // its cells with a move longer than the figure is away, and
            // past the horizon with no move at all; there the anchor
            // eases toward the figure, which the view is aimed at and so
            // brings it to the centre.
            let (ax, ay, az) = self.anchor;
            let (tx, ty) = (x - ax, y - ay);
            let reach = tx.hypot(ty);
            let (gx, gy) = match self.ground_shift(x, y, z, dx as f32, dy as f32).filter(|(gx, gy)| gx.hypot(*gy) <= reach) {
                Some(shift) => shift,
                // The anchor is on the figure and the angle keeps it out
                // of the zone: there is nothing left to ease.
                None if reach < 1e-3 => return true,
                None => (tx * Camera::EASE, ty * Camera::EASE),
            };
            // The height the view is aimed at eases after the ground the
            // figure walks on, which the ground move cannot buy: a row is
            // a depth and a height at once, and a figure below the aimed
            // height sits under the horizon whatever the ground move.
            self.anchor = (ax + gx, ay + gy, az + (z - az) * Camera::EASE);
            self.aim();
            return false;
        }
        self.ox += dx as f32;
        self.oy += dy as f32;
        false
    }

    /// Switch tile size, keeping whatever is at the screen centre there.
    /// The zoom is the table's own fixed quantity, so a placement has
    /// none and is left alone.
    pub fn set_zoom(&mut self, zoom: usize, sw: i32, sh: i32) {
        if self.mode != Mode::Table {
            return;
        }
        let z = self.focus_z;
        let (x, y) = self.focus(sw, sh);
        self.preset(zoom);
        self.look_at_point(x, y, z, sw, sh);
    }

    /// Step the vantage's own scale, wrapping at either end: the table's
    /// preset index under either projection. A chase or shoulder view
    /// halves or doubles its distance instead, between three metres and
    /// sixty-four; a first-person view has nothing to zoom.
    pub fn zoom_by(&mut self, steps: i32, sw: i32, sh: i32) {
        if self.mode != Mode::Table {
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

/// The least depth a perspective projection divides by: five centimetres
/// in front of the eye. Nearer than that a point has no screen place, and
/// `project` answers with its off-screen sentinel.
const NEAR_DEPTH: f32 = 0.05;

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
                    assert_eq!(cam.b(), Camera::table(1).b() / 2.0);
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
            let cam = Camera::table(zoom);
            let old = Old { angle: 0.0, ox: 0.0, oy: 0.0, hw, hh };
            let (a, b, rpm) = old.scales();
            assert_eq!((cam.a(), cam.rows_per_metre(), cam.detail_rows()), (a, rpm, rpm), "zoom {zoom}");
            if zoom > 0 {
                assert_eq!(cam.b(), b, "zoom {zoom}");
            }
            assert_eq!((cam.pitch, cam.relief()), (Camera::TILT_RANGE.0, Camera::RELIEF));
            assert!(!cam.is_perspective() && cam.fov == 0.0, "every preset is orthographic");
            // The general form built from the preset's own numbers is the
            // preset again and carries it.
            let general = Camera::orthographic(cam.angle(), cam.pitch, cam.relief(), cam.columns_per_metre());
            assert_eq!((general.basis(), general.detail_rows(), general.zoom), (cam.basis(), rpm, zoom), "zoom {zoom}");
        }
        // The far zoom is a true half of the mid zoom: its rows halve and
        // its rise is unchanged.
        let (far, mid) = (Camera::table(0), Camera::table(1));
        assert_eq!(far.b(), mid.b() / 2.0);
        assert_eq!(far.rows_per_metre(), 0.75);
        // The apparent pitch is the number ADR-007 pinned, the tilt taken
        // for foreshortening with the relief: 25.24 degrees at every zoom.
        for zoom in 0..ZOOMS.len() {
            let cam = Camera::table(zoom);
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
        // Kept with the basis: a tilted table's is its own, and an eye's
        // is the preset it carries, through aiming, flying and a mode
        // switch.
        let mut steep = Camera::table(2);
        steep.set_pitch(70.0 * DEG);
        assert_eq!(steep.footprint(), ((steep.a() * SQRT_2).round() as i32, (steep.b() * SQRT_2).round() as i32));
        let mut eye = Camera::chase(FRAC_PI_4);
        eye.look_at_point(8.5, 8.5, 3.0, 120, 40);
        assert_eq!(eye.footprint(), Camera::table(eye.zoom).footprint());
        let mut free = eye.in_mode(Camera::MODES.len() - 1);
        free.fly(80.0, 0.0);
        assert_eq!(free.footprint(), Camera::table(free.zoom).footprint());
        assert_eq!(free.in_mode(0).footprint(), Camera::table(free.zoom).footprint());
    }

    #[test]
    fn cloud_sample_moves_by_c_over_c_minus_h_per_tile_of_pan() {
        // At both ends of the tilt range: the parallax comes from the
        // virtual altitude, and the plan view keeps it.
        let (w, h) = (120, 40);
        for (zoom, tilt) in [(0, Camera::TILT_RANGE.0), (1, Camera::TILT_RANGE.0), (0, Camera::TILT_RANGE.1), (1, Camera::TILT_RANGE.1)] {
            let mut cam = Camera::new();
            cam.set_zoom(zoom, w, h);
            cam.set_pitch(tilt);
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
    fn the_tilt_is_the_angle_the_ground_draws_at_and_the_relief_its_exaggeration() {
        // At every tilt a metre of ground depth toward the camera is
        // sin(tilt) of a metre across, in pixels of an 8 by 16 cell, and a
        // metre of height is relief times cos(tilt) of it; a row of ground
        // is 2 / sin(tilt) columns of it at every zoom; and the detail
        // scale does not move.
        let mut cam = Camera::table(3);
        cam.look_at_point(4.5, 4.5, 0.0, 120, 40);
        for deg in (30..=90).step_by(5) {
            cam.set_pitch((deg as f32).to_radians());
            assert_eq!(cam.pitch_degrees(), deg);
            let (s, c) = (deg as f32).to_radians().sin_cos();
            for zoom in 0..ZOOMS.len() {
                cam.set_zoom(zoom, 120, 40);
                let across = cam.a() / TILE_METRES * 8.0;
                let depth = cam.b() / TILE_METRES * 16.0;
                let height = cam.rows_per_metre() * 16.0;
                assert!((depth / across - s).abs() < 1e-5, "{deg} at zoom {zoom}: sin {}", depth / across);
                if deg < 90 {
                    assert!((height / (across * c) - Camera::RELIEF).abs() < 1e-4, "{deg} at zoom {zoom}: relief {}", height / (across * c));
                }
                assert!((cam.a() / cam.b() - 2.0 / s).abs() < 1e-4, "{deg} at zoom {zoom}: a row is {} columns", cam.a() / cam.b());
                assert_eq!(cam.detail_rows(), Camera::table(zoom).detail_rows(), "{deg} at zoom {zoom}: the detail scale tilts");
            }
        }
        // Straight down: the rise is exactly zero, the ray's drift is zero,
        // a column of world projects to one point, and the detail scale is
        // the floor's number.
        cam.set_zoom(3, 120, 40);
        cam.set_pitch(FRAC_PI_2);
        assert_eq!(cam.rows_per_metre(), 0.0);
        assert_eq!(cam.ray(17.5, 9.5).d, (0.0, 0.0));
        assert_eq!(cam.project(4.5, 4.5, 120.0), cam.project(4.5, 4.5, 0.0));
        assert_eq!(cam.b(), cam.a() / 2.0);
        assert_eq!(cam.detail_rows(), 6.0);
        assert_eq!(cam.footprint(), (32, 16));
        // A view built at a steep tilt still carries the preset its
        // columns per metre name.
        let steep = Camera::orthographic(0.0, 80.0 * DEG, Camera::RELIEF, Camera::table(2).columns_per_metre());
        assert_eq!(steep.zoom, 2);
    }

    #[test]
    fn the_angle_clamps_to_its_projection_and_survives_a_zoom_step_and_a_mode_round_trip() {
        let mut cam = Camera::table(1);
        cam.look_at_point(4.5, 4.5, 0.0, 120, 40);
        cam.set_pitch(2.0);
        assert_eq!(cam.pitch, Camera::TILT_RANGE.1);
        cam.pitch_by(-3.0);
        assert_eq!(cam.pitch, Camera::TILT_RANGE.0);
        cam.pitch_by(20.0 * DEG);
        assert_eq!(cam.pitch_degrees(), 50);
        // Whatever is at the screen centre stays there through a tilt.
        let (x, y) = cam.focus(120, 40);
        cam.pitch_by(10.0 * DEG);
        let (x2, y2) = cam.focus(120, 40);
        assert!((x - x2).abs() < 1.0 / cam.b() && (y - y2).abs() < 1.0 / cam.b(), "({x}, {y}) then ({x2}, {y2})");
        cam.zoom_by(1, 120, 40);
        assert_eq!(cam.pitch_degrees(), 60);
        // The label announces the angle only off the floor.
        assert_eq!(cam.view_label(), "1:2 near 60deg");
        assert_eq!(Camera::table(0).view_label(), "1:8 far");
        // A placement comes up at its own angle, and the table at the
        // one it was left at, kept the way its zoom is (ADR-009).
        let chase = cam.in_mode(1);
        assert!(chase.is_perspective() && chase.pitch_degrees() == 30);
        let back = chase.in_mode(0);
        assert_eq!((back.pitch_degrees(), back.zoom), (60, cam.zoom));
        assert_eq!(back.basis(), cam.basis());
        // The eye's own look is not the table's angle.
        let mut looked = chase;
        looked.set_pitch(-20.0 * DEG);
        assert_eq!(looked.in_mode(0).pitch_degrees(), 60, "the table's angle, not the eye's");
        // The inset shares the angle; a perspective view's inset is at the
        // floor.
        assert_eq!((cam.inset().pitch, cam.inset().zoom), (cam.pitch, Camera::inset_zoom(cam.zoom)));
        assert_eq!(chase.inset().pitch, Camera::TILT_RANGE.0);
        // One angle, with the range its projection allows: an eye looks
        // level and the table stops at its floor.
        let mut eye = chase;
        eye.set_pitch(0.0);
        assert_eq!(eye.pitch_degrees(), 0);
        let mut still = cam;
        still.set_pitch(0.0);
        assert_eq!((still.pitch_degrees(), still.basis()), (30, Camera::table(cam.zoom).basis()));
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
            // The detail scale is the vantage's scale at that depth, and
            // the height scale is that foreshortened by the pitch.
            assert!((cam.detail_rows() - cam.focal_rows() / depth).abs() < 1e-3, "{}: {} rows a metre, stated {}", cam.mode_name(), cam.focal_rows() / depth, cam.detail_rows());
            assert!((cam.rows_per_metre() - cam.detail_rows() * cam.pitch.cos()).abs() < 1e-4, "{}", cam.mode_name());
            assert!((cam.detail_rows_at(cx, cy, cz) - cam.detail_rows()).abs() < 1e-3);
            let far = (cx + (cx - ex), cy + (cy - ey), cz + (cz - ez));
            assert!((cam.detail_rows_at(far.0, far.1, far.2) * 2.0 - cam.detail_rows()).abs() < 1e-2, "{}: twice the depth is half the rows", cam.mode_name());
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
        let cam = Camera::table(3);
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
        let iso = Camera::table(3);
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
        world.spawn_player(&map, 32, 32, 0.0);
        let (sw, sh) = (120, 40);
        let mut cam = Camera::table(3);
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
        world.spawn_player(&map, 8, 8, 0.0);
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
            assert_eq!(cam.is_perspective(), i != 0);
            // The placed modes aim at the point the chase view aimed at;
            // the free camera's anchor is the eye it took (ADR-009).
            let aimed = if cam.mode() == Mode::Free { chase.eye() } else { chase.anchor_point() };
            assert_eq!(cam.anchor_point(), aimed);
        }
        let back_to_iso = chase.in_mode(0);
        assert_eq!(back_to_iso.zoom, chase.in_mode(0).in_mode(1).in_mode(0).zoom);
    }

    /// The free camera (ADR-009): entered without a jump from either kind
    /// of view, flown by the keys, following nobody, and leaving the mode
    /// it suspended where it was.
    #[test]
    fn the_free_eye_is_entered_where_the_view_was_and_flies_from_there() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(16, 16, assets.clone(), 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 8, 8, 0.0);
        let (sw, sh) = (120, 40);
        let free = Camera::MODES.len() - 1;
        // From the table: thirty metres back along the yaw and up by the
        // tilt, looking down it, so the ground under the screen centre is
        // still under the screen centre.
        let mut table = Camera::table(2);
        table.set_pitch(50.0 * DEG);
        table.look_at(8, 8, &map, sw, sh);
        let (fx, fy) = table.focus(sw, sh);
        let eye = table.in_mode(free);
        assert_eq!(eye.mode(), Mode::Free);
        assert!(eye.is_perspective() && !eye.hides_player(), "an eye, and the character is drawn");
        assert_eq!(eye.pitch_degrees(), table.pitch_degrees(), "looking down the table's angle");
        let (ex, ey, ez) = eye.eye();
        let back = ((ex - fx) * TILE_METRES).hypot((ey - fy) * TILE_METRES).hypot(ez - table.focus_z);
        assert!((back - Placement::SHOULDER.distance).abs() < 1e-3, "{back} m from the point under the centre");
        assert!(ez > table.focus_z, "above the table's own plane");
        // From an eye, the eye it already had.
        let mut fp = Camera::first_person(FRAC_PI_4);
        fp.look_at_entity(world.player().unwrap(), &map, sw, sh);
        assert_eq!(fp.in_mode(free).eye(), fp.eye(), "the first-person eye stays put");
        // The keys fly it and the character stands where it was: forward
        // is the view direction, pitch and all, so a look down descends.
        let (p0, mut flown) = (*world.player().unwrap(), eye);
        flown.fly(10.0, 0.0);
        let (nx, ny, nz) = flown.eye();
        let moved = ((nx - ex) * TILE_METRES).hypot((ny - ey) * TILE_METRES).hypot(nz - ez);
        assert!((moved - 10.0).abs() < 1e-3, "ten metres flown, not {moved}");
        assert!(nz < ez, "looking down, forward descends");
        assert_eq!(*world.player().unwrap(), p0, "and the character has not moved");
        // It follows nobody, however far the character walks from it.
        assert!(world.try_move(&map, 400, 400));
        assert!(flown.follow(&world, &map, sw, sh), "settled where it is");
        assert_eq!(flown.eye(), (nx, ny, nz));
        // Leaving restores the mode it suspended, table and angle.
        let landed = flown.in_mode(0);
        assert!(!landed.is_perspective() && landed.mode() == Mode::Table);
        assert_eq!(landed.pitch_degrees(), table.pitch_degrees());
    }

    /// The orthographic `eye_ray` is the table's own ray in the
    /// perspective form: down it to height zero is `ray`'s ground point,
    /// and along it is `ray`'s drift.
    /// The fly keys normalise as the walk keys do (ADR-009).
    #[test]
    fn two_fly_keys_are_a_unit_diagonal() {
        assert_eq!(Camera::held_fly([(0, -1)]), Some((1.0, 0.0)), "one key is the pace forward");
        let two = Camera::held_fly([(0, -1), (1, 0)]).expect("two keys fly");
        assert!((two.0.hypot(two.1) - 1.0).abs() < 1e-6, "{two:?} is not one key's pace");
        assert!((two.0 - two.1).abs() < 1e-6, "and shares it between the two");
        let diagonal = Camera::held_fly([(1, -1)]).expect("a diagonal key flies");
        assert!((diagonal.0 - two.0).abs() < 1e-6 && (diagonal.1 - two.1).abs() < 1e-6, "a diagonal key is the two it stands for");
        assert_eq!(Camera::held_fly([(1, 0), (-1, 0)]), None, "two opposite keys are nothing to fly toward");
        assert_eq!(Camera::held_fly([]), None);
    }

    #[test]
    fn the_orthographic_eye_ray_is_the_tables_own_ray() {
        for degrees in [30.0, 45.0, 70.0, 90.0] {
            let mut cam = Camera::table(2);
            cam.set_pitch(degrees * DEG);
            cam.look_at_point(8.5, 8.5, 4.0, 120, 40);
            let (sx, sy) = (37.5, 21.0);
            let flat = cam.ray(sx, sy);
            let eye = cam.eye_ray(sx, sy);
            let t = eye.eye.2 / -eye.dir.2;
            let (gx, gy) = eye.ground(t);
            assert!((gx - flat.p0.0).abs() < 1e-3 && (gy - flat.p0.1).abs() < 1e-3, "{degrees}: {gx}, {gy} is not {:?}", flat.p0);
            // `Ray::d` is tiles per metre of height: along the ray, its
            // drift over what it climbs.
            let (dx, dy) = eye.drift();
            let (dx, dy) = (dx / eye.dir.2, dy / eye.dir.2);
            assert!((dx - flat.d.0).abs() < 1e-4 && (dy - flat.d.1).abs() < 1e-4, "{degrees}: {dx}, {dy} is not {:?}", flat.d);
        }
    }

    /// Leaving the free camera undoes the push that entered it: the table
    /// comes back where it was, and a flown eye lands on the ground it
    /// was looking at rather than on the eye itself.
    #[test]
    fn leaving_the_free_eye_undoes_the_push_that_entered_it() {
        let (sw, sh) = (120, 40);
        let free = Camera::MODES.len() - 1;
        let mut table = Camera::table(3);
        table.set_pitch(50.0 * DEG);
        table.look_at_point(8.5, 8.5, 5.0, sw, sh);
        let back = table.in_mode(free).in_mode(0);
        assert_eq!((back.ox, back.oy), (table.ox, table.oy), "there and back is where it was");
        assert!((back.focus_z - table.focus_z).abs() < 1e-3, "aimed at the same height, not at the eye's");
        let (fx, fy) = table.focus(sw, sh);
        let (bx, by) = back.focus(sw, sh);
        assert!((bx - fx).abs() < 0.05 && (by - fy).abs() < 0.05, "{bx}, {by} is not {fx}, {fy}");
        // Pitch up and fly, and the return aims at the ground the eye is
        // looking at: thirty metres down the view, well below the eye.
        let mut flown = table.in_mode(free);
        flown.pitch_by(-20.0 * DEG);
        flown.fly(60.0, 0.0);
        let landed = flown.in_mode(0);
        let expected = flown.eye().2 - Placement::SHOULDER.distance * flown.pitch.sin();
        assert!((landed.focus_z - expected).abs() < 1e-3, "{} is not the ground under the view", landed.focus_z);
        assert!(landed.focus_z < flown.eye().2, "on the ground, not in the air");
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
        iso.pitch_by(0.3);
        assert_eq!(iso.pitch_degrees(), 47, "the table tilts instead");
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

    /// The three values of the `projection` row, as `in_projection` takes
    /// them.
    const VANTAGE: usize = 0;
    const ORTHO: usize = 1;
    const PERSP: usize = 2;

    #[test]
    fn a_vantage_states_one_scale_under_either_projection_and_the_tables_eye_is_focal_over_columns() {
        for (sw, sh) in [(120, 40), (80, 25), (168, 71)] {
            for zoom in 0..ZOOMS.len() {
                let mut table = Camera::table(zoom);
                table.look_at_point(20.5, 14.5, 3.0, sw, sh);
                let eye = table.in_projection(PERSP);
                assert!(eye.is_perspective() && eye.mode() == Mode::Table && eye.zoom == zoom);
                // A scale and a distance are the same fact stated twice,
                // joined by the field of view and the screen's width.
                let columns = table.columns_per_metre();
                assert!((eye.columns_per_metre() - columns).abs() < 1e-3, "{sw}x{sh} zoom {zoom}: {} against {columns}", eye.columns_per_metre());
                let (ex, ey, ez) = eye.eye();
                let (ax, ay, az) = eye.anchor_point();
                let back = ((ex - ax) * TILE_METRES).hypot((ey - ay) * TILE_METRES).hypot(ez - az);
                let focal = eye.focal_rows() * 2.0;
                assert!((back - focal / columns).abs() < 1e-2, "{sw}x{sh} zoom {zoom}: the eye is {back} m out, not {}", focal / columns);
            }
            // A placement fixes the distance and the projection derives
            // the scale, so the two read the same number at one angle.
            for mut cam in [Camera::chase(0.3), Camera::shoulder(0.3)] {
                cam.look_at_point(20.5, 14.5, 3.0, sw, sh);
                cam.set_pitch(40.0 * DEG);
                let flat = cam.in_projection(ORTHO);
                assert!(!flat.is_perspective() && flat.pitch_degrees() == 40);
                assert!((flat.columns_per_metre() - cam.columns_per_metre()).abs() < 1e-3, "{}: {} against {}", cam.mode_name(), flat.columns_per_metre(), cam.columns_per_metre());
                assert!((flat.detail_rows() - cam.detail_rows()).abs() < 1e-4, "{}", cam.mode_name());
                assert_eq!(flat.zoom, cam.zoom, "{}: the preset everything keyed by zoom reads", cam.mode_name());
            }
        }
    }

    #[test]
    fn the_detail_scale_is_the_vantages_own_under_either_projection_and_at_every_angle() {
        for (sw, sh) in [(120, 40), (80, 25)] {
            for zoom in 0..ZOOMS.len() {
                let mut table = Camera::table(zoom);
                table.look_at_point(20.5, 14.5, 3.0, sw, sh);
                let want = table.detail_rows();
                let mut eye = table.in_projection(PERSP);
                for deg in [-90.0, -30.0, 0.0, 30.0, 60.0, 90.0] {
                    eye.set_pitch(deg * DEG);
                    assert_eq!(eye.detail_rows(), want, "{sw}x{sh} zoom {zoom} at {deg}");
                    assert_eq!(eye.zoom, zoom, "{sw}x{sh} zoom {zoom} at {deg}");
                    assert_eq!(crate::raster::lod_of(eye.detail_rows()).volumes, crate::raster::lod_of(want).volumes);
                }
            }
            let mut chase = Camera::chase(0.3);
            chase.look_at_point(20.5, 14.5, 3.0, sw, sh);
            for deg in [30.0, 60.0, 90.0] {
                chase.set_pitch(deg * DEG);
                let flat = chase.in_projection(ORTHO);
                assert!((flat.detail_rows() - chase.detail_rows()).abs() < 1e-4, "{deg}: {} against {}", flat.detail_rows(), chase.detail_rows());
                assert_eq!(flat.zoom, chase.zoom, "{deg}");
            }
        }
    }

    #[test]
    fn the_projection_row_clamps_the_angle_keeps_the_view_and_says_nothing_to_an_eye_at_its_anchor() {
        let (sw, sh) = (120, 40);
        let mut table = Camera::table(1);
        table.set_pitch(60.0 * DEG);
        table.look_at_point(20.5, 14.5, 3.0, sw, sh);
        // Out to an eye and back is the view it left: the yaw, the zoom,
        // the angle and the ground under the screen centre.
        let eye = table.in_projection(PERSP);
        assert!(eye.is_perspective() && eye.mode() == Mode::Table && eye.pitch_degrees() == 60);
        assert_eq!(eye.view_label(), "1:4 mid 60deg persp");
        let back = eye.in_projection(ORTHO);
        assert!(!back.is_perspective());
        assert_eq!((back.pitch_degrees(), back.zoom, back.angle()), (60, table.zoom, table.angle()));
        assert_eq!(back.basis(), table.basis());
        let ((bx, by), (fx, fy)) = (back.focus(sw, sh), table.focus(sw, sh));
        assert!((bx - fx).abs() < 0.05 && (by - fy).abs() < 0.05, "({bx}, {by}) is not ({fx}, {fy})");
        // And the angle a vantage was left at survives the trip, as it
        // survives a mode round trip (ADR-009).
        assert_eq!(back.in_mode(1).in_mode(0).pitch_degrees(), 60);
        // The first value is the projection the vantage has always had.
        assert!(!eye.in_projection(VANTAGE).is_perspective());
        assert!(Camera::chase(0.0).in_projection(VANTAGE).is_perspective());
        // An eye turns through the full sphere; an angle outside the
        // table's floor is clamped on the way in, and the jump is visible.
        let mut level = eye;
        level.set_pitch(-90.0 * DEG);
        assert_eq!(level.pitch_degrees(), -90);
        assert_eq!(level.in_projection(ORTHO).pitch_degrees(), 30);
        // The shoulder view's twenty degrees is under that floor too.
        let mut shoulder = Camera::shoulder(0.3);
        shoulder.look_at_point(20.5, 14.5, 3.0, sw, sh);
        assert_eq!(shoulder.pitch_degrees(), 20);
        let flat = shoulder.in_projection(ORTHO);
        assert_eq!(flat.pitch_degrees(), 30);
        assert_eq!(flat.anchor_point(), shoulder.anchor_point());
        assert_eq!(flat.view_label(), "shoulder 30m ortho");
        // The row is inert where the eye is its own anchor.
        for cam in [Camera::first_person(0.4), Camera::first_person(0.4).in_mode(Camera::MODES.len() - 1)] {
            for row in [VANTAGE, ORTHO, PERSP] {
                let same = cam.in_projection(row);
                assert!(same.is_perspective(), "{} takes no projection", cam.mode_name());
                assert_eq!((same.basis(), same.eye()), (cam.basis(), cam.eye()), "{}", cam.mode_name());
            }
        }
    }

    #[test]
    fn at_the_poles_the_view_axes_stay_orthonormal_and_the_yaw_becomes_a_roll() {
        let dot = |a: V3, b: V3| a.0 * b.0 + a.1 * b.1 + a.2 * b.2;
        for deg in [-90.0f32, 90.0] {
            let mut radii = Vec::new();
            for yaw in [0.0, 0.7, FRAC_PI_4, 3.0] {
                let mut cam = Camera::perspective((5.0, 5.0, 30.0), yaw, 0.0, 60.0 * DEG);
                cam.set_pitch(deg * DEG);
                assert_eq!(cam.pitch_degrees(), deg as i32);
                let (d, r, u) = cam.view_axes();
                for v in [d, r, u] {
                    assert!((dot(v, v).sqrt() - 1.0).abs() < 1e-5, "{deg} at {yaw}: {v:?} is not a unit vector");
                }
                assert!(dot(d, r).abs() < 1e-6 && dot(d, u).abs() < 1e-6 && dot(r, u).abs() < 1e-6, "{deg} at {yaw}");
                // Straight up or down the view runs along the vertical and
                // the up vector lies flat, pointing the way the yaw faces.
                assert!((d.2.abs() - 1.0).abs() < 1e-6 && d.0.abs() < 1e-6 && d.1.abs() < 1e-6, "{deg} at {yaw}: {d:?}");
                assert!(u.2.abs() < 1e-6 && (u.0.hypot(u.1) - 1.0).abs() < 1e-5, "{deg} at {yaw}: {u:?}");
                // So turning the yaw spins the image about its centre: a
                // point below the eye keeps its distance from the centre.
                let (ex, ey, ez) = cam.eye();
                let (px, py) = cam.project(ex + 4.0, ey, ez - 20.0 * deg.signum());
                radii.push((px - cam.ox).hypot((py - cam.oy) * 2.0));
            }
            let first = radii[0];
            assert!(radii.iter().all(|r| (r - first).abs() < 1e-2), "{deg}: {radii:?}");
        }
    }

    #[test]
    fn the_cloud_plane_is_sampled_over_a_hit_on_its_far_side() {
        let (w, h) = (120, 40);
        // The far overview stands above the plane, so the ground under it
        // takes cloud and a peak through it does not.
        let mut over = Camera::table(0);
        over.look_at_point(20.5, 14.5, 0.0, w, h);
        let over = over.in_projection(PERSP);
        assert!(over.eye().2 > World::CLOUD_ALTITUDE, "the eye is {} m up", over.eye().2);
        let view = over.cloud_view(w, h);
        assert!(view.sample(&over, 60.5, 25.0).is_some(), "a descending ray meets the plane");
        assert!(view.beyond_plane(0.0) && view.beyond_plane(World::CLOUD_ALTITUDE - 1.0));
        assert!(!view.beyond_plane(World::CLOUD_ALTITUDE + 1.0));
        // From under the plane it is the other way about, so the chase
        // view's ground draws no cloud over it.
        let mut under = Camera::chase(0.3);
        under.look_at_point(20.5, 14.5, 3.0, w, h);
        assert!(under.eye().2 < World::CLOUD_ALTITUDE, "the eye is {} m up", under.eye().2);
        let view = under.cloud_view(w, h);
        assert!(!view.beyond_plane(0.0) && !view.beyond_plane(World::CLOUD_ALTITUDE - 1.0));
        assert!(view.beyond_plane(World::CLOUD_ALTITUDE + 1.0));
        assert!(view.sample(&under, 60.5, 39.0).is_none(), "a descending ray from under the plane never reaches it");
    }

    #[test]
    fn the_wheel_the_pan_and_the_follow_ask_the_vantage_and_not_the_projection() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(64, 64, assets, 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 32, 32, 0.0);
        let (sw, sh) = (120, 40);
        // The table's zoom steps its preset under either projection.
        let mut table = Camera::table(1);
        table.look_at_point(32.5, 32.5, 5.0, sw, sh);
        let mut eye = table.in_projection(PERSP);
        let was = eye.eye();
        eye.zoom_by(1, sw, sh);
        assert_eq!(eye.zoom, 2, "the perspective table steps the preset");
        assert!(eye.eye().2 < was.2, "and a step in moves the eye nearer");
        assert_eq!(eye.distance(), 0.0, "the table has no distance of its own to halve");
        // A placement's zoom is its distance under either projection.
        let mut flat = Camera::chase(0.0).in_projection(ORTHO);
        flat.look_at_point(32.5, 32.5, 5.0, sw, sh);
        let columns = flat.columns_per_metre();
        flat.zoom_by(-1, sw, sh);
        assert!((flat.distance() - 2.0 * Placement::CHASE.distance).abs() < 1e-4, "the orthographic chase doubles its distance");
        assert!((flat.columns_per_metre() * 2.0 - columns).abs() < 1e-3, "which halves the scale it draws at");
        // The pan slides the offset only on the table, which owns one;
        // every other vantage moves the point it is aimed at.
        let (ox, oy) = (table.ox, table.oy);
        let anchor = table.anchor_point();
        table.pan(1, -2);
        let (fw, fh) = table.footprint();
        assert_eq!((table.ox - ox, table.oy - oy), (fw as f32, (-2 * fh) as f32));
        assert_eq!(table.anchor_point(), anchor);
        let anchor = flat.anchor_point();
        flat.pan(1, 0);
        assert_ne!(flat.anchor_point(), anchor, "the orthographic chase pans its anchor");
        // The table's dead zone is measured on screen either way, and the
        // cells it owes are spent into the anchor under perspective.
        eye.look_at_entity(world.player().unwrap(), &map, sw, sh);
        assert!(eye.follow(&world, &map, sw, sh), "settled on the figure");
        let anchor = eye.anchor_point();
        world.player_mut().unwrap().set_tile(44, 20);
        let mut ticks = 0;
        while !eye.follow(&world, &map, sw, sh) {
            ticks += 1;
            assert!(ticks < 200, "never settles");
        }
        assert!(ticks > 0 && eye.anchor_point() != anchor, "the anchor took the step, not the offset");
        let (x, y, z) = eye.entity_point(world.player().unwrap(), &map);
        let (px, py) = eye.project(x, y, z);
        // The zone is the cell the figure is drawn in, which is its
        // position floored, as `dead_zone_step` reads it.
        let (cx, cy) = (px.floor() as i32, py.floor() as i32);
        assert!(cx >= sw / 3 && cx <= sw - sw / 3, "back inside the zone at column {cx}");
        assert!(cy >= sh / 3 && cy <= sh - sh / 3, "and at row {cy}");
    }

    #[test]
    fn a_placement_aims_at_the_creature_under_either_projection_and_the_table_at_its_tile() {
        let assets = crate::assets::test_assets();
        // Ground that climbs a level every three tiles, so the top of the
        // tile under a walking figure and the ground it walks on part.
        let map = Map::synthetic(24, 24, assets.clone(), 1, |x, _| crate::map::Tile::flat(3 + x / 3));
        let mut world = World::new(1);
        world.spawn_player(&map, 11, 11, 0.0);
        let (sw, sh) = (120, 40);
        let placements = || [Camera::chase(0.3), Camera::chase(0.3).in_projection(ORTHO), Camera::shoulder(0.3), Camera::shoulder(0.3).in_projection(ORTHO), Camera::first_person(0.3)];
        let tables = || [Camera::table(1), Camera::table(1).in_projection(PERSP)];
        // The vantage answers who the view is placed from, and every
        // placement is placed from the character under either projection.
        for cam in placements() {
            assert!(cam.placed_on_character(), "{}", cam.view_label());
        }
        for cam in tables() {
            assert!(!cam.placed_on_character(), "{}", cam.view_label());
        }
        let aimed = |cam: &Camera, world: &World| cam.entity_point(world.player().unwrap(), &map).2;
        let p = world.player().unwrap();
        let (px, py) = p.pos();
        let creature = &assets.creatures[p.kind as usize % assets.creatures.len()];
        let middle = map.ground_at(px, py) + 0.5 * creature.size[2];
        for cam in placements() {
            let want = if cam.mode() == Mode::FirstPerson { map.ground_at(px, py) + Camera::EYE_HEIGHT * creature.size[2] } else { middle };
            assert!((aimed(&cam, &world) - want).abs() < 1e-4, "{}: aimed at {}, not {want}", cam.view_label(), aimed(&cam, &world));
        }
        let top = map.get(p.mx(), p.my()).expect("a tile under the figure").draw_z() as f32;
        for cam in tables() {
            assert_eq!(aimed(&cam, &world), top, "{}: the table aims at the drawn height of the tile", cam.view_label());
        }
        // So walking across a tile boundary the placement's aimed height
        // follows the ground, where the tile's top steps a whole metre
        // and an orthographic chase would bob with it.
        let mut chase = Camera::chase(0.3).in_projection(ORTHO);
        chase.look_at_entity(world.player().unwrap(), &map, sw, sh);
        let (mut was_chase, mut was_table) = (aimed(&chase, &world), aimed(&tables()[0], &world));
        let (mut chase_step, mut table_step) = (0.0f32, 0.0f32);
        for _ in 0..6 {
            assert!(world.try_move(&map, 40, 0), "walk 40 cm along the climb");
            let (now_chase, now_table) = (aimed(&chase, &world), aimed(&tables()[0], &world));
            chase_step = chase_step.max((now_chase - was_chase).abs());
            table_step = table_step.max((now_table - was_table).abs());
            (was_chase, was_table) = (now_chase, now_table);
        }
        assert!(chase_step < 0.5, "the chase glides: {chase_step} m in a step");
        assert!(table_step >= 1.0, "and the tile top is where the whole metre is: {table_step} m");
    }

    #[test]
    fn the_dead_zone_step_is_total_whatever_the_projection_answers() {
        // A point at the eye projects to the off-screen sentinel and one
        // a whisker in front of the near plane much further out still, so
        // the cast to a cell saturates and the step overflows unless the
        // position is clamped first.
        for s in [-1.0e5, 1.0e5, -3.0e38, 3.0e38, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            for n in [25, 40, 80, 120, 400] {
                let step = Camera::dead_zone_step(s, n);
                let landed = (s.clamp(-1.0e6, 1.0e6).floor() as i32).saturating_add(step);
                assert!(landed >= n / 3 && landed <= n - n / 3, "{s} on {n}: stepped to {landed}");
            }
        }
    }

    #[test]
    fn a_figure_inside_the_near_plane_eases_the_perspective_table_rather_than_reading_the_sentinel() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(64, 64, assets, 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 32, 32, 0.0);
        let (sw, sh) = (120, 40);
        let mut table = Camera::table(0);
        table.look_at_point(32.5, 32.5, 5.0, sw, sh);
        let mut eye = table.in_projection(PERSP);
        eye.look_at_entity(world.player().unwrap(), &map, sw, sh);
        // Put the eye two centimetres behind the figure: in front of it,
        // so the depth is positive, and inside the five centimetres
        // `project` divides by, so it answers with the sentinel.
        let (fx, fy, fz) = eye.anchor_point();
        let (ex, ey, ez) = eye.eye();
        let back = ((ex - fx) * TILE_METRES).hypot((ey - fy) * TILE_METRES).hypot(ez - fz);
        let k = (back - 0.02) / back;
        eye.look_at_point(fx - (ex - fx) * k, fy - (ey - fy) * k, fz - (ez - fz) * k, sw, sh);
        let (x, y, z) = eye.entity_point(world.player().unwrap(), &map);
        let depth = eye.view_depth(x, y, z);
        assert!(depth > 0.0 && depth < NEAR_DEPTH, "the figure is {depth} m in front of the eye");
        assert_eq!(eye.project(x, y, z), (-1.0e5, -1.0e5), "which the projection puts off screen");
        let gap = |cam: &Camera| {
            let (ax, ay, _) = cam.anchor_point();
            (x - ax).hypot(y - ay)
        };
        let was = gap(&eye);
        assert!(!eye.follow(&world, &map, sw, sh) && gap(&eye) < was, "the anchor eases toward the figure");
        let mut ticks = 0;
        while !eye.follow(&world, &map, sw, sh) {
            ticks += 1;
            assert!(ticks < 200, "never settles");
        }
        let (x, y, z) = eye.entity_point(world.player().unwrap(), &map);
        let (px, py) = eye.project(x, y, z);
        assert!(px.floor() as i32 >= sw / 3 && (px.floor() as i32) <= sw - sw / 3, "inside the zone at column {px}");
        assert!(py.floor() as i32 >= sh / 3 && (py.floor() as i32) <= sh - sh / 3, "and at row {py}");
    }

    #[test]
    fn the_perspective_tables_dead_zone_closes_at_every_angle_of_the_sphere_and_every_zoom() {
        let assets = crate::assets::test_assets();
        let map = Map::synthetic(64, 64, assets, 0, |_, _| crate::map::Tile::flat(5));
        let mut world = World::new(1);
        world.spawn_player(&map, 32, 32, 0.0);
        let (sw, sh) = (120, 40);
        for zoom in 0..ZOOMS.len() {
            for deg in [-90.0, -45.0, -10.0, -1.0, 0.0, 1.0, 5.0, 15.0, 30.0, 60.0, 89.0, 90.0] {
                let mut table = Camera::table(zoom);
                table.look_at_point(32.5, 32.5, 5.0, sw, sh);
                let mut eye = table.in_projection(PERSP);
                eye.set_pitch(deg * DEG);
                world.player_mut().unwrap().set_tile(32, 32);
                eye.look_at_entity(world.player().unwrap(), &map, sw, sh);
                let at = |cam: &Camera, world: &World| {
                    let (x, y, z) = cam.entity_point(world.player().unwrap(), &map);
                    cam.project(x, y, z)
                };
                assert!(eye.follow(&world, &map, sw, sh), "zoom {zoom} at {deg}: aimed at the figure and settled");
                // Eighteen tiles out is off the screen at every one of
                // the zooms, so every angle owes the dead zone a step.
                world.player_mut().unwrap().set_tile(50, 14);
                let mut ticks = 0;
                while !eye.follow(&world, &map, sw, sh) {
                    ticks += 1;
                    let (ax, ay, _) = eye.anchor_point();
                    assert!(ax.abs() < 1.0e4 && ay.abs() < 1.0e4, "zoom {zoom} at {deg}: the anchor ran to {ax}, {ay} on tick {ticks}");
                    assert!(ticks < 200, "zoom {zoom} at {deg}: never settles, the figure at {:?}", at(&eye, &world));
                }
                let (px, py) = at(&eye, &world);
                let (cx, cy) = (px.floor() as i32, py.floor() as i32);
                assert!(cx >= sw / 3 && cx <= sw - sw / 3, "zoom {zoom} at {deg}: back inside the zone at column {cx}");
                assert!(cy >= sh / 3 && cy <= sh - sh / 3, "zoom {zoom} at {deg}: and at row {cy}");
            }
        }
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
