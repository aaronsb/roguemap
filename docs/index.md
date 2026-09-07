# roguemap

roguemap draws a height-mapped world in a terminal, in isometric
projection, at 24-bit colour. There is no tile atlas and no sprite sheet
for the landscape. Every screen cell casts a ray back into the world and
walks down the continuous height field under it until it meets a surface,
so the camera turns to any angle and the terrain has no seams.

The crate is a library with two binaries. `roguemap` is the game: a world,
a player, a clock, weather and the frames drawn over the scene.
`roguemap-edit` is the asset editor: it opens a directory of TOML tables
and edits them with previews drawn by the game's own renderer.

![island](screenshots/island.png)

The world above is seed 7 at the widest zoom. Ranges, a coastline, forest
and snow all come out of one continuous field; nothing in that picture is
a placed tile.

## The gallery

Every frame here is rendered by `make screenshots`, which drives the
headless snapshot renderer and the Unscii font. What you see is what a
terminal shows with the same font.

| | |
|---|---|
| ![island](screenshots/island.png) The island at 1:8 | ![scale-zclose](screenshots/scale-zclose.png) The yardstick at 1:1: a 2 m person, an 18 m oak, a house |
| ![lsystem-stand-near](screenshots/lsystem-stand-near.png) A boreal stand at 1:2, each spruce grown from its habit | ![lsystem-stand-mid](screenshots/lsystem-stand-mid.png) The same stand at 1:4: tiered spires, each with its own outline |
| ![village](screenshots/village.png) A village on the steppe at 1:4 | ![filled](screenshots/filled.png) The filled world at 1:4 |
| ![night](screenshots/night.png) Night: a campfire and lit windows | ![winter](screenshots/winter.png) Winter after two simulated days of storm |
| ![clouds](screenshots/clouds.png) Above the cloud layer at the widest zoom | ![worldmap](screenshots/worldmap.png) The world map, four tiles per column |
| ![editor](screenshots/editor.png) The asset editor on the `oak` species row | ![lsystem-conifer](screenshots/lsystem-conifer.png) A spruce, side on, from the `excurrent` habit |
| ![lsystem-oak](screenshots/lsystem-oak.png) A gnarled oak in summer | ![lsystem-oak-winter](screenshots/lsystem-oak-winter.png) The same oak bare in winter |

## The doc tree

| Page | What it covers |
|---|---|
| [world.md](world.md) | The height field, relief in metres, rivers, climate, biomes, chunks and the world map |
| [rendering.md](rendering.md) | The ray walk, block and tree geometry, shadows, lighting, antialiasing, level of detail, cost |
| [scale.md](scale.md) | Metres, the four zooms and their ratios, movement, the inset view |
| [trees.md](trees.md) | The growth habits, their parameters, seasons, dead trees, how a species becomes a tree at each zoom |
| [assets-and-editor.md](assets-and-editor.md) | The asset directory, the loader, the editor, how to add a row |
| [frames.md](frames.md) | Frames over the scene: the HUD, settings, world map, inset, panes |
| [weather.md](weather.md) | Wind, clouds, precipitation, accumulation, seasons, day and night |
| [testing.md](testing.md) | What is tested and how the golden frames work |

The catalogues and decision records the pages above lean on:

| Document | What it is |
|---|---|
| [assets.md](assets.md) | The asset directory layout and the art file format |
| [properties.md](properties.md) | Every property a row may carry, with type and default |
| [structures.md](structures.md) | Block geometry, adjacency, faces, shadows, trees as volumes |
| [lsystem.md](lsystem.md) | The L-system grammar, its symbols and the eight habits |
| [ADR-001](adr/ADR-001-assets-as-files.md) | Assets as files rather than Rust constants |
| [ADR-002](adr/ADR-002-block-geometry-structures-and-trees.md) | Structures as block geometry, trees as volumes |
| [ADR-003](adr/ADR-003-asset-editor.md) | The editor as a second binary over a shared library |
| [ADR-004](adr/ADR-004-world-scale.md) | World scale in metres and zoom as exact ratios |
| [ADR-005](adr/ADR-005-overlay-frames.md) | One frame system for everything drawn over the scene |

## Running it

```
make run                  # the game in the current terminal, seed 7, a 32x32 map
make term                 # a Konsole window with the canonical font and size
make build                # cargo build --release
make edit                 # the asset editor on ./assets-export
make check                # clippy as errors, unit tests, asset tests, golden frames
```

`make run` passes `SEED` and `SIZE` through, so `make run SEED=12 SIZE=64`
opens a larger world. The binary takes the same two arguments directly:

```
cargo run --release -- 12 64
```

Anything from 80x25 upward works. The starting zoom is the largest at
which the whole map fits the window.

To look at one frame without opening a terminal, render it headless:

```
make snap OUT=shot.png ARGS="fill=1 zoom=2 tod=21 fire=1"
```

That writes a PNG of a 168x71 frame of the filled world at 1:2, at nine in
the evening, with a campfire at the centre. The full key list is in
[frames.md](frames.md). `make screenshots` runs the whole set into
`docs/screenshots`.

## The canonical look

The snapshot renderer is the reference for how the game should look, not
the terminal. `roguemap --snap` writes a cell dump — one line per cell,
giving a codepoint and a foreground and background colour — and
`tools/cells2png.py` draws it with Unscii 16 at exactly 16 pixels. Every
cell is 8 pixels wide and 16 tall, there is no font smoothing, and the
24-bit colours come straight out of the cell buffer.

A terminal emulator matches that when its font is Unscii at a pixel height
of 16, its line spacing is zero, font antialiasing is off, and bold does
not brighten colours. `./term.sh` opens Konsole configured that way; it
derives the point size from the screen DPI, which is 12 points at 96 DPI,
and asks for 168 columns by 71 rows.

```
./term.sh 7 32
```

Any other emulator with those font settings renders the same pixels. The
PETSCII tileset needs a font covering U+1FB00 to U+1FBFF, the Unicode
Symbols for Legacy Computing block; Unscii 16 Full and Adwaita Mono both
cover it. `make fonts` lists the fonts on the machine that do.

![ascii](screenshots/ascii.png)

The ASCII tileset is the fallback for a font that does not, and `g` swaps
between the two at any time.

## The two binaries

`roguemap [seed] [size]` runs the game. It also has two headless modes:
`--snap W H out.cells key=value...` renders one frame, and
`--export-assets DIR` writes the embedded asset tables out as files.

`roguemap-edit DIR` edits the asset tables in `DIR`, `--export DIR` writes
the embedded set there first, and `--snap W H out.cells key=value...`
renders one editor screen. Both binaries load and validate assets through
the same library code, so the editor's preview is the game's renderer.
