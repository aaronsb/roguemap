//! Key bindings as tables, one per input mode. The help lines are generated
//! from them, and the README key table is derived from them by hand.

use crossterm::event::{KeyCode, MouseEvent, MouseEventKind};

use crate::world::World;

/// What a key does. Scene actions come first, then the settings popover's,
/// then the world map's, then the ones every focused frame shares.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Open or close the frame of this name (ADR-005); the row in
    /// `assets/ui.toml` carries the same key, which a test checks.
    Toggle(&'static str),
    Quit,
    /// Set the player walking in a screen (or map) direction at the
    /// creature's speed for a short grace, renewed by each press (ADR-008).
    Walk(i32, i32),
    /// The same at the run speed.
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
    Binding { shift: true, keys: &[(Up, Run(0, -1)), (Down, Run(0, 1)), (Left, Run(-1, 0)), (Right, Run(1, 0))], label: "shift+arrows", help: "run" },
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
    Binding { shift: false, keys: &[(Char('x'), Toggle("inset"))], label: "x", help: "inset" },
    Binding { shift: false, keys: &[(Char('{'), Pitch(-5.0)), (Char('}'), Pitch(5.0))], label: "{ }", help: "pitch" },
    Binding { shift: false, keys: &[(Char('<'), Step("fov", -1)), (Char('>'), Step("fov", 1))], label: "< >", help: "fov" },
    // The roguelike diagonals, so one key is a diagonal in a terminal that
    // cannot report two keys held at once (ADR-008); the capitals run.
    // They sit at the end of the table because the bottom bar of a
    // 120-column frame shows the first 120 columns of the generated help
    // line, and the golden frames pin those.
    Binding {
        shift: false,
        keys: &[(Char('y'), Walk(-1, -1)), (Char('u'), Walk(1, -1)), (Char('b'), Walk(-1, 1)), (Char('n'), Walk(1, 1)), (Char('Y'), Run(-1, -1)), (Char('U'), Run(1, -1)), (Char('B'), Run(-1, 1)), (Char('N'), Run(1, 1))],
        label: "yubn",
        help: "diagonals",
    },
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
/// group first, then the capital of a letter, since that is how a shifted
/// letter usually arrives and how the tables spell it, and last the plain
/// groups. The capital comes before the plain key because a terminal
/// reporting every key as an escape code (ADR-008) sends the base key and
/// a shift modifier, and `R` must not fall through to `r`.
pub fn lookup(table: &[Binding], key: KeyCode, shift: bool) -> Option<Action> {
    let find = |want: bool, key: KeyCode| table.iter().filter(|b| b.shift == want).flat_map(|b| b.keys.iter()).find(|(k, _)| *k == key).map(|&(_, a)| a);
    if !shift {
        return find(false, key);
    }
    let capital = match key {
        Char(c) if c.is_ascii_lowercase() => Some(Char(c.to_ascii_uppercase())),
        _ => None,
    };
    find(true, key).or_else(|| capital.and_then(|k| find(true, k))).or_else(|| capital.and_then(|k| find(false, k))).or_else(|| find(false, key))
}

/// What the mouse does with the view (ADR-008): the `mouse` settings row.
/// There is no pointer lock in a terminal, so `free` turns while the
/// pointer moves and stops when it reaches a screen edge, the way a mouse
/// stops at the edge of its mousepad — lift it and carry on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseMode {
    /// Turn while a button is held.
    Drag,
    /// Turn on any pointer motion.
    Free,
    /// Ignore the mouse.
    Off,
}

impl MouseMode {
    /// The values of the settings row, in order; a test keeps the two in
    /// step.
    pub const NAMES: [&'static str; 3] = ["drag", "free", "off"];

    pub fn from_index(i: usize) -> MouseMode {
        match i {
            1 => MouseMode::Free,
            2 => MouseMode::Off,
            _ => MouseMode::Drag,
        }
    }
}

/// What a mouse event asks of the view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Look {
    /// Turn by degrees: yaw positive to the right, pitch positive down.
    Turn(f32, f32),
    /// One notch of the wheel: 1 in, -1 out.
    Wheel(i32),
}

