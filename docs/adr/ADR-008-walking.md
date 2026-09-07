# ADR-008: Walking

Amends the movement rule of [ADR-006](ADR-006-movement-by-screen-cell.md)
and closes stage 3 of [ADR-007](ADR-007-general-camera.md). ADR-006's
units, its centimetre position, its per-tile collision and its snapshot
arguments stand.

## Context

ADR-006 made a keypress move the figure one screen cell. It is precise,
and it reads as sliding a piece across a board. The owner's report: "the
player movement is technically better with the individual steps, but
also, feels like moving stake around on the ground. it doesn't feel like a
character moving." The figure is text; what makes text read as a person
walking is the same as anywhere else — it moves at a steady rate through
time, its legs change, it faces where it is going, and the view follows
it with some give rather than jumping.

The event loop already ticks every 40 ms for the clock and the weather.
Every creature row has a `speed` in metres per second that nothing reads.
A terminal delivers no key-up: a held arrow arrives as a press, a pause of
the repeat delay, then a press per repeat interval (on the owner's
desktop, `xset q`: 600 ms then 25 a second, one every 40 ms — the tick).
Sprites are one pose per tier, drawn as they are drawn whichever way the
figure went.

## Decision

**Motion over time.** A press does not move the figure; it sets a heading
and the figure walks. The heading is a unit vector in map space: under the
screen-space traversal setting the ground under the pressed screen
direction at the character's depth (`Camera::heading`), under the
map-axes setting the map axis the key names. Each tick the figure
advances `speed * dt` metres along it, in centimetres, carrying the
fraction of a centimetre a tick leaves over to the next so the distance
walked is the speed times the time to the centimetre. Every tick's step
goes through `World::try_move`, so the walk stops at the tile it may not
enter; a step refused as a whole is tried along each axis alone, so a
diagonal walk along a shore slides along it instead of sticking. There is
no pathing. The player walks at the creature's `speed` — the person 1.4
m/s, a brisk walk — and shift with an arrow runs at `World::RUN` times
it, three.

**A press is a short lease on the heading.** A press sets `Walk::grace`
to `World::GRACE`, a tenth of a second, and each tick spends `dt` of it;
the walk ends when it runs out. A held key renews it every repeat
interval, so a hold is a steady walk, and a release stops the figure
within three ticks. A tap walks the grace's worth, 14 cm. The gap between
a first press and the first repeat is the terminal's repeat delay and
shows as a short pause; the grace is not stretched to hide it, because
that would make every release late. The walk replaces the cell step in
every camera mode; `Camera::cell_step` stays as the camera's account of
what a cell covers, which the snapshot's `player_dx` and the ADR-006
tests still use.

**A walk cycle by distance.** An art file may hold more than one pose: a
line starting with `#` after the first begins another pose of the same
sprite, the same height as the first, sharing the header's `center` and
`base_rows`. The first pose is the figure at rest; the ones after it are
the walk cycle in order. While walking, the pose is picked by distance
walked on this walk: `Entity::walked` metres through a stride of
`World::STRIDE` = 0.7 m, so `n` poses each hold `0.7 / n` metres and the
cadence follows the speed — a run cycles three times faster. Stopping
returns the figure to rest and zeroes the distance, so every walk starts
on its first pose and a snapshot mid-stride is reproducible. The player's
large tier (12 rows) carries four walk poses, contact and passing for
either leg; the medium tier (6 rows) two; the small and tiny tiers keep
their one pose.

