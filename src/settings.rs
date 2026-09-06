//! User preferences as one table. The popover renders the table generically
//! and every shortcut key edits the same fields.

/// One row of the settings table: a name and its cyclable values.
pub struct Item {
    pub name: &'static str,
    pub values: &'static [&'static str],
}

pub const TRAVERSAL: usize = 0;
pub const WORLD: usize = 1;
pub const GLYPHS: usize = 2;
pub const HUD: usize = 3;
pub const CLOCK: usize = 4;
pub const WEATHER: usize = 5;
pub const WIND: usize = 6;
pub const DAY_LENGTH: usize = 7;

pub const ITEMS: [Item; 8] = [
    Item { name: "Traversal", values: &["screen space", "map axes"] },
    Item { name: "World view", values: &["island", "filled"] },
    Item { name: "Glyphs", values: &["petscii", "ascii"] },
    Item { name: "HUD", values: &["shown", "hidden"] },
    Item { name: "Clock", values: &["running", "paused"] },
    Item { name: "Weather", values: &["auto", "clear", "cloudy", "rain", "storm"] },
    Item { name: "Wind", values: &["auto", "calm", "breeze", "windy", "gale"] },
    Item { name: "Day length", values: &["2 min", "10 min", "1 hour", "24 hours"] },
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

    pub fn label(&self, item: usize) -> &'static str {
        ITEMS[item].values[self.values[item]]
    }

    pub fn screen_space(&self) -> bool {
        self.get(TRAVERSAL) == 0
    }

    pub fn filled(&self) -> bool {
        self.get(WORLD) == 1
    }
}
