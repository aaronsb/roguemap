//! The crown sweep at a low sun. A crown's sweep ends at `MAX_SWEEP`; its
//! start was not capped, so a crown whose underside sits more than
//! `MAX_SWEEP / shadow_per_metre` above the ground began its shadow past
//! where it ended and the mask's clamp panicked.

use std::rc::Rc;

use roguemap::assets::Assets;

/// Seed 204 an hour after sunrise: a chase view sweeps every crown in
/// reach, so it meets a tall crown at the low sun that broke the clamp.
/// The isometric close zooms panicked on the same frame, and the far zoom
/// through its inset.
#[test]
fn a_chase_view_at_a_low_sun_over_a_tall_crown_renders() {
    let assets = Rc::new(Assets::load().expect("assets"));
    let _ = roguemap::snapshot::render(assets, 120, 40, &["camera=chase", "seed=204", "tod=7"]);
}

/// The same hour from the table, where the close zooms sweep crowns too.
#[test]
fn the_close_zoom_at_a_low_sun_renders() {
    let assets = Rc::new(Assets::load().expect("assets"));
    let _ = roguemap::snapshot::render(assets, 120, 40, &["zoom=3", "seed=204", "tod=7"]);
}

/// Dusk, the other end of the window.
#[test]
fn a_chase_view_at_dusk_renders() {
    let assets = Rc::new(Assets::load().expect("assets"));
    let _ = roguemap::snapshot::render(assets, 120, 40, &["camera=chase", "seed=204", "tod=17.2"]);
}
