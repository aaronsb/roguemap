//! Preview panes (ADR-003): one renderer, canvas and camera per sprite
//! tier, each drawing the fixture with the real `Renderer` at that tier's
//! first zoom, then blitted into the screen canvas.

use super::fixture::Fixture;
use crate::assets::schema::TIERS;
use crate::assets::Tier;
use crate::camera::{rows_per_metre_of, Camera};
use crate::canvas::Canvas;
use crate::render::{RenderOptions, Renderer, Scene};
use crate::tileset::{Tileset, ZOOMS};

/// Full-size pane interiors per tier, columns by rows.
pub const PANE_SIZES: [(Tier, i32, i32); 4] = [(Tier::Tiny, 14, 9), (Tier::Small, 22, 13), (Tier::Medium, 36, 22), (Tier::Large, 56, 34)];

/// The nominal interior size of a tier's pane.
pub fn pane_size(tier: Tier) -> (i32, i32) {
    PANE_SIZES.iter().find(|(t, _, _)| *t == tier).map(|&(_, w, h)| (w, h)).unwrap_or((56, 34))
}

/// The pane title: the tier and the tile size it is drawn at, named as
/// the game names zooms (2x1 to 16x4).
pub fn pane_title(tier: Tier) -> String {
    let (hw, hh) = ZOOMS[tier.min_zoom()];
    format!("{} {}x{}", tier.name(), hw, hh)
}

/// Rows a pane needs to show a subject `top` metres tall at a tier: its
/// height in rows per metre at that tile size, and the row the pane's
/// own label sits on.
pub fn rows_for(tier: Tier, top: f32) -> i32 {
    (top * rows_per_metre_of(ZOOMS[tier.min_zoom()].0)).ceil() as i32 + 1
}

/// The tier a pane `w` by `h` can show a subject `top` metres tall at:
/// the largest at or below `want` whose subject fits. This is the
/// editor's LOD, the same shape as `Camera::fitting_zoom` — a tile of the
/// tier has to fit across the pane and the whole subject up it, or the
/// pane clips the thing it is there to show. The drop stops at `small`,
/// however short the pane and however tall the subject: below 1.5 rows a
/// metre trees are one-glyph billboards (`raster::lod_of`) and the pane
/// would show ground rather than the thing being edited. `tiny` is shown
/// only when it is the tier asked for.
pub fn fitting_tier(want: Tier, top: f32, w: i32, h: i32) -> Tier {
    let fits = |t: Tier| rows_for(t, top) <= h && 2 * ZOOMS[t.min_zoom()].0 <= w;
    let floor = want.min_zoom().min(Tier::Small.min_zoom());
    TIERS[floor..=want.min_zoom()].iter().copied().rev().find(|&t| fits(t)).unwrap_or(TIERS[floor])
}

pub struct Pane {
    /// The tier asked for.
    pub tier: Tier,
    /// The tier drawn: `tier`, or a smaller one that fits (see
    /// `fitting_tier`); they differ only in the one-pane layout.
    pub shown: Tier,
    /// Rows `tier` would need, for the label to say why it was dropped.
    pub want_rows: i32,
    pub w: i32,
    pub h: i32,
    renderer: Renderer,
    pub canvas: Canvas,
    cam: Camera,
}

impl Pane {
    fn new(tier: Tier, w: i32, h: i32) -> Pane {
        let (w, h) = (w.max(1), h.max(1));
        Pane { tier, shown: tier, want_rows: h, w, h, renderer: Renderer::new(w, h), canvas: Canvas::new(w as u16, h as u16), cam: Camera::new() }
    }
}

/// The set of panes on screen, rebuilt when the layout changes.
#[derive(Default)]
pub struct Preview {
    pub panes: Vec<Pane>,
    /// Whether a pane may drop to a tier that fits. The full strip shows
    /// the four tiers side by side, so its close panes are close-ups by
    /// design; the one pane of a small screen is the only view there is,
    /// so it has to hold the whole subject.
    fit: bool,
}

impl Preview {
    /// Make the panes match a list of (tier, interior width, height).
    pub fn resize(&mut self, sizes: &[(Tier, i32, i32)], fit: bool) {
        self.fit = fit;
        let same = self.panes.len() == sizes.len() && self.panes.iter().zip(sizes).all(|(p, &(t, w, h))| p.tier == t && p.w == w && p.h == h);
        if !same {
            self.panes = sizes.iter().map(|&(t, w, h)| Pane::new(t, w, h)).collect();
        }
    }

