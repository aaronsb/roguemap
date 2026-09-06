//! Billboards over the terrain: creatures, the one-glyph trees of the two
//! smallest zooms, small ground props, and campfire flames. Buildings and
//! the larger trees are geometry on the ray walk (ADR-002).

use crate::camera::Anchor;
use crate::canvas::Rgb;
use crate::map::{Flora, Terrain, Tile, SEA};
use crate::noise::hash;
use crate::render::{GCell, Renderer, Scene, FACE_TOP};
use crate::sprite::Sprite;

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

/// Something standing on a tile that draws as a billboard.
enum SpriteItem {
    Tree(Flora),
    /// A creature, by kind.
    Entity(u8),
}

/// A sprite item resolved against the scene: what to draw and how.
struct Resolved<'a> {
    sprite: &'a Sprite,
    colors: SpriteColors,
    /// Added to the tile depth so creatures stand in front of what they share
    /// a tile with.
    depth_bias: f32,
}

/// Whether trees are billboards at this zoom rather than volumes.
pub(crate) fn tree_billboards(hw: i32) -> bool {
    hw <= 3
}

impl SpriteItem {
    fn resolve<'a>(&self, sc: &Scene<'a>, tile: &Tile) -> Resolved<'a> {
        let (assets, ts, pal, world, cam) = (sc.assets, sc.ts, &sc.pal, sc.world, sc.cam);
        match *self {
            SpriteItem::Tree(flora) => {
                let sp = flora.species(assets);
                let snow = world.snow_at(tile.temp as f32) * 0.7;
                Resolved {
                    sprite: ts.tree(sp.form),
                    colors: SpriteColors {
                        top: Tint {
                            bg: crate::biome::seasonal(&sp.canopy, world.season).lerp(pal.snow(), snow),
                            fg: crate::biome::seasonal(&sp.canopy_glyph, world.season).lerp(pal.snow_glyph(), snow * 0.5),
                        },
                        base: Tint { bg: pal.trunk, fg: pal.trunk_glyph },
                    },
                    depth_bias: 0.0,
                }
            }
            SpriteItem::Entity(kind) => {
                let creature = &assets.creatures[kind as usize % assets.creatures.len()];
                let tint = Tint { bg: creature.color, fg: creature.glyph };
                Resolved {
                    sprite: assets.art.for_zoom(&creature.art, cam.zoom).expect("creature art was checked at load"),
                    colors: SpriteColors { top: tint, base: tint },
                    depth_bias: 0.01,
                }
            }
        }
    }
}

