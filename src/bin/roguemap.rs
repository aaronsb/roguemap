//! roguemap: the game binary. Argument parsing, the headless snapshot and
//! the event loop; everything else lives in the library.

use std::rc::Rc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};

use roguemap::assets::Assets;
use roguemap::camera::Camera;
use roguemap::canvas::Canvas;
use roguemap::frame::{Flow, FrameCtx, Frames, Item, List};
use roguemap::input::{self, Action, Held, Look, Mouse};
use roguemap::map::Map;
use roguemap::render::{Renderer, Scene};
use roguemap::settings::Settings;
use roguemap::tileset::Tileset;
use roguemap::world::World;
use roguemap::worldmap::WorldMap;
use roguemap::{lsystem, snapshot, terminal, ui};

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
    /// The corner the inset view was last in, so the toggle key can put it
    /// back where the settings row had it.
    inset_corner: usize,
    /// Whether the isometric view is still easing after a walk (ADR-008):
    /// set by a walk key, cleared once the figure is back inside the dead
    /// zone or by a pan, so the pan keys keep their effect.
    settling: bool,
    /// Whether the player was walking after the last tick, so the end of
    /// a walk can be logged once.
    walking: bool,
    /// The direction keys down (ADR-008); their sum is the heading each
    /// tick, so two of them walk a diagonal.
    held: Held,
    /// The pointer, which turns the view (ADR-008).
    mouse: Mouse,
    /// Screen size in cells.
    sw: i32,
    sh: i32,
}

impl App {
    fn new(assets: Rc<Assets>, seed: u64, size: usize, sw: i32, sh: i32, key_release: bool) -> App {
        let tilesets = Tileset::all(&assets);
        let frames = ui::frames(&assets);
        let mut map = Map::new(size, size, seed, assets.clone());
        let mut world = World::new(seed);
        let settings = Settings::new(&assets);
        let mut cam = Camera::new();
        settings.apply(&mut map, &mut world, &mut cam);
        cam.set_zoom(Camera::fitting_zoom(&map, sw, sh), sw, sh);
        cam.look_at(map.w as i32 / 2, map.h as i32 / 2, &map, sw, sh);
        world.spawn_player(&map, map.w as i32 / 2, map.h as i32 / 2);
        let inset_corner = settings.get("inset").max(1);
        App {
            map,
            world,
            cam,
            settings,
            inset_corner,
            settling: false,
            walking: false,
            held: Held::new(key_release),
            mouse: Mouse::new(),
            wmap: WorldMap::new(),
            frames,
            renderer: Renderer::new(sw, sh),
            tilesets,
            sw,
            sh,
        }
    }

    /// One tick of time (ADR-008): the clock and the weather, the heading
    /// the keys down make, the player's walk along it, and the camera
    /// easing after them — every tick in a perspective mode, and in the
    /// isometric mode while a walk is still settling. The end of a walk
    /// goes to the history.
    fn tick(&mut self, dt: f32) {
        self.world.tick(dt);
        self.walk_held();
        let walking = self.world.step_walk(&self.map, dt);
        if self.walking && !walking {
            if let Some((x, y)) = self.world.player().map(|p| p.metres()) {
                self.log(format!("walked to {x:.2}, {y:.2} m"));
            }
        }
        self.walking = walking;
        if self.cam.is_perspective() || self.settling {
            let settled = self.cam.follow(&self.world, &self.map, self.sw, self.sh);
            self.settling = !settled;
        }
        // The leases are spent after the walk, so a tap that renews none
        // of them walks exactly its grace's worth.
        self.held.tick(dt);
    }

    /// Point the walk where the keys down say (ADR-008): the normalised
    /// sum of what each one means, which is a diagonal for two of them and
    /// nothing for two opposite ones, held for as long as their leases
    /// have left. With no key down the figure stops on this tick.
    fn walk_held(&mut self) {
        // A focused frame owns the keyboard, so the figure stands still
        // under one; the keys count again when it closes.
        if self.frames.focus().is_some() {
            self.held.clear();
        }
        let Some(dir) = self.cam.held_heading(self.settings.screen_space(), self.held.dirs()) else {
            self.world.stop_walk();
            return;
        };
        self.world.walk_for(dir, self.held.running(), self.cam.facing_of(dir), self.held.grace());
        self.settling = true;
    }

    fn resize(&mut self, w: i32, h: i32) {
        self.sw = w;
        self.sh = h;
        self.renderer.resize(w, h);
        self.mouse.forget();
    }

    /// Turn the view with the pointer (ADR-008). A focused frame owns the
    /// screen, so the mouse does nothing under one and forgets where it
    /// was, and the next motion after it closes is a fresh start rather
    /// than a jump. The wheel narrows and widens a perspective view's
    /// field of view and steps the isometric zoom.
    fn mouse_event(&mut self, m: MouseEvent) {
        if self.frames.focus().is_some() {
            self.mouse.forget();
            return;
        }
        let Some(look) = self.mouse.event(m, self.settings.mouse_mode()) else { return };
        match look {
            Look::Turn(yaw, pitch) => {
                self.cam.rotate_by(yaw.to_radians(), self.sw, self.sh);
                // An isometric view has no pitch of its own to turn.
                self.cam.pitch_by(pitch.to_radians());
            }
            Look::Wheel(dir) if self.cam.is_perspective() => self.settings.step_fov(-dir, self.cam.fov_degrees()),
            Look::Wheel(dir) => self.cam.zoom_by(dir, self.sw, self.sh),
        }
    }

