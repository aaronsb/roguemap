//! roguemap: an isometric, height-mapped terrain renderer for the terminal.

mod canvas;
mod map;
mod noise;
mod palette;
mod render;
mod settings;
mod terminal;
mod tileset;
mod world;

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use settings::{Settings, CLOCK, GLYPHS, HUD, WEATHER, WORLD};

/// Push the settings table into the objects that act on it.
fn apply(settings: &Settings, map: &mut map::Map, world: &mut world::World, renderer: &mut render::Renderer) {
    map.bounded = !settings.filled();
    world.auto_time = settings.get(CLOCK) == 0;
    world.weather = match settings.get(WEATHER) {
        1 => world::Weather::Rain,
        2 => world::Weather::Snow,
        _ => world::Weather::Clear,
    };
    renderer.show_hud = settings.get(HUD) == 0;
}

/// Headless mode: `--snap W H OUT [key=value...]` renders one frame and dumps it.
/// Keys: seed, t, tod, season, weather (clear|rain|snow), glyphs (petscii|ascii),
/// rot, zoom, size, fill (1 for an unbounded world), popover (1), fire (1 to
/// place a campfire at centre), player (1), hud (0|1).
fn snapshot(args: &[String]) -> std::io::Result<()> {
    let w: u16 = args[0].parse().unwrap_or(200);
    let h: u16 = args[1].parse().unwrap_or(60);
    let out = &args[2];
    let mut kv = std::collections::HashMap::new();
    for a in &args[3..] {
        if let Some((k, v)) = a.split_once('=') {
            kv.insert(k.to_string(), v.to_string());
        }
    }
    let get = |k: &str, d: f32| kv.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let seed = get("seed", 7.0) as u64;
    let size = get("size", 32.0) as usize;
    let mut map = map::Map::new(size, size, seed);
    map.bounded = get("fill", 0.0) < 0.5;
    let mut settings = Settings::new();
    settings.set(WORLD, (!map.bounded) as usize);
    settings.open = get("popover", 0.0) > 0.5;
    let ts = if kv.get("glyphs").map(|s| s.as_str()) == Some("ascii") { tileset::Tileset::ascii() } else { tileset::Tileset::petscii() };
    let mut cv = canvas::Canvas::new(w, h);
    let mut renderer = render::Renderer::new(w as i32, h as i32);
    renderer.show_hud = get("hud", 1.0) > 0.5;
    let mut cam = render::Camera::new();
    cam.rot = get("rot", 0.0) as u8;
    let zoom = kv.get("zoom").and_then(|v| v.parse().ok()).unwrap_or_else(|| render::Camera::fitting_zoom(&map, w as i32, h as i32));
    cam.set_zoom(zoom, &map, w as i32, h as i32);
    cam.look_at(map.w as i32 / 2, map.h as i32 / 2, &map, w as i32, h as i32);
    let mut world = world::World::new();
    world.tod = get("tod", 13.0);
    world.season = get("season", 1.0);
    world.weather = match kv.get("weather").map(|s| s.as_str()) {
        Some("rain") => world::Weather::Rain,
        Some("snow") => world::Weather::Snow,
        _ => world::Weather::Clear,
    };
    if get("player", 0.0) > 0.5 {
        world.entities.push(spawn(&map));
    }
    if get("fire", 0.0) > 0.5 {
        let (mx, my) = cam.center_tile(&map, w as i32, h as i32);
        let z = map.get(mx, my).map(|t| t.draw_z()).unwrap_or(map::SEA);
        world.add_campfire(mx, my, z);
    }
    renderer.draw(&mut cv, &map, &ts, &world, &cam, &settings, get("t", 0.0));
    cv.dump(out)
}

/// Find a land tile near the map centre for the player to start on.
fn spawn(map: &map::Map) -> world::Entity {
    let (cx, cy) = (map.w as i32 / 2, map.h as i32 / 2);
    for r in 0..map.w as i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if let Some(t) = map.get(cx + dx, cy + dy) {
                    if t.terrain != map::Terrain::Water {
                        return world::Entity { mx: cx + dx, my: cy + dy };
                    }
                }
            }
        }
    }
    world::Entity { mx: cx, my: cy }
}

/// Move the player one step, refusing water and the map edge. In screen
/// space a key moves the figure that way on screen, which is a diagonal in
/// map space; in map-axes mode keys follow the map's own north and east.
fn walk(world: &mut world::World, map: &map::Map, cam: &render::Camera, settings: &Settings, dir: (i32, i32)) {
    let (dx, dy) = if settings.screen_space() {
        let (dvx, dvy) = match dir {
            (0, -1) => (-1, -1),
            (0, 1) => (1, 1),
            (-1, 0) => (-1, 1),
            _ => (1, -1),
        };
        cam.view_delta_to_map(dvx, dvy)
    } else {
        dir
    };
    if let Some(p) = world.entities.first_mut() {
        let (nx, ny) = (p.mx + dx, p.my + dy);
        if let Some(t) = map.get(nx, ny) {
            if t.terrain != map::Terrain::Water {
                p.mx = nx;
                p.my = ny;
            }
        }
    }
}

