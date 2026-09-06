//! Time of day, season, weather, precipitation accumulation, lights and
//! entities.
//!
//! Weather is a small state: cloud cover, wind speed and direction, and
//! precipitation intensity. Left on automatic it drifts along slow noise on
//! the day clock. Snowpack and ground wetness accumulate per temperature
//! band, so cold uplands hold snow after the valleys have melted.

use crate::biome::seasonal_temp;
use crate::canvas::Rgb;
use crate::map::{Map, Terrain};
use crate::noise::{fbm, smoothstep, value};

/// How a kind of light glows; a `Light` is one placed in the world.
#[derive(Clone, Copy, Debug)]
pub struct LightSpec {
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
    pub flicker: bool,
}

impl LightSpec {
    /// Place this light at a tile, with its colour scaled by `strength`.
    pub fn at(&self, mx: i32, my: i32, z: i32, strength: f32) -> Light {
        Light {
            mx,
            my,
            z,
            color: [self.color[0] * strength, self.color[1] * strength, self.color[2] * strength],
            radius: self.radius,
            intensity: self.intensity,
            flicker: self.flicker,
        }
    }
}

pub const CAMPFIRE: LightSpec = LightSpec { color: [1.0, 0.62, 0.22], radius: 7.5, intensity: 2.2, flicker: true };
/// A lit window; its strength follows how dark the sky is.
pub const WINDOW: LightSpec = LightSpec { color: [1.0, 0.75, 0.4], radius: 4.0, intensity: 0.8, flicker: false };

/// A point light in map coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub mx: i32,
    pub my: i32,
    pub z: i32,
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
    pub flicker: bool,
}

/// The creature kind the player is; the creature table comes with the asset
/// pass.
pub const PLAYER: u8 = 0;

/// A creature standing on a tile; drawn with the player sprite tier.
#[derive(Clone, Copy, Debug)]
pub struct Entity {
    /// Index into the creature table the asset pass brings; the player is 0.
    #[allow(dead_code)]
    pub kind: u8,
    pub mx: i32,
    pub my: i32,
}

/// Current sky and wind, all in `[0, 1]` except direction in radians.
#[derive(Clone, Copy, Debug)]
pub struct Weather {
    pub cover: f32,
    pub wind: f32,
    pub wind_dir: f32,
    pub precip: f32,
}

/// Named weather presets: cover, precipitation.
pub const WEATHER_PRESETS: [(f32, f32); 4] = [(0.15, 0.0), (0.75, 0.0), (0.9, 0.5), (1.0, 1.0)];
pub const STORM: usize = 3;
/// Named wind presets.
pub const WIND_PRESETS: [f32; 4] = [0.05, 0.2, 0.55, 1.0];
/// Day lengths in seconds.
pub const DAY_LENGTHS: [f32; 4] = [120.0, 600.0, 3600.0, 86400.0];

const BANDS: usize = 71;
const BAND_MIN: f32 = -40.0;

pub struct World {
    /// Continuous season in `[0, 4)`: spring, summer, autumn, winter.
    pub season: f32,
    /// Hour of day in `[0, 24)`.
    pub tod: f32,
    pub auto_time: bool,
    pub day_secs: f32,
    /// Elapsed simulated days, the clock the weather noise runs on.
    pub days: f32,
    pub weather: Weather,
    /// `None` for automatic, else a preset index.
    pub weather_preset: Option<usize>,
    pub wind_preset: Option<usize>,
    /// Accumulated cloud drift in tiles.
    pub cloud_offset: (f32, f32),
    snowpack: [f32; BANDS],
    wetness: [f32; BANDS],
    pub lights: Vec<Light>,
    pub entities: Vec<Entity>,
    seed: u64,
}

impl World {
    /// Cloud base altitude in height units (rows at 1x zoom).
    pub const CLOUD_ALTITUDE: f32 = 10.0;

