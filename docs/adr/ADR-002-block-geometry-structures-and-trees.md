# ADR-002: Block geometry for structures and trees

Status: Accepted (implemented 2026-09-06; see Implementation notes)
Date: 2026-09-06
Deciders: @aaronsb, @claude

## Context

Buildings and trees are billboards. `Map::tile` sets `building: Option<u8>`
(settlement field over 0.64 and a 14% roll) and `tree: Option<u8>` plus
`species`; `sprite_pass` (`src/sprites.rs`) sorts them by depth and stamps
rows from `src/sprite.rs` with a depth test against the terrain
(`GCell.depth = p . f`, the ground point on the camera-forward unit vector
`f`; a billboard's depth is its tile's front corner). A billboard is not
occluded correctly by terrain behind its base, has one view angle, and two
crowns never merge. `docs/structures.md` sets the target: geometry the walk
finds.

`Renderer::ray` (`src/raster.rs`) today: from `top = min(Map::ceiling over
the segment's chunks, MAX_Z)` it samples `z` downward in `1/steps` units
(`steps = ceil(2 / b)`: 2 per unit at 2x1 and 3x1, else 1), unprojects each
sample to a ground point, fetches the tile with `Map::get`, and returns a
top hit when `draw_z == z` or a side hit when `draw_z > z` (a slab test
picks the axis face, mapped to `FACE_RIGHT`/`FACE_LEFT` by camera angle).

Where `docs/structures.md` and the code disagree:

- "At the overview a stack is its roof colour on a taller column, which the
  existing renderer already draws": nothing above `draw_z` exists in the
  walk, and `Chunk.max_z`, which bounds `top`, knows only terrain.
- `faces` as "glyph rows per level" would need art per tier; this ADR uses
  band parameters so one row per block kind serves every zoom.
- Trees "stay a one-glyph column" at the overview: a tiny billboard; kept.

Measured 2026-09-06 (release, 168x71, seed 7, `fill=1`, `frames=40`):
10.2-10.6 ms at zooms 0-4, 7.4 ms at zoom 6, so the brief's "current 10 ms"
is already slightly exceeded at wide zooms. `perf` puts `Map::get`, the
SipHash `HashMap<(i32,i32)>` lookup behind it and `Map::ceiling` at 36% of
the frame, `Renderer::ray` 14%, `fbm` 14%, `roundf`/`floorf` 13%: the
walk's cost is the chunk lookup, not the geometry.

## Decision

### Data

`Tile` gains `stack: Option<Stack>`, `Stack { kind: u8, levels: u8 }`, and
drops `building`. `assets/blocks.toml` (`[[block]]`; types per ADR-001):

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | str | required | house, tower, barn, road, field, wall, bridge |
| `level_height` | f32 | 2.0 | height units per level; 0 for ground kinds |
| `roof` | none, flat, gable, hip | flat | profile above the column top |
| `pitch` | f32 | 1.5 | rise in units per tile of horizontal run |
| `max_rise` | f32 | 1.5 | cap on the profile height |
| `material` | `"by_biome"` or ref | by_biome | colours from `materials.toml` |
| `merge` | bool | true | same-kind 4-neighbours share roof and walls |
| `ground` | none, flatten, pave, till | flatten | what the tile top becomes |
| `windows` | [f32; 2] or [] | [0.45, 0.8] | band within a level for windows |
| `window_pitch` | f32 | 0.5 | tiles between window centres |
| `door` | bool | true | one door at ground level on an open face |
| `light` | ref(lights) | none | pushed per tile at night, as today |
| `max_levels` | u8 | 3 | validation and editor cap |
| `deck` | bool | false | bridge: top at the bank height over water |

