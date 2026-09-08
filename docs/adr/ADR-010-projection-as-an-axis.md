# ADR-010: Projection as an axis of its own

Amends [ADR-009](ADR-009-camera-modes-and-controls.md)'s `camera` row and
[ADR-007](ADR-007-general-camera.md)'s account of what a camera is.
ADR-009's tilt, its coupling, its free camera and its four zooms stand;
[ADR-004](ADR-004-world-scale.md)'s ratios and
[ADR-008](ADR-008-walking.md)'s walk are untouched.

## Context

The owner, on the camera modes: "I'd like to make an adjustment to camera
modes - having both a perspective and iso for the maximum zoom out mode.
for perspective, I'd like to be able to free look in any direction." And
then what is behind it: "the thing I find restrictive is when zoomed out
all the way, I want to see the horizon".

**An orthographic projection has no horizon.** A horizon is the vanishing
line of a perspective projection: the place where lines parallel to the
ground converge, at eye level. An orthographic projection has no
vanishing point, so its parallels stay parallel and its ground plane runs
off the top of the screen at whatever angle it is drawn at. Tilting
ADR-009's table from 90 degrees to 30 makes the ground shallower and
never makes it end. The overview cannot show a horizon at any tilt, at
any zoom, with any relief. So this is not a request for a second look
alongside the isometric one; it is a request for the one thing that
projection cannot do.

The other four values of the `camera` row are perspective already, so the
projection the owner wants is in the engine and is out of reach from the
one value that is zoomed out.

**What the horizon needs beyond this ADR.** A horizon is a place where
the ground stops. On a flat world nothing stops it but the fade —
`World::visibility` and the `fog` row — so the horizon would be the fog
distance, about 106 m on a clear day at noon, and on the bounded
`island` view the world's own edge arrives first: a 32-tile map is 64 m
across. The owner has since raised the shape of the world itself: "maybe
we project onto a spherical world rather than a flat world. we can do
play toy sized world for now, but I think something that can project as
ortho-ish, for game play reasons, but when zoomed out sufficiently
becomes an entire planet". On a sphere the horizon is geometric — the
tangent from the eye, at `sqrt(2 R h)` for eye height `h` — and it bounds
the far field for free and without an edge. Which of those two the
horizon is decides what the far field costs and what is drawn past it, so
this ADR ships the projection and leaves the horizon to the decision
about the world's shape. Out of scope below says what is left open.

**The seam is already in the code.** `Camera::aim` builds a perspective
camera's basis from `depth_ref`, the metres from the eye to the point it
is aimed at:

```
focal = (sw / 2) / tan(fov / 2)
basis = { cols: focal / depth * TILE_METRES, rows: .. * sin(pitch), rise: .. * cos(pitch) }
```

`Camera::table_basis` builds the table's from the zoom's columns per
metre, the tilt and the relief. Both end at a `Basis` of three numbers
and `project` reads that basis and nothing else. What separates them is
where the scale comes from: an eye's is `focal / depth`, a table's is the
zoom. Set those two equal and the pair is one identity,

```
columns per metre * metres of depth = focal
```

so a scale and a distance are the same fact stated twice, joined by the
field of view and the screen's width. The `camera` row has been hiding
that identity, and it is what lets two overviews be compared at a named
ratio.

**The pitch clamp is not the restriction.** `Camera::PITCH_RANGE` is
`(-80 deg, 85 deg)` and positive pitch looks down: `view_axes` gives the
view direction `(-s cp, -c cp, -sp)`, and `Placement::CHASE`'s
`pitch: 30 deg` puts the eye above the character. The range already
covers all but ten degrees of the way up and five of the way down, so
opening it is worth fifteen degrees. What it is worth them for is the
angle at the middle of the range: an overview drawn at eye level is the
one that could show a horizon, and ADR-009's table floors at 30 degrees
because every angle in 30 to 90 looks down.

Nor is the clamp what makes a perspective view feel locked. `aim` places
the eye from the anchor, and in the chase and shoulder views the anchor
is the character, so turning the pitch swings the eye around them and the
character stays at the screen centre. The first-person and free vantages
have `distance: 0.0`, the anchor is the eye, and there the pitch is a
look. A perspective table's anchor is the ground under the screen centre,
which follows the pan keys and not the character. Free look in the
owner's sense — the view direction pointing anywhere while the ground
stays put — comes from the vantage, and the fifteen degrees ride along.

