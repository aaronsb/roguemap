//! Parametric L-system trees: a grammar in `species.toml` rewritten into a
//! symbol string, then walked by a turtle into a `TreeModel` of branch
//! segments and leaf clusters, in metres (ADR-004).
//!
//! The module is self-contained. It reads a species row, produces geometry,
//! and offers two ways out: `preview` draws the model side-on into a
//! `Canvas` so a tree can be looked at in a snapshot, and `TreeModel::volumes`
//! hands the same geometry to the ray walk of ADR-002 as cylinders and
//! ellipsoids. Nothing here reaches into the renderer.
//!
//! The grammar, its symbols and how to author a species are in
//! docs/lsystem.md.

pub mod preview;
pub mod style;

pub use preview::{preview, snap, PreviewStyle};
pub use style::{Overrides, Params, Style};

use std::collections::BTreeMap;

use crate::biome::Species;
use crate::canvas::Rgb;
use crate::noise::{hash, hash01};

/// Deepest rewriting a grammar may ask for. Every rewriting multiplies the
/// symbol count, so this is a guard rail, not a taste.
pub const MAX_DEPTH: u8 = 12;

/// Symbols an expansion may reach before rewriting stops. A tree of any
/// readable shape is a few thousand symbols; this only stops a runaway
/// grammar from eating the frame.
pub const MAX_SYMBOLS: usize = 400_000;

/// Radius of the first segment as a fraction of its length. Radii are
/// relative until `TreeModel::set_trunk_radius` puts the thickest segment
/// at the species' `trunk_radius` in metres, so this value cancels: what
/// shapes a tree is how `taper` and `!` thin the branches from it.
pub const TRUNK_RATIO: f32 = 0.12;

/// The turtle symbols, in the order docs/lsystem.md lists them.
pub const SYMBOLS: [char; 13] = ['F', 'f', '+', '-', '&', '^', '\\', '/', '|', '[', ']', '!', 'L'];

// Vectors are plain `[f32; 3]` in metres: x east, y north, z up.

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let n = dot(v, v).sqrt();
    if n > 1e-6 {
        scaled(v, 1.0 / n)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Rodrigues' rotation of `v` about the unit `axis` by the angle whose sine
/// and cosine are given.
fn rotate(v: [f32; 3], axis: [f32; 3], sin: f32, cos: f32) -> [f32; 3] {
    let a = scaled(v, cos);
    let b = scaled(cross(axis, v), sin);
    let c = scaled(axis, dot(axis, v) * (1.0 - cos));
    add(add(a, b), c)
}

/// One branch: a capsule from `a` to `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

/// One leaf cluster: an ellipsoid of foliage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaf {
    pub centre: [f32; 3],
    pub radius: [f32; 3],
}

/// An axis-aligned box in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }
}

impl Bounds {
    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.min[i] > self.max[i])
    }

    /// Width, depth and height; zero on every axis when empty.
    pub fn size(&self) -> [f32; 3] {
        if self.is_empty() {
            return [0.0; 3];
        }
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }

    pub fn centre(&self) -> [f32; 3] {
        if self.is_empty() {
            return [0.0; 3];
        }
        [(self.min[0] + self.max[0]) * 0.5, (self.min[1] + self.max[1]) * 0.5, (self.min[2] + self.max[2]) * 0.5]
    }

    /// Grow to hold the ellipsoid at `c` with per-axis radii `r`.
    fn add(&mut self, c: [f32; 3], r: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(c[i] - r[i]);
            self.max[i] = self.max[i].max(c[i] + r[i]);
        }
    }
}

/// Whether a tree is growing or standing dead.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    #[default]
    Alive,
    /// Standing deadwood: no leaves whatever the season, grey bark, and a
    /// broken crown (`dead_rules`, or the grammar one level shallower).
    Dead,
}

/// How a particular tree grew: how much of its foliage is out, and whether
/// it is alive. The caller derives `foliage` from the species (an evergreen
/// stays at 1, a deciduous follows `biome::vigour`); a dead tree has none.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Growth {
    /// Leaf cluster radius as a fraction of `leaf_radius`, 0..1. Zero drops
    /// the clusters, leaving the branch skeleton.
    pub foliage: f32,
    pub state: State,
}

impl Default for Growth {
    fn default() -> Growth {
        Growth { foliage: 1.0, state: State::Alive }
    }
}

impl Growth {
    /// A live tree in full leaf.
    pub const FULL: Growth = Growth { foliage: 1.0, state: State::Alive };

    /// A live tree stripped of its leaves, as a deciduous is in winter.
    pub const BARE: Growth = Growth { foliage: 0.0, state: State::Alive };

    /// Standing deadwood.
    pub const DEAD: Growth = Growth { foliage: 0.0, state: State::Dead };
}

/// A leaf cluster smaller than this fraction of its full radius is not
/// drawn at all: the last few leaves of autumn are a bare tree.
pub const BARE_BELOW: f32 = 0.05;

/// A tree as geometry: branch capsules and foliage ellipsoids, in metres,
/// with the ground at `bounds.min[2]` and the trunk near the x-y origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeModel {
    pub segments: Vec<Segment>,
    pub leaves: Vec<Leaf>,
    pub bounds: Bounds,
    /// Alive or dead; a renderer reads it for the bark colour.
    pub state: State,
}

