//! A grid of coloured cells that the renderer draws into and the terminal
//! diffs against its previous frame.

/// 24-bit colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Multiply each channel by `f`, clamped to the byte range.
    pub fn scale(self, f: f32) -> Rgb {
        let m = |c: u8| (c as f32 * f).round().clamp(0.0, 255.0) as u8;
        Rgb(m(self.0), m(self.1), m(self.2))
    }

    /// Linear blend from `self` toward `other` by `t` in 0..=1.
    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }
}

/// One terminal cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

impl Cell {
    pub const BLANK: Cell = Cell { ch: ' ', fg: Rgb(0, 0, 0), bg: Rgb(0, 0, 0) };
}

/// A fixed-size cell grid.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(w: u16, h: u16) -> Canvas {
        Canvas { w: w as i32, h: h as i32, cells: vec![Cell::BLANK; w as usize * h as usize] }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some((y * self.w + x) as usize)
        }
    }

    /// Write a full cell; out-of-range coordinates are ignored.
    #[inline]
    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: Rgb, bg: Rgb) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = Cell { ch, fg, bg };
        }
    }

    /// Write a glyph over whatever background is already there.
    #[inline]
    pub fn glyph(&mut self, x: i32, y: i32, ch: char, fg: Rgb) {
        if let Some(i) = self.idx(x, y) {
            let bg = self.cells[i].bg;
            self.cells[i] = Cell { ch, fg, bg };
        }
    }

    /// Write a string starting at `(x, y)`.
    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: Rgb, bg: Rgb) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i as i32, y, ch, fg, bg);
        }
    }

    /// Write the grid as text: a header line `w h`, then one line per cell
    /// of `codepoint fr fg fb br bg bb`, row-major.
    pub fn dump(&self, path: &str) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        writeln!(f, "{} {}", self.w, self.h)?;
        for c in &self.cells {
            writeln!(f, "{} {} {} {} {} {} {}", c.ch as u32, c.fg.0, c.fg.1, c.fg.2, c.bg.0, c.bg.1, c.bg.2)?;
        }
        Ok(())
    }
}
