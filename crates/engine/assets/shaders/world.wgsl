
var<private> TRI_FLIP: array<u32, 6> = array<u32, 6>(1u, 2u, 3u, 1u, 3u, 0u);
var<private> AO: array<f32, 4> = array<f32, 4>(0.42, 0.62, 0.8, 1.0);

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Textures (bindless indices) of this frame and the next.
    @location(1) @interpolate(flat) layer: u32,
    @location(2) color: vec3<f32>,
    @location(3) light: vec4<f32>,
    @location(4) dist: f32,
    @location(5) @interpolate(flat) layer2: u32,
    @location(6) @interpolate(flat) blend: f32,
    // Overlay texture + 1 (0: none) and the tint it takes; without an overlay the tint is also
    // in `color` (`tint` then only colors emission).
    @location(7) @interpolate(flat) overlay: u32,
    @location(8) tint: vec3<f32>,
    // Glow mask texture (LabPBR emission), all ones: none.
    @location(9) @interpolate(flat) glow: u32,
    // Block shader slot of the texture, 0 none, and the block coordinates for it.
    @location(10) @interpolate(flat) slot: u32,
    @location(11) pos: vec3<f32>,
    @location(12) @interpolate(flat) size: f32,
}

fn shader_of(tex: u32) -> u32 {
    if tex >= arrayLength(&tex_shader) {
        return 0u;
    }
    return tex_shader[tex];
}

fn glow_of(tex: u32) -> u32 {
    if tex >= arrayLength(&glow_map) {
        return 0xffffffffu;
    }
    return glow_map[tex];
}

fn unpack_rgb(c: u32) -> vec3<f32> {
    return vec3<f32>(f32((c >> 16u) & 255u), f32((c >> 8u) & 255u), f32(c & 255u)) / 255.0;
}

fn column_tint(x: i32, z: i32, tint: u32) -> u32 {
    let t = i32(g.ids.w);
    let tx = ((x >> 4u) % t + t) % t;
    let tz = ((z >> 4u) % t + t) % t;
    let i = g.ids.z + u32(tz * t + tx) * 256u + u32((z & 15) * 16 + (x & 15));
    return biomes[i * g.extra.z + tint - 1u];
}

// Tint at a block corner: average of the four columns touching it, falling back to the face's
// own biome where colors are not uploaded yet.
fn tint_color(tint: u32, biome: u32, x: i32, z: i32) -> vec3<f32> {
    if tint == 0u {
        return vec3(1.0);
    }
    var sum = vec3(0.0);
    var n = 0.0;
    for (var k = 0; k < 4; k++) {
        let c = column_tint(x - 1 + (k & 1), z - 1 + (k >> 1u), tint);
        if c != 0u {
            sum += unpack_rgb(c);
            n += 1.0;
        }
    }
    if n > 0.0 {
        return sum / n;
    }
    return unpack_rgb(biomes[biome * g.extra.z + tint - 1u]);
}

struct Frame {
    a: u32,
    b: u32,
    blend: f32,
    size: f32,
}

fn texture_frame(name: u32) -> Frame {
    let base = tex_info[name * 4u];
    let info = tex_info[name * 4u + 1u];
    // A size of 0 would make every derivative NaN, and the sampler the smallest mip.
    let size = f32(max(tex_info[name * 4u + 2u], 1u));
    if info == 0u {
        return Frame(base, base, 0.0, size);
    }
    let count = info & 65535u;
    let total = f32((info >> 16u) & 32767u);
    let ticks = scene.light.z;
    let t = ticks - floor(ticks / total) * total;
    var acc = 0.0;
    for (var i = 0u; i < count; i++) {
        let e = frames[base + i];
        let d = f32(e >> 16u);
        if t < acc + d || i + 1u == count {
            let next = frames[base + (i + 1u) % count];
            var f = 0.0;
            if (info >> 31u) == 1u {
                f = (t - acc) / d;
            }
            return Frame(e & 65535u, next & 65535u, f, size);
        }
        acc += d;
    }
    return Frame(base, base, 0.0, size);
}

// A body's blocks have their own light and are lit as under open sky; where it is, the world has
// its say.
fn on_body(l: vec4<f32>, k: i32) -> vec4<f32> {
    if k == 0 {
        return l;
    }
    let w = unpack_light(bitcast<u32>(offsets[k].w));
    return vec4(min(l.x, w.x), max(l.yzw, w.yzw));
}

