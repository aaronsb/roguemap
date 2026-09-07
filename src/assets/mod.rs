//! Asset tables loaded from `assets/` (ADR-001).
//!
//! The tables are TOML files embedded in the binary by `build.rs` and
//! overridable with `ROGUEMAP_ASSETS=<dir>`. `Assets::load` builds one
//! struct holding every table, resolved: names replaced by indices and
//! every cross-reference checked, so the rest of the program indexes
//! without looking anything up. Errors name the file, the line and the row.

pub mod schema;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::biome::{Biome, Block, Creature, MaterialRule, Prop, Species, KOPPEN_CODES};
use crate::biome::{Material, MaterialRule::Local};
use crate::frame::{parse_key, FrameSpec};
use crate::lsystem::{Alternative, Grammar, Overrides, Params, Style};
use crate::palette::{Density, Palette, Season, Surface, SurfaceColors, Surfaces, SEASON_NAMES, SURFACE_NAMES};
use crate::properties::{Conditions, Hooks, Identity, Physical};
use crate::settings::{SettingItem, REQUIRED_SETTINGS};
use crate::sprite::Sprite;
use crate::volume::Shape;
use crate::world::LightSpec;
use schema::*;

pub use schema::{ArtFile, Tier, TilesetFile as TilesetSpec, CONDITIONS};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// The table files every asset set must have, in load order.
pub const TABLES: [&str; 12] =
    ["biomes.toml", "species.toml", "materials.toml", "props.toml", "blocks.toml", "creatures.toml", "surfaces.toml", "lights.toml", "settings.toml", "ui.toml", "tree_styles.toml", "editor-ui.toml"];

/// Where a set of assets came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Embedded,
    Dir(PathBuf),
    /// `Assets::from_strings`.
    Memory,
}

/// A load failure: the file, the line for parse errors, the table row for
/// resolve errors, and what went wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetError {
    pub file: String,
    pub line: Option<usize>,
    /// Table name, row index and row name.
    pub row: Option<(String, usize, String)>,
    pub msg: String,
}

impl AssetError {
    fn file(file: &str, msg: impl Into<String>) -> AssetError {
        AssetError { file: file.to_string(), line: None, row: None, msg: msg.into() }
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
        }
        write!(f, ": ")?;
        if let Some((table, i, name)) = &self.row {
            write!(f, "{table}[{i}] {name:?}: ")?;
        }
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for AssetError {}

impl From<AssetError> for io::Error {
    fn from(e: AssetError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }
}

/// One hand-drawn sprite with where it was loaded from, and its mirror
/// for a figure facing the other way (ADR-008), built once here.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtEntry {
    pub path: String,
    pub file: ArtFile,
    pub sprite: Sprite,
    pub mirrored: Sprite,
}

/// Hand-drawn sprites indexed by (name, tier), picked per zoom by each
/// tier's `min_zoom`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArtIndex {
    pub entries: Vec<ArtEntry>,
    /// Entry indices per name, in ascending `min_zoom` order.
    by_name: HashMap<String, Vec<usize>>,
}

impl ArtIndex {
    fn insert(&mut self, path: &str, file: ArtFile) -> Result<(), AssetError> {
        let slots = self.by_name.entry(file.name.clone()).or_default();
        for &i in slots.iter() {
            let other = &self.entries[i];
            if other.file.tier == file.tier {
                return Err(AssetError::file(path, format!("art {:?} tier {} is already defined in {}", file.name, file.tier.name(), other.path)));
            }
            if other.file.min_zoom == file.min_zoom {
                return Err(AssetError::file(path, format!("art {:?} tier {} starts at zoom {} like tier {} in {}", file.name, file.tier.name(), file.min_zoom, other.file.tier.name(), other.path)));
            }
        }
        let sprite = Sprite { rows: file.rows.clone(), poses: file.poses.clone(), center: file.center, base_rows: file.base_rows };
        let mirrored = sprite.mirrored();
        let at = slots.iter().position(|&i| self.entries[i].file.min_zoom > file.min_zoom).unwrap_or(slots.len());
        slots.insert(at, self.entries.len());
        self.entries.push(ArtEntry { path: path.to_string(), file, sprite, mirrored });
        Ok(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The sprite of exactly this tier.
    pub fn get(&self, name: &str, tier: Tier) -> Option<&Sprite> {
        self.by_name.get(name)?.iter().map(|&i| &self.entries[i]).find(|e| e.file.tier == tier).map(|e| &e.sprite)
    }

    /// The sprite for a zoom: the tier with the largest `min_zoom` at or
    /// below the zoom, or the lowest tier there is when none is below.
    pub fn for_zoom(&self, name: &str, zoom: usize) -> Option<&Sprite> {
        let slots = self.by_name.get(name)?;
        let below = slots.iter().rev().find(|&&i| self.entries[i].file.min_zoom <= zoom);
        below.or(slots.first()).map(|&i| &self.entries[i].sprite)
    }

    /// The sprite for a thing that stands `rows` rows tall at this zoom:
    /// the tier whose own row count is nearest, so a 2 m person picks the
    /// twelve-row figure at 1:1 and the one-glyph one at 1:8 (ADR-004).
    /// The sprite is drawn at the tier's size with its feet on the ground.
    pub fn for_rows(&self, name: &str, rows: f32) -> Option<&Sprite> {
        self.for_rows_facing(name, rows, false)
    }

    /// `for_rows` for a figure facing left, which is the art's mirror
    /// (ADR-008).
    pub fn for_rows_facing(&self, name: &str, rows: f32, left: bool) -> Option<&Sprite> {
        let slots = self.by_name.get(name)?;
        let distance = |i: usize| (self.entries[i].sprite.rows.len() as f32 - rows).abs();
        let best = slots.iter().copied().reduce(|a, b| if distance(b) < distance(a) { b } else { a })?;
        let e = &self.entries[best];
        Some(if left { &e.mirrored } else { &e.sprite })
    }

    /// Every art name, sorted.
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.by_name.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }
}

/// The parsed files, kept so the set can be written back out.
#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub biomes: BiomesFile,
    pub species: SpeciesFile,
    pub materials: MaterialsFile,
    pub props: PropsFile,
    pub blocks: BlocksFile,
    pub creatures: CreaturesFile,
    pub surfaces: SurfacesFile,
    pub lights: LightsFile,
    pub settings: SettingsFile,
    pub ui: UiFile,
    pub tree_styles: TreeStylesFile,
    /// The editor's panes, the same shape as `ui`.
    pub editor_ui: UiFile,
    /// (relative path, file).
    pub tilesets: Vec<(String, TilesetFile)>,
}

/// Every table, resolved and validated.
#[derive(Clone, Debug, PartialEq)]
pub struct Assets {
    pub biomes: Vec<Biome>,
    pub species: Vec<Species>,
    pub materials: Vec<Material>,
    pub props: Vec<Prop>,
    pub blocks: Vec<Block>,
    pub creatures: Vec<Creature>,
    pub surfaces: Surfaces,
    pub lights: Vec<LightSpec>,
    pub settings: Vec<SettingItem>,
    /// Overlay frames (ADR-005).
    pub frames: Vec<FrameSpec>,
    /// The asset editor's panes, the same system over its own content
    /// kinds (ADR-005 step 6).
    pub editor_frames: Vec<FrameSpec>,
    /// Growth habits for L-system species (docs/lsystem.md).
    pub styles: Vec<Style>,
    pub tilesets: Vec<TilesetSpec>,
    pub art: ArtIndex,
    /// Köppen code to biome index.
    pub koppen: HashMap<String, usize>,
    pub source: Source,
    pub raw: Raw,
    /// The files as loaded, for `export`.
    files: Vec<(String, String)>,
}

/// Table, file and row context while resolving.
struct Ctx<'a> {
    file: &'a str,
    table: &'a str,
}

impl Ctx<'_> {
    fn row(&self, i: usize, name: &str, msg: impl Into<String>) -> AssetError {
        AssetError { file: self.file.to_string(), line: None, row: Some((self.table.to_string(), i, name.to_string())), msg: msg.into() }
    }
}

/// Index of `name` in a list of names.
fn find(names: &[&str], name: &str) -> Option<usize> {
    names.iter().position(|n| *n == name)
}

fn unique(ctx: &Ctx, names: &[&str]) -> Result<(), AssetError> {
    for (i, n) in names.iter().enumerate() {
        if names[..i].contains(n) {
            return Err(ctx.row(i, n, format!("duplicate name {n:?}")));
        }
    }
    Ok(())
}

fn reference(ctx: &Ctx, i: usize, row: &str, what: &str, names: &[&str], name: &str) -> Result<usize, AssetError> {
    find(names, name).ok_or_else(|| ctx.row(i, row, format!("unknown {what} {name:?}")))
}

fn optional_reference(ctx: &Ctx, i: usize, row: &str, what: &str, names: &[&str], name: Option<&String>) -> Result<Option<usize>, AssetError> {
    name.map(|n| reference(ctx, i, row, what, names, n)).transpose()
}

fn unit(ctx: &Ctx, i: usize, row: &str, field: &str, v: Option<f32>) -> Result<(), AssetError> {
    match v {
        Some(x) if !(0.0..=1.0).contains(&x) => Err(ctx.row(i, row, format!("{field} {x} is outside 0..1"))),
        _ => Ok(()),
    }
}

fn non_negative(ctx: &Ctx, i: usize, row: &str, field: &str, v: Option<f32>) -> Result<(), AssetError> {
    match v {
        Some(x) if x < 0.0 => Err(ctx.row(i, row, format!("{field} {x} is negative"))),
        _ => Ok(()),
    }
}

/// A `size = [w, d, h]` in metres must be positive in every dimension.
fn sized(ctx: &Ctx, i: usize, row: &str, size: [f32; 3]) -> Result<(), AssetError> {
    if size.iter().any(|v| v.is_nan() || *v <= 0.0) {
        return Err(ctx.row(i, row, format!("size {size:?} must be positive in width, depth and height (metres)")));
    }
    Ok(())
}

/// Rule keys are single symbols; turn a file's `{"A": ...}` table into the
/// grammar's `{'A': ...}` one.
fn keyed(ctx: &Ctx, i: usize, row: &str, what: &str, table: &BTreeMap<String, Rule>) -> Result<BTreeMap<char, Vec<Alternative>>, AssetError> {
    let mut out = BTreeMap::new();
    for (sym, rule) in table {
        let mut chars = sym.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(ctx.row(i, row, format!("{what} key {sym:?} must be one symbol")));
        };
        out.insert(c, rule.alternatives().into_iter().map(|(replacement, weight)| Alternative { replacement, weight }).collect());
    }
    Ok(out)
}