/// The shape the ray walk of ADR-002 consumes: the primitives its per-sample
/// quadratic solver already knows. `TreeModel::volumes` is the whole adapter,
/// so wiring the walk to L-system trees is registering this list in the frame
/// grid the way a cone or an ellipsoid canopy is registered today. Nothing in
/// the renderer calls it yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Volume {
    /// A branch: the capsule's axis and its radius.
    Cylinder { a: [f32; 3], b: [f32; 3], radius: f32 },
    /// A leaf cluster: centre and per-axis radii.
    Ellipsoid { centre: [f32; 3], radii: [f32; 3] },
}

impl TreeModel {
    /// Recompute `bounds` from the geometry, branch radii included.
    pub fn rebound(&mut self) {
        let mut b = Bounds::default();
        for s in &self.segments {
            let r = [s.radius; 3];
            b.add(s.a, r);
            b.add(s.b, r);
        }
        for l in &self.leaves {
            b.add(l.centre, l.radius);
        }
        self.bounds = b;
    }

    /// The per-axis scale that maps this model onto a `[w, d, h]` metre box,
    /// and the point it is measured from: the middle of the footprint at the
    /// foot of the trunk, which is where the turtle started and so is z = 0.
    ///
    /// Height is measured from the foot, not across the whole model, because
    /// a weeping species hangs below it: those whips are under the ground
    /// and the ground hides them.
    fn scale_to(&self, size: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        if self.bounds.is_empty() {
            return ([1.0; 3], [0.0; 3]);
        }
        let raw = self.bounds.size();
        let sz = if self.bounds.max[2] > 1e-4 { size[2] / self.bounds.max[2] } else { 1.0 };
        let sx = if raw[0] > 1e-4 { size[0] / raw[0] } else { sz };
        let sy = if raw[1] > 1e-4 { size[1] / raw[1] } else { sz };
        let c = self.bounds.centre();
        ([sx, sy, sz], [c[0], c[1], 0.0])
    }

    /// Move `origin` to the origin and scale each axis. Branch radii take
    /// the mean of the two horizontal factors, since a branch is round; a
    /// leaf cluster's radius is already metres and does not scale, so a
    /// clump of leaves stays a clump whatever shape its tree is.
    pub fn transform(&mut self, scale: [f32; 3], origin: [f32; 3]) {
        let put = |p: [f32; 3]| [(p[0] - origin[0]) * scale[0], (p[1] - origin[1]) * scale[1], (p[2] - origin[2]) * scale[2]];
        let rs = (scale[0] + scale[1]) * 0.5;
        for s in &mut self.segments {
            s.a = put(s.a);
            s.b = put(s.b);
            s.radius *= rs;
        }
        for l in &mut self.leaves {
            l.centre = put(l.centre);
        }
        self.rebound();
    }

    /// Scale and translate the model so it fills a `[w, d, h]` metre box:
    /// centred on the trunk in x and y, standing on z = 0. A grammar
    /// therefore fits its species' declared size whatever its depth.
    /// Returns the scale it applied, for growing a bare tree to the size its
    /// leafy self would have had.
    ///
    /// Leaf clusters keep their radius while the branches around them move,
    /// so this is a fixed point rather than one division; a few passes reach
    /// it.
    pub fn fit(&mut self, size: [f32; 3]) -> [f32; 3] {
        let mut total = [1.0; 3];
        for _ in 0..8 {
            self.rebound();
            if self.bounds.is_empty() {
                return total;
            }
            let (scale, origin) = self.scale_to(size);
            self.transform(scale, origin);
            for (t, s) in total.iter_mut().zip(scale) {
                *t *= s;
            }
            if scale.iter().all(|s| (s - 1.0).abs() < 1e-3) {
                break;
            }
        }
        total
    }

    /// Scale every branch radius so the thickest is `radius` metres, keeping
    /// the taper the grammar gave them. This is what makes a bole a bole: an
    /// old oak and a birch sapling can share a grammar and differ only in
    /// the species' `trunk_radius`.
    pub fn set_trunk_radius(&mut self, radius: f32) {
        let thickest = self.segments.iter().fold(0.0f32, |m, s| m.max(s.radius));
        if thickest <= 1e-6 || radius <= 0.0 {
            return;
        }
        let f = radius / thickest;
        for s in &mut self.segments {
            s.radius *= f;
        }
        self.rebound();
    }

    /// The model as the volumes the ray walk tests: one cylinder per branch,
    /// one ellipsoid per leaf cluster, branches first.
    pub fn volumes(&self) -> Vec<Volume> {
        let mut out = Vec::with_capacity(self.segments.len() + self.leaves.len());
        out.extend(self.segments.iter().map(|s| Volume::Cylinder { a: s.a, b: s.b, radius: s.radius }));
        out.extend(self.leaves.iter().map(|l| Volume::Ellipsoid { centre: l.centre, radii: l.radius }));
        out
    }

    /// The leaf clusters as the walk's own placed volumes
    /// (`crate::volume::Volume`), for a tree standing at tile `(mx, my)`
    /// with its trunk at `(cx, cy)` tiles on ground height `ground` metres.
    ///
    /// Only the clusters convert: `volume::Shape` has no primitive for a
    /// branch at an arbitrary angle, so the branches stay in this module's
    /// own `Volume::Cylinder` until the walk grows one.
    pub fn canopies(&self, species: u8, mx: i32, my: i32, cx: f32, cy: f32, ground: f32) -> Vec<crate::volume::Volume> {
        self.leaves
            .iter()
            .map(|l| crate::volume::Volume {
                shape: crate::volume::Shape::Ellipsoid,
                cx: cx + l.centre[0] / crate::map::TILE_METRES,
                cy: cy + l.centre[1] / crate::map::TILE_METRES,
                ground,
                h0: ground + l.centre[2] - l.radius[2],
                height: 2.0 * l.radius[2],
                radius: 0.5 * (l.radius[0] + l.radius[1]) / crate::map::TILE_METRES,
                trunk_radius: 0.0,
                shear: (0.0, 0.0),
                species,
                mx,
                my,
            })
            .collect()
    }

