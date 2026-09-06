//! Row properties shared across the placeable tables (docs/properties.md):
//! identity, interaction hooks, physical and lifecycle values, and
//! condition response. These are the resolved forms with defaults applied;
//! the file format is the `*Row` structs in `assets/schema.rs`. Apart from
//! `Conditions::wet_darkening` on surface rows, nothing reads them yet.

use std::collections::BTreeMap;

use crate::assets::schema::{ConditionsRow, HooksRow, IdentityRow, PhysicalRow};
use crate::canvas::Rgb;

/// Description, category and aliases of a row.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Identity {
    pub description: String,
    /// Grouping for the editor's list; the table name when unset.
    pub category: String,
    pub aliases: Vec<String>,
}

impl Identity {
    pub fn from_row(row: &IdentityRow, table: &str) -> Identity {
        Identity {
            description: row.description.clone(),
            category: row.category.clone().unwrap_or_else(|| table.to_string()),
            aliases: row.aliases.clone(),
        }
    }
}

/// Interaction hooks: what a thing is, emits and can act on, and how far.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Hooks {
    pub tags: Vec<String>,
    pub emits: Vec<String>,
    pub affects: Vec<String>,
    /// Tiles; zero means it only emits.
    pub reach: f32,
}

impl Hooks {
    pub fn from_row(row: &HooksRow) -> Hooks {
        Hooks { tags: row.tags.clone(), emits: row.emits.clone(), affects: row.affects.clone(), reach: row.reach.unwrap_or(0.0) }
    }
}

/// Physical, placement and lifecycle properties.
#[derive(Clone, Debug, PartialEq)]
pub struct Physical {
    pub passable: bool,
    pub blocks_sight: bool,
    /// How much accumulated snow shows on it, 0..1.
    pub snow_cover: f32,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub max_slope: Option<f32>,
    pub cluster: f32,
    pub spacing: f32,
    pub verbs: Vec<String>,
    pub yields: Vec<(String, u32)>,
    /// Days from sapling to mature; none means static.
    pub growth: Option<f32>,
    /// Days from ruined to gone.
    pub decay: Option<f32>,
    /// How much wind moves it, 0..1.
    pub sway: f32,
    /// Warmth given off, 0..1.
    pub heat: f32,
    /// Minutes it burns; none means indefinitely.
    pub fuel: Option<f32>,
}

impl Physical {
    /// Resolve with the table's defaults for `passable` and `sway`.
    pub fn from_row(row: &PhysicalRow, passable: bool, sway: f32) -> Physical {
        Physical {
            passable: row.passable.unwrap_or(passable),
            blocks_sight: row.blocks_sight.unwrap_or(false),
            snow_cover: row.snow_cover.unwrap_or(1.0),
            min_height: row.min_height,
            max_height: row.max_height,
            max_slope: row.max_slope,
            cluster: row.cluster.unwrap_or(0.0),
            spacing: row.spacing.unwrap_or(0.0),
            verbs: row.verbs.clone(),
            yields: row.yields.clone(),
            growth: row.growth,
            decay: row.decay,
            sway: row.sway.unwrap_or(sway),
            heat: row.heat.unwrap_or(0.0),
            fuel: row.fuel,
        }
    }
}

/// How visibly a thing shows each condition the world sets, and its named
/// looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Conditions {
    pub wet_darkening: f32,
    pub dry_fading: f32,
    pub weathering: f32,
    pub mossing: f32,
    pub soiling: f32,
    pub condition_colors: BTreeMap<String, Rgb>,
    pub states: Vec<String>,
}

impl Conditions {
    pub fn from_row(row: &ConditionsRow) -> Conditions {
        Conditions {
            wet_darkening: row.wet_darkening.unwrap_or(1.0),
            dry_fading: row.dry_fading.unwrap_or(0.0),
            weathering: row.weathering.unwrap_or(0.0),
            mossing: row.mossing.unwrap_or(0.0),
            soiling: row.soiling.unwrap_or(0.0),
            condition_colors: row.condition_colors.clone(),
            states: row.states.clone(),
        }
    }
}

impl Default for Conditions {
    fn default() -> Conditions {
        Conditions::from_row(&ConditionsRow::default())
    }
}
