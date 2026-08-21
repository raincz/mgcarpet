// PROTOTYPE fire-particle pass (throwaway — augments the fireball).
//
// Screen-aligned soft flame discs, drawn with PREMULTIPLIED-alpha
// blending (One / OneMinusSrcAlpha): a hot particle carries alpha≈0
// so it ADDS to the scene (glowing core, overlapping fireballs merge
// their light), a cool/sooty particle carries alpha>0 so it occludes.
// Colour comes from an analytic fire ramp (heat 0..1). No texture.
//
// The app feeds a cloud of these per frame (stateless motion law:
// trail = head - velocity * age); the shape/flicker below is keyed by
// the per-particle `seed` (the app rolls a time term into it so the
// dapple shimmers frame to frame).

struct Globals {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    fog_color: vec4<f32>,
    atlas: vec4<u32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    // Pre-bank basis (see billboard.wgsl) — flames stand on the
    // terrain, not the rolled viewport.
    bb_right: vec4<f32>,
    bb_up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct Instance {
    // World-space CENTER (wrap-adjusted near the camera).
    @location(0) pos: vec3<f32>,
    // World half-extents (x = width, y = height; flames run taller).
    @location(1) size: vec2<f32>,
    // 0..1 heat (1 = white-hot core, 0 = dark ember / soot).
    @location(2) heat: f32,
    // 0..1 coverage / opacity multiplier.
    @location(3) alpha: f32,
    // Per-particle procedural phase (dapple + flicker seed).
    @location(4) seed: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    // Local quad coord in [-1, 1].
    @location(0) local: vec2<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) heat: f32,
    @location(3) @interpolate(flat) alpha: f32,
    @location(4) @interpolate(flat) seed: f32,
    @location(5) @interpolate(flat) depth: f32,
    // Fog computed at the quad CORNERS and interpolated, instead of per
    // fragment — with this much overdraw the fragment shader runs on far
    // more samples than there are vertices, so hoisting the distance/fog
    // math to the vertex stage is a real fill-rate win. Interpolated (not
    // flat) so a big quad straddling the fog wall still fades its near
    // half correctly rather than vanishing whole.
    @location(6) fog: f32,
};

const DEPTH_RANGE: f32 = 768.0;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, inst: Instance) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vid];
    var anchor = inst.pos;
    var up = globals.bb_up.xyz;
    // Water-reflection MIRROR pass (atlas.w = 2): the flame hangs
    // upside-down under the water — flip the ANCHOR about the sea
    // plane and expand DOWN the real camera's up axis (flipping the
    // finished quad would counter-tilt it with pitch and collapse it
    // edge-on at a 45° look-down; see billboard.wgsl).
    if globals.atlas.w == 2u {
        anchor.y = -anchor.y;
        up = -up;
    }
    let world = anchor
        + globals.bb_right.xyz * (c.x * inst.size.x)
        + up * (c.y * inst.size.y);
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.local = c;
    out.world = world;
    out.heat = inst.heat;
    out.alpha = inst.alpha;
    out.seed = inst.seed;
    // Plan-distance depth biased ~1.5 tiles toward the camera so the
    // flame wins the depth test against the OPAQUE fireball sprite
    // sitting at the same tile (it would otherwise cut the glow like a
    // sticker), while terrain genuinely in front still occludes.
    out.depth = clamp(
        (length(inst.pos.xz - globals.camera.xz) - 1.5) / DEPTH_RANGE,
        0.0,
        0.999999,
    );
    // Distance fog (retail band law), evaluated at this corner and
    // interpolated across the quad.
    out.fog = 0.0;
    let d = globals.camera.w;
    if d > 0.0 {
        let dv = world - globals.camera.xyz;
        let dist2 = dot(dv, dv);
        let start2 = 0.5625 * d * d;
        let end2 = 0.9025 * d * d;
        out.fog = clamp((dist2 - start2) / (end2 - start2), 0.0, 1.0);
    }
    return out;
}

