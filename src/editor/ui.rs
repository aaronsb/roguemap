//! The editor screen (ADR-003): a tables-and-rows pane on the left, the
//! preview strip, the row form or art grid, and a status line. Below
//! 120x50 the strip shows one tier and the panes shrink; 80x25 is the
//! floor, as for the game.

use super::fields::Kind;
use super::keys::{self, Mode};
use super::preview::{self, pane_size, pane_title};
use super::{Editor, FieldEdit, Level, Pane};
use crate::assets::Tier;
use crate::canvas::{Canvas, Rgb};
use crate::palette::{season_blend, SEASON_NAMES};
use crate::ui::CHROME;

/// Width of the left pane including its border column.
pub const LEFT_W: i32 = 20;
/// Fewest rows the form keeps under the preview strip.
const MIN_FORM_H: i32 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where everything goes, all as interiors (borders lie just outside).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub tables: Rect,
    pub rows: Rect,
    /// The preview strip interior, spanning every pane.
    pub strip: Rect,
    pub panes: Vec<(Tier, Rect)>,
    pub form: Rect,
    pub status_y: i32,
    /// One tier only.
    pub small: bool,
}

/// Lay the screen out for a size. Full-size panes need 120x50; below that
/// one tier is shown, sized to what is left.
pub fn layout(w: i32, h: i32, tables: usize, tier: Tier) -> Layout {
    let (w, h) = (w.max(40), h.max(12));
    let right_x = LEFT_W;
    let right_w = w - LEFT_W - 1;
    let small = w < 120 || h < 50;
    let status_y = h - 1;
    // Rows: 0 top border; 1.. strip; form border; form; status.
    let max_strip = h - 1 - 1 - 1 - MIN_FORM_H;
    let (strip_h, panes): (i32, Vec<(Tier, Rect)>) = if small {
        let (_, nh) = pane_size(tier);
        let strip_h = ((h - 3) / 2).clamp(4, nh).min(max_strip.max(4));
        (strip_h, vec![(tier, Rect { x: right_x, y: 1, w: right_w, h: strip_h })])
    } else {
        let sizes = preview::PANE_SIZES;
        let avail = right_w - (sizes.len() as i32 - 1);
        let nominal: i32 = sizes.iter().map(|s| s.1).sum();
        let scale = if avail >= nominal { 1.0 } else { avail as f32 / nominal as f32 };
        let strip_h = sizes.iter().map(|s| s.2).max().unwrap_or(34).min(max_strip.max(4));
        let mut x = right_x;
        let mut panes = Vec::new();
        for &(t, pw, ph) in &sizes {
            let pw = ((pw as f32 * scale).floor() as i32).max(6);
            panes.push((t, Rect { x, y: 1, w: pw, h: ph.min(strip_h) }));
            x += pw + 1;
        }
        (strip_h, panes)
    };
    let form_y = 1 + strip_h + 1;
    let form = Rect { x: right_x, y: form_y, w: right_w, h: (status_y - form_y).max(1) };
    let strip = Rect { x: right_x, y: 1, w: right_w, h: strip_h };
    let tables_h = (tables as i32).min(((h - 4) / 3).max(4));
    let tables_r = Rect { x: 0, y: 1, w: LEFT_W - 1, h: tables_h };
    let rows_y = 1 + tables_h + 1;
    let rows = Rect { x: 0, y: rows_y, w: LEFT_W - 1, h: (status_y - rows_y).max(1) };
    Layout { tables: tables_r, rows, strip, panes, form, status_y, small }
}

/// First index shown so the cursor stays visible in a list of `len`
/// entries in `height` rows.
pub fn window(cursor: usize, len: usize, height: usize) -> usize {
    if len <= height || height == 0 {
        return 0;
    }
    let half = height / 2;
    cursor.saturating_sub(half).min(len - height)
}

/// Truncate to a width, counting characters.
fn clip(s: &str, w: i32) -> String {
    s.chars().take(w.max(0) as usize).collect()
}

