//! The wgpu renderer.
//!
//! Reads simulation state, never mutates it; interpolates between fixed
//! ticks for smooth motion at any display rate.
//!
//! Design commitments (see project README):
//! - Terrain, billboarded sprites, and water from baked packages.
//! - Palette-index data kept all the way to the fragment shader
//!   (palette-as-LUT) so the authentic 8-bit look is the baseline and
//!   enhanced rendering is a toggle, not a rewrite.
//!
//! Current scope: the terrain pass — a 256x256 tile mesh (one vertex
//! per grid point, engine-authentic alternating diagonals), tiles
//! textured in the fragment shader from the baked terrain atlas (the
//! terrain-type byte is the atlas cell index), texels resolved through
//! the engine's shade LUT and palette; flat map colors as the fallback
//! when no atlas is baked. Per-vertex hillshade, distance fog.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use mgc_sim::{HEIGHT_SCALE, MAP_TILES};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Number of light levels in the engine's shade-remap table.
pub const SHADE_LEVELS: usize = 64;

/// Width in pixels of a baked terrain-texture atlas (`terrain-atlas-N.bin`).
pub const ATLAS_WIDTH: usize = 256;
/// Edge length of one atlas cell (one terrain texture).
pub const ATLAS_CELL: usize = 32;

/// Which game's water-surface animation rule the terrain pass applies
/// (the per-corner sine wave in the original tile projectors; ROADMAP
/// "Terrain water animation").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveMode {
    /// No animation (static comparison renders).
    #[default]
    Off,
    /// MC1 (remc1 sub_main.cpp:33955): deep-water corners (angle bit 3)
    /// swell by ±¼ tile and shimmer by ±8 shade rows.
    Mc1,
    /// MC2 (remc2 GameRenderOriginal.cpp:1054): every water corner
    /// (type 0) ripples by ±1/32 tile, shimmer gated on shade < 56;
    /// phase advances at half MC1's rate.
    Mc2,
}

/// Everything the renderer needs from a loaded level: terrain arrays
/// from the package, color tables from the baked assets. Pixels resolve
/// exactly like the original engine: a base palette index — an atlas
/// texel where a terrain atlas is available, else the tile's flat map
/// color `tile_colors[type]` — through the shade remap and palette:
/// `palette[shade_lut[shade][index]]`.
pub struct LevelView {
    /// 256x256 terrain-type bytes, row-major `y * 256 + x`.
    pub tile_type: Vec<u8>,
    /// 256x256 height bytes, same layout.
    pub height: Vec<u8>,
    /// 256x256 light levels (the generator's shading array); None for
    /// packages baked without it (a synthetic hillshade fills in).
    pub shading: Option<Vec<u8>>,
    /// 256 RGB triplets (sRGB bytes, as baked).
    pub palette: [[u8; 3]; 256],
    /// Terrain type -> base palette index (`tile-colors-N.bin`).
    pub tile_colors: [u8; 256],
    /// Shade level x base index -> final palette index
    /// (`shade-lut-N.bin`, [`SHADE_LEVELS`] rows of 256).
    pub shade_lut: Vec<u8>,
    /// Terrain-texture atlas (`terrain-atlas-N.bin`): 8-bit palette
    /// indices, [`ATLAS_WIDTH`] wide, [`ATLAS_CELL`]-square cells, the
    /// terrain-type byte indexing cells row-major. None renders every
    /// tile with its flat map color.
    pub atlas: Option<Vec<u8>>,
    /// 256x256 angle/flags bytes (`terrain/angle.bin`): bits 4-6 pick
    /// the tile's texture UV orientation. None renders orientation 0
    /// everywhere (transition tiles like shorelines will misalign).
    pub angle: Option<Vec<u8>>,
    /// The game's water-surface animation rule.
    pub wave: WaveMode,
    /// MC2 cave second heightmap (`terrain/ceiling.bin`): 256x256
    /// ceiling bytes. Some ⇒ the renderer draws the CEILING PASS
    /// (the same grid, inverted plane, fixed wall texture) and the
    /// caller should keep the sky the cave fill color. None on
    /// every non-cave level.
    pub ceiling: Option<Vec<u8>>,
}

/// One entity dot on the overhead map: tile-unit position and the
/// palette index the original plots for its category.
#[derive(Debug, Clone, Copy)]
pub struct MapDot {
    pub x: f32,
    pub z: f32,
    pub color: u8,
    /// Pixel side length: 1 for the standard dot, 2 for the
    /// original's grown portal dot (sub_48710 v60).
    pub size: u8,
}

/// A [`MapDot`] lifted out of the map-texture bake for screen-space
/// drawing (the marker-size deviation, `marker_scale != 1.0`): tile
/// position, the dot's texel span (1 or 2), and its palette color
/// resolved to a linear-space tint for the solid-quad UI path.
#[derive(Debug, Clone, Copy)]
struct ScreenDot {
    x: f32,
    z: f32,
    size: f32,
    tint: [f32; 4],
}

/// A tinted circle on the overhead map (the trigger-volume overlay —
/// an opt-in enhancement/debugging aid, never drawn by the original).
/// Tile-unit center and radius, direct RGB (deliberately outside the
/// palette: this layer is explicitly non-faithful).
#[derive(Debug, Clone, Copy)]
pub struct MapArea {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
    pub color: [u8; 3],
}

/// An icon stamped onto the overhead map (the original's castle /
/// balloon UI-sprite markers, remc1 sub_48710 :57224-37). Because both
/// maps are yaw-rotated, stamps must stay SCREEN-UPRIGHT — a flag/
/// balloon always points up regardless of heading — so they are NOT
/// baked into the (rotated) map texture; the renderer projects each
/// one's world position through the map's rotation to a screen rect and
/// blits the upright sprite from the UI atlas. `uv` is the atlas texel
/// rect (x, y, w, h); `w`/`h` double as the on-screen pixel size.
#[derive(Debug, Clone, Copy)]
pub struct MapStamp {
    pub x: f32,
    pub z: f32,
    pub w: u32,
    pub h: u32,
    pub uv: [f32; 4],
    /// Fractional anchor: the point on the sprite (0..1 of its w/h, from
    /// the top-left) that pins to the world position. Per remc1
    /// sub_48710's per-range anchors: castle (58-65) = bottom-LEFT
    /// `(0, 1)` — the flagpole foot; balloon (66-73) = bottom-CENTER
    /// `(0.5, 1)` — the balloon base.
    pub anchor: [f32; 2],
}

/// The marching-ants guide line (remc1 :57161-82): a single-pixel mark
/// every 4 MAP-SURFACE pixels along the screen-projected player→castle
/// line, starting at `(tick & 3) + 4` so the ants march, each plotted
/// through the brighten blend. Drawn screen-space over the rotated map
/// (like the stamps) — NOT baked into the world texture, where the
/// spacing was 4 world TILES and stretched with the radar zoom instead
/// of staying 4px. Endpoints in tile units; `phase` = the 0..3 cycle.
#[derive(Debug, Clone, Copy)]
pub struct MapPath {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub phase: u8,
}

/// One target of the current MC2 objective, for the non-optional
/// objective-guide overlay (retail cannot disable it). Drawn
/// screen-space over whichever map surface is active — a blinking red
/// outline on every mark, plus a red arrow from the player (surface
/// center) toward the `nearest` one, so far/off-radar targets still
/// steer the player. Position in tile units (like `MapDot`); projected
/// with the same transform as the icon stamps.
#[derive(Debug, Clone, Copy)]
pub struct ObjectiveMark {
    pub x: f32,
    pub z: f32,
    pub nearest: bool,
    /// Outline colour: YELLOW for the fly-to point, RED otherwise
    /// (remc2 GameUI: only type-5 uses CLRD 0xFF0).
    pub yellow: bool,
}

/// Everything baked into the map terrain texture, in draw order: areas
/// (enhancement), then entity dots. (Icon stamps and the guide path
/// draw screen-space at render time — see `Renderer::set_map_stamps` /
/// `Renderer::set_map_path` — so they stay upright/evenly-spaced under
/// rotation.)
#[derive(Debug, Clone, Default)]
pub struct MapOverlay {
    pub dots: Vec<MapDot>,
    pub areas: Vec<MapArea>,
}

/// Flat-color overhead map: one RGBA pixel per tile (256x256, row-major
/// like the terrain grids), each resolved through the engine's map-view
/// color path `palette[shade_lut[shade][tile_colors[type]]]` — the
/// exact lookup the original's fullscreen map uses (remc2 GameUI) —
/// then the (opt-in) area overlay, then entity dots plotted on top, one
/// pixel per entity, exactly like the original (the enhanced marker
/// mode is a planned opt-in).
pub fn map_pixels(level: &LevelView, overlay: &MapOverlay) -> Vec<u8> {
    map_pixels_impl(level, overlay, true)
}

/// [`map_pixels`] with the dot layer optional: the marker-size
/// deviation (`Renderer::set_marker_scale` != 1.0) draws the dots
/// screen-space instead, so baking them too would double them up.
fn map_pixels_impl(level: &LevelView, overlay: &MapOverlay, bake_dots: bool) -> Vec<u8> {
    let n = MAP_TILES;
    let mut out = vec![0u8; n * n * 4];
    for i in 0..n * n {
        // The CAVE map variant (remc2 GameUI:2414-2443): a SEALED
        // tile (mapAngle bit3 — floor meets ceiling) draws palette
        // index 0, pitch black; only open cells show their terrain
        // color.
        if level.ceiling.is_some() && level.angle.as_ref().is_some_and(|a| a[i] & 8 != 0) {
            out[i * 4..i * 4 + 3].copy_from_slice(&level.palette[0]);
            out[i * 4 + 3] = 255;
            continue;
        }
        let ty = level.tile_type[i] as usize;
        let shade = level
            .shading
            .as_ref()
            .map(|s| (s[i] as usize).min(SHADE_LEVELS - 1))
            .unwrap_or(32);
        let base = level.tile_colors[ty] as usize;
        let idx = level.shade_lut[shade * 256 + base] as usize;
        out[i * 4..i * 4 + 3].copy_from_slice(&level.palette[idx]);
        out[i * 4 + 3] = 255;
    }
    // Area overlay: a light tint fill with a stronger rim, wrapping
    // toroidally like everything else on the map.
    for a in &overlay.areas {
        let r = a.radius.max(0.5);
        let (cx, cz) = (a.x, a.z);
        let span = r.ceil() as i32;
        for dz in -span..=span {
            for dx in -span..=span {
                let d = ((dx * dx + dz * dz) as f32).sqrt();
                if d > r + 0.5 {
                    continue;
                }
                let blend = if d > r - 1.0 { 0.75 } else { 0.30 };
                let x = (cx as i32 + dx).rem_euclid(n as i32) as usize;
                let z = (cz as i32 + dz).rem_euclid(n as i32) as usize;
                let i = (z * n + x) * 4;
                for c in 0..3 {
                    let base = out[i + c] as f32;
                    out[i + c] = (base + (a.color[c] as f32 - base) * blend) as u8;
                }
            }
        }
    }
    // NOTE: the marching-ants guide path is NOT baked here — retail
    // steps it in MAP-SURFACE pixels along the projected line
    // (:57161-82), so it draws screen-space with the stamps (see
    // `project_guide_path`); baking it stepped in world tiles read
    // ~1.5× sparser on the book map and stretched with radar zoom.
    if bake_dots {
        for dot in &overlay.dots {
            let x = (dot.x as usize).min(n - 1);
            let z = (dot.z as usize).min(n - 1);
            // `size` covers the original's 2x2 grown dot (portals).
            for dz in 0..dot.size as usize {
                for dx in 0..dot.size as usize {
                    let i = ((z + dz) % n) * n + (x + dx) % n;
                    out[i * 4..i * 4 + 3].copy_from_slice(&level.palette[dot.color as usize]);
                    out[i * 4 + 3] = 255;
                }
            }
        }
    }
    // NOTE: icon stamps (castle/balloon) are NOT baked here — they must
    // stay screen-upright under map rotation, so the renderer projects
    // and blits them as upright screen-space quads after the rotated
    // map draw (see `Renderer::map_stamp_quads`).
    out
}

/// Clip a textured quad to `bounds` (both `[x, y, w, h]` pixels),
/// trimming the atlas `uv` rect proportionally so the visible texels
/// stay put — the retail DrawBitmap clips marker sprites at the map
/// window's edge the same way. None when nothing remains.
fn clip_quad_to(rect: [f32; 4], uv: [f32; 4], bounds: [f32; 4]) -> Option<([f32; 4], [f32; 4])> {
    let x0 = rect[0].max(bounds[0]);
    let y0 = rect[1].max(bounds[1]);
    let x1 = (rect[0] + rect[2]).min(bounds[0] + bounds[2]);
    let y1 = (rect[1] + rect[3]).min(bounds[1] + bounds[3]);
    if x1 <= x0 || y1 <= y0 || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    let fx = (x0 - rect[0]) / rect[2];
    let fy = (y0 - rect[1]) / rect[3];
    let fw = (x1 - x0) / rect[2];
    let fh = (y1 - y0) / rect[3];
    Some((
        [x0, y0, x1 - x0, y1 - y0],
        [
            uv[0] + uv[2] * fx,
            uv[1] + uv[3] * fy,
            uv[2] * fw,
            uv[3] * fh,
        ],
    ))
}

/// Project map stamps onto one map surface as upright screen-space
/// quads — the pure core of [`Renderer::map_stamp_quads`], mirroring
/// map.wgsl's sampling transform (inverted): a stamp at world delta
/// `d` from the player lands at pane offset `R(-yaw)·d` scaled so
/// `zoom/2` tiles fill the pane half-extent (shorter axis; the longer
/// axis stretches by `aspect` exactly as the shader does).
///
/// TOROIDAL VISIBILITY: the world tiles every 256 tiles on both axes
/// and the shader samples it with `fract()`, so a stamp is visible
/// wherever ANY wrapped image of it lands on the pane. Wrapping the
/// delta per-axis BEFORE rotation and testing after loses diagonal
/// images — a (+100,+100) delta rotated 45° lands 141 tiles out, off a
/// 128-half-tile pane, while the map texture still shows that spot via
/// the wrap (the icon blinked out at diagonal headings). Testing each
/// candidate image (±256 per axis) fixes it; edge positions may
/// legitimately draw twice, matching the texture repeat.
///
/// The anchor POINT must land on the surface (retail only marks
/// entities whose map position is inside the window); the sprite rect
/// is then clipped to the surface bounds — for the round radar, to the
/// disc's bounding square (the rim corners can still bleed a few px; a
/// per-pixel disc mask can ride along with the LUT-bake pass if it
/// reads badly in play).
#[allow(clippy::too_many_arguments)]
fn project_map_stamps(
    stamps: &[MapStamp],
    cx: f32,
    cy: f32,
    half_x: f32,
    half_y: f32,
    px: f32,
    pz: f32,
    yaw: f32,
    zoom: f32,
    round: bool,
    aspect: f32,
    scale: f32,
) -> Vec<UiQuad> {
    let half_tiles = zoom * 0.5;
    let tiles = MAP_TILES as f32;
    // Match the shader: screen-up (-y) maps to "ahead"; the sample is
    // world = player + (off.x·cos + off.y·sin, off.x·sin − off.y·cos).
    // The forward map [c s; s −c] is an involution, so the same matrix
    // maps world delta → the shader's centered coords `p`.
    let (s, c) = yaw.sin_cos();
    let bounds = [cx - half_x, cy - half_y, half_x * 2.0, half_y * 2.0];
    let mut quads = Vec::new();
    for st in stamps {
        // Base image in [0, tiles); the −tiles sibling per axis covers
        // every offset a ≤full-world (stretched ≤~1.42×half) pane can
        // show. (The map screen's √2 zoom-out CAN reach farther on a
        // wide pane's far edges — beyond ±1 period — but everything
        // out there is >256 tiles from the player, deep inside the
        // extent fog's full black; player-ruled fine to leave those
        // duplicate images unmarked.)
        let bx = (st.x - px).rem_euclid(tiles);
        let bz = (st.z - pz).rem_euclid(tiles);
        for dx in [bx, bx - tiles] {
            for dz in [bz, bz - tiles] {
                let ox = dx * c + dz * s;
                let oy = dx * s - dz * c;
                // Tiles → pane-normalized [-1,1]. The shader's `p.y` is
                // NDC (y-UP), UiQuad space is y-DOWN — the flip keeps
                // stamps co-rotating with the map. The shader stretches
                // the LONGER axis's world span by `aspect`; mirror that.
                let mut nx = ox / half_tiles;
                let mut ny = -oy / half_tiles;
                if aspect >= 1.0 {
                    nx /= aspect;
                } else {
                    ny *= aspect;
                }
                if round && (nx * nx + ny * ny) > 1.0 {
                    continue;
                }
                if nx.abs() > 1.0 || ny.abs() > 1.0 {
                    continue;
                }
                let scx = cx + nx * half_x;
                let scy = cy + ny * half_y;
                let (w, h) = (st.w as f32 * scale, st.h as f32 * scale);
                // Per-stamp anchor (remc1 sub_48710 :57344-64): the
                // world point pins to `anchor`·(w,h) from the top-left
                // — castle (58-65) bottom-LEFT `DrawBitmap(v41,
                // v42−h)`, balloon (66-73) bottom-CENTER `(v41−w/2,
                // v42−h)`. uv is atlas texels (ui.wgsl divides).
                let rect = [scx - st.anchor[0] * w, scy - st.anchor[1] * h, w, h];
                if let Some((rect, uv)) = clip_quad_to(rect, st.uv, bounds) {
                    quads.push(UiQuad {
                        rect,
                        uv,
                        tint: [1.0, 1.0, 1.0, 1.0],
                    });
                }
            }
        }
    }
    quads
}

/// The marker-size deviation's entity dots as screen-space solid
/// quads: the same wrap-image walk and projection as
/// [`project_map_stamps`], but centered on the entity and sized by
/// `dot_px` — the side of a size-1 dot in screen pixels, into which
/// the caller folds the marker scale AND the zoom compensation (the
/// radar divides by its DEFAULT zoom, not the current one, so dots
/// hold their size while zooming; the map screens' zooms are fixed).
#[allow(clippy::too_many_arguments)]
fn project_map_dots(
    dots: &[ScreenDot],
    cx: f32,
    cy: f32,
    half_x: f32,
    half_y: f32,
    px: f32,
    pz: f32,
    yaw: f32,
    zoom: f32,
    round: bool,
    aspect: f32,
    dot_px: f32,
) -> Vec<UiQuad> {
    let half_tiles = zoom * 0.5;
    let tiles = MAP_TILES as f32;
    let (s, c) = yaw.sin_cos();
    let bounds = [cx - half_x, cy - half_y, half_x * 2.0, half_y * 2.0];
    let mut quads = Vec::new();
    for d in dots {
        let bx = (d.x - px).rem_euclid(tiles);
        let bz = (d.z - pz).rem_euclid(tiles);
        for dx in [bx, bx - tiles] {
            for dz in [bz, bz - tiles] {
                let ox = dx * c + dz * s;
                let oy = dx * s - dz * c;
                let mut nx = ox / half_tiles;
                let mut ny = -oy / half_tiles;
                if aspect >= 1.0 {
                    nx /= aspect;
                } else {
                    ny *= aspect;
                }
                if round && (nx * nx + ny * ny) > 1.0 {
                    continue;
                }
                if nx.abs() > 1.0 || ny.abs() > 1.0 {
                    continue;
                }
                let scx = cx + nx * half_x;
                let scy = cy + ny * half_y;
                let side = d.size * dot_px;
                let rect = [scx - side * 0.5, scy - side * 0.5, side, side];
                // Solid quad (uv.z == 0); the zero uv survives the
                // proportional clip untouched.
                if let Some((rect, uv)) = clip_quad_to(rect, [0.0; 4], bounds) {
                    quads.push(UiQuad {
                        rect,
                        uv,
                        tint: d.tint,
                    });
                }
            }
        }
    }
    quads
}

