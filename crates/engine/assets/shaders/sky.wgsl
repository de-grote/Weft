// The sky, drawn after the opaque world wherever nothing covers it. Every object is one entry of
// `sky`; things that fill the screen are a single triangle, bodies a quad, stars one quad per star
// made here from their number.

var<private> CORNER: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
);

struct VOut {
    @builtin(position) clip: vec4<f32>,
    // Screen position (-1..1) for what fills the screen, else the direction.
    @location(0) at: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) index: u32,
}

fn mat(s: Sky) -> mat3x3<f32> {
    return mat3x3(s.axes[0].xyz, s.axes[1].xyz, s.axes[2].xyz);
}

fn hash(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn unit(v: u32) -> f32 {
    return f32(hash(v)) / 4294967295.0;
}

fn far_point(dir: vec3<f32>) -> vec4<f32> {
    // At depth 0, the far plane: the sky is behind everything.
    let c = scene.view_proj * vec4(dir * 100.0, 1.0);
    return vec4(c.xy, 0.0, c.w);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let s = sky[ii];
    var out: VOut;
    out.index = ii;
    if s.ids.y == SKY_BODY {
        let c = CORNER[vi % 6u];
        let local = vec3(1.0, (0.5 - c.y) * 2.0 * s.extra.x, (c.x - 0.5) * 2.0 * s.extra.x);
        let dir = mat(s) * local;
        out.clip = far_point(dir);
        out.at = dir;
        out.uv = mix(s.uv.xy, s.uv.zw, c);
    } else if s.ids.y == SKY_STARS {
        // A random point on the sphere, a random size and turn, from the star's number alone.
        let star = vi / 6u + u32(s.uv.y) * 7919u;
        let z = unit(star * 4u) * 2.0 - 1.0;
        let a = unit(star * 4u + 1u) * 6.2831853;
        let r = sqrt(1.0 - z * z);
        let toward = vec3(r * cos(a), z, r * sin(a));
        let size = s.uv.x * (0.6 + 0.4 * unit(star * 4u + 2u)) * 2.0;
        let turn = unit(star * 4u + 3u) * 6.2831853;
        var side = cross(toward, vec3(0.0, 1.0, 0.0));
        if dot(side, side) < 0.001 {
            side = vec3(1.0, 0.0, 0.0);
        }
        side = normalize(side);
        let up = cross(side, toward);
        let c = CORNER[vi % 6u] - vec2(0.5);
        let q = vec2(c.x * cos(turn) - c.y * sin(turn), c.x * sin(turn) + c.y * cos(turn));
        let dir = mat(s) * (toward + (side * q.x + up * q.y) * size);
        out.clip = far_point(dir);
        out.at = dir;
        out.uv = CORNER[vi % 6u];
    } else {
        let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
        // The overlay of a liquid goes over everything, the hand too; depth 1 is the near plane.
        out.clip = vec4(p, select(0.0, 1.0, s.ids.y == SKY_OVERLAY), 1.0);
        out.at = vec3(p, 1.0);
        out.uv = vec2(0.0);
    }
    return out;
}

// Clouds: each pixel's ray walks the picture's grid between the two planes of the layer, so one
// triangle draws them however far they reach. From outside the layer a ray crosses few cells even
// at the horizon, since it soon leaves through the other plane.
// uv: the camera's place in its cell (x, z), the cell in the picture (x, z). extra: cell size,
// thickness and height over the camera. Scene environment.yz supplies cloud fog distances.
struct CloudOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) at: vec3<f32>,
    @location(1) @interpolate(flat) index: u32,
}

