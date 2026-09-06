# ADR-003: Asset editor

Status: Proposed
Date: 2026-09-06
Deciders: @aaronsb, @claude

## Context

`docs/assets.md` asks for an editor that lists assets by table, previews one
at every zoom tier in a chosen biome and season, edits rows and art grids,
and writes back to the asset directory. ADR-001 gives it tables to edit and
`Assets::from_strings` to validate with; ADR-002 gives it stacks to paint.

The code today: one binary; `src/main.rs` declares every module and holds
the game loop and the headless `snapshot()`, `src/input.rs` the keys,
`src/ui.rs` the HUD and the settings popover (a framed text box on a
canvas). `Renderer::draw` renders into any `Canvas` from a `Scene` with
`RenderOptions { aa, clouds }`; `Terminal` wraps crossterm with a diffed
present. `Map` is a pure function of position and seed with no way to
place a tile by hand; `World` has entities and lights but no placed props.

Where the notes and the decision differ: `docs/assets.md` says "a TUI
editor mode in the same binary". The project direction of 2026-09-06 is a
separate binary over a shared library, which this ADR records. The notes
also say art is "edited as a grid" and rows "in place"; both stand.

## Decision

### Crate layout

The crate becomes a library plus two binaries:

```
src/lib.rs            pub mod assets, biome, camera, canvas, input, lighting, map, noise,
                      overlay, palette, raster, render, settings, snapshot, sprite, sprites,
                      terminal, tileset, ui, world, worldmap, editor
src/bin/roguemap.rs        the game: argument parsing and the event loop
src/bin/roguemap-edit.rs   the editor: argument parsing and the event loop
src/editor/{mod,document,fields,fixture,preview,ui,keys}.rs
```

`Cargo.toml` lists both `[[bin]]`s with `default-run = "roguemap"`, so
`cargo run` and `make run` are unchanged. `snapshot()` moves into the
library as `snapshot::run` so both binaries dump frames the same way. The
editor logic lives in the library (`editor/`) so it is testable without a
terminal; the binary is thin. Both binaries call the same `Assets::load`.

### Invocation

`roguemap-edit DIR` edits the tables in `DIR`. `roguemap-edit --export DIR`
writes the embedded set there first. With no argument it uses
`ROGUEMAP_ASSETS`, and with neither it exits with a message: an embedded
set has nowhere to be written back to. `roguemap-edit --snap W H OUT
[key=value...]` renders one editor screen headless (`dir`, `table`, `row`,
`biome`, `season`, `tod`, `glyphs`, `tier`, `pattern`) for goldens and
documentation screenshots.

### Screen layout

```
┌ tables ─────┬ species: oak   biome temperate forest  summer 12:00  petscii  45deg ─┐
│ biomes      │ tiny 2x1    small 4x1     medium 8x2          large 16x4              │
│>species     │ [14x9]      [22x13]       [36x22]             [56x34]                 │
│ materials   │                                                                        │
│ ...         ├ row ───────────────────────────────────────────────────────────────────┤
│ rows        │  name         oak                                                      │
│>oak         │  form         < broadleaf >                                            │
│ birch       │  scale        1.0                                                      │
│ ...         │  canopy       [72,142,60] [40,110,50] [180,100,40] [105,82,64]         │
├ status ─────┴────────────────────────────────────────────────────────────────────────┤
│ species.toml *   s save  u undo  Tab pane  b biome  [ ] season  , . time  g glyphs   │
```

Left pane, 20 columns: the table list above, the current table's rows
below (`+ art` entries appear under the row that references them). Right
pane: the preview strip on top (one framed pane per tier, the pane height
set by the `large` tier, 34 rows), then the row form or the art grid, then
a one-line status. Below 120x50 the strip shows one tier (`t` cycles) and
the panes shrink to fit; 80x25 is the floor, as for the game.

### Previews

