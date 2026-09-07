# ADR-009: The tilt of the table, and who turns with the view

Amends [ADR-007](ADR-007-general-camera.md)'s account of the isometric
pitch and splits its `camera` setting in two. [ADR-004](ADR-004-world-scale.md)'s
four ratios stand and one of its footprints is corrected;
[ADR-006](ADR-006-movement-by-screen-cell.md)'s step table gains a
corrected row; [ADR-008](ADR-008-walking.md)'s walk, held-key set, facing
and mouse look stand.

## Context

The owner: "I think the camera controls are all over the map. we started
with isometric view and fixed camera, then later added floating camera
etc. I think the biggest issue is the engine cant decide if it's a
roguelike turn based move situation, or a fps pov situation." And the
shape of the fix: "the zoom in isometric isn't a clean stepping now, it
has different angles. so let's just make it so in isometric mode, we can
tilt the whole table from say, 45 degrees to 90 degrees. (true iso, to
straight down). straight down is essentially the most honest 'roguelike'
— and we get to keep one of my favorite effects, the cloud parallax
effect." Then: "in perspective mode, let's have uncoupled and coupled
modes. Mouse and arrowkeys in freelook, or fps view. Let's just adopt
minecraft player movements... When in isometric, we still use the same
movement approach."

**What the four zooms actually draw.** `Camera::preset` builds the basis
`{ cols: hw sqrt 2, rows: hh sqrt 2, rise: 3 hw / 8 }` from
`ZOOMS = [(2,1), (4,1), (8,2), (16,4)]`, in columns and rows. A cell is
not square: the canonical font is Unscii 16 at 8 by 16 pixels, so a
column is half a row. In pixels per metre, with `TILE_METRES` = 2, a view
draws a metre of ground across the screen as `(cols / 2) * 8`, a metre of
ground depth toward the camera as `(rows / 2) * 16`, and a metre of
height as `rise * 16`. A true orthographic camera at ground-plane angle
theta with a uniform scale S pixels per metre draws those three as S,
`S sin(theta)` and `S cos(theta)`.

| zoom | preset | across | ground depth | sin(theta) | theta | height | true height | exaggeration |
|---|---|---|---|---|---|---|---|---|
| far | 2x1 | 11.314 | 11.314 | 1 | 90 | 12 | 0 | — |
| mid | 4x1 | 22.627 | 11.314 | 0.5 | 30 | 24 | 19.596 | 1.2247 |
| near | 8x2 | 45.255 | 22.627 | 0.5 | 30 | 48 | 39.192 | 1.2247 |
| close | 16x4 | 90.510 | 45.255 | 0.5 | 30 | 96 | 78.384 | 1.2247 |

The sine falls out of the preset as `2 hh / hw`, so the three 4:1
footprints are one angle and the far zoom's 2x1 is another. The
exaggeration is exact:

```
height / (across cos 30)  =  6 hw / (4 sqrt 2 hw * sqrt 3 / 2)
                          =  6 / (2 sqrt 6)  =  3 / sqrt 6  =  sqrt 1.5
```

So three of the four zooms are a 30 degree ground plane with height
stretched by `sqrt 1.5`, and the far zoom is already a plan view — the
ground square on the screen, at 90 degrees — with height drawn straight
up it at 1.06 times the ground scale, a cavalier oblique. That is what
"different angles" is: the far zoom is a different projection, and the
other three share one.

The number ADR-007 wrote down, `atan(4 sqrt 2 hh / (3 hw))` — 25.24
degrees for 4:1 and 43.31 for 2:1 — is the angle a reader backs out by
taking the height exaggeration for foreshortening:
`atan(tan 30 / sqrt 1.5)` = 25.24. The ground on screen has never been at
25.24 degrees. The floor of the tilt range below is today's look, and
today's look is 30. (The owner chose "today's look is the floor" from a
menu that labelled it 25 degrees; the label was wrong and the frames it
described are the same frames.)

**The modes answer two questions at once.** ADR-007's `camera` row runs
`isometric | chase | shoulder | first-person`, which is where the eye
sits. Whether the mouse turns the character with the view is a second
question, and the row has no room for it: the walk keys are read against
the camera's yaw every tick, so today the body always turns with the
view, and the values that would let it not do so have nowhere to live.

## Decision

**The isometric view has a tilt and a relief.** `Camera` gains `tilt`,
the angle of the ground plane on screen, from 30 degrees to 90, and
`relief`, the factor by which height is drawn taller than the tilt
implies, defaulting to `sqrt 1.5`. The basis is those two and the scale,
in floats:

