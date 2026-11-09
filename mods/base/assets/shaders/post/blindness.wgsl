// Blindness: all but a little around the middle of the view goes black.
fn apply(p: PostPixel) -> vec4<f32> {
    let d = length((p.uv - vec2(0.5)) * vec2(p.size.x / p.size.y, 1.0));
    let seen = 1.0 - smoothstep(0.05, 0.35, d);
    return vec4(p.color.rgb * mix(1.0, seen * 0.4, p.strength), 1.0);
}
