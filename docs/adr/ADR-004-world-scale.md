# ADR-004: World scale in metres, zoom as exact ratios, and the inset view

## Context

Heights are in rows and tiles are unitless. At the closest zoom a person
is 9 rows and a tree 16, so the person reads as tens of metres tall and
the world as a model. Zoom footprints (2x1, 3x1, 4x1, 6x2, 8x2, 12x3,
16x4) give ratios between moves that are not whole numbers. The owner's
intent: the character is 2 metres tall and everything scales around
that; the smallest unit is one centimetre; one keypress at the closest
zoom moves one block on screen and every other zoom has a stated ratio;
a secondary view always shows the other end of the scale.

## Decision

**Units.** World positions are metres as `f32`; anything stored discretely
is centimetres as `i32`. A tile is 2 m by 2 m. Heights, radii and reaches
in every asset table are metres. Terrain heights are metres: sea level 0,
relief up to 120 m in ranges, a sea bed 12 m down, a valley floor a few
metres above the water. The person is 2.0 m. A house level is 3 m, a barn
5 m, a tower level 4 m. Trees are 1 m (sagebrush) to 30 m (kapok) by
species.

**Zoom levels and ratios.** Four footprints, each an exact halving:

| Name | Footprint | Ratio | Columns per metre | Rows per metre |
|---|---|---|---|---|
| close | 16x4 | 1:1 | 11.3 | 6.0 |
| near | 8x2 | 1:2 | 5.7 | 3.0 |
| mid | 4x1 | 1:4 | 2.8 | 1.5 |
| far | 2x0.5 | 1:8 | 1.4 | 0.75 |

The far footprint was recorded here as 2x1, the whole cells it was drawn
in; [ADR-009](ADR-009-camera-modes-and-controls.md) made it the halving
this table claims, and corrected the cell.

Rows per metre is derived from the footprint so vertical and horizontal
scale agree; projection uses it for heights instead of one row per unit.
One keypress moves one tile at every zoom: one block at 1:1, half a block
at 1:2, and so on. Shift with an arrow moves eight tiles.

**Inset view.** A secondary pane in a screen corner shows the other end
of the scale, following the character. When the main view is zoomed out
at all (1:2, 1:4, 1:8), the inset shows the character at 1:1. When the
main view is at 1:1, the inset shows 1:8. Two views never share a level.
The inset is a second Camera and Renderer over the same Scene, sized
about a quarter of the screen width, hidden below 100 columns, with a
settings row for corner and off.

**Consequences for rendering.** At 1:1 a 20 m tree is 120 rows and a
person stands under its canopy; at 1:8 it is 15 rows. Only geometry
renders that range, so this decision depends on ADR-002 landing first;
billboards remain for the person and small props, whose art tiers are
sized in metres (a tier is drawn at the rows its height implies). Level
of detail keys off rows per metre rather than the footprint.

## Consequences

- Assets change units once: heights, radii, reaches and light radii to
  metres. The property catalogue says metres and centimetres.
- The camera gains `rows_per_metre()` and `columns_per_metre()`; the
  height field, stacks and volumes project through them.
- Terrain generation rescales: height units become metres with a larger
  range and the same noise; SEA and ALPINE constants become metres.
- Zoom footprints 3x1, 6x2 and 12x3 are removed.
- Golden frames are re-recorded; the commit lists the ratios.

## Implementation plan

1. Camera: replace ZOOMS with the four footprints and add the two scale
   functions; project heights through rows per metre.
2. Map: heights in metres (multiply the field by a chosen relief scale,
   set SEA and ALPINE in metres); keep the gameplay tile at 2 m.
3. Assets: convert every height, radius, reach and light radius to metres
   in one commit; update docs/properties.md units.
4. Sprites: art tiers pick by rows per metre; the person is 2 m at every
   zoom (12, 6, 3, 1.5 rows, the last as the one-glyph figure).
5. Inset view: second camera and renderer, corner setting, bias rule.
6. Re-record golden frames; update README and CLAUDE.md.