`species.toml` gains `shape cone|ellipsoid|dome|cactus` (default by form),
`radius` (tiles), `height`, `trunk` (units), `trunk_radius` (tiles), with
defaults `0.55s, 2.8s, 0.7s` (cone), `0.6s, 2.4s, 1.0s` (ellipsoid),
`0.45s, 1.0s, 0` (dome), `0.18s, 3.0s, 0` (cactus), `trunk_radius 0.12s`,
`s = scale`. Variant `v` scales radius and height by `0.85, 0.95, 1.05,
1.15`; the centre is jittered 0.15 tiles by `tile.seed`. `Map::tile` sets
`stack = Some(house, 1-2 levels)` where it set `building`; roads, fields
and towns wait for the settlement system. `Map::set_stack(x, y,
Option<Stack>)` records an override consulted by `get` and refreshes the
chunk's `max_z`, which becomes `max(draw_z + levels * level_height +
max_rise, draw_z + trunk + height)`; `ray` caps `top` at `MAX_Z + 8`.

### FrameGrid replaces HeightGrid

Built once per frame over `visible_bounds` (already `MAX_Z + 24` units and
a 16-column margin; canopy radius never exceeds 1.5 tiles), at every zoom.
Per tile: `draw_z: i8`, `hf: f32` (set to `draw_z` under a `flatten`
stack), `terrain`, `biome`, `temp`, `seed`, `stack`, `run_x, run_y: (u8
len, u8 index)` for roof profiles, `top: f32` (highest solid above the
tile: column plus rise, or any canopy over it), `vol: (u32 start, u8 len)`
into a per-frame `Vec<Volume>`. A tree registers its volume in every tile
its footprint box touches (at most 3x3). `ray` reads only the grid;
`Map::get` runs only while building it.

### The walk

Per screen cell, walking `z` downward with previous sample `z_prev`
(initially `top`), ground path `p(z) = p0 + d z`, `d = (sin a, cos a) / b`
(from `Camera::unproject`):

1. `cell = grid.at(floor p(z))`; skip if none.
2. If `z <= cell.top` and `z > cell.draw_z`, test the segment `[z, z_prev]`
   (not just the point) against:
   a. **Volumes** in `cell.vol`. Each shape is a quadratic in `z` along
      the ray: ellipsoid `|p(z)-c|^2/R^2 + (z-cz)^2/(H/2)^2 = 1`; cone
      `|p(z)-c|^2 = (R (1-(z-h0)/H))^2` on `[h0, h0+H]`; dome, an
      ellipsoid clipped to `z >= h0`; trunks and cactus, vertical cylinders
      `|p(z)-c|^2 = r^2` on a height range, arms as two offset cylinders
      plus horizontal links. Keep the largest root inside the segment.
   b. **Structure** if `cell.stack.levels > 0`. Slab-intersect the ground
      path with the tile square for the range `[z_in, z_out]` where `p(z)`
      is inside the footprint, clipped to the segment. With `S(z) = zs +
      r(u(z), v(z))`, `zs = draw_z + levels * level_height`: if `z_out >
      S(z_out)` and `z_in <= S(z_in)` the ray crosses the roof; bisect four
      times (five at 16x4) for `z*`, a **roof** hit with normal `(-dS/dx,
      -dS/dy, 1)`. If already `z_out <= S(z_out)` the ray came in through
      the side: a **wall** hit at `z* = z_out`, axis face = the slab
      entered, mapped to a screen side as terrain cliffs are today, `below
      = ceil(S - z*)` for the row within the face.
   The highest `z*` among the candidates is the hit.
3. Otherwise the terrain test as today.

Because the test covers the segment, thin features (a gable ridge, a
sapling) cannot fall between samples. `Hit` gains `kind: Terrain | Wall |
Roof | Canopy | Trunk`, `z: f32`, `normal` and the volume or block index;
`ids` gets the kind in bits 5-7 so roof/wall and canopy/ground seams are
supersampled.

### Roof profiles

With `n_a, k_a` the along-ridge run length and this tile's index in it,
`n_c, k_c` the same across the ridge, and `u, v` the position within the
tile: `U = (k_a + u) / n_a`, `V = (k_c + v) / n_c`, edge distances in tiles
`d_a = n_a min(U, 1-U)`, `d_c = n_c min(V, 1-V)`.

- flat: `r = 0`
- gable: `r = min(pitch d_c, max_rise)`, ridge along the longer run
- hip: `r = min(pitch min(d_c, d_a), max_rise)`

Gable ends and eaves need no special case: a wall is the column under
`S`, so an end face rises to the ridge as a triangle, long sides stop at `zs`.

### Adjacency

Tiles merge when 4-adjacent with the same `kind` and `levels` and `merge =
true`; runs are counted per axis through each tile over merged tiles.
Ridge axis: x if `run_x.len > run_y.len`, y if less, on a tie x when the
run's first tile has an even `seed`; L-shapes give a cross-gable at the
corner, which is acceptable. A face is **open** when the neighbour across
it is not merged with this tile. A taller merged neighbour shows its wall
above a shorter one's roof by itself, since the walk meets the taller
column first. Ground kinds: `pave` colours the top with the material's wall
colour and the dirt texture pair; `till` draws the dirt glyph in
alternating rows at 50%; `flatten` sets `hf = draw_z`; `deck` puts the top
at the highest 4-neighbour land `draw_z`.

### Faces and glyphs

- Wall: albedo `material.wall` (snow lerp as today), face `FACE_RIGHT` /
  `FACE_LEFT` so `light_pass` shades it, glyph `ts.wall[side]` in
  `wall_glyph`. At `hw >= 8`: inside the `windows` band, where the position
  along the face modulo `window_pitch` lies in the middle 0.2 tiles, glyph
  `art.window`, at night in the light's colour with one `window` light per
  tile as today. The door (`art.door`, ground band `[0, 0.5]`, 0.25 tiles
  wide, centred) goes on the open face toward an adjacent `road`, else the
  first open face in `(+y, +x, -y, -x)` rotated by `seed % 4`.
- Roof: albedo `material.roof` scaled by `1 + 0.18 lit daylight`, `lit =
  -(dS/dx + dS/dy)` normalised as in `surface_color`; face `FACE_TOP`;
  glyph `art.roof_fill` in `roof_glyph`, 35% hash density at `hw >= 8`.
- Canopy: albedo the species' seasonal canopy colour (snow lerp) with the
  same normal rule at `hw >= 8`; glyph from the form's pool (`pine_fill`,
  `round_mid[1]`, `cactus`) at 30% (`hw` 4-6) or 45% (`hw >= 8`) hash
  density keyed to the world grid as `shade` does; where the normal's
  screen-horizontal component exceeds 0.7, `pine_l`/`pine_r` or
  `round_mid[0]`/`[2]` keep the set's outline. Trunk: `pal.trunk` /
  `pal.trunk_glyph`. Gust: the centre shifts along the wind by `0.12 gust
  sin(t (0.8 + 1.2 gust) + phase) (z - h0) / H` tiles.

### Depth

Every geometry hit stores `depth = p(z*) . f` and `wz = z*`. Along one
screen ray `p . f` grows with `z`, so this orders geometry among itself and
against billboards, whose depth stays the tile's front corner. Creatures
and props never stand on a tile with `levels > 0` or a tree; the remaining
case, a crown overhanging a neighbour, is genuinely nearer and rightly wins.

### Level of detail

| zoom (hw) | structures | trees |
|---|---|---|
| 0-1 (2-3) | column to `zs`, `r = 0`, material colours, no glyph bands | tiny billboard as today |
| 2-3 (4-6) | profiles on, no window or door glyphs | volumes, flat colour, 30% glyphs |
| 4-5 (8-12) | windows, doors, roof glyphs, 4 bisections | normal shading, trunks, outline glyphs |
| 6 (16) | 5 bisections, door two cells wide | cactus arms, 45% glyphs |

### Cost

Per sample today: unproject (about 3 ns) plus `Map::get` (RefCell borrow,
SipHash lookup, 40-byte `Tile` copy: about 35 ns, the measured 36%). After:
a grid index and one `top` compare, about 4 ns. Added only when `z <=
top`: a structure segment test is two divisions and at most six `S`
evaluations (about 60 ns, once per cell reaching a roof band); each volume
quadratic about 15 ns, 1-3 volumes per envelope sample, 2-4 such samples
per cell in a dense stand. Grid build: at most about 8,000 tiles at 2x1,
about 0.3 ms. Estimate: 3-4 ms removed, 1-2 ms added in the worst forest
or town view, so every golden view lands under 10 ms at 168x71. The gate
is the measurement; the knobs in order are bisection count, volume tests
only at `hw >= 6`, and skipping AA supersampling on canopy seams.

## Implementation notes

Built as decided, with these departures, each for a reason found on the
way; `docs/structures.md` describes what stands:

- The walk marches the continuous height field (this landed after the ADR
  was written), so there are no integer columns: a stack's base is the
  tile's smooth height, walls run down into the ground and `flatten` is a
  level pad at that height rather than `draw_z`. `HeightGrid` was extended
  into the per-frame grid instead of a new `FrameGrid`.
- Sizes are metres with a 2 m tile (owner's decision during the build):
  `size = [w, d, h]` on every placeable row, `level_height` defaulting to
  the block's height, `pitch` as metres per metre, species volumes derived
  from `size` by shape with `radius`/`height`/`trunk`/`trunk_radius` as
  overrides. One unit still draws as one row; ADR-004 will scale rows per
  metre by zoom, so trees are tall at the close zooms today.
- Walls are entered through the footprint edge, so `below` is not needed;
  the wall's face and the position along the merged run give the window
  and door bands. A window is about a cell and a half wide at every zoom
  and the band defaults to the upper half of a level, which is one row of
  windows per level at one row per metre.
- Trunks, normal shading and outline glyphs are on at every zoom with
  volumes (2 and up), not only from zoom 4: crowns are tens of cells wide
  at zoom 2 once sizes are metres, and flat colour loses their form.
- Sextant supersampling of crown seams is skipped below zoom 4 (the third
  cost knob): in a forest at zoom 2 it cost 16 ms of a 25 ms frame. The
  sub-rays of a supersampled cell start no higher than the cell's and its
  neighbours' hits plus four metres. Zoom 2 in the forest went from 24 ms
  to 13 ms; every zoom is under 16 ms at 168x71.
- Tree footprints are registered with a 0.4 tile margin so a walk step
  cannot cross an unregistered volume, and each tile's list is sorted
  tallest first so the walk stops testing once the rest are below its
  segment; the columns of the previous sample's tile (and the corner tiles
  of a diagonal step) are tested too.
- Cast shadows (from `docs/structures.md`, not this ADR) are a height mask:
  each sample holds the highest blocked sun ray, occluders never stamp
  their own footprint, and the lookup is a fifth of a tile along the sun
  ray, so sunlit faces are not shadowed by their own column.
- `Hit.z` is gone; `h` is the surface height, and the kind is in `ids`
  bits 5-7 as decided. Bridges (`deck`) are parsed and validated but not
  drawn; roads, fields and walls have rows but the generator places only
  houses, towers, barns and fields until the settlement system.

## Consequences

### Positive
- Correct occlusion at any angle; crowns merge; buildings scale by
  stacking and adjacency with one table row per kind.
- The walk gets faster in the common case (the chunk lookup leaves it),
  and the existing face and normal rules light the new geometry.

### Negative
- Houses change look (a level is 2 units; today's sprite is 1.6-1.8) and
  every golden frame with a house or tree changes; re-record after review.
- `raster.rs` grows: split into `walk.rs`, `roof.rs`, `volume.rs` after
  the current refactor lands. 16x4 gains the most work (7.4 ms headroom).

### Neutral
- L-systems fit later as cylinder and ellipsoid lists; props and
  creatures stay billboards.

## Alternatives considered

- **Billboards with per-row depth**: no rotation correctness, merging or roofs.
- **3x3 `Map::get` search per sample**: nine hash lookups, ten times today's cost.
- **Per-chunk volume lists only**: avoids the per-frame build but keeps
  `Map::get` in the loop; the grid gives both.
- **Voxelise into the height column**: flat roofs only, no canopy shapes.
- **Signed-distance marching**: far more evaluations than closed-form roots.

## Implementation plan

1. `blocks.toml` and the species volume fields in `assets/` and the loader
   (ADR-001), seed rows house, tower, barn, road, field, wall, bridge.
2. `Stack` in `Tile`, `Map::tile` placement, `Map::set_stack`, chunk
   `max_z` including stacks and trees; extend `ceiling_bounds_every_tile`.
3. `FrameGrid` replacing `HeightGrid`, built every frame; `ray` reads it.
   `make golden-check` byte-identical (no geometry yet). Measure.
4. Structure column with flat roofs in `ray` (wall and roof hits, depth,
   faces); `sprite_pass` stops drawing houses. Record new goldens.
5. Gable and hip profiles; runs and adjacency in the grid build.
6. Window bands, doors, roof glyphs at `hw >= 8`; frame lights per tile.
7. Tree volumes: shapes, quadratic roots, canopy and trunk shading, glyph
   pools, gust; `sprite_pass` draws trees only at `hw <= 3`.
8. LOD gates per the table; `frames=20` on the golden views under 10 ms.
9. Unit tests: profiles at edges, ridge and corners; runs on 1x3, 2x2 and
   L groups; a ridge thinner than a step; quadric roots; depth vs billboard.
10. Update `docs/structures.md` (`faces` to band fields, LOD table).
