//! roguemap: the game binary. Argument parsing, the headless snapshot and
//! the event loop; everything else lives in the library.

use std::rc::Rc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use roguemap::assets::Assets;
use roguemap::camera::Camera;
use roguemap::canvas::Canvas;
use roguemap::frame::{Flow, FrameCtx, Frames, Item, List};
use roguemap::input::{self, Action};
use roguemap::map::Map;
use roguemap::render::{Renderer, Scene};
use roguemap::settings::Settings;
use roguemap::tileset::Tileset;
use roguemap::world::World;
use roguemap::worldmap::WorldMap;
use roguemap::{snapshot, terminal, ui};

/// Whether the main loop goes on after a key.
enum Loop {
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
    /// The overlay frames of ADR-005; which of them are open is the mode.
    frames: Frames,
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
        let frames = ui::frames(&assets);
        let mut map = Map::new(size, size, seed, assets.clone());
        let mut world = World::new(seed);
        let settings = Settings::new(&assets);
        settings.apply(&mut map, &mut world);
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
            frames,
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

    /// Draw the scene and every open frame into `cv` at animation time
    /// `t`. A full-screen opaque frame covers the scene, so nothing is
    /// rendered under one.
    fn frame(&mut self, cv: &mut Canvas, t: f32) {
        let opts = self.settings.apply(&mut self.map, &mut self.world);
        let hud = self.settings.get("hud") == 0;
        self.frames.set_open("hud-top", hud);
        self.frames.set_open("hud-help", hud);
        let ts = &self.tilesets[self.settings.get("glyphs")];
        let mut lights = 0;
        if !self.frames.is_open("worldmap") {
            self.renderer.draw(cv, &Scene::new(&self.map, ts, &self.world, &self.cam, t), &opts);
            lights = self.world.lights.len() + self.renderer.frame_light_count();
        }
        let ctx = FrameCtx { map: &self.map, world: &self.world, cam: &self.cam, ts, settings: &self.settings, wmap: &self.wmap, lights, focused: false };
        self.frames.update(&ctx);
        self.frames.draw(cv, &ctx);
    }

    /// Add a line to the history frame, stamped with the time of day.
    fn log(&mut self, text: impl Into<String>) {
        let stamp = format!("{:02}:{:02}", self.world.tod.floor() as i32, (self.world.tod.fract() * 60.0) as i32);
        if let Some(list) = self.frames.content_mut::<List>("history") {
            list.push(Item::detailed(text, stamp));
        }
    }

    /// Open or close a frame. Opening the world map puts its cursor on the
    /// player.
    fn toggle(&mut self, name: &str) {
        if self.frames.toggle(name) && name == "worldmap" {
            self.wmap.cursor = self.world.player().map(|e| (e.mx, e.my)).unwrap_or((0, 0));
        }
    }

    /// Dispatch a key press: the focused frame first, then the binding
    /// table of the frames whose state the app owns, and the scene table
    /// when nothing has focus.
    fn handle_key(&mut self, k: KeyEvent) -> Loop {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Loop::Quit;
        }
        let Some(focused) = self.frames.focus().map(str::to_string) else {
            return match input::lookup(input::SCENE, k.code) {
                Some(a) => self.scene_action(a),
                None => Loop::Continue,
            };
        };
        match self.frames.key(k.code) {
            Flow::Handled | Flow::Close => return Loop::Continue,
            Flow::Submit(line) => {
                self.log(format!("said \"{line}\""));
                return Loop::Continue;
            }
            Flow::Pass => {}
        }
        match focused.as_str() {
            "settings" => {
                if let Some(a) = input::lookup(input::SETTINGS, k.code) {
                    self.settings_action(a);
                }
            }
            "worldmap" => {
                if let Some(a) = input::lookup(input::WORLDMAP, k.code) {
                    self.worldmap_action(a);
                }
            }
            _ => {}
        }
        Loop::Continue
    }

    fn scene_action(&mut self, a: Action) -> Loop {
        let (sw, sh) = (self.sw, self.sh);
        match a {
            Action::Quit => return Loop::Quit,
            Action::Toggle(name) => self.toggle(name),
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
            Action::Cycle(key) => {
                self.settings.cycle(key, 1);
                if key == "weather" {
                    let row = self.settings.find(key).expect("the loader checks every required row exists");
                    self.log(format!("weather set to {}", self.settings.label(row)));
                }
            }
            Action::StepSeason(q) => self.world.step_season(q),
            Action::StepHour(h) => self.world.step_hour(h),
            Action::Campfire => self.light_campfire(),
            Action::ClearFires => {
                self.world.lights.clear();
                self.log("put the fires out");
            }
            _ => {}
        }
        Loop::Continue
    }

    fn settings_action(&mut self, a: Action) {
        match a {
            Action::Close => self.frames.set_open("settings", false),
            Action::CursorMove(dir) => self.settings.move_cursor(dir),
            Action::Adjust(dir) => self.settings.cycle_row(self.settings.cursor, dir),
            _ => {}
        }
    }

    fn worldmap_action(&mut self, a: Action) {
        match a {
            Action::Close => self.frames.set_open("worldmap", false),
            Action::CursorStep(dx, dy) => self.wmap.move_cursor(dx, dy),
            Action::Extent(dir) => self.wmap.step_extent(dir),
            Action::Teleport => {
                let (tx, ty) = self.wmap.teleport(&self.map, &mut self.world);
                self.cam.look_at(tx, ty, &self.map, self.sw, self.sh);
                self.frames.set_open("worldmap", false);
                self.log(format!("teleported to {tx}, {ty}"));
            }
            _ => {}
        }
    }

    /// Move the player one step and keep the camera on them. In screen
    /// space a key moves the figure that way on screen, which is a diagonal
    /// in map space; in map-axes mode keys follow the map's own north and
    /// east.
    fn walk(&mut self, dir: (i32, i32)) {
        let (dx, dy) = self.cam.walk_step(self.settings.screen_space(), dir.0, dir.1);
        if self.world.try_move(&self.map, dx, dy) {
            if let Some((mx, my)) = self.world.player().map(|p| (p.mx, p.my)) {
                self.log(format!("walked to {mx}, {my}"));
            }
        }
        self.cam.follow(&self.world, &self.map, self.sw, self.sh);
    }

    /// Light a campfire on the tile at the screen centre.
    fn light_campfire(&mut self) {
        let (mx, my) = self.cam.center_tile(&self.map, self.sw, self.sh);
        if self.world.light_campfire(&self.map, mx, my) {
            self.log(format!("lit a campfire at {mx}, {my}"));
        }
    }
}

/// Headless mode: `--snap W H OUT [key=value...]` renders one frame through
/// `snapshot::render` and writes the cell dump to OUT.
fn snap(assets: Rc<Assets>, args: &[String]) -> std::io::Result<()> {
    let w: u16 = args[0].parse().unwrap_or(200);
    let h: u16 = args[1].parse().unwrap_or(60);
    let out = &args[2];
    snapshot::render(assets, w, h, &args[3..]).dump(out)
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
        return snap(assets, &argv[2..]);
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
                    if let Loop::Quit = app.handle_key(k) {
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
