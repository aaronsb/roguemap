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

## The ray walk

`Camera::project` maps a world point to a screen cell —
`sx = a * (x cos - y sin) + ox` and
`sy = b * (x sin + y cos) - z * rows_per_metre + oy`.

Invert that at `z = 0` to get a ground point `p0`, and the ray is
parameterised by height rather than by distance: a metre of height moves
the ground point by `d = (sin * rpm / b, cos * rpm / b)` tiles, which is
exactly the drift that keeps the screen position fixed, so the path is
`p(z) = p0 + d * z`.

The walk starts at the highest thing that could be met — the grid's
ceiling, capped at `TOP_CAP`, which is the top of the world plus 40 metres
for a crown or a roof — and steps down. The step is `(rpm / 2).ceil()`
samples per metre, about two screen rows, which is fine enough that a step
cannot cross a terrace. High above the coarse 8-tile ceiling grid the walk
jumps straight down to that ceiling, so open sky costs nothing.

At each sample the walk tests the geometry over the segment just
descended, then the field itself: the bilinear sample between tile centres
plus `Map::detail`. The first crossing found is the nearest to the camera,
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

Neighbours merge when they are 4-adjacent, carry the same kind and the
same level count, sit above the ground, and the kind's `merge` is true.
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
thousand. What the walk tests is the model at the size it is drawn,
simplified as [trees.md](trees.md) describes.

Crowns are porous. `volume::foliage_at` hashes a point on a lattice of
three cells per metre and hits foliage with the species' `leaf_density` as
its probability; a failed roll passes the ray through, so a sparse crown
shows flecks of what is behind it.

## The shadow mask

Once per frame `ShadowMask::build` lays a mask over the visible tiles at
four samples per tile, holding the highest sun ray any occluder blocks
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
dead one throws 0.45. Individual branches and clusters are never stamped:
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
and at 1:1, once trees are grown geometry, a crown against another crown
is still skipped, because the branches already cut the outline at cell
resolution and in a stand that seam is most of the screen. What is
supersampled is the stand's edge against the sky, the ground or a wall.

## Level of detail

Detail keys off rows per metre, not the footprint, so the table lines up
with the zoom scale in [scale.md](scale.md).

| zoom | rpm | roof profiles | trees | window and roof glyphs | crown seams | bisections | detail octaves |
|---|---|---|---|---|---|---|---|
| far 1:8 | 0.75 | flat columns | one-glyph billboard | no | no | 4 | 0 |
| mid 1:4 | 1.5 | yes | stand-in volumes | no | no | 4 | 1 |
| near 1:2 | 3 | yes | grown models | yes | no | 4 | 2 |
| close 1:1 | 6 | yes | grown, finer | yes | yes | 5 | 3 |

Canopy glyph fill is 30 percent of crown cells below 1:1 and 45 percent at
it, cactus arms appear only at 1:1, and wind is off at the overview.
Nothing is placed per level of detail; the block kind, the species and the
adjacency rules produce all of it.

## Cost

The only instrument in the code is the snapshot renderer's `frames=N`,
which renders N frames and prints the mean:

```
./target/release/roguemap --snap 168 71 /dev/null \
  fill=1 zoom=2 cx=500 cy=-300 t=3 tod=12 inset=1 frames=20
```

At 168x71 with the inset open, seed 7, on the boreal stand at
`cx=500 cy=-300` — the densest forest in the world — a frame takes about
15.0 ms at 1:8, 14.6 at 1:4, 17.4 at 1:2 and 19.0 at 1:1. Over the filled
world at the origin: 12.1, 14.1, 15.1 and 17.8. Part of the cost at the
two outer zooms is the inset itself, which draws at 1:1. The event loop
polls on a 40 ms tick, so the budget is 25 frames a second and every
measurement above sits inside it. The first frame after a camera move is
dearer than the rest, because the trees that came into view are grown on
it.
