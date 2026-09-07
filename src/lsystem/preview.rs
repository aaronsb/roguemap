//! A side view of a `TreeModel` drawn into a `Canvas`, so an L-system can be
//! looked at with `roguemap --snap-tree NAME OUT.cells` and
//! `python3 tools/cells2png.py`.
//!
//! The projection is orthographic from the side: x runs across the screen, z
//! up, and y is depth, used only to order what is drawn. Cells are twice as
//! tall as they are wide in the canonical font, so a metre is twice as many
//! columns as rows and the tree keeps its proportions.
//!
//! Branches are drawn as lines whose glyph follows their thickness in
//! columns: `|` (or the matching diagonal) for a thin twig, a half block for
//! a branch about a cell wide, a solid column for a trunk. Leaf clusters are
//! filled ellipses in the canopy colour, textured with the tileset's canopy
//! glyph pool and edged with its quadrant and half-block glyphs, so a blob
//! has a rounded outline instead of a staircase.

use crate::assets::Assets;
use crate::biome::seasonal;
use crate::canvas::{Canvas, Rgb};
use crate::noise::hash01;
use crate::palette::DIRT;
use crate::tileset::Tileset;

use super::{dead_bark, Growth, State, TreeModel};

/// Mean annual temperature the previews stand in, in degrees Celsius: a
/// temperate lowland, so a deciduous species is in full leaf in summer and
/// bare in winter (`Species::foliage`).
pub const PREVIEW_CLIMATE: f32 = 4.0;

/// Colours and glyphs the preview draws with. The glyph fields come from a
/// tileset's `[art]` vocabulary through `PreviewStyle::from_tileset`.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewStyle {
    /// Behind the tree.
    pub sky: Rgb,
    /// The ground line under it.
    pub ground: Rgb,
    pub trunk: Rgb,
    pub trunk_glyph: Rgb,
    /// Bark of a shed whorl: deadwood on a living tree.
    pub dead_trunk: Rgb,
    pub dead_trunk_glyph: Rgb,
    pub canopy: Rgb,
    pub canopy_glyph: Rgb,
    /// Canopy fill glyphs, sprinkled over the blobs.
    pub fill: Vec<char>,
    /// Quadrant glyphs for a blob's corners: upper left, upper right, lower
    /// left, lower right.
    pub quadrant: [char; 4],
    /// Half blocks for a blob's sides: left, right, top, bottom.
    pub half: [char; 4],
    /// Thin branch glyphs by direction: vertical, horizontal, rising,
    /// falling.
    pub thin: [char; 4],
    /// A branch at least a cell wide.
    pub solid: char,
    /// How many canopy cells carry a fill glyph, 0..1.
    pub fill_density: f32,
}

impl Default for PreviewStyle {
    fn default() -> PreviewStyle {
        PreviewStyle {
            sky: Rgb(16, 20, 28),
            ground: Rgb(58, 50, 40),
            trunk: Rgb(92, 72, 56),
            trunk_glyph: Rgb(130, 104, 80),
            dead_trunk: dead_bark(Rgb(92, 72, 56)),
            dead_trunk_glyph: dead_bark(Rgb(130, 104, 80)),
            canopy: Rgb(56, 120, 62),
            canopy_glyph: Rgb(110, 176, 96),
            fill: vec!['▓', '▒'],
            quadrant: ['▘', '▝', '▖', '▗'],
            half: ['▌', '▐', '▀', '▄'],
            thin: ['|', '─', '/', '\\'],
            solid: '█',
            fill_density: 0.45,
        }
    }
}

impl PreviewStyle {
    /// The glyph vocabulary of a tileset with a species' colours. The
    /// quadrant and half-block roles are `round_top`, `round_mid` and
    /// `round_bot`, which are exactly the canopy outline glyphs the sprite
    /// builders draw round crowns with; the fill pool is `round_mid[1]` and
    /// `pine_fill`.
    pub fn from_tileset(ts: &Tileset, canopy: Rgb, canopy_glyph: Rgb, trunk: Rgb, trunk_glyph: Rgb) -> PreviewStyle {
        let a = &ts.art;
        PreviewStyle {
            canopy,
            canopy_glyph,
            trunk,
            trunk_glyph,
            dead_trunk: dead_bark(trunk),
            dead_trunk_glyph: dead_bark(trunk_glyph),
            fill: vec![a.round_mid[1], a.pine_fill[0], a.pine_fill[1]],
            // round_top is the lower half of a crown's top row: lower left,
            // lower half, lower right; round_bot the upper half.
            quadrant: [a.round_bot[2], a.round_bot[0], a.round_top[2], a.round_top[0]],
            half: [a.round_mid[2], a.round_mid[0], a.round_bot[1], a.round_top[1]],
            ..PreviewStyle::default()
        }
    }
}

