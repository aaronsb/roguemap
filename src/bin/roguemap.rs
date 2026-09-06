//! roguemap: the game binary. Argument parsing, the headless snapshot and
//! the event loop; everything else lives in the library.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use roguemap::assets::Assets;
use roguemap::camera::Camera;
use roguemap::canvas::Canvas;
use roguemap::input::{self, Action};
use roguemap::map::Map;
use roguemap::render::{RenderOptions, Renderer, Scene};
use roguemap::settings::Settings;
use roguemap::tileset::Tileset;
use roguemap::world::World;
use roguemap::worldmap::WorldMap;
use roguemap::{terminal, ui, world};

/// Push the settings table into the objects that act on it, and return
/// what the renderer needs to know.
fn apply(settings: &Settings, map: &mut Map, world: &mut World) -> RenderOptions {
    map.bounded = !settings.filled();
    world.auto_time = settings.get("clock") == 0;
    world.weather_preset = settings.get("weather").checked_sub(1);
    world.wind_preset = settings.get("wind").checked_sub(1);
    world.day_secs = world::DAY_LENGTHS[settings.get("day_length")];
    RenderOptions { aa: settings.get("antialias") == 0, clouds: settings.get("clouds") == 0 }
}

/// Whether the main loop goes on after a key.
enum Flow {
    Continue,
    Quit,
}

/// Everything the interactive session holds.
struct App {
    map: Map,
    world: World,
    cam: Camera,
    settings: Settings,
    wmap: WorldMap,
    renderer: Renderer,
    /// One per value of the glyphs setting.
    tilesets: Vec<Tileset>,
    /// Screen size in cells.
    sw: i32,
    sh: i32,
}

impl App {
    fn new(assets: Rc<Assets>, seed: u64, size: usize, sw: i32, sh: i32) -> App {
        let tilesets = Tileset::all(&assets);
        let mut map = Map::new(size, size, seed, assets.clone());
        let mut world = World::new(seed);
        let settings = Settings::new(&assets);
        apply(&settings, &mut map, &mut world);
        let mut cam = Camera::new();
        cam.set_zoom(Camera::fitting_zoom(&map, sw, sh), sw, sh);
        cam.look_at(map.w as i32 / 2, map.h as i32 / 2, &map, sw, sh);
        world.spawn_player(&map, map.w as i32 / 2, map.h as i32 / 2);
        App {
            map,
            world,
            cam,
            settings,
            wmap: WorldMap::new(),
            renderer: Renderer::new(sw, sh),
            tilesets,
            sw,
            sh,
        }
    }

    fn resize(&mut self, w: i32, h: i32) {
        self.sw = w;
        self.sh = h;
        self.renderer.resize(w, h);
    }

    /// Draw the current mode into `cv` at animation time `t`.
    fn frame(&mut self, cv: &mut Canvas, t: f32) {
        let opts = apply(&self.settings, &mut self.map, &mut self.world);
        let ts = &self.tilesets[self.settings.get("glyphs")];
        if self.wmap.open {
            let player = self.world.player().map(|e| (e.mx, e.my));
            self.wmap.draw(cv, &self.map, &self.world, player);
            return;
        }
        self.renderer.draw(cv, &Scene::new(&self.map, ts, &self.world, &self.cam, t), &opts);
        if self.settings.get("hud") == 0 {
            ui::hud(cv, &self.map, ts, &self.world, &self.cam, self.world.lights.len() + self.renderer.frame_light_count());
        }
        if self.settings.open {
            ui::popover(cv, &self.settings);
        }
    }