    /// The bark colour a renderer should use: the live colour, or a greyed
    /// and darkened version of it for standing deadwood.
    pub fn bark(&self, live: Rgb) -> Rgb {
        match self.state {
            State::Alive => live,
            State::Dead => live.lerp(Rgb(150, 146, 138), 0.55).scale(0.85),
        }
    }

    /// One instance of a species: its grammar grown at `seed`, scaled to the
    /// species' `size` in metres. None unless the row is
    /// `shape = "lsystem"`.
    pub fn build(species: &Species, seed: u64, foliage: f32, state: State) -> Option<TreeModel> {
        species.tree_model(seed, Growth { foliage, state })
    }
}

/// One right-hand side of a rule, with its relative weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Alternative {
    pub replacement: String,
    pub weight: u8,
}

/// A rewriting grammar with the turtle parameters that interpret it.
/// Built from a species row by `Grammar::from_row`; see docs/lsystem.md.
#[derive(Clone, Debug, PartialEq)]
pub struct Grammar {
    /// The string rewriting starts from.
    pub axiom: String,
    /// Symbol to its alternatives, in file order.
    pub rules: BTreeMap<char, Vec<Alternative>>,
    /// Rules a dead tree is grown with instead: fewer and shorter branches,
    /// a broken crown. Empty means `rules` one level shallower.
    pub dead_rules: BTreeMap<char, Vec<Alternative>>,
    /// How many times the rules are applied.
    pub depth: u8,
    /// Turn of every `+ - & ^ \ /`, in degrees.
    pub angle: f32,
    /// Length of a first-level `F`, in metres.
    pub length: f32,
    /// Factor a segment's length and radius are multiplied by on entering a
    /// branch (`[`) and on every `!`. In `0.1..=1`.
    pub taper: f32,
    /// Radius of an `L` cluster, in metres.
    pub leaf_radius: f32,
    /// Extra pitch applied to every segment drawn inside a branch, as a
    /// fraction of `angle`: positive bends the branch down (a willow's
    /// whips), negative lifts it (a vase's limbs). In `-1..=1`.
    pub droop: f32,
    /// Chance in `0..=1` that an `L` becomes a cluster. Thinning the
    /// foliage opens a crown without touching the branches.
    pub leaf_density: f32,
    /// How one-sided the instance is, `0..=1`: a seeded direction the trunk
    /// leans in, with the limbs on that side longer and the far side
    /// shorter. A parkland tree keeps it low, a wind-shaped one high.
    pub asymmetry: f32,
    /// Small seeded noise on every branch's angle and length and on every
    /// cluster's position, `0..=1`. A healthy tree is symmetric but never
    /// exactly, so a live tree always has some.
    pub jitter: f32,
    /// Fraction of the tree's height carrying no live branches, `0..1`: a
    /// tree self-prunes as it grows and the lower limbs die away.
    pub prune_height: f32,
}

/// The turtle knobs with no effect: a bare grammar with no habit.
impl Grammar {
    /// Field defaults for a grammar written out by hand, so a species that
    /// gives only axiom and rules behaves like a plain L-system.
    pub const PLAIN: Habit = Habit { droop: 0.0, leaf_density: 1.0, asymmetry: 0.0, jitter: 0.08, prune_height: 0.0 };
}

/// The habit knobs on their own, for defaulting and overriding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Habit {
    pub droop: f32,
    pub leaf_density: f32,
    pub asymmetry: f32,
    pub jitter: f32,
    pub prune_height: f32,
}

