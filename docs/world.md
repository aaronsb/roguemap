# The world

The world is a pure function of position and seed. Nothing is authored and
nothing is stored: ask for a tile and it is computed, ask twice and you get
the same answer. The whole thing is one continuous height field, a climate
laid over it, and a lazily generated cache of 32-tile chunks.

![island](screenshots/island.png)

Seed 7 at the widest zoom, in the island view. `make snap OUT=shot.png
ARGS="zoom=0 t=3 tod=12"` renders it.

## The height field

`Map::height_smooth(xf, yf)` is the only continuous sampler, and it is
`relief(field(xf, yf))`. The field is unitless; `relief` turns it into
metres.

The field is three layers of value noise.

**The control layer** is the slowest: three octaves at 0.0035 cycles per
tile, so its base wavelength is about 286 tiles, or 570 metres. It decides
where the oceans, the plains and the ranges are. It is deliberately
separable — an authored or edited control grid can replace it without
touching anything below.

**The local layer** is four octaves at 0.045 cycles per tile, a base
wavelength of about 22 tiles. It is the hills.

The two mix 0.62 control to 0.38 local, are stretched 1.6 times about the
midpoint for contrast, raised to the power 1.15, and scaled to 15 field
units.

**The detail octaves** are a separate layer that never reaches a tile's
stored height. `Map::detail` is noise at 0.6 cycles per tile with an
amplitude of ±0.45 metres, and the ray walk adds it to the bilinear grid
sample as it descends. The octave count comes from the zoom: none at 1:8,
one at 1:4, two at 1:2, three at 1:1. It stays under a metre, so it
roughens a slope without ever moving a terrace.

The noise primitives are in `src/noise.rs`: a splitmix-style integer
`hash`, bilinear `value` noise with a smoothstep ease, and `fbm` with
lacunarity 2 and gain 0.5.

## Relief in metres

```rust
pub fn relief(units: f32) -> f32 {
    if units <= SEA_UNITS { (units - SEA_UNITS) * DEPTH }
    else { RELIEF * ((units - SEA_UNITS) / (FIELD_UNITS - SEA_UNITS)).clamp(0.0, 1.0).powf(RELIEF_EXP) }
}
```

Below the shoreline the mapping is linear, four metres of water per field
unit, bottoming out at `FLOOR = -12.0`. Above it the curve is a 1.6 power,
so 3 units is the shoreline at 0 m and 15 units is `RELIEF = 120.0` m at
the top of a range. Because the exponent is greater than one, lowlands
stay flat: a valley floor a unit above the shoreline is only a metre or
two above the water.

| Constant | Value | Meaning |
|---|---|---|
| `SEA` | 0 | sea level |
| `FLOOR` | -12.0 | the deepest sea bed |
| `RELIEF` | 120.0 | the top of the tallest range |
| `STONE_Z` | 40 | buildings are stone from here up |
| `ROCK_Z` | 51 | rock is the ground terrain from here up |

`relief_fraction` inverts the curve: zero at the shore, one at a range
top. The lapse rate, the world map's height shading and the scene's colour
lift are all linear in that fraction rather than in metres, which is what
keeps the treeline and the biome bands where they read.

A tile's stored height is the field sampled at the tile centre and floored
to whole metres. The renderer uses the unfloored value.

## Rivers

Rivers are a contour band of an independent slow field, not a drainage
network. There is no flow accumulation and no erosion.

A single octave of value noise at 0.012 cycles per tile — a lattice period
of about 83 tiles — is sampled, and the river is the band where that field
is near 0.5. Three thresholds carve it:

- within 0.008 of the middle the height is set to 6 metres of water;
- within 0.016 it is clamped to at most 2 metres of water;
- within 0.045 it is clamped to a ramp rising to a graded valley
  shoulder.

Only the two inner bands are set outright; the outer band never raises
ground. Rivers exist only between about 2 and 51 metres of terrain, so
they stop short of both the shoreline and the rock line. Because the width
follows the local gradient of the noise rather than a width constant, the
deep channel is on the order of a few tiles across and the shoulder
several times that.

Water is simply any tile below sea level. Each chunk floods its water
bodies to find their size, capped at 200 tiles, so a pond stays calm while
anything larger is treated as open water and gets whitecaps.

## Climate

Temperature is a latitude-like field, not a real latitude. Two octaves at
0.0022 cycles per tile — a base wavelength of about 455 tiles — run from
26 °C down to −10 °C at sea level, plus a ±3 °C local wobble, minus the
lapse:

```
temp = 26 - 36 * (lat + 1) / 2 + local * 3 - LAPSE * relief_fraction(z)
```

`LAPSE` is 26.4 degrees from shore to range top, spent through
`relief_fraction` rather than per metre, so half of it is gone by about 39
metres.

Precipitation is a separate 0 to 100 scale: 80 percent from three octaves
at 0.003 cycles per tile and 20 percent from two at 0.03, contrast
stretched. There is no orographic term and no rain shadow.

