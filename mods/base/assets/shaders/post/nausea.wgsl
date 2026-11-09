// Nausea: the picture sways and swims, tinted a little sick.
fn apply(p: PostPixel) -> vec4<f32> {
    let wave = vec2(sin(p.time * 1.7 + p.uv.y * 9.0), cos(p.time * 1.3 + p.uv.x * 7.0)) * 0.012 * p.strength;
    let c = scene_at(p, clamp(p.uv + wave, vec2(0.0), vec2(1.0))).rgb;
    return vec4(mix(c, c * vec3(0.9, 1.05, 0.85), p.strength * 0.5), 1.0);
}
