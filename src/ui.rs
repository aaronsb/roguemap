//! The game's frame contents (ADR-005): the status bar, the help line, the
//! settings form, the world map, the inset view, and the inventory, stats,
//! history and conversation panes. `frame.rs` owns the rectangles, the
//! chrome and the focus; this module says what goes inside them and
//! supplies the content kind for each row of `assets/ui.toml`.

use std::any::Any;
use std::cell::RefCell;

use crate::assets::Assets;
use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::frame::{pad, Anchor, Content, FrameCtx, Frames, Item, List, Rect, Text};
use crate::input::{self, SCENE, SETTINGS};
use crate::palette::{season_blend, SEASON_NAMES};
use crate::render::{FogMode, RenderOptions, Renderer, Scene};
use crate::settings::Settings;

/// Colours of the text chrome.
pub struct Chrome {
    /// Status and legend bars.
    pub bar: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    /// World map legend labels.
    pub legend: Rgb,
    /// The modal window.
    pub panel: Rgb,
    pub panel_text: Rgb,
    pub panel_dim: Rgb,
    pub selected: Rgb,
}

pub const CHROME: Chrome = Chrome {
    bar: Rgb(30, 32, 44),
    text: Rgb(220, 220, 230),
    dim: Rgb(160, 160, 176),
    legend: Rgb(200, 200, 210),
    panel: Rgb(28, 30, 44),
    panel_text: Rgb(225, 225, 235),
    panel_dim: Rgb(140, 140, 160),
    selected: Rgb(200, 200, 215),
};

/// The content kinds `assets/ui.toml` may name.
pub const CONTENT_KINDS: [&str; 9] = ["hud", "help", "settings", "worldmap", "inset", "inventory", "stats", "history", "conversation"];

/// A content per kind; the loader has already checked the name is one of
/// `CONTENT_KINDS`.
pub fn content_for(kind: &str) -> Option<Box<dyn Content>> {
    Some(match kind {
        "hud" => Box::new(Hud) as Box<dyn Content>,
        "help" => Box::new(Help),
        "settings" => Box::new(SettingsForm),
        "worldmap" => Box::new(WorldMapView),
        "inset" => Box::<Inset>::default(),
        "inventory" => Box::new(List::hinted("carrying nothing")),
        "stats" => Box::new(List::live(stats_items)),
        "history" => Box::new(List::ring(HISTORY, "nothing has happened yet")),
        "conversation" => Box::new(Text::prompting(vec!["Nobody is listening yet.".to_string()], "say: ")),
        _ => return None,
    })
}

/// How many events the history frame keeps.
pub const HISTORY: usize = 50;

/// The game's frame set from `assets/ui.toml`.
pub fn frames(assets: &Assets) -> Frames {
    Frames::new(&assets.frames, &content_for).expect("the loader checked every content kind")
}

/// The corners the `inset` settings row offers, in the order of its values
/// after `off`.
pub const INSET_CORNERS: [Anchor; 4] = [Anchor::BottomRight, Anchor::BottomLeft, Anchor::TopRight, Anchor::TopLeft];

/// Push the settings rows that govern frames into the frame set: the HUD
/// bars, and the inset view's corner or its absence. The `ui.toml` show
/// rule still has the last word, so the inset stays hidden on a screen too
/// narrow for it whatever the row says.
pub fn apply_settings(frames: &mut Frames, settings: &Settings) {
    let hud = settings.get("hud") == 0;
    frames.set_open("hud-top", hud);
    frames.set_open("hud-help", hud);
    let corner = settings.get("inset");
    frames.set_open("inset", corner != 0);
    if let Some(&anchor) = corner.checked_sub(1).and_then(|i| INSET_CORNERS.get(i)) {
        frames.set_anchor("inset", anchor);
    }
}

/// Status bar: heading, the zoom as its ratio and name, season, clock,
/// weather, glyph set, the light count and the tile under the player.
pub struct Hud;

