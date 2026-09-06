# Contributing

roguemap is a Rust library crate with two binaries, `roguemap` (the game)
and `roguemap-edit` (the asset editor). Stable Rust 1.87 or newer, no
system dependencies beyond a terminal with a font that covers the Symbols
for Legacy Computing block (Unscii 16 is the reference; Adwaita Mono works).

## Build and check

```
make build          # release binaries
make run            # the game in this terminal
make check          # the gate: clippy -D warnings, tests, golden frames
make format         # cargo fmt over the tree
make snap OUT=x.png ARGS="fill=1 zoom=2 tod=21 fire=1"   # look at a change
```

`make check` must be green before a pull request. CI runs the same gates
and adds `cargo fmt --check`, so run `make format` before pushing.

## How work goes

Iterate on look and feel first: change, `make snap`, look at the PNG,
adjust. Write tests at the end of a cycle for what settled, not during.
The golden frame comparison is fuzzy (98 percent identical cells, mean
colour distance 2); re-record with `make golden-record` after an
intended visual change and say why in the commit message.

## Where things go

- Anything that describes a thing in the world is a row in `assets/`
  (TOML tables, text art), never a Rust constant or a match arm. The
  property catalogue is `docs/properties.md`.
- Large things are geometry on the ray walk (`docs/structures.md`);
  billboards are only for creatures and small props.
- Anything drawn over the scene is a frame: a row in `assets/ui.toml`
  plus a content kind (`docs/adr/ADR-005-overlay-frames.md`).
- Design decisions are recorded in `docs/adr/`. A change that sets a new
  direction gets a record before code.

`docs/index.md` is the map of the design. `CLAUDE.md` holds the same rules
for agents working in the tree.

## Style

`cargo fmt` with the repository's `rustfmt.toml`; clippy clean with
warnings as errors; doc comments on public items; one idea per sentence in
prose.

## License

Contributions are accepted under the same terms as the project: MIT or
Apache 2.0 at the user's option. By submitting a change you agree to
license it that way.