fn finish(local: vec3<f32>, origin: vec4<i32>) -> vec4<f32> {
    let pos = vec3<f32>(origin.xyz) * 16.0 + local - g.cam_frac.xyz + offsets[origin.w].xyz;
    return vec4(pos, 1.0);
}

@vertex
fn vs_face(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let f = vi / 4u;
    let w0 = records[f * 4u];
    let w1 = records[f * 4u + 1u];
    let w2 = records[f * 4u + 2u];
    let w3 = records[f * 4u + 3u];
    let a = face_ao(w0);
    let flip = a.x + a.z < a.y + a.w;
    // Indexed as (0 1 2)(0 2 3); starting one corner later puts the diagonal on 1-3.
    var c = vi % 4u;
    if flip {
        c = (c + 1u) % 4u;
    }
    let dir = face_dir(w0);
    let local = face_pos(w0) + CORNERS[dir * 4u + c];
    let pos = finish(local, origins[ii]);
    var out: VOut;
    out.clip = scene.view_proj * pos;
    out.dist = length(pos.xyz);
    out.uv = UVS[(c + ((w0 >> FACE_TURN_SHIFT) & FACE_TURN_MASK)) % 4u];
    let tex_id = w1 & FACE_TEX_MASK;
    let fr = texture_frame(tex_id);
    out.size = fr.size;
    out.layer = fr.a;
    out.layer2 = fr.b;
    out.blend = fr.blend;
    out.glow = glow_of(fr.a);
    out.light = on_body(corner_light(w2, w3, c), origins[ii].w);
    let o = origins[ii] + g.cam_cube;
    out.slot = shader_of(tex_id);
    out.pos = vec3<f32>(o.xyz * 16) + local;
    let tint_kind = (w0 >> FACE_TINT_SHIFT) & TINT_MASK;
    var tint = vec3(1.0);
    if tint_kind != 0u {
        tint = tint_color(tint_kind, w1 >> FACE_BIOME_SHIFT, o.x * 16 + i32(local.x), o.z * 16 + i32(local.z));
    }
    out.tint = tint;
    let ov = frames[g.extra.x + tex_id];
    if ov != 0u {
        out.overlay = texture_frame(ov - 1u).a + 1u;
        out.color = vec3(SHADE[dir] * AO[a[c]]);
    } else {
        out.overlay = 0u;
        out.color = tint * SHADE[dir] * AO[a[c]];
    }
    return out;
}

@vertex
fn vs_quad(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let q = vi / 4u;
    let c = vi % 4u;
    let base = q * 8u;
    let p0 = quad_pos(records[base]);
    let p1 = quad_pos(records[base + 1u]);
    let p2 = quad_pos(records[base + 2u]);
    let local = quad_pos(records[base + c]);
    let w4 = records[base + 4u];
    let uvw = records[base + 5u];
    let uv = quad_uv_fine(uvw, records[base], records[base + 1u], records[base + 2u], records[base + 3u]);
    var uvs = array<vec2<f32>, 4>(vec2(uv.x, uv.w), vec2(uv.z, uv.w), vec2(uv.z, uv.y), vec2(uv.x, uv.y));
    let n = normalize(cross(p1 - p0, p2 - p1));
    // Face shading blended by normal; plants opt out like vanilla's crossed squares.
    var shade = n.x * n.x * 0.6 + n.z * n.z * 0.8 + n.y * n.y * select(0.5, 1.0, n.y > 0.0);
    if ((uvw >> QUAD_TINT_SHIFT) & QUAD_FLAT) != 0u {
        shade = 1.0;
    }
    let pos = finish(local, origins[ii]);
    var out: VOut;
    out.clip = scene.view_proj * pos;
    out.dist = length(pos.xyz);
    out.uv = uvs[c];
    let tex_id = w4 & FACE_TEX_MASK;
    let fr = texture_frame(tex_id);
    out.size = fr.size;
    out.layer = fr.a;
    out.layer2 = fr.b;
    out.blend = fr.blend;
    out.glow = glow_of(fr.a);
    out.light = on_body(corner_light(records[base + 6u], records[base + 7u], c), origins[ii].w);
    let o = origins[ii] + g.cam_cube;
    out.slot = shader_of(tex_id);
    out.pos = vec3<f32>(o.xyz * 16) + local;
    let tint_kind = (uvw >> QUAD_TINT_SHIFT) & TINT_MASK;
    var tint = vec3(1.0);
    if tint_kind != 0u {
        tint = tint_color(tint_kind, w4 >> FACE_BIOME_SHIFT, o.x * 16 + i32(round(local.x)), o.z * 16 + i32(round(local.z)));
    }
    out.tint = tint;
    let ov = frames[g.extra.x + tex_id];
    if ov != 0u {
        out.overlay = texture_frame(ov - 1u).a + 1u;
        out.color = vec3(shade);
    } else {
        out.overlay = 0u;
        out.color = tint * shade;
    }
    return out;
}

