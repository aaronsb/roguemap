//! Block geometry for structures (ADR-002, docs/structures.md): roof
//! profiles over merged runs of same-kind tiles, and the test of a screen
//! ray against one tile's column and roof.
//!
//! A stack is a column over its tile from the ground to the eaves at
//! `base + levels * level_height`; above the eaves the roof profile
//! `r(u, v)` rises inside the tile. Adjacent tiles of the same kind and
//! level count merge: the runs through a tile along each axis stretch the
//! profile across the seam, the shared faces stop being faces, and the
//! gable rides the longer run.

use serde::{Deserialize, Serialize};

use crate::map::TILE_METRES;

/// Profile above the column top.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Roof {
    None,
    #[default]
    Flat,
    Gable,
    Hip,
}

/// What the tile top under a stack becomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ground {
    None,
    #[default]
    Flatten,
    Pave,
    Till,
}

/// A block kind stacked on a tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    /// Index into the block table.
    pub kind: u8,
    /// Levels of the column; zero for ground kinds such as roads.
    pub levels: u8,
}

/// The merged runs through a tile: length and this tile's index along x
/// and along y, in tiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Runs {
    pub nx: u8,
    pub kx: u8,
    pub ny: u8,
    pub ky: u8,
    /// Whether the gable ridge runs along x.
    pub ridge_x: bool,
}

impl Runs {
    /// A tile on its own.
    pub const SINGLE: Runs = Runs { nx: 1, kx: 0, ny: 1, ky: 0, ridge_x: true };
}

/// Faces of a tile column, as bit and as index: +x, -x, +y, -y.
pub const FACE_PX: u8 = 0;
pub const FACE_NX: u8 = 1;
pub const FACE_PY: u8 = 2;
pub const FACE_NY: u8 = 3;
pub const NO_FACE: u8 = 255;

/// Outward normal of a column face.
pub fn face_normal(face: u8) -> (f32, f32) {
    match face {
        FACE_PX => (1.0, 0.0),
        FACE_NX => (-1.0, 0.0),
        FACE_PY => (0.0, 1.0),
        _ => (0.0, -1.0),
    }
}

/// The roof profile of a kind: metres of rise per metre of run from the
/// eaves, and the cap in metres.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Profile {
    pub roof: Roof,
    pub pitch: f32,
    pub max_rise: f32,
}

impl Profile {
    /// Rise above the eaves at `(u, v)` within a tile of the runs given.
    /// Distances to the ends of the merged runs are in tiles, so the
    /// profile continues across merged tiles and a lone tile peaks at its
    /// centre.
    pub fn rise(&self, runs: Runs, u: f32, v: f32) -> f32 {
        let along_x = (runs.kx as f32 + u).min(runs.nx as f32 - runs.kx as f32 - u);
        let along_y = (runs.ky as f32 + v).min(runs.ny as f32 - runs.ky as f32 - v);
        let d = match self.roof {
            Roof::None | Roof::Flat => return 0.0,
            Roof::Gable => {
                if runs.ridge_x {
                    along_y
                } else {
                    along_x
                }
            }
            Roof::Hip => along_x.min(along_y),
        };
        (self.pitch * d.max(0.0) * TILE_METRES).min(self.max_rise)
    }

    /// The highest point of the profile over these runs.
    pub fn peak(&self, runs: Runs) -> f32 {
        match self.roof {
            Roof::None | Roof::Flat => 0.0,
            Roof::Gable => {
                let n = if runs.ridge_x { runs.ny } else { runs.nx } as f32;
                (self.pitch * n * 0.5 * TILE_METRES).min(self.max_rise)
            }
            Roof::Hip => (self.pitch * (runs.nx.min(runs.ny) as f32) * 0.5 * TILE_METRES).min(self.max_rise),
        }
    }
}

/// Where a ray met a column: on the roof surface or entering a wall face.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ColumnHit {
    pub z: f32,
    /// `NO_FACE` for the roof, else the wall face entered.
    pub face: u8,
    /// Outward normal, unnormalised for the roof (`-dS/dx, -dS/dy, 1`).
    pub normal: (f32, f32, f32),
}