impl Grammar {
    /// Check the grammar the way the asset loader does, returning the
    /// message it would report. Also expands once with seed 0, so a grammar
    /// that can never draw anything is caught at load rather than at sight.
    pub fn validate(&self) -> Result<(), String> {
        if self.axiom.trim().is_empty() {
            return Err("axiom is empty".to_string());
        }
        if self.depth > MAX_DEPTH {
            return Err(format!("depth {} is above the limit of {MAX_DEPTH}", self.depth));
        }
        if !(0.0..=180.0).contains(&self.angle) {
            return Err(format!("angle {} is outside 0..180 degrees", self.angle));
        }
        if self.length <= 0.0 || !self.length.is_finite() {
            return Err(format!("length {} must be positive", self.length));
        }
        if !(0.1..=1.0).contains(&self.taper) {
            return Err(format!("taper {} is outside 0.1..1", self.taper));
        }
        if self.leaf_radius < 0.0 {
            return Err(format!("leaf_radius {} is negative", self.leaf_radius));
        }
        if !(-1.0..=1.0).contains(&self.droop) {
            return Err(format!("droop {} is outside -1..1", self.droop));
        }
        for (field, v) in [("leaf_density", self.leaf_density), ("asymmetry", self.asymmetry), ("jitter", self.jitter), ("prune_height", self.prune_height)] {
            if !(0.0..=1.0).contains(&v) {
                return Err(format!("{field} {v} is outside 0..1"));
            }
        }
        if self.prune_height >= 1.0 {
            return Err("prune_height 1 prunes the whole tree".to_string());
        }
        check_word("axiom", &self.axiom)?;
        for (what, rules) in [("rule", &self.rules), ("dead rule", &self.dead_rules)] {
            for (sym, alts) in rules {
                if !sym.is_ascii_alphabetic() {
                    return Err(format!("{what} {sym:?} must be a letter; the turtle symbols {} cannot be rewritten", SYMBOLS.iter().collect::<String>()));
                }
                if alts.is_empty() {
                    return Err(format!("{what} {sym:?} has no replacement"));
                }
                for a in alts {
                    if a.weight == 0 {
                        return Err(format!("{what} {sym:?} replacement {:?} has weight 0", a.replacement));
                    }
                    check_word(&format!("{what} {sym:?}"), &a.replacement)?;
                }
            }
        }
        if !self.expand(0, Growth::FULL).chars().any(|c| c == 'F' || c == 'L') {
            return Err("the expansion draws nothing: no F or L survives the rules".to_string());
        }
        if !self.dead_rules.is_empty() && !self.expand(0, Growth::DEAD).chars().any(|c| c == 'F') {
            return Err("the dead rules draw nothing: no F survives them".to_string());
        }
        Ok(())
    }

    /// The rules and depth a growth uses: a dead tree takes `dead_rules`, or
    /// the live rules with the last level of growth removed.
    fn generation(&self, growth: Growth) -> (&BTreeMap<char, Vec<Alternative>>, u8) {
        match growth.state {
            State::Alive => (&self.rules, self.depth),
            State::Dead if !self.dead_rules.is_empty() => (&self.dead_rules, self.depth),
            State::Dead => (&self.rules, self.depth.saturating_sub(1)),
        }
    }

    /// Rewrite the axiom `depth` times. Where a symbol has several
    /// alternatives the choice is a hash of the symbol's position, the
    /// generation and `seed`, so one instance seed always gives one tree and
    /// two neighbours of a species differ.
    pub fn expand(&self, seed: u64, growth: Growth) -> String {
        let (rules, depth) = self.generation(growth);
        let mut s = self.axiom.clone();
        for g in 0..depth as u64 {
            if s.len() >= MAX_SYMBOLS {
                break;
            }
            let mut out = String::with_capacity(s.len() * 2);
            for (i, ch) in s.chars().enumerate() {
                match rules.get(&ch) {
                    Some(alts) => out.push_str(&pick(alts, hash(i as i64, g as i64, seed))),
                    None => out.push(ch),
                }
            }
            s = out;
        }
        s
    }

    /// Walk the expansion with the turtle, in the grammar's own metres.
    pub fn model(&self, seed: u64, growth: Growth) -> TreeModel {
        let word = self.expand(seed, growth);
        // Pruning needs the height the tree reaches, so walk it once with
        // nothing pruned and once for real. The first walk is only measured.
        let m = self.walk(&word, seed, growth, f32::NEG_INFINITY);
        if self.prune_height <= 0.0 || m.bounds.is_empty() {
            return m;
        }
        let pruned = self.walk(&word, seed, growth, self.prune_height.min(0.95) * m.bounds.max[2]);
        // A habit whose every branch starts low would prune away its whole
        // crown; keep the unpruned tree rather than a bare pole.
        if pruned.segments.is_empty() || (pruned.leaves.is_empty() && !m.leaves.is_empty()) {
            m
        } else {
            pruned
        }
    }

