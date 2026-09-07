//! Billboard sprites: the player, props and the one-glyph trees of the
//! smallest zooms, all hand-drawn under `assets/art`. Buildings and larger
//! trees are geometry on the ray walk (ADR-002), not sprites.

/// A billboard sprite: rows top to bottom, all one width. `center` is the
/// column index that sits over the tile centre; the last `base_rows` rows
/// take the base colours (a trunk under a crown). `rows` is the figure at
/// rest and `poses` its walk cycle in order (ADR-008), each the same
/// height and width as `rows`; a sprite that does not walk has none.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub rows: Vec<String>,
    pub poses: Vec<Vec<String>>,
    pub center: i32,
    pub base_rows: usize,
}

/// Glyph pairs that swap when a sprite is mirrored; anything else is its
/// own mirror.
const MIRROR_PAIRS: [(char, char); 5] = [('/', '\\'), ('(', ')'), ('<', '>'), ('[', ']'), ('{', '}')];

/// The glyph a mirrored sprite draws in place of `ch`.
pub fn mirror_glyph(ch: char) -> char {
    for (a, b) in MIRROR_PAIRS {
        if ch == a {
            return b;
        }
        if ch == b {
            return a;
        }
    }
    ch
}

impl Sprite {
    /// The rows of a pose: `None` or a sprite with no poses is the figure
    /// at rest; `Some(i)` is the walk cycle's pose `i`, wrapping.
    pub fn pose(&self, index: Option<usize>) -> &[String] {
        match index {
            Some(i) if !self.poses.is_empty() => &self.poses[i % self.poses.len()],
            _ => &self.rows,
        }
    }

    /// This sprite facing the other way: every row reversed with its
    /// paired glyphs swapped, the centre column mirrored with them.
    pub fn mirrored(&self) -> Sprite {
        let flip = |rows: &[String]| rows.iter().map(|r| r.chars().rev().map(mirror_glyph).collect()).collect::<Vec<String>>();
        let width = self.rows.first().map(|r| r.chars().count()).unwrap_or(1) as i32;
        Sprite { rows: flip(&self.rows), poses: self.poses.iter().map(|p| flip(p)).collect(), center: width - 1 - self.center, base_rows: self.base_rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::biome::FORMS;
    use crate::tileset::Tileset;

    fn check(sp: &Sprite, what: &str) {
        assert!(!sp.rows.is_empty(), "{what}: no rows");
        let width = sp.rows[0].chars().count();
        assert!(width > 0, "{what}: empty rows");
        for r in sp.rows.iter().chain(sp.poses.iter().flatten()) {
            assert_eq!(r.chars().count(), width, "{what}: rows not one width");
        }
        for p in &sp.poses {
            assert_eq!(p.len(), sp.rows.len(), "{what}: a pose of another height");
        }
        assert!((sp.center as usize) < width, "{what}: centre outside the sprite");
        assert!(sp.base_rows <= sp.rows.len(), "{what}: more base rows than rows");
    }

    #[test]
    fn art_sprites_hold_their_invariants() {
        let assets = test_assets();
        for ts in Tileset::all(&assets) {
            for &form in &FORMS {
                check(ts.tree(form), &format!("{} tiny {form:?}", ts.name));
            }
        }
        for e in &assets.art.entries {
            check(&e.sprite, &e.path);
            check(&e.mirrored, &format!("{} mirrored", e.path));
        }
    }

    #[test]
    fn a_mirrored_sprite_swaps_its_paired_glyphs_and_its_centre() {
        let sp = Sprite { rows: vec!["(o) ".to_string(), "/|  ".to_string()], poses: vec![vec![" <o>".to_string(), " /\\ ".to_string()]], center: 1, base_rows: 1 };
        let m = sp.mirrored();
        assert_eq!(m.rows, vec![" (o)".to_string(), "  |\\".to_string()]);
        assert_eq!(m.poses, vec![vec!["<o> ".to_string(), " /\\ ".to_string()]]);
        assert_eq!((m.center, m.base_rows), (2, 1));
        assert_eq!(m.mirrored(), sp, "mirroring twice is the sprite");
        // Poses wrap, and a sprite with none rests whatever is asked.
        assert_eq!(sp.pose(Some(3)), sp.poses[0].as_slice());
        assert_eq!(sp.pose(None), sp.rows.as_slice());
        let still = Sprite { poses: Vec::new(), ..sp.clone() };
        assert_eq!(still.pose(Some(1)), still.rows.as_slice());
    }
}
