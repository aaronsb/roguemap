//! The asset editor (ADR-003), headless: the document model, field
//! descriptors, key tables, fixtures and previews, and the screen layout.
//! `Editor` is the state machine: it consumes `Action`s and draws into a
//! `Canvas`; `src/bin/roguemap-edit.rs` is the terminal loop around it.

pub mod document;
pub mod fields;
pub mod fixture;
pub mod keys;
pub mod preview;
pub mod ui;

use std::rc::Rc;

use crate::assets::schema::TIERS;
use crate::assets::{ArtFile, Assets, Tier};
use crate::canvas::Canvas;
use crate::tileset::Tileset;
use document::{Document, SaveError};
use fields::{Field, Kind, TableKind};
use fixture::{Fixture, PreviewSettings, Subject, PATTERNS};
use keys::{Action, Mode};
use preview::Preview;
use ui::Layout;

/// Which part of the screen the cursor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Tables,
    Rows,
    Form,
}

/// One entry of the rows list: a row, or an art file listed under the row
/// that references it (or, in the art table, the art file itself).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowEntry {
    pub row: usize,
    pub art: Option<usize>,
}

/// One line of the row form.
#[derive(Clone, Debug)]
pub struct FormItem {
    pub name: String,
    pub kind: Kind,
    pub required: bool,
    pub value: Option<toml::Value>,
}

impl FormItem {
    pub fn shown(&self) -> String {
        self.value.as_ref().map(fields::show).unwrap_or_default()
    }
}

/// The live editor for one field.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldEdit {
    Text { buf: Vec<char>, cursor: usize },
    Choice { options: Vec<String>, index: usize },
    /// Three channels, or twelve for a seasonal colour.
    Color { channels: Vec<u8>, index: usize },
    Checklist { options: Vec<String>, on: Vec<bool>, index: usize },
}

/// Cursor over an art file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridEdit {
    pub art: usize,
    pub cx: usize,
    pub cy: usize,
}

/// The glyph picker over the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct PickerState {
    pub items: Vec<char>,
    pub index: usize,
}

/// What a `y` would confirm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pending {
    DeleteRow,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// The status line's message.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub text: String,
    pub level: Level,
}

/// Header fields of an art file, edited through the form.
pub const ART_FIELDS: [Field; 5] = [
    Field { name: "name", kind: Kind::Str, required: true },
    Field { name: "tier", kind: Kind::Enum(&["tiny", "small", "medium", "large"]), required: true },
    Field { name: "center", kind: Kind::U8 { max: 255 }, required: true },
    Field { name: "base_rows", kind: Kind::U8 { max: 255 }, required: true },
    Field { name: "min_zoom", kind: Kind::U8 { max: 3 }, required: true },
];

pub struct Editor {
    pub doc: Document,
    /// The last set that validated, which the previews draw.
    pub assets: Rc<Assets>,
    pub tilesets: Vec<Tileset>,
    pub settings: PreviewSettings,
    pub preview: Preview,
    pub mode: Mode,
    pub pane: Pane,
    pub table: usize,
    /// Cursor into each table's row entries.
    pub row_cursor: Vec<usize>,
    pub field: usize,
    pub field_edit: Option<FieldEdit>,
    pub grid: Option<GridEdit>,
    pub picker: Option<PickerState>,
    pub status: Status,
    pub pending: Option<Pending>,
    pub quit: bool,
    pub sw: i32,
    pub sh: i32,
    pub layout: Layout,
    fixture: Option<Fixture>,
    preview_stale: bool,
    /// What the fixture was built for, to skip rebuilding.
    fixture_key: Option<(Subject, PreviewSettings)>,
}

impl Editor {
    pub fn new(doc: Document, assets: Assets, sw: i32, sh: i32) -> Editor {
        let assets = Rc::new(assets);
        let tilesets = Tileset::all(&assets);
        let settings = PreviewSettings { biome: assets.biomes.iter().position(|b| b.koppen == "Cf").unwrap_or(0), ..PreviewSettings::default() };
        let n = doc.table_count();
        let mut ed = Editor {
            doc,
            assets,
            tilesets,
            settings,
            preview: Preview::default(),
            mode: Mode::Normal,
            pane: Pane::Rows,
            table: 0,
            row_cursor: vec![0; n],
            field: 0,
            field_edit: None,
            grid: None,
            picker: None,
            status: Status { text: String::new(), level: Level::Info },
            pending: None,
            quit: false,
            sw,
            sh,
            layout: Layout::default(),
            fixture: None,
            preview_stale: true,
            fixture_key: None,
        };
        ed.resize(sw, sh);
        ed
    }

    /// Open a directory of assets.
    pub fn open(dir: &std::path::Path, sw: i32, sh: i32) -> Result<Editor, crate::assets::AssetError> {
        let assets = Assets::from_dir(dir)?;
        let doc = Document::from_dir(dir)?;
        Ok(Editor::new(doc, assets, sw, sh))
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        self.sw = w;
        self.sh = h;
        self.layout = ui::layout(w, h, self.doc.table_count(), self.settings.tier);
        let sizes: Vec<(Tier, i32, i32)> = self.layout.panes.iter().map(|(t, r)| (*t, r.w, r.h)).collect();
        self.preview.resize(&sizes);
        self.preview_stale = true;
    }

    // Selection

    pub fn kind(&self) -> TableKind {
        self.doc.kind(self.table)
    }

    /// The rows list of a table: every row, with the art files it names
    /// listed under it.
    pub fn entries(&self, t: usize) -> Vec<RowEntry> {
        let kind = self.doc.kind(t);
        let mut out = Vec::new();
        if kind == TableKind::Art {
            return (0..self.doc.art.len()).map(|i| RowEntry { row: i, art: Some(i) }).collect();
        }
        let art_fields: Vec<&'static str> = fields::fields(kind).iter().filter(|f| matches!(f.kind, Kind::Ref { table: TableKind::Art, .. })).map(|f| f.name).collect();
        for i in 0..self.doc.row_count(t) {
            out.push(RowEntry { row: i, art: None });
            for f in &art_fields {
                if let Some(name) = self.doc.get(t, i, f).and_then(|v| v.as_str()) {
                    for ai in self.doc.art_named(name) {
                        out.push(RowEntry { row: i, art: Some(ai) });
                    }
                }
            }
        }
        out
    }

    pub fn entry(&self) -> RowEntry {
        let e = self.entries(self.table);
        e.get(self.row_cursor[self.table].min(e.len().saturating_sub(1))).copied().unwrap_or(RowEntry { row: 0, art: None })
    }

