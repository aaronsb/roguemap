//! User preferences as one table from `settings.toml`. The popover renders
//! the table generically, code reads rows by key, and every shortcut key
//! edits the same values.

use crate::assets::Assets;
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
pub const REQUIRED_SETTINGS: [&str; 12] = ["traversal", "camera", "view", "glyphs", "hud", "inset", "clock", "weather", "wind", "day_length", "clouds", "antialias"];

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

    pub fn filled(&self) -> bool {
        self.get("view") == 1
    }

    /// Push the table into the objects that act on it, and return what the
    /// renderer needs to know.
    pub fn apply(&self, map: &mut Map, world: &mut World) -> RenderOptions {
        map.bounded = !self.filled();
        world.auto_time = self.get("clock") == 0;
        world.weather_preset = self.get("weather").checked_sub(1);
        world.wind_preset = self.get("wind").checked_sub(1);
        world.day_secs = world::DAY_LENGTHS[self.get("day_length")];
        RenderOptions { aa: self.get("antialias") == 0, clouds: self.get("clouds") == 0, fog: FogMode::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::camera::Camera;
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
        assert_eq!(a.setting("camera").unwrap().values[a.setting("camera").unwrap().default as usize], "isometric");
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
        let opts = s.apply(&mut map, &mut world);
        assert!(map.bounded && world.auto_time && world.weather_preset.is_none() && world.wind_preset.is_none());
        assert_eq!(world.day_secs, DAY_LENGTHS[s.get("day_length")]);
        assert!(opts.aa && opts.clouds, "the defaults draw everything");

        s.set("view", 1);
        s.set("clock", 1);
        s.set("weather", 4);
        s.set("wind", 3);
        s.set("day_length", 0);
        s.set("clouds", 1);
        s.set("antialias", 1);
        let opts = s.apply(&mut map, &mut world);
        assert!(!map.bounded, "filled view unbounds the map");
        assert!(!world.auto_time, "paused clock stops time");
        assert_eq!(world.weather_preset, Some(3), "storm is the last preset");
        assert_eq!(world.wind_preset, Some(2), "windy is the third");
        assert_eq!(world.day_secs, DAY_LENGTHS[0]);
        assert!(!opts.aa && !opts.clouds);

        s.set("weather", 0);
        s.set("wind", 0);
        s.apply(&mut map, &mut world);
        assert_eq!((world.weather_preset, world.wind_preset), (None, None), "auto rows clear the presets");
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
            if let Action::Cycle(key) = b.1 {
                assert!(a.setting(key).is_some(), "input.rs cycles unknown setting {key}");
            }
        }
        assert_eq!(input::lookup(SCENE, KeyCode::Char('v'), false), Some(Action::Cycle("view")));
    }
}
