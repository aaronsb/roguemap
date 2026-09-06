//! Raw-mode terminal wrapper with double-buffered, diff-only output.

use std::io::{self, Stdout, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::style::{Color, Print, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};

use crate::canvas::{Canvas, Cell, Rgb};

pub struct Terminal {
    out: Stdout,
    front: Canvas,
    back: Canvas,
    dirty: bool,
}

impl Terminal {
    /// Enter raw mode and the alternate screen.
    pub fn new() -> io::Result<Terminal> {
        let mut out = io::stdout();
        enable_raw_mode()?;
        execute!(out, EnterAlternateScreen, Hide)?;
        let (w, h) = terminal::size()?;
        Ok(Terminal { out, front: Canvas::new(w, h), back: Canvas::new(w, h), dirty: true })
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
        let _ = execute!(self.out, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}
