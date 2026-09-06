//! Overlay frames (ADR-005): one rectangle system for everything drawn
//! over the scene.
//!
//! A frame is a row in `assets/ui.toml` — where it goes, how it is framed,
//! when it shows and which key toggles it — plus a `Content` that draws
//! into it and, when focused, takes keys. `Layout` places the rows for a
//! screen size: anchors and sizes resolve, frames whose show rule fails or
//! whose minimum does not fit drop out, and overlaps are settled by
//! priority. The game's content kinds live in `ui.rs`; the generic list
//! and text contents live here.

use std::any::Any;

use crossterm::event::KeyCode;
use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::input::{self, Action};
use crate::map::Map;
use crate::properties::Identity;
use crate::settings::Settings;
use crate::tileset::Tileset;
use crate::ui::CHROME;
use crate::world::World;
use crate::worldmap::WorldMap;

/// A rectangle of cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Whether the two rectangles share a cell.
    pub fn overlaps(&self, o: &Rect) -> bool {
        !self.is_empty() && !o.is_empty() && self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    /// The rectangle shrunk by `n` cells on every side.
    pub fn inset(&self, n: i32) -> Rect {
        Rect { x: self.x + n, y: self.y + n, w: self.w - 2 * n, h: self.h - 2 * n }
    }
}

/// Where a frame sits on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    /// The whole screen.
    Full,
}

/// `"fill"` takes the whole axis; `"auto"` takes what the content asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtentName {
    Fill,
    Auto,
}

/// One axis of a frame's size: whole cells, a fraction of the screen, or a
/// name.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Extent {
    Cells(i32),
    Fraction(f32),
    Named(ExtentName),
}

fn is_zero(n: &i32) -> bool {
    *n == 0
}

/// A frame's size: an extent per axis and a minimum in cells. A frame
/// whose minimum does not fit the screen is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub cols: Extent,
    pub rows: Extent,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min_cols: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min_rows: i32,
}

impl Size {
    /// Cells on one axis: the extent resolved against the screen, never
    /// below the minimum and never over the screen. `preferred` is what
    /// the content asked for, including the border.
    fn axis(e: Extent, screen: i32, min: i32, preferred: Option<i32>) -> i32 {
        let n = match e {
            Extent::Cells(c) => c,
            Extent::Fraction(f) => (screen as f32 * f).round() as i32,
            Extent::Named(ExtentName::Fill) => screen,
            Extent::Named(ExtentName::Auto) => preferred.unwrap_or(min),
        };
        n.max(min).min(screen)
    }
}

/// The chrome around a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Border {
    None,
    Line,
    Double,
    /// Rounded corners, for panes that are part of the furniture rather
    /// than modal windows.
    Chrome,
}

impl Border {
    /// Corner and edge glyphs: top-left, top, top-right, left, right,
    /// bottom-left, bottom, bottom-right.
    pub fn glyphs(self) -> Option<[char; 8]> {
        Some(match self {
            Border::None => return None,
            Border::Line => ['┌', '─', '┐', '│', '│', '└', '─', '┘'],
            Border::Double => ['╔', '═', '╗', '║', '║', '╚', '═', '╝'],
            Border::Chrome => ['╭', '─', '╮', '│', '│', '╰', '─', '╯'],
        })
    }

    /// Cells the border takes on each side.
    pub fn pad(self) -> i32 {
        i32::from(self != Border::None)
    }
}

/// What lies under a frame's content.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    /// The scene shows through; the content paints what it needs.
    None,
    /// Chrome fill. Only opaque frames hide the ones below them.
    Opaque,
    /// The scene tinted toward the chrome colour, 0..1.
    Tint(f32),
}

/// When a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Show {
    Always,
    ZoomedOut,
    ZoomedIn,
    /// Only once its key has been pressed.
    OnKey,
    /// Not yet built.
    Never,
    /// Only on a screen at least this wide.
    MinColumns(i32),
}

impl Show {
    /// Whether the rule lets the frame show at this screen width and zoom.
    pub fn allows(self, cols: i32, zoom: usize, zooms: usize) -> bool {
        match self {
            Show::Always | Show::OnKey => true,
            Show::Never => false,
            Show::ZoomedOut => zoom * 2 < zooms,
            Show::ZoomedIn => zoom * 2 >= zooms,
            Show::MinColumns(n) => cols >= n,
        }
    }

    /// Whether the frame is open before any key is pressed.
    pub fn starts_open(self) -> bool {
        !matches!(self, Show::OnKey | Show::Never)
    }
}

