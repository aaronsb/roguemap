//! Climate, biomes, tree species, building kinds, props and creatures: the
//! resolved forms of the tables in `assets/` (ADR-001).
//!
//! Climate is temperature and precipitation. A simplified Köppen scheme maps
//! the pair to a code, the biome table names a biome per code, and the
//! biome names its ground colour, how many trees it carries, which species,
//! and what its buildings are made of.

use serde::{Deserialize, Serialize};

use crate::blocks::{Ground, Roof};
use crate::canvas::Rgb;
use crate::lsystem::{Grammar, Growth, TreeModel};
use crate::map::Terrain;
use crate::palette::season_blend;
use crate::properties::{Conditions, Hooks, Identity, Physical};
use crate::volume::{Dims, Shape};

/// Form a species is drawn with: its glyph pool and its one-glyph sprite
/// at the smallest zooms. The canopy volume is `Shape`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Form {
    Pine,
    Broadleaf,
    Scrub,
    Cactus,
}

pub const FORMS: [Form; 4] = [Form::Pine, Form::Broadleaf, Form::Scrub, Form::Cactus];

/// Which of a form's sprite variants a species draws. Each form has four
/// variants per zoom: a smaller pair then a larger pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeClass {
    /// Always the smaller pair.
    Small,
    /// Either pair, chosen by the tile's variant.
    #[default]
    Mixed,
    /// Always the larger pair.
    Large,
}

impl SizeClass {
    /// Index of the first sprite of the pair a tile variant draws from.
    pub fn pair(self, variant: u8) -> usize {
        match self {
            SizeClass::Small => 0,
            SizeClass::Large => 2,
            SizeClass::Mixed => (variant as usize / 2 % 2) * 2,
        }
    }
}

/// Ground cover kinds; the tileset carries a glyph triple for each, indexed
/// by the discriminant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cover {
    Grass = 0,
    Dry = 1,
    Moss = 2,
    Bare = 3,
}

pub const COVERS: [Cover; 4] = [Cover::Grass, Cover::Dry, Cover::Moss, Cover::Bare];

#[derive(Clone, Debug, PartialEq)]
pub struct Species {
    pub name: String,
    pub identity: Identity,
    pub form: Form,
    pub size_class: SizeClass,
    /// Spread and height of a mature tree in metres.
    pub size: [f32; 3],
    /// Canopy colour by season: spring, summer, autumn, winter.
    pub canopy: [Rgb; 4],
    pub canopy_glyph: [Rgb; 4],
    /// Canopy volume (ADR-002), dimensions in metres; absent values come
    /// from the shape's defaults, and the shape from the form.
    pub shape: Option<Shape>,
    pub radius: Option<f32>,
    pub height: Option<f32>,
    pub trunk: Option<f32>,
    pub trunk_radius: Option<f32>,
    /// The grammar of a `shape = "lsystem"` species, checked at load
    /// (docs/lsystem.md).
    pub lsystem: Option<Grammar>,
    /// The canopy volume an L-system species reads as at the far zooms: its
    /// style's `stand_in` (docs/structures.md, "Volumes are stand-ins").
    pub stand_in: Option<Shape>,
    /// How solid the crown is, 0..1: the chance a ray sample inside it meets
    /// foliage rather than passing through (docs/structures.md, "Porous
    /// canopies"). The grammar's `leaf_density` when there is one, else the
    /// form's.
    pub leaf_density: f32,
    /// Whether it drops leaves in autumn.
    pub sheds: bool,
    /// Chance in 0..1 that an instance stands dead.
    pub dead_chance: f32,
    /// Index into the light table.
    pub light: Option<usize>,
    pub hooks: Hooks,
    pub physical: Physical,
    pub conditions: Conditions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub identity: Identity,
    pub wall: Rgb,
    pub wall_glyph: Rgb,
    pub roof: Rgb,
    pub roof_glyph: Rgb,
}

