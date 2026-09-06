# roguemap

[github.com/aaronsb/roguemap](https://github.com/aaronsb/roguemap)

An isometric, height-mapped terrain renderer for the terminal, in Rust.

There is no tile atlas. Every screen cell casts a ray back into the world
and walks down the continuous height field under it, testing the buildings
and trees in view on the way, until it meets a surface — so the camera
sits at any angle and the terrain has no seams. Buildings are block
geometry that merges with its neighbours under one roof. Trees are grown
from L-system grammars and tested on the same walk, with porous canopies
you can see through. A deferred lighting pass adds ambient sky light, a
sun shadowed by drifting clouds and by everything that casts, and point
lights. Cells on a boundary are supersampled 2x3 and drawn as a sextant
glyph in two colours.

The world is a pure function of position and seed: a continuous height
field, a climate over it, a simplified Köppen classification, and biomes
that follow. Nothing is authored.

![island](docs/screenshots/island.png)

## Gallery

Rendered by `make screenshots` through the headless snapshot pipeline.

| | |
|---|---|
| ![scale-zclose](docs/screenshots/scale-zclose.png) The yardstick at 1:1: a 2 m person, an 18 m oak, a house | ![scale-zfar](docs/screenshots/scale-zfar.png) The same ground at 1:8 |
| ![lsystem-stand-near](docs/screenshots/lsystem-stand-near.png) A boreal stand at 1:2, every spruce grown from its habit | ![lsystem-stand-close](docs/screenshots/lsystem-stand-close.png) The same stand at 1:1: trunks, whorls and porous crowns |
| ![village](docs/screenshots/village.png) A village on the steppe at 1:4 | ![filled](docs/screenshots/filled.png) The filled world at 1:4 |
| ![night](docs/screenshots/night.png) Night, a campfire and lit windows | ![winter](docs/screenshots/winter.png) Winter after two days of storm |
| ![clouds](docs/screenshots/clouds.png) Above the clouds at the smallest zoom | ![steppe](docs/screenshots/steppe.png) Steppe with adobe houses |
| ![worldmap](docs/screenshots/worldmap.png) The world map | ![ascii](docs/screenshots/ascii.png) The ASCII glyph set |
| ![lsystem-oak](docs/screenshots/lsystem-oak.png) A gnarled oak in summer | ![lsystem-conifer](docs/screenshots/lsystem-conifer.png) A spruce from the `excurrent` habit |
| ![editor](docs/screenshots/editor.png) The asset editor | ![settings](docs/screenshots/settings.png) The settings frame |

## Run

```
cargo run --release -- [seed] [size]
make run                # the same, seed 7 and a 32x32 map
make term               # a Konsole window with the canonical font and size
make edit               # the asset editor on ./assets-export
make check              # clippy as errors, tests, asset tests, golden frames
```

Anything from 80x25 upward works. The starting zoom is the largest at
which the whole map fits the window.

The snapshot renderer is the reference for how the game should look:
`tools/cells2png.py` draws a cell dump with Unscii 16 at exactly 16
pixels, so every cell is 8x16 with no smoothing. `./term.sh` opens Konsole
configured to match. The PETSCII tileset needs a font covering U+1FB00 to
U+1FBFF; `make fonts` lists the ones installed that do.

## Keys

Generated from the binding table in `src/input.rs`, which also writes the
help line at the bottom of the screen.

| Key | Action |
|---|---|
| `w a s d` or `h j k l` | walk the player; the camera follows |
| arrows | pan by one tile; shift with an arrow strides eight |
| `c` | centre on the player |
| `r` / `R`, `(` / `)` | rotate a quarter turn, or five degrees |
| `z` / `Z` | zoom through far 1:8, mid 1:4, near 1:2, close 1:1 |
| `v` | island or filled world |
| `g` | PETSCII or ASCII glyphs |
| `[` / `]`, `,` / `.`, `p` | step the season, step the clock, pause it |
| `W`, `f`, `F` | cycle the weather, light a campfire, put the fires out |
| `Tab` or `o` | the settings frame |
| `m` | the world map |
| `n` | the inset view at the other end of the zoom scale |
| `i`, `I`, `L`, `C` | inventory, stats, history, conversation |
| `H` | hide the HUD |
| `q` or `Esc` | quit |

Every frame key toggles: the same key opens and closes it, and `Esc`
closes the focused frame and gives the keys back to the scene. `Ctrl-C`
quits from any mode.

## Documentation

The design tree starts at **[docs/index.md](docs/index.md)**, which has
the gallery, the map of the pages and how to run everything.

| Page | What it covers |
|---|---|
| [world.md](docs/world.md) | The height field, relief in metres, rivers, climate, biomes, chunks, the world map |
| [rendering.md](docs/rendering.md) | The ray walk, block and tree geometry, shadows, lighting, sextant antialiasing, level of detail, cost |
| [scale.md](docs/scale.md) | Metres, the four zooms and their ratios, movement, the inset view |
| [trees.md](docs/trees.md) | The eight growth habits, their parameters, seasons, dead trees, the stand-in rule |
| [assets-and-editor.md](docs/assets-and-editor.md) | The asset directory, the loader, the editor, how to add a row |
| [frames.md](docs/frames.md) | Frames over the scene: HUD, settings, world map, inset, panes |
| [weather.md](docs/weather.md) | Wind, clouds, precipitation, accumulation, seasons, day and night |
| [testing.md](docs/testing.md) | What is tested and how the golden frames work |

The catalogues and decision records those pages lean on:

| Document | What it is |
|---|---|
| [assets.md](docs/assets.md) | The asset directory layout and the art file format |
| [properties.md](docs/properties.md) | Every property a row may carry, with type and default |
| [structures.md](docs/structures.md) | Block geometry, adjacency, faces, shadows, trees as volumes |
| [lsystem.md](docs/lsystem.md) | The L-system grammar, its symbols and the eight habits |
| [ADR-001](docs/adr/ADR-001-assets-as-files.md) | Assets as files rather than Rust constants |
| [ADR-002](docs/adr/ADR-002-block-geometry-structures-and-trees.md) | Structures as block geometry, trees as volumes |
| [ADR-003](docs/adr/ADR-003-asset-editor.md) | The editor as a second binary over a shared library |
| [ADR-004](docs/adr/ADR-004-world-scale.md) | World scale in metres and zoom as exact ratios |
| [ADR-005](docs/adr/ADR-005-overlay-frames.md) | One frame system for everything drawn over the scene |

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the build, the gate and where
things go. In short: everything that describes a thing in the world is a
row under `assets/`, not a Rust constant; large things are geometry on the
ray walk, not sprites; everything drawn over the scene is a frame.
`make check` must be green, and a change that alters pixels re-records the
golden frames in the same commit and says which moved and why.

## License

Licensed under either of Apache License 2.0 or MIT license at your option.
See [LICENSE-APACHE](LICENSE-APACHE), [LICENSE-MIT](LICENSE-MIT) and
[NOTICE](NOTICE).