/// The pointer, turning the view (ADR-008). Terminals report the cell the
/// pointer is in, so a turn is a number of columns and rows moved times
/// the degrees each is worth.
#[derive(Default)]
pub struct Mouse {
    /// The cell the pointer was last seen in.
    last: Option<(u16, u16)>,
    /// Whether a button is down, so a drag turns the view.
    dragging: bool,
}

impl Mouse {
    /// Degrees of yaw a column of pointer movement is worth.
    pub const YAW_PER_COLUMN: f32 = 2.0;
    /// Degrees of pitch a row is worth. A cell is twice as tall as it is
    /// wide, so a row is worth more than a column.
    pub const PITCH_PER_ROW: f32 = 3.0;

    pub fn new() -> Mouse {
        Mouse::default()
    }

    /// Forget where the pointer was, so the next motion is a fresh start
    /// rather than a jump: after a frame took the screen, or a resize.
    pub fn forget(&mut self) {
        self.last = None;
        self.dragging = false;
    }

    /// What one mouse event does. Motion turns the view by the cells moved
    /// since the last one — while a button is down in `drag`, on any
    /// motion in `free` — and the wheel is passed on for the camera mode
    /// to read. `off` does nothing at all.
    pub fn event(&mut self, ev: MouseEvent, mode: MouseMode) -> Option<Look> {
        if mode == MouseMode::Off {
            self.forget();
            return None;
        }
        let at = (ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Down(_) => {
                self.dragging = true;
                self.last = Some(at);
                None
            }
            MouseEventKind::Up(_) => {
                self.dragging = false;
                self.last = Some(at);
                None
            }
            MouseEventKind::ScrollUp => Some(Look::Wheel(1)),
            MouseEventKind::ScrollDown => Some(Look::Wheel(-1)),
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                let from = self.last.replace(at);
                if ev.kind == MouseEventKind::Moved {
                    self.dragging = false;
                }
                let turning = mode == MouseMode::Free || self.dragging;
                let (fx, fy) = from.filter(|_| turning)?;
                let (dx, dy) = (at.0 as f32 - fx as f32, at.1 as f32 - fy as f32);
                // Moving right turns right, which is the yaw counting down;
                // moving down looks down, which is the pitch counting up.
                (dx != 0.0 || dy != 0.0).then_some(Look::Turn(-dx * Mouse::YAW_PER_COLUMN, dy * Mouse::PITCH_PER_ROW))
            }
            _ => None,
        }
    }
}

/// One direction key that is down (ADR-008).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pressed {
    code: KeyCode,
    /// The screen (or map) direction the key names.
    dir: (i32, i32),
    /// Whether it was pressed with shift, so the walk is a run.
    run: bool,
    /// Seconds left on the key's lease; not spent when releases arrive.
    lease: f32,
}

/// The direction keys down at once (ADR-008), which is what makes a
/// diagonal out of two of them.
///
/// A terminal that speaks the keyboard enhancement protocol reports
/// releases, and a key then stays down from its press until its release.
/// One that does not delivers a press and then repeats, and while two keys
/// are held it repeats only the last pressed, so a key instead holds a
/// lease of `World::GRACE` seconds that every press or repeat renews and
/// every tick spends: alternating two keys walks a diagonal, and a single
/// held key behaves as it always has.
pub struct Held {
    /// Whether the terminal reports releases.
    release_events: bool,
    keys: Vec<Pressed>,
}

impl Held {
    pub fn new(release_events: bool) -> Held {
        Held { release_events, keys: Vec::new() }
    }

    /// Whether releases are reported, which the HUD names.
    pub fn release_events(&self) -> bool {
        self.release_events
    }