/// Resolve a species row's growth habit into a checked `Grammar`: the
/// style's templates with this row's parameter overrides, or the row's own
/// axiom and rules when it writes a grammar out itself. `shape =
/// "lsystem"` and a habit imply each other, so a row cannot carry a grammar
/// nothing grows or ask for one it has not written.
fn grammar(ctx: &Ctx, i: usize, s: &SpeciesRow, styles: &[Style]) -> Result<Option<Grammar>, AssetError> {
    let wants = s.shape == Some(Shape::Lsystem);
    let row = s.lsystem.clone().unwrap_or_default();
    let own = row.axiom.is_some() || !row.rules.is_empty();
    if !wants {
        if s.style.is_some() || own || s.lsystem.is_some() {
            return Err(ctx.row(i, &s.name, format!("a growth habit needs shape = \"lsystem\"; this row is {}", s.shape.map_or("the form's shape".to_string(), |sh| format!("{sh:?}").to_lowercase()))));
        }
        return Ok(None);
    }
    let over = Overrides {
        branch_angle: row.angle,
        forks: row.forks,
        taper: row.taper,
        droop: row.droop,
        leaf_density: row.leaf_density,
        asymmetry: row.asymmetry,
        jitter: row.jitter,
        prune_height: row.prune_height,
        dead_whorls: row.dead_whorls,
        depth: row.depth,
        length: row.length,
        leaf_radius: row.leaf_radius,
        leaf_flat: row.leaf_flat,
    };
    let g = match &s.style {
        Some(name) => {
            let style = styles.iter().find(|st| st.name == *name).ok_or_else(|| {
                let known: Vec<&str> = styles.iter().map(|st| st.name.as_str()).collect();
                ctx.row(i, &s.name, format!("unknown style {name:?}; tree_styles.toml has {}", known.join(", ")))
            })?;
            let mut g = style.grammar(&over).map_err(|e| ctx.row(i, &s.name, format!("lsystem: {e}")))?;
            // A species may still replace the habit's rules outright.
            if let Some(a) = &row.axiom {
                g.axiom = a.clone();
            }
            if !row.rules.is_empty() {
                g.rules = keyed(ctx, i, &s.name, "rule", &row.rules)?;
            }
            if !row.dead_rules.is_empty() {
                g.dead_rules = keyed(ctx, i, &s.name, "dead_rules", &row.dead_rules)?;
            }
            g
        }
        None if own => {
            let missing = |f: &str| ctx.row(i, &s.name, format!("a species without a style writes its own grammar and needs {f} (docs/lsystem.md)"));
            let plain = Grammar::PLAIN;
            Grammar {
                axiom: row.axiom.clone().ok_or_else(|| missing("axiom"))?,
                rules: keyed(ctx, i, &s.name, "rule", &row.rules)?,
                dead_rules: keyed(ctx, i, &s.name, "dead_rules", &row.dead_rules)?,
                depth: row.depth.ok_or_else(|| missing("depth"))?,
                angle: row.angle.ok_or_else(|| missing("angle"))?,
                length: row.length.ok_or_else(|| missing("length"))?,
                taper: row.taper.unwrap_or(1.0),
                leaf_radius: row.leaf_radius.ok_or_else(|| missing("leaf_radius"))?,
                droop: row.droop.unwrap_or(plain.droop),
                leaf_density: row.leaf_density.unwrap_or(plain.leaf_density),
                asymmetry: row.asymmetry.unwrap_or(plain.asymmetry),
                jitter: row.jitter.unwrap_or(plain.jitter),
                prune_height: row.prune_height.unwrap_or(plain.prune_height),
                dead_whorls: row.dead_whorls.unwrap_or(plain.dead_whorls),
                leaf_flat: row.leaf_flat.unwrap_or(plain.leaf_flat),
            }
        }
        None => {
            return Err(ctx.row(i, &s.name, "shape = \"lsystem\" needs a style from tree_styles.toml, or its own axiom and rules in [species.lsystem] (docs/lsystem.md)"));
        }
    };
    g.validate().map_err(|e| ctx.row(i, &s.name, format!("lsystem: {e}")))?;
    Ok(Some(g))
}

/// How solid a crown of this form is when no growth habit says
/// (docs/structures.md, "Porous canopies"): an evergreen holds the light
/// out, a broadleaf lets flecks of what is behind it through, and a cactus
/// has no foliage to be porous with.
fn leaf_density_of(form: crate::biome::Form) -> f32 {
    match form {
        crate::biome::Form::Pine => 0.85,
        crate::biome::Form::Broadleaf => 0.7,
        crate::biome::Form::Scrub => 0.8,
        crate::biome::Form::Cactus => 1.0,
    }
}

/// Resolve `tree_styles.toml` into the growth habits species pick from.
fn styles(raw: &TreeStylesFile) -> Result<Vec<Style>, AssetError> {
    let ctx = Ctx { file: TABLES[10], table: "style" };
    let names: Vec<&str> = raw.style.iter().map(|s| s.name.as_str()).collect();
    unique(&ctx, &names)?;
    let mut out = Vec::new();
    for (i, r) in raw.style.iter().enumerate() {
        described(&ctx, i, &r.name, &r.identity)?;
        let p = &r.params;
        if p.forks == 0 {
            return Err(ctx.row(i, &r.name, "forks 0: a habit has at least one branch or stem"));
        }
        let style = Style {
            name: r.name.clone(),
            identity: Identity::from_row(&r.identity, "tree styles"),
            stand_in: r.stand_in,
            axiom: r.axiom.clone(),
            rules: keyed(&ctx, i, &r.name, "rule", &r.rules)?,
            dead_rules: keyed(&ctx, i, &r.name, "dead_rules", &r.dead_rules)?,
            params: Params {
                branch_angle: p.branch_angle,
                forks: p.forks,
                taper: p.taper,
                droop: p.droop,
                leaf_density: p.leaf_density,
                asymmetry: p.asymmetry,
                jitter: p.jitter,
                prune_height: p.prune_height,
                dead_whorls: p.dead_whorls,
                depth: p.depth,
                length: p.length,
                leaf_radius: p.leaf_radius,
                leaf_flat: p.leaf_flat,
            },
        };
        // The defaults must themselves make a tree, so a species that
        // overrides nothing still loads.
        style.grammar(&Overrides::default()).and_then(|g| g.validate()).map_err(|e| ctx.row(i, &r.name, e))?;
        out.push(style);
    }
    Ok(out)
}

/// Resolve one table of overlay frames (ADR-005): `ui.toml` for the game
/// and `editor-ui.toml` for the editor, which share the row shape and
/// differ only in the content kinds their code supplies.
fn frame_rows(ctx: &Ctx, rows: &[FrameRow], kinds: &[&str], category: &str) -> Result<Vec<FrameSpec>, AssetError> {
    let names: Vec<&str> = rows.iter().map(|f| f.name.as_str()).collect();
    unique(ctx, &names)?;
    let mut frames = Vec::new();
    for (i, f) in rows.iter().enumerate() {
        described(ctx, i, &f.name, &f.identity)?;
        let content = f.content.clone().unwrap_or_else(|| f.name.clone());
        if !kinds.contains(&content.as_str()) {
            return Err(ctx.row(i, &f.name, format!("unknown content kind {content:?}; code supplies {}", kinds.join(", "))));
        }
        let key = match &f.key {
            None => None,
            Some(k) => Some(parse_key(k).ok_or_else(|| ctx.row(i, &f.name, format!("unknown key {k:?}; name a key as \"tab\", \"esc\", \"enter\", \"space\" or one character")))?),
        };
        if f.size.min_cols < 0 || f.size.min_rows < 0 {
            return Err(ctx.row(i, &f.name, "minimum size is negative"));
        }
        if [f.margin.left, f.margin.top, f.margin.right, f.margin.bottom].iter().any(|n| *n < 0) {
            return Err(ctx.row(i, &f.name, "margin is negative"));
        }
        frames.push(FrameSpec {
            name: f.name.clone(),
            identity: Identity::from_row(&f.identity, category),
            title: f.title.clone(),
            content,
            anchor: f.anchor,
            size: f.size,
            margin: f.margin,
            border: f.border,
            background: f.background,
            z: f.z,
            priority: f.priority,
            show: f.show,
            key,
        });
    }
    Ok(frames)
}

fn described(ctx: &Ctx, i: usize, row: &str, id: &IdentityRow) -> Result<(), AssetError> {
    if id.description.trim().is_empty() {
        return Err(ctx.row(i, row, "missing description"));
    }
    Ok(())
}

fn check_hooks(ctx: &Ctx, i: usize, row: &str, h: &HooksRow) -> Result<(), AssetError> {
    non_negative(ctx, i, row, "reach", h.reach)
}

fn check_physical(ctx: &Ctx, i: usize, row: &str, p: &PhysicalRow) -> Result<(), AssetError> {
    unit(ctx, i, row, "snow_cover", p.snow_cover)?;
    unit(ctx, i, row, "cluster", p.cluster)?;
    unit(ctx, i, row, "sway", p.sway)?;
    unit(ctx, i, row, "heat", p.heat)?;
    for (f, v) in [("spacing", p.spacing), ("growth", p.growth), ("decay", p.decay), ("fuel", p.fuel), ("max_slope", p.max_slope)] {
        non_negative(ctx, i, row, f, v)?;
    }
    if let (Some(lo), Some(hi)) = (p.min_height, p.max_height) {
        if lo > hi {
            return Err(ctx.row(i, row, format!("min_height {lo} is above max_height {hi}")));
        }
    }
    Ok(())
}

fn check_conditions(ctx: &Ctx, i: usize, row: &str, c: &ConditionsRow) -> Result<(), AssetError> {
    for (f, v) in [("wet_darkening", c.wet_darkening), ("dry_fading", c.dry_fading), ("weathering", c.weathering), ("mossing", c.mossing), ("soiling", c.soiling)] {
        unit(ctx, i, row, f, v)?;
    }
    for k in c.condition_colors.keys() {
        if !CONDITIONS.contains(&k.as_str()) {
            return Err(ctx.row(i, row, format!("unknown condition {k:?}; conditions are {}", CONDITIONS.join(", "))));
        }
    }
    Ok(())
}

/// Parse one TOML file, mapping the error to a line and, when the span
/// falls inside an array-of-tables row, to that row.
fn parse<T: serde::de::DeserializeOwned>(path: &str, text: &str) -> Result<T, AssetError> {
    toml::from_str::<T>(text).map_err(|e| {
        let (line, row) = match e.span() {
            Some(span) => (Some(text[..span.start.min(text.len())].matches('\n').count() + 1), locate_row(text, span.start)),
            None => (None, None),
        };
        AssetError { file: path.to_string(), line, row, msg: e.message().to_string() }
    })
}