// Analytic fire ramp: dark ember -> red -> orange -> warm yellow.
// The blue channel is kept LOW even at the hot end: additive stacking
// saturates the red first, so a low blue keeps merged cores orange
// instead of blowing out to white.
fn fire_ramp(h: f32) -> vec3<f32> {
    let ember = vec3<f32>(0.35, 0.03, 0.0);
    let red = vec3<f32>(0.85, 0.12, 0.02);
    let orange = vec3<f32>(1.0, 0.45, 0.05);
    let hot = vec3<f32>(1.0, 0.80, 0.30);
    var col = mix(ember, red, smoothstep(0.0, 0.35, h));
    col = mix(col, orange, smoothstep(0.30, 0.68, h));
    col = mix(col, hot, smoothstep(0.68, 1.0, h));
    return col;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    // PROTOTYPE shockwave (heat sentinel < 0): a soft, cool vapor band —
    // a faintly blue-white pressure haze that lightens the scene, not
    // flame. Part-additive (alpha < coverage) so it reads as compressed
    // air/dust rather than an opaque disc.
    if in.heat < 0.0 {
        let rad = length(in.local);
        let core = smoothstep(1.0, 0.12, rad);
        let a = core * in.alpha * (1.0 - in.fog);
        if a <= 0.003 {
            discard;
        }
        // A medium, faintly-cool GREY — lighter than the soot smoke, so it
        // reads as compressed dusty air, not more smoke.
        let col = vec3<f32>(0.60, 0.61, 0.64);
        var so: FsOut;
        so.color = vec4<f32>(col * a, a);
        so.depth = in.depth;
        return so;
    }
    let p = in.local;
    let s = in.seed;
    // Domain-warp the radius with scrolling sines so the disc breaks
    // into ragged licks instead of a clean circle. The warp rides
    // `seed` (time-rolled by the app) to shimmer. Kept modest so it
    // cannot punch the shape out to the quad edge.
    let warp = 0.14 * sin(p.x * 6.5 + s * 6.2831)
        + 0.10 * sin(p.y * 5.0 - s * 4.3 + 1.7)
        + 0.06 * sin((p.x + p.y) * 9.0 + s * 8.1);
    let rad = length(p);
    let r = rad + warp;
    // Body coverage — a firm core with a soft rim.
    let core = smoothstep(1.0, 0.20, r);
    // Hard containment: a window that reaches ZERO before the quad
    // boundary in EVERY direction (kills the sharp top edge the squished
    // shape used to clip against, and keeps the flame inside its tile).
    let win = 1.0 - smoothstep(0.62, 0.98, rad);
    let flick = 0.82 + 0.18 * sin(s * 24.0);
    let cover = core * win * in.alpha * flick;
    if cover < 0.01 {
        discard;
    }
    // Hotter toward the center; the rim cools to red.
    let heat = clamp(in.heat * (0.35 + 0.75 * core), 0.0, 1.0);
    // Smokiness is keyed on the PER-PARTICLE heat (not the rim-cooled
    // value), so only genuinely cool tail particles turn to soot while
    // hot heads keep fiery rims. A cool particle reads as grey smoke.
    let smokiness = 1.0 - smoothstep(0.12, 0.40, in.heat);
    let smoke = vec3<f32>(0.11, 0.10, 0.095);
    let col = mix(fire_ramp(heat), smoke, smokiness);

    // Distance fog (computed per particle in the vertex stage).
    let vis = 1.0 - in.fog;

    // Two-part premultiplied model (avoids the additive-over-bright-sky
    // wash-out): an OCCLUDING body that replaces the sky, plus a
    // heat-gated ADDITIVE glow only in the hot cores.
    //   result = rgb + dst*(1 - a)
    // body:  rgb += col*a (normal over)   glow: rgb += hot*g (additive)
    // Smoke occludes a touch more softly than flame, but not as thin as
    // before — with fewer overlapping soot puffs it needs the extra body
    // to read as a filled crater rather than a sparse haze.
    let body_a = cover * mix(0.85, 0.66, smokiness) * vis;
    let glow = pow(clamp(in.heat, 0.0, 1.0), 2.5) * core * in.alpha * 0.9 * vis;
    let hot = fire_ramp(min(heat + 0.2, 1.0));
    var out: FsOut;
    out.color = vec4<f32>(col * body_a + hot * glow, body_a);
    out.depth = in.depth;
    return out;
}
