# L-system trees

A tree can be a rule set instead of a shape. `src/lsystem/` rewrites a
short string with a grammar, walks the result with a turtle, and produces
a `TreeModel`: branch capsules and leaf-cluster ellipsoids in metres
(ADR-004), scaled to the species' declared `size` so a rule set fits its
height and spread whatever depth it is grown to.

Most trees are one of a small number of growth habits, so the grammars
live in `assets/tree_styles.toml` as eight named **styles** with named
parameters, and a species picks one and overrides what it wants
different. A species that no habit describes may still write its own
axiom and rules.

Nothing in the renderer reads any of this yet. The canopy volumes of
ADR-002 (cone, ellipsoid, dome) remain what the ray walk tests, and stay
the right thing at far zooms, where a tree is a few cells; each style
names the volume it reads as through `stand_in`. The L-system gives the
true silhouette at near zooms once `TreeModel::volumes` is wired into the
walk. Until then the way to look at a tree is a preview:

```
make build
./target/release/roguemap --snap-tree "gnarled oak" oak.cells 120 60 7 1
python3 tools/cells2png.py oak.cells oak.png
```

or `./tree-snap.sh oak.png "gnarled oak" 120 60 7 1`, which does both;
`make tree-snap OUT=oak.png NAME="gnarled oak" ARGS="110 55 7 1"` wraps
that, and `make screenshots` regenerates the six previews in
docs/screenshots.
The arguments after the output file are `cols rows seed season`, and
`foliage=F` and `state=dead` may follow in any order.

## Styles

`assets/tree_styles.toml` holds one row per habit. Each carries a
description, the far-zoom `stand_in` volume, an axiom, rule and
`dead_rules` tables, and a `[style.params]` block of defaults.

| Style | Habit | Stand-in |
|---|---|---|
| `excurrent` | one straight leader, a whorl of near-horizontal branches per tier, tiers shortening upward | cone |
| `decurrent` | a trunk forking low into spreading limbs, broad rounded crown | ellipsoid |
| `columnar` | one slender leader with short side shoots, narrow upright crown | ellipsoid |
| `weeping` | limbs that arch up and out, then hang | dome |
| `umbrella` | a tall clean trunk ending in one whorl of flat spreading limbs | ellipsoid |
| `vase` | limbs fanning up and out from a short trunk, no leader | ellipsoid |
| `palm` | one unbranched stem with a crown of arching fronds | ellipsoid |
| `shrub` | many stems from the ground, no trunk, low rounded mass | dome |

`vase` would rather be an upturned cone and `palm` a sphere on a pole;
`Shape` has neither, so both take the nearest volume there is.

## Parameters

Every parameter has a default per style and may be overridden per
species in `[species.lsystem]`.

| Parameter | Range | Meaning |
|---|---|---|
| `branch_angle` | 0..180 degrees | The turn of every `+ - & ^ \ /`. |
| `forks` | 1.. | Branches in a whorl, or ways the trunk forks. |
| `taper` | 0.1..1 | Factor a segment's length and radius take on entering a branch (`[`) and on every `!`. |
| `droop` | -1..1 | Extra bend per segment drawn inside a branch, as a fraction of `branch_angle`: positive hangs the branch, negative lifts it. |
| `leaf_density` | 0..1 | Chance an `L` becomes a cluster; thinning opens a crown without touching the branches. |
| `asymmetry` | 0..1 | How one-sided the instance is: a seeded direction the trunk leans in, with limbs on that side longer and the far side shorter. A parkland tree keeps it low, a gnarled or wind-shaped one high. |
| `jitter` | 0..1 | Small seeded noise on every branch's angle and length and every cluster's position. A healthy tree is symmetric but never exactly, so a live tree always carries some; the default is 0.08. |
| `prune_height` | 0..1 | Fraction of the tree's height with no live branches, because a tree self-prunes as it grows. |
| `depth` | 0..12 | How many times the rules are applied. |
| `length` | > 0 metres | Length of a first-level `F`, before the model is scaled to `size`. |
| `leaf_radius` | >= 0 metres | Radius of one `L` cluster, in final metres: a clump of leaves is about a metre across whatever shape its tree is, so this does not scale with the tree. |

`asymmetry` and `jitter` are different things and both are wanted.
Asymmetry is one bias for the whole instance; jitter is independent noise
per branch. At `asymmetry = 0` a tree's mass balances left and right to
within the jitter; at `jitter = 0` a symmetric grammar gives mirror-image
branches.

Two more parameters live on the species row rather than the grammar,
because they are about the tree and not its habit: `trunk_radius`
(metres, the thickest branch; the taper the grammar gave the rest is
kept) and `dead_chance` (0..1, default 0.02).

## Symbols

The turtle starts at the origin pointing up, carrying a heading, a left
and an up axis. Every turn is `branch_angle` degrees.

