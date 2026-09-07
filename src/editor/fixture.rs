//! Preview fixtures (ADR-003): a small flat `Map` in a chosen biome with
//! the selected row placed at its centre, and a `World` carrying the
//! chosen season, hour and weather. The renderer draws these exactly as it
//! draws the game.

use std::rc::Rc;

use super::fields::TableKind;
use crate::assets::{Assets, Tier};
use crate::blocks::Stack;
use crate::map::{FixtureSpec, Flora, Map, Terrain, SEA};
use crate::world::{Entity, PlacedProp, World, WEATHER_PRESETS};

/// Side of the fixture map in tiles; the subject sits at the centre.
pub const SIZE: usize = 12;
/// Height of the fixture ground.
pub const GROUND_Z: i32 = SEA + 3;

/// Block footprints `P` cycles through, as offsets from the centre tile.
/// The first is the ground the generator gives a house, so the preview
/// shows what the game builds; the rest exercise merging.
pub const PATTERNS: [(&str, &[(i32, i32)]); 4] = [
    ("4x3", &[(-2, -1), (-1, -1), (0, -1), (1, -1), (-2, 0), (-1, 0), (0, 0), (1, 0), (-2, 1), (-1, 1), (0, 1), (1, 1)]),
    ("1x1", &[(0, 0)]),
    ("1x3", &[(-1, 0), (0, 0), (1, 0)]),
    ("L", &[(0, -1), (0, 0), (0, 1), (1, 1)]),
];

/// Representative annual temperature per Köppen code.
pub fn temp_for(koppen: &str) -> i8 {
    match koppen {
        "Af" => 26,
        "Aw" => 24,
        "BW" => 22,
        "BS" => 12,
        "Cs" => 14,
        "Cf" => 10,
        "Df" => -2,
        "ET" => -10,
        "EF" => -20,
        _ => 12,
    }
}

/// The row being previewed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subject {
    pub kind: TableKind,
    pub row: usize,
}

/// What the previews are drawn in: chosen with the keys shared with the
/// game, and by `--snap`.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewSettings {
    /// Index into the biome table.
    pub biome: usize,
    pub season: f32,
    pub tod: f32,
    /// Whether the hour still follows the subject (night for lights).
    pub tod_auto: bool,
    /// Index into the tileset list, as the `glyphs` setting.
    pub glyphs: usize,
    /// Camera angle in radians.
    pub angle: f32,
    /// `None` for the fixture's clear sky, else a weather preset.
    pub weather: Option<usize>,
    /// Block footprint, into `PATTERNS`.
    pub pattern: usize,
    /// Tree variant, 0..=3; for blocks, which level count in the kind's
    /// range.
    pub variant: u8,
    pub animate: bool,
    /// The one tier shown on small screens.
    pub tier: Tier,
}

impl Default for PreviewSettings {
    fn default() -> PreviewSettings {
        PreviewSettings { biome: 0, season: 1.0, tod: 12.0, tod_auto: true, glyphs: 0, angle: std::f32::consts::FRAC_PI_4, weather: None, pattern: 0, variant: 0, animate: false, tier: Tier::Large }
    }
}

/// A map and world ready to render, with the subject at `(cx, cy)`.
pub struct Fixture {
    pub map: Map,
    pub world: World,
    pub cx: i32,
    pub cy: i32,
    /// The biome the fixture is in: the setting's, or the subject's own.
    pub biome: usize,
    /// The tileset the subject asks for, if it is a tileset row.
    pub glyphs: Option<usize>,
    /// A line naming what was placed, for the preview title.
    pub caption: String,
    /// How tall the subject stands over the ground, in metres, so a pane
    /// can frame it (ADR-004): a tree's crown, a stack's roof, a creature.
    pub top: f32,
}

fn spec(assets: &Assets, biome: usize, terrain: Terrain) -> FixtureSpec {
    let b = &assets.biomes[biome % assets.biomes.len()];
    let temp = temp_for(&b.koppen);
    let terrain = if terrain == Terrain::Grass && temp <= -16 { Terrain::Snow } else { terrain };
    FixtureSpec { w: SIZE, h: SIZE, z: GROUND_Z, biome: biome % assets.biomes.len(), terrain, temp, grass: b.grass, material: b.material }
}