/// One tile's column for the ray test.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Column {
    pub mx: i32,
    pub my: i32,
    /// Height of the eaves.
    pub zs: f32,
    pub profile: Profile,
    pub runs: Runs,
}

impl Column {
    /// Roof height over a ground point.
    pub fn surface(&self, x: f32, y: f32) -> f32 {
        self.zs + self.profile.rise(self.runs, x - self.mx as f32, y - self.my as f32)
    }

    /// Test the ground path `p(z) = p0 + d z` over `z` in `[lo, hi]`
    /// (`hi` nearer the camera) against this column. The path is clipped to
    /// the footprint; if it is under the roof surface where it enters the
    /// footprint the hit is a wall on the face entered, otherwise the roof
    /// crossing is found by `bisections` halvings.
    pub fn hit(&self, p0: (f32, f32), d: (f32, f32), lo: f32, hi: f32, bisections: u32) -> Option<ColumnHit> {
        self.hit_along::<false>(p0, d, lo, hi, bisections)
    }

    /// The same test for a ray walking either way: `UP` for one climbing,
    /// which enters the footprint at the low end of its path, as a
    /// perspective eye's rays above the horizon do.
    pub fn hit_along<const UP: bool>(&self, p0: (f32, f32), d: (f32, f32), lo: f32, hi: f32, bisections: u32) -> Option<ColumnHit> {
        let (mut za, mut zb) = (lo, hi);
        let mut entry = NO_FACE;
        let mut entry_z = if UP { f32::NEG_INFINITY } else { f32::INFINITY };
        for (axis, x0, dx, m) in [(0, p0.0, d.0, self.mx as f32), (1, p0.1, d.1, self.my as f32)] {
            if dx.abs() < 1e-6 {
                if x0 < m || x0 >= m + 1.0 {
                    return None;
                }
                continue;
            }
            let t1 = (m - x0) / dx;
            let t2 = (m + 1.0 - x0) / dx;
            let (ta, tb) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            za = za.max(ta);
            // The near end is the larger z (walking down) or the smaller
            // (climbing); the path moves toward +axis with z when dx > 0,
            // so walking down it enters through the +axis face and
            // climbing through the -axis face.
            if UP {
                if ta > entry_z {
                    entry_z = ta;
                    entry = match (axis, dx > 0.0) {
                        (0, true) => FACE_NX,
                        (0, false) => FACE_PX,
                        (1, true) => FACE_NY,
                        _ => FACE_PY,
                    };
                }
            } else if tb < entry_z {
                entry_z = tb;
                entry = match (axis, dx > 0.0) {
                    (0, true) => FACE_PX,
                    (0, false) => FACE_NX,
                    (1, true) => FACE_PY,
                    _ => FACE_NY,
                };
            }
            zb = zb.min(tb);
        }
        if za > zb {
            return None;
        }
        let f = |z: f32| z - self.surface(p0.0 + d.0 * z, p0.1 + d.1 * z);
        let (near, far) = if UP { (za, zb) } else { (zb, za) };
        if f(near) <= 0.0 {
            // Under the roof where the path comes in: a wall if that is a
            // footprint edge, else the roof itself (only when the walk
            // started inside the column).
            let face = if (near - entry_z).abs() < 1e-5 { entry } else { NO_FACE };
            let (nx, ny) = if face == NO_FACE { (0.0, 0.0) } else { face_normal(face) };
            let normal = if face == NO_FACE { self.normal(p0.0 + d.0 * near, p0.1 + d.1 * near) } else { (nx, ny, 0.0) };
            return Some(ColumnHit { z: near, face, normal });
        }
        if f(far) > 0.0 {
            return None;
        }
        let (mut above, mut below) = (near, far);
        for _ in 0..bisections {
            let mid = 0.5 * (above + below);
            if f(mid) > 0.0 {
                above = mid;
            } else {
                below = mid;
            }
        }
        let z = 0.5 * (above + below);
        Some(ColumnHit { z, face: NO_FACE, normal: self.normal(p0.0 + d.0 * z, p0.1 + d.1 * z) })
    }