/// What a building kind is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaterialRule {
    /// The tile's local material: the biome's, or stone in the highlands.
    Local,
    /// One material everywhere, by index into the material table.
    Fixed(usize),
}

/// A block kind: where it is placed, its column and roof, its faces, what
/// it is made of and whether it glows at night (ADR-002).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub name: String,
    pub identity: Identity,
    /// Settlement field value a tile needs before one may stand on it.
    pub settle_min: f32,
    /// Percentage of qualifying tiles that carry one.
    pub chance: u64,
    /// Terrain kinds it stands on.
    pub terrain: Vec<Terrain>,
    /// One tile of the kind at one level, in metres.
    pub size: [f32; 3],
    /// Levels a generated stack has, `[min, max]`.
    pub levels: [u8; 2],
    /// Smallest and largest ground the generator gives one, in tiles.
    pub footprint: [[u8; 2]; 2],
    /// Metres per level; zero for ground kinds.
    pub level_height: f32,
    pub roof: Roof,
    /// Metres of rise per metre of run, and its cap in metres.
    pub pitch: f32,
    pub max_rise: f32,
    pub material: MaterialRule,
    /// Whether same-kind neighbours share walls and roof.
    pub merge: bool,
    pub ground: Ground,
    /// The colour that ground takes in place of the terrain's, where the
    /// kind lays one; the material's wall colour when absent.
    pub ground_color: Option<Rgb>,
    /// Metres between the rows of the ground texture: a field's furrows.
    pub ground_pitch: f32,
    /// Window band within a level, as fractions; none for no windows.
    pub windows: Option<[f32; 2]>,
    /// Metres between window centres.
    pub window_pitch: f32,
    pub door: bool,
    pub max_levels: u8,
    pub deck: bool,
    /// Index into the light table, pushed per building at night.
    pub light: Option<usize>,
    pub hooks: Hooks,
    pub physical: Physical,
    pub conditions: Conditions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Biome {
    pub name: String,
    pub identity: Identity,
    pub koppen: String,
    /// Ground cover kind drawn over the ground colour.
    pub cover: Cover,
    /// Summer ground colour and glyph colour; seasons modulate them.
    pub ground: Rgb,
    pub ground_glyph: Rgb,
    /// Whether the ground colour follows the seasons.
    pub seasonal: bool,
    /// Grass tuft density, 0..=3.
    pub grass: u8,
    /// Fraction of eligible sites carrying a tree in the densest patches.
    pub tree_density: f32,
    /// Factor on each species' spacing here: under one the crowns of a
    /// dense stand overlap, over one they stand apart.
    pub spacing: f32,
    /// Species indices with relative weights, in file order.
    pub species: Vec<(usize, u8)>,
    /// Index into the material table.
    pub material: usize,
}

/// A small ground prop and where it may stand.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub name: String,
    pub identity: Identity,
    /// Art name; the tier is picked from the prop's height in rows.
    pub art: String,
    /// Width, depth and height in metres.
    pub size: [f32; 3],
    /// Background colour; black draws the glyph over the ground.
    pub color: Rgb,
    pub glyph: Rgb,
    /// Chance per 4x4 sub-cell of a qualifying tile.
    pub density: f32,
    pub terrain: Vec<Terrain>,
    /// Ground cover kinds it needs, empty for any.
    pub cover: Vec<Cover>,
    /// Whether the tile must touch water.
    pub near_water: bool,
    /// Smallest zoom it is drawn at.
    pub min_zoom: u8,
    pub light: Option<usize>,
    pub hooks: Hooks,
    pub physical: Physical,
    pub conditions: Conditions,
}

