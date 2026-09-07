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
  references are committed (about nine tenths of a megabyte for nineteen
  120x40 frames). The comparison scores a frame by the percentage of
  identical cells, the percentage with the same glyph, and the mean six-channel
  colour distance; it passes at `GOLDEN_MIN_IDENTICAL` (default 98) and
  `GOLDEN_MAX_DISTANCE` (default 2.0), or with every cell identical under
  `GOLDEN_STRICT=1`. Every run prints the per-frame scores; a failure
  lists the first ten differing cells with expected and actual glyph and
  colours. `make golden-record` rewrites the references after an
  intentional visual change, and the commit that does so says which
  frames changed and why. The comparator has its own tests: identical
  frames score 100 and 0, one changed cell in 4800 scores as expected,
  and a sprite shifted one cell is reported by its differing cells.
  One frame, `props`, is there for the prop shadows: a close view of a
  boulder field at 15:00, the only shot whose sun is low enough for the
  mask to be built at a zoom where props cast. Three are perspective
  views (ADR-007 stage 2): `chase` at the boreal stand's edge, `shoulder`
  over the village, and `firstperson` on the island's river shore, pitched
  five degrees down. Every isometric frame is unchanged to the bit by
  stage 2, which `GOLDEN_STRICT=1` checks. `stride` (ADR-008) is the
  yardstick at 1:1 with the person 0.3 s into a walk to the right, on
  the third of the large tier's four poses; the frames with the player
  at rest are unchanged to the bit by walking. Two are the table
  straight down (ADR-009): `plan`, the yardstick at 1:1 at 90 degrees,
  and `overhead`, the cloud scene at 1:8 at 90, which is the parallax at
  the plan view.
- **Camera** (ADR-007). The general projection gives the old formulas'
  bits: the footprint-era `project` and `unproject` live in the test
  module and a lattice of 441 points by six heights, at every zoom and
  seven headings, projects and unprojects to exactly the same `f32`s
  through the basis; the walk's ray is the old walk's `p0` and drift to
  the bit and every point along it projects back to its cell. The
  table at the floor tilt is each footprint preset's basis to the bit at
  1:4, 1:2 and 1:1, and the far zoom's rows are exactly half the mid
  zoom's with its rise unchanged; every preset is orthographic, and the
  general `Camera::orthographic` built from a preset's own tilt, relief
  and columns per metre gives the preset's basis back and carries the
  preset, whatever the tilt; a top-down view puts all its rows in ground
  depth and a view along the ground all of them in height. The inset
  camera keeps the heading and the tilt at the other end of the scale;
  `forward`, `right`, `depth` and `project_vector` are the yaw's sine and
  cosine, and a unit of ground
  toward the camera is `b` rows; the footprint is the cells a pan slides
  by, kept with the basis, so a tilted table reports its own and an eye
  the preset it carries, through aiming, flying and a mode switch; an
  orthographic `eye_ray` is the table's own ray in the perspective form,
  `ray`'s ground point down it and `ray`'s drift along it, at tilts from
  30 degrees to 90. Project then unproject returns the input at every
  zoom and several
  angles; `screen_dir_to_map` gives the eight compass steps; `tile_depth`
  orders a nearer tile above a farther one at every angle. Perspective
  (stage 2): the eye's ray through a cell starts at the eye, is a unit
  direction, and every point along it projects back to that cell, in all
  three presets; the height form the walk's geometry takes passes through
  the eye, and a level ray keeps a finite drift under the level guard;
  the target lies the chase distance down the centre ray; the scale is
  the on-axis differential at the character's depth, measured on the
  centre ray, twice the depth gives half the rows, and the preset carried
  is the nearest; the first-person eye sits `EYE_HEIGHT` of the creature's
  height over the ground (1.7 m up a 2 m person), hides the character and
  is put back on them by `follow`; the chase eye is the placement's
  distance behind and above the character's middle along the heading at
  its pitch; the shoulder view puts the figure in the left half and the
  lower third; every mode in `MODES` builds and switching keeps the yaw
  and the aimed point; zoom halves and doubles the chase distance within
  its range, the pitch clamps to its range, the field-of-view override
  carries across a mode switch and `None` restores the preset's; the
  compass snap turns to the next diagonal heading. Walking (ADR-008): a
  screen-space heading is the unit ground vector under the key — the map
  diagonals at the compass view, away from the eye for `w` in a chase
  view — and a map-axes heading the axis
  the key names; facing is the screen-space sign of the heading, right
  or left, and none straight toward or away from the camera. The camera
  settles on the figure: inside the middle third of the screen `follow`
  moves nothing, outside it the offset closes by whole cells over several
  ticks until the figure is back inside and then rests; a chase view
  closes `EASE` of the gap to the character each tick and converges, and
  the first-person eye snaps in one.
