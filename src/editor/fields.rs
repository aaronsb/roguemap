//! Field descriptors for every table (ADR-003): name, kind and whether the
//! row must carry it. They drive the row form, choose the editor a field
//! opens in, format values for display, and parse typed text on commit.
//! Nested TOML tables are addressed by dotted names (`roles.cover.grass`).

use crate::assets::schema::CONDITIONS;
use crate::assets::TABLES;

/// The tables the left pane lists. All but `Art` are TOML row tables.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableKind {
    Biomes,
    Species,
    Materials,
    Props,
    Blocks,
    Creatures,
    /// `surfaces.toml` `[[season]]`.
    Seasons,
    /// `surfaces.toml` `[[surface]]`.
    Surfaces,
    /// `surfaces.toml` `[density]`, one row.
    Density,
    Lights,
    Settings,
    /// One row per `tilesets/*.toml`.
    Tilesets,
    /// One row per art file; edited as a grid.
    Art,
}

pub const TABLE_KINDS: [TableKind; 13] = [
    TableKind::Biomes,
    TableKind::Species,
    TableKind::Materials,
    TableKind::Props,
    TableKind::Blocks,
    TableKind::Creatures,
    TableKind::Seasons,
    TableKind::Surfaces,
    TableKind::Density,
    TableKind::Lights,
    TableKind::Settings,
    TableKind::Tilesets,
    TableKind::Art,
];

impl TableKind {
    pub fn name(self) -> &'static str {
        match self {
            TableKind::Biomes => "biomes",
            TableKind::Species => "species",
            TableKind::Materials => "materials",
            TableKind::Props => "props",
            TableKind::Blocks => "blocks",
            TableKind::Creatures => "creatures",
            TableKind::Seasons => "seasons",
            TableKind::Surfaces => "surfaces",
            TableKind::Density => "density",
            TableKind::Lights => "lights",
            TableKind::Settings => "settings",
            TableKind::Tilesets => "tilesets",
            TableKind::Art => "art",
        }
    }

    /// The table file and the array-of-tables key inside it, for the row
    /// tables; `None` for tilesets and art, which are one row per file.
    pub fn file_and_key(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            TableKind::Biomes => (TABLES[0], "biome"),
            TableKind::Species => (TABLES[1], "species"),
            TableKind::Materials => (TABLES[2], "material"),
            TableKind::Props => (TABLES[3], "prop"),
            TableKind::Blocks => (TABLES[4], "block"),
            TableKind::Creatures => (TABLES[5], "creature"),
            TableKind::Seasons => (TABLES[6], "season"),
            TableKind::Surfaces => (TABLES[6], "surface"),
            TableKind::Density => (TABLES[6], "density"),
            TableKind::Lights => (TABLES[7], "light"),
            TableKind::Settings => (TABLES[8], "setting"),
            TableKind::Tilesets | TableKind::Art => return None,
        })
    }

    /// The field that names a row.
    pub fn name_field(self) -> &'static str {
        match self {
            TableKind::Settings => "key",
            _ => "name",
        }
    }

    /// Whether rows may be added and removed: the array tables whose
    /// length the engine does not fix.
    pub fn resizable(self) -> bool {
        !matches!(self, TableKind::Seasons | TableKind::Surfaces | TableKind::Density | TableKind::Tilesets | TableKind::Art)
    }
}

/// What a field holds, which decides its editor and its checks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Str,
    /// A sentence or two; the same editor as `Str`, shown wider.
    Text,
    Bool,
    U8 { max: u8 },
    U64,
    F32 { min: f32, max: f32 },
    Enum(&'static [&'static str]),
    /// The `name` of a row in another table, plus fixed extra choices.
    Ref { table: TableKind, extra: &'static [&'static str] },
    Rgb,
    /// One `[r, g, b]` or four, in season order.
    Seasonal,
    /// `[r, g, b]` as 0..1 floats.
    Unit3,
    EnumList(&'static [&'static str]),
    StrList,
    /// `[[name, weight], ...]` into another table.
    Pairs(TableKind),
    Glyph,
    /// An array of one-character strings of a fixed length.
    GlyphList(usize),
    /// Any TOML value, edited as text.
    Any,
}