/// A creature kind; the player is the first row.
#[derive(Clone, Debug, PartialEq)]
pub struct Creature {
    pub name: String,
    pub identity: Identity,
    pub art: String,
    /// Width, depth and height in metres; the art tier is picked from the
    /// rows the height is worth at the zoom (ADR-004).
    pub size: [f32; 3],
    pub color: Rgb,
    pub glyph: Rgb,
    /// Terrain it may walk on.
    pub can_enter: Vec<Terrain>,
    /// Metres per second.
    pub speed: f32,
    /// Degrees per second the body turns on the spot (ADR-009).
    pub turn: f32,
    pub diet: Vec<String>,
    pub behaviour: String,
    /// Perception range in tiles.
    pub sight: f32,
    /// Index into the block table: where it returns at night.
    pub home: Option<usize>,
    pub spacing: f32,
    pub light: Option<usize>,
    pub hooks: Hooks,
    pub conditions: Conditions,
}

impl Species {
    /// The canopy shape and its dimensions at size scale one: from `size`
    /// by shape unless the row says otherwise.
    pub fn volume(&self) -> (Shape, Dims) {
        let mut shape = self.shape.unwrap_or_else(|| Shape::for_form(self.form));
        if shape == Shape::Lsystem {
            // A grown species has no volume of its own: from far away it
            // reads as its habit's stand-in.
            shape = self.stand_in.unwrap_or(Shape::Ellipsoid);
        }
        // A species grown from an L-system prunes its stand-in to the same
        // height its branches start at.
        let prune = self.lsystem.as_ref().map(|g| g.prune_height).filter(|p| *p > 0.0);
        let d = Dims::pruned(shape, self.size, prune.unwrap_or_else(|| crate::volume::prune_height(shape)));
        (shape, Dims { radius: self.radius.unwrap_or(d.radius), height: self.height.unwrap_or(d.height), trunk: self.trunk.unwrap_or(d.trunk), trunk_radius: self.trunk_radius.unwrap_or(d.trunk_radius) })
    }

    /// How far this species stands from its own kind, in metres: its
    /// `spacing` when the row gives one, else three quarters of the crown's
    /// width, so a 7 m spruce keeps five metres and a 3 m juniper two.
    pub fn spacing(&self) -> f32 {
        if self.physical.spacing > 0.0 {
            self.physical.spacing
        } else {
            0.75 * 0.5 * (self.size[0] + self.size[1])
        }
    }

    /// How much of an instance's foliage is out, in 0..1: an evergreen keeps
    /// all of it, a species that `sheds` follows the season's `vigour`, so a
    /// deciduous tree goes bare across autumn and leafs out again in spring.
    /// `annual` is the tile's mean temperature in degrees Celsius.
    pub fn foliage(&self, annual: f32, season: f32) -> f32 {
        if self.sheds {
            vigour(annual, season)
        } else {
            1.0
        }
    }

    /// Whether the instance grown from `seed` stands dead. The roll is a
    /// hash, so one tile's tree is dead every time the chunk is generated.
    pub fn dead(&self, seed: u64) -> bool {
        self.dead_chance > 0.0 && crate::noise::hash01(seed as i64, 0x0dead, 0xdead_beef) < self.dead_chance
    }

    /// One instance as geometry: the species' grammar grown at `seed`,
    /// scaled to its `size`, with the leaves this growth carries. None
    /// unless the row is `shape = "lsystem"`.
    pub fn tree_model(&self, seed: u64, growth: Growth) -> Option<TreeModel> {
        let mut m = self.lsystem.as_ref()?.grow(seed, self.size, growth);
        // The grammar's radii are relative; the row's trunk_radius, or the
        // shape's default from `size`, puts them in metres.
        m.set_trunk_radius(self.volume().1.trunk_radius);
        Some(m)
    }
}

/// The Köppen codes `classify` emits; the biome table must name each.
pub const KOPPEN_CODES: [&str; 9] = ["Af", "Aw", "BW", "BS", "Cs", "Cf", "Df", "ET", "EF"];

