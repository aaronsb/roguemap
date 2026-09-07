# ADR-011: A sandbox on a sphere, drawn in three tiers

Answers the horizon that [ADR-010](ADR-010-projection-as-an-axis.md)'s Out
of scope left to a decision about the world's shape.
[ADR-004](ADR-004-world-scale.md)'s ratios,
[ADR-006](ADR-006-movement-by-screen-cell.md)'s centimetres,
[ADR-007](ADR-007-general-camera.md)'s camera and
[ADR-009](ADR-009-camera-modes-and-controls.md)'s tilt stand unaltered.
[rendering.md](../rendering.md)'s "What the medium is" governs, and the
three tiers below are its ladder read as distance.

Every measurement here is from
[the spike](../design-notes/spherical-world-spike.md) on
`spike-spherical-world`, which built a prototype far tier and revised its
recommendation three times against the owner's reframings.

## Context

The owner, in the order the question arrived: "maybe we project onto a
spherical world rather than a flat world ... something that can project as
ortho-ish, for game play reasons, but when zoomed out sufficiently becomes
an entire planet". Then the scale: "actual horizon scale vistas would be
really nice. might as well be ambitious but a 12,000 km sphere would be
nice." Then the reframing that decides the whole shape of this: "any
'world' where one might play an rpg in is really just one small sandbox.
why not locate that sandbox inside the size of something of the real
world?" Then what comes with the planet — "we can borrow a lot from earth.
incline tilt, a moon, etc.", and "it doesn't mean we need to inherit the
land masses" — and what does not: "I'm not trying to recreate sim-earth
with plate tectonics etc either ... for now we can just have placeholder
math to get things working."

**A horizon is a fog distance forty times today's.** `World::CLEAR_VISIBILITY`
is 120 m (`src/world.rs:263`). A 1.7 m eye on a sphere of radius 6,000 km
sees 4.5 km, `sqrt(2 R h)`. The whole of today's far field is the first
three per cent of that, and the fade is what hides the rest: from eye
height the ground past 120 m is a band under two rows tall and the quadratic
fade takes it to sky. So a horizon is a new kind of content rather than a
longer version of the content there is.

**The curvature is one subtractive term and the walk needs no change.**
Subtract `d² / 2R` from the height field, `d` metres from the viewer's
ground point. Height stays height above a tangent plane, so
`Camera::project`, `unproject` and `ray` keep `p(z) = p0 + d z`, the walk
keeps its height parameterisation, and the horizon falls out where the
ground bends below the ray. Measured on the yardstick fixture, the 1:1
isometric frame is bit-identical with the term and without it.

**And it draws no curve.** At 168 by 71 with a 60 degree field the screen
is 72.7 rows per radian, so the sag angle
`d / 2R` reaches one row at 165 km. From a 1.7 m eye the horizon is 4.5 km
and the sag there is 0.027 rows; from the world's highest tile at 119.7 m
the horizon is 38 km and the sag is 0.23 rows. The term's job is to bound
the walk where the geometry bounds it, so an elevated eye sees farther for
a physical reason and a hill beyond the horizon shows only its top.

**What a per-cell far field costs, and what it looks like.** The spike
measured five ways of marching to a kilometre horizon, single-threaded at
the 16 ms budget. Removing the perspective step cap — `2t / rows` uncapped
in place of today's clamp at 4 m — is the whole saving in samples and makes
the cost logarithmic in the distance: 10 km and 40 km from a peak cost the
same frame. It is still not enough. A level view from 121 m marched to
40 km is 54.0 ms at 168 by 71, and 30 of those milliseconds are the
antialiaser drawing four rays a cell over the half of the screen the far
field fills. A per-cell statistic from a coarse field takes it to 26.2 ms.
Neither fits.

Nor does either read as landscape. Marched, the far field is a fizz of
one-cell speckle with lakes at twenty kilometres broken into blue dashes;
as a statistic it is three flat washes with a hard edge between them. They
differ in 25 per cent of their cells at a mean colour distance of 27, which
is two pictures rather than two settings.

