# ADR-001: Assets as files

Status: Proposed
Date: 2026-09-06
Deciders: @aaronsb, @claude

## Context

Every thing in the world is a Rust `const` table: `SPECIES`, `BIOMES`,
`MATERIALS`, `PROPS` in `src/biome.rs`; the glyph vocabularies
(`ASCII_ART`, `PETSCII_ART`) and `Tileset` roles in `src/tileset.rs` (the
sprite builders now live in `src/sprite.rs`); `ITEMS` in `src/settings.rs`;
the seasonal `Palette`s in `src/palette.rs`; light specs inline in
`World::add_campfire` and `Renderer::sprite_pass`. Adding a species means a
rebuild, and the editor in ADR-003 cannot write to a `const`.

Where `docs/assets.md` and the code disagree today:

- The note lists `buildings.toml`; `docs/structures.md` calls it
  `blocks.toml`. Code has no building table: houses are the procedural
  `house()` builder, material picked in `sprite_pass` (biome material,
  stone on rock or near the snow line). This ADR uses `blocks.toml`.
- The note lists `creatures.toml`; code has only `player_for(hw)`.
- The note lists `surfaces.toml` as "per-terrain colour and texture
  density"; code keeps colours in the seasonal `Palette` and the densities
  as literals in `Renderer::shade` (0.14 sand, 0.22 dirt, 0.24 rock, 0.18
  snow, grass `0.14 * tile.grass + 0.04`).
- The art header says `base_rows`; `Sprite` calls it `trunk_rows`. Props
  are said to carry "art references"; code holds the rows inline.
- The note and README say the settings table is the single source for the
  shortcut keys; they are hard-coded match arms in `src/input.rs`.
- `Species` has a continuous `scale`, not the note's "size class";
  `sprite_pass` picks the variant pair by `scale < 0.9` / `> 1.2`.
- `biome::classify` returns a position in `BIOMES`; that coupling must go.

Two hard constraints: `make golden-check` stays byte-identical through the
migration (hash-driven picks depend on row order), and the binary stays
standalone.

## Decision

Tables live in `assets/` as TOML, embedded with `include_str!` and
overridable with `ROGUEMAP_ASSETS=<dir>`. One `Assets` struct holds every
table, is built once at start-up, and fails with an error that names the
file and row. Only two dependencies are added: `serde` (derive) and `toml`.

The loader lives in the library crate (`src/lib.rs`, see ADR-003), so the
game binary and the editor binary load and validate the same tables with
the same code.

### Layout and embedding

```
assets/
  biomes.toml  species.toml  materials.toml  props.toml  blocks.toml
  creatures.toml  surfaces.toml  lights.toml  settings.toml
  tilesets/petscii.toml  tilesets/ascii.toml
  art/**/*.txt
```

`include_str!` takes a literal path and cannot glob, so a `build.rs` (std
only) walks `assets/`, emits `$OUT_DIR/embedded.rs` with `pub const FILES:
&[(&str, &str)]` of (relative path, `include_str!(abs path)`) pairs, and
prints `cargo:rerun-if-changed=assets`. `Assets::load()` reads
`ROGUEMAP_ASSETS`; if set, every relative path is read from that directory
and a missing file is an error (no silent fallback); otherwise the embedded
set is used. `Assets::export(dir)` writes the embedded set out, which is
how an editing session starts.

### Value encodings

- Colour: `[r, g, b]` array of `u8`; `Rgb(u8, u8, u8)` derives serde
  directly. Hex strings need a hand-written deserializer for no gain.
- Glyph: one-character string; serde maps it to `char` natively.
- Enum: lowercase string via `#[serde(rename_all = "lowercase")]`.
- Cross-reference: the referenced row's `name`, resolved to an index at
  load. Weighted lists are pairs `[["oak", 5], ["birch", 3]]` so order is
  preserved (the weighted pick in `Map::tile` depends on it).
- Seasonal colour: either one `[r, g, b]` (evergreen, expands to four) or
  four of them `[spring, summer, autumn, winter]`; `#[serde(untagged)]`.

### Schema per file

Types: `str`, `rgb`, `glyph`, `bool`, `u8`, `f32`, `[T]`, `ref(table)`.
Required unless a default is given. Every row table is an array of tables
(`[[biome]]`, `[[species]]`, ...), and row order in the file is the
runtime index order.

**biomes.toml** `[[biome]]`: `name str` (unique), `koppen str` (unique;
exactly the nine codes `classify` emits must be present), `cover
grass|dry|moss|bare`, `ground rgb`, `ground_glyph rgb`, `seasonal bool =
false`, `grass u8 = 0` (0..=3), `tree_density f32 = 0` (0..=1), `species
[[ref(species), u8 weight > 0]] = []`, `material ref(materials)`.