    /// One turtle walk of an expansion. A branch that starts below
    /// `prune_z` is skipped whole, which is how a tree ends up with a clean
    /// trunk under its crown.
    fn walk(&self, word: &str, seed: u64, growth: Growth, prune_z: f32) -> TreeModel {
        let leaf = if growth.state == State::Dead || growth.foliage < BARE_BELOW { 0.0 } else { self.leaf_radius * growth.foliage.min(1.0) };
        let density = if growth.state == State::Dead { 0.0 } else { self.leaf_density.clamp(0.0, 1.0) };
        let mut m = TreeModel { state: growth.state, ..TreeModel::default() };
        let (sin, cos) = self.angle.to_radians().sin_cos();
        // The instance's lean: a seeded compass direction and how hard the
        // tree leans that way. Jitter is per branch, drawn from the same
        // seed, so one seed is always one tree.
        let lean = self.asymmetry.clamp(0.0, 1.0);
        let jitter = self.jitter.clamp(0.0, 1.0);
        let phi = hash01(0, 7, seed) * std::f32::consts::TAU;
        let toward = [phi.cos(), phi.sin(), 0.0];
        let mut t = Turtle {
            pos: [0.0, 0.0, 0.0],
            frame: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
            len: self.length,
            rad: self.length * TRUNK_RATIO,
        };
        if lean > 0.0 {
            // Lean the whole tree up to twelve degrees toward `phi`.
            let (s, c) = (lean * 12.0f32).to_radians().sin_cos();
            let axis = normalise(cross([0.0, 0.0, 1.0], toward));
            for v in t.frame.iter_mut() {
                *v = normalise(rotate(*v, axis, s, c));
            }
        }
        let mut stack: Vec<Turtle> = Vec::new();
        // Event counters, so every draw from the hash has its own stream.
        let (mut branches, mut clusters) = (0i64, 0i64);
        let mut skip = 0usize;
        let (droop_sin, droop_cos) = (self.droop.clamp(-1.0, 1.0) * self.angle).to_radians().sin_cos();
        for ch in word.chars() {
            if skip > 0 {
                match ch {
                    '[' => skip += 1,
                    ']' => skip -= 1,
                    _ => {}
                }
                continue;
            }
            match ch {
                'F' | 'f' => {
                    let b = add(t.pos, scaled(t.frame[0], t.len));
                    if ch == 'F' {
                        m.segments.push(Segment { a: t.pos, b, radius: t.rad.max(1e-4) });
                    }
                    t.pos = b;
                    // Inside a branch every segment bends a little further
                    // down (or up), which is what makes a whip hang.
                    if !stack.is_empty() && self.droop != 0.0 {
                        t.bend(droop_sin, droop_cos);
                    }
                }
                '+' => t.turn(2, sin, cos),
                '-' => t.turn(2, -sin, cos),
                '&' => t.turn(1, sin, cos),
                '^' => t.turn(1, -sin, cos),
                '\\' => t.turn(0, sin, cos),
                '/' => t.turn(0, -sin, cos),
                '|' => t.turn(2, 0.0, -1.0),
                '[' => {
                    if t.pos[2] < prune_z {
                        skip = 1;
                        continue;
                    }
                    stack.push(t);
                    t.len *= self.taper;
                    t.rad *= self.taper;
                    branches += 1;
                    if jitter > 0.0 {
                        // A nudge of the heading and of the length, so no
                        // two branches of a symmetric rule are identical.
                        let a = (hash01(branches, 1, seed) - 0.5) * 2.0 * jitter * self.angle;
                        let roll = hash01(branches, 2, seed) * std::f32::consts::TAU;
                        let (js, jc) = a.to_radians().sin_cos();
                        let axis = normalise(add(scaled(t.frame[1], roll.cos()), scaled(t.frame[2], roll.sin())));
                        for v in t.frame.iter_mut() {
                            *v = normalise(rotate(*v, axis, js, jc));
                        }
                        t.len *= 1.0 + (hash01(branches, 3, seed) - 0.5) * 0.8 * jitter;
                    }
                    if lean > 0.0 {
                        // Limbs reaching the way the tree leans grow longer,
                        // the ones facing away shorter.
                        t.len *= 1.0 + 0.5 * lean * dot(t.frame[0], toward);
                    }
                }
                ']' => {
                    if let Some(p) = stack.pop() {
                        t = p;
                    }
                }
                '!' => {
                    t.len *= self.taper;
                    t.rad *= self.taper;
                }
                'L' => {
                    clusters += 1;
                    if leaf > 0.0 && (density >= 1.0 || hash01(clusters, 4, seed) < density) {
                        let mut c = add(t.pos, scaled(t.frame[0], leaf * 0.4));
                        if jitter > 0.0 {
                            for (i, v) in c.iter_mut().enumerate() {
                                *v += (hash01(clusters, 5 + i as i64, seed) - 0.5) * jitter * leaf;
                            }
                        }
                        m.leaves.push(Leaf { centre: c, radius: [leaf; 3] });
                    }
                }
                _ => {}
            }
        }
        m.rebound();
        m
    }

    /// The model scaled to a species' `[w, d, h]` size in metres. A bare or
    /// dead tree keeps the box its species declares, so a winter oak stands
    /// as tall as a summer one.
    pub fn grow(&self, seed: u64, size: [f32; 3], growth: Growth) -> TreeModel {
        let mut m = self.model(seed, growth);
        if growth == Growth::FULL {
            m.fit(size);
            return m;
        }
        // Anything less than a tree in full leaf keeps the scale that tree
        // had, so a bare winter oak is a bare oak and not a swollen one, and
        // a dead one with a broken crown stands shorter than its neighbour.
        // Its own footprint and floor still set the origin, so it stands on
        // the ground centred on its trunk.
        let scale = self.model(seed, Growth::FULL).fit(size);
        let c = m.bounds.centre();
        m.transform(scale, [c[0], c[1], 0.0]);
        m
    }
}

/// Reject anything the turtle would silently ignore: only the turtle
/// symbols and letters (which stand for rewriting placeholders) are legal,
/// and brackets must balance so a branch cannot leak into its parent.
fn check_word(what: &str, word: &str) -> Result<(), String> {
    let mut depth = 0i32;
    for ch in word.chars() {
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth < 0 {
                return Err(format!("{what}: {word:?} closes a branch that was never opened"));
            }
        } else if ch.is_whitespace() {
            // Space is free: a template may use it to stay readable.
            continue;
        } else if !SYMBOLS.contains(&ch) && !ch.is_ascii_alphabetic() {
            return Err(format!("{what}: {word:?} has the unknown symbol {ch:?}; the turtle knows {} and letters stand for rules", SYMBOLS.iter().collect::<String>()));
        }
    }
    if depth != 0 {
        return Err(format!("{what}: {word:?} leaves {depth} branch(es) open"));
    }
    Ok(())
}

/// Choose an alternative by weight from a hash roll.
fn pick(alts: &[Alternative], roll: u64) -> String {
    if alts.len() == 1 {
        return alts[0].replacement.clone();
    }
    let total: u32 = alts.iter().map(|a| a.weight as u32).sum();
    let mut r = (roll % total.max(1) as u64) as u32;
    for a in alts {
        if r < a.weight as u32 {
            return a.replacement.clone();
        }
        r -= a.weight as u32;
    }
    alts[0].replacement.clone()
}