- **The tilt** (ADR-009). At every tilt from 30 degrees to 90 and every
  zoom, a metre of ground depth toward the camera is `sin(tilt)` of a
  metre across in the cell's own pixels, a metre of height is `relief`
  times `cos(tilt)` of it, a row of ground is `2 / sin(tilt)` columns,
  and the detail scale is the floor tilt's number, so level of detail,
  the sprite tier and the walk's step count do not move with the table.
  Straight down the rise is exactly zero, the ray's drift is zero, a
  column of world projects to one point and the detail scale is still
  six rows at 1:1. The tilt clamps at both ends, survives a zoom step and
  a mode round trip, and the cloud sample moves by C/(C − H) per tile of
  pan at both ends of the range.
- **Coupling** (ADR-009). Under `body-turns` a walk key is the view's own
  heading and a walk in progress turns with the view; under `view-only`
  it is the body's, which the view turning leaves alone, and a diagonal
  key turns the body to the direction it names and walks it; under
  `map axes` both couplings walk the way the key names. A turn key spends
  the creature's `turn` in degrees a second and turns the body without
  moving it, a tap turns the grace's worth, and `w` held with `a` walks a
  half turn in a second that lands the figure a diameter to the left, a
  curve of radius `speed / turn`. The rows of the keys down pace a body
  and the columns turn it, two opposite keys cancelling as they do on
  screen.
- **The free camera** (ADR-009). Entering from the table puts the eye
  thirty metres from the point under the screen centre, above the table's
  plane and looking down the tilt, and entering from an eye takes the eye
  it found; the character is drawn and does not move; flying ten metres
  forward moves the eye ten metres and descends when the view looks down;
  `follow` leaves it where it is however far the character walks; and
  leaving it is a table again at the tilt it lent, aimed where the eye
  looked: entered and left with nothing between, the table's offset and
  focus height are the ones it had, and after a pitch and a flight the
  point aimed at is the ground thirty metres down the view rather than
  the eye's own height. Two fly keys are a unit diagonal, a diagonal key
  the two it stands for, and two opposite keys nothing to fly toward.
  The game binary's own tests, in a module beside it, drive the doors:
  entering by `V` or by the popover's `camera` row ends the walk, keeps
  the mode to return to and leaves no walk for a later tick to log, and
  two fly keys move the eye one key's pace.
- **Scale** (ADR-004). Each zoom's rows and columns per metre are an exact
  halving of the next (0.75, 1.5, 3, 6 rows and 1.41, 2.83, 5.66, 11.31
  columns), so a 2 m person is 1.5, 3, 6 and 12 rows; heights project
  through rows per metre and unproject back, and no height moves a point
  sideways. The art tiers a sprite picks are the ones nearest
  those rows. `relief` turns the generator's field into metres: zero at
  the shoreline, the sea bed at `FLOOR`, a valley floor a metre or two
  over the water and a range at `RELIEF`, rising all the way and inverted
  by `relief_fraction`; generated heights stay inside that band and a
  tile's height is the field floored to whole metres.
- **Inset view** (ADR-004, ADR-005). The bias rule sends each main zoom to
  the other end of the scale: 1:2, 1:4 and 1:8 all put the inset at 1:1
  and 1:1 puts it at 1:8, so the two views never share a level. The inset
  follows the player and not the camera it hangs off — panning the main
  view leaves it where it was, moving the player does not — and its title
  is the row's plus the ratio it draws at. The `Inset` settings row places
  it in each of the four corners and turns it off, and the show rule keeps
  it off a screen under a hundred columns wide. Headless, a 168x71 frame
  carries the inset and its title, an 80x25 one does not, and the status
  bar names the zoom by ratio and name.
- **Settlements.** Every generated building is a rectangle within its
  kind's footprint range and its level range, inside its own plot with a
  tile of margin; every tile of one carries its stack, its ground is
  cleared of trees, and the ring around it is clear, so two buildings
  never merge.
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
  below the eaves, at zooms 2, 4 and 6; a live pine, grown from its habit
  at every one of those zooms, is met down its leader a metre and a half
  under its top, on its flank half way up wherever the tiers leave no gap,
  and ground four tiles from its trunk is ground; at the overview the
  column still stands but no volumes are built.