A `Fixture` is a small bounded `Map` built by `Map::fixture(FixtureSpec)`:
12x12 tiles, flat at `z = 6` (`hf = 6.5`), all one biome, `terrain` the
subject's first allowed terrain (grass by default), `temp` a representative
value per Köppen code, biome grass density, no generated trees or stacks.
The subject sits at the centre through hooks the library gains for this:

- tree: `Map::set_tile(x, y, Tile)` override (same mechanism as ADR-002's
  `set_stack`), species and variant from the row; `P` cycles variant.
- block: `set_stack` in one of the patterns 1x1, 1x3, 2x2, L (`P` cycles),
  levels 1-3 (`+`/`-`).
- prop: `World.placed: Vec<PlacedProp { x, y, prop }>`, drawn by
  `prop_pass` after the hashed ones; the preview places four at the centre
  tile's sub-cells.
- creature: `Entity` gains `creature: u8`; the fixture pushes one.
- biome, material, surface, light, tileset rows: a plain fixture in that
  biome (material: a 2x2 house; light: a placed light at night).

Each tier has its own `Renderer::new(w, h)`, `Canvas` and `Camera`
(`set_zoom` to 1, 3, 5 or 6, `look_at` the centre, `rotate_by` on `r`).
`Renderer::draw` runs with `RenderOptions { aa, clouds: false }` and no
`ui::hud` call; the fixture `World` carries the chosen season, hour and
weather; `t` is fixed at 3.0 unless `a` toggles animation. The four
canvases are blitted into the screen canvas at the pane origins. This is
the real renderer with no preview-only path, so what the editor shows is
what `--snap` and the game draw. A change re-renders on the next frame,
not per keystroke.

### Key model

Three modes, shown in the status line. Keys shared with the game keep
their meaning: `[ ]` season, `, .` hour, `g` glyph set, `r` rotate, `W`
weather, `Ctrl-C` quits everywhere.

**Normal**: `Tab`/`Shift-Tab` cycle panes (tables, rows, form/grid); `j k`
or arrows move; `h l` switch table in the tables pane; `Enter` edits the
field under the cursor (or opens the art grid on an art entry); `n` adds a
row as a copy of the current one with `-copy` appended to `name`; `d`
deletes the row after `y`, refused with a message when another table
references it; `u` undo, `U` redo; `s` saves the current table, `S` all;
`b`/`B` cycle biome; `t` tier (small screens); `P` pattern or variant;
`+ -` levels; `q` quits, asking once if anything is unsaved.

**Field**: a line editor for text and numbers (`Left Right Backspace`,
`Enter` commits, `Esc` cancels); enum and reference fields cycle with
`Left`/`Right` through the allowed names; colour fields select a channel
with `Left`/`Right` and step it with `Up`/`Down` (1) or `PgUp`/`PgDn`
(16); seasonal colours have four such groups; lists (`terrain`, `species`)
open a checklist or a name-and-weight sublist with `Space` to toggle.

**Grid** (art): arrows move the cursor; a printable key places that
glyph; `Space` clears; `i`/`X` insert or delete a row; `>`/`<` widen or
narrow; `c` sets `center` to the cursor column; `B` sets `base_rows` from
the cursor row; `p` opens a glyph picker listing the current tileset's
roles and the Symbols for Legacy Computing block; `Esc` returns to Normal.

Events map to an `Action` enum in `editor/keys.rs` through per-mode
lookup tables, the same pattern as `input::lookup` in `src/input.rs`;
`Document::apply` consumes actions. The UI never mutates state directly.

### Document model and validation

`Document` holds the raw row tables from ADR-001's `schema.rs`
(`Serialize + Deserialize`), one `dirty` flag per table, and an undo stack
of table snapshots capped at 100 (tables are a few KB). Field descriptors
(`editor/fields.rs`: name, kind `Str | U8 {max} | F32 {min, max} | Bool |
Enum | Rgb | SeasonalRgb | Ref(table) | List`) drive both the form and
per-field checks on commit: type, range, enum membership, referenced name
present (a warning while editing, an error on save).