**species.toml** `[[species]]`: `name str`, `form
pine|broadleaf|scrub|cactus` (the glyph-vocabulary selector), `scale f32 =
1.0` (> 0), `canopy seasonal rgb`, `canopy_glyph seasonal rgb`. ADR-002
adds `shape` and the volume dimensions.

**materials.toml** `[[material]]`: `name str`, `wall rgb`, `wall_glyph
rgb`, `roof rgb`, `roof_glyph rgb`.

**props.toml** `[[prop]]`: `name str`, `art ref(art name)`, `color rgb =
[0,0,0]` (black means transparent background, as today), `glyph rgb`,
`density f32` (chance per 4x4 sub-cell), `terrain [water|sand|grass|dirt|
rock|snow]` (non-empty), `cover [grass|dry|moss|bare] = []`, `near_water
bool = false`, `min_zoom u8 = 3` (today's `cam.hw < 6` gate).

**blocks.toml** `[[block]]`: defined in ADR-002; here only `material`
(`"by_biome"` or `ref(materials)`) and `light` (`ref(lights)`) are checked.

**creatures.toml** `[[creature]]`: `name str`, `art ref(art name)`, `color
rgb`, `glyph rgb`, `terrain [..]` (walkable), `light ref(lights)` optional.
The first row is `player` (bg `[52,74,150]`, fg `[240,214,176]`).

**surfaces.toml**: `[[season]]` exactly four rows in order `spring,
summer, autumn, winter`, each with `name` and every `Palette` field as
`rgb` (`grass, grass_glyph, canopy, canopy_glyph, trunk, trunk_glyph, sand,
sand_glyph, dirt, dirt_glyph, rock, rock_glyph, snow, snow_glyph,
water_shallow, water_deep, water_glyph`); plus one `[density]` table:
`sand f32 = 0.14`, `dirt = 0.22`, `rock = 0.24`, `snow = 0.18`,
`grass_base = 0.04`, `grass_per_level = 0.14`, `cattail = 0.10`.

**lights.toml** `[[light]]`: `name str`, `color [f32; 3]`, `radius f32`,
`intensity f32`, `flicker bool = false`. Seed rows: `campfire`
(`[1.0,0.62,0.22]`, 7.5, 2.2, true) and `window` (`[1.0,0.75,0.4]`, 4.0,
0.8, false; code still scales it by `night`).

**settings.toml** `[[setting]]`: `key str` (unique; code looks rows up by
key, replacing the index constants), `label str`, `values [str]`
(non-empty), `default u8 = 0`, `shortcut glyph` optional. Load fails if a
key the engine needs (`traversal, world, glyphs, hud, clock, weather, wind,
day_length, clouds, antialias`) is missing; the shortcut column makes the
README's "single source for the shortcuts" claim true.

**tilesets/*.toml**: `name str`, `antialias bool`; `[roles]` with every
`Tileset` field (`cover.grass/dry/moss/bare [glyph;3]`, `stubble [glyph;3]`,
`cattail [glyph;2]`, `water [glyph;4]`, `texture.sand/dirt/rock/snow`,
`wall`, `star`, `snowflake [glyph;2]`, `flame [glyph;3]`, `rain glyph`);
`[art]` with every
`Art` field (`pine_l, pine_r, cactus, roof_l, roof_r, roof_fill, wall_fill,
door, window glyph`, `pine_fill [glyph;2]`, `round_top/mid/bot [glyph;3]`,
`trunk [str;3]`, `tiny_house str`, `tiny.pine/broadleaf/scrub/cactus [str]`).

### Art file format

```
# name=house/adobe tier=large center=7 base_rows=2
    ◢▒▒▒▒◣
   ◢▒▒▒▒▒▒◣
  □   ▐   □
```

Line 1 is the header: `#` then `key=value` pairs. `name` and `tier` are
required; `center` defaults to `width / 2`; `base_rows` defaults to 0 and
maps onto `Sprite.trunk_rows` (`src/sprite.rs`). Every following line is a
row; spaces are transparent, tabs an error, trailing blank lines dropped,
rows padded to the widest on load. Tiers: `tiny` (zooms 0-1), `small`
(2-3), `medium` (4-5), `large` (6); a sprite uses the largest tier at or
below its zoom's tier, so `small` plus `large` behaves as today. Art is
indexed by (name, tier) from headers; a duplicate pair is a load error.

### Loader API

```rust
pub struct Assets {
    pub biomes: Vec<Biome>, pub species: Vec<Species>, pub materials: Vec<Material>,
    pub props: Vec<Prop>, pub blocks: Vec<Block>, pub creatures: Vec<Creature>,
    pub surfaces: Surfaces, pub lights: Vec<LightSpec>, pub settings: Vec<SettingItem>,
    pub tilesets: Vec<TilesetSpec>, pub art: ArtIndex,
    pub koppen: HashMap<String, usize>,   // Köppen code -> biome index
    pub source: Source,                   // Embedded | Dir(PathBuf)
}
impl Assets {
    pub fn load() -> Result<Assets, AssetError>;           // env var or embedded
    pub fn from_dir(dir: &Path) -> Result<Assets, AssetError>;
    pub fn from_strings(files: &[(String, String)]) -> Result<Assets, AssetError>;
    pub fn export(&self, dir: &Path) -> io::Result<()>;
}
pub struct AssetError { pub file: String, pub line: Option<usize>,
                        pub row: Option<(String, usize, String)>, pub msg: String }
// Display: `species.toml: species[4] "juniper": unknown form "bush"`
```

Raw row structs (`src/assets/schema.rs`, derive `Serialize + Deserialize`,
names as `String`) are the file format; the resolved structs are today's
`Biome`, `Species`, ... with `String`/`Vec` and indices for references.
`Assets::resolve` converts one to the other and holds every cross-reference
check. Parse errors map `toml::de::Error::span()` to a line; resolve errors
carry table, row index and row name.

## Consequences

### Positive
- New species, biome, prop or light is a file edit, no rebuild.
- Validation runs once at start-up and names the location; the
  `species_and_materials_resolve` test becomes the loader's job.
- `Assets::from_strings` gives ADR-003's editor the boot-time checks.

### Negative
- `&'static str` and slices become `String` and `Vec`; consumers take
  `&Assets` (`Rc<Assets>` in `Map`, which generates chunks lazily inside
  `get(&self)`). Sequence this after the current refactor lands.
- Two structs per table (raw and resolved) is more code than one; the
  `build.rs` step and the embedded strings (tens of KB) are small costs.
- Row order is semantically load-bearing; the editor must never reorder.

### Neutral
- `toml` serialisation drops comments; ADR-003 preserves a leading comment
  block per file and regenerates the body.

## Alternatives considered

- **RON**: closer to Rust syntax, but a second syntax for the same authors,
  weaker editor support, and no advantage for flat record tables.
- **JSON**: no comments, no trailing commas, worse for hand-edited colour
  tables; `serde_json` weighs the same as `toml`.
- **Keep consts, generate them from TOML at build time**: keeps `&'static`
  but kills `ROGUEMAP_ASSETS` live reload and the editor's write-back.
- **Manual embedded manifest instead of `build.rs`**: every new art file
  would need a source edit, which is the thing being removed.
- **Names resolved per frame through a `HashMap`**: hashing is already 15%
  of frame time in the profile.

## Implementation plan

1. `Cargo.toml`: `serde = { version = "1", features = ["derive"] }`, `toml
   = "0.9"` (0.8+ works; only `from_str`, `to_string_pretty`, `span` are
   used), `build = "build.rs"`.
2. Write `build.rs`: walk `assets/`, emit `embedded.rs` as above.
3. `src/assets/schema.rs`: raw row structs and the art header parser;
   `src/assets/mod.rs`: `Assets`, `Source`, `AssetError`, the API above.
4. Generate the TOML from the consts, not by hand: a `#[test] #[ignore]
   fn export_const_tables()` serialises `SPECIES`, `BIOMES`, `MATERIALS`,
   `PROPS`, the `Art`/`Tileset` fields, `ITEMS`, the four `Palette`s and
   the two light specs through the raw structs. Run once, review the diff.
5. Art files for the player (four tiers, `name=player`) and the props
   (`small`, `large`), with the same rows as today.
6. Tests in `src/assets/`: embedded set loads; every cross-reference
   resolves; a broken string fails with the expected file, row and
   message; tier fallback per zoom; `ROGUEMAP_ASSETS=assets` loads
   identically to embedded.
7. Thread `&Assets`: `Map::new(w, h, seed, Rc<Assets>)`;
   `Tileset::from_spec(&TilesetSpec, &Assets)`; `Scene` in `render.rs`
   gains `assets: &Assets` so every pass reads it; `Settings::new(&Assets)`
   with lookups by key; `classify` returns the Köppen code and `Map::tile`
   resolves it via `assets.koppen`. Delete the consts last.
8. `make golden` before step 7, `make golden-check` after: byte-identical.
9. Drop `species_and_materials_resolve`; fix the names in `docs/assets.md`.
