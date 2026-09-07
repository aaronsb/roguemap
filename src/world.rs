//! Time of day, season, weather, precipitation accumulation, lights and
//! entities.
//!
//! Weather is a small state: cloud cover, wind speed and direction, and
//! precipitation intensity. Left on automatic it drifts along slow noise on
//! the day clock. Snowpack and ground wetness accumulate per temperature
//! band, so cold uplands hold snow after the valleys have melted.

use crate::biome::seasonal_temp;
use crate::canvas::Rgb;
use crate::map::{Map, Terrain, TILE_CM};
use crate::noise::{fbm, smoothstep, value, FbmCache};
use crate::properties::Identity;

/// How a kind of light glows, a row of `lights.toml`; a `Light` is one
/// placed in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct LightSpec {
    pub name: String,
    pub identity: Identity,
    /// Colour as 0..1 floats.
    pub color: [f32; 3],
    /// Throw in metres.
    pub radius: f32,
    pub intensity: f32,
    /// Falloff exponent over the normalised distance.
    pub falloff: f32,
    /// Flicker depth in 0..1 and speed in hertz.
    pub flicker_amount: f32,
    pub flicker_rate: f32,
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
            falloff: self.falloff,
            flicker_amount: self.flicker_amount,
            flicker_omega: self.flicker_rate * std::f32::consts::TAU,
        }
    }
}

/// A point light in map coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub mx: i32,
    pub my: i32,
    pub z: i32,
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
    pub falloff: f32,
    pub flicker_amount: f32,
    /// Flicker speed in radians per second.
    pub flicker_omega: f32,
}

/// The creature kind the player is: the first row of `creatures.toml`.
pub const PLAYER: u8 = 0;

/// Which way a figure faces on screen (ADR-008). `Right` is the art as
/// drawn; `Left` is its mirror.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
}

/// A walk under way (ADR-008): a heading held on a short lease that each
/// press renews, at a multiple of the creature's speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walk {
    /// Unit heading in map space.
    pub dir: (f32, f32),
    /// Seconds left before the walk ends unless a press renews it.
    pub grace: f32,
    /// Speed multiplier: 1 walking, `World::RUN` running.
    pub factor: f32,
    /// The fraction of a centimetre a tick left over on each axis, owed
    /// to the next, so the distance walked is the speed times the time.
    carry: (f32, f32),
}

/// A creature standing at a point of the map, drawn with its kind's art.
/// Its position is centimetres (ADR-006); the tile it stands in is
/// derived, and collision is per tile. It faces left or right and may be
/// walking (ADR-008).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entity {
    /// Index into the creature table; the player is 0.
    pub kind: u8,
    pub x_cm: i32,
    pub y_cm: i32,
    pub facing: Facing,
    /// Metres walked on the walk under way, which picks the pose; zero at
    /// rest.
    pub walked: f32,
    pub walk: Option<Walk>,
}

impl Entity {
    /// A creature standing at the centre of a tile, facing right, at rest.
    pub fn at_tile(kind: u8, mx: i32, my: i32) -> Entity {
        let mut e = Entity { kind, x_cm: 0, y_cm: 0, facing: Facing::Right, walked: 0.0, walk: None };
        e.set_tile(mx, my);
        e
    }

    /// The walk-cycle pose to draw, of `n` in the art: `None` at rest,
    /// else the pose the distance walked reaches through a stride of
    /// `World::STRIDE` metres, wrapping, so the cadence follows the speed.
    pub fn pose(&self, n: usize) -> Option<usize> {
        if self.walk.is_none() || n == 0 {
            return None;
        }
        Some((self.walked / World::STRIDE * n as f32).floor() as usize % n)
    }

    /// Put the creature at the centre of a tile.
    pub fn set_tile(&mut self, mx: i32, my: i32) {
        self.x_cm = mx * TILE_CM + TILE_CM / 2;
        self.y_cm = my * TILE_CM + TILE_CM / 2;
    }

