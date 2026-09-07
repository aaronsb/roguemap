//! Raw-mode terminal wrapper with double-buffered, diff-only output.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use crate::canvas::{Canvas, Cell, Rgb};

/// What a terminal that speaks the keyboard enhancement protocol is asked
/// for (ADR-008). `REPORT_EVENT_TYPES` is the release and repeat events
/// the held-key set wants; a plain-text key is only reported at all three
/// event types once every key comes as an escape code, so
/// `REPORT_ALL_KEYS_AS_ESCAPE_CODES` comes with it, and
/// `REPORT_ALTERNATE_KEYS` with that, since a shifted letter then arrives
/// as its base key and the alternate codepoint is what makes `R` an `R`
/// again. `DISAMBIGUATE_ESCAPE_CODES` tells a real escape from the start
/// of a sequence.
const ENHANCEMENTS: KeyboardEnhancementFlags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    .union(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    .union(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS)
    .union(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES);

pub struct Terminal {
    out: Stdout,
    front: Canvas,
    back: Canvas,
    dirty: bool,
    /// Whether this terminal was asked for key release events and said it
    /// could, so the flags have to be popped on the way out.
    key_release: bool,
}

impl Terminal {
    /// Enter raw mode and the alternate screen.
    pub fn new() -> io::Result<Terminal> {
        Terminal::enter(false)
    }

    /// The same, asking the terminal to report key releases and repeats
    /// (ADR-008) where it speaks the keyboard enhancement protocol —
    /// Konsole, kitty, foot, WezTerm and Alacritty do — so more than one
    /// direction key can be held at once. `key_release` says whether it
    /// agreed.
    pub fn with_key_release() -> io::Result<Terminal> {
        Terminal::enter(true)
    }

    fn enter(want_key_release: bool) -> io::Result<Terminal> {
        let mut out = io::stdout();
        enable_raw_mode()?;
        execute!(out, EnterAlternateScreen, Hide)?;
        // The query is a round trip to the terminal, so it is asked once,
        // here, and never while the loop is reading keys.
        let key_release = want_key_release && supports_keyboard_enhancement().unwrap_or(false);
        if key_release {
            execute!(out, PushKeyboardEnhancementFlags(ENHANCEMENTS))?;
            PUSHED.store(true, Ordering::SeqCst);
        }
        let (w, h) = terminal::size()?;
        Ok(Terminal { out, front: Canvas::new(w, h), back: Canvas::new(w, h), dirty: true, key_release })
    }

    /// Whether the terminal reports key releases, so a held key is known
    /// to be held rather than guessed at from its repeats (ADR-008).
    pub fn key_release(&self) -> bool {
        self.key_release
    }

    pub fn width(&self) -> i32 {
        self.back.w
    }

    pub fn height(&self) -> i32 {
        self.back.h
    }

    /// The canvas the next frame is drawn into.
    pub fn canvas(&mut self) -> &mut Canvas {
        &mut self.back
    }

    /// Reallocate buffers after the window changed size.
    pub fn resize(&mut self, w: u16, h: u16) {
        self.front = Canvas::new(w, h);
        self.back = Canvas::new(w, h);
        self.dirty = true;
    }

    /// Emit only the cells that differ from the previous frame.
    pub fn present(&mut self) -> io::Result<()> {
        let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
        let mut cur_fg: Option<Rgb> = None;
        let mut cur_bg: Option<Rgb> = None;
        let mut cursor: Option<(i32, i32)> = None;

        for y in 0..self.back.h {
            for x in 0..self.back.w {
                let i = (y * self.back.w + x) as usize;
                let cell: Cell = self.back.cells[i];
                if !self.dirty && self.front.cells[i] == cell {
                    continue;
                }
                if cursor != Some((x, y)) {
                    queue!(buf, MoveTo(x as u16, y as u16))?;
                }
                if cur_fg != Some(cell.fg) {
                    queue!(buf, SetForegroundColor(color(cell.fg)))?;
                    cur_fg = Some(cell.fg);
                }
                if cur_bg != Some(cell.bg) {
                    queue!(buf, SetBackgroundColor(color(cell.bg)))?;
                    cur_bg = Some(cell.bg);
                }
                queue!(buf, Print(cell.ch))?;
                cursor = Some((x + 1, y));
                self.front.cells[i] = cell;
            }
        }
        self.dirty = false;
        self.out.write_all(&buf)?;
        self.out.flush()
    }
}

fn color(c: Rgb) -> Color {
    Color::Rgb { r: c.0, g: c.1, b: c.2 }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore();
    }
}

/// Whether the enhancement flags are on the terminal's stack, so the
/// restore knows to pop them. A process has one terminal.
static PUSHED: AtomicBool = AtomicBool::new(false);

/// Put the terminal back as it was found: pop the keyboard enhancement
/// flags if they were pushed, show the cursor, leave the alternate screen
/// and drop raw mode. Idempotent, so the drop and the panic hook may both
/// run it.
fn restore() {
    let mut out = io::stdout();
    if PUSHED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, Show, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Restore the terminal before a panic prints. Without it the message
/// goes to the alternate screen and vanishes with it, and a terminal left
/// in the keyboard enhancement mode reports keys the shell does not
/// expect.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}