    pub fn row(&self) -> usize {
        self.entry().row
    }

    /// What the previews show.
    pub fn subject(&self) -> Subject {
        let kind = self.kind();
        let row = if kind == TableKind::Art {
            let name = self.doc.art.get(self.row()).map(|a| a.file.name.as_str()).unwrap_or("");
            self.assets.art.names().iter().position(|n| *n == name).unwrap_or(usize::MAX)
        } else {
            self.row()
        };
        Subject { kind, row }
    }

    /// The form for the selected row: the descriptor fields in order, then
    /// any other keys the row carries.
    pub fn form(&self) -> Vec<FormItem> {
        let kind = self.kind();
        if kind == TableKind::Art {
            let Some(a) = self.doc.art.get(self.row()) else { return Vec::new() };
            let f = &a.file;
            let vals = [
                toml::Value::String(f.name.clone()),
                toml::Value::String(f.tier.name().to_string()),
                toml::Value::Integer(f.center as i64),
                toml::Value::Integer(f.base_rows as i64),
                toml::Value::Integer(f.min_zoom as i64),
            ];
            return ART_FIELDS.iter().zip(vals).map(|(d, v)| FormItem { name: d.name.to_string(), kind: d.kind, required: d.required, value: Some(v) }).collect();
        }
        let Some(row) = self.doc.row(self.table, self.row()) else { return Vec::new() };
        let mut items: Vec<FormItem> = fields::fields(kind).iter().map(|d| FormItem { name: d.name.to_string(), kind: d.kind, required: d.required, value: document::get_path(row, d.name).cloned() }).collect();
        let mut leaves = Vec::new();
        document::leaf_paths(row, "", &mut leaves);
        for path in leaves {
            if !items.iter().any(|i| i.name == path) {
                items.push(FormItem { name: path.clone(), kind: Kind::Any, required: false, value: document::get_path(row, &path).cloned() });
            }
        }
        items
    }

    fn current_item(&self) -> Option<FormItem> {
        self.form().get(self.field).cloned()
    }

    fn set_status(&mut self, level: Level, text: impl Into<String>) {
        self.status = Status { text: text.into(), level };
    }

    fn info(&mut self, text: impl Into<String>) {
        self.set_status(Level::Info, text);
    }

    fn error(&mut self, text: impl Into<String>) {
        self.set_status(Level::Error, text);
    }

    /// Rebuild the preview assets from the document; a set that no longer
    /// validates keeps the last good one and says why.
    fn refresh_assets(&mut self) {
        match self.doc.validate() {
            Ok(a) => {
                self.assets = Rc::new(a);
                self.tilesets = Tileset::all(&self.assets);
            }
            Err(e) => self.set_status(Level::Warn, format!("preview kept: {}", e.msg)),
        }
        self.fixture_key = None;
        self.preview_stale = true;
    }

    /// Jump the cursor to a table and row.
    pub fn jump(&mut self, table: usize, row: usize) {
        self.table = table.min(self.doc.table_count() - 1);
        let entries = self.entries(self.table);
        self.row_cursor[self.table] = entries.iter().position(|e| e.row == row && e.art.is_none()).or_else(|| entries.iter().position(|e| e.row == row)).unwrap_or(0);
        self.pane = Pane::Rows;
        self.field = 0;
        self.preview_stale = true;
    }

    fn clamp_cursors(&mut self) {
        let n = self.entries(self.table).len();
        let c = &mut self.row_cursor[self.table];
        *c = (*c).min(n.saturating_sub(1));
        let nf = self.form().len();
        self.field = self.field.min(nf.saturating_sub(1));
    }

    // Actions

    /// Consume one action in the current mode.
    pub fn apply(&mut self, a: Action) {
        // A pending confirmation is answered by `y` and cancelled by
        // anything else.
        if let Some(p) = self.pending.take() {
            match (p, a) {
                (Pending::DeleteRow, Action::Yes) => self.delete_row(),
                (Pending::Quit, Action::Yes | Action::Quit) => self.quit = true,
                _ => self.info("cancelled"),
            }
            return;
        }
        match self.mode {
            Mode::Normal => self.normal(a),
            Mode::Field => self.field_action(a),
            Mode::Grid => self.grid_action(a),
            Mode::Picker => self.picker_action(a),
        }
        self.clamp_cursors();
    }

