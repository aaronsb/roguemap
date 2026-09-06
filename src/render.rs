//! The frame buffer the render passes share, and the order they run in.
//!
//! Terrain is drawn by inverse projection (raster.rs): every screen cell
//! walks down the height column under it until it meets a tile top or a
//! cliff face, so the camera can sit at any angle. Sprites (sprites.rs) are
//! billboards drawn back to front with a depth test against the terrain.
//! A lighting pass (lighting.rs) then lights every cell from ambient sky
//! light, the sun shadowed by drifting clouds, and point lights; overlays
//! (overlay.rs) add precipitation and the cloud layer.

use crate::assets::Assets;
use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::map::{Map, Tile, MAX_Z};
use crate::noise::hash;
use crate::palette::Palette;
use crate::raster::HeightGrid;
use crate::tileset::Tileset;
use crate::world::{Light, World};

/// What the renderer needs to know from the settings.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Supersample surface boundaries with sextant glyphs.
    pub aa: bool,
    /// Draw the cloud layer at the smallest zooms.
    pub clouds: bool,
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
}

impl<'a> Scene<'a> {
    /// Gather one frame's inputs; the seasonal palette and water choppiness
    /// are derived once here.
    pub fn new(map: &'a Map, ts: &'a Tileset, world: &'a World, cam: &'a Camera, t: f32) -> Scene<'a> {
        let assets: &'a Assets = &map.assets;
        Scene { map, assets, ts, pal: assets.surfaces.for_season(world.season), world, cam, t, chop: world.choppiness() }
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

/// What a screen cell's ray met in the height column.
#[derive(Clone, Copy)]
pub(crate) struct Hit {
    pub tile: Tile,
    pub mx: i32,
    pub my: i32,
    pub x: f32,
    pub y: f32,
    /// Surface height at the hit.
    pub h: f32,
    pub z: i32,
    pub face: u8,
    /// Steepness bands for cliff shading, 0 on gentle ground.
    pub below: i32,
    /// Sun-facing factor of the surface, -1 away to 1 toward.
    pub sun: f32,
}

pub struct Renderer {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) g: Vec<GCell>,
    /// Per-cell surface identity from the centre sample, for edge detection.
    pub(crate) ids: Vec<u64>,
    pub(crate) heights: Option<HeightGrid>,
    /// Lights discovered while drawing this frame, such as lit windows.
    pub(crate) frame_lights: Vec<Light>,
}

impl Renderer {
    pub fn new(w: i32, h: i32) -> Renderer {
        let sky = GCell { albedo: Rgb(0, 0, 0), ch: ' ', glyph: Rgb(0, 0, 0), wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
        Renderer { w, h, g: vec![sky; (w * h) as usize], ids: vec![0; (w * h) as usize], heights: None, frame_lights: Vec::new() }
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

    /// Draw one frame of the scene into `cv`.
    pub fn draw(&mut self, cv: &mut Canvas, sc: &Scene, opts: &RenderOptions) {
        self.frame_lights.clear();
        self.sky_pass(sc);
        let (x0, y0, x1, y1) = self.visible_bounds(sc.cam);
        self.heights = Some(HeightGrid::build(sc.map, x0, y0, x1, y1));
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
                self.g[(y * self.w + x) as usize] =
                    GCell { albedo: sky, ch, glyph, wx: 0.0, wy: 0.0, wz: 0.0, face: 0, lit: false, depth: SKY_DEPTH };
            }
        }
    }

    /// Map-space bounding box of everything that can appear on screen.
    pub(crate) fn visible_bounds(&self, cam: &Camera) -> (i32, i32, i32, i32) {
        let corners = [(0.0, 0.0), (self.w as f32, 0.0), (0.0, self.h as f32), (self.w as f32, self.h as f32)];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(sx, sy) in &corners {
            for z in [0.0, (MAX_Z + 24) as f32] {
                let (x, y) = cam.unproject(sx, sy, z);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        // Widest sprite art is about 16 columns either side of its tile.
        let m = (16.0 / cam.a()).ceil() as i32 + 2;
        (x0.floor() as i32 - m, y0.floor() as i32 - m, x1.ceil() as i32 + m, y1.ceil() as i32 + m)
    }
}
