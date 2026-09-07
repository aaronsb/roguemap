//! The ground texture's scatter, on the frames that showed it moving (#35).
//!
//! Sparkle is temporal, so no single frame can show it: the pin is two
//! frames a tick apart with the eye moved a fraction of a cell, and the
//! count of cells whose speck came or went between them. A snowy mountain
//! seen from the ground is where it showed worst — the far slopes rake
//! away from the eye, so a step of the character sweeps a distant cell's
//! hit point across metres of ground.
//!
//! Before the lattice was anchored in the world, a tick moved more specks
//! than the frame had: the whole field re-rolled. The threshold here is a
//! cell in a hundred, which the frames pass with room and the camera-hung
//! lattice failed at both screen sizes.

use std::rc::Rc;

use roguemap::assets::Assets;
use roguemap::canvas::Canvas;
use roguemap::snapshot;
use roguemap::tileset::Tileset;

/// A snowy mountain in the unbounded world of seed 7, with the character
/// standing on it.
const MOUNTAIN: [&str; 7] = ["fill=1", "t=3", "tod=12", "wind=0", "cover=0", "px=-300", "py=-580"];

fn shot(w: u16, h: u16, extra: &[&str]) -> Canvas {
    let assets = Rc::new(Assets::load().expect("assets"));
    let mut args: Vec<String> = MOUNTAIN.iter().map(|s| s.to_string()).collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    snapshot::render(assets, w, h, &args)
}

/// The texture glyphs of the plain surfaces: the specks that sparkled.
fn specks() -> Vec<char> {
    let assets = Rc::new(Assets::load().expect("assets"));
    let sets = Tileset::all(&assets);
    sets[0].texture.iter().flatten().copied().collect()
}

/// Cells carrying a speck in the first frame, and cells whose speck came
/// or went between the two.
fn churn(a: &Canvas, b: &Canvas) -> (usize, usize) {
    let sp = specks();
    let speck = |ch: char| sp.contains(&ch);
    let n = a.cells.len().min(b.cells.len());
    let held = (0..n).filter(|&i| speck(a.cells[i].ch)).count();
    let moved = (0..n).filter(|&i| speck(a.cells[i].ch) != speck(b.cells[i].ch)).count();
    (held, moved)
}

/// One 40 ms tick of walking, which the first-person eye follows
/// continuously, so the camera moves a fraction of a cell.
fn a_tick_of_walking(w: u16, h: u16) {
    let view = ["camera=first-person", "cx=-300", "cy=-580"];
    let mut walked: Vec<&str> = view.to_vec();
    walked.push("walk=d,0.04");
    let (before, after) = (shot(w, h, &view), shot(w, h, &walked));
    let (held, moved) = churn(&before, &after);
    let cells = before.cells.len();
    assert!(held * 200 >= cells, "{w}x{h}: the frame must carry specks to say anything about them: {held} in {cells}");
    assert!(moved * 100 <= cells, "{w}x{h}: {moved} of {cells} cells changed their speck in one tick, of {held} carrying one; the scatter is following the camera, not the ground");
}

#[test]
fn a_tick_of_walking_leaves_the_specks_where_they_are() {
    a_tick_of_walking(168, 71);
}

/// The floor CLAUDE.md sets, where each glyph carries six times the world
/// and there is no detail budget to absorb a moving one.
#[test]
fn the_smallest_screen_holds_its_specks_too() {
    a_tick_of_walking(80, 25);
}
