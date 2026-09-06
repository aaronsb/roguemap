//! Glyph palettes and level-of-detail sprites. A tileset maps drawing roles
//! to characters and builds a sprite set for every tile size, so a tree is
//! one glyph at the overview and a many-row object up close. Two glyph sets
//! are provided: plain ASCII, and PETSCII-style shapes from the Unicode
//! Symbols for Legacy Computing block plus box and block drawing.

use crate::noise::hash;

/// Tile footprints as (half width in columns, half height in rows). Tiles
/// step by these amounts and each footprint tessellates the screen. With 1:2
/// cells, 2x1 reads as a 45-degree diamond, 3x1 about 1.5:1, the rest 2:1.
pub const ZOOMS: [(i32, i32); 7] = [(2, 1), (3, 1), (4, 1), (6, 2), (8, 2), (12, 3), (16, 4)];

/// A billboard sprite: rows top to bottom. `center` is the column index that
/// sits over the tile centre; the last `trunk_rows` rows use trunk colours.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub rows: Vec<String>,
    pub center: i32,
    pub trunk_rows: usize,
}

/// Glyph vocabulary the procedural sprite builders draw with.
struct Art {
    pine_l: char,
    pine_r: char,
    pine_fill: [char; 2],
    round_top: [char; 3],
    round_mid: [char; 3],
    round_bot: [char; 3],
    /// Trunks of width 1, 2 and 4.
    trunk: [&'static str; 3],
    tiny: [&'static [&'static str]; 4],
}

const ASCII_ART: Art = Art {
    pine_l: '/',
    pine_r: '\\',
    pine_fill: ['^', '^'],
    round_top: ['.', '-', '.'],
    round_mid: ['(', '@', ')'],
    round_bot: ['`', '-', '\''],
    trunk: ["|", "||", "|##|"],
    tiny: [&["^", "|"], &["/\\", "||"], &["@", "|"], &["^", "^", "|"]],
};

const PETSCII_ART: Art = Art {
    pine_l: '◢',
    pine_r: '◣',
    pine_fill: ['▓', '▒'],
    round_top: ['▗', '▄', '▖'],
    round_mid: ['▐', '▓', '▌'],
    round_bot: ['▝', '▀', '▘'],
    trunk: ["▌", "▐▌", "▐██▌"],
    tiny: [&["▲", "▌"], &["◢◣", "▐▌"], &["▄", "▌"], &["▲", "▲", "▌"]],
};

fn pad_center(s: &str, width: usize) -> String {
    let len = s.chars().count();
    let left = (width - len) / 2;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(width - len - left))
}

fn trunk_rows(n: usize) -> usize {
    (n / 4).max(1)
}

fn trunk_for(art: &Art, n: usize, even: bool) -> &'static str {
    if n >= 12 {
        art.trunk[2]
    } else if even || n >= 3 {
        art.trunk[1]
    } else {
        art.trunk[0]
    }
}

/// Assemble canopy rows plus a trunk into a sprite, padding rows to the
/// widest one. Widths are even so the sprite centres on an even footprint.
fn assemble(canopy: Vec<String>, trunk: &str, trunk_n: usize) -> Sprite {
    let width = canopy.iter().map(|r| r.chars().count()).max().unwrap_or(1).max(trunk.chars().count());
    let width = if width % 2 == 1 { width + 1 } else { width };
    let mut rows: Vec<String> = canopy.iter().map(|r| pad_center(r, width)).collect();
    for _ in 0..trunk_n {
        rows.push(pad_center(trunk, width));
    }
    Sprite { rows, center: (width / 2) as i32, trunk_rows: trunk_n }
}

/// A conifer with `n` canopy rows. Large trees are drawn in layered tiers.
fn pine(art: &Art, n: usize, seed: u64) -> Sprite {
    let n = n.max(1);
    let tiers = if n < 6 { 1 } else if n < 11 { 2 } else { 3 };
    let len = n.div_ceil(tiers);
    let step = if n >= 8 { 4 } else { 2 };
    let mut rows = Vec::with_capacity(n);
    for r in 0..n {
        let tier = r / len;
        let w = step * (r - tier * len) + 2 + 2 * tier;
        let mut s = String::with_capacity(w);
        s.push(art.pine_l);
        for c in 0..w.saturating_sub(2) {
            let h = hash(r as i64, c as i64, seed);
            s.push(art.pine_fill[(h % 5 == 0) as usize]);
        }
        s.push(art.pine_r);
        rows.push(s);
    }
    assemble(rows, trunk_for(art, n, true), trunk_rows(n))
}

