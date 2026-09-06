# Assets and the editor

Everything that describes a thing in the world is a file, not a Rust
constant: biomes, species, growth habits, materials, props, block kinds,
creatures, surfaces, lights, settings rows, overlay frames, glyph sets and
sprites are all rows in TOML tables or text art under `assets/`, so adding
a thing means adding a row. The decision is
[ADR-001](adr/ADR-001-assets-as-files.md) and every property a row may
carry is catalogued in [properties.md](properties.md).

![editor](screenshots/editor.png)

The asset editor on the `oak` row of `species.toml`, with the preview
strip drawn by the game's own renderer.

## The directory

| File | Table | Holds |
|---|---|---|
| `biomes.toml` | `[[biome]]` | Köppen code, cover, ground colours, grass, tree density, spacing, weighted species, material |
| `species.toml` | `[[species]]` | trees: form, size class, size in metres, canopy colours, canopy overrides, growth habit |
| `tree_styles.toml` | `[[style]]` | the eight growth habits, their parameters and rules |
| `props.toml` | `[[prop]]` | ground props: art, size, colours, placement rules |
| `blocks.toml` | `[[block]]` | block kinds: size, levels, footprint, roof, material, merging, faces, light |
| `creatures.toml` | `[[creature]]` | art, colours, walkable terrain; the player is the first row |
| `surfaces.toml` | `[[season]]`, `[[surface]]`, `[density]` | four seasonal palettes, four surface texture rules, grass and cattail densities |
| `lights.toml` | `[[light]]` | colour, radius, intensity, falloff, flicker |
| `ui.toml` | `[[frame]]` | overlay frames (see [frames.md](frames.md)) |

Beside them, `materials.toml` holds wall and roof colours, `settings.toml`
the settings rows, `tilesets/*.toml` the glyph roles and sprite vocabulary
of each glyph set, and `art/**/*.txt` the hand-drawn sprites. The full
layout is in [assets.md](assets.md).

References are by name. A biome names its material and a weighted list of
species; a species, prop, block or creature names a light; a prop or
creature names an art file; a block names a material or `by_biome`; a
creature names its home block. A name that does not resolve is a load
error naming the file and the row. Row order inside a file is the runtime
index order and is load-bearing: the weighted species pick and the prop
scatter read the lists in order, so reordering rows changes the world.

## The loader

`Assets::load` reads `ROGUEMAP_ASSETS` and loads that directory if it is
set, and otherwise the set `build.rs` embedded into the binary. There is
no fallback: a bad override directory is a hard error, never a silent
retreat to the embedded set.

```
make assets-export      # roguemap --export-assets assets-export
ROGUEMAP_ASSETS=assets-export make run
```

`--export-assets` writes the files out byte for byte as they were loaded,
so an exported directory is a faithful starting point; the editor's own
`--export DIR` writes the set and then opens on it. Tables are parsed in a
fixed order and resolved in dependency order — materials, lights, habits,
species, biomes, props, blocks, creatures, surfaces, settings, frames,
tilesets — so a reference is always resolved against something already
built. A `.toml` under `assets/` that is neither a table nor a tileset is
rejected rather than ignored.

An error names the file, the line for a parse error, and the table row:

```
species.toml:31: species[4] "juniper": unknown variant `bush`
```

### What validation checks

Beyond types, the loader enforces the invariants the engine relies on.
Names are unique per table, every placeable row carries a non-empty
description, sizes are positive in all three dimensions, densities and
fractions sit in 0 to 1, radii and reaches are non-negative, and a
seasonal colour table has exactly four entries. Some checks are
structural. Every one of the nine Köppen codes the classifier can emit
must be claimed by exactly one biome; there must be four seasons and four
surfaces, in order; all eleven settings keys must exist and every value of
the `glyphs` row must name a tileset; a material called `stone` must
exist, because buildings on rock and near the snow line are built of it; a
prop called `campfire` with a light must exist, because the `f` key places
it; `player` must be the first creature; and every frame in `ui.toml` must
name a content kind the code supplies and a key that can be parsed.

