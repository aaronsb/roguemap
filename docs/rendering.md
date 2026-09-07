# Rendering

There is no tile atlas. Every screen cell casts a ray back into the world
and walks down until it meets something, so the camera turns to any angle
and the terrain has no seams. This page is the pipeline from that walk to
the lit cell.

![lsystem-stand-close](screenshots/lsystem-stand-close.png)

A boreal stand at 1:1. Every trunk, whorl and porous crown in that frame
is geometry met by the walk; nothing is a sprite.

## What the medium is

A cell carries a glyph and two colours: roughly eight bits of shape against
forty-eight of colour, and both are flat over the cell. So the medium's
native output is regions of flat colour meeting at hard edges, in discrete
tonal steps. A photograph's soft gradients fight that. A faceted painting,
or Frontier's banded sky over flat polygon clouds, is what it does without
being asked.

**The viewer finishes the picture.** A glyph of grass is not a poor drawing
of grass; it is a mark that says texture is here, and the reader supplies
the rest. The owner: "it gives the human viewing it a chance to fill in
with their mind the missing pixels." That is why the scene still reads at
80 by 25, where 2,000 cells carry six times more world each than at 168 by
71 — what survives compression is structure, and structure is what the eye
completes from.

**The geometry is physical; the surface is suggested.** A horizon comes
from a real curvature, a crown from a grown tree, a shadow from a swept
sun. The rendering of them is impressionistic, and it can be because the
thing underneath is coherent. A viewer completes toward something. Fake the
geometry as well and there is nothing to complete toward, and the same
glyphs read as noise.

**Motion is what carries it.** The owner again: "when motion comes into
play, it is the thing that gives the human mind the substance to color in
the gaps." A still frame is ambiguous where a moving one is not — parallax
resolves depth, a walk cycle resolves a figure, drifting cloud resolves the
plane it sits on. The unit of legibility is the sequence, not the frame, so
a snapshot may look sparser than the game does.

**Everything descends the same ladder, and it ends in colour.** A thing is
geometry while it is large on the screen, then a glyph, then a dot, then a
tint on the cell and nothing more. The owner, on prairie grass: "even the
tall prarie grass that is an l system at close distance fades to | and then
. and finally just a color at distance." Trees already do this; so do tall
grasses, tufts, boulders and roofs.

The rung is chosen by **angular size**, not by distance. A screen column
subtends 0.0069 radians, so a thing of size `s` at distance `d` spans
`s / (0.0069 d)` columns, and it drops a rung as that passes a few columns,
one column, and a third of one. One threshold serves everything: a ten
metre tree holds its glyph to about 1.4 km where a half metre tuft holds
its to about 70 m, from the same rule rather than three tables.

The last rung is why the far field needs no renderer of its own. A distant
hillside's colour is its grass tint over its tree tint over its rock tint,
summed — every species on rung four at once. The band of flat tone a
horizon is made of is the ladder's end, not a mechanism beside it.

Two things follow, and they are the constraints this page's passes are
judged against.

**A mark must hold still.** Suggestion needs somewhere to settle: a greeble
that moves is not texture, it is noise, and it spends the channel motion
was going to complete the picture through. A scatter anchored to the camera
rather than the world sparkles for exactly this reason, and a still frame
cannot show it — the golden frames are blind to the failure that matters
most, which is why a temporal test measures cell churn between two frames a
fraction of a cell apart.

**An edge must move smoothly.** Sextant antialiasing places a boundary to a
third of a row, so an edge glides as the view turns instead of snapping
from row to row. Sub-cell precision buys temporal smoothness, and that is
most of what it is for.

**The range to hold.** Low cloud, closed in, dark and rainy at one end;
brilliant white puffs against deep azure over golden fields to the horizon
at the other. Both rendered with physicality and with the texture of
glyphs. A change that serves one and breaks the other has not landed.

## The frame

`Scene` is the per-frame context: the map, the asset tables, the tileset,
the seasonal palette, the world, the camera, the animation time and the
water choppiness. Passes take a `Scene` and nothing else; a pass that
needs a new input gets a new `Scene` field rather than a new parameter.

