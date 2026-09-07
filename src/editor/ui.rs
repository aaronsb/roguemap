//! The editor screen as frames (ADR-005 step 6): every pane is a row in
//! `assets/editor-ui.toml`, placed by `src/frame.rs`, and this module
//! supplies the content kind each row names — the table and row lists, the
//! preview strip and the one pane behind it, the row form, the art grid,
//! the glyph picker and the status line.
//!
//! The screen tiles: the lists hold a twenty-cell left column, the strip
//! and the form take the rest, and each pane draws a line along its top,
//! which carries its title, and one down its right, so no edge is drawn
//! twice. Nothing here decides where a pane goes. The 80x25 collapse is
//! the strip row's own minimum size, and priority hides the one-tier pane
//! wherever the four-tier strip fits; that one pane drops to a tier that
//! fits when it is too short for the tier asked for
//! (`preview::fitting_tier`) and its label says which tier it is drawn at
//! and why.

use super::fields::Kind;
use super::keys::{self, Mode};
use super::preview::{self, pane_size, pane_title};
use super::{Editor, FieldEdit, Level, Pane};
use crate::assets::{Assets, Tier};
use crate::canvas::{Canvas, Rgb};
use crate::frame::{self, Panes, Rect};
use crate::palette::{season_blend, SEASON_NAMES};
use crate::ui::CHROME;

/// Width of the left column including its border column.
pub const LEFT_W: i32 = 20;
/// Fewest rows the form keeps under the preview strip.
const MIN_FORM_H: i32 = 8;

/// The content kinds `assets/editor-ui.toml` may name.
pub const CONTENT_KINDS: [&str; 8] = ["tables", "rows", "strip", "pane", "form", "grid", "picker", "status"];

/// A pane per kind; the loader has already checked the name is one of
/// `CONTENT_KINDS`.
pub fn pane_for(kind: &str) -> Option<Box<dyn frame::Pane<Editor>>> {
    Some(match kind {
        "tables" => Box::new(Tables) as Box<dyn frame::Pane<Editor>>,
        "rows" => Box::new(Rows),
        "strip" => Box::new(Strip { one: false }),
        "pane" => Box::new(Strip { one: true }),
        "form" => Box::new(Form),
        "grid" => Box::new(Grid),
        "picker" => Box::new(Picker),
        "status" => Box::new(Status),
        _ => return None,
    })
}

/// The editor's frame set from `assets/editor-ui.toml`.
pub fn panes(assets: &Assets) -> Panes<Editor> {
    Panes::new(&assets.editor_frames, &pane_for).expect("the loader checked every content kind")
}

/// Where everything ended up, as interiors: what the frames left their
/// contents, kept on the `Editor` so the previews and the key handlers can
/// read it without laying out again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub tables: Rect,
    pub rows: Rect,
    /// The preview strip interior, spanning every pane.
    pub strip: Rect,
    pub panes: Vec<(Tier, Rect)>,
    /// The form, the art grid or the glyph picker: they share a rectangle
    /// and the mode says which of them is open.
    pub form: Rect,
    pub status_y: i32,
    /// One tier only: the strip did not fit and the one pane showed.
    pub small: bool,
}

/// Read the frames' rectangles back out of a resolved layout.
pub fn resolve(panes: &Panes<Editor>, placed: &frame::Layout, ed: &Editor) -> Layout {
    let interior = |name: &str| panes.interior(placed, name);
    let small = interior("strip").is_none();
    let strip = interior(if small { "pane" } else { "strip" }).unwrap_or_default();
    let form = ["form", "grid", "picker"].iter().find_map(|n| interior(n)).unwrap_or_default();
    Layout {
        tables: interior("tables").unwrap_or_default(),
        rows: interior("rows").unwrap_or_default(),
        strip,
        panes: preview::tiles(strip, ed.settings.tier, small),
        form,
        status_y: interior("status").map(|r| r.y).unwrap_or(ed.sh - 1),
        small,
    }
}