**The owner's answer was that the question was wrong.** On those renders:
"those test renders to the distance only look 'bad' because we are
rendering the landscape with the incorrect frequency, and we're not using
far distance LOD to our advantage. looking at a far horizon of plains, the
only thing we'd see would be the horizon which could be made up of some
colored bands and a skymap, and the typical gradient one sees to the
horizon." And how the distance divides: "swap out the close distance with
grassy clump greebles, middle distance with rendered hills, and far
distance with essentially proxy place holders and use a liberal high/true
color palette, and it would be quite plausable."

A photograph of the Flint Hills at sunset, measured by band of screen
height, says the same thing in numbers. The far ground's horizontal
contrast is 2.10 against the sky's own 2.08 and the foreground's 6.60: the
far ground is as textureless as the air over it. Saturation runs 143, 54,
46 from foreground to horizon, so aerial perspective is a collapse of
saturation and not of brightness — the sun sets the luminance ordering,
the distance does not. And distance compresses hyperbolically: the nearest
few hundred metres are 40 per cent of the frame and everything from 3 km
to the horizon is 14 per cent.

This is rendering.md's ladder arriving at its last rung. A thing is
geometry, then a glyph, then a dot, then a tint on the cell. A far
hillside's colour is its grass tint over its tree tint over its rock tint,
summed, and the band of flat tone a horizon is made of is where that ends.

**Built, the far tier is one ray per screen column.** The spike's `band=D`
renders from the walk's reach out to `D` as a skyline elevation per screen
column over a banded haze, the land's colour at the row's implied distance
lerped into the sky, with a sextant at each column's boundary cell. On the
level view from 121 m, out to a 40 km horizon, at 168 by 71:

| the walk reaches | ms with no far tier | ms with it | the tier | rays added |
|---|---|---|---|---|
| 120 m | 10.4 | 13.0 | 2.6 | 168 |
| 300 m | 22.9 | 25.3 | 2.4 | 168 |
| 600 m | 52.3 | 53.9 | 1.6 | 168 |
| 120 m, at 120 by 40 | 4.2 | 5.6 | 1.4 | 120 |

The specification and the measurement are the same number: 168 rays on a
168-column screen, 120 on a 120-column one. A 40 km horizon costs 2.6 ms
over a frame that has none, and the whole frame is 13.0 ms at 168 by 71 and
5.6 at 120 by 40. Every per-cell far field the spike measured was over the
budget; this is the first configuration under it.

The sextants change job. Antialiasing costs 6.6 ms with the far tier and
6.7 without — a band of flat tone has no boundary between visibly different
surfaces to supersample. What the frame gains is placement: 167 sextants on
one row, one per column, putting the skyline on a third of a row and giving
it 213 vertical positions on a 71-row screen. The machinery that was
spending 30 ms on one-cell speckle now spends nothing and draws the horizon
line.

**What dominates the frame is now the middle tier's reach.** 10.4 ms at
120 m, 22.9 at 300 m, 52.3 at 600 m, with no far tier in any of them. How
far real geometry has to reach is a gameplay question.

**And none of it reads until #36 lands.** The terrain noise runs at a 44 m
wavelength against 120 m of relief, which is where issue #36's 38 degree
median slope comes from. A screen column is 0.0069 radians, so 6.9 m of
ground at 1 km and 275 m at 40 km: a 44 m feature is under one column
beyond 6.4 km and a sixth of a column at 40 km. The spike's own far-tier
render shows the other end of it — the coarse field's 570 m period puts a
lake across twenty columns at 2 km, as a hard blue slab. The far tier's
structure is right and its content is noise. Every render in the spike is
of a world with no horizontal scale.

## Decision

**The world is a sandbox on a sphere of 12,000 km diameter, and the sphere
is a constant and a fog distance rather than a renderer.** `R` is
6,000,000 m as a constant in `World`. The curvature `1 / 2R` and the
viewer's tangent point are two new `Scene` fields, and the sag `d² / 2R` is
subtracted from the height field. Nothing else about the world's shape
enters the code.

**Where the term lives.** The grid holds it for what the grid holds, and
the far sampler holds it for what the grid does not.

