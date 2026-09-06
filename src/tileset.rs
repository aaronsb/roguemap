//! Glyph palettes and level-of-detail sprite caches. A tileset maps drawing
//! roles to characters and holds a sprite set for every tile size, so a
//! tree is one glyph at the overview and a many-row object up close. Two
//! glyph sets are provided: plain ASCII, and PETSCII-style shapes from the
//! Unicode Symbols for Legacy Computing block plus box and block drawing.

use crate::biome::{Form, FORMS};
use crate::sprite::{house, player_for, trees_for, Sprite};

/// Tile footprints as (half width in columns, half height in rows). Tiles
/// step by these amounts and each footprint tessellates the screen. With 1:2
/// cells, 2x1 reads as a 45-degree diamond, 3x1 about 1.5:1, the rest 2:1.
pub const ZOOMS: [(i32, i32); 7] = [(2, 1), (3, 1), (4, 1), (6, 2), (8, 2), (12, 3), (16, 4)];

/// Tree sprites indexed by zoom, then form, then variant.
type TreeLod = Vec<Vec<Vec<Sprite>>>;

/// Glyph vocabulary the procedural sprite builders draw with.
pub struct Art {
    pub pine_l: char,
    pub pine_r: char,
    pub pine_fill: [char; 2],
    pub round_top: [char; 3],
    pub round_mid: [char; 3],
    pub round_bot: [char; 3],
    /// Trunks of width 1, 2 and 4.
    pub trunk: [&'static str; 3],
    /// One-glyph sprites for the smallest tiles, per form.
    pub tiny: [&'static [&'static str]; 4],
    pub tiny_house: &'static str,
    pub cactus: char,
    pub roof_l: char,
    pub roof_r: char,
    pub roof_fill: char,
    pub wall_fill: char,
    pub door: char,
    pub window: char,
}

pub const ASCII_ART: Art = Art {
    pine_l: '/',
    pine_r: '\\',
    pine_fill: ['^', '^'],
    round_top: ['.', '-', '.'],
    round_mid: ['(', '@', ')'],
    round_bot: ['`', '-', '\''],
    trunk: ["|", "||", "|##|"],
    tiny: [&["^", "|"], &["@", "|"], &["*"], &["Y"]],
    tiny_house: "n",
    cactus: '|',
    roof_l: '/',
    roof_r: '\\',
    roof_fill: '#',
    wall_fill: ' ',
    door: '|',
    window: 'o',
};

pub const PETSCII_ART: Art = Art {
    pine_l: '◢',
    pine_r: '◣',
    pine_fill: ['▓', '▒'],
    round_top: ['▗', '▄', '▖'],
    round_mid: ['▐', '▓', '▌'],
    round_bot: ['▝', '▀', '▘'],
    trunk: ["▌", "▐▌", "▐██▌"],
    tiny: [&["▲", "▌"], &["▄", "▌"], &["▚"], &["╫"]],
    tiny_house: "⌂",
    cactus: '▌',
    roof_l: '◢',
    roof_r: '◣',
    roof_fill: '▒',
    wall_fill: ' ',
    door: '▐',
    window: '□',
};

#[derive(Clone)]
pub struct Tileset {
    pub name: &'static str,
    /// The glyph vocabulary the sprites were built from, so shapes built
    /// later (the asset pass) draw with this tileset's glyphs.
    #[allow(dead_code)]
    pub art: &'static Art,
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
    player_lod: Vec<Sprite>,
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

    pub fn player(&self, zoom: usize) -> &Sprite {
        &self.player_lod[zoom % ZOOMS.len()]
    }

    fn build(art: &Art) -> (TreeLod, Vec<Vec<Sprite>>, Vec<Sprite>) {
        let trees = ZOOMS.iter().map(|&(hw, _)| FORMS.iter().map(|&f| trees_for(art, f, hw)).collect()).collect();
        let houses = ZOOMS.iter().map(|&(hw, _)| (0..4).map(|v| house(art, hw, v)).collect()).collect();
        let players = ZOOMS.iter().map(|&(hw, _)| player_for(hw)).collect();
        (trees, houses, players)
    }

    pub fn ascii() -> Tileset {
        let (tree_lod, house_lod, player_lod) = Self::build(&ASCII_ART);
        Tileset {
            name: "ascii",
            art: &ASCII_ART,
            cover: [['\\', '|', '/'], [',', '\'', ';'], ['"', '`', '"'], [' ', ' ', ' ']],
            stubble: [';', '.', '\''],
            cattail: [';', 'i'],
            antialias: false,
            water: ['~', '-', '=', ' '],
            texture: [['.', ':'], ['.', ','], ['^', '%'], ['*', '+']],
            wall: [' ', ' '],
            star: ['.', '*'],
            flame: ['^', '*', '^'],
            rain: '|',
            snowflake: ['*', '.'],
            tree_lod,
            house_lod,
            player_lod,
        }
    }

    pub fn petscii() -> Tileset {
        let (tree_lod, house_lod, player_lod) = Self::build(&PETSCII_ART);
        Tileset {
            name: "petscii",
            art: &PETSCII_ART,
            cover: [['╲', '│', '╱'], [',', '\'', ';'], ['·', '∙', '·'], [' ', ' ', ' ']],
            stubble: [';', '.', '`'],
            cattail: [';', '╿'],
            antialias: true,
            water: ['🭸', '🭹', '🭺', '🭷'],
            texture: [['·', '∙'], ['·', '‥'], ['▲', '◆'], ['╳', '·']],
            wall: ['▒', '░'],
            star: ['·', '✦'],
            flame: ['▲', '△', '▲'],
            rain: '🭰',
            snowflake: ['╳', '·'],
            tree_lod,
            house_lod,
            player_lod,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_zoom_has_every_form_and_house() {
        for ts in [Tileset::ascii(), Tileset::petscii()] {
            for zoom in 0..ZOOMS.len() {
                for &form in &FORMS {
                    assert_eq!(ts.trees(zoom, form).len(), 4, "{} zoom {zoom} {form:?}", ts.name);
                }
                assert_eq!(ts.houses(zoom).len(), 4, "{} zoom {zoom}", ts.name);
                assert!(!ts.player(zoom).rows.is_empty());
            }
        }
    }
}