`Renderer::draw` runs the passes in order. **Sky** fills every cell with
the sky colour plus stars that fade as the sky brightens. **Height grid**
reads every tile in view once — its smooth height, its stack resolved into
a column with merged runs, and the tree volumes whose footprint touches it
— and it is the only place `Map::get` runs, so the walk never touches the
chunk map. The **shadow mask** covers the same tiles, and **stack lights**
adds one light per in-view stack whose block kind names one, at night.
**Terrain and geometry** is the ray walk, then shading, then edge
antialiasing. **Sprites, props and lights** are billboards drawn back to
front with a depth test. The **light pass** is the only pass that writes
the scene to the canvas, and **weather and clouds** draws precipitation
and then the cloud layer over it.

## The camera

A `Camera` is a yaw (the heading, `angle`), a pitch above the horizon, a
field of view, a scale and a screen offset
([ADR-007](adr/ADR-007-general-camera.md)). A field of view of zero is an
orthographic view, which the isometric mode is; a positive one is a
perspective view from an eye, which the chase, shoulder, first-person and
free modes are. An orthographic view projects through a basis of three
numbers — `a` columns per tile across the screen, `b` rows per tile of
ground depth toward the camera, and `rpm` rows per metre of height —
which are the scale times the tilt's sine and cosine:

```
sx = a * (x cos - y sin) + ox
sy = b * (x sin + y cos) - z * rpm + oy
```

The isometric mode is a table
([ADR-009](adr/ADR-009-camera-modes-and-controls.md)):
`Camera::isometric(zoom)` builds its basis from the zoom's columns per
metre, the `tilt` the ground plane draws at and the `relief` height is
drawn taller by.

```
cols = columns_per_metre * TILE_METRES
rows = cols * CELL_ASPECT * sin(tilt)
rise = relief * columns_per_metre * CELL_ASPECT * cos(tilt)
```

The tilt runs from 30 degrees, the isometric look, to 90, straight down,
where `rise` is zero and height displaces nothing; the relief is
`sqrt 1.5` and vanishes with the cosine it multiplies, so it is a knob in
the tilted range alone. `Basis::apparent_pitch` is the angle the
exaggeration implies, 25.24 degrees at the floor, and `Camera::tilt` is
the real one. `Camera::orthographic(yaw, tilt, relief, columns)` is the
general form the presets are instances of. The basis is computed when a
camera is built, its zoom set or its tilt changed, not on every
projection, so the presets' numbers are exact; the yaw's sine and cosine
are cached the same way, so the heading is set through `set_angle`.

A tilt moves the height on screen and must not move the detail on screen,
so the two are separate numbers: `rows_per_metre` is `rise`, read by the
projection and the screen-extent culls, and `detail_rows` is the rows an
upright thing facing the viewer draws as, which is `rise` at the floor
tilt and does not move with the table. Level of detail, the sprite tier,
the walk's samples per metre, the cloud-layer switch and the prop shadow
threshold read `detail_rows`.

### The eye

A perspective mode places its eye from the character by a `Placement`:
the screen centre is the point the camera is aimed at pushed `ahead`
metres away along the ground and `lateral` metres to the right, the eye
sits `distance` metres back from that centre along the view direction,
pitched down by `pitch`, and the mode names a default field of view that
the `fov` settings row overrides. `Camera::perspective(eye, yaw, pitch,
fov)` is the general form the three presets are instances of.

| mode | distance | pitch | fov | centre from the character | the eye | fog |
|---|---|---|---|---|---|---|
| chase | 12 m | 30 | 60 | on it | close behind and above the character's middle, following them | 1x |
| shoulder | 30 m | 20 | 40 | 12 m ahead, 3 m right | well back and high, off to one side, looking past the shoulder; the figure sits low and off-centre | 1.5x |
| first-person | 0 | 0 | 60 | on it | the character's eye, `EYE_HEIGHT` (0.85) of their height over the ground; the character is not drawn | 1x |
| free | 0 | the view's | 60 | it is the eye | detached, flown by the walk keys; the character stands where it was left | 1x |

