//! Every mark the walk draws, on the frames that showed one moving (#35, #50).
//!
//! Sparkle is temporal, so no single frame can show it: the pin is two
//! frames a tick apart with the eye moved a fraction of a cell, and the
//! count of cells whose glyph changed while both colours held. Identical
//! colours mean the same surface under the same shading, so what is left
//! is a mark that moved.
//!
//! The first version of this counted only the ground texture's specks, on
//! one snowy mountain, which is the case #35 fixed. Crowns and roofs took
//! their marks from a lattice no depth ever reached, so they churned at
//! forty times the ground's rate under a count that could not see them
//! (#50). The count here is every glyph on every surface, over one scene
//! per kind of mark.

use std::rc::Rc;

use roguemap::assets::Assets;
use roguemap::canvas::Canvas;
use roguemap::snapshot;

/// A snowy mountain in the unbounded world of seed 7, with the character
/// standing on it: the ground scatter, where the far slopes rake away from
/// the eye and a step of the character sweeps a distant cell's hit point
/// across metres of ground.
const MOUNTAIN: [&str; 9] = ["fill=1", "t=3", "tod=12", "wind=0", "cover=0", "px=-300", "py=-580", "cx=-300", "cy=-580"];

/// A conifer stand seen across a meadow, twenty to forty metres off: the
/// distance at which a screen cell covers several leaf grains.
const FOREST: [&str; 7] = ["fill=1", "t=3", "tod=12", "wind=0", "cx=485", "cy=-300", "deg=0"];

/// The village, from the road below it: roof tiles at the spread of
/// distances a settlement puts them at.
const VILLAGE: [&str; 7] = ["fill=1", "t=3", "tod=12", "wind=0", "cx=130", "cy=-8", "deg=90"];

fn shot(w: u16, h: u16, scene: &[&str], extra: &[&str]) -> Canvas {
    let assets = Rc::new(Assets::load().expect("assets"));
    let mut args: Vec<String> = scene.iter().map(|s| s.to_string()).collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    snapshot::render(assets, w, h, &args)
}

/// Cells carrying both colours unchanged between the two frames, and how
/// many of those changed their glyph.
fn churn(a: &Canvas, b: &Canvas) -> (usize, usize) {
    let n = a.cells.len().min(b.cells.len());
    let held: Vec<usize> = (0..n).filter(|&i| a.cells[i].fg == b.cells[i].fg && a.cells[i].bg == b.cells[i].bg).collect();
    let moved = held.iter().filter(|&&i| a.cells[i].ch != b.cells[i].ch).count();
    (held.len(), moved)
}

/// One 40 ms tick of walking, which the first-person eye follows
/// continuously, so the camera moves a fraction of a cell. `ceiling` is
/// how many of the colour-stable cells in a thousand may change glyph.
fn a_tick_of_walking(w: u16, h: u16, scene: &[&str], ceiling: usize) {
    let view = ["camera=first-person", "pitch=2"];
    let mut walked: Vec<&str> = view.to_vec();
    walked.push("walk=d,0.04");
    let (before, after) = (shot(w, h, scene, &view), shot(w, h, scene, &walked));
    let (held, moved) = churn(&before, &after);
    let cells = before.cells.len();
    assert!(held * 4 >= cells, "{w}x{h}: too little of the frame holds still to say anything about it: {held} of {cells}");
    let share = moved * 1000 / held.max(1);
    assert!(share <= ceiling, "{w}x{h}: {moved} of {held} colour-stable cells changed their glyph in one tick, {share} in a thousand against a ceiling of {ceiling}; a mark is following the camera, not the ground");
}

/// The ground scatter, which #35 anchored in the world: 4 marks in a
/// thousand at 168x71 and 6 at 80x25, where the camera-hung lattice moved
/// more specks than the frame carried.
#[test]
fn the_ground_keeps_its_specks_through_a_tick() {
    a_tick_of_walking(168, 71, &MOUNTAIN, 10);
    a_tick_of_walking(80, 25, &MOUNTAIN, 10);
}

/// Conifer crowns, which #50 band-limited to their own footprint at their
/// own depth: 5 in a thousand at 168x71 and 4 at 80x25, from 7 and 6.
#[test]
fn a_crown_keeps_its_leaves_through_a_tick() {
    a_tick_of_walking(168, 71, &FOREST, 8);
    a_tick_of_walking(80, 25, &FOREST, 8);
}

/// Roof tiles, the other hit kind that took the finest lattice at every
/// distance: 12 in a thousand at 168x71 and 24 at 80x25, from 16 and 30.
/// The small screen carries a sixth of the marks, so its share is the
/// noisier of the two and its ceiling the looser.
#[test]
fn a_roof_keeps_its_tiles_through_a_tick() {
    a_tick_of_walking(168, 71, &VILLAGE, 15);
    a_tick_of_walking(80, 25, &VILLAGE, 28);
}
