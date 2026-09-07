//! Headless frames: `roguemap --snap W H OUT key=value...` and the golden
//! test both render through here, so a frame is the same bytes whichever
//! way it is asked for.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crossterm::event::KeyCode;

use crate::assets::Assets;
use crate::blocks::Stack;
use crate::camera::Camera;
use crate::canvas::Canvas;
use crate::frame::{FrameCtx, Frames};
use crate::input::{self, Coupling, Held};
use crate::map::{FixtureSpec, Flora, Map, Terrain};
use crate::render::{FogMode, Renderer, Scene};
use crate::settings::Settings;
use crate::tileset::Tileset;
use crate::ui;
use crate::world::{self, World};
use crate::worldmap::WorldMap;

/// The game's tick in seconds, which `walk=` steps by.
pub const TICK: f32 = 0.04;

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
/// popover (1), fire (1 to place a campfire at centre), player (1) with
/// player_dx, player_dy (centimetres to walk them from the tile centre,
/// through the same move the keys make, so water refuses it), hud
/// (0|1), inset (0 off, 1..4 the corner of the inset view), worldmap (1)
/// with scale, open (frame names of ui.toml, comma separated), frames (N,
/// to time rendering), scene (`scale` for the yardstick of ADR-004: a
/// person, an oak and a house on flat ground), camera (isometric, chase,
/// shoulder or first-person: ADR-007), pitch and fov (degrees, for a
/// perspective camera), tilt (degrees, 30 to 90, of the isometric table:
/// ADR-009), fog (metres of visibility, 0 for no fade),
/// fogmode (perspective, always or never), px and py (the tile the
/// character stands on; a perspective view's default is cx, cy), walk
/// (`KEYS,SECONDS`: hold those walk keys that long in 40 ms ticks,
/// ADR-008) with run (1 for the run speed), coupling (body-turns or
/// view-only: whether the body turns with the view, ADR-009).
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
    settings.set("inset", a.num("inset", settings.get("inset") as f32) as usize);
    // camera=isometric|chase|shoulder|first-person picks the mode (ADR-007)
    // through its settings row; a perspective mode takes pitch= and fov=
    // in degrees, fov= through its row where the row lists the value.
    let mode = a.text("camera").and_then(|m| Camera::MODES.iter().position(|n| *n == m)).unwrap_or(0);
    settings.set("camera", mode);
    // coupling=body-turns|view-only is whether the body turns with the
    // view (ADR-009), which is what the walk keys below mean.
    if let Some(c) = a.text("coupling").and_then(|c| Coupling::NAMES.iter().position(|n| *n == c)) {
        settings.set("coupling", c);
    }
    let fov = a.kv.get("fov").and_then(|v| v.parse::<f32>().ok());
    if let Some(fov) = fov {
        settings.set_fov_near(fov);
        if settings.fov_degrees() != Some(fov) {
            settings.set("fov", 0);
        }
    }
    if let Some(fog) = a.text("fogmode").and_then(|m| FogMode::NAMES.iter().position(|n| *n == m)) {
        settings.set("fog", fog);
    }
    let zoom = a.kv.get("zoom").and_then(|v| v.parse().ok()).unwrap_or_else(|| Camera::fitting_zoom(&map, sw, sh));
    let mut cam = Camera::isometric(zoom);
    let opts = settings.apply(&mut map, &mut world, &mut cam);
    if cam.is_perspective() {
        if let Some(fov) = fov {
            cam.set_fov_override(Some(fov));
        }
        if let Some(pitch) = a.kv.get("pitch").and_then(|v| v.parse::<f32>().ok()) {
            cam.pitch_by(pitch.to_radians() - cam.pitch);
        }
    }
    let mut frames = ui::frames(&assets);
    ui::apply_settings(&mut frames, &settings);
    frames.set_open("settings", a.flag("popover"));
    frames.set_open("worldmap", a.flag("worldmap"));
    // open=name,name opens any other frame of ui.toml.
    for name in a.text("open").unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
        frames.set_open(name, true);
    }
    let ts = &tilesets[settings.get("glyphs")];

    let mut cv = Canvas::new(w, h);
    let mut renderer = Renderer::new(sw, sh);
    // tilt=DEGREES tilts the isometric table (ADR-009); the floor is what
    // every frame drew before it.
    cam.set_tilt(a.num("tilt", Camera::TILT_RANGE.0.to_degrees()).to_radians());
    cam.set_angle(std::f32::consts::FRAC_PI_4 + a.num("rot", 0.0) * std::f32::consts::FRAC_PI_2 + a.num("deg", 0.0).to_radians());
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
        world.spawn_player(&map, mx, my, cam.angle());
    } else if a.flag("player") || cam.is_perspective() {
        // A perspective view is placed from the character, so it always
        // has one, standing at the view centre; px, py put them elsewhere.
        let (px, py) = if cam.is_perspective() { (cx, cy) } else { (map.w as i32 / 2, map.h as i32 / 2) };
        world.spawn_player(&map, a.num("px", px as f32) as i32, a.num("py", py as f32) as i32, cam.angle());
    }
    // player_dx / player_dy walk the player from the spawn, in centimetres,
    // the way a run of keypresses would, so a stepped figure is reproducible.
    let (pdx, pdy) = (a.num("player_dx", 0.0) as i32, a.num("player_dy", 0.0) as i32);
    if (pdx, pdy) != (0, 0) {
        world.try_move(&map, pdx, pdy);
    }
    if a.flag("fire") {
        let (mx, my) = cam.center_tile(&map, sw, sh);
        world.light_campfire(&map, mx, my);
    }
    // A perspective view is placed from the character.
    if let Some(p) = world.player().filter(|_| cam.is_perspective()) {
        cam.look_at_entity(p, &map, sw, sh);
    }
    // walk=KEYS,SECONDS holds walk keys (w, a, s, d and the diagonals) for
    // that long in 40 ms ticks of the same held-key set, walk and camera
    // follow the game runs (ADR-008, ADR-009), with run=1 for the run
    // speed, so a frame mid-stride — or mid-curve under view-only — is
    // reproducible.
    if let Some((keys, secs)) = a.text("walk").and_then(|w| w.split_once(',')) {
        let ticks = (secs.parse::<f32>().unwrap_or(0.0) / TICK).round().max(0.0) as usize;
        let mut held = Held::new(true);
        for key in keys.chars() {
            if let Some((dir, run)) = input::lookup(input::SCENE, KeyCode::Char(key), false).and_then(Held::movement) {
                held.press(KeyCode::Char(key), dir, run || a.flag("run"));
            }
        }
        for _ in 0..ticks {
            input::walk_keys(&mut world, &cam, settings.coupling(), settings.screen_space(), &held);
            world.step_walk(&map, TICK);
            cam.follow(&world, &map, sw, sh);
        }
    }
    let t = a.num("t", 0.0);
    let mut wmap = WorldMap::new();
    wmap.scale = a.num("scale", 1.0) as usize;
    wmap.cursor = (cx, cy);

    // One whole frame: the scene, then the overlay frames over it. The
    // inset view renders inside the second half, so its cost is in the
    // number `frames=N` prints.
    let base = FrameCtx { map: &map, world: &world, cam: &cam, ts, settings: &settings, wmap: &wmap, lights: 0, t, focused: false, keys: "" };
    let one = |cv: &mut Canvas, renderer: &mut Renderer, frames: &mut Frames, t: f32| {
        let mut lights = 0;
        if !frames.is_open("worldmap") {
            // fog=N sets the visibility in metres for this frame; fog=0
            // leaves the far field unfaded.
            let mut scene = Scene::new(base.map, base.ts, base.world, base.cam, t).with_fog(opts.fog);
            if let Some(fog) = a.kv.get("fog").and_then(|v| v.parse::<f32>().ok()) {
                if fog > 0.0 {
                    scene.far = fog;
                    scene.fog = scene.fog.map(|_| fog);
                } else {
                    scene.fog = None;
                }
            }
            renderer.draw(cv, &scene, &opts);
            lights = base.world.lights.len() + renderer.frame_light_count();
        }
        let ctx = FrameCtx { lights, t, ..base };
        frames.update(&ctx);
        frames.draw(cv, &ctx);
    };

    // frames=N renders N frames first and prints the average time per frame.
    let repeats = a.num("frames", 0.0) as usize;
    if repeats > 0 {
        let start = Instant::now();
        for i in 0..repeats {
            one(&mut cv, &mut renderer, &mut frames, t + i as f32 * 0.04);
        }
        eprintln!("{:.2} ms/frame", start.elapsed().as_secs_f32() * 1000.0 / repeats as f32);
    }
    one(&mut cv, &mut renderer, &mut frames, t);
    cv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    /// The glyphs of a frame, one line per row, so a title drawn into a
    /// border can be read back.
    fn glyphs(cv: &Canvas) -> String {
        (0..cv.h).map(|y| (0..cv.w).map(|x| cv.cells[(y * cv.w + x) as usize].ch).collect::<String>()).collect::<Vec<String>>().join("\n")
    }

    fn shot(w: u16, h: u16, extra: &[&str]) -> String {
        let mut args = vec!["fill=1", "cx=0", "cy=0", "t=3", "tod=12", "player=1"];
        args.extend_from_slice(extra);
        glyphs(&render(test_assets(), w, h, &args))
    }

    #[test]
    fn the_inset_shows_on_a_wide_screen_at_the_other_end_of_the_scale() {
        // Zoomed out, the inset is the close view; at 1:1 it is the far one.
        assert!(shot(168, 71, &["zoom=1"]).contains("inset 1:1"), "the inset is titled with the ratio it draws at");
        assert!(shot(168, 71, &["zoom=3"]).contains("inset 1:8"), "at 1:1 the inset shows 1:8");
        // The floor size has no room for it, and the settings row can say
        // no on any screen.
        assert!(!shot(80, 25, &["zoom=1"]).contains("inset"), "no inset below a hundred columns");
        assert!(!shot(168, 71, &["zoom=1", "inset=0"]).contains("inset"), "the settings row turns it off");
        // The status bar names the zoom by ratio and name, not by index.
        let hud = shot(168, 71, &["zoom=3"]);
        let top = hud.lines().next().expect("a frame has rows");
        assert!(top.contains("45deg  1:1 close  "), "{top}");
        assert!(!top.contains("zoom "), "{top}");
    }

    #[test]
    fn a_centimetre_step_moves_the_figure_and_water_refuses_it() {
        // On the flat yardstick scene the player is the only twelve-row
        // figure; a step of a few cells moves it and nothing else.
        let base = glyphs(&render(test_assets(), 120, 40, &["scene=scale", "zoom=3", "t=3", "tod=12", "hud=0"]));
        let moved = glyphs(&render(test_assets(), 120, 40, &["scene=scale", "zoom=3", "t=3", "tod=12", "hud=0", "player_dx=25", "player_dy=25"]));
        assert_ne!(base, moved, "a 25 cm step is a visible row at 1:1");
        let world_of = |extra: &[&str]| {
            let mut args = vec!["player=1", "zoom=0", "t=3", "tod=12"];
            args.extend_from_slice(extra);
            render(test_assets(), 20, 8, &args)
        };
        // The argument goes through try_move: a step that lands off the island
        // is refused and the frame is the spawn's.
        assert_eq!(glyphs(&world_of(&[])), glyphs(&world_of(&["player_dx=-9999999"])));
    }

    #[test]
    fn a_walk_is_reproducible_and_moves_the_figure_into_its_stride() {
        let walk = |extra: &[&str]| {
            let mut args = vec!["scene=scale", "zoom=3", "t=3", "tod=12", "hud=0"];
            args.extend_from_slice(extra);
            glyphs(&render(test_assets(), 120, 40, &args))
        };
        let rest = walk(&[]);
        let once = walk(&["walk=d,0.3"]);
        assert_eq!(once, walk(&["walk=d,0.3"]), "the same ticks give the same frame");
        assert_ne!(once, rest, "0.3 s at 1.4 m/s is half a metre and a stride");
        assert_ne!(walk(&["walk=d,0.3", "run=1"]), once, "running covers more ground in the same time");
        assert_ne!(walk(&["walk=a,0.3"]), once, "left is the other way");
        assert_eq!(walk(&["walk=d,0"]), rest, "no time, no walk");
    }
}