    fn normal(&mut self, a: Action) {
        match a {
            Action::Quit => {
                if self.doc.is_dirty() {
                    self.pending = Some(Pending::Quit);
                    self.set_status(Level::Warn, "unsaved changes: y or q again quits, anything else stays");
                } else {
                    self.quit = true;
                }
            }
            Action::Cancel => self.info(""),
            Action::NextPane => self.cycle_pane(1),
            Action::PrevPane => self.cycle_pane(-1),
            Action::Move(dx, dy) => self.move_cursor(dx, dy),
            Action::Page(d) => self.move_cursor(0, d * 10),
            Action::Home => self.move_cursor(0, -100_000),
            Action::End => self.move_cursor(0, 100_000),
            Action::Enter => self.enter(),
            Action::AddRow => match self.doc.add_row(self.table, self.row()) {
                Ok(i) => {
                    let name = self.doc.row_name(self.table, i);
                    self.jump(self.table, i);
                    self.refresh_assets();
                    self.info(format!("added {name}"));
                }
                Err(e) => self.error(e),
            },
            Action::DeleteRow => {
                if !self.kind().resizable() {
                    self.error(format!("{} has a fixed set of rows", self.kind().name()));
                } else {
                    self.pending = Some(Pending::DeleteRow);
                    self.set_status(Level::Warn, format!("delete {}? y to confirm", self.doc.row_name(self.table, self.row())));
                }
            }
            Action::ClearField => {
                if self.pane == Pane::Form {
                    if let Some(item) = self.current_item() {
                        match self.doc.clear_field(self.table, self.row(), &item.name) {
                            Ok(()) => {
                                self.refresh_assets();
                                self.info(format!("{} cleared", item.name));
                            }
                            Err(e) => self.error(e),
                        }
                    }
                }
            }
            Action::Undo => {
                if self.doc.undo() {
                    self.refresh_assets();
                    self.info("undone");
                } else {
                    self.info("nothing to undo");
                }
            }
            Action::Redo => {
                if self.doc.redo() {
                    self.refresh_assets();
                    self.info("redone");
                } else {
                    self.info("nothing to redo");
                }
            }
            Action::Save => self.save(Some(self.table)),
            Action::SaveAll => self.save(None),
            Action::Biome(d) => {
                let n = self.assets.biomes.len() as i32;
                self.settings.biome = (self.settings.biome as i32 + d).rem_euclid(n) as usize;
                self.preview_stale = true;
            }
            Action::Season(q) => {
                self.settings.season = (self.settings.season + q).rem_euclid(4.0);
                self.preview_stale = true;
            }
            Action::Hour(h) => {
                self.settings.tod = (self.settings.tod + h).rem_euclid(24.0);
                self.settings.tod_auto = false;
                self.preview_stale = true;
            }
            Action::Glyphs => {
                self.settings.glyphs = (self.settings.glyphs + 1) % self.tilesets.len().max(1);
                self.preview_stale = true;
            }
            Action::RotateQuarter(q) => {
                self.settings.angle = (self.settings.angle + q as f32 * std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
                self.preview_stale = true;
            }
            Action::Weather => {
                self.settings.weather = match self.settings.weather {
                    None => Some(0),
                    Some(i) if i + 1 < crate::world::WEATHER_PRESETS.len() => Some(i + 1),
                    Some(_) => None,
                };
                self.preview_stale = true;
            }
            Action::Tier => {
                let i = TIERS.iter().position(|t| *t == self.settings.tier).unwrap_or(0);
                self.settings.tier = TIERS[(i + 1) % TIERS.len()];
                self.resize(self.sw, self.sh);
            }
            Action::Pattern => {
                if self.kind() == TableKind::Species {
                    self.settings.variant = (self.settings.variant + 1) % 4;
                    self.info(format!("variant {}", self.settings.variant));
                } else {
                    self.settings.pattern = (self.settings.pattern + 1) % PATTERNS.len();
                    self.info(format!("pattern {}", PATTERNS[self.settings.pattern].0));
                }
                self.preview_stale = true;
            }
            Action::Levels(_) => self.info("levels arrive with block geometry (ADR-002)"),
            Action::Animate => {
                self.settings.animate = !self.settings.animate;
                self.info(if self.settings.animate { "animating" } else { "still" });
            }
            _ => {}
        }
    }

    fn cycle_pane(&mut self, d: i32) {
        let order = [Pane::Tables, Pane::Rows, Pane::Form];
        let i = order.iter().position(|p| *p == self.pane).unwrap_or(0) as i32;
        self.pane = order[(i + d).rem_euclid(3) as usize];
    }

    fn switch_table(&mut self, d: i32) {
        let n = self.doc.table_count() as i32;
        self.table = (self.table as i32 + d).rem_euclid(n) as usize;
        self.field = 0;
        self.preview_stale = true;
    }

    fn move_cursor(&mut self, dx: i32, dy: i32) {
        match self.pane {
            Pane::Tables => {
                if dy != 0 {
                    self.switch_table(dy.signum());
                } else if dx != 0 {
                    self.switch_table(dx.signum());
                }
            }
            Pane::Rows => {
                if dx != 0 {
                    self.switch_table(dx.signum());
                } else {
                    let n = self.entries(self.table).len() as i64;
                    let c = &mut self.row_cursor[self.table];
                    *c = (*c as i64 + dy as i64).clamp(0, (n - 1).max(0)) as usize;
                    self.preview_stale = true;
                }
            }
            Pane::Form => {
                if dx != 0 {
                    self.quick_adjust(dx);
                } else {
                    let n = self.form().len() as i64;
                    self.field = (self.field as i64 + dy as i64).clamp(0, (n - 1).max(0)) as usize;
                }
            }
        }
    }

    /// Left and right on a choice-like field cycle it without opening the
    /// editor, as the settings popover does.
    fn quick_adjust(&mut self, d: i32) {
        let Some(item) = self.current_item() else { return };
        let options = match self.choices(&item) {
            Some(o) => o,
            None => return,
        };
        let cur = item.value.as_ref().and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_bool().map(|b| b.to_string())));
        let i = cur.and_then(|c| options.iter().position(|o| *o == c)).unwrap_or(0) as i32;
        let next = options[(i + d).rem_euclid(options.len() as i32) as usize].clone();
        let value = if item.kind == Kind::Bool { toml::Value::Boolean(next == "true") } else { toml::Value::String(next) };
        self.commit_value(&item, value);
    }

    /// The options a choice field cycles through.
    fn choices(&self, item: &FormItem) -> Option<Vec<String>> {
        Some(match item.kind {
            Kind::Bool => vec!["false".to_string(), "true".to_string()],
            Kind::Enum(o) => o.iter().map(|s| s.to_string()).collect(),
            Kind::Ref { table, extra } => extra.iter().map(|s| s.to_string()).chain(self.doc.names(table)).collect(),
            _ => return None,
        })
    }

    fn enter(&mut self) {
        match self.pane {
            Pane::Tables => self.pane = Pane::Rows,
            Pane::Rows => {
                if let Some(ai) = self.entry().art {
                    self.open_grid(ai);
                } else {
                    self.pane = Pane::Form;
                }
            }
            Pane::Form => self.open_field(),
        }
    }

    fn open_field(&mut self) {
        let Some(item) = self.current_item() else { return };
        let edit = match item.kind {
            Kind::Bool | Kind::Enum(_) | Kind::Ref { .. } => {
                let options = self.choices(&item).unwrap_or_default();
                let cur = item.shown();
                let index = options.iter().position(|o| *o == cur).unwrap_or(0);
                FieldEdit::Choice { options, index }
            }
            Kind::Rgb => FieldEdit::Color { channels: item.value.as_ref().and_then(fields::rgb_of).unwrap_or([0; 3]).to_vec(), index: 0 },
            Kind::Seasonal => {
                let t = item.value.as_ref().and_then(fields::seasonal_of).unwrap_or([[0; 3]; 4]);
                FieldEdit::Color { channels: t.iter().flatten().copied().collect(), index: 0 }
            }
            Kind::EnumList(options) => {
                let current: Vec<String> = item.value.as_ref().and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
                FieldEdit::Checklist { options: options.iter().map(|s| s.to_string()).collect(), on: options.iter().map(|o| current.iter().any(|c| c == o)).collect(), index: 0 }
            }
            _ => {
                let buf: Vec<char> = item.shown().chars().collect();
                let cursor = buf.len();
                FieldEdit::Text { buf, cursor }
            }
        };
        self.field_edit = Some(edit);
        self.mode = Mode::Field;
        self.info("");
    }

    fn field_action(&mut self, a: Action) {
        let Some(edit) = self.field_edit.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        match (edit, a) {
            (_, Action::Cancel) => {
                self.field_edit = None;
                self.mode = Mode::Normal;
                self.info("cancelled");
            }
            (_, Action::Enter) => self.commit_field(),
            (FieldEdit::Text { buf, cursor }, a) => match a {
                Action::Move(dx, _) => *cursor = (*cursor as i64 + dx as i64).clamp(0, buf.len() as i64) as usize,
                Action::Home => *cursor = 0,
                Action::End => *cursor = buf.len(),
                Action::Backspace => {
                    if *cursor > 0 {
                        *cursor -= 1;
                        buf.remove(*cursor);
                    }
                }
                Action::Delete => {
                    if *cursor < buf.len() {
                        buf.remove(*cursor);
                    }
                }
                Action::Insert(c) => {
                    buf.insert(*cursor, c);
                    *cursor += 1;
                }
                _ => {}
            },
            (FieldEdit::Choice { options, index }, Action::Move(dx, dy)) => {
                let d = if dx != 0 { dx } else { dy };
                *index = (*index as i32 + d).rem_euclid(options.len().max(1) as i32) as usize;
            }
            (FieldEdit::Color { channels, index }, a) => match a {
                Action::Move(dx, 0) => *index = (*index as i32 + dx).rem_euclid(channels.len() as i32) as usize,
                Action::Move(_, dy) => channels[*index] = (channels[*index] as i32 - dy).clamp(0, 255) as u8,
                Action::Page(d) => channels[*index] = (channels[*index] as i32 - d * 16).clamp(0, 255) as u8,
                Action::Home => *index = 0,
                Action::End => *index = channels.len() - 1,
                _ => {}
            },
            (FieldEdit::Checklist { options, on, index }, a) => match a {
                Action::Move(dx, dy) => {
                    let d = if dy != 0 { dy } else { dx };
                    *index = (*index as i32 + d).rem_euclid(options.len() as i32) as usize;
                }
                Action::Toggle | Action::Insert(' ') => on[*index] = !on[*index],
                _ => {}
            },
            _ => {}
        }
    }

    /// Parse the field editor's state into a value and set it on the row.
    fn commit_field(&mut self) {
        let Some(item) = self.current_item() else { return };
        let Some(edit) = self.field_edit.clone() else { return };
        let value = match edit {
            FieldEdit::Text { buf, .. } => match fields::parse(item.kind, &buf.iter().collect::<String>()) {
                Ok(v) => v,
                Err(e) => {
                    self.error(e);
                    return;
                }
            },
            FieldEdit::Choice { options, index } => {
                let s = options.get(index).cloned().unwrap_or_default();
                if item.kind == Kind::Bool {
                    toml::Value::Boolean(s == "true")
                } else {
                    toml::Value::String(s)
                }
            }
            FieldEdit::Color { channels, .. } => {
                if channels.len() == 3 {
                    fields::rgb_value([channels[0], channels[1], channels[2]])
                } else {
                    let mut t = [[0u8; 3]; 4];
                    for (i, c) in channels.iter().enumerate() {
                        t[i / 3][i % 3] = *c;
                    }
                    fields::seasonal_value(t)
                }
            }
            FieldEdit::Checklist { options, on, .. } => toml::Value::Array(options.iter().zip(on).filter(|(_, on)| *on).map(|(o, _)| toml::Value::String(o.clone())).collect()),
        };
        if self.commit_value(&item, value) {
            self.field_edit = None;
            self.mode = Mode::Normal;
        }
    }

    /// Set a value on the selected row (or art header), reporting the
    /// outcome; returns whether it was accepted.
    fn commit_value(&mut self, item: &FormItem, value: toml::Value) -> bool {
        let result = if self.kind() == TableKind::Art { self.set_art_header(&item.name, &value) } else { self.doc.set_field(self.table, self.row(), &item.name, value.clone()) };
        match result {
            Ok(()) => {
                self.refresh_assets();
                let mut note = format!("{} = {}", item.name, fields::show(&value));
                if let (Kind::Ref { table, extra }, Some(s)) = (item.kind, value.as_str()) {
                    if !extra.contains(&s) && !self.doc.names(table).iter().any(|n| n == s) {
                        note = format!("{note}  (no {} named {s:?}; saving will refuse)", table.name());
                        self.set_status(Level::Warn, note);
                        return true;
                    }
                }
                if self.status.level != Level::Warn {
                    self.info(note);
                }
                true
            }
            Err(e) => {
                self.error(format!("{}: {e}", item.name));
                false
            }
        }
    }

    fn set_art_header(&mut self, name: &str, value: &toml::Value) -> Result<(), String> {
        let ai = self.row();
        let mut f = self.doc.art.get(ai).ok_or("no art selected")?.file.clone();
        let int = || value.as_integer().ok_or_else(|| "not a number".to_string());
        match name {
            "name" => f.name = value.as_str().ok_or("not a string")?.trim().to_string(),
            "tier" => f.tier = value.as_str().and_then(Tier::parse).ok_or("unknown tier")?,
            "center" => f.center = int()? as i32,
            "base_rows" => f.base_rows = int()? as usize,
            "min_zoom" => f.min_zoom = int()? as usize,
            _ => return Err(format!("art has no field {name}")),
        }
        let checked = ArtFile::parse(&f.to_text()).map_err(|(_, m)| m)?;
        if checked.name.is_empty() {
            return Err("name is empty".to_string());
        }
        self.doc.set_art(ai, checked);
        Ok(())
    }

    // Grid

    fn open_grid(&mut self, ai: usize) {
        if ai >= self.doc.art.len() {
            return;
        }
        self.grid = Some(GridEdit { art: ai, cx: 0, cy: 0 });
        self.mode = Mode::Grid;
        self.info(format!("editing {}", self.doc.art[ai].path));
    }

    /// The art file under the grid cursor.
    pub fn grid_art(&self) -> Option<&ArtFile> {
        self.grid.and_then(|g| self.doc.art.get(g.art)).map(|a| &a.file)
    }

    fn grid_action(&mut self, a: Action) {
        let Some(mut g) = self.grid else {
            self.mode = Mode::Normal;
            return;
        };
        let Some(file) = self.grid_art().cloned() else {
            self.mode = Mode::Normal;
            return;
        };
        let mut rows: Vec<Vec<char>> = file.rows.iter().map(|r| r.chars().collect()).collect();
        let (w, h) = (rows[0].len(), rows.len());
        let mut f = file.clone();
        let mut changed = true;
        match a {
            Action::Cancel => {
                self.grid = None;
                self.mode = Mode::Normal;
                self.info("");
                return;
            }
            Action::Undo => {
                if self.doc.undo() {
                    self.refresh_assets();
                    self.info("undone");
                }
                return;
            }
            Action::Move(dx, dy) => {
                g.cx = (g.cx as i32 + dx).clamp(0, w as i32 - 1) as usize;
                g.cy = (g.cy as i32 + dy).clamp(0, h as i32 - 1) as usize;
                changed = false;
            }
            Action::Home => {
                g.cx = 0;
                changed = false;
            }
            Action::End => {
                g.cx = w - 1;
                changed = false;
            }
            Action::Put(c) => {
                if c == '\t' {
                    self.error("tabs are not allowed in art");
                    return;
                }
                rows[g.cy][g.cx] = c;
                if g.cx + 1 < w {
                    g.cx += 1;
                }
            }
            Action::ClearCell => rows[g.cy][g.cx] = ' ',
            Action::InsertGridRow => rows.insert(g.cy, vec![' '; w]),
            Action::DeleteGridRow => {
                if h <= 1 {
                    self.error("the last row cannot be deleted");
                    return;
                }
                rows.remove(g.cy);
                g.cy = g.cy.min(h - 2);
                f.base_rows = f.base_rows.min(h - 1);
            }
            Action::Widen => rows.iter_mut().for_each(|r| r.push(' ')),
            Action::Narrow => {
                if w <= 1 {
                    self.error("the last column cannot be removed");
                    return;
                }
                rows.iter_mut().for_each(|r| {
                    r.pop();
                });
                g.cx = g.cx.min(w - 2);
                f.center = f.center.min(w as i32 - 2);
            }
            Action::SetCenter => {
                f.center = g.cx as i32;
                self.info(format!("center = {}", f.center));
            }
            Action::SetBaseRows => {
                f.base_rows = h - g.cy;
                self.info(format!("base_rows = {}", f.base_rows));
            }
            Action::OpenPicker => {
                self.picker = Some(PickerState { items: self.picker_items(), index: 0 });
                self.mode = Mode::Picker;
                return;
            }
            _ => changed = false,
        }
        self.grid = Some(g);
        if changed {
            f.rows = rows.iter().map(|r| r.iter().collect()).collect();
            if f.rows.iter().all(|r| r.trim().is_empty()) {
                self.error("art needs at least one glyph");
                return;
            }
            let text = f.to_text();
            match ArtFile::parse(&text) {
                Ok(mut checked) => {
                    // Keep trailing blank rows while editing; the loader drops them.
                    checked.rows = f.rows.clone();
                    self.doc.set_art(g.art, checked);
                    self.refresh_assets();
                }
                Err((_, m)) => self.error(m),
            }
        }
    }

    /// Glyphs the picker offers: the current tileset's roles and art
    /// vocabulary, then box and block drawing and the Symbols for Legacy
    /// Computing block.
    pub fn picker_items(&self) -> Vec<char> {
        let mut items: Vec<char> = Vec::new();
        if let Some(ts) = self.assets.tilesets.get(self.settings.glyphs.min(self.assets.tilesets.len().saturating_sub(1))) {
            let r = &ts.roles;
            let a = &ts.art;
            let groups: Vec<Vec<char>> = vec![
                r.cover.grass.to_vec(),
                r.cover.dry.to_vec(),
                r.cover.moss.to_vec(),
                r.stubble.to_vec(),
                r.cattail.to_vec(),
                r.water.to_vec(),
                r.texture.sand.to_vec(),
                r.texture.dirt.to_vec(),
                r.texture.rock.to_vec(),
                r.texture.snow.to_vec(),
                r.wall.to_vec(),
                r.star.to_vec(),
                r.snowflake.to_vec(),
                r.flame.to_vec(),
                vec![r.rain, a.pine_l, a.pine_r, a.cactus, a.roof_fill, a.door, a.window],
                a.pine_fill.to_vec(),
                a.round_top.to_vec(),
                a.round_mid.to_vec(),
                a.round_bot.to_vec(),
                a.trunk.iter().flat_map(|s| s.chars()).collect(),
            ];
            items.extend(groups.into_iter().flatten().filter(|c| *c != ' '));
        }
        items.extend((0x2500..=0x259F).filter_map(char::from_u32));
        items.extend((0x1FB00..=0x1FBFF).filter_map(char::from_u32));
        let mut seen = std::collections::HashSet::new();
        items.retain(|c| seen.insert(*c));
        items
    }

    fn picker_action(&mut self, a: Action) {
        let Some(p) = self.picker.as_mut() else {
            self.mode = Mode::Grid;
            return;
        };
        let cols = self.layout.form.w.max(2) as usize / 2;
        let n = p.items.len();
        match a {
            Action::Cancel => {
                self.picker = None;
                self.mode = Mode::Grid;
            }
            Action::Move(dx, dy) => p.index = (p.index as i64 + dx as i64 + dy as i64 * cols as i64).clamp(0, n as i64 - 1) as usize,
            Action::Page(d) => {
                let page = cols * self.layout.form.h.max(1) as usize;
                p.index = (p.index as i64 + d as i64 * page as i64).clamp(0, n as i64 - 1) as usize;
            }
            Action::Enter => {
                let c = p.items[p.index];
                self.picker = None;
                self.mode = Mode::Grid;
                self.grid_action(Action::Put(c));
            }
            _ => {}
        }
    }

    // Save

    fn save(&mut self, only: Option<usize>) {
        match self.doc.save(only) {
            Ok(written) if written.is_empty() => self.info("nothing to save"),
            Ok(written) => {
                let note = format!("wrote {}", written.join(", "));
                self.reload_from_disk(note);
            }
            Err(SaveError { msg, at }) => {
                if let Some((t, r)) = at {
                    self.jump(t, r);
                }
                self.error(format!("not saved: {msg}"));
            }
        }
    }

    /// After a write, load the directory as the game would so the preview
    /// shows exactly that.
    fn reload_from_disk(&mut self, note: String) {
        let Some(dir) = self.doc.dir.clone() else { return };
        match Assets::from_dir(&dir) {
            Ok(a) => {
                self.assets = Rc::new(a);
                self.tilesets = Tileset::all(&self.assets);
                self.fixture_key = None;
                self.preview_stale = true;
                self.info(note);
            }
            Err(e) => self.set_status(Level::Warn, format!("{note}; the directory does not load yet: {e}")),
        }
    }

    fn delete_row(&mut self) {
        let name = self.doc.row_name(self.table, self.row());
        match self.doc.delete_row(self.table, self.row()) {
            Ok(()) => {
                self.refresh_assets();
                self.info(format!("deleted {name}"));
            }
            Err(e) => self.error(e),
        }
        self.clamp_cursors();
    }

    // Drawing

    /// Render the previews if anything changed, then the screen.
    pub fn draw(&mut self, cv: &mut Canvas, t: f32) {
        let subject = self.subject();
        let key = (subject, self.settings.clone());
        if self.fixture_key.as_ref() != Some(&key) {
            self.fixture = Some(fixture::build(&self.assets, subject, &self.settings));
            self.fixture_key = Some(key);
            self.preview_stale = true;
        }
        if self.preview_stale || self.settings.animate {
            if let Some(fx) = &self.fixture {
                let g = fx.glyphs.unwrap_or(self.settings.glyphs).min(self.tilesets.len().saturating_sub(1));
                let t = if self.settings.animate { t } else { 3.0 };
                self.preview.render(fx, &self.tilesets[g], self.settings.angle, t);
            }
            self.preview_stale = false;
        }
        ui::draw(cv, self);
    }

    /// The caption of the current fixture.
    pub fn caption(&self) -> String {
        self.fixture.as_ref().map(|f| f.caption.clone()).unwrap_or_default()
    }

    /// The biome the previews are drawn in.
    pub fn fixture_biome(&self) -> usize {
        self.fixture.as_ref().map(|f| f.biome).unwrap_or(self.settings.biome)
    }

    /// The tileset name the previews use.
    pub fn glyphs_name(&self) -> String {
        let g = self.fixture.as_ref().and_then(|f| f.glyphs).unwrap_or(self.settings.glyphs);
        self.tilesets.get(g).map(|t| t.name.clone()).unwrap_or_default()
    }
}