fn shade(in: VOut, albedo: vec4<f32>) -> vec4<f32> {
    var light = in.color * lightmap(in.light, scene.params.z, scene.light.x, scene.light.y);
    // Glowing texels are lifted towards full brightness by their mask: lit = albedo * max(light,
    // emission), so they stay their own (tinted, unshaded) color in the dark.
    if in.glow != 0xffffffffu {
        let e = textureSampleLevel(textures[in.glow], nearest_clamp, in.uv, 0.0).r;
        light = max(light, select(in.tint, vec3(1.0), in.overlay != 0u) * e);
    }
    var linear_light: vec3<f32>;
    if light.r == light.g && light.r == light.b {
        linear_light = vec3(pow(light.r, 2.2));
    } else {
        linear_light = pow(light, vec3(2.2));
    }
    var lit: vec4<f32>;
    if in.slot == 0u {
        lit = vec4(albedo.rgb * linear_light, albedo.a);
    } else {
        lit = block_shade(in.slot, BlockPixel(in.uv, albedo, linear_light, in.pos, scene.params.w));
    }
    let fog = fog_amount(in.dist, scene.params.x, scene.params.y);
    return vec4(mix(lit.rgb, scene.fog_color.rgb, fog), lit.a);
}

// Nearest texels with antialiased borders (vanilla 26.1's sampleNearest): up close each texel
// keeps its color but its edge blends over one screen pixel, so blocks look clean, not jagged.
fn nearest_uv(size: f32, uv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>) -> vec2<f32> {
    let inv_s = 1.0 / size;
    let t = uv * size;
    let r = round(t);
    let inv_pp = inverseSqrt(max(du * du + dv * dv, vec2(1e-10))) * inv_s;
    let off = clamp((t - r) * inv_pp + 0.5, vec2(0.0), vec2(1.0));
    return (r - 0.5 + off) * inv_s;
}

fn sample(in: VOut, nuv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>) -> vec4<f32> {
    let a = textureSampleGrad(textures[in.layer], linear_mipped, nuv, du, dv);
    if in.blend <= 0.0 {
        return a;
    }
    let b = textureSampleGrad(textures[in.layer2], linear_mipped, nuv, du, dv);
    return mix(a, b, in.blend);
}

// The tinted overlay over the base texture; its tint is a gamma-space factor like the others.
fn with_overlay(in: VOut, albedo: vec4<f32>, nuv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>) -> vec4<f32> {
    if in.overlay == 0u {
        return albedo;
    }
    let ov = textureSampleGrad(textures[in.overlay - 1u], linear_mipped, nuv, du, dv);
    return vec4(mix(albedo.rgb, ov.rgb * pow(in.tint, vec3(2.2)), ov.a), albedo.a);
}

@fragment
fn fs_opaque(in: VOut) -> @location(0) vec4<f32> {
    let du = dpdx(in.uv);
    let dv = dpdy(in.uv);
    let nuv = nearest_uv(in.size, in.uv, du, dv);
    let albedo = with_overlay(in, sample(in, nuv, du, dv), nuv, du, dv);
    return vec4(shade(in, albedo).rgb, 1.0);
}

// Mip level from the smaller screen derivative: a plant quad seen almost edge-on would otherwise
// pick a tiny mip whose averaged texels cover the whole quad and draw it as a solid strip.
fn sample_cutout(in: VOut, nuv: vec2<f32>, du: vec2<f32>, dv: vec2<f32>) -> vec4<f32> {
    let size = in.size;
    let t = in.uv * size;
    let lod = max(log2(max(min(length(du * size), length(dv * size)), 1e-6)), 0.0);
    let a = textureSampleLevel(textures[in.layer], linear_mipped, nuv, lod);
    if in.blend <= 0.0 {
        return a;
    }
    let b = textureSampleLevel(textures[in.layer2], linear_mipped, nuv, lod);
    return mix(a, b, in.blend);
}

