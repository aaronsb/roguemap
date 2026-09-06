//! User preferences as one table from `settings.toml`. The popover renders
//! the table generically, code reads rows by key, and every shortcut key
//! edits the same values.

use crate::assets::Assets;
use crate::properties::Identity;

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
pub const REQUIRED_SETTINGS: [&str; 10] = ["traversal", "view", "glyphs", "hud", "clock", "weather", "wind", "day_length", "clouds", "antialias"];

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
        assert_eq!(input::lookup(SCENE, KeyCode::Char('v')), Some(Action::Cycle("view")));
    }
}