#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// Dotted path into the row table.
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
}

const fn req(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: true }
}

const fn opt(name: &'static str, kind: Kind) -> Field {
    Field { name, kind, required: false }
}

pub const TERRAINS: [&str; 6] = ["water", "sand", "grass", "dirt", "rock", "snow"];
pub const COVERS: [&str; 4] = ["grass", "dry", "moss", "bare"];
pub const FORMS: [&str; 4] = ["pine", "broadleaf", "scrub", "cactus"];
pub const SIZE_CLASSES: [&str; 3] = ["small", "mixed", "large"];
pub const BEHAVIOURS: [&str; 6] = ["idle", "wander", "graze", "flee", "hunt", "patrol"];

const UNIT: Kind = Kind::F32 { min: 0.0, max: 1.0 };
const NON_NEG: Kind = Kind::F32 { min: 0.0, max: f32::INFINITY };
const ANY_F: Kind = Kind::F32 { min: f32::NEG_INFINITY, max: f32::INFINITY };

const IDENTITY: [Field; 3] = [opt("description", Kind::Text), opt("category", Kind::Str), opt("aliases", Kind::StrList)];

const HOOKS: [Field; 5] = [
    opt("light", Kind::Ref { table: TableKind::Lights, extra: &[] }),
    opt("tags", Kind::StrList),
    opt("emits", Kind::StrList),
    opt("affects", Kind::StrList),
    opt("reach", NON_NEG),
];

const PHYSICAL: [Field; 16] = [
    opt("passable", Kind::Bool),
    opt("blocks_sight", Kind::Bool),
    opt("snow_cover", UNIT),
    opt("min_height", ANY_F),
    opt("max_height", ANY_F),
    opt("max_slope", NON_NEG),
    opt("cluster", UNIT),
    opt("spacing", NON_NEG),
    opt("verbs", Kind::StrList),
    opt("yields", Kind::Any),
    opt("growth", NON_NEG),
    opt("decay", NON_NEG),
    opt("sway", UNIT),
    opt("heat", UNIT),
    opt("fuel", NON_NEG),
    opt("mass", NON_NEG),
];

const CONDITION_FIELDS: [Field; 7] = [
    opt("wet_darkening", UNIT),
    opt("dry_fading", UNIT),
    opt("weathering", UNIT),
    opt("mossing", UNIT),
    opt("soiling", UNIT),
    opt("condition_colors", Kind::Any),
    opt("states", Kind::StrList),
];

const BIOME: [Field; 10] = [
    req("name", Kind::Str),
    req("koppen", Kind::Str),
    req("cover", Kind::Enum(&COVERS)),
    req("ground", Kind::Rgb),
    req("ground_glyph", Kind::Rgb),
    opt("seasonal", Kind::Bool),
    opt("grass", Kind::U8 { max: 3 }),
    opt("tree_density", UNIT),
    opt("species", Kind::Pairs(TableKind::Species)),
    req("material", Kind::Ref { table: TableKind::Materials, extra: &[] }),
];

const SPECIES: [Field; 8] = [
    req("name", Kind::Str),
    req("form", Kind::Enum(&FORMS)),
    opt("size_class", Kind::Enum(&SIZE_CLASSES)),
    req("canopy", Kind::Seasonal),
    req("canopy_glyph", Kind::Seasonal),
    opt("radius", NON_NEG),
    opt("height", NON_NEG),
    opt("sheds", Kind::Bool),
];

const MATERIAL: [Field; 5] = [req("name", Kind::Str), req("wall", Kind::Rgb), req("wall_glyph", Kind::Rgb), req("roof", Kind::Rgb), req("roof_glyph", Kind::Rgb)];

