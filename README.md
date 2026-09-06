# roguemap

An isometric, height-mapped terrain renderer for the terminal. Terrain is
drawn by inverse projection: every screen cell walks down the continuous
height field under it, testing the buildings and trees in view on the way,
until it meets a surface, so the camera can sit at any angle. Buildings are
block geometry (a kind and a level count per tile, merging with same-kind
neighbours under one roof) and trees are volumes (cones, ellipsoids, domes,
cacti) on the same walk; creatures and small props are billboards drawn
back to front with a depth test. Cells on a boundary between two surfaces
are supersampled 2x3 and drawn with a sextant glyph and two colours. A
lighting pass then applies ambient sky light, a sun shadowed by drifting
clouds and by the shadows the terrain, buildings and crowns cast, and point
lights such as campfires. Seasons are
palette swaps on the unlit colour. Glyphs come from a switchable tileset:
plain ASCII, or PETSCII-style shapes from the Unicode Symbols for Legacy
Computing block.

## Screenshots

Rendered by `make screenshots` with the canonical snapshot pipeline.

| | |
|---|---|
| ![island](docs/screenshots/island.png) The island view | ![rotated](docs/screenshots/rotated.png) Rotated 25 degrees off compass |
| ![filled](docs/screenshots/filled.png) Filled world at 4x1 | ![closeup](docs/screenshots/closeup.png) Encounter zoom at 16x4 |
| ![scale-zclose](docs/screenshots/scale-zclose.png) The yardstick at 1:1: a 2 m person, an 18 m oak, a house | ![scale-zfar](docs/screenshots/scale-zfar.png) The same ground at 1:8 |
| ![village](docs/screenshots/village.png) A village on the steppe at 1:4 | ![scale-zmid](docs/screenshots/scale-zmid.png) The yardstick at 1:4 |
| ![night](docs/screenshots/night.png) Night, campfire and lit windows | ![winter](docs/screenshots/winter.png) Winter after two days of storm |
| ![clouds](docs/screenshots/clouds.png) Above the clouds at the smallest zoom | ![steppe](docs/screenshots/steppe.png) Steppe with adobe houses |
| ![worldmap](docs/screenshots/worldmap.png) World map, large extent | ![ascii](docs/screenshots/ascii.png) The ASCII glyph set |

## Run

```
cargo run --release -- [seed] [size]
```

Defaults are seed 7 and a 32x32 map. The starting zoom is the largest at
which the whole map fits the window; larger maps or smaller windows pan.
Anything from 80x25 upward works.

## Keys

The table follows the binding table in `src/input.rs`, which also generates
the help line at the bottom of the screen.

| Key | Action |
|---|---|
| `Tab` or `o` | the settings window |
| `m` | the world map |
| `w a s d` or `h j k l` | walk the player; the camera follows |
| arrows | pan by one tile |
| `c` | centre on the player |
| `r` / `R` | rotate a quarter turn about the screen centre |
| `(` / `)` | rotate five degrees |
| `z` / `Z` | zoom in / out through the four scales: far 2x1 (1:8), mid 4x1 (1:4), near 8x2 (1:2), close 16x4 (1:1) |
| `v` | toggle island / filled world view |
| `g` | toggle PETSCII / ASCII glyphs |
| `[` / `]` | step the season by a quarter |
| `,` / `.` | step the clock by an hour |
| `p` | pause or resume the clock (a day is ten minutes by default) |
| `W` | cycle weather: auto, clear, cloudy, rain, storm |
| `f` | light a campfire at the screen centre |
| `F` | put out all fires |
| `H` | toggle the HUD |
| `i` | the inventory |
| `I` | the stats pane |
| `L` | the history log |
| `C` | the conversation prompt |
| `n` | the inset view: the same scene at the other end of the zoom scale |
| `q` or `Esc` | quit |

