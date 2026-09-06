//! The editor's key model (ADR-003): three modes, one `Action` enum, and a
//! binding table per mode in the same shape as `input::Binding` for the
//! game. Keys shared with the game keep their meaning: `[ ]` season, `, .`
//! hour, `g` glyph set, `r` rotate, `W` weather, arrows move, `Tab` cycles,
//! `Esc` backs out, `q` quits.

use crossterm::event::KeyCode;

/// Which key table is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Navigating tables, rows and fields.
    Normal,
    /// Editing one field's value.
    Field,
    /// Editing an art file cell by cell.
    Grid,
    /// Choosing a glyph for the grid.
    Picker,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Field => "field",
            Mode::Grid => "grid",
            Mode::Picker => "picker",
        }
    }
}

/// What a key does. `Document::apply` and the editor state consume these;
/// the UI never mutates state directly.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Quit,
    /// Back out: cancel a pending confirmation, a field edit, or the grid.
    Cancel,
    NextPane,
    PrevPane,
    /// Move the cursor: `(dx, dy)` in the current pane or grid.
    Move(i32, i32),
    /// Coarser movement: page or channel step by 16.
    Page(i32),
    Home,
    End,
    /// Edit the field under the cursor, open an art grid, or commit.
    Enter,
    AddRow,
    DeleteRow,
    /// Answer a pending confirmation.
    Yes,
    /// Remove an optional field so its default applies.
    ClearField,
    Undo,
    Redo,
    /// Save the current table's file.
    Save,
    /// Save every changed file.
    SaveAll,
    Biome(i32),
    Season(f32),
    Hour(f32),
    Glyphs,
    RotateQuarter(i32),
    Weather,
    /// Cycle the previewed tier on small screens.
    Tier,
    /// Cycle the block pattern or tree variant.
    Pattern,
    Levels(i32),
    Animate,
    // Field mode
    Backspace,
    Delete,
    /// Toggle a checklist entry.
    Toggle,
    /// Type a character into the line editor.
    Insert(char),
    // Grid mode
    /// Place a glyph at the cursor.
    Put(char),
    ClearCell,
    InsertGridRow,
    DeleteGridRow,
    Widen,
    Narrow,
    SetCenter,
    SetBaseRows,
    OpenPicker,
}

/// A group of keys that share a help entry, as in `input::Binding`.
pub struct Binding {
    pub keys: &'static [(KeyCode, Action)],
    pub label: &'static str,
    pub help: &'static str,
}

use Action::*;
use KeyCode::{BackTab, Backspace as KBackspace, Char, Delete as KDelete, Down, End as KEnd, Enter as KEnter, Esc, Home as KHome, Left, PageDown, PageUp, Right, Tab, Up};

pub const NORMAL: &[Binding] = &[
    Binding { keys: &[(Tab, NextPane), (BackTab, PrevPane)], label: "tab", help: "pane" },
    Binding {
        keys: &[(Up, Move(0, -1)), (Char('k'), Move(0, -1)), (Down, Move(0, 1)), (Char('j'), Move(0, 1)), (Left, Move(-1, 0)), (Char('h'), Move(-1, 0)), (Right, Move(1, 0)), (Char('l'), Move(1, 0))],
        label: "arrows",
        help: "move",
    },
    Binding { keys: &[(PageUp, Page(-1)), (PageDown, Page(1)), (KHome, Home), (KEnd, End)], label: "", help: "" },
    Binding { keys: &[(KEnter, Enter)], label: "enter", help: "edit" },
    Binding { keys: &[(Char('n'), AddRow)], label: "n", help: "new row" },
    Binding { keys: &[(Char('d'), DeleteRow)], label: "d", help: "delete" },
    Binding { keys: &[(Char('y'), Yes)], label: "", help: "" },
    Binding { keys: &[(Char('x'), ClearField)], label: "x", help: "clear" },
    Binding { keys: &[(Char('u'), Undo), (Char('U'), Redo)], label: "u/U", help: "undo/redo" },
    Binding { keys: &[(Char('s'), Save), (Char('S'), SaveAll)], label: "s/S", help: "save" },
    Binding { keys: &[(Char('b'), Biome(1)), (Char('B'), Biome(-1))], label: "b", help: "biome" },
    Binding { keys: &[(Char('['), Season(-0.25)), (Char(']'), Season(0.25))], label: "[ ]", help: "season" },
    Binding { keys: &[(Char(','), Hour(-1.0)), (Char('.'), Hour(1.0))], label: ", .", help: "time" },
    Binding { keys: &[(Char('g'), Glyphs)], label: "g", help: "glyphs" },
    Binding { keys: &[(Char('r'), RotateQuarter(1)), (Char('R'), RotateQuarter(-1))], label: "r", help: "rotate" },
    Binding { keys: &[(Char('W'), Weather)], label: "W", help: "weather" },
    Binding { keys: &[(Char('t'), Tier)], label: "t", help: "tier" },
    Binding { keys: &[(Char('P'), Pattern)], label: "P", help: "pattern" },
    Binding { keys: &[(Char('+'), Levels(1)), (Char('-'), Levels(-1))], label: "", help: "" },
    Binding { keys: &[(Char('a'), Animate)], label: "a", help: "animate" },
    Binding { keys: &[(Esc, Cancel)], label: "", help: "" },
    Binding { keys: &[(Char('q'), Quit)], label: "q", help: "quit" },
];

