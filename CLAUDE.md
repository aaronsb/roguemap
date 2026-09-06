# roguemap

Terminal isometric terrain renderer in Rust. A library crate with two
binaries: `roguemap` (the game) and `roguemap-edit` (the asset editor).

## Commands

- `make build`, `make run`, `make term` (Konsole with the canonical font)
- `make snap OUT=x.png ARGS="fill=1 zoom=2 tod=21 fire=1"` renders one
  headless frame to PNG; this is how to look at a change
- `make screenshots` regenerates docs/screenshots
- `make check` is the gate: clippy with warnings as errors, then all tests
  including the golden frame comparison
- `make golden-record` re-records the reference frames after an intended
  visual change; say why in the commit

## Development flow

Work in cycles. During a cycle, iterate on look and feel: change, `make
snap`, look at the PNG, adjust. Do not write tests for behaviour that is
still moving. At the end of a cycle, run `make check`, write the tests
that pin what the cycle settled (see docs/testing.md for what kinds), and
re-record golden frames with the reason. Golden comparison is fuzzy (a
percentage of identical cells and a mean colour distance), so small
intended tweaks pass and broken frames fail; use `GOLDEN_STRICT=1` only
when exactness matters.

## What goes where

- Anything that describes a thing in the world is an asset under
  `assets/` (TOML tables, text art), never a Rust constant or a match arm.
  The property catalogue is docs/properties.md; the layout is
  docs/assets.md and docs/adr/ADR-001. Add rows, not code.
- Large things are block geometry, not sprites: docs/structures.md and
  docs/adr/ADR-002. Trees are volumes on the same walk. Billboards remain
  only for creatures and small props.
- The renderer is modules under src/: camera, raster (ray walk and
  antialiasing), sprites, lighting, overlay, ui, with render.rs as the
  frame buffer and Scene as the per-frame context. Do not add parameters
  to the passes; extend Scene.
- Settings are one keyed table; keybindings are one table in input.rs;
  help lines are generated from it.

## Look

The snapshot renderer (tools/cells2png.py, Unscii 16 at 8x16 pixels) is
the canonical look; a terminal matches it with that font at 16 px, no
smoothing, no bold brightening. Everything must work from 80x25 up. The
terrain is a continuous field at every zoom: no visible tile blocks, no
stair-stepped contours; terraces exist only as gameplay height.

## Agents

Builders run on isolated worktrees with `make check` and the golden test
as their gate, and commit on their branch. Reviews are read-only. Give
every agent the relevant doc as its brief rather than restating it.
