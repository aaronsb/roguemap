//! The frame buffer the render passes share, and the order they run in.
//!
//! Terrain and geometry are drawn by inverse projection (raster.rs): every
//! screen cell walks down the continuous height field under it, testing
//! the structure columns and tree volumes in view (grid.rs, blocks.rs,
//! volume.rs) until it meets a surface, so the camera can sit at any
//! angle. Sprites (sprites.rs) are billboards for creatures, props and the
//! one-glyph trees of the smallest zooms, drawn back to front with a depth
//! test. A lighting pass (lighting.rs) then lights every cell from ambient
//! sky light, the sun shadowed by drifting clouds and by the cast shadow
//! mask (shadow.rs), and point lights; overlays (overlay.rs) add
//! precipitation and the cloud layer.

use std::cell::Cell;

use crate::assets::Assets;
use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::grid::{HeightGrid, ModelCache};
use crate::map::{Map, Tile};
use crate::noise::hash;
use crate::palette::Palette;
use crate::shadow::ShadowMask;
use crate::tileset::Tileset;
use crate::world::{Light, World};

/// Where the `fog` settings row puts the fade to the sky colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FogMode {
    /// Perspective views only, where the far field must end somewhere.
    #[default]
    Perspective,
    /// The isometric view too, by depth past the screen centre.
    Always,
    Never,
}

impl FogMode {
    /// The row's values, in order.
    pub const NAMES: [&'static str; 3] = ["perspective", "always", "never"];

    pub fn from_index(i: usize) -> FogMode {
        match i % 3 {
            0 => FogMode::Perspective,
            1 => FogMode::Always,
            _ => FogMode::Never,
        }
    }
}

/// What the renderer needs to know from the settings.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Supersample surface boundaries with sextant glyphs.
    pub aa: bool,
    /// Draw the cloud layer at the smallest zooms.
    pub clouds: bool,
    /// Where the fog fades the far field.
    pub fog: FogMode,
}

/// Everything one frame is drawn from, built once per frame and passed to
/// every pass.
pub struct Scene<'a> {
    pub map: &'a Map,
    /// The tables the map was built from.
    pub assets: &'a Assets,
    pub ts: &'a Tileset,
    pub pal: Palette,
    pub world: &'a World,
    pub cam: &'a Camera,
    /// Animation time in seconds.
    pub t: f32,
    /// Water choppiness for this frame.
    pub chop: f32,
    /// `World::daylight` for this frame, asked for at every hit.
    pub daylight: f32,
    /// How far the frame looks, in metres: the weather's visibility scaled
    /// by the camera's mode (#21). A perspective walk stops here.
    pub far: f32,
    /// The distance the light pass fades the scene to the sky colour over,
    /// or `None` for no fog: the far field is faded in a perspective view
    /// and, when the `fog` settings row asks, in the isometric one too.
    pub fog: Option<f32>,
    /// Metres of level ground one cell covers at the depth the scale is
    /// stated at, across the view and along it. The ground is
    /// foreshortened, so the second is several times the first; the ground
    /// texture is computed at the resolution the two imply (`Hit::span`).
    pub(crate) span: (f32, f32),
    /// The view's ground direction scaled by the cotangent of its tilt.
    /// A surface whose gradient meets this at one is edge-on, where a cell
    /// covers unbounded ground.
    pub(crate) lean: (f32, f32),
    /// `World::snow_at` by annual temperature, `temp + 128` as the index:
    /// a tile's temperature is a whole degree, and every hit asks.
    snow: [f32; 256],
}

