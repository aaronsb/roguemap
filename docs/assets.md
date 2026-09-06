# Assets

Everything that describes a thing in the world is data, and the code is the
system that renders or simulates it. This note fixes the layout so new
props, trees, buildings and creatures are added by writing a file.

## Layout

```
assets/
  biomes.toml       climate classes: ground colour, cover kind, species and
                    building weights, material
  species.toml      trees: shape, size class, seasonal canopy colours
  props.toml        ground props: placement rules, colours, art references
  buildings.toml    building kinds: shape, material rule, light
  creatures.toml    creatures: art, colours, terrain rules, light
  materials.toml    wall and roof colours by material
  surfaces.toml     per-terrain colour and texture density
  lights.toml       named light specs (campfire, window, torch)
  settings.toml     settings rows: key, label, values
  tilesets/
    petscii.toml    glyph roles for the PETSCII set
    ascii.toml      glyph roles for the ASCII set
  art/
    <name>.txt      hand-drawn sprites, one file per tier
```

Tables are embedded into the binary with `include_str!`, so the executable
stays standalone. Setting `ROGUEMAP_ASSETS=<dir>` loads the same files from
disk instead, for authoring without a rebuild.

## Art files

A sprite file is a header line then rows of glyphs:

```
# name=house/adobe tier=large center=7 base_rows=2
    ◢▒▒▒▒◣
   ◢▒▒▒▒▒▒◣
  ◢▒▒▒▒▒▒▒▒◣
 ◢▒▒▒▒▒▒▒▒▒▒◣
  □   ▐   □
```

`center` is the column over the tile centre; `base_rows` counts rows drawn
in the base colour (trunk, walls) rather than the top colour (canopy,
roof). Spaces are transparent. Rows are padded to one width on load.

Procedural shapes (pines, broadleaf, scrub, cactus, and later L-systems)
are named in the table instead of drawn; the generator builds a tier per
zoom.

## Schema and validation

Each table has a Rust struct that is its schema. Loading validates
cross-references (a biome's species exist, a prop's art files exist, a
building's material exists) and reports the file and row on failure. The
existing tests that walk the tables become the load-time check.

## Editor

The editor is its own binary, `roguemap-edit`, sharing the library crate
with the game so previews use the real renderer. It lists assets by table,
previews one at every zoom tier in a chosen biome and season, edits rows
in place and art files as a grid, and writes back to the asset directory.