impl Content for Hud {
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx) {
        let world = ctx.world;
        let s = world.season.rem_euclid(4.0);
        let here = world
            .player()
            .and_then(|e| ctx.map.get(e.mx(), e.my()))
            .map(|t| {
                let b = t.biome(&ctx.map.assets);
                format!("{} ({}) {}C z{}", b.name, b.koppen, t.temp, t.z)
            })
            .unwrap_or_default();
        let wx = &world.weather;
        let weather = format!("cloud {:.0}% wind {:.0}% precip {:.0}%", wx.cover * 100.0, wx.wind * 100.0, wx.precip * 100.0);
        let line = format!(
            " roguemap  {}deg  {}  {} ({:.2})  {:02}:{:02}{}  {}  glyphs:{}  lights:{}  {} ",
            ctx.cam.degrees(),
            ctx.cam.view_label(),
            SEASON_NAMES[season_blend(world.season).0],
            s,
            world.tod.floor() as i32,
            ((world.tod.fract()) * 60.0) as i32,
            if world.auto_time { "" } else { " (paused)" },
            weather,
            ctx.ts.name,
            ctx.lights,
            here,
        );
        cv.text(rect.x, rect.y, &line, CHROME.text, CHROME.bar);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The generated scene help line.
pub struct Help;

impl Content for Help {
    fn draw(&self, cv: &mut Canvas, rect: Rect, _ctx: &FrameCtx) {
        cv.text(rect.x, rect.y, &input::help_line(SCENE, "  "), CHROME.dim, CHROME.bar);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Width and height the settings form wants: one row per table item, with
/// a blank row above and the key hints below.
pub fn settings_size(settings: &Settings) -> (i32, i32) {
    let items = &settings.items;
    let name_w = items.iter().map(|i| i.label.len()).max().unwrap_or(8);
    let val_w = items.iter().flat_map(|i| i.values.iter().map(|v| v.len())).max().unwrap_or(8);
    let hint = input::help_line(SETTINGS, "   ");
    ((name_w + val_w + 11).max(hint.len()) as i32, items.len() as i32 + 2)
}

/// The settings table, one row per item, the selected row inverted.
pub fn settings_form(cv: &mut Canvas, rect: Rect, settings: &Settings) {
    let items = &settings.items;
    let name_w = items.iter().map(|i| i.label.len()).max().unwrap_or(8);
    let val_w = items.iter().flat_map(|i| i.values.iter().map(|v| v.len())).max().unwrap_or(8);
    let hint = input::help_line(SETTINGS, "   ");
    let (fg, bg, dim) = (CHROME.panel_text, CHROME.panel, CHROME.panel_dim);
    cv.text(rect.x, rect.y, &pad("", rect.w), dim, bg);
    for (i, item) in items.iter().enumerate() {
        let selected = i == settings.cursor;
        let body = format!("  {:<nw$}   {} {:^vw$} {}", item.label, if selected { '<' } else { ' ' }, settings.label(i), if selected { '>' } else { ' ' }, nw = name_w, vw = val_w);
        let (lf, lb) = if selected { (bg, CHROME.selected) } else { (fg, bg) };
        cv.text(rect.x, rect.y + 1 + i as i32, &pad(&body, rect.w), lf, lb);
    }
    cv.text(rect.x, rect.y + rect.h - 1, &pad(&hint, rect.w), dim, bg);
}

/// The modal settings window. The rows live in `Settings`, so the frame
/// only draws and leaves the keys to the settings binding table.
pub struct SettingsForm;

impl Content for SettingsForm {
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx) {
        settings_form(cv, rect, ctx.settings);
    }

    fn preferred(&self, ctx: &FrameCtx) -> Option<(i32, i32)> {
        Some(settings_size(ctx.settings))
    }

    fn focusable(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The world map plot. Its cursor and extent live in `WorldMap`.
pub struct WorldMapView;

impl Content for WorldMapView {
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx) {
        ctx.wmap.draw(cv, rect, ctx.map, ctx.world, ctx.world.player().map(|e| e.tile()));
    }

    fn focusable(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The camera, renderer and canvas the inset draws with, at the size of
/// the frame's interior.
struct InsetView {
    w: i32,
    h: i32,
    cam: Camera,
    renderer: Renderer,
    canvas: Canvas,
}

impl InsetView {
    fn new(w: i32, h: i32) -> InsetView {
        let (w, h) = (w.max(1), h.max(1));
        InsetView { w, h, cam: Camera::new(), renderer: Renderer::new(w, h), canvas: Canvas::new(w as u16, h as u16) }
    }
}

/// The second view of ADR-004: a camera and a renderer of its own over the
/// same scene, following the player at the other end of the zoom scale.
/// While the main view is zoomed out at all — 1:2, 1:4 or 1:8 — the inset
/// shows 1:1; at 1:1 it shows 1:8, so the two never share a level. It draws
/// the scene alone, with no HUD over it and with antialiasing and the cloud
/// layer off, then blits into the frame's interior. Which corner it sits in,
/// or whether it shows at all, is the `inset` settings row.
///
/// A content draws from `&self`, so the view it owns sits behind a
/// `RefCell`; it is rebuilt whenever the interior changes size.
pub struct Inset {
    view: RefCell<InsetView>,
}

impl Default for Inset {
    fn default() -> Inset {
        Inset { view: RefCell::new(InsetView::new(1, 1)) }
    }
}

impl Content for Inset {
    fn draw(&self, cv: &mut Canvas, rect: Rect, ctx: &FrameCtx) {
        if rect.is_empty() {
            return;
        }
        let mut view = self.view.borrow_mut();
        if (view.w, view.h) != (rect.w, rect.h) {
            *view = InsetView::new(rect.w, rect.h);
        }
        let InsetView { w, h, cam, renderer, canvas } = &mut *view;
        let (w, h) = (*w, *h);
        *cam = ctx.cam.inset();
        // The inset follows the player wherever they stand; with nobody in
        // the world it keeps the main view's centre.
        match ctx.world.player() {
            Some(p) => cam.look_at_entity(p, ctx.map, w, h),
            None => {
                let (mx, my) = ctx.cam.center_tile(ctx.map, cv.w, cv.h);
                cam.look_at(mx, my, ctx.map, w, h);
            }
        }
        let scene = Scene::new(ctx.map, ctx.ts, ctx.world, cam, ctx.t);
        renderer.draw(canvas, &scene, &RenderOptions { aa: false, clouds: false, fog: FogMode::Perspective });
        cv.blit(canvas, rect.x, rect.y);
    }

    fn title(&self, row: &str, ctx: &FrameCtx) -> Option<String> {
        Some(format!("{row} {}", Camera::inset_ratio(ctx.cam.zoom)).trim().to_string())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The stats readout, rebuilt from the world every frame.
fn stats_items(ctx: &FrameCtx) -> Vec<Item> {
    let world = ctx.world;
    let player = world.player();
    let position = player.map(|e| {
        let (x, y) = e.metres();
        format!("{x:.2}, {y:.2} m  tile {}, {}", e.mx(), e.my())
    });
    let mut items = vec![Item::detailed("position", position.unwrap_or_else(|| "unplaced".to_string()))];
    if let Some(t) = player.and_then(|e| ctx.map.get(e.mx(), e.my())) {
        let b = t.biome(&ctx.map.assets);
        items.push(Item::detailed("biome", format!("{} ({})", b.name, b.koppen)));
        items.push(Item::detailed("temperature", format!("{} C", t.temp)));
        items.push(Item::detailed("height", format!("z{}", t.z)));
    }
    let season = SEASON_NAMES[season_blend(world.season).0];
    items.push(Item::detailed("time", format!("{:02}:{:02}  {season}", world.tod.floor() as i32, (world.tod.fract() * 60.0) as i32)));
    let wx = &world.weather;
    items.push(Item::detailed("weather", format!("cloud {:.0}%  wind {:.0}%  precip {:.0}%", wx.cover * 100.0, wx.wind * 100.0, wx.precip * 100.0)));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;
    use crate::camera::Camera;
    use crate::frame::{Flow, Rect};
    use crate::input::{lookup, Action};
    use crate::map::Map;
    use crate::tileset::Tileset;
    use crate::world::World;
    use crate::worldmap::WorldMap;
    use crossterm::event::KeyCode;

    #[test]
    fn every_content_kind_has_a_content() {
        for kind in CONTENT_KINDS {
            assert!(content_for(kind).is_some(), "{kind}");
        }
        assert!(content_for("nonesuch").is_none());
    }

    #[test]
    fn the_game_frame_set_builds_from_the_tables() {
        let assets = test_assets();
        let frames = frames(&assets);
        assert_eq!(frames.len(), assets.frames.len());
        for f in frames.iter() {
            assert!(CONTENT_KINDS.contains(&f.spec.content.as_str()), "{}", f.spec.name);
        }
        assert!(frames.is_open("hud-top") && frames.is_open("hud-help"), "the bars start shown");
        assert!(!frames.is_open("settings") && !frames.is_open("worldmap"), "the rest wait for their key");
    }

    #[test]
    fn the_settings_form_asks_for_the_size_it_draws() {
        let settings = Settings::new(&test_assets());
        let (w, h) = settings_size(&settings);
        assert_eq!(h, settings.items.len() as i32 + 2, "a blank row above and the hints below");
        assert!(w >= input::help_line(SETTINGS, "   ").len() as i32, "the hints fit");
    }

    #[test]
    fn toggle_keys_agree_with_the_binding_table() {
        let a = test_assets();
        for f in &a.frames {
            let Some(key) = f.key else { continue };
            match lookup(SCENE, key, false) {
                Some(Action::Toggle(name)) => assert_eq!(name, f.name, "{key:?} toggles the wrong frame"),
                other => panic!("ui.toml gives {} the key {key:?}, which input.rs binds to {other:?}", f.name),
            }
        }
        for &(key, action) in SCENE.iter().flat_map(|b| b.keys) {
            if let Action::Toggle(name) = action {
                let frame = a.frame(name).unwrap_or_else(|| panic!("input.rs toggles unknown frame {name:?}"));
                assert!(frame.key.is_some(), "{name} is toggled by {key:?} but names no key in ui.toml");
            }
        }
    }

    /// Everything the frame contents draw from, over a small flat world.
    struct Fixture {
        map: Map,
        world: World,
        cam: Camera,
        tilesets: Vec<Tileset>,
        settings: Settings,
        wmap: WorldMap,
    }

    impl Fixture {
        fn new() -> Fixture {
            let assets = test_assets();
            Fixture { map: Map::new(8, 8, 1, assets.clone()), world: World::new(1), cam: Camera::new(), tilesets: Tileset::all(&assets), settings: Settings::new(&assets), wmap: WorldMap::new() }
        }

        fn ctx(&self) -> FrameCtx<'_> {
            FrameCtx { map: &self.map, world: &self.world, cam: &self.cam, ts: &self.tilesets[0], settings: &self.settings, wmap: &self.wmap, lights: 0, t: 0.0, focused: false }
        }
    }

    #[test]
    fn the_game_frames_lay_out_at_both_screen_sizes() {
        let fx = Fixture::new();
        let ctx = fx.ctx();
        let mut f = frames(&test_assets());
        let at = |name: &str| f.index(name).unwrap_or_else(|| panic!("{name} is a frame"));
        let (top, help, inset) = (at("hud-top"), at("hud-help"), at("inset"));
        for (cols, rows) in [(80, 25), (168, 71)] {
            let l = f.layout(cols, rows, &ctx);
            assert_eq!(l.rect(top), Some(Rect::new(0, 0, cols, 1)), "the status bar spans the top row");
            assert_eq!(l.rect(help), Some(Rect::new(0, rows - 1, cols, 1)), "the help line sits on the last row: the inset ranks below it");
            if cols < 100 {
                assert_eq!(l.rect(inset), None, "no room for the inset below a hundred columns");
                assert_eq!(l.order, vec![top, help]);
            } else {
                assert_eq!(l.rect(inset), Some(Rect::new(cols - cols / 4, rows - 21, cols / 4, 21)), "a quarter of the width in the bottom right corner");
                assert_eq!(l.order, vec![top, help, inset], "and it draws over the tail of the help line");
            }
        }
        // The popover sizes itself from the settings table and clears both bars.
        let (w, h) = settings_size(&fx.settings);
        f.set_open("settings", true);
        for (cols, rows) in [(80, 25), (168, 71)] {
            let l = f.layout(cols, rows, &ctx);
            let want = Rect::new((cols - w - 2) / 2, (rows - h - 2) / 2, w + 2, h + 2);
            assert_eq!(l.rect(f.index("settings").unwrap()), Some(want), "the border pays for itself out of the auto size");
            assert!(l.rect(top).is_some() && l.rect(help).is_some(), "the popover does not reach the bars");
        }
        // The world map is opaque, full screen and outranks everything.
        f.set_open("worldmap", true);
        assert_eq!(f.layout(80, 25, &ctx).order, vec![f.index("worldmap").unwrap()]);
        // The four panes sit side by side on a big screen; on the floor
        // size the lower-priority ones give way where they collide.
        f.set_open("worldmap", false);
        f.set_open("settings", false);
        for pane in ["inventory", "stats", "history", "conversation"] {
            f.set_open(pane, true);
        }
        let l = f.layout(168, 71, &ctx);
        for pane in ["inventory", "stats", "history", "conversation"] {
            assert!(l.rect(f.index(pane).unwrap()).is_some(), "{pane} fits at 168x71");
        }
        assert_eq!(l.rect(help), None, "the conversation covers the help line");
        let l = f.layout(80, 25, &ctx);
        assert!(l.rect(f.index("conversation").unwrap()).is_some(), "the highest-priority pane survives");
        assert_eq!(l.rect(f.index("inventory").unwrap()), None, "the inventory yields to it at 80x25");
        assert_eq!(l.rect(f.index("stats").unwrap()), None);
        assert!(l.rect(f.index("history").unwrap()).is_some(), "history clears it");
    }

    /// Draw the inset into a canvas of its own and return its cells.
    fn inset_cells(inset: &Inset, fx: &Fixture, w: i32, h: i32) -> Vec<crate::canvas::Cell> {
        let mut cv = Canvas::new(w as u16, h as u16);
        inset.draw(&mut cv, Rect::new(0, 0, w, h), &fx.ctx());
        cv.cells
    }

    #[test]
    fn the_inset_follows_the_player_and_names_the_ratio_it_draws_at() {
        let mut fx = Fixture::new();
        fx.world.spawn_player(&fx.map, 2, 2);
        let inset = Inset::default();
        let here = inset_cells(&inset, &fx, 24, 10);

        // The main view panning away leaves the inset where it was: it
        // follows the player, not the camera it hangs off.
        fx.cam.pan(3, 3);
        assert_eq!(inset_cells(&inset, &fx, 24, 10), here, "panning the main view does not move the inset");

        // Walking does move it, by a step short of a tile as much as by tiles.
        let p = fx.world.player_mut().expect("the player was spawned");
        let (mx, my) = p.tile();
        p.x_cm += 60;
        p.y_cm += 60;
        assert_eq!(p.tile(), (mx, my), "still in the same tile");
        assert_ne!(inset_cells(&inset, &fx, 24, 10), here, "the inset followed the player within the tile");
        let p = fx.world.player_mut().expect("the player was spawned");
        p.set_tile(mx + 4, my + 4);
        assert_ne!(inset_cells(&inset, &fx, 24, 10), here, "the inset followed the player");

        // The title is the row's plus the ratio, which is the far end of
        // the scale from the main view.
        assert_eq!(inset.title("inset", &fx.ctx()), Some("inset 1:1".to_string()), "the main view starts at 1:8");
        fx.cam.set_zoom(crate::tileset::ZOOMS.len() - 1, 24, 10);
        assert_eq!(inset.title("inset", &fx.ctx()), Some("inset 1:8".to_string()), "and at 1:1 the inset shows 1:8");
    }

    #[test]
    fn the_inset_settings_row_picks_a_corner_or_turns_the_view_off() {
        let assets = test_assets();
        let fx = Fixture::new();
        let ctx = fx.ctx();
        let mut f = frames(&assets);
        let mut s = Settings::new(&assets);
        let at = f.index("inset").expect("inset is a frame");
        let row = s.find("inset").expect("inset is a settings row");
        assert_eq!(s.label(row), "bottom-right", "the view starts in the bottom right corner");

        // A quarter of the width by about a third of the height, in the
        // corner the row names.
        let (cols, rows) = (168, 71);
        let (w, h) = (42, 21);
        let corners = [(cols - w, rows - h), (0, rows - h), (cols - w, 0), (0, 0)];
        for (i, (x, y)) in corners.iter().enumerate() {
            s.set("inset", i + 1);
            apply_settings(&mut f, &s);
            assert_eq!(f.layout(cols, rows, &ctx).rect(at), Some(Rect::new(*x, *y, w, h)), "{}", s.label(row));
        }

        // Off hides it, whatever the screen.
        s.set("inset", 0);
        apply_settings(&mut f, &s);
        assert_eq!(f.layout(cols, rows, &ctx).rect(at), None, "off hides the view");

        // And below a hundred columns there is no room for it at all.
        s.set("inset", 1);
        apply_settings(&mut f, &s);
        assert_eq!(f.layout(99, 71, &ctx).rect(at), None, "hidden below a hundred columns");
        assert!(f.layout(100, 71, &ctx).rect(at).is_some(), "and shown at a hundred");
        assert_eq!(f.layout(80, 25, &ctx).rect(at), None, "the floor size has no inset");
    }

    #[test]
    fn focus_routes_keys_and_escape_returns_it_to_the_scene() {
        let mut f = frames(&test_assets());
        assert_eq!(f.focus(), None, "the scene starts with focus");
        assert_eq!(f.key(KeyCode::Char('x')), Flow::Pass, "with no focused frame every key falls through");
        assert!(f.is_open("hud-top") && f.focus().is_none(), "passive frames never take focus");

        f.set_open("history", true);
        assert_eq!(f.focus(), Some("history"));
        let list = f.content_mut::<List>("history").expect("history is a list");
        list.push(Item::new("one"));
        list.push(Item::detailed("two", "a detail"));
        assert_eq!(list.cursor, 1);
        assert_eq!(f.key(KeyCode::Up), Flow::Handled, "the focused frame takes the key");
        assert_eq!(f.content_mut::<List>("history").unwrap().cursor, 0);

        // Opening another focusable frame moves the focus; only one has it.
        f.set_open("conversation", true);
        assert_eq!(f.focus(), Some("conversation"));
        for c in "hi".chars() {
            assert_eq!(f.key(KeyCode::Char(c)), Flow::Handled, "the prompt takes characters, not commands");
        }
        assert_eq!(f.key(KeyCode::Enter), Flow::Submit("hi".to_string()));
        assert_eq!(f.key(KeyCode::Esc), Flow::Close, "escape closes the frame");
        assert_eq!(f.focus(), None, "and returns focus to the scene");
        assert!(!f.is_open("conversation") && f.is_open("history"), "the other frame stays open but unfocused");

        // The toggle key is the same door in and out.
        assert!(f.toggle("settings") && f.focus() == Some("settings"));
        assert!(!f.toggle("settings") && f.focus().is_none());
    }
}
