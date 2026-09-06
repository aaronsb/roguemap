//! Headless frames: `roguemap --snap W H OUT key=value...` and the golden
//! test both render through here, so a frame is the same bytes whichever
//! way it is asked for.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::assets::Assets;
use crate::camera::Camera;
use crate::canvas::Canvas;
use crate::map::Map;
use crate::render::{Renderer, Scene};
use crate::settings::Settings;
use crate::tileset::Tileset;
use crate::world::{self, World};
use crate::worldmap::WorldMap;
use crate::ui;

/// `key=value` arguments of a headless snapshot.
pub struct SnapArgs {
    pub kv: HashMap<String, String>,
}

impl SnapArgs {
    /// Parse `key=value` words; anything without `=` is ignored.
    pub fn parse<S: AsRef<str>>(args: &[S]) -> SnapArgs {
        let mut kv = HashMap::new();
        for a in args {
            if let Some((k, v)) = a.as_ref().split_once('=') {
                kv.insert(k.to_string(), v.to_string());
            }
        }
        SnapArgs { kv }
    }

    pub fn num(&self, key: &str, default: f32) -> f32 {
        self.kv.get(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(default)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.num(key, 0.0) > 0.5
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.kv.get(key).map(|s| s.as_str())
    }
}

/// Render one frame of `w` x `h` cells from `key=value` arguments.
/// Keys: seed, t, tod, season, cover, wind, precip (0..1), simdays (run a
/// storm that many days first), glyphs (petscii|ascii), rot, deg, zoom,
/// size, fill (1 for an unbounded world), cx, cy (tile to centre on),
/// popover (1), fire (1 to place a campfire at centre), player (1), hud
/// (0|1), worldmap (1) with scale, frames (N, to time rendering).
pub fn render<S: AsRef<str>>(assets: Rc<Assets>, w: u16, h: u16, args: &[S]) -> Canvas {
    let a = SnapArgs::parse(args);
    let (sw, sh) = (w as i32, h as i32);
    let seed = a.num("seed", 7.0) as u64;
    let size = a.num("size", 32.0) as usize;
    let tilesets = Tileset::all(&assets);
    let mut map = Map::new(size, size, seed, assets.clone());
    let mut world = World::new(seed);

    let mut settings = Settings::new(&assets);
    settings.set("view", (a.num("fill", 0.0) >= 0.5) as usize);
    settings.set("hud", (a.num("hud", 1.0) <= 0.5) as usize);
    let glyphs = a.text("glyphs").unwrap_or("petscii");
    settings.set("glyphs", settings.items[settings.find("glyphs").unwrap()].values.iter().position(|v| v == glyphs).unwrap_or(0));
    settings.open = a.flag("popover");
    let opts = settings.apply(&mut map, &mut world);
    let ts = &tilesets[settings.get("glyphs")];

    let mut cv = Canvas::new(w, h);
    let mut renderer = Renderer::new(sw, sh);
    let mut cam = Camera::new();
    cam.angle = std::f32::consts::FRAC_PI_4 + a.num("rot", 0.0) * std::f32::consts::FRAC_PI_2 + a.num("deg", 0.0).to_radians();
    let zoom = a.kv.get("zoom").and_then(|v| v.parse().ok()).unwrap_or_else(|| Camera::fitting_zoom(&map, sw, sh));
    cam.set_zoom(zoom, sw, sh);
    let (cx, cy) = (a.num("cx", map.w as f32 / 2.0) as i32, a.num("cy", map.h as f32 / 2.0) as i32);
    cam.look_at(cx, cy, &map, sw, sh);

    let (tod, precip) = (a.num("tod", 13.0), a.num("precip", 0.0));
    world.tod = tod;
    world.season = a.num("season", 1.0);
    world.weather.cover = a.num("cover", 0.3);
    world.weather.wind = a.num("wind", 0.2);
    world.weather.precip = precip;
    // Optionally run a storm for some days first to build accumulations,
    // at one simulated day per second of clock.
    let warm = a.num("simdays", 0.0);
    if warm > 0.0 {
        world.day_secs = 1.0;
        world.weather_preset = Some(world::STORM);
        let mut acc = 0.0;
        while acc < warm {
            world.tick(0.05);
            acc += 0.05;
        }
        world.tod = tod;
        world.weather.precip = precip;
    }
    if a.flag("player") {
        world.spawn_player(&map, map.w as i32 / 2, map.h as i32 / 2);
    }
    if a.flag("fire") {
        let (mx, my) = cam.center_tile(&map, sw, sh);
        world.light_campfire(&map, mx, my);
    }
    let t = a.num("t", 0.0);
    // frames=N renders N extra frames and prints the average time per frame.
    let frames = a.num("frames", 0.0) as usize;
    if frames > 0 {
        let start = Instant::now();
        for i in 0..frames {
            renderer.draw(&mut cv, &Scene::new(&map, ts, &world, &cam, t + i as f32 * 0.04), &opts);
        }
        eprintln!("{:.2} ms/frame", start.elapsed().as_secs_f32() * 1000.0 / frames as f32);
    }
    if a.flag("worldmap") {
        let mut wm = WorldMap::new();
        wm.scale = a.num("scale", 1.0) as usize;
        wm.cursor = (cx, cy);
        wm.draw(&mut cv, &map, &world, world.player().map(|e| (e.mx, e.my)));
    } else {
        renderer.draw(&mut cv, &Scene::new(&map, ts, &world, &cam, t), &opts);
        if settings.get("hud") == 0 {
            ui::hud(&mut cv, &map, ts, &world, &cam, world.lights.len() + renderer.frame_light_count());
        }
        if settings.open {
            ui::popover(&mut cv, &settings);
        }
    }
    cv
}
