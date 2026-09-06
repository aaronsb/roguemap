//! Headless frames: `roguemap --snap W H OUT key=value...` and the golden
//! test both render through here, so a frame is the same bytes whichever
//! way it is asked for.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::assets::Assets;
use crate::blocks::Stack;
use crate::camera::Camera;
use crate::canvas::Canvas;
use crate::frame::FrameCtx;
use crate::map::{FixtureSpec, Flora, Map, Terrain};
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
/// (0|1), worldmap (1) with scale, open (frame names of ui.toml, comma
/// separated), frames (N, to time rendering), scene (`scale` for the
/// yardstick of ADR-004: a person, an oak and a house on flat ground).
pub fn render<S: AsRef<str>>(assets: Rc<Assets>, w: u16, h: u16, args: &[S]) -> Canvas {
    let a = SnapArgs::parse(args);
    let (sw, sh) = (w as i32, h as i32);
    let seed = a.num("seed", 7.0) as u64;
    let size = a.num("size", 32.0) as usize;
    let tilesets = Tileset::all(&assets);
    // `scene=scale` is the yardstick of ADR-004: flat ground with a 2 m
    // person between an 18 m oak and a house, so one frame per zoom shows
    // what a metre is worth there.
    let scale = a.text("scene") == Some("scale");
    let mut map = if scale {
        let biome = assets.koppen.get("Cf").copied().unwrap_or(0);
        let spec = FixtureSpec { w: 24, h: 24, z: 3, biome, terrain: Terrain::Grass, temp: 10, grass: 2, material: assets.biomes[biome].material };
        Map::fixture(spec, assets.clone())
    } else {
        Map::new(size, size, seed, assets.clone())
    };
    let mut world = World::new(seed);

    let mut settings = Settings::new(&assets);
    settings.set("view", (a.num("fill", 0.0) >= 0.5) as usize);
    settings.set("hud", (a.num("hud", 1.0) <= 0.5) as usize);
    let glyphs = a.text("glyphs").unwrap_or("petscii");
    settings.set("glyphs", settings.items[settings.find("glyphs").unwrap()].values.iter().position(|v| v == glyphs).unwrap_or(0));
    let opts = settings.apply(&mut map, &mut world);
    let mut frames = ui::frames(&assets);
    let hud = settings.get("hud") == 0;
    frames.set_open("hud-top", hud);
    frames.set_open("hud-help", hud);
    frames.set_open("settings", a.flag("popover"));
    frames.set_open("worldmap", a.flag("worldmap"));
    // open=name,name opens any other frame of ui.toml.
    for name in a.text("open").unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
        frames.set_open(name, true);
    }
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
    if scale {
        // The three yardsticks side by side: a 2 m person, an 18 m oak and a
        // house of four tiles by three under one roof.
        let (mx, my) = (map.w as i32 / 2, map.h as i32 / 2);
        let oak = assets.species.iter().position(|s| s.name == "oak").unwrap_or(0) as u8;
        let house = assets.blocks.iter().position(|b| b.name == "house").unwrap_or(0) as u8;
        if let Some(mut t) = map.get(mx - 4, my) {
            t.tree = Some(Flora { species: oak, variant: 2 });
            map.set_tile(mx - 4, my, t);
        }
        for dy in 0..3 {
            for dx in 0..4 {
                map.set_stack(mx + 3 + dx, my - 1 + dy, Some(Stack { kind: house, levels: 1 }));
            }
        }
        world.spawn_player(&map, mx, my);
    } else if a.flag("player") {
        world.spawn_player(&map, map.w as i32 / 2, map.h as i32 / 2);
    }
    if a.flag("fire") {
        let (mx, my) = cam.center_tile(&map, sw, sh);
        world.light_campfire(&map, mx, my);
    }
    let t = a.num("t", 0.0);
    // frames=N renders N extra frames and prints the average time per frame.
    let repeats = a.num("frames", 0.0) as usize;
    if repeats > 0 {
        let start = Instant::now();
        for i in 0..repeats {
            renderer.draw(&mut cv, &Scene::new(&map, ts, &world, &cam, t + i as f32 * 0.04), &opts);
        }
        eprintln!("{:.2} ms/frame", start.elapsed().as_secs_f32() * 1000.0 / repeats as f32);
    }
    let mut wmap = WorldMap::new();
    wmap.scale = a.num("scale", 1.0) as usize;
    wmap.cursor = (cx, cy);
    let mut lights = 0;
    if !frames.is_open("worldmap") {
        renderer.draw(&mut cv, &Scene::new(&map, ts, &world, &cam, t), &opts);
        lights = world.lights.len() + renderer.frame_light_count();
    }
    let ctx = FrameCtx { map: &map, world: &world, cam: &cam, ts, settings: &settings, wmap: &wmap, lights, focused: false };
    frames.update(&ctx);
    frames.draw(&mut cv, &ctx);
    cv
}
