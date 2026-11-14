# Environment sources

`Environment` describes what an observer perceives: fog, sky illumination, screen overlays,
post shaders and acoustics. These parameters are independent. A sound filter does not require a
liquid, and changing fog does not change weather simulation, damage, AI or block light levels.

## Content presets

Attach a named contribution with `.environment(..)`:

```rust
r.block(&VAPOR).environment(
    Environment::new("vapor_haze").fog(0x809070, 0.0, 18.0).fade(0.4, 0.8)
);
r.item(&HELMET).environment(
    Environment::new("helmet_audio").observer(environment::observer::BODY)
        .acoustics(1400.0, 0.0, 0.75)
);
```

Sources are collected from the current dimension and weather, the biome and block at the
observation point, the liquid volume containing it, the player's entity kind, active entity
effects and equipped items. An entity kind can also emit a zone around its visible instances:

```rust
r.entity(&SPIRIT).environment(
    Environment::new("spirit_zone").radius(8.0, 16.0)
        .fog(0x705080, 2.0, 32.0).fade(0.5, 1.0)
);
```

Liquid `.fog(rgb, start, end)`, `.overlay(texture, opacity)` and `.no_overlay()` are presets
of this same mechanism. Start and end are explicit block distances. Water's low-pass filter is
an independent environment contribution; heat never selects an audio filter.
`EntityEffectBuilder::screen(&SHADER)` is likewise a post-shader preset. Native dimension,
weather, cave reverb, inside-block and fire appearances all contribute to the same result.

## Control from code

`Client::environment(&layer)` adds or replaces a persistent local source. Its name is relative
to the calling mod. Use the same name to update it; `clear_environment(name)` removes it.
A client frame callback can calculate any field from mod state:

```rust
r.client_frame(|c| {
    let mut haze = Environment::new("altitude_haze").priority(20);
    let altitude = c.state().camera_pos.y as f32;
    haze.values.fog_end = (256.0 - altitude).clamp(24.0, 256.0);
    c.environment(&haze);
});
```

`World::environment(entity, &layer)` uses an ordinary saved, public entity component. The
server sends changed data to the owner and watchers, and each client resolves it for its own
observer. This source can be set or cleared from any server event, tick or command:

```rust
let layer = Environment::new("darkened_air").priority(100).fade(1.5, 1.5)
    .sunlight(0.0).fog(0x181028, 4.0, 40.0).sky_color(0x181028);
w.environment(player, &layer);
// Later:
w.clear_environment(player, "darkened_air");
```

Radius zero on an entity source affects only that entity's observer. `.radius(inner, outer)`
creates a zone following its entity: full strength inside `inner`, zero outside `outer`,
linear between. The entity must be known to the client, so its radius does not extend network
visibility. Apply a source to each affected player when it must cover a whole dimension.
`.around([x, y, z], inner, outer)` fixes a zone at a position instead of following its owner.
A local source without a zone affects the client everywhere until cleared or the world closes.

`sunlight(0)` suppresses sky illumination, including the night brightness floor, and directional
sunlight. Clouds use this illumination together with ambient light. Emitted block light
and dimension ambient light remain independent. To suppress
ambient light too, set `layer.values.ambient = 0.0`; this removes the native minimum
illumination as well. This field is linear intensity. Night vision is another independent field.
To hide a celestial object, use its dimension-qualified handle:

```rust
let sun = OVERWORLD.sky(&SUN);
let layer = Environment::new("hidden_body").sky_object(sun, 0.0);
```

This controls the object's visibility, including its sunset glow and fog warming. It does not
change illumination. Conversely, suppressing illumination does not hide the body's picture.
`sky_opacity(0)` hides the entire sky, while `values.cloud_opacity = 0.0` hides just clouds.