**What a perspective table looks like.** At the golden frames' 120
columns and a 60 degree field of view, `focal` is 103.9 columns, so the
eye sits `focal / columns` metres from the point at the screen centre:

| zoom | columns per metre | eye distance | height above the focus at 30 deg | at 90 deg |
|---|---|---|---|---|
| far 1:8 | 1.414 | 73 m | 37 m | 73 m |
| mid 1:4 | 2.828 | 37 m | 18 m | 37 m |
| near 1:2 | 5.657 | 18 m | 9 m | 18 m |
| close 1:1 | 11.314 | 9 m | 5 m | 9 m |

On an 80 column screen the same zooms put the eye at 49 m and 6 m, since
a narrower screen showing the same ground per column looks at it from
nearer. The cloud plane is at `World::CLOUD_ALTITUDE`, 20 m, so the far
and mid overviews stand above the clouds and the near and close ones
under them.

## Decision

**A vantage and a projection.** A vantage is an anchor and a placement:
what the screen centre holds, and how far back, how high and how far to
the side the eye sits from it. It yields a view direction, a focus point
and a reference distance. A projection turns those three into a basis.
Orthographic takes the scale at the reference distance and drops the
distance itself; perspective puts the eye at that distance and takes the
field of view. Each vantage fixes one of the pair and the projection
derives the other through `focal`:

| vantage | anchor | fixes | derives |
|---|---|---|---|
| table | the ground under the screen centre | the scale: the zoom's columns per metre | the distance, `focal / columns` |
| chase | the character | 12 m | the scale at 12 m |
| shoulder | the character, pushed 12 m on and 3 m right | 30 m | the scale at 30 m |
| first-person | the character's eye | 0 m | the scale at `FIRST_PERSON_DEPTH`, 4 m |
| free | the eye itself | 0 m | the scale at 4 m |

`Camera::depth_ref` already answers the fourth column for the four
placements, and `Camera::orthographic(yaw, tilt, relief, columns)` — the
general constructor ADR-009 added for the table — takes a continuous
scale and so already builds an orthographic camera for any row of it.
Neither of them is new work.

**One row for it.** `projection` runs `vantage | orthographic |
perspective`, defaulting to `vantage`: the projection the vantage has
always had, orthographic for the table and perspective for the other
four. The other two values override it. This is the `fov` row's shape,
where `preset` means the placement's own and a number overrides it, and
it is what keeps a mode switch meaning what it has always meant — cycling
`camera` from the table to `chase` gives the perspective chase.

Two values cannot do that, and the reason is worth stating. A two-value
row has one default, and the two halves of the `camera` row want
opposite ones: default it to `orthographic` and every perspective view
changes projection the moment it is entered, so `camera=chase` draws an
orthographic chase and the three perspective golden frames stop being
what they were recorded as; default it to `perspective` and the table
does, so every isometric frame goes. Coupling the two rows in
`Settings::apply` — a vantage switch writing the projection row — would
answer it and undo the split, since the row the player set would be
overwritten by the row beside it. A third value carries "ask the other
row" honestly, and a reader who has met the `fov` row's `preset` has
already met the idea.

**Eight of the ten combinations mean something.**

| vantage | orthographic | perspective |
|---|---|---|
| table | today's isometric look: four zooms, tilt 30 to 90 | the overview with an eye in it: the same four scales, the eye at `focal / columns`, the pitch the full sphere |
| chase | the table locked to the character at the placement's 30 degrees, no dead zone | today's chase |
| shoulder | the same at 30 degrees, the placement's 20 being under the orthographic floor | today's shoulder |
| first-person | inert | today's first person |
| free | inert | today's free camera |

The two inert cells are the two vantages whose distance is zero. An
orthographic projection ignores the distance, so a vantage that has none
has nothing left to say: the eye's place along the view stops mattering,
the fly keys of the free camera push the eye along a direction that moves
no pixel, and the level pitch that first person is built around is the
one angle the orthographic clamp below forbids. So `projection` has no
effect on `first-person` and `free`, and its row description says so. A
row that is inert in some modes is this table's own habit: `fov` does
nothing to an orthographic view and `traversal` does nothing under
`view-only`.

