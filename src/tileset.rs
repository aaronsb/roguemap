//! Glyph palettes: a tileset maps drawing roles to characters and carries
//! the glyph vocabulary the geometry is textured with, plus the one-glyph
//! tree sprites of the smallest zooms. The sets come from
//! `assets/tilesets/*.toml`: plain ASCII, and PETSCII-style shapes from
//! the Unicode Symbols for Legacy Computing block plus box and block
//! drawing.

use crate::assets::{Assets, Tier, TilesetSpec};
use crate::biome::{Form, COVERS, FORMS};
use crate::sprite::Sprite;

/// Tile footprints as (half width in columns, half height in rows), one per
/// zoom, each an exact halving of the next (ADR-004): far 2x1 (1:8), mid
/// 4x1 (1:4), near 8x2 (1:2), close 16x4 (1:1). Tiles step by these amounts
/// and each footprint tessellates the screen; with 1:2 cells, 2x1 and 4x1
/// read as 45-degree diamonds, the rest 2:1.
pub const ZOOMS: [(i32, i32); 4] = [(2, 1), (4, 1), (8, 2), (16, 4)];

/// The zooms by name, in `ZOOMS` order.
pub const ZOOM_NAMES: [&str; 4] = ["far", "mid", "near", "close"];

/// The scale each zoom draws at, in `ZOOMS` order.
pub const ZOOM_RATIOS: [&str; 4] = ["1:8", "1:4", "1:2", "1:1"];

/// Glyph vocabulary for canopies, trunks, roofs and walls, and the
/// hand-drawn one-glyph tree sprites for the smallest tiles.
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
    /// Bare wood — a shed whorl, a snag's crown — by the direction the
    /// branch runs on screen: vertical, horizontal, rising, falling. A
    /// trunk glyph on a branch that reaches sideways reads as a slab; a
    /// stroke along it reads as a bare branch.
    pub dead_branch: [char; 4],
    /// Sprites for the smallest tiles, per form in `FORMS` order.
    pub tiny: [Sprite; 4],
    pub cactus: char,
    pub roof_fill: char,
    pub door: char,
    pub window: char,
}

#[derive(Clone)]
pub struct Tileset {
    pub name: String,
    /// The glyph vocabulary the geometry draws with.
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
    /// Cliff and wall face glyph for the face pointing screen-right and
    /// screen-left.
    pub wall: [char; 2],
    pub star: [char; 2],
    pub flame: [char; 3],
    pub rain: char,
    pub snowflake: [char; 2],
}

impl Tileset {
    /// The one-glyph tree of a form, for the smallest zooms.
    pub fn tree(&self, form: Form) -> &Sprite {
        let fi = FORMS.iter().position(|&f| f == form).unwrap_or(0);
        &self.art.tiny[fi]
    }

    /// Build the roles and art from a tileset file. The art references
    /// were checked when the assets loaded.
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
            dead_branch: a.dead_branch,
            tiny: [tiny(&a.tiny.pine), tiny(&a.tiny.broadleaf), tiny(&a.tiny.scrub), tiny(&a.tiny.cactus)],
            cactus: a.cactus,
            roof_fill: a.roof_fill,
            door: a.door,
            window: a.window,
        };
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
    fn every_set_has_a_tiny_tree_per_form() {
        let a = test_assets();
        let sets = Tileset::all(&a);
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].name, "petscii");
        for ts in &sets {
            for &form in &FORMS {
                assert!(!ts.tree(form).rows.is_empty(), "{} {form:?}", ts.name);
            }
        }
    }
}
