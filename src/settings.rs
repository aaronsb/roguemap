//! User preferences as one table from `settings.toml`. The popover renders
//! the table generically, code reads rows by key, and every shortcut key
//! edits the same values.

use crate::assets::Assets;
use crate::camera::Camera;
use crate::input::{Coupling, MouseMode};
use crate::map::Map;
use crate::properties::Identity;
use crate::render::{FogMode, RenderOptions};
use crate::world::{self, World};

/// One row of the settings table: a stable key for saved files and code,
/// a display label and its cyclable values.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingItem {
    pub key: String,
    pub identity: Identity,
    pub label: String,
    pub values: Vec<String>,
    pub default: u8,
    /// Scene key that cycles the row, mirrored by the binding table in
    /// `input.rs`.
    pub shortcut: Option<char>,
}

/// Keys the engine reads; loading fails if one is missing.
pub const REQUIRED_SETTINGS: [&str; 16] = ["traversal", "mouse", "camera", "coupling", "fov", "fog", "view", "glyphs", "hud", "inset", "clock", "weather", "wind", "day_length", "clouds", "antialias"];

pub struct Settings {
    pub items: Vec<SettingItem>,
    pub values: Vec<usize>,
    /// Whether the popover is open, and which row is selected.
    pub open: bool,
    pub cursor: usize,
}

impl Settings {
    pub fn new(assets: &Assets) -> Settings {
        let items = assets.settings.clone();
        let values = items.iter().map(|i| i.default as usize).collect();
        Settings { items, values, open: false, cursor: 0 }
    }

