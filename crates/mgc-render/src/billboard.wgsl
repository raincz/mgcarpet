// Billboard (world sprite) pass: screen-aligned quads anchored at the
// entity's feet, textured from the RGBA sprite atlas.
//
// Same color path as terrain: the fragment resolves an atlas texel
// (alpha 0 = transparent, the original blitter's index-0 skip) through
// the colour grade (grade.rs; the material byte picks the normal or
// glowing table) and applies the shared distance fog. Pixels stay
// chunky by default: integer texel loads — the billboard is the
// original sprite scaled, like the engine's affine rasterizer; the
// smooth-textures enhancement filters them instead.

struct Globals {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    fog_color: vec4<f32>,
    atlas: vec4<u32>,
    // Rolled camera basis (unused here; layout prefix only).
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    // Pre-bank basis for quad expansion: billboards tilt with pitch
    // like the original's 2D screen blit but stay upright over the
    // terrain when the carpet banks (retail counter-rotates the
    // sprite rasterizer by -roll, SetBillboards_3B560).
    bb_right: vec4<f32>,
    bb_up: vec4<f32>,
    // Terrain-only fields, declared to reach `grade`.
    viewport: vec4<f32>,
    lights: array<vec4<f32>, 16>,
    // x = greyscale amount (retail's black-and-white palette mode),
    // y = smooth-texture SHARPNESS (0 = off, retail's chunky texels;
    // 1 = plain bilinear; s = upscale s× nearest, then bilinear).
    grade: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
// RGBA sprite atlas (8-bit channels) + its per-texel material byte.
@group(0) @binding(1) var t_sprites: texture_2d<u32>;
@group(0) @binding(4) var t_sprite_mat: texture_2d<u32>;
// The colour grade: layer 0 = normal, 1 = glowing texels.
@group(0) @binding(2) var t_grade: texture_2d_array<f32>;
@group(0) @binding(3) var s_grade: sampler;
// Group 1 is the terrain pass's mirror+sky group (bindings 0/1 are
// the mirror slots this shader never reads); the sky slots feed the
// same fog/extinction melts as terrain.wgsl. A 1x1 fog-constant
// texel when no sky is loaded.
@group(1) @binding(2) var t_sky: texture_2d<f32>;
@group(1) @binding(3) var s_sky: sampler;

struct Instance {
    // Feet-center world position (wrap-adjusted near the camera).
    @location(0) pos: vec3<f32>,
    // World-space quad size.
    @location(1) size: vec2<f32>,
    // Frame rect in atlas texels.
    @location(2) uv_pos: vec2<f32>,
    @location(3) uv_size: vec2<f32>,
    // x = horizontal mirror flag, y = shade LUT row.
    @location(4) flags: vec2<u32>,
    // Opacity: 1.0 opaque; 1/3 (smoke) / 2/3 (glows) for the retail
    // translucency raster modes. Only takes effect on the blend
    // pipeline (the opaque pipeline has blending disabled).
    @location(5) alpha: f32,
    // Retail co-tile paint order in (0, 1): the sprite's place in its
    // tile's entity chain, head->tail. Higher = later in retail's walk
    // = ON TOP. See `Billboard::chain_depth`.
    @location(6) chain: f32,
    // Screen-plane bank, radians (positive = right edge down), about
    // the quad's centre. 0 for every retail sprite.
    @location(7) roll: f32,
    // Segment-order painter key, plan distance in tiles (a multipart
    // body's member under the enhancement); negative = the anchor
    // tile key + chain rank below.
    @location(8) key: f32,
    // Where the entity stands in the quad, fractions from the left /
    // bottom edge (the impostor image's anchor, already mirrored with
    // a mirrored cell). Retail = (0.5, 0).
    @location(9) anchor: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) frac: vec2<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) uv_pos: vec2<f32>,
    @location(3) @interpolate(flat) uv_size: vec2<f32>,
    @location(4) @interpolate(flat) flags: vec2<u32>,
    // The sprite's painter-order depth, written for every fragment.
    // The depth channel carries HORIZONTAL camera distance (see
    // terrain.wgsl): the sprite is keyed to its anchor TILE's plan
    // distance minus half a tile — the original's "blit the sprite
    // right after its own tile's triangles" (sub_main.cpp :33673).
    // Walls the sprite stands against are farther tiles → never clip
    // it; tiles in front always hide it; ridge silhouettes still
    // occlude partially because the terrain side varies per pixel.
    @location(5) @interpolate(flat) anchor_depth: f32,
    @location(6) @interpolate(flat) alpha: f32,
};