    /// Draw the fixture into every pane at animation time `t`.
    pub fn render(&mut self, fx: &Fixture, ts: &Tileset, angle: f32, t: f32) {
        let opts = RenderOptions { aa: true, clouds: false };
        let hf = fx.map.get(fx.cx, fx.cy).map(|t| t.hf).unwrap_or(0.0);
        let fit = self.fit;
        for p in &mut self.panes {
            p.want_rows = rows_for(p.tier, fx.top);
            p.shown = if fit { fitting_tier(p.tier, fx.top, p.w, p.h) } else { p.tier };
            p.cam.angle = angle;
            p.cam.set_zoom(p.shown.min_zoom(), p.w, p.h);
            // What is shown grows upward from the tile, so the tile sits in
            // the lower part of the pane: aim half way up the subject, and
            // at least a quarter of the pane's rows above the ground
            // (ADR-004: those rows are rows per metre metres).
            let above = (fx.top * 0.5).max((p.h / 4) as f32 / p.cam.rows_per_metre());
            p.cam.look_at_point(fx.cx as f32 + 0.5, fx.cy as f32 + 0.5, hf + above, p.w, p.h);
            p.renderer.draw(&mut p.canvas, &Scene::new(&fx.map, ts, &fx.world, &p.cam, t), &opts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::editor::fields::TableKind;
    use crate::editor::fixture::{build, PreviewSettings, Subject};

    #[test]
    fn oak_stands_in_the_centre_column_at_every_tier() {
        let a = test_assets();
        let ts = Tileset::all(&a);
        let s = PreviewSettings::default();
        let oak = a.species.iter().position(|s| s.name == "oak").unwrap();
        let fx = build(&a, Subject { kind: TableKind::Species, row: oak }, &s);
        let mut pv = Preview::default();
        pv.resize(&PANE_SIZES, false);
        pv.render(&fx, &ts[0], s.angle, 3.0);
        let sky = fx.world.sky();
        for p in &pv.panes {
            let col = p.w / 2;
            let ground = (0..p.h).filter(|&y| p.canvas.cells[(y * p.w + col) as usize].bg != sky).count();
            assert!(ground > p.h as usize / 2, "{}: {ground} of {} centre cells are ground", p.tier.name(), p.h);
        }
        // The canopy colour of an oak in summer shows in the large pane.
        let large = pv.panes.iter().find(|p| p.tier == Tier::Large).unwrap();
        let glyphs: usize = large.canvas.cells.iter().filter(|c| c.ch == ts[0].art.round_mid[1]).count();
        assert!(glyphs > 4, "{glyphs} canopy glyphs");
    }

    #[test]
    fn blit_clips_and_resize_is_lazy() {
        let mut dst = Canvas::new(4, 4);
        let mut src = Canvas::new(3, 3);
        src.text(0, 0, "abc", crate::canvas::Rgb(1, 1, 1), crate::canvas::Rgb(0, 0, 0));
        dst.blit(&src, 2, 3);
        assert_eq!(dst.cells[(3 * 4 + 2) as usize].ch, 'a');
        assert_eq!(dst.cells[(3 * 4 + 3) as usize].ch, 'b');
        let mut pv = Preview::default();
        pv.resize(&[(Tier::Tiny, 5, 4)], false);
        let before = pv.panes.len();
        pv.resize(&[(Tier::Tiny, 5, 4)], false);
        assert_eq!(pv.panes.len(), before);
        assert_eq!(pane_title(Tier::Large), "large 16x4");
        assert_eq!(pane_title(Tier::Tiny), "tiny 2x1");
    }

    /// The one pane of the 80x25 floor is 59 by 11: too short for the
    /// large tier's six rows a metre, so a tall subject drops to a tier
    /// whose whole height fits, and a short one keeps the tier asked for.
    #[test]
    fn the_small_pane_drops_to_a_tier_that_fits() {
        let (w, h) = (59, 11);
        // A campfire is 0.8 m: five rows at 16x4, so it keeps large.
        assert_eq!(fitting_tier(Tier::Large, 0.8, w, h), Tier::Large);
        // A 2 m creature needs thirteen rows at 16x4 and seven at 8x2.
        assert!(rows_for(Tier::Large, 2.0) > h && rows_for(Tier::Medium, 2.0) <= h);
        assert_eq!(fitting_tier(Tier::Large, 2.0, w, h), Tier::Medium);
        // A grown tree fits no tier at all, so the drop stops at small,
        // here and in the taller pane of a screen between the two sizes,
        // where tiny would fit but would show ground, not the tree.
        assert!(rows_for(Tier::Small, 14.4) > h);
        assert_eq!(fitting_tier(Tier::Large, 14.4, w, h), Tier::Small);
        assert!(rows_for(Tier::Tiny, 14.4) <= 18);
        assert_eq!(fitting_tier(Tier::Large, 14.4, 79, 18), Tier::Small);
        // The drop only ever drops: a tier asked for below the fitting
        // one is left alone, and a full-size pane keeps every tier.
        assert_eq!(fitting_tier(Tier::Tiny, 14.4, w, h), Tier::Tiny);
        for &(t, pw, ph) in &PANE_SIZES {
            assert_eq!(fitting_tier(t, 0.8, pw, ph), t);
        }
    }

    /// The pane draws at the tier it dropped to, and only in the one-pane
    /// layout: the full strip's close panes stay close-ups.
    #[test]
    fn the_pane_renders_the_tier_it_shows() {
        let a = test_assets();
        let ts = Tileset::all(&a);
        let s = PreviewSettings::default();
        let oak = a.species.iter().position(|s| s.name == "oak").unwrap();
        let fx = build(&a, Subject { kind: TableKind::Species, row: oak }, &s);
        let mut fit = Preview::default();
        fit.resize(&[(Tier::Large, 59, 11)], true);
        fit.render(&fx, &ts[0], s.angle, 3.0);
        assert_eq!(fit.panes[0].shown, Tier::Small);
        assert_eq!(fit.panes[0].want_rows, rows_for(Tier::Large, fx.top));
        let mut kept = Preview::default();
        kept.resize(&[(Tier::Large, 59, 11)], false);
        kept.render(&fx, &ts[0], s.angle, 3.0);
        assert_eq!(kept.panes[0].shown, Tier::Large);
        // The dropped pane is a different picture: the whole crown is in
        // it, where the large tier had a trunk across the pane.
        let canopy = |p: &Pane| p.canvas.cells.iter().filter(|c| c.ch == ts[0].art.round_mid[1]).count();
        assert!(canopy(&fit.panes[0]) > canopy(&kept.panes[0]), "{} then {}", canopy(&fit.panes[0]), canopy(&kept.panes[0]));
    }
}
