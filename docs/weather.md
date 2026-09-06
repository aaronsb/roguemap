# Weather, seasons and the clock

The sky is four numbers: cloud cover, wind speed, wind direction and
precipitation. They drift along slow noise on the day clock, or follow a
preset from the settings. Everything else — the gusts in a crown, the
whitecaps on a lake, the snow on a roof, the colour of the sun — falls out
of those four and the time of day.

![clouds](screenshots/clouds.png)

Above the cloud layer at the widest zoom. `make snap OUT=shot.png
ARGS="fill=1 zoom=0 cx=0 cy=0 t=3 tod=14 cover=0.4"` renders it.

## The state

```rust
pub struct Weather { pub cover: f32, pub wind: f32, pub wind_dir: f32, pub precip: f32 }
```

Cover, wind and precipitation are 0 to 1; direction is radians. The world
starts at cover 0.3, wind 0.2, direction 0.6 and no precipitation, at one
in the afternoon in summer.

`World::tick` runs only while the clock is running. It advances the clock,
then the weather, then accumulation. A day is `day_secs` of real time, ten
minutes by default, and the `Day length` setting offers 2 minutes, 10
minutes, 1 hour and 24 hours.

On automatic, each of the three scalars has a target read from
one-dimensional value noise on the day count, at its own rate and offset:

```
cover  = smoothstep(0.25, 0.8, n(1.4, 3.0))
precip = smoothstep(0.7, 0.95, cover) * smoothstep(0.35, 0.7, n(0.9, 40.0))
wind   = (n(2.1, 90.0) * 0.5 + precip * 0.5).clamp(0.02, 1.0)
```

Precipitation needs cover to be well past its threshold before it starts,
so it rains only under a full sky, and wind rises with rain. The current
values approach their targets at eight per day, so a front takes a few
hours to arrive rather than snapping on. The direction wanders on its own
noise.

The `Weather` setting overrides cover and precipitation with a preset:
clear is `(0.15, 0.0)`, cloudy `(0.75, 0.0)`, rain `(0.9, 0.5)` and storm
`(1.0, 1.0)`. The `Wind` setting overrides wind with calm 0.05, breeze
0.2, windy 0.55 or gale 1.0. `W` cycles the weather presets from the
scene.

## Wind and gusts

Wind is one scalar and one direction, but what you see is a field. A gust
is value noise at 0.06 cycles per tile advected along the wind direction,
thresholded at `0.95 - wind * 0.9` and softened over a quarter, then
scaled by `0.3 + 0.7 * wind`. In calm air the threshold is nearly out of
reach and almost nothing gusts; in a gale most of the field is over it.

A tree's crown shears along the wind by its gust, oscillating on a phase
taken from its tile's seed, so no two trees in a stand sway together. The
shear is linear in height, which is what keeps the ray's path through a
crown a straight line.

Grass is separate and cheaper: two sine waves in space and time whose
speed rises with the wind, bucketed into a left, upright or right glyph.
At the widest zoom grass wind is switched off entirely.

Water choppiness is `(wind * 0.9 + precip * 0.2)`, clamped, and it is what
raises whitecaps — on open water only, since a body of 200 tiles or fewer
is treated as a pond and stays calm.

## Clouds and parallax

The cloud field is three octaves of noise at 0.07 cycles per tile, drifting
with the wind at `0.6 * wind²` tiles a second, so a gale moves the sky
about a tile every two seconds. Cover sets the threshold at `1.05 - cover
* 0.85`, so raising cover lowers the bar and more of the field becomes
cloud.

The layer draws only at the two smallest zooms, where the view is above
it. Each screen cell samples the field where a ray from a virtual camera
meets the cloud plane. The camera's altitude is 100 metres at the outer
zooms and more as you close in, and the cloud plane is 20 metres up, so
panning moves the clouds faster than the ground by `C / (C - H)`. A cell
well past the threshold is drawn solid; one near it is blended.

Cloud shadow is the same field sampled at an offset: the cloud altitude
times the shadow length per metre, along the sun's ground direction, which
is the same direction the cast shadow mask uses. So a cloud's shadow lies
where the sun would put it, and it lengthens at dawn and dusk.

