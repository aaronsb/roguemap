//! Trees as volumes (ADR-002, docs/structures.md): a species names a canopy
//! shape and a trunk, and the ray walk tests the volumes whose footprint
//! covers its ground point. Every shape is a quadratic in `z` along the
//! ray's ground path, so the crossing is a closed-form root.

use serde::{Deserialize, Serialize};

use crate::biome::{Form, SizeClass};

/// Canopy shape of a species.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Apex up, base at the top of the trunk.
    Cone,
    /// Centred half way up the canopy.
    Ellipsoid,
    /// The upper half of an ellipsoid, sitting on the ground.
    Dome,
    /// A column, with two arms at the closest zoom.
    Cactus,
    /// Branches and leaf clusters grown from an L-system grammar
    /// (docs/lsystem.md). The walk has no primitive for a branch at an
    /// arbitrary angle yet, so a placed L-system tree is tested as the
    /// ellipsoid envelope of its crown; its own geometry comes from
    /// `lsystem::TreeModel::volumes`.
    Lsystem,
}

impl Shape {
    /// The shape a form is drawn with when the species does not say.
    pub fn for_form(form: Form) -> Shape {
        match form {
            Form::Pine => Shape::Cone,
            Form::Broadleaf => Shape::Ellipsoid,
            Form::Scrub => Shape::Dome,
            Form::Cactus => Shape::Cactus,
        }
    }
}

/// Canopy dimensions of a species in metres: radius, canopy height, trunk
/// height and trunk radius, before the size class and variant scale them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dims {
    pub radius: f32,
    pub height: f32,
    pub trunk: f32,
    pub trunk_radius: f32,
}

/// Fraction of a tree's height with no live crown under it: a tree
/// self-prunes as it grows, so a conifer carries its crown from a fifth of
/// its height and a broadleaf from a third, and from beside or below there
/// is bare trunk under the canopy. A shrub or a cactus has none. The
/// L-system's own `prune_height` overrides it for a species that has one.
pub fn prune_height(shape: Shape) -> f32 {
    match shape {
        Shape::Cone => 0.2,
        Shape::Ellipsoid | Shape::Lsystem => 0.35,
        Shape::Dome | Shape::Cactus => 0.0,
    }
}

impl Dims {
    /// Dimensions from a mature tree's spread and height `[w, d, h]` in
    /// metres: the crown radius is the mean half-spread, and the shape's
    /// prune height says how much of the height is bare trunk.
    pub fn from_size(shape: Shape, size: [f32; 3]) -> Dims {
        Dims::pruned(shape, size, prune_height(shape))
    }

    /// The same with the crown base given as a fraction of the height.
    pub fn pruned(shape: Shape, size: [f32; 3], prune: f32) -> Dims {
        let radius = 0.25 * (size[0] + size[1]);
        let h = size[2];
        let prune = prune.clamp(0.0, 0.8);
        match shape {
            Shape::Dome | Shape::Cactus => Dims { radius, height: h, trunk: 0.0, trunk_radius: 0.0 },
            _ => Dims { radius, height: (1.0 - prune) * h, trunk: prune * h, trunk_radius: 0.02 * h },
        }
    }
}

/// Scale of a size class on the species' dimensions.
pub fn size_scale(size: SizeClass) -> f32 {
    match size {
        SizeClass::Small => 0.8,
        SizeClass::Mixed => 1.0,
        SizeClass::Large => 1.15,
    }
}

/// Scale of a tile's variant, so a stand has old and young trees.
pub fn variant_scale(variant: u8) -> f32 {
    [0.8, 0.9, 1.0, 1.1][variant as usize % 4]
}

/// One instance's own scales from its tile's seed: height, crown radius
/// and canopy brightness, each a quarter either way (an eighth for the
/// colour), so no two trees in a stand are the same tree.
pub fn instance(seed: u32) -> Instance {
    let unit = |k: u32| ((seed.wrapping_mul(k) >> 9) % 1024) as f32 / 1024.0;
    Instance { height: 0.75 + 0.5 * unit(0x9E3779B1), radius: 0.75 + 0.5 * unit(0x85EBCA77), tint: 0.88 + 0.24 * unit(0xC2B2AE35), hue: unit(0x27D4EB2F) - 0.5 }
}

