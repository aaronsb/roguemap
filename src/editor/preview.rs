//! Preview panes (ADR-003): one renderer, canvas and camera per sprite
//! tier, each drawing the fixture with the real `Renderer` at that tier's
//! first zoom, then blitted into the screen canvas.

use super::fixture::Fixture;
use crate::assets::Tier;
use crate::camera::Camera;
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

pub struct Pane {
    pub tier: Tier,
    pub w: i32,
    pub h: i32,
    renderer: Renderer,
    pub canvas: Canvas,
    cam: Camera,
}

impl Pane {
    fn new(tier: Tier, w: i32, h: i32) -> Pane {
        let (w, h) = (w.max(1), h.max(1));
        Pane { tier, w, h, renderer: Renderer::new(w, h), canvas: Canvas::new(w as u16, h as u16), cam: Camera::new() }
    }
}

/// The set of panes on screen, rebuilt when the layout changes.
#[derive(Default)]
pub struct Preview {
    pub panes: Vec<Pane>,
}

impl Preview {
    /// Make the panes match a list of (tier, interior width, height).
    pub fn resize(&mut self, sizes: &[(Tier, i32, i32)]) {
        let same = self.panes.len() == sizes.len() && self.panes.iter().zip(sizes).all(|(p, &(t, w, h))| p.tier == t && p.w == w && p.h == h);
        if !same {
            self.panes = sizes.iter().map(|&(t, w, h)| Pane::new(t, w, h)).collect();
        }
    }

    /// Draw the fixture into every pane at animation time `t`.
    pub fn render(&mut self, fx: &Fixture, ts: &Tileset, angle: f32, t: f32) {
        let opts = RenderOptions { aa: true, clouds: false };
        let hf = fx.map.get(fx.cx, fx.cy).map(|t| t.hf).unwrap_or(0.0);
        for p in &mut self.panes {
            p.cam.angle = angle;
            p.cam.set_zoom(p.tier.min_zoom(), p.w, p.h);
            // Sprites grow upward from the tile, so the tile sits in the
            // lower part of the pane: aim a quarter of the height above it.
            p.cam.look_at_point(fx.cx as f32 + 0.5, fx.cy as f32 + 0.5, hf + (p.h / 4) as f32, p.w, p.h);
            p.renderer.draw(&mut p.canvas, &Scene::new(&fx.map, ts, &fx.world, &p.cam, t), &opts);
        }
    }
}

/// Copy one canvas into another at an offset; cells off the target are
/// dropped.
pub fn blit(dst: &mut Canvas, src: &Canvas, x0: i32, y0: i32) {
    for y in 0..src.h {
        for x in 0..src.w {
            let c = src.cells[(y * src.w + x) as usize];
            dst.put(x0 + x, y0 + y, c.ch, c.fg, c.bg);
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
        pv.resize(&PANE_SIZES);
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
        blit(&mut dst, &src, 2, 3);
        assert_eq!(dst.cells[(3 * 4 + 2) as usize].ch, 'a');
        assert_eq!(dst.cells[(3 * 4 + 3) as usize].ch, 'b');
        let mut pv = Preview::default();
        pv.resize(&[(Tier::Tiny, 5, 4)]);
        let before = pv.panes.len();
        pv.resize(&[(Tier::Tiny, 5, 4)]);
        assert_eq!(pv.panes.len(), before);
        assert_eq!(pane_title(Tier::Large), "large 16x4");
        assert_eq!(pane_title(Tier::Tiny), "tiny 2x1");
    }
}
