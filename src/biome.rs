//! Climate, biomes, tree species and building materials as data tables.
//!
//! Climate is temperature and precipitation. A simplified Köppen scheme maps
//! the pair to a biome, and the biome names its ground colour, how many trees
//! it carries, which species, and what its buildings are made of.

use crate::canvas::Rgb;

/// Shape a species is drawn with; the tileset builds sprites per form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Pine,
    Broadleaf,
    Scrub,
    Cactus,
}

pub const FORMS: [Form; 4] = [Form::Pine, Form::Broadleaf, Form::Scrub, Form::Cactus];

pub struct Species {
    pub name: &'static str,
    pub form: Form,
    /// Size relative to the zoom's base tree size.
    pub scale: f32,
    /// Canopy colour by season: spring, summer, autumn, winter.
    pub canopy: [Rgb; 4],
    pub canopy_glyph: [Rgb; 4],
}

const fn evergreen(c: Rgb, g: Rgb) -> ([Rgb; 4], [Rgb; 4]) {
    ([c, c, c, c], [g, g, g, g])
}

macro_rules! species {
    ($name:expr, $form:expr, $scale:expr, ev $c:expr, $g:expr) => {{
        let (canopy, canopy_glyph) = evergreen($c, $g);
        Species { name: $name, form: $form, scale: $scale, canopy, canopy_glyph }
    }};
    ($name:expr, $form:expr, $scale:expr, $c:expr, $g:expr) => {
        Species { name: $name, form: $form, scale: $scale, canopy: $c, canopy_glyph: $g }
    };
}

pub const SPECIES: &[Species] = &[
    species!(
        "oak",
        Form::Broadleaf,
        1.0,
        [Rgb(72, 142, 60), Rgb(40, 110, 50), Rgb(180, 100, 40), Rgb(105, 82, 64)],
        [Rgb(130, 200, 100), Rgb(80, 160, 80), Rgb(230, 150, 60), Rgb(140, 118, 96)]
    ),
    species!(
        "birch",
        Form::Broadleaf,
        0.8,
        [Rgb(120, 190, 90), Rgb(90, 160, 70), Rgb(220, 180, 60), Rgb(150, 140, 130)],
        [Rgb(180, 230, 140), Rgb(150, 210, 120), Rgb(250, 220, 110), Rgb(210, 205, 200)]
    ),
    species!("pine", Form::Pine, 1.0, ev Rgb(30, 92, 50), Rgb(70, 140, 80)),
    species!("spruce", Form::Pine, 1.3, ev Rgb(22, 72, 46), Rgb(56, 118, 74)),
    species!("juniper", Form::Scrub, 0.5, ev Rgb(70, 110, 70), Rgb(120, 160, 110)),
    species!("sagebrush", Form::Scrub, 0.4, ev Rgb(130, 140, 110), Rgb(180, 190, 150)),
    species!("saguaro", Form::Cactus, 0.9, ev Rgb(78, 138, 78), Rgb(150, 200, 130)),
    species!("kapok", Form::Broadleaf, 1.5, ev Rgb(30, 100, 45), Rgb(70, 150, 80)),
    species!(
        "acacia",
        Form::Broadleaf,
        0.9,
        [Rgb(110, 140, 60), Rgb(100, 130, 55), Rgb(130, 120, 50), Rgb(120, 110, 60)],
        [Rgb(170, 200, 100), Rgb(160, 190, 90), Rgb(190, 170, 80), Rgb(170, 160, 100)]
    ),
];

pub struct Material {
    pub name: &'static str,
    pub wall: Rgb,
    pub wall_glyph: Rgb,
    pub roof: Rgb,
    pub roof_glyph: Rgb,
}

pub const MATERIALS: &[Material] = &[
    Material { name: "adobe", wall: Rgb(198, 152, 108), wall_glyph: Rgb(150, 108, 70), roof: Rgb(156, 92, 60), roof_glyph: Rgb(200, 130, 90) },
    Material { name: "wood", wall: Rgb(112, 76, 46), wall_glyph: Rgb(160, 118, 76), roof: Rgb(72, 46, 30), roof_glyph: Rgb(120, 84, 56) },
    Material { name: "stone", wall: Rgb(138, 138, 144), wall_glyph: Rgb(96, 96, 104), roof: Rgb(88, 90, 100), roof_glyph: Rgb(130, 132, 142) },
];

pub const ADOBE: usize = 0;
pub const WOOD: usize = 1;
pub const STONE: usize = 2;

/// Ground cover kinds; the tileset carries a glyph triple for each.
pub const COVER_GRASS: usize = 0;
pub const COVER_DRY: usize = 1;
pub const COVER_MOSS: usize = 2;
pub const COVER_BARE: usize = 3;

pub struct Biome {
    pub name: &'static str,
    pub koppen: &'static str,
    /// Ground cover kind drawn over the ground colour.
    pub cover: usize,
    /// Summer ground colour and glyph colour; seasons modulate them.
    pub ground: Rgb,
    pub ground_glyph: Rgb,
    /// Whether the ground colour follows the seasons.
    pub seasonal: bool,
    /// Grass tuft density, 0..=3.
    pub grass: u8,
    /// Fraction of eligible tiles carrying a tree in the densest patches.
    pub tree_density: f32,
    /// Species indices with relative weights.
    pub species: &'static [(usize, u8)],
    pub material: usize,
}

