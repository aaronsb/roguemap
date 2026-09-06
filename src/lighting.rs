//! Deferred lighting: every cell is lit from ambient sky light, the sun
//! shadowed by drifting clouds, and the point lights, then written to the
//! canvas.

use crate::canvas::{Canvas, Rgb};
use crate::render::{Renderer, Scene};
use crate::world::Light;

fn mul(c: Rgb, l: [f32; 3]) -> Rgb {
    Rgb(
        (c.0 as f32 * l[0]).clamp(0.0, 255.0) as u8,
        (c.1 as f32 * l[1]).clamp(0.0, 255.0) as u8,
        (c.2 as f32 * l[2]).clamp(0.0, 255.0) as u8,
    )
}

/// Summed point light at a world position. Height counts half as much as
/// distance along the ground; flickering lights pulse on their own phase.
fn point_light_at(wx: f32, wy: f32, wz: f32, lights: &[&Light], t: f32) -> [f32; 3] {
    let mut pl = [0.0f32; 3];
    for (li, light) in lights.iter().enumerate() {
        let dx = wx - light.mx as f32;
        let dy = wy - light.my as f32;
        let dz = (wz - light.z as f32) * 0.5;
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d >= light.radius {
            continue;
        }
        let mut f = (1.0 - d / light.radius).powi(2) * light.intensity;
        if light.flicker {
            f *= 0.78 + 0.22 * (t * 11.0 + li as f32 * 1.7).sin() * (t * 5.3).cos().abs();
        }
        pl = [pl[0] + light.color[0] * f, pl[1] + light.color[1] * f, pl[2] + light.color[2] * f];
    }
    pl
}

impl Renderer {
    pub(crate) fn light_pass(&self, cv: &mut Canvas, sc: &Scene) {
        let (world, t) = (sc.world, sc.t);
        let amb = world.ambient();
        let sun = world.sun();
        let sunny = sun[0] + sun[1] + sun[2] > 0.01;
        let face_k = [1.0f32, 0.78, 0.5];
        let lights: Vec<&Light> = world.lights.iter().chain(self.frame_lights.iter()).collect();
        for y in 0..self.h {
            for x in 0..self.w {
                let g = self.g[(y * self.w + x) as usize];
                if !g.lit {
                    cv.put(x, y, g.ch, g.glyph, g.albedo);
                    continue;
                }
                let mut l = amb;
                let fk = face_k[g.face as usize];
                let amb_face = 0.75 + 0.25 * fk;
                l = [l[0] * amb_face, l[1] * amb_face, l[2] * amb_face];
                if sunny {
                    let shadow = world.cloud_shadow(g.wx, g.wy);
                    let s = fk * (1.0 - 0.72 * shadow);
                    l = [l[0] + sun[0] * s, l[1] + sun[1] * s, l[2] + sun[2] * s];
                }
                let pl = point_light_at(g.wx, g.wy, g.wz, &lights, t);
                // Soft knee so clustered lights saturate instead of blowing out.
                let knee = |v: f32| 1.6 * (1.0 - (-v / 1.6).exp());
                l = [l[0] + knee(pl[0]), l[1] + knee(pl[1]), l[2] + knee(pl[2])];
                cv.put(x, y, g.ch, mul(g.glyph, l), mul(g.albedo, l));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_light_falls_off_to_its_radius() {
        let l = Light { mx: 0, my: 0, z: 0, color: [1.0, 0.5, 0.0], radius: 4.0, intensity: 1.0, flicker: false };
        let lights = [&l];
        let near = point_light_at(0.0, 0.0, 0.0, &lights, 0.0);
        let mid = point_light_at(2.0, 0.0, 0.0, &lights, 0.0);
        let out = point_light_at(4.0, 0.0, 0.0, &lights, 0.0);
        assert!(near[0] > mid[0] && mid[0] > 0.0);
        assert_eq!(out, [0.0, 0.0, 0.0]);
        assert_eq!(near[2], 0.0);
    }
}
