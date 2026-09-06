//! The asset file format (ADR-001): one raw row struct per table, derived
//! `Serialize + Deserialize`, with cross-references as names. `Assets`
//! resolves these into the runtime structs (`Biome`, `Species`, ...) and
//! checks every reference. Row order in a file is the runtime index order.
//!
//! Also here: the art file format, a header line of `key=value` pairs over
//! rows of glyphs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::biome::{Cover, Form, SizeClass};
use crate::blocks::{Ground, Roof};
use crate::canvas::Rgb;
use crate::frame::{Anchor, Background, Border, Show, Size};
use crate::map::Terrain;
use crate::volume::Shape;

/// A colour that is either the same all year (one `[r, g, b]`) or given per
/// season as `[spring, summer, autumn, winter]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Seasonal {
    One(Rgb),
    Four([Rgb; 4]),
}

impl Seasonal {
    pub fn expand(&self) -> [Rgb; 4] {
        match *self {
            Seasonal::One(c) => [c, c, c, c],
            Seasonal::Four(t) => t,
        }
    }

    /// The shortest encoding of a season table.
    pub fn from_table(t: &[Rgb; 4]) -> Seasonal {
        if t.iter().all(|c| *c == t[0]) {
            Seasonal::One(t[0])
        } else {
            Seasonal::Four(*t)
        }
    }
}

fn near_zoom() -> u8 {
    2
}

fn three_levels() -> u8 {
    3
}

fn two() -> f32 {
    2.0
}

fn by_biome() -> String {
    "by_biome".to_string()
}

fn one() -> f32 {
    1.0
}

fn one_and_a_half() -> f32 {
    1.5
}

fn yes() -> bool {
    true
}

fn one_level() -> [u8; 2] {
    [1, 1]
}

fn one_tile() -> [[u8; 2]; 2] {
    [[1, 1], [1, 1]]
}

fn default_windows() -> Vec<f32> {
    vec![0.5, 1.0]
}

#[allow(clippy::ptr_arg)]
fn is_none_or_empty<T>(v: &Vec<T>) -> bool {
    v.is_empty()
}

/// Identity fields every row carries (docs/properties.md).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct IdentityRow {
    /// One or two sentences for the editor and for players who examine
    /// the thing. Required on placeable rows.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Grouping for the editor's list; defaults to the table name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub aliases: Vec<String>,
}

/// Interaction hooks on placeable rows: reserved for a future interaction
/// graph, validated and stored but not read by any system yet.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct HooksRow {
    /// Row in lights.toml this asset emits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// What it is: flammable, edible, wooden, stone, wet, sacred.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub tags: Vec<String>,
    /// What it puts out continuously: light, heat, smoke, sound, scent.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub emits: Vec<String>,
    /// Tags it can act on within reach.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub affects: Vec<String>,
    /// How far its effects act, in metres; zero means it only emits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<f32>,
}

/// Reserved physical, lifecycle and weather properties of props, species
/// and blocks. Absent fields take the defaults in docs/properties.md.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PhysicalRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks_sight: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snow_cover: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_slope: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub verbs: Vec<String>,
    /// `[[name, count], ...]` produced by harvesting.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub yields: Vec<(String, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub growth: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sway: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heat: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<f32>,
}

/// Condition names a `condition_colors` table may carry.
pub const CONDITIONS: [&str; 7] = ["wet", "dry", "weathered", "mossy", "soiled", "burnt", "frozen"];

/// How visibly a thing shows each continuous condition the world sets,
/// and its named looks. Shared by every placeable table and the surface
/// rows (docs/properties.md, Conditions and Looks). Only `wet_darkening`
/// on surface rows is read today.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ConditionsRow {
    /// How much rain darkens it; default 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wet_darkening: Option<f32>,
    /// How much drought bleaches it; default 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_fading: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weathering: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mossing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soiling: Option<f32>,
    /// Colour per condition name, from `CONDITIONS`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub condition_colors: BTreeMap<String, Rgb>,
    /// Named looks with their own art or colours: unlit, lit, burning,
    /// burnt, ruined, sapling, mature, dead, open, closed, occupied,
    /// asleep, carrying.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub states: Vec<String>,
}

