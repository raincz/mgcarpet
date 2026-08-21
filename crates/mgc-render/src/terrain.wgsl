// Terrain pass: palette-indexed terrain textures (or flat tile colors),
// per-vertex hillshade, distance fog toward the horizon color.
//
// Color path stays index-based to the end (README design): the fragment
// shader resolves an 8-bit palette index — an atlas texel when the
// level has a terrain-texture atlas, else the tile's flat color from
// the tile-colors LUT — then feeds it through the engine's shade remap
// composed with the palette (t_colormap, sRGB texture):
//   rgb = palette[shade_lut[shade][index]]
// exactly the original's textured-terrain inner loop (remc2
// GameRenderOriginal "mode 7": shade_lut[shade*256 + texel]).

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz = camera position (tile units), w = fog view distance in
    // tiles (full occlusion at 0.95·w, band from 0.75·w — the retail
    // 15..19-tile ramp scaled; 0 = fog off)
    camera: vec4<f32>,
    // rgb = fog/sky color (linear), a = water animation turn (the
    // game's per-tick counter, fractional for render interpolation)
    fog_color: vec4<f32>,
    // x = atlas cell count (0 = untextured),
    // y = smooth shading (1 = interpolate the per-tile shade level
    //     across tile centers instead of the original's per-tile snap),
    // z = water wave rule (0 = off, 1 = MC1, 2 = MC2),
    // w = pass arm: 0 = normal, 1 = the MC2 cave ceiling draw
    //     (t_height carries the CEILING bytes, texture fixed to the
    //     wall cell, water animation off), 2 = the water-reflection
    //     MIRROR draw (terrain y-flipped about the sea plane)
    atlas: vec4<u32>,
    // Camera bases (billboard/sky consumers) — unused here, declared
    // to keep the buffer layout aligned with the Rust Globals struct.
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    bb_right: vec4<f32>,
    bb_up: vec4<f32>,
    // xy = framebuffer size (px); z = 1 when this pass may sample the
    // mirror texture for sea reflections (0 in the mirror pass and
    // with reflections off); w = dynamic light count.
    viewport: vec4<f32>,
    // Dynamic point lights: xyz = world pos (tiles), w = intensity
    // (1 = retail's 128 spell baseline). Night/Cave only (app gate).
    lights: array<vec4<f32>, 16>,
};

// The mirror texture (last mirror pass's output) for sea reflections;
// a 1x1 dummy when viewport.z = 0.
@group(1) @binding(0) var t_mirror: texture_2d<f32>;
@group(1) @binding(1) var s_mirror: sampler;
// The parallax sky bitmap — the melt target for the mirror-arm fog
// and the extinction ramp (the pixel a vanishing fragment must match
// is the sky pass's, never the flat constant). A 1x1 texel of the
// fog constant when no sky is loaded (caves, sky off), degenerating
// every melt to the plain constant fade.
@group(1) @binding(2) var t_sky: texture_2d<f32>;
@group(1) @binding(3) var s_sky: sampler;

// Atlas geometry: 256 px wide, 32x32 cells, 8 per row (BLK*-1.DAT).
const ATLAS_CELL: i32 = 32;
const ATLAS_CELLS_PER_ROW: i32 = 8;

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var t_type: texture_2d<u32>;
// Per-tile light level (the generator's shading array), same layout.
@group(0) @binding(2) var t_shade: texture_2d<u32>;
// Colormap: x = palette index (texel or flat color), y = shade level;
// palette[shade_lut[shade][x]] composed on the CPU.
@group(0) @binding(3) var t_colormap: texture_2d<f32>;
// Terrain type -> flat base palette index (tile-colors.bin), 256x1.
@group(0) @binding(4) var t_tile_colors: texture_2d<u32>;
// Terrain-texture atlas, 8-bit palette indices; 1x1 dummy when absent.
@group(0) @binding(5) var t_atlas: texture_2d<u32>;
// Per-tile angle/flags byte; bits 4-6 = texture UV orientation.
@group(0) @binding(6) var t_angle: texture_2d<u32>;
// Height bytes (1 = 1/8 tile), sampled per grid corner in the vertex
// stage — heights live here (not in the vertex buffer) so runtime
// terrain mutation (craters, quakes) is a texture update.
@group(0) @binding(7) var t_height: texture_2d<u32>;
// Baked shore-distance field for the mirror blend's shore haze:
// SHORE_RES texels per tile, R8Unorm = rect-distance to the nearest
// non-deep-water tile / SHORE_MAX, saturated. The distance law itself
// (the old in-shader 7x7 tile kernel) is baked on the CPU at terrain
// upload — 49 textureLoads per water fragment inflated this shader's
// register footprint enough to halve the framerate on every terrain
// fragment, water on screen or not.
@group(0) @binding(8) var t_shore: texture_2d<f32>;