@fragment
fn fs_cutout(in: VOut) -> @location(0) vec4<f32> {
    let du = dpdx(in.uv);
    let dv = dpdy(in.uv);
    let nuv = nearest_uv(in.size, in.uv, du, dv);
    let albedo = with_overlay(in, sample_cutout(in, nuv, du, dv), nuv, du, dv);
    // Vanilla's alpha test (GL_GREATER 0.1).
    if albedo.a <= 0.1 {
        discard;
    }
    return vec4(shade(in, albedo).rgb, 1.0);
}

@fragment
fn fs_translucent(in: VOut) -> @location(0) vec4<f32> {
    let du = dpdx(in.uv);
    let dv = dpdy(in.uv);
    let nuv = nearest_uv(in.size, in.uv, du, dv);
    let albedo = with_overlay(in, sample(in, nuv, du, dv), nuv, du, dv);
    return shade(in, albedo);
}

// Distant terrain (`client::lod`): a record is the `dir` face of a box in its tile, textured and
// tinted like a near face, full sky light. Its pixels in columns whose cubes show their ground
// are dropped (`lod_mask`); it sits a little lower so the cubes win along the edges.
fn lod_uv(dir: u32, p: vec3<f32>) -> vec2<f32> {
    if dir == 2u || dir == 3u {
        return p.xz;
    }
    if dir == 0u {
        return vec2(p.z, -p.y);
    }
    if dir == 1u {
        return vec2(-p.z, -p.y);
    }
    if dir == 4u {
        return vec2(-p.x, -p.y);
    }
    return vec2(p.x, -p.y);
}

@vertex
fn vs_lod(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let f = vi / 4u;
    let w0 = records[f * 4u];
    let w1 = records[f * 4u + 1u];
    let w2 = records[f * 4u + 2u];
    let w3 = records[f * 4u + 3u];
    let dir = lod_dir(w0);
    let a = lod_ao(w3);
    var c = vi % 4u;
    if a.x + a.z < a.y + a.w {
        c = (c + 1u) % 4u;
    }
    let local = lod_pos(w0, w2) + lod_corner(w0, w2, w3, CORNERS[dir * 4u + c]) - vec3(0.0, 0.125, 0.0);
    let pos = finish(local, origins[ii]);
    var out: VOut;
    out.clip = scene.view_proj * pos;
    out.dist = length(pos.xyz);
    let o = origins[ii] + g.cam_cube;
    let world = vec3<f32>(o.xyz * 16) + local + vec3(0.0, 0.125, 0.0);
    out.uv = lod_uv(dir, world);
    out.pos = world - vec3(0.0, 0.125, 0.0);
    let tex_id = w1 & FACE_TEX_MASK;
    let fr = texture_frame(tex_id);
    out.size = fr.size;
    out.layer = fr.a;
    out.layer2 = fr.b;
    out.blend = fr.blend;
    out.glow = 0xffffffffu;
    out.slot = 0u;
    out.light = vec4(1.0, lod_light(w0, w2, w3));
    let kind = lod_tint(w0);
    var tint = vec3(1.0);
    if kind != 0u {
        tint = unpack_rgb(biomes[(w1 >> FACE_BIOME_SHIFT) * g.extra.z + kind - 1u]);
    }
    out.tint = tint;
    let shade = SHADE[dir] * AO[a[c]];
    let ov = frames[g.extra.x + tex_id];
    if ov != 0u {
        out.overlay = texture_frame(ov - 1u).a + 1u;
        out.color = vec3(shade);
    } else {
        out.overlay = 0u;
        out.color = tint * shade;
    }
    return out;
}

@fragment
fn fs_lod(in: VOut) -> @location(0) vec4<f32> {
    let du = dpdx(in.uv);
    let dv = dpdy(in.uv);
    let half = i32(g.extra.w);
    let col = vec2<i32>(floor(in.pos.xz / 16.0));
    let d = col - g.cam_cube.xz;
    if abs(d.x) <= half && abs(d.y) <= half {
        let w = 2 * half + 1;
        let i = u32(((col.y % w) + w) % w * w + ((col.x % w) + w) % w);
        if ((lod_mask[i / 32u] >> (i % 32u)) & 1u) != 0u {
            discard;
        }
    }
    let uv = fract(in.uv);
    let nuv = nearest_uv(in.size, uv, du, dv);
    var albedo = with_overlay(in, sample(in, nuv, du, dv), nuv, du, dv);
    let cover = smoothstep(0.1, 0.6, albedo.a);
    let tinted = any(in.tint != vec3(1.0));
    albedo = vec4(mix(select(albedo.rgb, vec3(1.0), tinted), albedo.rgb, cover), 1.0);
    return vec4(shade(in, albedo).rgb, 1.0);
}
