# Particles and effects reference

Particles, bursts and what the GPU does with them.

## Burst

Particles sent when this is dropped: `w.spawn_particles(&SPARK, pos).count(10).speed(1.5)`.

- <a id="Burst-to"></a>`.to(who: impl Into<crate::Who>)`: For whom, of the players on the server (those near it unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L373)
- <a id="Burst-area"></a>`.area(shape: &crate::Shape)`: The particles start anywhere inside `shape` (a ring, a shell, a magic circle) instead of a box around the position; `outward` sends them away from the position. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L379)
- <a id="Burst-count"></a>`.count(n: u32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L383)
- <a id="Burst-spread"></a>`.spread(s: f32)`: Each particle starts up to this far from the position on every axis. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L388)
- <a id="Burst-spread_box"></a>`.spread_box(s: impl Into<Vec3>)`: `spread` of its own on each axis: a flat sheet, a column. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L393)
- <a id="Burst-velocity"></a>`.velocity(v: impl Into<Vec3>)`: Starting velocity, blocks per second. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L398)
- <a id="Burst-speed"></a>`.speed(s: f32)`: Random extra velocity up to this, in any direction. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L403)
- <a id="Burst-color"></a>`.color(rgb: u32)`: 0xRRGGBB the kind's color is multiplied by: one white kind in any color. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L408)
- <a id="Burst-outward"></a>`.outward(mut self)`: Each flies away from the middle of the burst, more or less, like the pieces of a broken block. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L413)
- <a id="Burst-block"></a>`.block(state: u32)`: Block state the textureless kinds (block dust) take their texture from. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L418)

## Client

- <a id="Client-post_effect"></a>`.post_effect(effect: &crate::Shader, strength: f32) -> bool`: Turns a screen effect (`shaders/post/<name>.wgsl`) on at `strength` (0 turns it off). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L470)

## ConditionCtx

What a condition is asked about.

- <a id="ConditionCtx-world"></a>`.world() -> Option<World>`: The server's world, when the server asks (loot, spawning). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L340)
- <a id="ConditionCtx-client"></a>`.client() -> Option<ClientView>`: The client, when it asks (music, text): its own player. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L344)
- <a id="ConditionCtx-pos"></a>`.pos() -> Vec3`: The spot, the block, the player. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L348)
- <a id="ConditionCtx-entity"></a>`.entity() -> u64`: Who it is about (the killer or breaker for loot), 0 for nobody. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L352)
- <a id="ConditionCtx-arg"></a>`.arg() -> &str`: What is between its parentheses where it is used, empty for none. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L356)

## MusicBuilder

Background music being registered (`Registry::music`): after a quiet while, a track of the first music whose conditions hold.

- <a id="MusicBuilder-tracks"></a>`.tracks(names: &[&str])`: Files streamed (`sounds/<name>.ogg|.wav`), relative to this mod. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L542)
- <a id="MusicBuilder-dimension"></a>`.dimension(dimension: &crate::Dimension)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L546)
- <a id="MusicBuilder-when"></a>`.when(c: impl Into<crate::Cond>)`: Plays only while this holds (`&conditions::NIGHT`, a mod's own, any `Cond`); call again for more, all of which have to hold. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L551)

## ParticleBuilder

A particle kind being registered (`Registry::particle`): half a second to a second, a tenth to a fifth of a block, white, without gravity, unless told otherwise.

- <a id="ParticleBuilder-on_land"></a>`.on_land(when: When, then: Then)`: What landing on a block does where `when` holds; rules are tried in order, and without one that holds it lies there. Any landing or hitting rule makes it stop on blocks. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L98)
- <a id="ParticleBuilder-on_wall"></a>`.on_wall(when: When, then: Then)`: Running into a block's side; without a rule it slides along. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L102)
- <a id="ParticleBuilder-on_ceiling"></a>`.on_ceiling(when: When, then: Then)`: Running into a block's underside; without a rule it stops. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L106)
- <a id="ParticleBuilder-on_enter_liquid"></a>`.on_enter_liquid(when: When, then: Then)`: Going into a liquid (`when` asks about that liquid). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L110)
- <a id="ParticleBuilder-on_leave_liquid"></a>`.on_leave_liquid(when: When, then: Then)`: Coming out of one: a bubble pops. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L114)
- <a id="ParticleBuilder-on_expire"></a>`.on_expire(when: When, then: Then)`: The end of its life: by default the end of it, `Then::turn_into` goes on as another kind (a spark dying into smoke). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L119)
- <a id="ParticleBuilder-textures"></a>`.textures(names: &[&str])`: Frames over its life; relative names are under `particle/`. None: block dust (pieces of the burst's block). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L124)
- <a id="ParticleBuilder-life"></a>`.life(min: f32, max: f32)`: Seconds, picked between the two. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L129)
- <a id="ParticleBuilder-size"></a>`.size(min: f32, max: f32)`: Edge length in blocks, picked between the two. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L133)
- <a id="ParticleBuilder-gravity"></a>`.gravity(g: f32)`: Blocks per second squared, pulling down (negative rises). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L137)
- <a id="ParticleBuilder-drag"></a>`.drag(d: f32)`: Fraction of speed kept per tick (vanilla 0.98). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L141)
- <a id="ParticleBuilder-color"></a>`.color(start: u32, end: u32)`: 0xRRGGBBAA at birth and at the end of its life. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L145)
- <a id="ParticleBuilder-colliding"></a>`.colliding()`: Stops on blocks: lies where it lands, slides along walls. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L149)
- <a id="ParticleBuilder-fullbright"></a>`.fullbright()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L152)
- <a id="ParticleBuilder-translucent"></a>`.translucent()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L155)
- <a id="ParticleBuilder-shrinking"></a>`.shrinking()` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L158)
- <a id="ParticleBuilder-shader"></a>`.shader(shader: &crate::Shader)`: Colors it with `shaders/particle/<name>.wgsl` (`fn shade(p: ParticleFrag) -> vec4<f32>`). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L162)