/// Render one species side-on, as `roguemap --snap-tree NAME OUT.cells
/// [cols rows seed season] [foliage=F] [state=dead]` asks for it.
///
/// Positional arguments are taken in order and anything with an `=` is a
/// keyword, so the two forms mix. `season` is the continuous 0..4 the world
/// uses; the foliage a live tree carries follows from it and the species,
/// and `foliage=` overrides. The name is written along the top with the
/// tree's size and how many branches and clusters it came to.
pub fn snap(assets: &Assets, name: &str, args: &[String]) -> Result<Canvas, String> {
    let mut pos: Vec<&str> = Vec::new();
    let mut kv: Vec<(&str, &str)> = Vec::new();
    for a in args {
        match a.split_once('=') {
            Some((k, v)) => kv.push((k, v)),
            None => pos.push(a),
        }
    }
    let get = |k: &str| kv.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    let num = |i: usize, k: &str, d: f32| get(k).or_else(|| pos.get(i).copied()).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let (cols, rows) = (num(0, "cols", 120.0) as u16, num(1, "rows", 60.0) as u16);
    let seed = num(2, "seed", 7.0) as u64;
    let season = num(3, "season", 1.0);

    let sp = assets.species.iter().find(|s| s.name == name).ok_or_else(|| {
        let known: Vec<&str> = assets.species.iter().filter(|s| s.lsystem.is_some()).map(|s| s.name.as_str()).collect();
        format!("no species named {name:?}; the L-system rows are {}", known.join(", "))
    })?;
    let state = if get("state").is_some_and(|v| v == "dead") { State::Dead } else { State::Alive };
    let foliage = match get("foliage").and_then(|v| v.parse::<f32>().ok()) {
        Some(f) => f,
        None => sp.foliage(PREVIEW_CLIMATE, season),
    };
    let model = sp.tree_model(seed, Growth { foliage, state }).ok_or_else(|| format!("species {name:?} is not shape = \"lsystem\"; it has no grammar to grow"))?;

    let pal = assets.surfaces.for_season(season);
    let bark = model.bark(pal.trunk);
    let ts = &Tileset::all(assets)[0];
    let mut style = PreviewStyle::from_tileset(ts, seasonal(&sp.canopy, season), seasonal(&sp.canopy_glyph, season), bark, model.bark(pal.trunk_glyph));
    style.ground = pal.surfaces[DIRT].color;
    let mut cv = preview(&model, cols, rows, &style);
    let size = model.bounds.size();
    let head = format!(
        "{name}  {:.1} x {:.1} x {:.1} m  seed {seed}  season {season:.1}  {}  {} branches  {} clusters",
        size[0],
        size[1],
        size[2],
        if state == State::Dead { "dead".to_string() } else { format!("foliage {foliage:.2}") },
        model.segments.len(),
        model.leaves.len()
    );
    cv.text(1, 0, &head, Rgb(150, 150, 160), style.sky);
    Ok(cv)
}

/// Where a metre lands on the canvas.
struct View {
    /// Columns and rows per metre; columns are twice rows.
    cpm: f32,
    rpm: f32,
    /// Canvas position of the model's origin.
    x0: f32,
    y0: f32,
}

impl View {
    fn col(&self, x: f32) -> f32 {
        self.x0 + x * self.cpm
    }

    fn row(&self, z: f32) -> f32 {
        self.y0 - z * self.rpm
    }
}

/// Draw `model` side-on into a fresh `cols` x `rows` canvas.
///
/// The whole model is fitted into the canvas with a margin, keeping its
/// proportions; the bottom row is the ground it stands on. Farther geometry
/// is drawn first, so a near branch covers a far one and a canopy blob
/// closes over the twigs inside it.
pub fn preview(model: &TreeModel, cols: u16, rows: u16, style: &PreviewStyle) -> Canvas {
    let mut cv = Canvas::new(cols, rows);
    for c in cv.cells.iter_mut() {
        c.bg = style.sky;
    }
    let (w, h) = (cols as i32, rows as i32);
    let ground = |cv: &mut Canvas| {
        for x in 0..w {
            cv.put(x, h - 1, ' ', style.ground, style.ground);
        }
    };
    ground(&mut cv);
    let size = model.bounds.size();
    let top = model.bounds.max[2];
    if model.bounds.is_empty() || top <= 0.0 {
        return cv;
    }
    // One row is two columns of the same distance in the canonical font.
    // Height is measured from the foot of the trunk, so a weeping species'
    // whips run past the ground line and the ground covers them.
    let usable_rows = (h - 2).max(1) as f32;
    let usable_cols = (w - 2).max(1) as f32;
    let rpm = (usable_rows / top).min(usable_cols / (2.0 * size[0].max(1e-3)));
    let view = View { cpm: rpm * 2.0, rpm, x0: w as f32 * 0.5 - model.bounds.centre()[0] * rpm * 2.0, y0: h as f32 - 1.0 };

    // Painter's order: the viewer stands at y = -infinity, so larger y is
    // farther and is drawn first.
    let mut order: Vec<(f32, usize, usize)> = Vec::with_capacity(model.segments.len() + model.leaves.len());
    order.extend(model.segments.iter().enumerate().map(|(i, s)| ((s.a[1] + s.b[1]) * 0.5, 0, i)));
    order.extend(model.leaves.iter().enumerate().map(|(i, l)| (l.centre[1], 1, i)));
    order.sort_by(|a, b| b.0.total_cmp(&a.0));

    let depth = size[1].max(1e-3);
    for (y, kind, i) in order {
        // Nearer geometry is a little brighter, which separates the layers.
        let lit = 0.86 + 0.28 * ((model.bounds.max[1] - y) / depth);
        if kind == 0 {
            branch(&mut cv, &view, &model.segments[i], style, lit);
        } else {
            blob(&mut cv, &view, &model.leaves[i], style, lit, i as i64);
        }
    }
    ground(&mut cv);
    cv
}

