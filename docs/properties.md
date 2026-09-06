# Asset properties

Every placeable asset (prop, tree species, block kind, creature, light)
is a row in a table. This catalogue names the properties a row may carry,
what each means, its type and default, and which tables use it. Rows
carry only the properties that apply; the loader supplies defaults for the
rest. Properties marked *reserved* are validated and stored but not yet
read by any system.

The tables catalogued here are `biomes`, `species`, `materials`, `props`,
`blocks`, `creatures`, `seasons`, `surfaces` and `lights`. In the Tables
column *all* means every one of them and *all placeables* means props,
species, blocks and creatures, the four tables that describe a thing
standing in the world. The settings, frame and tileset tables describe the
program rather than the world and are documented with their own systems
(docs/frames.md, ADR-005). The row structs are `src/assets/schema.rs`, and
a test parses the tables below and holds each property against them
(docs/testing.md), so a property is added to both or to neither.

## Identity

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| name | string | required | all | Unique key within its table; referenced by other tables by this name. |
| description | string | "" | all | One or two sentences for the editor and for players who examine the thing. Written for a reader, not a parser. |
| category | string | table name | all | Grouping for the editor's list: tree, shrub, rock, dwelling, workplace, light, animal, person, vehicle. |
| aliases | string list | [] | all | Other names the same row answers to, for authoring and lookups. |

## Appearance

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| form | enum | required | species | Glyph pool and one-glyph sprite: pine, broadleaf, scrub, cactus, later lsystem. |
| shape | enum | by form | species | Canopy volume for the geometry renderer (ADR-002): cone, ellipsoid, dome, cactus, or lsystem for a tree grown from a grammar (docs/lsystem.md). |
| style | name | none | species | Growth habit from `tree_styles.toml`, for `shape = "lsystem"`: excurrent, decurrent, columnar, weeping, umbrella, vase, palm, shrub. |
| lsystem | table | {} | species | Parameter overrides for the habit, or a grammar written out in full: `branch_angle`, `forks`, `taper`, `droop`, `leaf_density`, `asymmetry`, `jitter`, `prune_height`, `depth`, `length`, `leaf_radius`, `axiom`, `rules`, `dead_rules` (docs/lsystem.md). |
| dead_chance | 0..1 | 0.02 | species | Chance an instance stands dead: no leaves, grey bark, a broken crown. The roll is a hash of the tile seed, so the same tree is dead every time. |
| roof | enum | flat | blocks | Profile above the column: none, flat, gable, hip. |
| art | name | required | props, creatures | Name of an art file set; the tier is chosen by zoom. Every prop and creature has one. |
| min_zoom | zoom index | 2 (near) | props | Smallest zoom the prop is drawn at; a pebble is not worth a cell at 1:8. |
| size | [w, d, h] metres | required | props, species, blocks, creatures | Real extent: width, depth, height. A block's is one tile at one level (a house level [2, 2, 3]); a species' is a mature tree's spread and height (an oak [10, 10, 18]); a boulder [1.2, 1.0, 0.8]; the person [0.6, 0.4, 2.0]. The renderer draws it through the zoom's rows and columns per metre (ADR-004), and a sprite's art tier is the one whose rows are nearest the height it stands. |
| size_class | small, mixed, large | mixed | species | Scales the species' size: 0.8, 1, 1.15. |
| radius, height, trunk, trunk_radius | metres | from size by shape | species | Crown radius and height, trunk height and radius, when a row wants other proportions than its shape derives. The canopy is `size` plus these; there is no other spelling of it. |
| color | rgb | props black, creatures required | props, creatures | Fill behind the glyph. Black on a prop means the glyph is drawn over the ground with no background. Lights carry their own `color` (below). |
| glyph | rgb | required | props, creatures | Glyph colour. `glyph_color` is accepted as the older spelling. |
| canopy, canopy_glyph | rgb x4 | required | species | Spring, summer, autumn, winter colours. |
| material | name or rule | by_biome | blocks | Wall and roof colours by material; `by_biome` (or `local`) takes the tile's material. |
| pitch, max_rise | metres per metre, metres | 1.0, 1.5 | blocks | Roof rise per metre of run from the eaves, and its cap. |
| merge | bool | true | blocks | Whether same-kind neighbours share walls and roof. |
| ground | enum | flatten | blocks | What the tile top becomes: none, flatten, pave, till. |
| windows, window_pitch | fraction pair, metres | [0.5, 1.0], 1.0 | blocks | Band within a level where windows go (empty for none) and the spacing along a face. |
| door | bool | true | blocks | One door at ground level on an open face, toward a road when there is one. |
| seasonal | bool | false | biomes | Whether the biome's ground and cover colours follow the seasons: a rainforest and a desert say no, a temperate wood yes. A species has no such flag; its canopy is seasonal whenever its `canopy` table gives four colours. |
| snow_cover | 0..1 | 1 | props, species, blocks | How much accumulated snow shows on it; a smooth boulder holds less than a roof. |