/// Rows the table list asks for: one per table, but never more than a
/// third of the screen.
fn table_rows(ed: &Editor) -> i32 {
    (ed.doc.table_count() as i32).min(((ed.sh - 4) / 3).max(4))
}

/// Rows the preview strip asks for: the tallest pane of the four, or half
/// the screen for the one pane of a small screen, and in both cases no
/// more than what leaves the form its floor.
fn strip_rows(ed: &Editor, one: bool) -> i32 {
    // A row each for the strip's top, the form's top and the status line.
    let most = (ed.sh - 3 - MIN_FORM_H).max(4);
    if one {
        let (_, nh) = pane_size(ed.settings.tier);
        ((ed.sh - 3) / 2).clamp(4, nh).min(most)
    } else {
        preview::PANE_SIZES.iter().map(|s| s.2).max().unwrap_or(34).min(most)
    }
}

/// The interior width of the panes right of the left column.
fn right_w(ed: &Editor) -> i32 {
    ed.sw - LEFT_W - 1
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

fn vline(cv: &mut Canvas, x: i32, y0: i32, y1: i32, fg: Rgb, bg: Rgb) {
    for y in y0..=y1 {
        cv.put(x, y, '│', fg, bg);
    }
}

/// Draw the whole editor screen: the panel ground, then every frame that
/// survived the layout.
pub fn draw(cv: &mut Canvas, ed: &Editor) {
    let bg = CHROME.panel;
    for y in 0..cv.h {
        for x in 0..cv.w {
            cv.put(x, y, ' ', bg, bg);
        }
    }
    ed.panes.draw(cv, ed);
}

/// The list of tables down the left.
struct Tables;

impl frame::Pane<Editor> for Tables {
    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        Some((LEFT_W - 1, table_rows(ed)))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
        let n = ed.doc.table_count();
        let start = window(ed.table, n, r.h as usize);
        for (line, t) in (start..n).take(r.h as usize).enumerate() {
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
            cv.text(r.x, r.y + line as i32, &pad(&text, r.w), tf, tb);
        }
    }
}

/// The rows of the current table, with the art files each row names listed
/// under it.
struct Rows;

impl frame::Pane<Editor> for Rows {
    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        // What the table list and the status line leave, less this frame's
        // own top edge.
        Some((LEFT_W - 1, (ed.sh - table_rows(ed) - 3).max(1)))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
        let entries = ed.entries(ed.table);
        let cursor = ed.row_cursor[ed.table].min(entries.len().saturating_sub(1));
        let start = window(cursor, entries.len(), r.h as usize);
        for (line, (i, e)) in entries.iter().enumerate().skip(start).take(r.h as usize).enumerate() {
            let selected = i == cursor;
            let mark = if selected && ed.pane == Pane::Rows { '>' } else { ' ' };
            let text = match e.art {
                Some(ai) if ed.kind() != super::fields::TableKind::Art => format!("{mark} + {}", ed.doc.art.get(ai).map(|a| a.file.tier.name()).unwrap_or("?")),
                _ => format!("{mark}{}", ed.doc.row_name(ed.table, e.row)),
            };
            let (tf, tb) = if selected {
                (bg, if ed.pane == Pane::Rows { CHROME.selected } else { dim })
            } else if e.art.is_some() && ed.kind() != super::fields::TableKind::Art {
                (dim, bg)
            } else {
                (fg, bg)
            };
            cv.text(r.x, r.y + line as i32, &pad(&text, r.w), tf, tb);
        }
    }
}

/// The preview strip: the four tiers side by side, or the one pane of a
/// small screen. Both draw the tiles the layout worked out; they differ in
/// the height they ask for and in which of the two frames survives.
struct Strip {
    one: bool,
}