Save: serialise the dirty tables with `toml::to_string_pretty`, combine
with the untouched files, and run `Assets::from_strings` on the whole set.
Any `AssetError` is shown in the status with file and row, the cursor
jumps there, and nothing is written. On success each file is written to
`<name>.tmp` and renamed over the original; the leading `#` comment block
of the original is kept and the body regenerated; row order is preserved
exactly (ADR-001 makes it load-bearing). Art files are rewritten with a
regenerated header (`name`, `tier`, `center`, `base_rows`) and rows padded
to one width; tabs are rejected at commit. After a save the `Assets` and
tilesets are reloaded from the directory, so the preview shows what the
game will load.

## Consequences

### Positive
- The game binary carries no editor code or key handling; the editor has
  the whole renderer and the same loader, so previews cannot drift.
- `Action`-driven state is unit-testable; `--snap` puts editor screens in
  the golden set and the README gallery.
- `Map::fixture`, `set_tile`, `World.placed` and `Entity.creature` are
  reusable by tests and by the settlement system.

### Negative
- Restructuring into lib plus bins touches `main.rs` while it is being
  refactored; do it as the first step of that work, or right after.
- TOML round-trips drop comments inside the body; only the head comment
  survives. Authors who annotate rows should use a `note` field instead
  (ADR-001 tables accept unknown fields only if `deny_unknown_fields` is
  off; keep it off for `note`).
- Four renderers per frame are four full draws of small canvases; at the
  pane sizes above that is about a fifth of a game frame, fine at 25 fps.
- Settings and tileset edits take effect in the game on restart only.

### Neutral
- The editor needs a terminal with the same fonts as the game.
- No mouse; consistent with the game.

## Alternatives considered

- **Editor mode in the game binary** (the notes' plan): one binary, but the
  game loop takes on modal editor state and a larger binary; superseded by
  the project direction of 2026-09-06.
- **Edit TOML by hand with `ROGUEMAP_ASSETS`**: already works after
  ADR-001 and stays the fallback; it gives no preview and no grid editing.
- **A web or GUI editor**: a second renderer, which defeats "preview with
  the real renderer".
- **`ratatui` for widgets**: a dependency for boxes and lists the `Canvas`
  and `popover` code already draw; not worth it at this size.
- **Comment-preserving `toml_edit`**: keeps comments but doubles the
  serialisation code path; revisit if hand-written comments matter.

## Implementation plan

1. Crate split: `src/lib.rs` with `pub mod` for every module; move the game
   loop to `src/bin/roguemap.rs` and `snapshot()` to `src/snapshot.rs`;
   `[[bin]]` entries and `default-run` in `Cargo.toml`. `make golden-check`
   byte-identical.
2. Library hooks: `Map::fixture(FixtureSpec)`, `Map::set_tile` overrides
   (shared with ADR-002's `set_stack`), `World.placed` drawn by
   `prop_pass`, `Entity.creature`; unit tests for each.
3. `editor/document.rs`: load raw tables from a directory, dirty flags,
   undo, `apply(Action)`, `to_files()`, `validate()` via
   `Assets::from_strings`, atomic `save()` with head-comment retention.
4. `editor/fields.rs`: descriptors for every table and the art header.
5. `editor/fixture.rs` and `editor/preview.rs`: tier panes, renderers,
   cameras, blit; a test renders the oak fixture at each tier and checks
   the centre column is not sky.
6. `editor/ui.rs` (layout, panes, form, grid, status, small-screen rules)
   and `editor/keys.rs` (`Action` mapping for the three modes).
7. `src/bin/roguemap-edit.rs`: arguments (`DIR`, `--export`, `--snap`),
   terminal loop: event, action, apply, draw, present.
8. Tests: add/edit/delete/undo on a `Document`; save then `Assets::from_dir`
   equals the in-memory set; a bad reference leaves the file untouched and
   names the row; `--snap` editor frame added to `tools/golden.sh`.
9. `Makefile`: `edit` target (`roguemap-edit assets`); `screenshots` adds
   an editor frame; README and `docs/assets.md` updated for the two
   binaries.