    /// Ground offset in tiles from a cloud to its shadow: the sun's azimuth
    /// times the altitude over the tangent of its elevation, capped.
    pub fn shadow_shift(&self) -> (f32, f32) {
        let elev = self.elevation().max(0.08);
        let len = (Self::CLOUD_ALTITUDE * 0.5 * (1.0 - elev * elev).sqrt() / elev).min(18.0);
        // Sun to the south-east in map space; shadows fall north-west.
        (-len * 0.7, -len * 0.7)
    }

    pub fn new(seed: u64) -> World {
        World {
            season: 1.0,
            tod: 13.0,
            auto_time: true,
            day_secs: 600.0,
            days: 0.0,
            weather: Weather { cover: 0.3, wind: 0.2, wind_dir: 0.6, precip: 0.0 },
            weather_preset: None,
            wind_preset: None,
            cloud_offset: (0.0, 0.0),
            snowpack: [0.0; BANDS],
            wetness: [0.0; BANDS],
            lights: Vec::new(),
            entities: Vec::new(),
            seed,
        }
    }

    /// Advance the clock, weather and accumulators by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        if !self.auto_time {
            return;
        }
        let ddays = dt / self.day_secs;
        self.tick_clock(ddays);
        self.tick_weather(ddays, dt);
        self.tick_accumulation(ddays);
    }

    fn tick_clock(&mut self, ddays: f32) {
        self.tod = (self.tod + ddays * 24.0).rem_euclid(24.0);
        self.days += ddays;
    }

    /// Move the sky toward its targets: presets when set, else slow noise
    /// on the day clock; then drift the clouds with the wind.
    fn tick_weather(&mut self, ddays: f32, dt: f32) {
        let n = |k: f32, off: f32| value(self.days * k + off, 0.5, self.seed);
        let (cover_t, precip_t) = match self.weather_preset {
            Some(i) => WEATHER_PRESETS[i],
            None => {
                let c = smoothstep(0.25, 0.8, n(1.4, 3.0));
                let moist = n(0.9, 40.0);
                (c, smoothstep(0.7, 0.95, c) * smoothstep(0.35, 0.7, moist))
            }
        };
        let wind_t = match self.wind_preset {
            Some(i) => WIND_PRESETS[i],
            None => (n(2.1, 90.0) * 0.5 + precip_t * 0.5).clamp(0.02, 1.0),
        };
        // Approach targets over a few hours of simulated time.
        let k = (ddays * 8.0).min(1.0);
        self.weather.cover += (cover_t - self.weather.cover) * k;
        self.weather.precip += (precip_t - self.weather.precip) * k;
        self.weather.wind += (wind_t - self.weather.wind) * k;
        self.weather.wind_dir += (n(0.3, 200.0) - 0.5) * ddays * 2.0;

        // Clouds drift with the wind: about 0.6 tiles per second in a gale.
        let speed = 0.6 * self.weather.wind * self.weather.wind;
        self.cloud_offset.0 += self.weather.wind_dir.cos() * speed * dt;
        self.cloud_offset.1 += self.weather.wind_dir.sin() * speed * dt;
    }

    /// Snow and rain accumulate by temperature band, in day units, and melt
    /// or dry with warmth and sun.
    fn tick_accumulation(&mut self, ddays: f32) {
        let daylight = self.daylight();
        let (precip, season) = (self.weather.precip, self.season);
        for (i, (snow, wet)) in self.snowpack.iter_mut().zip(self.wetness.iter_mut()).enumerate() {
            let t = seasonal_temp(BAND_MIN + i as f32, season);
            if precip > 0.0 {
                if t < 1.0 {
                    *snow = (*snow + precip * ddays * 3.0).min(1.5);
                } else {
                    *wet = (*wet + precip * ddays * 6.0).min(1.0);
                }
            }
            let melt = (t - 0.5).max(0.0) * 0.06 * ddays * (1.0 + daylight);
            *snow = (*snow - melt).max(0.0);
            let dry = (t + 5.0).max(0.5) * 0.08 * ddays * (0.5 + daylight);
            *wet = (*wet - dry).max(0.0);
        }
    }

    /// Step the clock by some hours, wrapping at midnight.
    pub fn step_hour(&mut self, hours: f32) {
        self.tod = (self.tod + hours).rem_euclid(24.0);
    }

    /// Step the season by a fraction of a year.
    pub fn step_season(&mut self, quarters: f32) {
        self.season += quarters;
    }

    fn band(temp: f32) -> usize {
        ((temp - BAND_MIN).round().clamp(0.0, BANDS as f32 - 1.0)) as usize
    }

    /// Snow cover fraction at a tile of the given annual temperature:
    /// permanent snow where it is very cold, plus the accumulated pack.
    pub fn snow_at(&self, temp: f32) -> f32 {
        let t = seasonal_temp(temp, self.season);
        let permanent = smoothstep(-2.0, -9.0, t);
        (permanent + self.snowpack[Self::band(temp)]).clamp(0.0, 1.0)
    }

    pub fn wet_at(&self, temp: f32) -> f32 {
        self.wetness[Self::band(temp)]
    }

    /// Whether precipitation falls as snow at this temperature now.
    pub fn snowing_at(&self, temp: f32) -> bool {
        seasonal_temp(temp, self.season) < 1.0
    }

    /// Signed sun elevation in `[-1, 1]`, positive between 06:00 and 18:00.
    pub fn elevation(&self) -> f32 {
        ((self.tod - 6.0) / 12.0 * std::f32::consts::PI).sin()
    }

    /// Direct sun strength in `[0, 1]`; zero from dusk to dawn.
    pub fn daylight(&self) -> f32 {
        self.elevation().clamp(0.0, 1.0).powf(0.6)
    }

    /// Sky brightness in `[0, 1]`, with a twilight band either side of sunset.
    pub fn skylight(&self) -> f32 {
        smoothstep(-0.18, 0.35, self.elevation())
    }

    /// Direct sunlight colour and strength, warm near the horizon and dimmed
    /// by cloud cover.
    pub fn sun(&self) -> [f32; 3] {
        let d = self.daylight();
        let warm = [1.0, 0.55, 0.30];
        let noon = [1.0, 0.97, 0.90];
        let k = d.powf(0.5);
        let dim = 1.0 - 0.75 * self.weather.cover * self.weather.cover;
        let s = d * dim;
        [
            (warm[0] + (noon[0] - warm[0]) * k) * s,
            (warm[1] + (noon[1] - warm[1]) * k) * s,
            (warm[2] + (noon[2] - warm[2]) * k) * s,
        ]
    }

    /// Sky-dome ambient light; overcast days are flatter and a touch cooler.
    pub fn ambient(&self) -> [f32; 3] {
        let d = self.skylight();
        let night = [0.15, 0.17, 0.32];
        let clear = [0.42, 0.44, 0.48];
        let overcast = [0.40, 0.41, 0.44];
        let c = self.weather.cover;
        let day = [clear[0] + (overcast[0] - clear[0]) * c, clear[1] + (overcast[1] - clear[1]) * c, clear[2] + (overcast[2] - clear[2]) * c];
        [
            night[0] + (day[0] - night[0]) * d,
            night[1] + (day[1] - night[1]) * d,
            night[2] + (day[2] - night[2]) * d,
        ]
    }

    /// How rough open water is, 0 glassy to 1 whitecaps.
    pub fn choppiness(&self) -> f32 {
        (self.weather.wind * 0.9 + self.weather.precip * 0.2).clamp(0.0, 1.0)
    }

    /// Cloud density threshold above which a shadow is cast; high cover
    /// lowers it until most of the ground is shaded.
    pub fn cloud_threshold(&self) -> f32 {
        1.05 - self.weather.cover * 0.85
    }

    /// The cloud field at a point already offset by the drift.
    fn cloud_field(x: f32, y: f32) -> f32 {
        fbm(x * 0.07, y * 0.07, 0xC10D, 3)
    }

    /// Cloud density in `[0, 1)` over a map point, after the drift so far.
    pub fn cloud_density(&self, x: f32, y: f32) -> f32 {
        let (ox, oy) = self.cloud_offset;
        Self::cloud_field(x + ox, y + oy)
    }

    /// How much of the sun a ground point loses to the cloud whose shadow
    /// falls on it, 0 clear to 1 fully shaded.
    pub fn cloud_shadow(&self, x: f32, y: f32) -> f32 {
        let (ox, oy) = self.cloud_offset;
        let (sx, sy) = self.shadow_shift();
        let th = self.cloud_threshold();
        smoothstep(th, th + 0.10, Self::cloud_field(x + ox + sx, y + oy + sy))
    }

    /// Local gust strength at a tile, 0 still to 1 full sway. Calm air gives
    /// an occasional stir in one place; a gale moves everything.
    pub fn gust(&self, mx: f32, my: f32, t: f32) -> f32 {
        let w = self.weather.wind;
        let (dx, dy) = (self.weather.wind_dir.cos(), self.weather.wind_dir.sin());
        let field = value(mx * 0.06 - dx * t * (0.2 + w), my * 0.06 - dy * t * (0.2 + w), self.seed ^ 0x6057);
        let threshold = 0.95 - w * 0.9;
        smoothstep(threshold, threshold + 0.25, field) * (0.3 + 0.7 * w)
    }

    pub fn sky(&self) -> Rgb {
        let clear = Rgb(34, 44, 72);
        let overcast = Rgb(40, 44, 56);
        Rgb(10, 12, 22).lerp(clear.lerp(overcast, self.weather.cover), self.skylight())
    }

    /// Light a campfire on a tile unless it is water or off the map.
    /// Returns whether one was lit.
    pub fn light_campfire(&mut self, map: &Map, mx: i32, my: i32) -> bool {
        match map.get(mx, my) {
            Some(tile) if tile.terrain != Terrain::Water => {
                self.lights.push(CAMPFIRE.at(mx, my, tile.draw_z(), 1.0));
                true
            }
            _ => false,
        }
    }

    /// Put the player on the nearest land to a position.
    pub fn spawn_player(&mut self, map: &Map, mx: i32, my: i32) {
        let (mx, my) = map.nearest_land(mx, my);
        self.entities.push(Entity { kind: PLAYER, mx, my });
    }

    /// The player is the first entity, by convention.
    pub fn player(&self) -> Option<&Entity> {
        self.entities.first()
    }

    pub fn player_mut(&mut self) -> Option<&mut Entity> {
        self.entities.first_mut()
    }

    /// Move the player one step, refusing water and the map edge. Returns
    /// whether it moved.
    pub fn try_move(&mut self, map: &Map, dx: i32, dy: i32) -> bool {
        let Some(p) = self.player_mut() else { return false };
        let (nx, ny) = (p.mx + dx, p.my + dy);
        match map.get(nx, ny) {
            Some(t) if t.terrain != Terrain::Water => {
                p.mx = nx;
                p.my = ny;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_steps_wrap() {
        let mut w = World::new(1);
        w.tod = 23.5;
        w.step_hour(1.0);
        assert!((w.tod - 0.5).abs() < 1e-5);
        w.step_hour(-1.0);
        assert!((w.tod - 23.5).abs() < 1e-5);
    }

    #[test]
    fn player_refuses_water() {
        let map = Map::new(32, 32, 7);
        let mut w = World::new(7);
        w.spawn_player(&map, 16, 16);
        let p = *w.player().unwrap();
        assert_ne!(map.get(p.mx, p.my).unwrap().terrain, Terrain::Water);
        for _ in 0..200 {
            w.try_move(&map, 1, 0);
        }
        let p = *w.player().unwrap();
        assert!(map.get(p.mx, p.my).is_some_and(|t| t.terrain != Terrain::Water));
    }
}
