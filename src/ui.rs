//! Text chrome over the scene: the status bar and help line, and the modal
//! settings window.

use crate::camera::Camera;
use crate::canvas::{Canvas, Rgb};
use crate::input::{self, SCENE, SETTINGS};
use crate::map::Map;
use crate::palette::{season_blend, SEASON_NAMES};
use crate::settings::Settings;
use crate::tileset::Tileset;
use crate::world::World;

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

/// Status bar on the top row and help line on the bottom. `lights` is how
/// many point lights the frame carried, placed and discovered.
pub fn hud(cv: &mut Canvas, map: &Map, ts: &Tileset, world: &World, cam: &Camera, lights: usize) {
    let s = world.season.rem_euclid(4.0);
    let here = world
        .player()
        .and_then(|e| map.get(e.mx, e.my))
        .map(|t| {
            let b = t.biome(&map.assets);
            format!("{} ({}) {}C z{}", b.name, b.koppen, t.temp, t.z)
        })
        .unwrap_or_default();
    let wx = &world.weather;
    let weather = format!("cloud {:.0}% wind {:.0}% precip {:.0}%", wx.cover * 100.0, wx.wind * 100.0, wx.precip * 100.0);
    let line = format!(
        " roguemap  {}deg  zoom {}  {} ({:.2})  {:02}:{:02}{}  {}  glyphs:{}  lights:{}  {} ",
        cam.degrees(),
        cam.zoom,
        SEASON_NAMES[season_blend(world.season).0],
        s,
        world.tod.floor() as i32,
        ((world.tod.fract()) * 60.0) as i32,
        if world.auto_time { "" } else { " (paused)" },
        weather,
        ts.name,
        lights,
        here,
    );
    cv.text(0, 0, &line, CHROME.text, CHROME.bar);
    cv.text(0, cv.h - 1, &input::help_line(SCENE, "  "), CHROME.dim, CHROME.bar);
}

/// Modal settings window, one row per table item.
pub fn popover(cv: &mut Canvas, settings: &Settings) {
    let items = &settings.items;
    let name_w = items.iter().map(|i| i.label.len()).max().unwrap_or(8);
    let val_w = items.iter().flat_map(|i| i.values.iter().map(|v| v.len())).max().unwrap_or(8);
    let hint = input::help_line(SETTINGS, "   ");
    let inner = (name_w + val_w + 11).max(hint.len());
    let rows = items.len() as i32 + 4;
    let x0 = (cv.w - inner as i32 - 2) / 2;
    let y0 = (cv.h - rows) / 2;
    let (fg, bg, dim) = (CHROME.panel_text, CHROME.panel, CHROME.panel_dim);
    let row = |body: &str| format!("│{:<w$}│", body, w = inner);
    cv.text(x0, y0, &format!("┌{}┐", "─".repeat(inner)), dim, bg);
    cv.text(x0 + 2, y0, " settings ", fg, bg);
    cv.text(x0, y0 + 1, &row(""), dim, bg);
    for (i, item) in items.iter().enumerate() {
        let selected = i == settings.cursor;
        let body = format!(
            "  {:<nw$}   {} {:^vw$} {}",
            item.label,
            if selected { '<' } else { ' ' },
            settings.label(i),
            if selected { '>' } else { ' ' },
            nw = name_w,
            vw = val_w
        );
        let (lf, lb) = if selected { (bg, CHROME.selected) } else { (fg, bg) };
        let y = y0 + 2 + i as i32;
        cv.text(x0, y, &row(&body), lf, lb);
        cv.put(x0, y, '│', dim, bg);
        cv.put(x0 + inner as i32 + 1, y, '│', dim, bg);
    }
    cv.text(x0, y0 + rows - 2, &row(&hint), dim, bg);
    cv.text(x0, y0 + rows - 1, &format!("└{}┘", "─".repeat(inner)), dim, bg);
}