/// The `[[table]]` row an offset falls in: its name, index and `name`
/// field, when the file is laid out one row per header.
fn locate_row(text: &str, offset: usize) -> Option<(String, usize, String)> {
    let mut table: Option<String> = None;
    let mut index = 0usize;
    let mut name: Option<String> = None;
    let mut pos = 0usize;
    for line in text.lines() {
        if pos > offset {
            break;
        }
        let t = line.trim();
        if let Some(header) = t.strip_prefix("[[").and_then(|h| h.strip_suffix("]]")) {
            match &table {
                Some(prev) if prev == header.trim() => index += 1,
                _ => {
                    table = Some(header.trim().to_string());
                    index = 0;
                }
            }
            name = None;
        } else if let Some(v) = t.strip_prefix("name") {
            let v = v.trim_start();
            if let Some(v) = v.strip_prefix('=') {
                name = Some(v.trim().trim_matches('"').to_string());
            }
        }
        pos += line.len() + 1;
    }
    Some((table?, index, name.unwrap_or_default()))
}

impl Assets {
    /// The embedded set, or the directory `ROGUEMAP_ASSETS` names.
    pub fn load() -> Result<Assets, AssetError> {
        match std::env::var_os("ROGUEMAP_ASSETS") {
            Some(dir) => Assets::from_dir(Path::new(&dir)),
            None => Assets::embedded(),
        }
    }

    /// The set compiled into the binary.
    pub fn embedded() -> Result<Assets, AssetError> {
        let files: Vec<(String, String)> = embedded::FILES.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
        Assets::build(files, Source::Embedded)
    }

    /// Every `.toml` and `.txt` file under a directory. A directory that
    /// cannot be read, or one missing a table, is an error.
    pub fn from_dir(dir: &Path) -> Result<Assets, AssetError> {
        let shown = dir.display().to_string();
        if !dir.is_dir() {
            return Err(AssetError::file(&shown, "asset directory does not exist or is not a directory"));
        }
        let mut paths = Vec::new();
        walk(dir, &mut paths).map_err(|e| AssetError::file(&shown, format!("cannot read asset directory: {e}")))?;
        let mut files = Vec::with_capacity(paths.len());
        for p in paths {
            let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(&p).map_err(|e| AssetError::file(&rel, format!("cannot read: {e}")))?;
            files.push((rel, text));
        }
        Assets::build(files, Source::Dir(dir.to_path_buf()))
    }

    /// A set from (relative path, contents) pairs, as the editor validates
    /// before saving.
    pub fn from_strings(files: &[(String, String)]) -> Result<Assets, AssetError> {
        Assets::build(files.to_vec(), Source::Memory)
    }

    fn build(files: Vec<(String, String)>, source: Source) -> Result<Assets, AssetError> {
        let text = |name: &str| -> Result<&str, AssetError> { files.iter().find(|(p, _)| p == name).map(|(_, t)| t.as_str()).ok_or_else(|| AssetError::file(name, "missing")) };
        let raw = Raw {
            biomes: parse(TABLES[0], text(TABLES[0])?)?,
            species: parse(TABLES[1], text(TABLES[1])?)?,
            materials: parse(TABLES[2], text(TABLES[2])?)?,
            props: parse(TABLES[3], text(TABLES[3])?)?,
            blocks: parse(TABLES[4], text(TABLES[4])?)?,
            creatures: parse(TABLES[5], text(TABLES[5])?)?,
            surfaces: parse(TABLES[6], text(TABLES[6])?)?,
            lights: parse(TABLES[7], text(TABLES[7])?)?,
            settings: parse(TABLES[8], text(TABLES[8])?)?,
            ui: parse(TABLES[9], text(TABLES[9])?)?,
            tree_styles: parse(TABLES[10], text(TABLES[10])?)?,
            editor_ui: parse(TABLES[11], text(TABLES[11])?)?,
            tilesets: {
                let mut v = Vec::new();
                for (p, t) in &files {
                    if p.starts_with("tilesets/") && p.ends_with(".toml") {
                        v.push((p.clone(), parse::<TilesetFile>(p, t)?));
                    }
                }
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v
            },
        };
        for (p, _) in &files {
            if p.ends_with(".toml") && !TABLES.contains(&p.as_str()) && !p.starts_with("tilesets/") {
                return Err(AssetError::file(p, format!("not an asset table; the tables are {} and tilesets/*.toml", TABLES.join(", "))));
            }
        }
        let mut art = ArtIndex::default();
        let mut art_paths: Vec<&(String, String)> = files.iter().filter(|(p, _)| p.starts_with("art/") && p.ends_with(".txt")).collect();
        art_paths.sort_by(|a, b| a.0.cmp(&b.0));
        for (p, t) in art_paths {
            let file = ArtFile::parse(t).map_err(|(line, msg)| AssetError { file: p.clone(), line: Some(line), row: None, msg })?;
            art.insert(p, file)?;
        }
        Assets::resolve(raw, art, files, source)
    }