    /// The tile the creature stands in.
    pub fn mx(&self) -> i32 {
        self.x_cm.div_euclid(TILE_CM)
    }

    pub fn my(&self) -> i32 {
        self.y_cm.div_euclid(TILE_CM)
    }

    pub fn tile(&self) -> (i32, i32) {
        (self.mx(), self.my())
    }

    /// The position in tiles, fractional, as the camera projects it.
    pub fn pos(&self) -> (f32, f32) {
        (self.x_cm as f32 / TILE_CM as f32, self.y_cm as f32 / TILE_CM as f32)
    }

    /// The position in metres, for a readout.
    pub fn metres(&self) -> (f32, f32) {
        (self.x_cm as f32 / 100.0, self.y_cm as f32 / 100.0)
    }
}

/// A ground prop placed by hand rather than by the hashed scatter, at a
/// fractional map position; drawn by the prop pass after the scattered
/// ones, at every zoom. The editor's previews use these (ADR-003).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedProp {
    pub x: f32,
    pub y: f32,
    /// Index into the prop table.
    pub prop: usize,
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
/// The cloud field's noise: three octaves of `fbm` on its own seed.
const CLOUD_SEED: u64 = 0xC10D;
const CLOUD_OCTAVES: u32 = 3;

/// The cloud shadow field at one moment (`World::cloud_shadows`).
pub struct CloudShadows {
    offset: (f32, f32),
    shift: (f32, f32),
    threshold: f32,
    /// The noise lattice over the tiles a pass covers, if it asked for one.
    cache: Option<FbmCache>,
}

impl CloudShadows {
    /// What `World::cloud_shadow` gives at a ground point.
    #[inline]
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let ((ox, oy), (sx, sy), th) = (self.offset, self.shift, self.threshold);
        let (fx, fy) = (x + ox + sx, y + oy + sy);
        let field = match &self.cache {
            Some(c) => c.fbm(fx * 0.07, fy * 0.07),
            None => World::cloud_field(fx, fy),
        };
        smoothstep(th, th + 0.10, field)
    }
}

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
    /// Props placed by hand, drawn whatever the zoom.
    pub placed: Vec<PlacedProp>,
    seed: u64,
}

impl World {
    /// Cloud base altitude in metres over the ground.
    pub const CLOUD_ALTITUDE: f32 = 20.0;

    /// How far a clear noon lets a view see, in metres (#21).
    pub const CLEAR_VISIBILITY: f32 = 120.0;

    /// How far the air lets a view see, in metres: the clear-day distance
    /// cut by cloud cover and precipitation, and halved by full night, so
    /// a storm at dusk closes in to a few tens of metres. Never under
    /// fifteen, so a scene is always something.
    pub fn visibility(&self) -> f32 {
        let weather = (1.0 - 0.4 * self.weather.cover) * (1.0 - 0.6 * self.weather.precip);
        let light = 0.5 + 0.5 * self.skylight();
        (Self::CLEAR_VISIBILITY * weather * light).max(15.0)
    }

    /// Tiles of shadow per metre of height: the cotangent of the sun's
    /// elevation over the tile's own size, capped so dawn and dusk stretch
    /// shadows without covering the map. Cloud and cast shadows both use it.
    pub fn shadow_per_metre(&self) -> f32 {
        let elev = self.elevation().max(0.08);
        ((1.0 - elev * elev).sqrt() / elev / crate::map::TILE_METRES).min(1.8)
    }

    /// Unit ground direction shadows fall along: the sun stands to the
    /// south-east in map space, so shadows fall north-west.
    pub fn shadow_dir(&self) -> (f32, f32) {
        (-std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2)
    }

    /// Ground offset in tiles from a cloud to its shadow: the shadow
    /// direction times the altitude times the length per metre.
    pub fn shadow_shift(&self) -> (f32, f32) {
        let len = Self::CLOUD_ALTITUDE * self.shadow_per_metre();
        let (ux, uy) = self.shadow_dir();
        (len * ux, len * uy)
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
            placed: Vec::new(),
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
        [(warm[0] + (noon[0] - warm[0]) * k) * s, (warm[1] + (noon[1] - warm[1]) * k) * s, (warm[2] + (noon[2] - warm[2]) * k) * s]
    }

