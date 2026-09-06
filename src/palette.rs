//! Seasonal colour palettes. Surface colours are unlit albedo; lighting is
//! applied afterwards.

use crate::canvas::Rgb;

#[derive(Clone, Copy)]
pub struct Palette {
    pub grass: Rgb,
    pub grass_glyph: Rgb,
    pub canopy: Rgb,
    pub canopy_glyph: Rgb,
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

const SPRING: Palette = Palette {
    grass: Rgb(92, 158, 66),
    grass_glyph: Rgb(178, 224, 110),
    canopy: Rgb(38, 108, 56),
    canopy_glyph: Rgb(96, 170, 96),
    trunk: Rgb(92, 58, 34),
    trunk_glyph: Rgb(140, 96, 60),
    sand: Rgb(196, 180, 130),
    sand_glyph: Rgb(150, 136, 92),
    dirt: Rgb(122, 92, 60),
    dirt_glyph: Rgb(88, 64, 40),
    rock: Rgb(124, 124, 130),
    rock_glyph: Rgb(90, 90, 96),
    snow: Rgb(228, 232, 240),
    snow_glyph: Rgb(255, 255, 255),
    water_shallow: Rgb(46, 128, 176),
    water_deep: Rgb(16, 48, 104),
    water_glyph: Rgb(176, 214, 240),
};

const SUMMER: Palette = Palette {
    grass: Rgb(78, 140, 54),
    grass_glyph: Rgb(140, 200, 90),
    canopy: Rgb(30, 92, 44),
    canopy_glyph: Rgb(70, 140, 76),
    water_shallow: Rgb(40, 132, 170),
    water_deep: Rgb(14, 50, 98),
    ..SPRING
};

const AUTUMN: Palette = Palette {
    grass: Rgb(150, 128, 58),
    grass_glyph: Rgb(210, 176, 80),
    canopy: Rgb(160, 84, 36),
    canopy_glyph: Rgb(230, 150, 60),
    water_shallow: Rgb(52, 112, 150),
    water_deep: Rgb(18, 44, 90),
    ..SPRING
};

const WINTER: Palette = Palette {
    grass: Rgb(222, 228, 238),
    grass_glyph: Rgb(180, 188, 204),
    canopy: Rgb(196, 208, 220),
    canopy_glyph: Rgb(60, 96, 80),
    sand: Rgb(214, 214, 222),
    sand_glyph: Rgb(170, 170, 184),
    dirt: Rgb(170, 168, 178),
    dirt_glyph: Rgb(120, 118, 130),
    rock: Rgb(196, 200, 212),
    rock_glyph: Rgb(140, 144, 156),
    water_shallow: Rgb(120, 168, 200),
    water_deep: Rgb(30, 64, 110),
    water_glyph: Rgb(210, 230, 245),
    ..SPRING
};

pub const SEASONS: [Palette; 4] = [SPRING, SUMMER, AUTUMN, WINTER];
pub const SEASON_NAMES: [&str; 4] = ["spring", "summer", "autumn", "winter"];

impl Palette {
    /// Blend between two palettes field by field.
    pub fn lerp(&self, o: &Palette, t: f32) -> Palette {
        Palette {
            grass: self.grass.lerp(o.grass, t),
            grass_glyph: self.grass_glyph.lerp(o.grass_glyph, t),
            canopy: self.canopy.lerp(o.canopy, t),
            canopy_glyph: self.canopy_glyph.lerp(o.canopy_glyph, t),
            trunk: self.trunk.lerp(o.trunk, t),
            trunk_glyph: self.trunk_glyph.lerp(o.trunk_glyph, t),
            sand: self.sand.lerp(o.sand, t),
            sand_glyph: self.sand_glyph.lerp(o.sand_glyph, t),
            dirt: self.dirt.lerp(o.dirt, t),
            dirt_glyph: self.dirt_glyph.lerp(o.dirt_glyph, t),
            rock: self.rock.lerp(o.rock, t),
            rock_glyph: self.rock_glyph.lerp(o.rock_glyph, t),
            snow: self.snow.lerp(o.snow, t),
            snow_glyph: self.snow_glyph.lerp(o.snow_glyph, t),
            water_shallow: self.water_shallow.lerp(o.water_shallow, t),
            water_deep: self.water_deep.lerp(o.water_deep, t),
            water_glyph: self.water_glyph.lerp(o.water_glyph, t),
        }
    }

    /// Palette for a continuous season value in `[0, 4)`; wraps around.
    pub fn for_season(season: f32) -> Palette {
        let s = season.rem_euclid(4.0);
        let i = s.floor() as usize % 4;
        let j = (i + 1) % 4;
        SEASONS[i].lerp(&SEASONS[j], s - s.floor())
    }
}
