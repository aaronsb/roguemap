//! Hashing and value noise shared by map generation, clouds, and effects.

/// 64-bit integer hash of a coordinate pair and seed.
pub fn hash(x: i64, y: i64, seed: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    h
}

/// Floor of a float within the `i32` range as an integer, without the
/// library call.
#[inline]
pub fn ifloor(x: f32) -> i32 {
    let i = x as i32;
    i - (x < i as f32) as i32
}

/// Ceiling of a float within the `i32` range as an integer, without the
/// library call.
#[inline]
pub fn iceil(x: f32) -> i32 {
    let i = x as i32;
    i + (x > i as f32) as i32
}

/// Hash mapped to `[0, 1)`.
pub fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    (hash(x, y, seed) >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear value noise in `[0, 1)`.
pub fn value(x: f32, y: f32, seed: u64) -> f32 {
    let (xi, yi) = (ifloor(x), ifloor(y));
    let fx = smooth(x - xi as f32);
    let fy = smooth(y - yi as f32);
    let (xi, yi) = (xi as i64, yi as i64);
    let a = hash01(xi, yi, seed);
    let b = hash01(xi + 1, yi, seed);
    let c = hash01(xi, yi + 1, seed);
    let d = hash01(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    top + (bot - top) * fy
}

/// Fractal Brownian motion over `value`, normalised to `[0, 1)`.
pub fn fbm(x: f32, y: f32, seed: u64, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut freq = 1.0;
    for o in 0..octaves {
        sum += amp * value(x * freq, y * freq, seed.wrapping_add(o as u64 * 7919));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// One octave's lattice of `hash01` values over a rectangle.
struct Layer {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    v: Vec<f32>,
}

/// `fbm` over a rectangle with its lattice hashed once: a field the walk
/// samples a hundred thousand times a frame interpolates from memory and
/// comes out the same to the bit, since the hashes and the arithmetic on
/// them are the ones `fbm` would do. A point outside the rectangle is
/// hashed as `fbm` hashes it.
pub struct FbmCache {
    seed: u64,
    layers: Vec<Layer>,
}

impl FbmCache {
    /// The lattices for `fbm(x, y, seed, octaves)` over `x0..=x1` by
    /// `y0..=y1` in noise units.
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32, seed: u64, octaves: u32) -> FbmCache {
        let mut layers = Vec::with_capacity(octaves as usize);
        let mut freq = 1.0f32;
        for o in 0..octaves {
            let lseed = seed.wrapping_add(o as u64 * 7919);
            let (lx0, ly0) = (ifloor(x0 * freq) as i64 - 1, ifloor(y0 * freq) as i64 - 1);
            let (lx1, ly1) = (ifloor(x1 * freq) as i64 + 2, ifloor(y1 * freq) as i64 + 2);
            let (w, h) = ((lx1 - lx0 + 1).max(0) as usize, (ly1 - ly0 + 1).max(0) as usize);
            let mut v = Vec::with_capacity(w * h);
            for yi in ly0..=ly1 {
                for xi in lx0..=lx1 {
                    v.push(hash01(xi, yi, lseed));
                }
            }
            layers.push(Layer { x0: lx0, y0: ly0, w, h, v });
            freq *= 2.0;
        }
        FbmCache { seed, layers }
    }

    /// What `fbm(x, y, seed, octaves)` returns.
    #[inline]
    pub fn fbm(&self, x: f32, y: f32) -> f32 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut norm = 0.0;
        let mut freq = 1.0;
        for (o, layer) in self.layers.iter().enumerate() {
            let (xs, ys) = (x * freq, y * freq);
            let (xi, yi) = (ifloor(xs), ifloor(ys));
            let fx = smooth(xs - xi as f32);
            let fy = smooth(ys - yi as f32);
            let (lx, ly) = (xi as i64 - layer.x0, yi as i64 - layer.y0);
            let (a, b, c, d) = if lx >= 0 && ly >= 0 && (lx as usize) + 1 < layer.w && (ly as usize) + 1 < layer.h {
                let i = ly as usize * layer.w + lx as usize;
                (layer.v[i], layer.v[i + 1], layer.v[i + layer.w], layer.v[i + layer.w + 1])
            } else {
                self.corners(o, xi, yi)
            };
            let top = a + (b - a) * fx;
            let bot = c + (d - c) * fx;
            sum += amp * (top + (bot - top) * fy);
            norm += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        sum / norm
    }

    /// The four corner hashes of octave `o` around a lattice point outside
    /// the cached rectangle, as `value` would hash them. Kept out of line
    /// so the common path stays small enough to inline.
    #[cold]
    #[inline(never)]
    fn corners(&self, o: usize, xi: i32, yi: i32) -> (f32, f32, f32, f32) {
        let lseed = self.seed.wrapping_add(o as u64 * 7919);
        let (xi, yi) = (xi as i64, yi as i64);
        (hash01(xi, yi, lseed), hash01(xi + 1, yi, lseed), hash01(xi, yi + 1, lseed), hash01(xi + 1, yi + 1, lseed))
    }
}

/// Hermite step from 0 at `e0` to 1 at `e1`.
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    smooth(((x - e0) / (e1 - e0)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_stays_in_unit_range() {
        for i in 0..2000 {
            let x = i as f32 * 0.37 - 300.0;
            let v = fbm(x, x * 0.61, 11, 4);
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn iceil_agrees_with_ceil() {
        for i in -400..400 {
            let x = i as f32 * 0.37;
            assert_eq!(iceil(x), x.ceil() as i32, "{x}");
        }
        assert_eq!(iceil(2.0), 2);
        assert_eq!(iceil(-0.5), 0);
        assert_eq!(iceil(0.001), 1);
    }

    #[test]
    fn cached_fbm_is_the_hashed_fbm_to_the_bit() {
        let cache = FbmCache::new(-3.0, 2.0, 40.0, 50.0, 0xD7, 3);
        for i in 0..3000 {
            // Inside the rectangle and well outside it.
            let (x, y) = (-10.0 + i as f32 * 0.021, 1.0 + (i as f32 * 0.037).sin() * 60.0);
            assert_eq!(cache.fbm(x, y).to_bits(), fbm(x, y, 0xD7, 3).to_bits(), "at ({x}, {y})");
        }
    }

    #[test]
    fn ifloor_agrees_with_floor() {
        for i in -400..400 {
            let x = i as f32 * 0.37;
            assert_eq!(ifloor(x), x.floor() as i32, "{x}");
        }
        assert_eq!(ifloor(-0.0), 0);
        assert_eq!(ifloor(-1.0), -1);
        assert_eq!(ifloor(2.999), 2);
    }

    #[test]
    fn smoothstep_clamps_both_ways() {
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(smoothstep(1.0, 0.0, 1.0), 0.0);
        assert_eq!(smoothstep(1.0, 0.0, 0.0), 1.0);
    }
}