| Symbol | Meaning |
|---|---|
| `F` | Move forward one length, drawing a branch segment. |
| `f` | Move forward without drawing. |
| `+` `-` | Yaw left, yaw right (turn about the up axis). |
| `&` `^` | Pitch down, pitch up (turn about the left axis). |
| `\` `/` | Roll left, roll right (turn about the heading). |
| <code>&#124;</code> | Turn 180 degrees. |
| `[` `]` | Push and pop the turtle. Entering a branch multiplies length and radius by `taper`; a branch that starts below `prune_height` is skipped whole. |
| `!` | Multiply length and radius by `taper` again, for a bole that narrows or a whip that thins. |
| `L` | A leaf cluster of `leaf_radius` metres at the turtle, subject to `leaf_density`. |

Any other ASCII letter is a rewriting placeholder that draws nothing:
`A`, `B`, `S` and so on carry the recursion. Whitespace is ignored, so a
long rule may be spaced out to stay readable. Brackets must balance
within the axiom and within every replacement, and any other character is
a load error.

## Rules

A rule table maps one symbol to its replacements:

```toml
[style.rules]
A = [
  ['F([(+)*1!AL](/)*spread)*forks', 4],
  ['FF([(+)*1!AL](/)*spread)*forks', 2],
]
B = 'FF'                 # one replacement, weight 1
C = ['FA', 'FB']         # several, equal weight
```

Where a symbol has several alternatives the choice is a hash of the
symbol's position in the string, the generation and the instance seed, so
one seed always gives one tree and two neighbours of a species differ.

### Templates

Rule strings are templates. `(BODY)*N` repeats `BODY` N times, nested to
any depth, where N is a whole number or one of four counts derived from
the parameters:

| Count | Value |
|---|---|
| `forks` | the `forks` parameter |
| `spread` | rolls between the branches of a whorl: about a full turn divided by `forks` |
| `pitch` | pitch steps to swing a branch near horizontal |
| `lift` | pitch steps to angle a branch upward |

So `([(&)*pitchB](/)*spread)*forks` is "a whorl of `forks` branches, each
pitched near horizontal, spaced evenly around the trunk", and the same
line serves a five-branch conifer whorl and a two-way oak fork. A number
runs to its last digit and a count name is matched whole, so `(F)*2A` and
`(&)*pitchB` both parse.

## Authoring a species

```toml
[[species]]
name = "gnarled oak"
description = "An old lowland oak with a short thick bole ..."
category = "tree"
form = "broadleaf"          # still the glyph pool and the tiny sprite
size_class = "large"
size = [13.0, 12.0, 17.0]   # metres; the model is scaled to fit it
shape = "lsystem"
style = "decurrent"
trunk_radius = 0.45
canopy = [[78, 138, 58], [44, 106, 48], [176, 104, 42], [104, 84, 66]]
canopy_glyph = [[136, 198, 96], [86, 156, 78], [226, 152, 62], [142, 120, 98]]
dead_chance = 0.05
tags = ["flammable", "wooden"]

[species.lsystem]
asymmetry = 0.3
jitter = 0.16
leaf_radius = 0.38
prune_height = 0.1
depth = 7
```

`shape = "lsystem"` and a habit imply each other: a row with one and not
the other is a load error naming the row. A species with no `style` must
write its own grammar in `[species.lsystem]` with at least `axiom`,
`rules`, `depth`, `angle`, `length` and `leaf_radius`; a species with a
style may also replace the habit's `axiom`, `rules` or `dead_rules`
outright. Everything is checked when the assets load, and the message
names the file, the row and what is wrong.

## Bare and dead trees

`TreeModel::build(species, seed, foliage, state)` grows one instance.
`foliage` in 0..1 scales every cluster; 0 leaves the branch skeleton, so
a deciduous species goes bare across autumn and leafs out again in
spring. The caller derives it: `Species::foliage(annual, season)` keeps
an evergreen at 1 and follows `biome::vigour` for a species that `sheds`.

`state` is `Alive` or `Dead`. Standing deadwood has no leaves whatever
the season, takes its bark colour from `TreeModel::bark`, which greys the
live colour, and grows from the style's `dead_rules`: fewer and shorter
branches, a broken crown. Without dead rules it grows the live grammar
one level shallower. Which instances are dead is `Species::dead(seed)`, a
hash against `dead_chance`, so the same tile carries the same dead tree
every time the chunk is generated.

A bare or dead tree keeps the scale its leafy self had, so a winter oak
is a bare oak and not a swollen one, and a dead one with a broken crown
stands shorter than its neighbour.

## The model

```rust
pub struct TreeModel {
    pub segments: Vec<Segment>,   // { a, b, radius } capsules, metres
    pub leaves: Vec<Leaf>,        // { centre, radius } ellipsoids, metres
    pub bounds: Bounds,
    pub state: State,
}
```

The foot of the trunk is the origin and z = 0 is the ground. Height is
measured from the foot, not across the whole model, because a weeping
species hangs below it; those whips are under the ground and the ground
hides them.

`TreeModel::volumes` is the adapter to the ray walk of ADR-002: one
`Volume::Cylinder` per branch, one `Volume::Ellipsoid` per cluster.
`volume::Shape` has no primitive for a branch at an arbitrary angle yet,
so the walk needs a cylinder test before it can consume the branches;
the clusters already convert to its own placed volumes through
`TreeModel::canopies`.

## The preview

`lsystem::preview(model, cols, rows, style)` projects the model
orthographically from the side: x across, z up, y depth, drawn far to
near so a nearer branch covers a farther one. Cells are twice as tall as
wide in the canonical font, so a metre is twice as many columns as rows
and the tree keeps its proportions.

A branch is drawn as spans across its axis, with the glyph following its
thickness: `|` or the matching diagonal for a twig, a half block for a
branch about a cell wide, `█` for a trunk. A leaf cluster is a filled
ellipse in the canopy colour, textured with the tileset's canopy glyph
pool and edged with its quadrant and half-block glyphs, so a blob has a
rounded outline rather than a staircase.

`PreviewStyle::from_tileset` takes those glyphs from a tileset's `[art]`
roles, so the preview and the renderer draw a canopy from the same
vocabulary.