    /// The direction a movement action carries, and whether it runs.
    pub fn movement(a: Action) -> Option<((i32, i32), bool)> {
        match a {
            Action::Walk(dx, dy) => Some(((dx, dy), false)),
            Action::Run(dx, dy) => Some(((dx, dy), true)),
            _ => None,
        }
    }

    /// A press or a repeat of a direction key: the key is down, with a
    /// fresh lease.
    pub fn press(&mut self, code: KeyCode, dir: (i32, i32), run: bool) {
        match self.keys.iter_mut().find(|d| d.code == code) {
            Some(d) => *d = Pressed { code, dir, run, lease: World::GRACE },
            None => self.keys.push(Pressed { code, dir, run, lease: World::GRACE }),
        }
    }

    /// A release: the key is up, whatever modifiers it came back with.
    pub fn release(&mut self, code: KeyCode) {
        self.keys.retain(|d| d.code != code);
    }

    /// Every key is up: a focused frame owns the keyboard, so the figure
    /// stands still under one however the keys were left.
    pub fn clear(&mut self) {
        self.keys.clear();
    }

    /// Spend `dt` seconds of every lease and drop the keys whose lease has
    /// run out. Where releases are reported the leases are not spent — the
    /// release is what lifts a key.
    pub fn tick(&mut self, dt: f32) {
        if self.release_events {
            return;
        }
        for d in &mut self.keys {
            d.lease -= dt;
        }
        self.keys.retain(|d| d.lease > 1e-6);
    }

    /// Every key still down, so the heading is their sum.
    pub fn dirs(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.keys.iter().map(|d| d.dir)
    }

    /// Whether any key down was pressed with shift.
    pub fn running(&self) -> bool {
        self.keys.iter().any(|d| d.run)
    }

    /// The lease the walk gets: the longest any key still has, which is
    /// the whole grace while releases are reported.
    pub fn grace(&self) -> f32 {
        self.keys.iter().map(|d| d.lease).fold(0.0, f32::max)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
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
        assert_eq!(lookup(SCENE, Char('e'), false), None);
        assert_eq!(lookup(WORLDMAP, Char('t'), false), Some(Teleport));
        // Arrows pan; with shift they are a stride of eight tiles, and a
        // shifted key with no group of its own still finds its plain one.
        assert_eq!(lookup(SCENE, Up, false), Some(Pan(0, 1)));
        assert_eq!(lookup(SCENE, Up, true), Some(Run(0, -1)));
        assert_eq!(lookup(SCENE, Char('Z'), true), Some(Zoom(-1)));
    }

    #[test]
    fn a_shifted_letter_finds_the_capital_before_the_plain_key() {
        // A terminal reporting every key as an escape code sends the base
        // key with a shift modifier, so `shift+r` must be `R`'s action and
        // not `r`'s, and `shift+u` a diagonal run.
        assert_eq!(lookup(SCENE, Char('r'), true), Some(RotateQuarter(-1)));
        assert_eq!(lookup(SCENE, Char('R'), true), Some(RotateQuarter(-1)));
        assert_eq!(lookup(SCENE, Char('u'), true), Some(Run(1, -1)));
        assert_eq!(lookup(SCENE, Char('u'), false), Some(Walk(1, -1)));
        // A letter with no capital bound falls back to its plain action.
        assert_eq!(lookup(SCENE, Char('d'), true), Some(Walk(1, 0)));
    }

    #[test]
    fn the_roguelike_diagonals_are_one_key_each() {
        for (key, dir) in [('y', (-1, -1)), ('u', (1, -1)), ('b', (-1, 1)), ('n', (1, 1))] {
            assert_eq!(lookup(SCENE, Char(key), false), Some(Walk(dir.0, dir.1)), "{key} walks {dir:?}");
            let capital = Char(key.to_ascii_uppercase());
            assert_eq!(lookup(SCENE, capital, false), Some(Run(dir.0, dir.1)), "{key} with shift runs {dir:?}");
        }
    }

