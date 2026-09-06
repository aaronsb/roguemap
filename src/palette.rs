//! Seasonal colour palettes and the unlit surface colour. Surface colours
//! are albedo; lighting is applied afterwards.

use crate::biome;
use crate::canvas::Rgb;
use crate::map::{Terrain, Tile, SEA};
use crate::world::World;

/// Texture rule for one of the plain ground surfaces, indexed by
/// `Terrain::surface`. Grass and water keep bespoke rules in the rasteriser.
pub struct Surface {
    /// Name, the key the asset pass will load the row by.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Fraction of ground cells carrying a texture glyph.
    pub texture_density: f32,
    /// Whether height lifts and wetness darkens the colour; snow stays flat.
    pub relief: bool,
}

/// Sand, dirt, rock, snow.
pub const SURFACES: [Surface; 4] = [
    Surface { name: "sand", texture_density: 0.14, relief: true },
    Surface { name: "dirt", texture_density: 0.22, relief: true },
    Surface { name: "rock", texture_density: 0.24, relief: true },
    Surface { name: "snow", texture_density: 0.18, relief: false },
];

pub const SAND: usize = 0;
pub const DIRT: usize = 1;
pub const ROCK: usize = 2;
pub const SNOW: usize = 3;

/// Unlit colour and texture glyph colour of a plain ground surface.
#[derive(Clone, Copy)]
pub struct SurfaceColors {
    pub color: Rgb,
    pub glyph: Rgb,
}

const fn sc(color: Rgb, glyph: Rgb) -> SurfaceColors {
    SurfaceColors { color, glyph }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub trunk: Rgb,
    pub trunk_glyph: Rgb,
    /// Colours of the plain ground surfaces, in `SURFACES` order.
    pub surfaces: [SurfaceColors; 4],
    pub water_shallow: Rgb,
    pub water_deep: Rgb,
    pub water_glyph: Rgb,
}

pub const SPRING: Palette = Palette {
    trunk: Rgb(92, 58, 34),
    trunk_glyph: Rgb(140, 96, 60),
    surfaces: [
        sc(Rgb(196, 180, 130), Rgb(150, 136, 92)),
        sc(Rgb(122, 92, 60), Rgb(88, 64, 40)),
        sc(Rgb(124, 124, 130), Rgb(90, 90, 96)),
        sc(Rgb(228, 232, 240), Rgb(255, 255, 255)),
    ],
    water_shallow: Rgb(46, 128, 176),
    water_deep: Rgb(16, 48, 104),
    water_glyph: Rgb(176, 214, 240),
};

const SUMMER: Palette = Palette { water_shallow: Rgb(40, 132, 170), water_deep: Rgb(14, 50, 98), ..SPRING };

const AUTUMN: Palette = Palette { water_shallow: Rgb(52, 112, 150), water_deep: Rgb(18, 44, 90), ..SPRING };

const WINTER: Palette = Palette {
    surfaces: [
        sc(Rgb(214, 214, 222), Rgb(170, 170, 184)),
        sc(Rgb(170, 168, 178), Rgb(120, 118, 130)),
        sc(Rgb(196, 200, 212), Rgb(140, 144, 156)),
        sc(Rgb(228, 232, 240), Rgb(255, 255, 255)),
    ],
    water_shallow: Rgb(120, 168, 200),
    water_deep: Rgb(30, 64, 110),
    water_glyph: Rgb(210, 230, 245),
    ..SPRING
};

pub const SEASONS: [Palette; 4] = [SPRING, SUMMER, AUTUMN, WINTER];
pub const SEASON_NAMES: [&str; 4] = ["spring", "summer", "autumn", "winter"];

/// Where a continuous season value in `[0, 4)` falls: the season it is in,
/// the next one, and how far along toward it.
pub fn season_blend(season: f32) -> (usize, usize, f32) {
    let s = season.rem_euclid(4.0);
    let i = s.floor() as usize % 4;
    (i, (i + 1) % 4, s - s.floor())
}

impl Palette {
    pub fn snow(&self) -> Rgb {
        self.surfaces[SNOW].color
    }

    pub fn snow_glyph(&self) -> Rgb {
        self.surfaces[SNOW].glyph
    }

    pub fn dirt(&self) -> Rgb {
        self.surfaces[DIRT].color
    }

    /// Blend between two palettes field by field.
    pub fn lerp(&self, o: &Palette, t: f32) -> Palette {
        let mut surfaces = self.surfaces;
        for (s, other) in surfaces.iter_mut().zip(o.surfaces.iter()) {
            s.color = s.color.lerp(other.color, t);
            s.glyph = s.glyph.lerp(other.glyph, t);
        }
        Palette {
            trunk: self.trunk.lerp(o.trunk, t),
            trunk_glyph: self.trunk_glyph.lerp(o.trunk_glyph, t),
            surfaces,
            water_shallow: self.water_shallow.lerp(o.water_shallow, t),
            water_deep: self.water_deep.lerp(o.water_deep, t),
            water_glyph: self.water_glyph.lerp(o.water_glyph, t),
        }
    }

    /// Palette for a continuous season value in `[0, 4)`; wraps around.
    pub fn for_season(season: f32) -> Palette {
        let (i, j, f) = season_blend(season);
        SEASONS[i].lerp(&SEASONS[j], f)
    }
}

/// Unlit surface colour of a tile: water by depth, ground by biome and
/// season, and a snow blend wherever the seasonal temperature is below
/// freezing.
pub fn surface_color(tile: &Tile, pal: &Palette, world: &World) -> Rgb {
    if tile.terrain == Terrain::Water {
        let depth = ((SEA - tile.z) as f32 / 3.0).clamp(0.0, 1.0);
        return pal.water_shallow.lerp(pal.water_deep, depth);
    }
    let season = world.season;
    let lift = (0.86 + tile.draw_z() as f32 * 0.018) * (1.0 - 0.28 * world.wet_at(tile.temp as f32));
    let c = if let Some(i) = tile.terrain.surface() {
        let color = pal.surfaces[i].color;
        if SURFACES[i].relief {
            color.scale(lift)
        } else {
            color
        }
    } else {
        // Grass: seasonal biomes go dormant as vigour drops.
        let b = tile.biome();
        let g = biome::ground_color(b, season);
        let dormant = Rgb(142, 126, 92);
        let k = if b.seasonal { (1.0 - biome::vigour(tile.temp as f32, season)) * 0.85 } else { 0.0 };
        g.lerp(dormant, k).scale(lift)
    };
    c.lerp(pal.snow(), world.snow_at(tile.temp as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_table_matches_terrain_order() {
        assert_eq!(SURFACES[Terrain::Sand.surface().unwrap()].name, "sand");
        assert_eq!(SURFACES[Terrain::Dirt.surface().unwrap()].name, "dirt");
        assert_eq!(SURFACES[Terrain::Rock.surface().unwrap()].name, "rock");
        assert_eq!(SURFACES[Terrain::Snow.surface().unwrap()].name, "snow");
        assert!(Terrain::Water.surface().is_none());
        assert!(Terrain::Grass.surface().is_none());
    }

    #[test]
    fn season_blend_wraps() {
        assert_eq!(season_blend(0.0), (0, 1, 0.0));
        assert_eq!(season_blend(3.5), (3, 0, 0.5));
        assert_eq!(season_blend(-0.25), (3, 0, 0.75));
        assert_eq!(season_blend(4.0), (0, 1, 0.0));
    }
}
