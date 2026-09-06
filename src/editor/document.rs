//! The editable set of asset files (ADR-003): every TOML table as a generic
//! `toml::Table` per row, every art file as an `ArtFile`, one dirty flag per
//! file, an undo stack of snapshots, and an atomic save that validates the
//! whole set through `Assets::from_strings` first.
//!
//! Rows are edited generically and checked against the typed row structs
//! in `assets/schema.rs` on every commit; files are written back through
//! those structs, so the output layout is the loader's own.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fields::{self, Kind, TableKind, TABLE_KINDS};
use crate::assets::schema::*;
use crate::assets::{ArtFile, AssetError, Assets, Source, TABLES};

const UNDO_CAP: usize = 100;

/// Which typed file a TOML document is, for checks and rendering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Biomes,
    Species,
    Materials,
    Props,
    Blocks,
    Creatures,
    Surfaces,
    Lights,
    Settings,
    /// The overlay frames of ADR-005. Held so the set round-trips and
    /// validates; the editor has no pane for them yet.
    Ui,
    /// `tree_styles.toml`; not an editable table either, but kept and
    /// written back so a save does not lose it.
    TreeStyles,
    Tileset,
}

impl FileKind {
    pub fn of_path(path: &str) -> Option<FileKind> {
        Some(match path {
            p if p == TABLES[0] => FileKind::Biomes,
            p if p == TABLES[1] => FileKind::Species,
            p if p == TABLES[2] => FileKind::Materials,
            p if p == TABLES[3] => FileKind::Props,
            p if p == TABLES[4] => FileKind::Blocks,
            p if p == TABLES[5] => FileKind::Creatures,
            p if p == TABLES[6] => FileKind::Surfaces,
            p if p == TABLES[7] => FileKind::Lights,
            p if p == TABLES[8] => FileKind::Settings,
            p if p == TABLES[9] => FileKind::Ui,
            p if p == TABLES[10] => FileKind::TreeStyles,
            p if p.starts_with("tilesets/") && p.ends_with(".toml") => FileKind::Tileset,
            _ => return None,
        })
    }

    /// The file rendered through its typed struct, so the layout matches
    /// what the loader's own export writes.
    pub fn render(self, root: &toml::Table) -> Result<String, String> {
        match self {
            FileKind::Biomes => render_as::<BiomesFile>(root),
            FileKind::Species => render_as::<SpeciesFile>(root),
            FileKind::Materials => render_as::<MaterialsFile>(root),
            FileKind::Props => render_as::<PropsFile>(root),
            FileKind::Blocks => render_as::<BlocksFile>(root),
            FileKind::Creatures => render_as::<CreaturesFile>(root),
            FileKind::Surfaces => render_as::<SurfacesFile>(root),
            FileKind::Lights => render_as::<LightsFile>(root),
            FileKind::Settings => render_as::<SettingsFile>(root),
            FileKind::Ui => render_as::<UiFile>(root),
            FileKind::TreeStyles => render_as::<TreeStylesFile>(root),
            FileKind::Tileset => render_as::<TilesetFile>(root),
        }
    }

    /// Check one row against its typed struct.
    pub fn check_row(self, key: &str, row: &toml::Table) -> Result<(), String> {
        match (self, key) {
            (FileKind::Biomes, _) => check_as::<BiomeRow>(row),
            (FileKind::Species, _) => check_as::<SpeciesRow>(row),
            (FileKind::Materials, _) => check_as::<MaterialRow>(row),
            (FileKind::Props, _) => check_as::<PropRow>(row),
            (FileKind::Blocks, _) => check_as::<BlockRow>(row),
            (FileKind::Creatures, _) => check_as::<CreatureRow>(row),
            (FileKind::Surfaces, "season") => check_as::<SeasonRow>(row),
            (FileKind::Surfaces, "surface") => check_as::<SurfaceRow>(row),
            (FileKind::Surfaces, _) => check_as::<DensityRow>(row),
            (FileKind::Lights, _) => check_as::<LightRow>(row),
            (FileKind::Settings, _) => check_as::<SettingRow>(row),
            (FileKind::Ui, _) => check_as::<FrameRow>(row),
            (FileKind::TreeStyles, _) => check_as::<StyleRow>(row),
            (FileKind::Tileset, _) => check_as::<TilesetFile>(row),
        }
    }
}