pub const BIOMES: &[Biome] = &[
    Biome { name: "rainforest", cover: COVER_GRASS, koppen: "Af", ground: Rgb(50, 122, 52), ground_glyph: Rgb(110, 190, 100), seasonal: false, grass: 3, tree_density: 0.9, species: &[(7, 6), (0, 2)], material: WOOD },
    Biome { name: "savanna", cover: COVER_DRY, koppen: "Aw", ground: Rgb(162, 150, 72), ground_glyph: Rgb(210, 195, 110), seasonal: false, grass: 2, tree_density: 0.15, species: &[(8, 5), (4, 1)], material: ADOBE },
    Biome { name: "desert", cover: COVER_BARE, koppen: "BW", ground: Rgb(206, 176, 122), ground_glyph: Rgb(170, 140, 90), seasonal: false, grass: 0, tree_density: 0.05, species: &[(6, 5), (5, 1)], material: ADOBE },
    Biome { name: "steppe", cover: COVER_DRY, koppen: "BS", ground: Rgb(172, 160, 92), ground_glyph: Rgb(210, 200, 130), seasonal: true, grass: 1, tree_density: 0.08, species: &[(5, 4), (4, 2)], material: ADOBE },
    Biome { name: "mediterranean", cover: COVER_DRY, koppen: "Cs", ground: Rgb(140, 150, 72), ground_glyph: Rgb(190, 200, 110), seasonal: true, grass: 2, tree_density: 0.35, species: &[(4, 4), (0, 2)], material: STONE },
    Biome { name: "temperate forest", cover: COVER_GRASS, koppen: "Cf", ground: Rgb(86, 150, 60), ground_glyph: Rgb(150, 210, 100), seasonal: true, grass: 3, tree_density: 0.6, species: &[(0, 5), (1, 3), (2, 1)], material: WOOD },
    Biome { name: "boreal forest", cover: COVER_MOSS, koppen: "Df", ground: Rgb(70, 112, 62), ground_glyph: Rgb(120, 170, 100), seasonal: true, grass: 1, tree_density: 0.7, species: &[(3, 5), (2, 3), (1, 1)], material: WOOD },
    Biome { name: "tundra", cover: COVER_MOSS, koppen: "ET", ground: Rgb(122, 132, 102), ground_glyph: Rgb(170, 180, 140), seasonal: true, grass: 1, tree_density: 0.05, species: &[(4, 3), (5, 1)], material: STONE },
    Biome { name: "ice cap", cover: COVER_BARE, koppen: "EF", ground: Rgb(226, 232, 240), ground_glyph: Rgb(255, 255, 255), seasonal: false, grass: 0, tree_density: 0.0, species: &[], material: STONE },
];

/// Simplified Köppen classification from annual mean temperature in degrees
/// Celsius and precipitation on a 0..100 scale.
pub fn classify(temp: f32, precip: f32) -> usize {
    if temp <= -16.0 {
        8
    } else if temp <= -6.0 {
        7
    } else if precip < 22.0 {
        2
    } else if precip < 42.0 {
        3
    } else if temp >= 18.0 {
        if precip >= 68.0 {
            0
        } else {
            1
        }
    } else if temp >= 3.0 {
        if precip < 58.0 {
            4
        } else {
            5
        }
    } else {
        6
    }
}

/// Blend a four-season table at a continuous season value in `[0, 4)`.
pub fn seasonal(table: &[Rgb; 4], season: f32) -> Rgb {
    let s = season.rem_euclid(4.0);
    let i = s.floor() as usize % 4;
    table[i].lerp(table[(i + 1) % 4], s - s.floor())
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
        assert_eq!(BIOMES[classify(25.0, 80.0)].koppen, "Af");
        assert_eq!(BIOMES[classify(25.0, 50.0)].koppen, "Aw");
        assert_eq!(BIOMES[classify(20.0, 10.0)].koppen, "BW");
        assert_eq!(BIOMES[classify(10.0, 30.0)].koppen, "BS");
        assert_eq!(BIOMES[classify(12.0, 50.0)].koppen, "Cs");
        assert_eq!(BIOMES[classify(8.0, 70.0)].koppen, "Cf");
        assert_eq!(BIOMES[classify(-2.0, 70.0)].koppen, "Df");
        assert_eq!(BIOMES[classify(-10.0, 70.0)].koppen, "ET");
        assert_eq!(BIOMES[classify(-20.0, 70.0)].koppen, "EF");
    }

    #[test]
    fn species_and_materials_resolve() {
        for b in BIOMES {
            assert!(b.material < MATERIALS.len(), "{}", b.name);
            for &(sp, w) in b.species {
                assert!(sp < SPECIES.len() && w > 0, "{}", b.name);
            }
        }
    }

    #[test]
    fn vigour_falls_with_cold() {
        assert!(vigour(20.0, 1.0) > 0.99);
        assert!(vigour(5.0, 3.0) < 0.2);
        assert!(vigour(5.0, 1.0) > vigour(5.0, 3.0));
    }
}