// biomes.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiomesFile {
    pub biome: Vec<BiomeRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiomeRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    pub koppen: String,
    pub cover: Cover,
    pub ground: Rgb,
    pub ground_glyph: Rgb,
    #[serde(default)]
    pub seasonal: bool,
    #[serde(default)]
    pub grass: u8,
    #[serde(default)]
    pub tree_density: f32,
    /// How far this biome's trees stand apart, as a factor on the species'
    /// own spacing: a rainforest or a boreal stand lets crowns overlap
    /// (0.4), a temperate wood keeps them touching (0.7), a savanna sets
    /// them wide apart (2). One when absent.
    #[serde(default = "one")]
    pub spacing: f32,
    /// `[[species name, weight], ...]`; order is load-bearing for the pick.
    #[serde(default)]
    pub species: Vec<(String, u8)>,
    pub material: String,
}

// species.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeciesFile {
    pub species: Vec<SpeciesRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeciesRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Glyph pool and tiny art: pine, broadleaf, scrub, cactus.
    pub form: Form,
    #[serde(default)]
    pub size_class: SizeClass,
    /// Spread and height of a mature tree in metres: width, depth, height.
    pub size: [f32; 3],
    pub canopy: Seasonal,
    pub canopy_glyph: Seasonal,
    /// Canopy volume for the geometry renderer (ADR-002): the shape is the
    /// form's by default, and the dimensions, all in metres (crown radius,
    /// crown height, trunk height and trunk radius, before the size class
    /// and variant scale them), are derived from `size` by shape when
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// Growth habit from `tree_styles.toml`, for `shape = "lsystem"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trunk: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trunk_radius: Option<f32>,
    /// Whether it drops leaves in autumn; deciduous (four-colour canopy)
    /// species do by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheds: Option<bool>,
    /// Chance in 0..1 that an instance stands dead; 0.02 by default. The
    /// roll is a hash of the tile seed, so the same tree is dead every time
    /// the chunk is generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_chance: Option<f32>,
    #[serde(flatten)]
    pub hooks: HooksRow,
    #[serde(flatten)]
    pub physical: PhysicalRow,
    #[serde(flatten)]
    pub conditions: ConditionsRow,
    /// The grammar `shape = "lsystem"` grows the tree from, as the
    /// sub-table `[species.lsystem]`. Last in the row because TOML wants a
    /// table after the plain values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsystem: Option<LsystemRow>,
}

/// One rule's right-hand sides: a single replacement, several with equal
/// weight, or `[[replacement, weight], ...]` pairs (docs/lsystem.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rule {
    One(String),
    Weighted(Vec<(String, u8)>),
    Any(Vec<String>),
}

impl Rule {
    /// The replacements with their weights, in file order.
    pub fn alternatives(&self) -> Vec<(String, u8)> {
        match self {
            Rule::One(s) => vec![(s.clone(), 1)],
            Rule::Weighted(v) => v.clone(),
            Rule::Any(v) => v.iter().map(|s| (s.clone(), 1)).collect(),
        }
    }
}