```
cols = scale.columns * TILE_METRES
rows = cols * CELL_ASPECT * sin(tilt)
rise = relief * scale.columns * CELL_ASPECT * cos(tilt)
```

`CELL_ASPECT` is 0.5, the canonical cell's 8 pixels over 16. At
`tilt = 30` and `relief = sqrt 1.5` these give the preset's own numbers
bit for bit at 1:4, 1:2 and 1:1: `sin(30)` is exactly 0.5 in `f32`, and
`rise` lands on 6.0, 3.0, 1.5 and 0.75 with no residue. At the top of the
range `rise` is set to zero rather than to `cos(90)`'s float remainder,
so height displaces nothing and the terrain reads through shading alone.
`relief` multiplies `cos(tilt)` and so vanishes with it: it is a knob in
the tilted range and has no effect at the plan view.

The tilt is a field of the camera, kept through a zoom step and through a
round trip into a perspective mode and back, the way the zoom already is.

**Zoom is scale alone.** ADR-004's four ratios stay as four steps of
`Scale`, and none of them touches the tilt. The integer footprint stops
driving the basis, and the far zoom becomes a genuine half of the mid
zoom at the same tilt: its `rows` halves from 1.4142 to 0.7071 and its
`rise` is unchanged at 0.75. ADR-007 recorded why it was not a halving —
"a 2x0.5 footprint cannot be drawn in whole cells" — and with a float
basis it need not be, since nothing draws a tile as a block of cells; the
raster casts a ray per cell. The ground a screen cell covers becomes
uniform across the scale: a column is `TILE_CM / cols` and a row is
`2 / sin(tilt)` columns of it, four at the floor and two at the plan
view. ADR-006's table gains the corrected far row, 2.83 m where it says
1.41 m, and every other cell of it stands.

**Height on screen and detail on screen are two numbers.**
`rows_per_metre` does two jobs today: it is `rise`, how far a metre of
world height displaces a thing up the screen, and it is the key for level
of detail, for the sprite tier a billboard picks, for the height a
billboard is drawn at, and for how many samples the walk takes per metre.
Under a tilt the first goes to zero and the second must not: a plan view
at 1:1 resolves the ground exactly as finely as a tilted one, and a
person seen from overhead is still a person-sized billboard. So
`Camera::detail_rows()` is the second job — `relief * columns_per_metre *
CELL_ASPECT * cos(30)`, the rows a metre of an upright thing facing the
viewer draws as, which is `rise` at the floor tilt to the bit and does
not move when the table tilts. `raster::lod_of`, `raster::detail_octaves`,
the walk's `steps`, `grid::Detail::of`, `sprites`' tier choice and its
`wz` reconstruction, `overlay`'s cloud-layer switch and `shadow`'s prop
threshold read `detail_rows`. `rows_per_metre` keeps its name and its
meaning as `rise`, read by the projection and by the screen-extent culls
that ask how many rows a thing occupies.

**The plan view keeps the cloud parallax.** An orthographic camera has no
parallax at any tilt; `CloudView`'s C/(C − H) comes from a virtual
altitude the camera states (`Camera::altitude`), and that is what makes
the effect exist. It stays at every tilt, the plan view included. The
altitude is keyed off the footprint today (`hw` in three bands); it is
keyed off the zoom index instead, which gives the same three numbers —
100 m at 1:8, 130 at 1:4, 240 at 1:2 and 1:1 — and does not move with the
tilt. At the plan view the vertical offset `rows` is zero and the sample
is the ground point pulled toward the screen centre by `1 - H/C`, so the
clouds still slide over the ground as the view pans.

**The inset shares the tilt.** The owner: "The inset view still respects
the zoom comparison approach." `Camera::inset`, `inset_zoom` and
`inset_ratio` stay as they are, and the inset takes the main view's tilt
along with its yaw, so the one thing that differs between the two panes
is the scale, which is what the pane is there to compare. A perspective
camera has no tilt to lend, so its inset uses the floor: a placement's
pitch is an eye's, and the inset is a table.

**Two settings rows.** The `camera` row is where the eye sits and gains a
fifth value:

| value | the eye |
|---|---|
| `isometric` | the orthographic table, at its tilt |
| `chase` | close behind and above the character |
| `shoulder` | well back and high, off to one side |
| `first-person` | at the character's eye |
| `free` | detached, flown by the player |