    fn resolve(raw: Raw, art: ArtIndex, files: Vec<(String, String)>, source: Source) -> Result<Assets, AssetError> {
        let material_names: Vec<&str> = raw.materials.material.iter().map(|m| m.name.as_str()).collect();
        let light_names: Vec<&str> = raw.lights.light.iter().map(|l| l.name.as_str()).collect();
        let species_names: Vec<&str> = raw.species.species.iter().map(|s| s.name.as_str()).collect();
        let block_names: Vec<&str> = raw.blocks.block.iter().map(|b| b.name.as_str()).collect();
        let tileset_names: Vec<&str> = raw.tilesets.iter().map(|(_, t)| t.name.as_str()).collect();

        // materials
        let ctx = Ctx { file: TABLES[2], table: "material" };
        unique(&ctx, &material_names)?;
        let materials: Vec<Material> = raw
            .materials
            .material
            .iter()
            .map(|m| Material { name: m.name.clone(), identity: Identity::from_row(&m.identity, "materials"), wall: m.wall, wall_glyph: m.wall_glyph, roof: m.roof, roof_glyph: m.roof_glyph })
            .collect();
        if find(&material_names, "stone").is_none() {
            return Err(AssetError::file(ctx.file, "a material named \"stone\" is required: buildings on rock and near the snow line are built of it"));
        }

        // lights
        let ctx = Ctx { file: TABLES[7], table: "light" };
        unique(&ctx, &light_names)?;
        let mut lights = Vec::new();
        for (i, l) in raw.lights.light.iter().enumerate() {
            described(&ctx, i, &l.name, &l.identity)?;
            if l.radius <= 0.0 {
                return Err(ctx.row(i, &l.name, format!("radius {} must be positive", l.radius)));
            }
            unit(&ctx, i, &l.name, "flicker_amount", Some(l.flicker_amount))?;
            non_negative(&ctx, i, &l.name, "flicker_rate", Some(l.flicker_rate))?;
            non_negative(&ctx, i, &l.name, "intensity", Some(l.intensity))?;
            if l.falloff <= 0.0 {
                return Err(ctx.row(i, &l.name, format!("falloff {} must be positive", l.falloff)));
            }
            lights.push(LightSpec {
                name: l.name.clone(),
                identity: Identity::from_row(&l.identity, "lights"),
                color: l.color,
                radius: l.radius,
                intensity: l.intensity,
                falloff: l.falloff,
                flicker_amount: l.flicker_amount,
                flicker_rate: l.flicker_rate,
            });
        }

        // tree styles, before the species that name them
        let tree_styles = styles(&raw.tree_styles)?;

        // species
        let ctx = Ctx { file: TABLES[1], table: "species" };
        unique(&ctx, &species_names)?;
        let mut species = Vec::new();
        for (i, s) in raw.species.species.iter().enumerate() {
            described(&ctx, i, &s.name, &s.identity)?;
            sized(&ctx, i, &s.name, s.size)?;
            check_hooks(&ctx, i, &s.name, &s.hooks)?;
            check_physical(&ctx, i, &s.name, &s.physical)?;
            check_conditions(&ctx, i, &s.name, &s.conditions)?;
            for (f, v) in [("radius", s.radius), ("height", s.height), ("trunk", s.trunk), ("trunk_radius", s.trunk_radius)] {
                non_negative(&ctx, i, &s.name, f, v)?;
            }
            if s.radius == Some(0.0) || s.height == Some(0.0) {
                return Err(ctx.row(i, &s.name, "radius and height must be positive: a canopy has some size"));
            }
            unit(&ctx, i, &s.name, "dead_chance", s.dead_chance)?;
            let lsystem = grammar(&ctx, i, s, &tree_styles)?;
            // A grown species reads as its habit's stand-in from far away,
            // and its crown is as solid as the habit's leaf density says;
            // one that is not grown takes the density its form implies.
            let style = s.style.as_ref().and_then(|n| tree_styles.iter().find(|st| st.name == *n));
            let stand_in = style.map(|st| st.stand_in());
            let leaf_density = match &lsystem {
                Some(g) => g.leaf_density,
                None => leaf_density_of(s.form),
            };
            species.push(Species {
                name: s.name.clone(),
                identity: Identity::from_row(&s.identity, "species"),
                form: s.form,
                size_class: s.size_class,
                size: s.size,
                canopy: s.canopy.expand(),
                canopy_glyph: s.canopy_glyph.expand(),
                shape: s.shape,
                radius: s.radius,
                height: s.height,
                trunk: s.trunk,
                trunk_radius: s.trunk_radius,
                lsystem,
                stand_in,
                leaf_density,
                sheds: s.sheds.unwrap_or(matches!(s.canopy, Seasonal::Four(_))),
                dead_chance: s.dead_chance.unwrap_or(0.02),
                light: optional_reference(&ctx, i, &s.name, "light", &light_names, s.hooks.light.as_ref())?,
                hooks: Hooks::from_row(&s.hooks),
                physical: Physical::from_row(&s.physical, false, 1.0),
                conditions: Conditions::from_row(&s.conditions),
            });
        }

        // biomes
        let ctx = Ctx { file: TABLES[0], table: "biome" };
        let biome_names: Vec<&str> = raw.biomes.biome.iter().map(|b| b.name.as_str()).collect();
        unique(&ctx, &biome_names)?;
        let mut biomes = Vec::new();
        let mut koppen = HashMap::new();
        for (i, b) in raw.biomes.biome.iter().enumerate() {
            if koppen.insert(b.koppen.clone(), i).is_some() {
                return Err(ctx.row(i, &b.name, format!("Köppen code {:?} is already used", b.koppen)));
            }
            if b.grass > 3 {
                return Err(ctx.row(i, &b.name, format!("grass {} is above 3", b.grass)));
            }
            unit(&ctx, i, &b.name, "tree_density", Some(b.tree_density))?;
            if !(0.1..=8.0).contains(&b.spacing) {
                return Err(ctx.row(i, &b.name, format!("spacing {} is outside 0.1..8, the factor on a species' own spacing", b.spacing)));
            }
            let mut sp = Vec::new();
            for (name, w) in &b.species {
                let ix = reference(&ctx, i, &b.name, "species", &species_names, name)?;
                if *w == 0 {
                    return Err(ctx.row(i, &b.name, format!("species {name:?} has weight 0")));
                }
                sp.push((ix, *w));
            }
            biomes.push(Biome {
                name: b.name.clone(),
                identity: Identity::from_row(&b.identity, "biomes"),
                koppen: b.koppen.clone(),
                cover: b.cover,
                ground: b.ground,
                ground_glyph: b.ground_glyph,
                seasonal: b.seasonal,
                grass: b.grass,
                tree_density: b.tree_density,
                spacing: b.spacing,
                species: sp,
                material: reference(&ctx, i, &b.name, "material", &material_names, &b.material)?,
            });
        }
        for code in KOPPEN_CODES {
            if !koppen.contains_key(code) {
                return Err(AssetError::file(ctx.file, format!("no biome carries Köppen code {code:?}; the classifier needs every one of {}", KOPPEN_CODES.join(", "))));
            }
        }

        // props
        let ctx = Ctx { file: TABLES[3], table: "prop" };
        let prop_names: Vec<&str> = raw.props.prop.iter().map(|p| p.name.as_str()).collect();
        unique(&ctx, &prop_names)?;
        let mut props = Vec::new();
        for (i, p) in raw.props.prop.iter().enumerate() {
            described(&ctx, i, &p.name, &p.identity)?;
            if !art.has(&p.art) {
                return Err(ctx.row(i, &p.name, format!("unknown art {:?}", p.art)));
            }
            if p.terrain.is_empty() {
                return Err(ctx.row(i, &p.name, "terrain list is empty"));
            }
            sized(&ctx, i, &p.name, p.size)?;
            unit(&ctx, i, &p.name, "density", Some(p.density))?;
            check_hooks(&ctx, i, &p.name, &p.hooks)?;
            check_physical(&ctx, i, &p.name, &p.physical)?;
            check_conditions(&ctx, i, &p.name, &p.conditions)?;
            props.push(Prop {
                name: p.name.clone(),
                identity: Identity::from_row(&p.identity, "props"),
                art: p.art.clone(),
                size: p.size,
                color: p.color,
                glyph: p.glyph,
                density: p.density,
                terrain: p.terrain.clone(),
                cover: p.cover.clone(),
                near_water: p.near_water,
                min_zoom: p.min_zoom,
                light: optional_reference(&ctx, i, &p.name, "light", &light_names, p.hooks.light.as_ref())?,
                hooks: Hooks::from_row(&p.hooks),
                physical: Physical::from_row(&p.physical, true, 0.0),
                conditions: Conditions::from_row(&p.conditions),
            });
        }
        match find(&prop_names, "campfire") {
            Some(i) if props[i].light.is_some() => {}
            Some(i) => return Err(ctx.row(i, "campfire", "the campfire prop needs a light; the f key places one")),
            None => return Err(AssetError::file(ctx.file, "a prop named \"campfire\" with a light is required: the f key places one")),
        }

        // blocks
        let ctx = Ctx { file: TABLES[4], table: "block" };
        unique(&ctx, &block_names)?;
        let mut blocks = Vec::new();
        for (i, b) in raw.blocks.block.iter().enumerate() {
            described(&ctx, i, &b.name, &b.identity)?;
            if b.terrain.is_empty() {
                return Err(ctx.row(i, &b.name, "terrain list is empty"));
            }
            check_hooks(&ctx, i, &b.name, &b.hooks)?;
            check_physical(&ctx, i, &b.name, &b.physical)?;
            check_conditions(&ctx, i, &b.name, &b.conditions)?;
            let material = match b.material.as_str() {
                "by_biome" | "local" => Local,
                m => MaterialRule::Fixed(reference(&ctx, i, &b.name, "material", &material_names, m)?),
            };
            if b.chance > 100 {
                return Err(ctx.row(i, &b.name, format!("chance {} is a percentage", b.chance)));
            }
            sized(&ctx, i, &b.name, b.size)?;
            let level_height = b.level_height.unwrap_or(b.size[2]);
            for (f, v) in [("level_height", level_height), ("pitch", b.pitch), ("max_rise", b.max_rise)] {
                non_negative(&ctx, i, &b.name, f, Some(v))?;
            }
            if b.window_pitch <= 0.0 {
                return Err(ctx.row(i, &b.name, format!("window_pitch {} must be positive", b.window_pitch)));
            }
            let windows = match b.windows.as_slice() {
                [] => None,
                [lo, hi] if (0.0..=1.0).contains(lo) && (0.0..=1.0).contains(hi) && lo < hi => Some([*lo, *hi]),
                _ => return Err(ctx.row(i, &b.name, format!("windows {:?} must be empty or a rising pair within 0..1", b.windows))),
            };
            let [[wmin, dmin], [wmax, dmax]] = b.footprint;
            if wmin == 0 || dmin == 0 || wmin > wmax || dmin > dmax {
                return Err(ctx.row(i, &b.name, format!("footprint {:?} must be a positive [[w, d], [w, d]] smallest to largest, in tiles", b.footprint)));
            }
            if wmax > 14 || dmax > 14 {
                return Err(ctx.row(i, &b.name, format!("footprint {:?} is wider than a settlement plot (14 tiles)", b.footprint)));
            }
            if b.levels[0] > b.levels[1] || b.levels[1] > b.max_levels {
                return Err(ctx.row(i, &b.name, format!("levels {:?} must rise and stay within max_levels {}", b.levels, b.max_levels)));
            }
            if b.levels[1] > 0 && level_height <= 0.0 {
                return Err(ctx.row(i, &b.name, "a kind with levels needs a positive level_height"));
            }
            blocks.push(Block {
                name: b.name.clone(),
                identity: Identity::from_row(&b.identity, "blocks"),
                settle_min: b.settle_min,
                chance: b.chance,
                terrain: b.terrain.clone(),
                size: b.size,
                levels: b.levels,
                footprint: b.footprint,
                level_height,
                roof: b.roof,
                pitch: b.pitch,
                max_rise: b.max_rise,
                material,
                merge: b.merge,
                ground: b.ground,
                ground_color: b.ground_color,
                ground_pitch: b.ground_pitch,
                windows,
                window_pitch: b.window_pitch,
                door: b.door,
                max_levels: b.max_levels,
                deck: b.deck,
                light: optional_reference(&ctx, i, &b.name, "light", &light_names, b.hooks.light.as_ref())?,
                hooks: Hooks::from_row(&b.hooks),
                physical: Physical::from_row(&b.physical, false, 0.0),
                conditions: Conditions::from_row(&b.conditions),
            });
        }

        // creatures
        let ctx = Ctx { file: TABLES[5], table: "creature" };
        let creature_names: Vec<&str> = raw.creatures.creature.iter().map(|c| c.name.as_str()).collect();
        unique(&ctx, &creature_names)?;
        if creature_names.first() != Some(&"player") {
            return Err(AssetError::file(ctx.file, "the first creature must be \"player\""));
        }
        let mut creatures = Vec::new();
        for (i, c) in raw.creatures.creature.iter().enumerate() {
            described(&ctx, i, &c.name, &c.identity)?;
            if !art.has(&c.art) {
                return Err(ctx.row(i, &c.name, format!("unknown art {:?}", c.art)));
            }
            let can_enter = c.can_enter.clone().unwrap_or_else(|| vec![Terrain::Sand, Terrain::Grass, Terrain::Dirt, Terrain::Rock, Terrain::Snow]);
            if can_enter.is_empty() {
                return Err(ctx.row(i, &c.name, "can_enter list is empty"));
            }
            sized(&ctx, i, &c.name, c.size)?;
            check_hooks(&ctx, i, &c.name, &c.hooks)?;
            check_conditions(&ctx, i, &c.name, &c.conditions)?;
            non_negative(&ctx, i, &c.name, "speed", c.speed)?;
            non_negative(&ctx, i, &c.name, "sight", c.sight)?;
            non_negative(&ctx, i, &c.name, "spacing", c.spacing)?;
            creatures.push(Creature {
                name: c.name.clone(),
                identity: Identity::from_row(&c.identity, "creatures"),
                art: c.art.clone(),
                size: c.size,
                color: c.color,
                glyph: c.glyph,
                can_enter,
                speed: c.speed.unwrap_or(2.0),
                diet: c.diet.clone(),
                behaviour: c.behaviour.clone().unwrap_or_else(|| "idle".to_string()),
                sight: c.sight.unwrap_or(16.0),
                home: optional_reference(&ctx, i, &c.name, "block", &block_names, c.home.as_ref())?,
                spacing: c.spacing.unwrap_or(0.0),
                light: optional_reference(&ctx, i, &c.name, "light", &light_names, c.hooks.light.as_ref())?,
                hooks: Hooks::from_row(&c.hooks),
                conditions: Conditions::from_row(&c.conditions),
            });
        }

        // surfaces
        let ctx = Ctx { file: TABLES[6], table: "season" };
        let sf = &raw.surfaces;
        if sf.season.len() != 4 {
            return Err(AssetError::file(ctx.file, format!("{} seasons; exactly four are needed: {}", sf.season.len(), SEASON_NAMES.join(", "))));
        }
        let mut seasons = Vec::new();
        for (i, s) in sf.season.iter().enumerate() {
            if s.name != SEASON_NAMES[i] {
                return Err(ctx.row(i, &s.name, format!("season {i} must be {:?}", SEASON_NAMES[i])));
            }
            seasons.push(Season {
                name: s.name.clone(),
                identity: Identity::from_row(&s.identity, "surfaces"),
                palette: Palette {
                    trunk: s.trunk,
                    trunk_glyph: s.trunk_glyph,
                    surfaces: [
                        SurfaceColors { color: s.sand, glyph: s.sand_glyph },
                        SurfaceColors { color: s.dirt, glyph: s.dirt_glyph },
                        SurfaceColors { color: s.rock, glyph: s.rock_glyph },
                        SurfaceColors { color: s.snow, glyph: s.snow_glyph },
                    ],
                    water_shallow: s.water_shallow,
                    water_deep: s.water_deep,
                    water_glyph: s.water_glyph,
                },
            });
        }
        let ctx = Ctx { file: TABLES[6], table: "surface" };
        if sf.surface.len() != 4 {
            return Err(AssetError::file(ctx.file, format!("{} surfaces; exactly four are needed: {}", sf.surface.len(), SURFACE_NAMES.join(", "))));
        }
        let mut surface = Vec::new();
        for (i, s) in sf.surface.iter().enumerate() {
            if s.name != SURFACE_NAMES[i] {
                return Err(ctx.row(i, &s.name, format!("surface {i} must be {:?}", SURFACE_NAMES[i])));
            }
            unit(&ctx, i, &s.name, "texture_density", Some(s.texture_density))?;
            check_conditions(&ctx, i, &s.name, &s.conditions)?;
            surface.push(Surface { name: s.name.clone(), identity: Identity::from_row(&s.identity, "surfaces"), texture_density: s.texture_density, relief: s.relief, conditions: Conditions::from_row(&s.conditions) });
        }
        let d = &sf.density;
        for (f, v) in [("grass_base", d.grass_base), ("grass_per_level", d.grass_per_level), ("cattail", d.cattail)] {
            if !(0.0..=1.0).contains(&v) {
                return Err(AssetError::file(ctx.file, format!("density.{f} {v} is outside 0..1")));
            }
        }
        let surfaces = Surfaces {
            seasons: seasons.try_into().expect("four seasons"),
            surface: surface.try_into().expect("four surfaces"),
            density: Density { grass_base: d.grass_base, grass_per_level: d.grass_per_level, cattail: d.cattail },
        };

        // settings
        let ctx = Ctx { file: TABLES[8], table: "setting" };
        let keys: Vec<&str> = raw.settings.setting.iter().map(|s| s.key.as_str()).collect();
        unique(&ctx, &keys)?;
        for k in REQUIRED_SETTINGS {
            if find(&keys, k).is_none() {
                return Err(AssetError::file(ctx.file, format!("setting {k:?} is missing; the engine needs {}", REQUIRED_SETTINGS.join(", "))));
            }
        }
        let mut settings = Vec::new();
        for (i, s) in raw.settings.setting.iter().enumerate() {
            if s.values.is_empty() {
                return Err(ctx.row(i, &s.key, "values list is empty"));
            }
            if s.default as usize >= s.values.len() {
                return Err(ctx.row(i, &s.key, format!("default {} is outside the {} values", s.default, s.values.len())));
            }
            if s.key == "glyphs" {
                for v in &s.values {
                    if find(&tileset_names, v).is_none() {
                        return Err(ctx.row(i, &s.key, format!("glyph set {v:?} has no tilesets/*.toml")));
                    }
                }
            }
            settings.push(SettingItem { key: s.key.clone(), identity: Identity::from_row(&s.identity, "settings"), label: s.label.clone(), values: s.values.clone(), default: s.default, shortcut: s.shortcut });
        }

        // ui frames, for the game and for the editor's panes: one shape,
        // one loader, a content kind list each.
        let frames = frame_rows(&Ctx { file: TABLES[9], table: "frame" }, &raw.ui.frame, &crate::ui::CONTENT_KINDS, "ui")?;
        let editor_frames = frame_rows(&Ctx { file: TABLES[11], table: "frame" }, &raw.editor_ui.frame, &crate::editor::ui::CONTENT_KINDS, "editor ui")?;

        // tilesets
        if raw.tilesets.is_empty() {
            return Err(AssetError::file("tilesets/", "no tilesets/*.toml found; at least one glyph set is needed"));
        }
        for (i, (path, t)) in raw.tilesets.iter().enumerate() {
            let ctx = Ctx { file: path, table: "tileset" };
            if tileset_names[..i].contains(&t.name.as_str()) {
                return Err(ctx.row(i, &t.name, format!("duplicate tileset name {:?}", t.name)));
            }
            for (field, name) in [("tiny.pine", &t.art.tiny.pine), ("tiny.broadleaf", &t.art.tiny.broadleaf), ("tiny.scrub", &t.art.tiny.scrub), ("tiny.cactus", &t.art.tiny.cactus)] {
                if art.get(name, Tier::Tiny).is_none() {
                    return Err(ctx.row(i, &t.name, format!("art.{field} {name:?} has no tiny tier")));
                }
            }
        }
        let tilesets = raw.tilesets.iter().map(|(_, t)| t.clone()).collect();

        Ok(Assets { biomes, species, materials, props, blocks, creatures, surfaces, lights, settings, frames, editor_frames, styles: tree_styles, tilesets, art, koppen, source, raw, files })
    }

