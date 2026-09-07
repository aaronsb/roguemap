//! Growth habits: the eight ways a tree is put together, as parametric
//! grammars in `assets/tree_styles.toml`.
//!
//! Most trees are one of a small number of habits. A style holds the
//! grammar for one of them with named parameters and their defaults; a
//! species names a style, gives its size, and overrides the parameters it
//! wants different. Raw `axiom` and `rules` on a species remain the escape
//! hatch for a tree no habit describes.
//!
//! The rule strings are templates: `(BODY)*N` repeats `BODY` N times, where
//! N is a whole number or one of the counts derived from the parameters
//! (`forks`, `spread`, `pitch`, `lift`). That is what makes one grammar
//! serve a five-branch conifer whorl and a two-way oak fork.

use std::collections::BTreeMap;

use crate::properties::Identity;
use crate::volume::Shape;

use super::{Alternative, Grammar};

/// The knobs a style carries and a species may override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Turn of every `+ - & ^ \ /`, in degrees.
    pub branch_angle: f32,
    /// Branches in a whorl, or ways a trunk forks.
    pub forks: u8,
    /// Length and radius factor per branch level and per `!`.
    pub taper: f32,
    /// Bend per segment inside a branch, as a fraction of `branch_angle`;
    /// negative lifts.
    pub droop: f32,
    /// Chance an `L` becomes a cluster.
    pub leaf_density: f32,
    /// How one-sided the instance is.
    pub asymmetry: f32,
    /// Small noise on every branch and cluster.
    pub jitter: f32,
    /// Fraction of the height with no live branches.
    pub prune_height: f32,
    /// Rewritings.
    pub depth: u8,
    /// Length of a first-level `F`, in metres.
    pub length: f32,
    /// Radius of an `L` cluster, in metres.
    pub leaf_radius: f32,
    /// How flat a cluster on a level branch is, 0 round to 1 flat.
    pub leaf_flat: f32,
}

/// The counts a template may repeat by, matched whole after `*`.
pub const COUNTS: [&str; 4] = ["forks", "spread", "pitch", "lift"];

/// Longest repeat a template may ask for, so a runaway count cannot eat
/// the expansion.
const MAX_REPEAT: u32 = 64;

impl Params {
    /// The counts a template may repeat by, derived from the parameters:
    /// `forks` branches in a whorl, `spread` rolls between them (about a
    /// full turn divided by the count), `pitch` steps to swing a branch
    /// near horizontal, `lift` steps to angle one upward.
    pub fn count(&self, name: &str) -> Option<u32> {
        let angle = self.branch_angle.max(1.0);
        let forks = self.forks.max(1) as f32;
        Some(match name {
            "forks" => self.forks.max(1) as u32,
            "spread" => (360.0 / (forks * angle)).round().max(1.0) as u32,
            "pitch" => (72.0 / angle).round().max(1.0) as u32,
            "lift" => (34.0 / angle).round().max(1.0) as u32,
            _ => return None,
        })
    }
}

/// One growth habit.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub name: String,
    pub identity: Identity,
    /// The canopy volume that stands in for this habit at far zooms.
    pub stand_in: Shape,
    pub axiom: String,
    pub rules: BTreeMap<char, Vec<Alternative>>,
    pub dead_rules: BTreeMap<char, Vec<Alternative>>,
    pub params: Params,
}

impl Style {
    /// The volume shape the ray walk uses for this habit until the L-system
    /// adapter lands, and at far zooms after it does: a cone for a conifer,
    /// an ellipsoid for a spreading crown, a dome for a shrub.
    pub fn stand_in(&self) -> Shape {
        self.stand_in
    }

    /// The grammar for a species: this habit's templates expanded with its
    /// parameters, after `over` has changed the ones the species names.
    pub fn grammar(&self, over: &Overrides) -> Result<Grammar, String> {
        let p = over.apply(self.params);
        let expand = |table: &BTreeMap<char, Vec<Alternative>>| -> Result<BTreeMap<char, Vec<Alternative>>, String> {
            table
                .iter()
                .map(|(sym, alts)| {
                    let out: Result<Vec<Alternative>, String> = alts.iter().map(|a| Ok(Alternative { replacement: template(&a.replacement, &p)?, weight: a.weight })).collect();
                    Ok((*sym, out?))
                })
                .collect()
        };
        Ok(Grammar {
            axiom: template(&self.axiom, &p)?,
            rules: expand(&self.rules)?,
            dead_rules: expand(&self.dead_rules)?,
            depth: p.depth,
            angle: p.branch_angle,
            length: p.length,
            taper: p.taper,
            leaf_radius: p.leaf_radius,
            droop: p.droop,
            leaf_density: p.leaf_density,
            asymmetry: p.asymmetry,
            jitter: p.jitter,
            prune_height: p.prune_height,
            leaf_flat: p.leaf_flat,
        })
    }
}

/// What a species changes about its style. Every field is optional; an
/// absent one keeps the style's default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Overrides {
    pub branch_angle: Option<f32>,
    pub forks: Option<u8>,
    pub taper: Option<f32>,
    pub droop: Option<f32>,
    pub leaf_density: Option<f32>,
    pub asymmetry: Option<f32>,
    pub jitter: Option<f32>,
    pub prune_height: Option<f32>,
    pub depth: Option<u8>,
    pub length: Option<f32>,
    pub leaf_radius: Option<f32>,
    pub leaf_flat: Option<f32>,
}

