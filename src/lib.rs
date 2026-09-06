//! roguemap: an isometric, height-mapped terrain renderer for the terminal.
//!
//! The library holds everything but the event loops: the game binary
//! (`src/bin/roguemap.rs`) and the asset editor (`src/bin/roguemap-edit.rs`)
//! share the renderer, the world model and the asset loader.

pub mod assets;
pub mod biome;
pub mod camera;
pub mod canvas;
pub mod input;
pub mod lighting;
pub mod map;
pub mod noise;
pub mod overlay;
pub mod palette;
pub mod properties;
pub mod raster;
pub mod render;
pub mod settings;
pub mod sprite;
pub mod sprites;
pub mod terminal;
pub mod tileset;
pub mod ui;
pub mod world;
pub mod worldmap;
