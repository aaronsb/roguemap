# Testing

`make check` is the gate: clippy with warnings as errors, then every test
below. The golden frames are a cargo test (`tests/golden.rs`), so
`make golden-check` and `make check` run the same comparison and the gate
is one command.

## Rendering

- **Golden frames.** `tools/golden.sh` renders a fixed set of headless
  frames as cell dumps. `tests/golden.rs` reads that script's `shot`
  lines, renders the same frames in-process through
  `roguemap::snapshot::render`, and compares each with its reference in
  `tests/golden/<name>.frame`: width and height as u32 little-endian, then
  per cell a u32 codepoint and the foreground and background bytes. The
  references are committed (about half a megabyte for eleven 120x40
  frames). The comparison scores a frame by the percentage of identical
  cells, the percentage with the same glyph, and the mean six-channel
  colour distance; it passes at `GOLDEN_MIN_IDENTICAL` (default 98) and
  `GOLDEN_MAX_DISTANCE` (default 2.0), or with every cell identical under
  `GOLDEN_STRICT=1`. Every run prints the per-frame scores; a failure
  lists the first ten differing cells with expected and actual glyph and
  colours. `make golden-record` rewrites the references after an
  intentional visual change, and the commit that does so says which
  frames changed and why. The comparator has its own tests: identical
  frames score 100 and 0, one changed cell in 4800 scores as expected,
  and a sprite shifted one cell is reported by its differing cells.
- **Camera.** Project then unproject returns the input at every zoom and
  several angles; `screen_dir_to_map` gives the eight compass steps;
  `tile_depth` orders a nearer tile above a farther one at every angle.
- **Scale** (ADR-004). Each zoom's rows and columns per metre are an exact
  halving of the next (0.75, 1.5, 3, 6 rows and 1.41, 2.83, 5.66, 11.31
  columns), so a 2 m person is 1.5, 3, 6 and 12 rows; heights project
  through rows per metre and unproject back, and no height moves a point
  sideways; one keypress is one tile at every zoom and that tile is one
  footprint on screen. The art tiers a sprite picks are the ones nearest
  those rows. `relief` turns the generator's field into metres: zero at
  the shoreline, the sea bed at `FLOOR`, a valley floor a metre or two
  over the water and a range at `RELIEF`, rising all the way and inverted
  by `relief_fraction`; generated heights stay inside that band and a
  tile's height is the field floored to whole metres.
- **Asset units.** Every distance in the tables is metres: the person is
  2 m, a house level 3 m, an oak 18 m, the campfire's light reaches 8 m,
  and every size, crown, roof rise, reach, sight and light radius sits in
  a plausible metre range.
- **Ray walk.** The walk marches the continuous height field. On a flat
  field it hits the top at the field height for every cell, exactly at the
  overview zooms and within the sub-tile relief up close; a single raised
  tile is a peak whose front slopes are cliff hits with the face on the
  screen side the angle predicts, and just below the apex the walk reaches
  the raised tile; an island's far edge returns sky, its side is a cliff
  down to sea level, and below the waterline the plinth is a wall hit
  until height zero, with sky beneath. These run on synthetic maps
  (`Map::synthetic`, `Tile::flat`, test-only) whose tiles come from a
  closure instead of the noise fields, with the height grid built for the
  view as `draw` builds it.
- **Geometry.** A stack on a flat plain is met as a roof over its tile
  centre between the eaves and the peak, and as a wall at its near corner
  below the eaves, at zooms 2, 4 and 6; a pine is met on its flank facing
  the camera half way up, and ground four tiles from its trunk is ground;
  at the overview the column still stands but no volumes are built.
- **Roof profiles and runs** (`src/blocks.rs`). A gable is flat at the
  eaves and peaks at the ridge, continues across a merged run without a
  step and caps at `max_rise`; a hip rises from every edge; flat stays
  flat. Runs are labelled per axis with the longer run carrying the ridge
  and a tie decided by the first tile's seed; merging needs the same kind
  and levels and a kind that merges; the door goes toward a road, else
  round the open faces by seed. A column meets a vertical ray on its ridge
  and slope, a path coming in under the eaves as a wall on the face
  entered, and a path coming in above as the roof crossing.