/// Draw one branch as a run of spans across its axis: a standing branch is
/// thick across the columns, a reaching one across the rows, so the bark
/// keeps its width whichever way it points.
fn branch(cv: &mut Canvas, view: &View, s: &super::Segment, style: &PreviewStyle, lit: f32) {
    let (ax, ay) = (view.col(s.a[0]), view.row(s.a[2]));
    let (bx, by) = (view.col(s.b[0]), view.row(s.b[2]));
    // The same metres are twice as many columns as rows.
    let cols = (2.0 * s.radius * view.cpm).max(0.25);
    let rows = cols * 0.5;
    // A shed whorl is grey wood on a living tree, the colour a whole snag
    // takes (`dead_bark`).
    let (glyph, bark) = if s.dead { (style.dead_trunk_glyph, style.dead_trunk) } else { (style.trunk_glyph, style.trunk) };
    let fg = glyph.scale(lit);
    let bg = bark.scale(lit);
    let (dx, dy) = (bx - ax, by - ay);
    let steps = (dx.abs().max(dy.abs()) * 2.0).ceil().max(1.0) as i32;
    let thin = if dy.abs() * 2.0 < dx.abs() {
        style.thin[1]
    } else if dx.abs() * 2.0 < dy.abs() {
        style.thin[0]
    } else if dx * dy < 0.0 {
        style.thin[2]
    } else {
        style.thin[3]
    };
    let steep = dy.abs() >= dx.abs();
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let (x, y) = (ax + dx * t, ay + dy * t);
        if steep {
            span(cv, x - cols * 0.5, x + cols * 0.5, y.round() as i32, thin, style, fg, bg, true);
        } else {
            span(cv, y - rows * 0.5, y + rows * 0.5, x.round() as i32, thin, style, fg, bg, false);
        }
    }
}

/// Fill the cells from `a0` to `a1` along one axis, at `fixed` on the
/// other: `across` true runs along the columns of row `fixed`, false along
/// the rows of column `fixed`. A partly covered edge cell takes the half
/// block facing the middle of the span, a span narrower than a cell takes
/// the thin glyph, and a wholly covered cell is the solid glyph on the bark
/// colour, so a trunk reads as a block and a twig as a stroke.
#[allow(clippy::too_many_arguments)]
fn span(cv: &mut Canvas, a0: f32, a1: f32, fixed: i32, thin: char, style: &PreviewStyle, fg: Rgb, bg: Rgb, across: bool) {
    let at = |a: i32| if across { (a, fixed) } else { (fixed, a) };
    if a1 - a0 < 0.8 {
        let (x, y) = at(((a0 + a1) * 0.5).floor() as i32);
        cv.glyph(x, y, thin, fg);
        return;
    }
    let (first, last) = (a0.floor() as i32, (a1 - 1e-4).floor() as i32);
    for a in first..=last {
        let (x, y) = at(a);
        let cover = (a1.min(a as f32 + 1.0) - a0.max(a as f32)).clamp(0.0, 1.0);
        if cover > 0.85 {
            // A fifth of the cells take the lighter bark colour, which
            // gives a wide trunk some grain.
            let grain = if hash01(x as i64, y as i64, 0xba2c) < 0.2 { fg } else { bg };
            cv.put(x, y, style.solid, grain, bg);
        } else if cover > 0.3 {
            // The covered half is the one facing the middle of the span:
            // right or left across the columns, bottom or top down the rows.
            let near = (a as f32) < (a0 + a1) * 0.5;
            let side = match (across, near) {
                (true, true) => style.half[1],
                (true, false) => style.half[0],
                (false, true) => style.half[3],
                (false, false) => style.half[2],
            };
            cv.glyph(x, y, side, bg);
        }
    }
}