    /// Row index of a key, for saved files and scripts.
    pub fn find(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|i| i.key == key)
    }

    /// Row index of a key the engine relies on; the loader guarantees the
    /// required keys exist.
    fn row(&self, key: &str) -> usize {
        self.find(key).unwrap_or_else(|| panic!("setting {key:?} is not in settings.toml"))
    }

    pub fn get(&self, key: &str) -> usize {
        self.values[self.row(key)]
    }

    pub fn set(&mut self, key: &str, value: usize) {
        let i = self.row(key);
        self.values[i] = value % self.items[i].values.len();
    }

    /// Step a row's value forward or back, wrapping.
    pub fn cycle(&mut self, key: &str, dir: i32) {
        let i = self.row(key);
        self.cycle_row(i, dir);
    }

    /// Step the value of a row by index, wrapping.
    pub fn cycle_row(&mut self, row: usize, dir: i32) {
        let n = self.items[row].values.len() as i32;
        self.values[row] = (self.values[row] as i32 + dir).rem_euclid(n) as usize;
    }

    /// Move the popover cursor, wrapping.
    pub fn move_cursor(&mut self, dir: i32) {
        let n = self.items.len() as i32;
        self.cursor = (self.cursor as i32 + dir).rem_euclid(n) as usize;
    }

    /// The label of a row's current value.
    pub fn label(&self, row: usize) -> &str {
        &self.items[row].values[self.values[row]]
    }

    pub fn screen_space(&self) -> bool {
        self.get("traversal") == 0
    }

    /// What the mouse does with the view (ADR-008).
    pub fn mouse_mode(&self) -> MouseMode {
        MouseMode::from_index(self.get("mouse"))
    }

    /// Whether the body turns with the view (ADR-009).
    pub fn coupling(&self) -> Coupling {
        Coupling::from_index(self.get("coupling"))
    }

    pub fn filled(&self) -> bool {
        self.get("view") == 1
    }

    /// The field of view the `fov` row asks for, in degrees, or `None` for
    /// the camera mode's own.
    pub fn fov_degrees(&self) -> Option<f32> {
        let row = self.row("fov");
        self.items[row].values[self.values[row]].parse().ok()
    }

    /// Put the `fov` row on the value nearest `degrees`, for the keys that
    /// step it from wherever the camera stands.
    pub fn set_fov_near(&mut self, degrees: f32) {
        let row = self.row("fov");
        let nearest = self.items[row].values.iter().enumerate().filter_map(|(i, v)| v.parse::<f32>().ok().map(|d| (i, (d - degrees).abs()))).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = nearest {
            self.values[row] = i;
        }
    }

    /// Step the `fov` row's degrees by `dir` without wrapping through
    /// `preset`: from `preset` the step starts at the value nearest what
    /// the camera shows.
    pub fn step_fov(&mut self, dir: i32, from_degrees: f32) {
        let row = self.row("fov");
        if self.values[row] == 0 {
            self.set_fov_near(from_degrees);
        }
        let n = self.items[row].values.len() as i32;
        self.values[row] = (self.values[row] as i32 + dir).clamp(1, n - 1) as usize;
    }

    /// Push the table into the objects that act on it — the map, the
    /// world and the camera, which takes its mode and field of view — and
    /// return what the renderer needs to know.
    pub fn apply(&self, map: &mut Map, world: &mut World, cam: &mut Camera) -> RenderOptions {
        map.bounded = !self.filled();
        world.auto_time = self.get("clock") == 0;
        world.weather_preset = self.get("weather").checked_sub(1);
        world.wind_preset = self.get("wind").checked_sub(1);
        world.day_secs = world::DAY_LENGTHS[self.get("day_length")];
        let mode = self.get("camera");
        if cam.mode_index() != mode {
            *cam = cam.in_mode(mode);
        }
        cam.set_fov_override(self.fov_degrees());
        RenderOptions { aa: self.get("antialias") == 0, clouds: self.get("clouds") == 0, fog: FogMode::from_index(self.get("fog")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::input::{self, Action, SCENE};
    use crate::world::{DAY_LENGTHS, WEATHER_PRESETS, WIND_PRESETS};
    use crossterm::event::KeyCode;

    #[test]
    fn preset_tables_align_with_settings_rows() {
        let a = test_assets();
        // Row 0 of each preset row is "auto"; the rest index the world table.
        let weather = a.setting("weather").unwrap();
        let wind = a.setting("wind").unwrap();
        assert_eq!(weather.values.len(), WEATHER_PRESETS.len() + 1);
        assert_eq!(wind.values.len(), WIND_PRESETS.len() + 1);
        assert_eq!(a.setting("day_length").unwrap().values.len(), DAY_LENGTHS.len());
        assert_eq!(weather.values[0], "auto");
        assert_eq!(wind.values[0], "auto");
        // The camera row's values are the camera's own modes (ADR-007).
        assert_eq!(a.setting("camera").unwrap().values, Camera::MODES);
        assert_eq!(a.setting("camera").unwrap().values[a.setting("camera").unwrap().default as usize], "table");
        // The fog row's values are the renderer's fog modes, and the field
        // of view row is `preset` then whole degrees, rising.
        assert_eq!(a.setting("fog").unwrap().values, FogMode::NAMES);
        // The mouse row's values are the modes the pointer has (ADR-008),
        // and each of them is what the setting reads back.
        let mouse = a.setting("mouse").unwrap();
        assert_eq!(mouse.values, MouseMode::NAMES);
        assert_eq!(mouse.values[mouse.default as usize], "drag");
        let mut s = Settings::new(&a);
        for (i, mode) in [MouseMode::Drag, MouseMode::Free, MouseMode::Off].into_iter().enumerate() {
            s.set("mouse", i);
            assert_eq!(s.mouse_mode(), mode, "{}", MouseMode::NAMES[i]);
        }
        // The coupling row's values are the couplings (ADR-009), and each
        // of them is what the setting reads back.
        let coupling = a.setting("coupling").unwrap();
        assert_eq!(coupling.values, Coupling::NAMES);
        assert_eq!(coupling.values[coupling.default as usize], "body-turns");
        for (i, c) in [Coupling::BodyTurns, Coupling::ViewOnly].into_iter().enumerate() {
            s.set("coupling", i);
            assert_eq!(s.coupling(), c, "{}", Coupling::NAMES[i]);
        }
        let fov = a.setting("fov").unwrap();
        assert_eq!(fov.values[fov.default as usize], "preset");
        let degrees: Vec<f32> = fov.values[1..].iter().map(|v| v.parse::<f32>().expect("a whole number of degrees")).collect();
        assert!(degrees.windows(2).all(|w| w[1] > w[0]) && degrees[0] >= 20.0 && *degrees.last().unwrap() <= 120.0, "{degrees:?}");
    }

    #[test]
    fn keys_are_unique_and_findable() {
        let s = Settings::new(&test_assets());
        for (i, item) in s.items.iter().enumerate() {
            assert_eq!(s.find(&item.key), Some(i), "{}", item.key);
        }
        assert_eq!(s.get("day_length"), 1);
    }

    #[test]
    fn every_row_cycles_through_all_its_values_and_wraps() {
        let mut s = Settings::new(&test_assets());
        for row in 0..s.items.len() {
            let n = s.items[row].values.len();
            let start = s.values[row];
            let mut seen = Vec::with_capacity(n);
            for _ in 0..n {
                s.cycle_row(row, 1);
                seen.push(s.values[row]);
            }
            assert_eq!(s.values[row], start, "{}: {n} steps forward come back round", s.items[row].key);
            seen.sort_unstable();
            assert_eq!(seen, (0..n).collect::<Vec<usize>>(), "{}: every value is visited once", s.items[row].key);
            for _ in 0..n {
                s.cycle_row(row, -1);
            }
            assert_eq!(s.values[row], start, "{}: and {n} steps back", s.items[row].key);
            s.cycle_row(row, -1);
            assert_eq!(s.values[row], (start + n - 1) % n, "{}: one step back from the start wraps to the end", s.items[row].key);
            s.cycle(&s.items[row].key.clone(), 1);
            assert_eq!(s.values[row], start, "cycling by key is the same as by row");
            assert_eq!(s.label(row), s.items[row].values[start]);
        }
    }

    #[test]
    fn apply_pushes_every_row_into_what_it_governs() {
        let assets = test_assets();
        let mut s = Settings::new(&assets);
        let mut map = Map::new(4, 4, 1, assets.clone());
        let mut world = World::new(1);
        let mut cam = Camera::new();
        cam.look_at(2, 2, &map, 120, 40);
        let opts = s.apply(&mut map, &mut world, &mut cam);
        assert!(map.bounded && world.auto_time && world.weather_preset.is_none() && world.wind_preset.is_none());
        assert_eq!(world.day_secs, DAY_LENGTHS[s.get("day_length")]);
        assert!(opts.aa && opts.clouds, "the defaults draw everything");
        assert_eq!(opts.fog, FogMode::Perspective);
        assert!(!cam.is_perspective() && cam.mode_index() == 0, "the default camera is the table");

        s.set("view", 1);
        s.set("clock", 1);
        s.set("weather", 4);
        s.set("wind", 3);
        s.set("day_length", 0);
        s.set("clouds", 1);
        s.set("antialias", 1);
        s.set("camera", 1);
        s.set("fov", 8);
        s.set("fog", 2);
        let opts = s.apply(&mut map, &mut world, &mut cam);
        assert!(cam.is_perspective() && cam.mode_name() == "chase", "the camera row switches the mode");
        assert!((cam.fov_degrees() - 100.0).abs() < 1e-3, "the fov row overrides the preset's");
        assert_eq!(opts.fog, FogMode::Never);
        assert!(!map.bounded, "filled view unbounds the map");
        assert!(!world.auto_time, "paused clock stops time");
        assert_eq!(world.weather_preset, Some(3), "storm is the last preset");
        assert_eq!(world.wind_preset, Some(2), "windy is the third");
        assert_eq!(world.day_secs, DAY_LENGTHS[0]);
        assert!(!opts.aa && !opts.clouds);

        s.set("weather", 0);
        s.set("wind", 0);
        s.set("fov", 0);
        s.apply(&mut map, &mut world, &mut cam);
        assert_eq!((world.weather_preset, world.wind_preset), (None, None), "auto rows clear the presets");
        assert!((cam.fov_degrees() - 60.0).abs() < 1e-3, "preset gives the chase view its own sixty degrees");
        s.set("camera", 0);
        s.apply(&mut map, &mut world, &mut cam);
        assert!(!cam.is_perspective(), "and back to the table");
        // The fov keys step the row's degrees from wherever the camera
        // shows and never wrap through `preset`.
        s.set("camera", 2);
        s.apply(&mut map, &mut world, &mut cam);
        assert_eq!(s.fov_degrees(), None);
        s.step_fov(1, cam.fov_degrees());
        assert_eq!(s.fov_degrees(), Some(50.0), "one step wider than the shoulder view's forty");
        for _ in 0..20 {
            s.step_fov(-1, cam.fov_degrees());
        }
        assert_eq!(s.fov_degrees(), Some(30.0), "clamped at the narrow end, not wrapped to preset");
        for _ in 0..20 {
            s.step_fov(1, cam.fov_degrees());
        }
        assert_eq!(s.fov_degrees(), Some(110.0));
        s.set("view", 5);
        assert_eq!(s.get("view"), 1, "set wraps into the row's values");
    }

    #[test]
    fn cycle_and_cursor_wrap() {
        let mut s = Settings::new(&test_assets());
        s.cycle("view", -1);
        assert!(s.filled());
        s.cycle("view", 1);
        assert!(!s.filled());
        s.move_cursor(-1);
        assert_eq!(s.cursor, s.items.len() - 1);
        s.move_cursor(1);
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn shortcuts_agree_with_the_binding_table() {
        let a = test_assets();
        for item in &a.settings {
            let bound = SCENE.iter().flat_map(|b| b.keys.iter()).find_map(|(k, act)| match (k, act) {
                (KeyCode::Char(c), Action::Cycle(key)) if *key == item.key => Some(*c),
                _ => None,
            });
            assert_eq!(bound, item.shortcut, "settings.toml and input.rs disagree on the shortcut for {}", item.key);
        }
        for b in SCENE.iter().flat_map(|b| b.keys.iter()) {
            if let Action::Cycle(key) | Action::Step(key, _) = b.1 {
                assert!(a.setting(key).is_some(), "input.rs cycles unknown setting {key}");
            }
        }
        assert_eq!(input::lookup(SCENE, KeyCode::Char('>'), true), Some(Action::Step("fov", 1)));
        assert_eq!(input::lookup(SCENE, KeyCode::Char('v'), false), Some(Action::Cycle("view")));
    }
}