    /// The keys down make one heading, so two of them are a diagonal.
    #[test]
    fn held_keys_are_a_set_the_release_lifts_and_the_lease_runs_out_of() {
        let mut h = Held::new(true);
        assert!(h.is_empty(), "nothing is held to start with");
        h.press(Char('w'), (0, -1), false);
        h.press(Char('d'), (1, 0), false);
        assert_eq!(h.dirs().collect::<Vec<_>>(), vec![(0, -1), (1, 0)], "two keys are both down");
        assert!(!h.running(), "neither was pressed with shift");
        // With releases reported a lease is never spent, however long the
        // tick, and the release is what lifts the key.
        h.tick(10.0);
        assert_eq!(h.dirs().count(), 2, "a held key stays down until its release");
        h.release(Char('w'));
        assert_eq!(h.dirs().collect::<Vec<_>>(), vec![(1, 0)], "releasing one leaves the other");
        h.release(Char('d'));
        assert!(h.is_empty());
        h.press(Char('w'), (0, -1), false);
        h.clear();
        assert!(h.is_empty(), "a frame taking focus lifts them all");
    }

    #[test]
    fn a_lease_is_spent_by_the_tick_and_renewed_by_a_repeat() {
        let mut h = Held::new(false);
        h.press(Up, (0, -1), true);
        assert!(h.running(), "the key that named a run runs");
        assert_eq!(h.grace(), World::GRACE, "a press is a whole grace");
        h.press(Left, (-1, 0), false);
        assert_eq!(h.dirs().count(), 2, "two keys pressed within the grace are both live");
        // Three ticks of 40 ms spend the grace of the older key, while a
        // repeat of the other renews it.
        h.tick(0.04);
        h.press(Left, (-1, 0), false);
        h.tick(0.04);
        h.press(Left, (-1, 0), false);
        h.tick(0.04);
        assert_eq!(h.dirs().collect::<Vec<_>>(), vec![(-1, 0)], "the key that stopped repeating fell out");
        assert!(!h.running(), "and its run went with it");
        h.tick(World::GRACE);
        assert!(h.is_empty(), "a lease nothing renews runs out");
    }