/// A key named in `ui.toml`: `"tab"`, `"esc"`, `"enter"`, `"space"` or one
/// character.
pub fn parse_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "tab" => KeyCode::Tab,
        "esc" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        s => {
            let mut it = s.chars();
            let c = it.next()?;
            if it.next().is_some() {
                return None;
            }
            KeyCode::Char(c)
        }
    })
}

/// A frame's row in `ui.toml`, resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSpec {
    pub name: String,
    pub identity: Identity,
    /// Shown in the top border; blank for none.
    pub title: String,
    /// The content kind code supplies.
    pub content: String,
    pub anchor: Anchor,
    pub size: Size,
    pub border: Border,
    pub background: Background,
    /// Draw order; focused frames draw last.
    pub z: i32,
    /// Which frames survive when they collide.
    pub priority: i32,
    pub show: Show,
    /// Toggle key, bound to `Action::Toggle(name)` in `input.rs`.
    pub key: Option<KeyCode>,
}

/// Everything a content draws from. Cheap to copy: it is all references.
#[derive(Clone, Copy)]
pub struct FrameCtx<'a> {
    pub map: &'a Map,
    pub world: &'a World,
    pub cam: &'a Camera,
    pub ts: &'a Tileset,
    pub settings: &'a Settings,
    pub wmap: &'a WorldMap,
    /// Point lights the frame carried, placed and discovered.
    pub lights: usize,
    /// Whether this frame has focus.
    pub focused: bool,
}

/// What a focused frame did with a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Flow {
    /// Not for the frame; whoever asked should deal with it.
    Pass,
    Handled,
    /// Close the frame and give focus back to the scene.
    Close,
    /// A prompt collected a line.
    Submit(String),
}

/// What a frame draws and, when focused, what it does with keys.
pub trait Content: Any {
    /// Draw into the frame's interior, inside any border.
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx);

    /// Refresh state from the world, once per frame before drawing.
    fn update(&mut self, ctx: &FrameCtx) {
        let _ = ctx;
    }

    /// A bound action while focused.
    fn input(&mut self, action: Action) -> Flow {
        let _ = action;
        Flow::Pass
    }

    /// A raw key while focused, offered before `input` so a prompt can
    /// collect characters.
    fn key(&mut self, key: KeyCode) -> Flow {
        let _ = key;
        Flow::Pass
    }

    /// The interior size the content would like, for `auto` extents.
    fn preferred(&self, ctx: &FrameCtx) -> Option<(i32, i32)> {
        let _ = ctx;
        None
    }

    /// Whether opening the frame gives it focus. Passive frames only draw.
    fn focusable(&self) -> bool {
        false
    }

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// What `Layout` needs from one frame.
pub struct Placement<'a> {
    pub spec: &'a FrameSpec,
    pub open: bool,
    /// The content's preferred interior size.
    pub preferred: Option<(i32, i32)>,
}

/// The screen the frames are laid out on.
#[derive(Clone, Copy, Debug)]
pub struct LayoutCtx {
    pub cols: i32,
    pub rows: i32,
    pub zoom: usize,
    /// How many zoom levels there are, for the zoomed-in and zoomed-out
    /// show rules.
    pub zooms: usize,
    /// Index of the focused frame, which draws last.
    pub focus: Option<usize>,
}

/// Where every frame ended up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// One entry per frame: its rectangle, or `None` when it is hidden or
    /// dropped.
    pub rects: Vec<Option<Rect>>,
    /// The frames that survived, in draw order.
    pub order: Vec<usize>,
}

impl Layout {
    /// Place every frame: resolve anchors and sizes, drop the ones whose
    /// show rule fails or whose minimum does not fit, and settle overlaps
    /// by priority.
    pub fn resolve(items: &[Placement], lc: &LayoutCtx) -> Layout {
        let mut rects: Vec<Option<Rect>> = items.iter().map(|it| Layout::place(it, lc)).collect();

        // Highest priority first; an opaque frame hides anything below it.
        let mut by_priority: Vec<usize> = (0..items.len()).filter(|&i| rects[i].is_some()).collect();
        by_priority.sort_by_key(|&i| (std::cmp::Reverse(items[i].spec.priority), std::cmp::Reverse(items[i].spec.z), i));
        let mut kept: Vec<usize> = Vec::new();
        for &i in &by_priority {
            let r = rects[i].expect("only placed frames are ranked");
            let hidden = kept.iter().any(|&k| {
                items[k].spec.priority > items[i].spec.priority
                    && matches!(items[k].spec.background, Background::Opaque)
                    && rects[k].expect("kept frames are placed").overlaps(&r)
            });
            if hidden {
                rects[i] = None;
            } else {
                kept.push(i);
            }
        }

        let mut order = kept;
        order.sort_by_key(|&i| (items[i].spec.z, i));
        if let Some(f) = lc.focus {
            if let Some(at) = order.iter().position(|&i| i == f) {
                let i = order.remove(at);
                order.push(i);
            }
        }
        Layout { rects, order }
    }

