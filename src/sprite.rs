//! Billboard sprites: the player, props and the one-glyph trees of the
//! smallest zooms, all hand-drawn under `assets/art`. Buildings and larger
//! trees are geometry on the ray walk (ADR-002), not sprites.

/// A billboard sprite: rows top to bottom, all one width. `center` is the
/// column index that sits over the tile centre; the last `base_rows` rows
/// take the base colours (a trunk under a crown).
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub rows: Vec<String>,
    pub center: i32,
    pub base_rows: usize,
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
        for r in &sp.rows {
            assert_eq!(r.chars().count(), width, "{what}: rows not one width");
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
        }
    }
}
