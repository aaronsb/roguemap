//! Billboard sprites and the procedural shape builders that make them:
//! conifers, broadleaf canopies, scrub, cacti and houses, each sized for a
//! tile footprint and drawn with a tileset's glyph vocabulary. Hand-drawn
//! sprites (the player, props, the tiny tiers) come from `assets/art`.

use crate::biome::{Form, FORMS};
use crate::noise::hash;
use crate::tileset::Art;

/// A billboard sprite: rows top to bottom, all one width. `center` is the
/// column index that sits over the tile centre; the last `base_rows` rows
/// take the base colours (a trunk, or walls under a roof).
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub rows: Vec<String>,
    pub center: i32,
    pub base_rows: usize,
}

fn pad_center(s: &str, width: usize) -> String {
    let len = s.chars().count();
    let left = (width - len) / 2;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(width - len - left))
}

/// Trunk rows under a canopy of `n` rows.
fn trunk_height(n: usize) -> usize {
    (n / 4).max(1)
}

fn trunk_for(art: &Art, n: usize, even: bool) -> &str {
    if n >= 12 {
        &art.trunk[2]
    } else if even || n >= 3 {
        &art.trunk[1]
    } else {
        &art.trunk[0]
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
    Sprite { rows, center: (width / 2) as i32, base_rows: trunk_n }
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
            s.push(art.pine_fill[h.is_multiple_of(5) as usize]);
        }
        s.push(art.pine_r);
        rows.push(s);
    }
    assemble(rows, trunk_for(art, n, true), trunk_height(n))
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
    assemble(rows, trunk_for(art, n, true), trunk_height(n))
}

/// A saguaro-style cactus `n` rows tall with two arms.
fn cactus(art: &Art, n: usize, seed: u64) -> Sprite {
    let n = n.max(2);
    let width = 7usize;
    let mut grid = vec![vec![' '; width]; n];
    for row in grid.iter_mut() {
        row[3] = art.cactus;
    }
    let arm = |grid: &mut [Vec<char>], col: usize, dir: i32, at: usize, len: usize| {
        let elbow = (3_i32 + dir) as usize;
        grid[at][elbow] = art.cactus;
        for row in grid.iter_mut().take(at + 1).skip(at.saturating_sub(len)) {
            row[col] = art.cactus;
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
    Sprite { rows, center: 3, base_rows: 0 }
}

/// A bush: a low broadleaf canopy with no trunk.
fn scrub(art: &Art, n: usize) -> Sprite {
    let mut s = round_tree(art, n.max(1));
    s.rows.truncate(s.rows.len() - s.base_rows);
    s.base_rows = 0;
    s
}

/// Four variants of one form sized for a tile of half width `hw`: a smaller
/// pair then a larger pair.
pub fn trees_for(art: &Art, form: Form, hw: i32) -> Vec<Sprite> {
    let fi = FORMS.iter().position(|&f| f == form).unwrap_or(0);
    if hw <= 2 {
        let t = art.tiny[fi].clone();
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

/// Gable roof rows over walls `wall_w` wide, ending with the eave row that
/// spans the full sprite width.
fn roof(art: &Art, wall_w: usize, roof_h: usize, width: usize) -> Vec<String> {
    let mut rows = Vec::with_capacity(roof_h + 1);
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
    let mut eave = String::new();
    eave.push(art.roof_l);
    for _ in 0..wall_w {
        eave.push(art.roof_fill);
    }
    eave.push(art.roof_r);
    rows.push(eave);
    rows
}

/// Wall rows with a door on the ground row and windows on the top row of
/// wide enough houses, each inset one column under the eave.
fn walls(art: &Art, wall_w: usize, wall_h: usize) -> Vec<String> {
    let mut rows = Vec::with_capacity(wall_h);
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
    rows
}

/// A house: a gable roof over walls with a door and windows. Roof rows take
/// the top colours and wall rows the base colours.
pub fn house(art: &Art, hw: i32, variant: u8) -> Sprite {
    if hw <= 3 {
        return art.tiny_house.clone();
    }
    let wide = variant % 2 == 1;
    let wall_w = (if hw <= 6 { 4 } else if hw <= 12 { 8 } else { 14 }) + (if wide { 2 } else { 0 });
    let roof_h = (wall_w / 4).max(1);
    let wall_h = if hw <= 6 { 1 } else if hw <= 12 { 2 } else { 4 };
    let width = wall_w + 2;
    let mut rows = roof(art, wall_w, roof_h, width);
    rows.extend(walls(art, wall_w, wall_h));
    Sprite { rows, center: (width / 2) as i32, base_rows: wall_h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::tileset::{Tileset, ZOOMS};

    fn check(sp: &Sprite, what: &str) {
        assert!(!sp.rows.is_empty(), "{what}: no rows");
        let width = sp.rows[0].chars().count();
        assert!(width > 0, "{what}: empty rows");
        for r in &sp.rows {
            assert_eq!(r.chars().count(), width, "{what}: rows not one width");
        }
        assert!((sp.center as usize) < width, "{what}: centre outside the sprite");
        assert!(sp.base_rows <= sp.rows.len(), "{what}: more base rows than rows");
    }

    #[test]
    fn tree_sprites_hold_their_invariants() {
        let assets = test_assets();
        for ts in Tileset::all(&assets) {
            let art = &ts.art;
            for &(hw, _) in &ZOOMS {
                for &form in &FORMS {
                    let set = trees_for(art, form, hw);
                    assert_eq!(set.len(), 4, "{form:?} hw {hw}");
                    for (v, sp) in set.iter().enumerate() {
                        check(sp, &format!("{form:?} hw {hw} v{v}"));
                    }
                }
            }
        }
    }

    #[test]
    fn house_and_art_sprites_hold_their_invariants() {
        let assets = test_assets();
        for ts in Tileset::all(&assets) {
            for &(hw, _) in &ZOOMS {
                for v in 0..4 {
                    check(&house(&ts.art, hw, v), &format!("{} house hw {hw} v{v}", ts.name));
                }
            }
        }
        for e in &assets.art.entries {
            check(&e.sprite, &e.path);
        }
    }
}
