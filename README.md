# roguemap

An isometric, height-mapped terrain renderer for the terminal. Terrain is
drawn by inverse projection: every screen cell walks down the height column
under it until it meets a tile top or a cliff face, so the camera can sit at
any angle. Cells on a boundary between two surfaces are supersampled 2x3 and
drawn with a sextant glyph and two colours. Sprites are billboards drawn
back to front with a depth test. A lighting pass then applies ambient sky
light, a sun shadowed by drifting clouds, and point lights such as
campfires. Seasons are
palette swaps on the unlit colour. Glyphs come from a switchable tileset:
plain ASCII, or PETSCII-style shapes from the Unicode Symbols for Legacy
Computing block.

## Run

```
cargo run --release -- [seed] [size]
```

Defaults are seed 7 and a 32x32 map. The starting zoom is the largest at
which the whole map fits the window; larger maps or smaller windows pan.
Anything from 80x25 upward works.

## Keys

| Key | Action |
|---|---|
| `Tab` or `o` | open the settings window |
| `m` | open the world map; arrows move the cursor, `z` changes extent, `Enter` teleports |
| `w a s d` or `h j k l` | walk the player; the camera follows |
| `v` | toggle island / filled world view |
| arrows | pan |
| `c` | centre on the player |
| `r` / `R` | rotate a quarter turn about the screen centre |
| `(` / `)` | rotate five degrees |
| `z` / `Z` | zoom in / out through seven tile sizes, 2x1 to 16x4 cells |
| `g` | toggle PETSCII / ASCII glyphs |
| `[` / `]` | step the season by a quarter |
| `,` / `.` | step the clock by an hour |
| `p` | pause or resume the clock (a day is two minutes) |
| `W` | cycle weather: clear, rain, snow |
| `f` | light a campfire at the screen centre |
| `F` | put out all fires |
| `H` | toggle the HUD |
| `q` | quit |

## Settings

`Tab` opens a modal window over the scene. Up and down pick a row, left and
right change it, `Esc` closes. Every row also has a shortcut key.

| Setting | Values |
|---|---|
| Traversal | screen space (a key moves the figure that way on screen) or map axes |
| World view | island (a bounded map floating in the sky) or filled (terrain in every direction) |
| Glyphs | petscii or ascii |
| HUD | shown or hidden |
| Clock | running or paused |
| Weather | auto, clear, cloudy, rain, storm |
| Wind | auto, calm, breeze, windy, gale |
| Day length | 2 minutes, 10 minutes, 1 hour, 24 hours |
| Cloud layer | shown or hidden |
| Antialias | on or off (PETSCII glyphs only) |

The settings table in `src/settings.rs` is the single source for the window
and for the shortcuts.

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

## Level of detail

Every zoom level carries its own sprite tier. Trees are generated
procedurally per tier: a single glyph at 2x1, a small pine at 4x1, layered
conifers and broadleaf canopies twenty-odd cells wide at 16x4. The player is
`@` at the overview and a nine-row figure at the largest zoom.

## Headless snapshots

```
roguemap --snap WIDTH HEIGHT out.cells key=value...
```

Keys: `seed`, `size`, `t`, `tod`, `season`, `weather`, `glyphs`, `rot`,
`zoom`, `fire`, `hud`. The output lists one cell per line as codepoint and
foreground and background colour; `tools/cells2png.py` renders it with the
Unscii font.

## Fonts

The PETSCII tileset needs a font covering U+1FB00 to U+1FBFF. Unscii 16 Full
and Adwaita Mono both do; Konsole falls back to them for missing glyphs.
