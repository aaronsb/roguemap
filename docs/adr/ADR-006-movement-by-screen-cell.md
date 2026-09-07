# ADR-006: Movement by screen cell at centimetre precision

Amends the movement rule of [ADR-004](ADR-004-world-scale.md). Its units,
zooms and inset view stand.

## Context

ADR-004 put the world in metres, made the smallest discrete unit a
centimetre, and said one keypress moves one tile at every zoom. At 1:1 a
tile is a 16x4 footprint, 11.3 columns and 6 rows per metre, so a press
jumps the figure a whole block: sixteen columns sideways, a leap of two
metres in a view where a person is twelve rows tall. The owner's report:
"we need to likely move at centimeter precision at maximum zoom."

`Entity` holds `mx, my` in tiles and `World::try_move` steps one tile.
Sprites already stand at fractional map positions through the camera's
projection of a point, so nothing in the renderer needs a tile to draw a
figure.

## Decision

**Position.** Every entity's position is centimetres as `i32`, `x_cm,
y_cm`. The tile it stands in is derived: `floor(cm / 200)`, with `mx()`
and `my()` accessors so the rest of the code keeps reading the tile. A
tile is `TILE_CM` = 200 cm, the same 2 m as `TILE_METRES`. The point may
sit anywhere inside its tile; the figure's feet are drawn at the exact
point, at `Map::ground_at`, the smooth field between tile centres (the
bilinear of the four nearest tiles' `hf`, which at a tile's centre is the
tile's own).

**One keypress is one screen cell.** A press moves the figure one screen
cell in the pressed direction at the current zoom — a column for left
and right, a row for up and down — so a press always moves the figure a
visible cell and no more. The figure is drawn at its point floored to a
cell, so the step is aimed at the centre of the next cell: the point is
projected, the centre of the cell one over is unprojected at the same
height, and the difference is rounded to centimetres. From a cell centre
that is the ground under one cell; from the exact cell boundary a spawn,
a teleport or `c` leaves the figure on it is half a cell or one and a
half; and after any press the figure stands within a centimetre of a
cell centre, so no press ever lands on a boundary where float noise
would decide whether it moved. A fixed step cannot do this: one a hair
short misses a cell now and then, one a hair long doubles one, and the
boundary cases are exactly the ones a spawn produces. Shift runs eight
cells, one at a time from where the figure then stands, and stops where
a step is refused. Under the map-axes traversal setting the step is the
cell's ground length along the map axis the key names.

The ground under a cell at each zoom is `TILE_CM / (hw * √2)` for a
column and `TILE_CM / (hh * √2)` for a row, so columns halve exactly
with each zoom in and rows halve wherever the footprint's half height
does (far and mid share a half height of one, so they share a row step):

| zoom | footprint | one column | one row | run of eight columns | map axes, sideways | map axes, up or down |
|---|---|---|---|---|---|---|
| close | 16x4 | 8.8 cm | 35.4 cm | 71 cm | 9 cm | 35 cm |
| near | 8x2 | 17.7 cm | 70.7 cm | 1.4 m | 18 cm | 71 cm |
| mid | 4x1 | 35.4 cm | 141.4 cm | 2.8 m | 35 cm | 141 cm |
| far | 2x0.5 | 70.7 cm | 282.8 cm | 5.7 m | 71 cm | 283 cm |

The far row was recorded as a 2x1 footprint with a 141.4 cm row, the
whole cells it was drawn in; [ADR-009](ADR-009-camera-modes-and-controls.md)
made the far zoom a true halving of the mid zoom and corrected the row,
so rows halve exactly with every zoom in as columns do.

At the compass view a column is a map diagonal, (6.25, -6.25) cm at 1:1,
and a row is (25, 25) cm; at other angles the vector turns with the
camera and its length is the cell's. On sloping ground the feet follow
the field, so a row press up a hill draws a row and the rise on top; the
ground stepped is still one cell's worth.

**Collision stays per tile.** A move is refused when the tile the new
point falls in is off the map or not in the creature's `can_enter`.
There is no sub-tile collision.

**Everything reads the centimetre position.** The camera follows and
centres on the point; the inset looks at it; the status bar and the
stats pane derive the tile from it; the world map's `teleport` lands at
a tile centre; the snapshot's `player=1` spawns at a tile centre and
`player_dx=` / `player_dy=` (centimetres) walk the player from there
through the same `try_move`, so a scene with a stepped figure is
reproducible headless. The spawn is a tile centre, so the golden frames
that show the player do not change.

## Out of scope

Held-key repeat, time-based movement and pathing stay out. Diagonal
keys are not bound; a diagonal press would be one column and one row.

## Consequences

- `Entity` gains `x_cm, y_cm`, `mx()`, `my()`, `pos()`, `at_tile` and
  `set_tile`; `World::try_move` takes a centimetre delta.
- `Camera::cell_step(screen_space, dx, dy, from, z)` replaces `walk_step`
  and returns centimetres; `Camera::anchor_at` anchors a fractional point
  and `look_at_entity` centres on one. `screen_dir_to_map` stays for the
  compass tests.
- The sprite pass draws each entity at its own point at `Map::ground_at`,
  not at its tile's centre; the step and the drawing share that one
  height.
- The help line and README say "run eight" of cells rather than tiles;
  docs/scale.md carries the step table.
- Tests: a press moves the figure's cell by exactly one column or row at
  every zoom and angle, from a boundary, from odd centimetres, at any
  height and through a run of eight; from a cell centre the step is the
  table's ground length, columns halve exactly between zooms and rows
  halve wherever the half height does; a move into water is refused by
  the tile it lands in; teleport lands at a tile centre; a stepped figure
  is reproducible headless; the golden frames pass unchanged.