Season is on top of both. `seasonal_temp` swings the annual figure by ±9
degrees, peaking at season 1 and bottoming at season 3, and `vigour`
smoothsteps that between −5 and 7 degrees to say how much a deciduous
tree is carrying.

### Köppen

`biome::classify(temp, precip)` returns one of nine codes. Aridity is
tested before warmth, so a hot dry place is a desert rather than a
savanna.

| Test | Code | Biome |
|---|---|---|
| temp ≤ −16 | `EF` | ice cap |
| temp ≤ −6 | `ET` | tundra |
| precip < 22 | `BW` | desert |
| precip < 42 | `BS` | steppe |
| temp ≥ 18, precip ≥ 68 | `Af` | rainforest |
| temp ≥ 18 | `Aw` | savanna |
| temp ≥ 3, precip < 58 | `Cs` | mediterranean |
| temp ≥ 3 | `Cf` | temperate forest |
| otherwise | `Df` | boreal forest |

The boundaries are Rust; the table the codes point at is data. Each row of
`assets/biomes.toml` names its Köppen code, and the loader refuses a set
that leaves any of the nine unclaimed or claims one twice.

```toml
[[biome]]
name = "desert"
koppen = "BW"
cover = "bare"
ground = [206, 176, 122]
seasonal = false
grass = 0
tree_density = 0.05
spacing = 2.5
species = [["saguaro", 5], ["sagebrush", 1]]
material = "adobe"
```

A biome names the ground colour, the cover kind, how much grass grows, how
dense the forest is, how far apart its crowns stand, its weighted species
list and the material its buildings are made of. Row order in the species
list is load-bearing: the weighted pick reads it in order.

Ground terrain then falls out of height and temperature: water below sea
level, sand at the shoreline, snow below −16 °C, rock above `ROCK_Z`, a
patch of dirt where a third noise field is high, and grass otherwise.

Sand is a shore terrain, so both of its bands ask how far the water is as
well as how low the ground: a tile at sea level is beach where water lies
within `SHORE_TILES` (three), a tile a metre up only where the water is
its neighbour. A basin that happens to sit at sea level inland is grass
like the plain around it. The continuous surface follows the tiles: below
0.45 m it is sand where the tile it stands on or one beside it is sand or
water, and below 1.3 m where the water is within a step.

![steppe](screenshots/steppe.png)

The steppe biome, `BS`, with adobe houses and wide-spaced acacias.

## Chunks and determinism

Tiles are generated in 32 by 32 chunks — 64 metres square — on demand.
`Map` holds them in a `HashMap` keyed by chunk coordinate. Generating a
chunk builds its 1024 tiles, labels its water bodies, lays its settlement
plots, applies any hand-placed stacks, and records the chunk's ceiling so
the renderer can skip empty sky above it.

Only the chunk is cached. Climate, height, temperature and precipitation
are recomputed on every call; there is no eviction, so an unbounded map's
chunk table grows as you explore.

Determinism rests on the noise being a pure function of position and seed.
Every consumer mixes a distinct tag into the seed — the control layer, the
local layer, rivers, detail, dirt patches, latitude, precipitation, forest
density, grass, tree spacing, the settlement field and each block kind's
roll all have their own. Two tests pin the contract: a tile is the same
whichever order chunks are touched in, and the same whether the map was
generated bounded or unbounded.

A stack placed by hand goes into a side table that survives regeneration,
so it overrides the generator and lifts the chunk ceiling even when it was
placed before its chunk existed.

Trees are placed the same way. A tile carries a tree only on grass above
sea level in a biome with species, only if it wins a local-maximum test
against its neighbours within its species' spacing, and then only at the
biome's tree density. That is what keeps a dense stand reading as trunks
and gaps rather than one mass.

## The world map

![worldmap](screenshots/worldmap.png)

`m` opens the world map. It plots biomes top-down at three extents — one,
four or sixteen tiles per column, and twice that per row, to keep
proportions on cells that are twice as tall as they are wide.

The palette is pinned to spring so the plot and its legend stay readable
under winter snow. Water lerps from shallow to deep over the twelve metres
down to the floor; land takes its biome's ground colour, overridden by
rock above `ROCK_Z` and by sand at the shoreline, then shaded by height
and blended toward snow. The height lift is stronger than the scene's, so
relief still reads at sixteen tiles per cell. At the coarse extents each
cell averages a two-by-two subsample rather than aliasing on one.

The cursor sits at the screen centre and the map moves under it. The
header names the extent, the cell size in tiles, the cursor position, the
biome and its Köppen code, the temperature, the precipitation, the height,
the biome's first species and its material. The bottom row is a legend of
every biome's colour and code.

Arrows, `w a s d` or `h j k l` move the cursor by one cell, `z` and `Z`
change the extent, `Enter` or `t` teleports the player to the nearest land
tile, and `m`, `Esc` or `q` close it.