impl Overrides {
    pub fn apply(&self, base: Params) -> Params {
        Params {
            branch_angle: self.branch_angle.unwrap_or(base.branch_angle),
            forks: self.forks.unwrap_or(base.forks),
            taper: self.taper.unwrap_or(base.taper),
            droop: self.droop.unwrap_or(base.droop),
            leaf_density: self.leaf_density.unwrap_or(base.leaf_density),
            asymmetry: self.asymmetry.unwrap_or(base.asymmetry),
            jitter: self.jitter.unwrap_or(base.jitter),
            prune_height: self.prune_height.unwrap_or(base.prune_height),
            depth: self.depth.unwrap_or(base.depth),
            length: self.length.unwrap_or(base.length),
            leaf_radius: self.leaf_radius.unwrap_or(base.leaf_radius),
            leaf_flat: self.leaf_flat.unwrap_or(base.leaf_flat),
        }
    }
}

/// Expand `(BODY)*N` repeat groups, nested to any depth. `N` is a whole
/// number or a parameter count (`forks`, `spread`, `pitch`, `lift`).
pub fn template(src: &str, p: &Params) -> Result<String, String> {
    let chars: Vec<char> = src.chars().collect();
    let (out, end) = group(&chars, 0, p, false)?;
    if end != chars.len() {
        return Err(format!("{src:?}: a repeat group closes that was never opened"));
    }
    Ok(out)
}

/// Expand from `at` until the end, or until the `)` that closes the group
/// this call is inside. Returns the text and where it stopped.
fn group(chars: &[char], at: usize, p: &Params, nested: bool) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut i = at;
    while i < chars.len() {
        match chars[i] {
            '(' => {
                let (body, end) = group(chars, i + 1, p, true)?;
                let (n, next) = count(chars, end, p)?;
                for _ in 0..n {
                    out.push_str(&body);
                }
                i = next;
            }
            ')' if nested => return Ok((out, i + 1)),
            ')' => return Err("a repeat group closes that was never opened".to_string()),
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    if nested {
        return Err("a repeat group is never closed".to_string());
    }
    Ok((out, i))
}

/// Read the `*N` after a group: a whole number or a parameter count.
fn count(chars: &[char], at: usize, p: &Params) -> Result<(u32, usize), String> {
    if chars.get(at) != Some(&'*') {
        return Err("a repeat group must be followed by *N".to_string());
    }
    let start = at + 1;
    // A number runs to its last digit, so `(F)*2A` repeats twice and then
    // carries on with `A`. A name is one of the four counts, matched
    // whole, so `(&)*pitchB` pitches and then carries on with `B`.
    if chars.get(start).is_some_and(|c| c.is_ascii_digit()) {
        let mut end = start;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let word: String = chars[start..end].iter().collect();
        return Ok((word.parse::<u32>().unwrap_or(0).min(MAX_REPEAT), end));
    }
    for name in COUNTS {
        if chars[start..].starts_with(&name.chars().collect::<Vec<char>>()[..]) {
            let n = p.count(name).unwrap_or(1);
            return Ok((n.min(MAX_REPEAT), start + name.len()));
        }
    }
    let word: String = chars[start..].iter().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if word.is_empty() {
        return Err("a repeat group must be followed by *N".to_string());
    }
    Err(format!("unknown repeat count {word:?}; the counts are {}, or a whole number", COUNTS.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params { branch_angle: 20.0, forks: 5, taper: 0.9, droop: 0.0, leaf_density: 1.0, asymmetry: 0.1, jitter: 0.08, prune_height: 0.2, depth: 6, length: 1.0, leaf_radius: 0.4, leaf_flat: 0.0 }
    }

    #[test]
    fn a_template_repeats_by_number_and_by_parameter() {
        let p = params();
        assert_eq!(template("FFA", &p).unwrap(), "FFA");
        assert_eq!(template("(F)*3A", &p).unwrap(), "FFFA");
        assert_eq!(template("(&)*pitch", &p).unwrap(), "&&&&");
        assert_eq!(template("([B](/)*spread)*forks", &p).unwrap(), "[B]////[B]////[B]////[B]////[B]////");
        // Nesting works, and a zero count drops the group.
        assert_eq!(template("((AB)*2C)*2", &p).unwrap(), "ABABCABABC");
        assert_eq!(template("(X)*0Y", &p).unwrap(), "Y");
    }

    #[test]
    fn counts_follow_the_parameters() {
        let mut p = params();
        assert_eq!(p.count("spread"), Some(4));
        p.forks = 2;
        p.branch_angle = 30.0;
        assert_eq!(p.count("spread"), Some(6));
        assert_eq!(p.count("pitch"), Some(2));
        assert_eq!(p.count("forks"), Some(2));
        assert_eq!(p.count("nonsense"), None);
    }

    #[test]
    fn a_broken_template_says_what_is_wrong() {
        let p = params();
        assert!(template("(F*2", &p).unwrap_err().contains("never closed"));
        assert!(template("F)*2", &p).unwrap_err().contains("never opened"));
        assert!(template("(F)2", &p).unwrap_err().contains("*N"));
        assert!(template("(F)*wide", &p).unwrap_err().contains("wide"));
    }

    #[test]
    fn overrides_replace_only_what_they_name() {
        let base = params();
        let o = Overrides { asymmetry: Some(0.4), forks: Some(3), ..Overrides::default() };
        let p = o.apply(base);
        assert_eq!((p.asymmetry, p.forks), (0.4, 3));
        assert_eq!((p.branch_angle, p.depth, p.leaf_radius), (base.branch_angle, base.depth, base.leaf_radius));
    }
}