The projection is the eye's: a point's offset from the eye is resolved
along the view direction, screen right and screen up and divided by its
depth, with a focal length of `(sw / 2) / tan(fov / 2)` columns and half
that in rows, since a cell is twice as tall as it is wide. A point behind
the eye lands far off screen. The basis of a perspective view is the
scale at the character's depth: `rows_per_metre` is `focal / 2 / depth *
cos(pitch)`, the first-person view stating it four metres out
(`FIRST_PERSON_DEPTH`), so level of detail, the sprite tiers of ADR-004
and the zoom preset carried for what is keyed by zoom keep one answer per
frame. `detail_rows_at` gives any other point's, which is what a
creature, a prop and a tree take at their own distance. Rotation turns
the eye about the character, `{` and `}` pitch it, zoom halves or
doubles the chase distance between three metres and sixty-four, `<` and
`>` widen and narrow the field of view, and the camera follows the
character every frame. The `camera` settings row switches the mode
through `Camera::in_mode`, which keeps the yaw and the aimed point.

The free mode is the exception to all of that: its anchor is the eye
itself, so `Camera::fly` moves the eye along the view direction and
across it, the pan keys move it too, `follow` does not run, and `c`
brings it back to the character. `V` enters it from whatever view was in
play and returns to that view; entered from an eye it takes the eye it
found, and from the table the point under the screen centre pushed back
along the yaw and up by the tilt at thirty metres, so the switch does not
jump. Leaving is that push undone, the point thirty metres down the eye's
own view: `V` twice is the view it was entered from, and a flown eye
leaves the table on the ground ahead of it.
`Camera::addresses_character` is the question that separates the
two: the walk keys walk the character and the view follows, or they fly
the eye and nothing follows.

### What the camera owns

`camera.rs` is the one place that maps the world to the screen. A module
that needs a camera quantity asks for it rather than deriving it:

| Method | Gives | Used by |
|---|---|---|
| `project`, `unproject`, `project_tile`, `anchor_at` | a world point on screen and back | sprites, lights, the grid's culling, the view bounds |
| `ray(sx, sy)` | the ray through a cell, `p0 + d * z` | the walk and its sub-rays |
| `eye_ray(sx, sy)` | the eye's ray through a cell, `eye + dir * t` | the perspective walk, the cloud plane from the eye |
| `detail_rows()`, `detail_rows_at(x, y, z)` | the rows an upright metre draws as, here and at a point's own depth | level of detail, sprite and prop tiers, a tree's model detail |
| `fog_origin(sw, sh)`, `fog_depth` | where the fog is measured from and how deep a point is in it | the light pass, the sub-rays' start |
| `reach(w, far)` | the ground under the frustum out to the fog | the grid's box |
| `in_mode(i)`, `hides_player()`, `addresses_character()`, `view_label()` | the mode switch, whether the character is the eye, whether the keys address the character, the status line | the settings row, the sprite pass, the tick, the HUD |
| `forward()`, `right()`, `depth(x, y)`, `tile_depth` | the map-space view axes and the depth sort | face shading, the sprite and prop sort, crown seams |
| `project_vector(run, rise)` | a world displacement in cells | stroke directions for bare branches and furrows |
| `footprint()` | the cells a tile spans | the ground texture lattice, door and window widths, `pan` |
| `rows_per_metre()`, `columns_per_metre()` | the scale, height and ground | the projection, the screen-extent culls |
| `tilt()`, `set_tilt()`, `pitch_by()` | the table's angle and the keys that change it | `{` and `}`, the mouse's rows, the snapshot's `tilt=` |
| `cloud_view(w, h)` | the cloud plane's parallax | the cloud layer |
| `inset()` | the second camera at the other end of the scale | the inset frame |
| `focus(sw, sh)`, `cell_step`, `screen_dir_to_map` | the target and the walk keys' steps | zoom and rotation pivots, movement |
| `Camera::isometric(zoom)` | a preset's scale | the editor's preview panes, `fitting_zoom` |

Level of detail is a table keyed off rows per metre, `raster::lod_of`,
and every switch that depends on the zoom — roof profiles, models, glyph
bands, crown seams, bisections, the wind, the cloud layer, prop shadows —
is a field of it. The world map is not a view of the scene: it plots
whole tiles per cell top-down with a cursor and has no camera.

## The ray walk

`Camera::ray` gives the walk its ray: invert the projection at `z = 0`
to get a ground point `p0`, and parameterise by height rather than by
distance: a metre of height moves the ground point by
`d = (sin * rpm / b, cos * rpm / b)` tiles, which is exactly the drift
that keeps the screen position fixed, so the path is `p(z) = p0 + d * z`.
In an orthographic view every cell's drift is the same; a perspective
ray from an eye has its own, in the same form.

### The walk from an eye

A level ray has no height to step by, so a perspective ray is marched by
distance from the eye instead (`Renderer::ray_march`): `P(t) = eye + dir
* t`, from the eye (or, for a sub-ray, from a couple of metres before the
nearest hit around its cell) out to the fog distance, the edge of the
grid, the sea bed or the height of the tallest thing in view. The step
grows with the depth so it stays about two screen rows of travel, between
a quarter of a metre and four. Each segment goes to the same geometry
tests in their height form: a whisker of tilt (`Camera::level_guard`,
a thousandth) keeps a level ray's drift finite, the direction is a
constant the tests are instantiated for (`hit_along::<UP>`: a climbing
ray's nearest crossing is the lowest, it enters a column at the low end
of its path, and a crown it is already inside counts at its foot), and
every segment is solved with its own foot as the zero of height and the
ground point there as `p0`, because a near-level ray's drift of hundreds
of tiles per metre loses the quadratics' digits in `f32` with the world's
zero. A terrain crossing is then bisected along the ray. The isometric
walk is the other instantiation of the same code, with neither the
direction nor the shift in it, which is what keeps its bits.

### The walk down

The walk starts at the highest thing that could be met — the grid's
ceiling, capped at `TOP_CAP`, which is the top of the world plus 40 metres
for a crown or a roof — and steps down. The step is `(rpm / 2).ceil()`
samples per metre, about two screen rows, which is fine enough that a step
cannot cross a terrace. High above the coarse 8-tile ceiling grid the walk
jumps straight down to that ceiling, so open sky costs nothing.

At each sample the walk tests the geometry over the segment just
descended, then the field itself: the bilinear sample between tile centres
plus `Map::detail`, from a noise lattice hashed once for the frame (see
"Once a frame"). The first crossing found is the nearest to the camera,
so it returns immediately; a walk that reaches the bottom returns sky.

A terrain hit takes its face from the gradient. Below `CLIFF = 1.5`
(three metres of rise per two of run) it is a top face; above it the
steeper axis picks a left or right wall and the drop is drawn as one to
six rows of cliff.

## Blocks

A tile carries a `Stack { kind, levels }`. Its column runs from the ground
to the eaves at `base + levels * level_height`, and the roof is a height
profile above that, so a gable rises to a ridge and the walk finds the
slope at sub-tile resolution rather than in whole tiles. Roof profiles are
`none`, `flat`, `gable` and `hip`: a gable rises with the distance to the
nearer edge across the ridge, a hip rises from every edge, and both cap at
`max_rise`. Ground kinds are `none`, `flatten`, `pave` and `till`.

Neighbours are one plot when they are 4-adjacent, carry the same kind and
the same level count, and the kind's `merge` is true; they merge — share
walls and a roof — when that plot also sits above the ground, so a field
is one plot with an axis for its furrows but has no walls to share.
Runs are labelled per axis, so a profile continues across the seam and the
ridge follows the longer run, a tie broken by the first tile's seed. A
face is open when the neighbour across it is not merged, which is where
windows and the door go.

`Column::hit` clips the ground path to the tile footprint, notes the entry
face, and asks whether the ray is under the roof surface where it enters.
If it is, the hit is a wall on that face. If not, the roof crossing is
found by bisection: four rounds at the three outer zooms and five at 1:1.
One row of `assets/blocks.toml` carries all of that:

```toml
[[block]]
name = "house"
size = [2.0, 2.0, 3.0]
levels = [1, 2]
footprint = [[3, 2], [5, 3]]
roof = "gable"
material = "by_biome"
merge = true
ground = "flatten"
windows = [0.5, 1.0]
door = true
light = "window"
```

Adjacency, faces and the settlement generator are in
[structures.md](structures.md).

## Trees

A tree is geometry on the same walk. `Shape` is `cone`, `ellipsoid`,
`dome`, `cactus` or `lsystem` for a declared canopy, plus `branch` and
`cluster`, which only a grown model produces. Every shape is a quadratic
in `z` along the ray's ground path, so a crossing is a closed-form root; a
branch is a capsule tested in metres; wind shear leans a crown by an
offset that is still linear in `z`, so the path stays straight. A crown a
ray enters and does not leave still counts: a segment with no crossing
whose top lies inside the shape is a hit at that top, which is what lets a
ray that came in through a hole go on meeting foliage as it descends.

An L-system species is grown into a `TreeModel` of branch capsules and
leaf clusters, cached in `grid::ModelCache` by species, tile seed, foliage
quantised to nine steps, dead or alive, and detail level, so a season
creeping forward does not regrow every tree every frame. A model lives
four frames past the last frame that wanted it, and the cache holds three
thousand. The walk draws the model wherever a tree is tall enough to read,
which is every zoom but the overview, and what it tests is the model at
the size it is drawn, simplified as [trees.md](trees.md) describes: a
coarse level at 1:4, finer at 1:2 and finest at 1:1.

A leaf cluster is solid: a grown crown's holes are the gaps its grammar
left between the clusters, so sky and ground show through a thin crown
where its branches really are. The habit's `stand_in` volume is the
porous one — `volume::foliage_at` hashes a point on a lattice of three
cells per metre and hits foliage with the species' `leaf_density` as its
probability, and a failed roll passes the ray through — but the walk only
meets a stand-in where a species has no grammar, since every habit's
stand-in is built for the shadow mask alone.

A stand reads as trees rather than as one canopy because each tree is
separated from its neighbours twice over. Every instance takes its own
canopy tint from its tile's seed, a sixth brighter or dimmer and nudged
toward yellow or blue. And where one tree stands in front of another, the
cells of the tree behind along the seam are darkened by a third
(`Renderer::outline_crowns`), so every crown carries an outline against
the crowns behind it. The seam is found in screen space from the hits:
a tree cell whose four-neighbour belongs to a different tree on a nearer
tile is on it.

## The shadow mask

Once per frame `ShadowMask::build` lays a mask over the visible tiles at
four samples per tile — sixteen at the close zooms, where props cast and a
boulder's shadow is shorter than a coarse sample — holding the highest sun ray any occluder blocks
over each ground point; a surface below that height is in shadow. The mask
is skipped at night and when the sun is near overhead. Every occluder is a
disc swept along the sun's ground direction, which is fixed north-west and
is the direction the cloud shadows use. `World::shadow_per_metre` gives
tiles of shadow per metre of height as the cotangent of the sun's
elevation over the 2 m tile, capped at 1.8 so dawn and dusk stretch
shadows without covering the map; a sweep stops at 12 tiles. Terrain casts
where the ground drops more than two tiles' worth of shadow across a tile.
A stack is a disc over its footprint swept from the eaves plus half the
roof peak. A crown is a disc of its stand-in radius, with the species'
leaf density as its opacity, so a thin tree throws a light shadow and a
dead one throws 0.45. A prop is a disc of its own footprint from its
`size` height, at 1:2 and 1:1 only: at the overview it is one glyph and
its shadow would be shorter than the cell it stands in. Anything under a
quarter metre — a patch of moss — casts nothing, and a prop drawn as a
bare glyph over the ground rather than a solid fill is thin, so a tuft of
grass or a stand of reeds stops half the sun the way a sparse crown does.
A prop narrower than a mask sample may fall between four of them and cast
nothing at all: the mask holds what it can hold. Individual branches and clusters are never stamped:
a hundred thousand discs would cost more than the frame.

An occluder never stamps its own footprint, and the light pass looks the
mask up a fifth of a tile along the sun ray from the surface point, so a
sunlit wall stays lit while its away side is dark from its normal.

![night](screenshots/night.png)

## Lighting

The light pass is deferred: the walk writes an unlit colour, a glyph and a
world position per cell, and one pass lights them all. Ambient sky light
is the night floor `[0.15, 0.17, 0.32]` lerped toward the day colour by
`World::skylight`, and the day colour itself is clear sky lerped toward
overcast by the cloud cover. Faces get a little of that directionally:
`0.75 + 0.25 * face_k` with `face_k` of 1.0, 0.78 and 0.5 for top, right
and left.

Direct sun is the sun colour times

```
s = face_k * (1 - 0.72 * cloud_shadow) * (1 - 0.6 * cast_shadow)
```

so cloud shadow takes at most 72 percent of the sun and the cast mask at
most 60. Ambient is untouched by either, which is what keeps shadowed
ground readable.

Point lights sum first and are then passed through a knee, so a cluster
of campfires saturates instead of blowing out:

```rust
pub(crate) fn knee(v: f32) -> f32 {
    1.6 * (1.0 - (-v / 1.6).exp())
}
```

Near zero the knee is the identity; it rises monotonically and never
reaches 1.6. Each light falls off as `(1 - d / radius)^falloff` times its
intensity, with height counting half as much as ground distance, and
flickers on its own phase. Radii are metres, so the `torch` row —
`color = [1.0, 0.7, 0.35]`, `radius = 5.0`, `intensity = 1.2`,
`falloff = 2.0`, `flicker_amount = 0.3`, `flicker_rate = 2.5` — reaches
two and a half tiles and flickers hard.

### Fog

The last thing the light pass does to a lit cell is fade it toward the
sky colour by its depth in the fog (#21): `Scene.fog` is the distance,
`fog_factor(depth, distance)` is nothing at the eye, everything at the
distance and the square in between, so the middle distance keeps its
colour and the far field goes to sky where the perspective walk stops
anyway. The depth is the distance from the eye, or in the isometric
view the depth along the view past the screen centre, so it can be
fogged too (the `fog` settings row: perspective views only, always, or
never). The distance is a weather quantity, `World::visibility`: 120 m on
a clear noon, cut by cloud cover and precipitation, halved by full
night, never under 15 m, and scaled by the camera mode — the shoulder
view, a narrow look into the distance, sees half as far again. `Scene.far`
is that distance whatever the row says; it bounds the walk and sizes the
grid.

## Sextant antialiasing

A cell on a boundary between two visibly different surfaces is
supersampled 2 columns by 3 rows and drawn as one glyph in two colours.
Six rays go through the cell; each returns an unlit colour or the sky.

`quantise` takes the first sample as one colour and the sample farthest
from it as the other, in L1 distance over RGB. If the two are within 24
the cell is left alone. Otherwise each sample goes to whichever colour it
is nearer, and the six-bit pattern becomes a glyph: the empty and full
patterns are space and full block, the two column patterns are the half
blocks, and the other sixty map bijectively onto U+1FB00 to U+1FB3B in
the Unicode Symbols for Legacy Computing block.

Antialiasing needs both the setting and a tileset that allows it. The
ASCII tileset sets `antialias = false`, so no sextant is ever emitted
there and the same scene reads as plain printable characters. Crown seams
are the expensive case: below 1:1 crown cells are not supersampled at all,
and at 1:1 a crown against another crown is still skipped, because the
branches and clusters already cut the outline at cell resolution and in a
stand that seam is most of the screen. What is supersampled is the
stand's edge against the sky, the ground or a wall; what separates two
crowns is the seam outline described under Trees.

## Level of detail

Detail keys off rows per metre, not the footprint, so the table lines up
with the zoom scale in [scale.md](scale.md).

| zoom | rpm | roof profiles | trees | window and roof glyphs | crown seams | bisections | detail octaves | wind | cloud layer | prop shadows |
|---|---|---|---|---|---|---|---|---|---|---|
| far 1:8 | 0.75 | flat columns | one-glyph billboard | no | no | 4 | 0 | off | yes | no |
| mid 1:4 | 1.5 | yes | grown models, coarse | no | no | 4 | 1 | on | no | no |
| near 1:2 | 3 | yes | grown models | yes | no | 4 | 2 | on | no | yes |
| close 1:1 | 6 | yes | grown, finer | yes | yes | 5 | 3 | on | no | yes |

Canopy glyph fill is 30 percent of crown cells below 1:1 and 45 percent at
it, and cactus arms appear only at 1:1.
Nothing is placed per level of detail; the block kind, the species and the
adjacency rules produce all of it.

From an eye the table is read twice over: once at the character's depth
for the frame — the profiles, bands, bisections, wind, prop shadows —
and once per tree at its own, so a spruce twelve metres off is its model
at the level its rows fall in (the detail snapped to that level's preset,
so every tree at a level shares one simplification and the model cache
one key) and one seventy metres off is its stand-in cone. Sprites and
props pick their tiers the same way. The cloud layer is drawn from the eye
in every perspective mode, over the sky cells only.

## Once a frame

The walk is bound by latency — dependent loads and float chains — rather
than by instruction count, so whatever it asks for at every sample or
every hit is worked out once a frame wherever the answer is the same.
Every item below gives the same bytes as evaluating in place; the golden
frames hold to the bit.

- `Map::fields` hashes the lattices of the detail and dirt-patch noise
  over the grid's tile range (`noise::FbmCache`), so a field the walk
  samples a hundred thousand times a frame interpolates from memory with
  the arithmetic `fbm` would do. The cloud field under the light pass is
  cached the same way (`World::cloud_shadows_over`).
- The surface colour of a tile is worked out once per surface kind
  (`Renderer::colors`): a hit and the six sub-rays around it share the
  tile, and `surface_color` depends on nothing else. Water keeps its
  depth colour per hit.
- `Scene` carries the frame's daylight and a snow table by whole-degree
  temperature; each tile's `Geo` carries its block's ceiling and whether
  it is on the map, so a sample reads one record; the sprite and prop
  passes and the shore test read tiles from the grid rather than the
  chunk map.
- Rounding is integer conversion (`round_u8`, `ifloor`, `iceil`) rather
  than the library calls, which were a tenth of a frame between them; the
  bucket scan multiplies by the reciprocal of the stroke length against a
  stored squared radius, widened by a few ulps so the exact solver still
  sees every entry it saw.

What did not pay, and why: skipping samples tile by tile or over a 3x3
window, because the ground path drifts about a tile per metre of height,
so a tile is one to three samples of travel at every zoom and the skip
costs what it saves; and testing several samples over a tile as one
segment, because the entries that survive the stroke test must then be
solved sample by sample to keep the walk's answers, which costs more than
the scan it saves. The block ceiling at eight tiles is the coarse level
that works.

## Cost

The only instrument in the code is the snapshot renderer's `frames=N`,
which renders N frames and prints the mean:

```
./target/release/roguemap --snap 168 71 /dev/null \
  fill=1 zoom=2 cx=500 cy=-300 t=3 tod=12 inset=1 frames=20
