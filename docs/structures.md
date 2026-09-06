# Structures as block geometry

Buildings, roads, fields and walls are block geometry on the tile grid,
not sprites. A tile carries a stack: a block kind and a level count.
Stacking a block on a block makes it taller. Placing the same kind on
neighbouring tiles makes one larger building, the way Townscaper works.
Trees are volumes on the same ray walk. The decision is
[ADR-002](adr/ADR-002-block-geometry-structures-and-trees.md); this note
is what is built.

Sizes are metres: a tile is 2 m square and a metre of height draws as the
zoom's rows per metre, 6 at 1:1 down to 0.75 at 1:8 (ADR-004).

## Data

`assets/blocks.toml` names each block kind (`[[block]]`; the placement and
identity properties are in [properties.md](properties.md)):

| Field | Default | Meaning |
|---|---|---|
| name | required | house, tower, barn, road, field, wall |
| size | required | `[w, d, h]` metres of one tile of the kind at one level: 2 by 2 by the level height |
| levels | [1, 1] | levels the generator gives a stack, `[min, max]`; `[0, 0]` for ground kinds |
| footprint | [[1, 1], [1, 1]] | smallest and largest ground one covers, in tiles: house 3x2 to 5x3, tower 2x2, barn 4x3 to 6x3, field 6x6 to 12x8 |
| level_height | size's h | metres per level |
| roof | flat | none, flat, gable, hip; gables ridge along the longer run |
| pitch | 1.0 | metres of rise per metre of run from the eaves |
| max_rise | 1.5 | cap on the rise, metres |
| material | by_biome | colours from `materials.toml`: the tile's local material, or a named one |
| merge | true | whether same-kind neighbours share walls and roof |
| ground | flatten | what the tile beneath becomes: none, flatten (a pad at the tile's height), pave, till |
| windows | [0.5, 1.0] | band within a level, as fractions of its height, where windows go; `[]` for none |
| window_pitch | 1.0 | metres between window centres along a face |
| door | true | one door at ground level on an open face |
| light | none | a `lights.toml` row pushed per stack at night |
| max_levels | 3 | validation and editor cap |
| deck | false | reserved for bridges: the top at the bank height over water |
| settle_min, chance | 1.0, 0 | the generator's rule: settlement field a tile needs, and the percentage of qualifying tiles that carry one; chance 0 means placed only by hand |

A tile's structure is `Stack { kind, levels }`, and a building is a
rectangle of tiles all carrying the same one. The world is cut into
settlement plots of sixteen tiles square; each plot holds at most one
building, a pure function of the plot and the seed: the first kind in
table order whose rule passes the settlement field and its own roll, sized
from its `footprint` range, set inside the plot with a tile of margin so
buildings of neighbouring plots never touch and merge, on ground of a
terrain the kind stands on and flat within two metres. Deeper into the
settlement a building has more levels. Its own tiles take the stack and
the ring two tiles around it is cleared of trees, so a house in a wood
stands in a clearing. `Map::set_stack` places or clears one tile by hand
and lifts the chunk ceiling. Roads and walls have no chance and are placed
by hand.

## Geometry

Once per frame the renderer builds a grid over the tiles in view
(`src/grid.rs`): each tile's smooth height, its tile, its stack resolved
into a column, and the tree volumes whose footprint touches it. The ray
walk (`src/raster.rs`) reads only the grid.

A stack is a column over its tile from the ground to the eaves at `base +
levels * level_height`, where `base` is the tile's smooth height. Between
two samples of the walk the segment of the ray is tested against the
column (`src/blocks.rs`): the ground path is clipped to the tile's
footprint; if the ray is under the roof surface where it enters, the hit
is a wall on the face entered, otherwise the roof crossing is found by
bisection. The roof is a height profile above the eaves, so a gable rises
to a ridge and the walk finds the slope at sub-tile resolution. Walls run
down into the ground, so a house on a slope has a taller downhill wall;
`ground = flatten` makes the ground under the tile a level pad at the
tile's height so the door sits at one height.

The profiles, with `d` the distance in tiles to the nearest edge of the
merged run across the ridge (`d_a` along it):

- flat: no rise
- gable: `min(pitch * 2 d, max_rise)`, ridge along the longer run
- hip: `min(pitch * 2 min(d, d_a), max_rise)`

## Adjacency

Tiles merge when 4-adjacent with the same kind and level count, a level
above the ground, and a kind that merges. Runs are counted per axis
through each tile over merged tiles, so the profile continues across the
seam and the end tiles of a run carry the gable ends. The ridge follows the
longer run; on a tie the run's first tile's seed decides; L-shapes give a
cross-gable at the corner. A face is open when the neighbour across it is
not merged with this tile; shared faces are never met by the walk because
the roof is continuous across them. A taller merged neighbour shows its
wall above a shorter one's roof by itself, since the walk meets the taller
column first.

Ground kinds have no column: `pave` colours the top with the material's
wall colour and the dirt texture pair, `till` draws the dirt glyph in
alternating rows.

## Faces and glyphs

- Wall: the material's wall colour, shaded toward or away from the sun by
  the face's normal, with the screen side the face points to so the light
  pass shades it as it does cliffs; the cliff glyph in the wall glyph
  colour. From zoom 4, windows: in the kind's band of each level, where the
  position along the face (counted along the merged run) falls on the
  window pitch, the tileset's `window` glyph, in the light's colour at
  night. The door: the tileset's `door` glyph in the ground half of the
  ground level, centred on the door face, which is the open face toward an
  adjacent road, else the first open face in `(+y, +x, -y, -x)` rotated by
  the tile's seed.
- Canopy: the species' seasonal colour, tinted by the instance, lit by
  the crown's normal toward the sun, lightened toward the tip so a spire
  reads as a spire, and darkened where other crowns stand over the same
  point, which gives a stand its depth.
- Roof: the material's roof colour, snow-covered by the kind's
  `snow_cover`, shaded by the slope's normal so the two sides of a gable
  differ; from zoom 4 the `roof_fill` glyph at 35% density.
- Lit windows: at night every stack in view whose kind names a light
  pushes one light at its tile, scaled by how dark the sky is.

## Trees as volumes

A species names a canopy shape and a size (`species.toml`): `size = [w,
d, h]` is a mature tree's spread and height in metres, and `shape` is
cone (conifers), ellipsoid (broadleaf), dome (scrub), cactus (a column
with two arms at the closest zoom) or lsystem (grown from a growth habit,
docs/lsystem.md, and reading as the habit's `stand_in` volume from far
away), the form's shape by default. The
crown radius is half the spread; the shape says how much of the height is
trunk (a quarter for a cone, four tenths for an ellipsoid, none for a
dome or cactus); a row may give `radius`, `height`, `trunk` and
`trunk_radius` in metres instead. The size class scales a species (small
0.8, mixed 1, large 1.15), the tile's variant scales each tree (0.8 to
1.1), and the tile's seed gives every instance its own height and crown
radius a quarter either way, a canopy an eighth brighter or dimmer and
slightly warmer or cooler, and a chance (the species' `dead_chance`) of
standing dead: a grey-brown snag with no foliage that still casts. The
trunk is jittered within its tile by the tile's seed.

Trees stand apart by their crowns. A species' `spacing` is three quarters
of its crown width unless the row says otherwise, and the biome's own
factor multiplies it (0.4 where crowns interlock, 2 in a savanna). A tile
carries a tree only if it is the highest draw of the tiles within that
spacing, and then only at the biome's `tree_density`: a dense stand keeps
trunks and gaps between its crowns instead of reading as one mass. The
crown itself starts at the species' prune height above the ground, a
fifth of the height for a conifer and a third for a broadleaf, so there is
bare trunk under the canopy.

Each volume is registered on every tile its footprint touches, tallest
first. A ray segment tests the volumes of its sample's tile until their
tops fall below the segment; every shape is a quadratic in `z` along the
ray's ground path, so the crossing is a closed-form root, and the trunk is
a thin cylinder. A gust shears the crown along the wind by height, which
keeps the path linear. So a tree occludes correctly at every angle, crowns
in a dense stand merge into one mass, and the crown's normal shades it
toward the sun. Glyph texture goes on the surface the walk finds: the
form's pool from the tileset (`pine_fill`, `round_mid`, `cactus`) at 30%
density, 45% at the closest zoom, with the set's outline glyphs
(`pine_l`/`pine_r`, `round_mid` ends) where the crown turns away
sideways, and the trunk glyph on the trunk. At the two smallest zooms a
tree stays the one-glyph billboard from `art/tiny`.

## Cast shadows

Once per frame a mask is built in map space over the visible tiles
(`src/shadow.rs`) at four samples per tile, holding the height of the
highest sun ray any occluder blocks over each ground point; a surface
point below that height is in shadow. Every occluder is a disc swept along
the sun's ground direction, the one the cloud shadows use, by its height
times the shadow length per metre of height (`World::shadow_per_metre`:
the cotangent of the sun's elevation over the tile's 2 m, capped at 1.8
tiles per metre as the cloud shadows are capped, so dawn and dusk stretch
shadows without covering the map; sweeps stop at 12 tiles). Terrain casts
where the ground drops more than two tiles' worth of shadow across a tile
along that direction; a stack is a
disc over its footprint swept from the eaves plus half the roof peak; a
canopy a disc of its radius swept from its base. An occluder never stamps
its own footprint, and the light pass looks the mask up a fifth of a tile
along the sun ray from the surface point, so a sunlit wall or the near
side of a crown stays lit while the away side is dark from its normal.

The light pass multiplies direct sun by one minus the mask value at the
surface point, which is the coverage blended and tightened times the
occluder's opacity, sampled bilinearly and tightened at the middle of the
blend, and leaves ambient light alone, so shadowed ground stays readable.
At night there is no sun and the mask is skipped. Terrain and stacks cast
at every zoom; canopies from zoom 2, where trees are volumes. Props do not
cast yet.

## Level of detail

Detail keys off the zoom's rows per metre (ADR-004), not its footprint:

| zoom | rows per metre | structures | trees | shadows |
|---|---|---|---|---|
| far 2x1 (1:8) | 0.75 | column to the eaves, flat, material colours, no glyph bands | one-glyph billboard | terrain, stacks |
| mid 4x1 (1:4) | 1.5 | profiles on, no window or door glyphs, 4 bisections | volumes with normal shading, trunks, outline glyphs, 30% fill; crown seams not supersampled | + canopies |
| near 8x2 (1:2) | 3 | windows, doors, roof glyphs | grown from the habit: branches and leaf clusters | |
| close 16x4 (1:1) | 6 | 5 bisections, door two cells wide | cactus arms, 45% fill, a finer model; crown seams supersampled against sky and ground, not against each other | |

Nothing is placed per detail; the block kind, the species and the
adjacency rules produce all of it. Measured at 168x71 in the snapshot
timing (`frames=20`, seed 7, a boreal stand at `cx=500 cy=-300`, which is
the densest forest in the world, and with the inset view of ADR-004 open),
a frame takes 15.0 ms at 1:8, 14.6 at 1:4, 17.4 at 1:2 and 19.0 at 1:1;
over the filled world at the origin, 12.1, 14.1, 15.1 and 17.8. Against
the same measurement before the trees were grown, the boreal stand is
1.1 ms dearer at 1:8 and 1.8 at 1:4 — both of those are the inset, which
draws 1:1 — 2.1 dearer at 1:2, and 1.1 cheaper at 1:1, where what the
crown seams no longer cost in supersampling more than pays for the
geometry. The first frame after a camera move is dearer than the rest,
because the trees that came into view are grown on it.

## Order of work

1. Done: the asset pass (block table with house, tower, barn, road, field,
   wall; species volumes; sizes in metres).
2. Done: the structure test above the terrain with flat, gable and hip
   profiles; adjacency merging and face glyph bands; trees as volumes;
   cast shadows.
3. Done: the scale pass (ADR-004): heights are metres and project through
   the zoom's rows per metre.
4. Done: L-system species on the walk (docs/lsystem.md): every species
   names a growth habit, the near zooms test its grown branches and leaf
   clusters, and crowns are porous.
5. Next: the settlement system paints towns and roads; the editor paints
   by hand through `Map::set_stack`; bridges (`deck`) and props casting
   short shadows at the closest zooms.

## Volumes are stand-ins

The canopy shapes (cone, ellipsoid, dome, column) are the far-zoom level
of detail. They give a tree its mass and its shadow at a distance. An
evergreen up close is not a smooth cone: it is one straight trunk with
whorls of near-horizontal branches at intervals, each whorl shorter than
the one below, foliage in tiers with gaps between them, the lowest whorls
dead or shed, and a ragged outline. That silhouette comes from the
L-system species (docs/lsystem.md).

Every species names a growth habit, so every tree in the world is grown
from a grammar; the saguaro, which is a column and not a tree, keeps its
own shape. From the near zooms up (three rows per metre and closer) the
walk tests the tree's model: a `Branch` capsule per segment, round in
metres and at any angle, and a `Cluster` ellipsoid per leaf clump. Below
that a tree is the one volume its habit names through `stand_in`, sized
from the species' `size` exactly as before, so the far zooms are
unchanged. The stand-in is built either way: it is what the shadow mask
sweeps, since a hundred thousand discs would cost more than the frame.

What the walk tests is the model at the size it is drawn, not the model
the grammar grew (`TreeModel::simplify`): leaf clusters within a lattice
four rows of height across are merged into one, and branches thinner than
about one column are dropped, because the foliage that grew on them
covers them. A tree with no foliage — bare in winter, or a snag — keeps
every twig, since the twigs are all there is of it. A grown model is
cached per species, seed, foliage (quantised to nine steps), state and
detail level in the renderer (`grid::ModelCache`), so a tree in view is
grown once and not once a frame.

## Porous canopies

A crown is not solid. A ray sample inside a canopy hits foliage with the
species' leaf density as its probability, seeded by position so the holes
stay put from frame to frame, and otherwise passes through and keeps
walking; a segment with no crossing whose top lies inside the crown counts
as a sample too, so a ray that entered through a hole goes on meeting
foliage as it descends. Through a sparse crown you see flecks of what is
behind: a character, a wall, the sky. L-system leaf clusters give the same
result from their real gaps. The leaf density is the habit's
`leaf_density`, 0.85 for the evergreen habits and 0.7 for the broadleaf
ones, and for a species with no habit it is the form's. The shadow mask
uses the leaf density as the crown's opacity, so a thin tree throws a
light shadow.