### Conditions

Conditions are continuous values the world sets; the asset says how
visibly it shows each one. Rendering blends toward the condition's colour
by the world value times the asset's factor.

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| wet_darkening | 0..1 | 1 | all placeables, surfaces | How much rain darkens it. Stone shows less than earth. |
| dry_fading | 0..1 | 0 | all placeables, surfaces | How much drought bleaches it toward the dry colour. |
| weathering | 0..1 | 0 | all placeables | How fast it takes on a weathered look with age: greying wood, patina, worn edges, faded cloth. |
| mossing | 0..1 | 0 | all placeables | Tendency to grow moss on shaded, wet faces. |
| soiling | 0..1 | 0 | all placeables | Grime and soot accumulation near fires and traffic. |
| condition_colors | table | {} | all placeables, surfaces | Colour per condition name: wet, dry, weathered, mossy, soiled, burnt, frozen. Missing entries use the table's defaults. |

General: every placeable table (props, species, blocks, creatures) and
the surface table carries the condition properties; a creature can be wet
or soiled like anything else.

### Looks

Discrete states carry their own art or colours in `states`, on every
placeable table; a look is named there and chosen by the world: unlit,
lit, burning, burnt, ruined, sapling, mature, dead, open, closed,
occupied, asleep, carrying.

## Placement

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| terrain | terrain list | required | props, blocks | Ground kinds it may stand on. A creature says `can_enter` instead, and a species is placed by its biome's weighted list. |
| cover | cover list | any | props | Ground cover kinds required (grass, dry, moss, bare). |
| near_water | bool | false | props | Requires a cardinal neighbour of water. |
| min_height, max_height | metres | none | props, species, blocks | Elevation band above sea level. |
| max_slope | metres per metre | none | blocks, props | Refuses steep ground; buildings want flat tiles. |
| density | 0..1 | required for props | props | Chance per placement cell where the rules pass. |
| cluster | 0..1 | 0 | props, species | Tendency to appear beside its own kind; 0 is independent, 1 is only in groups. |
| spacing | metres | from size | species | How far a tree stands from its own kind: three quarters of its crown width when the row says nothing, so a 7 m spruce keeps five metres and a 3 m juniper two. The biome's own `spacing` factor multiplies it. |
| spacing | factor | 1 | biomes | How far apart this biome's trees stand, on each species' own spacing: a rainforest or a boreal stand lets crowns overlap (0.4), a temperate wood keeps them touching (0.7), a savanna sets them wide (2). |
| spacing | metres | 0 | blocks, creatures | Minimum distance from another of the same kind. |
| levels | count pair | [1, 1] | blocks | Levels the generator gives a stack, `[min, max]`; `[0, 0]` for ground kinds. |
| footprint | [[w, d], [w, d]] tiles | [[1, 1], [1, 1]] | blocks | Smallest and largest ground one of these covers: a house 3x2 to 5x3 tiles, a tower 2x2, a barn 4x3 to 6x3, a field 6x6 to 12x8. The generator picks a size in the range and lays whole tiles of one kind and level count, which merge into one building. |
| settle_min, chance | 0..1, percent | 1.0, 0 | blocks | The generator's rule: the settlement field a plot needs, and the share of qualifying plots that carry one. A plot is sixteen tiles square and holds at most one building, with a tile of margin, so buildings never touch. |

## Physical

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| passable | bool | props true, blocks false, trees false | props, species, blocks | Whether a creature may enter the tile. |
| blocks_sight | bool | false | props, species, blocks | Whether it stops line of sight (reserved). |
| level_height | metres | size's height | blocks | Metres per level; a stack of two is twice as tall. |
| max_levels, deck | count, bool | 3, false | blocks | Validation cap on levels; `deck` is reserved for bridges. |