    /// Roof normal at a ground point from the profile's slope.
    pub fn normal(&self, x: f32, y: f32) -> (f32, f32, f32) {
        let e = 0.02;
        let sx = (self.surface(x + e, y) - self.surface(x - e, y)) / (2.0 * e);
        let sy = (self.surface(x, y + e) - self.surface(x, y - e)) / (2.0 * e);
        (-sx, -sy, 1.0)
    }
}

/// Whether two tiles' stacks are one laid thing: the same kind and level
/// count, and a kind that merges. This is what the runs are labelled with,
/// so a field or a road is one plot with an extent and an axis even though
/// it has no walls.
pub fn one_plot(a: Option<Stack>, b: Option<Stack>, kind_merges: impl Fn(u8) -> bool) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.kind == b.kind && a.levels == b.levels && kind_merges(a.kind),
        _ => false,
    }
}

/// Whether two tiles' stacks merge into one building: one plot, standing a
/// level above the ground, so its walls and roof are shared.
pub fn merges(a: Option<Stack>, b: Option<Stack>, kind_merges: impl Fn(u8) -> bool) -> bool {
    one_plot(a, b, kind_merges) && a.is_some_and(|a| a.levels > 0)
}

/// Label the merged runs along one axis of a line of tiles: for each
/// position, the run length and the index within it. `merged(i)` says
/// whether positions `i` and `i + 1` merge.
pub fn label_runs(n: usize, merged: impl Fn(usize) -> bool) -> Vec<(u8, u8)> {
    let mut out = vec![(1u8, 0u8); n];
    let mut start = 0;
    while start < n {
        let mut end = start;
        while end + 1 < n && merged(end) {
            end += 1;
        }
        let len = (end - start + 1).min(255) as u8;
        for (k, slot) in out.iter_mut().enumerate().take(end + 1).skip(start) {
            *slot = (len, (k - start).min(255) as u8);
        }
        start = end + 1;
    }
    out
}

/// Ridge axis for a tile from its runs: the longer run, and on a tie x when
/// the run's first tile has an even seed.
pub fn ridge_along_x(nx: u8, ny: u8, first_seed: u32) -> bool {
    match nx.cmp(&ny) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => first_seed.is_multiple_of(2),
    }
}

