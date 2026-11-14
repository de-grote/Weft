# Sky guide

Everything you see in the sky is an object registered for a specific dimension, and that includes the sun and the moon, which are just two ordinary objects of the base game that start half a turn apart and aren't connected in any other way. This means that you can add your own objects and change or remove objects from any other mod, and even a large number of them will barely affect performance.

## Note
Examples that you can copy from:
- `mods/base/src/lib.rs` has the sun, the moon, the stars and the clouds
- `mods/testmod/src/lib.rs` has Mars, with a sun that doesn't move and two moons that go in opposite directions

## Quick links
[bodies](#bodies) | [changing what is there](#changing-what-is-there) | [bodies in front of each other](#bodies-in-front-of-each-other) | [stars](#stars) | [clouds](#clouds) | [skyboxes](#skyboxes) | [shaders](#shaders) | [the sky's colors](#the-skys-colors)

## Bodies
To add a body, you only need one line with its texture, its size and the way it moves. `orbit` is measured in ticks per turn, so a value of 1200 means one turn per minute:
```rust
static PLANET: SkyObject = SkyObject::named("planet");
r.sky_body(&OVERWORLD, &PLANET).texture("planet").size(6.0).orbit(96000.0).phase(0.1).tilt(20.0);
```
The texture goes to `assets/textures/sky/planet.png` and can be any size, and everything except the texture is optional:

| method | what it does |
|---|---|
| `orbit(ticks)` | ticks per turn around the sky, and a negative value makes it go the other way. Without it, the body follows the day of the dimension like the sun does, which means it slows down a little around noon and midnight |
| `orbit_still()` | keeps the body where its phase, tilt and heading put it |
| `orbit_day()` | makes the body follow the day again, for example if it comes from another mod and you gave it an orbit |
| `phase(turns)` | sets where the body starts: 0 is rising in the east, 0.25 is overhead, 0.5 is setting, and 0.75 is under your feet |
| `tilt(degrees)` | leans the orbit to the south, away from overhead |
| `heading(degrees)` | rotates the orbit around the vertical axis, so with a value of 90 the body rises in the north |
| `size(degrees)` | sets how much of the sky the body takes across (33 for the sun and 23 for the moon) |
| `spin(ticks)` | ticks per turn around the body's own center |
| `color(rgb)` | sets a color that is multiplied with the texture |
| `opacity(day, night)` | sets how visible the body is during full day and deep night, from 0 to 1 |
| `frames(columns, rows)` | treats the texture as a grid of frames that are shown one after another, like the phases of the moon |
| `frame_ticks(ticks)` | sets how long each frame lasts, which is one day of the dimension by default |
| `glow(rgb)` | lights the horizon while the body rises and sets. The sunset of the sun is `glow(0xFFE633)`, and two suns will give you two sunsets |
| `blend_add()`, `blend_cover()` | `blend_add()` adds the body's light to what is behind it instead of covering it, and `blend_cover()` switches it back |
| `order(n)` | sets the drawing order from low to high, and bodies with equal values are drawn in the order they were registered |

Daylight and the light on blocks come from the day of the dimension (`day_length`, `fixed_time`, `daylight` for its curve) and not from any body, which means that if you remove the sun, the day will still happen, and if you give the sun a different orbit, it will no longer match the light.

## Changing what is there
An object of a dimension is `DIM.sky(&OBJECT)`, and it is edited and removed like anything else registered. An edit changes only the fields you set and the object stays the same type, so this example makes the moon bigger, while its texture, phases and orbit stay the same:
```rust
use base_api::{MOON, OVERWORLD, STARS, SUN};
r.edit(OVERWORLD.sky(&MOON)).size(40.0);
r.edit(OVERWORLD.sky(&SUN)).texture("mymod:blue_sun").glow(0x80C0FF);
r.remove(OVERWORLD.sky(&STARS));
```
If several mods change the same field, the one loaded last wins. `r.replace(OVERWORLD.sky(&MOON))` starts it again from nothing, a body unless you say otherwise with `.set(&comps::sky::KIND, abi::sky_kind::DOME)`. All of this works per dimension, and since base registers its four objects in both `OVERWORLD` and `FLATLANDS`, you have to change or remove them in each dimension separately. A resource pack can also replace the textures themselves, for example `textures/sky/sun.png`.

## Bodies in front of each other
A body covers everything behind it wherever its texture is opaque, and the body that was registered later or has a higher `order` is drawn in front. Orbits of different lengths bring two bodies together from time to time, so one of them passes in front of the other, and for example orbits of 24000 and 22000 ticks meet about every 11 days.
This doesn't dim the daylight, since the light on blocks comes from the day and not from what is drawn in the sky. Stars go behind bodies in the same way, so an opaque moon hides the stars behind it.

## Stars
```rust
r.sky_stars(&OVERWORLD, &MY_STARS).count(4000).size(0.3).seed(7).color(0xFFE0C0).orbit(48000.0);
```
Stars turn together on the orbit you give them, or with the day if you don't give them one, and they are visible at night, which is `opacity(0.0, 0.22)` by default. `count` sets the number of stars and `seed` decides how they are scattered, and since the GPU generates them from their number alone, even a large count doesn't need any memory and is drawn in one call.

## Clouds
```rust
r.sky_clouds(&OVERWORLD, &MY_CLOUDS).texture("clouds").height(300.0).drift(0.03).color(0xFFE0E0);
```
Every filled pixel of the texture becomes a box that is 12 blocks wide and 4 blocks high, and the texture repeats across the sky, further than the terrain and distant terrain reach. They are one draw whatever the size of the picture, as a shader walks it pixel by pixel. Without `.height` the underside is at 224.33, and players can raise any clouds in the video settings. Clouds exist in the world, which means that they hide what is behind them and you can fly through them. Rain makes them gray and hides bodies and stars, and which weathers come is the dimension's `.weathers(&[..])`, while `w.raining()`, `w.weather()` and `w.set_weather(&weathers::THUNDER, ticks)` let you read and change it. Weathers are values like any: `r.weather(&ACID_RAIN).darkness(0.5).duration(2400, 6000).weight(1)` adds one, which comes now and then after clear weather where a dimension lists it, and `/weather mymod:acid_rain` sets it. What falls is the biome's: rain, snow with `.snows()`, or your own picture with `r.precipitation(&ASH).texture("ash").speed(0.3).density(0.4)` and `.precipitation(&ASH)` on the biome.

## Skyboxes
A dome covers the entire sky at once and doesn't move unless you give it an orbit, and it is drawn over the sky's colors, but under the stars and bodies:
```rust
r.sky_dome(&OVERWORLD, &NEBULA).texture("nebula").opacity(0.0, 1.0).orbit(24000.0);
```
The layout of a dome is determined by the shape of its texture. A texture that is twice as wide as it is high is treated as a panorama, which is the format that any skybox tool exports, and a texture with a ratio of three to two is treated as the six faces of a box, with down, up and south on the first row, and west, north and east on the second.

A dome stays at full strength during both day and night unless you set its opacity, so if you want a sky that changes, you have to register one dome with `opacity(1.0, 0.0)` and another one with `opacity(0.0, 1.0)`. The horizon still fades into the fog color of the dimension, which is set with `.sky_color(..)`, and sunsets are drawn over domes, so a sun without `glow` won't have a sunset.

## Shaders
Any object can be colored by a shader snippet instead of its plain texture, which allows you to create auroras and clouds of gas without any texture at all:
```wgsl
// assets/shaders/sky/aurora.wgsl
fn shade(p: SkyPixel) -> vec4<f32> {
    let wave = sin(p.dir.x * 8.0 + p.time) * 0.5 + 0.5;
    let band = smoothstep(0.1, 0.5, p.dir.y) * smoothstep(0.9, 0.5, p.dir.y);
    return vec4(vec3(0.2, 1.0, 0.6) * wave, band * (1.0 - p.daylight));
}
```
```rust
r.sky_dome(&OVERWORLD, &AURORA).shader(&AURORA_SHADER);
```
`SkyPixel` contains `dir`, which is the direction of the pixel in the world, `local`, which is the same direction in the object's own frame that turns with its orbit, with x pointing at a body, and also `uv`, `albedo` (the texture, or white if there is none), `color`, `daylight` and `time`. The snippet returns the color and how much it covers, and `opacity` is applied on top of that. You can read more about snippets in the [shaders guide](shaders.md#snippets).

## The sky's colors
Behind everything there is a gradient from the `sky_color(sky, fog)` of the dimension, with the sky color above and the fog color at the horizon, and both of them get darker during the night like in vanilla. A dome with an opaque texture simply covers this gradient.

The video settings have separate cloud elevation and render-distance sliders. Cloud distance
is a camera-centered radius in blocks, with fog fading at its edge; mods can override it with
`Environment::cloud_fog(start, end)` independently of world fog.

Liquids supply environment presets that hide the sky and set their own fog. See [liquids](details/liquids.md) for the convenience methods and [environment sources](environment.md) for live overrides of fog, sky objects, illumination and clouds.