    /// Draw the scene and every open frame into `cv` at animation time
    /// `t`. A full-screen opaque frame covers the scene, so nothing is
    /// rendered under one.
    fn frame(&mut self, cv: &mut Canvas, t: f32) {
        let opts = self.settings.apply(&mut self.map, &mut self.world, &mut self.cam);
        ui::apply_settings(&mut self.frames, &self.settings);
        let corner = self.settings.get("inset");
        if corner != 0 {
            self.inset_corner = corner;
        }
        let ts = &self.tilesets[self.settings.get("glyphs")];
        let mut lights = 0;
        if !self.frames.is_open("worldmap") {
            self.renderer.draw(cv, &Scene::new(&self.map, ts, &self.world, &self.cam, t).with_fog(opts.fog), &opts);
            lights = self.world.lights.len() + self.renderer.frame_light_count();
        }
        let keys = if self.held.release_events() { "  keys:held" } else { "  keys:repeat" };
        let ctx = FrameCtx { map: &self.map, world: &self.world, cam: &self.cam, ts, settings: &self.settings, wmap: &self.wmap, lights, t, focused: false, keys };
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
    /// player. The inset view's own state is the `inset` settings row,
    /// which is pushed back into the frame set every frame, so its key
    /// flips the row between off and the corner it was last in.
    fn toggle(&mut self, name: &str) {
        if name == "inset" {
            let on = self.settings.get("inset") != 0;
            self.settings.set("inset", if on { 0 } else { self.inset_corner });
            return;
        }
        if self.frames.toggle(name) && name == "worldmap" {
            self.wmap.cursor = self.world.player().map(|e| e.tile()).unwrap_or((0, 0));
        }
    }

    /// Dispatch a key press: the focused frame first, then the binding
    /// table of the frames whose state the app owns, and the scene table
    /// when nothing has focus.
    fn handle_key(&mut self, k: KeyEvent) -> Loop {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Loop::Quit;
        }
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let Some(focused) = self.frames.focus().map(str::to_string) else {
            return match input::lookup(input::SCENE, k.code, shift) {
                Some(a) => self.scene_action(a, k.code),
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
                if let Some(a) = input::lookup(input::SETTINGS, k.code, shift) {
                    self.settings_action(a);
                }
            }
            "worldmap" => {
                if let Some(a) = input::lookup(input::WORLDMAP, k.code, shift) {
                    self.worldmap_action(a);
                }
            }
            _ => {}
        }
        Loop::Continue
    }

    fn scene_action(&mut self, a: Action, code: KeyCode) -> Loop {
        let (sw, sh) = (self.sw, self.sh);
        if let Some((dir, run)) = Held::movement(a) {
            // A direction key does not move the figure; it joins the keys
            // down, and the tick walks their sum (ADR-008).
            self.held.press(code, dir, run);
            return Loop::Continue;
        }
        match a {
            Action::Quit => return Loop::Quit,
            Action::Toggle(name) => self.toggle(name),
            Action::Pan(dx, dy) => {
                self.cam.pan(dx, dy);
                self.settling = false;
            }
            Action::Centre => {
                if let Some(p) = self.world.player() {
                    self.cam.look_at_entity(p, &self.map, sw, sh);
                }
            }
            Action::RotateQuarter(steps) => self.cam.rotate(steps, sw, sh),
            Action::RotateDegrees(deg) => self.cam.rotate_by(deg.to_radians(), sw, sh),
            Action::Zoom(steps) => self.cam.zoom_by(steps, sw, sh),
            Action::Pitch(deg) => self.cam.pitch_by(deg.to_radians()),
            Action::Step(key, dir) => {
                if key == "fov" {
                    self.settings.step_fov(dir, self.cam.fov_degrees());
                }
            }
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

/// Headless mode: `--snap-tree NAME OUT [cols rows seed season]
/// [foliage=F] [state=dead]` draws one L-system species side-on and writes
/// the cell dump to OUT, for `python3 tools/cells2png.py` (docs/lsystem.md).
fn snap_tree(assets: &Assets, args: &[String]) -> std::io::Result<()> {
    let ([name, out], rest) = (args.get(..2).map(|a| [&a[0], &a[1]]).ok_or_else(|| std::io::Error::other("usage: roguemap --snap-tree NAME OUT.cells [cols rows seed season] [foliage=F] [state=dead]"))?, &args[2..]);
    lsystem::snap(assets, name, rest).map_err(std::io::Error::other)?.dump(out)
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
    if argv.get(1).map(|s| s.as_str()) == Some("--snap-tree") {
        return snap_tree(&assets, &argv[2..]);
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

    // The hook restores the terminal before the message prints, so a
    // panic is readable instead of going to the alternate screen with a
    // raw, enhanced keyboard behind it.
    terminal::install_panic_hook();
    let mut term = terminal::Terminal::with_key_release()?;
    term.capture_mouse()?;
    let mut app = App::new(assets, seed, size, term.width(), term.height(), term.key_release());

    let start = Instant::now();
    let frame = Duration::from_millis(40);
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;
        app.tick(dt);
        let t = start.elapsed().as_secs_f32();

        app.frame(term.canvas(), t);
        term.present()?;

        while event::poll(frame.saturating_sub(now.elapsed()))? {
            match event::read()? {
                // A repeat is the terminal saying the key is still down,
                // which is a press to every binding: without release
                // events it is the only thing that renews a walk, and
                // with them it is what a repeating key has always done.
                Event::Key(k) if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat => {
                    if let Loop::Quit = app.handle_key(k) {
                        return Ok(());
                    }
                }
                // A release lifts a direction key whatever modifiers it
                // comes back with, and whatever frame has focus, so a walk
                // never outlives the key that started it (ADR-008).
                Event::Key(k) if k.kind == KeyEventKind::Release => app.held.release(k.code),
                Event::Mouse(m) => app.mouse_event(m),
                Event::Resize(w, h) => {
                    term.resize(w, h);
                    app.resize(w as i32, h as i32);
                }
                _ => {}
            }
        }
    }
}
