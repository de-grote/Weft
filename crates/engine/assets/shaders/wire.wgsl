// Redstone wire: a quad on a block's face for each piece and each arm of one, colored by the power
// of its piece (`client/wires.rs`).

struct WirePush {
    origin: vec4<i32>,
    frac: vec4<f32>,
}
var<immediate> push: WirePush;

var<private> CORNER: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
);

// Off the block it lies on, not to fight with it.
const LIFT: f32 = 0.015;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) data: vec2<u32>,
    @location(2) @interpolate(flat) power: f32,
    @location(3) dist: f32,
    // On a body: the world's light where that is.
    @location(4) @interpolate(flat) around: u32,
    @location(5) @interpolate(flat) on_body: f32,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let q = quads[ii];
    let face = u32(q.pos.w) & 7u;
    let mode = u32(q.pos.w) >> 3u;
    var c = CORNER[vi % 6u];
    // The line picture runs along v. On u it is turned; an arm is half of it.
    var uv = c;
    switch mode {
        case 1u: { uv = c.yx; }
        case 2u: { c.x = 0.5 + c.x * 0.5; uv = c.yx; }
        case 3u: { c.x = c.x * 0.5; uv = c.yx; }
        case 4u: { c.y = 0.5 + c.y * 0.5; uv = c; }
        case 5u: { c.y = c.y * 0.5; uv = c; }
        default: {}
    }
    var u = vec3(1.0, 0.0, 0.0);
    var v = vec3(0.0, 1.0, 0.0);
    switch face / 2u {
        case 0u: { u = vec3(0.0, 0.0, 1.0); }
        case 1u: { v = vec3(0.0, 0.0, 1.0); }
        default: {}
    }
    let cell = vec3<f32>(q.pos.xyz - push.origin.xyz) - push.frac.xyz;
    let world = cell + vec3(0.5) + FACE_DIRS[face] * (0.5 - LIFT) + u * (c.x - 0.5) + v * (c.y - 0.5);
    var out: VOut;
    out.clip = scene.view_proj * vec4(world, 1.0);
    out.uv = uv;
    out.data = vec2(q.data.x, q.data.y);
    out.power = f32(levels[q.data.z]) / 15.0;
    out.dist = length(world);
    out.around = bitcast<u32>(push.origin.w);
    out.on_body = push.frac.w;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(textures[in.data.x], nearest_clamp, in.uv, 0.0);
    if color.a < 0.5 {
        discard;
    }
    // Vanilla's red, from dull to bright.
    let p = in.power;
    let red = pow(vec3(p * 0.6 + select(0.3, 0.4, p > 0.0), clamp(p * p * 0.7 - 0.5, 0.0, 1.0), clamp(p * p * 0.6 - 0.7, 0.0, 1.0)), vec3(2.2));
    var own = unpack_light(in.data.y);
    if in.on_body > 0.5 {
        let w = unpack_light(in.around);
        own = vec4(min(own.x, w.x), max(own.yzw, w.yzw));
    }
    let light = pow(lightmap(own, scene.params.z, scene.light.x, scene.light.y, scene.light.w, scene.environment.x), vec3(2.2));
    // Power shows in the dark as well.
    let shown = color.rgb * red * max(light, vec3(p * 0.7));
    let fog = fog_amount(in.dist, scene.params.x, scene.params.y);
    return vec4(mix(shown, scene.fog_color.rgb, fog), 1.0);
}