    fn place(it: &Placement, lc: &LayoutCtx) -> Option<Rect> {
        let s = it.spec;
        if !it.open || !s.show.allows(lc.cols, lc.zoom, lc.zooms) {
            return None;
        }
        let pad = 2 * s.border.pad();
        let w = Size::axis(s.size.cols, lc.cols, s.size.min_cols, it.preferred.map(|p| p.0 + pad));
        let h = Size::axis(s.size.rows, lc.rows, s.size.min_rows, it.preferred.map(|p| p.1 + pad));
        if w <= 0 || h <= 0 || w < s.size.min_cols || h < s.size.min_rows {
            return None;
        }
        let (x, y) = Layout::anchor_at(s.anchor, w, h, lc.cols, lc.rows);
        Some(Rect { x, y, w, h })
    }

    /// The top-left corner an anchor puts a `w` by `h` frame at.
    pub fn anchor_at(anchor: Anchor, w: i32, h: i32, cols: i32, rows: i32) -> (i32, i32) {
        let (cx, cy) = ((cols - w) / 2, (rows - h) / 2);
        let (rx, by) = (cols - w, rows - h);
        match anchor {
            Anchor::TopLeft => (0, 0),
            Anchor::Top => (cx, 0),
            Anchor::TopRight => (rx, 0),
            Anchor::Left => (0, cy),
            Anchor::Centre | Anchor::Full => (cx, cy),
            Anchor::Right => (rx, cy),
            Anchor::BottomLeft => (0, by),
            Anchor::Bottom => (cx, by),
            Anchor::BottomRight => (rx, by),
        }
    }

    pub fn rect(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).copied().flatten()
    }
}

/// Fill and border. The interior is left to the content, which draws over
/// the fill.
pub fn draw_chrome(cv: &mut Canvas, rect: Rect, spec: &FrameSpec) {
    if rect.is_empty() {
        return;
    }
    let (fill, edge, title) = (CHROME.panel, CHROME.panel_dim, CHROME.panel_text);
    match spec.background {
        Background::None => {}
        Background::Opaque => {
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    cv.put(x, y, ' ', edge, fill);
                }
            }
        }
        Background::Tint(strength) => {
            let t = strength.clamp(0.0, 1.0);
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    if let Some(c) = cell_at(cv, x, y) {
                        cv.put(x, y, c.0, c.1.lerp(fill, t), c.2.lerp(fill, t));
                    }
                }
            }
        }
    }
    if let Some(g) = spec.border.glyphs() {
        let (x1, y1) = (rect.right() - 1, rect.bottom() - 1);
        for x in rect.x..=x1 {
            cv.put(x, rect.y, g[1], edge, fill);
            cv.put(x, y1, g[6], edge, fill);
        }
        for y in rect.y..=y1 {
            cv.put(rect.x, y, g[3], edge, fill);
            cv.put(x1, y, g[4], edge, fill);
        }
        cv.put(rect.x, rect.y, g[0], edge, fill);
        cv.put(x1, rect.y, g[2], edge, fill);
        cv.put(rect.x, y1, g[5], edge, fill);
        cv.put(x1, y1, g[7], edge, fill);
        if !spec.title.is_empty() && rect.w > 5 {
            cv.text(rect.x + 2, rect.y, &clip(&format!(" {} ", spec.title), rect.w - 3), title, fill);
        }
    }
}

fn cell_at(cv: &Canvas, x: i32, y: i32) -> Option<(char, Rgb, Rgb)> {
    if x < 0 || y < 0 || x >= cv.w || y >= cv.h {
        return None;
    }
    let c = cv.cells[(y * cv.w + x) as usize];
    Some((c.ch, c.fg, c.bg))
}

/// Truncate to a width, counting characters.
pub fn clip(s: &str, w: i32) -> String {
    s.chars().take(w.max(0) as usize).collect()
}

/// Truncate, then pad with spaces to exactly `w` characters.
pub fn pad(s: &str, w: i32) -> String {
    let mut out = clip(s, w);
    for _ in out.chars().count() as i32..w {
        out.push(' ');
    }
    out
}