fn pad(s: &str, w: i32) -> String {
    let mut out = clip(s, w);
    let n = out.chars().count() as i32;
    for _ in n..w {
        out.push(' ');
    }
    out
}

fn hline(cv: &mut Canvas, x0: i32, x1: i32, y: i32, fg: Rgb, bg: Rgb) {
    for x in x0..=x1 {
        cv.put(x, y, '─', fg, bg);
    }
}

fn vline(cv: &mut Canvas, x: i32, y0: i32, y1: i32, fg: Rgb, bg: Rgb) {
    for y in y0..=y1 {
        cv.put(x, y, '│', fg, bg);
    }
}

fn fill(cv: &mut Canvas, r: Rect, bg: Rgb) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            cv.put(x, y, ' ', bg, bg);
        }
    }
}

/// A title set into a horizontal border.
fn title(cv: &mut Canvas, x: i32, y: i32, text: &str, w: i32, fg: Rgb, bg: Rgb) {
    let t = clip(&format!(" {text} "), w);
    cv.text(x, y, &t, fg, bg);
}

/// Draw the whole editor screen.
pub fn draw(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, dim) = (CHROME.panel, CHROME.panel_dim);
    fill(cv, Rect { x: 0, y: 0, w: cv.w, h: cv.h }, bg);
    let w = cv.w;

    // Borders.
    hline(cv, 0, w - 1, 0, dim, bg);
    vline(cv, LEFT_W - 1, 0, l.status_y - 1, dim, bg);
    vline(cv, w - 1, 0, l.status_y - 1, dim, bg);
    cv.put(0, 0, '┌', dim, bg);
    cv.put(LEFT_W - 1, 0, '┬', dim, bg);
    cv.put(w - 1, 0, '┐', dim, bg);
    let rows_border = l.rows.y - 1;
    hline(cv, 0, LEFT_W - 2, rows_border, dim, bg);
    cv.put(LEFT_W - 1, rows_border, '┤', dim, bg);
    let form_border = l.form.y - 1;
    hline(cv, LEFT_W, w - 2, form_border, dim, bg);
    cv.put(LEFT_W - 1, form_border, '├', dim, bg);
    cv.put(w - 1, form_border, '┤', dim, bg);
    if rows_border == form_border {
        cv.put(LEFT_W - 1, form_border, '┼', dim, bg);
    }

    draw_tables(cv, ed);
    draw_rows(cv, ed);
    draw_previews(cv, ed);
    match ed.mode {
        Mode::Grid => draw_grid(cv, ed),
        Mode::Picker => draw_picker(cv, ed),
        _ => draw_form(cv, ed),
    }
    draw_status(cv, ed);
}

fn draw_tables(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    title(cv, 1, 0, "tables", LEFT_W - 3, fg, bg);
    let n = ed.doc.table_count();
    let start = window(ed.table, n, l.tables.h as usize);
    for (line, t) in (start..n).take(l.tables.h as usize).enumerate() {
        let kind = ed.doc.kind(t);
        let dirty = match kind {
            super::fields::TableKind::Art => ed.doc.art.iter().any(|a| a.dirty),
            super::fields::TableKind::Tilesets => ed.doc.files.iter().any(|f| f.dirty && f.kind == super::document::FileKind::Tileset),
            _ => ed.doc.tables[t].file.is_some_and(|f| ed.doc.files[f].dirty),
        };
        let selected = t == ed.table;
        let mark = if selected && ed.pane == Pane::Tables { '>' } else { ' ' };
        let text = format!("{mark}{}{}", kind.name(), if dirty { " *" } else { "" });
        let (tf, tb) = if selected { (bg, if ed.pane == Pane::Tables { CHROME.selected } else { dim }) } else { (fg, bg) };
        cv.text(l.tables.x, l.tables.y + line as i32, &pad(&text, l.tables.w), tf, tb);
    }
}

