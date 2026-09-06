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

A tile's structure is `Stack { kind, levels }`. The generator places the
first kind in table order whose rule passes, with more levels deeper into
the settlement; `Map::set_stack` places or clears one by hand and lifts
the chunk ceiling. Towns and roads wait for the settlement system.

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
- Roof: the material's roof colour, snow-covered by the kind's
  `snow_cover`, shaded by the slope's normal so the two sides of a gable
  differ; from zoom 4 the `roof_fill` glyph at 35% density.
- Lit windows: at night every stack in view whose kind names a light
  pushes one light at its tile, scaled by how dark the sky is.

## Trees as volumes

A species names a canopy shape and a size (`species.toml`): `size = [w,
d, h]` is a mature tree's spread and height in metres, and `shape` is
cone (conifers), ellipsoid (broadleaf), dome (scrub) or cactus (a column
with two arms at the closest zoom), the form's shape by default. The
crown radius is half the spread; the shape says how much of the height is
trunk (a quarter for a cone, four tenths for an ellipsoid, none for a
dome or cactus); a row may give `radius`, `height`, `trunk` and
`trunk_radius` in metres instead. The size class scales a species (small
0.8, mixed 1, large 1.15) and the tile's variant scales each tree (0.8 to
1.1); the trunk is jittered within its tile by the tile's seed.

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
surface point, sampled bilinearly and tightened at the middle of the
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
| near 8x2 (1:2) | 3 | windows, doors, roof glyphs | | |
| close 16x4 (1:1) | 6 | 5 bisections, door two cells wide | cactus arms, 45% fill, crown seams supersampled | |

Nothing is placed per detail; the block kind, the species and the
adjacency rules produce all of it. Measured at 168x71 in the snapshot
timing (`frames=20`, seed 7, the filled world at the origin), every zoom
draws in 11 to 14 ms.

## Order of work

1. Done: the asset pass (block table with house, tower, barn, road, field,
   wall; species volumes; sizes in metres).
2. Done: the structure test above the terrain with flat, gable and hip
   profiles; adjacency merging and face glyph bands; trees as volumes;
   cast shadows.
3. Done: the scale pass (ADR-004): heights are metres and project through
   the zoom's rows per metre.
4. Next: the settlement system paints towns and roads; the editor paints
   by hand through `Map::set_stack`; bridges (`deck`) and props casting
   short shadows at the closest zooms.

## Volumes are stand-ins

The canopy shapes (cone, ellipsoid, dome, column) are the far-zoom level
of detail. They give a tree its mass and its shadow at a distance. An
evergreen up close is not a smooth cone: it is one straight trunk with
whorls of near-horizontal branches at intervals, each whorl shorter than
the one below, foliage in tiers with gaps between them, the lowest whorls
dead or shed, and a ragged outline. That silhouette comes from the
L-system species (docs/lsystem.md), whose branch and leaf volumes replace
the stand-in shape at the near zooms once the adapter lands.

## Porous canopies

A crown is not solid. A ray sample inside a canopy hits foliage with the
species' leaf density as its probability, seeded by position so the holes
stay put from frame to frame, and otherwise passes through and keeps
walking. Through a sparse crown you see flecks of what is behind: a
character, a wall, the sky. L-system leaf clusters give the same result
from their real gaps. The shadow mask uses the leaf density as the crown's
opacity, so a thin tree throws a light shadow.
