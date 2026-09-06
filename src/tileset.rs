//! Glyph palettes and level-of-detail sprite caches. A tileset maps drawing
//! roles to characters and holds a sprite set for every tile size, so a
//! tree is one glyph at the overview and a many-row object up close. The
//! glyph sets come from `assets/tilesets/*.toml`: plain ASCII, and
//! PETSCII-style shapes from the Unicode Symbols for Legacy Computing block
//! plus box and block drawing.

use crate::assets::{Assets, Tier, TilesetSpec};
use crate::biome::{Form, COVERS, FORMS};
use crate::sprite::{house, trees_for, Sprite};

/// Tile footprints as (half width in columns, half height in rows). Tiles
/// step by these amounts and each footprint tessellates the screen. With 1:2
/// cells, 2x1 reads as a 45-degree diamond, 3x1 about 1.5:1, the rest 2:1.
pub const ZOOMS: [(i32, i32); 7] = [(2, 1), (3, 1), (4, 1), (6, 2), (8, 2), (12, 3), (16, 4)];

/// Tree sprites indexed by zoom, then form, then variant.
type TreeLod = Vec<Vec<Vec<Sprite>>>;

/// Glyph vocabulary the procedural sprite builders draw with, and the
/// hand-drawn one-glyph sprites for the smallest tiles.
#[derive(Clone, Debug)]
pub struct Art {
    pub pine_l: char,
    pub pine_r: char,
    pub pine_fill: [char; 2],
    pub round_top: [char; 3],
    pub round_mid: [char; 3],
    pub round_bot: [char; 3],
    /// Trunks of width 1, 2 and 4.
    pub trunk: [String; 3],
    /// Sprites for the smallest tiles, per form in `FORMS` order.
    pub tiny: [Sprite; 4],
    pub tiny_house: Sprite,
    pub cactus: char,
    pub roof_l: char,
    pub roof_r: char,
    pub roof_fill: char,
    pub wall_fill: char,
    pub door: char,
    pub window: char,
}

#[derive(Clone)]
pub struct Tileset {
    pub name: String,
    /// The glyph vocabulary the sprites were built from.
    pub art: Art,
    /// Ground cover glyphs per cover kind, for wind leaning left, upright,
    /// leaning right: grass, dry stubble, moss, bare.
    pub cover: [[char; 3]; 4],
    /// Sparse remnants shown as cover dies back toward winter.
    pub stubble: [char; 3],
    /// Reeds in still shallow water.
    pub cattail: [char; 2],
    /// Whether sextant glyphs may be used for edge antialiasing.
    pub antialias: bool,
    /// Water surface glyphs, cycled by wave phase.
    pub water: [char; 4],
    /// Texture glyph pairs for the plain surfaces: sand, dirt, rock, snow.
    pub texture: [[char; 2]; 4],
    /// Cliff face glyph for the face pointing screen-right and screen-left.
    pub wall: [char; 2],
    pub star: [char; 2],
    pub flame: [char; 3],
    pub rain: char,
    pub snowflake: [char; 2],
    /// Tree sprites per zoom level, per form, four variants each.
    tree_lod: TreeLod,
    /// House sprites per zoom level, four variants each.
    house_lod: Vec<Vec<Sprite>>,
}

impl Tileset {
    /// Tree variants of one form for a zoom level.
    pub fn trees(&self, zoom: usize, form: Form) -> &[Sprite] {
        let fi = FORMS.iter().position(|&f| f == form).unwrap_or(0);
        &self.tree_lod[zoom % ZOOMS.len()][fi]
    }

    pub fn houses(&self, zoom: usize) -> &[Sprite] {
        &self.house_lod[zoom % ZOOMS.len()]
    }

    /// Build the roles and sprite caches from a tileset file. The art
    /// references were checked when the assets loaded.
    pub fn from_spec(spec: &TilesetSpec, assets: &Assets) -> Tileset {
        let tiny = |name: &str| assets.art.get(name, Tier::Tiny).cloned().unwrap_or_else(|| panic!("tileset {}: art {name:?} has no tiny tier", spec.name));
        let a = &spec.art;
        let art = Art {
            pine_l: a.pine_l,
            pine_r: a.pine_r,
            pine_fill: a.pine_fill,
            round_top: a.round_top,
            round_mid: a.round_mid,
            round_bot: a.round_bot,
            trunk: a.trunk.clone(),
            tiny: [tiny(&a.tiny.pine), tiny(&a.tiny.broadleaf), tiny(&a.tiny.scrub), tiny(&a.tiny.cactus)],
            tiny_house: tiny(&a.tiny_house),
            cactus: a.cactus,
            roof_l: a.roof_l,
            roof_r: a.roof_r,
            roof_fill: a.roof_fill,
            wall_fill: a.wall_fill,
            door: a.door,
            window: a.window,
        };
        let tree_lod = ZOOMS.iter().map(|&(hw, _)| FORMS.iter().map(|&f| trees_for(&art, f, hw)).collect()).collect();
        let house_lod = ZOOMS.iter().map(|&(hw, _)| (0..4).map(|v| house(&art, hw, v)).collect()).collect();
        let r = &spec.roles;
        let c = &r.cover;
        let cover = [c.grass, c.dry, c.moss, c.bare];
        debug_assert_eq!(cover.len(), COVERS.len());
        Tileset {
            name: spec.name.clone(),
            art,
            cover,
            stubble: r.stubble,
            cattail: r.cattail,
            antialias: spec.antialias,
            water: r.water,
            texture: [r.texture.sand, r.texture.dirt, r.texture.rock, r.texture.snow],
            wall: r.wall,
            star: r.star,
            flame: r.flame,
            rain: r.rain,
            snowflake: r.snowflake,
            tree_lod,
            house_lod,
        }
    }

    /// One tileset per value of the `glyphs` setting, in that order, so the
    /// setting's value indexes the list.
    pub fn all(assets: &Assets) -> Vec<Tileset> {
        let glyphs = assets.setting("glyphs").expect("the glyphs setting is required");
        glyphs.values.iter().map(|v| Tileset::from_spec(assets.tileset(v).expect("checked at load"), assets)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    #[test]
    fn every_zoom_has_every_form_and_house() {
        let a = test_assets();
        let sets = Tileset::all(&a);
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].name, "petscii");
        for ts in &sets {
            for zoom in 0..ZOOMS.len() {
                for &form in &FORMS {
                    assert_eq!(ts.trees(zoom, form).len(), 4, "{} zoom {zoom} {form:?}", ts.name);
                }
                assert_eq!(ts.houses(zoom).len(), 4, "{} zoom {zoom}", ts.name);
            }
        }
    }
}
