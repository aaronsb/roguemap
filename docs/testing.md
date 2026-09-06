# Testing

`make check` is the gate: clippy with warnings as errors, then every test
below. `make golden-check` is folded into `cargo test` as a rendering test
so the gate is one command.

## Rendering

- **Golden frames.** `tools/golden.sh` renders a fixed set of headless
  frames. A test renders the same set in-process and compares a hash of
  each frame against `tests/golden/<name>.sha256`. The hashes are small
  and committed; the frames are not. `make golden-record` rewrites the
  hashes after an intentional visual change, and the commit that does so
  says why.
- **Camera.** Project then unproject returns the input at every zoom and
  several angles; `screen_dir_to_map` gives the eight compass steps;
  `tile_depth` orders a nearer tile above a farther one at every angle.
- **Ray walk.** On a flat map the walk hits the top at the tile height for
  every cell; on a single raised tile the cells below its front edge are
  wall hits with the face on the screen side the angle predicts; an
  island's far edge returns sky and its near edge returns the plinth.
- **Antialiasing.** The sextant map is a bijection onto U+1FB00..U+1FB3B
  plus the two half blocks; quantising six identical colours yields no
  glyph; two colours split three and three yield the expected pattern.
- **Lighting.** The knee is monotonic and bounded; a point light at
  distance zero gives its intensity and at its radius gives zero; ambient
  at midnight is the night floor.
- **Overlays.** The cloud sample for a screen cell moves by C/(C-H) tiles
  per tile of camera pan; precipitation kind follows the temperature at
  the screen centre.

## Interaction

- `World::try_move` refuses water and the island edge and moves on land.
- `World::light_campfire` refuses water; the light appears in the frame
  light count at night and not at noon.
- Weather: a storm preset raises precipitation; snowpack grows below
  freezing and melts above; wetness dries faster in sun.
- Settings: each row cycles through all values and wraps; `apply` pushes
  every row into the objects it governs; presets and rows agree in length.
- Traversal: a screen-space step at 45 degrees is a diagonal map step; a
  map-axes step is a cardinal one.
- World map: teleport lands on the nearest land tile to the cursor.

## Data

- Every table loads from the embedded assets with no error.
- Every cross-reference resolves: biome species, materials and buildings;
  prop covers and terrains; asset lights; creature terrains.
- Every placeable row has a non-empty description and a category.
- Numeric ranges: densities and fractions in 0..1, radii and reaches
  non-negative, colours in range, seasonal tables have four entries.
- Weighted lists are non-empty wherever a biome's tree density is
  positive.
- Export then reload gives identical resolved tables.
- Sprite invariants over every zoom, form and art: four variants, rows one
  width, centre inside the width, base rows within the row count.
- Map: a tile is the same whether generated bounded or unbounded and
  regardless of chunk order; the chunk ceiling bounds every tile; climate
  classification covers each Köppen class at a known temperature and
  precipitation.

## Conventions

- Tests live beside the code they test in `#[cfg(test)]` modules, except
  the golden test, which lives in `tests/golden.rs` and uses the library
  as a binary would.
- A test names the property it checks, not the function it calls.
- A change that alters pixels re-records the golden hashes in the same
  commit and says which frames changed and why.