- **Level of detail.** A model is grown from one and a half rows per
  metre up and not at the overview; the detail level counts the zooms a
  model is grown at, and its lattice and twig cut halve with each zoom in.
  An oak at the mid zoom is its model with fewer primitives than at the
  near zoom, its stand-in beside them for the shadow mask. From a
  first-person eye a pine twelve metres off is grown and one seventy-five
  metres off is its stand-in cone alone, and the walk meets each; the
  isometric mid zoom grows both.
- **Grown models** (`src/lsystem/mod.rs`). A merged cluster is no smaller
  than its biggest member and no wider on any axis than the box its
  members fill or the screen they would cover side by side, and a cluster
  on its own comes back as it was; the bole is a chain from the foot that
  thins upward, survives a cut above every branch and joins into one
  capsule under a coarse lattice; a bare tree keeps twigs down to
  `BARE_TWIG` of the cut a leafy one drops them at; `leaf_flat` halves the
  height of a cluster on a level branch and leaves an upright one round.
  `dead_whorls` marks the lowest whorls of a deterministic conifer dead
  without removing their wood, takes their clusters and the leader's
  below them, leaves the whorl above untouched, never sheds the last
  whorl and does nothing to a snag; a fraction sheds one whorl or two
  from the seed and the same seed always the same. A spruce grown from
  the embedded set carries deadwood in the bottom of its crown at every
  seed, none of its foliage below it, fewer clusters than the same tree
  with `dead_whorls = 0`, and stubs still marked dead once simplified.
- **Crown seams.** With two pines on the camera's diagonal, every cell of
  the far crown beside a cell of the near one is darkened by `CROWN_SEAM`
  and a cell inside the far crown keeps its colour.
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
  there is no mask. A placed prop shades the ground down-sun by its own
  height times that length and no further, nothing on the sun side, and a
  boulder's shadow stops where a thing twice as tall still reaches; at the
  overview a prop casts nothing. Both run on a plain of sand under a cover
  no prop asks for, so the only occluders are the ones the test places.
- **Once a frame.** The cached noise lattice (`noise::FbmCache`) gives
  `fbm`'s value to the bit inside and outside its rectangle; `Map::fields`
  gives `Map::detail` and `Map::patch` to the bit, and no detail with no
  octaves or on a fixture; the cloud shadow over a range is `cloud_shadow`
  to the bit; `round_u8` is `round() as u8` for every channel value,
  including the ones just under a half; `iceil` is `ceil`.
- **Antialiasing.** The sextant map is a bijection onto U+1FB00..U+1FB3B
  plus the two half blocks; quantising six identical colours yields no
  glyph; two colours split three and three yield the expected pattern.