/// Build the fixture for a subject. Row indices past the table's end fall
/// back to a plain fixture, so a stale subject never panics.
pub fn build(assets: &Rc<Assets>, subject: Subject, s: &PreviewSettings) -> Fixture {
    let (cx, cy) = (SIZE as i32 / 2, SIZE as i32 / 2);
    let mut world = World::new(1);
    world.auto_time = false;
    world.season = s.season;
    world.tod = s.tod;
    if let Some(w) = s.weather {
        let (cover, precip) = WEATHER_PRESETS[w % WEATHER_PRESETS.len()];
        world.weather.cover = cover;
        world.weather.precip = precip;
        world.weather.wind = if precip > 0.0 { 0.6 } else { 0.2 };
    } else {
        world.weather.cover = 0.0;
        world.weather.precip = 0.0;
    }
    let mut biome = s.biome;
    let mut terrain = Terrain::Grass;
    let mut glyphs = None;
    let mut caption = String::new();
    let mut top = 0.0f32;

    // Rows that change the fixture's ground rather than stand on it.
    match subject.kind {
        TableKind::Biomes if subject.row < assets.biomes.len() => biome = subject.row,
        TableKind::Surfaces if subject.row < 4 => terrain = [Terrain::Sand, Terrain::Dirt, Terrain::Rock, Terrain::Snow][subject.row],
        TableKind::Seasons if subject.row < 4 => world.season = subject.row as f32,
        TableKind::Tilesets => glyphs = assets.setting("glyphs").and_then(|g| assets.tilesets.get(subject.row).and_then(|t| g.values.iter().position(|v| *v == t.name))),
        TableKind::Lights if s.tod_auto => world.tod = 22.0,
        _ => {}
    }
    if subject.kind == TableKind::Props {
        if let Some(p) = assets.props.get(subject.row) {
            terrain = p.terrain.iter().copied().find(|t| *t != Terrain::Water).unwrap_or(Terrain::Grass);
        }
    }
    let mut map = Map::fixture(spec(assets, biome, terrain), assets.clone());
    let centre = map.get(cx, cy).expect("fixture centre exists");
    let house = assets.blocks.iter().position(|b| b.name == "house").unwrap_or(0) as u8;

    match subject.kind {
        TableKind::Species if subject.row < assets.species.len() => {
            let mut t = centre;
            t.tree = Some(Flora { species: subject.row as u8, variant: s.variant % 4 });
            map.set_tile(cx, cy, t);
            let sp = &assets.species[subject.row];
            let (_, d) = sp.volume();
            top = (d.trunk + d.height) * crate::volume::size_scale(sp.size_class) * crate::volume::variant_scale(s.variant % 4);
            caption = format!("showing {} (variant {}) on {} ground", sp.name, s.variant % 4, assets.biomes[biome].name);
        }
        TableKind::Biomes => {
            let b = &assets.biomes[biome];
            if !b.species.is_empty() && b.tree_density > 0.0 {
                for (i, (dx, dy)) in [(-3, -2), (2, -3), (3, 2), (-2, 3), (0, -4)].iter().enumerate() {
                    let mut t = map.get(cx + dx, cy + dy).expect("inside the fixture");
                    let roll = (i as u64 * 7919) % 97;
                    t.tree = Some(Flora { species: crate::biome::weighted_pick(&b.species, roll) as u8, variant: (i % 4) as u8 });
                    map.set_tile(cx + dx, cy + dy, t);
                }
            }
            for (dx, dy) in PATTERNS[0].1 {
                let mut t = map.get(cx + dx, cy + dy).expect("inside the fixture");
                t.stack = Some(Stack { kind: house, levels: 1 });
                map.set_tile(cx + dx, cy + dy, t);
            }
            top = assets.blocks[house as usize].level_height + assets.blocks[house as usize].max_rise;
            let trees: Vec<&str> = b.species.iter().filter_map(|(i, _)| assets.species.get(*i).map(|sp| sp.name.as_str())).collect();
            let material = assets.materials.get(b.material).map(|m| m.name.as_str()).unwrap_or("?");
            caption =
                if trees.is_empty() { format!("showing {} ground and a house built of {}", b.name, material) } else { format!("showing {} ground with {} and a house built of {}", b.name, trees.join(", "), material) };
        }
        TableKind::Materials if subject.row < assets.materials.len() && !assets.blocks.is_empty() => {
            for (dx, dy) in PATTERNS[0].1 {
                let mut t = map.get(cx + dx, cy + dy).expect("inside the fixture");
                t.material = subject.row as u8;
                t.stack = Some(Stack { kind: house, levels: 1 });
                map.set_tile(cx + dx, cy + dy, t);
            }
            top = assets.blocks[house as usize].level_height + assets.blocks[house as usize].max_rise;
            caption = format!("showing a 4x3 house built of {}", assets.materials[subject.row].name);
        }
        TableKind::Blocks if subject.row < assets.blocks.len() => {
            let (name, offsets) = PATTERNS[s.pattern % PATTERNS.len()];
            // The variant picks the level count within the kind's range, so
            // every tile of the pattern merges into one building.
            let [lo, hi] = assets.blocks[subject.row].levels;
            let levels = lo + (s.variant % (hi - lo + 1));
            for (dx, dy) in offsets.iter() {
                let mut t = map.get(cx + dx, cy + dy).expect("inside the fixture");
                t.stack = Some(Stack { kind: subject.row as u8, levels });
                map.set_tile(cx + dx, cy + dy, t);
            }
            let b = &assets.blocks[subject.row];
            top = levels as f32 * b.level_height + b.max_rise;
            caption = format!("showing {} in pattern {name}", b.name);
        }
        TableKind::Props if subject.row < assets.props.len() => {
            for sub in [5, 6, 9, 10] {
                let x = cx as f32 + (sub % 4) as f32 / 4.0 + 0.125;
                let y = cy as f32 + (sub / 4) as f32 / 4.0 + 0.125;
                world.placed.push(PlacedProp { x, y, prop: subject.row });
            }
            top = assets.props[subject.row].size[2];
            caption = format!("showing four {} on the centre tile", assets.props[subject.row].name);
        }
        TableKind::Creatures if subject.row < assets.creatures.len() => {
            world.entities.push(Entity::at_tile(subject.row as u8, cx, cy, 0.0));
            top = assets.creatures[subject.row].size[2];
            caption = format!("showing {} standing on {} ground", assets.creatures[subject.row].name, assets.biomes[biome].name);
        }
        TableKind::Lights if subject.row < assets.lights.len() => {
            world.lights.push(assets.lights[subject.row].at(cx, cy, GROUND_Z, 1.0));
            caption = format!("showing {} lit at {:02}:00 on {} ground", assets.lights[subject.row].name, world.tod as i32, assets.biomes[biome].name);
        }
        TableKind::Art => {
            // Preview through whatever references the art: a prop, a
            // creature, or a tileset's tiny tree.
            let names: Vec<&str> = assets.art.names();
            let name = names.get(subject.row).copied().unwrap_or("");
            if let Some(pi) = assets.props.iter().position(|p| p.art == name) {
                return build(assets, Subject { kind: TableKind::Props, row: pi }, s);
            }
            if let Some(ci) = assets.creatures.iter().position(|c| c.art == name) {
                return build(assets, Subject { kind: TableKind::Creatures, row: ci }, s);
            }
            for t in &assets.tilesets {
                let tiny = &t.art.tiny;
                let form = [(&tiny.pine, crate::biome::Form::Pine), (&tiny.broadleaf, crate::biome::Form::Broadleaf), (&tiny.scrub, crate::biome::Form::Scrub), (&tiny.cactus, crate::biome::Form::Cactus)]
                    .into_iter()
                    .find(|(n, _)| n.as_str() == name)
                    .map(|(_, f)| f);
                if let Some(form) = form {
                    if let Some(si) = assets.species.iter().position(|sp| sp.form == form) {
                        let mut f = build(assets, Subject { kind: TableKind::Species, row: si }, s);
                        f.glyphs = assets.setting("glyphs").and_then(|g| g.values.iter().position(|v| *v == t.name));
                        return f;
                    }
                }
            }
        }
        _ => {}
    }
    if caption.is_empty() {
        caption = match subject.kind {
            TableKind::Surfaces => format!("showing {:?} ground", terrain).to_lowercase(),
            TableKind::Seasons => format!("showing {} ground in that season", assets.biomes[biome].name),
            TableKind::Tilesets => format!("showing {} ground drawn with these glyphs", assets.biomes[biome].name),
            _ => format!("showing {} ground", assets.biomes[biome].name),
        };
    }
    Fixture { map, world, cx, cy, biome: biome % assets.biomes.len().max(1), glyphs, caption, top }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    #[test]
    fn subjects_land_on_the_centre_tile() {
        let a = test_assets();
        let s = PreviewSettings::default();
        let oak = a.species.iter().position(|s| s.name == "oak").unwrap();
        let f = build(&a, Subject { kind: TableKind::Species, row: oak }, &s);
        assert_eq!(f.map.get(f.cx, f.cy).unwrap().tree.map(|t| t.species as usize), Some(oak));
        assert!(f.map.is_fixture());
        // The first pattern is the ground the generator gives a house: four
        // tiles by three, all of one kind and level count, so it merges.
        let f = build(&a, Subject { kind: TableKind::Blocks, row: 0 }, &PreviewSettings { pattern: 0, ..s.clone() });
        assert_eq!(f.map.get(f.cx + 1, f.cy + 1).unwrap().stack.map(|st| st.kind), Some(0));
        assert!(f.map.get(f.cx + 2, f.cy).unwrap().stack.is_none(), "the pattern is four wide, not five");
        assert_eq!(f.map.get(f.cx, f.cy).unwrap().stack, f.map.get(f.cx + 1, f.cy + 1).unwrap().stack, "one level count across the pattern, so it merges");
        let f = build(&a, Subject { kind: TableKind::Blocks, row: 0 }, &PreviewSettings { pattern: 1, ..s.clone() });
        assert!(f.map.get(f.cx + 1, f.cy).unwrap().stack.is_none(), "a lone tile is one tile");
        let f = build(&a, Subject { kind: TableKind::Props, row: 0 }, &s);
        assert_eq!(f.world.placed.len(), 4);
        assert!(f.world.placed.iter().all(|p| p.x.floor() as i32 == f.cx && p.y.floor() as i32 == f.cy));
        let f = build(&a, Subject { kind: TableKind::Creatures, row: 0 }, &s);
        assert_eq!(f.world.entities.len(), 1);
        let f = build(&a, Subject { kind: TableKind::Lights, row: 0 }, &s);
        assert_eq!(f.world.lights.len(), 1);
        assert_eq!(f.world.tod, 22.0);
        let f = build(&a, Subject { kind: TableKind::Materials, row: 2 }, &s);
        assert_eq!(f.map.get(f.cx, f.cy).unwrap().material, 2);
        let f = build(&a, Subject { kind: TableKind::Surfaces, row: 3 }, &s);
        assert_eq!(f.map.get(0, 0).unwrap().terrain, Terrain::Snow);
        let f = build(&a, Subject { kind: TableKind::Seasons, row: 3 }, &s);
        assert_eq!(f.world.season, 3.0);
        let ice = a.biomes.iter().position(|b| b.koppen == "EF").unwrap();
        let f = build(&a, Subject { kind: TableKind::Biomes, row: ice }, &s);
        assert_eq!(f.map.get(0, 0).unwrap().terrain, Terrain::Snow);
        let f = build(&a, Subject { kind: TableKind::Species, row: 999 }, &s);
        assert!(f.map.get(f.cx, f.cy).unwrap().tree.is_none());
    }

    #[test]
    fn art_previews_through_its_reference() {
        let a = test_assets();
        let s = PreviewSettings::default();
        let names = a.art.names();
        let boulder = names.iter().position(|n| *n == "boulder").unwrap();
        let f = build(&a, Subject { kind: TableKind::Art, row: boulder }, &s);
        assert_eq!(f.world.placed.len(), 4);
        let player = names.iter().position(|n| *n == "player").unwrap();
        let f = build(&a, Subject { kind: TableKind::Art, row: player }, &s);
        assert_eq!(f.world.entities.len(), 1);
        let pine = names.iter().position(|n| *n == "ascii/pine").unwrap();
        let f = build(&a, Subject { kind: TableKind::Art, row: pine }, &s);
        assert!(f.map.get(f.cx, f.cy).unwrap().tree.is_some());
        assert_eq!(f.glyphs, Some(1));
    }
}
