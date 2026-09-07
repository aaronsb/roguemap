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
    /// Move the player by a screen (or map) direction, one tile per press
    /// at every zoom.
    Walk(i32, i32),
    /// Move the player eight screen cells that way: a stride at any zoom.
    Run(i32, i32),
    /// Slide the view by whole tiles.
    Pan(i32, i32),
    Centre,
    RotateQuarter(i32),
    RotateDegrees(f32),
    Zoom(i32),
    /// Step a settings row forward, by key. `settings.toml` carries the
    /// same shortcut per row; a test keeps the two in step.
    Cycle(&'static str),
    /// Step a settings row either way without wrapping: the field of view.
    Step(&'static str, i32),
    /// Turn a perspective view up or down by degrees.
    Pitch(f32),
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
/// help line, blank to keep the group off it. `shift` marks a group that
/// needs the shift key; it only tells a plain press from a shifted one
/// where the key is not a character, since shift with a letter arrives as
/// the capital.
pub struct Binding {
    pub keys: &'static [(KeyCode, Action)],
    pub shift: bool,
    pub label: &'static str,
    pub help: &'static str,
}

use Action::*;
use KeyCode::{Char, Down, Enter, Esc, Left, Right, Tab, Up};

pub const SCENE: &[Binding] = &[
    Binding { shift: false, keys: &[(Tab, Toggle("settings")), (Char('o'), Toggle("settings"))], label: "tab", help: "settings" },
    Binding { shift: false, keys: &[(Char('m'), Toggle("worldmap"))], label: "m", help: "world map" },
    Binding {
        shift: false,
        keys: &[(Char('w'), Walk(0, -1)), (Char('k'), Walk(0, -1)), (Char('s'), Walk(0, 1)), (Char('j'), Walk(0, 1)), (Char('a'), Walk(-1, 0)), (Char('h'), Walk(-1, 0)), (Char('d'), Walk(1, 0)), (Char('l'), Walk(1, 0))],
        label: "wasd/hjkl",
        help: "walk",
    },
    Binding { shift: false, keys: &[(Left, Pan(1, 0)), (Right, Pan(-1, 0)), (Up, Pan(0, 1)), (Down, Pan(0, -1))], label: "arrows", help: "pan" },
    Binding { shift: true, keys: &[(Up, Run(0, -1)), (Down, Run(0, 1)), (Left, Run(-1, 0)), (Right, Run(1, 0))], label: "shift+arrows", help: "run eight" },
    Binding { shift: false, keys: &[(Char('c'), Centre)], label: "c", help: "centre" },
    Binding { shift: false, keys: &[(Char('r'), RotateQuarter(1)), (Char('R'), RotateQuarter(-1)), (Char('('), RotateDegrees(-5.0)), (Char(')'), RotateDegrees(5.0))], label: "r/R ( )", help: "rotate" },
    Binding { shift: false, keys: &[(Char('z'), Zoom(1)), (Char('Z'), Zoom(-1))], label: "z/Z", help: "zoom" },
    Binding { shift: false, keys: &[(Char('v'), Cycle("view"))], label: "v", help: "fill" },
    Binding { shift: false, keys: &[(Char('g'), Cycle("glyphs"))], label: "g", help: "glyphs" },
    Binding { shift: false, keys: &[(Char('['), StepSeason(-0.25)), (Char(']'), StepSeason(0.25))], label: "[ ]", help: "season" },
    Binding { shift: false, keys: &[(Char(','), StepHour(-1.0)), (Char('.'), StepHour(1.0))], label: ", .", help: "time" },
    Binding { shift: false, keys: &[(Char('p'), Cycle("clock"))], label: "p", help: "pause" },
    Binding { shift: false, keys: &[(Char('W'), Cycle("weather"))], label: "W", help: "weather" },
    Binding { shift: false, keys: &[(Char('f'), Campfire)], label: "f", help: "fire" },
    Binding { shift: false, keys: &[(Char('F'), ClearFires)], label: "F", help: "clear" },
    Binding { shift: false, keys: &[(Char('H'), Cycle("hud"))], label: "H", help: "hud" },
    Binding { shift: false, keys: &[(Char('i'), Toggle("inventory"))], label: "i", help: "inventory" },
    Binding { shift: false, keys: &[(Char('I'), Toggle("stats"))], label: "I", help: "stats" },
    Binding { shift: false, keys: &[(Char('L'), Toggle("history"))], label: "L", help: "history" },
    Binding { shift: false, keys: &[(Char('C'), Toggle("conversation"))], label: "C", help: "talk" },
    Binding { shift: false, keys: &[(Char('n'), Toggle("inset"))], label: "n", help: "inset" },
    Binding { shift: false, keys: &[(Char('{'), Pitch(-5.0)), (Char('}'), Pitch(5.0))], label: "{ }", help: "pitch" },
    Binding { shift: false, keys: &[(Char('<'), Step("fov", -1)), (Char('>'), Step("fov", 1))], label: "< >", help: "fov" },
    Binding { shift: false, keys: &[(Char('q'), Quit), (Esc, Quit)], label: "q", help: "quit" },
];

pub const SETTINGS: &[Binding] = &[
    Binding { shift: false, keys: &[(Up, CursorMove(-1)), (Char('k'), CursorMove(-1)), (Down, CursorMove(1)), (Char('j'), CursorMove(1))], label: "up/down", help: "select" },
    Binding { shift: false, keys: &[(Left, Adjust(-1)), (Char('h'), Adjust(-1)), (Right, Adjust(1)), (Char('l'), Adjust(1)), (Enter, Adjust(1)), (Char(' '), Adjust(1))], label: "left/right", help: "change" },
    Binding { shift: false, keys: &[(Esc, Close), (Tab, Close), (Char('q'), Close)], label: "esc", help: "close" },
];

pub const WORLDMAP: &[Binding] = &[
    Binding {
        shift: false,
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
    Binding { shift: false, keys: &[(Char('z'), Extent(1)), (Char('Z'), Extent(-1))], label: "z", help: "extent" },
    Binding { shift: false, keys: &[(Enter, Teleport), (Char('t'), Teleport)], label: "enter", help: "teleport" },
    Binding { shift: false, keys: &[(Esc, Close), (Char('m'), Close), (Char('q'), Close)], label: "m/esc", help: "close" },
];

/// Keys every focused frame shares (ADR-005). A frame's own content sees
/// the key first, so a prompt collects characters before these apply.
pub const FRAME: &[Binding] = &[
    Binding { shift: false, keys: &[(Up, CursorMove(-1)), (Char('k'), CursorMove(-1)), (Down, CursorMove(1)), (Char('j'), CursorMove(1))], label: "up/down", help: "scroll" },
    Binding { shift: false, keys: &[(Esc, Close)], label: "esc", help: "close" },
];

/// The action a key triggers in a mode. A shifted press takes a `shift`
/// group first, then falls back to the plain groups, since a capital letter
/// carries the modifier too.
pub fn lookup(table: &[Binding], key: KeyCode, shift: bool) -> Option<Action> {
    let find = |want: bool| table.iter().filter(|b| b.shift == want).flat_map(|b| b.keys.iter()).find(|(k, _)| *k == key).map(|&(_, a)| a);
    if shift {
        find(true).or_else(|| find(false))
    } else {
        find(false)
    }
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
            let keys: Vec<(bool, KeyCode)> = table.iter().flat_map(|b| b.keys.iter().map(|(k, _)| (b.shift, *k))).collect();
            for (i, k) in keys.iter().enumerate() {
                assert!(!keys[..i].contains(k), "{name}: {k:?} bound twice");
            }
        }
    }

    #[test]
    fn lookup_finds_aliases() {
        assert_eq!(lookup(SCENE, Char('k'), false), Some(Walk(0, -1)));
        assert_eq!(lookup(SCENE, Tab, false), Some(Toggle("settings")));
        assert_eq!(lookup(SCENE, Char('x'), false), None);
        assert_eq!(lookup(WORLDMAP, Char('t'), false), Some(Teleport));
        // Arrows pan; with shift they are a stride of eight tiles, and a
        // shifted key with no group of its own still finds its plain one.
        assert_eq!(lookup(SCENE, Up, false), Some(Pan(0, 1)));
        assert_eq!(lookup(SCENE, Up, true), Some(Run(0, -1)));
        assert_eq!(lookup(SCENE, Char('Z'), true), Some(Zoom(-1)));
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
        assert_eq!(&scene[..120], " tab settings  m world map  wasd/hjkl walk  arrows pan  shift+arrows run eight  c centre  r/R ( ) rotate  z/Z zoom  v fi");
        for frame in ["inventory", "stats", "history", "conversation"] {
            assert!(SCENE.iter().flat_map(|b| b.keys).any(|&(_, a)| a == Toggle(frame)), "{frame} has no toggle key");
        }
    }
}