const DEPTH_RANGE: f32 = 768.0;
// The co-tile chain nudge, in normalized depth. One tile of plan
// distance is 1/DEPTH_RANGE, so a quarter of that keeps every chain
// rank strictly inside its own tile's slot: co-tile sprites separate,
// nothing crosses a tile boundary. The chain rank itself is already
// normalized to (0, 1) over the tile's own members, so an arbitrarily
// crowded tile still fits (see `LivePose::chain_depth`).
const CHAIN_BIAS: f32 = 0.25 / DEPTH_RANGE;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, inst: Instance) -> VsOut {
    // Two triangles: corner x in {-0.5, 0.5}, y in {0 = feet, 1 = top}.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 0.0), vec2<f32>(0.5, 1.0),
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 1.0), vec2<f32>(-0.5, 1.0),
    );
    let c = corners[vid];
    var anchor = inst.pos;
    var up = globals.bb_up.xyz;
    // The water-reflection MIRROR pass (atlas.w = 2): the sprite's
    // reflection hangs upside-down below the water — flip the ANCHOR
    // about the sea plane and expand DOWN the real camera's up axis.
    // Flipping the finished quad instead would counter-tilt its plane
    // by 2x the camera pitch (edge-on at a 45° look-down): a camera-
    // facing billboard seen through a mirror no longer faces the
    // camera. Expanding in the true screen basis keeps the reflection
    // full-body at any pitch — and leaves the atlas frame and the
    // flags.x fold untouched, so the figure never reads as wrongly
    // rotated. The downward run still mirrors the image vertically
    // for free.
    if globals.atlas.w == 2u {
        anchor.y = -anchor.y;
        up = -up;
    }
    // The quad corner in the billboard plane, banked about the quad's
    // centre (a flat carpet rolls about its own middle, not its feet),
    // the centre placed off the entity by the picture's anchor (retail
    // = feet-centre (0.5, 0): centre straight above, half the height
    // up — the same arithmetic as before anchors existed).
    let ox = c.x * inst.size.x;
    let oy = c.y * inst.size.y - 0.5 * inst.size.y;
    let cr = cos(inst.roll);
    let sr = sin(inst.roll);
    let rx = ox * cr + oy * sr + (0.5 - inst.anchor.x) * inst.size.x;
    let ry = -ox * sr + oy * cr + (0.5 - inst.anchor.y) * inst.size.y;
    let world = anchor
        + globals.bb_right.xyz * rx
        + up * ry;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.frac = vec2<f32>(c.x + 0.5, 1.0 - c.y);
    out.world = world;
    out.uv_pos = inst.uv_pos;
    out.uv_size = inst.uv_size;
    out.flags = inst.flags;
    out.alpha = inst.alpha;
    let tile_center = floor(inst.pos.xz) + vec2<f32>(0.5, 0.5);
    // ⭐ THE CO-TILE TIEBREAK. Keying depth to the anchor tile makes
    // two sprites on one tile bit-identical, and the opaque pipeline
    // then resolves them by submission = pool-allocation luck. Retail
    // had no z-buffer at all: it walked the tile's entity chain
    // head->tail and painted in that order, so the LAST member walked
    // covers the rest. Pull the depth forward by the chain rank to
    // reproduce it. CHAIN_BIAS is a fraction of ONE tile's depth
    // quantum, so a co-tile pair can never cross its own tile
    // boundary and no cross-tile ordering moves.
    out.anchor_depth = clamp(
        (length(tile_center - globals.camera.xz) - 0.5) / DEPTH_RANGE
            - inst.chain * CHAIN_BIAS,
        0.0,
        0.999999,
    );
    if inst.key >= 0.0 {
        out.anchor_depth = clamp(inst.key / DEPTH_RANGE, 0.0, 0.999999);
    }
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    var fx = in.frac.x;
    if in.flags.x != 0u {
        fx = 1.0 - fx;
    }
    let lo = vec2<i32>(in.uv_pos);
    let hi = lo + vec2<i32>(in.uv_size) - vec2<i32>(1);
    let texel = vec2<i32>(
        i32(in.uv_pos.x) + min(i32(fx * in.uv_size.x), i32(in.uv_size.x) - 1),
        i32(in.uv_pos.y) + min(i32(in.frac.y * in.uv_size.y), i32(in.uv_size.y) - 1),
    );
    var src: vec3<f32>;
    if globals.grade.y > 0.0 {
        // SMOOTH TEXTURES (enhancement): bilinear over the frame's
        // own texels (clamped to its rect, so atlas neighbours never
        // bleed in), alpha-weighted so transparent texels lend no
        // colour; the silhouette is the half-coverage contour.
        let t = vec2<f32>(fx, in.frac.y) * in.uv_size - vec2<f32>(0.5);
        let i0 = vec2<i32>(floor(t));
        // Sharpened weights: each texel keeps a solid core and blends
        // only in a 1/(2s)-texel band at its border (= upscale s× with
        // nearest, then bilinear) — full bilinear smudged 1-px detail
        // (player review 2026-10-05).
        let f = clamp((t - floor(t) - 0.5) * globals.grade.y + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
        var acc = vec3<f32>(0.0);
        var cov = 0.0;
        for (var k = 0; k < 4; k = k + 1) {
            let d = vec2<i32>(k & 1, k >> 1u);
            let at = clamp(lo + i0 + d, lo, hi);
            let w = select(1.0 - f.x, f.x, d.x == 1) * select(1.0 - f.y, f.y, d.y == 1);
            let c = textureLoad(t_sprites, at, 0);
            let a = w * f32(c.a) / 255.0;
            acc += a * vec3<f32>(c.rgb);
            cov += a;
        }
        if cov < 0.5 {
            discard;
        }
        src = acc / (cov * 255.0);
    } else {
        let tx = textureLoad(t_sprites, texel, 0);
        // Alpha 0 = transparent (the original skips index-0 pixels).
        if tx.a == 0u {
            discard;
        }
        src = vec3<f32>(tx.rgb) / 255.0;
    }
    let shade = i32(min(in.flags.y, 63u));
    // The material (normal / glowing table) is the nearest texel's.
    let layer = i32(textureLoad(t_sprite_mat, texel, 0).r & 1u);
    let base = grade_linear(src, shade, layer);

    let dist = distance(in.world, globals.camera.xyz);
    // Distance fog, the retail band law (see terrain.wgsl fog_amount):
    // linear in squared distance across 0.75·D..0.95·D, D = camera.w
    // tiles (0 = off). Retail fogs sprites on the same ramp as
    // terrain (GRO:3499-3511).
    var fog = 0.0;
    let d = globals.camera.w;
    if d > 0.0 {
        let start2 = 0.5625 * d * d;
        let end2 = 0.9025 * d * d;
        fog = clamp((dist * dist - start2) / (end2 - start2), 0.0, 1.0);
    }
    // Sprites follow the terrain's silhouette law exactly: past the
    // fog wall they linger as fog-colored cutouts like the landscape
    // they stand on, then dissolve into the sky pixel across the same
    // extinction band (the renderer's melt_band, in bb_right.w /
    // bb_up.w). (Round 2: round 1 discarded at full fog, which popped
    // sprites in/out of existence at the wall where terrain kept
    // fading — player report 2026-08-08.)
    let ext = smoothstep(globals.bb_right.w, globals.bb_up.w, dist);
    if ext >= 1.0 || (globals.atlas.w == 2u && fog >= 1.0) {
        discard;
    }
    // MIRROR arm: fade toward the mirrored sky pixel, exactly like
    // mirrored terrain. Toward the flat constant, a reflected sprite
    // kept its full-contrast cutout against the bright reflected
    // cloud band — mobs and castle flags read clearly in the water
    // well before they resolved in the direct view (player report
    // 2026-08-08).
    var fog_target = globals.fog_color.rgb;
    if globals.atlas.w == 2u && fog > 0.0 {
        fog_target = sky_backdrop(in.world);
    }
    var rgb = mix(base, fog_target, fog);
    if ext > 0.0 {
        rgb = mix(rgb, sky_backdrop(in.world), ext);
    }
    // THE SPRITE FADE (lib.rs `sprite_fade_band`, enhancement):
    // past the fog wall the sprite also turns translucent, opacity
    // 1 → 0 from 0.95·D (fog off: the melt start) to the melt's end,
    // so the backdrop shows through instead of a full-contrast
    // fog-coloured cutout. Only the blend pipeline honours it; the
    // renderer routes every sprite reaching past the start there.
    var fade_start = globals.bb_right.w;
    if d > 0.0 {
        fade_start = 0.95 * d;
    }
    let opacity = 1.0 - smoothstep(fade_start, globals.bb_up.w, dist);
    var out: FsOut;
    out.color = vec4<f32>(rgb, in.alpha * opacity);
    // The anchor tile's painter depth, unless this fragment's OWN plan
    // distance is nearer. The quad spans sideways along the camera's
    // right axis, so a sprite off to one side reaches back across the
    // view line and its inner fragments sit plan-NEARER than its tile.
    // Keyed to the anchor alone, every terrain pixel behind them with
    // plan distance below the anchor's beat the sprite: a disc (centred
    // under the camera, radius = anchor depth) punched through its
    // middle — glaring under a steep look-down at a wide sprite passing
    // below the carpet (castle flag, player report 2026-09-25). Along a
    // descending ray plan distance only grows, so the fragment's own
    // plan distance orders it truly against the ground behind it, while
    // ground genuinely in front still wins. Narrow sprites ahead keep
    // the anchor (their fragments never undercut it), so the co-tile
    // chain order and the never-clipped-by-its-own-wall law stand.
    let own = clamp(length(in.world.xz - globals.camera.xz) / DEPTH_RANGE, 0.0, 0.999999);
    out.depth = min(in.anchor_depth, own);
    return out;
}