fn draw_rows(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    title(cv, 1, l.rows.y - 1, "rows", LEFT_W - 3, fg, bg);
    let entries = ed.entries(ed.table);
    let cursor = ed.row_cursor[ed.table].min(entries.len().saturating_sub(1));
    let start = window(cursor, entries.len(), l.rows.h as usize);
    for (line, (i, e)) in entries.iter().enumerate().skip(start).take(l.rows.h as usize).enumerate() {
        let selected = i == cursor;
        let mark = if selected && ed.pane == Pane::Rows { '>' } else { ' ' };
        let text = match e.art {
            Some(ai) if ed.kind() != super::fields::TableKind::Art => format!("{mark} + {}", ed.doc.art.get(ai).map(|a| a.file.tier.name()).unwrap_or("?")),
            _ => format!("{mark}{}", ed.doc.row_name(ed.table, e.row)),
        };
        let (tf, tb) = if selected { (bg, if ed.pane == Pane::Rows { CHROME.selected } else { dim }) } else if e.art.is_some() && ed.kind() != super::fields::TableKind::Art { (dim, bg) } else { (fg, bg) };
        cv.text(l.rows.x, l.rows.y + line as i32, &pad(&text, l.rows.w), tf, tb);
    }
}

fn draw_previews(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    let s = &ed.settings;
    let biome = ed.assets.biomes.get(ed.fixture_biome()).map(|b| b.name.as_str()).unwrap_or("?");
    let season = SEASON_NAMES[season_blend(s.season).0];
    let tod = ed.fixture_tod();
    let head = format!(
        "{}: {}  |  {}  |  biome {biome}  {season} {:02}:{:02}  {}  {}deg",
        ed.kind().name(),
        ed.doc.row_name(ed.table, ed.row()),
        ed.caption(),
        tod.floor() as i32,
        (tod.fract() * 60.0) as i32,
        ed.glyphs_name(),
        (s.angle.to_degrees().round() as i32).rem_euclid(360),
    );
    title(cv, LEFT_W + 1, 0, &head, l.strip.w - 2, fg, bg);
    // Pane frames sit inside the strip; titles go on the line below the
    // top border so the strip title keeps the full width.
    for (i, (tier, r)) in l.panes.iter().enumerate() {
        if let Some(p) = ed.preview.panes.get(i) {
            cv.blit(&p.canvas, r.x, r.y);
        }
        let label = format!(" {} ", pane_title(*tier));
        let label = clip(&label, r.w);
        cv.text(r.x, r.y, &label, dim, bg);
        if i + 1 < l.panes.len() {
            vline(cv, r.x + r.w, l.strip.y, l.strip.y + l.strip.h - 1, dim, bg);
        }
    }
}

impl Editor {
    /// The hour the previews are drawn at.
    pub fn fixture_tod(&self) -> f32 {
        if self.settings.tod_auto && self.kind() == super::fields::TableKind::Lights {
            22.0
        } else {
            self.settings.tod
        }
    }
}

/// The value column of a form line, with the live editor state when the
/// field is being edited.
fn field_text(ed: &Editor, item: &super::FormItem, editing: bool, w: i32) -> Vec<(String, bool)> {
    // Segments of text with a highlight flag.
    if !editing {
        let shown = item.shown();
        if item.value.is_none() {
            return vec![(if item.required { "(missing)".to_string() } else { "-".to_string() }, false)];
        }
        return vec![(clip(&shown, w), false)];
    }
    match ed.field_edit.as_ref() {
        Some(FieldEdit::Text { buf, cursor }) => {
            let start = cursor.saturating_sub((w - 2).max(1) as usize);
            let before: String = buf[start..*cursor].iter().collect();
            let at: String = buf.get(*cursor).map(|c| c.to_string()).unwrap_or_else(|| " ".to_string());
            let after: String = buf[(*cursor + 1).min(buf.len())..].iter().collect();
            vec![(before, false), (at, true), (after, false)]
        }
        Some(FieldEdit::Choice { options, index }) => vec![("< ".to_string(), false), (options.get(*index).cloned().unwrap_or_default(), true), (" >".to_string(), false)],
        Some(FieldEdit::Color { channels, index }) => {
            let mut out = Vec::new();
            for (i, c) in channels.iter().enumerate() {
                let open = i % 3 == 0;
                out.push((if open { if i == 0 { "[" } else { " [" } } else { "," }.to_string(), false));
                out.push((c.to_string(), i == *index));
                if i % 3 == 2 {
                    out.push(("]".to_string(), false));
                }
            }
            out
        }
        Some(FieldEdit::Checklist { options, on, index }) => {
            let mut out = Vec::new();
            for (i, (o, on)) in options.iter().zip(on).enumerate() {
                out.push((format!("{}{} ", if *on { "[x]" } else { "[ ]" }, o), i == *index));
            }
            out
        }
        None => vec![(item.shown(), false)],
    }
}

