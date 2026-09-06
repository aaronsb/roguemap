//! User preferences as one table. The popover renders the table generically
//! and every shortcut key edits the same fields.

/// One row of the settings table: a stable key for saved files, a display
/// name and its cyclable values.
pub struct Item {
    pub key: &'static str,
    pub name: &'static str,
    pub values: &'static [&'static str],
}

pub const TRAVERSAL: usize = 0;
pub const VIEW: usize = 1;
pub const GLYPHS: usize = 2;
pub const HUD: usize = 3;
pub const CLOCK: usize = 4;
pub const WEATHER: usize = 5;
pub const WIND: usize = 6;
pub const DAY_LENGTH: usize = 7;
pub const CLOUDS: usize = 8;
pub const AA: usize = 9;

pub const ITEMS: [Item; 10] = [
    Item { key: "traversal", name: "Traversal", values: &["screen space", "map axes"] },
    Item { key: "view", name: "World view", values: &["island", "filled"] },
    Item { key: "glyphs", name: "Glyphs", values: &["petscii", "ascii"] },
    Item { key: "hud", name: "HUD", values: &["shown", "hidden"] },
    Item { key: "clock", name: "Clock", values: &["running", "paused"] },
    Item { key: "weather", name: "Weather", values: &["auto", "clear", "cloudy", "rain", "storm"] },
    Item { key: "wind", name: "Wind", values: &["auto", "calm", "breeze", "windy", "gale"] },
    Item { key: "day_length", name: "Day length", values: &["2 min", "10 min", "1 hour", "24 hours"] },
    Item { key: "clouds", name: "Cloud layer", values: &["shown", "hidden"] },
    Item { key: "antialias", name: "Antialias", values: &["on", "off"] },
];

pub struct Settings {
    pub values: [usize; ITEMS.len()],
    /// Whether the popover is open, and which row is selected.
    pub open: bool,
    pub cursor: usize,
}

impl Settings {
    pub fn new() -> Settings {
        let mut values = [0; ITEMS.len()];
        values[DAY_LENGTH] = 1;
        Settings { values, open: false, cursor: 0 }
    }

    /// Row index of a key, for saved files and scripts.
    #[allow(dead_code)]
    pub fn find(key: &str) -> Option<usize> {
        ITEMS.iter().position(|i| i.key == key)
    }

    pub fn get(&self, item: usize) -> usize {
        self.values[item]
    }

    pub fn set(&mut self, item: usize, value: usize) {
        self.values[item] = value % ITEMS[item].values.len();
    }

    /// Step a row's value forward or back, wrapping.
    pub fn cycle(&mut self, item: usize, dir: i32) {
        let n = ITEMS[item].values.len() as i32;
        self.values[item] = (self.values[item] as i32 + dir).rem_euclid(n) as usize;
    }

    /// Move the popover cursor, wrapping.
    pub fn move_cursor(&mut self, dir: i32) {
        let n = ITEMS.len() as i32;
        self.cursor = (self.cursor as i32 + dir).rem_euclid(n) as usize;
    }

    pub fn label(&self, item: usize) -> &'static str {
        ITEMS[item].values[self.values[item]]
    }

    pub fn screen_space(&self) -> bool {
        self.get(TRAVERSAL) == 0
    }

    pub fn filled(&self) -> bool {
        self.get(VIEW) == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{DAY_LENGTHS, WEATHER_PRESETS, WIND_PRESETS};

    #[test]
    fn preset_tables_align_with_settings_rows() {
        // Row 0 of each preset row is "auto"; the rest index the world table.
        assert_eq!(ITEMS[WEATHER].values.len(), WEATHER_PRESETS.len() + 1);
        assert_eq!(ITEMS[WIND].values.len(), WIND_PRESETS.len() + 1);
        assert_eq!(ITEMS[DAY_LENGTH].values.len(), DAY_LENGTHS.len());
        assert_eq!(ITEMS[WEATHER].values[0], "auto");
        assert_eq!(ITEMS[WIND].values[0], "auto");
    }

    #[test]
    fn keys_are_unique_and_findable() {
        for (i, item) in ITEMS.iter().enumerate() {
            assert_eq!(Settings::find(item.key), Some(i), "{}", item.key);
        }
    }

    #[test]
    fn cycle_and_cursor_wrap() {
        let mut s = Settings::new();
        s.cycle(VIEW, -1);
        assert!(s.filled());
        s.cycle(VIEW, 1);
        assert!(!s.filled());
        s.move_cursor(-1);
        assert_eq!(s.cursor, ITEMS.len() - 1);
        s.move_cursor(1);
        assert_eq!(s.cursor, 0);
    }
}