fn check_as<T: DeserializeOwned>(row: &toml::Table) -> Result<(), String> {
    toml::Value::Table(row.clone()).try_into::<T>().map(|_| ()).map_err(|e| e.message().to_string())
}

fn render_as<T: DeserializeOwned + Serialize>(root: &toml::Table) -> Result<String, String> {
    let typed: T = toml::Value::Table(root.clone()).try_into().map_err(|e| e.message().to_string())?;
    toml::to_string(&typed).map_err(|e| e.to_string())
}

/// One TOML file: its leading comment block, kept verbatim, and its body
/// as a generic table.
#[derive(Clone, Debug, PartialEq)]
pub struct FileDoc {
    pub path: String,
    pub kind: FileKind,
    pub head: String,
    pub root: toml::Table,
    pub dirty: bool,
}

/// One art file.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtDoc {
    pub path: String,
    pub file: ArtFile,
    pub dirty: bool,
}

/// One entry of the left pane's table list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableDef {
    pub kind: TableKind,
    /// The file holding the rows, for the row tables.
    pub file: Option<usize>,
    /// The top-level key of the row array or table.
    pub key: &'static str,
}

#[derive(Clone)]
struct Snapshot {
    files: Vec<(toml::Table, bool)>,
    art: Vec<(ArtFile, bool)>,
}

/// A save that was refused: why, and the table and row to show.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveError {
    pub msg: String,
    pub at: Option<(usize, usize)>,
}

pub struct Document {
    /// Where the files are written; `None` for an embedded or in-memory
    /// set, which can be viewed but not saved.
    pub dir: Option<PathBuf>,
    pub files: Vec<FileDoc>,
    pub art: Vec<ArtDoc>,
    pub tables: Vec<TableDef>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

/// The leading run of comment and blank lines.
fn head_of(text: &str) -> String {
    let mut n = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            n += line.len();
        } else {
            break;
        }
    }
    text[..n].to_string()
}

/// Walk a dotted path into nested tables.
pub fn get_path<'a>(t: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let mut parts = path.split('.');
    let mut v = t.get(parts.next()?)?;
    for p in parts {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

/// Set a dotted path, creating intermediate tables.
pub fn set_path(t: &mut toml::Table, path: &str, value: toml::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = t;
    for p in &parts[..parts.len() - 1] {
        let entry = cur.entry(p.to_string()).or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if !entry.is_table() {
            *entry = toml::Value::Table(toml::Table::new());
        }
        cur = entry.as_table_mut().expect("just made a table");
    }
    cur.insert(parts[parts.len() - 1].to_string(), value);
}

/// Remove a dotted path; empty intermediate tables are left in place.
pub fn remove_path(t: &mut toml::Table, path: &str) -> Option<toml::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = t;
    for p in &parts[..parts.len() - 1] {
        cur = cur.get_mut(*p)?.as_table_mut()?;
    }
    cur.remove(parts[parts.len() - 1])
}

/// Every leaf of a table as dotted paths, depth first in key order.
pub fn leaf_paths(t: &toml::Table, prefix: &str, out: &mut Vec<String>) {
    for (k, v) in t {
        let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::Table(inner) if !inner.is_empty() && k != "condition_colors" => leaf_paths(inner, &path, out),
            _ => out.push(path),
        }
    }
}

