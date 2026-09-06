# roguemap

An isometric, height-mapped terrain renderer for the terminal. Tiles are
rasterised back to front into a G-buffer of unlit colour, glyph, world
position and face; a lighting pass then applies ambient sky light, a sun
shadowed by drifting clouds, and point lights such as campfires. Seasons are
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
| `w a s d` or `h j k l` | walk the player; the camera follows |
| arrows | pan |
| `c` | centre on the player |
| `r` / `R` | rotate a quarter turn about the screen centre |
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