`Tab`, `m`, `i`, `I`, `L`, `C` and `n` each toggle a frame (see below): the
same key opens and closes it, and `Esc` closes the focused frame and gives the
keys back to the scene.

`Ctrl-C` quits from any mode. In the world map, arrows or `w a s d` /
`h j k l` move the cursor, `z` / `Z` change the extent, `Enter` or `t`
teleports, and `m`, `Esc` or `q` close it. In the settings window, up and
down (or `k` / `j`) pick a row, left and right (or `h` / `l`, `Enter`,
space) change it, and `Esc`, `Tab` or `q` close it. In a list or text
frame, up and down (or `k` / `j`) scroll and select, and `Esc` closes;
in the conversation, typing goes to the prompt and `Enter` says it.

## Frames

Everything drawn over the scene is a frame: a rectangle with an anchor, a
size, a border, a background, a draw order, a priority and a rule for when
it shows. The rows live in `assets/ui.toml` and the content kinds live in
`src/ui.rs` ([ADR-005](docs/adr/ADR-005-overlay-frames.md)). At most one
frame is focused; it takes the keys first and draws last, and `Esc`
returns focus to the scene. Where two frames collide the higher priority
wins, so a small screen keeps the panes that matter; a frame whose minimum
size does not fit is dropped.

| Frame | Key | Where | What |
|---|---|---|---|
| hud-top | | top row | status: heading, the zoom as its ratio and name, season, clock, weather, glyph set, lights, the tile under the player |
| hud-help | | bottom row | the generated key help |
| settings | `Tab` or `o` | centre | the settings table, sized to itself |
| worldmap | `m` | full screen | biomes plotted top-down, with a teleport cursor |
| inset | `n` | a corner | the second view of [ADR-004](docs/adr/ADR-004-world-scale.md): the same scene at the other end of the zoom scale, following the player; hidden below 100 columns |
| inventory | `i` | left | what the character carries (empty for now) |
| stats | `I` | right | position, biome, temperature, height, time, weather |
| history | `L` | centre | the last fifty events: moves, campfires, teleports, weather changes, what was said |
| conversation | `C` | bottom | wrapped text with a prompt; what is said is echoed and logged |

`ui.toml` names each frame's toggle key and `src/input.rs` binds it to
`Toggle(name)`; a test keeps the two in step. `roguemap --snap` takes
`open=name,name` to render frames headless.

The inset is a second camera and renderer over the same scene, following
the player at the other end of the zoom scale: while the main view is
zoomed out at all — 1:2, 1:4 or 1:8 — the inset shows 1:1, and at 1:1 it
shows 1:8, so the two views never share a level. It draws the scene alone,
with no HUD over it and with antialiasing and the cloud layer off, and the
ratio it is drawing at is in its title. It takes a quarter of the screen
width and about a third of its height, needs a hundred columns to show at
all, and ranks below the bars, so it covers the tail of the line it sits
on rather than taking that line away. The `Inset` settings row picks its
corner or turns it off, and `n` flips between the two.

## Settings

`Tab` opens a modal window over the scene. Up and down pick a row, left and
right change it, `Esc` closes. Every row also has a shortcut key.

| Setting | Values |
|---|---|
| Traversal | screen space (a key moves the figure that way on screen) or map axes |
| World view | island (a bounded map floating in the sky) or filled (terrain in every direction) |
| Glyphs | petscii or ascii |
| HUD | shown or hidden |
| Inset | off, bottom-right (default), bottom-left, top-right, top-left |
| Clock | running or paused |
| Weather | auto, clear, cloudy, rain, storm |
| Wind | auto, calm, breeze, windy, gale |
| Day length | 2 minutes, 10 minutes (default), 1 hour, 24 hours |
| Cloud layer | shown or hidden |
| Antialias | on or off (PETSCII glyphs only) |

The settings table in `assets/settings.toml` drives the window; each row
names its shortcut key, and the scene binding table in `src/input.rs`
mirrors it (a test keeps the two in step).

## Assets