## PlaySound

A sound about to play (`Output::play_sound`).

- <a id="PlaySound-to"></a>`.to(who: impl Into<crate::Who>)`: Who hears it, of the players on the server (those in its range unless said). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L599)
- <a id="PlaySound-pitch"></a>`.pitch(pitch: f32)`: Playback speed (1 = as recorded, 2 an octave up) instead of one picked from the sound's range: a note block's `2f32.powf((key - 12) as f32 / 12.0)`. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L605)

## SoundBuilder

A sound event being registered (`Registry::sound`): at volume 1 and as recorded, heard 16 blocks away, a block sound, unless told otherwise.

- <a id="SoundBuilder-files"></a>`.files(names: &[&str])`: Variants picked at random: `sounds/<name>.ogg` or `.wav`, relative to this mod. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L490)
- <a id="SoundBuilder-category"></a>`.category(category: &SoundCategory)`: Category for its volume setting: the engine's (`core:creatures` ...) or one registered with `Registry::sound_category` (any mod's). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L496)
- <a id="SoundBuilder-volume"></a>`.volume(v: f32)` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L499)
- <a id="SoundBuilder-pitch"></a>`.pitch(min: f32, max: f32)`: Playback speed picked between the two. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L503)
- <a id="SoundBuilder-range"></a>`.range(blocks: f32)`: Blocks away it is still heard. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L507)
- <a id="SoundBuilder-falloff"></a>`.falloff(full_within: f32, curve: Falloff)`: Full volume within `full_within` blocks, then fading to nothing at `range` along `curve` (by default within 1 block, evenly). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L512)

## Then

What a particle's rule does.

- <a id="Then-die"></a>`.die() -> Then` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L65)
- <a id="Then-stop"></a>`.stop() -> Then`: Stays where it is for the rest of its life. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L69)
- <a id="Then-slide"></a>`.slide() -> Then`: Keeps going along what it hit. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L73)
- <a id="Then-bounce"></a>`.bounce(share: f32) -> Then`: Comes back off what it hit with `share` of its speed. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L77)
- <a id="Then-turn_into"></a>`.turn_into(kind: &Particle) -> Then`: Becomes a particle of `kind` where it is, its speed kept and its life starting anew; `kind` must be drawn the same way (`translucent` or not). [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L82)
- <a id="Then-turn_into_still"></a>`.turn_into_still(kind: &Particle) -> Then`: `turn_into`, standing still: a ripple where a drop fell. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L86)

## When

Where a particle's rule holds: the block it hit, or the one it is in.

- <a id="When-any"></a>`.any() -> When` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L41)
- <a id="When-liquid"></a>`.liquid() -> When` [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L44)
- <a id="When-tag"></a>`.tag(tag: &str) -> When`: A block with `tag` (`water`, `lava`, a mod's own). At most 5 different tags over all particle kinds of the game: the GPU keeps them with each block. [source](https://github.com/Weft-Rust/Weft/blob/master/crates/sdk/src/effects.rs#L49)