```

At 168x71 with the inset open, seed 7, on the boreal stand at
`cx=500 cy=-300` — the densest forest in the world — a frame takes about
12.2 ms at 1:8, 14.6 at 1:4, 15.2 at 1:2 and 19.2 at 1:1. From inside
that stand the perspective views cost more: chase 21.6, shoulder 23.0 and
first person 29.5. The gallery's own scenes — the chase view at the
stand's edge, the shoulder view over the village and the first-person
view on the island shore — are COST_CHASE, COST_SHOULDER and
COST_FIRST_PERSON. What bounds a perspective frame is the geometry near
the eye: the walk stops at the fog distance, but halving it from 120 m to
60 m saves under a millisecond in the stand, because most rays end on a
tree long before it, and the trees within the model range (48 m at 168
columns, where a tree's rows per metre fall under 1.5) are what the rays
test. The first-person view is the dearest because its rays start on the
ground among the finest models and its scale is stated four metres out,
so every switch the level-of-detail table has is on. Over the filled
world at 1:4: the origin 13.2, the village 15.8, a village with fields
15.5, the origin in winter 14.5. Open water is the cheapest scene there
is — the ocean at 1:8 is 6.1 — because the walk meets the flat sea at the
first sample under the clamp and there is nothing standing on it. Part of
the cost at the two outer zooms is the inset itself, which draws at 1:1.
The budget is 16 ms a frame (issue #1); only the stand at 1:1 is over it. The event loop polls on a 40 ms tick,
so 25 frames a second is the ceiling either way. The first frame after a
camera move is dearer than the rest, because the trees that came into view
are grown on it.

Where the time goes now: in an open scene it is the walk's samples through
air between the block ceiling and the ground, the terrain samples and the
four gradient taps every hit takes, and the sub-rays of the edge cells,
which are more than half as many as the primary rays. In the stand at 1:1
it is the bucket scan: about a dozen index entries per geometry call, set
by the leaf clusters' own spans rather than by the branches, and the
sub-rays at crown edges, which pass through the gaps to the ground. The
trees are still a smaller share than their count suggests, because the
index rejects a primitive on its distance from the segment's ground stroke
and solves a leaf cluster from the index entry alone. A winter scene at
1:4 is dearer than a summer one because rays pass through bare crowns to
the terrain.