/// Draw one leaf cluster as a filled ellipse with a rounded outline.
fn blob(cv: &mut Canvas, view: &View, l: &super::Leaf, style: &PreviewStyle, lit: f32, salt: i64) {
    let (cx, cy) = (view.col(l.centre[0]), view.row(l.centre[2]));
    let (rx, ry) = ((l.radius[0] * view.cpm).max(0.5), (l.radius[2] * view.rpm).max(0.5));
    let bg = style.canopy.scale(lit);
    let fg = style.canopy_glyph.scale(lit);
    let inside = |x: f32, y: f32| {
        let (u, v) = ((x - cx) / rx, (y - cy) / ry);
        u * u + v * v <= 1.0
    };
    let (x0, x1) = ((cx - rx).floor() as i32, (cx + rx).ceil() as i32);
    let (y0, y1) = ((cy - ry).floor() as i32, (cy + ry).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            // Sample the four quadrant centres of the cell.
            let (fx, fy) = (x as f32, y as f32);
            let mask = (inside(fx + 0.25, fy + 0.25) as u8) | (inside(fx + 0.75, fy + 0.25) as u8) << 1 | (inside(fx + 0.25, fy + 0.75) as u8) << 2 | (inside(fx + 0.75, fy + 0.75) as u8) << 3;
            match mask {
                0 => {}
                0b1111 | 0b0111 | 0b1011 | 0b1101 | 0b1110 | 0b1001 | 0b0110 => {
                    let ch = if hash01(x as i64, y as i64 * 31 + salt, 0x1eaf) < style.fill_density {
                        style.fill[(hash01(x as i64 + 7, y as i64 - salt, 0xf011) * style.fill.len() as f32) as usize % style.fill.len()]
                    } else {
                        ' '
                    };
                    cv.put(x, y, ch, fg, bg);
                }
                0b0011 => cv.glyph(x, y, style.half[2], bg),
                0b1100 => cv.glyph(x, y, style.half[3], bg),
                0b0101 => cv.glyph(x, y, style.half[0], bg),
                0b1010 => cv.glyph(x, y, style.half[1], bg),
                0b0001 => cv.glyph(x, y, style.quadrant[0], bg),
                0b0010 => cv.glyph(x, y, style.quadrant[1], bg),
                0b0100 => cv.glyph(x, y, style.quadrant[2], bg),
                _ => cv.glyph(x, y, style.quadrant[3], bg),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsystem::{Alternative, Grammar};
    use std::collections::BTreeMap;

    fn sample() -> TreeModel {
        Grammar {
            axiom: "F!FA".to_string(),
            rules: BTreeMap::from([('A', vec![Alternative { replacement: "F/[+!AL][-!AL]".to_string(), weight: 1 }])]),
            dead_rules: BTreeMap::new(),
            depth: 5,
            angle: 28.0,
            length: 1.2,
            taper: 0.8,
            leaf_radius: 0.9,
            droop: 0.0,
            leaf_density: 1.0,
            asymmetry: 0.0,
            jitter: 0.08,
            prune_height: 0.0,
            dead_whorls: 0.0,
            leaf_flat: 0.0,
        }
        .grow(3, [9.0, 9.0, 16.0], Growth::FULL)
    }

    #[test]
    fn the_preview_renders_at_every_useful_size() {
        for (c, r) in [(20u16, 10u16), (120, 60), (1, 1), (80, 25), (168, 71)] {
            let cv = preview(&sample(), c, r, &PreviewStyle::default());
            assert_eq!((cv.w, cv.h), (c as i32, r as i32));
        }
    }

    #[test]
    fn an_empty_model_draws_the_ground_and_nothing_else() {
        let cv = preview(&TreeModel::default(), 20, 10, &PreviewStyle::default());
        let style = PreviewStyle::default();
        assert!(cv.cells[..20 * 9].iter().all(|c| c.bg == style.sky));
        assert!(cv.cells[20 * 9..].iter().all(|c| c.bg == style.ground));
    }

    #[test]
    fn the_tree_covers_the_middle_of_the_canvas_and_keeps_its_margin() {
        let style = PreviewStyle::default();
        let cv = preview(&sample(), 120, 60, &style);
        let painted = cv.cells.iter().filter(|c| c.bg != style.sky && c.bg != style.ground).count();
        assert!(painted > 400, "the tree is drawn: {painted} cells");
        // The trunk stands in the middle columns, on the ground.
        let row = (58 * 120) as usize;
        assert!((50..70).any(|x| cv.cells[row + x].bg != style.sky), "trunk near the centre");
    }
}