fn draw_form(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    let head = format!("row: {}  {}", ed.doc.row_name(ed.table, ed.row()), ed.doc.path_of(ed.table, ed.row()));
    title(cv, LEFT_W + 1, l.form.y - 1, &head, l.form.w - 2, fg, bg);
    let items = ed.form();
    if items.is_empty() {
        cv.text(l.form.x + 1, l.form.y, "no fields; Enter on an art entry opens the grid", dim, bg);
        return;
    }
    let label_w = items.iter().map(|i| i.name.chars().count()).max().unwrap_or(8).clamp(6, 22) as i32;
    let cursor = ed.field.min(items.len() - 1);
    let start = window(cursor, items.len(), l.form.h as usize);
    let val_w = l.form.w - label_w - 4;
    for (line, (i, item)) in items.iter().enumerate().skip(start).take(l.form.h as usize).enumerate() {
        let y = l.form.y + line as i32;
        let selected = i == cursor;
        let editing = selected && ed.mode == Mode::Field;
        let active = selected && ed.pane == Pane::Form;
        let (lf, lb) = if active && !editing { (bg, CHROME.selected) } else { (if item.value.is_none() { dim } else { fg }, bg) };
        let mark = if active { '>' } else { ' ' };
        cv.text(l.form.x, y, &pad(&format!("{mark}{}", clip(&item.name, label_w)), label_w + 2), lf, lb);
        let mut x = l.form.x + label_w + 3;
        let mut left = val_w;
        for (seg, hi) in field_text(ed, item, editing, val_w) {
            if left <= 0 {
                break;
            }
            let s = clip(&seg, left);
            let (sf, sb) = if hi { (bg, CHROME.selected) } else if active && !editing { (fg, bg) } else if item.value.is_none() { (dim, bg) } else { (fg, bg) };
            cv.text(x, y, &s, sf, sb);
            let n = s.chars().count() as i32;
            x += n;
            left -= n;
        }
        if matches!(item.kind, Kind::Any) && item.value.is_some() && !editing {
            let hint = clip(" toml", left);
            cv.text(x, y, &hint, dim, bg);
        }
    }
}

fn draw_grid(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    let Some(g) = ed.grid else { return };
    let Some(art) = ed.grid_art() else { return };
    let w = art.rows.first().map(|r| r.chars().count()).unwrap_or(0) as i32;
    let head = format!("art: {} {}  {}x{}  center={} base_rows={}  cursor {},{}", art.name, art.tier.name(), w, art.rows.len(), art.center, art.base_rows, g.cx, g.cy);
    title(cv, LEFT_W + 1, l.form.y - 1, &head, l.form.w - 2, fg, bg);
    let x0 = l.form.x + 2;
    let y0 = l.form.y + 1;
    // Column ruler with the centre marked.
    for c in 0..w.min(l.form.w - 3) {
        let ch = if c == art.center { '^' } else if c % 5 == 0 { '·' } else { ' ' };
        cv.put(x0 + c, l.form.y, ch, dim, bg);
    }
    let base_from = art.rows.len().saturating_sub(art.base_rows);
    let vis_h = (l.form.h - 2).max(1) as usize;
    let start = window(g.cy, art.rows.len(), vis_h);
    for (line, (r, row)) in art.rows.iter().enumerate().skip(start).take(vis_h).enumerate() {
        let y = y0 + line as i32;
        let base = r >= base_from;
        cv.put(x0 - 1, y, if base { '▌' } else { ' ' }, dim, bg);
        let ground = if base { Rgb(70, 52, 40) } else { Rgb(44, 62, 48) };
        for (c, ch) in row.chars().enumerate().take((l.form.w - 3).max(0) as usize) {
            let here = r == g.cy && c == g.cx;
            let shown = if ch == ' ' { '·' } else { ch };
            let (cf, cb) = if here { (bg, CHROME.selected) } else if ch == ' ' { (dim, ground) } else { (fg, ground) };
            cv.put(x0 + c as i32, y, shown, cf, cb);
        }
    }
}

