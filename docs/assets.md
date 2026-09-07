# Assets

Everything that describes a thing in the world is data, and the code is the
system that renders or simulates it. This note fixes the layout so new
props, trees, buildings and creatures are added by writing a file. The
properties a row may carry, with types and defaults, are catalogued in
[properties.md](properties.md); the decision is
[ADR-001](adr/ADR-001-assets-as-files.md).

## Layout

```
assets/
  biomes.toml       climate classes: Köppen code, ground colour, cover kind,
                    weighted species, material
  species.toml      trees: form, size class, size and canopy shape in
                    metres, seasonal canopy colours
  tree_styles.toml  growth habits for L-system trees: eight parametric
                    grammars with their parameter defaults and the canopy
                    volume each stands in as at far zooms (lsystem.md)
  materials.toml    wall and roof colours by material
  props.toml        ground props: art, size, colours, placement rules
  blocks.toml       block kinds: size, levels, roof profile, material rule,
                    merging, faces, light, placement rule (ADR-002)
  creatures.toml    creatures: art, colours, walkable terrain; the player
                    is the first row
  surfaces.toml     the four seasonal palettes, the plain-surface texture
                    rules (sand, dirt, rock, snow) and the grass and
                    cattail densities
  lights.toml       named light specs: colour, radius, intensity, falloff,
                    flicker
  settings.toml     settings rows: key, label, values, default, shortcut
  ui.toml           overlay frames: name, title, content kind, anchor,
                    size, border, background, z, priority, show rule,
                    toggle key (ADR-005)
  editor-ui.toml    the editor's panes as frames, in the same row shape
                    as ui.toml
  tilesets/
    petscii.toml    glyph roles and sprite vocabulary for the PETSCII set
    ascii.toml      the same for the ASCII set
  art/
    player/*.txt    hand-drawn sprites, one file per tier
    props/<name>/*.txt
    tiny/<tileset>/*.txt   one-glyph trees per glyph set
```

Every file under `assets/` is embedded into the binary by `build.rs`, so
the executable stays standalone. Setting `ROGUEMAP_ASSETS=<dir>` loads the
same layout from disk instead, for authoring without a rebuild; the
directory must hold every table, and a missing directory or file is an
error, never a silent fallback to the embedded set. `make assets-export`
(`roguemap --export-assets DIR`) writes the embedded files out to start
from.

Row order in a file is the runtime index order and is load-bearing:
hash-driven picks (tree species by biome weights, prop scatter) read the
lists in order, so reordering rows changes the world.

## Values

- Colours are `[r, g, b]` integers in tables and `[r, g, b]` floats 0..1 in
  `lights.toml`.
- Glyphs are one-character strings.
- Enumerations are lowercase strings: `form = "pine"`, `cover = "moss"`,
  `terrain = ["grass", "dirt"]`.
- References are by name: `material = "adobe"`, `species = [["oak", 5],
  ["birch", 3]]` (name and weight pairs, in order), `art = "boulder"`,
  `light = "window"`. A name that does not resolve is a load error naming
  the file and row.
- Seasonal colours are one `[r, g, b]` for evergreens or four, in the
  order spring, summer, autumn, winter.
- Sizes are metres: every placeable row carries `size = [w, d, h]`
  (width, depth, height), and the block and species geometry fields are
  metres too. A tile is 2 m square; a height unit is a metre.
- Every row may carry `description`, `category` and `aliases`. Placeable
  rows (props, species, blocks, creatures, lights) must have a non-empty
  description.

### Interaction hooks

Props, species, blocks and creatures carry the reserved hooks `light`,
`tags`, `emits`, `affects` and `reach`. Props, species and blocks carry
the physical and lifecycle properties as well, and all four the condition
properties in the catalogue. They are validated and stored but
no system reads them yet; they are the seams for a future interaction
graph. A lamp with `reach = 0` only throws its light, while a bonfire with
`reach = 3` and `affects = ["flammable"]` can act on the tagged things
around it. The `campfire` prop is the one the `f` key places: it names the
`campfire` light, emits light and heat, and affects flammable things
within three metres, like every other distance in the tables (ADR-004).

### Conditions

Every placeable row and every surface row carries `wet_darkening`,
`dry_fading`, `weathering`, `mossing`, `soiling`, `condition_colors` and
`states`. Only `wet_darkening` on the surface rows is read today: rain
darkens sand, dirt and rock by the world's wetness times that factor.

## Art files

A sprite file is a header line then rows of glyphs:

```
# name=player tier=large center=4 base_rows=0
   ___
  (o o)
   \_/
 __|=|__
/  |=|  \
```

The header carries `key=value` pairs. `name` and `tier` are required and
index the sprite; a duplicate pair is a load error. `center` is the column
over the tile centre, `width / 2` by default; `base_rows` counts rows drawn
in the base colour (trunk, walls) rather than the top colour (canopy,
roof), 0 by default. Spaces are transparent, tabs are an error, trailing
blank lines are dropped and rows are padded to one width on load.

Tiers are `tiny`, `small`, `medium` and `large`, drawn from zooms 0, 2, 4
and 6 by default; `min_zoom=N` in the header overrides a tier's first zoom.
At each zoom a sprite uses the tier with the largest first zoom at or below
it, so a `small` file plus a `large min_zoom=5` file gives the props their
two looks, and the player's four files give four. Art is referenced from
the tables by name: `art = "boulder"` in `props.toml`, `art = "player"` in
`creatures.toml`, and the `[art.tiny]` entries of a tileset name the
one-glyph tree sprites for the two smallest zooms.

Buildings and trees have no art: they are block geometry and volumes on
the ray walk (docs/structures.md, ADR-002), textured with the tileset's
`[art]` vocabulary (`pine_fill`, `round_mid`, `cactus`, `trunk`,
`dead_branch`, `roof_fill`, `door`, `window`).

A species with `shape = "lsystem"` is grown from a grammar rather than a
canopy volume: it names a growth habit from `tree_styles.toml` and
overrides the parameters it wants different. The grammar, its symbols,
the eight habits and how to author such a species are in
[lsystem.md](lsystem.md).

## Schema and validation

Each table has a Rust struct that is its schema: the raw row structs in
`src/assets/schema.rs` are the file format, and the resolved structs
(`Biome`, `Species`, `Prop`, ...) hold indices in place of names. Loading
resolves every reference, checks the ranges in the catalogue (sizes
positive, block levels within `max_levels`, window bands rising within a
level, volume overrides positive), and requires what the engine relies on: every Köppen code the classifier emits has a
biome, the four seasons and four surfaces are in order, the ten settings
keys exist and every `glyphs` value has a tileset, a `stone` material, a
`campfire` prop with a light, `player` as the first creature, and every
frame of `ui.toml` naming a content kind code supplies and a key that can
be named. A
failure reports the file, the line for a parse error, and the table row,
as `species.toml:31: species[4] "juniper": unknown variant \`bush\``. The
loader tests (`make test-assets`) load the embedded set, check every
cross-reference, and round-trip the set through the serialiser and through
an exported directory.

## Editor

The editor is its own binary, `roguemap-edit`, sharing the library crate
with the game so previews use the real renderer (ADR-003). It lists assets
by table, previews one at every zoom tier in a chosen biome and season,
edits rows in place and art files as a grid, and writes back to the asset
directory after validating the whole set as the game loads it. `make
edit` opens it on `./assets-export`; the keys are in the README. Tables
can still be edited by hand with `ROGUEMAP_ASSETS`; the editor writes the
same files, through the same serialiser as `to_files`, keeping row order
and the leading comment block of each file (comments inside the body do
not survive a save).