/// What a species changes about the growth habit it names, or the whole
/// grammar when it writes one out itself (docs/lsystem.md). Every field is
/// optional: a species with `style` overrides only what it names, and one
/// without needs at least `axiom` and `rules`. The model is scaled to the
/// species' `size`, so a rule set fits its declared height and spread at
/// any depth.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LsystemRow {
    /// The string rewriting starts from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axiom: Option<String>,
    /// How many times the rules are applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u8>,
    /// Turn of every `+ - & ^ \ /` in degrees.
    #[serde(default, alias = "branch_angle", skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    /// Length of a first-level `F` in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f32>,
    /// Factor a segment's length and radius are multiplied by on entering a
    /// branch (`[`) and on every `!`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<f32>,
    /// Radius of an `L` leaf cluster in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_radius: Option<f32>,
    /// Branches in a whorl, or ways the trunk forks.
    #[serde(default, alias = "whorl_or_fork_count", skip_serializing_if = "Option::is_none")]
    pub forks: Option<u8>,
    /// Bend per segment inside a branch as a fraction of the angle;
    /// negative lifts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub droop: Option<f32>,
    /// Chance an `L` becomes a cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_density: Option<f32>,
    /// How one-sided the instance is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asymmetry: Option<f32>,
    /// Small noise on every branch and cluster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    /// Fraction of the height with no live branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune_height: Option<f32>,
    /// Symbol to its replacements. A table, so it comes last.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: BTreeMap<String, Rule>,
    /// Rules standing deadwood is grown with instead: fewer and shorter
    /// branches, a broken crown. Absent means `rules` one level shallower.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dead_rules: BTreeMap<String, Rule>,
}

// tree_styles.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeStylesFile {
    pub style: Vec<StyleRow>,
}

/// One growth habit: a parametric grammar with its defaults and the canopy
/// volume that stands in for it at far zooms (docs/lsystem.md).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Canopy volume of ADR-002 this habit reads as from far away.
    pub stand_in: Shape,
    /// The string rewriting starts from; a template like the rules.
    pub axiom: String,
    /// The habit's defaults; a species overrides what it names.
    pub params: StyleParamsRow,
    pub rules: BTreeMap<String, Rule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dead_rules: BTreeMap<String, Rule>,
}

/// The parameter defaults of a habit. Every one is also a species override.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleParamsRow {
    pub branch_angle: f32,
    pub forks: u8,
    pub taper: f32,
    pub droop: f32,
    pub leaf_density: f32,
    pub asymmetry: f32,
    pub jitter: f32,
    pub prune_height: f32,
    pub depth: u8,
    pub length: f32,
    pub leaf_radius: f32,
}

// materials.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialsFile {
    pub material: Vec<MaterialRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    pub wall: Rgb,
    pub wall_glyph: Rgb,
    pub roof: Rgb,
    pub roof_glyph: Rgb,
}

// props.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropsFile {
    pub prop: Vec<PropRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Art name; the tier is picked by zoom.
    pub art: String,
    /// Width, depth and height in metres.
    pub size: [f32; 3],
    /// Black means the glyph is drawn over the ground with no background.
    #[serde(default)]
    pub color: Rgb,
    #[serde(alias = "glyph_color")]
    pub glyph: Rgb,
    /// Chance per 4x4 sub-cell of a qualifying tile.
    pub density: f32,
    pub terrain: Vec<Terrain>,
    #[serde(default)]
    pub cover: Vec<Cover>,
    #[serde(default)]
    pub near_water: bool,
    /// Smallest zoom the prop is drawn at; near by default.
    #[serde(default = "near_zoom")]
    pub min_zoom: u8,
    #[serde(flatten)]
    pub hooks: HooksRow,
    #[serde(flatten)]
    pub physical: PhysicalRow,
    #[serde(flatten)]
    pub conditions: ConditionsRow,
}

// blocks.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlocksFile {
    pub block: Vec<BlockRow>,
}