The `coupling` row is whether the body turns with the view:

| value | what a walk key means | what the mouse turns |
|---|---|---|
| `body-turns` | the direction the view faces, every tick | the view and the walk with it |
| `view-only` | the direction the view faced when the key went down | the view alone |

Under `body-turns` the mouse yaw steers a walk in progress, which is
Minecraft's arrangement and is what the game does today: the event loop
calls `Camera::held_heading` on the current camera each tick. Under
`view-only` the heading is taken when the held set changes and then held
in map space, so the view can swing while the figure keeps walking north,
and the next press takes the yaw the view has then.

`Camera::heading` and `Camera::held_heading` do not change: they answer
what a screen direction means on the ground, which is the same question
under either coupling. `Entity::facing` does not change either: it is
left or right for the mirror of the art (ADR-008), taken from the screen
sign of the heading, and it follows the walk under both. The body's yaw
is not drawn — the art has a front view and its mirror, and ADR-008 left
the back view out — so coupling shows in where the figure walks and
nowhere else.

`traversal` stays and is not subsumed. It says what frame a walk key
names, the view or the map axes; `coupling` says whether the view's yaw
reaches the walk at all. Under `map axes` a key already names a compass
direction, so both couplings walk the same way and the row has no effect.

**A third axis: what a key addresses.** `camera` is where the eye sits
and `coupling` is whether the body turns with the view. What a walk key
*addresses* is a third question: the character, moving now (ADR-008's
walk), or a plan the character executes later. The owner has described a
planner mode of the second kind — pan the view without moving the
character, mark waypoints, commit them, watch them run at 1:1 — for the
zoomed-out views, which is filed as its own issue and decided in its own
ADR. This one names the axis and ships one value of it: the row is
`control`, its present value is `direct`, and the planner is a second
value of that row. The row is written into `settings.toml` when it has
that second value, since a row with one value is a popover line that
cannot be cycled.

**The free camera.** A detached eye for scouting, screenshots and
looking at what the renderer did. It is a perspective camera whose anchor
is the eye itself, entered from wherever the view was: from a perspective
mode, the eye it already had; from the isometric mode, the point under
the screen centre at `focus_z`, pushed back along the yaw and up by the
tilt at `Placement::SHOULDER`'s thirty metres, so the switch does not
jump. It ignores `coupling`, having no body to turn.

The walk keys fly it, since the character is not walking: forward is the
view direction with its pitch, so looking down and pressing `w` descends,
and shift flies at `World::RUN` times the pace. The pan keys keep panning
— `pan`'s perspective branch moves the anchor, which here is the eye —
and `c` returns the eye to the character. `follow` does not run, so the
character stands where it was left; the inset keeps looking at the
character, so it stays visible while the eye is away.

`free` is the perspective form of a detachment the isometric mode has
always had: the pan keys move the view while the character stands, and
`follow` is suppressed until a walk settles. Both are one rule —
`Camera::follow` runs while the controls are addressing the character —
and the guard at `src/bin/roguemap.rs:107`, today `is_perspective() ||
settling`, becomes that question. A planner's plotting phase is the same
detachment with a `control` value behind it, so it plots on whichever
camera is in play and needs none of its own: the isometric table it pans
over at 1:4, or the free eye it flies. The ground point a mark lands on
is `Camera::unproject` at the height under the cursor, which the ADR-006
step already calls.

**Keys.** `{` and `}` are `Pitch(-5)` and `Pitch(+5)` and do nothing in
the isometric mode, because `Camera::pitch_by` returns early for an
orthographic view. They tilt the table there instead, five degrees a
press, clamped to the range; in the perspective modes they keep the
pitch. The mouse's rows tilt the table for the same reason — ADR-008's
`Mouse::Turn` already sends them to `pitch_by`, where they were
discarded. The help entry's word becomes "tilt", which falls past the
120th column of the generated line and so leaves the bottom bar of every
golden frame where it is.

`V` enters the free camera and returns to the mode it was entered from,
the one binding this ADR adds. A view for looking around is reached
often and from anywhere, and a popover is the wrong door for it; the
planner of the deferred issue enters the same way, so the key is the
door to both. `coupling` and the four placed values of `camera` are
cycled in the settings popover, as `traversal`, `mouse` and `fog` are.
The binding goes at the end of `input::SCENE`, where ADR-008 put the
diagonals, so its help entry lands past the 120th column and no golden
frame's bottom bar moves.