/// The marching-ants guide path on one map surface (remc1 :57155-82),
/// as screen-space single-"pixel" quads. Retail projects the
/// player→target line onto the map surface and plots a mark every 4
/// SURFACE pixels starting at `(tick & 3) + 4` (the march), breaking at
/// the surface/window edge — the spacing is constant on screen no
/// matter the zoom. `scale` = screen px per native surface px; marks
/// are `scale`-sized like the surface's own pixels. The mark ink is
/// the blend-LUT brighten (byte_BB934 toward byte_AE167) — a
/// translucent near-white until the LUT bake.
#[allow(clippy::too_many_arguments)]
fn project_guide_path(
    path: &MapPath,
    cx: f32,
    cy: f32,
    half_x: f32,
    half_y: f32,
    px: f32,
    pz: f32,
    yaw: f32,
    zoom: f32,
    round: bool,
    aspect: f32,
    scale: f32,
) -> Vec<UiQuad> {
    const ANT_INK: [f32; 4] = [1.0, 1.0, 0.95, 0.8];
    let half_tiles = zoom * 0.5;
    let tiles = MAP_TILES as f32;
    let (s, c) = yaw.sin_cos();
    // Project a world point to pane pixels (no cull) — the same
    // transform as the stamps, shortest-wrap image of the delta.
    let project = |wx: f32, wz: f32| -> (f32, f32) {
        let dx = (wx - px + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let dz = (wz - pz + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let ox = dx * c + dz * s;
        let oy = dx * s - dz * c;
        let mut nx = ox / half_tiles;
        let mut ny = -oy / half_tiles;
        if aspect >= 1.0 {
            nx /= aspect;
        } else {
            ny *= aspect;
        }
        (cx + nx * half_x, cy + ny * half_y)
    };
    let (fx, fy) = project(path.from.0, path.from.1);
    let (tx, ty) = project(path.to.0, path.to.1);
    let (dx, dy) = (tx - fx, ty - fy);
    let dist = (dx * dx + dy * dy).sqrt();
    let mut quads = Vec::new();
    if dist < 1.0 || scale <= 0.0 {
        return quads;
    }
    let (ux, uy) = (dx / dist, dy / dist);
    let dot = scale.max(1.0);
    // March in native surface pixels: start (phase & 3) + 4, step 4
    // (:57161), breaking at the first mark off the surface/disc like
    // retail's bounds checks.
    let mut t = ((path.phase & 3) as f32 + 4.0) * scale;
    let step = 4.0 * scale;
    while t <= dist {
        let (mx, my) = (fx + ux * t, fy + uy * t);
        let (nx, ny) = ((mx - cx) / half_x, (my - cy) / half_y);
        if nx.abs() > 1.0 || ny.abs() > 1.0 {
            break;
        }
        if round && (nx * nx + ny * ny) > 1.0 {
            break;
        }
        quads.push(UiQuad {
            rect: [mx, my, dot, dot],
            uv: [0.0, 0.0, 0.0, 0.0], // solid quad (uv.z == 0)
            tint: ANT_INK,
        });
        t += step;
    }
    quads
}

/// The MC2 objective-guide overlay on one map surface (retail GameUI
/// :3040-3264): a tight blinking box outline around every
/// current-objective target (yellow for the fly-to point, red
/// otherwise), plus a red arrow-glyph that sits between the player
/// (surface center) and the nearest target, floating in front of it as
/// you approach. Vertices/edges are rasterized as dot runs (the UiQuad
/// stream is axis-aligned), so the rotated glyph works at any heading.
/// Blink is tick-driven to match retail exactly: the OUTLINE draws
/// 1-in-4 ticks (`!(tick & 3)` — a rapid shimmer), the ARROW draws when
/// `(tick & 0x40) && ((tick/6) & 1)` — ~5 six-tick blinks during the
/// 64-tick bit-6 window, then a 64-tick pause. Same projection transform
/// as [`project_map_stamps`].
#[allow(clippy::too_many_arguments)]
fn project_objective_marks(
    marks: &[ObjectiveMark],
    tick: u32,
    cx: f32,
    cy: f32,
    half_x: f32,
    half_y: f32,
    px: f32,
    pz: f32,
    yaw: f32,
    zoom: f32,
    round: bool,
    aspect: f32,
    scale: f32,
) -> Vec<UiQuad> {
    let half_tiles = zoom * 0.5;
    let tiles = MAP_TILES as f32;
    let (s, c) = yaw.sin_cos();
    // World point → surface pixels (shortest-wrap image of the delta),
    // plus whether it lands on the visible surface.
    let project = |wx: f32, wz: f32| -> (f32, f32, bool) {
        let dx = (wx - px + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let dz = (wz - pz + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let ox = dx * c + dz * s;
        let oy = dx * s - dz * c;
        let mut nx = ox / half_tiles;
        let mut ny = -oy / half_tiles;
        if aspect >= 1.0 {
            nx /= aspect;
        } else {
            ny *= aspect;
        }
        let on = if round {
            nx * nx + ny * ny <= 1.0
        } else {
            nx.abs() <= 1.0 && ny.abs() <= 1.0
        };
        (cx + nx * half_x, cy + ny * half_y, on)
    };
    let mut quads = Vec::new();
    let dot = scale.max(1.0);
    // Retail blink gates (remc2 GameUI): outline 1-in-4 ticks; arrow the
    // ~5-blinks-then-pause pattern.
    let outline_on = tick & 3 == 0;
    let arrow_on = tick & 0x40 != 0 && (tick / 6) & 1 != 0;
    let bounds = [cx - half_x, cy - half_y, half_x * 2.0, half_y * 2.0];
    // The target mark: retail's DrawObjectiveRectangle_64CE0 draws a
    // TIGHT 3·scale px box (scale-thick edges) centered on the projected
    // marker, blinking — the map dot/flag just gets a small blinking
    // outline. YELLOW for the fly-to point, RED otherwise.
    let half = (1.5 * scale).max(1.5);
    let bar = scale.max(1.0);
    let push_ring = |quads: &mut Vec<UiQuad>, mx: f32, my: f32, yellow: bool| {
        let ink = if yellow {
            [1.0, 1.0, 0.15, 1.0]
        } else {
            [1.0, 0.15, 0.15, 1.0]
        };
        for edge in [
            [mx - half, my - half, 2.0 * half, bar],
            [mx - half, my + half - bar, 2.0 * half, bar],
            [mx - half, my - half, bar, 2.0 * half],
            [mx + half - bar, my - half, bar, 2.0 * half],
        ] {
            let x0 = edge[0].max(bounds[0]);
            let y0 = edge[1].max(bounds[1]);
            let x1 = (edge[0] + edge[2]).min(bounds[0] + bounds[2]);
            let y1 = (edge[1] + edge[3]).min(bounds[1] + bounds[3]);
            if x1 > x0 && y1 > y0 {
                quads.push(UiQuad {
                    rect: [x0, y0, x1 - x0, y1 - y0],
                    uv: [0.0, 0.0, 0.0, 0.0],
                    tint: ink,
                });
            }
        }
    };
    let mut nearest: Option<(f32, f32)> = None; // nearest target TILE pos
    for m in marks {
        if outline_on {
            let (mx, my, on) = project(m.x, m.z);
            if on {
                push_ring(&mut quads, mx, my, m.yellow);
            }
        }
        if m.nearest {
            nearest = Some((m.x, m.z));
        }
    }
    // The steer arrow (retail GameUI :3186-3260): a RED 7-vertex outline
    // glyph placed at the WORLD point `player + dir·min(dist−512, 15872)`
    // — i.e. ~2 tiles short of the target (512 eng), capped at 62 tiles
    // (15872 eng) — so it floats in front of the target when close and
    // never overshoots. Rotated to point at the target; always red,
    // regardless of the outline colour.
    if let Some((twx, twz)) = nearest.filter(|_| arrow_on) {
        let dxw = (twx - px + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let dzw = (twz - pz + tiles * 0.5).rem_euclid(tiles) - tiles * 0.5;
        let dist_w = dxw.hypot(dzw);
        if dist_w > 0.01 {
            let reach = (dist_w - 2.0).clamp(0.0, 62.0);
            let (uxw, uzw) = (dxw / dist_w, dzw / dist_w);
            let (ax, ay, _) = project(px + uxw * reach, pz + uzw * reach);
            let (tx, ty, _) = project(twx, twz);
            // Screen forward = arrow → target; fall back to center →
            // target if the arrow sits on the target.
            let (mut fx, mut fy) = (tx - ax, ty - ay);
            if fx.hypot(fy) < 0.01 {
                fx = tx - cx;
                fy = ty - cy;
            }
            let fl = fx.hypot(fy).max(0.01);
            let (fx, fy) = (fx / fl, fy / fl);
            // Retail arrow polygon (local px; tip at origin, stem +y),
            // scaled. Rotate local −y (tip) → screen forward.
            let s = scale;
            let verts = [
                (0.0, 0.0),
                (9.0 * s, 13.0 * s),
                (4.0 * s, 13.0 * s),
                (4.0 * s, 23.0 * s),
                (-4.0 * s, 23.0 * s),
                (-4.0 * s, 13.0 * s),
                (-9.0 * s, 13.0 * s),
            ];
            let map = |lx: f32, ly: f32| (ax + lx * fy - ly * fx, ay - lx * fx - ly * fy);
            let arrow_ink = [1.0, 0.1, 0.1, 1.0];
            let mut edge = |a: (f32, f32), b: (f32, f32)| {
                let (p0, p1) = (map(a.0, a.1), map(b.0, b.1));
                let (rx, ry) = (p1.0 - p0.0, p1.1 - p0.1);
                let steps = (rx.hypot(ry).max(1.0) / dot).ceil() as i32;
                for i in 0..=steps {
                    let f = i as f32 / steps as f32;
                    quads.push(UiQuad {
                        rect: [p0.0 + rx * f, p0.1 + ry * f, dot, dot],
                        uv: [0.0, 0.0, 0.0, 0.0],
                        tint: arrow_ink,
                    });
                }
            };
            // The 7-edge closed outline (GameUI :3254-3260 order).
            edge(verts[0], verts[1]);
            edge(verts[1], verts[2]);
            edge(verts[2], verts[3]);
            edge(verts[3], verts[4]);
            edge(verts[4], verts[5]);
            edge(verts[5], verts[6]);
            edge(verts[6], verts[0]);
        }
    }
    quads
}

/// Camera state for one rendered frame (already interpolated).
#[derive(Debug, Clone, Copy)]
pub struct CameraView {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Camera bank in radians, positive = bank right (the faithful
    /// turn cue — retail renders the roll stick at full value,
    /// remc1 :52432 / remc2 EF:40258). Rolls the whole view basis:
    /// terrain, billboards and sky bank together, the true-3D
    /// equivalent of retail's DrawSky/SetBillboards screen rotation.
    pub roll: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
}

/// The rolled camera basis (right, up, fwd) shared by the view
/// matrix and the billboard expansion vectors.
/// Wrap one bolt segment to the camera as a RIGID unit on the `full`-tile
/// torus. A bolt is a multi-point primitive: resolving each endpoint to its
/// own nearest-camera image splits the segment a full map-width apart
/// whenever the two ends straddle the ±half-map seam — a strike sitting near
/// the camera's antipode (a rival dueling across the map), which read on
/// screen as a bolt "coming from the opposite side". Instead, resolve the
/// strike's ONE shared `anchor` (origin) to the camera and translate BOTH
/// endpoints by that same whole-map offset (∈ {0, ±full}). Every segment and
/// branch of a bolt carries the same anchor, so the whole channel stays
/// coherent no matter where the camera is.
fn wrap_bolt_to_camera(
    anchor: [f32; 2],
    p0: [f32; 3],
    p1: [f32; 3],
    cam_x: f32,
    cam_z: f32,
    full: f32,
) -> ([f32; 3], [f32; 3]) {
    // The whole-map shift that carries `a` into the camera's ±half-map
    // window (0, +full, or -full); the anchor rides along and so do both
    // endpoints, rigidly.
    let offset = |a: f32, c: f32| {
        let mut d = a - c;
        if d > full / 2.0 {
            d -= full;
        }
        if d < -full / 2.0 {
            d += full;
        }
        (c + d) - a
    };
    let ox = offset(anchor[0], cam_x);
    let oz = offset(anchor[1], cam_z);
    (
        [p0[0] + ox, p0[1], p0[2] + oz],
        [p1[0] + ox, p1[1], p1[2] + oz],
    )
}

/// The camera basis before bank: horizontal right, pitch-tilted up.
/// Billboards expand on this one — sprites keep the pitch tilt of the
/// original's 2D screen blit but stay upright over the terrain when
/// the carpet banks (retail counter-rotates the sprite rasterizer by
/// -roll, SetBillboards_3B560, to the same end).
fn camera_flat_basis(cam: &CameraView) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let fwd = [sy * cp, sp, -cy * cp];
    let flat_right = [cy, 0.0, sy];
    let flat_up = [
        flat_right[1] * fwd[2] - flat_right[2] * fwd[1],
        flat_right[2] * fwd[0] - flat_right[0] * fwd[2],
        flat_right[0] * fwd[1] - flat_right[1] * fwd[0],
    ];
    (flat_right, flat_up, fwd)
}

fn camera_basis(cam: &CameraView) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (flat_right, flat_up, fwd) = camera_flat_basis(cam);
    // Bank: rotate right/up about fwd; positive roll tips the up
    // vector toward +right (the camera leans into a right turn).
    let (sr, cr) = cam.roll.sin_cos();
    let right = std::array::from_fn(|i| flat_right[i] * cr - flat_up[i] * sr);
    let up = std::array::from_fn(|i| flat_up[i] * cr + flat_right[i] * sr);
    (right, up, fwd)
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Vertex {
    pos: [f32; 3],
    light: f32,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    fog_color: [f32; 4],
    /// x = atlas cell count (0 = untextured), y/z/w reserved.
    atlas: [u32; 4],
    /// Camera basis, bank included (sky ray reconstruction, mirror
    /// reach); the w slots carry tan(fov/2) h/v for the sky ray.
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    /// The pre-bank basis (`camera_flat_basis`) for billboard
    /// expansion: sprites keep the pitch tilt but stay upright over
    /// the terrain when the carpet banks, matching retail's -roll
    /// sprite counter-rotation (SetBillboards_3B560). w slots unused.
    bb_right: [f32; 4],
    bb_up: [f32; 4],
    /// x/y = framebuffer size in pixels, z = sea-sheen flag for this
    /// pass (1 = the main pass runs the mirror/haze blend on water; 0
    /// in the mirror pass itself and when reflections are off — note
    /// the mirror pre-pass may be reach-skipped while the sheen stays
    /// on, the water then wearing the pure sky haze), w = dynamic
    /// light count. Only terrain.wgsl declares this field on — the
    /// other shaders' shorter Globals structs bind a prefix.
    viewport: [f32; 4],
    /// Dynamic point lights: xyz = world position (tile units), w =
    /// intensity (1 = retail's 128 spell/explosion baseline; the
    /// standing fire is 80/128). Live count in `viewport.w`.
    lights: [[f32; 4]; MAX_LIGHTS],
}

/// Uniform-array cap for dynamic lights (retail keeps a 50-slot
/// cell-grid registry; our per-pixel pass rarely needs more than a
/// handful on screen).
const MAX_LIGHTS: usize = 16;

/// One world sprite to draw, resolved from a level entity. Static data;
/// the view-dependent part (which rotation view, mirroring) is computed
/// per frame from `yaw` and the camera.
#[derive(Debug, Clone, Copy)]
pub struct Billboard {
    /// Feet-center position, world units (x/z tile coords, y altitude).
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Facing, radians (same convention as [`CameraView::yaw`]).
    pub yaw: f32,
    /// First sprite id of the entity's view/animation family.
    pub sprite_base: u16,
    /// The original's view-selection mode (sprite flags high byte /
    /// stats-table draw type): 0/1/21 single view, 2..=16 animation,
    /// 17 = 8 views + mirrored back half, 18 = 16 views, 19/20 =
    /// 5-/3-view folds.
    pub draw_type: u8,
    /// Per-entity animation byte (entity offset 88): for the 2..=16
    /// animation draw types the original draws sprite `base + frame`.
    /// 0 for static/rotation-view entities.
    pub frame: u8,
    /// World height of the quad (engine `var_8 / 256`).
    pub world_h: f32,
    /// RETAIL CO-TILE PAINT ORDER, `(0, 1)`: this sprite's place in
    /// its tile's entity chain, head→tail (higher = drawn later = on
    /// top). See `mgc_sim::engine::world::LivePose::chain_depth` for
    /// the law. The depth channel keys every sprite to its anchor
    /// TILE, so co-tile sprites tie bit-for-bit and the tie would
    /// otherwise fall to pool-allocation luck; this breaks it the way
    /// retail's z-bufferless painter did. `0.5` = neutral, for
    /// instruments and the comparison paths that have no chain.
    pub chain_depth: f32,
    /// Retail translucency raster mode (MC2 DrawSprite_41BD3 modes;
    /// docs/traces/mc2-transparency-drawlist.md): 0 = opaque, 2 =
    /// 33%-opaque (smoke), 3 = 67%-opaque (glows/fades). The blend
    /// matrix `T[0x4000+…]` is `nearest_palette(⅓·src + ⅔·dst)`, so
    /// modes 2/3 render as plain alpha 1/3 / 2/3, back-to-front with
    /// depth writes off (retail draws them inline in painter order).
    ///
    /// ⚠ **4 IS NOT A RETAIL MODE.** It is the INSTRUMENT alpha (1/6),
    /// used by the replay ghost alone: deliberately fainter than any
    /// raster mode retail can produce, so an overlay can never be
    /// mistaken for a sprite that belongs in the world. Keep it out of
    /// every entity path — if a retail sprite ever wants this alpha,
    /// it wants mode 2 and a trace to justify it.
    pub blend: u8,
    /// RETAIL PROXIMITY CONCEALMENT: the sprite materializes only
    /// inside retail's OWN fog band, independent of the configured
    /// fog distance — alpha ramps 0→full across 19..15 tiles on
    /// retail's fog-row law and the instance is dropped entirely
    /// beyond it (`conceal_visibility`). Retail gets this for
    /// free from its short draw radius: every sprite hard-culls at
    /// 20 tiles (GRO:3498 `tileRenderCutOffDistance` = 5120²) with
    /// fog saturated from 19 (GRO:3505-3511); the port's extended
    /// fog would otherwise expose these entities map-wide. Carried by
    /// the MC2 wraith (5,26) always, and dwellers (5,23) under the
    /// `mc2_dweller_invisibility` patch.
    ///
    /// The volume is a SPHERE — slant (3D) distance, the port's own
    /// fog metric, so climbing out of the band conceals too
    /// (player-ruled 2026-08-08: one shape for every concealed
    /// entity, no special cases). Retail's literal metric is
    /// horizontal-only in both games (MC2 GRO:3498/1040, MC1 remc1
    /// sub_main.cpp:36843/34456 — altitude enters only the screen
    /// projection), but the shape was unobservable at retail flight
    /// ceilings, and the fog this law stands in for is spherical
    /// here.
    pub conceal: bool,
}

/// 16 view sectors folded to 5 sprites (draw type 19, `byte_906E8`).
const VIEW_FOLD_5: [u8; 16] = [0, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 2, 2, 1, 1, 0];
/// 16 view sectors folded to 3 sprites (draw type 20, `byte_906F8`).
const VIEW_FOLD_3: [u8; 16] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 1, 1, 1, 0, 0];

/// One monster health bar (unfaithful debug overlay): the classic
/// red-on-black rectangle floating above the sprite.
#[derive(Debug, Clone, Copy)]
pub struct HealthBar {
    /// Bar bottom-center, world units (x/z tile coords, y altitude).
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Bar width in world units.
    pub w: f32,
    /// Remaining life fraction 0..=1.
    pub frac: f32,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct BarInstance {
    pos: [f32; 3],
    size: [f32; 2],
    frac: f32,
}

/// One screen-space UI quad (spellbook icon, HUD slot, bar fill).
/// Pixel coordinates, origin top-left. `uv` addresses the RGBA UI
/// atlas in texels; a zero-width uv marks a solid quad drawn from
/// `tint` alone. Tint multiplies sampled color (dim = grey tint).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct UiQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub tint: [f32; 4],
}

/// PROTOTYPE fire particle (throwaway) — one glowing flame disc in the
/// world, fed per frame by the app's fireball-trail emitter.
#[derive(Debug, Clone, Copy)]
pub struct FireParticle {
    /// World center (x/z tile coords, y altitude).
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// World half-extents (width, height); flames run taller than wide.
    pub w: f32,
    pub h: f32,
    /// 0..1 heat (1 = white-hot core, 0 = dark ember / soot).
    pub heat: f32,
    /// 0..1 coverage / opacity.
    pub alpha: f32,
    /// Per-particle procedural phase (dapple + flicker seed).
    pub seed: f32,
}

/// PROTOTYPE lightning-bolt segment (enhanced lightning) — one thin
/// glowing ribbon piece of a strike's channel or branch, fed per frame
/// by the app's bolt ledger.
#[derive(Debug, Clone, Copy)]
pub struct BoltSegment {
    /// World endpoints (x/z tile coords, y altitude), like
    /// [`FireParticle`].
    pub p0: [f32; 3],
    pub p1: [f32; 3],
    /// The parent strike's origin (world x/z), shared by every segment
    /// of the same bolt. The torus wrap ([`Renderer::bolt_instances`])
    /// resolves this ONE point to the camera and translates the whole
    /// segment rigidly by the same whole-map offset — so a bolt sitting
    /// near the camera's antipode can never be split across the seam
    /// (wrapping the two endpoints independently would).
    pub anchor: [f32; 2],
    /// World half-width of the ribbon.
    pub width: f32,
    /// 0..1 strike energy (return stroke = 1, leader/decay < 1).
    pub energy: f32,
    /// 0..1 coverage (envelope fade).
    pub alpha: f32,
    /// Per-strike procedural phase (time-rolled flicker seed).
    pub seed: f32,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct BoltInstance {
    p0: [f32; 3],
    p1: [f32; 3],
    width: f32,
    energy: f32,
    alpha: f32,
    seed: f32,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct FireInstance {
    pos: [f32; 3],
    size: [f32; 2],
    heat: f32,
    alpha: f32,
    seed: f32,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct BillboardInstance {
    pos: [f32; 3],
    size: [f32; 2],
    uv_pos: [f32; 2],
    uv_size: [f32; 2],
    /// x = mirror, y = shade LUT row.
    flags: [u32; 2],
    /// Opacity: 1.0 opaque, 1/3 / 2/3 for the translucent raster
    /// modes (only consumed by the blend pipeline's fragment pass).
    alpha: f32,
    /// Co-tile chain rank, `(0, 1)` — see [`Billboard::chain_depth`].
    chain: f32,
}

/// The instance layout of [`BillboardInstance`], shared by BOTH
/// billboard pipelines (opaque and blend) because they run the SAME
/// shader and must agree with its `Instance` bindings exactly.
/// ⚠ Kept in one place deliberately: when these were two inline
/// `vertex_attr_array!`s, adding `chain` to one and not the other
/// built cleanly and blew up at pipeline creation, on the launch path,
/// where nothing but a real window can reach it.
const BILLBOARD_ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x2, 2 => Float32x2,
    3 => Float32x2, 4 => Uint32x2, 5 => Float32,
    6 => Float32,
];

/// Default sky/fog color, the classic hazy horizon (MC1's hand-picked
/// approximation; kept until the sky presentation trace lands). MC2
/// levels override per environment via [`Renderer::set_sky_color`] —
/// the shade LUT's row-0 fill is the engine's fog far color (night =
/// black, day = pale blue). sRGB, converted to linear where uploaded.
const SKY_SRGB: [f32; 3] = [0.42, 0.55, 0.75];

/// Soften the resolved sky bitmap with a small separable gaussian,
/// torus-wrapped on both axes (the cloud plane tiles infinitely).
/// The 256-texel bitmap was authored for ~1:1 texel:pixel at 320x200;
/// at modern resolutions one texel spans many screen pixels, and the
/// bilinear upscale exposes the texel grid and the period dither as
/// soft blocks. Baked once at load, the blur restores the smooth read
/// the original had at native resolution — player-ruled 2026-08-08
/// ("the sky genuinely looks better blurred", a net win independent
/// of the water-reflection blur, which this also feeds since the
/// mirrored sky and the fog/extinction melts sample the same
/// texture). Sigma 1 texel, 7 taps.
fn blur_sky(rgba: &mut [u8]) {
    const N: i32 = 256;
    // exp(-x²/2) for x = -3..=3 (sigma 1), normalized below.
    const K: [f32; 7] = [
        0.011109, 0.135335, 0.606531, 1.0, 0.606531, 0.135335, 0.011109,
    ];
    let norm: f32 = K.iter().sum();
    let mut tmp = vec![0u8; rgba.len()];
    let blur_axis = |src: &[u8], dst: &mut [u8], dx: i32, dy: i32| {
        for y in 0..N {
            for x in 0..N {
                let mut acc = [0.0f32; 3];
                for (i, w) in K.iter().enumerate() {
                    let o = i as i32 - 3;
                    let sx = (x + o * dx).rem_euclid(N);
                    let sy = (y + o * dy).rem_euclid(N);
                    let p = ((sy * N + sx) * 4) as usize;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += src[p + c] as f32 * w;
                    }
                }
                let d = ((y * N + x) * 4) as usize;
                for (c, a) in acc.iter().enumerate() {
                    dst[d + c] = (a / norm).round() as u8;
                }
                dst[d + 3] = 255;
            }
        }
    };
    blur_axis(rgba, &mut tmp, 1, 0);
    blur_axis(&tmp, rgba, 0, 1);
}

/// The 1x1 group-1 sky-dummy texel for a fog color. sRGB bytes — the
/// dummy is Rgba8UnormSrgb, so samples come back linear, matching the
/// globals' fog constant.
fn sky_texel(srgb: [f32; 3]) -> [u8; 4] {
    [
        (srgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (srgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (srgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    ]
}
/// Default fog VIEW DISTANCE in tiles: where the fog band fully
/// occludes. 20 = the retail law (remc2 GRO:668-679 — fade 15..19
/// tiles, geometry cutoff 20; the shaders scale that band as
/// 0.75·D..0.95·D). Most monster sight radii are 15-20 tiles, so the
/// retail distance is exactly what hides acquisition pop-in.
const DEFAULT_FOG_TILES: f32 = 20.0;
/// Fog view-distance cap, tiles: keeps the whole fog band (full
/// occlusion at 0.95·D = 85.5) short of the silhouette melt band
/// (terrain.wgsl EXT_START..EXT_END = 95..125), which runs
/// unconditionally and hides the ~128-tile torus-copy pop — the fog
/// and the melt never overlap (player-ruled 2026-08-08, round 2).
/// 0 stays "fog off". config::FOG_STOPS' top stop matches this.
pub const MAX_FOG_TILES: f32 = 90.0;

/// Shore-field bake geometry — must match terrain.wgsl's SHORE_RES /
/// SHORE_MAX: texels per tile edge, and the distance saturation.
const SHORE_RES: usize = 4;
const SHORE_MAX: f32 = 2.5;
/// Tile edge of one deep-water presence block (the mirror-pass gate).
const WATER_BLOCK: usize = 8;
/// Downsample factor of the reflection-blur chain (blur.wgsl's DIV
/// must match): the mirror image is gaussian-softened at this
/// fraction of the framebuffer before the water samples it. 2 =
/// player-tuned (round 3: the first build's 4 was "roughly twice"
/// the wanted blur diameter — the kernel is in downsampled texels,
/// so the diameter scales with this factor).
const REFLECTION_BLUR_DIV: u32 = 2;

/// Bake the shore-haze distance law into the sub-tile field the
/// terrain shader samples: for every SHORE_RES x SHORE_RES texel of
/// every tile in the given tile rect (torus-wrapped), the Euclidean
/// distance from the texel center to the nearest non-deep-water tile
/// rect (type != 0) within the 7x7 tile neighbourhood of the texel's
/// own tile — verbatim the shader's former per-fragment kernel —
/// saturated at SHORE_MAX and quantized to R8Unorm (value/SHORE_MAX).
fn bake_shore_region(
    types: &[u8],
    n: usize,
    field: &mut [u8],
    tx0: usize,
    tz0: usize,
    tw: usize,
    th: usize,
) {
    let s = n * SHORE_RES;
    for rz in 0..th {
        let tz = (tz0 + rz) % n;
        for rx in 0..tw {
            let tx = (tx0 + rx) % n;
            // Land rect origins near this tile, UNWRAPPED (the shader
            // measured against tile±3 in continuous world space; the
            // texture lookup alone wrapped).
            let mut land = [[0f32; 2]; 49];
            let mut nland = 0;
            for dz in -3i32..=3 {
                for dx in -3i32..=3 {
                    let wx = (tx as i32 + dx).rem_euclid(n as i32) as usize;
                    let wz = (tz as i32 + dz).rem_euclid(n as i32) as usize;
                    if types[wz * n + wx] != 0 {
                        land[nland] = [(tx as i32 + dx) as f32, (tz as i32 + dz) as f32];
                        nland += 1;
                    }
                }
            }
            for j in 0..SHORE_RES {
                let pz = tz as f32 + (j as f32 + 0.5) / SHORE_RES as f32;
                for i in 0..SHORE_RES {
                    let px = tx as f32 + (i as f32 + 0.5) / SHORE_RES as f32;
                    let mut shore = SHORE_MAX;
                    for l in &land[..nland] {
                        let ddx = (l[0] - px).max(px - (l[0] + 1.0)).max(0.0);
                        let ddz = (l[1] - pz).max(pz - (l[1] + 1.0)).max(0.0);
                        shore = shore.min((ddx * ddx + ddz * ddz).sqrt());
                    }
                    field[(tz * SHORE_RES + j) * s + tx * SHORE_RES + i] =
                        (shore / SHORE_MAX * 255.0).round() as u8;
                }
            }
        }
    }
}

/// Diff two tile-type planes and rebake (against `new`) every tile
/// whose 7x7 kernel saw a change — the incremental arm of the shore
/// bake for runtime terrain mutation.
fn rebake_shore_changed(field: &mut [u8], old: &[u8], new: &[u8], n: usize) {
    let mut dirty = vec![false; n * n];
    for z in 0..n {
        for x in 0..n {
            if old[z * n + x] == new[z * n + x] {
                continue;
            }
            for dz in -3i32..=3 {
                for dx in -3i32..=3 {
                    let wx = (x as i32 + dx).rem_euclid(n as i32) as usize;
                    let wz = (z as i32 + dz).rem_euclid(n as i32) as usize;
                    dirty[wz * n + wx] = true;
                }
            }
        }
    }
    for z in 0..n {
        for x in 0..n {
            if dirty[z * n + x] {
                bake_shore_region(new, n, field, x, z, 1, 1);
            }
        }
    }
}

// Both maps are player-centered, yaw-rotated and toroidally wrapping.
// World spans derive from the original's DrawMinimap_49300 params:
// span_tiles = a6 * a8 / a5 / 256 (BYTE1 tile step; hi-res halves
// a5/a6 and doubles a8, cancelling).
//
/// Book-screen (Enter) map zoom. The original passes 382/378/a8=170 →
/// ~251 tiles, JUST short of the 256-tile world, which is why its edges
/// clip ("questionable things at the edges"). This spans the FULL world
/// so nothing is cut (deliberate). Toroidal wrap makes it appear
/// infinite (the original's rounding-error void-mobs live at that wrap;
/// not reproduced).
const BOOK_MAP_ZOOM: f32 = MAP_TILES as f32;

/// The UI's authored coordinate space: the original's hi-res mode.
pub const NATIVE_W: f32 = 640.0;
pub const NATIVE_H: f32 = 480.0;

/// The non-4:3 presentation law, in one place.
///
/// Every HUD element is authored in the original's 640×480 native
/// coordinates. Historically we mapped them with the two INDEPENDENT
/// factors `w/640` and `h/480`, which is a stretch: at 16:9 the art
/// smears horizontally, and the further from 4:3 the worse it reads.
///
/// The law here instead uses ONE uniform scale, `s = min(w/640,
/// h/480)`, so native art never distorts, and spends the leftover
/// slack on ANCHORING rather than stretching:
///
/// * Wider than 4:3 (`s = h/480`): the vertical is exact and the
///   horizontal has slack. Left-anchored groups (the castle/wizard
///   controls) hug x=0; right-anchored groups (the equipped spell
///   hands) hug x=w. The gap opens in the MIDDLE of the top strip,
///   where the live sky shows through — the panels stay where the eye
///   and the mouse expect them, at the screen corners.
/// * Narrower than 4:3 (`s = w/640`): the horizontal is exact — the
///   whole HUD simply shrinks to match the screen width, exactly as
///   asked — and the vertical slack goes to the top/bottom anchors, so
///   the strip stays at the top and the selector pane at the bottom.
/// * Exactly 4:3: `s = w/640 = h/480` and every anchor collapses onto
///   the authored coordinate. The 4:3 layout is bit-for-bit what it was
///   before this law existed — that is the invariant the tests pin.
///
/// The 3D view is deliberately NOT part of this: it fills its rect and
/// takes its aspect from it, so the FOV widens/narrows with the screen
/// while pixels stay square (see `mgc_render::camera_matrix`).
#[derive(Debug, Clone, Copy)]
pub struct HudFrame {
    /// Uniform native→physical scale.
    pub s: f32,
    /// Physical viewport size.
    pub w: f32,
    pub h: f32,
}

impl HudFrame {
    pub fn new(w: f32, h: f32) -> Self {
        Self {
            s: (w / NATIVE_W).min(h / NATIVE_H),
            w,
            h,
        }
    }

    /// A native length in physical px.
    pub fn len(&self, n: f32) -> f32 {
        n * self.s
    }

    /// Native x, LEFT-anchored (measured from the screen's left edge).
    pub fn lx(&self, x: f32) -> f32 {
        x * self.s
    }

    /// Native x, RIGHT-anchored: `x` is still the authored 640-space
    /// coordinate, but its distance from the RIGHT edge is what is
    /// preserved.
    pub fn rx(&self, x: f32) -> f32 {
        self.w - (NATIVE_W - x) * self.s
    }

    /// Native y, TOP-anchored.
    pub fn ty(&self, y: f32) -> f32 {
        y * self.s
    }

    /// Native y, BOTTOM-anchored (distance from the bottom preserved).
    pub fn by(&self, y: f32) -> f32 {
        self.h - (NATIVE_H - y) * self.s
    }

    /// Native x, CENTER-anchored: the authored offset from the 640-space
    /// center is preserved about the physical center.
    pub fn cx(&self, x: f32) -> f32 {
        self.w * 0.5 + (x - NATIVE_W * 0.5) * self.s
    }
}

// The book/map screen topology (sub_20E60 case 4 + the spellbook grid
// at :26915), in the original's hi-res 640×480 native coordinates,
// scaled to the live resolution by w/640, hpx/480. The live world fills
// the background; the map pane and spellbook overlay it, leaving the
// world visible in the top-right L-remainder and the bottom log strip.
/// The book map pane: `DrawMinimap(0,0, 382,378, ...)` at the top-left
/// corner (native px).
const BOOK_MAP_X: f32 = 0.0;
const BOOK_MAP_Y: f32 = 0.0;
// Book/map screen native geometry, MEASURED from a hi-res retail
// screenshot, which is senior over the decompile's raw DrawMinimap args
// (382×378 was the sample size, not the on-screen pane). Layout: map
// pane top-left, world viewport top-right, spellbook bottom-right,
// ~64px black bar along the bottom. There is a 2px BLACK GAP forming a
// "T" between the three panes — taken out of the MAP and the LIVE VIEW,
// NOT the spellbook (which is 1:1 to retail).
//   spellbook:  x 384..640, y 194..416 (4 cols × 6 rows of 64×37) — FIXED
//   map:        (0,0) (384−GAP) × 416   [right edge recedes for the gap]
//   viewport:   x 384..640, y 0..(194−GAP)   [bottom recedes for the gap]
//   bottom bar: y 416..480 (black)
/// The 2px black demarcation between the book panes (native px).
const BOOK_GAP: f32 = 2.0;
/// The map pane's native BOTTOM (its width is no longer a constant —
/// it is derived per frame as "everything left of the spellbook
/// column, less the gap", which at 4:3 comes out to the authored
/// 384 − BOOK_GAP and at any other aspect is the whole remainder).
/// Public: the app's map-screen hover tests (the mana-roster strip)
/// share this bottom edge.
pub const BOOK_MAP_H: f32 = 416.0;
/// The spellbook grid origin (native px): 24 spells in 4 cols × 6 rows
/// of the slot-slab [3] = 64×37, tightly packed from (384,194). FIXED —
/// the gap is taken from the map/viewport, not here. The grid is drawn
/// app-side (`ui::book_quads` consumes these same constants — ONE
/// source for the measured layout); the renderer needs the LEFT + TOP
/// to place the world viewport.
pub const BOOK_SPELL_X: f32 = 384.0;
pub const BOOK_SPELL_Y: f32 = 194.0;

/// Which topology the fullscreen map screen uses (per game / per the
/// `spell_selector` option — the book half exists only where the MC1
/// map spellbook is live).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapScreenLayout {
    /// MC1's book screen: map pane top-left, world viewport top-right
    /// (aspect-true to its rect), spellbook bottom-right (app-drawn).
    #[default]
    Mc1Book,
    /// MC2's map screen (remc2 EF:21804-21871 + UI:770-782): minimap
    /// strip x 0..382, live world x 384..640 × y 0..400 rendered with
    /// the FLIGHT projection — the same FOV into a narrower
    /// destination reads horizontally squeezed, the authentic
    /// non-aspect live view. Bottom 80 native px stay black (the CTRL
    /// selector pane's zone). No spellbook half.
    Mc2Split,
}

/// MC2 map screen: the live-view/minimap bottom edge (native px;
/// remc2 `locViewportHeight/locMinimapHeight = 400`, EF:21804).
pub const MC2_MAP_VIEW_H: f32 = 400.0;
/// MC2 map-screen zoom (EF:21840-49): retail `DrawMinimap_63600`
/// scaling = 204 world-units/px over the 400-native-px pane height →
/// 400·204/256 = 318.75 tiles vertically — the whole 256-tile world
/// plus ~25% toroidal repeat, player-centered and yaw-rotated. Retail
/// blits the terrain as a SQUARE 318.75-tile region squished into the
/// 382-wide pane while its entity layer runs isotropic at 204 — a
/// ~4.6% horizontal terrain/entity misalignment we deliberately do
/// NOT reproduce: both our layers use the isotropic (entity) law,
/// 382·204/256 = 304.4 tiles across the native width.
const MC2_MAP_VIEW_SPAN_TILES: f32 = MC2_MAP_VIEW_H * 204.0 / 256.0;
// The HUD top strip is six tiles packed left-to-right from x=2 with 0px
// gaps (pixel-measured, matched to native sprite widths at scale
// 1.668): [40] radar frame (124) | three [41] sub-panels
// (128 each) | two spell frames [1]/[2] (64 each). Native tile origins:
// 2, 126, 254, 382, 510, 574.
/// In-flight radar: the disc is anchored at the screen CORNER (0,0) and
/// spans the full 128 native px — it touches both edges with NO margin
/// (retail: DrawMinimap(0,0,128,128,...); the [40] frame sprite is what
/// leaves the visible margin, drawn on top). So the disc is slightly
/// bigger than its frame tile, and radar objects read slightly larger.
/// Native px, scaled by w/640 to track the panels. Zoom stays FAITHFUL
/// at 128 tiles across; `+`/`-` adjust it at runtime.
const MINIMAP_DIAM: f32 = 128.0;
const MINIMAP_ZOOM: f32 = 128.0;
/// HUD transparency alpha (radar + panels; kept in sync with ui.rs's
/// PANEL_TINT). The whole HUD blends over the sky in faithful MC1.
pub const HUD_PANEL_ALPHA: f32 = 0.62;
/// Runtime radar-zoom bounds (`+`/`-`): from a tight 32-tile crop out
/// to a near-whole-world 224 tiles.
const MINIMAP_ZOOM_MIN: f32 = 32.0;
const MINIMAP_ZOOM_MAX: f32 = 224.0;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

enum Target {
    Window {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    Offscreen {
        color: wgpu::Texture,
        width: u32,
        height: u32,
    },
}

/// The supersample buffer: the whole frame rendered larger than the
/// window, then averaged down on the way to the surface.
struct Ssaa {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    /// Scaled pixel size of the buffer.
    size: (u32, u32),
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    depth: wgpu::TextureView,
    /// MSAA sample count baked into every pipeline; 1 = off. Fixed at
    /// construction — changing it means rebuilding all nine pipelines,
    /// so the option is startup-only.
    samples: u32,
    /// The multisampled colour target + its depth, when `samples > 1`.
    /// The main pass renders here and RESOLVES into the surface (or the
    /// supersample buffer, though the two modes are exclusive).
    msaa_color: Option<wgpu::TextureView>,
    /// The mirror pass shares its pipelines with the main pass, so its
    /// target has to carry the same sample count and resolve into the
    /// sampled reflection texture the water reads.
    msaa_mirror: Option<wgpu::TextureView>,
    msaa_mirror_size: (u32, u32),
    /// Supersampling factor, 1.0 = off. At 1.0 there is no offscreen
    /// buffer and no resolve pass at all — the frame goes straight to
    /// the surface exactly as it did before this existed.
    render_scale: f32,
    /// The offscreen scene buffer + its resolve bind group; `None`
    /// whenever `render_scale` is 1.0 or the target is already
    /// offscreen (screenshots render at their own size).
    ssaa: Option<Ssaa>,
    ssaa_pipeline: wgpu::RenderPipeline,
    ssaa_layout: wgpu::BindGroupLayout,
    ssaa_sampler: wgpu::Sampler,
    /// Color format of the render target (the mirror texture must
    /// match it — the reflection pass reuses the terrain pipeline).
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    /// The water-reflection MIRROR pass twin of `globals_buf`
    /// (atlas.w = 2 — the shader's y-flip arm).
    mirror_globals_buf: wgpu::Buffer,
    /// Terrain bind group over the mirror globals; rebuilt with
    /// `bind_group` at level load.
    mirror_bind_group: Option<wgpu::BindGroup>,
    /// Group-1 (mirror + sky textures) machinery: layout + shared
    /// samplers + the dummies bound when a real image is absent.
    reflection_layout: wgpu::BindGroupLayout,
    reflection_sampler: wgpu::Sampler,
    reflection_dummy_bind_group: wgpu::BindGroup,
    /// The 1x1 dummy for the group-1 mirror slot (kept for bind-group
    /// rebuilds on sky load/clear).
    mirror_dummy_view: wgpu::TextureView,
    /// Group-1 sky slot: the loaded parallax sky's view (`load_sky`,
    /// the fog/extinction melt target), or None — the 1x1
    /// fog-constant dummy is bound instead.
    sky_view: Option<wgpu::TextureView>,
    /// Repeat sampler shared by the sky pass and the terrain melt.
    sky_sampler: wgpu::Sampler,
    /// 1x1 fog-constant fallback for the group-1 sky slot; its texel
    /// is kept in sync by `set_sky_color`.
    sky_dummy_tex: wgpu::Texture,
    sky_dummy_view: wgpu::TextureView,
    /// The mirror render target (recreated on resize) + its group-1
    /// bind group for the main pass. The bind group's mirror slot
    /// carries the BLURRED B target, never the raw mirror image.
    reflection_view: Option<wgpu::TextureView>,
    reflection_bind_group: Option<wgpu::BindGroup>,
    reflection_size: (u32, u32),
    /// Reflection-blur chain (blur.wgsl): H/V pipelines + the
    /// 1/[`REFLECTION_BLUR_DIV`]-res A (H output) and B (V output —
    /// what the water actually samples) targets, recreated with the
    /// mirror target on resize.
    blur_layout: wgpu::BindGroupLayout,
    blur_h_pipeline: wgpu::RenderPipeline,
    blur_v_pipeline: wgpu::RenderPipeline,
    blur_a_view: Option<wgpu::TextureView>,
    blur_b_view: Option<wgpu::TextureView>,
    blur_h_bind_group: Option<wgpu::BindGroup>,
    blur_v_bind_group: Option<wgpu::BindGroup>,
    /// Water reflections on (config `render.preference.reflections`).
    reflections: bool,
    /// Live dynamic lights (already gated app-side to Night/Cave +
    /// the option), capped at [`MAX_LIGHTS`].
    lights: Vec<[f32; 4]>,
    /// The CEILING pass twin of `globals_buf` (`atlas.w = 1` — the
    /// shader's cave-ceiling arm selector); only written/drawn when
    /// the level carries a ceiling plane.
    ceiling_globals_buf: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    /// The MC2 cave ceiling pass: the same terrain grid drawn again
    /// with the ceiling heightmap (fixed wall texture, no water
    /// animation). None off-cave.
    ceiling_bind_group: Option<wgpu::BindGroup>,
    ceiling_tex: Option<wgpu::Texture>,
    vertex_buf: Option<wgpu::Buffer>,
    index_buf: Option<wgpu::Buffer>,
    index_count: u32,
    /// Cell count of the loaded terrain atlas (0 = render flat colors).
    atlas_cells: u32,
    /// The level's water-wave rule, as a shader selector (0/1/2).
    wave_mode: u32,
    /// Animation clock in original game turns (fractional between
    /// ticks); drives the water wave and sprite frame cycling.
    anim_turn: f32,
    /// Interpolate per-tile shade across tile centers (enhancement,
    /// off = the original's per-tile shade snap).
    smooth_shading: bool,
    /// Fog view distance in tiles (full occlusion; 0 = fog off).
    /// [`DEFAULT_FOG_TILES`] = the retail band.
    fog_distance: f32,
    /// Sky/fog color (sRGB): [`SKY_SRGB`] default, overridden per MC2
    /// environment from the bundle (shade LUT row 0 — night = black).
    sky_srgb: [f32; 3],
    /// The book screen (the original's Enter view): overhead map on the
    /// right half, left half reserved for the spell list.
    map_view: bool,
    /// Which topology the map screen uses (MC1 book vs MC2 split).
    map_layout: MapScreenLayout,
    map_pipeline: wgpu::RenderPipeline,
    /// The extent-fog overlay (opt-in deviation): black past the
    /// rotated true-extent rectangle, drawn over every map layer on
    /// the map screens. Same quad/globals as the map pane.
    fog_pipeline: wgpu::RenderPipeline,
    /// Whether the extent fog draws (`set_extent_fog`).
    extent_fog: bool,
    map_globals_buf: wgpu::Buffer,
    map_bind_group_layout: wgpu::BindGroupLayout,
    map_bind_group: Option<wgpu::BindGroup>,
    /// The in-flight round minimap (top-left corner): its own uniform +
    /// bind group over the SAME world map texture, drawn during normal
    /// flight (the book screen uses `map_bind_group`). None until a
    /// level is loaded (which is what gates the draw).
    minimap_globals_buf: wgpu::Buffer,
    minimap_bind_group: Option<wgpu::BindGroup>,
    /// Runtime radar zoom (tiles across the disc); `+`/`-` adjust it.
    minimap_zoom: f32,
    /// Runtime MAP-SCREEN zoom (`+`/`-` while the map is open — a
    /// port addition like the radar zoom, no retail analogue): a
    /// multiplier on the layout's base span. 1.0 = the full default
    /// view; clamped so the tightest crop is 1/8 of it (32 tiles on
    /// the MC1 book). Session-only — never persisted, like the
    /// radar's.
    map_zoom_mult: f32,
    /// Radar output alpha — HUD transparency (1 = opaque; the MC1
    /// default matches the translucent panels, MC2/opaque = 1).
    minimap_alpha: f32,
    /// Level-end fade coverage over the screen-space map markers
    /// (0 = none, 1 = black). The app's fade quad rides in
    /// `ui_quads`, but in FLIGHT the stamps draw after the app UI
    /// (the dots must read over the radar frame art), so the fade
    /// quad cannot cover them — they dim themselves instead.
    overlay_fade: f32,
    fill_pipeline: wgpu::RenderPipeline,
    /// The textured parallax-sky pass; the bind groups exist only
    /// while a level's sky bitmap is loaded (see `load_sky`). The
    /// mirror twin binds the mirror globals (atlas.w = 2 — the
    /// shader's reflected-ray arm) for the reflection pass.
    sky_pipeline: wgpu::RenderPipeline,
    sky_bind_group_layout: wgpu::BindGroupLayout,
    sky_bind_group: Option<wgpu::BindGroup>,
    sky_mirror_bind_group: Option<wgpu::BindGroup>,
    fill_bind_group: wgpu::BindGroup,
    // Billboard (world sprite) pass.
    billboard_pipeline: wgpu::RenderPipeline,
    billboard_blend_pipeline: wgpu::RenderPipeline,
    billboard_bind_group_layout: wgpu::BindGroupLayout,
    billboard_bind_group: Option<wgpu::BindGroup>,
    /// Billboard bind group over the mirror globals — sprite
    /// reflections in the water pass; rebuilt with its twin.
    billboard_mirror_bind_group: Option<wgpu::BindGroup>,
    billboard_buf: Option<wgpu::Buffer>,
    billboard_capacity: usize,

    // PROTOTYPE fire pass (throwaway): premultiplied-additive flame
    // discs over the world, sharing the globals bind group.
    fire_pipeline: wgpu::RenderPipeline,
    fire_bind_group: wgpu::BindGroup,
    /// Fire bind group over the mirror globals (atlas.w = 2) — draws the
    /// flames into the water-reflection pass.
    fire_mirror_bind_group: wgpu::BindGroup,
    fire_buf: Option<wgpu::Buffer>,
    fire_capacity: usize,
    fire_particles: Vec<FireParticle>,
    // PROTOTYPE lightning-bolt pass (enhanced lightning): thin glowing
    // ribbons, sharing the fire pass's globals bind groups.
    bolt_pipeline: wgpu::RenderPipeline,
    bolt_buf: Option<wgpu::Buffer>,
    bolt_capacity: usize,
    bolt_segments: Vec<BoltSegment>,
    /// CPU copy of the sprite index for per-frame view selection.
    sprite_index: Option<mgc_formats::bundle::SpriteIndex>,
    sprite_tex: Option<wgpu::Texture>,
    colormap_tex: Option<wgpu::Texture>,
    billboards: Vec<Billboard>,
    // Health-bar overlay pass (unfaithful debug enhancement).
    bar_pipeline: wgpu::RenderPipeline,
    bar_bind_group: wgpu::BindGroup,
    bar_buf: Option<wgpu::Buffer>,
    bar_capacity: usize,
    health_bars: Vec<HealthBar>,
    // Screen-space UI pass (spellbook / HUD).
    ui_pipeline: wgpu::RenderPipeline,
    ui_bind_group_layout: wgpu::BindGroupLayout,
    ui_globals_buf: wgpu::Buffer,
    ui_bind_group: Option<wgpu::BindGroup>,
    ui_buf: Option<wgpu::Buffer>,
    ui_capacity: usize,
    ui_quads: Vec<UiQuad>,
    /// Upright screen-space map icons (castle/balloon), projected onto
    /// whichever map surface is active each frame. World-positioned but
    /// drawn unrotated so they always point up.
    map_stamps: Vec<MapStamp>,
    /// Marker-size multiplier for the maps' entity dots + icon stamps
    /// (`set_marker_scale`; opt-in deviation). 1.0 = the baseline:
    /// dots baked into the map texture as tile texels. Any other value
    /// lifts the dots out of the bake into `screen_dots`, drawn
    /// screen-space at a size that no longer varies with radar zoom.
    marker_scale: f32,
    /// The dots lifted out of the texture bake when `marker_scale !=
    /// 1.0` (tile position, palette color resolved to a linear tint).
    /// Refreshed with the map texture every `update_map`.
    screen_dots: Vec<ScreenDot>,
    /// The marching-ants guide path (player → castle), projected onto
    /// the active map surface each frame in 4-surface-px steps.
    map_path: Option<MapPath>,
    /// The MC2 objective-guide targets (blinking marks + steer arrow),
    /// projected onto the active map surface each frame. Empty off-MC2
    /// or when the current objective has nothing spatial to point at.
    objective_marks: Vec<ObjectiveMark>,
    /// The sim tick, set with the marks — drives the retail blink gates
    /// (outline 1-in-4, arrow 5-then-pause) in `project_objective_marks`.
    objective_tick: u32,
    /// UI atlas dimensions, needed to convert stamp texel UVs. Set by
    /// `load_ui_atlas`.
    ui_atlas_size: (u32, u32),
    /// Terrain plane textures [type, shade, angle, height] kept for
    /// runtime updates (craters, quakes — `update_terrain`).
    plane_texs: Option<[wgpu::Texture; 4]>,
    /// Baked shore-distance plane (the shader's shore-haze law,
    /// precomputed — see `bake_shore_region`), its CPU image, and the
    /// tile-type snapshot it was baked from: `update_terrain` diffs
    /// against the snapshot and rebakes only around changed tiles.
    shore_tex: Option<wgpu::Texture>,
    shore_field: Vec<u8>,
    shore_types: Vec<u8>,
    /// Deep-water (tile type 0) presence per 8x8-tile block, plus the
    /// level-wide flag — the mirror-pass visibility gate: no deep
    /// water within visible reach means no fragment can sample the
    /// mirror, so the whole reflection pre-pass is skipped.
    water_blocks: Vec<bool>,
    has_deep_water: bool,
    /// Overhead map texture, rewritten when terrain/entities change.
    map_tex: Option<wgpu::Texture>,
}

#[derive(Debug)]
pub enum RenderError {
    NoAdapter,
    Device(String),
    Surface(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "no compatible GPU adapter found"),
            Self::Device(e) => write!(f, "device: {e}"),
            Self::Surface(e) => write!(f, "surface: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl Renderer {
    /// Renderer presenting to a winit window.
    /// `samples` is the MSAA sample count (1 = off). It is fixed for
    /// the renderer's life: every pipeline bakes it in, so changing it
    /// means rebuilding all of them — the option is startup-only.
    pub fn for_window(
        window: Arc<winit::window::Window>,
        samples: u32,
    ) -> Result<Self, RenderError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window)
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        let (adapter, device, queue) = request_device(&instance, Some(&surface))?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or(RenderError::NoAdapter)?;
        // Prefer an sRGB format so shader output is linear color.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *srgb;
        }
        surface.configure(&device, &config);
        let format = config.format;
        let (width, height) = (config.width, config.height);
        Ok(Self::finish_init(
            device,
            queue,
            Target::Window { surface, config },
            format,
            width,
            height,
            samples.clamp(1, 8),
        ))
    }

    /// Renderer drawing into an offscreen texture (screenshot mode,
    /// used for autonomous end-to-end verification).
    pub fn offscreen(width: u32, height: u32) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let (_adapter, device, queue) = request_device(&instance, None)?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Ok(Self::finish_init(
            device,
            queue,
            Target::Offscreen {
                color,
                width,
                height,
            },
            format,
            width,
            height,
            // Screenshots render at an exact size with no AA.
            1,
        ))
    }

    fn finish_init(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: Target,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        samples: u32,
    ) -> Self {
        // Every pipeline that draws into the scene target must agree
        // with it on the sample count — including the ones the MIRROR
        // pass reuses, which is why the reflection buffer is
        // multisampled too rather than those pipelines being duplicated.
        let ms = wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Tile types and shading feed the vertex stage too (the
                // per-corner water-wave gates), like the angle plane.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Height plane (vertex-stage altitude; runtime terrain
                // mutation rewrites it).
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Baked shore-distance field (R8Unorm; the shore-haze
                // law precomputed on the CPU, read with textureLoad).
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        // Group 1: the water-reflection mirror texture (the previous
        // mirror pass's output) — a 1x1 dummy when reflections are off
        // or inside the mirror pass itself — plus the sky slot: the
        // parallax sky bitmap the fog/extinction melts fade into (a
        // 1x1 fog-constant dummy when no sky is loaded). Always bound.
        let reflection_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reflection"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[&bind_group_layout, &reflection_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ceiling_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ceiling globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mirror_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mirror globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The always-bound group-1 resources: a linear clamping
        // sampler and a 1x1 dummy mirror texture for passes that must
        // not (mirror) or cannot (reflections off) sample one.
        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("reflection"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflection dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // The group-1 sky slot: the same repeat sampler the sky pass
        // uses, and a 1x1 fallback texel kept on the flat fog constant
        // (`set_sky_color`) for levels with no sky texture — the
        // shader's melts then degenerate to the plain constant fade.
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sky_dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            sky_dummy_tex.as_image_copy(),
            &sky_texel(SKY_SRGB),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let mirror_dummy_view = dummy_tex.create_view(&Default::default());
        let sky_dummy_view = sky_dummy_tex.create_view(&Default::default());
        let reflection_dummy_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reflection dummy"),
            layout: &reflection_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mirror_dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&reflection_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&sky_dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sky_sampler),
                },
            ],
        });

        // The reflection-blur pipelines (blur.wgsl): a separable
        // gaussian softens the mirror image before the water samples
        // it (player ask 2026-08-08 — retail's 320x200 reflection
        // block was inherently soft; a modern-res pixel-perfect
        // mirror reads "too clean"). Runs at 1/REFLECTION_BLUR_DIV
        // resolution, only when the mirror pass itself runs.
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur"),
            bind_group_layouts: &[&blur_layout],
            push_constant_ranges: &[],
        });
        let make_blur = |label: &str, entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&blur_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blur_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blur_shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let blur_h_pipeline = make_blur("blur-h", "fs_h");
        let blur_v_pipeline = make_blur("blur-v", "fs_v");

        // The map (book screen) pass: fullscreen-quad pipeline over the
        // CPU-composed map texture.
        let map_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });
        let map_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("map"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let map_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map"),
            bind_group_layouts: &[&map_bind_group_layout],
            push_constant_ranges: &[],
        });
        let map_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map"),
            layout: Some(&map_layout),
            vertex: wgpu::VertexState {
                module: &map_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &map_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Alpha blend so the in-flight radar can be
                    // translucent (HUD transparency); the book map and
                    // opaque-HUD radar pass alpha = 1 (a no-op blend).
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });
        // The extent-fog overlay: the same quad + globals as the map
        // pane, fragment `fs_fog` (black past the rotated true-extent
        // rectangle). Drawn between the map-layer quads and the app UI.
        let fog_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map extent fog"),
            layout: Some(&map_layout),
            vertex: wgpu::VertexState {
                module: &map_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &map_shader,
                entry_point: Some("fs_fog"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });
        let map_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map globals"),
            size: 48, // 3 vec4: rect, player(x,z,yaw,zoom), mode(round,aspect,_,_)
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let minimap_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimap globals"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Solid sky fill behind the book screen's world viewport. The
        // color comes from the globals' fog_color (the environment
        // sky), so it tracks `set_sky_color` like the pass clear.
        let fill_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fill"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fill.wgsl").into()),
        });
        let fill_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("fill"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let fill_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fill"),
            layout: &fill_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let fill_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fill"),
            bind_group_layouts: &[&fill_bind_group_layout],
            push_constant_ranges: &[],
        });
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fill"),
            layout: Some(&fill_layout),
            vertex: wgpu::VertexState {
                module: &fill_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fill_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        // Parallax sky pass: the baked 256x256 sky bitmap steered by
        // the camera ray (see sky.wgsl). Same one-triangle/no-depth
        // shape as the fill pass; the bind group is built by
        // `load_sky` when a level has a sky texture and the option is
        // on — absent, the flat fill/clear IS the sky (retail's
        // sky-off keyColor fill).
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let sky_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sky"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[&sky_bind_group_layout],
            push_constant_ranges: &[],
        });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&sky_layout),
            vertex: wgpu::VertexState {
                module: &sky_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        // Supersample resolve: the offscreen scene buffer blitted to
        // the surface through a linear sampler. Built always, used only
        // when `render_scale > 1`.
        let ssaa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssaa"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let ssaa_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssaa"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ssaa_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ssaa"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ssaa_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssaa"),
            bind_group_layouts: &[&ssaa_layout],
            push_constant_ranges: &[],
        });
        let ssaa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ssaa"),
            layout: Some(&ssaa_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ssaa_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ssaa_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            // The resolve writes to the SURFACE, which is never
            // multisampled — this one pipeline stays single-sampled
            // whatever `samples` is.
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });

        // Billboard pass: instanced screen-aligned quads over the
        // sprite atlas, same colormap as terrain.
        let billboard_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("billboard"),
            source: wgpu::ShaderSource::Wgsl(include_str!("billboard.wgsl").into()),
        });
        let billboard_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("billboard"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let billboard_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("billboard"),
            // Group 1 = the terrain pass's mirror+sky group: sprites
            // read only its sky slots, for the same fog/extinction
            // melts as terrain (billboard.wgsl sky_backdrop).
            bind_group_layouts: &[&billboard_bind_group_layout, &reflection_layout],
            push_constant_ranges: &[],
        });
        let billboard_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("billboard"),
            layout: Some(&billboard_layout),
            vertex: wgpu::VertexState {
                module: &billboard_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BillboardInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &BILLBOARD_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &billboard_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });
        // Translucent billboards (retail raster modes 2/3 → alpha
        // 1/3 / 2/3): same shader, alpha blending, depth TEST only —
        // instances draw back-to-front after all opaque world work,
        // standing in for retail's inline painter-order LUT blend.
        let billboard_blend_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("billboard-blend"),
                layout: Some(&billboard_layout),
                vertex: wgpu::VertexState {
                    module: &billboard_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<BillboardInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &BILLBOARD_ATTRS,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &billboard_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: ms,
                multiview: None,
                cache: None,
            });

        // PROTOTYPE fire pass: premultiplied-additive flame discs.
        // Globals-only bind group (camera basis + fog); own blend so
        // hot cores add and cool soot occludes.
        let fire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fire"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fire.wgsl").into()),
        });
        let fire_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("fire"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let fire_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fire"),
            layout: &fire_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let fire_mirror_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fire-mirror"),
            layout: &fire_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: mirror_globals_buf.as_entire_binding(),
            }],
        });
        let fire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fire"),
            bind_group_layouts: &[&fire_bind_group_layout],
            push_constant_ranges: &[],
        });
        // Premultiplied alpha: result = src.rgb + dst.rgb*(1-src.a).
        // src.a≈0 (hot) → additive glow; src.a>0 (soot) → over-occlude.
        let premul = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let fire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fire"),
            layout: Some(&fire_layout),
            vertex: wgpu::VertexState {
                module: &fire_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FireInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Float32, 4 => Float32,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fire_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premul),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        // PROTOTYPE lightning-bolt ribbons: same globals layout and
        // premultiplied blend as fire, segment-endpoint instances.
        let bolt_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bolt"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bolt.wgsl").into()),
        });
        let bolt_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bolt"),
            layout: Some(&fire_layout),
            vertex: wgpu::VertexState {
                module: &bolt_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BoltInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32,
                        4 => Float32, 5 => Float32,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &bolt_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premul),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        // Health-bar overlay: solid-color instanced quads on the same
        // camera basis; own single-binding layout so bars draw even
        // before any sprite atlas is loaded.
        let bar_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bar"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bar.wgsl").into()),
        });
        let bar_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bar"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let bar_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bar"),
            layout: &bar_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let bar_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bar"),
            bind_group_layouts: &[&bar_bind_group_layout],
            push_constant_ranges: &[],
        });
        let bar_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bar"),
            layout: Some(&bar_layout),
            vertex: wgpu::VertexState {
                module: &bar_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BarInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x2, 2 => Float32,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &bar_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        // Screen-space UI pass (spellbook / HUD): pixel-space textured
        // quads over an RGBA atlas the app pre-composites through the
        // engine's blend LUT. Alpha-blended, no depth.
        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let ui_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ui"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });
        let ui_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ui_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[&ui_bind_group_layout],
            push_constant_ranges: &[],
        });
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&ui_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<UiQuad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4, 1 => Float32x4, 2 => Float32x4,
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: ms,
            multiview: None,
            cache: None,
        });

        let depth = create_depth(&device, width, height, samples);

        Self {
            device,
            queue,
            target,
            depth,
            pipeline,
            globals_buf,
            bind_group_layout,
            bind_group: None,
            ceiling_globals_buf,
            ceiling_bind_group: None,
            ceiling_tex: None,
            vertex_buf: None,
            index_buf: None,
            index_count: 0,
            atlas_cells: 0,
            wave_mode: 0,
            anim_turn: 0.0,
            plane_texs: None,
            shore_tex: None,
            shore_field: Vec::new(),
            shore_types: Vec::new(),
            water_blocks: Vec::new(),
            has_deep_water: false,
            map_tex: None,
            smooth_shading: false,
            fog_distance: DEFAULT_FOG_TILES,
            sky_srgb: SKY_SRGB,
            map_view: false,
            map_layout: MapScreenLayout::default(),
            map_pipeline,
            fog_pipeline,
            extent_fog: false,
            map_globals_buf,
            minimap_globals_buf,
            minimap_bind_group: None,
            minimap_zoom: MINIMAP_ZOOM,
            map_zoom_mult: 1.0,
            minimap_alpha: 1.0,
            overlay_fade: 0.0,
            map_bind_group_layout,
            map_bind_group: None,
            fill_pipeline,
            fill_bind_group,
            sky_pipeline,
            sky_bind_group_layout,
            sky_bind_group: None,
            sky_mirror_bind_group: None,
            billboard_mirror_bind_group: None,
            format,
            mirror_globals_buf,
            mirror_bind_group: None,
            reflection_layout,
            reflection_sampler,
            reflection_dummy_bind_group,
            mirror_dummy_view,
            sky_view: None,
            sky_sampler,
            sky_dummy_tex,
            sky_dummy_view,
            blur_layout,
            blur_h_pipeline,
            blur_v_pipeline,
            blur_a_view: None,
            blur_b_view: None,
            blur_h_bind_group: None,
            blur_v_bind_group: None,
            reflection_view: None,
            reflection_bind_group: None,
            reflection_size: (0, 0),
            samples,
            msaa_color: None,
            msaa_mirror: None,
            msaa_mirror_size: (0, 0),
            render_scale: 1.0,
            ssaa: None,
            ssaa_pipeline,
            ssaa_layout,
            ssaa_sampler,
            reflections: true,
            lights: Vec::new(),
            billboard_pipeline,
            billboard_blend_pipeline,
            billboard_bind_group_layout,
            billboard_bind_group: None,
            billboard_buf: None,
            billboard_capacity: 0,
            fire_pipeline,
            fire_bind_group,
            fire_mirror_bind_group,
            fire_buf: None,
            bolt_pipeline,
            bolt_buf: None,
            bolt_capacity: 0,
            bolt_segments: Vec::new(),
            fire_capacity: 0,
            fire_particles: Vec::new(),
            sprite_index: None,
            sprite_tex: None,
            colormap_tex: None,
            billboards: Vec::new(),
            bar_pipeline,
            bar_bind_group,
            bar_buf: None,
            bar_capacity: 0,
            health_bars: Vec::new(),
            ui_pipeline,
            ui_bind_group_layout,
            ui_globals_buf,
            ui_bind_group: None,
            ui_buf: None,
            ui_capacity: 0,
            ui_quads: Vec::new(),
            map_stamps: Vec::new(),
            marker_scale: 1.0,
            screen_dots: Vec::new(),
            map_path: None,
            objective_marks: Vec::new(),
            objective_tick: 0,
            ui_atlas_size: (1, 1),
        }
    }

    /// Toggle the book screen (overhead map + reserved spell half).
    pub fn set_map_view(&mut self, on: bool) {
        self.map_view = on;
    }

    pub fn map_view(&self) -> bool {
        self.map_view
    }

    /// Select the map screen's topology (MC1 book vs MC2 split); set
    /// once at level load from the game + `spell_selector` resolution.
    pub fn set_map_layout(&mut self, layout: MapScreenLayout) {
        self.map_layout = layout;
    }

    /// The fullscreen map pane's zoom, in the map shader's convention
    /// (tiles across the pane's SHORTER pixel axis; `aspect` = pane
    /// w/h in px): the layout base span times the runtime `+`/`-`
    /// multiplier ([`Self::zoom_map_screen`]).
    fn map_pane_zoom(&self, aspect: f32) -> f32 {
        self.map_pane_zoom_base(aspect) * self.map_zoom_mult
    }

    /// The layout's UNZOOMED base span. MC1 book: the full world
    /// (deliberate — see [`BOOK_MAP_ZOOM`]). MC2 split: the faithful
    /// retail span — the VERTICAL axis always shows
    /// [`MC2_MAP_VIEW_SPAN_TILES`] (318.75, EF:21840-49) whichever
    /// axis is currently shorter, so window aspect only widens the
    /// horizontal wrap. Also the screen-space dots' size reference,
    /// so map-screen zoom keeps markers constant like radar zoom
    /// does.
    fn map_pane_zoom_base(&self, aspect: f32) -> f32 {
        match self.map_layout {
            MapScreenLayout::Mc1Book => BOOK_MAP_ZOOM,
            MapScreenLayout::Mc2Split => {
                if aspect >= 1.0 {
                    // Wide pane: the shorter axis IS the vertical.
                    MC2_MAP_VIEW_SPAN_TILES
                } else {
                    // Tall pane: shorter = width; the shader derives
                    // vertical = zoom/aspect.
                    MC2_MAP_VIEW_SPAN_TILES * aspect
                }
            }
        }
    }

    /// Marker-size multiplier for the overhead maps' entity dots and
    /// icon stamps (opt-in deviation). 1.0 = the baseline: dots baked
    /// into the map texture as tile texels, growing with radar zoom.
    /// Any other value draws the dots screen-space at a constant size
    /// (zoom-compensated) and scales the icon stamps to match. Dots
    /// take effect at the next map recompose (every sim tick).
    pub fn set_marker_scale(&mut self, scale: f32) {
        self.marker_scale = scale.clamp(0.25, 8.0);
    }

    /// Fog the map screens beyond the world's true (heading-rotated)
    /// extent, hiding the toroidal wrap's duplicate markers — the
    /// topmost map layer (opt-in deviation; the round minimap never
    /// shows duplicates, so it is untouched).
    pub fn set_extent_fog(&mut self, on: bool) {
        self.extent_fog = on;
    }

    /// Toggle smooth (tile-interpolated) shading; off is the original's
    /// per-tile shade snap. Takes effect on the next frame.
    pub fn set_smooth_shading(&mut self, on: bool) {
        self.smooth_shading = on;
    }

    /// Set the fog view distance in TILES: where the distance fog
    /// fully occludes (the band fades in from 0.75·D, retail's
    /// 15..19-tile ramp scaled). 0 disables fog entirely; the default
    /// is the retail 20. Nonzero values clamp to [`MAX_FOG_TILES`] so
    /// the fog band can never reach the silhouette melt band. Takes
    /// effect on the next frame.
    pub fn set_fog_distance(&mut self, tiles: f32) {
        self.fog_distance = if tiles <= 0.0 {
            0.0
        } else {
            tiles.min(MAX_FOG_TILES)
        };
    }

    /// Enable/disable water reflections (the per-frame mirrored-
    /// terrain pass sampled by sea fragments). On by default; the
    /// pass self-gates off caves, the book screen and non-water
    /// levels either way.
    pub fn set_reflections(&mut self, on: bool) {
        self.reflections = on;
    }

    /// Set this frame's dynamic point lights (`[x, alt, z,
    /// intensity]`, tile units; intensity 1 = retail's 128 baseline).
    /// The caller gates Night/Cave + the option; entries beyond
    /// [`MAX_LIGHTS`] are dropped.
    pub fn set_lights(&mut self, lights: &[[f32; 4]]) {
        self.lights = lights[..lights.len().min(MAX_LIGHTS)].to_vec();
    }

    /// Override the sky/fog color (sRGB) — the environment's fog far
    /// color (shade LUT row 0): what the clear, the book-screen sky
    /// fill and the distance fog all fade into.
    pub fn set_sky_color(&mut self, srgb: [f32; 3]) {
        self.sky_srgb = srgb;
        // Keep the group-1 sky-dummy texel on the constant: with no
        // sky texture loaded, the terrain fog/extinction melts sample
        // this instead and must degenerate to the plain constant.
        self.queue.write_texture(
            self.sky_dummy_tex.as_image_copy(),
            &sky_texel(srgb),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Load the level's parallax sky: the 256x256 8bpp sky bitmap
    /// (bundle `sky.bin`), resolved through the variant palette
    /// (RGBA, index straight through — retail DrawSky writes the
    /// palette index raw, no shade remap), then softened by
    /// [`blur_sky`]. Enables the textured sky pass; without it the
    /// flat fog-color fill IS the sky (retail's sky-off/cave keyColor
    /// fill).
    pub fn load_sky(&mut self, indices: &[u8], palette: &[[u8; 4]; 256]) {
        assert_eq!(indices.len(), 256 * 256, "sky.bin must be 256x256");
        let mut rgba = Vec::with_capacity(256 * 256 * 4);
        for &i in indices {
            let p = palette[i as usize];
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        blur_sky(&mut rgba);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            tex.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
        );
        // The shared sky sampler repeats on both axes (retail's 16-bit
        // wrapping index tiles the cloud plane infinitely) and filters
        // linearly — the bitmap was authored for ~1:1 at 320x200, so
        // at modern resolutions it upscales, and chunky texels would
        // read as noise.
        let view = tex.create_view(&Default::default());
        let device = &self.device;
        let layout = &self.sky_bind_group_layout;
        let sampler = &self.sky_sampler;
        let make = |globals: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let main_bg = make(&self.globals_buf);
        // The mirror twin: same texture, mirror globals (atlas.w = 2
        // flips the ray's y — the sky reflecting in the water).
        let mirror_bg = make(&self.mirror_globals_buf);
        self.sky_bind_group = Some(main_bg);
        self.sky_mirror_bind_group = Some(mirror_bg);
        // The terrain pass's group-1 sky slot (the fog/extinction
        // melt target) tracks the same texture.
        self.sky_view = Some(view);
        self.refresh_terrain_textures();
    }

    /// Drop the textured sky (back to the flat fog-color fill).
    pub fn clear_sky(&mut self) {
        self.sky_bind_group = None;
        self.sky_mirror_bind_group = None;
        self.sky_view = None;
        self.refresh_terrain_textures();
    }

    /// Rebuild both group-1 bind groups (the always-bound dummy and,
    /// when a mirror target exists, the real reflection one — over
    /// the BLURRED B target the water samples) so their sky slot
    /// tracks `sky_view`.
    fn refresh_terrain_textures(&mut self) {
        self.reflection_dummy_bind_group = self.terrain_textures("reflection dummy", None);
        let real = self
            .blur_b_view
            .as_ref()
            .map(|rv| self.terrain_textures("reflection", Some(rv)));
        self.reflection_bind_group = real;
    }

    /// Build a group-1 bind group: the mirror slot (the reflection
    /// image, or the 1x1 dummy) + the sky slot (the parallax sky, or
    /// the 1x1 fog-constant dummy) — terrain.wgsl's melt target.
    fn terrain_textures(&self, label: &str, mirror: Option<&wgpu::TextureView>) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.reflection_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        mirror.unwrap_or(&self.mirror_dummy_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.reflection_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        self.sky_view.as_ref().unwrap_or(&self.sky_dummy_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                },
            ],
        })
    }

    fn sky_color_linear(&self) -> [f64; 3] {
        [
            srgb_to_linear(self.sky_srgb[0]) as f64,
            srgb_to_linear(self.sky_srgb[1]) as f64,
            srgb_to_linear(self.sky_srgb[2]) as f64,
        ]
    }

    pub fn smooth_shading(&self) -> bool {
        self.smooth_shading
    }

    /// Advance the animation clock, in original game turns (one sim
    /// tick = one turn; pass a fractional part for render
    /// interpolation). Drives the water wave and the sprite frame
    /// cycling; both repeat within 4096 turns, so callers should wrap
    /// (`tick % 4096`) to keep f32 precision over long sessions.
    pub fn set_anim_turn(&mut self, turn: f32) {
        self.anim_turn = turn;
    }

    /// Set the radar's HUD transparency: `true` = translucent (faithful
    /// MC1, matches the panels), `false` = opaque (MC2 readability
    /// toggle). Alpha kept in sync with the panel alpha in ui.rs.
    pub fn set_hud_transparent(&mut self, transparent: bool) {
        self.minimap_alpha = if transparent { HUD_PANEL_ALPHA } else { 1.0 };
    }

    /// The level-end fade's reach into the screen-space map markers
    /// (dots/stamps/ants/objective marks): 0 = no fade, 1 = fully
    /// black. In flight those draw ABOVE the app's full-screen fade
    /// quad, so without this they sit at full opacity on the black
    /// screen; the baked radar disc fades for free in the world pass.
    pub fn set_overlay_fade(&mut self, fade: f32) {
        self.overlay_fade = fade.clamp(0.0, 1.0);
    }

    /// Multiply the radar zoom (tiles across the disc), clamped to a
    /// sane range. `factor` < 1 zooms in (fewer tiles), > 1 zooms out.
    /// Bound to `+`/`-` in the app (MC2/MC1 runtime radar zoom).
    pub fn zoom_minimap(&mut self, factor: f32) {
        self.minimap_zoom = (self.minimap_zoom * factor).clamp(MINIMAP_ZOOM_MIN, MINIMAP_ZOOM_MAX);
    }

    pub fn minimap_zoom(&self) -> f32 {
        self.minimap_zoom
    }

    /// Zoom the MAP SCREEN by a factor — the `+`/`-` keys while it is
    /// open (a port addition like [`Self::zoom_minimap`], deliberate;
    /// retail's book map has no zoom). Multiplier clamped to
    /// [1/8, √2]: from a 32-tile crop on the MC1 book out to the span
    /// that fits the WHOLE rotated world — the extent square's
    /// diagonal is base·√2, so the top stop keeps every corner on the
    /// pane at any heading (player ask: at 1x a 45° heading clipped
    /// the tip). Session-only, never persisted.
    pub fn zoom_map_screen(&mut self, factor: f32) {
        self.map_zoom_mult = (self.map_zoom_mult * factor).clamp(0.125, std::f32::consts::SQRT_2);
    }

    /// The map screen's current magnification (1 = the full default
    /// span), for the console echo.
    pub fn map_screen_mag(&self) -> f32 {
        1.0 / self.map_zoom_mult
    }

    /// Upload a level: build the terrain mesh, the color/type LUTs, and
    /// the overhead map (terrain + entity dots).
    /// Drop the loaded level's world drawables — the session-teardown
    /// counterpart of `load_level` (the frontend owns the frame with
    /// no level beneath: nothing of the torn-down world may render).
    /// The world pass is buffer-guarded, so rendering level-less is
    /// safe; textures and bind groups are simply orphaned until the
    /// next `load_level` replaces them.
    pub fn clear_level(&mut self) {
        self.vertex_buf = None;
        self.index_buf = None;
        self.index_count = 0;
        self.set_billboards(Vec::new());
        self.set_health_bars(Vec::new());
        self.set_lights(&[]);
        self.clear_sky();
        self.wave_mode = 0;
        self.fire_particles = Vec::new();
        self.bolt_segments = Vec::new();
        // The map surfaces die with their level: the disc/pane draws
        // are gated on these bind groups, and the screen-space marker
        // layers (dots, stamps, ants, objective marks) append to the
        // UI quad stream — any of them surviving would draw over the
        // frontend.
        self.map_bind_group = None;
        self.minimap_bind_group = None;
        self.screen_dots = Vec::new();
        self.map_stamps = Vec::new();
        self.map_path = None;
        self.objective_marks = Vec::new();
        self.objective_tick = 0;
        self.map_view = false;
    }

    pub fn load_level(&mut self, level: &LevelView, overlay: &MapOverlay) {
        let n = MAP_TILES;
        assert_eq!(level.height.len(), n * n);
        assert_eq!(level.tile_type.len(), n * n);
        self.wave_mode = match level.wave {
            WaveMode::Off => 0,
            WaveMode::Mc1 => 1,
            WaveMode::Mc2 => 2,
        };

        // Height at a wrapped grid point.
        let h = |x: usize, z: usize| -> f32 {
            level.height[(z % n) * n + (x % n)] as f32 * HEIGHT_SCALE
        };

        // One vertex per grid point, plus a duplicated wrap row/column so
        // the last tile closes the seam with the first.
        let verts_per_side = n + 1;
        let mut vertices = Vec::with_capacity(verts_per_side * verts_per_side);
        // When the package carries the generator's shading array, it is
        // the light source (vertex light stays 1.0). Otherwise fall back
        // to a synthetic hillshade: fixed sun from the north-west.
        let synthetic = level.shading.is_none();
        let sun = {
            let v: [f32; 3] = [-0.45, 0.8, -0.4];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / len, v[1] / len, v[2] / len]
        };
        for z in 0..verts_per_side {
            for x in 0..verts_per_side {
                let y = h(x, z);
                let light = if synthetic {
                    // Central-difference normal with wraparound neighbors.
                    let dx = h(x + 1, z) - h(x + n - 1, z);
                    let dz = h(x, z + 1) - h(x, z + n - 1);
                    let inv = 1.0 / (dx * dx + dz * dz + 4.0).sqrt();
                    let normal = [-dx * inv, 2.0 * inv, -dz * inv];
                    let ndotl = normal[0] * sun[0] + normal[1] * sun[1] + normal[2] * sun[2];
                    0.55 + 0.55 * ndotl.max(0.0)
                } else {
                    1.0
                };
                // y stays 0 in the buffer: the vertex shader reads the
                // height plane texture so runtime terrain mutation is
                // a texture write, not a mesh rebuild.
                let _ = y;
                vertices.push(Vertex {
                    pos: [x as f32, 0.0, z as f32],
                    light,
                });
            }
        }

        // Two triangles per tile; diagonal orientation alternates in a
        // checkerboard exactly like the engine's altitude interpolation
        // (sub_B5C60: `(tile_x + tile_z) & 1` picks the split).
        let mut indices: Vec<u32> = Vec::with_capacity(n * n * 6);
        let at = |x: usize, z: usize| (z * verts_per_side + x) as u32;
        for z in 0..n {
            for x in 0..n {
                let (a, b, c, d) = (at(x, z), at(x + 1, z), at(x + 1, z + 1), at(x, z + 1));
                if (x + z) & 1 == 0 {
                    // Split along the a-c diagonal.
                    indices.extend_from_slice(&[a, c, b, a, d, c]);
                } else {
                    // Split along the b-d diagonal.
                    indices.extend_from_slice(&[a, d, b, b, d, c]);
                }
            }
        }

        use wgpu::util::DeviceExt;
        self.vertex_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        self.index_buf = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain indices"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
        );
        self.index_count = indices.len() as u32;

        // A small helper: 2D R8Uint texture from a byte grid.
        let byte_tex = |label: &str, bytes: &[u8], width: u32, height: u32| {
            let extent = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                tex.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: None,
                },
                extent,
            );
            tex
        };

        let type_tex = byte_tex("tile types", &level.tile_type, n as u32, n as u32);
        // Without a baked shading array, a constant mid level keeps the
        // colormap row selection stable (vertex light shades instead).
        let flat_shading;
        let shading: &[u8] = match &level.shading {
            Some(s) => s,
            None => {
                flat_shading = vec![32u8; n * n];
                &flat_shading
            }
        };
        let shade_tex = byte_tex("tile shading", shading, n as u32, n as u32);

        // Type -> flat base palette index, for tiles rendered without a
        // texture (no atlas, or type beyond the atlas).
        let tile_colors_tex = byte_tex("tile colors", &level.tile_colors, 256, 1);

        // Terrain-texture atlas (a 1x1 dummy keeps the bind group layout
        // uniform when the level has none; the shader gates on the cell
        // count in Globals).
        let (atlas_data, atlas_w, atlas_h): (&[u8], u32, u32) = match &level.atlas {
            Some(a) => {
                assert_eq!(a.len() % (ATLAS_WIDTH * ATLAS_CELL), 0, "ragged atlas");
                (a, ATLAS_WIDTH as u32, (a.len() / ATLAS_WIDTH) as u32)
            }
            None => (&[0], 1, 1),
        };
        self.atlas_cells = level
            .atlas
            .as_ref()
            .map(|a| (a.len() / (ATLAS_WIDTH * ATLAS_CELL)) * (ATLAS_WIDTH / ATLAS_CELL))
            .unwrap_or(0) as u32;
        let atlas_tex = byte_tex("terrain atlas", atlas_data, atlas_w, atlas_h);

        // Per-tile texture orientation (angle bits 4-6); orientation 0
        // for packages baked before the angle member existed.
        let flat_angle;
        let angle: &[u8] = match &level.angle {
            Some(a) => {
                assert_eq!(a.len(), n * n);
                a
            }
            None => {
                flat_angle = vec![0u8; n * n];
                &flat_angle
            }
        };
        let angle_tex = byte_tex("tile angles", angle, n as u32, n as u32);
        let height_tex = byte_tex("tile heights", &level.height, n as u32, n as u32);
        // MC2 cave second heightmap (the ceiling pass's height slot).
        let cave_ceiling_tex = level
            .ceiling
            .as_ref()
            .map(|c| byte_tex("ceiling heights", c, n as u32, n as u32));

        // Shore-distance field + deep-water block mask, both derived
        // from the tile-type plane alone (rebaked incrementally on
        // terrain mutation — see `update_terrain`).
        self.shore_types = level.tile_type.clone();
        self.shore_field = vec![255u8; n * SHORE_RES * n * SHORE_RES];
        bake_shore_region(&self.shore_types, n, &mut self.shore_field, 0, 0, n, n);
        self.rebuild_water_blocks(n);
        let shore_extent = wgpu::Extent3d {
            width: (n * SHORE_RES) as u32,
            height: (n * SHORE_RES) as u32,
            depth_or_array_layers: 1,
        };
        let shore_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shore field"),
            size: shore_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            shore_tex.as_image_copy(),
            &self.shore_field,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((n * SHORE_RES) as u32),
                rows_per_image: None,
            },
            shore_extent,
        );
        let shore_view = shore_tex.create_view(&Default::default());

        // Colormap (x = palette index, y = shade): the engine's shade
        // remap composed with the palette on the CPU. sRGB format so
        // sampling yields linear color. Texture texels and flat tile
        // colors both resolve through this one table, exactly like the
        // original's textured inner loop `shade_lut[shade*256 + texel]`.
        assert_eq!(level.shade_lut.len(), SHADE_LEVELS * 256);
        let mut colormap = vec![0u8; SHADE_LEVELS * 256 * 4];
        for shade in 0..SHADE_LEVELS {
            for index in 0..256 {
                let final_idx = level.shade_lut[shade * 256 + index] as usize;
                let rgb = level.palette[final_idx];
                let o = (shade * 256 + index) * 4;
                colormap[o..o + 3].copy_from_slice(&rgb);
                colormap[o + 3] = 255;
            }
        }
        let colormap_extent = wgpu::Extent3d {
            width: 256,
            height: SHADE_LEVELS as u32,
            depth_or_array_layers: 1,
        };
        let colormap_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("type/shade colormap"),
            size: colormap_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            colormap_tex.as_image_copy(),
            &colormap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: None,
            },
            colormap_extent,
        );

        self.colormap_tex = Some(colormap_tex.clone());
        self.rebuild_billboard_bind_group();

        self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &type_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &shade_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &colormap_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &tile_colors_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(
                        &atlas_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(
                        &angle_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(
                        &height_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&shore_view),
                },
            ],
        }));
        // The water-reflection MIRROR pass twin: identical planes,
        // mirror globals (atlas.w = 2 = the shader's y-flip arm).
        self.mirror_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain mirror"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.mirror_globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &type_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &shade_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &colormap_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &tile_colors_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(
                        &atlas_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(
                        &angle_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(
                        &height_tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&shore_view),
                },
            ],
        }));
        // The MC2 cave CEILING pass: the identical grid drawn again
        // with the second heightmap in the height slot and the
        // ceiling globals (atlas.w = 1) in the uniform slot; all the
        // other planes are shared views.
        self.ceiling_bind_group = None;
        self.ceiling_tex = None;
        if let Some(ceiling_tex) = cave_ceiling_tex {
            self.ceiling_bind_group =
                Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("terrain ceiling"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.ceiling_globals_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                &type_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                &shade_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(
                                &colormap_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(
                                &tile_colors_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(
                                &atlas_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(
                                &angle_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 7,
                            resource: wgpu::BindingResource::TextureView(
                                &ceiling_tex.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 8,
                            resource: wgpu::BindingResource::TextureView(&shore_view),
                        },
                    ],
                }));
            self.ceiling_tex = Some(ceiling_tex);
        }
        self.plane_texs = Some([type_tex, shade_tex, angle_tex, height_tex]);
        self.shore_tex = Some(shore_tex);

        // Overhead map for the book screen, composed on the CPU through
        // the engine's map color path.
        let map_rgba = map_pixels_impl(level, overlay, self.marker_scale == 1.0);
        self.refresh_screen_dots(level, overlay);
        let map_extent = wgpu::Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        };
        let map_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overhead map"),
            size: map_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            map_tex.as_image_copy(),
            &map_rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n as u32 * 4),
                rows_per_image: None,
            },
            map_extent,
        );
        let map_view = map_tex.create_view(&Default::default());
        self.map_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map"),
            layout: &self.map_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.map_globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&map_view),
                },
            ],
        }));
        // The in-flight minimap shares the world map texture but has its
        // own globals (corner rect, tighter zoom, round mask).
        self.minimap_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimap"),
            layout: &self.map_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.minimap_globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&map_view),
                },
            ],
        }));
        self.map_tex = Some(map_tex);
    }

    /// Re-upload the terrain planes + overhead map after runtime world
    /// mutation (craters, quakes, spawned entities). The level view
    /// must carry the LIVE planes; mesh and bind groups are reused —
    /// this is four 64 KB texture writes plus the map compose.
    pub fn update_terrain(&mut self, level: &LevelView, overlay: &MapOverlay) {
        let n = MAP_TILES as u32;
        let Some([type_tex, shade_tex, angle_tex, height_tex]) = &self.plane_texs else {
            return;
        };
        let write = |tex: &wgpu::Texture, bytes: &[u8]| {
            self.queue.write_texture(
                tex.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(n),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: n,
                    height: n,
                    depth_or_array_layers: 1,
                },
            );
        };
        write(type_tex, &level.tile_type);
        write(height_tex, &level.height);
        if let Some(s) = &level.shading {
            write(shade_tex, s);
        }
        if let Some(a) = &level.angle {
            write(angle_tex, a);
        }
        if let (Some(c), Some(tex)) = (&level.ceiling, &self.ceiling_tex) {
            write(tex, c);
        }
        self.update_shore_field(&level.tile_type);
        self.update_map(level, overlay);
    }

    /// Rebake the shore-distance field around CHANGED tile types only
    /// (craters and quakes touch a handful of tiles; height-only
    /// mutation changes nothing here), re-upload it, and refresh the
    /// deep-water block mask.
    fn update_shore_field(&mut self, tile_type: &[u8]) {
        if self.shore_types == tile_type || self.shore_types.len() != tile_type.len() {
            return;
        }
        let n = MAP_TILES;
        rebake_shore_changed(&mut self.shore_field, &self.shore_types, tile_type, n);
        self.shore_types.copy_from_slice(tile_type);
        if let Some(tex) = &self.shore_tex {
            self.queue.write_texture(
                tex.as_image_copy(),
                &self.shore_field,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((n * SHORE_RES) as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: (n * SHORE_RES) as u32,
                    height: (n * SHORE_RES) as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.rebuild_water_blocks(n);
    }

    /// The mirror-pass visibility gate: is any deep-water (type 0)
    /// tile within the camera's visible reach? Conservative — reach
    /// is the fog cutoff, capped by where the HIGHEST frustum corner
    /// ray meets the wave-crest plane (looking down over a city that
    /// reach is short; any ray at or above the horizon makes it
    /// unbounded), with margins on top — false only when no visible
    /// fragment can possibly blend the mirror image.
    fn deep_water_in_reach(
        &self,
        cam: &CameraView,
        right: [f32; 3],
        up: [f32; 3],
        fwd: [f32; 3],
        tan_h: f32,
        tan_v: f32,
    ) -> bool {
        if !self.has_deep_water {
            return false;
        }
        let mut reach = if self.fog_distance > 0.0 {
            self.fog_distance
        } else {
            f32::INFINITY
        };
        // The highest surface a fragment can mirror-blend at: the
        // 0.6-tile altitude-fade top plus the ~0.25-tile swell.
        let h = cam.y - 0.9;
        if h > 0.0 {
            let mut ground = 0.0f32;
            for (sh, sv) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                let d = [
                    fwd[0] + right[0] * tan_h * sh + up[0] * tan_v * sv,
                    fwd[1] + right[1] * tan_h * sh + up[1] * tan_v * sv,
                    fwd[2] + right[2] * tan_h * sh + up[2] * tan_v * sv,
                ];
                if d[1] < 0.0 {
                    ground = ground.max(h * (d[0] * d[0] + d[2] * d[2]).sqrt() / -d[1]);
                } else {
                    ground = f32::INFINITY;
                }
            }
            reach = reach.min(ground);
        }
        if !reach.is_finite() {
            return true;
        }
        let reach = reach * 1.05 + 2.0;
        let n = MAP_TILES as f32;
        let nb = MAP_TILES / WATER_BLOCK;
        let half = WATER_BLOCK as f32 * 0.5;
        for bz in 0..nb {
            for bx in 0..nb {
                if !self.water_blocks[bz * nb + bx] {
                    continue;
                }
                // Torus-wrapped point-to-block-rect distance.
                let mut dx = (cam.x - (bx * WATER_BLOCK) as f32 - half).rem_euclid(n);
                if dx > n * 0.5 {
                    dx -= n;
                }
                let mut dz = (cam.z - (bz * WATER_BLOCK) as f32 - half).rem_euclid(n);
                if dz > n * 0.5 {
                    dz -= n;
                }
                let ax = (dx.abs() - half).max(0.0);
                let az = (dz.abs() - half).max(0.0);
                if ax * ax + az * az <= reach * reach {
                    return true;
                }
            }
        }
        false
    }

    /// Rebuild the coarse deep-water presence mask (the mirror-pass
    /// visibility gate) from the tile-type snapshot.
    fn rebuild_water_blocks(&mut self, n: usize) {
        let nb = n / WATER_BLOCK;
        self.water_blocks = vec![false; nb * nb];
        self.has_deep_water = false;
        for z in 0..n {
            for x in 0..n {
                if self.shore_types[z * n + x] == 0 {
                    self.water_blocks[(z / WATER_BLOCK) * nb + x / WATER_BLOCK] = true;
                    self.has_deep_water = true;
                }
            }
        }
    }

    /// Recompose + re-upload ONLY the overhead map texture (dots,
    /// icon stamps, the guide path, blink phases). Cheap enough to
    /// run every sim tick — the original redraws its map every frame,
    /// and the blink/marching-ants patterns need it.
    pub fn update_map(&mut self, level: &LevelView, overlay: &MapOverlay) {
        let n = MAP_TILES as u32;
        self.refresh_screen_dots(level, overlay);
        if let Some(map_tex) = &self.map_tex {
            let map_rgba = map_pixels_impl(level, overlay, self.marker_scale == 1.0);
            self.queue.write_texture(
                map_tex.as_image_copy(),
                &map_rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(n * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: n,
                    height: n,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Rebuild the screen-space dot set for the marker-size deviation:
    /// at `marker_scale == 1.0` it stays empty (the dots bake into the
    /// map texture as always); otherwise every overlay dot is lifted
    /// out with its palette color resolved to a linear tint. Runs with
    /// every map recompose so blink phases keep their cadence.
    fn refresh_screen_dots(&mut self, level: &LevelView, overlay: &MapOverlay) {
        self.screen_dots = if self.marker_scale == 1.0 {
            Vec::new()
        } else {
            overlay
                .dots
                .iter()
                .map(|d| {
                    let c = level.palette[d.color as usize];
                    ScreenDot {
                        x: d.x,
                        z: d.z,
                        size: d.size as f32,
                        tint: [
                            srgb_to_linear(c[0] as f32 / 255.0),
                            srgb_to_linear(c[1] as f32 / 255.0),
                            srgb_to_linear(c[2] as f32 / 255.0),
                            1.0,
                        ],
                    }
                })
                .collect()
        };
    }

    /// Upload the bundle's sprite atlas + index for billboard drawing.
    pub fn load_sprites(&mut self, index: mgc_formats::bundle::SpriteIndex, atlas: &[u8]) {
        assert_eq!(
            atlas.len(),
            index.atlas_width as usize * index.atlas_height as usize
        );
        let extent = wgpu::Extent3d {
            width: index.atlas_width,
            height: index.atlas_height,
            depth_or_array_layers: 1,
        };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            tex.as_image_copy(),
            atlas,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(index.atlas_width),
                rows_per_image: None,
            },
            extent,
        );
        self.sprite_tex = Some(tex);
        self.sprite_index = Some(index);
        self.rebuild_billboard_bind_group();
    }

    /// Replace the set of world sprites drawn each frame.
    /// Upload the RGBA UI atlas (app-side composited: HSPR indices
    /// resolved through the blend LUT + palette; index-0 texels carry
    /// alpha 0).
    pub fn load_ui_atlas(&mut self, width: u32, height: u32, rgba: &[u8]) {
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        self.ui_atlas_size = (width.max(1), height.max(1));
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        self.ui_bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &self.ui_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.ui_globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &tex.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }));
    }

    /// Replace this frame's UI quads (drawn last, in list order).
    pub fn set_ui_quads(&mut self, mut quads: Vec<UiQuad>) {
        // Callers lay the UI out in WINDOW pixels; the renderer's own
        // screen-space quads (the map stamps) and the UI uniform are in
        // the SCENE buffer's pixels, which supersampling makes larger.
        // Bring the caller's quads into that space so the two agree —
        // the UI is then rasterized at the supersampled resolution and
        // averaged down with everything else, which is what smooths its
        // scaled-up sprite edges.
        if let Some(ss) = &self.ssaa {
            let (sx, sy) = match &self.target {
                Target::Window { config, .. } => (
                    ss.size.0 as f32 / config.width.max(1) as f32,
                    ss.size.1 as f32 / config.height.max(1) as f32,
                ),
                Target::Offscreen { .. } => (1.0, 1.0),
            };
            for q in &mut quads {
                q.rect = [
                    q.rect[0] * sx,
                    q.rect[1] * sy,
                    q.rect[2] * sx,
                    q.rect[3] * sy,
                ];
            }
        }
        self.ui_quads = quads;
    }

    /// Set the upright map icons (own castle/balloons). They are drawn
    /// screen-space over the active map surface — never baked into the
    /// rotated map texture — so they stay upright under rotation.
    pub fn set_map_stamps(&mut self, stamps: Vec<MapStamp>) {
        self.map_stamps = stamps;
    }

    /// Set the marching-ants guide path (player → own castle). Drawn
    /// screen-space over the active map surface, a mark every 4
    /// surface pixels (see [`project_guide_path`]); None = no path.
    pub fn set_map_path(&mut self, path: Option<MapPath>) {
        self.map_path = path;
    }

    /// Set the MC2 objective-guide targets + the current sim `tick`
    /// (drives the retail blink gates). Drawn screen-space over the
    /// active map surface as blinking outlines + a steer arrow to the
    /// nearest (see [`project_objective_marks`]); empty draws nothing.
    pub fn set_objective_marks(&mut self, marks: Vec<ObjectiveMark>, tick: u32) {
        self.objective_marks = marks;
        self.objective_tick = tick;
    }

    /// The in-flight radar disc: (diameter, center_x, center_y) in
    /// pixels. The disc is anchored at the screen CORNER (0,0) so its
    /// center sits at its radius (retail DrawMinimap(0,0)) — scaled by
    /// the uniform HUD factor (`HudFrame::s`) to track the sprite
    /// panels, which are LEFT-anchored in the same corner. Single source
    /// of truth for both the shader uniform and the stamp projection;
    /// they MUST agree or terrain and stamps diverge.
    fn minimap_rect(&self, w: u32, hpx: u32) -> (f32, f32, f32) {
        let hud = HudFrame::new(w as f32, hpx as f32).s;
        let diam = (MINIMAP_DIAM * hud).min(w.min(hpx) as f32);
        // Anchored at the corner (0,0), touching both screen edges — the
        // disc center is exactly at its radius (retail DrawMinimap(0,0)).
        let c = diam * 0.5;
        (diam, c, c)
    }

    /// Project the map stamps onto one map surface as upright UI quads.
    /// `center`/`half` are the surface's screen rect (pixels): center
    /// point and half-extents. `zoom` = tiles across the shorter axis,
    /// `round` clips to the inscribed disc, `scale` = the surface's
    /// native→screen pixel factor (stamps keep their retail proportion
    /// at any window size). Mirrors the sampling transform in map.wgsl
    /// (inverted); see [`project_map_stamps`].
    #[allow(clippy::too_many_arguments)]
    fn map_stamp_quads(
        &self,
        cx: f32,
        cy: f32,
        half_x: f32,
        half_y: f32,
        px: f32,
        pz: f32,
        yaw: f32,
        zoom: f32,
        round: bool,
        aspect: f32,
        scale: f32,
    ) -> Vec<UiQuad> {
        project_map_stamps(
            &self.map_stamps,
            cx,
            cy,
            half_x,
            half_y,
            px,
            pz,
            yaw,
            zoom,
            round,
            aspect,
            // The marker-size deviation scales the icon stamps with
            // the dots (1.0 = no-op); the guide path and objective
            // marks keep the plain surface scale.
            scale * self.marker_scale,
        )
    }

    pub fn set_billboards(&mut self, billboards: Vec<Billboard>) {
        self.billboards = billboards;
    }

    /// PROTOTYPE: replace the fire-particle set (empty = no fire).
    pub fn set_fire_particles(&mut self, particles: Vec<FireParticle>) {
        self.fire_particles = particles;
    }

    /// PROTOTYPE: replace the lightning-bolt segment set (empty =
    /// no bolts).
    pub fn set_bolt_segments(&mut self, segments: Vec<BoltSegment>) {
        self.bolt_segments = segments;
    }

    /// Replace the monster health-bar overlay set (empty = off).
    pub fn set_health_bars(&mut self, bars: Vec<HealthBar>) {
        self.health_bars = bars;
    }

    fn rebuild_billboard_bind_group(&mut self) {
        let (Some(sprites), Some(colormap)) = (&self.sprite_tex, &self.colormap_tex) else {
            return;
        };
        let device = &self.device;
        let layout = &self.billboard_bind_group_layout;
        let make = |globals: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("billboard"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &sprites.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            &colormap.create_view(&Default::default()),
                        ),
                    },
                ],
            })
        };
        let main_bg = make(&self.globals_buf);
        // The mirror twin (atlas.w = 2 — the shader's y-flip arm):
        // sprite reflections in the water pass.
        let mirror_bg = make(&self.mirror_globals_buf);
        self.billboard_bind_group = Some(main_bg);
        self.billboard_mirror_bind_group = Some(mirror_bg);
    }

    /// Visibility of a proximity-concealed sprite ([`Billboard::
    /// conceal`]) at squared slant distance `d2` (tiles²): retail's
    /// own fog-row ramp, `(FogEnd − d²) / FogThickness` clamped to
    /// 0..1 (GRO:3505-3511), with retail's constants hardwired —
    /// FogStart 3840² / FogEnd 4864² engine units at 256/tile, i.e.
    /// full at ≤15 tiles, gone at ≥19. Retail's mode-2 ghost is
    /// never fogged; its concealment is the backdrop saturating to
    /// the fog color across this same band plus the hard sprite cull
    /// at 20 tiles (GRO:3498) — the ramp is that compound, expressed
    /// as alpha so it survives the port's extended fog distance.
    fn conceal_visibility(d2: f32) -> f32 {
        const START2: f32 = 15.0 * 15.0;
        const END2: f32 = 19.0 * 19.0;
        ((END2 - d2) / (END2 - START2)).clamp(0.0, 1.0)
    }

    /// Resolve each billboard against the camera (rotation view,
    /// mirroring, wrap-nearest position) into instance data — the
    /// original's per-sprite draw dispatch (remc1 DrawSprite3D_2F170),
    /// with the yaw quantization done in engine angle units.
    ///
    /// Returns the instances with all opaque ones first and the
    /// translucent tail sorted back-to-front, plus the opaque count —
    /// the two draw ranges (opaque pipeline / blend pipeline).
    fn billboard_instances(&self, cam: &CameraView) -> (Vec<BillboardInstance>, u32) {
        let Some(index) = &self.sprite_index else {
            return (Vec::new(), 0);
        };
        let mut out = Vec::with_capacity(self.billboards.len());
        let mut translucent = Vec::new();
        let full = MAP_TILES as f32;
        for b in &self.billboards {
            // 16 view sectors from relative yaw, exactly the engine's
            // `(((entityYaw - camYaw) >> 3) & 0xF0) >> 4` on 11-bit
            // angles: floor(rel / 128) of 2048 steps.
            let rel = (b.yaw - cam.yaw).rem_euclid(std::f32::consts::TAU);
            let view = ((rel * (2048.0 / std::f32::consts::TAU)) as i32 >> 7).clamp(0, 15) as u16;
            let (offset, mirror) = match b.draw_type {
                17 => {
                    if view < 8 {
                        (view, false)
                    } else {
                        (15 - view, true)
                    }
                }
                18 => (view, false),
                19 => (VIEW_FOLD_5[view as usize] as u16, view >= 8),
                20 => (VIEW_FOLD_3[view as usize] as u16, view >= 8),
                // Animation modes: the entity's anim byte selects the
                // family member (DrawSprite3D :37552; MC2 adds the
                // 22-36 band, remc2 GameRenderOriginal LABEL_26 —
                // same frame-offset draw).
                2..=16 | 22..=36 => (b.frame as u16, false),
                // 0/1/21 single view, and anything unknown: base.
                _ => (0, false),
            };
            let id = (b.sprite_base + offset) as usize;
            let Some(entry) = index.sprites.get(id) else {
                continue;
            };
            if entry.frames.is_empty() {
                continue; // known-corrupt source entry
            }
            // Animated entries (flags bit 0, the TMAPS FLC streams) step
            // one frame per turn in a forward loop, all in lockstep —
            // the original's per-frame driver (remc1 sub_590D0_595E0).
            let fi = if entry.flags & 1 != 0 {
                self.anim_turn as usize % entry.frames.len()
            } else {
                0
            };
            let frame = &entry.frames[fi];
            let (w, h) = (entry.width as f32, entry.height as f32);
            let world_w = b.world_h * w / h;
            // Nearest torus copy relative to the camera.
            let wrap = |p: f32, c: f32| {
                let mut d = p - c;
                if d > full / 2.0 {
                    d -= full;
                }
                if d < -full / 2.0 {
                    d += full;
                }
                c + d
            };
            let pos = [wrap(b.x, cam.x), b.y, wrap(b.z, cam.z)];
            let mut alpha = match b.blend {
                2 => 1.0 / 3.0,
                3 => 2.0 / 3.0,
                // The instrument alpha — see `Billboard::blend`.
                4 => 1.0 / 6.0,
                _ => 1.0,
            };
            if b.conceal {
                let (dx, dy, dz) = (pos[0] - cam.x, pos[1] - cam.y, pos[2] - cam.z);
                let vis = Self::conceal_visibility(dx * dx + dy * dy + dz * dz);
                if vis <= 0.0 {
                    continue;
                }
                alpha *= vis;
            }
            let inst = BillboardInstance {
                pos,
                size: [world_w, b.world_h],
                uv_pos: [frame.x as f32, frame.y as f32],
                uv_size: [w, h],
                flags: [mirror as u32, 32],
                alpha,
                chain: b.chain_depth,
            };
            if alpha < 1.0 {
                translucent.push(inst);
            } else {
                out.push(inst);
            }
        }
        let opaque = out.len() as u32;
        // Back-to-front by the DEPTH CHANNEL'S OWN metric — the anchor
        // TILE's plan distance (billboard.wgsl's `anchor_depth`), not
        // the sprite's raw position. The blend pipeline writes no
        // depth, so this sort is the only thing ordering translucent
        // sprites, and keying it to the tile is what lets co-tile pairs
        // tie exactly so the retail chain rank can break them (the
        // opaque pass gets the same resolution from the depth epsilon).
        // Higher `chain` = later in retail's tile walk = drawn last.
        translucent.sort_by(|a, b| {
            let d = |i: &BillboardInstance| {
                let (tx, tz) = (i.pos[0].floor() + 0.5, i.pos[2].floor() + 0.5);
                let (dx, dz) = (tx - cam.x, tz - cam.z);
                dx * dx + dz * dz
            };
            d(b).total_cmp(&d(a)).then(a.chain.total_cmp(&b.chain))
        });
        out.extend(translucent);
        (out, opaque)
    }

    /// PROTOTYPE: fire particles → GPU instances (nearest torus copy).
    fn fire_instances(&self, cam: &CameraView) -> Vec<FireInstance> {
        let full = MAP_TILES as f32;
        let wrap = |p: f32, c: f32| {
            let mut d = p - c;
            if d > full / 2.0 {
                d -= full;
            }
            if d < -full / 2.0 {
                d += full;
            }
            c + d
        };
        // Only a trivially-safe opacity cull here — dropping ~zero-alpha
        // particles before they become quads. (Spatial/fog culling by the
        // particle CENTER wrongly erases big particles straddling the fog
        // wall or the view edge, so the fog fade is left entirely to the
        // shader, which does it per fragment.)
        let mut out = Vec::with_capacity(self.fire_particles.len());
        for f in &self.fire_particles {
            if f.alpha <= 0.004 {
                continue;
            }
            out.push(FireInstance {
                pos: [wrap(f.x, cam.x), f.y, wrap(f.z, cam.z)],
                size: [f.w, f.h],
                heat: f.heat,
                alpha: f.alpha,
                seed: f.seed,
            });
        }
        out
    }

    fn bolt_instances(&self, cam: &CameraView) -> Vec<BoltInstance> {
        let full = MAP_TILES as f32;
        let mut out = Vec::with_capacity(self.bolt_segments.len());
        for s in &self.bolt_segments {
            if s.alpha <= 0.004 || s.energy <= 0.004 {
                continue;
            }
            let (p0, p1) = wrap_bolt_to_camera(s.anchor, s.p0, s.p1, cam.x, cam.z, full);
            out.push(BoltInstance {
                p0,
                p1,
                width: s.width,
                energy: s.energy,
                alpha: s.alpha,
                seed: s.seed,
            });
        }
        out
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if let Target::Window { surface, config } = &mut self.target {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);
        }
        self.rebuild_ssaa();
        // Depth and the MSAA colour buffer follow the SCENE buffer,
        // which is the supersampled one when supersampling is on.
        let (dw, dh) = self.size();
        self.depth = create_depth(&self.device, dw, dh, self.samples);
        self.msaa_color = self.make_msaa_target(dw, dh, "msaa-scene");
    }

    /// A multisampled colour attachment, or `None` when MSAA is off.
    fn make_msaa_target(&self, w: u32, h: u32, label: &str) -> Option<wgpu::TextureView> {
        if self.samples <= 1 {
            return None;
        }
        Some(
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
        )
    }

    /// The MSAA sample count in force (1 = off). Baked into the
    /// pipelines at construction, so this is what the CURRENT renderer
    /// is running, not what the config asks for.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Supersampling factor: render the whole frame this much larger
    /// than the window and average it down on the way to the screen.
    ///
    /// 1.0 turns it off completely — no offscreen buffer, no resolve
    /// pass, the frame goes straight to the surface as it always did.
    /// Above 1.0 it antialiases EVERYTHING the frame contains: terrain
    /// and sprite silhouettes in the 3D view, and the scaled-up 2D UI
    /// with them. Cost goes as the square, so 2.0 is four times the
    /// pixels.
    pub fn set_render_scale(&mut self, scale: f32) {
        let scale = scale.clamp(1.0, 4.0);
        if (self.render_scale - scale).abs() < f32::EPSILON {
            return;
        }
        self.render_scale = scale;
        self.rebuild_ssaa();
        let (dw, dh) = self.size();
        self.depth = create_depth(&self.device, dw, dh, self.samples);
    }

    /// (Re)create the supersample buffer for the current window size,
    /// or drop it when it is not wanted.
    fn rebuild_ssaa(&mut self) {
        let want = match &self.target {
            // An offscreen target is already rendering at whatever size
            // the caller asked for (screenshots) — supersampling it
            // would silently change that size.
            Target::Offscreen { .. } => None,
            Target::Window { config, .. } if self.render_scale > 1.0 => Some((
                ((config.width as f32 * self.render_scale) as u32).max(1),
                ((config.height as f32 * self.render_scale) as u32).max(1),
            )),
            Target::Window { .. } => None,
        };
        let Some(size) = want else {
            self.ssaa = None;
            return;
        };
        if self.ssaa.as_ref().is_some_and(|s| s.size == size) {
            return;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ssaa-scene"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssaa"),
            layout: &self.ssaa_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.ssaa_sampler),
                },
            ],
        });
        self.ssaa = Some(Ssaa {
            view,
            bind_group,
            size,
        });
    }

    /// Vertical sync: on = wait for the display (AutoVsync — FIFO
    /// everywhere), off = present as fast as frames come (AutoNoVsync —
    /// immediate/mailbox, whichever the surface supports), releasing
    /// the frame rate for FPS measurement. The Auto modes are the two
    /// present modes wgpu guarantees on every surface — a raw
    /// `Immediate` would panic on backends without it. Offscreen
    /// targets have no swapchain: no-op.
    pub fn set_vsync(&mut self, on: bool) {
        if let Target::Window { surface, config } = &mut self.target {
            let mode = if on {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            };
            if config.present_mode != mode {
                config.present_mode = mode;
                surface.configure(&self.device, config);
            }
        }
    }

    /// The size everything renders AT — the supersample buffer's when
    /// one is active, otherwise the target's. Layout and projection key
    /// off this, which is what lets the UI antialias too: it is laid out
    /// against the larger buffer and averaged down with the rest.
    fn size(&self) -> (u32, u32) {
        if let Some(ss) = &self.ssaa {
            return ss.size;
        }
        match &self.target {
            Target::Window { config, .. } => (config.width, config.height),
            Target::Offscreen { width, height, .. } => (*width, *height),
        }
    }

    /// Render one frame to the renderer's own target: acquire the
    /// swapchain image (window) or the offscreen color buffer, draw
    /// into it via [`Self::render_texture`], and present.
    pub fn render(&mut self, cam: &CameraView) -> Result<(), wgpu::SurfaceError> {
        let frame = match &self.target {
            Target::Window { surface, .. } => Some(surface.get_current_texture()?),
            Target::Offscreen { .. } => None,
        };
        let surface_view = match (&frame, &self.target) {
            (Some(f), _) => f.texture.create_view(&Default::default()),
            (None, Target::Offscreen { color, .. }) => color.create_view(&Default::default()),
            _ => unreachable!(),
        };
        self.render_texture(cam, &surface_view);
        if let Some(frame) = frame {
            frame.present();
        }
        Ok(())
    }

    /// Render one frame into `surface_view` — any color view of the
    /// surface format and current size. This is the whole frame minus
    /// target acquisition and present, so an embedder can point it at
    /// its own texture (an XR swapchain image, a capture buffer) and
    /// keep every downstream feature: the supersample and MSAA passes
    /// resolve into the given view exactly as they do onto the window
    /// surface.
    pub fn render_texture(&mut self, cam: &CameraView, surface_view: &wgpu::TextureView) {
        let (w, hpx) = self.size();

        // Book-screen layout (sub_20E60 case 4), native 640×480 scaled to
        // the live resolution. The live world fills the background; the
        // 382×378 map pane pastes top-left and the spellbook grid fills
        // bottom-right, leaving the world visible in the top-right corner
        // (right of the map, above the spellbook) and the bottom strip.
        // Native→screen scale for the book layout (kept distinct from the
        // camera basis's `sx/sy` sin/cos below — the collision zeroed the
        // map pane's height when yaw=0).
        let f = HudFrame::new(w as f32, hpx as f32);
        // The map pane's native height differs per topology: MC1 book
        // 416 (measured), MC2 split 400 (EF:21804 locMinimapHeight).
        let book_map_h = match self.map_layout {
            MapScreenLayout::Mc1Book => BOOK_MAP_H,
            MapScreenLayout::Mc2Split => MC2_MAP_VIEW_H,
        };
        // === The map screen at any aspect ===
        // The three panes are NOT equally elastic, so the layout is
        // solved in dependency order instead of stretched as a block:
        //
        //  1. The SPELLBOOK is rigid — its cells are art (4 cols of 64
        //     native px), so it gets the uniform scale and nothing
        //     else. That fixes the whole RIGHT COLUMN's width.
        //  2. The world VIEWPORT is the free one: it is a 3D view, so
        //     any rect is a legal rect (the projection takes its aspect
        //     from whatever it is handed). It fills the right column
        //     above the spellbook, however wide and tall that is.
        //  3. The MAP PANE takes everything left over — full width to
        //     the left of the column, full height above the log strip.
        //     Its zoom law already keys off its own aspect
        //     (`map_pane_zoom`), so a wider pane simply shows more
        //     world: the map still fits across the pane's SHORTER axis
        //     and wraps toroidally along the longer one, which is why
        //     an arbitrarily wide pane stays readable instead of
        //     zooming the world into a smear.
        //
        // At exactly 4:3 every anchor collapses onto the authored
        // coordinate, reproducing the measured retail layout unchanged.
        let col_x = f.rx(BOOK_SPELL_X);
        // The pane/viewport floor: the black log strip below keeps its
        // native height (MC1 64px; MC2 80px, the CTRL pane's zone).
        let pane_bottom = f.by(book_map_h);
        // The world viewport = the right column above the spellbook. In
        // MC1 its BOTTOM recedes by BOOK_GAP above the spellbook top so
        // a 2px black gap separates them — the horizontal bar of the
        // "T" demarcation (the gap comes out of the live view, not the
        // spellbook). MC2 has no spellbook: the view runs the full pane
        // height (EF:21804).
        let view_rect = (
            col_x as u32,
            0u32,
            (f.w - col_x) as u32,
            match self.map_layout {
                MapScreenLayout::Mc1Book => (f.by(BOOK_SPELL_Y) - f.len(BOOK_GAP)).max(0.0) as u32,
                MapScreenLayout::Mc2Split => pane_bottom as u32,
            },
        );
        // The map pane: origin at the screen corner, right edge receding
        // by BOOK_GAP from the column (the vertical bar of the "T").
        let map_pane = (
            BOOK_MAP_X,
            BOOK_MAP_Y,
            (col_x - f.len(BOOK_GAP)).max(0.0),
            pane_bottom,
        );

        let aspect = if self.map_view {
            // Aspect-true to the viewport rect in BOTH layouts: same
            // fov_y as flight, so the narrow rect shows the MIDDLE
            // SLICE of the normal view — horizontal FOV shrinks in
            // proportion to the width. Senior over the EF:21864
            // "squeeze" reading (which stretched the full-FOV frame into
            // the narrow rect and read squashed).
            view_rect.2 as f32 / view_rect.3.max(1) as f32
        } else {
            w as f32 / hpx as f32
        };
        // The FLIGHT view's field of view at non-4:3, anchored on the
        // retail 4:3 frustum so the projection stays perspective-true
        // (square pixels, no anamorphic squeeze) at any screen shape:
        //
        //   aspect ≥ 4:3 — hold the VERTICAL fov at the retail 60° and
        //     let the horizontal grow. The classic "Hor+" rule: a wide
        //     screen shows the retail frame plus more world to the
        //     sides, never a cropped-and-zoomed version of it.
        //   aspect < 4:3 — hold the HORIZONTAL fov at its 4:3 value and
        //     let the VERTICAL grow instead. Pure Hor+ would cut world
        //     off the sides here, which in a game with rival wizards
        //     hunting you is a real disadvantage handed out by monitor
        //     shape. Anchoring the other axis means every screen sees
        //     AT LEAST the retail 4:3 frustum, and never less.
        //
        // The map screen is deliberately exempt: its viewport keeps the
        // flight `fov_y` into a narrower rect on purpose (the middle-
        // slice ruling above), and re-widening it there would undo
        // that.
        let cam = &CameraView {
            fov_y: if self.map_view {
                cam.fov_y
            } else {
                flight_fov_y(cam.fov_y, aspect)
            },
            ..*cam
        };
        let view_proj = camera_matrix(cam, aspect);
        let sky = self.sky_color_linear();
        let (right, up, fwd) = camera_basis(cam);
        // Billboards expand on the PRE-bank basis — retail counter-
        // rotates its sprite rasterizer by -roll (SetBillboards_3B560),
        // so sprites stand on the terrain, not the rolled viewport.
        let (bb_right, bb_up, _) = camera_flat_basis(cam);
        // The basis w slots carry tan(fov/2) h/v — the sky shader's
        // per-pixel ray reconstruction (billboards read .xyz only).
        let tan_v = (cam.fov_y * 0.5).tan();
        let tan_h = tan_v * aspect;
        // The sea-sheen arm runs on the live view only (the book
        // viewport's sub-rect would need its own transform) and never
        // on caves (no open water; the ceiling pass would mirror
        // nonsense). Only water levels animate (wave_mode!=0).
        let sheen_active = self.reflections
            && !self.map_view
            && self.wave_mode != 0
            && self.ceiling_bind_group.is_none()
            && self.mirror_bind_group.is_some();
        // The mirror pre-pass itself — a full second scene render —
        // only runs when some deep-water tile is within visible
        // reach. With none, no visible fragment can sample the mirror
        // image (haze saturates to the flat sky sheen on all shore
        // water), so skipping the pass is free fps over cities and
        // inland stretches of water levels.
        let mirror_active =
            sheen_active && self.deep_water_in_reach(cam, right, up, fwd, tan_h, tan_v);
        let globals = Globals {
            view_proj,
            camera: [cam.x, cam.y, cam.z, self.fog_distance],
            // The fog alpha slot carries the animation clock (turns).
            fog_color: [sky[0] as f32, sky[1] as f32, sky[2] as f32, self.anim_turn],
            atlas: [
                self.atlas_cells,
                self.smooth_shading as u32,
                self.wave_mode,
                0,
            ],
            cam_right: [right[0], right[1], right[2], tan_h],
            cam_up: [up[0], up[1], up[2], tan_v],
            bb_right: [bb_right[0], bb_right[1], bb_right[2], 0.0],
            bb_up: [bb_up[0], bb_up[1], bb_up[2], 0.0],
            viewport: [
                w as f32,
                hpx as f32,
                // The sheen arm stays live even when the mirror pass
                // is skipped for reach — shore water then wears the
                // full sky haze, which needs no mirror image (the
                // dummy is bound in its place).
                sheen_active as u32 as f32,
                self.lights.len() as f32,
            ],
            lights: {
                let mut arr = [[0.0f32; 4]; MAX_LIGHTS];
                arr[..self.lights.len()].copy_from_slice(&self.lights);
                arr
            },
        };
        self.queue
            .write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        if self.ceiling_bind_group.is_some() {
            // The ceiling pass twin: atlas.w = 1 selects the shader's
            // cave-ceiling arms (fixed wall texture, no water wave).
            let ceiling_globals = Globals {
                atlas: [
                    self.atlas_cells,
                    self.smooth_shading as u32,
                    self.wave_mode,
                    1,
                ],
                ..globals
            };
            self.queue.write_buffer(
                &self.ceiling_globals_buf,
                0,
                bytemuck::bytes_of(&ceiling_globals),
            );
        }
        if mirror_active {
            // The mirror pass globals: atlas.w = 2 flips terrain y in
            // the vertex stage; viewport.z = 0 (a mirror never samples
            // itself).
            let mirror_globals = Globals {
                atlas: [
                    self.atlas_cells,
                    self.smooth_shading as u32,
                    self.wave_mode,
                    2,
                ],
                // A mirror never samples itself (z = 0); the lights
                // still apply (`..globals` keeps the array + count in
                // w) so reflected terrain glows too.
                viewport: [w as f32, hpx as f32, 0.0, self.lights.len() as f32],
                ..globals
            };
            self.queue.write_buffer(
                &self.mirror_globals_buf,
                0,
                bytemuck::bytes_of(&mirror_globals),
            );
            // (Re)create the mirror target at the framebuffer size.
            if self.reflection_view.is_none() || self.reflection_size != (w, hpx) {
                let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("reflection"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: hpx,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = tex.create_view(&Default::default());
                // The blur chain at 1/REFLECTION_BLUR_DIV resolution;
                // the water samples the blurred B, never the raw
                // mirror image.
                let bw = (w / REFLECTION_BLUR_DIV).max(1);
                let bh = (hpx / REFLECTION_BLUR_DIV).max(1);
                let blur_tex = |label: &str| {
                    self.device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: bw,
                                height: bh,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: self.format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                };
                let a_view = blur_tex("reflection-blur-a");
                let b_view = blur_tex("reflection-blur-b");
                let blur_bg = |label: &str, src: &wgpu::TextureView| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(label),
                        layout: &self.blur_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(src),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.reflection_sampler),
                            },
                        ],
                    })
                };
                self.blur_h_bind_group = Some(blur_bg("blur-h", &view));
                self.blur_v_bind_group = Some(blur_bg("blur-v", &a_view));
                self.reflection_bind_group =
                    Some(self.terrain_textures("reflection", Some(&b_view)));
                self.blur_a_view = Some(a_view);
                self.blur_b_view = Some(b_view);
                self.reflection_view = Some(view);
                self.reflection_size = (w, hpx);
                // The mirror pass reuses the main pass's pipelines, so
                // its attachment must match their sample count; it
                // resolves into the sampled texture the water reads.
                self.msaa_mirror = self.make_msaa_target(w, hpx, "msaa-mirror");
                self.msaa_mirror_size = (w, hpx);
            }
        }

        // Billboard instances for this camera (empty when no sprites
        // are loaded); opaque range first, translucent tail sorted
        // back-to-front for the blend pipeline.
        let (instances, opaque_count) = self.billboard_instances(cam);
        let instance_count = instances.len() as u32;
        if !instances.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&instances);
            let need = bytes.len();
            if self.billboard_buf.is_none() || self.billboard_capacity < need {
                self.billboard_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("billboard instances"),
                    size: need.next_power_of_two() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.billboard_capacity = need.next_power_of_two();
            }
            self.queue
                .write_buffer(self.billboard_buf.as_ref().unwrap(), 0, bytes);
        }

        // PROTOTYPE fire particle instances.
        let fire_insts = self.fire_instances(cam);
        let fire_count = fire_insts.len() as u32;
        if !fire_insts.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&fire_insts);
            let need = bytes.len();
            if self.fire_buf.is_none() || self.fire_capacity < need {
                self.fire_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fire instances"),
                    size: need.next_power_of_two() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.fire_capacity = need.next_power_of_two();
            }
            self.queue
                .write_buffer(self.fire_buf.as_ref().unwrap(), 0, bytes);
        }

        // PROTOTYPE lightning-bolt instances.
        let bolt_insts = self.bolt_instances(cam);
        let bolt_count = bolt_insts.len() as u32;
        if !bolt_insts.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&bolt_insts);
            let need = bytes.len();
            if self.bolt_buf.is_none() || self.bolt_capacity < need {
                self.bolt_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bolt instances"),
                    size: need.next_power_of_two() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.bolt_capacity = need.next_power_of_two();
            }
            self.queue
                .write_buffer(self.bolt_buf.as_ref().unwrap(), 0, bytes);
        }

        // Health-bar instances (wrap-nearest like billboards).
        let full = MAP_TILES as f32;
        let wrapn = |p: f32, c: f32| {
            let mut d = p - c;
            if d > full / 2.0 {
                d -= full;
            }
            if d < -full / 2.0 {
                d += full;
            }
            c + d
        };
        let bar_instances: Vec<BarInstance> = self
            .health_bars
            .iter()
            .map(|b| BarInstance {
                pos: [wrapn(b.x, cam.x), b.y, wrapn(b.z, cam.z)],
                size: [b.w, 0.09],
                frac: b.frac.clamp(0.0, 1.0),
            })
            .collect();
        let bar_count = bar_instances.len() as u32;
        if !bar_instances.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&bar_instances);
            let need = bytes.len();
            if self.bar_buf.is_none() || self.bar_capacity < need {
                self.bar_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bar instances"),
                    size: need.next_power_of_two() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.bar_capacity = need.next_power_of_two();
            }
            self.queue
                .write_buffer(self.bar_buf.as_ref().unwrap(), 0, bytes);
        }

        // Screen-space map decorations — upright icon stamps
        // (castle/balloon) and the marching-ants guide path — projected
        // onto whichever map surface is active and appended to the UI
        // quad stream, so they draw unrotated/evenly-spaced over the
        // rotated map. Rect math mirrors the map-globals block below.
        let mut stamp_quads: Vec<UiQuad> = Vec::new();
        {
            // (center, half-extents, zoom, round, aspect, scale) of the
            // active surface, shared by stamps and path.
            let surface = if self.map_view {
                // Same pane rect as the map-globals block, in px.
                let (px0, py0, pw, ph) = map_pane;
                let cx = px0 + pw * 0.5;
                let cy = py0 + ph * 0.5;
                // Icons scale with the pane like every book element
                // (retail only ever rendered ≤640 wide; native-size
                // icons at HD read a third of their proportion) — the
                // UNIFORM factor, so a wide pane spreads the stamps out
                // rather than fattening them.
                Some((
                    cx,
                    cy,
                    pw * 0.5,
                    ph * 0.5,
                    self.map_pane_zoom(pw / ph),
                    false,
                    pw / ph,
                    f.s,
                ))
            } else {
                // Same (diam, center) as the shader uniform — shared via
                // minimap_rect so terrain and stamps can't diverge. The
                // scale tracks the disc (128 native px × w/640, possibly
                // clamped on tiny windows).
                let (disc, cx, cy) = self.minimap_rect(w, hpx);
                Some((
                    cx,
                    cy,
                    disc * 0.5,
                    disc * 0.5,
                    self.minimap_zoom,
                    true,
                    1.0,
                    disc / MINIMAP_DIAM,
                ))
            };
            if let Some((cx, cy, hx, hy, zoom, round, aspect, scale)) = surface {
                // Marker-size deviation: the dots lifted out of the
                // texture bake draw first (under the stamps/ants, the
                // baked layer's z-order). A size-1 dot spans one tile
                // of the surface at its DEFAULT zoom — identical to
                // the baked texel on an unzoomed map screen — and is
                // zoom-INVARIANT on both surfaces (`+`/`-` never
                // resizes markers), times the scale.
                if !self.screen_dots.is_empty() {
                    let default_zoom = if self.map_view {
                        self.map_pane_zoom_base(aspect)
                    } else {
                        MINIMAP_ZOOM
                    };
                    let dot_px = self.marker_scale * 2.0 * hx.min(hy) / default_zoom;
                    stamp_quads = project_map_dots(
                        &self.screen_dots,
                        cx,
                        cy,
                        hx,
                        hy,
                        cam.x,
                        cam.z,
                        cam.yaw,
                        zoom,
                        round,
                        aspect,
                        dot_px,
                    );
                }
                stamp_quads.extend(self.map_stamp_quads(
                    cx, cy, hx, hy, cam.x, cam.z, cam.yaw, zoom, round, aspect, scale,
                ));
                if let Some(path) = &self.map_path {
                    stamp_quads.extend(project_guide_path(
                        path, cx, cy, hx, hy, cam.x, cam.z, cam.yaw, zoom, round, aspect, scale,
                    ));
                }
                if !self.objective_marks.is_empty() {
                    stamp_quads.extend(project_objective_marks(
                        &self.objective_marks,
                        self.objective_tick,
                        cx,
                        cy,
                        hx,
                        hy,
                        cam.x,
                        cam.z,
                        cam.yaw,
                        zoom,
                        round,
                        aspect,
                        scale,
                    ));
                }
            }
        }

        // The level-end fade over the marker layer: in flight the
        // stamps draw after the app UI (and so after its fade quad) —
        // dim them here so the radar markers sink into the black with
        // the rest of the HUD. On the map screen the app UI draws
        // last and the fade quad already covers them.
        if self.overlay_fade > 0.0 && !self.map_view {
            let keep = 1.0 - self.overlay_fade;
            for q in &mut stamp_quads {
                q.tint[3] *= keep;
            }
        }

        // UI quads (screen-space overlay, both views) + the projected
        // map stamps/ants on top — written as two regions of one
        // vertex buffer (no per-frame concatenation copy).
        let ui_count = (self.ui_quads.len() + stamp_quads.len()) as u32;
        // The map-layer region's size — where the extent fog splits
        // the stream on the map screen (map_view puts stamps first).
        let stamp_count = stamp_quads.len() as u32;
        if ui_count > 0 {
            self.queue.write_buffer(
                &self.ui_globals_buf,
                0,
                bytemuck::cast_slice(&[w as f32, hpx as f32, 0.0, 0.0]),
            );
            let ui_bytes: &[u8] = bytemuck::cast_slice(&self.ui_quads);
            let stamp_bytes: &[u8] = bytemuck::cast_slice(&stamp_quads);
            let need = ui_bytes.len() + stamp_bytes.len();
            if self.ui_buf.is_none() || self.ui_capacity < need {
                self.ui_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ui quads"),
                    size: need.next_power_of_two() as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.ui_capacity = need.next_power_of_two();
            }
            let buf = self.ui_buf.as_ref().unwrap();
            // Instance order = z-order (later draws on top). In FLIGHT
            // the stamps go last so the minimap dots read over the
            // radar frame art; on the MAP SCREEN the app UI goes last
            // so its overlays (the mana roster, the book) draw over
            // the projected castle/balloon stamps — retail's roster is
            // painted after the map's wizard marks (sub_22880 runs
            // after sub_48710 in the case-4 view; DrawSorcererScores
            // after DrawMinimapMarks, EF:21942/21952).
            let (first, second) = if self.map_view {
                (stamp_bytes, ui_bytes)
            } else {
                (ui_bytes, stamp_bytes)
            };
            if !first.is_empty() {
                self.queue.write_buffer(buf, 0, first);
            }
            if !second.is_empty() {
                self.queue.write_buffer(buf, first.len() as u64, second);
            }
        }

        // Everything draws into the supersample buffer when there is
        // one; the resolve pass at the end puts it on `surface_view`.
        let scene_view = match &self.ssaa {
            Some(ss) => ss.view.clone(),
            None => surface_view.clone(),
        };
        // With MSAA the main pass draws into the multisampled buffer
        // and RESOLVES into the scene view; without it, straight in.
        // (MSAA and supersampling are exclusive in the UI, but the
        // plumbing composes either way.)
        if self.msaa_color.is_none() && self.samples > 1 {
            let (sw, sh) = self.size();
            self.msaa_color = self.make_msaa_target(sw, sh, "msaa-scene");
        }
        let (color_view, color_resolve) = match &self.msaa_color {
            Some(ms) => (ms.clone(), Some(scene_view.clone())),
            None => (scene_view.clone(), None),
        };

        if self.map_view {
            // The book map pane at native (0,0) 382×378, player-centered
            // and yaw-rotated, rectangular (round mask off). Placed by
            // pixel rect → NDC so it matches the stamp projection.
            let (px0, py0, pw, ph) = map_pane;
            let cx_px = px0 + pw * 0.5;
            let cy_px = py0 + ph * 0.5;
            let map_globals: [f32; 12] = [
                cx_px / w as f32 * 2.0 - 1.0,   // pixel center → NDC x
                1.0 - cy_px / hpx as f32 * 2.0, // pixel center → NDC y (flip)
                pw / w as f32,                  // NDC half-width
                ph / hpx as f32,                // NDC half-height
                cam.x,
                cam.z,
                cam.yaw,
                self.map_pane_zoom(pw / ph),
                0.0,              // rectangular (no round mask)
                pw / ph,          // sampler aspect = pane w/h
                1.0,              // opaque (the map pane sits over the world)
                MAP_TILES as f32, // world period for the toroidal wrap
            ];
            self.queue
                .write_buffer(&self.map_globals_buf, 0, bytemuck::cast_slice(&map_globals));
        } else {
            // In-flight round minimap, corner-anchored at (0,0). Disc +
            // position scale with the HUD (w/640).
            let (disc, cx, cy) = self.minimap_rect(w, hpx);
            let hw = disc / w as f32; // NDC half-width
            let hh = disc / hpx as f32; // NDC half-height
            let minimap_globals: [f32; 12] = [
                cx / w as f32 * 2.0 - 1.0,   // pixel center → NDC x
                1.0 - cy / hpx as f32 * 2.0, // pixel center → NDC y (flip)
                hw,
                hh,
                cam.x,
                cam.z,
                cam.yaw,
                self.minimap_zoom,
                1.0,                // round mask
                1.0,                // square disc → aspect 1
                self.minimap_alpha, // HUD transparency
                MAP_TILES as f32,   // world period for the toroidal wrap
            ];
            self.queue.write_buffer(
                &self.minimap_globals_buf,
                0,
                bytemuck::cast_slice(&minimap_globals),
            );
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        // The water-reflection MIRROR pass: the terrain grid y-flipped
        // about the sea plane (atlas.w = 2), rendered into the mirror
        // texture the main pass's water fragments sample. Terrain
        // only, exactly retail's reflection block (GRO:1104-1431 —
        // sprites are never reflected); cleared to the sky color so
        // open water beyond the mirrored landscape reflects sky.
        if mirror_active
            && let (Some(rv), Some(bg), Some(vb), Some(ib)) = (
                &self.reflection_view,
                &self.mirror_bind_group,
                &self.vertex_buf,
                &self.index_buf,
            )
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mirror"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    // MSAA: draw into the multisampled mirror buffer and
                    // resolve into `rv`, the texture the water samples.
                    view: match &self.msaa_mirror {
                        Some(ms) => ms,
                        None => rv,
                    },
                    resolve_target: self.msaa_mirror.as_ref().map(|_| rv),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky[0],
                            g: sky[1],
                            b: sky[2],
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            // The mirrored SKY behind the mirrored world (no depth
            // write): clouds reflect in open water past the mirrored
            // landscape.
            if let Some(sky_bg) = &self.sky_mirror_bind_group {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(0, sky_bg, &[]);
                pass.draw(0..3, 0..1);
            }
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.set_bind_group(1, &self.reflection_dummy_bind_group, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..9);
            // Mirrored sprites (opaque range only — reflected smoke
            // isn't worth a sorted blend pass). NOT retail (GRO reflects
            // terrain only); a monster over water should show in the
            // water (deliberate).
            if let (1.., Some(bbg), Some(bbuf)) = (
                opaque_count,
                &self.billboard_mirror_bind_group,
                &self.billboard_buf,
            ) {
                pass.set_pipeline(&self.billboard_pipeline);
                pass.set_bind_group(0, bbg, &[]);
                // Group 1: sprites read the sky slots for the
                // mirrored-sky fog target (billboard.wgsl).
                pass.set_bind_group(1, &self.reflection_dummy_bind_group, &[]);
                pass.set_vertex_buffer(0, bbuf.slice(..));
                pass.draw(0..6, 0..opaque_count);
            }
            // PROTOTYPE fire in the reflection (mirror globals flip the
            // quads under the sea plane) — so the flame shows in water.
            if let (1.., Some(buf)) = (fire_count, &self.fire_buf) {
                pass.set_pipeline(&self.fire_pipeline);
                pass.set_bind_group(0, &self.fire_mirror_bind_group, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..fire_count);
            }
            // PROTOTYPE lightning in the reflection.
            if let (1.., Some(buf)) = (bolt_count, &self.bolt_buf) {
                pass.set_pipeline(&self.bolt_pipeline);
                pass.set_bind_group(0, &self.fire_mirror_bind_group, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..bolt_count);
            }
        }
        // Soften the fresh mirror image before the water samples it:
        // the separable gaussian into the 1/REFLECTION_BLUR_DIV-res
        // B target (blur.wgsl; the water's group-1 mirror slot binds
        // B, never the raw mirror).
        if mirror_active
            && let (Some(hbg), Some(vbg), Some(a), Some(b)) = (
                &self.blur_h_bind_group,
                &self.blur_v_bind_group,
                &self.blur_a_view,
                &self.blur_b_view,
            )
        {
            for (pipeline, bg, target) in [
                (&self.blur_h_pipeline, hbg, a),
                (&self.blur_v_pipeline, vbg, b),
            ] {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("reflection blur"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        {
            // The book screen: the world viewport fills the top-right,
            // the map pane the top-left, the spellbook the bottom-right;
            // everything below (the message-log zone) is pure BLACK in
            // retail — the clear shows through with no panel fill.
            let clear = if self.map_view {
                wgpu::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }
            } else {
                wgpu::Color {
                    r: sky[0],
                    g: sky[1],
                    b: sky[2],
                    a: 1.0,
                }
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: color_resolve.as_ref(),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let draw_world = |pass: &mut wgpu::RenderPass<'_>| {
                // The textured parallax sky first (no depth write) —
                // terrain and sprites paint over it, exactly retail's
                // sky-then-world order. Absent, the flat clear/fill
                // color underneath is the sky.
                if let Some(sky_bg) = &self.sky_bind_group {
                    pass.set_pipeline(&self.sky_pipeline);
                    pass.set_bind_group(0, sky_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
                if let (Some(bg), Some(vb), Some(ib)) =
                    (&self.bind_group, &self.vertex_buf, &self.index_buf)
                {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, bg, &[]);
                    // Group 1 = the mirror texture for the water
                    // fragments (a dummy when no mirror pass ran) +
                    // the sky slot the fog/extinction melts sample.
                    match (&self.reflection_bind_group, mirror_active) {
                        (Some(rbg), true) => pass.set_bind_group(1, rbg, &[]),
                        _ => pass.set_bind_group(1, &self.reflection_dummy_bind_group, &[]),
                    }
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    // 3x3 wrap copies; the vertex shader offsets by instance.
                    pass.draw_indexed(0..self.index_count, 0, 0..9);
                    // The MC2 cave ceiling: the same grid again with
                    // the ceiling heightmap + ceiling globals (the
                    // pipeline never culls, so the downward faces
                    // draw as-is; painter plan-depth composites it
                    // like retail's ceiling raster pass).
                    if let Some(cbg) = &self.ceiling_bind_group {
                        pass.set_bind_group(0, cbg, &[]);
                        pass.draw_indexed(0..self.index_count, 0, 0..9);
                    }
                }
                if let (1.., Some(bg), Some(buf)) = (
                    instance_count,
                    &self.billboard_bind_group,
                    &self.billboard_buf,
                ) {
                    pass.set_bind_group(0, bg, &[]);
                    // Group 1 (sky slots for the fog/extinction
                    // melts) — bound here too so sprites draw right
                    // even when no terrain is loaded.
                    match (&self.reflection_bind_group, mirror_active) {
                        (Some(rbg), true) => pass.set_bind_group(1, rbg, &[]),
                        _ => pass.set_bind_group(1, &self.reflection_dummy_bind_group, &[]),
                    }
                    pass.set_vertex_buffer(0, buf.slice(..));
                    if opaque_count > 0 {
                        pass.set_pipeline(&self.billboard_pipeline);
                        pass.draw(0..6, 0..opaque_count);
                    }
                    // Translucent tail (back-to-front): after ALL the
                    // opaque world so smoke blends over terrain and
                    // sprites alike, depth-tested but not written.
                    if instance_count > opaque_count {
                        pass.set_pipeline(&self.billboard_blend_pipeline);
                        pass.draw(0..6, opaque_count..instance_count);
                    }
                }
                // PROTOTYPE fire: premultiplied-additive flame discs,
                // over the world (depth-tested, not written).
                if let (1.., Some(buf)) = (fire_count, &self.fire_buf) {
                    pass.set_pipeline(&self.fire_pipeline);
                    pass.set_bind_group(0, &self.fire_bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..6, 0..fire_count);
                }
                // PROTOTYPE lightning: additive ribbon bolts over the
                // world (depth-tested, not written).
                if let (1.., Some(buf)) = (bolt_count, &self.bolt_buf) {
                    pass.set_pipeline(&self.bolt_pipeline);
                    pass.set_bind_group(0, &self.fire_bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..6, 0..bolt_count);
                }
                if let (1.., Some(buf)) = (bar_count, &self.bar_buf) {
                    pass.set_pipeline(&self.bar_pipeline);
                    pass.set_bind_group(0, &self.bar_bind_group, &[]);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..6, 0..bar_count);
                }
            };
            if self.map_view {
                // World viewport in the top-right corner: sky fill, then
                // the terrain, clipped to the rect.
                let (vx, vy, vw, vh) = view_rect;
                if vw > 0 && vh > 0 {
                    pass.set_viewport(vx as f32, vy as f32, vw as f32, vh as f32, 0.0, 1.0);
                    pass.set_scissor_rect(vx, vy, vw, vh);
                    pass.set_pipeline(&self.fill_pipeline);
                    pass.set_bind_group(0, &self.fill_bind_group, &[]);
                    pass.draw(0..3, 0..1);
                    draw_world(&mut pass);
                    pass.set_viewport(0.0, 0.0, w as f32, hpx as f32, 0.0, 1.0);
                    pass.set_scissor_rect(0, 0, w, hpx);
                }
                // The map pane; the rest of the dark clear is the book
                // backdrop (spell list placeholder).
                if let Some(bg) = &self.map_bind_group {
                    pass.set_pipeline(&self.map_pipeline);
                    pass.set_bind_group(0, bg, &[]);
                    pass.draw(0..6, 0..1);
                }
            } else {
                draw_world(&mut pass);
                // In-flight round minimap in the corner (round mask
                // discards outside the disc); present once a level is
                // loaded.
                if let Some(bg) = &self.minimap_bind_group {
                    pass.set_pipeline(&self.map_pipeline);
                    pass.set_bind_group(0, bg, &[]);
                    pass.draw(0..6, 0..1);
                }
            }
            // Screen-space UI on top of either view. With the extent
            // fog on, the map screen's stream splits: map layers (the
            // stamps region — dots, icons, ants, objective marks) →
            // fog → app UI, so the fog is the topmost MAP layer
            // (player ruling) but never covers the book/roster UI.
            let map_fog = self.map_view && self.extent_fog;
            if let (1.., Some(bg), Some(buf)) = (ui_count, &self.ui_bind_group, &self.ui_buf) {
                pass.set_pipeline(&self.ui_pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                if map_fog && let Some(mbg) = &self.map_bind_group {
                    if stamp_count > 0 {
                        pass.draw(0..6, 0..stamp_count);
                    }
                    pass.set_pipeline(&self.fog_pipeline);
                    pass.set_bind_group(0, mbg, &[]);
                    pass.draw(0..6, 0..1);
                    if ui_count > stamp_count {
                        pass.set_pipeline(&self.ui_pipeline);
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..6, stamp_count..ui_count);
                    }
                } else {
                    pass.draw(0..6, 0..ui_count);
                }
            } else if map_fog && let Some(mbg) = &self.map_bind_group {
                // No UI quads at all — the fog still veils the pane.
                pass.set_pipeline(&self.fog_pipeline);
                pass.set_bind_group(0, mbg, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        // Resolve the supersample buffer down to the surface.
        if let Some(ss) = &self.ssaa {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ssaa-resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.ssaa_pipeline);
            pass.set_bind_group(0, &ss.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Read back the offscreen target as tightly-packed RGBA8 rows.
    /// Panics if the renderer targets a window.
    pub fn read_offscreen(&self) -> (u32, u32, Vec<u8>) {
        let Target::Offscreen {
            color,
            width,
            height,
        } = &self.target
        else {
            panic!("read_offscreen on a windowed renderer");
        };
        let (width, height) = (*width, *height);
        let unpadded = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).ok();
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .expect("map_async callback dropped")
            .expect("buffer map failed");
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((unpadded * height) as usize);
        for row in 0..height {
            let start = (row * padded) as usize;
            out.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        (width, height, out)
    }
}

fn request_device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), RenderError> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        force_fallback_adapter: false,
    }))
    .ok_or(RenderError::NoAdapter)?;
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("mgcarpet"),
            ..Default::default()
        },
        None,
    ))
    .map_err(|e| RenderError::Device(e.to_string()))?;
    Ok((adapter, device, queue))
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32, samples: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Column-major view-projection matrix. Yaw 0 faces -Z, positive pitch
/// looks up; right-handed, Y-up, depth 0..1.
/// Project a world point (tile x/z + altitude — [`LivePose`] space)
/// to surface pixels through the same wrap-to-camera rule and matrix
/// as the world pass. `None` when the point is at/behind the near
/// plane. For screen-space overlays anchored to world positions (the
/// aim crosshair; future name labels).
/// The FLIGHT view's effective vertical FOV at a given aspect, anchored
/// on the retail 4:3 frustum so the projection stays perspective-true
/// (square pixels, no anamorphic squeeze) at any screen shape:
///
/// * `aspect >= 4:3` — hold the VERTICAL fov at retail's 60° and let
///   the horizontal grow. The classic "Hor+" rule: a wide screen shows
///   the retail frame PLUS more world to the sides, never a cropped
///   and zoomed version of it.
/// * `aspect < 4:3` — hold the HORIZONTAL fov at its 4:3 value and let
///   the VERTICAL grow instead. Pure Hor+ would cut world off the sides
///   here, and in a game with rival wizards hunting you that is a real
///   disadvantage handed out by monitor shape. Anchoring the other axis
///   means every screen sees AT LEAST the retail 4:3 frustum.
///
/// At exactly 4:3 both arms return `fov_y` unchanged. Shared by the
/// renderer and [`world_to_screen`] — the crosshair and the world-
/// anchored markers project through the SAME frustum the scene is
/// drawn with, or they drift apart at non-4:3.
pub fn flight_fov_y(fov_y: f32, aspect: f32) -> f32 {
    let ref_aspect = NATIVE_W / NATIVE_H;
    if aspect >= ref_aspect {
        fov_y
    } else {
        2.0 * ((fov_y * 0.5).tan() * ref_aspect / aspect).atan()
    }
}