/// A block kind: placement, geometry (ADR-002) and faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Settlement field value a tile needs before one may stand on it;
    /// above one, the kind is only ever placed by hand.
    #[serde(default = "one")]
    pub settle_min: f32,
    /// Percentage of qualifying tiles that carry one.
    #[serde(default)]
    pub chance: u64,
    pub terrain: Vec<Terrain>,
    /// Width, depth and height in metres of one tile of the kind at one
    /// level: the footprint is a tile, the height a level.
    pub size: [f32; 3],
    /// Levels a generated stack may have, `[min, max]`; zero for ground
    /// kinds.
    #[serde(default = "one_level")]
    pub levels: [u8; 2],
    /// Ground the generator gives one of these, in tiles: `[[w, d], [w, d]]`
    /// smallest and largest. A house is 3x2 to 5x3 tiles (6 to 10 m by 4 to
    /// 6 m), a tower 2x2, a field a whole plot. One tile when absent.
    #[serde(default = "one_tile")]
    pub footprint: [[u8; 2]; 2],
    /// Metres per level; the height in `size` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_height: Option<f32>,
    /// Profile above the column top.
    #[serde(default)]
    pub roof: Roof,
    /// Metres of rise per metre of run from the eaves, and the cap in
    /// metres.
    #[serde(default = "one")]
    pub pitch: f32,
    #[serde(default = "one_and_a_half")]
    pub max_rise: f32,
    /// `"by_biome"` (or `"local"`) for the tile's local material, or a
    /// material name.
    #[serde(default = "by_biome")]
    pub material: String,
    /// Whether same-kind neighbours share walls and roof.
    #[serde(default = "yes")]
    pub merge: bool,
    /// What the tile top becomes.
    #[serde(default)]
    pub ground: Ground,
    /// Band within a level, as fractions, where windows go; empty for none.
    #[serde(default = "default_windows")]
    pub windows: Vec<f32>,
    /// Metres between window centres.
    #[serde(default = "one")]
    pub window_pitch: f32,
    /// One door at ground level on an open face.
    #[serde(default = "yes")]
    pub door: bool,
    /// Validation and editor cap on levels.
    #[serde(default = "three_levels")]
    pub max_levels: u8,
    /// Bridge: the top sits at the bank height over water.
    #[serde(default)]
    pub deck: bool,
    #[serde(flatten)]
    pub hooks: HooksRow,
    #[serde(flatten)]
    pub physical: PhysicalRow,
    #[serde(flatten)]
    pub conditions: ConditionsRow,
}

// creatures.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreaturesFile {
    pub creature: Vec<CreatureRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatureRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    pub art: String,
    /// Width, depth and height in metres: a person is 2 m tall.
    pub size: [f32; 3],
    pub color: Rgb,
    #[serde(alias = "glyph_color")]
    pub glyph: Rgb,
    /// Terrain it may walk on; absent means every land kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_enter: Option<Vec<Terrain>>,
    /// Metres per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Tags it eats.
    #[serde(default, skip_serializing_if = "is_none_or_empty")]
    pub diet: Vec<String>,
    /// idle, wander, graze, flee, hunt, patrol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behaviour: Option<String>,
    /// Perception range in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sight: Option<f32>,
    /// Block kind it returns to at night.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(flatten)]
    pub hooks: HooksRow,
    #[serde(flatten)]
    pub conditions: ConditionsRow,
}

// surfaces.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfacesFile {
    /// Exactly four, in order: spring, summer, autumn, winter.
    pub season: Vec<SeasonRow>,
    /// Exactly four, in `Terrain::surface` order: sand, dirt, rock, snow.
    pub surface: Vec<SurfaceRow>,
    pub density: DensityRow,
}

/// One season's palette.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeasonRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    pub trunk: Rgb,
    pub trunk_glyph: Rgb,
    pub sand: Rgb,
    pub sand_glyph: Rgb,
    pub dirt: Rgb,
    pub dirt_glyph: Rgb,
    pub rock: Rgb,
    pub rock_glyph: Rgb,
    pub snow: Rgb,
    pub snow_glyph: Rgb,
    pub water_shallow: Rgb,
    pub water_deep: Rgb,
    pub water_glyph: Rgb,
}

/// Texture rule for one plain ground surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Fraction of ground cells carrying a texture glyph.
    pub texture_density: f32,
    /// Whether height lifts and wetness darkens the colour; snow stays flat.
    pub relief: bool,
    #[serde(flatten)]
    pub conditions: ConditionsRow,
}