The two orthographic placements are the cheap half of the cross product
and worth their keep: they are the table's look locked to the character
without the dead zone, at the placement's own angle and at a scale
between the zoom presets. `Camera::orthographic` builds them from the
scale `focal / distance` gives — 8.7 columns per metre for the chase view
at 120 columns, between the near and close presets — and
`Camera::nearest_zoom` carries the preset that everything keyed by zoom
reads, as it already does for a perspective camera.

**The perspective table orbits, and the free camera looks.** A vantage's
anchor is what its turns are about. The table's anchor is the ground
under the screen centre, so under perspective the yaw and the angle swing
the eye around that point and the ground under the centre stays under it:
a turntable. ADR-009's tilt gesture is then continuous into perspective —
tilt the overview from straight down to level and the eye rides from
73 m above the point to level with it, at the same 73 m of range.
The free camera's anchor is the eye, so its turns leave the eye where it
is and sweep the world past it. Both answer the owner's "free look in any
direction", and they answer it differently: the turntable is the one a
zoom ratio makes sense of, and the eye that stays put is `V` away from
anywhere. A table that turned about its own eye was the third option, and
it is the free camera with a zoom bolted on.

**One angle above the ground, with a clamp per projection.** ADR-009's
`tilt` and ADR-007's `pitch` are the same angle measured from the same
place, and the code already knows it: `preset` and `orthographic` both
write `self.pitch = self.tilt`, and the orthographic branch of
`fog_depth` reads that pitch through `view_axes`. They become one field,
`pitch`, with the range the projection sets:

| projection | range | why |
|---|---|---|
| orthographic | 30 to 90 degrees | below 30 the ground plane flattens toward a line: `b()` is `cols * CELL_ASPECT * sin(pitch)`, which is zero at level, and `unproject` and `ray` divide by it |
| perspective | -90 to 90 degrees | the eye is a point and the ground plane never degenerates |

An angle outside the range it lands in is clamped on the way in, not
refused: a perspective view at 5 degrees switched to orthographic comes
up at 30 and the view jumps that far, which is the jump any clamp makes
and is visible. The angle is kept across the switch back, as ADR-009
keeps the tilt across a mode round trip, so the pair of switches returns
the view it left only when the angle was inside both ranges.

**The vantage's angle is remembered, as its zoom is.** One field for the
angle is what the camera is doing now, which is not what a vantage was
left at. ADR-009 decided the second: the tilt is "kept through a zoom
step and through a round trip into a perspective mode and back, the way
the zoom already is", and a player who tilts the table, enters the chase
view to look at something and comes back set that angle deliberately. So
the table carries a remembered angle beside its zoom, written whenever
the table's angle is set and restored when a vantage switch lands on the
table. A placement comes up at its own angle, as it always has, and
`pitch` stays one live field under one clamp per projection. The build
found this: merging the field without the memory returns a table tilted
to 60 at 30 after a trip through the chase view.

`relief` stays an orthographic idea and stays on the table: a perspective
view has no vertical exaggeration to give, and the relief multiplies a
cosine the perspective basis takes from the eye. ADR-009's zeroing of
`rise` at the top of the orthographic range stands.

`{` and `}` and the mouse's rows turn the one field, so the tilt gesture
of ADR-009 carries into perspective unchanged: tilt the overview down to
eye level and past it. The help entry's word becomes "look", which
falls past the 120th column of the generated line, where the golden
frames stop.

**The clamp opens to the full sphere.** `PITCH_RANGE` becomes
`(-90 deg, 90 deg)`, one range for every perspective vantage. Three
things at the limits, checked:

- At 90 degrees the axes stay orthonormal. `sin` is exactly 1 and `cos`
  is `-4.371139e-8`, so the view direction is `(0, 0, -1)` to within
  that residue and the up vector is `(-s, -c, 0)`, the compass direction
  the yaw was facing. Turning the yaw while looking straight down spins
  the image about its centre: the yaw becomes a roll. The same holds at
  -90 looking straight up.