Everything that describes a thing in the world (biomes, species, materials,
props, building kinds, creatures, surfaces, lights, settings rows, overlay
frames, glyph sets and hand-drawn sprites) is a TOML or text file under
`assets/`,
embedded into the binary at build time. Set `ROGUEMAP_ASSETS=<dir>` to load
the same layout from disk instead, with no rebuild; a missing directory or a
bad row is an error naming the file and row. `make assets-export` writes
the embedded set to `./assets-export` as a starting point. The layout and
the art format are in [docs/assets.md](docs/assets.md), every row property
in [docs/properties.md](docs/properties.md), and the decision in
[ADR-001](docs/adr/ADR-001-assets-as-files.md). The crate is a library plus
two binaries: `roguemap` (the game) and `roguemap-edit` (the editor below).

## Editor

```
roguemap-edit DIR              edit the asset tables in DIR
roguemap-edit --export DIR     write the embedded set to DIR, then edit it
make edit                      the same on ./assets-export
```

The editor ([ADR-003](docs/adr/ADR-003-asset-editor.md)) edits a directory
of assets with previews drawn by the game's own renderer. With no
directory it uses `ROGUEMAP_ASSETS`; with neither it exits with a message,
since the embedded set has nowhere to be written back to.

![editor](docs/screenshots/editor.png)

The left pane lists the tables and the current table's rows, with art
files under the row that names them. Across the top, one preview pane per
sprite tier (tiny 2x1, small 4x1, medium 8x2, large 16x4) draws a flat
fixture in the chosen biome and season with the selected row placed at
its centre: a tree, a block pattern, four of a prop, a creature, a lit
light at night, a house in a material. Below is the row form, or the art
grid, then a status line. Below 120x50 the strip shows one tier (`t`
cycles) and the panes shrink to fit; 80x25 is the floor, as for the game.

Saving validates the whole set exactly as the game loads it. An error
names the file and row, jumps there, and nothing is written. Files are
written atomically, row order and a leading comment block are kept, and
the directory is reloaded afterwards so the preview shows what the game
will load.

| Key | Normal mode |
|---|---|
| `Tab` / `Shift-Tab` | cycle the panes: tables, rows, form |
| arrows or `h j k l` | move; left and right switch table, or cycle a choice field |
| `Enter` | edit the field under the cursor; on an art entry, open the grid |
| `n` / `d` | add a row (a copy named `-copy`) / delete the row, after `y` |
| `x` | clear an optional field so its default applies |
| `u` / `U` | undo / redo |
| `s` / `S` | save the current table's file / every changed file |
| `b` / `B` | cycle the preview biome |
| `[` / `]` and `,` / `.` | step the season and the hour, as in the game |
| `g`, `r` / `R`, `W` | glyph set, rotate a quarter turn, weather preset, as in the game |
| `t`, `P`, `a` | preview tier (small screens), block pattern or tree variant, animate |
| `q` | quit, asking once if anything is unsaved |

Field mode: text and numbers get a line editor (`Left`, `Right`, `Home`,
`End`, `Backspace`, `Delete`); enum, reference and boolean fields cycle
with `Left` / `Right`; colours pick a channel with `Left` / `Right` and
step it with `Up` / `Down` (1) or `PgUp` / `PgDn` (16), four groups for a
seasonal colour; terrain and cover lists are checklists toggled with
`Space`; other lists are typed as TOML or comma-separated names. `Enter`
commits (a value that fails its check stays open with the reason; a
reference to a missing row is kept with a warning and refused at save),
`Esc` cancels.

Grid mode: arrows move, a printable key places itself and advances,
`Space` clears, `i` / `X` insert or delete a row, `>` / `<` widen or
narrow, `c` sets `center` to the cursor column, `B` sets `base_rows` from
the cursor row down, `p` opens a glyph picker (the tileset's roles and the
Symbols for Legacy Computing block), `Tab` undoes, `Esc` returns. The art
header (`name`, `tier`, `center`, `base_rows`, `min_zoom`) is edited in
the form. `Ctrl-C` quits from any mode.