const PROP: [Field; 9] = [
    req("name", Kind::Str),
    req("art", Kind::Ref { table: TableKind::Art, extra: &[] }),
    opt("color", Kind::Rgb),
    req("glyph", Kind::Rgb),
    req("density", UNIT),
    req("terrain", Kind::EnumList(&TERRAINS)),
    opt("cover", Kind::EnumList(&COVERS)),
    opt("near_water", Kind::Bool),
    opt("min_zoom", Kind::U8 { max: 6 }),
];

const BLOCK: [Field; 5] = [
    req("name", Kind::Str),
    req("settle_min", UNIT),
    req("chance", Kind::U64),
    req("terrain", Kind::EnumList(&TERRAINS)),
    opt("material", Kind::Ref { table: TableKind::Materials, extra: &["by_biome"] }),
];

const CREATURE: [Field; 11] = [
    req("name", Kind::Str),
    req("art", Kind::Ref { table: TableKind::Art, extra: &[] }),
    req("color", Kind::Rgb),
    req("glyph", Kind::Rgb),
    opt("can_enter", Kind::EnumList(&TERRAINS)),
    opt("speed", NON_NEG),
    opt("diet", Kind::StrList),
    opt("behaviour", Kind::Enum(&BEHAVIOURS)),
    opt("sight", NON_NEG),
    opt("home", Kind::Ref { table: TableKind::Blocks, extra: &[] }),
    opt("spacing", NON_NEG),
];

const SEASON: [Field; 14] = [
    req("name", Kind::Str),
    req("trunk", Kind::Rgb),
    req("trunk_glyph", Kind::Rgb),
    req("sand", Kind::Rgb),
    req("sand_glyph", Kind::Rgb),
    req("dirt", Kind::Rgb),
    req("dirt_glyph", Kind::Rgb),
    req("rock", Kind::Rgb),
    req("rock_glyph", Kind::Rgb),
    req("snow", Kind::Rgb),
    req("snow_glyph", Kind::Rgb),
    req("water_shallow", Kind::Rgb),
    req("water_deep", Kind::Rgb),
    req("water_glyph", Kind::Rgb),
];

const SURFACE: [Field; 3] = [req("name", Kind::Str), req("texture_density", UNIT), req("relief", Kind::Bool)];

const DENSITY: [Field; 3] = [req("grass_base", UNIT), req("grass_per_level", UNIT), req("cattail", UNIT)];

const LIGHT: [Field; 7] = [
    req("name", Kind::Str),
    req("color", Kind::Unit3),
    req("radius", Kind::F32 { min: 0.0, max: 1000.0 }),
    req("intensity", NON_NEG),
    opt("falloff", Kind::F32 { min: 0.0, max: 1000.0 }),
    opt("flicker_amount", UNIT),
    opt("flicker_rate", NON_NEG),
];

const SETTING: [Field; 5] = [req("key", Kind::Str), req("label", Kind::Str), req("values", Kind::StrList), opt("default", Kind::U8 { max: 255 }), opt("shortcut", Kind::Glyph)];