Geometry has its own rules: block levels rise and stay within
`max_levels`, footprints are smallest-to-largest and no wider than a
settlement plot, window bands are rising pairs inside a level, the window
pitch is positive, and `chance` is a percentage. `make test-assets` runs
all of it against the embedded set, and round-trips the set through the
serialiser and through an exported directory.

## The property catalogue

Four groups of properties ride on top of a row's own fields, resolved by
`src/properties.rs` and catalogued with types and defaults in
[properties.md](properties.md). **Identity** is `description`, `category`
and `aliases`, on every row. **Hooks** are `tags`, `emits`, `affects` and
`reach`, on species, props, blocks and creatures. **Physical** covers
passability, sight, snow cover, height and slope limits, spacing, verbs,
yields, growth, decay, sway, heat and fuel, on species, props and blocks.
**Conditions** are the wet, dry, weathered, mossy and soiled factors, a
colour per condition, and a state list. They are validated and stored, and
only `wet_darkening` on the surface rows is read today: rain darkens sand,
dirt and rock by the world's wetness times that factor. The rest are the
seams for a future interaction graph. The `campfire` prop is the worked
example — it names the `campfire` light, emits light and heat, and affects
flammable things within 1.5 tiles.

## The editor

`roguemap-edit` is a second binary over the same library, so its previews
are the game's renderer ([ADR-003](adr/ADR-003-asset-editor.md)).

```
roguemap-edit DIR              edit the tables in DIR
roguemap-edit --export DIR     write the embedded set to DIR, then edit it
make edit                      the same on ./assets-export
```

With no directory it uses `ROGUEMAP_ASSETS`; with neither it exits with a
message, since the embedded set has nowhere to be written back to.

### Panes

Three focusable panes cycle with `Tab`: **tables**, **rows** and the
**form**. The left column is twenty cells wide and holds the table list
over the row list. To its right is the preview strip — one pane per sprite
tier, `tiny 2x1`, `small 4x1`, `medium 8x2` and `large 16x4` — drawing a
flat fixture in the chosen biome and season with the selected row at its
centre: a tree, a block pattern, four of a prop, a creature, a lit light
at night, a house in a material. Under the strip is the row form, or the
art grid, then a status line. Below 120 by 50 the strip shows one tier and
`t` cycles it; the floor is 80 by 25, as for the game. Thirteen tables are
listed: `biomes species materials props blocks creatures seasons surfaces
density lights settings tilesets art`, where `seasons`, `surfaces` and
`density` are the three sections of `surfaces.toml`. `ui.toml` and
`tree_styles.toml` load and save so the set round-trips, but have no pane;
edit them by hand.

### Keys

| Key | Normal mode |
|---|---|
| `Tab` / `Shift-Tab` | cycle the panes |
| arrows or `h j k l` | move; left and right switch table, or cycle a choice field in place; `PgUp` / `PgDn` and `Home` / `End` page and jump |
| `Enter` | edit the field under the cursor; on an art entry, open the grid |
| `n` / `d` | add a row, a copy named `-copy` / delete the row, after `y` |
| `x` | clear an optional field so its default applies |
| `u` / `U` | undo / redo, a hundred deep |
| `s` / `S` | save the current table's file / every changed file |
| `b` / `B`, `[` / `]`, `,` / `.` | step the preview biome, the season and the hour |
| `g`, `r` / `R`, `W` | glyph set, rotate a quarter turn, weather preset |
| `t`, `P`, `a` | preview tier, block pattern or tree variant, animate |
| `q` | quit, asking once if anything is unsaved |

`Ctrl-C` quits from any mode without asking. In **field mode** text and
numbers get a line editor; enum, reference and boolean fields cycle with
left and right; a colour picks a channel with left and right and steps it
by one with up and down or sixteen with `PgUp` and `PgDn`; terrain and
cover lists are checklists toggled with `Space`. `Enter` commits and `Esc`
cancels; a value that fails its check keeps the editor open with the
reason, and a reference to a row that does not exist is kept with a
warning and refused at save.