- That cosine is the trap ADR-009 clamped `rise` for, and it bites again
  in perspective. `aim` sets the camera's detail scale to `basis.rise`,
  which carries `cos(pitch)`, and `detail_rows_at` multiplies by
  `self.pitch.cos()` per point. At 90 degrees both are a small negative:
  `lod_of` would read it as coarser than the far zoom and switch the
  cloud layer on over a first-person view of the ground, `Detail::at`
  would floor it at 0.1, and `sprites`' `wz` reconstruction divides by
  it. The fix is the framing above: the detail scale is the rows a metre
  of an upright thing is drawn as, which is the vantage's own fixed
  scale, and the cosine is the projection's foreshortening. So `detail`
  is `focal_rows / depth_ref` with no cosine for a placement, and the
  zoom's own detail — `table_basis(columns, floor tilt, relief).rise`,
  the number ADR-009 pinned — for the table under either projection. The
  level of detail, the sprite tier and the walk's step count then move
  with neither the tilt nor the projection, which is ADR-009's rule with
  one word added. A residue survives it where a placement pushes its
  screen centre past the character: the shoulder view's `depth_ref`
  projects that push onto the view direction, so its detail scale still
  moves a little with the angle. What the correction removes is the
  cosine.
- An upward ray costs less than a level one. `Renderer::ray_march`
  bounds `t1` by `(grid.max_top - ez) / dz` when the ray climbs, so it
  stops at the tallest geometry in the grid rather than at `sc.far`, and
  the `lo > ceiling` jump then sets `t` to `block_exit_t`, which for a
  steep ray has no horizontal edge to meet and returns `f32::MAX`: the
  walk leaves the loop at the first sample above the local ceiling. An
  eye already above `max_top` gets an inverted interval and `t0 >= t1`
  returns sky without taking a step, the same defensive shape as the
  `t0 >= t1` guard the crown sweep gained in `ShadowMask::stamp` — a walk
  handed an interval that runs backwards declines rather than asserting.
  The expensive rays are the near-level ones, which exist today at pitch
  0.

The detail correction is the one part of this that moves a pixel without
being asked to: dropping `cos(pitch)` raises the chase view's detail
scale from 3.75 to 4.33 and the shoulder's from 1.63 to 1.73, neither of
which crosses a `lod_of` threshold, so what moves is the sprite tier
choice, the tree lattice `Detail::at` asks for and the walk's sample
count.

**Zoom is the vantage's scale, whichever projection reads it.** `z` and
`Z` step the vantage's own fixed quantity: the table's preset index, a
placement's distance halved or doubled between 3 and 64 m, which is
ADR-009's rule unchanged. Under perspective a step of the table's zoom
moves the eye, since the distance is `focal / columns`; under
orthographic a step of a placement's distance changes the drawn scale
while the eye's place stops mattering. So `zoom` means how much world
fills the screen under both projections, and the two overviews are
comparable at a named ratio. The wheel keeps its two jobs and branches on
the vantage rather than on the projection: the table's wheel zooms, a
placement's narrows and widens the field of view.

**Panning and following ask the vantage first.** Both branch on the
vantage, and only the table's branch goes on to ask the projection.
`pan` on the table slides the view: `ox` and `oy` by the footprint under
orthographic, the anchor along the ground under perspective. `pan` on a
placement moves the anchor away from the character, which `follow` then
eases back, under both projections. `follow` on the table keeps ADR-008's
dead zone, the middle third of the screen each way, measured on screen
either way and spent into the offset under orthographic, or into the
anchor under perspective through `ground_shift` — the ground move that
carries a point a given number of columns and rows **at its own depth**,
solved in closed form. `ground_vector` was the first answer and is a
direction helper: it scales by the view's reference depth and has no
`1 / sin(pitch)` for rows, so it moved the figure `dy sin(pitch)` rows
and over-corrected without bound once the figure came nearer than the
reference. The build found it diverging and panicking at shallow and
negative angles.

`ground_shift`'s denominator vanishes at the horizon row, which is the
geometric statement that no ground move buys a row past the horizon, so
the solve is clamped to no farther than the figure already is and the
anchor eases toward the figure when it is refused. The aimed height eases
with it: a figure walking onto different ground leaves `az` stale, and a
figure below the aimed height sits under the horizon whatever the ground
move.

