//! Seasonal colour palettes, the plain-surface texture rules and the unlit
//! surface colour. Surface colours are albedo; lighting is applied
//! afterwards. The tables come from `surfaces.toml`.

use crate::assets::Assets;
use crate::biome;
use crate::canvas::Rgb;
use crate::map::{Terrain, Tile, SEA};
use crate::properties::{Conditions, Identity};
use crate::world::World;

/// Texture rule for one of the plain ground surfaces, indexed by
/// `Terrain::surface`. Grass and water keep bespoke rules in the rasteriser.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub name: String,
    pub identity: Identity,
    /// Fraction of ground cells carrying a texture glyph.
    pub texture_density: f32,
    /// Whether height lifts and wetness darkens the colour; snow stays flat.
    pub relief: bool,
    pub conditions: Conditions,
}

/// Indices into the plain-surface tables: sand, dirt, rock, snow.
pub const SAND: usize = 0;
pub const DIRT: usize = 1;
pub const ROCK: usize = 2;
pub const SNOW: usize = 3;
pub const SURFACE_NAMES: [&str; 4] = ["sand", "dirt", "rock", "snow"];

/// Unlit colour and texture glyph colour of a plain ground surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceColors {
    pub color: Rgb,
    pub glyph: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub trunk: Rgb,
    pub trunk_glyph: Rgb,
    /// Colours of the plain ground surfaces, in `SURFACE_NAMES` order.
    pub surfaces: [SurfaceColors; 4],
    pub water_shallow: Rgb,
    pub water_deep: Rgb,
    pub water_glyph: Rgb,
}

/// One season's palette with its row identity.
#[derive(Clone, Debug, PartialEq)]
pub struct Season {
    pub name: String,
    pub identity: Identity,
    pub palette: Palette,
}

/// Glyph densities of the surfaces with bespoke rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density {
    /// Grass tuft chance per cell at tuft level 0, plus this much per level.
    pub grass_base: f32,
    pub grass_per_level: f32,
    /// Cattail chance per cell in a still pond at full vigour.
    pub cattail: f32,
}

/// Everything from `surfaces.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Surfaces {
    /// Spring, summer, autumn, winter.
    pub seasons: [Season; 4],
    /// Sand, dirt, rock, snow.
    pub surface: [Surface; 4],
    pub density: Density,
}

pub const SEASON_NAMES: [&str; 4] = ["spring", "summer", "autumn", "winter"];

/// Where a continuous season value in `[0, 4)` falls: the season it is in,
/// the next one, and how far along toward it.
pub fn season_blend(season: f32) -> (usize, usize, f32) {
    let s = season.rem_euclid(4.0);
    let i = s.floor() as usize % 4;
    (i, (i + 1) % 4, s - s.floor())
}

impl Surfaces {
    /// Palette for a continuous season value in `[0, 4)`; wraps around.
    pub fn for_season(&self, season: f32) -> Palette {
        let (i, j, f) = season_blend(season);
        self.seasons[i].palette.lerp(&self.seasons[j].palette, f)
    }
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
}

/// Unlit surface colour of a tile: water by depth, ground by biome and
/// season, and a snow blend wherever the seasonal temperature is below
/// freezing. Wetness darkens the ground by the surface row's
/// `wet_darkening`; grass has no row and darkens fully.
pub fn surface_color(tile: &Tile, pal: &Palette, world: &World, assets: &Assets) -> Rgb {
    if tile.terrain == Terrain::Water {
        let depth = ((SEA - tile.z) as f32 / -crate::map::FLOOR).clamp(0.0, 1.0);
        return pal.water_shallow.lerp(pal.water_deep, depth);
    }
    let season = world.season;
    let surface = tile.terrain.surface().map(|i| &assets.surfaces.surface[i]);
    let wet_darkening = surface.map(|s| s.conditions.wet_darkening).unwrap_or(1.0);
    // Height lifts the colour through the relief, not per metre: the world
    // holds 300 m of it (ADR-004).
    let lift = (0.914 + 0.216 * crate::map::relief_fraction(tile.draw_z() as f32)) * (1.0 - 0.28 * world.wet_at(tile.temp as f32) * wet_darkening);
    let c = if let Some(i) = tile.terrain.surface() {
        let color = pal.surfaces[i].color;
        if assets.surfaces.surface[i].relief {
            color.scale(lift)
        } else {
            color
        }
    } else {
        // Grass: seasonal biomes go dormant as vigour drops.
        let b = tile.biome(assets);
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
        let a = Assets::embedded().unwrap();
        let s = &a.surfaces.surface;
        assert_eq!(s[Terrain::Sand.surface().unwrap()].name, "sand");
        assert_eq!(s[Terrain::Dirt.surface().unwrap()].name, "dirt");
        assert_eq!(s[Terrain::Rock.surface().unwrap()].name, "rock");
        assert_eq!(s[Terrain::Snow.surface().unwrap()].name, "snow");
        assert!(Terrain::Water.surface().is_none());
        assert!(Terrain::Grass.surface().is_none());
        assert!(!s[SNOW].relief && s[SAND].relief);
    }

    #[test]
    fn season_blend_wraps() {
        assert_eq!(season_blend(0.0), (0, 1, 0.0));
        assert_eq!(season_blend(3.5), (3, 0, 0.5));
        assert_eq!(season_blend(-0.25), (3, 0, 0.75));
        assert_eq!(season_blend(4.0), (0, 1, 0.0));
    }
}
