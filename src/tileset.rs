//! Glyph palettes and level-of-detail sprites. A tileset maps drawing roles
//! to characters and builds a sprite set for every tile size, so a tree is
//! one glyph at the overview and a many-row object up close. Two glyph sets
//! are provided: plain ASCII, and PETSCII-style shapes from the Unicode
//! Symbols for Legacy Computing block plus box and block drawing.

use crate::biome::{Form, FORMS};
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
    /// One-glyph sprites for the smallest tiles, per form.
    tiny: [&'static [&'static str]; 4],
    tiny_house: &'static str,
    cactus: char,
    roof_l: char,
    roof_r: char,
    roof_fill: char,
    wall_fill: char,
    door: char,
    window: char,
}

const ASCII_ART: Art = Art {
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

const PETSCII_ART: Art = Art {
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

/// A saguaro-style cactus `n` rows tall with two arms.
fn cactus(art: &Art, n: usize, seed: u64) -> Sprite {
    let n = n.max(2);
    let width = 7usize;
    let mut grid = vec![vec![' '; width]; n];
    for row in grid.iter_mut() {
        row[3] = art.cactus;
    }
    let arm = |grid: &mut Vec<Vec<char>>, col: usize, dir: i32, at: usize, len: usize| {
        let elbow = (3 as i32 + dir) as usize;
        grid[at][elbow] = art.cactus;
        for r in at.saturating_sub(len)..=at {
            grid[r][col] = art.cactus;
        }
    };
    if n >= 4 {
        let a = n / 2 + (hash(1, 1, seed) % 2) as usize;
        arm(&mut grid, 1, -1, a.min(n - 1), n / 3);
    }
    if n >= 6 {
        let b = n * 2 / 3;
        arm(&mut grid, 5, 1, b.min(n - 1), n / 4);
    }
    let rows: Vec<String> = grid.into_iter().map(|r| r.into_iter().collect()).collect();
    Sprite { rows, center: 3, trunk_rows: 0 }
}

/// A bush: a low broadleaf canopy with no trunk.
fn scrub(art: &Art, n: usize) -> Sprite {
    let mut s = round_tree(art, n.max(1));
    s.rows.truncate(s.rows.len() - s.trunk_rows);
    s.trunk_rows = 0;
    s
}

/// Four variants of one form sized for a tile of half width `hw`.
fn trees_for(art: &Art, form: Form, hw: i32) -> Vec<Sprite> {
    let fi = FORMS.iter().position(|&f| f == form).unwrap_or(0);
    if hw <= 2 {
        let t = tiny(art.tiny[fi]);
        return vec![t.clone(), t.clone(), t.clone(), t];
    }
    let n = hw as usize;
    match form {
        Form::Pine => vec![pine(art, n, 1), pine(art, n.saturating_sub(1).max(1), 2), pine(art, n + 1, 3), pine(art, n + 2, 4)],
        Form::Broadleaf => vec![round_tree(art, (n * 3 / 4).max(1)), round_tree(art, (n * 2 / 3).max(1)), round_tree(art, (n * 5 / 6).max(1)), round_tree(art, n.max(1))],
        Form::Scrub => vec![scrub(art, (n / 3).max(1)), scrub(art, (n / 2).max(1)), scrub(art, (n / 3).max(1)), scrub(art, (n * 2 / 5).max(1))],
        Form::Cactus => vec![cactus(art, n, 1), cactus(art, n + 2, 2), cactus(art, n.saturating_sub(1), 3), cactus(art, n + 1, 4)],
    }
}

/// A house: a gable roof over walls with a door and windows. Roof rows are
/// the canopy part of the sprite and wall rows the trunk part, so the two
/// material colours map onto the existing sprite colouring.
fn house(art: &Art, hw: i32, variant: u8) -> Sprite {
    if hw <= 3 {
        return tiny(&[art.tiny_house]);
    }
    let wide = variant % 2 == 1;
    let wall_w = if hw <= 6 { 4 } else if hw <= 12 { 8 } else { 14 } + if wide { 2 } else { 0 };
    let roof_h = (wall_w / 4).max(1);
    let wall_h = if hw <= 6 { 1 } else if hw <= 12 { 2 } else { 4 };
    let width = wall_w + 2;
    let mut rows = Vec::new();
    for r in 0..roof_h {
        let inner = (wall_w * (r + 1) / roof_h).max(2).min(wall_w);
        let mut s = String::new();
        s.push(art.roof_l);
        for _ in 0..inner - 2 {
            s.push(art.roof_fill);
        }
        s.push(art.roof_r);
        rows.push(pad_center(&s, width));
    }
    let mut roof_edge = String::new();
    roof_edge.push(art.roof_l);
    for _ in 0..wall_w {
        roof_edge.push(art.roof_fill);
    }
    roof_edge.push(art.roof_r);
    rows.push(roof_edge);
    for r in 0..wall_h {
        let mut s: Vec<char> = vec![art.wall_fill; wall_w];
        if r == wall_h - 1 {
            s[wall_w / 2] = art.door;
        }
        if wall_w >= 6 && r == 0 {
            s[1] = art.window;
            s[wall_w - 2] = art.window;
        }
        let body: String = s.into_iter().collect();
        rows.push(format!(" {} ", body));
    }
    Sprite { rows, center: (width / 2) as i32, trunk_rows: wall_h }
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
    /// Tree sprites per zoom level, per form, four variants each.
    tree_lod: Vec<Vec<Vec<Sprite>>>,
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

    fn build(art: &Art) -> (Vec<Vec<Vec<Sprite>>>, Vec<Vec<Sprite>>, Vec<Sprite>) {
        let trees = ZOOMS.iter().map(|&(hw, _)| FORMS.iter().map(|&f| trees_for(art, f, hw)).collect()).collect();
        let houses = ZOOMS.iter().map(|&(hw, _)| (0..4).map(|v| house(art, hw, v)).collect()).collect();
        let players = ZOOMS.iter().map(|&(hw, _)| player_for(hw)).collect();
        (trees, houses, players)
    }

    pub fn ascii() -> Tileset {
        let (tree_lod, house_lod, player_lod) = Self::build(&ASCII_ART);
        Tileset {
            name: "ascii",
            cover: [['\\', '|', '/'], [',', '\'', ';'], ['"', '`', '"'], [' ', ' ', ' ']],
            stubble: [';', '.', '\''],
            cattail: [';', 'i'],
            antialias: false,
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
            house_lod,
            player_lod,
        }
    }

    pub fn petscii() -> Tileset {
        let (tree_lod, house_lod, player_lod) = Self::build(&PETSCII_ART);
        Tileset {
            name: "petscii",
            cover: [['╲', '│', '╱'], [',', '\'', ';'], ['·', '∙', '·'], [' ', ' ', ' ']],
            stubble: [';', '.', '`'],
            cattail: [';', '╿'],
            antialias: true,
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
            house_lod,
            player_lod,
        }
    }
}