fn draw_picker(cv: &mut Canvas, ed: &Editor) {
    let l = &ed.layout;
    let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
    let Some(p) = &ed.picker else { return };
    let cols = (l.form.w.max(2) / 2) as usize;
    let rows = l.form.h.max(1) as usize;
    let head = format!("glyph picker  {}/{}  U+{:04X}", p.index + 1, p.items.len(), p.items.get(p.index).map(|c| *c as u32).unwrap_or(0));
    title(cv, LEFT_W + 1, l.form.y - 1, &head, l.form.w - 2, fg, bg);
    let start_row = (p.index / cols).saturating_sub(rows / 2).min((p.items.len() / cols + 1).saturating_sub(rows));
    for line in 0..rows {
        for c in 0..cols {
            let i = (start_row + line) * cols + c;
            let Some(&ch) = p.items.get(i) else { break };
            let (cf, cb) = if i == p.index { (bg, CHROME.selected) } else { (fg, bg) };
            cv.put(l.form.x + c as i32 * 2, l.form.y + line as i32, ch, cf, cb);
            cv.put(l.form.x + c as i32 * 2 + 1, l.form.y + line as i32, ' ', dim, bg);
        }
    }
}

fn draw_status(cv: &mut Canvas, ed: &Editor) {
    let y = ed.layout.status_y;
    let file = ed.doc.path_of(ed.table, ed.row());
    let dirty = if ed.doc.is_dirty() { " *" } else { "" };
    let msg = if ed.status.text.is_empty() { keys::help_line(ed.mode, "  ") } else { format!(" {} ", ed.status.text) };
    let line = format!(" {} {file}{dirty} {msg}", ed.mode.name());
    let fg = match ed.status.level {
        _ if ed.status.text.is_empty() => CHROME.dim,
        Level::Info => CHROME.text,
        Level::Warn => Rgb(240, 200, 90),
        Level::Error => Rgb(250, 120, 110),
    };
    cv.text(0, y, &pad(&line, cv.w), fg, CHROME.bar);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_fit_the_floor_and_the_reference_size() {
        let l = layout(80, 25, 13, Tier::Large);
        assert!(l.small);
        assert_eq!(l.panes.len(), 1);
        assert_eq!(l.status_y, 24);
        assert!(l.form.h >= MIN_FORM_H, "{l:?}");
        assert!(l.form.y + l.form.h <= l.status_y);
        assert!(l.rows.y + l.rows.h <= l.status_y);
        assert!(l.tables.h >= 4);
        let l = layout(168, 71, 13, Tier::Large);
        assert!(!l.small);
        assert_eq!(l.panes.len(), 4);
        assert_eq!(l.panes[3].1.w, 56);
        assert_eq!(l.panes[3].1.h, 34);
        assert!(l.panes[3].1.x + l.panes[3].1.w < 168);
        assert_eq!(l.tables.h, 13);
        assert!(l.form.h > 20);
        let l = layout(120, 50, 13, Tier::Large);
        assert!(!l.small);
        assert!(l.panes.iter().all(|(_, r)| r.w >= 6));
        assert!(l.panes[3].1.x + l.panes[3].1.w < 120);
        assert!(l.form.h >= MIN_FORM_H);
    }

    #[test]
    fn windows_keep_the_cursor_visible() {
        assert_eq!(window(0, 30, 10), 0);
        assert_eq!(window(29, 30, 10), 20);
        assert_eq!(window(15, 30, 10), 10);
        assert_eq!(window(3, 5, 10), 0);
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(clip("abcdef", 3), "abc");
    }
}