    fn at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers: crossterm::event::KeyModifiers::NONE }
    }

    /// A turn is the cells the pointer moved times the degrees each is
    /// worth, and it starts from where the pointer was, not from the press.
    #[test]
    fn the_pointer_turns_the_view_by_the_cells_it_moves() {
        use crossterm::event::MouseButton::Left;
        let mut m = Mouse::new();
        assert_eq!(m.event(at(MouseEventKind::Down(Left), 10, 10), MouseMode::Drag), None, "the press only marks where the drag began");
        assert_eq!(m.event(at(MouseEventKind::Drag(Left), 14, 10), MouseMode::Drag), Some(Look::Turn(-4.0 * Mouse::YAW_PER_COLUMN, 0.0)));
        assert_eq!(m.event(at(MouseEventKind::Drag(Left), 14, 13), MouseMode::Drag), Some(Look::Turn(0.0, 3.0 * Mouse::PITCH_PER_ROW)), "and on from there");
        assert_eq!(m.event(at(MouseEventKind::Drag(Left), 14, 13), MouseMode::Drag), None, "a report from the same cell turns nothing");
        // In drag, letting the button up ends the turn; motion after it is
        // only the pointer moving over the scene.
        m.event(at(MouseEventKind::Up(Left), 14, 13), MouseMode::Drag);
        assert_eq!(m.event(at(MouseEventKind::Moved, 30, 20), MouseMode::Drag), None);
        // In free, any motion turns, and the wheel is the wheel in both.
        let mut f = Mouse::new();
        assert_eq!(f.event(at(MouseEventKind::Moved, 30, 20), MouseMode::Free), None, "the first sighting is only a place to start from");
        assert_eq!(f.event(at(MouseEventKind::Moved, 31, 19), MouseMode::Free), Some(Look::Turn(-Mouse::YAW_PER_COLUMN, -Mouse::PITCH_PER_ROW)));
        assert_eq!(f.event(at(MouseEventKind::ScrollUp, 31, 19), MouseMode::Free), Some(Look::Wheel(1)));
        assert_eq!(f.event(at(MouseEventKind::ScrollDown, 31, 19), MouseMode::Drag), Some(Look::Wheel(-1)));
        // Off turns nothing and remembers nothing, whatever arrives.
        let mut o = Mouse::new();
        for kind in [MouseEventKind::Down(Left), MouseEventKind::Drag(Left), MouseEventKind::Moved, MouseEventKind::ScrollUp] {
            assert_eq!(o.event(at(kind, 40, 20), MouseMode::Off), None, "{kind:?} with the mouse off");
        }
        assert_eq!(o.event(at(MouseEventKind::Moved, 41, 20), MouseMode::Free), None, "and it starts afresh when it is turned back on");
    }

    /// Moving the pointer right turns the view right, and a perspective
    /// pitch stops at the end of its range rather than turning over.
    #[test]
    fn a_turn_right_is_a_turn_right_and_the_pitch_clamps() {
        use crate::camera::Camera;
        use crossterm::event::MouseButton::Left;
        let mut m = Mouse::new();
        m.event(at(MouseEventKind::Down(Left), 10, 10), MouseMode::Drag);
        // Forty-five columns right is a quarter turn at two degrees each.
        let Some(Look::Turn(yaw, _)) = m.event(at(MouseEventKind::Drag(Left), 55, 10), MouseMode::Drag) else { panic!("a drag turns") };
        assert_eq!(yaw, -90.0);
        let mut cam = Camera::isometric(3);
        cam.set_angle(0.0);
        assert_eq!(cam.forward(), (0.0, 1.0), "a yaw of zero looks south");
        cam.rotate_by(yaw.to_radians(), 120, 40);
        let (fx, fy) = cam.forward();
        assert!((fx + 1.0).abs() < 1e-4 && fy.abs() < 1e-4, "looking south and turning right looks west: {fx}, {fy}");
        // The isometric view has no pitch to turn; a chase view has, and it
        // stops at the end of its range.
        let flat = cam.pitch_degrees();
        cam.pitch_by(30f32.to_radians());
        assert_eq!(cam.pitch_degrees(), flat, "the isometric pitch is the footprint's");
        let mut chase = Camera::chase(std::f32::consts::FRAC_PI_4);
        for _ in 0..40 {
            chase.pitch_by((10.0 * Mouse::PITCH_PER_ROW).to_radians());
        }
        assert_eq!(chase.pitch_degrees(), 85, "however far the pointer is dragged down");
        for _ in 0..80 {
            chase.pitch_by((-10.0 * Mouse::PITCH_PER_ROW).to_radians());
        }
        assert_eq!(chase.pitch_degrees(), -80, "and up");
    }

    #[test]
    fn only_the_direction_actions_are_held() {
        assert_eq!(Held::movement(Walk(1, 0)), Some(((1, 0), false)));
        assert_eq!(Held::movement(Run(0, 1)), Some(((0, 1), true)));
        for a in [Pan(1, 0), Centre, Quit, Toggle("inset")] {
            assert_eq!(Held::movement(a), None, "{a:?} is not a direction key");
        }
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
        assert_eq!(&scene[..120], " tab settings  m world map  wasd/hjkl walk  arrows pan  shift+arrows run  c centre  r/R ( ) rotate  z/Z zoom  v fill  g ");
        assert!(scene.find("yubn diagonals").is_some_and(|at| at > 120), "an entry added to the table falls past the columns the golden frames pin");
        for frame in ["inventory", "stats", "history", "conversation"] {
            assert!(SCENE.iter().flat_map(|b| b.keys).any(|&(_, a)| a == Toggle(frame)), "{frame} has no toggle key");
        }
    }
}
