// Billboard (world sprite) pass: screen-aligned quads anchored at the
// entity's feet, textured from the bundle's 8bpp sprite atlas.
//
// Same palette-index color path as terrain: the fragment resolves an
// atlas texel (8-bit palette index; 0 = transparent, exactly the
// original blitter's per-pixel skip) through the colormap
// (palette[shade_lut[shade][index]]) and applies the shared distance
// fog. Pixels stay chunky: integer texel loads, no filtering — the
// billboard is the original sprite scaled, like the engine's affine
// rasterizer.

struct Globals {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    fog_color: vec4<f32>,
    atlas: vec4<u32>,
    // Camera basis for screen-aligned expansion (billboards tilt with
    // pitch like the original's 2D screen blit).
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var t_sprites: texture_2d<u32>;
@group(0) @binding(2) var t_colormap: texture_2d<f32>;
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
    var up = globals.cam_up.xyz;
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
    let world = anchor
        + globals.cam_right.xyz * (c.x * inst.size.x)
        + up * (c.y * inst.size.y);
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
    let texel = vec2<i32>(
        i32(in.uv_pos.x) + min(i32(fx * in.uv_size.x), i32(in.uv_size.x) - 1),
        i32(in.uv_pos.y) + min(i32(in.frac.y * in.uv_size.y), i32(in.uv_size.y) - 1),
    );
    let index = textureLoad(t_sprites, texel, 0).r;
    // Palette index 0 = transparent (the original skips zero pixels).
    if index == 0u {
        discard;
    }
    let shade = i32(min(in.flags.y, 63u));
    let base = textureLoad(t_colormap, vec2<i32>(i32(index), shade), 0).rgb;

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
    // fixed extinction band. (Round 2: round 1 discarded at full fog,
    // which popped sprites in/out of existence at the wall where
    // terrain kept fading — player report 2026-08-08.)
    let ext = smoothstep(EXT_START, EXT_END, dist);
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
    var out: FsOut;
    out.color = vec4<f32>(rgb, in.alpha);
    out.depth = in.anchor_depth;
    return out;
}

const TAU: f32 = 6.283185307179586;
// The extinction band — MUST match terrain.wgsl's EXT_START/EXT_END.
const EXT_START: f32 = 95.0;
const EXT_END: f32 = 125.0;

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
