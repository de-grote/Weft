// White, a flash (something about to go off).
fn effect(p: EntityPixel, c: vec4<f32>, strength: f32) -> vec4<f32> {
    return vec4(c.rgb * mix(1.0, 4.6, strength), c.a);
}