/// A broadleaf tree with an elliptical canopy of `n` rows.
fn round_tree(art: &Art, n: usize) -> Sprite {
    let n = n.max(1);
    let radius = n as f32;
    let mut rows = Vec::with_capacity(n);
    for r in 0..n {
        let y = (r as f32 + 0.5) / n as f32 * 2.0 - 1.0;
        let half = (radius * (1.0 - y * y).sqrt()).round().max(1.0) as usize;
        let w = half * 2;
        let set = if n == 1 {
            &art.round_mid
        } else if r == 0 {
            &art.round_top
        } else if r == n - 1 {
            &art.round_bot
        } else {
            &art.round_mid
        };
        let mut s = String::with_capacity(w);
        s.push(set[0]);
        for _ in 0..w - 2 {
            s.push(set[1]);
        }
        s.push(set[2]);
        rows.push(s);
    }
    assemble(rows, trunk_for(art, n, true), trunk_rows(n))
}

fn tiny(rows: &[&str]) -> Sprite {
    let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(1);
    Sprite {
        rows: rows.iter().map(|r| pad_center(r, width)).collect(),
        center: (width / 2) as i32,
        trunk_rows: 1,
    }
}

/// Four tree variants sized for a tile of half width `hw`.
fn trees_for(art: &Art, hw: i32) -> Vec<Sprite> {
    if hw <= 2 {
        return art.tiny.iter().map(|t| tiny(t)).collect();
    }
    let n = hw as usize;
    vec![
        pine(art, n, 1),
        pine(art, n.saturating_sub(1).max(1), 2),
        round_tree(art, (n * 3 / 4).max(1)),
        pine(art, n + 2, 3),
    ]
}

/// The player figure for a tile of half width `hw`.
fn player_for(hw: i32) -> Sprite {
    let rows: &[&str] = if hw <= 3 {
        &["@"]
    } else if hw <= 6 {
        &[" O ", "/|\\", "/ \\"]
    } else if hw <= 12 {
        &["  _  ", " (o) ", "/|=|\\", " | | ", "_| |_"]
    } else {
        &["   ___   ", "  (o o)  ", "   \\_/   ", " __|=|__ ", "/  |=|  \\", "   |=|   ", "   | |   ", "   | |   ", "  _| |_  "]
    };
    let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(1);
    Sprite { rows: rows.iter().map(|r| r.to_string()).collect(), center: (width / 2) as i32, trunk_rows: 0 }
}

#[derive(Clone)]
pub struct Tileset {
    pub name: &'static str,
    /// Grass tuft for wind leaning left, upright, leaning right.
    pub grass: [char; 3],
    /// Water surface glyphs, cycled by wave phase.
    pub water: [char; 4],
    pub sand: [char; 2],
    pub dirt: [char; 2],
    pub rock: [char; 2],
    pub snow: [char; 2],
    /// Cliff face glyph for the face pointing screen-right and screen-left.
    pub wall: [char; 2],
    pub star: [char; 2],
    pub flame: [char; 3],
    pub rain: char,
    pub snowflake: [char; 2],
    /// Tree sprites per zoom level, four variants each.
    tree_lod: Vec<Vec<Sprite>>,
    player_lod: Vec<Sprite>,
}

impl Tileset {
    /// Tree variants for a zoom level.
    pub fn trees(&self, zoom: usize) -> &[Sprite] {
        &self.tree_lod[zoom % ZOOMS.len()]
    }

    pub fn player(&self, zoom: usize) -> &Sprite {
        &self.player_lod[zoom % ZOOMS.len()]
    }

    fn build(art: &Art) -> (Vec<Vec<Sprite>>, Vec<Sprite>) {
        let trees = ZOOMS.iter().map(|&(hw, _)| trees_for(art, hw)).collect();
        let players = ZOOMS.iter().map(|&(hw, _)| player_for(hw)).collect();
        (trees, players)
    }

    pub fn ascii() -> Tileset {
        let (tree_lod, player_lod) = Self::build(&ASCII_ART);
        Tileset {
            name: "ascii",
            grass: ['\\', '|', '/'],
            water: ['~', '-', '=', ' '],
            sand: ['.', ':'],
            dirt: ['.', ','],
            rock: ['^', '%'],
            snow: ['*', '+'],
            wall: [' ', ' '],
            star: ['.', '*'],
            flame: ['^', '*', '^'],
            rain: '|',
            snowflake: ['*', '.'],
            tree_lod,
            player_lod,
        }
    }

    pub fn petscii() -> Tileset {
        let (tree_lod, player_lod) = Self::build(&PETSCII_ART);
        Tileset {
            name: "petscii",
            grass: ['╲', '│', '╱'],
            water: ['🭸', '🭹', '🭺', '🭷'],
            sand: ['·', '∙'],
            dirt: ['·', '‥'],
            rock: ['▲', '◆'],
            snow: ['╳', '·'],
            wall: ['▒', '░'],
            star: ['·', '✦'],
            flame: ['▲', '△', '▲'],
            rain: '🭰',
            snowflake: ['╳', '·'],
            tree_lod,
            player_lod,
        }
    }
}