`follow` on a placement eases the anchor and leaves the
rest to `look_at_point`, whose orthographic path already places a point
at the screen centre. Today both functions branch on `is_perspective`,
which answers for the vantage only because the table is the only
orthographic thing there is.

**The overview's clouds.** The cloud pass draws into sky cells alone
under a perspective eye. The far and mid perspective tables stand above
the cloud plane, so under that rule they would draw no clouds at all,
which loses the parallax ADR-009 kept at every tilt. Along a descending
ray the height falls with the distance, so the crossing at the cloud
altitude lies in front of the surface hit exactly when the hit is lower
than the plane: the sky test becomes "a sky cell, or a cell whose hit is
below `CLOUD_ALTITUDE`", reading `wz` the g-buffer already holds. From an
eye under the plane a descending ray never reaches it, so the test is
inert for the chase, shoulder and first-person views and their frames do
not move.

**The row's value is renamed.** `isometric` names a vantage and a
projection at once, and `isometric` under `projection = perspective` is a
contradiction on the screen the player is reading. The value becomes
`table`, ADR-009's own word for it throughout, and `Camera::isometric`
becomes `Camera::table`. The cost is user-visible and it is small: the
value appears in `settings.toml`, `Camera::MODES`, the README's settings
row, frames.md's table and the `settings` golden frame, and it appears in
no `shot` line of `tools/golden.sh`, which names only `chase`, `shoulder`
and `first-person`. The `settings` frame is re-recorded for the new row
in the same commit, so the rename rides in free. `Mode::Isometric`
becomes `Mode::Table` with it, since leaving the constructor and the enum
saying "isometric" would put back in the code the conflation the row just
lost.

**What the status bar says.** `view_label` gains the projection only when
the row overrides the vantage's own, so at the default every character of
the status line stays where it is. An overridden table reads
`1:8 far 30deg persp`; an overridden chase reads `chase 12m ortho`.

## Out of scope

**The horizon, and what bounds the far field.** A horizon needs a
perspective projection and a settled world shape, and this ADR delivers
the first. On a plane the only thing that ends the ground is the fade,
so the horizon would be `World::visibility` — 106 m on a clear day at
noon, 158 m at the shoulder view's `visibility: 1.5` — and under the
`island` view the map's own edge arrives before it, since a 32-tile map
is 64 m across. An island floating in the sky has an edge, not a
horizon. On a sphere the horizon is the tangent from the eye and needs
no fade at all. So the perspective table ships with the far field it has
— the mode's `visibility_scale` times the weather's — and this ADR
promises no horizon in it. What the overview draws at its far edge is
the fade it draws today, and the decision about the world's shape is
what turns that into a horizon.

**What a round world would change here.** The projection axis itself
survives a sphere: a vantage's anchor, its reference distance and the
identity that ties a scale to a distance through the focal length are
projective facts and do not care what the surface is. What assumes a
plane, in this ADR and under it:

- `project` and `unproject` take `z` as height above one global plane
  and solve for a fixed height. `Camera::ray`'s `p(z) = p0 + d z` is the
  same assumption in the walk's own form.
- The orthographic basis is three numbers with no depth term, which can
  draw a plane at an angle and cannot draw a globe.
- `Renderer::ray_march` bounds an upward ray by `(grid.max_top - ez) / dz`
  and a downward one by `(FLOOR - ez) / dz`, a global ceiling and floor
  in `z`. The consequence below that an upward ray is cheap rests on
  that ceiling; on a sphere a level ray leaves the surface and the bound
  is the tangent instead.
- The cloud test below argues from height falling monotonically along a
  descending ray, which is true over a plane. Over a sphere a level ray
  gains altitude with distance.
- `HeightGrid` is a rectangle of tiles clipped as an axis-aligned box;
  `Map` is a rectangle that is either bounded or filled, with no
  wrapping; `Camera::depth` and `tile_depth` sort by a dot product with
  a constant yaw; the shadow mask sweeps a constant sun direction;
  `CloudView` is a plane at a constant altitude and `Camera::altitude`
  a virtual one over it.

None of those is made worse by this ADR, and the perspective table is
the vantage that would first show their edges.