// Shore-field geometry; must match the CPU bake (lib.rs).
const SHORE_RES: f32 = 4.0;
const SHORE_MAX: f32 = 2.5;

struct VsIn {
    @builtin(instance_index) instance: u32,
    @location(0) pos: vec3<f32>,
    @location(1) light: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) light: f32,
    // Water-shimmer shade offset in LUT rows, interpolated across the
    // triangle exactly like the original's per-corner pnt5_32.
    @location(2) shade_wave: f32,
    // PRE-WAVE terrain height: the mirror blend's slope fade reads the
    // authored surface tilt from screen-space derivatives, and the wave
    // swell (up to ~14 degrees) must not flicker flat sea out of it.
    @location(3) flat_y: f32,
};

const TAU: f32 = 6.283185307179586;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    // The world is a 256x256 torus: draw a 3x3 grid of copies so the
    // horizon is seamless whichever way the camera flies. The fragment
    // tile lookup wraps by modulo, so copies shade identically.
    let wrap = vec3<f32>(
        (f32(in.instance % 3u) - 1.0) * 256.0,
        0.0,
        (f32(in.instance / 3u) - 1.0) * 256.0,
    );
    var out: VsOut;
    var pos = in.pos + wrap;
    // Altitude from the height plane (the buffer carries y = 0).
    let hg = vec2<i32>(
        (i32(in.pos.x) % 256 + 256) % 256,
        (i32(in.pos.z) % 256 + 256) % 256,
    );
    pos.y = f32(textureLoad(t_height, hg, 0).r) * 0.125;
    out.flat_y = pos.y;
    out.shade_wave = 0.0;

    // Water surface animation: the original's per-grid-corner sine
    // product (remc1 sub_main.cpp:33955, remc2 GameRenderOriginal:1054):
    //   sinprod = (sin[(y<<7 + turn<<S) & 0x7FF] >> 8)
    //           * (sin[(x<<7 + turn<<S) & 0x7FF] >> 8)
    // on the 2048-entry 16.16 sine table — i.e. amplitude 65536,
    // wavelength 16 tiles, phase advancing turn<<S of 2048 per tick
    // (S = 6 for MC1, 5 for MC2). Gating is per VERTEX cell, so shared
    // corners displace consistently across tiles. The wave repeats
    // every 256 tiles, so the 3x3 torus copies stay seamless.
    if globals.atlas.z != 0u && globals.atlas.w != 1u {
        let g = vec2<i32>(
            (i32(in.pos.x) % 256 + 256) % 256,
            (i32(in.pos.z) % 256 + 256) % 256,
        );
        let periods_per_turn = select(1.0 / 64.0, 1.0 / 32.0, globals.atlas.z == 1u);
        let phase = globals.fog_color.a * periods_per_turn;
        let sinprod = sin(TAU * (f32(g.x) / 16.0 + phase))
            * sin(TAU * (f32(g.y) / 16.0 + phase));
        if globals.atlas.z == 1u {
            // MC1: deep-water corners only (angle bit 3, the
            // generator's open-sea flag): +-1/4 tile swell, +-8 shade
            // rows of shimmer (alt -= sinprod >> 10 in 1/256-tile alt
            // units; pnt5 += 8 * sinprod in 8.16 shade).
            if (textureLoad(t_angle, g, 0).r & 8u) != 0u {
                pos.y -= sinprod * 0.25;
                out.shade_wave = sinprod * 8.0;
            }
        } else {
            // MC2: every water corner (terrain type 0) gets a gentle
            // +-1/32 tile ripple (alt -= sinprod >> 13); the shimmer is
            // skipped where the corner's shade level is 56 or darker.
            if textureLoad(t_type, g, 0).r == 0u {
                pos.y -= sinprod * (1.0 / 32.0);
                if textureLoad(t_shade, g, 0).r < 56u {
                    out.shade_wave = sinprod * 8.0;
                }
            }
        }
    }

    // The reflection MIRROR pass: flip the (waved) terrain about the
    // sea plane y = 0 — same camera, mirrored geometry = the planar
    // reflection the main pass's sea fragments sample.
    if globals.atlas.w == 2u {
        pos.y = -pos.y;
    }

    out.clip = globals.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    out.light = in.light;
    return out;
}