struct CloudPixel {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@vertex
fn vs_clouds(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> CloudOut {
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
    var out: CloudOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.at = vec3(p, 1.0);
    out.index = ii;
    return out;
}

fn cloud_at(s: Sky, cell: vec2<f32>, dims: vec2<f32>) -> bool {
    let at = fract((cell + s.uv.zw + vec2(0.5)) / dims);
    return textureSampleLevel(textures[s.ids.x], nearest_clamp, at, 0.0).a >= 0.5;
}

@fragment
fn fs_clouds(in: CloudOut) -> CloudPixel {
    let s = sky[in.index];
    let dir = normalize(mat(sky[0]) * in.at);
    let size = s.extra.x;
    let low = s.extra.z;
    let reach = scene.environment.z;
    let high = low + s.extra.y;
    var near = 0.0;
    var far = reach;
    if abs(dir.y) > 0.00001 {
        near = max(near, min(low / dir.y, high / dir.y));
        far = min(far, max(low / dir.y, high / dir.y));
    } else if low > 0.0 || high < 0.0 {
        discard;
    }
    if near >= far {
        discard;
    }
    let dims = vec2<f32>(f32(s.ids.z), f32(s.ids.w));
    let start = s.uv.xy + dir.xz * near / size;
    var cell = floor(start);
    let turn = sign(dir.xz);
    let delta = size / max(abs(dir.xz), vec2(0.000001));
    var next = near + select(start - cell, cell + vec2(1.0) - start, dir.xz > vec2(0.0)) * delta;
    var t = near;
    // 0: came in through a plane, 1 and 2: through a side along x or z.
    var side = 0u;
    // From inside a cloud, it isn't what is seen.
    var within = near == 0.0 && cloud_at(s, cell, dims);
    var hit = false;
    let steps = u32(ceil((far - near) / size * (abs(dir.x) + abs(dir.z)))) + 2u;
    for (var i = 0u; i < steps; i++) {
        let filled = cloud_at(s, cell, dims);
        if filled && !within {
            hit = true;
            break;
        }
        within = within && filled;
        if next.x < next.y {
            t = next.x;
            next.x += delta.x;
            cell.x += turn.x;
            side = 1u;
        } else {
            t = next.y;
            next.y += delta.y;
            cell.y += turn.y;
            side = 2u;
        }
        if t >= far {
            break;
        }
    }
    if !hit {
        discard;
    }
    // Lit from above, like blocks.
    var shade = select(0.9, 0.8, side == 1u);
    if side == 0u {
        shade = select(1.0, 0.7, dir.y > 0.0);
    }
    let rel = dir * t;
    let fog = fog_amount(t, scene.environment.y, reach);
    let color = mix(s.color.rgb * shade, scene.fog_color.rgb, smoothstep(0.0, 1.0, fog));
    let alpha = s.color.a * (1.0 - smoothstep(0.6, 1.0, fog));
    if alpha <= 0.0 { discard; }
    let clip = scene.view_proj * vec4(rel, 1.0);
    return CloudPixel(vec4(color * alpha, alpha), clip.z / clip.w);
}

// Vanilla's sky is a disc 16 blocks above the eye fading into fog with distance.
fn gradient(dir: vec3<f32>, s: Sky) -> vec4<f32> {
    if dir.y <= 0.0 {
        // Under the horizon: fog, and from below sea level the dark of vanilla's void.
        let dark = s.uv.x * smoothstep(0.0, 0.08, -dir.y);
        return vec4(scene.fog_color.rgb * (1.0 - dark) * s.color.a, s.color.a);
    }
    return vec4(mix(s.color.rgb, scene.fog_color.rgb, clamp(0.0625 / dir.y, 0.0, 1.0)) * s.color.a, s.color.a);
}

// Vanilla's sunset fan: an oval lying on the horizon on the side of the body.
fn glow(dir: vec3<f32>, s: Sky) -> vec4<f32> {
    let toward = dot(dir, s.axes[0].xyz);
    if toward <= 0.0 {
        return vec4(0.0);
    }
    let x = dot(dir, s.axes[2].xyz) / toward / 1.2;
    let y = dir.y / toward / max(0.4 * s.color.a, 0.001);
    let a = s.color.a * max(0.0, 1.0 - sqrt(x * x + y * y));
    return vec4(s.color.rgb, a);
}

// Panorama: longitude and latitude. Box: three by two faces; first row down, up, south; second
// west, north, east.
fn dome_uv(l: vec3<f32>, s: Sky) -> vec2<f32> {
    if s.uv.x == 0.0 {
        return vec2(atan2(l.x, -l.z) / 6.2831853 + 0.5, 0.5 - asin(clamp(l.y, -1.0, 1.0)) / 3.1415927);
    }
    let m = abs(l);
    var face: vec2<f32>;
    var cell: vec2<f32>;
    if m.y >= m.x && m.y >= m.z {
        face = vec2(l.x, select(-l.z, l.z, l.y > 0.0)) / m.y;
        cell = vec2(select(0.0, 1.0, l.y > 0.0), 0.0);
    } else if m.z >= m.x {
        face = vec2(select(l.x, -l.x, l.z > 0.0), -l.y) / m.z;
        cell = select(vec2(1.0, 1.0), vec2(2.0, 0.0), l.z > 0.0);
    } else {
        face = vec2(select(-l.z, l.z, l.x > 0.0), -l.y) / m.x;
        cell = select(vec2(0.0, 1.0), vec2(2.0, 1.0), l.x > 0.0);
    }
    return (cell + face * 0.5 + 0.5) / vec2(3.0, 2.0);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    let s = sky[in.index];
    if s.ids.y == SKY_OVERLAY {
        // Vanilla's: four tiles across the screen, sliding with the look. A texture with
        // animation frames shows its first.
        if s.extra.y != 0.0 && in.at.y > -0.25 {
            return vec4(0.0);
        }
        let tile = fract((in.at.xy * vec2(0.5, -0.5) + 0.5) * s.extra.x + s.uv.xy) * s.uv.zw + vec2(0.0, s.extra.z);
        let c = textureSampleLevel(textures[s.ids.x], nearest_clamp, tile, 0.0) * s.color;
        return vec4(c.rgb * c.a, c.a);
    }
    var dir = in.at;
    if s.ids.y == SKY_GRADIENT || s.ids.y == SKY_GLOW || s.ids.y == SKY_DOME {
        dir = mat(sky[0]) * in.at;
    }
    dir = normalize(dir);
    if s.ids.y == SKY_GRADIENT {
        return gradient(dir, s);
    }
    if s.ids.y == SKY_GLOW {
        let c = glow(dir, s);
        return vec4(c.rgb * c.a, c.a);
    }
    let local = dir * mat(s);
    var uv = in.uv;
    var albedo = vec4(1.0);
    if s.ids.y == SKY_DOME {
        uv = dome_uv(local, s);
        if s.ids.x != 0xffffffffu {
            albedo = textureSampleLevel(textures[s.ids.x], linear_clamp, uv, 0.0);
        }
    } else if s.ids.x != 0xffffffffu {
        albedo = textureSampleLevel(textures[s.ids.x], nearest_clamp, uv, 0.0);
    }
    let c = sky_shade(s.ids.z, SkyPixel(dir, local, uv, albedo, vec4(s.color.rgb, 1.0), scene.params.z, scene.params.w));
    let a = c.a * s.color.a;
    return vec4(c.rgb * a, select(a, 0.0, s.ids.w != 0u));
}