Headless: `roguemap-edit --snap W H out.cells key=value...` renders one
editor screen with `table`, `row`, `biome`, `season`, `tod`, `glyphs`,
`tier`, `pattern`, `deg`, `pane` and `grid=1`; `make edit-snap
OUT=editor.png ARGS="table=props row=campfire grid=1"` turns it into a PNG.

## World

Terrain is a pure function of position and seed, generated on demand in
32x32 chunks. A slow continental field sets oceans, plains and ranges over
hundreds of tiles; local noise adds hills; rivers follow the mid contour of
another slow field. The island view bounds it to the map size; the filled
view lets it run in every direction.

Climate is temperature (a latitude-like field minus a lapse rate with
height) and precipitation. A simplified Köppen scheme in `src/biome.rs` maps
the pair to a biome, and the biome table names ground colour, tree density,
weighted species and building material. Going uphill in one region passes
from broadleaf forest to conifers to scrub to snow.

## Weather

Cloud cover, wind speed and direction, and precipitation drift along slow
noise on the day clock, or follow the presets in settings. Wind moves the
clouds, stirs trees through a gust field, drives the grass and raises
whitecaps. Precipitation accumulates per temperature band as snowpack or
wetness and melts or dries with warmth and sun.

At the two smallest zooms the view is above the clouds. Each screen cell
samples the cloud field where a ray from a virtual camera meets the cloud
plane, so panning moves clouds faster than the ground by C/(C - H), and the
shadow of each cloud lies along the sun direction by the altitude over the
tangent of the sun's elevation. Precipitation is drawn beneath the layer.

## World map

`m` plots biomes top-down at three extents (1, 4 or 16 tiles per column).
The header names the biome, temperature and height under the cursor; `Enter`
teleports the player there.

## Structures and trees

Buildings, roads, fields and walls are block geometry: `assets/blocks.toml`
names each kind's size, levels, roof profile (flat, gable or hip), material
rule, faces and light, and a tile carries a stack of one kind. Same-kind
neighbours merge into one building whose roof continues across the seam
and whose gable rides the longer run; walls get window rows and a door on
an open face, and lit windows are lights at night. The generator lays
whole buildings: the world is cut into settlement plots of sixteen tiles,
each holding at most one house of three by two to five by three tiles, a
2x2 tower, a barn or a field, with a tile of margin so two never merge,
and a clearing of trees around it. Trees are volumes from
`assets/species.toml`: a size in metres and a canopy shape, tested by the
ray walk so crowns occlude correctly at any angle. They stand apart by
their crowns — three quarters of a crown's width, times the biome's own
spacing factor — and every tree takes its own height, crown and canopy
colour from its tile's seed, with a few standing dead, so a stand reads as
trees rather than one mass. Terrain steps, buildings and crowns cast shadows along the sun's
direction, the one the cloud shadows use. The note is
[docs/structures.md](docs/structures.md); the decision
[ADR-002](docs/adr/ADR-002-block-geometry-structures-and-trees.md). Sizes
in every table are metres, with a 2 m tile.

## Scale

The world is metres (ADR-004). A tile is 2 m square, the person 2 m tall,
a house level 3 m, an oak 18 m; terrain runs from a sea bed 12 m down to
120 m at the top of a range, and a tile's gameplay height is that field
floored to whole metres. Each zoom is an exact halving of the next, and
its footprint gives both scales:

| zoom | footprint | ratio | columns per metre | rows per metre | a 2 m person |
|---|---|---|---|---|---|
| close | 16x4 | 1:1 | 11.3 | 6 | 12 rows |
| near | 8x2 | 1:2 | 5.7 | 3 | 6 rows |
| mid | 4x1 | 1:4 | 2.8 | 1.5 | 3 rows |
| far | 2x1 | 1:8 | 1.4 | 0.75 | one glyph |