/// Glyph densities of the surfaces with bespoke rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DensityRow {
    /// Grass tuft chance per cell at tuft level 0, plus this much per level.
    pub grass_base: f32,
    pub grass_per_level: f32,
    /// Cattail chance per cell in a still pond at full vigour.
    pub cattail: f32,
}

// lights.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightsFile {
    pub light: Vec<LightRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Colour as 0..1 floats.
    pub color: [f32; 3],
    /// Throw in metres.
    pub radius: f32,
    pub intensity: f32,
    /// Falloff exponent.
    #[serde(default = "two")]
    pub falloff: f32,
    /// Flicker depth in 0..1 and speed in hertz.
    #[serde(default)]
    pub flicker_amount: f32,
    #[serde(default)]
    pub flicker_rate: f32,
}

// settings.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    pub setting: Vec<SettingRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingRow {
    pub key: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    pub label: String,
    pub values: Vec<String>,
    #[serde(default)]
    pub default: u8,
    /// Scene key that cycles the row; the binding table in `input.rs`
    /// must agree, which a test checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<char>,
}

// ui.toml

/// The overlay frames of ADR-005, one row per frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiFile {
    pub frame: Vec<FrameRow>,
}

/// A frame: where it goes, how it is framed, when it shows and which key
/// toggles it. Fields that may serialise as a table come last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameRow {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Shown in the top border; blank for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Content kind code supplies; the frame's own name by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub anchor: Anchor,
    pub border: Border,
    /// Draw order; focused frames draw last.
    #[serde(default)]
    pub z: i32,
    /// Which frames survive when they collide.
    #[serde(default)]
    pub priority: i32,
    /// Toggle key: `"tab"`, `"esc"`, `"enter"`, `"space"` or one
    /// character. `input.rs` must bind it to `Toggle(name)`, which a test
    /// checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub show: Show,
    pub background: Background,
    pub size: Size,
}

// tilesets/*.toml

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TilesetFile {
    pub name: String,
    #[serde(flatten)]
    pub identity: IdentityRow,
    /// Whether sextant glyphs may be used for edge antialiasing.
    pub antialias: bool,
    pub roles: RolesSpec,
    pub art: ArtSpec,
}

/// Glyphs for the terrain and weather roles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RolesSpec {
    /// Per cover kind: wind leaning left, upright, leaning right.
    pub cover: CoverGlyphs,
    /// Sparse remnants shown as cover dies back toward winter.
    pub stubble: [char; 3],
    /// Reeds in still shallow water.
    pub cattail: [char; 2],
    /// Water surface glyphs, cycled by wave phase.
    pub water: [char; 4],
    pub texture: TextureGlyphs,
    /// Cliff face glyph for the face pointing screen-right and screen-left.
    pub wall: [char; 2],
    pub star: [char; 2],
    pub snowflake: [char; 2],
    pub flame: [char; 3],
    pub rain: char,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoverGlyphs {
    pub grass: [char; 3],
    pub dry: [char; 3],
    pub moss: [char; 3],
    pub bare: [char; 3],
}

/// Texture glyph pairs for the plain surfaces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureGlyphs {
    pub sand: [char; 2],
    pub dirt: [char; 2],
    pub rock: [char; 2],
    pub snow: [char; 2],
}

/// Glyph vocabulary the geometry is textured with, plus the art names of
/// the one-glyph tree sprites for the smallest tiles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtSpec {
    pub pine_l: char,
    pub pine_r: char,
    pub pine_fill: [char; 2],
    pub round_top: [char; 3],
    pub round_mid: [char; 3],
    pub round_bot: [char; 3],
    /// Trunks of width 1, 2 and 4.
    pub trunk: [String; 3],
    pub cactus: char,
    pub roof_fill: char,
    pub door: char,
    pub window: char,
    /// Art names of the tiny tree sprites per form (tier `tiny`).
    pub tiny: TinyRefs,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TinyRefs {
    pub pine: String,
    pub broadleaf: String,
    pub scrub: String,
    pub cactus: String,
}