In **grid mode** arrows move, a printable key places itself and advances,
`Space` clears, `i` and `X` insert or delete a row, `>` and `<` widen and
narrow, `c` sets `center` to the cursor column, `B` sets `base_rows` from
the cursor row down, `p` opens a glyph picker over the tileset's roles and
the Legacy Computing block, `Tab` undoes and `Esc` returns.

### Saving

A save renders every file and validates the whole set exactly as the game
loads it. If validation fails, nothing at all is written: the error names
the file and row, the editor jumps there, and the status line says why.
Files are written to a `.tmp` beside the target and renamed over it, so a
save is atomic; no backup is kept. Row order survives, and so does a
file's leading comment block. Comments inside the body do not, because the
round-trip goes through the typed struct, and for the same reason optional
fields at their defaults are dropped and a field the schema does not know
is dropped silently. `s` writes the current table's file whether
or not it changed, so it will reformat a file you only looked at; `S`
writes the files that changed. After a write the directory is reloaded
from disk, so the preview shows what the game will load. A row cannot be
deleted if another table references it by name, if it is the last row, or
if its table is a fixed-shape one — seasons, surfaces, density, tilesets,
art.

### Headless

```
make edit-snap OUT=editor.png ARGS="table=props row=campfire grid=1"
```

That wraps `roguemap-edit --snap W H out.cells key=value...`, whose keys
are `dir`, `table`, `row`, `biome`, `season`, `tod`, `glyphs`, `tier`,
`pattern`, `deg`, `pane` and `grid=1`, the last of which opens the row's
first art file in the grid.

## Adding a row

**A species.** In `species.toml`, `name`, `description`, `form`, `size` in
metres and the `canopy` and `canopy_glyph` colours are required — one
triple for an evergreen, four in spring, summer, autumn, winter order for
a shedding tree. To grow it from a grammar add `shape = "lsystem"` and
`style` naming one of the eight habits, then override whichever parameters
you want ([trees.md](trees.md)). To make it appear in the world, add it to
a biome's `species` list with a non-zero weight.

**A prop.** In `props.toml`, `name`, `description`, `art` naming an art
file, `size`, `glyph` colour, `density` and a non-empty `terrain` list are
required. `color` defaults to black, which means the glyph is drawn over
the ground with no background; `cover`, `near_water` and `min_zoom` are
optional.

**A block.** In `blocks.toml`, `name`, `description`, `terrain` and `size`
are required and everything else has a default. Give it `levels`,
`footprint`, a `roof` profile, a `material` rule and a `windows` band to
make it a building, and a `settle_min` and `chance` to let the settlement
generator place it; `chance` at zero means it is placed only by hand.

**A light.** In `lights.toml`, `name`, `description`, `color` as three
floats in 0 to 1, `radius` in metres and `intensity` are required;
`falloff` defaults to 2 and the two flicker fields to zero. Reference it
from a species, prop, block or creature with `light = "<name>"`.

## Art files

A sprite file is a header line, with `name` and `tier` required, then rows
of glyphs:

```
# name=player tier=large center=4 base_rows=0
   ___
  (o o)
   \_/
 __|=|__
/  |=|  \
```

Spaces are transparent, tabs are an error, trailing blank rows are dropped
and rows are padded to one width on load. The four tiers are `tiny`,
`small`, `medium` and `large`, and their first zooms are 0, 1, 2 and 3 —
one per zoom level, which `min_zoom=N` can move. At draw time a sprite
picks the tier whose row count is nearest what the thing stands at that
zoom. The full format is in [assets.md](assets.md). Buildings and trees
have no art at all: they are geometry, textured with the tileset's
vocabulary — `pine_fill`, `round_mid`, `cactus`, `trunk`, `roof_fill`,
`door`, `window` — as [rendering.md](rendering.md) describes.