Heights project through rows per metre, so an 18 m oak stands 108 rows at
1:1 with the person under its canopy, and 13 rows at 1:8. The inset view
always shows the other end of that scale (see Frames). One keypress
moves one tile at every zoom — a whole block at 1:1, an eighth of one at
1:8 — and shift with an arrow strides eight tiles.

## Level of detail

Detail keys off rows per metre. A tree is a single glyph at the overview
and a volume from 1:4 up, with the set's outline glyphs and normal
shading; buildings are flat columns at the overview and gain roof
profiles, then windows and doors, as the zoom grows. Sprites pick the art
tier whose rows are nearest what the thing stands at that zoom, and stand
with their feet on the ground: the player is `@` at the overview and a
twelve-row figure at 1:1.

## Headless snapshots

```
roguemap --snap WIDTH HEIGHT out.cells key=value...
```

Keys: `seed`, `size`, `fill` (1 for the filled world), `cx`, `cy` (tile to
centre on), `zoom`, `rot` (quarter turns), `deg` (degrees), `t` (animation
time), `tod`, `season`, `cover`, `wind`, `precip` (0..1), `simdays` (run a
storm that many days first), `glyphs` (petscii or ascii), `player` (1),
`fire` (1 for a campfire at the centre), `hud` (0 or 1), `inset` (0 for
none, 1 to 4 for the corner), `popover` (1),
`worldmap` (1) with `scale`, `scene` (`scale` for the yardstick frame: a
person between an oak and a house on flat ground), and `frames` (N, to time
rendering). The output
lists one cell per line as codepoint and foreground and background colour;
`tools/cells2png.py` renders it with the Unscii font.

## Canonical look

The snapshot renderer is the reference for how the game should look.
`tools/cells2png.py` draws the cell dump with Unscii 16 at exactly 16 pixels,
so every cell is 8x16 pixels with no smoothing and the 24-bit colours come
straight from the cell buffer. `make screenshots` and `make snap` produce
that look.

A terminal emulator matches it when its font is Unscii at a pixel height of
16, its line spacing is zero, font antialiasing is off, and bold does not
brighten colours. `./term.sh` opens Konsole configured that way, deriving
the point size from the screen DPI (12 points at 96 DPI). Any other emulator
with the same font settings will render the same pixels.

## Tests

`make check` is the gate: clippy with warnings as errors, the unit tests
beside the code, the asset loader tests and the golden frames. The plan is
in [docs/testing.md](docs/testing.md).

```
make check          # everything
make test           # cargo test --release
make golden-check   # render the golden frames in-process and score them
make golden-record  # accept the current frames as the new reference
```

The golden frames are the eleven headless snapshots `tools/golden.sh`
renders, drawn in-process by `tests/golden.rs` and compared cell by cell
with the references committed under `tests/golden/*.frame`. Every run
prints each frame's score: the percentage of identical cells, the
percentage with the same glyph, and the mean colour distance. A frame
passes at 98 percent identical and a mean distance of 2.0 or less;
`GOLDEN_MIN_IDENTICAL` and `GOLDEN_MAX_DISTANCE` move those thresholds
and `GOLDEN_STRICT=1` demands every cell identical. A failure lists the
first ten differing cells with their expected and actual glyph and
colours, so a shifted sprite reads differently from a colour tweak.
`GOLDEN_DUMP=<dir>` also writes the rendered frames as `.cells` text dumps.

After an intentional visual change, `make golden-record` rewrites the
reference frames; commit them in the same change and say which frames
moved and why. `make golden` and `make golden-bytes` keep the older
byte-for-byte workflow through the binary: record cell dumps in `.golden`
before a refactor, compare after.

## Fonts

The PETSCII tileset needs a font covering U+1FB00 to U+1FBFF. Unscii 16 Full
and Adwaita Mono both do; Konsole falls back to them for missing glyphs.
