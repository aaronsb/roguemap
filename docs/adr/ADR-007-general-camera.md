# ADR-007: A general camera, with the isometric view as a mode

Builds on [ADR-004](ADR-004-world-scale.md), whose zooms, ratios and rows
per metre stand, and [ADR-006](ADR-006-movement-by-screen-cell.md), whose
movement rule holds in the isometric mode.

## Context

Every screen cell casts a ray into a continuous height field, block
geometry and grown trees; there is no tile atlas left anywhere in the
renderer. The camera is the last tile-era piece. `Camera` is a heading, a
screen offset and a tile footprint `(hw, hh)`, and its projection is

```
sx = a (x cos - y sin) + ox              a = hw sqrt 2
sy = b (x sin + y cos) - z rpm + oy      b = hh sqrt 2, rpm = 3 hw / 8
```

That is an oblique orthographic projection whose pitch is never written
down: it is whatever the ratio of `b` to `rpm` implies, and it changes
with the footprint. The heading is continuous and the zoom is a step
through four footprints. The owner's words: "with the amount of effort
we've put into things, the iso perspective camera doesn't make sense
anymore", and "if we change away from it, I'd like to still keep an
isometric camera mode though."

Three open issues need a camera that is not a footprint: per-degree
rotation (#19) is a yaw; a chase view behind the character and a first
person view (#20) are an eye and a pitch; atmospheric depth (#21) needs a
distance from the eye to fade by. None of them can be expressed against
`(hw, hh)`.

The projection is also not in one place. The walk in `raster.rs` builds
its own ray from the heading, `b` and rows per metre; the cloud parallax
in `overlay.rs` works from a virtual altitude chosen by footprint; the
ground texture, the door and window widths and the wind switch in
`raster.rs`, the cloud layer switch in `overlay.rs`, the prop shadow
threshold in `shadow.rs` and the editor's preview panes in
`editor/preview.rs` each do their own footprint arithmetic; the inset and
the preview each assemble a second camera by hand; the walk and the prop
pass each compute depth from the heading. A camera change would have to
be made in every one of those.

What the footprint implies, derived rather than assumed: a metre of
ground depth toward the camera is `b / TILE_METRES` rows and a metre of
height is `rpm` rows, so for a pitched orthographic view with vertical
scale `S` rows per metre, `S sin(pitch) = b / 2` and `S cos(pitch) = rpm`,
and `tan(pitch) = 4 sqrt 2 hh / (3 hw)`. For the three 4:1 footprints that
is `atan(sqrt 2 / 3)`, 25.24 degrees above the horizon — a hair flatter
than the 26.57 of classic 2:1 pixel isometry. The far zoom's 2x1
footprint is `atan(2 sqrt 2 / 3)`, 43.31 degrees: the overview has always
been a steeper camera than the other three, because a 2x0.5 footprint
cannot be drawn in whole cells. So "the isometric pitch" is two numbers,
and the general camera has to carry both.

## Decision

**A camera is an eye, a view direction, a field of view and a scale.**
The view direction is a yaw (the existing heading, `angle`) and a pitch
in radians above the horizon. The field of view in radians is zero for an
orthographic view; a positive one is perspective. The scale is columns per
metre across the screen and rows per metre up it. The eye is the target
— the point at the screen centre at `focus_z` — pushed back along the
view direction; in an orthographic view the distance is immaterial and is
not stored until perspective needs it. The screen offset `(ox, oy)` and
`focus_z` stay as they are.

An orthographic camera projects through a basis of three numbers, the
products of the scale with the pitch: columns per tile across (`a`), rows
per tile of ground depth (`b = S sin(pitch) * TILE_METRES`) and rows per
metre of height (`rpm = S cos(pitch)`). The formulas above are the
general orthographic formulas with that basis; there is no isometric
special case in `project` or `unproject`. The basis is computed when the
camera is set up, not from the pitch on every call: a camera built from a
pitch and a scale reproduces the isometric basis to a few ulps, and the
isometric constructor owns the exact numbers.

**The isometric view is a mode, not the camera.** `Camera::isometric(zoom)`
builds the orthographic camera from a footprint preset: the basis from
`(hw, hh)` as before, and the pitch as the number the basis implies, pinned
by a test. Its four scales are ADR-004's ratios and rows per metre,
unchanged; its yardstick, its golden frames and its keys stay. The mode is
a row of the settings table, `camera`, with the values `isometric` now and
`perspective` when stage 2 lands; the values are the camera's own list,
and a test keeps the two in step.

**Rays come from the camera.** `Camera::ray(sx, sy)` gives the ray through
a screen cell as a ground point at height zero and a drift in tiles per
metre of height, `p(z) = p0 + d z`, which is how the walk already marches:
parameterised by height, stepping down. In an orthographic view every
cell's drift is the same; in a perspective view it is the eye's line
through the cell, and any straight line that is not horizontal has the
same form. The walk asks the camera for its ray and knows nothing else
about the projection.

**The camera is the one place that maps the world to the screen.** Every
module that projects, unprojects, or reasons about the footprint or the
pitch goes through a `Camera` method, and a quantity two modules derive is
derived once:

| Method | What it owns | Was |
|---|---|---|
| `ray(sx, sy)` | the ray through a cell | built in `raster::ray_from` |
| `forward()`, `right()` | the map-space unit vectors toward the camera and screen-right | `angle.sin_cos()` unpacked in `raster` |
| `depth(x, y)` | depth of a point for the sprite sort | `x * fx + y * fy` in `raster` and `sprites` |
| `project_vector(run, rise)` | a world displacement in cells | `raster::stroke_dir` |
| `footprint()` | the cells a tile spans | `2 * hw` in `raster::ground_hash`, `pan`, the cloud test |
| `cloud_view(w, h)` | the cloud plane's parallax, C/(C-H) | `overlay::CloudView` |
| `inset()` | the second camera at the other end of the scale | assembled in `ui::Inset` |
| `focus(sw, sh)` | the target under the screen centre | inlined in `set_zoom` and `rotate_by` |
| `Camera::isometric(zoom)` | a preset's scale, for the editor's panes and `fitting_zoom` | `rows_per_metre_of(ZOOMS[..].0)` |

Level of detail stays keyed off rows per metre, and its thresholds live in
one table, `raster::lod_of`: the wind, the cloud layer and prop shadows
join the roof profiles, models, glyph bands and crown seams there instead
of comparing `hw` in three modules. The world map is not on this list: it
is a top-down plot at whole tiles per cell with a cursor, not a view of
the scene, and stays integer arithmetic of its own.

**Perspective is stage 2.** A perspective camera casts rays from the eye
through each cell: `ray` returns a per-cell drift and the walk is
unchanged for rays that descend. It needs a fog distance to fade the far
field into the sky (#21); a chase preset behind the character and a
first-person preset at the character's eye (#20); rows per metre becomes
a function of depth, evaluated at the character's depth, so that level of
detail and the sprite tiers of ADR-004 keep one answer per frame; the yaw
is per-degree rotation (#19). The cloud layer becomes the plane at cloud
altitude seen from the real eye rather than from a virtual one. The
shadow mask is a map-space sweep along the sun and does not depend on
the camera beyond the resolution it is laid at. Movement by screen cell
(ADR-006) holds in the isometric mode; the perspective mode walks by
heading and creature speed, which is its own decision.

## Out of scope

Perspective itself, the fog, the presets and walking by heading are stage
2. Parameterising the walk by distance along the ray, which a horizontal
first-person ray needs because its drift per metre of height is
unbounded, is a stage 2 cost and is not paid here.

## Consequences

- Stage 1 moves no pixel. `project`, `unproject`, `rows_per_metre`,
  `columns_per_metre`, `anchor_at`, `cell_step`, `tile_depth`,
  `screen_dir_to_map`, the `look_at` family, `follow`, `inset_zoom` and
  `zoom_name` keep their signatures and their results to the bit; the old
  `project` stays in the test module and a lattice of points at every zoom
  and several headings is asserted equal; the golden frames pass with
  every cell identical. The one frame that changes is `settings`, which
  gains the `Camera` row.
- The pitch is a number on the camera and in [scale.md](../scale.md), and
  the far zoom is documented as the steeper view it has always been.
- The consolidation touches `raster`, `overlay`, `shadow`, `sprites`,
  `ui`, `editor/preview`, `snapshot`, `grid` and `render`, each becoming a
  caller of a `Camera` method; `hw` and `hh` remain on the camera as the
  preset's footprint, read outside `camera.rs` only to name a pane.
  `rows_per_metre_of` goes: the scale of a zoom is reached through a
  camera and nowhere else.
- The basis is stored rather than recomputed from the pitch, so a caller
  that sets `pitch` directly changes nothing until it rebuilds the camera;
  in stage 1 nothing sets it but the constructors. That is the trade for
  bit-exactness, and stage 2's perspective constructor is where it will
  matter. The yaw's sine and cosine are cached the same way, so the
  heading is a private field set through `set_angle`; every projection
  reads the cache instead of calling `sin_cos`, which is where the
  overview got slightly cheaper.
- The walk keeps its height parameterisation. Perspective rays that
  descend fit it; horizontal ones do not, which bounds how low a
  first-person pitch can go until the walk is reparameterised.
- Cost: the ray function is the same arithmetic the walk did inline; the
  stand at 1:8 and 1:1 is measured before and after and reported in the
  commit, and no regression beyond noise is accepted.

## Implementation plan

Stage 1, this branch, byte-identical:

1. `Camera` gains `pitch`, `fov` and the basis; `Camera::isometric(zoom)`
   builds a preset; `new()` is the far preset. The test module keeps the
   old formulas and asserts equality, and pins the two pitches.
2. `Camera::ray`; `raster::ray_from` uses it.
3. The consolidation: the methods in the table above and their call
   sites; the level-of-detail switches into `lod_of`.
4. The `camera` settings row, `Camera::MODES`, the required key, the
   length test; the `settings` golden frame re-recorded for its new row.
5. Docs: the camera section and "what the camera owns" in
   [rendering.md](../rendering.md), the pitch in [scale.md](../scale.md),
   the bit-exact tests in [testing.md](../testing.md), the settings row in
   [frames.md](../frames.md).

Stage 2, perspective:

6. `Camera::perspective(eye, yaw, pitch, fov)` and `ray` from the eye; the
   `perspective` value of the settings row; rows per metre at the
   character's depth.
7. Fog distance (#21) as a `Scene` field the light pass reads.
8. The chase and first-person presets (#20); per-degree rotation (#19)
   as the yaw on both modes.
9. The cloud plane from the eye; walking by heading and creature speed;
   golden frames for the perspective mode.