## Out of scope

Jumping, gravity and a body that leaves the ground are the next
increment and have their own ADR; running is already the shift of
ADR-008, `World::RUN` = 3. The hexagonal event grid the owner raised
alongside this is deferred with it. A `relief` settings row: the tilt is
the knob asked for, and a second one can follow if the tilted look wants
tuning. A drawn body yaw, which would need the back view ADR-008 left
out. Walking a creature other than the player, and pathing.

The planner mode is out of scope and has its own issue; the seam is the
`control` row above. One position it inherits: the control mode is a
setting the player chooses, and the zoom does not choose it. The owner
ties the planner to the views beyond 1:1, and a zoom step that silently
changed what every key does would be the complaint this ADR answers,
stated in the owner's own words at the top. `traversal`, `coupling`,
`camera` and `mouse` are each an explicit row and none of them moves with
the zoom; the zoom decides what is drawn, and the frame system can gate a
frame on it (`Show::ZoomedOut`). The zoom can gate the planner the same
way — offered where it is useful, entered by the player — so `z` never
rebinds a key.

## Consequences

- Stage 1 moves no pixel. `Camera` gains `tilt`, `relief` and
  `detail_rows`; `preset` builds the basis from them; every golden frame
  is identical under `GOLDEN_STRICT=1`, with the far preset holding its
  old `rows` behind a two-line exception until stage 2 removes it.
- `Basis::pitch()` stops being the tilt. With `relief` in the basis it
  returns `atan(tan(tilt) / relief)` — 25.24 degrees at the floor, the
  number ADR-007 pinned — so it is renamed `apparent_pitch()` and
  documented as the angle the exaggeration implies, and `Camera::tilt()`
  is the real one. `preset` no longer writes `self.pitch` from the basis;
  the isometric camera's pitch field is its tilt, which changes the fog
  direction `fog_depth` uses in an orthographic view from 25.24 to 30
  degrees. No golden frame sets `fog = always`, so nothing moves; a
  scene that does gets the honest depth.
- `Camera::nearest_zoom` keys off columns per metre rather than `rise`,
  which is tilt-independent and so still names the right preset for a
  camera built by `orthographic` at a steep tilt.
- The far zoom changes at stage 2, and that is the cycle's intended
  visual change. `island`, `rotated`, `clouds`, `settings` and `worldmap`
  are drawn at 1:8 (the last through `fitting_zoom`, which cannot fit a
  32-tile map on 120 columns at any preset and falls back to the far
  one). Those five are re-recorded; the other twelve stay bit-identical.
- `hw` and `hh` retire. `footprint()` becomes the cells a tile spans at
  the compass view, `(cols * sqrt 2, rows * sqrt 2)` rounded to at least
  one cell — (4, 1) far, (8, 2) mid, (16, 4) near, (32, 8) close at the
  floor tilt, the far row being the one that changes, and a rounding that
  only bites at tilts between. `raster::ground_hash` and the door and
  window widths in `raster.rs` read the column half, which no tilt moves.
  `pan` slides by it. `editor/preview.rs` names its panes "16x4" from
  `cam.hw`; the name becomes the zoom's, `close 1:1`, since the preset is
  no longer an integer pair of cells. `frame.rs`'s `LayoutCtx` carries
  the zoom index and `ZOOMS.len()`, which are unchanged.
- `Camera::altitude` keys off the zoom index. The cloud test in
  `camera.rs` reads `footprint().0`, which stands.
- `settings.toml` gains a `coupling` row and a `free` value on `camera`;
  `REQUIRED_SETTINGS` goes from 15 keys to 16, `Camera::MODES` from four
  names to five, and `Settings::apply` maps the fifth to `Mode::Free`.
  The popover grows a line, which is the `settings` frame's second
  re-record; at 80 by 25 the sixteen rows and the border still fit.
- `src/bin/roguemap.rs`'s tick is where coupling lands: the walk heading
  is recomputed each tick under `body-turns` and latched under
  `view-only`, and `free` skips `follow` and `step_walk` and flies the
  eye instead. The `follow` guard at line 107 stops asking whether the
  view is perspective and asks whether the controls are addressing the
  character, which is the predicate a planner mode reuses.
- The snapshot gains `tilt=` in degrees, defaulting to the floor, so
  every existing invocation renders what it renders now.
  `camera=free` places the eye where the isometric switch would.