- **Lighting.** The knee is monotonic and bounded; a point light at
  distance zero gives its intensity and at its radius gives zero; ambient
  at midnight is the night floor. Fog (#21) is nothing at the eye,
  everything at the distance and rises through it, keeping most of the
  colour half way; a scene carries it for a perspective eye and not for
  the isometric view unless asked, the shoulder view sees half as far
  again, the depth from the eye is the distance to the point, and
  visibility closes in with cloud, rain and night, never under fifteen
  metres.
- **Overlays.** The cloud sample for a screen cell (`Camera::cloud_view`)
  moves by C/(C-H) tiles per tile of camera pan; precipitation kind
  follows the temperature at the screen centre.

## Interaction

- **The keys down** (ADR-008). Two perpendicular direction keys held at
  once are one unit diagonal heading, half way between what each means
  alone, under both traversal settings and from an eye as well as the
  isometric view; a diagonal key is exactly the two keys it stands for;
  two opposite keys are no heading at all, and so is nothing held. The
  held set lifts a key on its release and keeps the others; where
  releases are not reported each key holds its own lease instead, two
  keys pressed within the grace are both live, a repeat renews one while
  the other's runs out, and a lease nothing renews ends. `y u b n` are
  the four diagonals and their capitals the runs; a shifted letter finds
  the capital's action before the plain key's, which is how a terminal
  reporting every key as an escape code delivers it. The walk takes the
  lease as its grace, and stopping ends it on the tick with the stride
  rested. A help entry added to the binding table falls past the 120
  columns the golden frames pin.
- **The mouse** (ADR-008). A drag turns the view by the cells the pointer
  moved times the degrees each is worth, on from where it was last seen
  and not from the press, and a report from the same cell turns nothing;
  the press only marks where the drag began, and after the button is up a
  plain motion turns nothing in `drag` while any motion turns in `free`;
  the wheel is a notch either way in both. `off` turns nothing whatever
  arrives and forgets where the pointer was. Moving the pointer right
  turns the view right — looking south and turning a quarter looks west —
  its rows tilt the isometric table, fifteen rows down being fifteen
  degrees steeper, and both the tilt and a perspective pitch stop at the
  ends of their range however far the pointer is dragged. The `mouse`
  row's values are `MouseMode::NAMES` and each reads back as its mode.
- **Walking** (ADR-008). Over ticks of 40 ms with the key renewed each
  tick, the distance walked is the speed times the seconds to the
  centimetre, 2.8 m in two seconds at 1.4 m/s, and a run on a diagonal
  three times that; a tap walks `GRACE` seconds' worth, 14 cm over three
  ticks, then rests with its distance zeroed; nobody walks nothing. The
  walk stops at the pond within a step of its edge, by the tile the point
  would land in, with the key still held, and a diagonal into the shore
  slides along it. The pose index cycles by distance through a stride of
  0.7 m, wrapping, two poses holding 0.35 m each, and art with no poses
  rests. Facing follows the press, keeps when the press is toward or away
  from the camera, and persists when stopped.
- **Art poses.** A `# pose` line after the header starts another pose,
  padded to the common width, round-tripping through `to_text`; a pose
  of another height is refused on the line it began; a row that starts
  with `#` is glyphs. A mirrored sprite reverses its rows, swaps the
  paired glyphs and mirrors its centre, and mirrors back to itself;
  poses wrap and a sprite with none rests; every loaded sprite and its
  mirror hold the invariants.
- **Cell step** (ADR-006). `Camera::cell_step` moves the figure's screen
  cell by exactly one column or row at every zoom and angle, from the
  cell boundary a spawn leaves it on, from odd centimetres, at any height
  and through a run of eight; the first press from a boundary lands within
  a centimetre of a cell centre; from a cell centre the step is the ground
  under one cell, halving exactly between zooms for columns and with the
  half height for rows; a fractional anchor keeps its tile's depth and the
  centre anchor is the tile anchor.
- `World::try_move` takes centimetres, refuses water and the island edge
  by the tile the new point lands in — a centimetre short of the pond is
  fine, the next centimetre is not — and moves on land; the spawn is a
  tile centre and negative positions floor to their tile.
- `World::light_campfire` refuses water and places a light on land. The
  campfire is a placed light and counts at any hour; the lit-window light
  a building discovers while drawing joins the frame light count at night
  and not at noon.
- Weather: a storm preset raises precipitation; snowpack grows below
  freezing and melts above; wetness dries faster in sun.
- Settings: each row cycles through all values and wraps;
  `Settings::apply` pushes every row into the objects it governs,
  including the camera's mode and field of view; presets and rows agree
  in length, the `camera` row's values are `Camera::MODES`, the
  `coupling` row's are `Coupling::NAMES`, the `fog` row's are
  `FogMode::NAMES`, the `fov` row is `preset` then rising whole degrees,
  and its keys step from the camera's own field of view without wrapping
  through `preset`.
- Traversal: `Camera::cell_step` makes a screen-space step at 45 degrees a
  diagonal map step and a map-axes step a cardinal one.
- World map: `WorldMap::teleport` lands at the centre of the nearest land
  tile to the cursor.
- Snapshot: `player_dx` and `player_dy` move the figure and go through the
  same move the keys make, so a step off the island is refused; `walk=`
  gives the same frame twice, moves the figure into its stride, covers
  more ground with `run=1`, goes the other way for `a`, and walks nothing
  in no time; `tilt=` and `camera=free` render the table's angle and the
  detached eye.

## Data

- **The catalogue and the schema agree.** `src/assets/schema.rs` reads
  docs/properties.md's field tables back as data: every property the
  catalogue names is a field of the row struct behind each table it lists;
  a property whose default is *required* is one no row struct gives a
  default, and one with a default is one a row may leave out; and for the
  tables the catalogue is the whole story for (props, species, blocks,
  creatures, lights) every field of the row struct is catalogued. The
  editor's descriptors are held against the same structs, names and
  required flags alike (`src/editor/fields.rs`), since a save renders a
  row through its row struct and would silently drop a field the schema
  has never heard of. So a property is added to the doc, the schema and
  the editor together, or to none of them.
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
  regardless of chunk order; sand is a shore terrain, so a tile at sea
  level with no water within the beach band is not sand and neither is the
  continuous surface over it, while a lake shore at the same height is;
  the chunk ceiling bounds every tile with its stack and crown; a placed stack overrides the generator, lifts the
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
  named `pub(crate)` piece (`lighting::knee`, `camera::CloudView`) is
  preferred to restructuring modules.
- A change that alters pixels re-records the golden frames in the same
  commit and says which frames changed and why.
