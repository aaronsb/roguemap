# Frames

Everything drawn over the scene is a frame, and so is every pane of the
asset editor. A frame is a row in `assets/ui.toml` — or, for the editor,
`assets/editor-ui.toml` — saying where it goes, how it is framed and when
it shows, plus a content in `src/ui.rs` or `src/editor/ui.rs` that draws
into it and, when focused, takes the keys. Adding a pane means adding a
row and a content kind, not a draw call. The decision is
[ADR-005](adr/ADR-005-overlay-frames.md); this page is the system as it
stands.

![settings](screenshots/settings.png)

The settings frame over the island view. `make snap OUT=shot.png
ARGS="popover=1 zoom=0"` renders it.

## A row

```toml
[[frame]]
name = "stats"
description = "Where the character stands and what the world is doing there."
title = "stats"
content = "stats"
anchor = "right"
border = "line"
z = 50
priority = 65
key = "I"
show = "on_key"
background = "opaque"
size = { cols = 0.22, rows = 0.6, min_cols = 24, min_rows = 8 }
```

| Field | Values |
|---|---|
| `name` | unique; the key for settings and the toggle action |
| `description` | required on every row |
| `title` | drawn in the top border; blank for none |
| `content` | a kind the code supplies; the frame's name by default |
| `anchor` | `top_left top top_right left centre right bottom_left bottom bottom_right full` |
| `size` | `cols` and `rows` each a whole number of cells, a fraction of the screen, `"fill"` or `"auto"`; `min_cols` and `min_rows` drop the frame when it will not fit |
| `margin` | `left`, `top`, `right`, `bottom`: cells kept clear at the screen's edges before the anchor places it; zero everywhere unless a row says otherwise |
| `border` | `none`, `line`, `double`, `chrome`, `top_right` |
| `background` | `none`, `opaque`, or `{ tint = 0.0..1.0 }` |
| `z` | draw order, lowest first |
| `priority` | which frame survives when two overlap |
| `show` | `always`, `zoomed_out`, `zoomed_in`, `on_key`, `never`, `{ min_columns = N }`, or `{ min_size = { columns = N, rows = N } }` |
| `key` | the toggle key, bound to `Toggle(name)` in `src/input.rs` |

The loader refuses a row whose content is not a kind the code supplies or
whose key it cannot parse. A test keeps `ui.toml`'s keys and the binding
table in step in both directions, and another keeps `editor-ui.toml`'s
content kinds and `src/editor/ui.rs` in step, also in both directions.

`top_right` is the tiled pane's border: a line along the top, which carries
the title, and one down the right, leaving the other two edges to the
neighbours that draw them. With margins it lets a screen of panes meet
without sharing a cell or doubling a line, which is what the editor's
screen is.

## Layout

`Layout::resolve` runs on every frame, which is cheap for a list of ten
rectangles and needs no resize hook.

Each frame is dropped if it is closed or its show rule fails. It is then
sized and placed inside the screen less its margins, which is the whole
screen for every frame of the game. Its border costs a cell on each side
it draws, and that comes out of an `auto` size rather than being added to
it, so a content that asks for a 48 by 13 interior gets a 50 by 15 frame
with a line border and a 49 by 14 one with a `top_right` border. Each axis
then resolves to cells, a fraction of the area, the whole axis, or what
the content asked for, and is clamped to the area and to the minimum. A
frame smaller than its minimum is dropped.

The anchor places the rectangle; `centre` and `full` both centre it.
Overlaps then resolve by priority: a frame is hidden only when a frame of
*strictly greater* priority, with an opaque background, overlaps it. Equal
priority never drops either side, and a frame with no background or a tint
lets what is under it through, so the HUD bars survive a translucent pane.

What survives draws in `z` order, with the focused frame moved to the end.

## Focus

At most one frame is focused. Opening a focusable frame focuses it; the
HUD bars and the inset are passive and never take focus.

A focused frame gets the keys first, through its own handler and then
through the shared frame binding table: up and down scroll, `Esc` closes.
A content that does not want a key passes it on, and `Esc` on a passed key
closes the frame and hands the keys back to the scene. The conversation
frame's prompt eats printable characters before any of that, which is why
typing in it does not walk the player.

## The frames

| Frame | Key | Where | What |
|---|---|---|---|
| `hud-top` | | top row, always | the status line |
| `hud-help` | | bottom row, always | the generated key help |
| `settings` | `Tab` or `o` | centre, sized to itself | the settings table |
| `worldmap` | `m` | full screen | biomes plotted top-down with a teleport cursor |
| `inset` | `n` | a corner, above 100 columns | the second view at the other end of the zoom scale |
| `inventory` | `i` | left | what the character carries |
| `stats` | `I` | right | position, biome, temperature, height, time, weather |
| `history` | `L` | centre | the last fifty events |
| `conversation` | `C` | bottom | wrapped text with a prompt |

`roguemap --snap` takes `open=name,name` to render any of them headless,
`camera=chase` (or `shoulder`, `first-person`) for the perspective modes
with `pitch=` and `fov=` in degrees, `fog=` metres of visibility (`0` for
no fade), `fogmode=always` for the fog row, and `px=`, `py=` for the tile
the character stands on, which a perspective view otherwise puts at the
view centre.

## The HUD lines

The top bar is one line:

```
 roguemap  45deg  1:1 close  summer (1.00)  13:00  cloud 30% wind 20% precip 0%  glyphs:petscii  lights:3  temperate forest (Cf) 11C z14
```

