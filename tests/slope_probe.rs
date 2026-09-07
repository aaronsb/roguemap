use roguemap::assets::Assets;
use roguemap::map::{Map, TILE_METRES};
use std::rc::Rc;

/// The generated land's slope distribution, in metres of rise per metre of
/// run. Earth's median land slope is 2 to 3 degrees, an Alpine hillside 25
/// to 30, and loose material stops holding above about 34. A world whose
/// median is steeper than that is cliffs everywhere.
#[test]
fn slope_distribution() {
    let assets = Rc::new(Assets::load().expect("assets"));
    let map = Map::new(4096, 4096, 7, assets);
    let mut slopes: Vec<f32> = Vec::new();
    let mut hmax: f32 = 0.0;
    // Sample gradients on a coarse lattice over a wide area, in metres per metre.
    for y in (-1000..1000).step_by(7) {
        for x in (-1000..1000).step_by(7) {
            let h = map.ground_at(x as f32, y as f32);
            let hx = map.ground_at(x as f32 + 1.0, y as f32);
            let hy = map.ground_at(x as f32, y as f32 + 1.0);
            hmax = hmax.max(h);
            if h > 0.5 {
                let g = (((hx - h) / TILE_METRES).powi(2) + ((hy - h) / TILE_METRES).powi(2)).sqrt();
                slopes.push(g);
            }
        }
    }
    slopes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f32| slopes[((slopes.len() - 1) as f32 * p) as usize];
    let deg = |g: f32| g.atan().to_degrees();
    println!("land samples {}  max height {hmax:.0} m", slopes.len());
    for p in [0.5f32, 0.75, 0.9, 0.99, 1.0] {
        let g = q(p);
        println!("  p{:>3.0}  slope {g:6.2}  = {:5.1} deg", p * 100.0, deg(g));
    }
}