impl<'a> Scene<'a> {
    /// Gather one frame's inputs; the seasonal palette, water choppiness
    /// and the per-hit world queries are derived once here.
    pub fn new(map: &'a Map, ts: &'a Tileset, world: &'a World, cam: &'a Camera, t: f32) -> Scene<'a> {
        let assets: &'a Assets = &map.assets;
        let mut snow = [0.0f32; 256];
        for (i, s) in snow.iter_mut().enumerate() {
            *s = world.snow_at((i as i32 - 128) as f32);
        }
        let far = world.visibility() * cam.visibility_scale();
        let fog = cam.is_perspective().then_some(far);
        let b = cam.basis();
        let tm = crate::map::TILE_METRES;
        let span = (tm / b.cols.max(1e-3), tm / b.rows.max(1e-3));
        let (s, c) = cam.forward();
        let cot = b.rise * tm / b.rows.max(1e-3);
        Scene { map, assets, ts, pal: assets.surfaces.for_season(world.season), world, cam, t, chop: world.choppiness(), daylight: world.daylight(), far, fog, span, lean: (s * cot, c * cot), snow }
    }

    /// The same scene with the fog where a setting puts it: in perspective
    /// views only, everywhere, or nowhere. The far distance stands.
    pub fn with_fog(mut self, mode: FogMode) -> Scene<'a> {
        self.fog = match mode {
            FogMode::Perspective => self.cam.is_perspective().then_some(self.far),
            FogMode::Always => Some(self.far),
            FogMode::Never => None,
        };
        self
    }

    /// `World::snow_at` for a tile's annual temperature.
    #[inline]
    pub fn snow_at(&self, temp: i8) -> f32 {
        self.snow[(temp as i32 + 128) as usize]
    }
}

pub(crate) const FACE_TOP: u8 = 0;
pub(crate) const FACE_RIGHT: u8 = 1;
pub(crate) const FACE_LEFT: u8 = 2;

/// One unlit cell of the frame: what to draw, and where in the world it is
/// so the lighting pass can shade it.
#[derive(Clone, Copy)]
pub(crate) struct GCell {
    pub albedo: Rgb,
    pub ch: char,
    pub glyph: Rgb,
    pub wx: f32,
    pub wy: f32,
    pub wz: f32,
    pub face: u8,
    pub lit: bool,
    /// Distance toward the camera; larger is nearer.
    pub depth: f32,
}

pub(crate) const SKY_DEPTH: f32 = -1.0e9;

/// What kind of surface a ray met.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HitKind {
    Terrain = 0,
    Wall = 1,
    Roof = 2,
    /// A stand-in crown, or a leaf cluster of a grown tree: foliage.
    Canopy = 3,
    /// A stand-in's trunk, or a branch of a grown tree: wood.
    Trunk = 4,
}

impl HitKind {
    /// Whether a hit is part of a tree, so its cell is a crown seam rather
    /// than a surface boundary.
    pub(crate) fn is_tree(self) -> bool {
        matches!(self, HitKind::Canopy | HitKind::Trunk)
    }

    /// The kind a cell identity carries in bits 5 to 7; `None` for a cell
    /// whose ray met nothing.
    pub(crate) fn from_id(id: u64) -> Option<HitKind> {
        if id == 0 {
            return None;
        }
        Some(match (id >> 5) & 7 {
            1 => HitKind::Wall,
            2 => HitKind::Roof,
            3 => HitKind::Canopy,
            4 => HitKind::Trunk,
            _ => HitKind::Terrain,
        })
    }
}

/// What a screen cell's ray met on its way down.
#[derive(Clone, Copy)]
pub(crate) struct Hit {
    /// The tile the surface belongs to: the ground, the stack's tile or
    /// the tree's.
    pub tile: Tile,
    pub mx: i32,
    pub my: i32,
    pub x: f32,
    pub y: f32,
    /// Surface height at the hit.
    pub h: f32,
    /// The field under the drawn surface, before the sea-level clamp: over
    /// water it is the bed, which says the point is submerged and how deep.
    /// Only a terrain hit carries it; anything else repeats `h`.
    pub bed: f32,
    pub face: u8,
    /// Steepness bands for cliff shading, 0 on gentle ground.
    pub below: i32,
    /// Sun-facing factor of the surface, -1 away to 1 toward.
    pub sun: f32,
    pub kind: HitKind,
    /// For a wall, the column face entered; for a canopy or trunk, the
    /// index of its volume in the frame grid.
    pub which: u32,
    /// Screen-horizontal component of the surface normal, for outline
    /// glyphs on crowns.
    pub nsx: f32,
    /// Metres of ground the cell covers here: `Scene::span` at this
    /// point's depth, stretched by how far the surface leans away from
    /// the view. The scatter lattice coarsens to it.
    pub span: f32,
}