    /// Sky-dome ambient light; overcast days are flatter and a touch cooler.
    pub fn ambient(&self) -> [f32; 3] {
        let d = self.skylight();
        let night = [0.15, 0.17, 0.32];
        let clear = [0.42, 0.44, 0.48];
        let overcast = [0.40, 0.41, 0.44];
        let c = self.weather.cover;
        let day = [clear[0] + (overcast[0] - clear[0]) * c, clear[1] + (overcast[1] - clear[1]) * c, clear[2] + (overcast[2] - clear[2]) * c];
        [night[0] + (day[0] - night[0]) * d, night[1] + (day[1] - night[1]) * d, night[2] + (day[2] - night[2]) * d]
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
        fbm(x * 0.07, y * 0.07, CLOUD_SEED, CLOUD_OCTAVES)
    }

    /// Cloud density in `[0, 1)` over a map point, after the drift so far.
    pub fn cloud_density(&self, x: f32, y: f32) -> f32 {
        let (ox, oy) = self.cloud_offset;
        Self::cloud_field(x + ox, y + oy)
    }

    /// How much of the sun a ground point loses to the cloud whose shadow
    /// falls on it, 0 clear to 1 fully shaded.
    pub fn cloud_shadow(&self, x: f32, y: f32) -> f32 {
        self.cloud_shadows().at(x, y)
    }

    /// The cloud shadow field for this moment, its drift, shift and
    /// threshold worked out once for a pass that asks at every cell.
    pub fn cloud_shadows(&self) -> CloudShadows {
        CloudShadows { offset: self.cloud_offset, shift: self.shadow_shift(), threshold: self.cloud_threshold(), cache: None }
    }