/// The door face: toward an open face with a road across it, else the first
/// open face in `(+y, +x, -y, -x)` rotated by the seed.
pub fn door_face(open: u8, road: u8, seed: u32) -> u8 {
    const ORDER: [u8; 4] = [FACE_PY, FACE_PX, FACE_NY, FACE_NX];
    if open == 0 {
        return NO_FACE;
    }
    if let Some(f) = ORDER.iter().copied().find(|f| open & (1 << f) != 0 && road & (1 << f) != 0) {
        return f;
    }
    let rot = (seed % 4) as usize;
    ORDER.iter().cycle().skip(rot).take(4).copied().find(|f| open & (1 << f) != 0).unwrap_or(NO_FACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gable rising `pitch` metres per metre of run: with a 2 m tile a
    /// pitch of 1 rises a metre from the eave to a lone tile's ridge.
    fn gable(pitch: f32, max_rise: f32) -> Profile {
        Profile { roof: Roof::Gable, pitch, max_rise }
    }

    #[test]
    fn gable_peaks_at_the_ridge_and_falls_to_the_eaves() {
        let p = gable(1.0, 3.0);
        let lone = Runs::SINGLE;
        assert_eq!(p.rise(lone, 0.5, 0.0), 0.0, "the eave");
        assert_eq!(p.rise(lone, 0.5, 1.0), 0.0, "the other eave");
        assert_eq!(p.rise(lone, 0.5, 0.5), 1.0, "the ridge of a lone tile is half a tile from either eave");
        assert_eq!(p.rise(lone, 0.0, 0.5), p.rise(lone, 1.0, 0.5), "along the ridge the profile is constant");
        assert!((p.rise(lone, 0.3, 0.25) - 0.5).abs() < 1e-6);
        // Across a run of three tiles the ridge is 1.5 tiles from each
        // eave, capped by max_rise.
        let three = Runs { nx: 1, kx: 0, ny: 3, ky: 1, ridge_x: true };
        assert_eq!(p.rise(three, 0.5, 0.5), 3.0, "the middle tile carries the ridge at the cap");
        assert_eq!(p.peak(three), 3.0);
        let end = Runs { nx: 1, kx: 0, ny: 3, ky: 0, ridge_x: true };
        assert_eq!(p.rise(end, 0.5, 0.0), 0.0, "the end tile starts at the eave");
        assert_eq!(p.rise(end, 0.5, 1.0), 2.0, "and rises to two units at its far seam");
        let mid = Runs { nx: 1, kx: 0, ny: 3, ky: 1, ridge_x: true };
        assert_eq!(p.rise(mid, 0.5, 0.0), 2.0, "the middle tile continues from the seam without a step");
        assert_eq!(gable(1.0, 3.0).peak(Runs::SINGLE), 1.0);
    }

    #[test]
    fn hip_rises_toward_the_centre_from_every_edge_and_flat_stays_flat() {
        let hip = Profile { roof: Roof::Hip, pitch: 1.0, max_rise: 5.0 };
        for (u, v) in [(0.0, 0.5), (1.0, 0.5), (0.5, 0.0), (0.5, 1.0), (0.0, 0.0)] {
            assert_eq!(hip.rise(Runs::SINGLE, u, v), 0.0, "({u}, {v}) is on an edge");
        }
        assert_eq!(hip.rise(Runs::SINGLE, 0.5, 0.5), 1.0);
        assert!((hip.rise(Runs::SINGLE, 0.25, 0.4) - 0.5).abs() < 1e-6, "the nearer edge decides");
        let two_by_two = Runs { nx: 2, kx: 1, ny: 2, ky: 0, ridge_x: true };
        assert_eq!(hip.rise(two_by_two, 0.0, 1.0), 2.0, "the inner corner of a 2x2 is the peak");
        assert_eq!(hip.peak(two_by_two), 2.0);
        let flat = Profile { roof: Roof::Flat, pitch: 1.0, max_rise: 5.0 };
        assert_eq!(flat.rise(two_by_two, 0.3, 0.7), 0.0);
        assert_eq!(flat.peak(two_by_two), 0.0);
    }

    #[test]
    fn runs_are_labelled_per_axis_and_the_longer_run_carries_the_ridge() {
        // A 1x3 line: one run of three.
        let line = label_runs(5, |i| (1..=2).contains(&i));
        assert_eq!(line, vec![(1, 0), (3, 0), (3, 1), (3, 2), (1, 0)]);
        assert!(!ridge_along_x(1, 3, 0), "a run along y ridges along y");
        assert!(ridge_along_x(3, 1, 1));
        assert!(ridge_along_x(2, 2, 4) && !ridge_along_x(2, 2, 7), "a tie goes by the first tile's seed");
        // Nothing merges: every tile is a run of one.
        assert!(label_runs(3, |_| false).iter().all(|&r| r == (1, 0)));
        assert_eq!(label_runs(0, |_| true), vec![]);
    }

    #[test]
    fn merging_needs_the_same_kind_and_levels_and_a_kind_that_merges() {
        let a = Some(Stack { kind: 0, levels: 1 });
        let b = Some(Stack { kind: 0, levels: 2 });
        let c = Some(Stack { kind: 1, levels: 1 });
        let ground = Some(Stack { kind: 2, levels: 0 });
        assert!(merges(a, a, |_| true));
        assert!(!merges(a, b, |_| true), "a taller neighbour is its own building");
        assert!(!merges(a, c, |_| true), "another kind does not merge");
        assert!(!merges(a, None, |_| true));
        assert!(!merges(ground, ground, |_| true), "ground kinds have no walls to share");
        assert!(one_plot(ground, ground, |_| true), "but they are still one plot, with an extent and an axis");
        assert!(!merges(a, a, |k| k != 0), "a kind that does not merge keeps its walls");
    }

    #[test]
    fn door_goes_toward_a_road_else_round_the_open_faces_by_seed() {
        let all = 0b1111;
        assert_eq!(door_face(all, 1 << FACE_NX, 0), FACE_NX, "a road wins whatever the seed");
        assert_eq!(door_face(all, 0, 0), FACE_PY);
        assert_eq!(door_face(all, 0, 1), FACE_PX);
        assert_eq!(door_face(all, 0, 2), FACE_NY);
        assert_eq!(door_face(all, 0, 3), FACE_NX);
        assert_eq!(door_face(1 << FACE_NX, 0, 0), FACE_NX, "the only open face");
        assert_eq!(door_face(0, 1 << FACE_PX, 5), NO_FACE, "a tile with no open face has no door");
        assert_eq!(door_face(1 << FACE_PY, 1 << FACE_PX, 1), FACE_PY, "a road across a shared face is not reachable");
    }

    /// A one-tile column at the origin with its eaves at 5.
    fn column(roof: Roof) -> Column {
        Column { mx: 0, my: 0, zs: 5.0, profile: Profile { roof, pitch: 1.0, max_rise: 3.0 }, runs: Runs::SINGLE }
    }

    #[test]
    fn a_ray_over_the_footprint_meets_the_roof_and_one_coming_in_low_meets_a_wall() {
        let col = column(Roof::Gable);
        // Straight down onto the ridge: the path does not move with z.
        let top = col.hit((0.5, 0.5), (0.0, 0.0), 0.0, 10.0, 12).expect("a hit on the ridge");
        assert_eq!(top.face, NO_FACE);
        assert!((top.z - 6.0).abs() < 0.02, "ridge at eaves plus one: {}", top.z);
        assert!(top.normal.2 > 0.0 && top.normal.0.abs() < 1e-3, "on the ridge line the normal is nearly up: {:?}", top.normal);
        // Down onto the slope near the eave.
        let slope = col.hit((0.5, 0.1), (0.0, 0.0), 0.0, 10.0, 12).expect("a hit on the slope");
        assert!((slope.z - 5.2).abs() < 0.02, "{}", slope.z);
        assert!(slope.normal.1 < 0.0, "the near slope faces -y: {:?}", slope.normal);
        // A path crossing the footprint below the eaves: p(z) = (-1 + z/2,
        // 0.1) is inside for z in [2, 4], so it enters the +x face at z = 4
        // under the roof at 5.2: a wall.
        let d = (0.5, 0.0);
        let wall = col.hit((-1.0, 0.1), d, 0.0, 10.0, 6).expect("a wall hit");
        assert_eq!(wall.face, FACE_PX);
        assert!((wall.z - 4.0).abs() < 1e-5, "the wall is met where the path enters the footprint: {}", wall.z);
        assert_eq!(wall.normal, (1.0, 0.0, 0.0));
        // The same path shifted so it comes in at z = 6, above the roof,
        // crosses the slope inside the footprint instead.
        let roof = col.hit((-2.0, 0.1), d, 0.0, 12.0, 12).expect("a roof hit");
        assert_eq!(roof.face, NO_FACE);
        assert!((roof.z - 5.2).abs() < 0.01, "{}", roof.z);
        // Beside the footprint there is nothing.
        assert!(col.hit((3.5, 0.5), (0.0, 0.0), 0.0, 10.0, 6).is_none());
        // A segment entirely above the roof misses.
        assert!(col.hit((0.5, 0.5), (0.0, 0.0), 7.0, 10.0, 6).is_none());
        // A flat roof is met exactly at the eaves.
        let flat = column(Roof::Flat).hit((0.5, 0.5), (0.0, 0.0), 0.0, 10.0, 12).unwrap();
        assert!((flat.z - 5.0).abs() < 0.02 && flat.face == NO_FACE);
    }
}
