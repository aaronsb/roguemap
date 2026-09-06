# Asset properties

Every placeable asset (prop, tree species, block kind, creature, light)
is a row in a table. This catalogue names the properties a row may carry,
what each means, its type and default, and which tables use it. Rows
carry only the properties that apply; the loader supplies defaults for the
rest. Properties marked *reserved* are validated and stored but not yet
read by any system.

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
| shape | enum | required for species and blocks | species, blocks | Procedural form: pine, broadleaf, scrub, cactus, later lsystem; for blocks the roof profile. |
| art | string | none | props, creatures, blocks | Name of an art file set; tiers are chosen by zoom. Absent means procedural shape or no picture. |
| size_class | small, mixed, large | mixed | species | Which sprite variant pair the row draws from. |
| canopy | [radius, base, top] metres | from size | species | Canopy volume for the geometry renderer (ADR-002): radius, height of the canopy base above ground, height of its top. Defaults derive from `size`. |
| color, glyph_color | rgb | required | props, lights | Fill and glyph colours; seasonal tables use four values. |
| canopy, canopy_glyph | rgb x4 | required | species | Spring, summer, autumn, winter colours. |
| material | name or rule | biome | blocks | Wall and roof colours by material; `local` takes the tile's material. |
| seasonal | bool | true | species, biomes | Whether colours follow the seasons. |
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
| terrain | terrain list | any | props, blocks, creatures | Ground kinds it may stand on. |
| cover | cover list | any | props | Ground cover kinds required (grass, dry, moss, bare). |
| biomes | name list | any | props, blocks, creatures | Biomes it may appear in; species use the biome's own weighted list instead. |
| near_water | bool | false | props, blocks | Requires a cardinal neighbour of water. |
| min_height, max_height | height units | none | props, species, blocks | Elevation band. |
| max_slope | height units per tile | none | blocks, props | Refuses steep ground; buildings want flat tiles. |
| density | 0..1 | required for props | props | Chance per placement cell where the rules pass. |
| cluster | 0..1 | 0 | props, species | Tendency to appear beside its own kind; 0 is independent, 1 is only in groups. |
| spacing | tiles | 0 | blocks, creatures | Minimum distance from another of the same kind. |
| footprint | w x h tiles | 1x1 | blocks | Tiles occupied; merges with same-kind neighbours per ADR-002. |

## Physical

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| size | [w, d, h] metres | required | all placeables | Real extent: width, depth and height. Every prop, tree, block, creature and vehicle has one, natural or built. The renderer derives rows and columns at each zoom from it, art tiers scale to fit it, and footprint, occlusion and cast shadows follow from it. A boulder is [1.2, 1.0, 0.8]; an oak [10, 10, 18]; a person [0.6, 0.4, 2.0]; a house level [8, 6, 3]. |
| passable | bool | props true, blocks false, trees false | props, species, blocks | Whether a creature may enter the tile. |
| blocks_sight | bool | false | props, species, blocks | Whether it stops line of sight (reserved). |
| levels, level_height | count, height units | 1, 2 | blocks | Stack height; a stack of two is twice as tall. |
| mass | kg | 0 | props, creatures | Reserved for physics and pushing. |
| hardness | 0..1 | 1 | props, blocks | Resistance to damage and to being harvested. |

## Light and energy

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| light | name | none | props, blocks, creatures | Row in lights.toml this asset emits. |
| color, radius, intensity, falloff, flicker_amount, flicker_rate | see lights | required | lights | The light itself: colour, throw in tiles, brightness, falloff exponent, flicker depth and speed. |
| heat | 0..1 | 0 | props, blocks | Warmth given off; drives melting and comfort (reserved). |
| fuel | minutes | none | props | How long it burns before going out; none means indefinitely (reserved). |

## Interaction hooks (reserved)

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| tags | string list | [] | all placeables | What it is: flammable, edible, wooden, stone, wet, sacred. Read by other assets' `affects`. |
| emits | string list | [] | all placeables | What it puts out continuously: light, heat, smoke, sound, scent. |
| affects | string list | [] | all placeables | Tags it can act on within reach: a bonfire affects flammable; a lamp affects nothing. |
| reach | tiles | 0 | all placeables | How far its effects act. Zero means it only emits. |
| verbs | string list | [] | props, blocks, creatures | Actions a player may take: examine, harvest, enter, light, extinguish, climb. |
| yields | name and count list | [] | props, species | What harvesting produces, for a future inventory. |
| states | string list | [] | props, blocks, species | Named states with their own art or colours: unlit, lit, burning, burnt, ruined, sapling, mature, dead. |

## Lifecycle and weather (reserved)

| Property | Type | Default | Tables | Meaning |
|---|---|---|---|---|
| growth | days | none | species, props | Time from sapling to mature; none means static. |
| decay | days | none | props, blocks | Time from ruined to gone. |
| sway | 0..1 | species 1, others 0 | species, props | How much wind moves it. |
| sheds | bool | true for deciduous | species | Whether it drops leaves in autumn. |

## Creatures (reserved table)

| Property | Type | Default | Meaning |
|---|---|---|---|
| speed | tiles per second | 1 | Movement rate. |
| can_enter | terrain list | land | Terrain it may walk on; swimming and flying widen it. |
| diet | tag list | [] | Tags it eats; connects to `yields` and `tags`. |
| behaviour | name | idle | Wander, graze, flee, hunt, patrol. |
| sight | tiles | 8 | Perception range. |
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
- Distances and heights are metres; anything stored discretely is
  centimetres. A tile is 2 m, the person 2 m, a house level 3 m (ADR-004).
- Lists of pairs keep authoring order, since hash-driven picks read them
  in order.
- Every row has a description. The editor shows it beside the preview and
  refuses to save a placeable without one.