pub struct Renderer {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) g: Vec<GCell>,
    /// Per-cell surface identity from the centre sample, for edge detection.
    pub(crate) ids: Vec<u64>,
    pub(crate) heights: Option<HeightGrid>,
    /// The surface colour of each grid tile per surface kind, packed as a
    /// colour or `u32::MAX` until a hit asks for it: a hit and the sub-rays
    /// around it share a tile, and the colour is the tile's and the kind's.
    pub(crate) colors: Vec<Cell<u32>>,
    /// Cast shadows for the frame; none at night.
    pub(crate) shadow: Option<ShadowMask>,
    /// Lights discovered while drawing this frame, such as lit windows.
    pub(crate) frame_lights: Vec<Light>,
    /// Grown L-system trees kept between frames (docs/lsystem.md), so a
    /// tree that stays in view is grown once and not once a frame.
    pub(crate) models: ModelCache,
}

impl Renderer {
    pub fn new(w: i32, h: i32) -> Renderer {
        let sky = GCell { albedo: Rgb(0, 0, 0), ch: ' ', glyph: Rgb(0, 0, 0), wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
        Renderer { w, h, g: vec![sky; (w * h) as usize], ids: vec![0; (w * h) as usize], heights: None, colors: Vec::new(), shadow: None, frame_lights: Vec::new(), models: ModelCache::new() }
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        *self = Renderer::new(w, h);
    }

    /// Lights the last frame discovered while drawing, such as lit windows.
    pub fn frame_light_count(&self) -> usize {
        self.frame_lights.len()
    }

    #[inline]
    pub(crate) fn cell(&mut self, x: i32, y: i32) -> Option<&mut GCell> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some(&mut self.g[(y * self.w + x) as usize])
        }
    }

    /// The tile at a map position, as `Map::get` gives it, from the frame
    /// grid wherever the position is in it: the grid copied its tiles from
    /// the chunks when it was built, and the passes after it need not go
    /// back to the chunk map for every tile in view.
    pub(crate) fn tile_at(&self, sc: &Scene, mx: i32, my: i32) -> Option<Tile> {
        match self.heights.as_ref().filter(|g| g.index(mx, my).is_some()) {
            Some(g) => g.tile(mx, my).copied(),
            None => sc.map.get(mx, my),
        }
    }

    /// Draw one frame of the scene into `cv`.
    pub fn draw(&mut self, cv: &mut Canvas, sc: &Scene, opts: &RenderOptions) {
        self.frame_lights.clear();
        self.sky_pass(sc);
        let (x0, y0, x1, y1) = self.bounds_to(sc.cam, self.view_ceiling(sc), sc.far);
        let grid = HeightGrid::build(sc, x0, y0, x1, y1, self.w, self.h, &mut self.models);
        self.shadow = ShadowMask::build(sc, &grid);
        self.colors.clear();
        self.colors.resize(grid.len() * crate::raster::SURFACE_KINDS, Cell::new(u32::MAX));
        self.heights = Some(grid);
        self.stack_lights(sc);
        self.terrain_pass(sc, opts.aa && sc.ts.antialias);
        self.sprite_pass(sc);
        self.prop_pass(sc);
        self.draw_lights(sc);
        self.light_pass(cv, sc);
        // Precipitation falls beneath the cloud layer, so it is drawn first.
        self.weather_pass(cv, sc);
        if opts.clouds {
            self.cloud_pass(cv, sc);
        }
    }

    /// Fill the frame with sky and a scatter of stars that fade with daylight.
    fn sky_pass(&mut self, sc: &Scene) {
        let (world, t) = (sc.world, sc.t);
        let sky = world.sky();
        let fade = 1.0 - world.skylight() * 0.92;
        for y in 0..self.h {
            for x in 0..self.w {
                let h = hash(x as i64, y as i64, 0xBEEF);
                let (ch, glyph) = if h.is_multiple_of(89) {
                    let tw = 0.55 + 0.45 * (t * 1.7 + (h >> 20) as f32 * 0.01).sin();
                    let b = (tw * fade * 235.0) as u8;
                    (sc.ts.star[(h >> 8).is_multiple_of(5) as usize], sky.lerp(Rgb(b, b, b.saturating_add(15)), fade))
                } else {
                    (' ', sky)
                };
                self.g[(y * self.w + x) as usize] = GCell { albedo: sky, ch, glyph, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
            }
        }
    }

    /// Map-space bounding box of everything that can appear on screen, for
    /// terrain reaching `top` metres. A tall thing far behind the view can
    /// still show over the horizon, so the box stretches away from the
    /// camera by however many tiles `top` is worth in rows — up to
    /// `MAX_DEPTH`, past which a range hundreds of metres up is left out
    /// rather than made to cost a frame.
    pub(crate) fn visible_bounds(&self, cam: &Camera, top: f32) -> (i32, i32, i32, i32) {
        self.bounds_to(cam, top, cam.far_reach())
    }

    /// `visible_bounds` for a perspective view looking `far` metres: the
    /// ground under the frustum out to the fog, which bounds the walk, and
    /// the same margin for what leans in from beyond.
    pub(crate) fn bounds_to(&self, cam: &Camera, top: f32, far: f32) -> (i32, i32, i32, i32) {
        /// Tiles the box may stretch beyond the ground the screen covers.
        const MAX_DEPTH: f32 = 24.0;
        if let Some((x0, y0, x1, y1)) = cam.reach(self.w, far) {
            let m = ((16.0 / cam.a()).ceil() as i32 + 2).max(5);
            return (x0.floor() as i32 - m, y0.floor() as i32 - m, x1.ceil() as i32 + m, y1.ceil() as i32 + m);
        }
        let corners = [(0.0, 0.0), (self.w as f32, 0.0), (0.0, self.h as f32), (self.w as f32, self.h as f32)];
        let ground = corners.map(|(sx, sy)| cam.unproject(sx, sy, 0.0));
        let fold = |f: fn(f32, f32) -> f32, pick: fn((f32, f32)) -> f32, ps: &[(f32, f32)]| ps.iter().fold(pick(ps[0]), |a, &p| f(a, pick(p)));
        let (gx0, gx1) = (fold(f32::min, |p| p.0, &ground), fold(f32::max, |p| p.0, &ground));
        let (gy0, gy1) = (fold(f32::min, |p| p.1, &ground), fold(f32::max, |p| p.1, &ground));
        let high = corners.map(|(sx, sy)| cam.unproject(sx, sy, top.max(0.0)));
        let x0 = fold(f32::min, |p| p.0, &high).max(gx0 - MAX_DEPTH).min(gx0);
        let x1 = fold(f32::max, |p| p.0, &high).min(gx1 + MAX_DEPTH).max(gx1);
        let y0 = fold(f32::min, |p| p.1, &high).max(gy0 - MAX_DEPTH).min(gy0);
        let y1 = fold(f32::max, |p| p.1, &high).min(gy1 + MAX_DEPTH).max(gy1);
        // Widest sprite art is about 16 columns either side of its tile,
        // and a crown reaches a few tiles from its trunk.
        let m = ((16.0 / cam.a()).ceil() as i32 + 2).max(5);
        (x0.floor() as i32 - m, y0.floor() as i32 - m, x1.ceil() as i32 + m, y1.ceil() as i32 + m)
    }

    /// How high the frame must look: the chunk ceilings over the tiles the
    /// view can reach, plus what a crown or a roof adds over them. Two
    /// passes, since a taller ceiling widens the box it was measured over.
    fn view_ceiling(&self, sc: &Scene) -> f32 {
        let mut top = 0.0f32;
        for _ in 0..2 {
            let (x0, y0, x1, y1) = self.visible_bounds(sc.cam, top);
            // One sample every half chunk, so no chunk in the box is missed.
            let step = 16;
            let line = |a: i32, b: i32| (a..=b).step_by(step as usize).chain(std::iter::once(b)).collect::<Vec<i32>>();
            let (xs, ys) = (line(x0, x1), line(y0, y1));
            let ceiling = ys.iter().flat_map(|y| xs.iter().map(move |x| (*x, *y))).map(|(x, y)| sc.map.ceiling(x, y)).max().unwrap_or(0);
            let next = (ceiling as f32 + crate::grid::CROWN_CAP).min(crate::grid::TOP_CAP);
            if next <= top {
                break;
            }
            top = next;
        }
        top
    }

    /// The tile range the frame is drawn over: the grid's, once it is built.
    pub(crate) fn tile_bounds(&self, cam: &Camera) -> (i32, i32, i32, i32) {
        match &self.heights {
            Some(g) => g.bounds(),
            None => self.visible_bounds(cam, 0.0),
        }
    }
}
