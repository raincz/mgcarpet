// Health-bar overlay (unfaithful debug enhancement): screen-aligned
// solid-color quads above monsters — the classic red-fill-on-black
// rectangle showing remaining life. No texture, no fog shading, but
// CUT at the fog wall — an overlay must not reveal monsters the fog
// hides (player directive 2026-07-16).

struct Globals {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    fog_color: vec4<f32>,
    atlas: vec4<u32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    // Pre-bank basis (see billboard.wgsl) — bars stay level over the
    // terrain when the carpet banks.
    bb_right: vec4<f32>,
    bb_up: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct Instance {
    // Bar bottom-center world position (wrap-adjusted near the camera).
    @location(0) pos: vec3<f32>,
    // World-space bar size (width, height).
    @location(1) size: vec2<f32>,
    // Remaining life fraction 0..=1.
    @location(2) frac: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) fx: vec2<f32>,
    @location(1) @interpolate(flat) frac: f32,
    // Painter-order depth, like the billboard pass (see
    // terrain.wgsl): the bar rides its creature's tile key.
    @location(2) @interpolate(flat) anchor_depth: f32,
};

const DEPTH_RANGE: f32 = 768.0;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, inst: Instance) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 0.0), vec2<f32>(0.5, 1.0),
        vec2<f32>(-0.5, 0.0), vec2<f32>(0.5, 1.0), vec2<f32>(-0.5, 1.0),
    );
    let c = corners[vid];
    let world = inst.pos
        + globals.bb_right.xyz * (c.x * inst.size.x)
        + globals.bb_up.xyz * (c.y * inst.size.y);
    var out: VsOut;
    // The fog-wall cut (camera.w = fog view distance in tiles, 0 =
    // fog off; terrain fully occludes at 0.95·D — terrain.wgsl
    // fog_amount): collapse the quad behind the near plane.
    let fog_d = globals.camera.w;
    if fog_d > 0.0 && distance(inst.pos, globals.camera.xyz) > 0.95 * fog_d {
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        out.fx = vec2<f32>(0.0, 0.0);
        out.frac = 0.0;
        out.anchor_depth = 0.999999;
        return out;
    }
    out.clip = globals.view_proj * vec4<f32>(world, 1.0);
    out.fx = vec2<f32>(c.x + 0.5, c.y);
    out.frac = inst.frac;
    let tile_center = floor(inst.pos.xz) + vec2<f32>(0.5, 0.5);
    out.anchor_depth = clamp(
        (length(tile_center - globals.camera.xz) - 0.5) / DEPTH_RANGE,
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
    // A thin black border all around; red fill from the left up to
    // the life fraction, black beyond it.
    var out: FsOut;
    out.depth = in.anchor_depth;
    let border = in.fx.x < 0.03 || in.fx.x > 0.97 || in.fx.y < 0.12 || in.fx.y > 0.88;
    if !border && in.fx.x < in.frac {
        out.color = vec4<f32>(0.75, 0.05, 0.05, 1.0);
        return out;
    }
    out.color = vec4<f32>(0.02, 0.02, 0.02, 1.0);
    return out;
}