/// First row shown so the cursor stays visible in `len` rows of `height`.
pub fn window(cursor: usize, len: usize, height: usize) -> usize {
    if len <= height || height == 0 {
        return 0;
    }
    cursor.saturating_sub(height / 2).min(len - height)
}

/// Greedy word wrap. An empty paragraph is one empty line.
pub fn wrap(text: &str, width: i32) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: &str = word;
        while word.chars().count() > width {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            let cut = word.char_indices().nth(width).expect("the word is longer than the width").0;
            out.push(word[..cut].to_string());
            word = &word[cut..];
        }
        if line.is_empty() {
            line = word.to_string();
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            out.push(std::mem::replace(&mut line, word.to_string()));
        }
    }
    if out.is_empty() || !line.is_empty() {
        out.push(line);
    }
    out
}

// Generic contents.

/// One row of a list: a title and an optional detail line under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub title: String,
    pub detail: Option<String>,
}

impl Item {
    pub fn new(title: impl Into<String>) -> Item {
        Item { title: title.into(), detail: None }
    }

    pub fn detailed(title: impl Into<String>, detail: impl Into<String>) -> Item {
        Item { title: title.into(), detail: Some(detail.into()) }
    }
}

/// A scrolling list with a selection: the inventory, the stats readout and
/// the history log.
#[derive(Debug, Default)]
pub struct List {
    pub items: Vec<Item>,
    pub cursor: usize,
    /// Shown in place of an empty list.
    pub hint: String,
    /// Rebuilt from the world every frame when set.
    pub refresh: Option<fn(&FrameCtx) -> Vec<Item>>,
    /// Ring limit for pushed items; zero keeps everything.
    pub capacity: usize,
}

impl List {
    pub fn new() -> List {
        List::default()
    }

    /// An empty list with a line explaining why.
    pub fn hinted(hint: &str) -> List {
        List { hint: hint.to_string(), ..List::default() }
    }

    /// A list rebuilt from the world every frame.
    pub fn live(refresh: fn(&FrameCtx) -> Vec<Item>) -> List {
        List { refresh: Some(refresh), ..List::default() }
    }

    /// A log that keeps the last `capacity` items.
    pub fn ring(capacity: usize, hint: &str) -> List {
        List { capacity, hint: hint.to_string(), ..List::default() }
    }

    /// Append, dropping the oldest item once the ring is full. The cursor
    /// follows the newest item when it was already on it.
    pub fn push(&mut self, item: Item) {
        let at_end = self.items.is_empty() || self.cursor + 1 >= self.items.len();
        self.items.push(item);
        if self.capacity > 0 && self.items.len() > self.capacity {
            let drop = self.items.len() - self.capacity;
            self.items.drain(..drop);
        }
        if at_end {
            self.cursor = self.items.len() - 1;
        } else {
            self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        }
    }

    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.cursor)
    }

    /// Move the selection, stopping at the ends.
    pub fn move_cursor(&mut self, dir: i32) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        let last = self.items.len() as i32 - 1;
        self.cursor = (self.cursor as i32 + dir).clamp(0, last) as usize;
    }

    /// The rows the list draws: (item, whether it is the detail line).
    fn rows(&self) -> Vec<(usize, bool)> {
        let mut rows = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            rows.push((i, false));
            if it.detail.is_some() {
                rows.push((i, true));
            }
        }
        rows
    }
}