    /// The files this set was loaded from, as (relative path, contents).
    pub fn files(&self) -> &[(String, String)] {
        &self.files
    }

    /// Write the loaded files into a directory as they were read, so an
    /// editing session starts from the embedded set with its comments.
    pub fn export(&self, dir: &Path) -> io::Result<()> {
        for (rel, text) in &self.files {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, text)?;
        }
        Ok(())
    }

    /// The set regenerated from the parsed tables: the same layout as
    /// `assets/`, normalised by the serialiser (comments dropped).
    pub fn to_files(&self) -> Vec<(String, String)> {
        let ser = |v: &dyn erased::Ser| v.to_toml();
        let mut out: Vec<(String, String)> = vec![
            (TABLES[0].to_string(), ser(&self.raw.biomes)),
            (TABLES[1].to_string(), ser(&self.raw.species)),
            (TABLES[2].to_string(), ser(&self.raw.materials)),
            (TABLES[3].to_string(), ser(&self.raw.props)),
            (TABLES[4].to_string(), ser(&self.raw.blocks)),
            (TABLES[5].to_string(), ser(&self.raw.creatures)),
            (TABLES[6].to_string(), ser(&self.raw.surfaces)),
            (TABLES[7].to_string(), ser(&self.raw.lights)),
            (TABLES[8].to_string(), ser(&self.raw.settings)),
            (TABLES[9].to_string(), ser(&self.raw.ui)),
            (TABLES[10].to_string(), ser(&self.raw.tree_styles)),
            (TABLES[11].to_string(), ser(&self.raw.editor_ui)),
        ];
        for (p, t) in &self.raw.tilesets {
            out.push((p.clone(), ser(t)));
        }
        for e in &self.art.entries {
            out.push((e.path.clone(), e.file.to_text()));
        }
        out
    }

    // Lookups by name; each is a linear scan of a short table, meant for
    // set-up and rare events, not per-cell work.

    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.name == name)
    }

    pub fn light(&self, name: &str) -> Option<&LightSpec> {
        self.lights.iter().find(|l| l.name == name)
    }

    pub fn material_index(&self, name: &str) -> Option<usize> {
        self.materials.iter().position(|m| m.name == name)
    }

    pub fn tileset(&self, name: &str) -> Option<&TilesetSpec> {
        self.tilesets.iter().find(|t| t.name == name)
    }

    pub fn setting(&self, key: &str) -> Option<&SettingItem> {
        self.settings.iter().find(|s| s.key == key)
    }

    pub fn frame(&self, name: &str) -> Option<&FrameSpec> {
        self.frames.iter().find(|f| f.name == name)
    }
}

/// Serialise any table file to TOML without naming its type at the call
/// site.
mod erased {
    pub trait Ser {
        fn to_toml(&self) -> String;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_toml(&self) -> String {
            toml::to_string(self).expect("asset tables serialise")
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<io::Result<_>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.extension().is_some_and(|e| e == "toml" || e == "txt") {
            out.push(p);
        }
    }
    Ok(())
}

use crate::map::Terrain;

#[cfg(test)]
pub use tests::shared as test_assets;

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// The embedded set, shared by tests across the crate.
    pub fn shared() -> Rc<Assets> {
        Rc::new(Assets::embedded().expect("embedded assets load"))
    }

    fn with(edit: impl Fn(&str, &str) -> Option<String>) -> Result<Assets, AssetError> {
        let files: Vec<(String, String)> = embedded::FILES.iter().map(|(p, t)| (p.to_string(), edit(p, t).unwrap_or_else(|| t.to_string()))).collect();
        Assets::from_strings(&files)
    }

    fn replace_in(file: &str, from: &str, to: &str) -> Result<Assets, AssetError> {
        with(|p, t| {
            (p == file).then(|| {
                assert!(t.contains(from), "{file} lacks {from:?}");
                t.replacen(from, to, 1)
            })
        })
    }

    #[test]
    fn embedded_set_loads() {
        let a = Assets::embedded().unwrap();
        assert_eq!(a.source, Source::Embedded);
        assert_eq!(a.biomes.len(), 9);
        assert_eq!(a.koppen.len(), 9);
        assert_eq!(a.species.len(), 12);
        assert_eq!(a.materials.len(), 3);
        assert_eq!(a.blocks.len(), 6);
        assert_eq!(a.creatures[0].name, "player");
        assert_eq!(a.tilesets.len(), 2);
        assert_eq!(a.settings.len(), 15);
        assert_eq!(a.frames.len(), 9);
        assert_eq!(a.editor_frames.len(), 8, "the editor's panes are frames too (ADR-005 step 6)");
        let fire = a.light("campfire").unwrap();
        assert!(fire.radius > 0.0 && fire.intensity > 0.0);
        let fire_ix = a.lights.iter().position(|l| l.name == "campfire");
        assert_eq!(a.prop("campfire").unwrap().light, fire_ix);
        for table in TABLES {
            assert!(a.files().iter().any(|(p, _)| p == table), "{table} is among the loaded files");
        }
    }

    #[test]
    fn every_cross_reference_resolves() {
        let a = Assets::embedded().unwrap();
        for b in &a.biomes {
            assert!(b.material < a.materials.len(), "{}", b.name);
            assert_eq!(a.koppen[&b.koppen], a.biomes.iter().position(|x| x.name == b.name).unwrap());
            for &(sp, w) in &b.species {
                assert!(sp < a.species.len() && w > 0, "{}", b.name);
            }
        }
        for s in &a.species {
            assert!(s.light.is_none_or(|l| l < a.lights.len()), "{}", s.name);
        }
        for p in &a.props {
            assert!(a.art.has(&p.art), "{}", p.name);
            assert!(p.light.is_none_or(|l| l < a.lights.len()));
            assert!(!p.terrain.is_empty(), "{} stands on some terrain", p.name);
            assert!(p.cover.iter().all(|c| crate::biome::COVERS.contains(c)), "{}", p.name);
        }
        for k in &a.blocks {
            if let MaterialRule::Fixed(m) = k.material {
                assert!(m < a.materials.len(), "{}", k.name);
            }
            assert!(k.light.is_none_or(|l| l < a.lights.len()));
            assert!(!k.terrain.is_empty(), "{} stands on some terrain", k.name);
        }
        for c in &a.creatures {
            assert!(a.art.has(&c.art), "{}", c.name);
            assert!(!c.can_enter.is_empty(), "{} can enter some terrain", c.name);
            assert!(c.home.is_none_or(|h| h < a.blocks.len()), "{}", c.name);
            assert!(c.light.is_none_or(|l| l < a.lights.len()), "{}", c.name);
        }
        for t in &a.tilesets {
            for name in [&t.art.tiny.pine, &t.art.tiny.broadleaf, &t.art.tiny.scrub, &t.art.tiny.cactus] {
                assert!(a.art.get(name, Tier::Tiny).is_some(), "{}: {name}", t.name);
            }
        }
        for s in a.setting("glyphs").unwrap().values.iter() {
            assert!(a.tileset(s).is_some(), "{s}");
        }
        assert!(a.material_index("stone").is_some());
        assert!(a.block("house").is_some());
    }

