//! Key bindings as tables, one per input mode. The help lines are generated
//! from them, and the README key table is derived from them by hand.

use crossterm::event::KeyCode;

/// What a key does. Scene actions come first, then the settings popover's,
/// then the world map's, then the ones every focused frame shares.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Open or close the frame of this name (ADR-005); the row in
    /// `assets/ui.toml` carries the same key, which a test checks.
    Toggle(&'static str),
    Quit,
    /// Move the player by a screen (or map) direction.
    Walk(i32, i32),
    /// Slide the view by whole tiles.
    Pan(i32, i32),
    Centre,
    RotateQuarter(i32),
    RotateDegrees(f32),
    Zoom(i32),
    /// Step a settings row forward, by key. `settings.toml` carries the
    /// same shortcut per row; a test keeps the two in step.
    Cycle(&'static str),
    StepSeason(f32),
    StepHour(f32),
    Campfire,
    ClearFires,
    Close,
    CursorMove(i32),
    /// Change the selected settings row.
    Adjust(i32),
    /// Move the world map cursor by one cell.
    CursorStep(i32, i32),
    Extent(i32),
    Teleport,
}

/// A group of keys that share a help entry. `label` names the keys on the
/// help line, blank to keep the group off it.
pub struct Binding {
    pub keys: &'static [(KeyCode, Action)],
    pub label: &'static str,
    pub help: &'static str,
}

use Action::*;
use KeyCode::{Char, Down, Enter, Esc, Left, Right, Tab, Up};

pub const SCENE: &[Binding] = &[
    Binding { keys: &[(Tab, Toggle("settings")), (Char('o'), Toggle("settings"))], label: "tab", help: "settings" },
    Binding { keys: &[(Char('m'), Toggle("worldmap"))], label: "m", help: "world map" },
    Binding {
        keys: &[
            (Char('w'), Walk(0, -1)),
            (Char('k'), Walk(0, -1)),
            (Char('s'), Walk(0, 1)),
            (Char('j'), Walk(0, 1)),
            (Char('a'), Walk(-1, 0)),
            (Char('h'), Walk(-1, 0)),
            (Char('d'), Walk(1, 0)),
            (Char('l'), Walk(1, 0)),
        ],
        label: "wasd/hjkl",
        help: "walk",
    },
    Binding { keys: &[(Left, Pan(1, 0)), (Right, Pan(-1, 0)), (Up, Pan(0, 1)), (Down, Pan(0, -1))], label: "arrows", help: "pan" },
    Binding { keys: &[(Char('c'), Centre)], label: "c", help: "centre" },
    Binding {
        keys: &[(Char('r'), RotateQuarter(1)), (Char('R'), RotateQuarter(-1)), (Char('('), RotateDegrees(-5.0)), (Char(')'), RotateDegrees(5.0))],
        label: "r/R ( )",
        help: "rotate",
    },
    Binding { keys: &[(Char('z'), Zoom(1)), (Char('Z'), Zoom(-1))], label: "z/Z", help: "zoom" },
    Binding { keys: &[(Char('v'), Cycle("view"))], label: "v", help: "fill" },
    Binding { keys: &[(Char('g'), Cycle("glyphs"))], label: "g", help: "glyphs" },
    Binding { keys: &[(Char('['), StepSeason(-0.25)), (Char(']'), StepSeason(0.25))], label: "[ ]", help: "season" },
    Binding { keys: &[(Char(','), StepHour(-1.0)), (Char('.'), StepHour(1.0))], label: ", .", help: "time" },
    Binding { keys: &[(Char('p'), Cycle("clock"))], label: "p", help: "pause" },
    Binding { keys: &[(Char('W'), Cycle("weather"))], label: "W", help: "weather" },
    Binding { keys: &[(Char('f'), Campfire)], label: "f", help: "fire" },
    Binding { keys: &[(Char('F'), ClearFires)], label: "F", help: "clear" },
    Binding { keys: &[(Char('H'), Cycle("hud"))], label: "H", help: "hud" },
    Binding { keys: &[(Char('i'), Toggle("inventory"))], label: "i", help: "inventory" },
    Binding { keys: &[(Char('I'), Toggle("stats"))], label: "I", help: "stats" },
    Binding { keys: &[(Char('L'), Toggle("history"))], label: "L", help: "history" },
    Binding { keys: &[(Char('C'), Toggle("conversation"))], label: "C", help: "talk" },
    Binding { keys: &[(Char('q'), Quit), (Esc, Quit)], label: "q", help: "quit" },
];

pub const SETTINGS: &[Binding] = &[
    Binding { keys: &[(Up, CursorMove(-1)), (Char('k'), CursorMove(-1)), (Down, CursorMove(1)), (Char('j'), CursorMove(1))], label: "up/down", help: "select" },
    Binding {
        keys: &[(Left, Adjust(-1)), (Char('h'), Adjust(-1)), (Right, Adjust(1)), (Char('l'), Adjust(1)), (Enter, Adjust(1)), (Char(' '), Adjust(1))],
        label: "left/right",
        help: "change",
    },
    Binding { keys: &[(Esc, Close), (Tab, Close), (Char('q'), Close)], label: "esc", help: "close" },
];

pub const WORLDMAP: &[Binding] = &[
    Binding {
        keys: &[
            (Up, CursorStep(0, -1)),
            (Char('k'), CursorStep(0, -1)),
            (Char('w'), CursorStep(0, -1)),
            (Down, CursorStep(0, 1)),
            (Char('j'), CursorStep(0, 1)),
            (Char('s'), CursorStep(0, 1)),
            (Left, CursorStep(-1, 0)),
            (Char('h'), CursorStep(-1, 0)),
            (Char('a'), CursorStep(-1, 0)),
            (Right, CursorStep(1, 0)),
            (Char('l'), CursorStep(1, 0)),
            (Char('d'), CursorStep(1, 0)),
        ],
        label: "arrows",
        help: "move",
    },
    Binding { keys: &[(Char('z'), Extent(1)), (Char('Z'), Extent(-1))], label: "z", help: "extent" },
    Binding { keys: &[(Enter, Teleport), (Char('t'), Teleport)], label: "enter", help: "teleport" },
    Binding { keys: &[(Esc, Close), (Char('m'), Close), (Char('q'), Close)], label: "m/esc", help: "close" },
];

/// Keys every focused frame shares (ADR-005). A frame's own content sees
/// the key first, so a prompt collects characters before these apply.
pub const FRAME: &[Binding] = &[
    Binding { keys: &[(Up, CursorMove(-1)), (Char('k'), CursorMove(-1)), (Down, CursorMove(1)), (Char('j'), CursorMove(1))], label: "up/down", help: "scroll" },
    Binding { keys: &[(Esc, Close)], label: "esc", help: "close" },
];

/// The action a key triggers in a mode.
pub fn lookup(table: &[Binding], key: KeyCode) -> Option<Action> {
    table.iter().flat_map(|b| b.keys.iter()).find(|(k, _)| *k == key).map(|&(_, a)| a)
}

/// The help line for a mode: every labelled group as "keys action", joined
/// by `sep`, with a space either end.
pub fn help_line(table: &[Binding], sep: &str) -> String {
    let entries: Vec<String> = table.iter().filter(|b| !b.label.is_empty()).map(|b| format!("{} {}", b.label, b.help)).collect();
    format!(" {} ", entries.join(sep))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_within_a_mode() {
        for (name, table) in [("scene", SCENE), ("settings", SETTINGS), ("worldmap", WORLDMAP), ("frame", FRAME)] {
            let keys: Vec<KeyCode> = table.iter().flat_map(|b| b.keys.iter().map(|(k, _)| *k)).collect();
            for (i, k) in keys.iter().enumerate() {
                assert!(!keys[..i].contains(k), "{name}: {k:?} bound twice");
            }
        }
    }

    #[test]
    fn lookup_finds_aliases() {
        assert_eq!(lookup(SCENE, Char('k')), Some(Walk(0, -1)));
        assert_eq!(lookup(SCENE, Tab), Some(Toggle("settings")));
        assert_eq!(lookup(SCENE, Char('x')), None);
        assert_eq!(lookup(WORLDMAP, Char('t')), Some(Teleport));
    }

    #[test]
    fn help_lines_are_generated() {
        assert!(help_line(SCENE, "  ").starts_with(" tab settings  m world map  "));
        assert!(help_line(SCENE, "  ").ends_with("  q quit "));
        assert_eq!(help_line(SETTINGS, "   "), " up/down select   left/right change   esc close ");
        assert_eq!(help_line(FRAME, "  "), " up/down scroll  esc close ");
        // The bottom row of a 120-column frame shows only this much, so
        // entries added after it do not change the golden frames.
        let scene = help_line(SCENE, "  ");
        assert!(scene.len() > 120, "the scene help line already runs past 120 columns");
        assert_eq!(
            &scene[..120],
            " tab settings  m world map  wasd/hjkl walk  arrows pan  c centre  r/R ( ) rotate  z/Z zoom  v fill  g glyphs  [ ] season"
        );
        for frame in ["inventory", "stats", "history", "conversation"] {
            assert!(SCENE.iter().flat_map(|b| b.keys).any(|&(_, a)| a == Toggle(frame)), "{frame} has no toggle key");
        }
    }
}