Cloud appearance is resolved by the same sources: the dimension's time preset supplies
`values.cloud_color` (linear RGB), weather supplies `cloud_gray` and `cloud_dark`, and
`cloud_opacity` controls visibility. The renderer samples none of these from weather or time.
`.cloud_color(0xRRGGBB)` replaces the time tint and resets weather graying/darkening for clouds;
use the raw fields to combine them independently. Cloud tint is multiplied by the sky object's
own color and illuminated by the resolved sunlight/ambient. These fields support the same
priorities, conditions, blend modes and transitions as fog and other environment values.

Cloud fog has its own `.cloud_fog(start, end)` distances in blocks from the camera, independent
of world fog. The **Cloud render distance** video slider (`settings::CLOUD_DISTANCE`,
`core:cloud_distance`) supplies the native end distance, with fading starting halfway there.
An environment source can override either field (`values.cloud_fog_start`, `cloud_fog_end`)
using the usual mixing and transitions. End zero hides clouds. The fog color is the resolved
world fog color. Clouds outside the 3D end distance are not sampled or drawn; entire layers
beyond that sphere are skipped before submission.

```rust
let mist = Environment::new("local_mist").priority(50)
    .fog(0x809070, 12.0, 48.0)
    .cloud_fog(12.0, 100.0);
c.environment(&mist);
```

## Mixing and transitions

Each frame starts with the dimension's time-dependent defaults. Active sources then apply in
ascending `(priority, source identity)` order. Higher priority applies later. Identity includes
the source kind, owner/slot and layer name; equal-priority behavior is deterministic. A runtime
entity source with the same name overrides that entity kind's preset while present. Other
sources with the same layer name remain independent. Within one source, the last definition
of a name wins even when its condition is false. Convenience presets use stable `core:` names,
so editing another mod's liquid fog or status screen replaces that preset regardless of mod id.

| Blend | Scalar result at weight `w` |
| --- | --- |
| `environment::blend::REPLACE` (default) | `lerp(previous, value, w)` |
| `ADD` | `previous + value * w` |
| `MULTIPLY` | `previous * lerp(1, value, w)` |
| `MAX` | `lerp(previous, max(previous, value), w)` |

Only finite selected fields participate. `Environment::new` leaves every scalar unset (NaN),
so setting fog leaves sound and illumination alone. Public `values` fields expose all
parameters; colors there are linear RGB. `.fog` and `.sky_color` convert `0xRRGGBB` and reset
weather graying for that color, so a replacement stays the requested color.

Overlays append in source order. `.replace_overlays()` fades preceding overlays out, even if
the replacing layer adds none. `.post(&SHADER, strength)` and `.sky_object(..)` combine by
name using the selected blend mode. Post strength starts at zero, object visibility at one.
`.replace_post()` fades all preceding post effects out. A later source can also replace one
shader's strength with zero. `Client::post_effect` is a named local-source convenience;
post order follows source priority and identity, rather than call order.

`.fade(enter, leave)` controls activation weight in real seconds, including condition changes,
leaving a zone and explicit removal. The old material remains available during fade-out.
Repeated updates to the same name do not restart the fade. Parameter edits on an already
active source take effect immediately; animate the fields from a frame callback when needed.

Native preset priorities are below the default mod priority of zero:

| Preset | Priority |
| --- | ---: |
| Weather | -900 |
| Cave reverb | -800 |
| Liquid defaults | -700 |
| Liquid fog / overlay convenience | -699 / -698 |
| Inside block | -600 |
| Burning | -500 |
| Status screen convenience | -400 |

## Observation and conditions

Camera sampling is the default, including in detached and third-person modes.
`.observer(environment::observer::BODY)` samples the predicted local player's eyes. A suit can
therefore filter sound while a free camera sees a different atmosphere. A source's zone and
`.when("condition expression")` use the selected point. Block/state, height, sky light and
enclosure conditions read that point; entity attributes, equipment and custom conditions refer
to the local observing player. The swimming condition respects which liquids it can swim in.

`c.environment_values()` reads the final scalar values from the last completed evaluation.
Client frame callbacks run before the next evaluation, so they observe the previous frame.
The shared `ENVIRONMENT` component is available for ordinary component inspection and edits.
Camera effect flags still decide whether overlays and post shaders are drawn.