Camera collision. With a positive distance the negative half of the pitch
range puts the eye under the ground — a chase view at -80 degrees already
does today — and the walk then meets the surface on the first step of
every cell, so the screen fills with earth. Pitching back up is the
remedy. A vantage that pushes its eye out of the terrain is a separate
decision and wants the same test the walk uses.

A dolly on the perspective table between the four zoom steps: the steps
are ADR-004's ratios and the eye's distance follows them, and a
continuous distance is a fifth thing to tune. The field of view row
already moves the eye — a wider view at the same zoom stands nearer —
which is as much of a dolly as this ADR ships.

A key for the projection row. The row is cycled in the popover, as
`camera`, `coupling`, `fov` and `fog` are. `V` remains the free camera's
door.

A perspective inset. The inset is there to compare two scales, and a pane
that also changed projection would compare two things at once, so
`Camera::inset` stays an orthographic table and takes the main view's
angle only when the main view is one.

The planner of ADR-009's `control` row, jumping and gravity, and the
hexagonal event grid stay where ADR-009 left them.

## Consequences

- `Camera` gains `projection`, and `is_perspective()` stops being
  `fov > 0.0` and answers the field. Every camera then carries a field of
  view whether or not it uses one, which the orthographic placements
  need to turn a distance into a scale; `apply_fov` returns early for the
  orthographic table alone, the one combination that reads no field of
  view at all.
- Stage 1 moves no pixel: `Projection::Perspective` is set exactly where
  `fov` was set positive, and the golden frames are identical under
  `GOLDEN_STRICT=1`.
- `Mode` loses `Isometric` for `Table`; `Camera::isometric` becomes
  `Camera::table`; `MODES[0]` becomes `"table"`. `in_mode`, `preset`,
  `placement_of`, `free_from`, `inset`, `fitting_zoom`, `nearest_zoom`,
  `snapshot`'s `camera=` and `from=`, `editor/preview.rs` and the tests
  follow the rename mechanically. It is confined to stage 2 and to those
  names, and nothing else in this ADR depends on it: keeping `isometric`
  is a matter of reverting that stage and the `settings` frame it
  re-recorded, and every other decision here stands unaltered.
- `Camera::tilt`, `tilt_degrees` and `set_tilt` fold into `pitch`,
  `pitch_degrees` and `set_pitch`, with the clamp read from the
  projection. `pitch_by` loses its branch. The table's remembered angle
  stays a field of its own beside `zoom`, which `in_mode` restores when
  it lands on the table. The snapshot's `tilt=` becomes `pitch=`; the two
  shot lines that carry `tilt=90` (`plan` and `overhead`) say `pitch=90`
  and render the same frames to the bit.
- `Camera::in_projection(index)` is `in_mode`'s sibling: it keeps the
  yaw, the anchor, the zoom, the angle and the field-of-view override,
  clamps the angle into the new projection's range, and returns the
  camera unchanged for the two vantages with no distance.
  `Settings::apply` calls it beside `in_mode`, and the order matters:
  the vantage first, the projection second, since a vantage switch
  rebuilds the camera.
- `settings.toml` gains a `projection` row after `camera`;
  `REQUIRED_SETTINGS` goes from 16 keys to 17. The `settings` frame is
  re-recorded: the popover is sized `items.len() + 2` rows inside its
  border and centred, so a seventeenth row grows it by one and moves it
  up one, and every cell of the panel and the two lines it uncovers
  differ. Its width does not move — `orthographic` is twelve characters,
  which ties `first-person` and `bottom-right` — and at 80 by 25 the
  seventeen rows, the hint line and the border still fit in twenty-one.
- Golden frames: `settings` for the row and the renamed value; `chase`
  and `shoulder` for the detail correction, and `firstperson`
  if 0.4 percent of its detail scale moves a tier — the fuzzy comparison
  may pass all three, so the strict run is what says. The other sixteen
  stay bit-identical: no shot line sets `projection`, the perspective
  cloud test is inert below the plane, and `view_label` is unchanged at
  the default. ADR-009 predicted five frames and moved nine because the
  far zoom also drew inside other frames' insets; nothing here changes
  what an inset draws, since the inset is an orthographic table taking
  the main view's angle. ADR-009 also predicted a `settings` re-record
  that did not happen, because the popover draws each row's current value
  and not the whole list; this one draws a new row and a changed current
  value, so it does happen.