impl Document {
    /// The files of a loaded set, parsed generically. The set has already
    /// been validated, so the parse cannot fail on a loader-built set.
    pub fn from_assets(assets: &Assets) -> Result<Document, String> {
        let mut files = Vec::new();
        let mut art = Vec::new();
        for (path, text) in assets.files() {
            if let Some(kind) = FileKind::of_path(path) {
                let root: toml::Table = toml::from_str(text).map_err(|e| format!("{path}: {}", e.message()))?;
                files.push(FileDoc { path: path.clone(), kind, head: head_of(text), root, dirty: false });
            } else if path.starts_with("art/") && path.ends_with(".txt") {
                let file = ArtFile::parse(text).map_err(|(line, msg)| format!("{path}:{line}: {msg}"))?;
                art.push(ArtDoc { path: path.clone(), file, dirty: false });
            }
        }
        let mut tables = Vec::new();
        for kind in TABLE_KINDS {
            let (file, key) = match kind.file_and_key() {
                Some((f, k)) => (files.iter().position(|d| d.path == f), k),
                None => (None, ""),
            };
            if kind.file_and_key().is_some() && file.is_none() {
                return Err(format!("{} is missing", kind.file_and_key().unwrap().0));
            }
            tables.push(TableDef { kind, file, key });
        }
        let dir = match &assets.source {
            Source::Dir(d) => Some(d.clone()),
            _ => None,
        };
        Ok(Document { dir, files, art, tables, undo: Vec::new(), redo: Vec::new() })
    }

    /// Load and validate a directory, then take its files.
    pub fn from_dir(dir: &Path) -> Result<Document, AssetError> {
        let assets = Assets::from_dir(dir)?;
        let mut doc = Document::from_assets(&assets).map_err(|msg| AssetError { file: dir.display().to_string(), line: None, row: None, msg })?;
        doc.dir = Some(dir.to_path_buf());
        Ok(doc)
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    pub fn kind(&self, t: usize) -> TableKind {
        self.tables[t].kind
    }

    /// Index of a table by kind.
    pub fn table_of(&self, kind: TableKind) -> usize {
        self.tables.iter().position(|d| d.kind == kind).expect("every kind is listed")
    }

    /// Indices of the tileset files.
    fn tileset_files(&self) -> Vec<usize> {
        self.files.iter().enumerate().filter(|(_, f)| f.kind == FileKind::Tileset).map(|(i, _)| i).collect()
    }

    pub fn row_count(&self, t: usize) -> usize {
        let def = self.tables[t];
        match def.kind {
            TableKind::Art => self.art.len(),
            TableKind::Tilesets => self.tileset_files().len(),
            TableKind::Density => 1,
            _ => self.files[def.file.unwrap()].root.get(def.key).and_then(|v| v.as_array()).map_or(0, |a| a.len()),
        }
    }

    /// The file a table's rows live in; for tilesets and art, the file of
    /// one row.
    pub fn file_of(&self, t: usize, row: usize) -> Option<usize> {
        let def = self.tables[t];
        match def.kind {
            TableKind::Art => None,
            TableKind::Tilesets => self.tileset_files().get(row).copied(),
            _ => def.file,
        }
    }

    /// The path of the file a table (row) is stored in.
    pub fn path_of(&self, t: usize, row: usize) -> String {
        match self.tables[t].kind {
            TableKind::Art => self.art.get(row).map(|a| a.path.clone()).unwrap_or_default(),
            _ => self.file_of(t, row).map(|f| self.files[f].path.clone()).unwrap_or_default(),
        }
    }

    pub fn row(&self, t: usize, i: usize) -> Option<&toml::Table> {
        let def = self.tables[t];
        match def.kind {
            TableKind::Art => None,
            TableKind::Tilesets => Some(&self.files[*self.tileset_files().get(i)?].root),
            TableKind::Density => self.files[def.file?].root.get(def.key)?.as_table(),
            _ => self.files[def.file?].root.get(def.key)?.as_array()?.get(i)?.as_table(),
        }
    }

    fn row_mut(&mut self, t: usize, i: usize) -> Option<&mut toml::Table> {
        let def = self.tables[t];
        match def.kind {
            TableKind::Art => None,
            TableKind::Tilesets => {
                let f = *self.tileset_files().get(i)?;
                Some(&mut self.files[f].root)
            }
            TableKind::Density => self.files[def.file?].root.get_mut(def.key)?.as_table_mut(),
            _ => self.files[def.file?].root.get_mut(def.key)?.as_array_mut()?.get_mut(i)?.as_table_mut(),
        }
    }

    /// The name a row is listed under.
    pub fn row_name(&self, t: usize, i: usize) -> String {
        let kind = self.tables[t].kind;
        match kind {
            TableKind::Art => self.art.get(i).map(|a| format!("{} {}", a.file.name, a.file.tier.name())).unwrap_or_default(),
            TableKind::Density => "density".to_string(),
            _ => self.row(t, i).and_then(|r| r.get(kind.name_field())).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("#{i}")),
        }
    }