impl frame::Pane<Editor> for Strip {
    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        Some((right_w(ed), strip_rows(ed, self.one)))
    }

    /// The strip's title is what is being previewed and in what world.
    fn title(&self, _row: &str, ed: &Editor) -> Option<String> {
        let s = &ed.settings;
        let biome = ed.assets.biomes.get(ed.fixture_biome()).map(|b| b.name.as_str()).unwrap_or("?");
        let season = SEASON_NAMES[season_blend(s.season).0];
        let tod = ed.fixture_tod();
        Some(format!(
            "{}: {}  |  {}  |  biome {biome}  {season} {:02}:{:02}  {}  {}deg",
            ed.kind().name(),
            ed.doc.row_name(ed.table, ed.row()),
            ed.caption(),
            tod.floor() as i32,
            (tod.fract() * 60.0) as i32,
            ed.glyphs_name(),
            (s.angle.to_degrees().round() as i32).rem_euclid(360),
        ))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, dim) = (CHROME.panel, CHROME.panel_dim);
        // Pane titles go on the strip's first row, so the frame's own top
        // edge keeps the full width for the header.
        for (i, (tier, tile)) in ed.layout.panes.iter().enumerate() {
            let mut label = format!(" {} ", pane_title(*tier));
            if let Some(p) = ed.preview.panes.get(i) {
                cv.blit(&p.canvas, tile.x, tile.y);
                // A pane too short for the tier asked for drops to one that
                // fits and says which, and what the asked-for tier wanted.
                if p.shown != *tier {
                    label = format!(" {}  ({} needs {} of {} rows) ", pane_title(p.shown), pane_title(*tier), p.want_rows, tile.h);
                }
            }
            let label = clip(&label, tile.w);
            cv.text(tile.x, tile.y, &label, dim, bg);
            if i + 1 < ed.layout.panes.len() {
                vline(cv, tile.x + tile.w, r.y, r.y + r.h - 1, dim, bg);
            }
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
                out.push((
                    if open {
                        if i == 0 {
                            "["
                        } else {
                            " ["
                        }
                    } else {
                        ","
                    }
                    .to_string(),
                    false,
                ));
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

/// Rows the panes under the strip ask for: what the strip and the status
/// line leave, less their own top edge.
fn under_strip(ed: &Editor) -> Option<(i32, i32)> {
    Some((right_w(ed), (ed.sh - strip_rows(ed, ed.layout.small) - 3).max(1)))
}

/// The selected row's fields.
struct Form;

impl frame::Pane<Editor> for Form {
    fn open(&self, ed: &Editor) -> bool {
        !matches!(ed.mode, Mode::Grid | Mode::Picker)
    }

    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        under_strip(ed)
    }

    fn title(&self, _row: &str, ed: &Editor) -> Option<String> {
        Some(format!("row: {}  {}", ed.doc.row_name(ed.table, ed.row()), ed.doc.path_of(ed.table, ed.row())))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
        let items = ed.form();
        if items.is_empty() {
            cv.text(r.x + 1, r.y, "no fields; Enter on an art entry opens the grid", dim, bg);
            return;
        }
        let label_w = items.iter().map(|i| i.name.chars().count()).max().unwrap_or(8).clamp(6, 22) as i32;
        let cursor = ed.field.min(items.len() - 1);
        let start = window(cursor, items.len(), r.h as usize);
        let val_w = r.w - label_w - 4;
        for (line, (i, item)) in items.iter().enumerate().skip(start).take(r.h as usize).enumerate() {
            let y = r.y + line as i32;
            let selected = i == cursor;
            let editing = selected && ed.mode == Mode::Field;
            let active = selected && ed.pane == Pane::Form;
            let (lf, lb) = if active && !editing { (bg, CHROME.selected) } else { (if item.value.is_none() { dim } else { fg }, bg) };
            let mark = if active { '>' } else { ' ' };
            cv.text(r.x, y, &pad(&format!("{mark}{}", clip(&item.name, label_w)), label_w + 2), lf, lb);
            let mut x = r.x + label_w + 3;
            let mut left = val_w;
            for (seg, hi) in field_text(ed, item, editing, val_w) {
                if left <= 0 {
                    break;
                }
                let s = clip(&seg, left);
                let (sf, sb) = if hi {
                    (bg, CHROME.selected)
                } else if active && !editing {
                    (fg, bg)
                } else if item.value.is_none() {
                    (dim, bg)
                } else {
                    (fg, bg)
                };
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
}

/// The art grid, in the form's place while grid mode is on.
struct Grid;

impl frame::Pane<Editor> for Grid {
    fn open(&self, ed: &Editor) -> bool {
        ed.mode == Mode::Grid
    }

    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        under_strip(ed)
    }

    fn title(&self, _row: &str, ed: &Editor) -> Option<String> {
        let g = ed.grid?;
        let art = ed.grid_art()?;
        let w = art.rows.first().map(|r| r.chars().count()).unwrap_or(0) as i32;
        Some(format!("art: {} {}  {}x{}  center={} base_rows={}  cursor {},{}", art.name, art.tier.name(), w, art.rows.len(), art.center, art.base_rows, g.cx, g.cy))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
        let Some(g) = ed.grid else { return };
        let Some(art) = ed.grid_art() else { return };
        let w = art.rows.first().map(|r| r.chars().count()).unwrap_or(0) as i32;
        let x0 = r.x + 2;
        let y0 = r.y + 1;
        // Column ruler with the centre marked.
        for c in 0..w.min(r.w - 3) {
            let ch = if c == art.center {
                '^'
            } else if c % 5 == 0 {
                '·'
            } else {
                ' '
            };
            cv.put(x0 + c, r.y, ch, dim, bg);
        }
        let base_from = art.rows.len().saturating_sub(art.base_rows);
        let vis_h = (r.h - 2).max(1) as usize;
        let start = window(g.cy, art.rows.len(), vis_h);
        for (line, (row, text)) in art.rows.iter().enumerate().skip(start).take(vis_h).enumerate() {
            let y = y0 + line as i32;
            let base = row >= base_from;
            cv.put(x0 - 1, y, if base { '▌' } else { ' ' }, dim, bg);
            let ground = if base { Rgb(70, 52, 40) } else { Rgb(44, 62, 48) };
            for (c, ch) in text.chars().enumerate().take((r.w - 3).max(0) as usize) {
                let here = row == g.cy && c == g.cx;
                let shown = if ch == ' ' { '·' } else { ch };
                let (cf, cb) = if here {
                    (bg, CHROME.selected)
                } else if ch == ' ' {
                    (dim, ground)
                } else {
                    (fg, ground)
                };
                cv.put(x0 + c as i32, y, shown, cf, cb);
            }
        }
    }
}

/// The glyph picker, in the form's place while picker mode is on.
struct Picker;

impl frame::Pane<Editor> for Picker {
    fn open(&self, ed: &Editor) -> bool {
        ed.mode == Mode::Picker
    }

    fn preferred(&self, ed: &Editor) -> Option<(i32, i32)> {
        under_strip(ed)
    }

    fn title(&self, _row: &str, ed: &Editor) -> Option<String> {
        let p = ed.picker.as_ref()?;
        Some(format!("glyph picker  {}/{}  U+{:04X}", p.index + 1, p.items.len(), p.items.get(p.index).map(|c| *c as u32).unwrap_or(0)))
    }

    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
        let (bg, fg, dim) = (CHROME.panel, CHROME.panel_text, CHROME.panel_dim);
        let Some(p) = &ed.picker else { return };
        let cols = (r.w.max(2) / 2) as usize;
        let rows = r.h.max(1) as usize;
        let start_row = (p.index / cols).saturating_sub(rows / 2).min((p.items.len() / cols + 1).saturating_sub(rows));
        for line in 0..rows {
            for c in 0..cols {
                let i = (start_row + line) * cols + c;
                let Some(&ch) = p.items.get(i) else { break };
                let (cf, cb) = if i == p.index { (bg, CHROME.selected) } else { (fg, bg) };
                cv.put(r.x + c as i32 * 2, r.y + line as i32, ch, cf, cb);
                cv.put(r.x + c as i32 * 2 + 1, r.y + line as i32, ' ', dim, bg);
            }
        }
    }
}

/// The status line on the last row.
struct Status;

impl frame::Pane<Editor> for Status {
    fn draw(&self, cv: &mut Canvas, r: Rect, ed: &Editor) {
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
        cv.text(r.x, r.y, &pad(&line, r.w), fg, CHROME.bar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::editor::document::Document;

    fn editor(w: i32, h: i32) -> Editor {
        let a = test_assets();
        let doc = Document::from_assets(&a).unwrap();
        Editor::new(doc, (*a).clone(), w, h)
    }

    #[test]
    fn every_content_kind_has_a_pane_and_every_row_names_one() {
        let a = test_assets();
        for kind in CONTENT_KINDS {
            assert!(pane_for(kind).is_some(), "{kind}");
        }
        assert!(pane_for("nonesuch").is_none());
        // The table and the code agree in both directions, as ui.toml and
        // the binding table do for the game.
        for f in &a.editor_frames {
            assert!(CONTENT_KINDS.contains(&f.content.as_str()), "{} names {}, which code does not supply", f.name, f.content);
        }
        for kind in CONTENT_KINDS {
            assert!(a.editor_frames.iter().any(|f| f.content == kind), "no row of editor-ui.toml names {kind}");
        }
        assert_eq!(panes(&a).len(), a.editor_frames.len());
    }

    #[test]
    fn the_panes_tile_the_floor_and_the_reference_size() {
        // The floor: one preview pane, and the four panes and the status
        // line cover every cell between them.
        let ed = editor(80, 25);
        let l = &ed.layout;
        assert!(l.small);
        assert_eq!(l.panes.len(), 1);
        assert_eq!(l.status_y, 24);
        assert_eq!((l.tables.x, l.tables.y, l.tables.w, l.tables.h), (0, 1, LEFT_W - 1, 7));
        assert_eq!((l.rows.x, l.rows.y, l.rows.w), (0, 9, LEFT_W - 1));
        assert_eq!(l.rows.y + l.rows.h, l.status_y);
        assert_eq!((l.strip.x, l.strip.y, l.strip.w, l.strip.h), (LEFT_W, 1, 59, 11), "the one pane of the floor is 59 by 11");
        assert_eq!((l.form.x, l.form.y, l.form.w, l.form.h), (LEFT_W, 13, 59, 11));
        assert!(l.form.h >= MIN_FORM_H);

        // The reference size: the four tiers side by side, none of them
        // over the right edge.
        let ed = editor(168, 71);
        let l = &ed.layout;
        assert!(!l.small);
        assert_eq!(l.panes.len(), 4);
        assert_eq!((l.panes[3].1.w, l.panes[3].1.h), (56, 34));
        assert!(l.panes[3].1.x + l.panes[3].1.w < 168);
        assert_eq!(l.tables.h, 13, "every table is listed");
        assert_eq!((l.strip.y, l.strip.w, l.strip.h), (1, 147, 34));
        assert_eq!((l.form.y, l.form.h), (36, 34));
        assert_eq!(l.rows.y + l.rows.h, l.status_y);

        // The strip needs a hundred columns and thirty-five rows of its
        // own: 120 by 50 has them, a row fewer than its height does not.
        let ed = editor(120, 50);
        assert!(!ed.layout.small);
        assert!(ed.layout.panes.iter().all(|(_, r)| r.w >= 6));
        assert!(ed.layout.panes[3].1.x + ed.layout.panes[3].1.w < 120);
        assert!(ed.layout.form.h >= MIN_FORM_H);
        assert!(editor(119, 50).layout.small, "under a hundred columns of its own the strip goes");
        assert!(editor(120, 44).layout.small, "and under the rows for a strip and a form");
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
