//! Time of day, season, weather, and point lights.

use crate::canvas::Rgb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weather {
    Clear,
    Rain,
    Snow,
}

impl Weather {
    pub fn name(self) -> &'static str {
        match self {
            Weather::Clear => "clear",
            Weather::Rain => "rain",
            Weather::Snow => "snow",
        }
    }
}

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

/// A creature standing on a tile; drawn with the player sprite tier.
#[derive(Clone, Copy, Debug)]
pub struct Entity {
    pub mx: i32,
    pub my: i32,
}

pub struct World {
    /// Continuous season in `[0, 4)`: spring, summer, autumn, winter.
    pub season: f32,
    /// Hour of day in `[0, 24)`.
    pub tod: f32,
    pub auto_time: bool,
    pub weather: Weather,
    pub lights: Vec<Light>,
    pub entities: Vec<Entity>,
}

impl World {
    pub fn new() -> World {
        World { season: 1.0, tod: 13.0, auto_time: true, weather: Weather::Clear, lights: Vec::new(), entities: Vec::new() }
    }

    /// Advance the clock; a full day takes `day_secs` seconds of real time.
    pub fn tick(&mut self, dt: f32, day_secs: f32) {
        if self.auto_time {
            self.tod = (self.tod + dt * 24.0 / day_secs).rem_euclid(24.0);
        }
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
        crate::noise::smoothstep(-0.18, 0.35, self.elevation())
    }

    /// Direct sunlight colour and strength, warm near the horizon.
    pub fn sun(&self) -> [f32; 3] {
        let d = self.daylight();
        let warm = [1.0, 0.55, 0.30];
        let noon = [1.0, 0.97, 0.90];
        let k = d.powf(0.5);
        let dim = match self.weather {
            Weather::Clear => 1.0,
            Weather::Rain => 0.35,
            Weather::Snow => 0.5,
        };
        let s = d * dim;
        [
            (warm[0] + (noon[0] - warm[0]) * k) * s,
            (warm[1] + (noon[1] - warm[1]) * k) * s,
            (warm[2] + (noon[2] - warm[2]) * k) * s,
        ]
    }

    /// Sky-dome ambient light.
    pub fn ambient(&self) -> [f32; 3] {
        let d = self.skylight();
        let night = [0.15, 0.17, 0.32];
        let day = match self.weather {
            Weather::Clear => [0.42, 0.44, 0.48],
            Weather::Rain => [0.40, 0.42, 0.46],
            Weather::Snow => [0.48, 0.49, 0.52],
        };
        [
            night[0] + (day[0] - night[0]) * d,
            night[1] + (day[1] - night[1]) * d,
            night[2] + (day[2] - night[2]) * d,
        ]
    }

    /// How rough open water is, 0 glassy to 1 whitecaps.
    pub fn choppiness(&self, t: f32) -> f32 {
        let base = match self.weather {
            Weather::Clear => 0.4,
            Weather::Rain => 1.0,
            Weather::Snow => 0.3,
        };
        (base * (0.85 + 0.15 * (t * 0.23).sin())).clamp(0.0, 1.0)
    }

    /// Cloud density threshold above which a shadow is cast.
    pub fn cloud_threshold(&self) -> f32 {
        match self.weather {
            Weather::Clear => 0.56,
            Weather::Rain => 0.30,
            Weather::Snow => 0.36,
        }
    }

    pub fn sky(&self) -> Rgb {
        Rgb(10, 12, 22).lerp(Rgb(34, 44, 72), self.skylight())
    }

    pub fn add_campfire(&mut self, mx: i32, my: i32, z: i32) {
        self.lights.push(Light { mx, my, z, color: [1.0, 0.62, 0.22], radius: 7.5, intensity: 2.2, flicker: true });
    }
}