/// Simplified Köppen classification from annual mean temperature in degrees
/// Celsius and precipitation on a 0..100 scale, as the code the biome
/// table is keyed by.
pub fn classify(temp: f32, precip: f32) -> &'static str {
    if temp <= -16.0 {
        "EF"
    } else if temp <= -6.0 {
        "ET"
    } else if precip < 22.0 {
        "BW"
    } else if precip < 42.0 {
        "BS"
    } else if temp >= 18.0 {
        if precip >= 68.0 {
            "Af"
        } else {
            "Aw"
        }
    } else if temp >= 3.0 {
        if precip < 58.0 {
            "Cs"
        } else {
            "Cf"
        }
    } else {
        "Df"
    }
}

/// Pick from a table of `(index, weight)` pairs by a roll, returning the
/// chosen index; zero when the table is empty.
pub fn weighted_pick(table: &[(usize, u8)], roll: u64) -> usize {
    let total: u32 = table.iter().map(|&(_, w)| w as u32).sum();
    let mut pick = (roll % total.max(1) as u64) as u32;
    for &(ix, w) in table {
        if pick < w as u32 {
            return ix;
        }
        pick -= w as u32;
    }
    0
}

/// Blend a four-season table at a continuous season value in `[0, 4)`.
pub fn seasonal(table: &[Rgb; 4], season: f32) -> Rgb {
    let (i, j, f) = season_blend(season);
    table[i].lerp(table[j], f)
}

/// Seasonal modulation of a biome's summer ground colour.
pub fn ground_color(biome: &Biome, season: f32) -> Rgb {
    if !biome.seasonal {
        return biome.ground;
    }
    let g = biome.ground;
    let table = [g.lerp(Rgb(120, 190, 90), 0.25), g, g.lerp(Rgb(170, 130, 60), 0.4), g.lerp(Rgb(150, 150, 140), 0.35)];
    seasonal(&table, season)
}

/// Plant vigour in `[0, 1]` from the seasonal temperature: full growth in
/// warmth, stubble near freezing, nothing in deep cold.
pub fn vigour(annual: f32, season: f32) -> f32 {
    crate::noise::smoothstep(-5.0, 7.0, seasonal_temp(annual, season))
}

/// Temperature at a moment of the year: the annual mean plus a swing that
/// peaks in summer (season 1) and bottoms in winter (season 3).
pub fn seasonal_temp(annual: f32, season: f32) -> f32 {
    annual + 9.0 * ((season - 1.0) * std::f32::consts::FRAC_PI_2).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_the_table() {
        assert_eq!(classify(25.0, 80.0), "Af");
        assert_eq!(classify(25.0, 50.0), "Aw");
        assert_eq!(classify(20.0, 10.0), "BW");
        assert_eq!(classify(10.0, 30.0), "BS");
        assert_eq!(classify(12.0, 50.0), "Cs");
        assert_eq!(classify(8.0, 70.0), "Cf");
        assert_eq!(classify(-2.0, 70.0), "Df");
        assert_eq!(classify(-10.0, 70.0), "ET");
        assert_eq!(classify(-20.0, 70.0), "EF");
        for code in KOPPEN_CODES {
            let found = [(25.0, 80.0), (25.0, 50.0), (20.0, 10.0), (10.0, 30.0), (12.0, 50.0), (8.0, 70.0), (-2.0, 70.0), (-10.0, 70.0), (-20.0, 70.0)].iter().any(|&(t, p)| classify(t, p) == code);
            assert!(found, "{code}");
        }
    }

    #[test]
    fn weighted_pick_honours_weights_and_empty_tables() {
        let table = [(4, 1), (9, 3)];
        assert_eq!(weighted_pick(&table, 0), 4);
        assert_eq!(weighted_pick(&table, 1), 9);
        assert_eq!(weighted_pick(&table, 3), 9);
        assert_eq!(weighted_pick(&table, 4), 4);
        assert_eq!(weighted_pick(&[], 17), 0);
    }

    #[test]
    fn vigour_falls_with_cold() {
        assert!(vigour(20.0, 1.0) > 0.99);
        assert!(vigour(5.0, 3.0) < 0.2);
        assert!(vigour(5.0, 1.0) > vigour(5.0, 3.0));
    }
}