const TILESET: [Field; 35] = [
    req("name", Kind::Str),
    req("antialias", Kind::Bool),
    req("roles.cover.grass", Kind::GlyphList(3)),
    req("roles.cover.dry", Kind::GlyphList(3)),
    req("roles.cover.moss", Kind::GlyphList(3)),
    req("roles.cover.bare", Kind::GlyphList(3)),
    req("roles.stubble", Kind::GlyphList(3)),
    req("roles.cattail", Kind::GlyphList(2)),
    req("roles.water", Kind::GlyphList(4)),
    req("roles.texture.sand", Kind::GlyphList(2)),
    req("roles.texture.dirt", Kind::GlyphList(2)),
    req("roles.texture.rock", Kind::GlyphList(2)),
    req("roles.texture.snow", Kind::GlyphList(2)),
    req("roles.wall", Kind::GlyphList(2)),
    req("roles.star", Kind::GlyphList(2)),
    req("roles.snowflake", Kind::GlyphList(2)),
    req("roles.flame", Kind::GlyphList(3)),
    req("roles.rain", Kind::Glyph),
    req("art.pine_l", Kind::Glyph),
    req("art.pine_r", Kind::Glyph),
    req("art.pine_fill", Kind::GlyphList(2)),
    req("art.round_top", Kind::GlyphList(3)),
    req("art.round_mid", Kind::GlyphList(3)),
    req("art.round_bot", Kind::GlyphList(3)),
    req("art.trunk", Kind::StrList),
    req("art.cactus", Kind::Glyph),
    req("art.roof_l", Kind::Glyph),
    req("art.roof_r", Kind::Glyph),
    req("art.roof_fill", Kind::Glyph),
    req("art.wall_fill", Kind::Glyph),
    req("art.door", Kind::Glyph),
    req("art.window", Kind::Glyph),
    req("art.tiny_house", Kind::Ref { table: TableKind::Art, extra: &[] }),
    req("art.tiny.pine", Kind::Ref { table: TableKind::Art, extra: &[] }),
    req("art.tiny.broadleaf", Kind::Ref { table: TableKind::Art, extra: &[] }),
];

const TILESET_TINY: [Field; 2] = [req("art.tiny.scrub", Kind::Ref { table: TableKind::Art, extra: &[] }), req("art.tiny.cactus", Kind::Ref { table: TableKind::Art, extra: &[] })];

/// The fields of a table, in form order: the table's own, then the shared
/// identity, hook, physical and condition groups it carries.
pub fn fields(kind: TableKind) -> Vec<Field> {
    let mut v: Vec<Field> = Vec::new();
    let mut add = |f: &[Field]| v.extend_from_slice(f);
    match kind {
        TableKind::Biomes => {
            add(&BIOME);
            add(&IDENTITY);
        }
        TableKind::Species => {
            add(&SPECIES);
            add(&IDENTITY);
            add(&HOOKS);
            add(&PHYSICAL);
            add(&CONDITION_FIELDS);
        }
        TableKind::Materials => {
            add(&MATERIAL);
            add(&IDENTITY);
        }
        TableKind::Props => {
            add(&PROP);
            add(&IDENTITY);
            add(&HOOKS);
            add(&PHYSICAL);
            add(&CONDITION_FIELDS);
        }
        TableKind::Blocks => {
            add(&BLOCK);
            add(&IDENTITY);
            add(&HOOKS);
            add(&PHYSICAL);
            add(&CONDITION_FIELDS);
        }
        TableKind::Creatures => {
            add(&CREATURE);
            add(&IDENTITY);
            add(&HOOKS);
            add(&CONDITION_FIELDS);
        }
        TableKind::Seasons => {
            add(&SEASON);
            add(&IDENTITY);
        }
        TableKind::Surfaces => {
            add(&SURFACE);
            add(&IDENTITY);
            add(&CONDITION_FIELDS);
        }
        TableKind::Density => add(&DENSITY),
        TableKind::Lights => {
            add(&LIGHT);
            add(&IDENTITY);
        }
        TableKind::Settings => {
            add(&SETTING);
            add(&IDENTITY);
        }
        TableKind::Tilesets => {
            add(&TILESET);
            add(&TILESET_TINY);
            add(&IDENTITY);
        }
        TableKind::Art => {}
    }
    v
}

/// Whether a `condition_colors` key is one of the known conditions; the
/// loader checks this too, this is for the form's warning.
pub fn is_condition(name: &str) -> bool {
    CONDITIONS.contains(&name)
}

// Display and parsing of TOML values per kind.

/// A value as one line for the form: strings bare, everything else as
/// inline TOML.
pub fn show(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => {
            let inner: Vec<String> = t.iter().map(|(k, v)| format!("{k} = {}", show_inline(v))).collect();
            format!("{{ {} }}", inner.join(", "))
        }
        other => show_inline(other),
    }
}