- New frames: `tabletop`, the island at 1:8 with `projection=perspective`
  at the floor angle, where the terrain converges and the same ratio is
  drawn from an eye; and `vista`, the filled world at 1:8 at an angle of
  5 degrees, which is the low-angle overview at its most expensive and
  is the frame that would show a horizon once there is one to show. Both
  cost a shot line and a reference frame.
- `visibility_scale` reads the placement's factor and the table has none,
  so the table's placement gains one. At 1:8 the eye stands 73 m from the
  point it looks at and a clear noon fades the ground at 106 m, about a
  screen's width of it; the shoulder view's 1.5 is the precedent for
  giving the vantage its own factor and the cycle settles the number.
  What the fade means at the far edge is the horizon question, deferred
  above.
- Cost, measured rather than assumed. A perspective table builds a larger
  grid than an orthographic one at the same zoom: `Renderer::bounds_to`
  takes the frustum from `Camera::reach` out to the fog, a box about 53
  tiles either way from the eye at a clear noon, against the 42 tiles
  across that the orthographic far zoom's screen corners unproject to.
  The near-level rays a low angle casts are the longest the walk takes.
  Timed on this branch with `frames=20` at the 168 by 71 Konsole
  geometry, in the filled world of the `chase` shot:

  | view | ms/frame |
  |---|---|
  | orthographic far zoom, tilt 30 | 17.5 |
  | orthographic far zoom, tilt 90 | 7.9 |
  | chase, its own 30 degrees (the ADR-009 reference, 22.1) | 22.9 |
  | chase, level | 27.6 |
  | chase, level, 110 degrees wide | 18.7 |
  | shoulder, its own 20 degrees | 22.3 |
  | shoulder, level | 26.7 |
  | shoulder, level, 110 degrees wide | 18.4 |
  | shoulder, 110 degrees wide, pitched 80 up | 7.8 |
  | first person, level | 29.2 |

  Levelling a perspective view costs about a fifth: 22.9 to 27.6 for the
  chase view. Widening it takes more than that back, because the scale is
  `focal / depth` and a wide field of view shortens `focal`, so the
  detail scale falls and `lod_of` drops the models and the volumes: the
  wide level views come in at 18.4 and 18.7, under the chase view the
  engine draws today and a shade over the orthographic overview's 17.5.
  The perspective table at 1:8 is that case — wide, level, and coarse by
  its zoom — so it costs about what the overview costs now. Looking up is
  the cheapest frame measured, 7.8 ms, which is the fixed cost of the
  grid, the sprites and the lighting with almost no march at all. The map
  size does not enter it (28.7 at 64 tiles against 29.2 at 32), since the
  filled world's grid is cut to the frustum; a shorter far field does
  (16.8 at dusk against 18.4 at noon).
- Issue #26 is a screen-geometry problem. The same views measured at the
  golden frames' 120 by 40 are every one under the 16 ms budget — 9.9 for
  the wide level shoulder, 10.8 for the chase, 11.8 for the orthographic
  close zoom — and the 168 by 71 Konsole geometry the issue was raised at
  is 2.5 times the cells. What is over budget is a screen size, and the
  perspective modes are what happened to be pointed at when it was
  noticed. The low-angle overview this ADR adds does not worsen it: at
  either geometry it costs less than the chase view already shipped, and
  at 1:8 about what the orthographic overview costs. The issue wants a
  cost per cell, and this ADR neither raises nor lowers it.
- `src/bin/roguemap.rs:117`'s follow guard is unchanged, since ADR-009
  already made it a question about the controls, and `fly_held` is
  unchanged, since the free camera is perspective whatever the row says.
- `Camera::aim` is where most of the work lands. It returns early for the
  isometric mode today; it becomes the setup for every vantage but the
  table, and under orthographic it pushes the target by the placement's
  `lateral` and `ahead`, takes the scale from `focal / depth_ref`,
  builds the basis through `table_basis` at the placement's angle, and
  sets `ox` and `oy` from the projected target rather than placing an
  eye. The table's own setup stays in `preset`.