**Facing.** The figure faces its direction of travel. A press sets
`Entity::facing` from the screen-space sign of the heading, left or right,
and keeps the last facing when the heading is straight toward or away from
the camera; facing persists when stopped. A figure facing left is drawn
as the mirror of its art: rows reversed and the paired glyphs swapped
(`/` and `\`, the brackets), the centre column mirrored with them. The
mirror is built once when the art loads. A back view is not drawn: the
art would need a fourth axis (name, tier, pose, facing) and nothing in
the front view of a text figure is wrong from behind at these sizes. That
is left.

**The camera eases.** In every mode `Camera::follow` runs once a tick and
moves the camera a fraction, `Camera::EASE` = 0.3, of what is left toward
the figure, instead of recentring in one jump. In the isometric mode the
figure has a dead zone, the middle third of the screen each way: while it
is inside, the view does not move, so a few steps do not scroll the
ground; when it crosses out, the view eases by whole cells (never less
than one while any offset remains) until the figure is back inside, so a
long walk scrolls the ground steadily with the figure held near the
zone's edge. The offset stays whole cells, so the terrain never shifts by
a fraction of a cell. The chase and shoulder views ease the aimed point
toward the character; the first-person view is the character's eye and
snaps, since a lagging eye would look out from behind its own head. The
isometric ease runs only while a walk is settling — after a press until
the figure is back inside the zone — so the pan keys keep their effect;
`c` still snaps.

**Snapshot.** `walk=KEY,SECONDS` holds a walk key (`w`, `a`, `s`, `d`)
for that long in 40 ms ticks through the same `World::step_walk` and
`Camera::follow` the game runs, with `run=1` for the run speed, so a
frame mid-stride is reproducible: the golden frame `stride` is the
yardstick scene at 1:1 with the person 0.3 s into a walk to the right, on
their third pose, and the frames with the player at rest are unchanged to
the bit.

## Out of scope

A back view, pathing, other creatures walking (the walk state is on every
entity, but only the player is stepped), and stretching the grace over
the terminal's repeat delay.

## Consequences

- `Entity` gains `facing`, `walked` and `walk: Option<Walk>`; `Walk` is
  the heading, the grace and the centimetre carry. `World::walk_toward`
  sets it and `World::step_walk` spends a tick of it; `World::STRIDE`,
  `World::RUN` and `World::GRACE` are the constants above. `Entity` is no
  longer `Eq`.
- `Camera::heading` gives the unit map vector a key means; `Camera::follow`
  is the eased step and reports whether it has settled;
  `Camera::look_at_entity` is the snap.
- `ArtFile` and `Sprite` gain `poses`; `ArtIndex::for_rows` takes a
  facing and hands back the mirrored sprite for a figure facing left. The
  editor's grid edits the first pose and writes the others back untouched.
- `creatures.toml`: the player's `speed` is 1.4. The catalogue's `speed`
  is read.
- The event loop moves the walk and the camera in its tick; a key press
  only sets the heading. The history logs where a walk ended rather than
  every press.
- Tests: distance walked over ticks is speed times seconds to the
  centimetre, at walking and running speed; the walk stops at water by the
  tile it would enter, and slides along a shore; the pose index cycles by
  distance and rests at zero; facing follows the heading and persists;
  the camera converges to the figure and does not move inside the dead
  zone; the mirror swaps the paired glyphs and the centre; a multi-pose
  file loads, round-trips and refuses poses of another height; the
  snapshot's `walk=` gives the same frame twice.
- Docs: scale.md's movement section, assets.md's art format, testing.md,
  the README key table, properties.md's `speed` row.

## Consequences of holding more than one key

The owner, walking in Konsole: "it seems I can't press more than one key
at once for movement." A terminal that delivers only presses and repeats
repeats the last key pressed and no other, so the walk had one heading at
a time and a diagonal by chord was impossible. Three things follow, and
none of them changes what a walk is once it has a heading.

**The keys down are a set, not a press.** `input::Held` holds the
direction keys down; `Camera::held_heading` is the normalised sum of what
each means on its own, each key counting as the unit heading of every
axis it names. Two perpendicular keys are the diagonal between them, two
opposite ones are nothing, and a diagonal key is exactly the two keys it
stands for. A key pressed with shift makes the walk a run. The event loop
sets the heading every tick rather than on the press, and
`World::stop_walk` ends the walk on the tick the last key comes up.
`World::walk_for` is `walk_toward` with the lease given, since the set
knows what its keys have left.

**Key releases where the terminal reports them.**
`Terminal::with_key_release` asks for the keyboard enhancement protocol
when `supports_keyboard_enhancement` says the terminal speaks it, pushing
`DISAMBIGUATE_ESCAPE_CODES`, `REPORT_EVENT_TYPES`,
`REPORT_ALTERNATE_KEYS` and `REPORT_ALL_KEYS_AS_ESCAPE_CODES` — the last
because a plain-text key is reported at all three event types only when
every key comes as an escape code, and the alternate keycode because a
shifted letter then arrives as its base key, and `R` must stay `R`.
`input::lookup` takes the capital of a shifted letter before the plain
key for the same reason. The flags are popped on leave and by the panic
hook, which restores the terminal before the message prints. The status
bar reads `keys:held` or `keys:repeat` so the mode is visible.

**A lease per key where they are not.** Each key keeps its own lease of
`GRACE` seconds that a press or repeat renews and the tick spends, and
the heading is the sum of the leases still live, so two keys alternated
walk a diagonal and one held key behaves exactly as it did. A held chord
still cannot be delivered, so `y u b n` walk the four diagonals with one
key each and their capitals run them. `n` was the inset view, which moves
to `x` in the binding table and in `assets/ui.toml`; the new help entries
go at the end of the table, since the bottom bar of a 120-column frame
shows the first 120 columns of the generated line and the golden frames
pin those.

The event loop treats a repeat as a press, which is what a repeat has
always been, and a release lifts a direction key whatever modifiers it
comes back with and whatever frame has focus, so a walk never outlives
its key.