Cloud cover feeds back into the light: it dims the sun by `1 - 0.75 *
cover²`, pulls the ambient sky from clear toward overcast, and takes up to
72 percent of the direct sun where a cloud is overhead.

## Precipitation

Precipitation draws under the cloud layer, and nothing draws below 0.02.
Which kind falls is decided by the temperature at the screen centre: below
one degree of seasonal temperature it snows, otherwise it rains.

Snow is about one flake per forty cells times the precipitation, each
falling at its own speed with a sideways drift that is part sine and part
wind. Rain is denser — one drop per twenty-eight cells — and much faster,
26 cells a second and up, with the wind shearing the column.

## Accumulation

Snow and wetness are not stored per tile. They are stored per temperature
band: 71 buckets, one for each whole degree from −40 to +30. A tile reads
the bucket its annual temperature falls in. That is what lets a storm lay
snow on the mountains and rain in the valley in one pass, without touching
a tile.

Per day, in a band whose seasonal temperature is below one degree,
snowpack grows by three times the precipitation and caps at 1.5;
otherwise wetness grows by six times and caps at 1. Snow melts faster the
warmer and brighter it is, and wetness dries faster still. Both scale with
daylight, so a sunny afternoon clears a shower quickly and a cold night
does not.

On top of the pack there is permanent snow: a smoothstep between −2 and −9
degrees of seasonal temperature, so the high ground is white whatever the
weather has done lately.

Wetness is what `wet_darkening` reads. Rain darkens sand, dirt and rock by
the wetness times each surface's factor.

![winter](screenshots/winter.png)

Winter after two simulated days of storm. `make snap OUT=shot.png
ARGS="fill=1 zoom=1 cx=500 cy=-300 t=3 tod=12 season=3 simdays=2"` renders
it: `simdays` shortens the day to a second, forces the storm preset, and
runs the clock until that many days have passed, so the accumulation is
real rather than painted on.

## Seasons and vigour

Season is a continuous number in 0 to 4 — spring, summer, autumn, winter —
and the palette lerps between the two it sits between, so a quarter-step
with `[` or `]` is visible. The seasonal temperature of a place swings ±9
degrees about its annual figure, peaking at summer and bottoming at
winter.

Vigour smoothsteps that seasonal temperature between −5 and 7 degrees. It
is what a deciduous species carries as foliage, so a tree loses its leaves
when its own place gets cold rather than on a calendar date; a tree at the
treeline is bare while the same species in the valley still has its
canopy. Vigour also drives the grass: above 0.55 the ground carries full
cover glyphs, between 0.15 and 0.55 it is stubble, and below that nothing.
A seasonal biome's ground colour is tinted toward dormancy by `(1 -
vigour) * 0.85`.

## Day and night

The clock is an hour figure in 0 to 24. Sun elevation is a sine that is
positive between six and eighteen:

```
elevation = sin((tod - 6) / 12 * PI)
daylight  = clamp(elevation, 0, 1) ^ 0.6
skylight  = smoothstep(-0.18, 0.35, elevation)
```

`daylight` drives the direct sun, `skylight` the sky and the ambient
light, and the two differ deliberately: the sky is already blue before the
sun clears the horizon and stays lit after it sets, which is what gives
dusk and dawn.

The sun's colour runs from a warm low-angle `[1.0, 0.55, 0.30]` to a noon
`[1.0, 0.97, 0.90]`, dimmed by cover. Ambient runs from a night floor of
`[0.15, 0.17, 0.32]` — which cloud cover cannot take lower — to a day
colour that is clear sky lerped toward overcast. The sky itself runs from
near black to a blue that flattens under cloud, with stars fading out as
skylight rises.

Shadow length per metre of height is the cotangent of the sun's elevation
over the 2 m tile, capped at 1.8 tiles, so dawn and dusk stretch shadows a
long way without covering the map. At night the sun is zero, the shadow
mask is skipped entirely, and lit windows appear: every stack in view
whose block kind names a light pushes one, scaled by how dark the sky is.

`,` and `.` step the clock by an hour, `p` pauses it, and `[` and `]` step
the season by a quarter. In a headless frame, `tod` and `season` set them
directly.