impl Renderer {
    /// Entities, and at the smallest zooms trees, back to front with a
    /// depth test.
    pub(crate) fn sprite_pass(&mut self, sc: &Scene) {
        let (map, world, cam) = (sc.map, sc.world, sc.cam);
        let tiny_trees = tree_billboards(cam.hw);
        let (x0, y0, x1, y1) = self.visible_bounds(cam);
        let mut items: Vec<(f32, i32, i32, Tile)> = Vec::new();
        for my in y0..=y1 {
            for mx in x0..=x1 {
                let Some(tile) = map.get(mx, my) else { continue };
                let has_entity = world.entities.iter().any(|e| e.mx == mx && e.my == my);
                if !(tiny_trees && tile.tree.is_some()) && !has_entity {
                    continue;
                }
                items.push((cam.tile_depth(mx, my), mx, my, tile));
            }
        }
        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, mx, my, tile) in items {
            let a = cam.anchor_f(mx, my, tile.hf.max(SEA as f32));
            if a.sx < -40 || a.sx > self.w + 40 || a.sy < -40 || a.sy > self.h + 40 {
                continue;
            }
            if tiny_trees {
                if let Some(flora) = tile.tree {
                    self.draw_item(sc, &SpriteItem::Tree(flora), &tile, &a);
                }
            }
            for e in world.entities.iter().filter(|e| e.mx == mx && e.my == my) {
                self.draw_item(sc, &SpriteItem::Entity(e.kind), &tile, &a);
            }
        }
    }

    fn draw_item(&mut self, sc: &Scene, item: &SpriteItem, tile: &Tile, a: &Anchor) {
        let r = item.resolve(sc, tile);
        let anchor = Anchor { depth: a.depth + r.depth_bias, ..*a };
        self.sprite(r.sprite, &anchor, &r.colors);
    }

    /// Draw a billboard anchored so its bottom row sits on the tile's centre
    /// row. Cells already holding nearer terrain, geometry or sprites are
    /// left alone.
    fn sprite(&mut self, sp: &Sprite, a: &Anchor, col: &SpriteColors) {
        let n = sp.rows.len() as i32;
        for (r, row) in sp.rows.iter().enumerate() {
            let r = r as i32;
            let height = n - 1 - r;
            let y = a.sy - height;
            let base = r as usize >= sp.rows.len() - sp.base_rows;
            let tint = if base { col.base } else { col.top };
            for (c, ch) in row.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let x = a.sx + c as i32 - sp.center;
                let wz = a.z + height as f32;
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
    /// table, with the art tier picked by zoom. Tiles carrying a tree or a
    /// stack have no room for them.
    pub(crate) fn prop_pass(&mut self, sc: &Scene) {
        let (assets, map, world, cam) = (sc.assets, sc.map, sc.world, sc.cam);
        let props = &assets.props;
        if world.placed.is_empty() && props.iter().all(|p| (cam.zoom as u8) < p.min_zoom) {
            return;
        }
        let (fx, fy) = cam.forward();
        let (x0, y0, x1, y1) = self.visible_bounds(cam);
        let mut items: Vec<(f32, f32, f32, f32, usize)> = Vec::new();
        for my in y0..=y1 {
            for mx in x0..=x1 {
                let Some(tile) = map.get(mx, my) else { continue };
                if tile.tree.is_some() || tile.stack.is_some() || tile.terrain == Terrain::Water {
                    continue;
                }
                let cover = tile.biome(assets).cover;
                for sub in 0..16 {
                    let hv = hash(mx as i64 * 4 + sub % 4, my as i64 * 4 + sub / 4, (tile.seed as u64) ^ 0x9A0B);
                    let roll = (hv % 10000) as f32 / 10000.0;
                    let mut acc = 0.0;
                    for (pi, p) in props.iter().enumerate() {
                        if (cam.zoom as u8) < p.min_zoom || !p.terrain.contains(&tile.terrain) || (p.near_water && !tile.near_water) || (!p.cover.is_empty() && !p.cover.contains(&cover)) {
                            continue;
                        }
                        acc += p.density;
                        if roll < acc {
                            let x = mx as f32 + (sub % 4) as f32 / 4.0 + 0.125;
                            let y = my as f32 + (sub / 4) as f32 / 4.0 + 0.125;
                            let depth = x * fx + y * fy;
                            items.push((depth, x, y, tile.hf.max(SEA as f32), pi));
                            break;
                        }
                    }
                }
            }
        }
        // Hand-placed props draw at every zoom: they are explicit, not scattered.
        for pl in world.placed.iter().filter(|pl| pl.prop < props.len()) {
            let Some(tile) = map.get(pl.x.floor() as i32, pl.y.floor() as i32) else { continue };
            items.push((pl.x * fx + pl.y * fy, pl.x, pl.y, tile.hf.max(SEA as f32), pl.prop));
        }
        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (depth, x, y, z, pi) in items {
            let p = &props[pi];
            let Some(sprite) = assets.art.for_zoom(&p.art, cam.zoom) else { continue };
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
                        *cell = GCell { albedo: bg, ch, glyph: fg, wx: x, wy: y, wz: z + (n - 1 - r as i32) as f32, face: FACE_TOP, lit: true, depth };
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
