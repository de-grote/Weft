// Shared by the engine's shaders: put in front of each one (shader packs' too).

// The uv of each face corner (`CORNERS`, from the prelude), the two triangles of a face, and
// vanilla's face shading per direction (-X +X -Y +Y -Z +Z).
var<private> UVS: array<vec2<f32>, 4> = array<vec2<f32>, 4>(vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0), vec2(0.0, 0.0));
var<private> TRI: array<u32, 6> = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
var<private> SHADE: array<f32, 6> = array<f32, 6>(0.6, 0.6, 0.5, 1.0, 0.8, 0.8);

// `SHADE` blended for any unit normal.
fn normal_shade(n: vec3<f32>) -> f32 {
    return n.x * n.x * 0.6 + n.z * n.z * 0.8 + n.y * n.y * select(0.5, 1.0, n.y > 0.0);
}

// How far into the fog a point at `dist` is, 0..1.
fn fog_amount(dist: f32, start: f32, end: f32) -> f32 {
    if dist <= start {
        return 0.0;
    }
    if dist >= end {
        return 1.0;
    }
    return (dist - start) / (end - start);
}

fn brightness_curve(level: f32) -> f32 {
    return level / (4.0 - 3.0 * level);
}

// Light as the world stores it, a byte each for sky, red, green, blue (0..15), to 0..1.
fn unpack_light(l: u32) -> vec4<f32> {
    let v = vec4<f32>(f32(l & 255u), f32((l >> 8u) & 255u), f32((l >> 16u) & 255u), f32(l >> 24u));
    return min(v, vec4(15.0)) / 15.0;
}

// Vanilla 26.1's lightmap: dim ambient, sky light that turns blue and weaker at night, warm block
// light that whitens towards both ends, then the Brightness setting bends it up keeping the hue.
// `day` daylight 0..1, `ambient` the dimension's floor 0..1, `bright` the Brightness setting,
// `night` night vision 0..1: lit everywhere as under the open sky at noon, lamps adding their
// warmth on top (not vanilla's raising of each color to full, which makes a torch at night look
// like a dark patch).
fn lightmap(light: vec4<f32>, day: f32, ambient: f32, bright: f32, night: f32, sunlight: f32) -> vec3<f32> {
    let d = clamp((mix(day, 1.0, night) - 0.12) / 0.88, 0.0, 1.0);
    let sky = brightness_curve(mix(light.x, 1.0, night)) * mix(0.24, 1.0, d) * mix(sunlight, 1.0, night);
    var c = vec3(ambient) + mix(vec3(0.478, 0.478, 1.0), vec3(1.0), d) * sky;
    let block = light.yzw;
    if any(block > vec3(0.0)) {
        // Vanilla's warmth of block light: yellower the dimmer it is, white at full, by the
        // level of its brightest channel so a colored lamp only grows brighter toward it.
        let level = max(max(block.x, block.y), block.z);
        let tint = vec3(1.0, (level * 0.6 + 0.4) * 0.6 + 0.4, level * level * 0.6 + 0.4);
        // Added to the sky's, each channel on its own: a lamp only ever brightens, and a
        // colored one shows by day too.
        c += min(tint * vec3(brightness_curve(block.x), brightness_curve(block.y), brightness_curve(block.z)) * 1.4, vec3(0.85));
    }
    // Light adds up past what a screen shows: what is bright bends toward full instead of
    // being cut there, so sun and lamp together are brighter than the sun alone.
    let knee = 0.8;
    c = select(c, knee + (1.0 - knee) * (1.0 - exp(-(c - knee) / (1.0 - knee))), c > vec3(knee));
    c = clamp(c, vec3(0.0), vec3(1.0));
    let m = max(max(c.x, c.y), c.z);
    if m > 1e-4 {
        let one_minus_m = 1.0 - m;
        let m2 = one_minus_m * one_minus_m;
        let m4 = m2 * m2;
        let lifted = c * ((1.0 - m4) / m);
        return mix(c, lifted, bright);
    }
    return c;
}

// What snippets get (`shaders.rs`), here so a shader pack replacing an engine shader keeps them.

// What a block shader (`shaders/block/<name>.wgsl`, `fn shade(p: BlockPixel) -> vec4<f32>`) gets;
// it returns the lit color before fog.
struct BlockPixel {
    // 0..1 across the face.
    uv: vec2<f32>,
    // The texel (with its overlay) and the light on it (shading, AO, tint, glow), both linear.
    albedo: vec4<f32>,
    light: vec3<f32>,
    // Block coordinates of the pixel (in a moving structure, the structure's own).
    pos: vec3<f32>,
    time: f32,
}

// What an entity shader (`fn shade(p: EntityPixel) -> vec4<f32>`) gets; it returns the lit
// color, alpha below 0.5 cuts the pixel out.
struct EntityPixel {
    uv: vec2<f32>,
    // Skin texel and the light on it (shading, hurt flash and glow included), linear.
    albedo: vec4<f32>,
    light: vec3<f32>,
    time: f32,
}

// What a text effect (`shaders/fx/<name>.wgsl`, `fn effect(g: Glyph) -> Glyph`) may change.
struct Glyph {
    // Added to this corner of the glyph, physical pixels.
    offset: vec2<f32>,
    // Linear color and alpha.
    color: vec4<f32>,
    time: f32,
    // Number of the glyph in its text, for waves running along it.
    index: f32,
    // Which corner, 0..1 in each axis.
    corner: vec2<f32>,
    // Physical pixels per GUI pixel.
    scale: f32,
    // The dark copy drawn under the text.
    shadow: bool,
}

// A themed panel filled by a shader (`shaders/gui/<name>.wgsl`, `fn fill(p: GuiPixel) -> vec4<f32>`).
struct GuiPixel {
    // 0..1 across the panel.
    uv: vec2<f32>,
    // Panel size in physical pixels.
    size: vec2<f32>,
    // The theme color, linear.
    color: vec4<f32>,
    time: f32,
}

// What a screen effect (`shaders/post/<name>.wgsl`, `fn apply(p: PostPixel) -> vec4<f32>`) gets.
struct PostPixel {
    // 0..1 across the screen, top left first.
    uv: vec2<f32>,
    // The image so far at `uv`, linear.
    color: vec4<f32>,
    // 0..1 as the mod set it, for fading in and out.
    strength: f32,
    time: f32,
    // Pixels.
    size: vec2<f32>,
    scene: u32,
}

// What a sky shader (`shaders/sky/<name>.wgsl`, `fn shade(p: SkyPixel) -> vec4<f32>`) gets; it
// returns the color and how much it covers, and the opacity is applied after it.
struct SkyPixel {
    // Unit direction of the pixel in the world, and in the object's own frame, which turns with
    // its orbit (x points at a body).
    dir: vec3<f32>,
    local: vec3<f32>,
    // 0..1 across the quad, or across the picture of a dome.
    uv: vec2<f32>,
    // The picture's sample (white without one) and the object's color, both linear.
    albedo: vec4<f32>,
    color: vec4<f32>,
    daylight: f32,
    time: f32,
}

// What a particle shader (`shaders/particle/<name>.wgsl`, `fn shade(p: ParticleFrag) -> vec4<f32>`)
// gets; it returns the color before lighting and fog.
struct ParticleFrag {
    // 0..1 across the quad.
    uv: vec2<f32>,
    // The texture sample and the kind's color at this age, both linear.
    albedo: vec4<f32>,
    color: vec4<f32>,
    // 0 at birth, 1 at the end of its life.
    life: f32,
    time: f32,
}
