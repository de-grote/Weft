// What cubes get drawn is decided here. `table` has every cube of the world with something opaque
// or cutout to draw; `cull` writes the draws of those in view and not behind what the last frame
// drew (distant terrain's tiles too, by their boxes). `depths` holds that frame's depth and under it ever smaller copies, each texel the farthest
// of those it covers. `test` does the same for the draws the CPU made, by zeroing their instances.
// The prelude (`gpu_layout::occlusion_prelude`) declares the bindings and `args`.

// The depth buffer has 1 at the near plane and 0 at infinity: farther is less.
@compute @workgroup_size(16, 16)
fn down(@builtin(global_invocation_id) id: vec3<u32>) {
    let s = args.size;
    let d = (s + vec2<u32>(1u)) / 2u;
    if id.x >= d.x || id.y >= d.y {
        return;
    }
    let lo = id.xy * 2u;
    let hi = min(lo + vec2<u32>(1u), s - vec2<u32>(1u));
    let far = min(
        min(depths[args.src + lo.y * s.x + lo.x], depths[args.src + lo.y * s.x + hi.x]),
        min(depths[args.src + hi.y * s.x + lo.x], depths[args.src + hi.y * s.x + hi.x]),
    );
    depths[args.dst + id.y * d.x + id.x] = far;
}

// Where `level` starts in `depths`, and how big it is.
fn level_of(level: u32) -> vec3<u32> {
    var s = view.size;
    var at = 0u;
    for (var l = 0u; l < level; l++) {
        at += s.x * s.y;
        s = (s + vec2<u32>(1u)) / 2u;
    }
    return vec3<u32>(at, s);
}

// All of the box from `lo` to `hi`, relative to this frame's camera, behind the last frame's depth.
fn hidden(cube_lo: vec3<f32>, cube_hi: vec3<f32>) -> bool {
    if view.hiz == 0u {
        return false;
    }
    let grow = vec3<f32>(view.moved.w);
    let lo = cube_lo + view.moved.xyz - grow;
    let hi = cube_hi + view.moved.xyz + grow;
    var small = vec2<f32>(1e9);
    var large = vec2<f32>(-1e9);
    var nearest = 0.0;
    for (var c = 0u; c < 8u; c++) {
        let p = vec3<f32>(
            select(lo.x, hi.x, (c & 1u) != 0u),
            select(lo.y, hi.y, (c & 2u) != 0u),
            select(lo.z, hi.z, (c & 4u) != 0u),
        );
        let clip = view.prev * vec4<f32>(p, 1.0);
        // Reaches behind that camera: there is nothing to go by.
        if clip.w < 0.05 {
            return false;
        }
        let ndc = clip.xyz / clip.w;
        small = min(small, ndc.xy);
        large = max(large, ndc.xy);
        nearest = max(nearest, ndc.z);
    }
    // Any of it off that frame's screen: what is there wasn't seen.
    if small.x < -1.0 || small.y < -1.0 || large.x > 1.0 || large.y > 1.0 {
        return false;
    }
    let screen = vec2<f32>(view.size);
    let a = vec2<f32>(small.x + 1.0, 1.0 - large.y) * 0.5 * screen;
    let b = vec2<f32>(large.x + 1.0, 1.0 - small.y) * 0.5 * screen;
    let level = min(u32(ceil(log2(max(max(b.x - a.x, b.y - a.y), 1.0)))), view.levels - 1u);
    let info = level_of(level);
    let top = vec2<u32>(screen) - vec2<u32>(1u);
    let from_ = min(vec2<u32>(a), top) >> vec2<u32>(level);
    let to = min(vec2<u32>(b), top) >> vec2<u32>(level);
    var far = 1.0;
    for (var y = from_.y; y <= to.y; y++) {
        for (var x = from_.x; x <= to.x; x++) {
            far = min(far, depths[info.x + y * info.y + x]);
        }
    }
    return nearest < far;
}

@compute @workgroup_size(256)
fn test(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= args.count {
        return;
    }
    // Distant terrain isn't in cubes: its tiles' boxes follow the draws' origins, two corners each.
    if i >= view.skip.x && i < view.skip.y {
        let at = args.count + 2u * (i - view.skip.x);
        let lo = vec3<f32>(origins[at].xyz) - view.cam_frac.xyz;
        let hi = vec3<f32>(origins[at + 1u].xyz) - view.cam_frac.xyz;
        if hidden(lo, hi) {
            draws[i * 5u + 1u] = 0u;
        }
        return;
    }
    let o = origins[i];
    if o.w != 0 {
        return;
    }
    let lo = vec3<f32>(o.xyz) * 16.0 - view.cam_frac.xyz;
    if hidden(lo, lo + vec3<f32>(16.0)) {
        draws[i * 5u + 1u] = 0u;
    }
}

fn put_draw(bucket: u32, rel: vec3<i32>, start: u32, quads: u32) {
    let at = buckets[bucket].base + atomicAdd(&buckets[bucket].count, 1u);
    let d = at * 5u;
    out_draws[d] = quads * 6u;
    out_draws[d + 1u] = 1u;
    out_draws[d + 2u] = 0u;
    out_draws[d + 3u] = start * 4u;
    out_draws[d + 4u] = at;
    out_origins[at] = vec4<i32>(rel, 0);
}

const EMPTY: u32 = 4294967295u;

@compute @workgroup_size(256)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= args.count {
        return;
    }
    let e = table[id.x];
    if e.bucket == EMPTY {
        return;
    }
    let rel = e.cube - view.cam_cube.xyz;
    let lo = vec3<f32>(rel) * 16.0 - view.cam_frac.xyz;
    let center = vec4<f32>(lo + vec3<f32>(8.0), 1.0);
    // A cube's half diagonal.
    for (var p = 0u; p < 4u; p++) {
        if dot(view.planes[p], center) <= -13.86 {
            return;
        }
    }
    if hidden(lo, lo + vec3<f32>(16.0)) {
        return;
    }
    if (e.bucket & 4u) != 0u {
        put_draw(e.bucket, rel, e.start, e.counts[0]);
        atomicAdd(&faces_drawn.count, e.counts[0]);
        return;
    }
    // Only the sides that face the camera, each run of neighbours one draw.
    var seen = array<bool, 6>((lo.x < 0.0), (lo.y < 0.0), (lo.z < 0.0), (lo.x > -16.0), (lo.y > -16.0), (lo.z > -16.0));
    var at = e.start;
    var run = 0u;
    var total = 0u;
    for (var rank = 0u; rank < 6u; rank++) {
        let n = (table[id.x].counts[rank / 2u] >> ((rank % 2u) * 16u)) & 65535u;
        if n == 0u {
            continue;
        }
        if seen[rank] {
            run += n;
            total += n;
        } else if run > 0u {
            put_draw(e.bucket, rel, at, run);
            at += run;
            run = 0u;
            at += n;
        } else {
            at += n;
        }
    }
    if run > 0u {
        put_draw(e.bucket, rel, at, run);
    }
    atomicAdd(&faces_drawn.count, total);
}