/// Index of a name, or a number, in a list of names.
fn index_of(names: &[String], key: &str) -> Option<usize> {
    key.parse::<usize>().ok().filter(|i| *i < names.len()).or_else(|| names.iter().position(|n| n == key))
}

/// Render one editor screen headless (`roguemap-edit --snap W H OUT
/// key=value...`). Keys: `table` (name), `row` (name or index), `biome`
/// (name or index), `season` (0..4), `tod` (hour), `glyphs` (tileset
/// name), `tier` (tiny|small|medium|large, for small screens), `pattern`
/// (0..4), `deg` (camera angle), `pane` (tables|rows|form), `grid` (1 to
/// open the row's first art file in the grid).
pub fn snapshot<S: AsRef<str>>(assets: Assets, w: u16, h: u16, args: &[S]) -> Result<Canvas, String> {
    let a = crate::snapshot::SnapArgs::parse(args);
    let doc = Document::from_assets(&assets)?;
    let mut ed = Editor::new(doc, assets, w as i32, h as i32);
    if let Some(name) = a.text("table") {
        let t = fields::TABLE_KINDS.iter().position(|k| k.name() == name).ok_or_else(|| format!("no table named {name:?}"))?;
        ed.jump(t, 0);
    }
    if let Some(row) = a.text("row") {
        let names: Vec<String> = (0..ed.doc.row_count(ed.table)).map(|i| ed.doc.row_name(ed.table, i)).collect();
        let i = index_of(&names, row).ok_or_else(|| format!("no row {row:?} in {}", ed.kind().name()))?;
        ed.jump(ed.table, i);
    }
    if let Some(b) = a.text("biome") {
        let names: Vec<String> = ed.assets.biomes.iter().map(|b| b.name.clone()).collect();
        ed.settings.biome = index_of(&names, b).ok_or_else(|| format!("no biome {b:?}"))?;
    }
    if a.text("season").is_some() {
        ed.settings.season = a.num("season", 1.0).rem_euclid(4.0);
    }
    if a.text("tod").is_some() {
        ed.settings.tod = a.num("tod", 12.0).rem_euclid(24.0);
        ed.settings.tod_auto = false;
    }
    if let Some(g) = a.text("glyphs") {
        ed.settings.glyphs = ed.tilesets.iter().position(|t| t.name == g).ok_or_else(|| format!("no tileset {g:?}"))?;
    }
    if let Some(t) = a.text("tier") {
        ed.settings.tier = Tier::parse(t).ok_or_else(|| format!("no tier {t:?}"))?;
        ed.resize(w as i32, h as i32);
    }
    if a.text("pattern").is_some() {
        ed.settings.pattern = a.num("pattern", 0.0) as usize % PATTERNS.len();
    }
    if a.text("deg").is_some() {
        ed.settings.angle = a.num("deg", 45.0).to_radians();
    }
    if let Some(p) = a.text("pane") {
        ed.pane = match p {
            "tables" => Pane::Tables,
            "form" => Pane::Form,
            _ => Pane::Rows,
        };
    }
    if a.flag("grid") {
        // Open the grid on the row's first art file.
        let row = ed.row();
        let entries = ed.entries(ed.table);
        let at = entries.iter().position(|e| e.row == row && e.art.is_some()).ok_or_else(|| format!("{} has no art to edit", ed.doc.row_name(ed.table, row)))?;
        ed.row_cursor[ed.table] = at;
        ed.pane = Pane::Rows;
        ed.apply(Action::Enter);
    }
    let mut cv = Canvas::new(w, h);
    ed.draw(&mut cv, 3.0);
    Ok(cv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crossterm::event::KeyCode;

    fn editor() -> Editor {
        let a = test_assets();
        let doc = Document::from_assets(&a).unwrap();
        Editor::new(doc, (*a).clone(), 120, 40)
    }

    fn keys(ed: &mut Editor, ks: &[KeyCode]) {
        for k in ks {
            let a = keys::lookup(ed.mode, *k).unwrap_or_else(|| panic!("{k:?} unbound in {}", ed.mode.name()));
            ed.apply(a);
        }
    }

    fn type_text(ed: &mut Editor, s: &str) {
        for c in s.chars() {
            ed.apply(Action::Insert(c));
        }
    }

    #[test]
    fn navigation_moves_between_tables_rows_and_fields() {
        let mut ed = editor();
        assert_eq!(ed.pane, Pane::Rows);
        assert_eq!(ed.kind(), TableKind::Biomes);
        ed.apply(Action::Move(1, 0));
        assert_eq!(ed.kind(), TableKind::Species);
        ed.apply(Action::Move(0, 2));
        assert_eq!(ed.doc.row_name(ed.table, ed.row()), "pine");
        ed.apply(Action::PrevPane);
        assert_eq!(ed.pane, Pane::Tables);
        ed.apply(Action::Move(0, -1));
        assert_eq!(ed.kind(), TableKind::Biomes);
        ed.apply(Action::Enter);
        assert_eq!(ed.pane, Pane::Rows);
        ed.apply(Action::Enter);
        assert_eq!(ed.pane, Pane::Form);
        ed.apply(Action::Move(0, 3));
        assert_eq!(ed.form()[ed.field].name, "ground");
        ed.apply(Action::End);
        assert_eq!(ed.field, ed.form().len() - 1);
        ed.apply(Action::NextPane);
        assert_eq!(ed.pane, Pane::Tables);
        // Props list their art under each row.
        ed.jump(ed.doc.table_of(TableKind::Props), 0);
        let entries = ed.entries(ed.table);
        assert_eq!(entries[0], RowEntry { row: 0, art: None });
        assert!(entries[1].art.is_some() && entries[1].row == 0);
        assert_eq!(ed.subject().kind, TableKind::Props);
    }

    #[test]
    fn field_edits_validate_on_commit() {
        let mut ed = editor();
        let species = ed.doc.table_of(TableKind::Species);
        ed.jump(species, 0);
        ed.pane = Pane::Form;
        // Text field with a bad value stays open with an error.
        let radius = ed.form().iter().position(|f| f.name == "radius").unwrap();
        ed.field = radius;
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Field);
        type_text(&mut ed, "-3");
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Field);
        assert_eq!(ed.status.level, Level::Error);
        keys(&mut ed, &[KeyCode::Backspace, KeyCode::Backspace]);
        type_text(&mut ed, "2.5");
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Normal);
        assert_eq!(ed.doc.get(species, 0, "radius").unwrap().as_float(), Some(2.5));
        assert_eq!(ed.assets.species[0].radius, Some(2.5));
        // Enum cycles by arrow without opening the editor.
        ed.field = ed.form().iter().position(|f| f.name == "form").unwrap();
        ed.apply(Action::Move(-1, 0));
        assert_eq!(ed.doc.get(species, 0, "form").unwrap().as_str(), Some("pine"));
        // Colour channels step and commit.
        ed.field = ed.form().iter().position(|f| f.name == "canopy").unwrap();
        ed.apply(Action::Enter);
        assert!(matches!(ed.field_edit, Some(FieldEdit::Color { ref channels, .. }) if channels.len() == 12));
        ed.apply(Action::Move(0, -1));
        ed.apply(Action::Page(-1));
        ed.apply(Action::Enter);
        assert_eq!(fields::seasonal_of(ed.doc.get(species, 0, "canopy").unwrap()).unwrap()[0][0], 72 + 17);
        // A dangling reference is accepted with a warning.
        let biomes = ed.doc.table_of(TableKind::Biomes);
        ed.jump(biomes, 0);
        ed.pane = Pane::Form;
        ed.field = ed.form().iter().position(|f| f.name == "material").unwrap();
        ed.apply(Action::Enter);
        assert!(matches!(ed.field_edit, Some(FieldEdit::Choice { .. })));
        ed.apply(Action::Move(1, 0));
        ed.apply(Action::Enter);
        assert_ne!(ed.doc.get(biomes, 0, "material").unwrap().as_str(), Some("wood"));
        // Checklist toggles.
        let props = ed.doc.table_of(TableKind::Props);
        ed.jump(props, 0);
        ed.pane = Pane::Form;
        ed.field = ed.form().iter().position(|f| f.name == "terrain").unwrap();
        ed.apply(Action::Enter);
        ed.apply(Action::Insert(' '));
        ed.apply(Action::Enter);
        assert!(ed.doc.get(props, 0, "terrain").unwrap().as_array().unwrap().iter().any(|t| t.as_str() == Some("water")));
        // Clearing an optional field and undoing.
        ed.field = ed.form().iter().position(|f| f.name == "tags").unwrap();
        ed.apply(Action::ClearField);
        assert!(ed.doc.get(props, 0, "tags").is_none());
        ed.apply(Action::Undo);
        assert!(ed.doc.get(props, 0, "tags").is_some());
        ed.apply(Action::Cancel);
    }

    #[test]
    fn rows_are_added_and_deleted_with_confirmation() {
        let mut ed = editor();
        let lights = ed.doc.table_of(TableKind::Lights);
        ed.jump(lights, 0);
        let n = ed.doc.row_count(lights);
        ed.apply(Action::AddRow);
        assert_eq!(ed.doc.row_count(lights), n + 1);
        assert_eq!(ed.doc.row_name(lights, ed.row()), "torch-copy");
        assert_eq!(ed.assets.lights.len(), n + 1);
        ed.apply(Action::DeleteRow);
        assert_eq!(ed.pending, Some(Pending::DeleteRow));
        ed.apply(Action::Cancel);
        assert_eq!(ed.doc.row_count(lights), n + 1);
        ed.apply(Action::DeleteRow);
        ed.apply(Action::Yes);
        assert_eq!(ed.doc.row_count(lights), n);
        // A referenced row is refused.
        ed.jump(lights, ed.doc.names(TableKind::Lights).iter().position(|l| l == "campfire").unwrap());
        ed.apply(Action::DeleteRow);
        ed.apply(Action::Yes);
        assert_eq!(ed.status.level, Level::Error);
        assert_eq!(ed.doc.row_count(lights), n);
        // Quit asks once while dirty.
        assert!(ed.doc.is_dirty());
        ed.apply(Action::Quit);
        assert!(!ed.quit);
        ed.apply(Action::Quit);
        assert!(ed.quit);
    }

    #[test]
    fn grid_edits_art_and_headers() {
        let mut ed = editor();
        let art = ed.doc.table_of(TableKind::Art);
        let ai = ed.doc.art_named("boulder").into_iter().find(|&i| ed.doc.art[i].file.tier == Tier::Large).unwrap();
        ed.jump(art, ai);
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Grid);
        let before = ed.grid_art().unwrap().clone();
        // Placing a glyph advances the cursor.
        keys(&mut ed, &[KeyCode::Char('#'), KeyCode::Char('@')]);
        let now = ed.grid_art().unwrap();
        assert!(now.rows[0].starts_with("#@"), "{:?}", now.rows);
        assert_eq!(now.rows[0].chars().count(), before.rows[0].chars().count());
        keys(&mut ed, &[KeyCode::Char('>'), KeyCode::Char('c'), KeyCode::Down, KeyCode::Char('B')]);
        let now = ed.grid_art().unwrap();
        assert_eq!(now.rows[0].chars().count(), before.rows[0].chars().count() + 1);
        assert_eq!(now.center, 2);
        assert_eq!(now.base_rows, 1);
        keys(&mut ed, &[KeyCode::Char('i')]);
        assert_eq!(ed.grid_art().unwrap().rows.len(), before.rows.len() + 1);
        keys(&mut ed, &[KeyCode::Char('X'), KeyCode::Char('<')]);
        assert_eq!(ed.grid_art().unwrap().rows.len(), before.rows.len());
        assert_eq!(ed.grid_art().unwrap().rows[0].chars().count(), before.rows[0].chars().count());
        assert!(ed.doc.art[ai].dirty);
        // The preview assets carry the edit.
        assert!(ed.assets.art.get("boulder", Tier::Large).unwrap().rows[0].starts_with("#@"));
        // Picker places a glyph.
        keys(&mut ed, &[KeyCode::Char('p')]);
        assert_eq!(ed.mode, Mode::Picker);
        ed.apply(Action::Move(1, 0));
        let glyph = ed.picker.as_ref().unwrap().items[1];
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Grid);
        assert!(ed.grid_art().unwrap().rows.iter().any(|r| r.contains(glyph)));
        ed.apply(Action::Cancel);
        assert_eq!(ed.mode, Mode::Normal);
        // Header fields through the form.
        ed.pane = Pane::Form;
        ed.field = 2;
        ed.apply(Action::Enter);
        keys(&mut ed, &[KeyCode::Backspace]);
        type_text(&mut ed, "99");
        ed.apply(Action::Enter);
        assert_eq!(ed.mode, Mode::Field);
        assert_eq!(ed.status.level, Level::Error);
        ed.apply(Action::Cancel);
        assert_eq!(ed.mode, Mode::Normal);
        assert!(ed.doc.art[ai].file.center < 99);
    }

    #[test]
    fn save_round_trips_to_a_directory() {
        let a = test_assets();
        let dir = std::env::temp_dir().join(format!("roguemap-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        a.export(&dir).unwrap();
        let mut ed = Editor::open(&dir, 80, 25).unwrap();
        assert!(ed.layout.small);
        let species = ed.doc.table_of(TableKind::Species);
        ed.jump(species, 1);
        ed.pane = Pane::Form;
        ed.field = ed.form().iter().position(|f| f.name == "size_class").unwrap();
        ed.apply(Action::Move(1, 0));
        let chosen = ed.doc.get(species, 1, "size_class").unwrap().as_str().unwrap().to_string();
        // A bad reference elsewhere blocks the save and jumps there.
        let biomes = ed.doc.table_of(TableKind::Biomes);
        ed.doc.set_field(biomes, 2, "material", toml::Value::String("brick".into())).unwrap();
        ed.apply(Action::SaveAll);
        assert_eq!(ed.status.level, Level::Error);
        assert_eq!((ed.table, ed.row()), (biomes, 2));
        assert!(ed.doc.is_dirty());
        ed.apply(Action::Undo);
        ed.apply(Action::SaveAll);
        assert_eq!(ed.status.level, Level::Info, "{}", ed.status.text);
        assert!(!ed.doc.is_dirty());
        let b = Assets::from_dir(&dir).unwrap();
        assert_eq!(b.species[1].size_class, toml::Value::String(chosen.clone()).try_into().unwrap());
        assert_eq!(ed.assets.species[1].size_class, b.species[1].size_class);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn snapshot_renders_both_reference_sizes() {
        let a = (*test_assets()).clone();
        let cv = snapshot(a.clone(), 80, 25, &["table=species", "row=oak"]).unwrap();
        assert_eq!((cv.w, cv.h), (80, 25));
        let top: String = cv.cells[..80].iter().map(|c| c.ch).collect();
        assert!(top.contains("species: oak"), "{top}");
        let cv = snapshot(a.clone(), 168, 71, &["table=props", "row=boulder", "biome=steppe", "season=3", "tod=22", "glyphs=ascii", "pane=form"]).unwrap();
        let top: String = cv.cells[..168].iter().map(|c| c.ch).collect();
        assert!(top.contains("props: boulder") && top.contains("showing four boulder") && top.contains("biome steppe  winter 22:00  ascii"), "{top}");
        let cv = snapshot(a.clone(), 80, 25, &["table=props", "row=boulder", "grid=1"]).unwrap();
        let rows: Vec<String> = (0..25).map(|y| cv.cells[y * 80..(y + 1) * 80].iter().map(|c| c.ch).collect()).collect();
        assert!(rows.iter().any(|r| r.contains("art: boulder")), "{rows:?}");
        assert!(snapshot(a.clone(), 80, 25, &["table=species", "row=oak", "grid=1"]).is_err());
        assert!(snapshot(a.clone(), 80, 25, &["table=nothing"]).is_err());
        assert!(snapshot(a, 80, 25, &["table=lights", "row=lantern"]).is_err());
    }

    #[test]
    fn preview_settings_follow_the_shared_keys() {
        let mut ed = editor();
        keys(&mut ed, &[KeyCode::Char(']'), KeyCode::Char('.'), KeyCode::Char('g'), KeyCode::Char('b'), KeyCode::Char('r'), KeyCode::Char('W')]);
        assert_eq!(ed.settings.season, 1.25);
        assert_eq!(ed.settings.tod, 13.0);
        assert_eq!(ed.settings.glyphs, 1);
        assert_eq!(ed.settings.weather, Some(0));
        assert!(!ed.settings.tod_auto);
        keys(&mut ed, &[KeyCode::Char('t')]);
        assert_eq!(ed.settings.tier, Tier::Tiny);
        let mut cv = Canvas::new(168, 71);
        ed.resize(168, 71);
        ed.draw(&mut cv, 0.0);
        assert!(!ed.layout.small);
        assert_eq!(ed.layout.panes.len(), 4);
        ed.resize(80, 25);
        let mut cv = Canvas::new(80, 25);
        ed.draw(&mut cv, 0.0);
        assert_eq!(ed.layout.panes.len(), 1);
        assert_eq!(ed.layout.panes[0].0, Tier::Tiny);
    }
}