- Tests: the basis at the floor tilt is the preset's to the bit at 1:4,
  1:2 and 1:1, and the far zoom's `rows` is exactly half the mid zoom's
  with its `rise` unchanged; the tilt a camera is set to is the tilt its
  ground plane draws at, across the range, and the height exaggeration is
  `relief` at every tilt below the top; at the top `rise` is exactly zero,
  the ray's drift is zero and a column of world projects to one point; a
  row of ground is `2 / sin(tilt)` columns at every zoom and tilt; the
  detail scale is the same number at 30 degrees and at 90 for one zoom,
  so the level of detail, the sprite tier and the walk's step count do not
  move with the tilt; the cloud sample moves by C/(C − H) per tile of pan
  at both ends of the range; the tilt clamps at both ends and survives a
  zoom step and a mode round trip; under `body-turns` a walk in progress
  turns with the yaw and under `view-only` it holds its map heading while
  a fresh press takes the new one, and under `map axes` both walk the
  same way; the free camera's eye moves while the player's position does
  not, and leaving it restores the previous mode; the settings rows'
  values are `Camera::MODES` and `Coupling::NAMES`.
- `input::SCENE` gains `V` at the end of the table, and `Action::Toggle`
  does not fit it: the free camera is a value of a settings row rather
  than a frame, and returning to the mode it was entered from is state no
  row holds. It is its own action, `FreeCamera`, and the mode it
  suspended is a field of the game state beside the camera.
- The input test at `src/input.rs:520` asserts that the isometric pitch
  does not move when the mouse drags down. It is rewritten to assert the
  tilt moves and clamps at 30 and 90.
- Cost: the basis gains a sine, a cosine and two multiplies, computed
  when the camera is built or its tilt set, never per projection. The
  plan view's walk has zero drift, so every sample of a cell's ray is one
  ground point; the stand at 1:1 is measured at the floor tilt and at the
  plan view and reported in the commit.

## Implementation plan

Stage 1, bit-identical, gated by `make check` with `GOLDEN_STRICT=1`:

1. `Camera` gains `tilt`, `relief` and `CELL_ASPECT`; `preset` builds the
   basis from the tilt and the scale, with the far preset holding its old
   `rows`. `Basis::pitch` becomes `apparent_pitch`; `Camera::tilt()` is
   the real angle. The old formulas stay in the test module and the
   lattice assertion of ADR-007 stands.
2. `Camera::detail_rows()` and its callers: `raster::lod_of`,
   `detail_octaves` and the walk's `steps`, `grid::Detail::of`,
   `sprites`' tier and `wz`, `overlay`'s cloud switch, `shadow`'s
   threshold. `altitude()` off the zoom index; `nearest_zoom` off columns
   per metre.

Stage 2, the far zoom, one commit with its reason:

3. Drop the far exception; `footprint()` from the basis; `hw` and `hh`
   go, and `editor/preview.rs` names panes by ratio. Re-record `island`,
   `rotated`, `clouds`, `worldmap` and `settings`; the rest pass strict.
   ADR-004's footprint column and ADR-006's step table are corrected in
   the same commit.

Stage 3, the tilt in play:

4. The range and its clamp, `{` and `}` on the isometric camera, the
   mouse's rows, `tilt=` in the snapshot, `relief` zeroed at the top.
5. Two golden frames: `plan`, the yardstick scene at 1:1 straight down,
   and `overhead`, the cloud scene at 1:4 straight down, which is the
   parallax at the plan view.

Stage 4, coupling:

6. The `coupling` row and `Coupling`; the tick's two paths; the tests
   above. The `settings` frame re-recorded for the row.

Stage 5, the free camera:

7. `Mode::Free`, `Placement::FREE`, `Action::FreeCamera` on `V` and the
   suspended mode it restores, the entry from either kind of view, the
   walk keys as the fly keys, `c` back to the character, `follow`
   skipped. The `settings` frame re-recorded for the fifth value, with
   stage 4's row in the same shot if the two land together.

Stage 6, docs:

8. The tilt and relief in [rendering.md](../rendering.md) and
   [scale.md](../scale.md), replacing 25.24 and 43.31 with the tilt and
   the exaggeration; the corrected far row in scale.md's step table; the
   two rows in [frames.md](../frames.md); the tests in
   [testing.md](../testing.md); `V` and `{` `}` in the README key table.
   [index.md](../index.md)'s decision table lists ADR-001 to ADR-007 and
   needs lines for ADR-008 and this one.