// Dynamic-light shade boost (retail sub_84EA0, per-pixel instead of
// the 5x5 cell grid): each light adds `31 · (1 − d²/R²) · intensity`
// shade rows within R = 543 world units ≈ 2.12 tiles (R² ≈ 4.5),
// capped at retail's 31. On the Night/Cave tables added rows =
// brighter (the polarity that makes retail gate day off — the app
// sends no lights on day maps).
fn light_boost(world: vec3<f32>) -> f32 {
    var add = 0.0;
    let n = u32(globals.viewport.w);
    for (var i = 0u; i < n; i = i + 1u) {
        let l = globals.lights[i];
        let d = world - l.xyz;
        let d2 = dot(d, d);
        const R2: f32 = 4.5;
        if d2 < R2 {
            add += 31.0 * (1.0 - d2 / R2) * l.w;
        }
    }
    return min(add, 31.0);
}

// Shade level of a tile, wrapped to the torus, clamped to the LUT.
fn shade_at(t: vec2<i32>) -> f32 {
    let wrapped = vec2<i32>((t.x % 256 + 256) % 256, (t.y % 256 + 256) % 256);
    return f32(min(textureLoad(t_shade, wrapped, 0).r, 63u));
}

// Painter-order depth (the original's compositing model): the depth
// channel carries HORIZONTAL camera distance, not ray depth. The
// original renderer draws tiles back-to-front and blits each tile's
// queued sprite right after the tile's own triangles (sub_main.cpp
// :33673) — occlusion is painter order at tile granularity. On a
// heightfield (no overhangs) plan distance orders identically to ray
// depth along every view ray, so terrain-vs-terrain occlusion is
// unchanged — but sprites keyed by their anchor TILE's plan distance
// composite exactly like the original: never clipped by the wall
// they stand against, always hidden by tiles in front.
const DEPTH_RANGE: f32 = 768.0;