impl Content for List {
    fn update(&mut self, ctx: &FrameCtx) {
        if let Some(f) = self.refresh {
            self.items = f(ctx);
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    fn draw(&self, cv: &mut Canvas, rect: Rect, _ctx: &FrameCtx) {
        if rect.is_empty() {
            return;
        }
        let (fg, bg, dim) = (CHROME.panel_text, CHROME.panel, CHROME.panel_dim);
        if self.items.is_empty() {
            cv.text(rect.x, rect.y, &pad(&self.hint, rect.w), dim, bg);
            return;
        }
        let rows = self.rows();
        let cursor_row = rows.iter().position(|&(i, d)| i == self.cursor && !d).unwrap_or(0);
        let top = window(cursor_row, rows.len(), rect.h as usize);
        for r in 0..rect.h {
            let y = rect.y + r;
            let Some(&(i, detail)) = rows.get(top + r as usize) else {
                cv.text(rect.x, y, &pad("", rect.w), dim, bg);
                continue;
            };
            let item = &self.items[i];
            let selected = i == self.cursor;
            let text = match detail {
                false => format!(" {}", item.title),
                true => format!("   {}", item.detail.as_deref().unwrap_or("")),
            };
            let (lf, lb) = match (selected, detail) {
                (true, false) => (bg, CHROME.selected),
                (true, true) => (fg, bg),
                (false, false) => (fg, bg),
                (false, true) => (dim, bg),
            };
            cv.text(rect.x, y, &pad(&text, rect.w), lf, lb);
        }
    }

    fn input(&mut self, action: Action) -> Flow {
        match action {
            Action::CursorMove(dir) => {
                self.move_cursor(dir);
                Flow::Handled
            }
            Action::Close => Flow::Close,
            _ => Flow::Pass,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Wrapped paragraphs that scroll, with an optional prompt line that
/// collects a string: the conversation frame.
#[derive(Debug, Default)]
pub struct Text {
    pub body: Vec<String>,
    /// First wrapped line shown.
    pub scroll: usize,
    /// Prompt label; without one the frame is read-only.
    pub prompt: Option<String>,
    /// What has been typed so far.
    pub entry: String,
}

impl Text {
    pub fn new(body: Vec<String>) -> Text {
        Text { body, ..Text::default() }
    }

    /// A text frame with a prompt line.
    pub fn prompting(body: Vec<String>, prompt: &str) -> Text {
        Text { body, prompt: Some(prompt.to_string()), ..Text::default() }
    }

    pub fn push(&mut self, line: impl Into<String>) {
        self.body.push(line.into());
    }

    /// Every paragraph wrapped to `width`.
    pub fn lines(&self, width: i32) -> Vec<String> {
        self.body.iter().flat_map(|p| wrap(p, width)).collect()
    }

    /// Rows the body may use inside a `h`-row interior.
    fn body_rows(&self, h: i32) -> i32 {
        (h - i32::from(self.prompt.is_some())).max(0)
    }
}

impl Content for Text {
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx) {
        if rect.is_empty() {
            return;
        }
        let (fg, bg, dim) = (CHROME.panel_text, CHROME.panel, CHROME.panel_dim);
        let body_h = self.body_rows(rect.h);
        let lines = self.lines(rect.w);
        let top = self.scroll.min(lines.len().saturating_sub(body_h.max(0) as usize));
        for r in 0..body_h {
            let text = lines.get(top + r as usize).map(String::as_str).unwrap_or("");
            cv.text(rect.x, rect.y + r, &pad(text, rect.w), fg, bg);
        }
        if let Some(p) = &self.prompt {
            let caret = if ctx.focused { "_" } else { "" };
            cv.text(rect.x, rect.y + body_h, &pad(&format!("{p}{}{caret}", self.entry), rect.w), dim, bg);
        }
    }

    fn input(&mut self, action: Action) -> Flow {
        match action {
            Action::CursorMove(dir) => {
                self.scroll = (self.scroll as i32 + dir).max(0) as usize;
                Flow::Handled
            }
            Action::Close => Flow::Close,
            _ => Flow::Pass,
        }
    }

    fn key(&mut self, key: KeyCode) -> Flow {
        if self.prompt.is_none() {
            return Flow::Pass;
        }
        match key {
            KeyCode::Char(c) => {
                self.entry.push(c);
                Flow::Handled
            }
            KeyCode::Backspace => {
                self.entry.pop();
                Flow::Handled
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.entry);
                if line.is_empty() {
                    return Flow::Handled;
                }
                self.push(format!("> {line}"));
                self.scroll = usize::MAX / 2;
                Flow::Submit(line)
            }
            _ => Flow::Pass,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// The frame set.

/// A frame: its row, whether it is open, and its content.
pub struct Frame {
    pub spec: FrameSpec,
    pub open: bool,
    pub content: Box<dyn Content>,
}

/// Every frame of one screen, with at most one of them focused.
#[derive(Default)]
pub struct Frames {
    frames: Vec<Frame>,
    focus: Option<usize>,
}

impl Frames {
    /// Build the set from the loaded rows, asking `content_for` for a
    /// content per kind. An unknown kind is an error naming the frame.
    pub fn new(specs: &[FrameSpec], content_for: &dyn Fn(&str) -> Option<Box<dyn Content>>) -> Result<Frames, String> {
        let mut frames = Vec::with_capacity(specs.len());
        for spec in specs {
            let content = content_for(&spec.content).ok_or_else(|| format!("frame {:?}: unknown content kind {:?}", spec.name, spec.content))?;
            frames.push(Frame { open: spec.show.starts_open(), spec: spec.clone(), content });
        }
        Ok(Frames { frames, focus: None })
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Frame> {
        self.frames.iter()
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.frames.iter().position(|f| f.spec.name == name)
    }

    pub fn get(&self, name: &str) -> Option<&Frame> {
        self.index(name).map(|i| &self.frames[i])
    }

    pub fn is_open(&self, name: &str) -> bool {
        self.get(name).is_some_and(|f| f.open)
    }

    /// Open or close a frame; opening one that takes focus focuses it.
    pub fn set_open(&mut self, name: &str, open: bool) {
        let Some(i) = self.index(name) else { return };
        if self.frames[i].open == open {
            return;
        }
        self.frames[i].open = open;
        if open {
            if self.frames[i].content.focusable() {
                self.focus = Some(i);
            }
        } else if self.focus == Some(i) {
            self.focus = None;
        }
    }

    /// Flip a frame open or shut; returns whether it is now open.
    pub fn toggle(&mut self, name: &str) -> bool {
        let open = !self.is_open(name);
        self.set_open(name, open);
        open
    }

    /// The focused frame's name, or `None` when the scene has focus.
    pub fn focus(&self) -> Option<&str> {
        self.focus.map(|i| self.frames[i].spec.name.as_str())
    }

    /// Focus a frame by name, or give focus back to the scene.
    pub fn set_focus(&mut self, name: Option<&str>) {
        self.focus = match name {
            None => None,
            Some(n) => match self.index(n) {
                Some(i) if self.frames[i].open && self.frames[i].content.focusable() => Some(i),
                _ => return,
            },
        };
    }

    /// A frame's content, downcast to what code put there.
    pub fn content_mut<T: Content>(&mut self, name: &str) -> Option<&mut T> {
        let i = self.index(name)?;
        self.frames[i].content.as_any_mut().downcast_mut::<T>()
    }

    /// Refresh every open frame's content from the world.
    pub fn update(&mut self, ctx: &FrameCtx) {
        for f in &mut self.frames {
            if f.open {
                f.content.update(ctx);
            }
        }
    }

    /// Place the frames on a screen of `cols` by `rows`.
    pub fn layout(&self, cols: i32, rows: i32, ctx: &FrameCtx) -> Layout {
        let items: Vec<Placement> = self.frames.iter().map(|f| Placement { spec: &f.spec, open: f.open, preferred: f.content.preferred(ctx) }).collect();
        Layout::resolve(&items, &LayoutCtx { cols, rows, zoom: ctx.cam.zoom, zooms: crate::tileset::ZOOMS.len(), focus: self.focus })
    }

    /// Draw every frame that survived layout, chrome then content.
    pub fn draw(&self, cv: &mut Canvas, ctx: &FrameCtx) {
        let layout = self.layout(cv.w, cv.h, ctx);
        for &i in &layout.order {
            let Some(rect) = layout.rect(i) else { continue };
            let f = &self.frames[i];
            draw_chrome(cv, rect, &f.spec);
            let inner = rect.inset(f.spec.border.pad());
            if !inner.is_empty() {
                let mut c = *ctx;
                c.focused = self.focus == Some(i);
                f.content.draw(cv, inner, &c);
            }
        }
    }

    /// Route a key to the focused frame: the content's own key handling
    /// first, then the frame binding table, then Escape, which closes the
    /// frame and returns focus to the scene.
    pub fn key(&mut self, key: KeyCode) -> Flow {
        let Some(i) = self.focus else { return Flow::Pass };
        let mut flow = self.frames[i].content.key(key);
        if flow == Flow::Pass {
            if let Some(a) = input::lookup(input::FRAME, key) {
                flow = self.frames[i].content.input(a);
            }
        }
        if flow == Flow::Close || (flow == Flow::Pass && key == KeyCode::Esc) {
            self.frames[i].open = false;
            self.focus = None;
            return Flow::Close;
        }
        flow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, anchor: Anchor, size: Size, priority: i32) -> FrameSpec {
        FrameSpec {
            name: name.to_string(),
            identity: Identity::default(),
            title: String::new(),
            content: name.to_string(),
            anchor,
            size,
            border: Border::None,
            background: Background::Opaque,
            z: priority,
            priority,
            show: Show::Always,
            key: None,
        }
    }

    fn size(cols: Extent, rows: Extent) -> Size {
        Size { cols, rows, min_cols: 0, min_rows: 0 }
    }

    fn ctx(cols: i32, rows: i32) -> LayoutCtx {
        LayoutCtx { cols, rows, zoom: 0, zooms: 7, focus: None }
    }

    fn place(specs: &[FrameSpec], lc: &LayoutCtx) -> Layout {
        let items: Vec<Placement> = specs.iter().map(|s| Placement { spec: s, open: true, preferred: Some((10, 3)) }).collect();
        Layout::resolve(&items, lc)
    }

    #[test]
    fn anchors_and_sizes_resolve_at_both_screen_sizes() {
        let fill1 = size(Extent::Named(ExtentName::Fill), Extent::Cells(1));
        let specs = [
            spec("top", Anchor::Top, fill1, 10),
            spec("bottom", Anchor::Bottom, fill1, 10),
            spec("centre", Anchor::Centre, size(Extent::Named(ExtentName::Auto), Extent::Named(ExtentName::Auto)), 5),
            spec("right", Anchor::Right, size(Extent::Fraction(0.25), Extent::Fraction(0.5)), 1),
            spec("full", Anchor::Full, size(Extent::Named(ExtentName::Fill), Extent::Named(ExtentName::Fill)), 0),
        ];
        // The three that do not collide, on the small screen and the big one.
        for (cols, rows) in [(80, 25), (168, 71)] {
            let l = place(&specs[..3], &ctx(cols, rows));
            assert_eq!(l.rect(0), Some(Rect::new(0, 0, cols, 1)), "the top bar spans the width");
            assert_eq!(l.rect(1), Some(Rect::new(0, rows - 1, cols, 1)), "the bottom bar sits on the last row");
            // Auto takes the content's size, centred.
            assert_eq!(l.rect(2), Some(Rect::new((cols - 10) / 2, (rows - 3) / 2, 10, 3)));
        }
        assert_eq!(place(&specs[3..4], &ctx(168, 71)).rect(0), Some(Rect::new(168 - 42, (71 - 36) / 2, 42, 36)), "a fraction rounds to whole cells");
        assert_eq!(place(&specs[4..5], &ctx(168, 71)).rect(0), Some(Rect::new(0, 0, 168, 71)), "full is the whole screen");
        // A border pays for itself out of an auto size.
        let mut bordered = spec("box", Anchor::Centre, size(Extent::Named(ExtentName::Auto), Extent::Named(ExtentName::Auto)), 0);
        bordered.border = Border::Line;
        assert_eq!(place(&[bordered], &ctx(80, 25)).rect(0), Some(Rect::new((80 - 12) / 2, (25 - 5) / 2, 12, 5)));
    }

    #[test]
    fn frames_that_do_not_fit_are_dropped() {
        let mut wide = spec("wide", Anchor::Centre, size(Extent::Fraction(0.5), Extent::Cells(10)), 1);
        wide.size.min_cols = 100;
        let mut tall = spec("tall", Anchor::Left, size(Extent::Cells(20), Extent::Cells(8)), 1);
        tall.size.min_rows = 40;
        let specs = [wide, tall];
        let small = place(&specs, &ctx(80, 25));
        assert_eq!((small.rect(0), small.rect(1)), (None, None), "neither minimum fits 80x25");
        assert!(small.order.is_empty());
        let big = place(&specs, &ctx(168, 71));
        assert_eq!(big.rect(0), Some(Rect::new((168 - 100) / 2, (71 - 10) / 2, 100, 10)), "the minimum wins over the fraction");
        assert_eq!(big.rect(1), Some(Rect::new(0, (71 - 40) / 2, 20, 40)));
    }

    #[test]
    fn overlaps_go_to_the_higher_priority_frame() {
        let full = size(Extent::Named(ExtentName::Fill), Extent::Named(ExtentName::Fill));
        let bar = size(Extent::Named(ExtentName::Fill), Extent::Cells(1));
        let specs = [spec("hud", Anchor::Top, bar, 20), spec("help", Anchor::Bottom, bar, 20), spec("map", Anchor::Full, full, 90)];
        let l = place(&specs, &ctx(120, 40));
        assert_eq!(l.order, vec![2], "the full-screen map hides both bars");
        assert_eq!((l.rect(0), l.rect(1)), (None, None));
        // A tinted frame lets what is under it through.
        let mut glass = specs[2].clone();
        glass.background = Background::Tint(0.5);
        let l = place(&[specs[0].clone(), specs[1].clone(), glass], &ctx(120, 40));
        assert_eq!(l.order, vec![0, 1, 2], "z order, lowest first");
        // Equal priority never drops either side.
        let mut same = specs[2].clone();
        same.priority = 20;
        assert_eq!(place(&[specs[0].clone(), same], &ctx(120, 40)).order.len(), 2);
        // The focused frame draws last whatever its z.
        let l = place(&specs[..2], &LayoutCtx { focus: Some(0), ..ctx(120, 40) });
        assert_eq!(l.order, vec![1, 0]);
    }

    #[test]
    fn show_rules_gate_on_width_and_zoom() {
        assert!(Show::Always.allows(1, 0, 7) && !Show::Never.allows(1000, 0, 7));
        assert!(Show::MinColumns(100).allows(120, 0, 7) && !Show::MinColumns(100).allows(80, 0, 7));
        assert!(Show::ZoomedOut.allows(80, 0, 7) && !Show::ZoomedOut.allows(80, 6, 7));
        assert!(Show::ZoomedIn.allows(80, 6, 7) && !Show::ZoomedIn.allows(80, 0, 7));
        assert!(Show::Always.starts_open() && !Show::OnKey.starts_open() && !Show::Never.starts_open());
        let mut hidden = spec("inset", Anchor::BottomRight, size(Extent::Cells(20), Extent::Cells(8)), 1);
        hidden.show = Show::Never;
        assert_eq!(place(&[hidden], &ctx(168, 71)).rect(0), None);
    }

    #[test]
    fn keys_are_named_in_the_table() {
        assert_eq!(parse_key("tab"), Some(KeyCode::Tab));
        assert_eq!(parse_key("i"), Some(KeyCode::Char('i')));
        assert_eq!(parse_key("I"), Some(KeyCode::Char('I')));
        assert_eq!(parse_key("space"), Some(KeyCode::Char(' ')));
        assert_eq!(parse_key("nope"), None);
        assert_eq!(parse_key(""), None);
    }

    #[test]
    fn text_wraps_and_the_prompt_collects_a_line() {
        assert_eq!(wrap("the quick brown fox", 9), vec!["the quick", "brown fox"]);
        assert_eq!(wrap("", 10), vec![""], "an empty paragraph is one empty line");
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"], "a word longer than the line is cut");
        let mut t = Text::prompting(vec!["hello there".to_string()], "say: ");
        assert_eq!(t.lines(5), vec!["hello", "there"]);
        for c in "hi".chars() {
            assert_eq!(t.key(KeyCode::Char(c)), Flow::Handled);
        }
        assert_eq!(t.key(KeyCode::Backspace), Flow::Handled);
        assert_eq!(t.entry, "h");
        assert_eq!(t.key(KeyCode::Char('o')), Flow::Handled);
        assert_eq!(t.key(KeyCode::Enter), Flow::Submit("ho".to_string()));
        assert_eq!(t.body.last().map(String::as_str), Some("> ho"), "what was said joins the body");
        assert!(t.entry.is_empty());
        assert_eq!(t.key(KeyCode::Enter), Flow::Handled, "an empty line is not submitted");
        assert_eq!(t.key(KeyCode::Esc), Flow::Pass, "escape is left to the frame");
        assert_eq!(Text::new(vec![]).key(KeyCode::Char('x')), Flow::Pass, "a text frame with no prompt types nothing");
    }

    #[test]
    fn lists_scroll_and_select() {
        let mut l = List::ring(3, "nothing yet");
        assert!(l.selected().is_none());
        for n in 0..5 {
            l.push(Item::detailed(format!("event {n}"), "detail"));
        }
        assert_eq!(l.items.len(), 3, "the ring keeps the last three");
        assert_eq!(l.selected().map(|i| i.title.as_str()), Some("event 4"), "the cursor follows the newest");
        assert_eq!(l.rows().len(), 6, "each item takes a title row and a detail row");
        assert_eq!(l.input(Action::CursorMove(-1)), Flow::Handled);
        assert_eq!(l.cursor, 1);
        l.move_cursor(-9);
        assert_eq!(l.cursor, 0, "the selection stops at the top");
        l.move_cursor(9);
        assert_eq!(l.cursor, 2, "and at the bottom");
        assert_eq!(l.input(Action::Close), Flow::Close);
        assert_eq!(l.input(Action::Teleport), Flow::Pass);
        // The window keeps the cursor in view.
        assert_eq!(window(0, 6, 4), 0);
        assert_eq!(window(5, 6, 4), 2);
        assert_eq!(window(3, 4, 10), 0, "a list shorter than the frame never scrolls");
    }
}