// art/**/*.txt

/// Sprite tiers, one per zoom (ADR-004): tiny is 1:8, small 1:4, medium
/// 1:2 and large 1:1. A tier names the zoom its sprite is drawn at
/// (`min_zoom`), which is what the editor's panes show; what the scene
/// draws is the tier whose row count is nearest the thing's height in rows
/// at that zoom (`ArtIndex::for_rows`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tier {
    /// Zoom 0, far, 1:8.
    Tiny = 0,
    /// Zoom 1, mid, 1:4.
    Small = 1,
    /// Zoom 2, near, 1:2.
    Medium = 2,
    /// Zoom 3, close, 1:1.
    Large = 3,
}

pub const TIERS: [Tier; 4] = [Tier::Tiny, Tier::Small, Tier::Medium, Tier::Large];

impl Tier {
    pub fn of_zoom(zoom: usize) -> Tier {
        TIERS[zoom.min(TIERS.len() - 1)]
    }

    /// The zoom a tier is drawn at by default: its own.
    pub fn min_zoom(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Large => "large",
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        TIERS.iter().copied().find(|t| t.name() == s)
    }
}

/// A parsed art file: the header fields and the rows padded to one width.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtFile {
    pub name: String,
    pub tier: Tier,
    /// First zoom this sprite is used at; the tier's default unless the
    /// header says otherwise.
    pub min_zoom: usize,
    pub center: i32,
    pub base_rows: usize,
    pub rows: Vec<String>,
}

impl ArtFile {
    /// Parse the format: a `#` header of `key=value` pairs (`name`, `tier`,
    /// optional `center`, `base_rows`, `min_zoom`), then rows. Spaces are
    /// transparent, tabs an error, trailing blank lines dropped, rows padded
    /// to the widest. Errors carry a 1-based line number.
    pub fn parse(text: &str) -> Result<ArtFile, (usize, String)> {
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
        let header = lines.next().unwrap_or("");
        let Some(fields) = header.strip_prefix('#') else {
            return Err((1, "art files start with a `# name=... tier=...` header".to_string()));
        };
        let (mut name, mut tier, mut center, mut base_rows, mut min_zoom) = (None, None, None, 0usize, None);
        for field in fields.split_whitespace() {
            let Some((k, v)) = field.split_once('=') else {
                return Err((1, format!("header field `{field}` is not key=value")));
            };
            match k {
                "name" => name = Some(v.to_string()),
                "tier" => tier = Some(Tier::parse(v).ok_or_else(|| (1, format!("unknown tier `{v}`")))?),
                "center" => center = Some(v.parse::<i32>().map_err(|_| (1, format!("center `{v}` is not a number")))?),
                "base_rows" => base_rows = v.parse::<usize>().map_err(|_| (1, format!("base_rows `{v}` is not a number")))?,
                "min_zoom" => min_zoom = Some(v.parse::<usize>().map_err(|_| (1, format!("min_zoom `{v}` is not a number")))?),
                _ => return Err((1, format!("unknown header field `{k}`"))),
            }
        }
        let name = name.ok_or_else(|| (1, "header has no name".to_string()))?;
        let tier = tier.ok_or_else(|| (1, "header has no tier".to_string()))?;
        let min_zoom = min_zoom.unwrap_or(tier.min_zoom());
        if min_zoom >= crate::tileset::ZOOMS.len() {
            return Err((1, format!("min_zoom {min_zoom} is beyond the last zoom {}", crate::tileset::ZOOMS.len() - 1)));
        }
        let mut rows: Vec<String> = Vec::new();
        for (i, line) in lines.enumerate() {
            if line.contains('\t') {
                return Err((i + 2, "tabs are not allowed in art rows".to_string()));
            }
            rows.push(line.to_string());
        }
        while rows.last().is_some_and(|r| r.trim().is_empty()) {
            rows.pop();
        }
        if rows.is_empty() {
            return Err((2, "art has no rows".to_string()));
        }
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        for r in rows.iter_mut() {
            let n = r.chars().count();
            r.extend(std::iter::repeat_n(' ', width - n));
        }
        let center = center.unwrap_or(width as i32 / 2);
        if center < 0 || center >= width as i32 {
            return Err((1, format!("center {center} is outside the {width}-column sprite")));
        }
        if base_rows > rows.len() {
            return Err((1, format!("base_rows {base_rows} exceeds the {} rows", rows.len())));
        }
        Ok(ArtFile { name, tier, min_zoom, center, base_rows, rows })
    }

