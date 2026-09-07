# Rendering

There is no tile atlas. Every screen cell casts a ray back into the world
and walks down until it meets something, so the camera turns to any angle
and the terrain has no seams. This page is the pipeline from that walk to
the lit cell.

![lsystem-stand-close](screenshots/lsystem-stand-close.png)

A boreal stand at 1:1. Every trunk, whorl and porous crown in that frame
is geometry met by the walk; nothing is a sprite.

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
orthographic view, and every camera is one until perspective lands. An
orthographic view projects through a basis of three numbers — `a`
columns per tile across the screen, `b` rows per tile of ground depth
toward the camera, and `rpm` rows per metre of height — which are the
scale times the pitch's sine and cosine:

```
sx = a * (x cos - y sin) + ox
sy = b * (x sin + y cos) - z * rpm + oy
```

The isometric mode is `Camera::isometric(zoom)`: the basis from a
footprint preset, `hw sqrt 2`, `hh sqrt 2` and `3 hw / 8`, and the pitch
those imply, `atan(4 sqrt 2 hh / (3 hw))` — 25.24 degrees for the three
4:1 footprints and 43.31 for the far zoom's 2x1, which has always been the
steeper view. `Camera::orthographic(yaw, pitch, scale)` is the general
form the presets are instances of. The basis is computed when a camera is
built or its zoom set, not from the pitch on every call, so the preset's
numbers are exact; the yaw's sine and cosine are cached the same way, so
the heading is set through `set_angle`.

### What the camera owns

`camera.rs` is the one place that maps the world to the screen. A module
that needs a camera quantity asks for it rather than deriving it:

| Method | Gives | Used by |
|---|---|---|
| `project`, `unproject`, `project_tile`, `anchor_at` | a world point on screen and back | sprites, lights, the grid's culling, the view bounds |
| `ray(sx, sy)` | the ray through a cell, `p0 + d * z` | the walk and its sub-rays |
| `forward()`, `right()`, `depth(x, y)`, `tile_depth` | the map-space view axes and the depth sort | face shading, the sprite and prop sort, crown seams |
| `project_vector(run, rise)` | a world displacement in cells | stroke directions for bare branches and furrows |
| `footprint()` | the cells a tile spans | the ground texture lattice, door and window widths, `pan` |
| `rows_per_metre()`, `columns_per_metre()` | the scale | level of detail (`raster::lod_of`), sprite tiers, model detail |
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
12.2 ms at 1:8, 14.6 at 1:4, 15.2 at 1:2 and 19.2 at 1:1. Over the filled
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