    #[test]
    fn every_placeable_row_is_described_and_categorised() {
        let a = Assets::embedded().unwrap();
        let mut rows: Vec<(&str, &Identity)> = Vec::new();
        rows.extend(a.species.iter().map(|s| (s.name.as_str(), &s.identity)));
        rows.extend(a.props.iter().map(|p| (p.name.as_str(), &p.identity)));
        rows.extend(a.blocks.iter().map(|b| (b.name.as_str(), &b.identity)));
        rows.extend(a.creatures.iter().map(|c| (c.name.as_str(), &c.identity)));
        rows.extend(a.lights.iter().map(|l| (l.name.as_str(), &l.identity)));
        assert!(rows.len() > 20);
        for (name, id) in rows {
            assert!(!id.description.trim().is_empty(), "{name} has no description");
            assert!(!id.category.trim().is_empty(), "{name} has no category");
        }
        // Categories default to the table name, so a row without one still has one.
        let plain = Identity::from_row(&IdentityRow { description: "x".into(), category: None, aliases: vec![] }, "props");
        assert_eq!(plain.category, "props");
    }

    #[test]
    fn numeric_ranges_hold_across_the_tables() {
        let a = Assets::embedded().unwrap();
        let unit = |v: f32| (0.0..=1.0).contains(&v);
        for b in &a.biomes {
            assert!(unit(b.tree_density), "{}: tree_density {}", b.name, b.tree_density);
            assert!(b.grass <= 3, "{}: grass {}", b.name, b.grass);
        }
        for p in &a.props {
            assert!(unit(p.density), "{}: density {}", p.name, p.density);
            assert!(p.hooks.reach >= 0.0 && p.physical.spacing >= 0.0, "{}", p.name);
            assert!(unit(p.physical.snow_cover) && unit(p.physical.sway) && unit(p.physical.heat) && unit(p.physical.cluster), "{}", p.name);
            assert!((p.min_zoom as usize) < crate::tileset::ZOOMS.len(), "{}: min_zoom {}", p.name, p.min_zoom);
        }
        for s in &a.species {
            assert!(s.radius.is_none_or(|r| r >= 0.0) && s.height.is_none_or(|h| h >= 0.0), "{}", s.name);
            assert!(s.hooks.reach >= 0.0 && unit(s.physical.sway), "{}", s.name);
            assert!(unit(s.conditions.wet_darkening) && unit(s.conditions.dry_fading), "{}", s.name);
            assert_eq!(s.canopy.len(), 4, "{}: a canopy table has four seasons", s.name);
            assert_eq!(s.canopy_glyph.len(), 4);
        }
        for k in &a.blocks {
            assert!(unit(k.settle_min) && k.chance <= 100, "{}: settle_min {} chance {}", k.name, k.settle_min, k.chance);
        }
        for c in &a.creatures {
            assert!(c.speed >= 0.0 && c.sight >= 0.0 && c.spacing >= 0.0 && c.hooks.reach >= 0.0, "{}", c.name);
        }
        for l in &a.lights {
            assert!(l.radius > 0.0 && l.intensity >= 0.0 && l.falloff > 0.0, "{}", l.name);
            assert!(unit(l.flicker_amount) && l.flicker_rate >= 0.0, "{}", l.name);
            assert!(l.color.iter().all(|&c| unit(c)), "{}: colour {:?} is outside 0..1", l.name, l.color);
        }
        for s in &a.surfaces.surface {
            assert!(unit(s.texture_density) && unit(s.conditions.wet_darkening), "{}", s.name);
        }
        let d = a.surfaces.density;
        assert!(unit(d.grass_base) && unit(d.grass_per_level) && unit(d.cattail));
        assert_eq!(a.raw.surfaces.season.len(), 4, "the palette file carries four seasons");
        assert_eq!(a.surfaces.seasons.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), SEASON_NAMES);
        assert_eq!(a.raw.surfaces.surface.len(), 4);
        for s in &a.raw.species.species {
            assert_eq!(s.canopy.expand().len(), 4, "{}", s.name);
        }
    }

    #[test]
    fn every_distance_in_the_tables_is_metres() {
        // The yardsticks of ADR-004: a tile is 2 m, the person 2 m, a house
        // level 3 m, an oak 18 m, and light radii are metres too (a
        // campfire reaches four tiles).
        let a = Assets::embedded().unwrap();
        assert_eq!(crate::map::TILE_METRES, 2.0);
        assert_eq!(a.creatures[0].name, "player");
        assert_eq!(a.creatures[0].size, [0.6, 0.4, 2.0], "the person is two metres tall");
        assert_eq!(a.block("house").unwrap().level_height, 3.0, "a house level is three metres");
        assert_eq!(a.species.iter().find(|s| s.name == "oak").unwrap().size[2], 18.0);
        assert_eq!(a.light("campfire").unwrap().radius, 8.0, "four tiles of throw, in metres");
        // Plausible metre ranges, so a row in tiles or rows stands out.
        for c in &a.creatures {
            assert!((0.1..=6.0).contains(&c.size[2]), "{}: {} m tall", c.name, c.size[2]);
            assert!(c.size[0] > 0.0 && c.size[1] > 0.0, "{}", c.name);
            assert!(c.sight <= 200.0 && c.speed <= 30.0, "{}: sight and speed are metres", c.name);
        }
        for sp in &a.species {
            assert!((0.5..=40.0).contains(&sp.size[2]), "{}: {} m tall", sp.name, sp.size[2]);
            let (_, d) = sp.volume();
            assert!(d.radius <= 20.0 && d.height <= 40.0, "{}: crown in metres", sp.name);
        }
        for b in &a.blocks {
            assert!((0.05..=10.0).contains(&b.level_height), "{}: {} m a level", b.name, b.level_height);
            assert!(b.max_rise <= 10.0 && b.window_pitch <= 10.0, "{}: roof and windows in metres", b.name);
        }
        for p in &a.props {
            assert!((0.01..=5.0).contains(&p.size[2]), "{}: {} m tall", p.name, p.size[2]);
            assert!(p.hooks.reach <= 20.0, "{}: reach in metres", p.name);
        }
        for l in &a.lights {
            assert!((0.5..=100.0).contains(&l.radius), "{}: {} m of throw", l.name, l.radius);
        }
    }

    #[test]
    fn weighted_species_lists_back_every_wooded_biome() {
        let a = Assets::embedded().unwrap();
        for b in &a.biomes {
            if b.tree_density > 0.0 {
                assert!(!b.species.is_empty(), "{} has trees but no species to draw", b.name);
                assert!(b.species.iter().all(|&(_, w)| w > 0), "{} has a zero-weight species", b.name);
                let total: u32 = b.species.iter().map(|&(_, w)| w as u32).sum();
                for roll in 0..total as u64 {
                    let pick = crate::biome::weighted_pick(&b.species, roll);
                    assert!(b.species.iter().any(|&(ix, _)| ix == pick), "{}: pick {pick} is not in the list", b.name);
                }
            }
        }
        assert!(a.biomes.iter().any(|b| b.tree_density == 0.0 && b.species.is_empty()), "the ice cap has neither");
    }

    #[test]
    fn errors_name_file_line_and_row() {
        let e = replace_in("species.toml", "form = \"scrub\"", "form = \"bush\"").unwrap_err();
        assert_eq!(e.file, "species.toml");
        assert!(e.line.is_some());
        assert_eq!(e.row, Some(("species".to_string(), 4, "juniper".to_string())));
        assert!(e.msg.contains("bush"), "{e}");
        assert!(e.to_string().starts_with("species.toml:"), "{e}");

        let e = replace_in("biomes.toml", "material = \"adobe\"", "material = \"brick\"").unwrap_err();
        assert_eq!(e.row, Some(("biome".to_string(), 1, "savanna".to_string())));
        assert_eq!(e.msg, "unknown material \"brick\"");
        assert_eq!(e.to_string(), "biomes.toml: biome[1] \"savanna\": unknown material \"brick\"");

        let e = replace_in("props.toml", "art = \"reeds\"", "art = \"weeds\"").unwrap_err();
        assert_eq!(e.row, Some(("prop".to_string(), 3, "reeds".to_string())));

        let e = replace_in("props.toml", "description = \"Reeds along the waterline on sand or grass.\"", "description = \"\"").unwrap_err();
        assert_eq!(e.msg, "missing description");

        let e = replace_in("lights.toml", "name = \"campfire\"", "name = \"hearth\"").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("campfire"));
        assert!(e.msg.contains("unknown light"), "{e}");

        let e = replace_in("settings.toml", "key = \"clock\"", "key = \"timer\"").unwrap_err();
        assert!(e.msg.contains("\"clock\" is missing"), "{e}");

        let e = replace_in("ui.toml", "content = \"worldmap\"", "content = \"atlas\"").unwrap_err();
        assert_eq!(e.row, Some(("frame".to_string(), 3, "worldmap".to_string())));
        assert!(e.msg.contains("unknown content kind \"atlas\""), "{e}");

        let e = replace_in("ui.toml", "key = \"tab\"", "key = \"shift-f4\"").unwrap_err();
        assert!(e.msg.contains("unknown key"), "{e}");

        let e = replace_in("ui.toml", "anchor = \"full\"", "anchor = \"middle\"").unwrap_err();
        assert_eq!(e.file, "ui.toml");
        assert!(e.msg.contains("middle"), "{e}");

        let e = replace_in("ui.toml", "name = \"stats\"", "name = \"inventory\"").unwrap_err();
        assert!(e.msg.contains("duplicate name"), "{e}");

        // The editor's table goes through the same loader over its own
        // content kinds, so a game kind is unknown there and the other way
        // about.
        let e = replace_in("editor-ui.toml", "content = \"strip\"", "content = \"worldmap\"").unwrap_err();
        assert_eq!(e.file, "editor-ui.toml");
        assert!(e.msg.contains("unknown content kind \"worldmap\""), "{e}");

        let e = replace_in("ui.toml", "content = \"worldmap\"", "content = \"strip\"").unwrap_err();
        assert!(e.msg.contains("unknown content kind \"strip\""), "{e}");

        let e = replace_in("editor-ui.toml", "margin = { left = 20, bottom = 1 }", "margin = { left = -20 }").unwrap_err();
        assert!(e.msg.contains("margin is negative"), "{e}");

        let e = replace_in("art/player/tiny.txt", "@", "\t@").unwrap_err();
        assert_eq!((e.file.as_str(), e.line), ("art/player/tiny.txt", Some(2)));

        let e = with(|p, t| (p == "art/player/small.txt").then(|| t.replace("tier=small", "tier=tiny"))).unwrap_err();
        assert!(e.msg.contains("already defined"), "{e}");

        let e = replace_in("props.toml", "passable = false", "passable = false\nsnow_cover = 1.5").unwrap_err();
        assert!(e.msg.contains("snow_cover"), "{e}");

        let e = replace_in("props.toml", "passable = false", "passable = false\n[prop.condition_colors]\nsoggy = [1, 2, 3]").unwrap_err();
        assert!(e.msg.contains("unknown condition"), "{e}");

        let files: Vec<(String, String)> = embedded::FILES.iter().filter(|(p, _)| *p != "lights.toml").map(|(p, t)| (p.to_string(), t.to_string())).collect();
        let e = Assets::from_strings(&files).unwrap_err();
        assert_eq!(e.to_string(), "lights.toml: missing");
    }

    /// A plain species standing in for whatever habit is being checked.
    fn styled(a: &Assets, style: &str, size: [f32; 3]) -> Species {
        let mut s = a.species.iter().find(|s| s.name == "gnarled oak").unwrap().clone();
        s.size = size;
        s.lsystem = Some(a.styles.iter().find(|st| st.name == style).unwrap().grammar(&Overrides::default()).unwrap());
        s
    }

    #[test]
    fn every_growth_habit_grows_a_tree() {
        let a = Assets::embedded().unwrap();
        assert_eq!(a.styles.len(), 8, "the eight habits are in the set");
        for st in &a.styles {
            assert!(!st.identity.description.trim().is_empty(), "{}", st.name);
            let sp = styled(&a, &st.name, [8.0, 8.0, 16.0]);
            for seed in [0u64, 4, 91] {
                let m = sp.tree_model(seed, crate::lsystem::Growth::FULL).unwrap();
                assert!(!m.segments.is_empty(), "{} at seed {seed} has branches", st.name);
                assert!(!m.leaves.is_empty(), "{} at seed {seed} has leaf clusters", st.name);
                assert!((m.bounds.max[2] - 16.0).abs() / 16.0 < 0.05, "{} stands {} m", st.name, m.bounds.max[2]);
            }
            // A habit that never dies is no use either.
            let dead = sp.tree_model(4, crate::lsystem::Growth::DEAD).unwrap();
            assert!(!dead.segments.is_empty() && dead.leaves.is_empty(), "{}", st.name);
        }
    }

    #[test]
    fn each_habit_has_its_own_signature() {
        let a = Assets::embedded().unwrap();
        let model = |style: &str, size: [f32; 3]| styled(&a, style, size).tree_model(11, crate::lsystem::Growth::FULL).unwrap();

        // Excurrent: tiers of branches up the leader, shortening as they rise.
        let spruce = model("excurrent", [7.0, 7.0, 24.0]);
        let mut tiers: Vec<(f32, f32)> = Vec::new();
        for s in &spruce.segments {
            let reach = (s.b[0] * s.b[0] + s.b[1] * s.b[1]).sqrt();
            let band = (s.b[2] / 3.0).floor();
            match tiers.iter_mut().find(|(z, _)| *z == band) {
                Some((_, r)) => *r = r.max(reach),
                None => tiers.push((band, reach)),
            }
        }
        tiers.sort_by(|x, y| x.0.total_cmp(&y.0));
        assert!(tiers.len() >= 4, "the leader carries several tiers: {}", tiers.len());
        let (low, high) = (tiers[1].1, tiers[tiers.len() - 1].1);
        assert!(high < low * 0.75, "the tiers narrow upward: {low} at the bottom, {high} at the top");

        // Weeping: the foliage hangs, so it sits lower in the tree than a
        // spreading crown of the same box does.
        let height = |m: &crate::lsystem::TreeModel| m.leaves.iter().map(|l| l.centre[2]).sum::<f32>() / m.leaves.len().max(1) as f32 / m.bounds.max[2];
        let willow = height(&model("weeping", [14.0, 14.0, 12.0]));
        let oak = height(&model("decurrent", [14.0, 14.0, 12.0]));
        assert!(willow < oak, "a weeping crown hangs below a spreading one: {willow} against {oak}");
        assert!(willow < 0.65, "and below the middle of the tree: {willow}");

        // Shrub: several stems leave the ground, and there is no trunk.
        let bush = model("shrub", [3.0, 3.0, 2.0]);
        let stems = bush.segments.iter().filter(|s| s.a[2] < 0.2).count();
        assert!(stems > 1, "a shrub has more than one stem at the ground: {stems}");

        // Umbrella: nothing leafy under the pruned trunk.
        let acacia = model("umbrella", [12.0, 12.0, 14.0]);
        let prune = a.styles.iter().find(|s| s.name == "umbrella").unwrap().params.prune_height;
        let lowest = acacia.leaves.iter().map(|l| l.centre[2]).fold(f32::MAX, f32::min);
        assert!(lowest > prune * 14.0 * 0.8, "the crown sits above the bare trunk: {lowest} m of 14");

        // Palm: a bare stem, and every cluster in the head.
        let palm = model("palm", [6.0, 6.0, 12.0]);
        let lowest = palm.leaves.iter().map(|l| l.centre[2]).fold(f32::MAX, f32::min);
        assert!(lowest > 6.0, "a palm's fronds are all at its head: {lowest} m of 12");

        // Columnar: narrower than it is tall for the same declared box.
        let poplar = model("columnar", [10.0, 10.0, 20.0]);
        let widest = poplar.leaves.iter().map(|l| (l.centre[0] * l.centre[0] + l.centre[1] * l.centre[1]).sqrt()).fold(0.0f32, f32::max);
        let tallest = poplar.bounds.max[2];
        assert!(widest * 2.0 < tallest, "a columnar crown is narrow: {widest} m across against {tallest} m tall");
    }

    #[test]
    fn a_style_stands_in_for_a_canopy_volume() {
        use crate::volume::Shape;
        let a = Assets::embedded().unwrap();
        let of = |name: &str| a.styles.iter().find(|s| s.name == name).unwrap().stand_in();
        assert_eq!(of("excurrent"), Shape::Cone);
        assert_eq!(of("decurrent"), Shape::Ellipsoid);
        assert_eq!(of("weeping"), Shape::Dome);
        assert_eq!(of("shrub"), Shape::Dome);
        for st in &a.styles {
            assert_ne!(st.stand_in(), Shape::Lsystem, "{} stands in as a volume, not as itself", st.name);
        }
    }

    #[test]
    fn every_lsystem_species_grows_a_tree() {
        let a = Assets::embedded().unwrap();
        let grown: Vec<&Species> = a.species.iter().filter(|s| s.lsystem.is_some()).collect();
        assert_eq!(grown.len(), a.species.len() - 1, "every species but the saguaro grows from a habit");
        for s in &grown {
            assert_eq!(s.shape, Some(Shape::Lsystem), "{}", s.name);
            for seed in [0u64, 3, 77] {
                let m = s.tree_model(seed, crate::lsystem::Growth::FULL).unwrap();
                assert!(!m.segments.is_empty(), "{} at seed {seed} has branches", s.name);
                assert!(!m.leaves.is_empty(), "{} at seed {seed} has leaf clusters", s.name);
                assert!((m.bounds.max[2] - s.size[2]).abs() / s.size[2] < 0.05, "{} stands {} m against {}", s.name, m.bounds.max[2], s.size[2]);
                assert!((m.bounds.size()[0] - s.size[0]).abs() / s.size[0] < 0.05, "{} spreads {} m against {}", s.name, m.bounds.size()[0], s.size[0]);
                // The thickest branch is the trunk the row asks for.
                let thickest = m.segments.iter().fold(0.0f32, |t, g| t.max(g.radius));
                assert!((thickest - s.volume().1.trunk_radius).abs() < 1e-3, "{}", s.name);
            }
            // Deadwood keeps its branches and drops every leaf.
            let dead = s.tree_model(5, crate::lsystem::Growth::DEAD).unwrap();
            assert!(!dead.segments.is_empty() && dead.leaves.is_empty(), "{}", s.name);
        }
        // Every tree a biome plants is grown from a habit, and its stand-in
        // is the volume it reads as from far away.
        for b in &a.biomes {
            for &(sp, _) in &b.species {
                let s = &a.species[sp];
                if s.shape == Some(Shape::Cactus) {
                    continue;
                }
                assert!(s.lsystem.is_some(), "{} plants {}, which grows from no habit", b.name, s.name);
                assert!(s.stand_in.is_some_and(|k| k != Shape::Lsystem), "{} has no stand-in volume", s.name);
                assert_eq!(s.volume().0, s.stand_in.unwrap(), "{} reads as its stand-in from far away", s.name);
            }
        }
    }

    #[test]
    fn a_dead_roll_is_the_same_every_time_for_a_seed() {
        let a = Assets::embedded().unwrap();
        let oak = a.species.iter().find(|s| s.name == "gnarled oak").unwrap();
        assert_eq!(oak.dead_chance, 0.05);
        let rolls: Vec<bool> = (0..400u64).map(|s| oak.dead(s)).collect();
        for (seed, want) in rolls.iter().enumerate() {
            assert_eq!(oak.dead(seed as u64), *want, "seed {seed}");
        }
        let dead = rolls.iter().filter(|d| **d).count();
        assert!((4..=40).contains(&dead), "about a twentieth of 400 stand dead, got {dead}");
        // A row that says nothing takes the default.
        assert_eq!(a.species.iter().find(|s| s.name == "oak").unwrap().dead_chance, 0.02);
    }

    #[test]
    fn lsystem_rows_are_validated() {
        // A habit and the shape imply each other.
        let e = replace_in("species.toml", "shape = \"lsystem\"\nstyle = \"decurrent\"", "shape = \"ellipsoid\"\nstyle = \"decurrent\"").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("oak"));
        assert!(e.msg.contains("shape = \"lsystem\""), "{e}");
        let e = replace_in("species.toml", "style = \"decurrent\"\n", "").unwrap_err();
        assert!(e.msg.contains("style") || e.msg.contains("axiom"), "{e}");
        let e = replace_in("species.toml", "style = \"decurrent\"", "style = \"bonsai\"").unwrap_err();
        assert!(e.msg.contains("bonsai") && e.msg.contains("decurrent"), "{e}");
        // The parameters are checked, and the error names the row.
        let e = replace_in("species.toml", "asymmetry = 0.3", "asymmetry = 3.0").unwrap_err();
        assert!(e.msg.contains("asymmetry"), "{e}");
        let e = replace_in("species.toml", "depth = 7", "depth = 40").unwrap_err();
        assert!(e.msg.contains("depth"), "{e}");
        let e = replace_in("species.toml", "prune_height = 0.1", "prune_height = 1.0").unwrap_err();
        assert!(e.msg.contains("prune_height"), "{e}");
        let e = replace_in("species.toml", "dead_chance = 0.05", "dead_chance = 5.0").unwrap_err();
        assert!(e.msg.contains("dead_chance"), "{e}");
        // And so are the habits themselves.
        let e = replace_in("tree_styles.toml", "axiom = \"F!A\"", "axiom = \"F[!A\"").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("decurrent"));
        assert!(e.msg.contains("open"), "{e}");
        let e = replace_in("tree_styles.toml", "stand_in = \"cone\"", "stand_in = \"pyramid\"").unwrap_err();
        assert!(e.msg.contains("pyramid"), "{e}");
        let e = replace_in("tree_styles.toml", "(/)*spread)*forks", "(/)*spread)*width").unwrap_err();
        assert!(e.msg.contains("width"), "{e}");
        let e = replace_in("tree_styles.toml", "forks = 5", "forks = 0").unwrap_err();
        assert!(e.msg.contains("forks"), "{e}");
    }

    #[test]
    fn geometry_rows_are_validated() {
        // Sizes are metres and must be positive in every dimension.
        let e = replace_in("species.toml", "size = [10.0, 10.0, 18.0]", "size = [10.0, 0.0, 18.0]").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("oak"));
        assert!(e.msg.contains("size") && e.msg.contains("metres"), "{e}");
        let e = replace_in("blocks.toml", "size = [2.0, 2.0, 3.0]", "size = [2.0, 2.0, -3.0]").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("house"));
        let e = replace_in("props.toml", "size = [1.2, 1.0, 0.8]", "size = [1.2, 1.0, 0.0]").unwrap_err();
        assert_eq!(e.row.as_ref().map(|r| r.2.as_str()), Some("boulder"));
        // A size is required.
        let e = replace_in("species.toml", "size = [10.0, 10.0, 18.0]\n", "").unwrap_err();
        assert!(e.msg.contains("size"), "{e}");
        // Volume overrides must be positive; a zero crown is no crown.
        let e = replace_in("species.toml", "shape = \"cactus\"", "shape = \"cactus\"\nradius = 0.0").unwrap_err();
        assert!(e.msg.contains("radius"), "{e}");
        let e = replace_in("species.toml", "shape = \"cactus\"", "shape = \"blob\"").unwrap_err();
        assert!(e.msg.contains("blob"), "{e}");
        // Blocks: levels rise and stay within max_levels; the window band is
        // a rising pair within a level; roof names are checked.
        let e = replace_in("blocks.toml", "levels = [1, 2]", "levels = [2, 1]").unwrap_err();
        assert!(e.msg.contains("levels"), "{e}");
        let e = replace_in("blocks.toml", "levels = [1, 2]", "levels = [1, 4]").unwrap_err();
        assert!(e.msg.contains("max_levels"), "{e}");
        let e = replace_in("blocks.toml", "windows = [0.5, 1.0]", "windows = [0.9, 0.2]").unwrap_err();
        assert!(e.msg.contains("windows"), "{e}");
        let e = replace_in("blocks.toml", "windows = [0.5, 1.0]", "windows = [0.5]").unwrap_err();
        assert!(e.msg.contains("windows"), "{e}");
        let e = replace_in("blocks.toml", "roof = \"gable\"", "roof = \"dome\"").unwrap_err();
        assert!(e.msg.contains("dome"), "{e}");
        let e = replace_in("blocks.toml", "window_pitch = 1.0", "window_pitch = 0.0").unwrap_err();
        assert!(e.msg.contains("window_pitch"), "{e}");
        let e = replace_in("blocks.toml", "chance = 45", "chance = 450").unwrap_err();
        assert!(e.msg.contains("percentage"), "{e}");
        // A footprint is a rising pair of tile sizes.
        let e = replace_in("blocks.toml", "footprint = [[3, 2], [5, 3]]", "footprint = [[5, 2], [3, 3]]").unwrap_err();
        assert!(e.msg.contains("footprint"), "{e}");
        let e = replace_in("blocks.toml", "footprint = [[3, 2], [5, 3]]", "footprint = [[3, 2], [20, 3]]").unwrap_err();
        assert!(e.msg.contains("plot"), "{e}");
        // Windows may be turned off, and level_height defaults to the size.
        let a = replace_in("blocks.toml", "windows = [0.5, 1.0]", "windows = []").unwrap();
        assert_eq!(a.block("house").unwrap().windows, None);
        assert_eq!(a.block("house").unwrap().level_height, 3.0);
        let a = replace_in("blocks.toml", "size = [2.0, 2.0, 3.0]", "size = [2.0, 2.0, 3.0]\nlevel_height = 2.5").unwrap();
        assert_eq!(a.block("house").unwrap().level_height, 2.5);
    }

    #[test]
    fn species_volumes_come_from_their_size_by_shape() {
        let a = Assets::embedded().unwrap();
        let oak = a.species.iter().find(|s| s.name == "oak").unwrap();
        let (shape, d) = oak.volume();
        assert_eq!(shape, crate::volume::Shape::Ellipsoid);
        assert_eq!(d.radius, 5.0, "half the spread");
        assert!((d.height + d.trunk - 18.0).abs() < 1e-5, "crown and trunk make the height");
        assert!(d.trunk_radius > 0.0);
        let bush = a.species.iter().find(|s| s.name == "juniper").unwrap();
        let (shape, d) = bush.volume();
        assert_eq!((shape, d.trunk), (crate::volume::Shape::Dome, 0.0), "a bush sits on the ground");
        assert_eq!(d.height, 2.5);
        for s in &a.species {
            let (_, d) = s.volume();
            assert!(d.radius > 0.0 && d.height > 0.0, "{}", s.name);
        }
    }

    #[test]
    fn missing_directory_is_an_error_naming_it() {
        let e = Assets::from_dir(Path::new("/nonexistent/roguemap-assets")).unwrap_err();
        assert_eq!(e.file, "/nonexistent/roguemap-assets");
        assert!(e.msg.contains("does not exist"), "{e}");
    }

    #[test]
    fn tier_fallback_per_zoom() {
        let a = Assets::embedded().unwrap();
        let rows = |zoom| a.art.for_zoom("player", zoom).unwrap().rows.len();
        assert_eq!([rows(0), rows(1), rows(2), rows(3)], [1, 3, 6, 12], "one tier per zoom, far to close");
        // The boulder has a small tier and a large one: the zooms between
        // fall back to the small.
        let boulder = |zoom| a.art.for_zoom("boulder", zoom).unwrap().rows.len();
        assert_eq!([boulder(0), boulder(1), boulder(2), boulder(3)], [1, 1, 1, 2]);
        assert!(a.art.get("boulder", Tier::Medium).is_none());
        // Tiny tiers only: every zoom gets the tiny sprite.
        assert!(a.art.for_zoom("petscii/pine", 3).is_some());
        assert!(a.art.for_zoom("nothing", 0).is_none());
    }

    #[test]
    fn art_tiers_are_picked_by_the_rows_a_thing_stands() {
        // The person is 2 m: 12 rows at 1:1, 6 at 1:2, 3 at 1:4 and 1.5 at
        // 1:8, so the tier nearest each is large, medium, small and tiny
        // (ADR-004).
        let a = Assets::embedded().unwrap();
        let rows = |r: f32| a.art.for_rows("player", r).unwrap().rows.len();
        assert_eq!([rows(12.0), rows(6.0), rows(3.0), rows(1.5)], [12, 6, 3, 1], "the person's own rows at every zoom");
        assert_eq!(rows(0.2), 1, "smaller than any tier is the smallest");
        assert_eq!(rows(40.0), 12, "taller than any tier is the largest");
        assert!(a.art.for_rows("nothing", 3.0).is_none());
    }

    #[test]
    fn asset_dir_loads_identically_to_embedded() {
        let a = Assets::embedded().unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let b = Assets::from_dir(&dir).unwrap();
        assert_eq!(b.source, Source::Dir(dir));
        assert_same(&a, &b);
    }

    fn assert_same(a: &Assets, b: &Assets) {
        assert_eq!(a.biomes, b.biomes);
        assert_eq!(a.species, b.species);
        assert_eq!(a.materials, b.materials);
        assert_eq!(a.props, b.props);
        assert_eq!(a.blocks, b.blocks);
        assert_eq!(a.creatures, b.creatures);
        assert_eq!(a.surfaces, b.surfaces);
        assert_eq!(a.lights, b.lights);
        assert_eq!(a.settings, b.settings);
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.tilesets, b.tilesets);
        assert_eq!(a.art, b.art);
        assert_eq!(a.koppen, b.koppen);
    }

    #[test]
    fn export_then_from_dir_round_trips() {
        let a = Assets::embedded().unwrap();
        let dir = std::env::temp_dir().join(format!("roguemap-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        a.export(&dir).unwrap();
        let b = Assets::from_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_same(&a, &b);
    }

    #[test]
    fn serialised_tables_round_trip() {
        // Through the serialiser rather than the stored text, with a
        // condition colour table to exercise the nested case.
        let a = replace_in("props.toml", "passable = false", "passable = false\n[prop.condition_colors]\nwet = [1, 2, 3]").unwrap();
        assert_eq!(a.props[0].conditions.condition_colors["wet"], crate::canvas::Rgb(1, 2, 3));
        let files = a.to_files();
        let b = Assets::from_strings(&files).unwrap();
        assert_same(&a, &b);
        assert_eq!(b.to_files(), files);
    }

    /// Regenerate `assets/` from the embedded set, normalised by the
    /// serialiser. Run with `cargo test export_tables -- --ignored`.
    #[test]
    #[ignore]
    fn export_tables() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (path, text) in Assets::embedded().unwrap().to_files() {
            let p = root.join(&path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
    }
}