/// The variation one tree carries over its species.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Instance {
    pub height: f32,
    pub radius: f32,
    /// Brightness factor on the canopy colour.
    pub tint: f32,
    /// Hue shift in `-0.5..0.5`, a nudge toward yellow or blue.
    pub hue: f32,
}

/// Whether an instance of a species stands dead, from its tile's seed.
pub fn stands_dead(seed: u32, dead_chance: f32) -> bool {
    ((seed.wrapping_mul(0x165667B1) >> 11) % 1024) as f32 / 1024.0 < dead_chance
}

/// One tree standing in the world for this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Volume {
    pub shape: Shape,
    /// Ground point of the trunk in tiles, jittered within the tile.
    pub cx: f32,
    pub cy: f32,
    /// Ground height under the trunk.
    pub ground: f32,
    /// Base and height of the canopy in height units (metres).
    pub h0: f32,
    pub height: f32,
    /// Radii in tiles.
    pub radius: f32,
    pub trunk_radius: f32,
    /// Ground shift of the canopy per unit of `(z - h0) / height`, from
    /// the wind.
    pub shear: (f32, f32),
    /// Index into the species table.
    pub species: u8,
    /// What this tree carries over its species: colour and, through the
    /// dimensions above, size.
    pub instance: Instance,
    /// A snag: no foliage, a grey-brown crown, and it still casts.
    pub dead: bool,
    pub mx: i32,
    pub my: i32,
}

/// What part of a volume a ray met.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Canopy,
    Trunk,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VolumeHit {
    pub z: f32,
    pub part: Part,
    /// Outward normal, unnormalised.
    pub normal: (f32, f32, f32),
}

