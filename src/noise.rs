//! Hashing and value noise shared by map generation, clouds, and effects.

/// 64-bit integer hash of a coordinate pair and seed.
pub fn hash(x: i64, y: i64, seed: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
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

/// Hash mapped to `[0, 1)`.
pub fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    (hash(x, y, seed) >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear value noise in `[0, 1)`.
pub fn value(x: f32, y: f32, seed: u64) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = smooth(x - xi);
    let fy = smooth(y - yi);
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