pub fn world_to_screen(
    cam: &CameraView,
    surface_w: f32,
    surface_h: f32,
    x: f32,
    alt: f32,
    z: f32,
) -> Option<(f32, f32)> {
    let full = MAP_TILES as f32;
    let wrapn = |p: f32, c: f32| {
        let mut d = p - c;
        if d > full / 2.0 {
            d -= full;
        }
        if d < -full / 2.0 {
            d += full;
        }
        c + d
    };
    let aspect = surface_w / surface_h;
    let m = camera_matrix(
        &CameraView {
            fov_y: flight_fov_y(cam.fov_y, aspect),
            ..*cam
        },
        aspect,
    );
    let v = [wrapn(x, cam.x), alt, wrapn(z, cam.z), 1.0];
    // `m` is column-major (see camera_matrix): clip_r = Σc m[c][r]·v[c].
    let clip = |r: usize| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2] + m[3][r];
    let w = clip(3);
    if w <= 0.05 {
        return None;
    }
    Some((
        (clip(0) / w * 0.5 + 0.5) * surface_w,
        (0.5 - clip(1) / w * 0.5) * surface_h,
    ))
}

fn camera_matrix(cam: &CameraView, aspect: f32) -> [[f32; 4]; 4] {
    let (right, up, fwd) = camera_basis(cam);
    let eye = [cam.x, cam.y, cam.z];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    // View matrix: camera basis rows, look direction mapped to -Z.
    let view = [
        [right[0], up[0], -fwd[0], 0.0],
        [right[1], up[1], -fwd[1], 0.0],
        [right[2], up[2], -fwd[2], 0.0],
        [-dot(right, eye), -dot(up, eye), dot(fwd, eye), 1.0],
    ];

    // Perspective, near 0.05 tiles, far 600 (a 256-tile world plus fog
    // headroom), depth 0..1.
    let (near, far) = (0.05_f32, 600.0_f32);
    let f = 1.0 / (cam.fov_y * 0.5).tan();
    let proj = [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ];

    // proj * view, both column-major.
    let mut out = [[0.0f32; 4]; 4];
    for (c, out_col) in out.iter_mut().enumerate() {
        for (r, out_cell) in out_col.iter_mut().enumerate() {
            *out_cell = (0..4).map(|k| proj[k][r] * view[c][k]).sum();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The retail proximity-concealment ramp: full inside FogStart
    /// (15 tiles), gone at FogEnd (19 tiles), retail's linear-in-d²
    /// fog-row law between (GRO:3505-3511 at 256 units/tile).
    #[test]
    fn conceal_visibility_runs_retails_fog_band() {
        assert_eq!(Renderer::conceal_visibility(0.0), 1.0);
        assert_eq!(Renderer::conceal_visibility(15.0 * 15.0), 1.0);
        assert_eq!(Renderer::conceal_visibility(19.0 * 19.0), 0.0);
        assert_eq!(Renderer::conceal_visibility(25.0 * 25.0), 0.0);
        // Midband: d² halfway between 15² and 19² → exactly half.
        let mid = (15.0 * 15.0 + 19.0 * 19.0) / 2.0;
        assert!((Renderer::conceal_visibility(mid) - 0.5).abs() < 1e-6);
    }

    /// world_to_screen: a point dead ahead lands at screen center; a
    /// point behind the camera is rejected; the world wrap picks the
    /// nearest image (a target across the seam still projects).
    #[test]
    fn world_to_screen_centers_rejects_and_wraps() {
        let cam = CameraView {
            x: 10.0,
            y: 5.0,
            z: 10.0,
            yaw: 0.0, // fwd = [0, 0, -1]
            pitch: 0.0,
            roll: 0.0,
            fov_y: 1.0,
        };
        let (w, h) = (640.0, 480.0);
        let (sx, sy) = world_to_screen(&cam, w, h, 10.0, 5.0, 0.0).unwrap();
        assert!((sx - 320.0).abs() < 0.01 && (sy - 240.0).abs() < 0.01);
        assert!(
            world_to_screen(&cam, w, h, 10.0, 5.0, 20.0).is_none(),
            "behind"
        );
        // Same point expressed across the 256-tile seam: z = -10 and
        // z = 246 are the same world position 20 tiles ahead.
        let a = world_to_screen(&cam, w, h, 12.0, 5.0, -10.0).unwrap();
        let b = world_to_screen(&cam, w, h, 12.0, 5.0, 246.0).unwrap();
        assert!((a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01);
    }

    /// A bolt segment near the camera's antipode must wrap as a RIGID
    /// unit: both endpoints stay the strike's true short length apart,
    /// never split a map-width by the ±half-map seam. Non-vacuity: the
    /// OLD per-endpoint wrap (`wrap(p0,cam)`, `wrap(p1,cam)` independently)
    /// would put the two ends ~256 tiles apart here, failing the assert.
    #[test]
    fn bolt_wraps_as_a_rigid_unit_across_the_seam() {
        let full = 256.0;
        // Camera at the origin; the strike sits ~128 tiles away (the
        // antipode) with its two ends straddling the +128 camera seam:
        // 127.6 stays put, 128.4 wraps to -127.6 under a per-endpoint wrap.
        let cam_x = 0.0;
        let cam_z = 0.0;
        let p0: [f32; 3] = [127.6, 5.0, 0.0];
        let p1: [f32; 3] = [128.4, 5.0, 0.0];
        let anchor: [f32; 2] = [p0[0], p0[2]]; // shared bolt origin
        let true_len = ((p1[0] - p0[0]).powi(2) + (p1[2] - p0[2]).powi(2)).sqrt();
        let (w0, w1) = wrap_bolt_to_camera(anchor, p0, p1, cam_x, cam_z, full);
        let drawn_len = ((w1[0] - w0[0]).powi(2) + (w1[2] - w0[2]).powi(2)).sqrt();
        // The whole bug in one line: independent per-endpoint wrapping puts
        // these ~0.8 tiles apart at ~255 tiles apart.
        assert!(
            (drawn_len - true_len).abs() < 1e-3,
            "the segment keeps its true length, not split by the seam \
             (true {true_len}, drawn {drawn_len})"
        );
        // Both ends resolve to (roughly) the camera's half-map window — a
        // seam-straddling bolt inherently has one end a hair past ±half.
        assert!(
            (w0[0] - cam_x).abs() <= full / 2.0 + 1.0 && (w1[0] - cam_x).abs() <= full / 2.0 + 1.0,
            "both ends resolve near the camera, not a map away"
        );
    }

    /// A short bolt near the camera is untouched (offset 0): the rigid
    /// wrap must not perturb the common case.
    #[test]
    fn bolt_near_camera_is_unchanged() {
        let full = 256.0;
        let anchor: [f32; 2] = [10.0, 10.0];
        let p0: [f32; 3] = [10.0, 3.0, 10.0];
        let p1: [f32; 3] = [12.0, 3.0, 13.0];
        let (w0, w1) = wrap_bolt_to_camera(anchor, p0, p1, 10.0, 10.0, full);
        assert_eq!((w0, w1), (p0, p1));
    }

    /// The flight FOV law: 4:3 is untouched, wide screens gain
    /// horizontal view without losing vertical, narrow screens gain
    /// vertical view without losing horizontal. In every case the
    /// frustum CONTAINS the retail 4:3 one — no screen shape can see
    /// less than retail did.
    #[test]
    fn flight_fov_never_shows_less_than_the_retail_frustum() {
        let base = 60.0_f32.to_radians();
        let ref_a = NATIVE_W / NATIVE_H;
        let tan_v_ref = (base * 0.5).tan();
        let tan_h_ref = tan_v_ref * ref_a;
        for &aspect in &[ref_a, 16.0 / 9.0, 21.0 / 9.0, 5.0 / 4.0, 1.0, 0.75] {
            let fov = flight_fov_y(base, aspect);
            let tan_v = (fov * 0.5).tan();
            let tan_h = tan_v * aspect;
            assert!(
                tan_v >= tan_v_ref - 1e-5,
                "vertical view shrank at aspect {aspect}"
            );
            assert!(
                tan_h >= tan_h_ref - 1e-5,
                "horizontal view shrank at aspect {aspect}"
            );
        }
        // 4:3 is the identity — the retail presentation is unmoved.
        assert_eq!(flight_fov_y(base, ref_a), base);
        // Wide: vertical pinned, horizontal grows.
        assert_eq!(flight_fov_y(base, 16.0 / 9.0), base);
        // Narrow: horizontal pinned exactly, vertical grows.
        let fov = flight_fov_y(base, 1.0);
        assert!(fov > base);
        assert!(((fov * 0.5).tan() * 1.0 - tan_h_ref).abs() < 1e-5);
    }

    /// The marker-size deviation's screen-space dots
    /// (`project_map_dots`): centered on the entity, sized purely by
    /// `dot_px` (zoom compensation is the CALLER folding the default
    /// zoom in, so the projector itself must not scale with `zoom`
    /// beyond position), the solid-quad uv (all zero) preserved
    /// through the clip, the wrap image visible across the seam, and
    /// the radar disc cull honored.
    #[test]
    fn screen_dots_center_wrap_and_clip() {
        let dot = |x, z, size| ScreenDot {
            x,
            z,
            size,
            tint: [0.5, 0.25, 0.125, 1.0],
        };
        let (cx, cy, hx, hy) = (200.0, 200.0, 200.0, 200.0);
        // At the player: centered on the pane center, side dot_px per
        // size unit, at ANY heading (the center is rotation-fixed).
        let q = project_map_dots(
            &[dot(50.0, 128.0, 1.0), dot(50.0, 128.0, 2.0)],
            cx,
            cy,
            hx,
            hy,
            50.0,
            128.0,
            0.7,
            256.0,
            false,
            1.0,
            6.0,
        );
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].rect, [cx - 3.0, cy - 3.0, 6.0, 6.0]);
        assert_eq!(q[1].rect, [cx - 6.0, cy - 6.0, 12.0, 12.0]);
        assert_eq!(q[0].uv, [0.0; 4], "solid-quad marker survives the clip");
        assert_eq!(q[0].tint, [0.5, 0.25, 0.125, 1.0]);
        // Across the seam: player at x=1, dot at x=255 is 2 tiles
        // LEFT (the wrapped image), not 254 right. 64 tiles across
        // 400 px = 6.25 px/tile → 12.5 px left of center.
        let q = project_map_dots(
            &[dot(255.0, 128.0, 1.0)],
            cx,
            cy,
            hx,
            hy,
            1.0,
            128.0,
            0.0,
            64.0,
            false,
            1.0,
            4.0,
        );
        assert_eq!(q.len(), 1);
        let center = (
            q[0].rect[0] + q[0].rect[2] * 0.5,
            q[0].rect[1] + q[0].rect[3] * 0.5,
        );
        assert!(
            (center.0 - (cx - 12.5)).abs() < 0.01 && (center.1 - cy).abs() < 0.01,
            "wrapped image at {center:?}"
        );
        // Radar disc cull: (+28,+28) at zoom 64 is inside the square
        // bounds (28/32 per axis) but outside the unit disc (1.24) —
        // only the round mask can drop it, and it must.
        let q = project_map_dots(
            &[dot(78.0, 156.0, 1.0)],
            cx,
            cy,
            hx,
            hy,
            50.0,
            128.0,
            0.0,
            64.0,
            true,
            1.0,
            4.0,
        );
        assert!(q.is_empty(), "outside the disc");
    }

    fn stamp_at(x: f32, z: f32) -> MapStamp {
        MapStamp {
            x,
            z,
            w: 16,
            h: 15,
            uv: [0.0, 0.0, 16.0, 15.0],
            anchor: [0.0, 1.0],
        }
    }

    #[test]
    fn book_map_stamp_survives_a_full_yaw_sweep() {
        // Any delta within the pane's inscribed 128-tile disc is
        // visible at EVERY heading (|R·d| ≤ |d| ≤ both half-spans), so
        // a (+90,+90) stamp must never vanish across a full rotation —
        // the rotated-space cull can't lose it. (Deltas OUTSIDE the
        // disc can legitimately leave the pane at diagonal headings —
        // a 256-tile span rotated 45° misses some wrapped images; the
        // map texture hides those tiles too, and the projection
        // matches the shader image-for-image by construction.)
        let stamps = [stamp_at(140.0, 218.0)]; // (+90,+90) from (50,128)
        let (pw, ph) = (382.0, 416.0);
        for i in 0..=90 {
            let yaw = i as f32 * std::f32::consts::TAU / 90.0;
            let quads = project_map_stamps(
                &stamps,
                pw * 0.5,
                ph * 0.5,
                pw * 0.5,
                ph * 0.5,
                50.0,
                128.0,
                yaw,
                256.0,
                false,
                pw / ph,
                1.0,
            );
            assert!(
                !quads.is_empty(),
                "stamp vanished at yaw {yaw:.3} (step {i})"
            );
        }
    }

    #[test]
    fn edge_stamps_draw_their_wrap_duplicate() {
        // The pane's y-span (256/aspect ≈ 279 tiles) exceeds the world
        // period, so tiles near the top/bottom edge appear TWICE — the
        // shader's fract() shows both, and the projection must emit
        // both quads (a shortest-wrap image would miss the second copy
        // at the opposite edge).
        let stamps = [stamp_at(50.0, 128.0 + 139.0)]; // near the +y limit
        let (pw, ph) = (382.0, 416.0);
        let quads = project_map_stamps(
            &stamps,
            pw * 0.5,
            ph * 0.5,
            pw * 0.5,
            ph * 0.5,
            50.0,
            128.0,
            0.0,
            256.0,
            false,
            pw / ph,
            1.0,
        );
        assert_eq!(quads.len(), 2, "both wrap images of an edge stamp draw");
    }

    #[test]
    fn stamps_scale_and_clip_to_the_surface() {
        // 2× scale doubles the rect; a stamp whose anchor sits just
        // inside the pane edge is clipped to the bounds with its uv
        // trimmed proportionally (never dropped, never bleeding).
        let stamps = [stamp_at(10.0, 128.0)];
        let (pw, ph) = (382.0, 416.0);
        let run = |scale: f32| {
            project_map_stamps(
                &stamps,
                pw * 0.5,
                ph * 0.5,
                pw * 0.5,
                ph * 0.5,
                10.0,
                128.0,
                0.0,
                256.0,
                false,
                pw / ph,
                scale,
            )
        };
        let q1 = run(1.0);
        let q2 = run(2.0);
        assert_eq!(q1.len(), 1);
        assert_eq!(q2.len(), 1);
        assert!(
            (q2[0].rect[2] - q1[0].rect[2] * 2.0).abs() < 1e-3,
            "rect scales"
        );

        // Anchor just inside the pane's left edge: bottom-left-anchored
        // sprite extends UP from the point; the top may clip at y=0
        // when near the top edge. Force a corner case: player centered,
        // stamp at the pane center → no clipping; stamp image near the
        // pane's top-left corner → the rect is clipped to bounds.
        let corner = [stamp_at(10.0 - 190.9, 128.0 - 276.0)]; // near pane top-left
        let q = project_map_stamps(
            &corner,
            pw * 0.5,
            ph * 0.5,
            pw * 0.5,
            ph * 0.5,
            10.0,
            128.0,
            0.0,
            256.0,
            false,
            pw / ph,
            1.0,
        );
        for quad in &q {
            assert!(
                quad.rect[0] >= 0.0 && quad.rect[1] >= 0.0,
                "clipped to bounds"
            );
            assert!(quad.rect[0] + quad.rect[2] <= pw + 1e-3);
            assert!(quad.rect[1] + quad.rect[3] <= ph + 1e-3);
            assert!(quad.uv[2] > 0.0, "uv width stays positive (textured mode)");
        }
    }

    /// A deterministic islands-and-sea tile-type plane for shore
    /// tests (LCG; ~1/4 land in clustered patches).
    fn shore_test_map(n: usize) -> Vec<u8> {
        let mut seed = 0x2545_f491_u32;
        let mut types = vec![0u8; n * n];
        for _ in 0..n * n / 24 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = (seed >> 8) as usize % n;
            let z = (seed >> 20) as usize % n;
            for dz in 0..3 {
                for dx in 0..3 {
                    types[((z + dz) % n) * n + (x + dx) % n] = 1;
                }
            }
        }
        types
    }

    /// The CPU shore bake reproduces the shader's former per-fragment
    /// 7x7 kernel verbatim: for every texel center, distance to the
    /// nearest non-deep-water tile rect among the texel's tile ±3,
    /// saturated and quantized identically.
    #[test]
    fn shore_bake_matches_the_shader_kernel_law() {
        let n = 64;
        let types = shore_test_map(n);
        let mut field = vec![255u8; n * SHORE_RES * n * SHORE_RES];
        bake_shore_region(&types, n, &mut field, 0, 0, n, n);
        let s = n * SHORE_RES;
        for tex_z in (0..s).step_by(3) {
            for tex_x in (0..s).step_by(3) {
                // The shader kernel, literally: fragment position =
                // texel center, tile = floor(position).
                let px = (tex_x as f32 + 0.5) / SHORE_RES as f32;
                let pz = (tex_z as f32 + 0.5) / SHORE_RES as f32;
                let (tbx, tbz) = (px.floor() as i32, pz.floor() as i32);
                let mut shore = 9.0f32;
                for dz in -3i32..=3 {
                    for dx in -3i32..=3 {
                        let (tx, tz) = (tbx + dx, tbz + dz);
                        let wx = tx.rem_euclid(n as i32) as usize;
                        let wz = tz.rem_euclid(n as i32) as usize;
                        if types[wz * n + wx] != 0 {
                            let ddx = (tx as f32 - px).max(px - (tx as f32 + 1.0)).max(0.0);
                            let ddz = (tz as f32 - pz).max(pz - (tz as f32 + 1.0)).max(0.0);
                            shore = shore.min((ddx * ddx + ddz * ddz).sqrt());
                        }
                    }
                }
                let want = (shore.min(SHORE_MAX) / SHORE_MAX * 255.0).round() as u8;
                assert_eq!(field[tex_z * s + tex_x], want, "texel ({tex_x},{tex_z})");
            }
        }
    }

    /// The incremental rebake (diff + dirty ±3 tiles) lands on the
    /// exact same field as a from-scratch bake of the mutated map —
    /// the runtime crater path can't drift from the law.
    #[test]
    fn shore_incremental_rebake_equals_full_bake() {
        let n = 64;
        let old = shore_test_map(n);
        let mut new = old.clone();
        // A crater: dig a 4x4 patch of land to deep water across a
        // wrap seam, and raise one sea tile.
        for dz in 0..4 {
            for dx in 0..4 {
                new[((62 + dz) % n) * n + (62 + dx) % n] = 0;
            }
        }
        new[10 * n + 10] = 1;
        let mut incremental = vec![255u8; n * SHORE_RES * n * SHORE_RES];
        bake_shore_region(&old, n, &mut incremental, 0, 0, n, n);
        rebake_shore_changed(&mut incremental, &old, &new, n);
        let mut full = vec![255u8; n * SHORE_RES * n * SHORE_RES];
        bake_shore_region(&new, n, &mut full, 0, 0, n, n);
        assert_eq!(incremental, full);
    }
}