pub const FIELD: &[Binding] = &[
    Binding { keys: &[(Left, Move(-1, 0)), (Right, Move(1, 0)), (Up, Move(0, -1)), (Down, Move(0, 1))], label: "arrows", help: "move/step" },
    Binding { keys: &[(PageUp, Page(-1)), (PageDown, Page(1))], label: "pgup/pgdn", help: "step 16" },
    Binding { keys: &[(KHome, Home), (KEnd, End)], label: "", help: "" },
    Binding { keys: &[(KBackspace, Backspace), (KDelete, Delete)], label: "", help: "" },
    Binding { keys: &[(Tab, Toggle)], label: "space", help: "toggle" },
    Binding { keys: &[(KEnter, Enter)], label: "enter", help: "commit" },
    Binding { keys: &[(Esc, Cancel)], label: "esc", help: "cancel" },
];

pub const GRID: &[Binding] = &[
    Binding { keys: &[(Up, Move(0, -1)), (Down, Move(0, 1)), (Left, Move(-1, 0)), (Right, Move(1, 0))], label: "arrows", help: "move" },
    Binding { keys: &[(KHome, Home), (KEnd, End)], label: "", help: "" },
    Binding { keys: &[(Char(' '), ClearCell), (KBackspace, ClearCell), (KDelete, ClearCell)], label: "space", help: "clear" },
    Binding { keys: &[(Char('i'), InsertGridRow), (Char('X'), DeleteGridRow)], label: "i/X", help: "row" },
    Binding { keys: &[(Char('>'), Widen), (Char('<'), Narrow)], label: "> <", help: "width" },
    Binding { keys: &[(Char('c'), SetCenter)], label: "c", help: "center" },
    Binding { keys: &[(Char('B'), SetBaseRows)], label: "B", help: "base" },
    Binding { keys: &[(Char('p'), OpenPicker)], label: "p", help: "picker" },
    Binding { keys: &[(Tab, Undo)], label: "tab", help: "undo" },
    Binding { keys: &[(Esc, Cancel), (KEnter, Cancel)], label: "esc", help: "done" },
];

pub const PICKER: &[Binding] = &[
    Binding { keys: &[(Up, Move(0, -1)), (Down, Move(0, 1)), (Left, Move(-1, 0)), (Right, Move(1, 0))], label: "arrows", help: "move" },
    Binding { keys: &[(PageUp, Page(-1)), (PageDown, Page(1))], label: "pgup/pgdn", help: "page" },
    Binding { keys: &[(KEnter, Enter)], label: "enter", help: "place" },
    Binding { keys: &[(Esc, Cancel)], label: "esc", help: "back" },
];

pub fn table(mode: Mode) -> &'static [Binding] {
    match mode {
        Mode::Normal => NORMAL,
        Mode::Field => FIELD,
        Mode::Grid => GRID,
        Mode::Picker => PICKER,
    }
}

/// The action a key triggers in a mode. In Field mode an unbound printable
/// key types itself; in Grid mode it places itself; space in Field mode
/// toggles a checklist entry, which the line editor treats as typing.
pub fn lookup(mode: Mode, key: KeyCode) -> Option<Action> {
    let bound = table(mode).iter().flat_map(|b| b.keys.iter()).find(|(k, _)| *k == key).map(|&(_, a)| a);
    match (bound, mode, key) {
        (Some(a), ..) => Some(a),
        (None, Mode::Field, Char(c)) => Some(Insert(c)),
        (None, Mode::Grid, Char(c)) => Some(Put(c)),
        _ => None,
    }
}

/// The help line for a mode, in the game's format.
pub fn help_line(mode: Mode, sep: &str) -> String {
    let entries: Vec<String> = table(mode).iter().filter(|b| !b.label.is_empty()).map(|b| format!("{} {}", b.label, b.help)).collect();
    format!(" {} ", entries.join(sep))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_within_a_mode() {
        for mode in [Mode::Normal, Mode::Field, Mode::Grid, Mode::Picker] {
            let keys: Vec<KeyCode> = table(mode).iter().flat_map(|b| b.keys.iter().map(|(k, _)| *k)).collect();
            for (i, k) in keys.iter().enumerate() {
                assert!(!keys[..i].contains(k), "{}: {k:?} bound twice", mode.name());
            }
        }
    }

    #[test]
    fn shared_keys_agree_with_the_game() {
        use crate::input::{self, SCENE};
        assert_eq!(lookup(Mode::Normal, Char('[')), Some(Season(-0.25)));
        assert_eq!(input::lookup(SCENE, Char(']')), Some(input::Action::StepSeason(0.25)));
        assert_eq!(lookup(Mode::Normal, Char('.')), Some(Hour(1.0)));
        assert_eq!(lookup(Mode::Normal, Char('g')), Some(Glyphs));
        assert_eq!(lookup(Mode::Normal, Char('q')), Some(Quit));
        assert_eq!(lookup(Mode::Normal, Char('r')), Some(RotateQuarter(1)));
        assert_eq!(lookup(Mode::Normal, Char('W')), Some(Weather));
        assert_eq!(lookup(Mode::Normal, Tab), Some(NextPane));
    }

    #[test]
    fn unbound_printables_type_or_place() {
        assert_eq!(lookup(Mode::Field, Char('z')), Some(Insert('z')));
        assert_eq!(lookup(Mode::Field, Char(' ')), Some(Insert(' ')));
        assert_eq!(lookup(Mode::Grid, Char('z')), Some(Put('z')));
        assert_eq!(lookup(Mode::Grid, Char('▓')), Some(Put('▓')));
        assert_eq!(lookup(Mode::Grid, Char(' ')), Some(ClearCell));
        assert_eq!(lookup(Mode::Normal, Char('z')), None);
        assert_eq!(lookup(Mode::Picker, Char('z')), None);
        assert!(help_line(Mode::Grid, "  ").contains("p picker"));
    }
}