    /// Dispatch a key press to the table for the current mode.
    fn handle_key(&mut self, k: KeyEvent) -> Flow {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Flow::Quit;
        }
        if self.wmap.open {
            if let Some(a) = input::lookup(input::WORLDMAP, k.code) {
                self.worldmap_action(a);
            }
        } else if self.settings.open {
            if let Some(a) = input::lookup(input::SETTINGS, k.code) {
                self.settings_action(a);
            }
        } else if let Some(a) = input::lookup(input::SCENE, k.code) {
            return self.scene_action(a);
        }
        Flow::Continue
    }

    fn scene_action(&mut self, a: Action) -> Flow {
        let (sw, sh) = (self.sw, self.sh);
        match a {
            Action::Quit => return Flow::Quit,
            Action::OpenSettings => self.settings.open = true,
            Action::OpenWorldMap => {
                self.wmap.cursor = self.world.player().map(|e| (e.mx, e.my)).unwrap_or((0, 0));
                self.wmap.open = true;
            }
            Action::Walk(dx, dy) => self.walk((dx, dy)),
            Action::Pan(dx, dy) => self.cam.pan(dx, dy),
            Action::Centre => {
                if let Some(p) = self.world.player() {
                    self.cam.look_at(p.mx, p.my, &self.map, sw, sh);
                }
            }
            Action::RotateQuarter(steps) => self.cam.rotate(steps, sw, sh),
            Action::RotateDegrees(deg) => self.cam.rotate_by(deg.to_radians(), sw, sh),
            Action::Zoom(steps) => self.cam.zoom_by(steps, sw, sh),
            Action::Cycle(key) => self.settings.cycle(key, 1),
            Action::StepSeason(q) => self.world.step_season(q),
            Action::StepHour(h) => self.world.step_hour(h),
            Action::Campfire => self.light_campfire(),
            Action::ClearFires => self.world.lights.clear(),
            _ => {}
        }
        Flow::Continue
    }

    fn settings_action(&mut self, a: Action) {
        match a {
            Action::Close => self.settings.open = false,
            Action::CursorMove(dir) => self.settings.move_cursor(dir),
            Action::Adjust(dir) => self.settings.cycle_row(self.settings.cursor, dir),
            _ => {}
        }
    }

    fn worldmap_action(&mut self, a: Action) {
        match a {
            Action::Close => self.wmap.open = false,
            Action::CursorStep(dx, dy) => self.wmap.move_cursor(dx, dy),
            Action::Extent(dir) => self.wmap.step_extent(dir),
            Action::Teleport => {
                let (tx, ty) = self.map.nearest_land(self.wmap.cursor.0, self.wmap.cursor.1);
                if let Some(p) = self.world.player_mut() {
                    p.mx = tx;
                    p.my = ty;
                }
                self.cam.look_at(tx, ty, &self.map, self.sw, self.sh);
                self.wmap.open = false;
            }
            _ => {}
        }
    }

    /// Move the player one step and keep the camera on them. In screen
    /// space a key moves the figure that way on screen, which is a diagonal
    /// in map space; in map-axes mode keys follow the map's own north and
    /// east.
    fn walk(&mut self, dir: (i32, i32)) {
        let (dx, dy) = if self.settings.screen_space() { self.cam.screen_dir_to_map(dir.0, dir.1) } else { dir };
        self.world.try_move(&self.map, dx, dy);
        self.cam.follow(&self.world, &self.map, self.sw, self.sh);
    }

    /// Light a campfire on the tile at the screen centre.
    fn light_campfire(&mut self) {
        let (mx, my) = self.cam.center_tile(&self.map, self.sw, self.sh);
        self.world.light_campfire(&self.map, mx, my);
    }
}

/// `key=value` arguments of a headless snapshot.
struct SnapArgs {
    kv: HashMap<String, String>,
}

impl SnapArgs {
    fn parse(args: &[String]) -> SnapArgs {
        let mut kv = HashMap::new();
        for a in args {
            if let Some((k, v)) = a.split_once('=') {
                kv.insert(k.to_string(), v.to_string());
            }
        }
        SnapArgs { kv }
    }

    fn num(&self, key: &str, default: f32) -> f32 {
        self.kv.get(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(default)
    }

    fn flag(&self, key: &str) -> bool {
        self.num(key, 0.0) > 0.5
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.kv.get(key).map(|s| s.as_str())
    }
}

/// Headless mode: `--snap W H OUT [key=value...]` renders one frame and dumps it.
/// Keys: seed, t, tod, season, cover, wind, precip (0..1), simdays (run a
/// storm that many days first), glyphs (petscii|ascii),
/// rot, deg, zoom, size, fill (1 for an unbounded world), cx, cy (tile to
/// centre on), popover (1), fire (1 to place a campfire at centre), player
/// (1), hud (0|1), worldmap (1) with scale, frames (N, to time rendering).
fn snapshot(assets: Rc<Assets>, args: &[String]) -> std::io::Result<()> {
    let w: u16 = args[0].parse().unwrap_or(200);
    let h: u16 = args[1].parse().unwrap_or(60);
    let out = &args[2];
    let a = SnapArgs::parse(&args[3..]);
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
    let opts = apply(&settings, &mut map, &mut world);
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
    cv.dump(out)
}

fn main() -> std::io::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    // Load and validate the tables before touching the terminal, so a bad
    // file is reported on a plain console.
    let assets = match Assets::load() {
        Ok(a) => Rc::new(a),
        Err(e) => {
            eprintln!("roguemap: cannot load assets: {e}");
            std::process::exit(1);
        }
    };
    if argv.get(1).map(|s| s.as_str()) == Some("--snap") {
        return snapshot(assets, &argv[2..]);
    }
    // `--export-assets DIR` writes the loaded set out for editing.
    if argv.get(1).map(|s| s.as_str()) == Some("--export-assets") {
        let Some(dir) = argv.get(2) else {
            eprintln!("usage: roguemap --export-assets DIR");
            std::process::exit(2);
        };
        assets.export(std::path::Path::new(dir))?;
        eprintln!("wrote {} files to {dir}", assets.files().len());
        return Ok(());
    }
    let seed: u64 = argv.get(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let size: usize = argv.get(2).and_then(|s| s.parse().ok()).unwrap_or(32);

    let mut term = terminal::Terminal::new()?;
    let mut app = App::new(assets, seed, size, term.width(), term.height());

    let start = Instant::now();
    let frame = Duration::from_millis(40);
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;
        app.world.tick(dt);
        let t = start.elapsed().as_secs_f32();

        app.frame(term.canvas(), t);
        term.present()?;

        while event::poll(frame.saturating_sub(now.elapsed()))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if let Flow::Quit = app.handle_key(k) {
                        return Ok(());
                    }
                }
                Event::Resize(w, h) => {
                    term.resize(w, h);
                    app.resize(w as i32, h as i32);
                }
                _ => {}
            }
        }
    }
}