## Light and energy

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| light | name | none | all placeables | Row in lights.toml this asset emits. |
| color | rgb 0..1 | required | lights | The light's colour, as three floats. |
| radius | metres | required | lights | How far it throws. |
| intensity | 0.. | required | lights | Brightness at the source. |
| falloff | exponent | 2 | lights | How the light falls off with distance. |
| flicker_amount | 0..1 | 0 | lights | Flicker depth; zero is a steady light. |
| flicker_rate | hertz | 0 | lights | Flicker speed. |
| heat | 0..1 | 0 | props, blocks | Warmth given off; drives melting and comfort (reserved). |
| fuel | minutes | none | props | How long it burns before going out; none means indefinitely (reserved). |

## Interaction hooks (reserved)

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| tags | string list | [] | all placeables | What it is: flammable, edible, wooden, stone, wet, sacred. Read by other assets' `affects`. |
| emits | string list | [] | all placeables | What it puts out continuously: light, heat, smoke, sound, scent. |
| affects | string list | [] | all placeables | Tags it can act on within reach: a bonfire affects flammable; a lamp affects nothing. |
| reach | metres | 0 | all placeables | How far its effects act. Zero means it only emits. |
| verbs | string list | [] | props, species, blocks | Actions a player may take: examine, harvest, enter, light, extinguish, climb. |
| yields | name and count list | [] | props, species | What harvesting produces, for a future inventory. |
| states | string list | [] | all placeables, surfaces | Named states with their own art or colours: unlit, lit, burning, burnt, ruined, sapling, mature, dead. |

## Lifecycle and weather (reserved)

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| growth | days | none | species, props | Time from sapling to mature; none means static. |
| decay | days | none | props, blocks | Time from ruined to gone. |
| sway | 0..1 | species 1, others 0 | species, props | How much wind moves it. |
| sheds | bool | true for deciduous | species | Whether it drops leaves in autumn. |

Self-pruning — the bare trunk under the canopy, a fraction of the height —
is not a row property of its own. The canopy volume takes it from the
shape (0.2 for a cone, 0.35 for an ellipsoid, none for a dome or cactus),
and a species grown from a grammar overrides it with `prune_height` inside
its `lsystem` table, which the stand-in volume then follows.

## Creatures

Creatures carry the identity, appearance, interaction-hook and condition
properties above and `size` in metres like every other placeable. The
physical, placement and lifecycle group belongs to the things that stand
still: a creature has none of it, and walks where `can_enter` lets it.
Their own properties are:

| Property | Type | Default | Meaning |
|---|---|---|---|
| speed | metres per second | 2 | Movement rate. |
| can_enter | terrain list | land | Terrain it may walk on; swimming and flying widen it. |
| diet | tag list | [] | Tags it eats; connects to `yields` and `tags`. |
| behaviour | name | idle | Wander, graze, flee, hunt, patrol. |
| sight | metres | 16 | Perception range. |
| home | block name | none | Where it returns at night. |

## Instances

An asset describes a kind; a placed instance carries its own values. A
weathered tent and a fresh tent are one asset with two instances:

| Instance field | Type | Meaning |
|---|---|---|
| asset | name | The row this instance is an example of. |
| state | name | The current look from the asset's `states`, or none. |
| conditions | table | Current values 0..1 per condition: wet, dry, weathered, mossy, soiled, burnt, frozen. The world moves them by weather, age and events, scaled by the asset's rates. |
| age | days | Time since placed; drives weathering, growth and decay. |
| levels | count | For stacks. |
| variant | index | Which art variant or seed. |

The renderer blends the asset's colours toward each condition colour by
the instance value, and switches art when a look's threshold is crossed
(a look may declare `when = { weathered = 0.7 }`). Generated instances
(props on the sub-grid, trees from the biome) get deterministic starting
values from the seed, so a forest has old and young trees without anyone
placing them.

## Conventions

- Names are lower case with spaces; references are by name, never by index.
- Colours are `[r, g, b]` integers 0..255 in tables and 0..1 floats in lights.
- Distances and heights are metres, in every table and every field:
  sizes, crown radii, roof rises, window pitches, reaches, perception
  ranges and light radii. Anything stored discretely is centimetres. A
  tile is 2 m, the person 2 m, a house level 3 m, a tower level 4 m, a
  barn 5 m, an oak 18 m (ADR-004). A metre draws as the zoom's rows per
  metre: 6 rows at 1:1, 3 at 1:2, 1.5 at 1:4 and 0.75 at 1:8.
- Lists of pairs keep authoring order, since hash-driven picks read them
  in order.
- Every row has a description. The editor shows it beside the preview and
  refuses to save a placeable without one.