/// Recentre when the player leaves the middle of the screen.
fn follow(cam: &mut render::Camera, world: &world::World, map: &map::Map, sw: i32, sh: i32) {
    if let Some(p) = world.entities.first() {
        let (vx, vy) = cam.to_view(p.mx, p.my, map);
        let z = map.get(p.mx, p.my).map(|t| t.draw_z()).unwrap_or(0);
        let (sx, sy) = cam.project(vx, vy, z);
        if sx < sw / 5 || sx > sw * 4 / 5 || sy < sh / 5 || sy > sh * 4 / 5 {
            cam.look_at(p.mx, p.my, map, sw, sh);
        }
    }
}

fn main() -> std::io::Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(|s| s.as_str()) == Some("--snap") {
        return snapshot(&argv[2..]);
    }
    let seed: u64 = argv.get(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let size: usize = argv.get(2).and_then(|s| s.parse().ok()).unwrap_or(32);
    let mut map = map::Map::new(size, size, seed);
    let tilesets = [tileset::Tileset::petscii(), tileset::Tileset::ascii()];
    let mut settings = Settings::new();

    let mut term = terminal::Terminal::new()?;
    let (sw, sh) = (term.width(), term.height());
    let mut renderer = render::Renderer::new(sw, sh);
    let mut cam = render::Camera::new();
    cam.set_zoom(render::Camera::fitting_zoom(&map, sw, sh), &map, sw, sh);
    cam.look_at(map.w as i32 / 2, map.h as i32 / 2, &map, sw, sh);
    let mut world = world::World::new();
    world.entities.push(spawn(&map));

    let start = Instant::now();
    let frame = Duration::from_millis(40);
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;
        world.tick(dt, 120.0);
        let t = start.elapsed().as_secs_f32();

        let (sw, sh) = (term.width(), term.height());
        apply(&settings, &mut map, &mut world, &mut renderer);
        let ts = &tilesets[settings.get(GLYPHS)];
        renderer.draw(term.canvas(), &map, ts, &world, &cam, &settings, t);
        term.present()?;

        while event::poll(frame.saturating_sub(now.elapsed()))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press && settings.open => match k.code {
                    KeyCode::Esc | KeyCode::Tab | KeyCode::Char('q') => settings.open = false,
                    KeyCode::Up | KeyCode::Char('k') => settings.cursor = (settings.cursor + settings::ITEMS.len() - 1) % settings::ITEMS.len(),
                    KeyCode::Down | KeyCode::Char('j') => settings.cursor = (settings.cursor + 1) % settings::ITEMS.len(),
                    KeyCode::Left | KeyCode::Char('h') => settings.cycle(settings.cursor, -1),
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => settings.cycle(settings.cursor, 1),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                    _ => {}
                },
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Tab | KeyCode::Char('o') => settings.open = true,
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                    KeyCode::Left => cam.ox += 2 * cam.hw,
                    KeyCode::Right => cam.ox -= 2 * cam.hw,
                    KeyCode::Up => cam.oy += 2 * cam.hh,
                    KeyCode::Down => cam.oy -= 2 * cam.hh,
                    KeyCode::Char('z') => cam.set_zoom(cam.zoom + 1, &map, sw, sh),
                    KeyCode::Char('Z') => cam.set_zoom(cam.zoom + tileset::ZOOMS.len() - 1, &map, sw, sh),
                    KeyCode::Char('c') => {
                        if let Some(p) = world.entities.first() {
                            cam.look_at(p.mx, p.my, &map, sw, sh);
                        }
                    }
                    KeyCode::Char('w') | KeyCode::Char('k') => { walk(&mut world, &map, &cam, &settings, (0, -1)); follow(&mut cam, &world, &map, sw, sh); }
                    KeyCode::Char('s') | KeyCode::Char('j') => { walk(&mut world, &map, &cam, &settings, (0, 1)); follow(&mut cam, &world, &map, sw, sh); }
                    KeyCode::Char('a') | KeyCode::Char('h') => { walk(&mut world, &map, &cam, &settings, (-1, 0)); follow(&mut cam, &world, &map, sw, sh); }
                    KeyCode::Char('d') | KeyCode::Char('l') => { walk(&mut world, &map, &cam, &settings, (1, 0)); follow(&mut cam, &world, &map, sw, sh); }
                    KeyCode::Char('r') => cam.rotate(1, &map, sw, sh),
                    KeyCode::Char('R') => cam.rotate(-1, &map, sw, sh),
                    KeyCode::Char('g') => settings.cycle(GLYPHS, 1),
                    KeyCode::Char('v') => settings.cycle(WORLD, 1),
                    KeyCode::Char('[') => world.season -= 0.25,
                    KeyCode::Char(']') => world.season += 0.25,
                    KeyCode::Char(',') => world.tod = (world.tod - 1.0).rem_euclid(24.0),
                    KeyCode::Char('.') => world.tod = (world.tod + 1.0).rem_euclid(24.0),
                    KeyCode::Char('p') => settings.cycle(CLOCK, 1),
                    KeyCode::Char('W') => settings.cycle(WEATHER, 1),
                    KeyCode::Char('f') => {
                        let (mx, my) = cam.center_tile(&map, sw, sh);
                        if let Some(tile) = map.get(mx, my) {
                            if tile.terrain != map::Terrain::Water {
                                world.add_campfire(mx, my, tile.draw_z());
                            }
                        }
                    }
                    KeyCode::Char('F') => world.lights.clear(),
                    KeyCode::Char('H') => settings.cycle(HUD, 1),
                    _ => {}
                },
                Event::Resize(w, h) => {
                    term.resize(w, h);
                    renderer.resize(w as i32, h as i32);
                }
                _ => {}
            }
        }
    }
}