It names the camera heading in degrees, the zoom as its ratio and its
name — or, in a perspective mode, the mode with its distance, field of
view and pitch, `chase 12m fov 60 pitch 30` — the season by name and as a
number, the clock with `(paused)` when
the clock is stopped, the cloud cover, wind and precipitation as
percentages, the glyph set, the light count — placed lights plus the ones
this frame discovered, such as lit windows — and the tile under the
player with its biome, Köppen code, temperature and height.

The bottom bar is `input::help_line(SCENE, "  ")`, generated from the
binding table, so a new binding with a label appears there without anyone
writing it out. It starts:

```
 tab settings  m world map  wasd/hjkl walk  arrows pan  shift+arrows run  c centre  r/R ( ) rotate  z/Z zoom  v fill  g 
```

`H` hides both bars together.

## Settings

`Tab` or `o` opens the settings frame. Up and down pick a row, left and
right change it, `Esc`, `Tab` or `q` close it. Every row also has a
shortcut that works from the scene, and a test keeps `settings.toml`'s
shortcut and the binding table in step.

| Setting | Values | Shortcut |
|---|---|---|
| Traversal | screen space, map axes | |
| Camera | isometric, chase, shoulder, first-person | |
| Field of view | preset, 30, 40, 50, 60, 70, 80, 90, 100, 110 | `<` `>` |
| Fog | perspective, always, never | |
| World view | island, filled | `v` |
| Glyphs | petscii, ascii | `g` |
| HUD | shown, hidden | `H` |
| Inset | off, bottom-right, bottom-left, top-right, top-left | |
| Clock | running, paused | `p` |
| Weather | auto, clear, cloudy, rain, storm | `W` |
| Wind | auto, calm, breeze, windy, gale | |
| Day length | 2 min, 10 min, 1 hour, 24 hours | |
| Cloud layer | shown, hidden | |
| Antialias | on, off | |

`Settings::apply` pushes each row into the object it governs: the world
view sets whether the map is bounded, the clock sets `auto_time`, weather
and wind set their presets, day length sets the seconds in a day, the
camera row puts the camera in its mode and the field of view row
overrides the mode's own field of view (the preset's is 60 degrees for
chase and first-person and 40 for shoulder; the isometric mode has none
and ignores the row), and antialias, cloud layer and fog become the
renderer's options. `<` and `>` step the field of view row in tens of
degrees from wherever the camera stands and never wrap it back to
`preset`; `{` and `}` pitch a perspective view by five degrees, which is
the camera's and not a row.

## The world map

![worldmap](screenshots/worldmap.png)

The world map is full screen and the highest priority frame, so it hides
everything else, and the scene is not rendered at all behind it. It plots
biomes top-down at three extents, with a header naming what is under the
cursor and a legend of every biome. The details are in
[world.md](world.md).

## The inset

The inset owns a second `Camera` and `Renderer` and its own canvas,
rebuilt whenever its interior changes size. It copies the main camera's
angle, sets its zoom by the bias rule of [scale.md](scale.md), looks at
the player, renders the same `Scene` with antialiasing and clouds off, and
blits the result. Its title is the row's title plus the ratio it is
drawing at, so it reads `inset 1:1`.

It follows the player, not the camera it hangs off: panning the main view
leaves it where it was. It is not focusable. The `Inset` settings row
places it in any of the four corners or turns it off, and `n` flips
between the last corner and off. Its priority of 15 is below the HUD
bars', so it covers the tail of the line it sits on rather than taking
that line away, and it yields to any pane opened over it.

## The panes

**Inventory** is a list with the hint `carrying nothing`. Nothing pushes
into it yet.

**Stats** is a live list, rebuilt every frame: position, then the biome
with its Köppen code, the temperature and the height of the tile under the
player, then the time and season, then the cloud, wind and precipitation
percentages.

**History** is a ring of the last fifty events, each with the time it
happened: walking to a tile, lighting a campfire, putting the fires out,
teleporting, setting the weather, and what was said.

**Conversation** is wrapped text with a prompt line. Typing goes to the
prompt, `Enter` says the line, and the line is echoed into the body and
logged in the history.

A list draws each item as a title row and, where there is one, an indented
detail row; the selected title is inverted and the window keeps the cursor
centred.

## The editor's panes

![editor](screenshots/editor.png)

The editor's screen is eight rows of `assets/editor-ui.toml` over the
content kinds `src/editor/ui.rs` supplies: `tables`, `rows`, `strip`,
`pane`, `form`, `grid`, `picker` and `status`. They draw from the `Editor`
rather than from the scene, so they are `Pane<Editor>` rather than
`Content`: `frame.rs` places them, draws their chrome and reads their
titles, and the editor keeps its own focus and key handling.

The screen tiles rather than floats. The lists hold a twenty-cell left
column, the preview and the form take what is right of it with a left
margin, the lists and the form leave the last row to the status line with
a bottom margin, and every pane has a `top_right` border, so the panes
meet exactly and cover the screen between them. Nothing in the code says
where a pane goes.

The 80 by 25 collapse is two rows and their rules. `strip` draws the four
sprite tiers side by side and shows only on a screen of at least 120 by
45 — the tallest tier with a form under it. `pane` draws the one tier `t`
picked and is always shown, but ranks below `strip` and is opaque, so
wherever the strip fits the pane is hidden by priority and wherever it
does not the pane is all there is. That one pane drops to a tier whose
subject stands whole in it, which is the rule
[assets-and-editor.md](assets-and-editor.md) describes.

Which of `form`, `grid` and `picker` holds the bottom right is the
editor's mode, through `Pane::open`. They share a rectangle, so only one
is ever open.