/// Largest root of `a z^2 + b z + c = 0` within `[lo, hi]`, if any.
fn largest_root(a: f32, b: f32, c: f32, lo: f32, hi: f32) -> Option<f32> {
    if a.abs() < 1e-9 {
        if b.abs() < 1e-9 {
            return None;
        }
        let z = -c / b;
        return (z >= lo && z <= hi).then_some(z);
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let (r1, r2) = ((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
    let (small, big) = if r1 < r2 { (r1, r2) } else { (r2, r1) };
    if big >= lo && big <= hi {
        Some(big)
    } else if small >= lo && small <= hi {
        Some(small)
    } else {
        None
    }
}

impl Volume {
    /// Top of the canopy.
    pub fn top(&self) -> f32 {
        self.h0 + self.height
    }

    /// Ground box the volume can cover, with the wind shear and any arms.
    pub fn footprint(&self) -> (f32, f32, f32, f32) {
        let arms = if self.shape == Shape::Cactus { 2.8 } else { 1.0 };
        let r = self.radius * arms + self.shear.0.abs().max(self.shear.1.abs());
        (self.cx - r, self.cy - r, self.cx + r, self.cy + r)
    }

    /// Test the ground path `p(z) = p0 + d z` for `z` in `[lo, hi]` against
    /// the canopy and the trunk, returning the nearest (highest) crossing.
    /// `arms` adds the cactus arms.
    pub fn hit(&self, p0: (f32, f32), d: (f32, f32), lo: f32, hi: f32, arms: bool) -> Option<VolumeHit> {
        // Cheap rejects first: the segment is below the ground or above the
        // crown, or its ground path stays clear of the widest part.
        if hi < self.ground || lo > self.h0 + self.height {
            return None;
        }
        let widest = if arms && self.shape == Shape::Cactus { self.radius * 2.8 } else { self.radius };
        let reach = widest + self.shear.0.abs().max(self.shear.1.abs()) + 0.5 * (d.0.abs() + d.1.abs()) * (hi - lo);
        let (ax, ay) = (p0.0 + d.0 * lo - self.cx, p0.1 + d.1 * lo - self.cy);
        let (bx, by) = (p0.0 + d.0 * hi - self.cx, p0.1 + d.1 * hi - self.cy);
        if (ax * ax + ay * ay).min(bx * bx + by * by) > reach * reach {
            return None;
        }
        let mut best: Option<VolumeHit> = None;
        let mut consider = |h: Option<VolumeHit>| {
            if let Some(h) = h {
                if best.is_none_or(|b| h.z > b.z) {
                    best = Some(h);
                }
            }
        };
        let (h0, hh) = (self.h0, self.height);
        let clo = lo.max(h0);
        let chi = hi.min(h0 + hh);
        if clo <= chi {
            // The canopy centre drifts with the wind by height: the offset
            // path is still linear in z.
            let (sx, sy) = (self.shear.0 / hh.max(1e-3), self.shear.1 / hh.max(1e-3));
            let a = (p0.0 - self.cx + sx * h0, p0.1 - self.cy + sy * h0);
            let b = (d.0 - sx, d.1 - sy);
            let (aa, ab, bb) = (a.0 * a.0 + a.1 * a.1, a.0 * b.0 + a.1 * b.1, b.0 * b.0 + b.1 * b.1);
            let r2 = self.radius * self.radius;
            match self.shape {
                Shape::Ellipsoid | Shape::Lsystem => {
                    let zc = h0 + 0.5 * hh;
                    let v2 = 0.25 * hh * hh;
                    let z = largest_root(bb / r2 + 1.0 / v2, 2.0 * ab / r2 - 2.0 * zc / v2, aa / r2 + zc * zc / v2 - 1.0, clo, chi);
                    consider(z.map(|z| VolumeHit { z, part: Part::Canopy, normal: (q(a, b, z).0 / r2, q(a, b, z).1 / r2, (z - zc) / v2) }));
                }
                Shape::Dome => {
                    let v2 = hh * hh;
                    let z = largest_root(bb / r2 + 1.0 / v2, 2.0 * ab / r2 - 2.0 * h0 / v2, aa / r2 + h0 * h0 / v2 - 1.0, clo, chi);
                    consider(z.map(|z| VolumeHit { z, part: Part::Canopy, normal: (q(a, b, z).0 / r2, q(a, b, z).1 / r2, (z - h0) / v2) }));
                }
                Shape::Cone => {
                    // |q| = R (1 - (z - h0) / H) = c0 + c1 z.
                    let c1 = -self.radius / hh;
                    let c0 = self.radius - c1 * h0;
                    let z = largest_root(bb - c1 * c1, 2.0 * (ab - c0 * c1), aa - c0 * c0, clo, chi);
                    consider(z.map(|z| {
                        let qq = q(a, b, z);
                        let len = (qq.0 * qq.0 + qq.1 * qq.1).sqrt().max(1e-4);
                        VolumeHit { z, part: Part::Canopy, normal: (qq.0 / len, qq.1 / len, self.radius / hh) }
                    }));
                }
                Shape::Cactus => {
                    consider(cylinder(a, b, aa, ab, bb, self.radius, clo, chi, h0 + hh, Part::Canopy));
                    if arms {
                        let r = self.radius * 0.6;
                        for (side, at, len) in [(-1.0, 0.45, 0.35), (1.0, 0.6, 0.25)] {
                            let off = side * self.radius * 2.2;
                            let (z0, z1) = (h0 + at * hh, h0 + (at + len) * hh);
                            // The arm: a smaller column beside the trunk.
                            let aa2 = (a.0 - off, a.1);
                            let dot = aa2.0 * aa2.0 + aa2.1 * aa2.1;
                            let ab2 = aa2.0 * b.0 + aa2.1 * b.1;
                            consider(cylinder(aa2, b, dot, ab2, bb, r, clo.max(z0), chi.min(z1), z1, Part::Canopy));
                            // The elbow: a box from the trunk to the arm.
                            let (bx0, bx1) = if side < 0.0 { (off, 0.0) } else { (0.0, off) };
                            consider(slab_box(a, b, bx0, bx1, -r, r, z0, z0 + 2.0 * r, clo, chi));
                        }
                    }
                }
            }
        }
        // The trunk: a thin column from the ground into the canopy.
        if self.trunk_radius > 0.0 && self.h0 > self.ground {
            let tlo = lo.max(self.ground);
            let thi = hi.min(self.h0 + 0.3 * self.height);
            if tlo <= thi {
                let a = (p0.0 - self.cx, p0.1 - self.cy);
                let (aa, ab, bb) = (a.0 * a.0 + a.1 * a.1, a.0 * d.0 + a.1 * d.1, d.0 * d.0 + d.1 * d.1);
                consider(cylinder(a, d, aa, ab, bb, self.trunk_radius, tlo, thi, f32::INFINITY, Part::Trunk));
            }
        }
        best
    }
}

/// Ground offset from the axis at height `z` along the offset path.
#[inline]
fn q(a: (f32, f32), b: (f32, f32), z: f32) -> (f32, f32) {
    (a.0 + b.0 * z, a.1 + b.1 * z)
}

/// A vertical cylinder of radius `r` on `[lo, hi]` with a flat cap at
/// `cap` when the segment reaches it.
#[allow(clippy::too_many_arguments)]
fn cylinder(a: (f32, f32), b: (f32, f32), aa: f32, ab: f32, bb: f32, r: f32, lo: f32, hi: f32, cap: f32, part: Part) -> Option<VolumeHit> {
    if lo > hi {
        return None;
    }
    // A path parallel to the axis is inside or outside for good: inside, it
    // meets the column where the segment starts.
    if bb < 1e-9 {
        return (aa <= r * r).then_some(VolumeHit { z: hi, part, normal: (0.0, 0.0, 1.0) });
    }
    // The cap first: the segment crosses the top plane inside the disc.
    if cap <= hi && cap >= lo {
        let qq = q(a, b, cap);
        if qq.0 * qq.0 + qq.1 * qq.1 <= r * r {
            return Some(VolumeHit { z: cap, part, normal: (0.0, 0.0, 1.0) });
        }
    }
    let z = largest_root(bb, 2.0 * ab, aa - r * r, lo, hi)?;
    let qq = q(a, b, z);
    Some(VolumeHit { z, part, normal: (qq.0, qq.1, 0.0) })
}

/// An axis-aligned box relative to the axis, entered at its highest z.
#[allow(clippy::too_many_arguments)]
fn slab_box(a: (f32, f32), b: (f32, f32), x0: f32, x1: f32, y0: f32, y1: f32, z0: f32, z1: f32, lo: f32, hi: f32) -> Option<VolumeHit> {
    let (mut za, mut zb) = (lo.max(z0), hi.min(z1));
    let mut normal = (0.0, 0.0, 1.0);
    for (v0, dv, m0, m1, n) in [(a.0, b.0, x0, x1, (1.0, 0.0)), (a.1, b.1, y0, y1, (0.0, 1.0))] {
        if dv.abs() < 1e-6 {
            if v0 < m0 || v0 > m1 {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((m0 - v0) / dv, (m1 - v0) / dv);
        let (ta, tb) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        za = za.max(ta);
        if tb < zb {
            zb = tb;
            normal = if dv > 0.0 { (n.0, n.1, 0.0) } else { (-n.0, -n.1, 0.0) };
        }
    }
    (za <= zb).then_some(VolumeHit { z: zb, part: Part::Canopy, normal })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oak() -> Volume {
        Volume { shape: Shape::Ellipsoid, cx: 0.0, cy: 0.0, ground: 0.0, h0: 1.0, height: 2.0, radius: 0.6, trunk_radius: 0.1, shear: (0.0, 0.0), species: 0, instance: instance(0), dead: false, mx: 0, my: 0 }
    }

    #[test]
    fn a_vertical_ray_meets_the_canopy_top_and_the_trunk_below_it() {
        let v = oak();
        let down = (0.0, 0.0);
        let hit = v.hit((0.0, 0.0), down, 0.0, 10.0, false).expect("through the centre");
        assert_eq!(hit.part, Part::Canopy);
        assert!((hit.z - 3.0).abs() < 1e-4, "the top of the ellipsoid: {}", hit.z);
        assert!(hit.normal.2 > 0.0 && hit.normal.0.abs() < 1e-6, "{:?}", hit.normal);
        // Off centre the surface is lower and the normal leans outward.
        let side = v.hit((0.5, 0.0), down, 0.0, 10.0, false).unwrap();
        assert!(side.z < 3.0 && side.z > 2.0, "{}", side.z);
        assert!(side.normal.0 > 0.0);
        // Beyond the radius only the ground is left.
        assert!(v.hit((0.7, 0.0), down, 0.0, 10.0, false).is_none());
        // Below the canopy the trunk is a thin column.
        let trunk = v.hit((0.05, 0.0), down, 0.0, 0.9, false).expect("the trunk");
        assert_eq!(trunk.part, Part::Trunk);
        assert!((trunk.z - 0.9).abs() < 1e-4, "the segment's top is inside the trunk: {}", trunk.z);
        assert!(v.hit((0.3, 0.0), down, 0.0, 0.9, false).is_none(), "beside the trunk under the crown is air");
    }

    #[test]
    fn a_slanting_ray_finds_the_nearest_crossing_in_its_segment() {
        let v = oak();
        // p(z) = (-2 + z, 0): crosses the ellipsoid's side around z = 2. The
        // higher z is nearer the camera, and there the path is at larger x,
        // so the crossing found is on the +x side.
        let d = (1.0, 0.0);
        let hit = v.hit((-2.0, 0.0), d, 0.0, 10.0, false).expect("the near side");
        assert_eq!(hit.part, Part::Canopy);
        let x = -2.0 + hit.z;
        let inside = x * x / 0.36 + (hit.z - 2.0) * (hit.z - 2.0) / 1.0;
        assert!((inside - 1.0).abs() < 1e-3, "the hit lies on the surface: {inside}");
        assert!(x > 0.0 && hit.normal.0 > 0.0, "the nearer crossing is the +x side: x {x}");
        // Restricting the segment below the crossing finds the far side or nothing.
        let lower = v.hit((-2.0, 0.0), d, 0.0, hit.z - 0.5, false);
        assert!(lower.is_none_or(|h| h.z < hit.z));
    }

    #[test]
    fn cones_and_domes_taper_and_cacti_have_caps() {
        let pine = Volume { shape: Shape::Cone, h0: 1.0, height: 3.0, radius: 0.5, ..oak() };
        let down = (0.0, 0.0);
        let apex = pine.hit((0.0, 0.0), down, 0.0, 10.0, false).unwrap();
        assert!((apex.z - 4.0).abs() < 1e-3, "{}", apex.z);
        let mid = pine.hit((0.25, 0.0), down, 0.0, 10.0, false).unwrap();
        assert!((mid.z - 2.5).abs() < 1e-3, "half way out the cone is half way up: {}", mid.z);
        assert!(mid.normal.0 > 0.0 && mid.normal.2 > 0.0);
        let bush = Volume { shape: Shape::Dome, h0: 0.0, height: 1.0, radius: 0.5, trunk_radius: 0.0, ..oak() };
        assert!((bush.hit((0.0, 0.0), down, 0.0, 10.0, false).unwrap().z - 1.0).abs() < 1e-3);
        assert!(bush.hit((0.0, 0.0), down, -5.0, -0.1, false).is_none(), "the dome has no underside");
        let cactus = Volume { shape: Shape::Cactus, h0: 0.0, height: 3.0, radius: 0.2, trunk_radius: 0.0, ..oak() };
        let cap = cactus.hit((0.1, 0.0), down, 0.0, 10.0, true).unwrap();
        assert!((cap.z - 3.0).abs() < 1e-4 && cap.normal == (0.0, 0.0, 1.0), "{cap:?}");
        assert!(cactus.hit((0.3, 0.0), down, 0.0, 10.0, false).is_none(), "no arms at a distance without arms");
        let arm = cactus.hit((-0.44, 0.0), down, 0.0, 10.0, true).expect("the left arm");
        assert!(arm.z > 1.0 && arm.z < 3.0, "{}", arm.z);
    }

    #[test]
    fn wind_shear_moves_the_crown_but_not_the_trunk() {
        let mut v = oak();
        v.shear = (0.5, 0.0);
        let down = (0.0, 0.0);
        let hit = v.hit((0.4, 0.0), down, 0.0, 10.0, false).unwrap();
        let still = oak().hit((0.4, 0.0), down, 0.0, 10.0, false).unwrap();
        assert!(hit.z > still.z, "the top of the crown has leaned toward +x: {} vs {}", hit.z, still.z);
        let trunk = v.hit((0.05, 0.0), down, 0.0, 0.9, false).unwrap();
        assert_eq!(trunk.part, Part::Trunk);
        assert_eq!(v.footprint(), (-1.1, -1.1, 1.1, 1.1));
    }
}