    /// The `name`s of a table's rows, in order; for art, the distinct art
    /// names sorted.
    pub fn names(&self, kind: TableKind) -> Vec<String> {
        if kind == TableKind::Art {
            let mut v: Vec<String> = self.art.iter().map(|a| a.file.name.clone()).collect();
            v.sort();
            v.dedup();
            return v;
        }
        let t = self.table_of(kind);
        (0..self.row_count(t)).map(|i| self.row_name(t, i)).collect()
    }

    /// Art files by art name, in index order.
    pub fn art_named(&self, name: &str) -> Vec<usize> {
        self.art.iter().enumerate().filter(|(_, a)| a.file.name == name).map(|(i, _)| i).collect()
    }

    pub fn get(&self, t: usize, i: usize, path: &str) -> Option<&toml::Value> {
        get_path(self.row(t, i)?, path)
    }

    fn take_snapshot(&mut self) {
        let snap = Snapshot { files: self.files.iter().map(|f| (f.root.clone(), f.dirty)).collect(), art: self.art.iter().map(|a| (a.file.clone(), a.dirty)).collect() };
        self.undo.push(snap);
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn current(&self) -> Snapshot {
        Snapshot { files: self.files.iter().map(|f| (f.root.clone(), f.dirty)).collect(), art: self.art.iter().map(|a| (a.file.clone(), a.dirty)).collect() }
    }

    fn restore(&mut self, snap: Snapshot) {
        for (f, (root, dirty)) in self.files.iter_mut().zip(snap.files) {
            f.root = root;
            f.dirty = dirty;
        }
        for (a, (file, dirty)) in self.art.iter_mut().zip(snap.art) {
            a.file = file;
            a.dirty = dirty;
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(snap) = self.undo.pop() else { return false };
        let now = self.current();
        self.redo.push(now);
        self.restore(snap);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snap) = self.redo.pop() else { return false };
        let now = self.current();
        self.undo.push(now);
        self.restore(snap);
        true
    }

    pub fn is_dirty(&self) -> bool {
        self.files.iter().any(|f| f.dirty) || self.art.iter().any(|a| a.dirty)
    }

    /// Set one field of a row, checking the row still deserialises into its
    /// typed struct; on failure the row is unchanged.
    pub fn set_field(&mut self, t: usize, i: usize, path: &str, value: toml::Value) -> Result<(), String> {
        let def = self.tables[t];
        let file = self.file_of(t, i).ok_or("this table has no fields")?;
        let kind = self.files[file].kind;
        let mut row = self.row(t, i).ok_or("no such row")?.clone();
        set_path(&mut row, path, value);
        kind.check_row(def.key, &row)?;
        self.take_snapshot();
        *self.row_mut(t, i).expect("row exists") = row;
        self.files[file].dirty = true;
        Ok(())
    }

    /// Remove an optional field so the loader's default applies.
    pub fn clear_field(&mut self, t: usize, i: usize, path: &str) -> Result<(), String> {
        let def = self.tables[t];
        let file = self.file_of(t, i).ok_or("this table has no fields")?;
        if fields::fields(def.kind).iter().any(|f| f.name == path && f.required) {
            return Err(format!("{path} is required"));
        }
        let mut row = self.row(t, i).ok_or("no such row")?.clone();
        if remove_path(&mut row, path).is_none() {
            return Err(format!("{path} is already unset"));
        }
        self.files[file].kind.check_row(def.key, &row)?;
        self.take_snapshot();
        *self.row_mut(t, i).expect("row exists") = row;
        self.files[file].dirty = true;
        Ok(())
    }

    /// Insert a copy of row `i` after it, named `<name>-copy`, and return
    /// the new index.
    pub fn add_row(&mut self, t: usize, i: usize) -> Result<usize, String> {
        let def = self.tables[t];
        if !def.kind.resizable() {
            return Err(format!("{} has a fixed set of rows", def.kind.name()));
        }
        let mut row = self.row(t, i).ok_or("no row to copy")?.clone();
        let name_field = def.kind.name_field();
        let name = row.get(name_field).and_then(|v| v.as_str()).unwrap_or("row").to_string();
        row.insert(name_field.to_string(), toml::Value::String(format!("{name}-copy")));
        self.take_snapshot();
        let file = def.file.expect("row tables have a file");
        let arr = self.files[file].root.get_mut(def.key).and_then(|v| v.as_array_mut()).ok_or("no row array")?;
        arr.insert(i + 1, toml::Value::Table(row));
        self.files[file].dirty = true;
        Ok(i + 1)
    }

    /// Other rows that name this one, as `table.row` strings.
    pub fn references(&self, kind: TableKind, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (t, def) in self.tables.iter().enumerate() {
            for f in fields::fields(def.kind) {
                let hits = match f.kind {
                    Kind::Ref { table, .. } if table == kind => true,
                    Kind::Pairs(table) if table == kind => true,
                    _ => false,
                };
                if !hits {
                    continue;
                }
                for i in 0..self.row_count(t) {
                    let Some(v) = self.get(t, i, f.name) else { continue };
                    let named = match (f.kind, v) {
                        (Kind::Pairs(_), toml::Value::Array(a)) => a.iter().any(|p| p.as_array().and_then(|p| p.first()).and_then(|n| n.as_str()) == Some(name)),
                        (_, toml::Value::String(s)) => s == name,
                        _ => false,
                    };
                    if named {
                        out.push(format!("{}.{}", def.kind.name(), self.row_name(t, i)));
                    }
                }
            }
        }
        out
    }

    /// Delete a row, refused when another table references it by name or
    /// the table would be empty.
    pub fn delete_row(&mut self, t: usize, i: usize) -> Result<(), String> {
        let def = self.tables[t];
        if !def.kind.resizable() {
            return Err(format!("{} has a fixed set of rows", def.kind.name()));
        }
        if self.row_count(t) <= 1 {
            return Err("the last row cannot be deleted".to_string());
        }
        let name = self.row_name(t, i);
        let refs = self.references(def.kind, &name);
        if !refs.is_empty() {
            return Err(format!("{name:?} is referenced by {}", refs.join(", ")));
        }
        self.take_snapshot();
        let file = def.file.expect("row tables have a file");
        let arr = self.files[file].root.get_mut(def.key).and_then(|v| v.as_array_mut()).ok_or("no row array")?;
        arr.remove(i);
        self.files[file].dirty = true;
        Ok(())
    }

    /// Replace an art file's contents after a grid edit.
    pub fn set_art(&mut self, i: usize, file: ArtFile) {
        self.take_snapshot();
        self.art[i].file = file;
        self.art[i].dirty = true;
    }

    /// Every file as (path, text): tables rendered through their typed
    /// structs under their head comments, art files regenerated.
    pub fn to_files(&self) -> Result<Vec<(String, String)>, SaveError> {
        let mut out = Vec::with_capacity(self.files.len() + self.art.len());
        for (fi, f) in self.files.iter().enumerate() {
            let body = f.kind.render(&f.root).map_err(|msg| SaveError { msg: format!("{}: {msg}", f.path), at: self.first_row_of_file(fi) })?;
            out.push((f.path.clone(), format!("{}{body}", f.head)));
        }
        for a in &self.art {
            out.push((a.path.clone(), a.file.to_text()));
        }
        Ok(out)
    }

    fn first_row_of_file(&self, file: usize) -> Option<(usize, usize)> {
        if self.files[file].kind == FileKind::Tileset {
            let row = self.tileset_files().iter().position(|&f| f == file)?;
            return Some((self.table_of(TableKind::Tilesets), row));
        }
        self.tables.iter().position(|d| d.file == Some(file)).map(|t| (t, 0))
    }

    /// The table and row a loader error points at.
    pub fn locate(&self, err: &AssetError) -> Option<(usize, usize)> {
        if let Some(i) = self.art.iter().position(|a| a.path == err.file) {
            return Some((self.table_of(TableKind::Art), i));
        }
        let file = self.files.iter().position(|f| f.path == err.file)?;
        if let Some((key, i, _)) = &err.row {
            if let Some(t) = self.tables.iter().position(|d| d.file == Some(file) && d.key == key) {
                return Some((t, (*i).min(self.row_count(t).saturating_sub(1))));
            }
        }
        self.first_row_of_file(file)
    }

    /// Validate the whole set as the game would load it.
    pub fn validate(&self) -> Result<Assets, SaveError> {
        let files = self.to_files()?;
        Assets::from_strings(&files).map_err(|e| SaveError { msg: e.to_string(), at: self.locate(&e) })
    }

    /// Validate, then write the changed files (all, or those of one table)
    /// atomically: each to `<name>.tmp` then renamed over the original.
    /// Returns the paths written.
    pub fn save(&mut self, only: Option<usize>) -> Result<Vec<String>, SaveError> {
        let dir = self.dir.clone().ok_or_else(|| SaveError { msg: "no directory to write to: run on an exported directory".to_string(), at: None })?;
        let rendered = self.to_files()?;
        Assets::from_strings(&rendered).map_err(|e| SaveError { msg: e.to_string(), at: self.locate(&e) })?;
        let wanted: Vec<String> = match only {
            None => self.files.iter().filter(|f| f.dirty).map(|f| f.path.clone()).chain(self.art.iter().filter(|a| a.dirty).map(|a| a.path.clone())).collect(),
            Some(t) => match self.tables[t].kind {
                TableKind::Art => self.art.iter().filter(|a| a.dirty).map(|a| a.path.clone()).collect(),
                TableKind::Tilesets => self.files.iter().filter(|f| f.dirty && f.kind == FileKind::Tileset).map(|f| f.path.clone()).collect(),
                _ => self.tables[t].file.map(|f| self.files[f].path.clone()).into_iter().collect(),
            },
        };
        let mut written = Vec::new();
        for (path, text) in rendered.iter().filter(|(p, _)| wanted.contains(p)) {
            let target = dir.join(path);
            let tmp = dir.join(format!("{path}.tmp"));
            let io = |e: std::io::Error| SaveError { msg: format!("{path}: {e}"), at: None };
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(&tmp, text).map_err(io)?;
            std::fs::rename(&tmp, &target).map_err(io)?;
            written.push(path.clone());
        }
        for f in self.files.iter_mut().filter(|f| written.contains(&f.path)) {
            f.dirty = false;
        }
        for a in self.art.iter_mut().filter(|a| written.contains(&a.path)) {
            a.dirty = false;
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    fn doc() -> Document {
        Document::from_assets(&test_assets()).unwrap()
    }

    #[test]
    fn tables_and_rows_come_from_the_files() {
        let d = doc();
        assert_eq!(d.table_count(), TABLE_KINDS.len());
        let species = d.table_of(TableKind::Species);
        assert_eq!(d.row_count(species), 13);
        assert_eq!(d.row_name(species, 0), "oak");
        assert_eq!(d.get(species, 0, "form").unwrap().as_str(), Some("broadleaf"));
        let tilesets = d.table_of(TableKind::Tilesets);
        assert_eq!(d.row_count(tilesets), 2);
        assert_eq!(d.row_name(tilesets, 0), "ascii");
        assert_eq!(d.get(tilesets, 0, "roles.cover.grass").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(d.row_count(d.table_of(TableKind::Density)), 1);
        assert_eq!(d.row_count(d.table_of(TableKind::Seasons)), 4);
        let art = d.table_of(TableKind::Art);
        assert!(d.row_count(art) > 20);
        assert!(d.names(TableKind::Art).contains(&"boulder".to_string()));
        assert_eq!(d.art_named("player").len(), 4);
        assert_eq!(d.path_of(species, 0), "species.toml");
        assert!(d.path_of(art, 0).starts_with("art/"));
        assert!(!d.is_dirty());
    }

    #[test]
    fn set_field_checks_types_and_marks_dirty() {
        let mut d = doc();
        let species = d.table_of(TableKind::Species);
        assert!(d.set_field(species, 0, "form", toml::Value::String("bush".into())).unwrap_err().contains("bush"));
        assert!(!d.is_dirty());
        d.set_field(species, 0, "form", toml::Value::String("pine".into())).unwrap();
        assert!(d.is_dirty());
        assert_eq!(d.get(species, 0, "form").unwrap().as_str(), Some("pine"));
        assert!(d.set_field(species, 0, "canopy", toml::Value::Integer(3)).is_err());
        d.set_field(species, 0, "reach", toml::Value::Float(2.0)).unwrap();
        assert!(d.clear_field(species, 0, "name").unwrap_err().contains("required"));
        d.clear_field(species, 0, "reach").unwrap();
        assert!(d.get(species, 0, "reach").is_none());
        assert!(d.clear_field(species, 0, "reach").is_err());
        assert!(d.undo());
        assert_eq!(d.get(species, 0, "reach").unwrap().as_float(), Some(2.0));
        assert!(d.redo());
        assert!(d.get(species, 0, "reach").is_none());
        assert!(d.undo() && d.undo() && d.undo());
        assert_eq!(d.get(species, 0, "form").unwrap().as_str(), Some("broadleaf"));
        assert!(!d.is_dirty());
        assert!(!d.undo());
        let tilesets = d.table_of(TableKind::Tilesets);
        d.set_field(tilesets, 1, "roles.rain", toml::Value::String("|".into())).unwrap();
        assert!(d.files.iter().any(|f| f.path == "tilesets/petscii.toml" && f.dirty));
    }

    #[test]
    fn add_and_delete_rows_respect_references() {
        let mut d = doc();
        let species = d.table_of(TableKind::Species);
        let n = d.add_row(species, 0).unwrap();
        assert_eq!(n, 1);
        assert_eq!(d.row_name(species, 1), "oak-copy");
        assert_eq!(d.row_count(species), 14);
        let refs = d.references(TableKind::Species, "oak");
        assert!(refs.contains(&"biomes.rainforest".to_string()), "{refs:?}");
        assert!(d.delete_row(species, 0).unwrap_err().contains("referenced"));
        d.delete_row(species, 1).unwrap();
        assert_eq!(d.row_count(species), 13);
        let seasons = d.table_of(TableKind::Seasons);
        assert!(d.add_row(seasons, 0).is_err());
        assert!(d.delete_row(seasons, 0).is_err());
        let art = d.table_of(TableKind::Art);
        assert!(d.add_row(art, 0).is_err());
        assert!(d.references(TableKind::Art, "boulder").contains(&"props.boulder".to_string()));
        assert!(d.references(TableKind::Art, "petscii/pine").contains(&"tilesets.petscii".to_string()));
    }

    #[test]
    fn rendered_files_equal_the_loaders_export() {
        let a = test_assets();
        let d = Document::from_assets(&a).unwrap();
        let ours = d.to_files().unwrap();
        let theirs = a.to_files();
        for (path, text) in &theirs {
            let mine = ours.iter().find(|(p, _)| p == path).map(|(_, t)| t).unwrap_or_else(|| panic!("{path} missing"));
            // The editor keeps a file's leading comment block and renders
            // the rest exactly as the loader's own export does.
            let head = d.files.iter().find(|f| &f.path == path).map(|f| f.head.as_str()).unwrap_or("");
            assert_eq!(mine, &format!("{head}{text}"), "{path}");
        }
        assert!(d.validate().is_ok());
    }

    #[test]
    fn save_validates_writes_atomically_and_reloads() {
        let a = test_assets();
        let dir = std::env::temp_dir().join(format!("roguemap-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        a.export(&dir).unwrap();
        std::fs::write(dir.join("species.toml"), format!("# hand notes\n\n{}", std::fs::read_to_string(dir.join("species.toml")).unwrap())).unwrap();
        let mut d = Document::from_dir(&dir).unwrap();
        assert_eq!(d.dir.as_deref(), Some(dir.as_path()));
        let species = d.table_of(TableKind::Species);
        let biomes = d.table_of(TableKind::Biomes);

        // A dangling reference is refused and located, and nothing is written.
        d.set_field(biomes, 1, "material", toml::Value::String("brick".into())).unwrap();
        let before = std::fs::read_to_string(dir.join("biomes.toml")).unwrap();
        let err = d.save(None).unwrap_err();
        assert!(err.msg.contains("brick"), "{}", err.msg);
        assert_eq!(err.at, Some((biomes, 1)));
        assert_eq!(std::fs::read_to_string(dir.join("biomes.toml")).unwrap(), before);
        assert!(d.undo());

        d.set_field(species, 0, "size_class", toml::Value::String("large".into())).unwrap();
        let written = d.save(Some(species)).unwrap();
        assert_eq!(written, vec!["species.toml".to_string()]);
        assert!(!d.is_dirty());
        let text = std::fs::read_to_string(dir.join("species.toml")).unwrap();
        assert!(text.starts_with("# hand notes\n\n[[species]]\n"), "{}", &text[..60]);
        assert!(!dir.join("species.toml.tmp").exists());
        let b = Assets::from_dir(&dir).unwrap();
        assert_eq!(b.species[0].size_class, crate::biome::SizeClass::Large);
        assert_eq!(b.species.len(), a.species.len());

        // Art round trip through the grid path.
        let art_i = d.art_named("boulder")[0];
        let mut f = d.art[art_i].file.clone();
        f.rows[0] = f.rows[0].replace(' ', "#");
        d.set_art(art_i, f.clone());
        let written = d.save(None).unwrap();
        assert_eq!(written, vec![d.art[art_i].path.clone()]);
        let c = Assets::from_dir(&dir).unwrap();
        assert_eq!(c.art.get("boulder", f.tier).unwrap().rows, f.rows);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_walk_nested_tables() {
        let mut t: toml::Table = toml::from_str("a = 1\n[b]\nc = 2\n[b.d]\ne = \"x\"\n").unwrap();
        assert_eq!(get_path(&t, "b.d.e").unwrap().as_str(), Some("x"));
        assert!(get_path(&t, "b.z").is_none());
        set_path(&mut t, "b.d.f", toml::Value::Integer(3));
        set_path(&mut t, "g.h", toml::Value::Integer(4));
        assert_eq!(get_path(&t, "g.h").unwrap().as_integer(), Some(4));
        let mut leaves = Vec::new();
        leaf_paths(&t, "", &mut leaves);
        assert_eq!(leaves, vec!["a", "b.c", "b.d.e", "b.d.f", "g.h"]);
        assert!(remove_path(&mut t, "b.d.f").is_some());
        assert!(remove_path(&mut t, "b.d.f").is_none());
        assert_eq!(head_of("# one\n\n# two\n[[x]]\n"), "# one\n\n# two\n");
        assert_eq!(head_of("[[x]]\n"), "");
    }
}