const TAU: f32 = 6.283185307179586;

// The sky-texture pixel behind a fragment — terrain.wgsl's
// sky_backdrop, duplicated verbatim (same ray law as sky.wgsl,
// including the mirror-arm y negation).
fn sky_backdrop(world: vec3<f32>) -> vec3<f32> {
    var dir = normalize(world - globals.camera.xyz);
    if globals.atlas.w == 2u {
        dir.y = -dir.y;
    }
    let az = atan2(dir.x, -dir.z);
    let el = asin(clamp(dir.y, -1.0, 1.0));
    let scale = 1024.0 / TAU / 256.0; // texture wraps per radian
    return textureSampleLevel(
        t_sky,
        s_sky,
        vec2<f32>(az * scale, 1.0 - el * scale),
        0.0,
    ).rgb;
}

// The colour grade — terrain.wgsl's grade_srgb/grade_linear,
// duplicated verbatim. Retail's shade remap as an RGB function
// of the texel colour (sRGB, 0..1) at shade `row`, in `layer` (0 =
// normal, 1 = glowing). The table packs each row's 33³ grid as b-slices
// of 33x33 side by side; r/g interpolate in hardware, b by hand. Every
// sample stays inside its own slice/row block (texel centres 0..32),
// so the bilinear footprint never reaches a neighbour.
const GRADE_N: f32 = 33.0;
const GRADE_W: f32 = 1089.0;
const GRADE_H: f32 = 2112.0;

fn grade_srgb(rgb: vec3<f32>, row: i32, layer: i32) -> vec3<f32> {
    let u = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * (GRADE_N - 1.0);
    let b0 = min(floor(u.b), GRADE_N - 2.0);
    let y = (f32(row) * GRADE_N + u.g + 0.5) / GRADE_H;
    let x0 = (b0 * GRADE_N + u.r + 0.5) / GRADE_W;
    let s0 = textureSampleLevel(t_grade, s_grade, vec2<f32>(x0, y), layer, 0.0).rgb;
    let s1 = textureSampleLevel(t_grade, s_grade, vec2<f32>(x0 + GRADE_N / GRADE_W, y), layer, 0.0).rgb;
    return mix(s0, s1, u.b - b0);
}

// Graded colour, greyscale-mixed in sRGB (retail's `(r+g+b)/3` palette
// mode), returned linear for the blend/fog math.
fn grade_linear(rgb: vec3<f32>, row: i32, layer: i32) -> vec3<f32> {
    var c = grade_srgb(rgb, row, layer);
    let g = globals.grade.x;
    if g > 0.0 {
        c = mix(c, vec3<f32>((c.r + c.g + c.b) / 3.0), g);
    }
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