fn plan_depth(world_xz: vec2<f32>) -> f32 {
    return clamp(length(world_xz - globals.camera.xz) / DEPTH_RANGE, 0.0, 0.999999);
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> FsOut {
    let dist = distance(in.world, globals.camera.xyz);
    let fog_a = fog_amount(dist);
    let ext = ext_amount(dist);
    // A fully melted fragment — or a fully fogged one in the MIRROR
    // arm — is pixel-identical to the sky sample it would return, and
    // the sky pass already painted exactly that behind it: discard
    // instead, which also recoups the fill rate of rasterizing the
    // far reaches of the 3x3 torus field (early-z is off here anyway
    // — this shader writes frag_depth — so the discard is free).
    if ext >= 1.0 || (globals.atlas.w == 2u && fog_a >= 1.0) {
        discard;
    }
    // A fragment seen from BEHIND its surface means the eye is inside
    // rock, or the near plane cut a hugged cave wall open — the wall-
    // peek x-ray (player report 2026-07-17). Paint it unlit black
    // (fogged, so the fog wall stays seamless): the peek reads as
    // shadowed rock instead of an inverted view of the far chamber.
    // Facing is per pass arm: the floor pass (arm 0) fronts UP; the
    // ceiling pass (arm 1) reuses the same index buffer viewed from
    // BELOW — its legit view is back-wound. The MIRROR pass (arm 2)
    // is EXEMPT — and facing CANNOT discriminate there: the flipped
    // landscape's plausible reflection is composed of BOTH sides
    // (slope top-sides are back-wound, the textured undersides of
    // flat terrain front-wound — painting either blanks half the
    // mirror; player reports 2026-07-17 round 2 and 2026-07-22). The
    // netherworld peek through sloped water is instead cured on the
    // SAMPLING side (see the slope fade at the mirror blend below).
    let peek = (globals.atlas.w == 0u && !front) || (globals.atlas.w == 1u && front);
    // (The peek RETURN sits below the texel lookup: watery arm-0
    // fragments are exempt and need `index` to identify themselves.)
    // Tile index from world position, wrapped to the 256x256 torus.
    let tile = vec2<i32>(
        (i32(floor(in.world.x)) % 256 + 256) % 256,
        (i32(floor(in.world.z)) % 256 + 256) % 256,
    );
    // The cave-ceiling pass paints every cell with the fixed WALL
    // texture (atlas cell 1 — retail's cave rock; the sculptors stamp
    // tile_type 1 on carved walls), lit by the same shade plane.
    var ty: i32;
    if globals.atlas.w == 1u {
        ty = 1;
    } else {
        ty = i32(textureLoad(t_type, tile, 0).r);
    }

    // Palette index: atlas texel (terrain type = atlas cell, nearest
    // sampling like the original rasterizer) or the flat tile color.
    var index: i32;
    if globals.atlas.x > 0u && ty < i32(globals.atlas.x) {
        let cell = vec2<i32>(ty % ATLAS_CELLS_PER_ROW, ty / ATLAS_CELLS_PER_ROW);
        // UV orientation from the angle byte (engine UVTable_D4350,
        // world-space rows): bit 4 flips x, bit 5 flips y, bit 6 swaps
        // the axes. Transition tiles (shorelines) depend on this.
        let orient = (textureLoad(t_angle, tile, 0).r >> 4u) & 7u;
        var st = fract(in.world.xz);
        if (orient & 1u) != 0u {
            st.x = 1.0 - st.x;
        }
        if (orient & 2u) != 0u {
            st.y = 1.0 - st.y;
        }
        if (orient & 4u) != 0u {
            st = st.yx;
        }
        let within = vec2<i32>(
            min(i32(st.x * f32(ATLAS_CELL)), ATLAS_CELL - 1),
            min(i32(st.y * f32(ATLAS_CELL)), ATLAS_CELL - 1),
        );
        index = i32(textureLoad(t_atlas, cell * ATLAS_CELL + within, 0).r);
    } else {
        index = i32(textureLoad(t_tile_colors, vec2<i32>(ty, 0), 0).r);
    }
    let watery = index < 12;

    // WATERY arm-0 backfaces are exempt from the peek black-out: the
    // shore-edge water corners WAVE, and on the up-swell the camera
    // sees the lifted face from UNDERNEATH — black-painting it opened
    // a "crack in reality" along wavy shorelines (player diagnosis
    // 2026-07-22). Retail's back-to-front tile blit had no underside
    // to show — the near tile's quad painted those pixels with its
    // own water texture — so falling through to normal texel shading
    // is exactly the retail look.
    if peek && !(globals.atlas.w == 0u && watery) {
        var out: FsOut;
        out.color = vec4<f32>(
            mix(globals.fog_color.rgb * fog_a, sky_backdrop(in.world), ext),
            1.0,
        );
        out.depth = plan_depth(in.world.xz);
        return out;
    }

    var base: vec3<f32>;
    if globals.atlas.y == 1u {
        // Smooth shading (opt-in enhancement): bilinear shade over the
        // four nearest tile centers, then a linear blend between the
        // two straddling shade-LUT rows. Colors still come only from
        // LUT rows — the palette pipeline stays intact, the light
        // gradient just stops snapping at tile edges.
        let p = in.world.xz - vec2<f32>(0.5, 0.5);
        let t0 = vec2<i32>(i32(floor(p.x)), i32(floor(p.y)));
        let f = fract(p);
        let s = clamp(
            mix(
                mix(shade_at(t0), shade_at(t0 + vec2<i32>(1, 0)), f.x),
                mix(shade_at(t0 + vec2<i32>(0, 1)), shade_at(t0 + vec2<i32>(1, 1)), f.x),
                f.y,
            ) + in.shade_wave + light_boost(in.world),
            0.0,
            63.0,
        );
        let s0 = i32(floor(s));
        let s1 = min(s0 + 1, 63);
        base = mix(
            textureLoad(t_colormap, vec2<i32>(index, s0), 0).rgb,
            textureLoad(t_colormap, vec2<i32>(index, s1), 0).rgb,
            fract(s),
        );
    } else {
        // Original look: one shade level per tile, plus the water
        // shimmer and the dynamic-light boost. The original rounds:
        // pnt5 carries (shade<<8 + 128) <<8 + 8*sinprod and the
        // rasterizer truncates the top byte.
        let shade = clamp(
            i32(round(shade_at(tile) + in.shade_wave + light_boost(in.world))),
            0,
            63,
        );
        base = textureLoad(t_colormap, vec2<i32>(index, shade), 0).rgb;
    }

    // `light` is 1.0 when the authentic shading array drives the look;
    // it carries a synthetic hillshade only for packages without one.
    let lit = base * in.light;

    // Fog target: the flat constant (retail's law — and the color the
    // silhouettes past the wall wear). In the MIRROR arm the backdrop
    // is never the muted horizon rows that constant was authored
    // against, but the elevation-negated sky — the bright authored
    // cloud band — so fading toward the constant left full-contrast
    // mountain cutouts in the reflection, presented crisp in NEAR
    // water (from carpet height a far peak's reflection lands in
    // close, fog-free water, at full sheen strength): the player-
    // reported "fog never applies to reflections". The mirror arm
    // instead fades toward the actual mirrored sky pixel behind the
    // fragment, so reflected terrain melts into the reflected clouds
    // at the same rate the direct view melts into the horizon.
    var fog_target = globals.fog_color.rgb;
    if globals.atlas.w == 2u && fog_a > 0.0 {
        fog_target = sky_backdrop(in.world);
    }
    var rgb = mix(lit, fog_target, fog_a);

    // Sea reflection (retail GRO reflection block, simplified): sea
    // fragments at sea level blend the mirror texture at their own
    // screen position, the sample point wobbled by the same wave that
    // shimmers the shade — the reflection ripples with the water.
    // Water identification is per game: MC2 = angle bit 3, the map
    // generator's OPEN-SEA flag (`mapAngle |= 8`, remc2 sub_43D50 —
    // the same bit that routes retail to the water raster mode 26;
    // tile TYPE 0 also covers the muddy shore, the 2026-07-16 wrong-
    // tiles report); MC1 = angle slope-nibble 0 (sub_11760's water
    // probe; deep sea sets bit 3 on top, so mask &7). The mirror
    // image is already fogged; 50% mirror keeps the water texture
    // readable (retail's <0xC texel holes blend about half the area).
    // WATER IS PER-TEXEL, exactly retail (playtest round 5, player
    // insight "the reflecting property is part of the data"): the
    // water raster blends screen content only where the TEXTURE's
    // palette index is < 0x0C (remc2 GRO:13945-65 mode 26) — the
    // waterline is painted into the transition-tile textures (atlas
    // data: cell 0 = 1024/1024 sub-0x0C texels, shore cells partial,
    // land 0 plus single-texel noise). So the mirror blend keys on
    // the fragment's own palette index; no tile flags at all.
    //
    // In the MIRROR pass those water texels are the mirror itself —
    // never part of the mirrored scene (a mirrored self-copy ghosted
    // in counterphase); discard them so the mirrored landscape / sky
    // shows through, while a transition tile's LAND texels still
    // reflect.
    if globals.atlas.w == 2u && watery {
        // ...but ONLY the mirror plane itself: near sea level AND
        // near-horizontal. Steep or elevated water is not part of the
        // y = 0 mirror (the main pass slope/altitude fades its OWN
        // mirror blend away) — it is scenery in the mirrored world,
        // and discarding it punched an x-ray window through the
        // flipped bank into the netherworld wherever a reflective
        // surface was itself reflected (player screenshot
        // 2026-07-22). Retained, it renders its plain water texture —
        // matching exactly what the main pass shows on that face.
        let flat_world = vec3<f32>(in.world.x, in.flat_y, in.world.z);
        let n = cross(dpdx(flat_world), dpdy(flat_world));
        if in.flat_y < 0.6 && abs(normalize(n).y) >= 0.97 {
            discard;
        }
    }
    if globals.viewport.z > 0.5 && globals.atlas.w == 0u && watery {
        // Altitude fade (0.2..0.6 tiles): elevated tiles reusing the
        // low palette indices (dark speckles) must not mirror.
        var water = clamp((0.6 - in.world.y) / 0.4, 0.0, 1.0);
        // Surface tilt: the y = 0 planar mirror is only valid on
        // HORIZONTAL water — on a tilted face (river runs, channel
        // walls) the screen-space sample peeks INSIDE the flipped
        // terrain volume, the netherworld x-ray (wrong in retail
        // too; player 2026-07-22). Tilt feeds the haze CONTENT
        // selector below, never the sheen weight: an earlier weight-
        // multiply version zeroed the sheen on steep faces, which
        // bypassed the haze entirely and left sharp plain-texture
        // patches against the sky-sheened water around them (player
        // screenshot, mc2:01 ~60-degree cliff-base face, round 5).
        // Level enough to mirror up to ~40 degrees, full sky haze
        // past ~60. Pre-wave (flat_y), so the swell can't flicker
        // the open sea's reflection.
        let flat_world = vec3<f32>(in.world.x, in.flat_y, in.world.z);
        let slope_n = cross(dpdx(flat_world), dpdy(flat_world));
        let level = smoothstep(0.5, 0.77, abs(normalize(slope_n).y));
        // Shore haze (player idea; certified "works rather well",
        // round 2 stretched + sky-colored it): near land the mirror
        // IMAGE gives way to flat SKY color — shallow shore water is
        // turbid, and what the haze replaces is mostly reflected sky
        // anyway, so the band is tonally continuous with the true
        // mirror around it ("can't see where it begins"). The sheen
        // weight itself is untouched: hazy water stays as reflective-
        // looking as open sea. Also masks the residual shoreline
        // artifact class (thin flipped-bank silhouettes, wave-lifted
        // samples). Distance to the nearest non-deep-water tile
        // (type != 0, atlas cell 0 = the all-water cell in both
        // games), the certified 7x7-tile-kernel law: fully opaque
        // only within 1/4 tile of the waterline (~10% of the reach),
        // then one smooth gradient across the rest, flat by ~2.2
        // tiles (round-3 player tuning: the earlier SQUARED curve
        // kept ~2 tiles near-opaque, visibly steep in daylight; night
        // was already perfect). Saturation stays inside the kernel's
        // guaranteed 2-tile reach so no onset ring shows where land
        // falls off the kernel's edge. The distance itself comes from
        // the baked t_shore field (manual bilinear — group 0 carries
        // no sampler).
        var haze = 0.0;
        if water > 0.0 {
            let sp = in.world.xz * SHORE_RES - 0.5;
            let s0 = vec2<i32>(floor(sp));
            let sf = sp - floor(sp);
            let sn = i32(256.0 * SHORE_RES);
            var v: array<f32, 4>;
            for (var i = 0; i < 4; i = i + 1) {
                let c = s0 + vec2<i32>(i % 2, i / 2);
                let wc = vec2<i32>((c.x % sn + sn) % sn, (c.y % sn + sn) % sn);
                v[i] = textureLoad(t_shore, wc, 0).r;
            }
            let shore = mix(mix(v[0], v[1], sf.x), mix(v[2], v[3], sf.x), sf.y)
                * SHORE_MAX;
            // Mirror content survives only where the water is BOTH
            // far enough from shore AND level enough; everything else
            // wears the flat sky sheen.
            haze = 1.0 - smoothstep(0.25, 2.2, shore) * level;
        }
        if water > 0.0 {
            // Sample the mirror where the RESTING surface point
            // projects, not at this fragment's own screen position:
            // the swell lifts a crest ~1/4 tile above the y = 0
            // mirror plane, raising its pixels past the flipped
            // bank's silhouette into reflected SKY — a sky-colored
            // crack opening along the shoreline every wave cycle
            // (player screenshot 2026-07-22). Re-projecting the
            // pre-wave point anchors the sample inside the valid
            // mirror image, and lets the reflection bob with the
            // swell for free.
            let rest = globals.view_proj * vec4<f32>(flat_world, 1.0);
            let rest_px = vec2<f32>(rest.x / rest.w + 1.0, 1.0 - rest.y / rest.w)
                * 0.5 * globals.viewport.xy;
            let wob = in.shade_wave * globals.viewport.y * 0.0006;
            let uv = (rest_px + vec2<f32>(wob, wob)) / globals.viewport.xy;
            // A heavy cool cast on the mirrored image (player taste,
            // round 3) — water never reflects neutrally. The shore
            // haze gets the same cast so its sky matches the mirror's
            // reflected sky exactly.
            let tint = vec3<f32>(0.60, 0.78, 1.20);
            let mirror = textureSampleLevel(t_mirror, s_mirror, uv, 0.0).rgb * tint;
            // The haze sky is the flat fog constant, but what the
            // mirror reflects on open sea is the SKY TEXTURE, whose
            // horizon rows run brighter than the constant in MC1
            // (atlas.z = 1) — brighten the haze there to match
            // (player round 6, "mc1 haze a bit too dark").
            // Multiplicative, so night's black sky stays black; MC2's
            // per-environment fog colors already track their skies.
            let sky_boost = select(1.0, 1.3, globals.atlas.z == 1u);
            let sheen = mix(mirror, globals.fog_color.rgb * sky_boost * tint, haze);
            // The sheen fades with the fog like everything else —
            // un-fogged it kept water (and the coastline haze) visible
            // straight through the fog band to any distance, revealing
            // shores far past the view distance (player 2026-07-23).
            rgb = mix(rgb, sheen, 0.5 * water * (1.0 - fog_a));
        }
    }

    // The extinction melt (ext_amount): approaching the far band the
    // silhouette dissolves into the sky pixel behind it.
    if ext > 0.0 {
        rgb = mix(rgb, sky_backdrop(in.world), ext);
    }

    var out: FsOut;
    out.color = vec4<f32>(rgb, 1.0);
    out.depth = plan_depth(in.world.xz);
    return out;
}

// Distance fog, the retail law (remc2 GRO:1038-1074): linear in
// SQUARED distance across the FogStart..FogEnd band. Retail hardcodes
// 15..19 tiles (cutoff 20); we scale that band by the configured view
// distance D (camera.w): start = 0.75·D, full = 0.95·D. D = 0 turns
// fog off.
fn fog_amount(dist: f32) -> f32 {
    let d = globals.camera.w;
    if d <= 0.0 {
        return 0.0;
    }
    let start2 = 0.5625 * d * d;  // (0.75 D)^2
    let end2 = 0.9025 * d * d;    // (0.95 D)^2
    return clamp((dist * dist - start2) / (end2 - start2), 0.0, 1.0);
}

// Silhouette extinction. Terrain past the fog wall stays visible as a
// flat fog-constant silhouette wherever that constant differs from
// the sky texture behind it (the whole 3x3 torus field rasterizes,
// with no reach cull) — the "distant ranges" scenery, kept. This
// second, far ramp melts those silhouettes into the actual sky pixel
// across the FIXED band EXT_START..EXT_END, fully gone before ~128
// tiles: the half-map distance where a peak's nearest torus copy
// switches sides as the camera flies — the sharp appear/vanish pop
// this ramp exists to hide. The band is independent of the fog
// distance and runs UNCONDITIONALLY (round 2: round 1 anchored the
// melt to the fog wall, entangling the two systems — player-ruled
// they must never overlap); the renderer instead caps the fog
// setting (lib.rs MAX_FOG_TILES = 90) so the whole fog band always
// ends short of EXT_START. Applies in every arm — in the MIRROR the
// fog discard usually preempts it, but a fog-off view melts its
// reflection at the same band as the world above.
// Billboard.wgsl copies these constants — keep them in lockstep.
const EXT_START: f32 = 95.0;
const EXT_END: f32 = 125.0;

fn ext_amount(dist: f32) -> f32 {
    return smoothstep(EXT_START, EXT_END, dist);
}

// The sky-texture pixel behind a fragment: the same ray law as
// sky.wgsl (azimuth/elevation at 1024 texels per turn, horizon on
// the bitmap's bottom edge), including its mirror-arm y negation —
// so a melt toward this sample sits pixel-exact on the sky the sky
// pass painted behind this very fragment.
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