/// Inline TOML for a value: strings quoted, arrays bracketed.
pub fn show_inline(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!("{s:?}"),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(a) => format!("[{}]", a.iter().map(show_inline).collect::<Vec<_>>().join(", ")),
        toml::Value::Table(t) => show(&toml::Value::Table(t.clone())),
    }
}

/// Parse a value typed as text for a field of a kind. Strings are taken
/// bare; numbers and lists are checked against the kind; lists of names
/// may be comma-separated without brackets.
pub fn parse(kind: Kind, text: &str) -> Result<toml::Value, String> {
    let t = text.trim();
    let int = |s: &str| s.parse::<i64>().map_err(|_| format!("{s:?} is not a whole number"));
    Ok(match kind {
        Kind::Str | Kind::Text => toml::Value::String(t.to_string()),
        Kind::Glyph => {
            let mut it = t.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => toml::Value::String(c.to_string()),
                _ => return Err(format!("{t:?} is not one character")),
            }
        }
        Kind::Bool => match t {
            "true" | "yes" | "on" | "1" => toml::Value::Boolean(true),
            "false" | "no" | "off" | "0" => toml::Value::Boolean(false),
            _ => return Err(format!("{t:?} is not true or false")),
        },
        Kind::U8 { max } => {
            let v = int(t)?;
            if v < 0 || v > max as i64 {
                return Err(format!("{v} is outside 0..={max}"));
            }
            toml::Value::Integer(v)
        }
        Kind::U64 => {
            let v = int(t)?;
            if v < 0 {
                return Err(format!("{v} is negative"));
            }
            toml::Value::Integer(v)
        }
        Kind::F32 { min, max } => {
            let v: f32 = t.parse().map_err(|_| format!("{t:?} is not a number"))?;
            if v < min || v > max {
                return Err(format!("{v} is outside {}", range_name(min, max)));
            }
            toml::Value::Float(v as f64)
        }
        Kind::Enum(options) => {
            if !options.contains(&t) {
                return Err(format!("{t:?} is not one of {}", options.join(", ")));
            }
            toml::Value::String(t.to_string())
        }
        Kind::Ref { .. } => toml::Value::String(t.to_string()),
        Kind::StrList | Kind::EnumList(_) | Kind::GlyphList(_) if !t.starts_with('[') => {
            let items: Vec<toml::Value> = t.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| toml::Value::String(s.trim_matches('"').to_string())).collect();
            check_list(kind, &items)?;
            toml::Value::Array(items)
        }
        _ => {
            let v = parse_toml(t)?;
            if let (Kind::StrList | Kind::EnumList(_) | Kind::GlyphList(_), toml::Value::Array(items)) = (kind, &v) {
                check_list(kind, items)?;
            }
            v
        }
    })
}

fn check_list(kind: Kind, items: &[toml::Value]) -> Result<(), String> {
    for item in items {
        let Some(s) = item.as_str() else { return Err(format!("{} is not a string", show_inline(item))) };
        match kind {
            Kind::EnumList(options) if !options.contains(&s) => return Err(format!("{s:?} is not one of {}", options.join(", "))),
            Kind::GlyphList(_) if s.chars().count() != 1 => return Err(format!("{s:?} is not one character")),
            _ => {}
        }
    }
    if let Kind::GlyphList(n) = kind {
        if items.len() != n {
            return Err(format!("{} glyphs given; {n} needed", items.len()));
        }
    }
    Ok(())
}

fn range_name(min: f32, max: f32) -> String {
    match (min.is_finite(), max.is_finite()) {
        (true, true) => format!("{min}..{max}"),
        (true, false) => format!("{min} and up"),
        (false, true) => format!("up to {max}"),
        (false, false) => "any number".to_string(),
    }
}

/// Parse one inline TOML value.
pub fn parse_toml(text: &str) -> Result<toml::Value, String> {
    let doc = format!("v = {text}\n");
    let table: toml::Table = toml::from_str(&doc).map_err(|e| e.message().to_string())?;
    table.get("v").cloned().ok_or_else(|| "empty value".to_string())
}

