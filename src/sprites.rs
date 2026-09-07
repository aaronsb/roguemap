//! Billboards over the terrain: creatures, the one-glyph trees of the two
//! smallest zooms, small ground props, and campfire flames. Buildings and
//! the larger trees are geometry on the ray walk (ADR-002).

use crate::camera::Anchor;
use crate::canvas::Rgb;
use crate::map::{Flora, Terrain, Tile, SEA};
use crate::noise::hash;
use crate::render::{GCell, Renderer, Scene, FACE_TOP};
use crate::sprite::Sprite;
use crate::world::{Entity, Facing};

/// Background and glyph colour of one part of a sprite.
#[derive(Clone, Copy)]
struct Tint {
    bg: Rgb,
    fg: Rgb,
}

/// Colours for a sprite's top part (canopy, figure) and its base rows
/// (trunk).
struct SpriteColors {
    top: Tint,
    base: Tint,
}

/// The scattered props of one tile: a deterministic 4x4 sub-grid, one roll
/// per cell against the props whose placement rules the tile passes, in
/// table order. `f` takes the prop's ground position and its row. Tiles
/// carrying a tree or a stack have no room for them, and water none at
/// all. The sprite pass draws these and the shadow mask sweeps them, so
/// the two agree on where a prop stands.
pub(crate) fn scatter(sc: &Scene, mx: i32, my: i32, tile: &Tile, mut f: impl FnMut(f32, f32, usize)) {
    let (assets, cam) = (sc.assets, sc.cam);
    if tile.tree.is_some() || tile.stack.is_some() || tile.terrain == Terrain::Water {
        return;
    }
    let cover = tile.biome(assets).cover;
    for sub in 0..16 {
        let hv = hash(mx as i64 * 4 + sub % 4, my as i64 * 4 + sub / 4, (tile.seed as u64) ^ 0x9A0B);
        let roll = (hv % 10000) as f32 / 10000.0;
        let mut acc = 0.0;
        for (pi, p) in assets.props.iter().enumerate() {
            if (cam.zoom as u8) < p.min_zoom || !p.terrain.contains(&tile.terrain) || (p.near_water && !tile.near_water) || (!p.cover.is_empty() && !p.cover.contains(&cover)) {
                continue;
            }
            acc += p.density;
            if roll < acc {
                f(mx as f32 + (sub % 4) as f32 / 4.0 + 0.125, my as f32 + (sub / 4) as f32 / 4.0 + 0.125, pi);
                break;
            }
        }
    }
}

/// Something standing on a tile that draws as a billboard.
enum SpriteItem {
    Tree(Flora),
    /// A creature: its kind, facing and walk pick the art (ADR-008).
    Entity(Entity),
}

/// A sprite item resolved against the scene: what to draw and how.
struct Resolved<'a> {
    sprite: &'a Sprite,
    /// The walk-cycle pose, or `None` at rest.
    pose: Option<usize>,
    colors: SpriteColors,
    /// Added to the tile depth so creatures stand in front of what they share
    /// a tile with.
    depth_bias: f32,
}

/// Whether trees are billboards at this zoom rather than volumes. A
/// perspective view builds a volume for every tree in reach, a stand-in
/// where the tree is too far for its model, so it never draws them.
pub(crate) fn tree_billboards(cam: &crate::camera::Camera) -> bool {
    !cam.is_perspective() && !crate::raster::lod_of(cam.detail_rows()).volumes
}

impl SpriteItem {
    /// The item's art and colours, its tier picked from the rows a metre
    /// is worth where it stands: the camera's scale in an orthographic
    /// view, the scale at its own depth in a perspective one.
    fn resolve<'a>(&self, sc: &Scene<'a>, tile: &Tile, detail_rows: f32) -> Resolved<'a> {
        let (assets, ts, pal, world) = (sc.assets, sc.ts, &sc.pal, sc.world);
        match *self {
            SpriteItem::Tree(flora) => {
                let sp = flora.species(assets);
                let snow = world.snow_at(tile.temp as f32) * 0.7;
                Resolved {
                    sprite: ts.tree(sp.form),
                    pose: None,
                    colors: SpriteColors {
                        top: Tint { bg: crate::biome::seasonal(&sp.canopy, world.season).lerp(pal.snow(), snow), fg: crate::biome::seasonal(&sp.canopy_glyph, world.season).lerp(pal.snow_glyph(), snow * 0.5) },
                        base: Tint { bg: pal.trunk, fg: pal.trunk_glyph },
                    },
                    depth_bias: 0.0,
                }
            }
            SpriteItem::Entity(e) => {
                let creature = &assets.creatures[e.kind as usize % assets.creatures.len()];
                let tint = Tint { bg: creature.color, fg: creature.glyph };
                let sprite = assets.art.for_rows_facing(&creature.art, creature.size[2] * detail_rows, e.facing == Facing::Left).expect("creature art was checked at load");
                Resolved { sprite, pose: e.pose(sprite.poses.len()), colors: SpriteColors { top: tint, base: tint }, depth_bias: 0.01 }
            }
        }
    }
}