- `raster`'s three gates (`ray`, `ray_from`, `antialias_edges`),
  `overlay`'s cloud gate, `grid`'s volume gate, `shadow`'s volume gate,
  `sprites`' `tree_billboards` and its sprite reach, `render`'s fog
  gates, `snapshot`'s three, `ground_shift`, `cell_step`,
  `entity_point`, `fog_origin`, `fog_depth`, `project`, `unproject`,
  `ray`, `eye_ray`, `project_vector`, `cloud_view` and `reach` all read
  `is_perspective` and keep reading it: the predicate's meaning is what
  changes, not its callers. Six ask the vantage instead — `pan`,
  `follow`, `focus`, `set_angle`, `set_zoom` and `zoom_by` — and so does
  the wheel at `src/bin/roguemap.rs:211`. `set_angle` is in that list
  because an orthographic placement's eye turns with the yaw and its
  offset has to follow, which only `aim` can do.
- Tests: a vantage's scale at its reference distance is the same number
  under both projections, at every zoom and every screen width, and the
  perspective table's eye distance is `focal / columns`; the detail scale
  of a vantage does not move with the projection or with the angle, so
  the level of detail, the sprite tier and the walk's step count are the
  same in both; an orthographic camera clamps its angle into 30 to 90 and
  a perspective one into -90 to 90, and the round trip through the
  projection row keeps the yaw, the zoom and the anchor; at -90 and 90
  the view axes are orthonormal and turning the yaw rolls the image;
  the projection row is inert on `first-person` and `free`; an
  orthographic shoulder view draws at 30 degrees, its placement's 20
  being under the floor; an upward ray from an eye above `max_top`
  returns no hit and takes no step, and one from under a canopy takes
  fewer steps than the level ray in the same frame; the cloud plane is
  sampled over a ground cell whose hit is below it and not over one above
  it; the settings row's values are `Projection::NAMES` and the camera
  row's are `Camera::MODES`, `MODES[0]` being `table`.
- docs/index.md's decision table lists ADR-001 to ADR-009 and needs a
  line for this one; its opening sentence says the world is drawn "in
  isometric projection", which the perspective modes already made
  partial and this makes a choice with a name.

## Implementation plan

Stage 1, bit-identical, gated by `make check` with `GOLDEN_STRICT=1`:

1. `Projection` and the `projection` field; `is_perspective()` answers
   the field; `Perspective` set wherever `fov` was set positive. No row,
   no behaviour, every frame identical.
2. `tilt` folds into `pitch`; the clamp comes from the projection;
   `set_tilt` becomes `set_pitch`; the snapshot's `tilt=` becomes
   `pitch=` and the two shot lines that carry it follow, rendering the
   same two frames.
3. `PITCH_RANGE` opens to `(-90 deg, 90 deg)`, which no shot reaches.
   The help entry's word becomes "look".

Stage 2, the rename, one commit:

4. `Mode::Table`, `Camera::table`, `MODES[0] = "table"` and the
   `settings.toml` value. The `settings` frame is re-recorded, since the
   popover draws each row's current value.

Stage 3, the detail scale, one commit with its reason:

5. `aim` and `detail_rows_at` drop the `cos(pitch)`; the table's detail
   is its zoom's under either projection. Re-record `chase` and
   `shoulder`, and `firstperson` if the strict run says it moved.

Stage 4, the axis in play:

6. The `projection` row and `Camera::in_projection`; `Settings::apply`
   in the order vantage then projection; the snapshot's `projection=`.
7. `aim` builds an orthographic placement: the target push, the scale
   from `focal / depth_ref`, the offset from the projected target.
8. `pan`, `follow`, `focus`, `set_angle`, `set_zoom`, `zoom_by` and the
   wheel branch on the vantage; the table's dead zone spent into the
   anchor under perspective; the table's placement and its visibility
   factor.
9. The cloud test over a hit below the plane.
10. The two new golden frames, `tabletop` and `vista`.

Stage 5, docs:

11. The vantage and the projection in [rendering.md](../rendering.md) and
    [scale.md](../scale.md), replacing "the isometric mode" where it
    means the orthographic table; the eye-distance table in scale.md; the
    two rows in [frames.md](../frames.md); the tests in
    [testing.md](../testing.md); the renamed value, the `projection` row
    and the `{` `}` word in the README key table;
    [index.md](../index.md)'s decision table and its opening sentence.
