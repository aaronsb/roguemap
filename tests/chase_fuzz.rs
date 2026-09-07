//! A long random session in the perspective modes, rendered every tick
//! the way the game loop does: walks and runs on held keys with a
//! variable tick, mouse turns and pitches, wheel zooms, mode switches
//! through the settings row, at several screen sizes and seeds. The
//! snapshot renders one frame after a walk; this renders all of them.
//! It is what found the crown-sweep panic that `crown_shadow.rs` pins:
//! a single frame at a fixed hour rarely lands in the low-sun window, and
//! a session that lets the clock run meets it within a few hundred ticks.

use std::rc::Rc;

use crossterm::event::KeyCode;
use roguemap::assets::Assets;
use roguemap::camera::Camera;
use roguemap::canvas::Canvas;
use roguemap::input::{self, Held};
use roguemap::map::Map;
use roguemap::render::{Renderer, Scene};
use roguemap::settings::Settings;
use roguemap::tileset::Tileset;
use roguemap::world::World;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 10000) as f32 / 10000.0
    }
}

fn session(seed: u64, size: usize, sw: i32, sh: i32, ticks: usize, rng: &mut Lcg) {
    let assets = Rc::new(Assets::load().expect("assets"));
    let tilesets = Tileset::all(&assets);
    let mut map = Map::new(size, size, seed, assets.clone());
    let mut world = World::new(seed);
    let mut settings = Settings::new(&assets);
    settings.set("view", rng.below(2) as usize);
    let mut cam = Camera::new();
    settings.apply(&mut map, &mut world, &mut cam);
    cam.set_zoom(Camera::fitting_zoom(&map, sw, sh), sw, sh);
    cam.look_at(map.w as i32 / 2, map.h as i32 / 2, &map, sw, sh);
    world.spawn_player(&map, map.w as i32 / 2, map.h as i32 / 2, cam.angle());
    let mut renderer = Renderer::new(sw, sh);
    let mut cv = Canvas::new(sw as u16, sh as u16);
    let mut held = Held::new(true);
    let keys = ['w', 'a', 's', 'd', 'q', 'e', 'z', 'x'];
    let modes = [1usize, 1, 1, 2, 3, 0, 4];
    settings.set("camera", 1);
    let mut settling = false;
    let mut t = 0.0f32;
    for i in 0..ticks {
        // Vary the tick like a real loop: mostly 40 ms, sometimes a stall.
        let dt = match rng.below(20) {
            0 => 0.5 + rng.unit(),
            1 => 0.001,
            _ => 0.03 + rng.unit() * 0.02,
        };
        t += dt;
        // Random input events between frames.
        match rng.below(12) {
            0 => {
                let k = keys[rng.below(keys.len() as u64) as usize];
                if let Some((dir, run)) = input::lookup(input::SCENE, KeyCode::Char(k), false).and_then(Held::movement) {
                    held.press(KeyCode::Char(k), dir, run || rng.below(2) == 0);
                }
            }
            1 => {
                let k = keys[rng.below(keys.len() as u64) as usize];
                held.release(KeyCode::Char(k));
            }
            2 => {
                let yaw = (rng.unit() - 0.5) * 40.0;
                let pitch = (rng.unit() - 0.5) * 30.0;
                cam.rotate_by(yaw.to_radians(), sw, sh);
                cam.pitch_by(pitch.to_radians());
            }
            3 => {
                let dir = if rng.below(2) == 0 { 1 } else { -1 };
                if cam.is_perspective() {
                    settings.step_fov(-dir, cam.fov_degrees());
                } else {
                    cam.zoom_by(dir, sw, sh);
                }
            }
            4 if rng.below(4) == 0 => {
                settings.set("camera", modes[rng.below(modes.len() as u64) as usize]);
            }
            5 if rng.below(3) == 0 => {
                settings.set("coupling", rng.below(2) as usize);
            }
            6 if rng.below(3) == 0 => {
                cam.zoom_by(if rng.below(2) == 0 { 1 } else { -1 }, sw, sh);
            }
            7 if rng.below(6) == 0 => {
                cam.rotate(if rng.below(2) == 0 { 1 } else { -1 }, sw, sh);
            }
            _ => {}
        }
        // App::tick.
        world.tick(dt);
        if cam.addresses_character() {
            settling |= input::walk_keys(&mut world, &cam, settings.coupling(), settings.screen_space(), &held);
            world.step_walk(&map, dt);
        } else if let Some((f, r)) = Camera::held_fly(held.dirs()) {
            cam.fly(f * 1.4 * dt, r * 1.4 * dt);
        }
        if cam.addresses_character() && (cam.is_perspective() || settling) {
            settling = !cam.follow(&world, &map, sw, sh);
        }
        held.tick(dt);
        // App::frame.
        let opts = settings.apply(&mut map, &mut world, &mut cam);
        let ts = &tilesets[settings.get("glyphs")];
        let scene = Scene::new(&map, ts, &world, &cam, t).with_fog(opts.fog);
        let label = format!("seed {seed} size {size} {sw}x{sh} tick {i} mode {} eye {:?} anchor {:?}", cam.mode_name(), cam.eye(), cam.anchor_point());
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.draw(&mut cv, &scene, &opts)));
        if let Err(e) = r {
            panic!("render panicked at {label}: {e:?}");
        }
    }
}

#[test]
fn a_long_random_perspective_session_renders_every_tick() {
    let seeds: Vec<u64> = std::env::var("FUZZ_SEEDS").ok().map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(|| (1..=4).collect());
    let ticks: usize = std::env::var("FUZZ_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    for &seed in &seeds {
        let mut rng = Lcg(seed * 7919 + 13);
        let size = [16, 24, 32, 48][(seed % 4) as usize];
        let (sw, sh) = [(80, 25), (120, 40), (168, 71)][(seed % 3) as usize];
        eprintln!("seed {seed} size {size} {sw}x{sh}");
        session(seed, size, sw, sh, ticks, &mut rng);
    }
}