impl Renderer {
    /// Entities, and at the smallest zooms trees, back to front with a
    /// depth test.
    pub(crate) fn sprite_pass(&mut self, sc: &Scene) {
        let (world, cam) = (sc.world, sc.cam);
        let tiny_trees = tree_billboards(cam);
        let (x0, y0, x1, y1) = self.tile_bounds(cam);
        // Each item at its point of the map: a tree at its tile's centre, a
        // creature wherever it stands (ADR-006). Depth is the tile's, so a
        // figure anywhere in a tile sorts in front of that tile's ground.
        let mut items: Vec<(f32, f32, f32, Tile, SpriteItem)> = Vec::new();
        if tiny_trees {
            for my in y0..=y1 {
                for mx in x0..=x1 {
                    let Some(tile) = self.tile_at(sc, mx, my) else { continue };
                    if let Some(flora) = tile.tree {
                        items.push((cam.tile_depth(mx, my), mx as f32 + 0.5, my as f32 + 0.5, tile, SpriteItem::Tree(flora)));
                    }
                }
            }
        }
        for e in &world.entities {
            // The first-person eye is the character's own; they are not
            // drawn.
            if cam.hides_player() && world.player().is_some_and(|p| std::ptr::eq(p, e)) {
                continue;
            }
            let (mx, my) = e.tile();
            if mx < x0 || mx > x1 || my < y0 || my > y1 {
                continue;
            }
            let Some(tile) = self.tile_at(sc, mx, my) else { continue };
            let (x, y) = e.pos();
            items.push((cam.tile_depth(mx, my), x, y, tile, SpriteItem::Entity(*e)));
        }
        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, x, y, tile, item) in items {
            let ground = sc.map.ground_at(x, y);
            let a = cam.anchor_at(x, y, ground);
            if a.sx < -40 || a.sx > self.w + 40 || a.sy < -40 || a.sy > self.h + 40 {
                continue;
            }
            self.draw_item(sc, &item, &tile, &a, cam.detail_rows_at(x, y, ground));
        }
    }

    fn draw_item(&mut self, sc: &Scene, item: &SpriteItem, tile: &Tile, a: &Anchor, detail_rows: f32) {
        let r = item.resolve(sc, tile, detail_rows);
        let anchor = Anchor { depth: a.depth + r.depth_bias, ..*a };
        self.sprite(r.sprite, r.pose, &anchor, &r.colors, detail_rows);
    }

    /// Draw a billboard, in the given walk pose or at rest, anchored so
    /// its feet sit on the tile's centre row, whatever tier the art came
    /// from. Cells already holding nearer terrain, geometry or sprites are
    /// left alone.
    fn sprite(&mut self, sp: &Sprite, pose: Option<usize>, a: &Anchor, col: &SpriteColors, detail_rows: f32) {
        let rows = sp.pose(pose);
        let n = rows.len() as i32;
        for (r, row) in rows.iter().enumerate() {
            let r = r as i32;
            let height = n - 1 - r;
            let y = a.sy - height;
            let base = r as usize >= rows.len() - sp.base_rows;
            let tint = if base { col.base } else { col.top };
            for (c, ch) in row.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let x = a.sx + c as i32 - sp.center;
                let wz = a.z + height as f32 / detail_rows;
                if let Some(cell) = self.cell(x, y) {
                    if cell.depth > a.depth {
                        continue;
                    }
                    *cell = GCell { albedo: tint.bg, ch, glyph: tint.fg, wx: a.mx as f32 + 0.5, wy: a.my as f32 + 0.5, wz, face: FACE_TOP, lit: true, depth: a.depth };
                }
            }
        }
    }

    /// Small ground props on a deterministic 4x4 sub-grid per tile, drawn
    /// from each prop's `min_zoom`: boulders, clumps, reeds, brush from the
    /// table, with the art tier picked from the prop's height in rows. Tiles carrying a tree or a
    /// stack have no room for them.
    pub(crate) fn prop_pass(&mut self, sc: &Scene) {
        let (assets, map, world, cam) = (sc.assets, sc.map, sc.world, sc.cam);
        let props = &assets.props;
        if world.placed.is_empty() && props.iter().all(|p| (cam.zoom as u8) < p.min_zoom) {
            return;
        }
        let (x0, y0, x1, y1) = self.tile_bounds(cam);
        // From an eye a prop is under a row past `focal_rows` metres, and
        // the tiles beyond that are not worth scattering.
        let reach = cam.is_perspective().then(|| (cam.eye(), cam.focal_rows() + 2.0));
        let mut items: Vec<(f32, f32, f32, f32, usize)> = Vec::new();
        for my in y0..=y1 {
            for mx in x0..=x1 {
                let Some(tile) = self.tile_at(sc, mx, my) else { continue };
                if reach.is_some_and(|(eye, range)| cam.fog_depth(eye, mx as f32 + 0.5, my as f32 + 0.5, tile.hf) > range) {
                    continue;
                }
                scatter(sc, mx, my, &tile, |x, y, pi| items.push((cam.depth(x, y), x, y, tile.hf.max(SEA as f32), pi)));
            }
        }
        // Hand-placed props draw at every zoom: they are explicit, not scattered.
        for pl in world.placed.iter().filter(|pl| pl.prop < props.len()) {
            let Some(tile) = map.get(pl.x.floor() as i32, pl.y.floor() as i32) else { continue };
            items.push((cam.depth(pl.x, pl.y), pl.x, pl.y, tile.hf.max(SEA as f32), pl.prop));
        }
        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (depth, x, y, z, pi) in items {
            let p = &props[pi];
            let detail_rows = cam.detail_rows_at(x, y, z);
            let Some(sprite) = assets.art.for_rows(&p.art, p.size[2] * detail_rows) else { continue };
            let (sx, sy) = cam.project(x, y, z);
            let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
            let n = sprite.rows.len() as i32;
            let temp = map.get(x.floor() as i32, y.floor() as i32).map(|t| t.temp as f32).unwrap_or(10.0);
            let snow = world.snow_at(temp);
            for (r, row) in sprite.rows.iter().enumerate() {
                let yy = sy - (n - 1 - r as i32);
                for (c, ch) in row.chars().enumerate() {
                    if ch == ' ' {
                        continue;
                    }
                    let xx = sx + c as i32 - sprite.center;
                    if let Some(cell) = self.cell(xx, yy) {
                        if cell.depth > depth {
                            continue;
                        }
                        let solid = p.color != Rgb(0, 0, 0);
                        let bg = if solid { p.color.lerp(Rgb(228, 232, 240), snow * 0.8) } else { cell.albedo };
                        let fg = p.glyph.lerp(Rgb(235, 238, 245), snow * 0.6);
                        *cell = GCell { albedo: bg, ch, glyph: fg, wx: x, wy: y, wz: z + (n - 1 - r as i32) as f32 / detail_rows, face: FACE_TOP, lit: true, depth };
                    }
                }
            }
        }
    }

    /// Flames and embers for every placed light; lit windows are not placed
    /// lights, they come from the stacks in view.
    pub(crate) fn draw_lights(&mut self, sc: &Scene) {
        let (ts, cam, t) = (sc.ts, sc.cam, sc.t);
        for l in &sc.world.lights {
            let a = cam.anchor(l.mx, l.my, l.z);
            let frame = ((t * 9.0) as usize + (l.mx as usize)) % 3;
            let flick = 0.8 + 0.2 * (t * 17.0 + l.mx as f32).sin();
            let hot = Rgb((255.0 * flick) as u8, (190.0 * flick) as u8, (70.0 * flick) as u8);
            let ember = Rgb(140, 50, 20);
            for (i, dx) in (-1..=1).enumerate() {
                let ch = ts.flame[(frame + i) % 3];
                let y = if dx == 0 { a.sy - 1 } else { a.sy };
                if let Some(c) = self.cell(a.sx + dx, y) {
                    let bg = c.albedo;
                    *c = GCell { albedo: bg, ch, glyph: hot, wx: l.mx as f32, wy: l.my as f32, wz: l.z as f32, face: FACE_TOP, lit: false, depth: a.depth };
                }
            }
            for dx in -1..=0 {
                if let Some(c) = self.cell(a.sx + dx, a.sy + 1) {
                    *c = GCell { albedo: ember, ch: ' ', glyph: ember, wx: 0.0, wy: 0.0, wz: 0.0, face: FACE_TOP, lit: false, depth: a.depth };
                }
            }
        }
    }
}
