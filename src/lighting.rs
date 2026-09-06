//! Deferred lighting: every cell is lit from ambient sky light, the sun
//! shadowed by drifting clouds and by the cast shadow mask, and the point
//! lights, then written to the canvas. Lit windows join the frame's lights
//! here from the stacks in view.

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

/// Summed point light at a world position, in metres: positions are tiles
/// and heights metres, and a light's radius is metres. Height counts half
/// as much as distance along the ground; flickering lights pulse on their
/// own phase, dipping by `flicker_amount` at `flicker_omega` radians per
/// second.
fn point_light_at(wx: f32, wy: f32, wz: f32, lights: &[&Light], t: f32) -> [f32; 3] {
    let mut pl = [0.0f32; 3];
    for (li, light) in lights.iter().enumerate() {
        let dx = (wx - light.mx as f32) * crate::map::TILE_METRES;
        let dy = (wy - light.my as f32) * crate::map::TILE_METRES;
        let dz = (wz - light.z as f32) * 0.5;
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d >= light.radius {
            continue;
        }
        let k = 1.0 - d / light.radius;
        let fall = if light.falloff == 2.0 { k * k } else { k.powf(light.falloff) };
        let mut f = fall * light.intensity;
        let a = light.flicker_amount;
        if a > 0.0 {
            f *= (1.0 - a) + a * (t * light.flicker_omega + li as f32 * 1.7).sin() * (t * 5.3).cos().abs();
        }
        pl = [pl[0] + light.color[0] * f, pl[1] + light.color[1] * f, pl[2] + light.color[2] * f];
    }
    pl
}

/// Soft knee on summed point light so clustered lights saturate instead of
/// blowing out: rises like `v` near zero and never exceeds 1.6.
pub(crate) fn knee(v: f32) -> f32 {
    1.6 * (1.0 - (-v / 1.6).exp())
}

impl Renderer {
    /// Lit windows: one light per stack in view whose kind names one, at
    /// night, scaled by how dark the sky is.
    pub(crate) fn stack_lights(&mut self, sc: &Scene) {
        let (assets, world, cam) = (sc.assets, sc.world, sc.cam);
        let night = 1.0 - world.skylight();
        if night <= 0.05 {
            return;
        }
        let grid = self.heights.as_ref().expect("height grid built for the frame");
        let margin = 20.0;
        for (mx, my, _, g) in grid.cells() {
            let Some(st) = g.stack else { continue };
            let b = &assets.blocks[st.kind as usize % assets.blocks.len()];
            let Some(li) = b.light else { continue };
            if st.levels == 0 {
                continue;
            }
            let (sx, sy) = cam.project(mx as f32 + 0.5, my as f32 + 0.5, g.zs);
            if sx < -margin || sx > self.w as f32 + margin || sy < -margin || sy > self.h as f32 + margin {
                continue;
            }
            self.frame_lights.push(assets.lights[li].at(mx, my, (g.base + 1.0).round() as i32, night));
        }
    }

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
                    let cast = self.shadow.as_ref().map(|m| m.at_surface(g.wx, g.wy, g.wz)).unwrap_or(0.0);
                    let s = fk * (1.0 - 0.72 * shadow) * (1.0 - 0.6 * cast);
                    l = [l[0] + sun[0] * s, l[1] + sun[1] * s, l[2] + sun[2] * s];
                }
                let pl = point_light_at(g.wx, g.wy, g.wz, &lights, t);
                l = [l[0] + knee(pl[0]), l[1] + knee(pl[1]), l[2] + knee(pl[2])];
                cv.put(x, y, g.ch, mul(g.glyph, l), mul(g.albedo, l));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    fn knee_is_monotonic_and_bounded() {
        assert_eq!(knee(0.0), 0.0);
        let mut last = 0.0;
        for i in 1..=400 {
            let v = i as f32 * 0.05;
            let k = knee(v);
            assert!(k > last, "knee falls between {} and {v}", v - 0.05);
            assert!(k < 1.6, "knee({v}) = {k} exceeds the bound");
            last = k;
        }
        assert!((knee(0.01) - 0.01).abs() < 1e-3, "near zero the knee is the identity");
        assert!(knee(20.0) > 1.59, "far out it saturates at the bound");
    }

    #[test]
    fn point_light_gives_its_intensity_at_zero_and_nothing_at_its_radius() {
        // The radius is metres and positions are tiles of 2 m (ADR-004), so
        // a radius of 8 m reaches four tiles.
        let l = Light { mx: 2, my: 3, z: 1, color: [1.0, 0.5, 0.0], radius: 8.0, intensity: 1.8, falloff: 2.0, flicker_amount: 0.0, flicker_omega: 0.0 };
        let lights = [&l];
        let at = |dx: f32| point_light_at(2.0 + dx, 3.0, 1.0, &lights, 0.0);
        assert_eq!(at(0.0), [1.8, 0.9, 0.0]);
        assert_eq!(at(4.0), [0.0, 0.0, 0.0]);
        assert_eq!(at(4.5), [0.0, 0.0, 0.0]);
        let (near, mid, edge) = (at(0.5), at(2.0), at(3.9));
        assert!(near[0] > mid[0] && mid[0] > edge[0] && edge[0] > 0.0, "it falls off through the radius");
        assert_eq!(near[2], 0.0, "a light with no blue adds no blue");
        // Height counts half as much as ground distance: four metres up is
        // one tile along.
        let (above, beside) = (point_light_at(2.0, 3.0, 1.0 + 4.0, &lights, 0.0), point_light_at(2.0 + 1.0, 3.0, 1.0, &lights, 0.0));
        assert_eq!(above, beside);
    }

    #[test]
    fn ambient_at_midnight_is_the_night_floor() {
        let mut w = World::new(1);
        w.tod = 0.0;
        assert_eq!(w.ambient(), [0.15, 0.17, 0.32]);
        w.weather.cover = 1.0;
        assert_eq!(w.ambient(), [0.15, 0.17, 0.32], "cloud cover cannot take the night any lower");
        assert_eq!(w.sun(), [0.0, 0.0, 0.0], "and there is no sun");
        w.tod = 12.0;
        let noon = w.ambient();
        assert!(noon.iter().zip([0.15, 0.17, 0.32]).all(|(d, n)| *d > n), "noon is brighter than the floor: {noon:?}");
    }
}