- **Volumes** (`src/volume.rs`). A vertical ray meets an ellipsoid's top
  and the trunk below it and nothing beyond the radius; a slanting ray
  finds the nearer crossing in its segment on the surface; cones and
  domes taper, cacti have caps and arms; wind shear leans the crown and
  not the trunk.
- **Shadow mask** (`src/shadow.rs`). A single tower at 15:00 shades the
  ground down-sun for its height times the per-unit length (half a tile
  per metre), not beyond, not on the sun side and not sideways; a point
  above the ray is lit and the roof is not in its own shadow; at night
  there is no mask.
- **Antialiasing.** The sextant map is a bijection onto U+1FB00..U+1FB3B
  plus the two half blocks; quantising six identical colours yields no
  glyph; two colours split three and three yield the expected pattern.
- **Lighting.** The knee is monotonic and bounded; a point light at
  distance zero gives its intensity and at its radius gives zero; ambient
  at midnight is the night floor.
- **Overlays.** The cloud sample for a screen cell (`CloudView`) moves by
  C/(C-H) tiles per tile of camera pan; precipitation kind follows the
  temperature at the screen centre.

## Interaction

- `World::try_move` refuses water and the island edge and moves on land.
- `World::light_campfire` refuses water and places a light on land. The
  campfire is a placed light and counts at any hour; the lit-window light
  a building discovers while drawing joins the frame light count at night
  and not at noon.
- Weather: a storm preset raises precipitation; snowpack grows below
  freezing and melts above; wetness dries faster in sun.
- Settings: each row cycles through all values and wraps;
  `Settings::apply` pushes every row into the objects it governs; presets
  and rows agree in length.
- Traversal: `Camera::walk_step` makes a screen-space step at 45 degrees a
  diagonal map step and a map-axes step a cardinal one.
- World map: `WorldMap::teleport` lands on the nearest land tile to the
  cursor.

## Data

- Every table loads from the embedded assets with no error.
- Every cross-reference resolves: biome species and materials; prop art,
  covers, terrains and lights; block materials, terrains and lights;
  creature art, terrains, homes and lights; tileset art.
- Every placeable row has a non-empty description and a category.
- Numeric ranges: densities and fractions in 0..1, radii and reaches
  non-negative, colours in range, seasonal tables have four entries.
- Geometry rows: sizes are positive in every dimension and required;
  volume overrides are positive and shapes named; block levels rise and
  stay within `max_levels`, window bands are rising pairs within a level
  or empty, the window pitch is positive, chance is a percentage, and
  `level_height` defaults to the size's height. Species volumes come from
  their size by shape: half the spread is the radius, crown and trunk make
  the height, a bush has no trunk.
- Weighted lists are non-empty wherever a biome's tree density is
  positive, and every roll picks a listed species.
- Export then reload gives identical resolved tables.
- Sprite invariants over every zoom, form and art: four variants, rows one
  width, centre inside the width, base rows within the row count.
- Map: a tile is the same whether generated bounded or unbounded and
  regardless of chunk order; the chunk ceiling bounds every tile with its
  stack and crown; a placed stack overrides the generator, lifts the
  ceiling and is applied when its chunk is generated later; climate
  classification covers each Köppen class at a known temperature and
  precipitation.

## Conventions

- Tests live beside the code they test in `#[cfg(test)]` modules, except
  the golden test, which lives in `tests/golden.rs` and uses the library
  as a binary would. The headless snapshot (`src/snapshot.rs`) is library
  code for that reason; the game binary is a thin wrapper over it.
- A test names the property it checks, not the function it calls.
- Where the code makes a property awkward to reach, a small test-only
  helper (`Map::synthetic`, `Tile::flat`, `World::set_wetness`) or a
  named `pub(crate)` piece (`lighting::knee`, `overlay::CloudView`) is
  preferred to restructuring modules.
- A change that alters pixels re-records the golden frames in the same
  commit and says which frames changed and why.