/// Read a colour triple from a value.
pub fn rgb_of(v: &toml::Value) -> Option<[u8; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let mut out = [0u8; 3];
    for (o, c) in out.iter_mut().zip(a) {
        *o = c.as_integer().filter(|i| (0..=255).contains(i))? as u8;
    }
    Some(out)
}

/// Read a seasonal colour: one triple expands to four.
pub fn seasonal_of(v: &toml::Value) -> Option<[[u8; 3]; 4]> {
    if let Some(c) = rgb_of(v) {
        return Some([c; 4]);
    }
    let a = v.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let mut out = [[0u8; 3]; 4];
    for (o, c) in out.iter_mut().zip(a) {
        *o = rgb_of(c)?;
    }
    Some(out)
}

pub fn rgb_value(c: [u8; 3]) -> toml::Value {
    toml::Value::Array(c.iter().map(|&x| toml::Value::Integer(x as i64)).collect())
}

/// The shortest encoding of a season table, as the loader writes it.
pub fn seasonal_value(t: [[u8; 3]; 4]) -> toml::Value {
    if t.iter().all(|c| *c == t[0]) {
        rgb_value(t[0])
    } else {
        toml::Value::Array(t.iter().map(|c| rgb_value(*c)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_table_has_a_name_field_first() {
        for kind in TABLE_KINDS {
            let f = fields(kind);
            if kind == TableKind::Art || kind == TableKind::Density {
                continue;
            }
            assert_eq!(f[0].name, kind.name_field(), "{}", kind.name());
            let names: Vec<&str> = f.iter().map(|f| f.name).collect();
            for (i, n) in names.iter().enumerate() {
                assert!(!names[..i].contains(n), "{}: {n} listed twice", kind.name());
            }
        }
    }

    #[test]
    fn parse_checks_kinds() {
        assert_eq!(parse(Kind::U8 { max: 3 }, "2").unwrap(), toml::Value::Integer(2));
        assert!(parse(Kind::U8 { max: 3 }, "4").is_err());
        assert!(parse(Kind::F32 { min: 0.0, max: 1.0 }, "1.5").unwrap_err().contains("0..1"));
        assert_eq!(parse(Kind::F32 { min: 0.0, max: 1.0 }, "0.5").unwrap(), toml::Value::Float(0.5));
        assert!(parse(Kind::Glyph, "ab").is_err());
        assert_eq!(parse(Kind::Glyph, "▓").unwrap().as_str(), Some("▓"));
        assert!(parse(Kind::Enum(&FORMS), "bush").is_err());
        assert_eq!(parse(Kind::StrList, "flammable, wooden").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(parse(Kind::StrList, "[\"a\"]").unwrap().as_array().unwrap().len(), 1);
        assert!(parse(Kind::EnumList(&TERRAINS), "grass, mud").is_err());
        assert!(parse(Kind::GlyphList(2), "a, b, c").is_err());
        assert_eq!(parse(Kind::Rgb, "[1, 2, 3]").unwrap(), rgb_value([1, 2, 3]));
        assert!(parse(Kind::Any, "[1, 2").is_err());
        assert_eq!(parse(Kind::Bool, "yes").unwrap(), toml::Value::Boolean(true));
    }

    #[test]
    fn colours_round_trip() {
        let one = rgb_value([9, 8, 7]);
        assert_eq!(rgb_of(&one), Some([9, 8, 7]));
        assert_eq!(seasonal_of(&one), Some([[9, 8, 7]; 4]));
        let four = seasonal_value([[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]]);
        assert_eq!(seasonal_of(&four).unwrap()[2], [3, 3, 3]);
        assert_eq!(seasonal_value([[5, 5, 5]; 4]), rgb_value([5, 5, 5]));
        assert_eq!(show(&four), "[[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]]");
        assert_eq!(show(&toml::Value::String("oak".into())), "oak");
        assert_eq!(show_inline(&toml::Value::Float(1.0)), "1.0");
    }
}