    /// The file text for this sprite.
    pub fn to_text(&self) -> String {
        let mut s = format!("# name={} tier={} center={} base_rows={}", self.name, self.tier.name(), self.center, self.base_rows);
        if self.min_zoom != self.tier.min_zoom() {
            s.push_str(&format!(" min_zoom={}", self.min_zoom));
        }
        s.push('\n');
        for r in &self.rows {
            s.push_str(r);
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_parses_header_and_pads_rows() {
        let a = ArtFile::parse("# name=x tier=small base_rows=1\n ab\ncdef\n\n\n").unwrap();
        assert_eq!(a.name, "x");
        assert_eq!(a.tier, Tier::Small);
        assert_eq!(a.center, 2);
        assert_eq!(a.base_rows, 1);
        assert_eq!(a.min_zoom, 1, "the small tier is drawn at the mid zoom");
        assert_eq!(a.rows, vec![" ab ".to_string(), "cdef".to_string()]);
        assert_eq!(ArtFile::parse(&a.to_text()).unwrap(), a);
        let b = ArtFile::parse("# name=x tier=large min_zoom=2\nab\n").unwrap();
        assert_eq!(b.min_zoom, 2);
        assert!(b.to_text().contains(" min_zoom=2"));
        assert_eq!(ArtFile::parse(&b.to_text()).unwrap(), b);
        assert!(ArtFile::parse("# name=x tier=large min_zoom=9\nab\n").is_err());
    }

    #[test]
    fn art_rejects_bad_files() {
        assert_eq!(ArtFile::parse("x\n").unwrap_err().0, 1);
        assert_eq!(ArtFile::parse("# name=x\nab\n").unwrap_err().1, "header has no tier");
        assert_eq!(ArtFile::parse("# name=x tier=tiny\na\n\tb\n").unwrap_err().0, 3);
        assert!(ArtFile::parse("# name=x tier=tiny\n").is_err());
        assert!(ArtFile::parse("# name=x tier=tiny center=9\nab\n").is_err());
        assert!(ArtFile::parse("# name=x tier=huge\nab\n").is_err());
    }

    #[test]
    fn tiers_follow_zooms() {
        // One tier per zoom, tiny at the overview and large at 1:1.
        assert_eq!(crate::tileset::ZOOMS.len(), TIERS.len());
        for (zoom, tier) in TIERS.iter().enumerate() {
            assert_eq!(Tier::of_zoom(zoom), *tier);
            assert_eq!(tier.min_zoom(), zoom);
        }
        assert_eq!(Tier::of_zoom(9), Tier::Large, "past the last zoom is the last tier");
        assert_eq!(Tier::parse("medium"), Some(Tier::Medium));
    }

    #[test]
    fn seasonal_shortens_evergreens() {
        let c = Rgb(1, 2, 3);
        assert_eq!(Seasonal::from_table(&[c; 4]), Seasonal::One(c));
        let t = [c, c, c, Rgb(0, 0, 0)];
        assert_eq!(Seasonal::from_table(&t), Seasonal::Four(t));
        assert_eq!(Seasonal::One(c).expand(), [c; 4]);
    }
}