| what | where the sag is applied |
|---|---|
| every tile the frame draws | `HeightGrid::build` subtracts it per tile as it copies `hf`, so `sample`, `hmax`, `Geo::base`, the block bases and the tree bases all follow at no per-sample cost |
| the sea | `SEA` becomes the field `SEA - sag` wherever it is clamped today: `grid.rs:297`, `grid.rs:309`, `grid.rs:407`, and `Renderer::surface_height` and `field_height` in `raster.rs` |
| the far tier | `Map::coarse_height` subtracts it at each sample, pointwise |

The raster's samplers are the wrong home — the spike put the term there and
it moved the frames at zero curvature, because the samplers then answer off
the grid where they used to clamp. The camera is the wrong home too: its
rays are straight, and bending them would break the height form the
geometry tests take. No pass gains a parameter. `Camera::fog_origin`
(`camera.rs:823`) already names the tangent point.

**Three tiers, and the crossovers are angular.** rendering.md chooses a
rung by angular size, and the same rule sets where a tier ends.

| tier | what it draws | where it ends | by what |
|---|---|---|---|
| near | grown geometry: the L-system rung, tall species (#39) | tens of metres | the species' own height at a few columns: a 1.5 m prairie grass at 72 m, a half-metre tuft at 24 m |
| middle | the ray walk on the height grid — terrain, blocks, volumes, trees, cast shadows | `Scene.reach`, the grid's own box | the rungs the walk descends inside it; today, the budget |
| far | one march of the coarse field per screen column, a skyline over banded haze | the geometric horizon | the tangent from the eye, plus the tangent to the tallest relief that can show over it |

The far distance is derived rather than set: `sqrt(2 R h_eye) + sqrt(2 R h_max)`
is 42 km from a 1.7 m eye over 120 m of relief and 76 km from the highest
tile. The far tier's cost is one ray a column and its march is logarithmic
in the distance, so a derived horizon is affordable where a tuned one buys
nothing. Its own content ends before the horizon does: at #36's 167 m
wavelength a feature is one column wide at 24 km, so the skyline steps out
to about there and is flat tone beyond.

The middle tier's crossover is the one the rule does not yet set. The
angular rule would put it at kilometres — a 10 m tree holds a glyph to
1.4 km — and the budget puts it at 120 to 300 m, two orders nearer. The gap
is that the middle tier draws every rung at full cost out to its edge:
volumes, gradients, bisection and a shadow lookup for a tree that is one
column wide. Closing it is descending the ladder inside the middle tier,
which is what #39 does at the near end and what #36 makes possible at the
far end. Until then the reach is a number, and this ADR makes it one: a
`reach` settings row in metres, defaulting to today's visibility.

**Three distances, where there is one today.** `Scene.far` is the walk's
reach, the fade distance and the grid's box at once, all of them
`World::visibility()`. They separate:

| number | what it bounds | today | after |
|---|---|---|---|
| `Scene.reach` | the walk and `bounds_to`'s grid box | 120 m by the weather | the `reach` row by the weather |
| `Scene.fog` | the fade to sky, `lighting::fog_factor` | the same 120 m | an atmosphere over kilometres |
| `Scene.horizon` | the far tier's march | — | `sqrt(2 R h_eye) + sqrt(2 R h_max)` |

`World::CLEAR_VISIBILITY` becomes the atmosphere's scale in kilometres and
stops being the walk's reach. A quadratic fade to sky over 4.6 km hides a
coast at 1.6 km, so the fade curve is the atmosphere's own work and belongs
with the sky (#38) rather than with the walk.

**The far tier is a pass, not a continuation of the walk.** `far_pass` runs
after `terrain_pass` and fills only the cells the terrain pass left as sky.
Per screen column it marches `Map::coarse_height` outward for a maximum
elevation angle; per cell below that angle it takes the land's colour at
the distance the row implies and lerps it into the sky by the haze; at the
column's boundary cell it writes a sextant, land over sky. Its cells carry
`lit: false`, so `light_pass` (`lighting.rs:99`) copies them through with no
sun, no cast shadow and no second fade — the haze is the far tier's own.
`HeightGrid::t_span` then bounds the walk exactly as it does today.

The spike ran it as a continuation: its march reached `Scene.far` whether or
not the ray ever crossed the height grid, so `t_span` stopped bounding the
walk, and its samplers answered off the grid where they used to clamp. Both
are changes to the flat world with no curvature in them, and both moved
golden frames. A pass that writes only into cells `terrain_pass` left as sky
cannot move a cell the walk drew, so stage 1 is bit-exact by construction
rather than by measurement. What the spike settled is that a horizon is
affordable and what one looks like; its plumbing is a prototype's, and this
is the one place the ADR departs from it.

`Map::coarse_height` is `Map::field` with the control layer alone: the
0.0035 noise at its 0.62 weight, the 0.045 local octaves at their mean, no
rivers, through the same `powf(1.15)` and `relief`. `control_height`
(`map.rs:512`) is already the seam, and its comment already says an
authored control grid can replace it.

**What the sphere is not.** No spherical mesh, no new coordinate system, no
floating origin, no wrap, no poles, no seams. `HeightGrid` stays a box of
tiles and `Map` a rectangle. The sandbox is a rectangle on a tangent plane
and the planet is where the rectangle sits.

**Precision holds by an order in the renderer and by three in the store.**
An `f32` metre position at the far edge of a 10 km sandbox resolves to
`ulp(10000) = 2^-10` m, 0.98 mm, a tenth of ADR-006's centimetre. ADR-006's
own store is `i32` centimetres, where 10 km is 10^6 counts against `i32`'s
2.1 × 10^9 — three orders to spare. Rendering the flat world at `cx = 10^7`
tiles would break; nothing asks for it.

**Global position is a low-precision quantity.** Where
the sandbox sits on the planet is a latitude, a longitude and an axial
tilt, carried as an asset row. Only three things read them: the annual
temperature field, the season, and the sun's path. `Map::temperature`
(`map.rs:657`) computes a "slow latitude-like field" from noise today and
becomes a real latitude plus a local field; `World::elevation` and
`World::sun` become a solar path at that latitude. None of that touches the
renderer, and the model behind it is #33's.

**What the eight plane assumptions do under the term.** The ADR-010 drafter's
inventory, checked against the code:

| assumption | verdict |
|---|---|
| `z` is height above one plane; `p(z) = p0 + d z` | stands: `z` is height above the tangent plane and the sphere is inside the height function |
| the orthographic `Basis` has no depth term | stands |
| an upward ray is bounded by `grid.max_top`, a downward one by `FLOOR` | stands; the floor bound gains the sag at the far distance |
| height falls monotonically along a descending ray | stands: height above the plane is still linear along a ray |
| `HeightGrid` is a box of tiles, `Map` a rectangle with no wrap | stands: beyond the grid the far tier samples the map's own function, or the sea |
| `depth` and `tile_depth` sort by a dot with a constant yaw | stands |
| the shadow mask sweeps a constant sun | stands: the sun's direction turns 0.04 degrees across 4.5 km |
| `CloudView` is a plane at constant altitude | stands: a 4.5 km cloud is 1.7 m low against the sphere, under a row |

The list is avoided rather than answered. What a true sphere would cost is
a closed surface, and the owner's reframing removed the need for one.

## Out of scope

**The sky.** Issue #38 has it, and it is half of why a horizon line reads:
the reference photograph's sky is palest just above the horizon, saturation
70 against 101 over the whole sky. `World::sky()` (`world.rs:508`) is one
flat colour and the far tier is drawn against it. The far tier lands
without a gradient behind it and looks better when #38 arrives.

**Where the middle tier stops, as a settled number.** This ADR makes it a
row and gives it today's default. What it should be is a gameplay question
about how far a player needs real geometry, and the cycle that builds the
far tier is where it gets answered.

**The far tier's own parameters.** How many strips, whether their
boundaries come from ridgelines in the coarse field or from fixed distance
rings, and whether a skyline per column wants smoothing across columns are
open. The prototype takes a haze scale and nothing else, and its curve is a
placeholder exponential.

**Whether the model holds away from a sunset.** The reference photograph
puts the sun on the horizon, so its far ground is darker than its sky. The
saturation and contrast collapse are the distance's; the luminance ordering
is the sun's. No midday reference was measured.

**Views from far above the sandbox.** The spike's worst case is an eye at
121 m, the world's highest tile. From 1,000 m the horizon is 110 km, and
whether what is out there is a painted distance, the coarse field or haze
alone is open. Whether the game offers an eye at height looking level at
all is open with it.

**The planet as a tiling of scenarios** (#34), **the climate model** (#33)
and **the hex encounter grid** (#28). All three hang off the sandbox having
a place on a planet and none is decided here. #28 in particular does not
change: a hex grid stays a local structure over a rectangle, and the
Goldberg, cube-sphere and lat/lon question does not arise.

**Parallelising the walk.** A ray per cell is independent and the box has
thirty-two threads; no frame the spike measured used more than one. A
horizon no longer needs it. What it buys the middle tier is open, and #26
is where it belongs.

**A 500 m toy planet.** On a 500 m sphere the sag reaches a row at 13.75 m
and a hill 80 m off sinks six metres, which is a different game and a
different renderer.

## Consequences

- `Scene` gains `curvature`, `tangent` and `horizon`, and `far` becomes
  `reach`. Every pass keeps its signature. `Scene::new` derives them,
  `curvature` from `World::PLANET_RADIUS` and `tangent` from
  `Camera::fog_origin`.
- `HeightGrid::build` (`grid.rs:280`) gains the sag as it fills `data` from
  `t.hf`, which is the one place the term needs to be for every tile
  consumer. `hmax`, `Geo::base`, `Geo::ceiling`, the stack bases at
  `grid.rs:309` and the tree bases at `grid.rs:407` follow with no further
  change.
- `SEA as f32` becomes a field at the five raster and grid sites that clamp
  to it; `map.rs`'s own uses stay, since the map generates in map space
  where there is no viewer and no tangent point.
- `Renderer::ray_march` (`raster.rs:643`) loses `MAX_STEP`. At the golden
  frames' 120 columns and a 60 degree field, `focal_rows` is 51.95, so the
  step rule `2t / rows` passes 4 m at 104 m and the cap bites only between
  there and `Scene.reach`. The `FLOOR` bound at `raster.rs:669` gains the
  sag at the far distance.
- `Renderer::bounds_to` (`render.rs:338`) takes `sc.reach` where it takes
  `sc.far`, so the grid box stays the size it is today when the fog
  distance grows to kilometres. `Camera::far_reach` (`camera.rs:611`) reads
  the reach and not `CLEAR_VISIBILITY`.
- `Renderer::draw` (`render.rs:283`) gains `far_pass` between
  `terrain_pass` and `sprite_pass`. `sky_pass` is unchanged and the far
  tier draws over what it left.
- `Map` gains `coarse_height`, beside `control_height` (`map.rs:512`) and
  built from the same layers as `field` (`map.rs:485`).
- `World::CLEAR_VISIBILITY` (`world.rs:263`) changes meaning and value, and
  the assertion in `lighting.rs:212` moves with it. `World::visibility`'s
  weather and light factors stay as they are, and the `reach` row is scaled
  by them the way `far` is today.
- `assets/settings.toml` gains a `reach` row and `REQUIRED_SETTINGS`
  (`settings.rs:28`) goes from 16 keys to 17, or from 17 to 18 with
  ADR-010's `projection`. The popover is sized `items.len() + 2` rows inside
  its border and centred, so the `settings` frame re-records and at 80 by 25
  eighteen rows, the hint line and the border fit in twenty-two of
  twenty-five. The `fog` row's description ends "The distance is the
  weather's visibility", which stops being true and is rewritten.
- The `snapshot` gains `sphere=`, `reach=` and `band=` on the spike's own
  spelling, so a frame can be rendered flat, and #37 wants them validated.
- `Renderer::terrain_hit` (`raster.rs:750`) is not reached by the far tier:
  a far cell has no gradient, no face and no `Hit`. It is a colour and a
  sextant, written straight into the g-buffer.
- The shadow mask and the colour cache are grid-indexed and clamp at their
  edge. The far tier asks neither, so the clamp keeps answering only for
  the grid. `ShadowMask::stamp`'s twelve-tile
  sweep sees a differential sag of 1.8 cm at 4.5 km.
- The inset shares the `Scene`, so it shares the main camera's tangent
  point. The sag across its fifteen metres is under a micron. The inset is
  an orthographic table and never sees a horizon.
- ADR-009's tilt is untouched: the tilted table at 1:8 spans 120 m by 200 m
  and the sag across it is 3 mm, and the 30 degree floor never looks along
  the ground.
- `view = island | filled` survives. Today an island's surroundings render
  as sky; under the far tier they are sea to the horizon.
- The world map is a top-down plot of the sandbox and does not change.
- Golden frames, in the order the stages move them:

  | stage | frames | why |
  |---|---|---|
  | 1, the plumbing | none | the sag is zero, the reach is today's `far`, and the far pass is not built. `GOLDEN_STRICT=1` says so |
  | 2, the step cap | `chase`, `shoulder`, `firstperson` | the cap bites past 104 m, where the fade has already taken three quarters of the colour. The fuzzy comparison may pass all three |
  | 3, the curvature on | the far-zoom frames and the 1:8 insets | below |
  | 4, the three distances | `settings` | its new `reach` row. The atmosphere lands dark, so the perspective frames hold |
  | 5, the far tier | `chase`, `shoulder`, `firstperson`, then `firstperson` again | the atmosphere turned on fades their far ground far less than 120 m does; and the island's surroundings become sea to a horizon where they were sky. Plus `horizon` and `shore`, new |

  Stage 3 is the one to predict carefully. The sea surface sits exactly on
  0.0, which is on the walk's height lattice, so a millimetre of sag moves
  those hits by a whole sample: the spike's 1:8 overview moved 267 of 11,928
  cells and its 1:1 yardstick moved none. So the movers are the frames with
  water at the far zoom — `island`, `rotated`, `clouds`, `overhead` and
  `settings` around its popover — and, by ADR-009's first mistake, the 1:8
  insets inside `closeup`, `props`, `scale`, `stride` and `plan`, since at
  120 columns a 1:1 main view draws a 1:8 inset. `worldmap` plots tile
  heights and does not move. ADR-009's second mistake was a `settings`
  re-record it predicted and did not get, because the popover draws each
  row's current value rather than the list; here the row itself is new, so
  it does.
- The spike's branch is red on `steppe` and `shoulder`, 4.8 and 3.8 per
  cent of cells at zero curvature. Those two frames are the price of
  inheriting its structure, and the Decision is what this plan pays instead.
  The sag goes into `HeightGrid::build`, where the grid's edge behaviour
  does not move, and the far tier is a pass over sky cells.
- What lands from the prototype: the `curvature` and `tangent` fields, the
  uncapped step, the skyline march per column, the haze lerp and the
  boundary sextant. What is prototype-only: the per-cell statistical far
  field and its crossover, the `Scene.far_stat` and `grid_far` keys, the
  `aa=0` key, the segment counters, and the samplers carrying the term.
- Cost, on the level view from 121 m at 168 by 71: 10.4 ms today with no
  horizon, 13.0 ms with a 40 km one. At 120 by 40, 4.2 and 5.6. #26 is a
  screen-geometry problem and this neither raises nor lowers the cost per
  cell of the middle tier; what it adds is 2.6 ms and 168 rays.
- Tests: the sag is zero at the tangent point and `d² / 2R` at `d`; a grid
  built at zero curvature is bit-identical to one built with the term
  absent; the horizon from an eye at `h` is `sqrt(2 R h)` within a sample;
  a hill beyond the horizon shows only its top; the far tier casts exactly
  one ray per screen column at three screen widths; a far cell is unlit and
  unfogged; `coarse_height` has no relief shorter than its 570 m period and
  agrees with `height_smooth` in the mean over a tile block; the three
  distances are independent, so a frame can have a near reach and a far
  horizon; `reach` is in `REQUIRED_SETTINGS` and its row's values parse.
- [the spike](../design-notes/spherical-world-spike.md) is not on `main`,
  so the link in this ADR's header resolves only once the note lands. Every
  timing, every cell count and the photograph's measurements are that
  note's, and nothing in the code records them. The note is the evidence and
  belongs on `main` ahead of stage 1. Its branch is not for merging; the
  note is.
- [index.md](../index.md)'s decision table lists ADR-001 to ADR-009 and
  needs lines for ADR-010 and for this one. Its opening sentence says the
  world is drawn "in isometric projection", which ADR-010 already made a
  choice with a name; this adds that the world is a sandbox on a sphere.
  [rendering.md](../rendering.md) gains the three tiers under its ladder,
  [world.md](../world.md) the sphere and the sandbox's place on it, and
  [testing.md](../testing.md) the far-tier tests.

## Implementation plan

Prerequisites. **#36 first**: a 44 m feature is a sixth of a screen column
at 40 km and nothing built over it reads as landscape at any price, and the
far tier's own lakes are hard blue slabs twenty columns wide at 2 km for
the same reason. **ADR-010's stages before
any of this is reachable at the overview**: an orthographic projection has
no horizon, so until the perspective table ships the horizon exists only in
the placed perspective views.

Stage 1, bit-identical, gated by `make check` with `GOLDEN_STRICT=1`:

1. `World::PLANET_RADIUS`; `Scene.curvature` and `Scene.tangent`, derived
   in `Scene::new` from the radius and `Camera::fog_origin`; the radius
   defaulted so the curvature is zero. `Scene.far` becomes `Scene.reach`
   and `bounds_to` takes it. No sag anywhere, every frame identical.
2. `HeightGrid::build` subtracts `curvature * d²` per tile; `SEA` becomes
   `SEA - sag` at the five clamp sites; `Map::coarse_height`. Still zero
   curvature, still identical.

Stage 2, the step, one commit with its reason:

3. `MAX_STEP` goes from `ray_march`. Re-record `chase`, `shoulder` and
   `firstperson` if the strict run says they moved; the fuzzy run is what
   decides whether the change is visible.

Stage 3, the sphere on:

4. `PLANET_RADIUS` becomes 6,000,000 m and the snapshot's `sphere=`
   overrides it. Re-record the far-zoom frames and the frames whose insets
   draw at 1:8, with the reason: the sea surface sits on the walk's height
   lattice and a millimetre of sag moves it by a sample.
5. The geometry tests: the horizon at `sqrt(2 R h)`, the hill beyond it
   showing only its top, the sag zero at the tangent point.

Stage 4, the three distances:

6. `Scene.fog` separates from `Scene.reach`; the `reach` settings row and
   its snapshot key; the `fog` row's description rewritten;
   `CLEAR_VISIBILITY` becomes the atmosphere's scale and `Camera::far_reach`
   reads the reach. **The atmosphere lands dark**: the scale defaults to
   the curve today draws, so only `settings` re-records for its new row and
   the three perspective frames stay identical. Stage 5 turns it on and
   records them once, against a fade settled beside the far tier's haze
   rather than before it. ADR-009's far preset held its old rows behind an
   exception through its stage 1 for the same reason: a reference frame
   recorded from something unfinished asserts that the picture is right.

Stage 5, the far tier:

7. `Scene.horizon` derived from the eye and the relief; `far_pass` after
   `terrain_pass`: the skyline march per column, the haze lerp, the
   boundary sextant, cells written unlit.
8. The atmosphere's scale turned on, with stage 4's three perspective
   frames re-recorded here and once — the fade and the far tier's haze meet
   at the horizon and are settled together.
9. The two parts the prototype leaves out: the strips quantised into bands,
   and the sky's own gradient palest at the horizon, which is #38's work
   arriving from the near side.
10. New golden frames. `horizon`, first-person on the world's highest tile
   looking level, which is the far tier's worst case and the frame the cost
   table is measured on; and `shore`, the open-sea eye at `cx=-2552 cy=1544`
   with the walk at 300 m and the far tier to 4.6 km, which is the frame
   where a coast at 1.6 km either reads as distance or does not.
11. Sweep the far distance for cost, which the spike measured only at 40 km,
    and confirm the march is logarithmic in it.

Stage 6, the sandbox's place:

11. The latitude, longitude and axial tilt as an asset row; `Map::temperature`
    reads a real latitude plus a local field; `World::elevation` and
    `World::sun` take a solar path. The climate model behind them is #33.

Stage 7, docs:

12. The three tiers in [rendering.md](../rendering.md) under the ladder;
    the sphere and the sandbox's place in [world.md](../world.md); the
    `reach` row in the README key table and [frames.md](../frames.md); the
    far-tier tests in [testing.md](../testing.md);
    [index.md](../index.md)'s decision table and its opening sentence.