    /// The same with the cloud noise hashed once over the tiles
    /// `x0..=x1` by `y0..=y1`, for a pass that asks at every cell of a
    /// frame.
    pub fn cloud_shadows_over(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> CloudShadows {
        let mut c = self.cloud_shadows();
        let ((ox, oy), (sx, sy)) = (c.offset, c.shift);
        let (fx0, fy0) = ((x0 as f32 + ox + sx) * 0.07, (y0 as f32 + oy + sy) * 0.07);
        let (fx1, fy1) = ((x1 as f32 + 1.0 + ox + sx) * 0.07, (y1 as f32 + 1.0 + oy + sy) * 0.07);
        c.cache = Some(FbmCache::new(fx0, fy0, fx1, fy1, CLOUD_SEED, CLOUD_OCTAVES));
        c
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

    /// Light a campfire on a tile unless it is water or off the map: the
    /// light of the `campfire` prop, which the loader guarantees. Returns
    /// whether one was lit.
    pub fn light_campfire(&mut self, map: &Map, mx: i32, my: i32) -> bool {
        let assets = &map.assets;
        let Some(spec) = assets.prop("campfire").and_then(|p| p.light).map(|i| &assets.lights[i]) else { return false };
        match map.get(mx, my) {
            Some(tile) if tile.terrain != Terrain::Water => {
                self.lights.push(spec.at(mx, my, tile.draw_z(), 1.0));
                true
            }
            _ => false,
        }
    }

    /// Put the player on the nearest land to a position.
    pub fn spawn_player(&mut self, map: &Map, mx: i32, my: i32) {
        let (mx, my) = map.nearest_land(mx, my);
        self.entities.push(Entity::at_tile(PLAYER, mx, my));
    }

    /// The player is the first entity, by convention.
    pub fn player(&self) -> Option<&Entity> {
        self.entities.first()
    }

    pub fn player_mut(&mut self) -> Option<&mut Entity> {
        self.entities.first_mut()
    }

    /// Move the player by a centimetre delta (ADR-006), refusing the move
    /// when the tile the new point falls in is off the map or terrain its
    /// kind cannot enter. Returns whether it moved.
    pub fn try_move(&mut self, map: &Map, dx_cm: i32, dy_cm: i32) -> bool {
        let Some(p) = self.player_mut() else { return false };
        let next = Entity { x_cm: p.x_cm + dx_cm, y_cm: p.y_cm + dy_cm, ..*p };
        let kind = &map.assets.creatures[p.kind as usize % map.assets.creatures.len()];
        match map.get(next.mx(), next.my()) {
            Some(t) if kind.can_enter.contains(&t.terrain) => {
                *p = next;
                true
            }
            _ => false,
        }
    }

    /// The stride of a walk cycle in metres (ADR-008): the art's poses
    /// share it, so `n` poses each hold `STRIDE / n` metres.
    pub const STRIDE: f32 = 0.7;
    /// How many times the creature's speed a run is.
    pub const RUN: f32 = 3.0;
    /// Seconds a press keeps the figure walking: two and a half ticks, so
    /// a held key renewed every repeat interval is a steady walk and a
    /// release stops the figure within three.
    pub const GRACE: f32 = 0.1;

    /// Set the player walking along `dir`, a unit map vector, for `GRACE`
    /// seconds more (ADR-008), at the run speed if `run`, facing `facing`
    /// when given and as before when not. A press on the same heading
    /// keeps the centimetre carry; a turn drops it.
    pub fn walk_toward(&mut self, dir: (f32, f32), run: bool, facing: Option<Facing>) {
        let Some(p) = self.player_mut() else { return };
        let factor = if run { World::RUN } else { 1.0 };
        let carry = match p.walk {
            Some(w) if w.dir == dir => w.carry,
            _ => (0.0, 0.0),
        };
        p.walk = Some(Walk { dir, grace: World::GRACE, factor, carry });
        if let Some(f) = facing {
            p.facing = f;
        }
    }

    /// Spend `dt` seconds of the player's walk (ADR-008): advance
    /// `speed * dt` metres along the heading in whole centimetres, the
    /// fraction carried, through `try_move`, so the tile the figure may
    /// not enter stops it — a diagonal refused as a whole slides along
    /// whichever axis is open. The tick is cut to the grace left, so a
    /// tap walks exactly `GRACE` seconds' worth. Returns whether the
    /// figure is still walking afterwards.
    pub fn step_walk(&mut self, map: &Map, dt: f32) -> bool {
        let Some(p) = self.player() else { return false };
        let Some(mut w) = p.walk else { return false };
        let dt = dt.min(w.grace).max(0.0);
        let speed = map.assets.creatures[p.kind as usize % map.assets.creatures.len()].speed * w.factor;
        let cm = speed * dt * 100.0;
        let want = (w.dir.0 * cm + w.carry.0, w.dir.1 * cm + w.carry.1);
        let (sx, sy) = (want.0.round() as i32, want.1.round() as i32);
        let from = (p.x_cm, p.y_cm);
        let moved = self.try_move(map, sx, sy) || (sx != 0 && sy != 0 && (self.try_move(map, sx, 0) || self.try_move(map, 0, sy)));
        let p = self.player_mut().expect("the player was there a moment ago");
        w.carry = if moved { (want.0 - sx as f32, want.1 - sy as f32) } else { (0.0, 0.0) };
        w.grace -= dt;
        p.walked += ((p.x_cm - from.0) as f32).hypot((p.y_cm - from.1) as f32) / 100.0;
        if w.grace <= 1e-6 {
            p.walk = None;
            p.walked = 0.0;
            return false;
        }
        p.walk = Some(w);
        true
    }
}

#[cfg(test)]
impl World {
    /// Set every temperature band's ground wetness.
    pub(crate) fn set_wetness(&mut self, w: f32) {
        self.wetness = [w; BANDS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::test_assets;

    #[test]
    fn cloud_shadows_over_a_range_are_the_cloud_shadow_to_the_bit() {
        let mut w = World::new(3);
        w.tod = 15.0;
        w.cloud_offset = (12.5, -7.25);
        w.weather.cover = 0.6;
        let c = w.cloud_shadows_over(-20, 30, 60, 90);
        for i in 0..1500 {
            // Inside the range and beyond it.
            let (x, y) = (-40.0 + i as f32 * 0.08, 20.0 + (i as f32 * 0.05).cos() * 60.0);
            assert_eq!(c.at(x, y).to_bits(), w.cloud_shadow(x, y).to_bits(), "at ({x}, {y})");
        }
    }
    use crate::blocks::Stack;
    use crate::camera::Camera;
    use crate::canvas::Canvas;
    use crate::map::Tile;
    use crate::render::{RenderOptions, Renderer, Scene};
    use crate::tileset::Tileset;

    /// An 8x8 island of grass 5 m over the water with a pond at (5, 4).
    fn pond_map() -> Map {
        Map::synthetic(8, 8, test_assets(), 0, |x, y| Tile::flat(if (x, y) == (5, 4) { -3 } else { 5 }))
    }

    #[test]
    fn player_refuses_water_and_the_island_edge_and_moves_on_land() {
        let map = pond_map();
        let mut w = World::new(1);
        w.spawn_player(&map, 4, 4);
        assert_eq!(w.player().map(|p| p.tile()), Some((4, 4)));
        assert_eq!(w.player().map(|p| (p.x_cm, p.y_cm)), Some((900, 900)), "the spawn is the tile's centre");
        assert!(!w.try_move(&map, TILE_CM, 0), "a whole tile into the pond");
        assert_eq!(w.player().map(|p| (p.x_cm, p.y_cm)), Some((900, 900)), "a refused move leaves the player put");
        assert!(w.try_move(&map, 0, TILE_CM), "onto grass");
        assert_eq!(w.player().map(|p| p.tile()), Some((4, 5)));
        assert!(w.try_move(&map, -TILE_CM, -TILE_CM), "diagonals are steps too");
        assert_eq!(w.player().map(|p| p.tile()), Some((3, 4)));
        w.player_mut().unwrap().set_tile(7, 0);
        assert!(!w.try_move(&map, TILE_CM, 0), "off the east edge");
        assert!(!w.try_move(&map, 0, -TILE_CM), "off the north edge");
        assert!(!w.try_move(&map, 0, -101), "a centimetre over the north edge is off it too");
        assert_eq!(w.player().map(|p| p.tile()), Some((7, 0)));
        assert!(w.try_move(&map, -TILE_CM, 0));
        let mut nobody = World::new(1);
        assert!(!nobody.try_move(&map, TILE_CM, 0), "no player, no move");
    }

    #[test]
    fn a_move_is_refused_by_the_tile_its_point_lands_in() {
        // The pond is tile (5, 4); from the centre of (4, 4) the water's
        // edge is a metre east. Steps that stay inside the tile are taken,
        // the centimetre that crosses the edge is not, and the point ends
        // anywhere in its tile rather than at the centre.
        let map = pond_map();
        let mut w = World::new(1);
        w.spawn_player(&map, 4, 4);
        assert!(w.try_move(&map, 60, 0));
        assert!(w.try_move(&map, 39, 0));
        assert_eq!(w.player().map(|p| (p.x_cm, p.tile())), Some((999, (4, 4))), "a centimetre short of the pond");
        assert!(!w.try_move(&map, 1, 0), "the next centimetre is in the water");
        assert_eq!(w.player().map(|p| p.x_cm), Some(999));
        assert!(w.try_move(&map, 0, 80), "along the shore is fine");
        assert_eq!(w.player().map(|p| p.tile()), Some((4, 4)));
        let (x, y) = w.player().unwrap().pos();
        assert!((x - 4.995).abs() < 1e-4 && (y - 4.9).abs() < 1e-4, "the point is fractional in tiles: {x}, {y}");
        // Negative positions still floor to their tile.
        let e = Entity { x_cm: -1, y_cm: -TILE_CM, ..Entity::at_tile(0, 0, 0) };
        assert_eq!(e.tile(), (-1, -1));
        assert_eq!(Entity::at_tile(0, -3, 2), Entity { x_cm: -500, y_cm: 500, ..e });
    }

    /// A flat plain of grass, sixty-four tiles square.
    fn plain() -> Map {
        Map::synthetic(64, 64, test_assets(), 0, |_, _| Tile::flat(5))
    }

    #[test]
    fn a_walk_covers_speed_times_seconds_over_ticks_and_a_tap_the_grace() {
        let map = plain();
        let speed = map.assets.creatures[PLAYER as usize].speed;
        assert!((speed - 1.4).abs() < 1e-6, "the person walks at 1.4 m/s");
        let mut w = World::new(1);
        w.spawn_player(&map, 32, 32);
        let start = w.player().unwrap().metres();
        // A held key renews the lease every tick: two seconds is 2.8 m to
        // the centimetre, the fractions carried from tick to tick.
        for _ in 0..50 {
            w.walk_toward((1.0, 0.0), false, Some(Facing::Right));
            assert!(w.step_walk(&map, 0.04));
        }
        let p = w.player().unwrap();
        assert!((p.metres().0 - start.0 - 2.8).abs() < 0.0051, "{} m", p.metres().0 - start.0);
        assert_eq!(p.metres().1, start.1);
        assert!(p.walk.is_some() && (p.walked - 2.8).abs() < 0.0051, "{:?} after {} m", p.walk, p.walked);
        // Running is three times as far in the same time, on a diagonal too.
        let mut r = World::new(1);
        r.spawn_player(&map, 32, 32);
        let d = std::f32::consts::FRAC_1_SQRT_2;
        for _ in 0..25 {
            r.walk_toward((d, d), true, None);
            r.step_walk(&map, 0.04);
        }
        let (x, y) = r.player().unwrap().metres();
        let dist = (x - start.0).hypot(y - start.1);
        assert!((dist - 4.2).abs() < 0.02, "{dist} m");
        // A tap walks GRACE seconds' worth, 14 cm, over three ticks, then
        // rests with the distance zeroed.
        let mut t = World::new(1);
        t.spawn_player(&map, 32, 32);
        t.walk_toward((0.0, 1.0), false, None);
        let mut ticks = 1;
        while t.step_walk(&map, 0.04) {
            ticks += 1;
        }
        assert_eq!(ticks, 3);
        let p = t.player().unwrap();
        assert!((p.metres().1 - start.1 - speed * World::GRACE).abs() < 0.0051, "{} m", p.metres().1 - start.1);
        assert!(p.walk.is_none() && p.walked == 0.0 && p.pose(4).is_none(), "{p:?}");
        assert!(!t.step_walk(&map, 0.04), "at rest a tick walks nothing");
        // Nobody: nothing walks.
        let mut n = World::new(1);
        n.walk_toward((1.0, 0.0), false, None);
        assert!(!n.step_walk(&map, 0.04));
    }

    #[test]
    fn a_walk_stops_at_water_by_the_tile_it_would_enter_and_slides_along_the_shore() {
        // The pond is tile (5, 4): its edge is a metre east of the spawn.
        let map = pond_map();
        let mut w = World::new(1);
        w.spawn_player(&map, 4, 4);
        for _ in 0..60 {
            w.walk_toward((1.0, 0.0), false, None);
            w.step_walk(&map, 0.04);
        }
        let p = *w.player().unwrap();
        assert_eq!(p.tile(), (4, 4), "never in the water");
        assert!(p.x_cm < 1000 && p.x_cm >= 1000 - 6, "stopped within a step of the edge at {}", p.x_cm);
        assert!(p.walk.is_some(), "the key is still held; the figure just cannot go");
        // A diagonal into the pond slides south along its shore.
        let d = std::f32::consts::FRAC_1_SQRT_2;
        for _ in 0..25 {
            w.walk_toward((d, d), false, None);
            w.step_walk(&map, 0.04);
        }
        let q = *w.player().unwrap();
        assert!(q.x_cm < 1000 && q.x_cm >= p.x_cm, "held at the shore: {}", q.x_cm);
        assert!(q.y_cm > p.y_cm + 90, "and walking south along it: {} from {}", q.y_cm, p.y_cm);
        assert!(map.get(q.mx(), q.my()).is_some_and(|t| t.terrain != Terrain::Water));
    }

    #[test]
    fn the_pose_cycles_by_distance_through_a_stride_and_rests_at_zero() {
        let mut e = Entity::at_tile(0, 0, 0);
        assert_eq!(e.pose(4), None, "at rest");
        e.walk = Some(Walk { dir: (1.0, 0.0), grace: 1.0, factor: 1.0, carry: (0.0, 0.0) });
        for (walked, pose) in [(0.0, 0), (0.17, 0), (0.18, 1), (0.36, 2), (0.6, 3), (0.71, 0), (1.05, 2)] {
            e.walked = walked;
            assert_eq!(e.pose(4), Some(pose), "{walked} m into a 0.7 m stride of four");
        }
        e.walked = 0.4;
        assert_eq!(e.pose(2), Some(1), "two poses hold 0.35 m each");
        assert_eq!(e.pose(0), None, "art with no poses rests");
    }

    #[test]
    fn facing_follows_the_press_and_persists_when_stopped() {
        let map = plain();
        let mut w = World::new(1);
        w.spawn_player(&map, 32, 32);
        assert_eq!(w.player().unwrap().facing, Facing::Right, "as the art is drawn");
        w.walk_toward((-1.0, 0.0), false, Some(Facing::Left));
        assert_eq!(w.player().unwrap().facing, Facing::Left);
        w.walk_toward((0.0, 1.0), false, None);
        assert_eq!(w.player().unwrap().facing, Facing::Left, "toward the camera keeps the facing");
        while w.step_walk(&map, 0.04) {}
        assert_eq!(w.player().unwrap().facing, Facing::Left, "and so does stopping");
        w.walk_toward((1.0, 0.0), true, Some(Facing::Right));
        assert_eq!(w.player().unwrap().facing, Facing::Right);
    }

    #[test]
    fn campfires_refuse_water_and_light_on_land() {
        let map = pond_map();
        let mut w = World::new(1);
        assert!(!w.light_campfire(&map, 5, 4), "on the pond");
        assert!(!w.light_campfire(&map, 9, 9), "off the map");
        assert!(w.lights.is_empty());
        assert!(w.light_campfire(&map, 4, 4));
        assert_eq!(w.lights.len(), 1);
        let fire = map.assets.light("campfire").unwrap();
        assert_eq!((w.lights[0].mx, w.lights[0].my, w.lights[0].z), (4, 4, 5), "the light sits on the tile top");
        assert_eq!((w.lights[0].radius, w.lights[0].intensity), (fire.radius, fire.intensity));
    }

    /// Draw one frame of the map with the world at `tod` and return the
    /// placed and discovered light counts the HUD would show.
    fn light_counts(map: &Map, world: &mut World, tod: f32) -> (usize, usize) {
        let ts = &Tileset::all(&map.assets)[0];
        let (w, h) = (120, 40);
        world.tod = tod;
        let mut cam = Camera::new();
        cam.set_zoom(4, w, h);
        cam.look_at(8, 8, map, w, h);
        let mut r = Renderer::new(w, h);
        let mut cv = Canvas::new(w as u16, h as u16);
        r.draw(&mut cv, &Scene::new(map, ts, world, &cam, 0.0), &RenderOptions { aa: true, clouds: true, fog: crate::render::FogMode::Perspective });
        (world.lights.len(), r.frame_light_count())
    }

    #[test]
    fn window_lights_join_the_frame_count_at_night_and_not_at_noon() {
        let assets = test_assets();
        assert!(assets.blocks[0].light.is_some(), "the first building kind glows at night");
        let map = Map::synthetic(16, 16, assets, 0, |x, y| {
            let mut t = Tile::flat(5);
            if (x, y) == (8, 8) {
                t.stack = Some(Stack { kind: 0, levels: 1 });
            }
            t
        });
        let mut w = World::new(1);
        assert!(w.light_campfire(&map, 6, 8));
        assert_eq!(light_counts(&map, &mut w, 12.0), (1, 0), "at noon only the campfire counts");
        assert_eq!(light_counts(&map, &mut w, 22.0), (1, 1), "at night the lit window joins it");
    }

    #[test]
    fn storm_preset_raises_precipitation() {
        let mut w = World::new(3);
        w.day_secs = 1.0;
        assert_eq!(w.weather.precip, 0.0);
        w.weather_preset = Some(STORM);
        for _ in 0..40 {
            w.tick(0.05);
        }
        assert!(w.weather.precip > 0.95 && w.weather.cover > 0.95, "{:?}", w.weather);
        w.weather_preset = Some(0);
        for _ in 0..40 {
            w.tick(0.05);
        }
        assert!(w.weather.precip < 0.05, "the clear preset dries it up: {:?}", w.weather);
    }

    #[test]
    fn snowpack_grows_below_freezing_and_melts_above() {
        let mut w = World::new(3);
        w.day_secs = 1.0;
        // Annual 8C is -1C in winter and 17C in summer, with no permanent snow.
        let annual = 8.0;
        w.season = 3.0;
        assert_eq!(w.snow_at(annual), 0.0);
        w.weather_preset = Some(STORM);
        let mut last = 0.0;
        for hours in 0..3 {
            for _ in 0..3 {
                w.tick(0.05);
            }
            assert!(w.snow_at(annual) > last, "storm stretch {hours}: the pack grows through a winter storm");
            last = w.snow_at(annual);
        }
        for _ in 0..40 {
            w.tick(0.05);
        }
        assert_eq!(w.snow_at(annual), 1.0, "two days of storm bury the band");
        assert!(w.snowing_at(annual));
        w.season = 1.0;
        w.weather_preset = Some(0);
        assert!(!w.snowing_at(annual));
        let mut last = w.snow_at(annual);
        for day in 0..6 {
            for _ in 0..20 {
                w.tick(0.05);
            }
            let now = w.snow_at(annual);
            assert!(now <= last, "day {day}: summer melts the pack");
            last = now;
        }
        assert_eq!(w.snow_at(annual), 0.0);
    }

    #[test]
    fn wetness_dries_faster_in_sun() {
        let mut sun = World::new(3);
        let mut night = World::new(3);
        for (w, tod) in [(&mut sun, 12.0), (&mut night, 0.0)] {
            w.tod = tod;
            w.weather_preset = Some(0);
            w.set_wetness(1.0);
            w.tick(1.0);
        }
        let (a, b) = (sun.wet_at(15.0), night.wet_at(15.0));
        assert!(a < b && b < 1.0, "sun {a} night {b}");
        // Rain wets the ground again.
        night.weather_preset = Some(STORM);
        night.day_secs = 1.0;
        night.set_wetness(0.0);
        for _ in 0..10 {
            night.tick(0.05);
        }
        assert!(night.wet_at(15.0) > 0.5, "{}", night.wet_at(15.0));
    }

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
    fn player_never_walks_into_generated_water() {
        let map = Map::new(32, 32, 7, test_assets());
        let mut w = World::new(7);
        w.spawn_player(&map, 16, 16);
        let p = *w.player().unwrap();
        assert_ne!(map.get(p.mx(), p.my()).unwrap().terrain, Terrain::Water);
        for _ in 0..600 {
            w.try_move(&map, 37, 0);
        }
        let p = *w.player().unwrap();
        assert!(map.get(p.mx(), p.my()).is_some_and(|t| t.terrain != Terrain::Water));
    }
}