/// Turtle state: where it is, the orthonormal frame `[heading, left, up]`
/// it points with, and the length and radius the next `F` draws.
#[derive(Clone, Copy)]
struct Turtle {
    pos: [f32; 3],
    frame: [[f32; 3]; 3],
    len: f32,
    rad: f32,
}

impl Turtle {
    /// Rotate the frame about one of its own axes: 0 heading (roll),
    /// 1 left (pitch), 2 up (yaw).
    fn turn(&mut self, axis: usize, sin: f32, cos: f32) {
        let a = self.frame[axis];
        for i in 0..3 {
            if i != axis {
                self.frame[i] = normalise(rotate(self.frame[i], a, sin, cos));
            }
        }
    }

    /// Bend the heading toward the ground by the given angle, whatever way
    /// it points; a negative angle lifts it. A heading already straight up
    /// or down has no such direction and does not bend.
    fn bend(&mut self, sin: f32, cos: f32) {
        let axis = cross(self.frame[0], [0.0, 0.0, 1.0]);
        if dot(axis, axis) < 1e-6 {
            return;
        }
        let axis = normalise(axis);
        for v in self.frame.iter_mut() {
            *v = normalise(rotate(*v, axis, -sin, cos));
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn alt(replacement: &str, weight: u8) -> Alternative {
        Alternative { replacement: replacement.to_string(), weight }
    }

    /// A grammar with one deterministic rule, for the expansion test.
    fn simple() -> Grammar {
        Grammar {
            axiom: "F".to_string(),
            rules: BTreeMap::from([('F', vec![alt("F[+F]F", 1)])]),
            dead_rules: BTreeMap::new(),
            depth: 2,
            angle: 25.0,
            length: 1.0,
            taper: 0.8,
            leaf_radius: 0.5,
            droop: 0.0,
            leaf_density: 1.0,
            asymmetry: 0.0,
            jitter: 0.0,
            prune_height: 0.0,
        }
    }

    /// A stochastic grammar shaped like the gnarled oak in species.toml,
    /// with dead rules that leave fewer branches.
    fn oak() -> Grammar {
        Grammar {
            axiom: "F!FA".to_string(),
            rules: BTreeMap::from([('A', vec![alt("F/[+!AL][-!AL]", 4), alt("F//[+!AL]\\[-!AL]", 3), alt("F[+!AL][-!AL][/&!AL]", 2)])]),
            dead_rules: BTreeMap::from([('A', vec![alt("F[+^!A]/FA", 3), alt("FF[-&!A]", 2), alt("F&F", 2)])]),
            depth: 5,
            angle: 30.0,
            length: 1.0,
            taper: 0.8,
            leaf_radius: 0.4,
            droop: 0.04,
            leaf_density: 1.0,
            asymmetry: 0.3,
            jitter: 0.1,
            prune_height: 0.1,
        }
    }


    /// A grammar that forks perfectly evenly, for the symmetry tests.
    fn mirrored() -> Grammar {
        Grammar {
            axiom: "FFA".to_string(),
            rules: BTreeMap::from([('A', vec![alt("F[+!AL][-!AL]", 1)])]),
            dead_rules: BTreeMap::new(),
            depth: 4,
            angle: 35.0,
            length: 1.0,
            taper: 0.8,
            leaf_radius: 0.3,
            droop: 0.0,
            leaf_density: 1.0,
            asymmetry: 0.0,
            jitter: 0.0,
            prune_height: 0.0,
        }
    }

    /// How far the foliage sits to one side: the leaf clusters' mean
    /// offset from the trunk against their mean reach, so a tree whose two
    /// sides balance scores zero however wide it is.
    fn balance(m: &TreeModel) -> f32 {
        let mut off = [0.0f32; 2];
        let mut reach = 0.0f32;
        for l in &m.leaves {
            off[0] += l.centre[0];
            off[1] += l.centre[1];
            reach += l.centre[0].abs() + l.centre[1].abs();
        }
        (off[0].abs() + off[1].abs()) / reach.max(1e-3)
    }

    #[test]
    fn a_symmetric_grammar_is_mirror_identical_without_jitter() {
        let size = [8.0, 8.0, 12.0];
        let m = mirrored().grow(5, size, Growth::FULL);
        // Every branch has its mirror image across the trunk.
        for s in &m.segments {
            let mirror = [-s.b[0], s.b[1], s.b[2]];
            let found = m.segments.iter().any(|o| (0..3).all(|i| (o.b[i] - mirror[i]).abs() < 1e-3) && (o.radius - s.radius).abs() < 1e-4);
            assert!(found, "{s:?} has no mirror");
        }
        assert!(balance(&m) < 1e-6, "the two sides carry the same mass");
    }

    #[test]
    fn jitter_breaks_the_mirror_and_repeats_for_a_seed() {
        let size = [8.0, 8.0, 12.0];
        let mut g = mirrored();
        g.jitter = 0.2;
        let a = g.grow(5, size, Growth::FULL);
        assert_eq!(a, g.grow(5, size, Growth::FULL), "the same seed is the same tree");
        assert_ne!(a, mirrored().grow(5, size, Growth::FULL), "jitter changed it");
        let exact = mirrored().grow(5, size, Growth::FULL);
        assert_eq!(a.segments.len(), exact.segments.len(), "jitter moves branches, it does not add any");
        // Still balanced, though: jitter is noise, not a bias.
        assert!(balance(&a) < 0.3, "balance {}", balance(&a));
    }

    #[test]
    fn asymmetry_leans_the_tree_and_zero_asymmetry_balances() {
        let size = [10.0, 10.0, 14.0];
        let mut g = mirrored();
        g.jitter = 0.08;
        let even: f32 = (0..6).map(|s| balance(&g.grow(s, size, Growth::FULL))).sum::<f32>() / 6.0;
        g.asymmetry = 0.7;
        let leaning: f32 = (0..6).map(|s| balance(&g.grow(s, size, Growth::FULL))).sum::<f32>() / 6.0;
        assert!(even < 0.25, "at asymmetry 0 the sides balance within the jitter: {even}");
        assert!(leaning > even, "asymmetry favours one side: {leaning} against {even}");
    }

    #[test]
    fn a_known_grammar_expands_to_the_expected_string() {
        let mut g = simple();
        g.depth = 0;
        assert_eq!(g.expand(1, Growth::FULL), "F");
        g.depth = 1;
        assert_eq!(g.expand(1, Growth::FULL), "F[+F]F");
        g.depth = 2;
        assert_eq!(g.expand(1, Growth::FULL), "F[+F]F[+F[+F]F]F[+F]F");
        // A deterministic grammar ignores the seed.
        assert_eq!(g.expand(1, Growth::FULL), g.expand(99, Growth::FULL));
    }

    #[test]
    fn a_stochastic_rule_follows_the_seed_and_repeats_for_it() {
        let mut g = simple();
        g.depth = 3;
        g.rules.insert('F', vec![alt("FF", 1), alt("F[+F]", 1)]);
        assert_eq!(g.expand(4, Growth::FULL), g.expand(4, Growth::FULL));
        let words: Vec<String> = (0..8).map(|s| g.expand(s, Growth::FULL)).collect();
        assert!(words.iter().any(|w| *w != words[0]), "some seed gives a different tree");
    }

    #[test]
    fn a_seed_gives_the_same_model_twice() {
        let g = oak();
        let a = g.grow(11, [10.0, 10.0, 18.0], Growth::FULL);
        assert_eq!(a, g.grow(11, [10.0, 10.0, 18.0], Growth::FULL));
        assert_ne!(a, g.grow(12, [10.0, 10.0, 18.0], Growth::FULL));
    }

    #[test]
    fn a_model_fits_its_declared_size() {
        for size in [[10.0, 10.0, 18.0], [3.0, 3.0, 9.0], [16.0, 14.0, 24.0]] {
            for seed in [0u64, 5, 41] {
                let m = oak().grow(seed, size, Growth::FULL);
                let got = [m.bounds.size()[0], m.bounds.size()[1], m.bounds.max[2]];
                for i in 0..3 {
                    assert!((got[i] - size[i]).abs() / size[i] < 0.05, "axis {i}: {got:?} against {size:?} at seed {seed}");
                }
                assert!(m.bounds.min[2] <= 1e-3, "the foot of the trunk is the ground: {:?}", m.bounds);
                assert!(!m.segments.is_empty() && !m.leaves.is_empty());
            }
        }
    }

    #[test]
    fn depth_changes_the_tree_but_not_its_size() {
        let size = [8.0, 8.0, 14.0];
        let mut g = oak();
        g.depth = 3;
        let shallow = g.grow(3, size, Growth::FULL);
        g.depth = 6;
        let deep = g.grow(3, size, Growth::FULL);
        assert!(deep.segments.len() > shallow.segments.len() * 2);
        for m in [&shallow, &deep] {
            assert!((m.bounds.max[2] - size[2]).abs() / size[2] < 0.05);
            assert!((m.bounds.size()[0] - size[0]).abs() / size[0] < 0.05);
        }
    }

    #[test]
    fn no_foliage_leaves_the_same_branches() {
        let size = [10.0, 10.0, 18.0];
        let leafy = oak().grow(9, size, Growth::FULL);
        let bare = oak().grow(9, size, Growth::BARE);
        assert!(!leafy.leaves.is_empty());
        assert!(bare.leaves.is_empty(), "foliage 0 drops every cluster");
        // The same skeleton at the same scale; only the centre of the
        // footprint moves, by the width the leaves added to it.
        assert_eq!(bare.segments.len(), leafy.segments.len(), "the skeleton is the same tree");
        for (b, l) in bare.segments.iter().zip(&leafy.segments) {
            assert!((b.radius - l.radius).abs() < 0.01, "{b:?} against {l:?}");
            for i in 0..3 {
                assert!((b.a[i] - l.a[i]).abs() < 0.05 && (b.b[i] - l.b[i]).abs() < 0.05, "{b:?} against {l:?}");
            }
        }
        assert_eq!(bare.state, State::Alive);
        // Half the foliage is the same clusters at half the radius.
        let half = oak().grow(9, size, Growth { foliage: 0.5, state: State::Alive });
        assert_eq!(half.leaves.len(), leafy.leaves.len());
        assert!((half.leaves[0].radius[0] - leafy.leaves[0].radius[0] * 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_dead_tree_is_bare_and_broken() {
        let size = [10.0, 10.0, 18.0];
        let live = oak().grow(9, size, Growth::FULL);
        let dead = oak().grow(9, size, Growth::DEAD);
        assert!(dead.leaves.is_empty());
        assert!(dead.segments.len() < live.segments.len(), "{} against {}", dead.segments.len(), live.segments.len());
        assert_eq!(dead.state, State::Dead);
        assert!(dead.bounds.max[2] < live.bounds.max[2], "a broken crown stands lower");
        // Without dead rules the grammar simply grows one level less.
        let mut g = oak();
        g.dead_rules.clear();
        let plain = g.grow(9, size, Growth::DEAD);
        assert!(plain.segments.len() < live.segments.len());
        assert!(plain.leaves.is_empty());
    }

    #[test]
    fn dead_bark_is_greyer_than_live_bark() {
        let live = oak().grow(1, [8.0, 8.0, 12.0], Growth::FULL);
        let dead = oak().grow(1, [8.0, 8.0, 12.0], Growth::DEAD);
        let bark = Rgb(92, 60, 40);
        assert_eq!(live.bark(bark), bark);
        let grey = dead.bark(bark);
        let spread = |c: Rgb| c.0.max(c.1).max(c.2) - c.0.min(c.1).min(c.2);
        assert!(spread(grey) < spread(bark), "{grey:?} against {bark:?}");
    }

    #[test]
    fn the_turtle_frame_stays_orthonormal_through_every_turn() {
        let mut t = Turtle { pos: [0.0; 3], frame: [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]], len: 1.0, rad: 0.1 };
        let (sin, cos) = 37f32.to_radians().sin_cos();
        for step in 0..40 {
            t.turn(step % 3, if step % 2 == 0 { sin } else { -sin }, cos);
        }
        for i in 0..3 {
            assert!((dot(t.frame[i], t.frame[i]) - 1.0).abs() < 1e-3, "unit axis {i}");
            for j in 0..3 {
                if i != j {
                    assert!(dot(t.frame[i], t.frame[j]).abs() < 1e-3, "axes {i} and {j} are square");
                }
            }
        }
    }

    #[test]
    fn volumes_carry_every_segment_and_leaf() {
        let m = oak().grow(2, [9.0, 9.0, 15.0], Growth::FULL);
        let v = m.volumes();
        assert_eq!(v.len(), m.segments.len() + m.leaves.len());
        assert!(matches!(v[v.len() - 1], Volume::Ellipsoid { .. }));
        let Volume::Cylinder { a, b, radius } = v[0] else { panic!("the first volume is a branch") };
        assert_eq!(Segment { a, b, radius }, m.segments[0]);
        // The clusters also convert to the walk's own placed volumes.
        let placed = m.canopies(3, 10, -4, 10.5, -3.5, 12.0);
        assert_eq!(placed.len(), m.leaves.len());
        assert_eq!(placed[0].shape, crate::volume::Shape::Ellipsoid);
        assert_eq!((placed[0].mx, placed[0].my, placed[0].species), (10, -4, 3));
        assert!((placed[0].top() - (12.0 + m.leaves[0].centre[2] + m.leaves[0].radius[2])).abs() < 1e-3);
    }

    #[test]
    fn a_trunk_radius_sets_the_thickest_branch_and_keeps_the_taper() {
        let mut m = oak().grow(6, [10.0, 10.0, 18.0], Growth::FULL);
        let thin = m.segments.iter().map(|s| s.radius).fold(f32::MAX, f32::min);
        let thick = m.segments.iter().map(|s| s.radius).fold(0.0, f32::max);
        m.set_trunk_radius(0.5);
        let after: Vec<f32> = m.segments.iter().map(|s| s.radius).collect();
        assert!((after.iter().copied().fold(0.0, f32::max) - 0.5).abs() < 1e-4);
        assert!((after.iter().copied().fold(f32::MAX, f32::min) - thin * 0.5 / thick).abs() < 1e-5);
    }

    #[test]
    fn validation_names_what_is_wrong() {
        let bad = |edit: &dyn Fn(&mut Grammar)| {
            let mut g = simple();
            edit(&mut g);
            g.validate().expect_err("rejected")
        };
        assert!(bad(&|g| g.axiom = String::new()).contains("axiom"));
        assert!(bad(&|g| g.axiom = "F[+F".to_string()).contains("open"));
        assert!(bad(&|g| g.axiom = "F]".to_string()).contains("never opened"));
        assert!(bad(&|g| g.axiom = "F*F".to_string()).contains("unknown symbol"));
        assert!(bad(&|g| g.depth = 30).contains("depth"));
        assert!(bad(&|g| g.angle = 400.0).contains("angle"));
        assert!(bad(&|g| g.length = 0.0).contains("length"));
        assert!(bad(&|g| g.taper = 2.0).contains("taper"));
        assert!(bad(&|g| g.leaf_radius = -1.0).contains("leaf_radius"));
        assert!(bad(&|g| {
            g.rules.insert('+', vec![alt("F", 1)]);
        })
        .contains("must be a letter"));
        assert!(bad(&|g| g.rules.get_mut(&'F').unwrap()[0].weight = 0).contains("weight 0"));
        assert!(bad(&|g| {
            g.dead_rules.insert('F', vec![alt("+-", 1)]);
        })
        .contains("dead rules draw nothing"));
        assert!(bad(&|g| {
            g.axiom = "A".to_string();
            g.rules = BTreeMap::from([('A', vec![alt("+A-", 1)])]);
        })
        .contains("draws nothing"));
        simple().validate().unwrap();
        oak().validate().unwrap();
    }
}
