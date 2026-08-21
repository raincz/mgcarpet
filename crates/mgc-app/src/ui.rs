//! Spellbook + HUD quad building over the bundle's HSPR UI sprites.
//!
//! Icon colors are resolved the original's way: every 2D blit runs
//! `blend[src | dest<<8]` against the pixel underneath (remc1
//! `strPal.byte_BB934_BB924`, sub_main.cpp:27444/27564 — the LUT is
//! the bundle's `blend-lut.bin`, TABLES +0x4000). We pre-composite
//! each sprite once against the book backdrop (dest 0 = black) into
//! an RGBA atlas, which reproduces the authentic icon colors (the
//! red heal heart) without palette machinery in the shader.
//!
//! Layout is functional-first: the book screen's bottom-right rect
//! gets the 24-slot grid in the original's display order
//! (`byte_99B88`); the in-game HUD gets the two equipped slots
//! bottom-left/right.

use mgc_formats::bundle::SpriteIndex;
use mgc_render::UiQuad;
use mgc_sim::engine::world::{LifeState, LoadoutView, PlayerVitals};
use mgc_sim::mc1::spells::{DISPLAY_ORDER, SPELL_COUNT, SpellId};
use mgc_sim::mc2::cast::Mc2BookView;

/// UI sprite ids (remc1 begSprTab layout).
const SPR_HILITE_LEFT: u32 = 1;
const SPR_HILITE_RIGHT: u32 = 2;
const SPR_SLOT_BG: u32 = 3;
const ICON_W: f32 = 62.0;
const ICON_H: f32 = 34.0;

pub struct UiAssets {
    pub atlas_w: u32,
    pub atlas_h: u32,
    pub atlas_rgba: Vec<u8>,
    /// Atlas uv (texels) of the pre-composited icon-on-slab tiles,
    /// indexed by internal spell id: [plain, left-equipped,
    /// right-equipped] — the equip highlights (sprites 1/2) are
    /// blend-composited over the icon like everything else, so they
    /// bake as whole-tile variants rather than overlay quads.
    slot_uv: [[[f32; 4]; 3]; SPELL_COUNT],
    /// Base-atlas frame rects (x, y, w, h) per HSPR sprite id — the
    /// map's icon-marker crops.
    sprite_rects: Vec<Option<(u32, u32, u32, u32)>>,
    /// Where the appended retail `DATA/POINTERS` bank begins in
    /// `sprite_rects` (0 = none/older bake). Entries follow at
    /// `pointer_base + k`, k = the retail bank index.
    pointer_base: usize,
    /// MC2 selector GRID tiles (pre-composited box + icon, one bit-
    /// copy per state so the draw path never layers): uv per spell ×
    /// [0 castable(89), 1 hovered shot-meter(87), 2 owned-unaffordable
    /// ghost(91 + LUT-blended icon), 3 unowned grey relief(89 +
    /// colourize-0xA6 icon)]. Empty on MC1 atlases.
    pane_uv: Vec<[[f32; 4]; 4]>,
    /// MC2 selector FLYOUT tiles: uv per (spell·3 + level) ×
    /// [0 lit(161), 1 pool-fail(162 + LUT-ghosted icon), 2 broke
    /// (162 + LIT icon — retail keys the tile on `canSubSummon &&
    /// mana/cost` at EF:22618 but ghosts the icon on the pool test
    /// ALONE at EF:22625-28)], number badge + per-level icon baked
    /// in. Empty on MC1 atlases.
    sub_uv: Vec<[[f32; 4]; 3]>,
    /// Messaging-font glyph UV rects (texels into `atlas_rgba`), indexed
    /// by sprite id = ASCII char + 1 (id 33 = space); None for absent
    /// glyphs. The masks are baked WHITE — text is tinted at draw time,
    /// faithful to the original's `DrawText(text, x, y, color)` where the
    /// glyph is a coverage mask and `color` picks the ink. Empty when the
    /// bundle carries no font.
    glyph_uv: Vec<Option<[f32; 4]>>,
    /// The font's line height (tallest glyph cell), source pixels.
    line_height: f32,
    /// Spider-web overlay tile UV rects (texels), indexed by HSPR
    /// sprite id — ids 1..=24 are the 6×4 grid covering the 640×480
    /// viewport (remc2 EF:21671-709). Empty when the bundle carries
    /// no web bank (MC1 / pre-epoch-15 bakes).
    web_uv: Vec<Option<[f32; 4]>>,
}

/// Inter-glyph advance added to each glyph's own width, source pixels.
/// The HSPR font glyphs carry their own side-bearing, so 0 tracks the
/// original's `x += GetLetterWidth` walk.
const GLYPH_SPACING: f32 = 0.0;
/// Fallback advance for an unmapped byte (source pixels).
const GLYPH_FALLBACK_ADVANCE: f32 = 6.0;

/// Icon treatment when compositing a pane tile — the three original
/// blit rules (trace §2.4): `DrawBitmap` raw, `DrawTransparentBitmap`
/// through the blend LUT, `DrawColourizedBitmap(0xA6)` = the LUT's
/// 0xA6 row against the box pixel (the dark-relief ink).
#[derive(Clone, Copy)]
enum PaneInk {
    Raw,
    Blend,
    Colour(u8),
}

/// Overlay one 8bpp sprite onto an 8bpp tile at (ox, oy) with an ink
/// rule; `resolve` = the blend LUT lookup.
fn overlay8(
    tile: &mut [u8],
    (tw, th): (usize, usize),
    spr: &(usize, usize, Vec<u8>),
    (ox, oy): (usize, usize),
    ink: PaneInk,
    resolve: &impl Fn(u8, u8) -> u8,
) {
    let (iw, ih, px) = spr;
    for y in 0..*ih {
        for x in 0..*iw {
            let (dx, dy) = (ox + x, oy + y);
            if dx >= tw || dy >= th {
                continue;
            }
            let s = px[y * iw + x];
            if s == 0 {
                continue;
            }
            let d = &mut tile[dy * tw + dx];
            *d = match ink {
                PaneInk::Raw => s,
                PaneInk::Blend => resolve(s, *d),
                PaneInk::Colour(c) => resolve(c, *d),
            };
        }
    }
}

/// Write an 8bpp tile into the RGBA atlas at (tx, ty) through the
/// palette (0 = transparent).
fn emit8(
    rgba: &mut [u8],
    base_w: usize,
    palette: &[[u8; 4]; 256],
    (tx, ty): (usize, usize),
    (tw, th): (usize, usize),
    tile: &[u8],
) {
    for y in 0..th {
        for x in 0..tw {
            let v = tile[y * tw + x];
            if v == 0 {
                continue;
            }
            let c = palette[v as usize];
            let o = ((ty + y) * base_w + tx + x) * 4;
            rgba[o..o + 3].copy_from_slice(&c[..3]);
            rgba[o + 3] = 255;
        }
    }
}

impl UiAssets {
    /// Composite the 8bpp UI atlas to RGBA through the blend LUT and
    /// the world palette. Two dest treatments, both the original's
    /// blit rule `blend[src | dest<<8]`:
    /// - the base atlas composites against dest 0 (dark backdrop);
    /// - the 24 spell icons ADDITIONALLY bake as icon-on-slot-slab
    ///   tiles (appended below the base atlas), compositing each
    ///   pixel against the slab sprite's — several icon ramps
    ///   (fireball flame, the possess glow) are luminous
    ///   brighten-the-dest rows that only read correctly over the
    ///   stone slab, exactly as the original draws them.
    ///
    /// `book_tiles` = pre-composite the MC1 icon-on-slab tiles (the
    /// MC1 book/HUD entry map: slab `[3]`, icons `[6+spell]`). False
    /// for MC2 atlases, whose ids mean different sprites (selector
    /// pane boxes at 87..91, icons at 97+, sub-icons at 179+).
    pub fn build(
        index: SpriteIndex,
        pixels: &[u8],
        palette: &[[u8; 4]; 256],
        blend_lut: Option<&[u8]>,
        book_tiles: bool,
        font: Option<(&SpriteIndex, &[u8])>,
        web: Option<(&SpriteIndex, &[u8])>,
    ) -> Self {
        let resolve = |src: u8, dest: u8| -> u8 {
            match blend_lut {
                Some(lut) => lut[src as usize | (dest as usize) << 8],
                None => src,
            }
        };
        let base_w = index.atlas_width as usize;
        let base_h = index.atlas_height as usize;

        // Slab sprite (entry 3) as an 8bpp grid, for per-pixel dests.
        let sprite_px = |id: usize| -> Option<(usize, usize, Vec<u8>)> {
            let e = index.sprites.get(id)?;
            let f = e.frames.first()?;
            let (w, h) = (e.width as usize, e.height as usize);
            let mut out = vec![0u8; w * h];
            for y in 0..h {
                let row = (f.y as usize + y) * base_w + f.x as usize;
                out[y * w..(y + 1) * w].copy_from_slice(&pixels[row..row + w]);
            }
            Some((w, h, out))
        };
        let slab = sprite_px(SPR_SLOT_BG as usize);

        // Composited slot tiles appended below the base atlas: 3
        // variants per spell (plain / left-equip / right-equip
        // highlight), 8 per row.
        let hilites = [
            None,
            sprite_px(SPR_HILITE_LEFT as usize),
            sprite_px(SPR_HILITE_RIGHT as usize),
        ];
        let (tile_w, tile_h) = slab
            .as_ref()
            .map(|(w, h, _)| (*w, *h))
            .unwrap_or((ICON_W as usize + 2, ICON_H as usize + 3));
        let tiles_per_row = base_w / tile_w;
        let tile_count = if book_tiles { SPELL_COUNT * 3 } else { 0 };
        let tile_rows = tile_count.div_ceil(tiles_per_row);

        // MC2 selector tiles (docs/traces/mc2-spell-selector-ui.md):
        // per spell 4 grid variants + 3 levels × 2 flyout variants,
        // appended below the base atlas like the MC1 book tiles.
        let pane_box = (!book_tiles).then(|| sprite_px(MC2_SPR_BOX)).flatten();
        let (gw, gh) = pane_box
            .as_ref()
            .map(|(w, h, _)| (*w, *h))
            .unwrap_or((48, 40));
        let sub_box = (!book_tiles).then(|| sprite_px(MC2_SPR_SUB_OK)).flatten();
        let (sw, sh) = sub_box
            .as_ref()
            .map(|(w, h, _)| (*w, *h))
            .unwrap_or((48, 36));
        let n_mc2 = if book_tiles { 0 } else { MC2_SPELL_NAMES.len() };
        let grid_per_row = (base_w / gw.max(1)).max(1);
        let grid_rows = (n_mc2 * 4).div_ceil(grid_per_row);
        let sub_per_row = (base_w / sw.max(1)).max(1);
        let sub_rows = (n_mc2 * 3 * 3).div_ceil(sub_per_row);
        let grid_y0 = base_h + tile_rows * tile_h;
        let sub_y0 = grid_y0 + grid_rows * gh;

        // The messaging font is appended as a WHITE mask block below all
        // the composited tiles (its glyphs are tinted at draw time).
        let font_y0 = sub_y0 + sub_rows * sh;
        let font_h = font.map_or(0, |(fi, _)| fi.atlas_height as usize);
        // The spider-web overlay bank rides below the font — plain
        // palette-resolved tiles like the base atlas (index 0
        // transparent), NOT white masks.
        let web_y0 = font_y0 + font_h;
        let web_h = web.map_or(0, |(wi, _)| wi.atlas_height as usize);
        let total_h = web_y0 + web_h;
        let mut rgba = vec![0u8; base_w * total_h * 4];

        // Base atlas = RAW palette colors, no blend. The original draws
        // the panel BACKGROUNDS (sub_23940) blended over the live
        // framebuffer (the bright sky) — NOT over black — and the icons/
        // glyphs (DrawBitmap_60CE0) raw with no blend at all. Compositing
        // the base atlas through `blend[src|0]` (over black) darkens
        // every panel sprite ~30% ([41] (109,109,117) → (81,73,69)); raw
        // palette keeps their true brightness. The luminous spell-icon
        // ramps that genuinely need the blend read over the stone slab in
        // the slot tiles below, not here.
        for (i, &src) in pixels.iter().enumerate() {
            if src == 0 {
                continue; // transparent
            }
            let c = palette[src as usize];
            rgba[i * 4..i * 4 + 3].copy_from_slice(&c[..3]);
            rgba[i * 4 + 3] = 255;
        }

        let mut slot_uv = [[[0.0f32; 4]; 3]; SPELL_COUNT];
        for spell in 0..tile_count / 3 {
            let icon = sprite_px(spell + 6);
            for (variant, hilite) in hilites.iter().enumerate() {
                let tile = spell * 3 + variant;
                let (tx, ty) = (
                    (tile % tiles_per_row) * tile_w,
                    base_h + (tile / tiles_per_row) * tile_h,
                );
                slot_uv[spell][variant] = [tx as f32, ty as f32, tile_w as f32, tile_h as f32];
                for y in 0..tile_h {
                    for x in 0..tile_w {
                        // Layered exactly like the original's blits:
                        // slab, then icon, then the equip highlight,
                        // each `blend[src | under<<8]`.
                        let mut v = match &slab {
                            Some((w, _, px)) => px[y * w + x],
                            None => 0,
                        };
                        if let Some((iw, ih, px)) = &icon {
                            let (ox, oy) = ((tile_w - iw) / 2, (tile_h - ih) / 2);
                            if x >= ox && x < ox + iw && y >= oy && y < oy + ih {
                                let s = px[(y - oy) * iw + (x - ox)];
                                if s != 0 {
                                    v = resolve(s, v);
                                }
                            }
                        }
                        if let Some((hw, hh, px)) = hilite {
                            // Top-aligned, clipped to the slab tile
                            // (the sprite runs a few rows taller —
                            // the HUD panel's bar area).
                            let ox = (tile_w.saturating_sub(*hw)) / 2;
                            if x >= ox && x < ox + hw && y < *hh {
                                let s = px[y * hw + (x - ox)];
                                if s != 0 {
                                    v = resolve(s, v);
                                }
                            }
                        }
                        if v == 0 {
                            continue;
                        }
                        let c = palette[v as usize];
                        let o = ((ty + y) * base_w + tx + x) * 4;
                        rgba[o..o + 3].copy_from_slice(&c[..3]);
                        rgba[o + 3] = 255;
                    }
                }
            }
        }

        // MC2 selector tiles: every grid/flyout state pre-composited
        // so the widget draws exactly one textured quad per box — the
        // ORIGINAL's pixel treatments happen here (raw copy for the
        // castable states, the blend LUT for the unaffordable ghost,
        // the 0xA6 colourize row for the unowned relief).
        let mut pane_uv = Vec::new();
        let mut sub_uv = Vec::new();
        if !book_tiles {
            let meter_box = sprite_px(MC2_SPR_TILE_BAR);
            let dark_box = sprite_px(MC2_SPR_BOX_DARK);
            let sub_dark_box = sprite_px(MC2_SPR_SUB_DARK);
            for spell in 0..n_mc2 {
                let icon = sprite_px(MC2_SPR_ICON_SMALL + spell);
                let variants: [(&Option<_>, PaneInk); 4] = [
                    (&pane_box, PaneInk::Raw),          // castable
                    (&meter_box, PaneInk::Raw),         // hovered (shot meter tile)
                    (&dark_box, PaneInk::Blend),        // owned, unaffordable
                    (&pane_box, PaneInk::Colour(0xA6)), // unowned relief
                ];
                let mut uvs = [[0.0f32; 4]; 4];
                for (variant, (bg, ink)) in variants.iter().enumerate() {
                    let t = spell * 4 + variant;
                    let (tx, ty) = ((t % grid_per_row) * gw, grid_y0 + (t / grid_per_row) * gh);
                    uvs[variant] = [tx as f32, ty as f32, gw as f32, gh as f32];
                    let mut tile = vec![0u8; gw * gh];
                    if let Some(b) = bg.as_ref() {
                        overlay8(&mut tile, (gw, gh), b, (0, 0), PaneInk::Raw, &resolve);
                    }
                    if let Some(ic) = &icon {
                        // At the BOX ORIGIN, not centred — retail's
                        // `DrawBitmap(posX + posIconsX, posIconsY,
                        // icon)` (EF:22543); the 41×23 art carries its
                        // own margins (centring reads as misaligned vs
                        // retail).
                        overlay8(&mut tile, (gw, gh), ic, (0, 0), *ink, &resolve);
                    }
                    emit8(&mut rgba, base_w, palette, (tx, ty), (gw, gh), &tile);
                }
                pane_uv.push(uvs);

                // Flyout tiles: number badge at (+6,+10), per-level
                // icon [179 + 3·spell + level] at (+18,+6) — the
                // original's fixed insets (trace §4.2).
                for level in 0..3usize {
                    let sub_icon = sprite_px(MC2_SPR_SUB_ICON + 3 * spell + level);
                    let badge = sprite_px(MC2_SPR_SUB_NUM + level);
                    let mut uvs = [[0.0f32; 4]; 3];
                    for (variant, (bg, ink)) in [
                        (&sub_box, PaneInk::Raw),
                        (&sub_dark_box, PaneInk::Blend),
                        // Dark frame, LIT icon: pool ok but the hand
                        // can't pay one cast (EF:22618 vs :22625-28).
                        (&sub_dark_box, PaneInk::Raw),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let t = (spell * 3 + level) * 3 + variant;
                        let (tx, ty) = ((t % sub_per_row) * sw, sub_y0 + (t / sub_per_row) * sh);
                        uvs[variant] = [tx as f32, ty as f32, sw as f32, sh as f32];
                        let mut tile = vec![0u8; sw * sh];
                        if let Some(b) = bg.as_ref() {
                            overlay8(&mut tile, (sw, sh), b, (0, 0), PaneInk::Raw, &resolve);
                        }
                        if let Some(bd) = &badge {
                            overlay8(&mut tile, (sw, sh), bd, (6, 10), PaneInk::Raw, &resolve);
                        }
                        if let Some(ic) = &sub_icon {
                            overlay8(&mut tile, (sw, sh), ic, (18, 6), *ink, &resolve);
                        }
                        emit8(&mut rgba, base_w, palette, (tx, ty), (sw, sh), &tile);
                    }
                    sub_uv.push(uvs);
                }
            }
        }

        // Messaging font: copy its 1-bit glyph atlas as a WHITE mask
        // block at y=font_y0, and record each glyph's uv (indexed by
        // sprite id = ASCII char + 1). Ink is baked white so the draw
        // path can tint any colour, matching DrawText's `color` arg.
        let mut glyph_uv = Vec::new();
        let mut line_height = 0.0f32;
        if let Some((fi, fpx)) = font {
            let fw = (fi.atlas_width as usize).min(base_w);
            for y in 0..fi.atlas_height as usize {
                for x in 0..fw {
                    if fpx[y * fi.atlas_width as usize + x] != 0 {
                        let o = ((font_y0 + y) * base_w + x) * 4;
                        rgba[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
            glyph_uv = fi
                .sprites
                .iter()
                .map(|e| {
                    e.frames
                        .first()
                        .filter(|_| e.width > 0 && e.height > 0)
                        .map(|f| {
                            [
                                f.x as f32,
                                (font_y0 as u32 + f.y as u32) as f32,
                                e.width as f32,
                                e.height as f32,
                            ]
                        })
                })
                .collect();
            line_height = fi.sprites.iter().map(|e| e.height).max().unwrap_or(0) as f32;
        }

        // The web overlay tiles: palette-resolved copy below the font
        // block, uv per HSPR sprite id (1..=24 = the viewport grid).
        let mut web_uv = Vec::new();
        if let Some((wi, wpx)) = web {
            let ww = (wi.atlas_width as usize).min(base_w);
            for y in 0..wi.atlas_height as usize {
                for x in 0..ww {
                    let src = wpx[y * wi.atlas_width as usize + x];
                    if src == 0 {
                        continue; // transparent
                    }
                    let c = palette[src as usize];
                    let o = ((web_y0 + y) * base_w + x) * 4;
                    rgba[o..o + 3].copy_from_slice(&c[..3]);
                    rgba[o + 3] = 255;
                }
            }
            web_uv = wi
                .sprites
                .iter()
                .map(|e| {
                    e.frames
                        .first()
                        .filter(|_| e.width > 0 && e.height > 0)
                        .map(|f| {
                            [
                                f.x as f32,
                                (web_y0 as u32 + f.y as u32) as f32,
                                e.width as f32,
                                e.height as f32,
                            ]
                        })
                })
                .collect();
        }

        // Frame rects per sprite id in the base atlas region (the
        // map's icon markers crop from here).
        let sprite_rects = index
            .sprites
            .iter()
            .map(|e| {
                e.frames
                    .first()
                    .map(|f| (f.x as u32, f.y as u32, e.width as u32, e.height as u32))
            })
            .collect();

        Self {
            atlas_w: index.atlas_width,
            atlas_h: total_h as u32,
            atlas_rgba: rgba,
            slot_uv,
            sprite_rects,
            pointer_base: index.pointer_base as usize,
            pane_uv,
            sub_uv,
            glyph_uv,
            line_height,
            web_uv,
        }
    }

    /// Whether the bundle carried the spider-web overlay bank.
    pub fn has_web(&self) -> bool {
        !self.web_uv.is_empty()
    }

    /// The fullscreen SPIDER-WEB overlay (remc2 EF:21668-710): tile
    /// the 6×4 grid of bank sprites 1..=24 across the viewport —
    /// retail walks `x += tile.width` per tile and `y += row height`
    /// per row over the 640×480 view, hard on/off with no fade;
    /// drawn while the paralyze web (`mobilizeCounter`) is live. The
    /// grid is authored for 640×480, so placement scales by
    /// (w/640, h/480) to cover any window.
    pub fn web_quads(&self, w: f32, h: f32) -> Vec<UiQuad> {
        // Uniform: the equipped-panel frame is art, and the cells it
        // sits in are already uniform (see `book_cell`).
        let (sx, sy) = {
            let s = HudFrame::new(w, h).s;
            (s, s)
        };
        let mut quads = Vec::with_capacity(24);
        let mut id = 1usize; // actPlayerIndex starts at 1 (EF:21680)
        let mut y = 0.0f32;
        for _row in 0..4 {
            let mut x = 0.0f32;
            let mut row_h = 0.0f32;
            for _col in 0..6 {
                let Some(uv) = self.web_uv.get(id).copied().flatten() else {
                    return quads; // short bank — draw what exists
                };
                quads.push(UiQuad {
                    rect: snap([x * sx, y * sy, uv[2] * sx, uv[3] * sy]),
                    uv,
                    tint: WHITE,
                });
                x += uv[2];
                row_h = uv[3];
                id += 1;
            }
            y += row_h;
        }
        quads
    }

    /// The messaging-font line height in source pixels (tallest glyph
    /// cell) — the caller's vertical metric for stacking lines/banners
    /// (the subtitle block; `text_quads` already advances newlines
    /// internally).
    pub fn font_line_height(&self) -> f32 {
        self.line_height
    }

    /// Whether the bundle carried a messaging font.
    pub fn has_font(&self) -> bool {
        !self.glyph_uv.is_empty()
    }

    /// Advance width of one byte in the messaging font (source pixels):
    /// the glyph's own width plus tracking; the original walks
    /// `x += GetLetterWidth` (proportional).
    fn glyph_advance(&self, b: u8) -> f32 {
        match self.glyph_uv.get(b as usize + 1).copied().flatten() {
            Some(uv) => uv[2] + GLYPH_SPACING,
            None => GLYPH_FALLBACK_ADVANCE + GLYPH_SPACING,
        }
    }

    /// Total advance width of `s` in the messaging font at scale 1
    /// (source pixels), for centering/right-alignment/clipping. Control
    /// bytes (tab/newline) count as their nominal advance; callers pass
    /// single lines (the subtitle wrap/centering; the plain toast is
    /// left-aligned).
    pub fn text_width(&self, s: &str) -> f32 {
        s.chars()
            .map(|c| self.glyph_advance(if c.is_ascii() { c as u8 } else { b'?' }))
            .sum()
    }

    /// Build quads for `s` in the messaging font, top-left at screen
    /// pixel (x, y), tinted `color`, each source pixel scaled by `scale`.
    /// Walks the bytes blitting glyph id = byte+1 and advancing by the
    /// glyph width — the original's `DrawText_2BC10` / `sub_6F940`.
    /// `\n` = newline (advance y by the line height, x back to the
    /// start); `\t` = one space-glyph width of horizontal space.
    pub fn text_quads(&self, s: &str, x: f32, y: f32, color: [f32; 4], scale: f32) -> Vec<UiQuad> {
        let mut quads = Vec::with_capacity(s.len());
        let (mut cx, mut cy) = (x, y);
        for c in s.chars() {
            match c {
                '\n' => {
                    cy += self.line_height * scale;
                    cx = x;
                    continue;
                }
                '\t' => {
                    // Retail advances tab (and space) by the space glyph
                    // width (`GetLetterWidth_6FC10` = glyph[33].width).
                    cx += self.glyph_advance(b' ') * scale;
                    continue;
                }
                _ => {}
            }
            // FONT1 is a byte-indexed ASCII bank — a multi-byte char
            // must never walk it per BYTE (an em-dash rendered as
            // THREE garbage glyphs, player-reported on the replay
            // HUD). Non-ASCII falls back to '?'.
            let b = if c.is_ascii() { c as u8 } else { b'?' };
            if let Some(uv) = self.glyph_uv.get(b as usize + 1).copied().flatten() {
                let (gw, gh) = (uv[2], uv[3]);
                if gh > 0.0 && gw > 0.0 {
                    quads.push(UiQuad {
                        rect: snap([cx, cy, gw * scale, gh * scale]),
                        uv,
                        tint: color,
                    });
                }
                cx += (gw + GLYPH_SPACING) * scale;
            } else {
                cx += (GLYPH_FALLBACK_ADVANCE + GLYPH_SPACING) * scale;
            }
        }
        quads
    }

    /// The retail in-game mouse pointer as a cursor quad, tip at
    /// (x, y) — the arrow + mana ball both games attach to their
    /// pointer (MC1 golden, MC2 grey). `entry` is the POINTERS-bank
    /// index: MC1 always 1 (sub_5C05C installs :42024/:49068); MC2
    /// per map type — day 1, night 9, cave 10 (SetCursor_8CD27 /
    /// LevelInit.cpp:24-38). None on an older bake (`pointer_base`
    /// 0) — the software arrow stands in.
    pub fn pointer_quad(&self, x: f32, y: f32, scale: f32, entry: usize) -> Option<UiQuad> {
        if self.pointer_base == 0 {
            return None;
        }
        self.sprite_quad(self.pointer_base + entry, x, y, scale)
    }

    /// A pre-composited MC2 selector grid tile (see `pane_uv`); None
    /// on MC1 atlases.
    fn pane_tile(&self, spell: usize, variant: usize) -> Option<[f32; 4]> {
        self.pane_uv.get(spell).map(|v| v[variant])
    }

    /// A pre-composited MC2 flyout tile (see `sub_uv`); None on MC1
    /// atlases. `variant`: 0 lit, 1 pool-fail (ghost icon), 2 broke
    /// (dark frame, lit icon).
    fn sub_tile(&self, spell: usize, level: usize, variant: usize) -> Option<[f32; 4]> {
        self.sub_uv.get(spell * 3 + level).map(|v| v[variant])
    }

    /// The UI-atlas UV rect (texels) for one HSPR sprite, for map
    /// stamping (castle 58+team, balloon 66+team — remc1 sub_48710
    /// :57230/:57234). The renderer projects the world position and
    /// blits it upright over the rotated map; position is filled by the
    /// caller per entity.
    pub fn map_stamp(&self, id: usize) -> Option<mgc_render::MapStamp> {
        let (x, y, w, h) = self.sprite_rects.get(id).copied().flatten()?;
        if w == 0 || h == 0 {
            return None;
        }
        // Per-range anchor (remc1 sub_48710 :57344-64): castle sprites
        // 58-65 pin at bottom-LEFT (the flagpole foot in the lower-left
        // of the rectangular flag icon); balloon sprites 66-73 pin at
        // bottom-CENTER (the balloon base). Others default center-bottom.
        let anchor = match id {
            58..=65 => [0.0, 1.0], // castle: bottom-left
            66..=73 => [0.5, 1.0], // balloon: bottom-center
            _ => [0.5, 1.0],
        };
        Some(mgc_render::MapStamp {
            x: 0.0,
            z: 0.0,
            w,
            h,
            uv: [x as f32, y as f32, w as f32, h as f32],
            anchor,
        })
    }

    /// Crop a WORLD sprite (frame 0) into fresh rows appended below
    /// the composited atlas and hand back its map stamp — the marker
    /// icon-swap's miniatures (jars, dolmens, statues). Plain palette
    /// resolve with index 0 transparent, like the base atlas; the
    /// stamp is shrunk to marker size — HALF the spell-stamp rule
    /// (player-sized 2026-08-07: at the shared 12-px cap the
    /// miniatures read double the other map marks), i.e. longer side
    /// capped at 6 native px AND small sprites halved too. Draw size
    /// only — the crop stays full-res. Must run before the atlas
    /// uploads, i.e. at level load.
    pub fn append_world_icon(
        &mut self,
        index: &SpriteIndex,
        pixels: &[u8],
        sprite: u16,
        palette: &[[u8; 4]; 256],
    ) -> Option<mgc_render::MapStamp> {
        let e = index.sprites.get(sprite as usize)?;
        let f = e.frames.first()?;
        let (w, h) = (e.width as usize, e.height as usize);
        let src_w = index.atlas_width as usize;
        let dst_w = self.atlas_w as usize;
        if w == 0 || h == 0 || w > dst_w {
            return None;
        }
        // The whole source rect must be in bounds BEFORE the atlas
        // grows, so a broken entry can't leave half-appended rows.
        if f.x as usize + w > src_w || (f.y as usize + h) * src_w > pixels.len() {
            return None;
        }
        let y0 = self.atlas_h as usize;
        self.atlas_rgba.resize((y0 + h) * dst_w * 4, 0);
        for y in 0..h {
            let srow = (f.y as usize + y) * src_w + f.x as usize;
            for x in 0..w {
                let src = pixels[srow + x];
                if src == 0 {
                    continue; // transparent
                }
                let c = palette[src as usize];
                let i = ((y0 + y) * dst_w + x) * 4;
                self.atlas_rgba[i..i + 3].copy_from_slice(&c[..3]);
                self.atlas_rgba[i + 3] = 255;
            }
        }
        self.atlas_h += h as u32;
        let (mut sw, mut sh) = (e.width as u32, e.height as u32);
        // Exactly 2x smaller than the spell-stamp rule would draw:
        // min(native, 12)/2 per axis via the shared long-side factor.
        let scale = (6.0 / sw.max(sh) as f32).min(0.5);
        sw = ((sw as f32 * scale) as u32).max(1);
        sh = ((sh as f32 * scale) as u32).max(1);
        Some(mgc_render::MapStamp {
            x: 0.0,
            z: 0.0,
            w: sw,
            h: sh,
            uv: [0.0, y0 as f32, e.width as f32, e.height as f32],
            anchor: [0.5, 1.0],
        })
    }

    /// The pre-composited icon-on-slab tile for a spell; `variant`
    /// 0 = plain, 1 = left-equipped, 2 = right-equipped highlight. Kept
    /// for the composited luminous-ramp look (the icon blended over the
    /// slab); the book draws slab + native-uniform icon separately to
    /// avoid the non-4:3 stretch. The equipped-hand variants are a parked
    /// binding indicator.
    #[allow(dead_code)]
    fn slot_quad(&self, spell: SpellId, variant: usize, rect: [f32; 4], tint: [f32; 4]) -> UiQuad {
        UiQuad {
            rect,
            uv: self.slot_uv[spell.0 as usize][variant],
            tint,
        }
    }

    /// Pixel dimensions of one `begSprTab[id]` UI sprite, or None if the
    /// sprite is empty/absent.
    pub fn sprite_dims(&self, id: usize) -> Option<(f32, f32)> {
        let (_, _, w, h) = self.sprite_rects.get(id).copied().flatten()?;
        (w != 0 && h != 0).then_some((w as f32, h as f32))
    }

    /// The top-of-screen notification anchor in 640-native HUD coords:
    /// the LEFT edge of the wizard info-boxes (just right of the radar
    /// cap [40]) and just BELOW the panel strip [41] — where the toast
    /// belongs relative to OUR 640-native HSPR HUD. Retail's 320-native
    /// `132,50` literal was authored against the half-size MSPR strip and
    /// doesn't map onto the bigger HSPR panels, so we anchor to the live
    /// sprite geometry instead (deliberate), keeping it below the castle/
    /// balloon boxes at any resolution. Left-aligned from this x.
    pub fn hud_notification_anchor(&self) -> (f32, f32) {
        let radar_w = self.sprite_dims(SPR_PANEL_BG).map_or(124.0, |(w, _)| w);
        let panel_h = self
            .sprite_dims(SPR_WIZ_BG)
            .or_else(|| self.sprite_dims(SPR_PANEL_BG))
            .map_or(45.0, |(_, h)| h);
        // x: 2px inset + radar width = the sub-panel origin (v22 in
        // hud_quads), plus a small gap so the first glyph clears the
        // radar's right edge instead of kissing it (retail leaves a
        // little air there). y: 2px top inset + panel height + a 2px gap
        // below the info-boxes.
        (2.0 + radar_w + 6.0, 2.0 + panel_h + 2.0)
    }

    /// Blit `begSprTab[id]` at screen pixel (x, y), opaque. For the
    /// icons/glyphs the original draws raw (DrawBitmap, no blend).
    fn sprite_quad(&self, id: usize, x: f32, y: f32, scale: f32) -> Option<UiQuad> {
        self.sprite_quad_tint(id, x, y, scale, WHITE)
    }

    /// Blit `begSprTab[id]` with an explicit tint (for the translucent
    /// panel BACKGROUNDS — the original's sub_23940 blends them over the
    /// live framebuffer, so HUD transparency is always on; we approximate
    /// with an alpha over the sky, which the UI pass already blends).
    fn sprite_quad_tint(
        &self,
        id: usize,
        x: f32,
        y: f32,
        scale: f32,
        tint: [f32; 4],
    ) -> Option<UiQuad> {
        let (sx, sy, w, h) = self.sprite_rects.get(id).copied().flatten()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some(UiQuad {
            rect: snap([x, y, w as f32 * scale, h as f32 * scale]),
            uv: [sx as f32, sy as f32, w as f32, h as f32],
            tint,
        })
    }

    /// Blit `begSprTab[id]` into an explicit destination rect (for the
    /// spellbook: the slab stretches to the cell, the icon draws at a
    /// uniform-scaled centered rect so it never distorts).
    fn sprite_quad_rect_tint(&self, id: usize, rect: [f32; 4], tint: [f32; 4]) -> Option<UiQuad> {
        let (sx, sy, w, h) = self.sprite_rects.get(id).copied().flatten()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some(UiQuad {
            rect: snap(rect),
            uv: [sx as f32, sy as f32, w as f32, h as f32],
            tint,
        })
    }

    /// Like [`Self::sprite_quad_rect_tint`] but MASK-DARKEN: the sprite is
    /// a coverage mask, and the shader fills it with the (translucent)
    /// tint so the destination beneath (the slab) shows through DARKENED —
    /// the dark-relief look of UNOWNED spellbook icons cut into the stone
    /// texture (the original's sub_23AE0 blend[0xA6 | dest]). A NEGATIVE
    /// uv width is the mode flag.
    fn sprite_quad_rect_mask(&self, id: usize, rect: [f32; 4], tint: [f32; 4]) -> Option<UiQuad> {
        let (sx, sy, w, h) = self.sprite_rects.get(id).copied().flatten()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some(UiQuad {
            rect: snap(rect),
            uv: [sx as f32, sy as f32, -(w as f32), h as f32],
            tint,
        })
    }
}

/// Push an optional quad (from the sprite-blit helpers) if present.
fn push_opt(quads: &mut Vec<UiQuad>, q: Option<UiQuad>) {
    if let Some(u) = q {
        quads.push(u);
    }
}

/// Snap a rect to the integer pixel grid, EDGE-consistently: left/top
/// and right/bottom round independently, so adjacent cells that share
/// an edge in native coordinates still share it after snapping (no
/// gaps, no overlaps). Without this, fractional scale factors (e.g.
/// 1.5 at 720p) rasterize identical native sources into visibly
/// different rows/columns. BANKED: the remaining in-sprite aliasing at
/// fractional scales wants a native 640×480 UI layer upscaled once with
/// a real filter.
fn snap(rect: [f32; 4]) -> [f32; 4] {
    let x0 = rect[0].round();
    let y0 = rect[1].round();
    let x1 = (rect[0] + rect[2]).round();
    let y1 = (rect[1] + rect[3]).round();
    [x0, y0, x1 - x0, y1 - y0]
}

/// Fit a fixed-size frontend screen into the window: the largest
/// integer-free scale that fits, CENTRED, with the remainder as
/// letterbox bars.
///
/// Every full-screen frontend surface (both main menus, the world map,
/// the FMV player) is authored at a fixed 4:3 resolution and blown up
/// to the window. Anchoring that at the top-left leaves the whole
/// screen shoved into a corner on any window that is not 4:3 — which
/// fullscreen usually is not.
///
/// Returns `(scale, offset_x, offset_y)`. Callers scale their authored
/// coordinates by `scale` and translate by the offset; input has to
/// make the same trip in reverse ([`unletterbox`]).
pub(crate) fn letterbox(size: (f32, f32), w: f32, h: f32) -> (f32, f32, f32) {
    let scale = (size.0 / w).min(size.1 / h);
    (
        scale,
        ((size.0 - w * scale) * 0.5).floor(),
        ((size.1 - h * scale) * 0.5).floor(),
    )
}

/// Window cursor → authored screen coordinates, the inverse of
/// [`letterbox`]. Hit tests are written against the authored layout,
/// so they must not see window pixels.
pub(crate) fn unletterbox(cursor: (f32, f32), size: (f32, f32), w: f32, h: f32) -> (f32, f32) {
    let (scale, ox, oy) = letterbox(size, w, h);
    ((cursor.0 - ox) / scale, (cursor.1 - oy) / scale)
}

/// Crop quads to a `w`x`h` viewport anchored at the origin, dropping
/// what falls entirely outside and proportionally cropping the rest
/// (rect AND uv, so partial sprites are cut rather than squashed).
///
/// A letterboxed screen needs this where an un-letterboxed one does
/// not: content that hangs off the viewport — a map portal half
/// scrolled past the edge — used to be clipped for free by the window
/// boundary, because the picture reached it. Once the picture is
/// centred, that same overhang lands in the visible bars instead.
pub(crate) fn clip_quads(quads: &mut Vec<UiQuad>, w: f32, h: f32) {
    quads.retain_mut(|q| {
        let (x, y, qw, qh) = (q.rect[0], q.rect[1], q.rect[2], q.rect[3]);
        if qw <= 0.0 || qh <= 0.0 || x >= w || y >= h || x + qw <= 0.0 || y + qh <= 0.0 {
            return false;
        }
        // Texels per rect pixel, so the uv window follows the crop.
        let (ux, uy) = (q.uv[2] / qw, q.uv[3] / qh);
        let (x0, y0) = (x.max(0.0), y.max(0.0));
        let (x1, y1) = ((x + qw).min(w), (y + qh).min(h));
        q.uv[0] += (x0 - x) * ux;
        q.uv[1] += (y0 - y) * uy;
        q.uv[2] = (x1 - x0) * ux;
        q.uv[3] = (y1 - y0) * uy;
        q.rect = [x0, y0, x1 - x0, y1 - y0];
        true
    });
}

/// Translate every quad into the letterboxed screen.
pub(crate) fn offset_quads(quads: &mut [UiQuad], ox: f32, oy: f32) {
    for q in quads {
        q.rect[0] += ox;
        q.rect[1] += oy;
    }
}

pub(crate) fn solid(rect: [f32; 4], tint: [f32; 4]) -> UiQuad {
    UiQuad {
        rect: snap(rect),
        uv: [0.0; 4],
        tint,
    }
}

/// A solid-color quad that draws screen-space even in VR.  Use this for
/// fullscreen flashes, fades, and backdrops that must cover the whole eye
/// rather than the world-space HUD panel.
pub(crate) fn solid_screen(rect: [f32; 4], tint: [f32; 4]) -> UiQuad {
    UiQuad {
        rect: snap(rect),
        // Negative uv.w signals the UI shader to bypass the VR panel
        // transform and draw the quad directly in screen-space NDC.
        // The screen-space pipeline reads uv.w only for textured quads
        // (uv.z != 0), so the marker is inert outside a panel mode.
        uv: [0.0, 0.0, 0.0, -1.0],
        tint,
    }
}

/// The rival wizard tag — retail MC2's boxed name + health bar
/// (`DrawSorcererNameAndHealthBar_2CB30`, remc2 GameRenderHD.cpp:
/// 2797-2879), drawn over every visible rival wizard sprite. Layout is
/// retail's, in 640-native units × `s`, anchored like retail: the box's
/// LEFT edge at the sprite's horizontal center (`(a4>>1) + a2`, :2833),
/// its top 20 px above the sprite top (:2841). Box = name width + 6
/// wide × 18 tall over a background fill, with a 2 px bevel (top/left
/// light, bottom/right dark, :2861-65), the name 4 px in (:2866), and
/// the 2 px health row at y+14: backdrop then the filled width
/// `floor(life · (w−2) / max)` in the team color (:2867-74). The name
/// and the fill share ONE color — the rival's team color. Retail
/// truncates the name (and box) against the viewport's right edge − 4
/// (:2848-55); reproduced by dropping tail bytes until the box fits.
///
/// Deviations, both forced by the font: retail reserves a monospace
/// 8 px/char cell where FONT1 is proportional (the box hugs the real
/// text width), and the ink sits at y+3 scaled to the 11 px interior
/// band instead of retail's y+0 (retail's glyph cells carry the
/// leading that lands them inside the bevel; FONT1's masks are tight).
#[allow(clippy::too_many_arguments)]
pub fn rival_tag_quads(
    quads: &mut Vec<UiQuad>,
    assets: &UiAssets,
    name: &str,
    life_frac: f32,
    team: [f32; 4],
    chrome: &crate::entities::TagChrome,
    sx: f32,
    sy: f32,
    s: f32,
    right_edge: f32,
) {
    // FONT1 is 320-native; the retail tag is authored in the 640
    // frame, so glyphs run at 2× — the toast law (`lib.rs` toast
    // block) — capped so the ink stays inside the 11 px band between
    // the top bevel and the bar row.
    let font_s = (11.0 / assets.font_line_height()).min(2.0);
    let mut name = name;
    let (mut inner, mut tw);
    loop {
        tw = assets.text_width(name) * font_s;
        inner = tw + 4.0;
        if name.is_empty() || sx + (inner + 2.0) * s <= right_edge - 4.0 * s {
            break;
        }
        name = &name[..name.len() - 1];
    }
    if name.is_empty() {
        return;
    }
    let r = |x: f32, y: f32, w: f32, h: f32| [sx + x * s, sy + y * s, w * s, h * s];
    // Background, then the bevel frame (top/left light, bottom/right
    // dark), retail's paint order.
    quads.push(solid(r(0.0, 0.0, inner + 2.0, 18.0), chrome.bg));
    quads.push(solid(r(0.0, 0.0, inner + 2.0, 2.0), chrome.bevel_tl));
    quads.push(solid(r(0.0, 16.0, inner + 2.0, 2.0), chrome.bevel_br));
    quads.push(solid(r(0.0, 0.0, 2.0, 16.0), chrome.bevel_tl));
    quads.push(solid(r(inner, 0.0, 2.0, 18.0), chrome.bevel_br));
    quads.extend(assets.text_quads(name, sx + 4.0 * s, sy + 3.0 * s, team, font_s * s));
    quads.push(solid(r(2.0, 14.0, inner - 2.0, 2.0), chrome.bar_empty));
    // Retail's integer fill width, floored in the 640 frame before
    // scaling (:2870).
    let fill = (life_frac.clamp(0.0, 1.0) * (inner - 2.0)).floor();
    if fill > 0.0 {
        quads.push(solid(r(2.0, 14.0, fill, 2.0), team));
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Unowned spell icons: the icon's outer SHAPE used as a mask, filled
/// with a dark TRANSLUCENT ink so the stone-slab texture shows through,
/// DARKENED — a dark relief cut into the tile. The original's sub_23AE0
/// writes blend[0xA6 | dest]; rgb = the dark ink, a = darkening
/// strength.
const UNOWNED_MASK: [f32; 4] = [0.05, 0.04, 0.03, 0.74];
/// The book slab tint. Our raw [3] sprite is a cool blue-grey
/// (~158,165,198); retail's slab reads a WARM DARK BROWN (the original
/// blends [3] through the LUT over the book background, warming +
/// darkening it). This tint warms toward brown (boosts red-relative,
/// cuts blue) AND darkens to match.
const SLAB_DIM: [f32; 4] = [0.58, 0.46, 0.32, 1.0];
/// Quick-select digit ink: the original blends the glyph toward
/// `byte_AD167_AD157[1]` (black); a black multiplicative tint blackens
/// the sprite's yellow ink while keeping its coverage/alpha.
const DIGIT_INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

// The spellbook grid (remc1 :26915-70), native 640×480 scaled by w/640,
// h/480. 24 spells iterate in DISPLAY_ORDER packed with NO gaps: cell =
// the slot-slab sprite [3] = 64×37, 4 cols × 6 rows from (384,194). The
// origin lives in mgc-render (which places the world viewport against
// the same edges) — consumed here so the two crates cannot drift; the
// grid bottom (194 + 6·37 = 416) is the map-pane base and the black-bar
// top.
const BOOK_GRID_X: f32 = mgc_render::BOOK_SPELL_X;
const BOOK_GRID_Y: f32 = mgc_render::BOOK_SPELL_Y;
const BOOK_CELL_W: f32 = 64.0;
const BOOK_CELL_H: f32 = 37.0;
const BOOK_GRID_COLS: usize = 4;
/// Quick-select digit glyphs: `[30 + slot]` (slot 0 = "1" … slot 9 =
/// "0"), 10×14 badges — the number retail stamps in a hotkeyed spell's
/// book cell (sub_24230 :27857).
const SPR_QUICK_DIGIT: usize = 30;
/// The slot slab's ACTIVE variant — retail swaps [3]→[4] while the
/// spell's burst counter runs (sub_24230 :27810).
const SPR_SLOT_BG_ACTIVE: usize = 4;

/// One grid cell's slab rect in screen pixels (the icon is pre-composited
/// onto the 64×37 slot slab). `k` = display index 0..24.
/// The grid is the RIGID pane of the map screen (see the map-screen
/// layout law in `mgc_render::Renderer::render`): the cells are art, so
/// they keep the uniform scale and never stretch. The block is anchored
/// into the screen's bottom-RIGHT corner — right-anchored so its last
/// column still ends flush at x=w, bottom-anchored so the black log
/// strip below keeps its 64 native px. The renderer derives the world
/// viewport and the map pane from the same two anchors, so the three
/// panes cannot drift apart.
fn book_cell(w: f32, h: f32, k: usize) -> [f32; 4] {
    let f = HudFrame::new(w, h);
    let col = (k % BOOK_GRID_COLS) as f32;
    let row = (k / BOOK_GRID_COLS) as f32;
    [
        f.rx(BOOK_GRID_X + col * BOOK_CELL_W),
        f.by(BOOK_GRID_Y + row * BOOK_CELL_H),
        f.len(BOOK_CELL_W),
        f.len(BOOK_CELL_H),
    ]
}

/// Book screen quads + the display slot under the cursor (if any).
pub fn book_quads(
    assets: &UiAssets,
    loadout: &LoadoutView,
    quick_binds: &[Option<u8>; 10],
    alert_blink: bool,
    w: f32,
    h: f32,
    cursor: (f32, f32),
) -> (Vec<UiQuad>, Option<SpellId>) {
    let mut quads = Vec::with_capacity(SPELL_COUNT * 3 + 2);
    let mut hovered = None;
    for (k, &spell) in DISPLAY_ORDER.iter().enumerate() {
        let spell_id = SpellId(spell);
        let cell = book_cell(w, h, k);
        // Two spellbook states, per the actual draw split (remc1
        // :26932/:26972):
        //   OWNED   → sub_24230: slab + the icon drawn in FULL color
        //             (DrawBitmap, raw). Affordability is shown by the
        //             separate diagonal-line marks (sub_247C0), NOT by
        //             dimming the icon.
        //   NOT owned → sub_23CF0: slab + the icon as a coverage mask
        //             DIM-TINTED toward color 0xA6 (sub_23AE0) — the full
        //             icon SHAPE stays visible, just darkened.
        // The slab itself is drawn via sub_23940 = a BLEND over the book's
        // black background, so it reads DARKER than the raw sprite. We
        // approximate the blend-over-black with a darkening tint.
        let owned = loadout.owned[spell as usize];
        // The :26926 BIND gate: the spell's castle_req (+132, the
        // castle-stored unlock ladder) vs the castle's stored mana —
        // NOT a player-mana affordability test. Computed sim-side.
        let bindable = owned && loadout.bindable[spell as usize];
        let over = cursor.0 >= cell[0]
            && cursor.0 < cell[0] + cell[2]
            && cursor.1 >= cell[1]
            && cursor.1 < cell[1] + cell[3];
        // Every OWNED hovered spell becomes the bind target — the
        // castle-req gate does NOT block assignment (quickselect keys
        // can be bound to not-yet-castable spells; the equip command
        // :48717-31 checks ownership only). The :26926 castle gate stays
        // purely visual (the LOCKED wash + the equipped-panel wash) and
        // the CAST keeps fizzling sim-side until the castle stores enough.
        if over && owned {
            hovered = Some(spell_id);
        }
        // THE EXPIRY BLINK (sub_24230 :27807 — the same gate as the
        // hand panel's :27670): the owned cell's whole draw — slab,
        // icon, badge — SKIPS on odd turns while the spell's
        // countdown runs its last window. Hover/bind stays live
        // above: retail never keys input to the draw phase.
        if owned && loadout.expiring[spell as usize] && !alert_blink {
            continue;
        }

        // The stone slab fills the cell, drawn DARKER (the original's
        // sub_23940 blends it over the black book background) —
        // stretching the slab texture is invisible. While the spell's
        // burst counter runs, retail swaps the slab to the ACTIVE
        // variant [4] (sub_24230 :27810); there is no cooldown veil.
        let slab = if owned && loadout.cooldown[spell as usize] > 0.0 {
            SPR_SLOT_BG_ACTIVE
        } else {
            SPR_SLOT_BG as usize
        };
        push_opt(
            &mut quads,
            assets
                .sprite_quad_rect_tint(slab, cell, SLAB_DIM)
                .or_else(|| assets.sprite_quad_rect_tint(SPR_SLOT_BG as usize, cell, SLAB_DIM)),
        );
        // The ICON at native 62×34 × the UNIFORM art scale, anchored at
        // the CELL ORIGIN — retail's sub_24230 draws it with
        // `DrawBitmap(a1, a2, icon)`: top-left at the cell corner, so
        // the 62×34 art leaves 2px right + 3px bottom slack (NOT centred/
        // fit-to-cell — that sits it too low, e.g. the castle icon
        // touching the cell bottom). Uniform scale =
        // min(w/640, h/480) so the art never stretches at non-4:3.
        // OWNED = full colour (raw). NOT owned = the icon's SHAPE cut
        // into the slab as a dark relief (the original's
        // blend[0xA6|dest], sub_23AE0).
        let su = HudFrame::new(w, h).s;
        let icon_id = SPR_SPELL_ICON + spell as usize;
        if let Some((iw, ih)) = assets.sprite_dims(icon_id) {
            let irect = [cell[0], cell[1], iw * su, ih * su];
            push_opt(
                &mut quads,
                if owned {
                    assets.sprite_quad_rect_tint(icon_id, irect, WHITE)
                } else {
                    assets.sprite_quad_rect_mask(icon_id, irect, UNOWNED_MASK)
                },
            );
        }
        if owned {
            // Quick-select number badge (sub_24230 :27857): a spell
            // bound to a number key shows its digit glyph [30+slot]
            // at the CELL ORIGIN (retail `sub_23AE0(a1, a2, ...)` —
            // the glyph's own margins do the placement), blended
            // toward `byte_AD167[1]` = BLACK ink (a coverage-mask
            // blend). Retail gates this on a per-spell countdown (+844,
            // decremented per draw — the badge FLASHES after assignment)
            // or a book-wide flag (+14421); we keep it always-on in the
            // book (deliberate: readable interpretation).
            if let Some(slot) = quick_binds.iter().position(|&b| b == Some(spell)) {
                push_opt(
                    &mut quads,
                    assets.sprite_quad_tint(
                        SPR_QUICK_DIGIT + slot,
                        cell[0],
                        cell[1],
                        su,
                        DIGIT_INK,
                    ),
                );
            }
            // LOCKED overlay (sub_24230 :27860): when castle_req
            // exceeds the castle's stored mana (or no castle stands),
            // retail remaps the WHOLE cell through fog row 0x30
            // (sub_247C0) — a uniform wash over slab + icon + badge.
            // This is the visual for the unlock ladder ("owned but
            // can't select" is faithful; the wash tells you why).
            if !bindable {
                quads.push(solid(cell, LOCKED_WASH));
            }
        }
        if over && !owned {
            // Hover ring (sub_24DA0/sub_24D20, ink byte_AE167): retail
            // rings EVERY hovered cell — unowned included — only the
            // bind-candidate recording is gated (on ownership). A
            // hovered OWNED cell gets the panel redraw below instead
            // of a ring. (Ring colour = a text-table ink; hand-tuned
            // until the LUT bake.)
            let f = cell;
            let t = [0.9, 0.85, 0.5, 0.9];
            quads.push(solid([f[0], f[1], f[2], 2.0], t));
            quads.push(solid([f[0], f[1] + f[3] - 2.0, f[2], 2.0], t));
            quads.push(solid([f[0], f[1], 2.0, f[3]], t));
            quads.push(solid([f[0] + f[2] - 2.0, f[1], 2.0, f[3]], t));
        }
    }
    // The hovered OWNED cell is redrawn as a full equipped-spell
    // panel at the cell origin — retail calls `sub_23D40(x, y, spell,
    // 1)` AFTER the grid loop (a4=1 = raw opaque DrawBitmap frame, not
    // the translucent sub_23940 blend), overdrawing its neighbours
    // with the 64×44 frame. Frame [1]/[2] by the burst counter, icon
    // raw, availability meter at (+4,+36).
    // The redraw carries sub_23D40's expiry-blink gate too (the
    // :26967 caller runs the same :27670 skip).
    if let Some(spell_id) = hovered.filter(|sp| alert_blink || !loadout.expiring[sp.0 as usize]) {
        let sp = spell_id.0;
        let k = DISPLAY_ORDER.iter().position(|&d| d == sp).unwrap_or(0);
        let cell = book_cell(w, h, k);
        let (sx, sy) = {
            let s = HudFrame::new(w, h).s;
            (s, s)
        };
        let frame = if loadout.cooldown[sp as usize] > 0.0 {
            SPR_SLOT_HELD
        } else {
            SPR_SLOT_IDLE
        };
        if let Some((fw, fh)) = assets.sprite_dims(frame) {
            push_opt(
                &mut quads,
                assets.sprite_quad_rect_tint(frame, [cell[0], cell[1], fw * sx, fh * sy], WHITE),
            );
        }
        let su = sx;
        let icon_id = SPR_SPELL_ICON + sp as usize;
        if let Some((iw, ih)) = assets.sprite_dims(icon_id) {
            // Retail draws the icon at the frame origin (DrawBitmap
            // (a1, a2)), uniform art scale.
            push_opt(
                &mut quads,
                assets.sprite_quad_rect_tint(icon_id, [cell[0], cell[1], iw * su, ih * su], WHITE),
            );
        }
        // Availability meter (sub_23D40 :27703-34): partial-cast
        // progress bar + one shaded dot per whole affordable cast. Live
        // per-cast cost (castle scales with level), not the static table.
        let cost = loadout.cost[sp as usize].max(1);
        let mana = loadout.mana;
        let (mx, my) = (cell[0] + 4.0 * sx, cell[1] + 36.0 * sy);
        let partial = (56.0 * (mana % cost) as f32 / cost as f32).floor();
        quads.push(solid([mx, my, partial * sx, 4.0 * sy], METER_GREY));
        meter_dots(&mut quads, mx, my, sx, sy, (mana / cost).min(54) as usize);
        // Retail's sub_23D40 re-stamps the quickselect digit inside
        // the redraw (:27749-67) — without it the badge vanishes
        // exactly while hovering the cell you're assigning.
        if let Some(slot) = quick_binds.iter().position(|&b| b == Some(sp)) {
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(SPR_QUICK_DIGIT + slot, cell[0], cell[1], su, DIGIT_INK),
            );
        }
    }
    // The whole screen bottom (below the map + spellbook) is simply
    // BLACK and empty in retail — the multiplayer message log draws
    // there (via the DrawText path, not built yet), but with no panel
    // fill or tint. The renderer's black clear shows through; nothing to
    // draw here.
    (quads, hovered)
}

/// One living wizard's row on the map-screen mana/kills scoreboard
/// ([`roster_quads`]): the roster name, the census mana total
/// (entity +136/0x8C — base + Σ owned entity mana, NOT the spendable
/// +140/0x90), the kill row, and the slot's `(box, text)` colors
/// (see `entities::roster_team_colors`).
pub struct RosterEntry {
    pub name: String,
    pub mana: u32,
    pub kills: [u16; 8],
    pub box_c: [f32; 4],
    pub text_c: [f32; 4],
}

/// The map screen's wizard scoreboard — one screen shared by both
/// games: MC1's bottom-strip hover roster (`sub_22880` :27009) and
/// MC2's ALT "sorcerer scores" (`DrawSorcererScores_2D1D0` EF:22207)
/// draw the same centered grid from the same tile sprites: head tile
/// [85] (name at +8,+6; total mana `%d` at +8,+20) plus an 8-wide
/// kill-matrix of cell tiles [86] (`%03d` at +8,+10, the Type_160+30 /
/// word_0x26_38 kill tallies). Rows exist for IN-PLAY wizards (slot
/// flag +6 == 1 — cleared only on elimination/banishment, never on a
/// temporary death, so a respawning wizard keeps the row); layout
/// centers by the in-play count.
///
/// Both games FILL the tile interiors (inset 4, size − 8) with the
/// wizard's box color, opaque: MC1 via sub_24C20 (:27070-89), MC2 via
/// `DrawLine_2BC80` — a MISNAMED filled-rect blitter (Basic.cpp:1865
/// `memset` per row), not a border (playtest-verified against retail
/// screenshots 2026-07-24). The SELF-cell is a black fill in both
/// (MC1 `byte_AD167[1]` ink :27109; MC2 clrd `[0]` EF:22333), no
/// number.
///
/// Per-game flavor kept faithful:
///  - Absent/eliminated columns: MC1 draws NOTHING but still ADVANCES the
///    cursor (:27100-24 — only the self-cell arm draws inside the
///    skip branch; v9 += cellW runs regardless), leaving a GAP at a
///    departed wizard's column. MC2 compacts — `blackBarX` advances only
///    inside the alive branch (EF:22318-56).
///  - Centering quirks are retail's own: MC1 widths count the living
///    columns + the head tile (:27042-47) though up to 8 column
///    positions span; MC2 counts living columns + ONE CELL width for
///    the head (EF:22264) though the head tile is wider.
///
/// `rows[slot] = None` = slot absent or eliminated (retail's +6 != 1).
pub fn roster_quads(
    assets: &UiAssets,
    rows: &[Option<RosterEntry>; 8],
    mc2: bool,
    w: f32,
    h: f32,
) -> Vec<UiQuad> {
    let s = HudFrame::new(w, h).s;
    let (pw, ph) = assets.sprite_dims(SPR_ROSTER_HEAD).unwrap_or((104.0, 38.0));
    let (cw, ch) = assets.sprite_dims(SPR_ROSTER_CELL).unwrap_or((36.0, 38.0));
    let alive = rows.iter().flatten().count() as f32;
    let mut quads = Vec::new();
    if alive == 0.0 {
        return quads;
    }
    // Retail centers in the native 640×480(400) field; we center on
    // the live screen (same thing at 4:3).
    let head_w_for_centering = if mc2 { cw } else { pw };
    let x0 = (w - (alive * cw + head_w_for_centering) * s) / 2.0;
    let mut y = (h - alive * ph * s) / 2.0;
    // A tile or its fallback: the sprite blit, else a plain dark slab
    // (the tiles exist in both shipped atlases; belt only).
    let tile = |quads: &mut Vec<UiQuad>, id: usize, rect: [f32; 4]| {
        push_opt(
            quads,
            assets
                .sprite_quad_rect_tint(id, rect, WHITE)
                .or(Some(solid(rect, [0.10, 0.09, 0.08, 0.92]))),
        );
    };
    // The opaque interior fill (inset 4, size − 8) — MC1 sub_24C20 /
    // MC2 DrawLine_2BC80, both raw memset fills of the palette color.
    let fill = |quads: &mut Vec<UiQuad>, r: [f32; 4], c: [f32; 4]| {
        quads.push(solid(
            [
                r[0] + 4.0 * s,
                r[1] + 4.0 * s,
                r[2] - 8.0 * s,
                r[3] - 8.0 * s,
            ],
            c,
        ));
    };
    // The self-cell ink: MC1 byte_AD167[1] / MC2 clrd[0], both black.
    const SELF_INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    for (i, row) in rows.iter().enumerate() {
        let Some(r) = row else { continue };
        let head = [x0, y, pw * s, ph * s];
        tile(&mut quads, SPR_ROSTER_HEAD, head);
        fill(&mut quads, head, r.box_c);
        quads.extend(assets.text_quads(&r.name, x0 + 8.0 * s, y + 6.0 * s, r.text_c, s));
        quads.extend(assets.text_quads(
            &format!("{}", r.mana),
            x0 + 8.0 * s,
            y + 20.0 * s,
            r.text_c,
            s,
        ));
        let mut x = x0 + pw * s;
        for (j, col) in rows.iter().enumerate() {
            match col {
                // Absent/dead column: draws nothing, and the columns
                // COMPACT in sync with the row removal (player-ruled,
                // deliberate — docs/DEVIATIONS.md): MC2 retail
                // compacts (EF:22318-56, blackBarX advances only in
                // the alive branch); MC1's decompiled loop advances
                // unconditionally (:27100-24), which would leave a
                // one-column hole at an eliminated wizard — unverified
                // against retail and reads as a bug, so MC1 follows
                // the MC2 law.
                None => continue,
                Some(_) if j == i => {
                    // The self-cell: a black-filled box, no number.
                    let cell = [x, y, cw * s, ch * s];
                    tile(&mut quads, SPR_ROSTER_CELL, cell);
                    fill(&mut quads, cell, SELF_INK);
                }
                Some(c) => {
                    let cell = [x, y, cw * s, ch * s];
                    tile(&mut quads, SPR_ROSTER_CELL, cell);
                    fill(&mut quads, cell, c.box_c);
                    quads.extend(assets.text_quads(
                        &format!("{:03}", r.kills[j]),
                        x + 8.0 * s,
                        y + 10.0 * s,
                        c.text_c,
                        s,
                    ));
                }
            }
            x += cw * s;
        }
        y += ph * s;
    }
    quads
}

/// A quad-built pixel arrow standing in for the OS pointer wherever
/// the fullscreen rule suppresses it (player ruling: a fullscreen
/// window keeps the cursor captured at all times, so the OS pointer
/// never shows). No retail cursor art lives in the LEVEL banks — the
/// original's in-game pointer is a dedicated low-level bitmap, not a
/// bank sprite — so this stands in on the surfaces without one (the
/// in-level UI, the MC1 menu); the MC2 temple menu and the world map
/// draw their own retail cursor sprites. White fill, black outline,
/// hotspot at the tip. All solid quads: draws over ANY bound atlas.
pub fn cursor_quads(x: f32, y: f32, s: f32) -> Vec<UiQuad> {
    const ROWS: [&str; 17] = [
        "X",
        "XX",
        "X.X",
        "X..X",
        "X...X",
        "X....X",
        "X.....X",
        "X......X",
        "X.......X",
        "X........X",
        "X.....XXXX",
        "X..X..X",
        "X.X X..X",
        "XX  X..X",
        "X    X..X",
        "      X..X",
        "       XX",
    ];
    const INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    const FILL: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let mut quads = Vec::new();
    for (ry, row) in ROWS.iter().enumerate() {
        for (rx, b) in row.bytes().enumerate() {
            let c = match b {
                b'X' => INK,
                b'.' => FILL,
                _ => continue,
            };
            quads.push(solid([x + rx as f32 * s, y + ry as f32 * s, s, s], c));
        }
    }
    quads
}

/// The map-roster tiles, same index in BOTH shipped banks (MC1
/// begSprTab :27083/:27130; MC2 MSPRD00 `SPELL_TILE`/`SPELL_TILE_MINI`,
/// GameBitmapIndexes.h:21-22): [85] = the 104×38 head tile (portrait/
/// name + mana), [86] = the 36×38 kill-matrix cell.
const SPR_ROSTER_HEAD: usize = 85;
const SPR_ROSTER_CELL: usize = 86;
// The retail POINTERS bank rides at the tail of each level UI atlas
// (`Assets::pointer_base`). Bank anatomy — settled by rendering each
// entry under each candidate palette (2026-07-24): [0] = null "don't
// draw" sentinel in BOTH games. MC1: [1] = the golden ARROW + MANA
// BALL in-game cursor (level palette); [2..=8] = the pointing HAND
// menu cursor pre-quantized per menu-screen palette ([7] =
// MAINMENU.PAL — the frontend blits it from its own copy of the
// bank). MC2: the grey arrow+ball per map type — day [1], night [9],
// cave [10]. STATIC always: same-size runs in these banks are
// PALETTE VARIANTS, not animation (cycling them reads as flicker).
/// The in-game pointer entry both games use on DAY-family palettes.
pub const POINTER_ENTRY_DEFAULT: usize = 1;
/// MC2's night / cave pointer entries (LevelInit.cpp:24-38).
pub const POINTER_ENTRY_NIGHT: usize = 9;
pub const POINTER_ENTRY_CAVE: usize = 10;

// Panel sprite ids (remc1 begSprTab). The panel strip is laid out at
// the original's 640-wide coordinates, scaled to the live resolution.
const SPR_SLOT_IDLE: usize = 1; // equipped-spell frame, idle
const SPR_SLOT_HELD: usize = 2; // equipped-spell frame, active/held
const SPR_PANEL_BG: usize = 40; // wizard-strip left cap
const SPR_WIZ_BG: usize = 41; // a wizard sub-panel background
const SPR_DIVIDER: usize = 42; // between the level digit and the bars
const SPR_CASTLE_LVL: usize = 43; // +level 0..7 = the castle-level glyph
const SPR_BALLOON_GLYPH: usize = 50; // +count 1..3 = the balloon-roster glyph
const SPR_WIZ_EMPTY: usize = 54; // no-wizard slot
const SPR_WIZ_ALERT: usize = 55; // castle-under-attack flash
const SPR_SPELL_ICON: usize = 6; // spell icon base: [spell + 6]
/// HUD panel background translucency — the original's panels blend over
/// the framebuffer (transparency is ALWAYS on, not a toggle). We
/// approximate with an alpha over the sky; the icons/glyphs/bars stay
/// opaque (drawn raw in retail).
const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, mgc_render::HUD_PANEL_ALPHA];
/// Life-bar color (remc1 uses palette index 0x7B, a team red).
const LIFE_RED: [f32; 4] = [0.85, 0.15, 0.12, 1.0];
/// Collected/banked mana bar (sub_22E50 :27377, color v29 =
/// byte_99B58[2*owner]) — WHITE, not blue.
const MANA_WHITE: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
/// Spell availability progress bar (sub_23D40 :27705, color v26 =
/// byte_99B58[1+2*owner]) — GREY, not blue; the partial mana toward
/// the next cast, under the equipped-spell icon.
const METER_GREY: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
/// The flyout XP bar (EF:22668-70): background = CLRD code 0,
/// fill = CLRD 3840 (0xF00 — pure red).
const XP_BAR_BG: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const XP_BAR_RED: [f32; 4] = [0.85, 0.1, 0.05, 1.0];
/// Locked-spell overlay (sub_24230 :27860 + sub_23D40 :27767): when a
/// spell's castle_req (+132) exceeds the linked castle's STORED mana
/// (+140) — or no castle stands — retail remaps the whole cell/panel
/// rect through fog row 0x30 (sub_247C0), a uniform wash over
/// everything beneath. DARKENS — which means the fog rows run dark-high
/// (WATCH: the map marker cross's white fade may need a polarity
/// re-check); exact shade lands with the LUT bake.
const LOCKED_WASH: [f32; 4] = [0.0, 0.0, 0.0, 0.5];
/// Bar geometry (sub_22810 draws a 64-wide fill; sub_22E50 offsets).
const BAR_W: f32 = 64.0;
const BAR_X: f32 = 58.0; // bars start +58 from the sub-panel origin
/// One HUD section = 640/5 = 128 native px (the 5×20% top strip).
const HUD_SECTION: f32 = 128.0;

/// The non-4:3 presentation law — ONE source, shared with the
/// renderer (which anchors the radar disc, the map panes and the world
/// viewport by the same rule). See [`mgc_render::HudFrame`].
pub use mgc_render::HudFrame;

/// A solid bar fill at panel-space (x,y) scaled to screen — the
/// original's `sub_22810(x,y,64,h,(val<<6)/max,color)`: `fill` is the
/// value/max fraction of the 64-px ruler. Faithful sub_22810 (:26991)
/// draws ONLY the clamped colored fill, straight on the panel marble —
/// no background track — and skips fills under 2px.
fn bar(quads: &mut Vec<UiQuad>, s: f32, x: f32, y: f32, h: f32, frac: f32, color: [f32; 4]) {
    let fill = (BAR_W * frac.clamp(0.0, 1.0)).max(0.0);
    if fill >= 2.0 {
        quads.push(solid([x * s, y * s, fill * s, h * s], color));
    }
}

/// A thin (2-px) balloon bar with no dark track — the original stacks
/// these per balloon (sub_22E50 :27338-39, `sub_22810(x, y, 64, 2,
/// frac, color)`). Just the colored fill; the panel marble shows
/// between them.
fn thin_bar(quads: &mut Vec<UiQuad>, s: f32, x: f32, y: f32, frac: f32, color: [f32; 4]) {
    let fill = (BAR_W * frac.clamp(0.0, 1.0)).max(0.0);
    if fill >= 1.0 {
        quads.push(solid([x * s, y * s, fill * s, 2.0 * s], color));
    }
}

/// The availability dots (sub_23D40 :27713-33): one dot per whole
/// affordable cast, filled column-major (2 rows at +0/+2, 27 columns
/// at +4,+6,…). Each dot is EXACTLY ONE native pixel — both screen
/// writers (sub_615D4 hi-res 640w, sub_61594 lo-res 320w) plot a
/// single byte; the "shaded 2×2" look in DOSBox captures is its
/// upscaler smearing that pixel across the 2-px spacing grid. `mx/my`
/// in screen px; `sx/sy` = native→screen scale (snap() keeps every dot
/// rasterizing alike).
fn meter_dots(quads: &mut Vec<UiQuad>, mx: f32, my: f32, sx: f32, sy: f32, casts: usize) {
    for d in 0..casts {
        let (col, row) = ((d / 2) as f32, (d % 2) as f32);
        let (x, y) = (mx + col * 2.0 * sx, my + row * 2.0 * sy);
        quads.push(solid([x, y, sx.max(1.0), sy.max(1.0)], MANA_WHITE));
    }
}

/// In-game HUD — the faithful top strip (remc1 sub_22E50 wizard panel +
/// sub_23D40 equipped-spell panels), laid out at the original's 640-wide
/// coordinates scaled by `w/640`. The rotating round minimap is drawn by
/// the renderer; here we place the wizard stat panel (left) and the two
/// equipped-spell panels (right, x=510/574).
///
/// `mc2` = an MC2 atlas is loaded: the equipped-spell panels skip
/// their icon/meter/wash — the MC1 sprite ids `[6+spell]` mean other
/// art there, and MC2's real top tile (the big icon `123+spell` + the
/// Roman-numeral level + mana pool, DrawSpellIcon_2E260) waits on the
/// level machinery + DrawText. The whole MC2 HUD layout is the parity
/// track.
pub fn hud_quads(
    assets: &UiAssets,
    loadout: &LoadoutView,
    vitals: &PlayerVitals,
    transparent: bool,
    alert_blink: bool,
    goal_blink: bool,
    mc2: bool,
    mc2_book: Option<&Mc2BookView>,
    dev_spells: bool,
    w: f32,
    h: f32,
) -> Vec<UiQuad> {
    let mut quads = Vec::new();
    // The strip splits into two anchored groups (see `HudFrame`): the
    // radar + three wizard sub-panels are LEFT-anchored — they already
    // read as `native * s` from x=0, so nothing below changes — and the
    // two equipped-spell panels are RIGHT-anchored, which is the only
    // place the anchor is spelled out.
    let f = HudFrame::new(w, h);
    let s = f.s;
    // Panel-background tint: translucent (faithful MC1, always-on
    // transparency) or opaque (the MC2 readability toggle).
    let panel_tint = if transparent { PANEL_TINT } else { WHITE };

    // --- Wizard stat strip (sub_22E50): three 128-wide sub-panels. ---
    // Tiles pack from x=2: [40] radar frame (124w), then sub-panels at
    // v22 = 2 + [40].w, then +128 each. The three panels are, in order
    // (the trace :27214/:27334/:27374):
    //   A (v22, `var_50`)  = the player's LINKED CASTLE — castle HP +
    //                        castle mana capacity/banked, level glyph.
    //   B (v23, `var_52[]`)= the player's MANA BALLOONS — 1..3 by castle
    //                        level, each a thin stacked HP + cargo bar.
    //   C (v24, `a1x`)     = the player's OWN wizard — self life + mana
    //                        capacity/banked, drawn UNCONDITIONALLY.
    let cap_w = assets.sprite_dims(SPR_PANEL_BG).map_or(124.0, |(w, _)| w);
    push_opt(
        &mut quads,
        assets.sprite_quad_tint(SPR_PANEL_BG, 2.0 * s, 2.0 * s, s, panel_tint),
    );
    let v22 = 2.0 + cap_w; // slot A = castle panel
    let v23 = v22 + HUD_SECTION; // slot B = balloons
    let v24 = v22 + 2.0 * HUD_SECTION; // slot C = self

    let world = loadout.world_mana.max(1) as f32;
    // The level-goal marks (:27267-74 slot A / :27380-87 slot C): TWO
    // 2×2 ticks at y=26 and y=38 bracketing the mana ruler at win_pct%
    // of its 64px width (`v20 + (pct<<6)/100`), colour alternating
    // between the two team-ramp entries per blink frame (v28 =
    // byte_99B58[2·owner + phase]). The player's ramp reads WHITE /
    // TRANSPARENT in retail (the off entry blends through the
    // sub_616C0 translucency LUT, word_9ADFC & 4 — player
    // retail-verified), so the off phase SKIPS the draw and the
    // pixel below shows through. No completion recolour — retail
    // keeps blinking after the goal is met. MC2's own ramp blinks
    // white/blue-ish; reusing the MC1 white/transparent marker is a
    // deliberate deviation (docs/DEVIATIONS.md).
    let win_tick = |quads: &mut Vec<UiQuad>, ox: f32| {
        if loadout.win_pct > 0 && goal_blink {
            let tx = (ox + BAR_X) * s + BAR_W * s * (loadout.win_pct as f32 / 100.0).min(1.0);
            for y in [26.0, 38.0] {
                quads.push(solid([tx, y * s, 2.0 * s, 2.0 * s], MANA_WHITE));
            }
        }
    };

    // === Slot A: the linked castle (:27215). Gated on the castle
    // existing AND level > 0 (else the bare marble [54]). player. ===
    // Alert marbles: retail flickers [55] on alternate blink frames
    // while the per-source hit counter runs (u8_391 castle / u8_393
    // balloons / u8_392 self, each decremented per flash) — the
    // `alert_blink` gate approximates that frame flicker.
    let castle = loadout.castle.filter(|(_, _, lvl)| *lvl > 0);
    let slot_a_bg = if castle.is_none() {
        SPR_WIZ_EMPTY
    } else if vitals.castle_alert && alert_blink {
        SPR_WIZ_ALERT
    } else {
        SPR_WIZ_BG
    };
    push_opt(
        &mut quads,
        assets.sprite_quad_tint(slot_a_bg, v22 * s, 2.0 * s, s, panel_tint),
    );
    if let Some((_stored, capacity, level)) = castle {
        let ox = v22;
        // Castle-level glyph [43+level] (emblem/heart/orb/digit baked
        // in) then the divider [42].
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_CASTLE_LVL + level as usize, (ox + 2.0) * s, 2.0 * s, s),
        );
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_DIVIDER, (ox + 38.0) * s, 2.0 * s, s),
        );
        // Life bar (+58, y=10) = the CASTLE's HP (v4x->actLife/maxLife,
        // palette 0x7B) — NOT the player's life (:27237). castle_hp is
        // the downgrade meter.
        let hp = loadout
            .castle_hp
            .map_or(1.0, |(cur, max)| cur.max(0) as f32 / max.max(1) as f32);
        bar(&mut quads, s, ox + BAR_X, 10.0, 10.0, hp, LIFE_RED);
        // Mana capacity + banked, world-relative (y=28), overlaid
        // (:27240-66 verbatim): capacity (castle +136) in v27 =
        // byte_99B58[1+2·team] — the GREY family, same index as the
        // spell meter — then the BANKED total (houses u32_308 + castle
        // stored +140 = loadout.banked; do NOT add `stored` again — that
        // double-counts and pins the bar full). banked == capacity
        // blinks the single full bar between the pair (:27242-53).
        if loadout.banked >= capacity && capacity > 0 {
            let c = if alert_blink { METER_GREY } else { MANA_WHITE };
            bar(
                &mut quads,
                s,
                ox + BAR_X,
                28.0,
                10.0,
                capacity as f32 / world,
                c,
            );
        } else {
            bar(
                &mut quads,
                s,
                ox + BAR_X,
                28.0,
                10.0,
                capacity as f32 / world,
                METER_GREY,
            );
            bar(
                &mut quads,
                s,
                ox + BAR_X,
                28.0,
                10.0,
                loadout.banked as f32 / world,
                MANA_WHITE,
            );
        }
        win_tick(&mut quads, ox);
    }

    // === Slot B: the mana balloons (:27278-344). The marble [54]
    // ONLY when no castle stands (:27281); otherwise the glyph is
    // [50+roster] where the roster WIDTH comes from castle level —
    // it does NOT shrink when balloons die (dead slots simply draw no
    // bars, :27335-40). Thin HP + cargo bars per live balloon. ===
    let balloons = &loadout.balloons;
    let slot_b_bg = if balloons.is_empty() {
        SPR_WIZ_EMPTY
    } else if vitals.balloon_alert && alert_blink {
        SPR_WIZ_ALERT
    } else {
        SPR_WIZ_BG
    };
    push_opt(
        &mut quads,
        assets.sprite_quad_tint(slot_b_bg, v23 * s, 2.0 * s, s, panel_tint),
    );
    if !balloons.is_empty() {
        let ox = v23;
        let roster = balloons.len().min(3);
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_BALLOON_GLYPH + roster, (ox + 2.0) * s, 2.0 * s, s),
        );
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_DIVIDER, (ox + 38.0) * s, 2.0 * s, s),
        );
        // Per LIVE balloon: HP bar at y=12+2i (red), cargo bar at
        // y=30+2i (banked-mana white) — the thin stacked lines
        // (:27338-39); dead/unspawned roster slots stay bar-less.
        for (i, slot) in balloons.iter().enumerate().take(3) {
            let Some((hp, cargo)) = *slot else { continue };
            let y = 2.0 * i as f32;
            thin_bar(&mut quads, s, ox + BAR_X, 12.0 + y, hp, LIFE_RED);
            thin_bar(&mut quads, s, ox + BAR_X, 30.0 + y, cargo, MANA_WHITE);
        }
    }

    // === Slot C: the player's OWN wizard (:27346-388). Always drawn
    // (no gate) — the wizard is always present. The alert marble here
    // is the PLAYER-hit flash (u8_392, :27347), independent of the
    // castle's u8_391. ===
    let slot_c_bg = if vitals.player_alert && alert_blink {
        SPR_WIZ_ALERT
    } else {
        SPR_WIZ_BG
    };
    push_opt(
        &mut quads,
        assets.sprite_quad_tint(slot_c_bg, v24 * s, 2.0 * s, s, panel_tint),
    );
    {
        let ox = v24;
        // Base wizard glyph [43] + divider [42] (:27358-72; the alert
        // /grace variant swaps a blended copy — we keep the plain draw).
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_CASTLE_LVL, (ox + 2.0) * s, 2.0 * s, s),
        );
        push_opt(
            &mut quads,
            assets.sprite_quad(SPR_DIVIDER, (ox + 38.0) * s, 2.0 * s, s),
        );
        // Self life bar (+58, y=10) = the PLAYER's health (a1x->actLife,
        // 0x7B red) — this is where player life belongs (:27375).
        bar(
            &mut quads,
            s,
            ox + BAR_X,
            10.0,
            10.0,
            vitals.life as f32 / vitals.life_max.max(1) as f32,
            LIFE_RED,
        );
        // Self mana: capacity (var_136 = mana_max, the v27 grey) +
        // current (var_140 = mana, white) over the world total
        // (:27376-77).
        bar(
            &mut quads,
            s,
            ox + BAR_X,
            28.0,
            10.0,
            loadout.mana_max as f32 / world,
            METER_GREY,
        );
        bar(
            &mut quads,
            s,
            ox + BAR_X,
            28.0,
            10.0,
            loadout.mana as f32 / world,
            MANA_WHITE,
        );
        win_tick(&mut quads, ox);
    }
    // --- Equipped-spell panels (sub_23D40) at x=510 and x=574. ---
    // Frame [1]/[2] (64x44), then the icon [spell+6] at its NATIVE 62x34
    // (top-aligned, NOT stretched to the frame), then the availability
    // meter at y=+36: a progress bar (partial mana toward the next cast)
    // plus a row of dots (whole casts currently affordable) — sub_23D40
    // :27700-34.
    // `px` is the panel's PHYSICAL left edge, right-anchored off the
    // authored 510/574 (the pair ends at 574+64 = 638, i.e. 2 native px
    // clear of the right edge — the mirror of the radar's x=2 margin,
    // so the two groups sit symmetrically in their corners).
    for (hand, px) in [(0usize, f.rx(510.0)), (1usize, f.rx(574.0))] {
        // The bound spell + cast-in-progress state per column: MC1
        // reads the loadout; MC2 reads the native spell book (the
        // quick-slots + the armed cast window).
        let (spell, active) = if let Some(bv) = mc2_book.filter(|_| mc2) {
            let b = if hand == 0 { bv.left } else { bv.right };
            let sp = u8::try_from(b).ok();
            (sp, sp.is_some_and(|sp| bv.armed[sp as usize]))
        } else {
            let sp = if hand == 0 {
                loadout.left
            } else {
                loadout.right
            };
            (sp, sp.is_some_and(|sp| loadout.cooldown[sp as usize] > 0.0))
        };
        // THE EXPIRY BLINK (sub_23D40 :27670-71 / DrawSpellIcon_2E260
        // GameUI.cpp:351-54): while the bound spell's countdown runs
        // its last window (< 64 of a > 64 full count on MC1; < 32 on
        // MC2's flag-4 long-runners), retail skips the WHOLE panel —
        // frame art included — every other turn (blink bank index [1]
        // = Turn & 1), letting the view show through. On Create
        // Castle / Global Death the same gate reads as "recast almost
        // ready" (their +48 is the recast lockout).
        let expiring = spell.is_some_and(|sp| {
            if let Some(bv) = mc2_book.filter(|_| mc2) {
                bv.expiring[sp as usize]
            } else {
                loadout.expiring[sp as usize]
            }
        });
        if expiring && !alert_blink {
            continue;
        }
        // Frame [2] = the CAST-IN-PROGRESS highlight (sub_23D40 :27675:
        // `a3x->var_48` = the burst/duration countdown, live from cast
        // to expiry), else the idle frame [1]. Equipped ≠ casting — the
        // highlight flashes on projectile casts and stays lit for
        // duration effects (speed etc.), driven by that countdown.
        let frame = if active { SPR_SLOT_HELD } else { SPR_SLOT_IDLE };
        push_opt(
            &mut quads,
            assets.sprite_quad_tint(frame, px, 2.0 * s, s, panel_tint),
        );
        if mc2 {
            // MC2 hand panel: the dedicated BIG spell-icon run —
            // retail's `DrawSpellIcon_2E260` draws
            // `posistruct[model + SPELL_FIREBALL_BIG]` = sprite
            // 123 + spell (GameUI.cpp:374; the CTRL grid's small
            // run at 97+ is DIFFERENT art). Same MSPR/HSPR bank, icon
            // at the frame origin like the MC1 panel; the meter below
            // is retail's DrawLine primitives
            // (docs/traces/mc2-hud-hand-icons.md).
            let Some(bv) = mc2_book else { continue };
            let Some(sp) = spell else { continue };
            push_opt(
                &mut quads,
                assets.sprite_quad(123 + sp as usize, px, 2.0 * s, s),
            );
            // The selected LEVEL numeral (retail's per-level "I/II/III"
            // art), top-right of the big icon — the SAME baked sprite the
            // CTRL-pane flyout uses (`MC2_SPR_SUB_NUM + level`), not
            // hand-drawn text (docs/traces/mc2-hud-hand-icons.md §2d).
            let level = bv.sel.get(sp as usize).copied().unwrap_or(0).min(2) as usize;
            let (nw, _nh) = assets
                .sprite_dims(MC2_SPR_SUB_NUM + level)
                .unwrap_or((10.0, 12.0));
            push_opt(
                &mut quads,
                assets.sprite_quad(MC2_SPR_SUB_NUM + level, px + (60.0 - nw) * s, 3.0 * s, s),
            );
            let cost = bv.cost[sp as usize].max(1);
            let mana = loadout.mana;
            let mx = px + 4.0 * s;
            let my = (2.0 + 36.0) * s;
            let partial = (56.0 * (mana % cost) as f32 / cost as f32).floor();
            quads.push(solid([mx, my, partial * s, 4.0 * s], METER_GREY));
            meter_dots(&mut quads, mx, my, s, s, (mana / cost).min(54) as usize);
            // Locked equipped spell (retail `DrawSpellIcon_2E260`
            // UI:398-405): the manifestation's +136 — the SELECTED
            // tier's `maxManaLimit` castle prerequisite — is nonzero
            // and unmet (no castle, or its STORED mana below it) →
            // the WHOLE box is colour-washed (`DrawSquareByColor`,
            // palette 16 non-Day / 48 Day). That condition is
            // exactly the pane grey-out's `castable[s][sel]`
            // canSummon law; LOCKED_WASH stands in for the palette
            // square, the same approximation the MC1 arm uses.
            // dev_spells bypasses the afford gate for real (the CTRL
            // pane's `|| dev` arm) — don't wash what casts fine.
            if !dev_spells && !bv.castable[sp as usize][level] {
                let (fw, fh) = assets.sprite_dims(frame).unwrap_or((64.0, 44.0));
                quads.push(solid([px, 2.0 * s, fw * s, fh * s], LOCKED_WASH));
            }
            continue;
        }
        if let Some(sp) = spell {
            // Icon drawn raw at the FRAME ORIGIN (sub_23D40's
            // `DrawBitmap(a1, a2, icon)` — the art's own margins do
            // the placement), native size × the HUD scale.
            push_opt(
                &mut quads,
                assets.sprite_quad(SPR_SPELL_ICON + sp as usize, px, 2.0 * s, s),
            );
            // Availability meter at (frame+4, frame+36) — sub_23D40
            // :27703-34: the grey partial-cast progress bar, then one
            // 2×2 SHADED dot per whole affordable cast over it. The cost
            // is the LIVE per-cast cost (castle scales with its level),
            // not the static table (sub_23D40 divides by the
            // manifestation's +136).
            let cost = loadout.cost[sp as usize].max(1);
            let mana = loadout.mana;
            let mx = px + 4.0 * s; // sub_23D40 a1+4
            let my = (2.0 + 36.0) * s; // a2+36
            let partial = (56.0 * (mana % cost) as f32 / cost as f32).floor();
            quads.push(solid([mx, my, partial * s, 4.0 * s], METER_GREY));
            meter_dots(&mut quads, mx, my, s, s, (mana / cost).min(54) as usize);
            // Locked equipped spell (sub_23D40 :27767): the same fog
            // wash as the book cell covers the whole panel while the
            // castle_req isn't met — the equipped hand tells you it
            // can't fire.
            if !loadout.bindable[sp as usize] {
                let (fw, fh) = assets.sprite_dims(frame).unwrap_or((64.0, 44.0));
                quads.push(solid([px, 2.0 * s, fw * s, fh * s], LOCKED_WASH));
            }
        }
    }
    quads
}

/// Pause indicator. Retail draws the text "PAUSED" at native (132,50)
/// (banked, waiting on the DrawText path); until the font lands, a ‖
/// pause glyph at the same spot marks the frozen sim so a still screen
/// doesn't read as a hang.
/// The retail OK / Cancel button sprites of the in-game MSPR bank
/// (`GameBitmapIndexes.h` SPELL_BUTTON_OK1/CANCEL1) — present in the
/// MC2 UI bank, absent from MC1's (which never had the dialog).
const OK_SPRITE: usize = 257;
const CANCEL_SPRITE: usize = 258;

/// The in-level abandon-confirm button rects in window pixels — the
/// retail geometry (`GetOkayCancelButtonPositions_30BE0`, GameUI.cpp:
/// 4578): a contiguous horizontal pair centered on screen, each half
/// 50×32 native (the sprite size). One geometry shared by the draw
/// and the click hit test.
pub fn exit_confirm_rects(w: f32, h: f32) -> ([f32; 4], [f32; 4]) {
    let s = HudFrame::new(w, h).s.max(1.0);
    let (bw, bh) = (50.0 * s, 32.0 * s);
    let ok = [w / 2.0 - bw, h / 2.0 - bh / 2.0, bw, bh];
    let cancel = [w / 2.0, h / 2.0 - bh / 2.0, bw, bh];
    (ok, cancel)
}

/// The in-level abandon-confirmation dialog — the retail MC2 law
/// (`DrawOkCancelMenu_30A60`, GameUI.cpp:4591-4636): the prompt in
/// the in-game font over the LIVE view (no panel — the world shows
/// through) above the centered OK/Cancel sprite pair (MSPR 257/258,
/// the in-level HUD bank). Everything draws from IN-LEVEL assets, so
/// MC1/HW and single-level mode reuse it verbatim — MC1's bank has
/// no OK/Cancel art (retail MC1 had no dialog), so it falls back to
/// labeled slab buttons in the same geometry. The mild hover tint is
/// presentational (deliberate; retail's feedback was the cursor itself).
/// MC2's objective TEXTBOX (`DrawCurrentObjectiveTextbox_30630`
/// GameUI.cpp:532-616): RED letters (CLRD 3840 = 0xF00) over a
/// translucent black wash, inside the stone tile frame — corners
/// [171], left/right columns [172], top/bottom runs [173]
/// (`DrawTextboxFrame_89690` GU:3571-3607). The interior snaps to
/// whole tile multiples like retail's `ComputeFrameSizes_89980`, the
/// box centers on x and sits upper-middle, and TEXT draws before the
/// FRAME (GU:614-15) so the border overlaps glyph bleed. Falls back
/// to plain slab edges when the frame tiles are absent (MC1 banks).
/// `lines` = pre-wrapped text (the caller owns the wrap width).
pub fn objective_box_quads(assets: &UiAssets, lines: &[String], w: f32, h: f32) -> Vec<UiQuad> {
    let mut quads = Vec::new();
    if lines.is_empty() || !assets.has_font() {
        return quads;
    }
    let s = HudFrame::new(w, h).s;
    let font_s = 2.0 * s;
    let lh = assets.font_line_height() * font_s;
    let text_w = lines
        .iter()
        .map(|l| assets.text_width(l) * font_s)
        .fold(0.0, f32::max);
    let text_h = lines.len() as f32 * lh;
    let (run_w, run_h) = assets
        .sprite_dims(MC2_SPR_FRAME_RUN)
        .unwrap_or((32.0, 10.0));
    let (side_w, side_h) = assets
        .sprite_dims(MC2_SPR_FRAME_SIDE)
        .unwrap_or((16.0, 14.0));
    // Interior snapped to whole frame tiles around the padded text.
    let tiles_x = ((text_w + 16.0 * s) / (run_w * s)).ceil().max(1.0);
    let tiles_y = ((text_h + 8.0 * s) / (side_h * s)).ceil().max(1.0);
    let iw = tiles_x * run_w * s;
    let ih = tiles_y * side_h * s;
    let ix = (w - iw) / 2.0;
    let iy = 0.32 * h - ih / 2.0;
    // The interior wash (retail's ColorizeScreen blend-LUT darken).
    quads.push(solid([ix, iy, iw, ih], [0.0, 0.0, 0.0, 0.55]));
    // Text, centered in the interior, pure red.
    let red = [1.0, 0.0, 0.0, 1.0];
    let mut ty = iy + (ih - text_h) / 2.0;
    for line in lines {
        let lw = assets.text_width(line) * font_s;
        quads.extend(assets.text_quads(line, ix + (iw - lw) / 2.0, ty, red, font_s));
        ty += lh;
    }
    if assets.sprite_dims(MC2_SPR_FRAME_CORNER).is_some() {
        // The tiled border, then the corners over the run ends.
        for i in 0..tiles_x as usize {
            let x = ix + i as f32 * run_w * s;
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(MC2_SPR_FRAME_RUN, x, iy - run_h * s, s, WHITE),
            );
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(MC2_SPR_FRAME_RUN, x, iy + ih, s, WHITE),
            );
        }
        for j in 0..tiles_y as usize {
            let y = iy + j as f32 * side_h * s;
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(MC2_SPR_FRAME_SIDE, ix - side_w * s, y, s, WHITE),
            );
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(MC2_SPR_FRAME_SIDE, ix + iw, y, s, WHITE),
            );
        }
        let (cw, ch) = assets
            .sprite_dims(MC2_SPR_FRAME_CORNER)
            .unwrap_or((16.0, 12.0));
        for (cx, cy) in [
            (ix - cw * s, iy - ch * s),
            (ix + iw, iy - ch * s),
            (ix - cw * s, iy + ih),
            (ix + iw, iy + ih),
        ] {
            push_opt(
                &mut quads,
                assets.sprite_quad_tint(MC2_SPR_FRAME_CORNER, cx, cy, s, WHITE),
            );
        }
    } else {
        // MC1-bank fallback: plain slab edges (the exit-confirm
        // idiom) so held-O still reads as a box.
        let t = 2.0 * s;
        let edge = [0.6, 0.6, 0.6, 0.9];
        quads.push(solid([ix - t, iy - t, iw + 2.0 * t, t], edge));
        quads.push(solid([ix - t, iy + ih, iw + 2.0 * t, t], edge));
        quads.push(solid([ix - t, iy, t, ih], edge));
        quads.push(solid([ix + iw, iy, t, ih], edge));
    }
    quads
}

pub fn exit_confirm_quads(
    assets: &UiAssets,
    text: &str,
    w: f32,
    h: f32,
    cursor: (f32, f32),
) -> Vec<UiQuad> {
    const INK: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const SLAB_BG: [f32; 4] = [0.0, 0.0, 0.0, 0.45];
    const EDGE: [f32; 4] = [0.75, 0.75, 0.75, 0.35];
    const HOVER: [f32; 4] = [1.0, 1.0, 1.0, 0.10];
    let fs = w / 320.0;
    let (ok, cancel) = exit_confirm_rects(w, h);
    let mut quads = Vec::new();
    // The prompt, centered above the button pair, on a soft slab for
    // readability over bright terrain (jar-marker idiom; retail's
    // palette font carried its own contrast).
    let tw = assets.text_width(text) * fs;
    let lh = assets.font_line_height() * fs;
    let ty = ok[1] - 2.0 * lh;
    let pad = 0.4 * lh;
    quads.push(solid(
        [
            (w - tw) / 2.0 - pad,
            ty - pad,
            tw + 2.0 * pad,
            lh + 2.0 * pad,
        ],
        SLAB_BG,
    ));
    quads.extend(assets.text_quads(text, (w - tw) / 2.0, ty, INK, fs));
    for (r, id, label) in [(ok, OK_SPRITE, "OK"), (cancel, CANCEL_SPRITE, "Cancel")] {
        match assets.map_stamp(id) {
            // The retail sprite fills its half exactly (50×32).
            Some(st) => quads.push(UiQuad {
                rect: r,
                uv: st.uv,
                tint: [1.0, 1.0, 1.0, 1.0],
            }),
            // MC1 fallback: a labeled slab in the same geometry.
            None => {
                quads.push(solid(r, SLAB_BG));
                let s = HudFrame::new(w, h).s.max(1.0);
                for e in [
                    [r[0], r[1], r[2], s],
                    [r[0], r[1] + r[3] - s, r[2], s],
                    [r[0], r[1], s, r[3]],
                    [r[0] + r[2] - s, r[1], s, r[3]],
                ] {
                    quads.push(solid(e, EDGE));
                }
                let lw = assets.text_width(label) * fs;
                quads.extend(assets.text_quads(
                    label,
                    r[0] + (r[2] - lw) / 2.0,
                    r[1] + (r[3] - lh) / 2.0,
                    INK,
                    fs,
                ));
            }
        }
        if rect_hit(r, cursor) {
            quads.push(solid(r, HOVER));
        }
    }
    quads
}

/// The retail PAUSED indicator (drawn at 320-native 132,50): two
/// small white bars over the live view. Kept alongside the pause
/// mini-menu — the panel is a MENU, this is the pause state, and the
/// player reads them in different places.
pub fn pause_quads(w: f32, h: f32) -> Vec<UiQuad> {
    let s = HudFrame::new(w, h).s.max(1.0);
    let (x, y) = (132.0 * s, 50.0 * s);
    let ink = [0.95, 0.95, 0.95, 0.95];
    vec![
        solid([x, y, 4.0 * s, 14.0 * s], ink),
        solid([x + 8.0 * s, y, 4.0 * s, 14.0 * s], ink),
    ]
}

/// Mortality overlays + the life bar (functional-first placement;
/// the faithful HUD layout is the banked UI/UX track). `blink`
/// drives the dead-screen respawn prompt; `grace_meter` opts into
/// the (unfaithful) spawn-grace strip.
pub fn vitals_quads(
    v: &PlayerVitals,
    w: f32,
    h: f32,
    blink: bool,
    grace_meter: bool,
) -> Vec<UiQuad> {
    let mut quads = Vec::new();
    let scale = HudFrame::new(w, h).s.max(1.0);
    let bw = w * 0.25;
    let y = h - 26.0 * scale;
    // Spawn-grace shimmer: a thin white strip draining bottom-center
    // (deliberate: no faithful equivalent — retail shows nothing for
    // grace; behind `render.debug.grace_meter`, a debug cue not a
    // default overlay).
    if grace_meter && v.grace > 0 && v.state == LifeState::Alive {
        quads.push(solid(
            [
                (w - bw) / 2.0,
                y - 3.0 * scale,
                bw * (v.grace as f32 / 100.0).min(1.0),
                2.0 * scale,
            ],
            [1.0, 1.0, 1.0, 0.8],
        ));
    }
    // Castle under attack: the faithful cue is the wizard strip's
    // alert marble [55] (hud_quads slot A), not anything drawn here.
    // The red hit flash (sub_44BE0(2) — palette row 2 in retail).
    if v.hit_flash > 0 && v.state == LifeState::Alive {
        let a = 0.08 * v.hit_flash as f32;
        quads.push(solid_screen([0.0, 0.0, w, h], [0.8, 0.05, 0.05, a]));
    }
    // The palette-row flash (sub_44BE0 → +152). Row 3 = Global Death's
    // detonation: retail pushes red +48 and saturates blue while
    // leaving green alone, so the whole frame washes violet for one
    // frame and fades home. Row 2 is the hit flash above; rows 6/7 are
    // drawn elsewhere or unported.
    if v.pal_flash.0 == 3 && v.state == LifeState::Alive {
        let a = 0.09 * v.pal_flash.1 as f32;
        quads.push(solid_screen([0.0, 0.0, w, h], [0.55, 0.12, 0.95, a]));
    }
    match v.state {
        // The death fall: a deepening red-out.
        LifeState::Falling => {
            quads.push(solid_screen([0.0, 0.0, w, h], [0.45, 0.03, 0.03, 0.35]));
        }
        // Dead: the grey screen (palette row 7) + a blinking center
        // strip as the Space prompt (no text renderer yet).
        LifeState::Dead => {
            quads.push(solid_screen([0.0, 0.0, w, h], [0.22, 0.22, 0.25, 0.55]));
            if blink {
                let pw = w * 0.30;
                quads.push(solid(
                    [(w - pw) / 2.0, h * 0.62, pw, 4.0 * scale],
                    [0.95, 0.95, 0.95, 0.9],
                ));
            }
        }
        LifeState::Alive => {}
    }
    quads
}

/// The aim crosshair + autoaim lock markers (SPLIT options since
/// 2026-07-23 — the caller passes `None`/empty for whichever is off):
/// `neutral` = the gameplay aim cursor (`render.preference.crosshair`
/// / C) — a black, white-edged cross at the TRUE aim point (the
/// faithful camera pitches at HALF the aim pitch, so the aim is never
/// screen center; under enhanced thrust it rides the chase-steering
/// desired heading). `locks` = the debug predictor
/// (`render.debug.autoaim_hints`): per-hand markers on the target
/// each hand's equipped spell would acquire this instant
/// (`World::aim_preview`): left hand = an upright `+`, right hand = a
/// diagonal `×`, cores blinking gently red while locked (both shapes
/// compose when the hands lock the same target). Acquisition ≠ hit —
/// homing yaw is authentically capped at 5/tick, so the marker shows
/// what the shot will CHASE, not what it will catch.
pub fn crosshair_quads(
    quads: &mut Vec<UiQuad>,
    w: f32,
    h: f32,
    neutral: Option<(f32, f32)>,
    locks: [Option<(f32, f32)>; 2],
    blink: f32,
) {
    let s = HudFrame::new(w, h).s;
    let red = [0.30 + 0.70 * blink.clamp(0.0, 1.0), 0.02, 0.02, 1.0];
    if let Some((cx, cy)) = neutral {
        plus_glyph(quads, cx, cy, s, [0.0, 0.0, 0.0, 1.0]);
    }
    if let Some((cx, cy)) = locks[0] {
        plus_glyph(quads, cx, cy, s, red);
    }
    if let Some((cx, cy)) = locks[1] {
        diag_glyph(quads, cx, cy, s, red);
    }
}

/// White edge under a colored core, both crosshair glyph shapes.
const GLYPH_EDGE: [f32; 4] = [1.0, 1.0, 1.0, 0.85];

/// Upright `+`: 16-native-px arms, white-edged.
fn plus_glyph(quads: &mut Vec<UiQuad>, cx: f32, cy: f32, s: f32, core: [f32; 4]) {
    quads.push(solid(
        [cx - 8.0 * s, cy - 2.0 * s, 16.0 * s, 4.0 * s],
        GLYPH_EDGE,
    ));
    quads.push(solid(
        [cx - 2.0 * s, cy - 8.0 * s, 4.0 * s, 16.0 * s],
        GLYPH_EDGE,
    ));
    quads.push(solid([cx - 7.0 * s, cy - 1.0 * s, 14.0 * s, 2.0 * s], core));
    quads.push(solid([cx - 1.0 * s, cy - 7.0 * s, 2.0 * s, 14.0 * s], core));
}

/// Diagonal `×`: chunky pixel diagonals (axis-aligned quads only),
/// white-edged; edges first so no core is covered by a neighbor.
fn diag_glyph(quads: &mut Vec<UiQuad>, cx: f32, cy: f32, s: f32, core: [f32; 4]) {
    const ARM: [f32; 7] = [-6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0];
    for i in ARM {
        quads.push(solid(
            [cx + (i - 2.0) * s, cy + (i - 2.0) * s, 4.0 * s, 4.0 * s],
            GLYPH_EDGE,
        ));
        quads.push(solid(
            [cx + (i - 2.0) * s, cy - (i + 2.0) * s, 4.0 * s, 4.0 * s],
            GLYPH_EDGE,
        ));
    }
    for i in ARM {
        quads.push(solid(
            [cx + (i - 1.0) * s, cy + (i - 1.0) * s, 2.0 * s, 2.0 * s],
            core,
        ));
        quads.push(solid(
            [cx + (i - 1.0) * s, cy - (i + 1.0) * s, 2.0 * s, 2.0 * s],
            core,
        ));
    }
}

// ================= The CTRL-hold spell-selector pane ==================
// MC2's faithful selection surface (docked bottom pane while CTRL is
// held), verbatim trace: docs/traces/mc2-spell-selector-ui.md. The
// widget is game-parametric so MC1 can opt in via
// `spell_selector = mc2 | mc1+mc2` (authenticity-matrix alternate).

/// MC2 selector sprite ids (remc2 GameBitmapIndexes.h; trace §2.6).
const MC2_SPR_TILE_BAR: usize = 87; // hovered box's live shot-meter tile
const MC2_SPR_EDGE: usize = 88; // pane end frame column
const MC2_SPR_BOX: usize = 89; // grid box, affordable
const MC2_SPR_BOX_FRAME: usize = 90; // hovered-slot highlight frame
const MC2_SPR_BOX_DARK: usize = 91; // grid box, unaffordable
const MC2_SPR_ICON_SMALL: usize = 97; // + spell (0..25): the grid icon

/// UI-atlas sprite id of a spell's icon in the running game's atlas:
/// MC1 (both tilesets) bakes the 24 book icons at `[6 + spell]` (the
/// book/HUD entry map), MC2 the 26 selector grid icons at
/// `[97 + spell]`. Feeds the expose-jar-spells debug markers.
pub fn spell_icon_sprite(game: mgc_sim::ids::GameId, spell: u8) -> Option<usize> {
    use mgc_sim::ids::GameId;
    match game {
        GameId::Mc1 | GameId::Mc1Hw => {
            (usize::from(spell) < SPELL_COUNT).then(|| 6 + usize::from(spell))
        }
        GameId::Mc2 => (spell < 26).then(|| MC2_SPR_ICON_SMALL + usize::from(spell)),
    }
}
const MC2_SPR_TAG_LEFT: usize = 149; // bound-to-LMB corner tag
const MC2_SPR_TAG_RIGHT: usize = 150; // bound-to-RMB corner tag
const MC2_SPR_SUB_OK: usize = 161; // level box, unlocked + affordable
const MC2_SPR_SUB_DARK: usize = 162; // level box, unlocked + unaffordable
const MC2_SPR_SUB_LOCKED: usize = 163; // level box, locked (drawn empty)
const MC2_SPR_SUB_GOLD: usize = 164; // chosen-level gold frame
// The textbox frame 9-slice (DrawTextboxFrame_89690 GU:3571-3607).
const MC2_SPR_FRAME_CORNER: usize = 171;
const MC2_SPR_FRAME_SIDE: usize = 172; // left/right column tile
const MC2_SPR_FRAME_RUN: usize = 173; // top/bottom run tile
const MC2_SPR_SUB_NUM: usize = 165; // + level: the "1/2/3" number bg
const MC2_SPR_SUB_ICON: usize = 179; // + 3*spell + level: per-level icon

/// MC2 spell names by `spell_t` model index (trace §3; grid order =
/// identity). Console labels — MC2 shows hint text in-game.
pub const MC2_SPELL_NAMES: [&str; 26] = [
    "Fireball",
    "Possession",
    "Castle",
    "Speed Up",
    "Metamorph",
    "Heal",
    "Shield",
    "Lightning",
    "Rebound",
    "Meteor",
    "Teleport",
    "Invisible",
    "Beyond Sight",
    "Steal Mana",
    "Duel",
    "Tremor",
    "Crater",
    "Earthquake",
    "Volcano",
    "Summon Army",
    "Gravity Well",
    "Whirlwind",
    "Fool's Mana",
    "Magic Mine",
    "Alliance",
    "Cave-In",
];

/// MC2 grid order = identity over `spell_t` (`spellIndex_D94FF`,
/// trace §0/§3): row 0 = 0..12, row 1 = 13..25.
const MC2_GRID_ORDER: [u8; 26] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
];

/// The pane's per-game shape: grid dimensions, slot→spell order,
/// level depth, and which art set draws it.
pub struct SelectorPane {
    /// Draw with the MC2 sprite set (87..256) vs the MC1 book set.
    mc2_art: bool,
    pub cols: usize,
    /// Grid slot → pane spell id.
    pub order: &'static [u8],
    /// Levels per spell (MC2 = 3 with the flyout; MC1 = 1, no flyout).
    pub levels: u8,
}

impl SelectorPane {
    /// MC1 adaptation: 24 spells, 2×12, book display order, MC1 art,
    /// single-level (no flyout). Cells shrink from the book's 64-wide
    /// slab to fit 12 across 640 native px.
    pub fn mc1() -> Self {
        SelectorPane {
            mc2_art: false,
            cols: 12,
            order: &DISPLAY_ORDER,
            levels: 1,
        }
    }

    /// The faithful MC2 pane: 26 spells, 2×13 in `spell_t` order,
    /// 3-level flyout.
    pub fn mc2() -> Self {
        SelectorPane {
            mc2_art: true,
            cols: 13,
            order: &MC2_GRID_ORDER,
            levels: 3,
        }
    }

    pub fn spell_count(&self) -> usize {
        self.order.len()
    }
}

/// Per-frame pane inputs, indexed by PANE SPELL ID (0..spell_count).
pub struct SelectorView<'a> {
    /// Possessed (drawn full, selectable).
    pub owned: &'a [bool],
    /// Affordable/enabled at the SELECTED tier (bright box);
    /// owned && !castable = dark box + ghost icon — the
    /// grey-not-disable rule (trace §7). Retail law = `canSummon`
    /// (EF:22503-08): the tier's castle-pool prerequisite is met.
    pub castable: &'a [bool],
    /// The same law PER TIER (`canSubSummon`, EF:22602-08) — the
    /// flyout's per-level dark tiles. All-true under the dev
    /// instrument and on MC1 (mirrors `castable`).
    pub castable_tier: &'a [[bool; 3]],
    /// The persistent per-spell selected level (`array_0x437`,
    /// trace §4.3).
    pub selected_level: &'a [u8],
    /// Max unlocked level per spell (`SpellLevels_0x41D`); the flyout
    /// shows locked levels as empty boxes and clamps selection.
    pub max_level: &'a [u8],
    /// Pane spell bound to each hand (corner tags; [left, right]).
    pub bound: [Option<u8>; 2],
    /// Cycle-ring membership per spell (`array_0x3B5`: 0 = none,
    /// 1 = the LEFT-button ring, 2 = RIGHT) — retail keys the hover
    /// corner tag on THIS (EF:22546-53), not on the equipped hand.
    pub ring: &'a [u8],
    /// Player mana + per-shot stand-in cost (the hovered box's live
    /// shot meter).
    pub mana: u32,
    pub cost: &'a [u32],
    /// Per-tier one-cast cost (`GetSpellManaCost_6D710` per tier,
    /// EF:22609) — the flyout's broke test against `mana`; `cost`
    /// above carries the SELECTED tier only.
    pub cost_tier: &'a [[u32; 3]],
    /// Effective spell XP (banked + volatile) — the flyout's per-tier
    /// unlock-progress bar (EF:22633-71). Empty on MC1.
    pub xp: &'a [i32],
    /// Expiry-blink eligibility per spell (the hand panels' law —
    /// EF:22493-99 runs the same gate on the pane cell).
    pub expiring: &'a [bool],
    /// The blink bank's index-[1] phase (Turn & 1); false = the
    /// skip frame for expiring cells.
    pub blink: bool,
    /// Per-tier `xpos1` thresholds (single-player ladder), same bar.
    pub xpos: &'a [[i32; 3]],
}

/// What the cursor is over, for the app's click/commit logic.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectorHover {
    /// Grid slot under the cursor (a valid slot, not necessarily
    /// owned).
    pub slot: Option<usize>,
    /// Level under the cursor in the visible flyout, already clamped
    /// to the anchor spell's max unlocked level.
    pub level: Option<u8>,
}

/// One grid cell's screen rect. Native geometry per the trace §2.2:
/// `x = edgeW + col·boxW`, `y = (480 − 2·boxH) + row·boxH`.
///
/// The pane is the "largely fixed" surface of the layout law: its
/// cells ARE the spell buttons, so they take the uniform scale and
/// nothing else. It stays BOTTOM-anchored (its retail home) and
/// CENTER-anchored horizontally — the strip is a symmetric band across
/// the bottom in retail, so at a wider-than-4:3 screen the slack
/// splits evenly to either side rather than piling up on one edge.
/// `sx`/`sy` are both the uniform factor, kept as two fields so the
/// per-cell offset arithmetic below reads unchanged.
struct PaneGeom {
    edge_w: f32,
    box_w: f32,
    box_h: f32,
    sub_w: f32,
    sub_h: f32,
    f: HudFrame,
    sx: f32,
    sy: f32,
}

impl PaneGeom {
    fn new(assets: &UiAssets, pane: &SelectorPane, w: f32, h: f32) -> Self {
        let f = HudFrame::new(w, h);
        let (sx, sy) = (f.s, f.s);
        if pane.mc2_art {
            let (bw, bh) = assets.sprite_dims(MC2_SPR_BOX).unwrap_or((48.0, 40.0));
            let (ew, _) = assets.sprite_dims(MC2_SPR_EDGE).unwrap_or((8.0, 80.0));
            let (sw, sh) = assets
                .sprite_dims(MC2_SPR_SUB_LOCKED)
                .unwrap_or((48.0, 36.0));
            PaneGeom {
                edge_w: ew,
                box_w: bw,
                box_h: bh,
                sub_w: sw,
                sub_h: sh,
                f,
                sx,
                sy,
            }
        } else {
            // MC1 adaptation: 12 cells across 640 with slim margins in
            // place of the MC2 edge frames; the book's 64×37 slab
            // shrinks to 52 wide (icons scale with the cell).
            PaneGeom {
                edge_w: 8.0,
                box_w: 52.0,
                box_h: 37.0,
                sub_w: 52.0,
                sub_h: 36.0,
                f,
                sx,
                sy,
            }
        }
    }

    /// Native y of the grid's top row.
    fn grid_top(&self) -> f32 {
        480.0 - 2.0 * self.box_h
    }

    /// Grid slot rect in screen px.
    fn cell(&self, slot: usize, cols: usize) -> [f32; 4] {
        let (col, row) = ((slot % cols) as f32, (slot / cols) as f32);
        [
            self.f.cx(self.edge_w + col * self.box_w),
            self.f.by(self.grid_top() + row * self.box_h),
            self.f.len(self.box_w),
            self.f.len(self.box_h),
        ]
    }

    /// The flyout row's native x origin for an anchor slot: centred
    /// over the slot's column, clamped to the pane (trace §4.1).
    fn submenu_x(&self, slot: usize, cols: usize, levels: f32) -> f32 {
        let cx = self.edge_w + (slot % cols) as f32 * self.box_w + self.box_w / 2.0;
        (cx - levels * self.sub_w / 2.0).clamp(0.0, 640.0 - levels * self.sub_w)
    }

    /// The flyout box rect (screen px) for level `l` of an anchored
    /// flyout ("directly above the grid's top row", trace §4.1).
    fn sub_cell(&self, slot: usize, cols: usize, levels: f32, l: u8) -> [f32; 4] {
        let x0 = self.submenu_x(slot, cols, levels);
        [
            self.f.cx(x0 + l as f32 * self.sub_w),
            self.f.by(self.grid_top() - self.sub_h),
            self.f.len(self.sub_w),
            self.f.len(self.sub_h),
        ]
    }
}

pub fn rect_hit(r: [f32; 4], c: (f32, f32)) -> bool {
    c.0 >= r[0] && c.0 < r[0] + r[2] && c.1 >= r[1] && c.1 < r[1] + r[3]
}

/// A corner-notch TRIANGLE — the MC1-art pane's stand-in for MC2's
/// corner-tag sprites: the old 6×6 tab square slashed on its
/// diagonal so it points INTO the top corner it sits in (player
/// direction). Built from one-native-pixel row slivers; the quad
/// batch has no triangle primitive. `x` = the tab area's LEFT edge,
/// `right` = right-align the shrinking rows (the top-right corner).
fn corner_tri(quads: &mut Vec<UiQuad>, x: f32, y: f32, sx: f32, sy: f32, right: bool, t: [f32; 4]) {
    const N: usize = 6;
    for i in 0..N {
        let w = (N - i) as f32 * sx;
        let rx = if right { x + i as f32 * sx } else { x };
        quads.push(solid([rx, y + i as f32 * sy, w, sy], t));
    }
}

/// Ring outline (the MC1-art hover feedback, book idiom).
fn ring(quads: &mut Vec<UiQuad>, f: [f32; 4], t: [f32; 4]) {
    quads.push(solid([f[0], f[1], f[2], 2.0], t));
    quads.push(solid([f[0], f[1] + f[3] - 2.0, f[2], 2.0], t));
    quads.push(solid([f[0], f[1], 2.0, f[3]], t));
    quads.push(solid([f[0] + f[2] - 2.0, f[1], 2.0, f[3]], t));
}

/// Draw the selector pane + hit-test the cursor. `drag_slot` = the
/// grid slot anchored by a held click (the flyout live-tracks the
/// cursor and the anchor stays put, trace §1.3); None = plain hover.
/// Returns the quads and what the cursor is over.
pub fn selector_quads(
    assets: &UiAssets,
    pane: &SelectorPane,
    view: &SelectorView,
    w: f32,
    h: f32,
    cursor: (f32, f32),
    drag_slot: Option<usize>,
) -> (Vec<UiQuad>, SelectorHover) {
    let g = PaneGeom::new(assets, pane, w, h);
    let n = pane.spell_count();
    let mut quads = Vec::with_capacity(n * 4 + 16);
    let mut hover = SelectorHover::default();

    // Hit-test the grid (hover is display state; clicks are the app's).
    for slot in 0..n {
        if rect_hit(g.cell(slot, pane.cols), cursor) {
            hover.slot = Some(slot);
        }
    }

    // The flyout's anchor: a live drag wins; otherwise the hovered
    // OWNED spell (the original draws it on plain hover too, §4).
    let anchor = drag_slot.or_else(|| {
        hover
            .slot
            .filter(|&s| pane.levels > 1 && view.owned[pane.order[s] as usize])
    });

    // --- The 2×13 (or 2×12) grid ---
    for slot in 0..n {
        let spell = pane.order[slot] as usize;
        let cell = g.cell(slot, pane.cols);
        let owned = view.owned[spell];
        let castable = owned && view.castable[spell];
        let hovered = hover.slot == Some(slot) || drag_slot == Some(slot);
        // THE EXPIRY BLINK on the pane cell (EF:22493-99 — the same
        // flag-4 + last-window gate as the hand panel skips the whole
        // cell body on odd turns). The hit-test above stays live:
        // retail never keys input to the draw phase.
        if owned && view.expiring.get(spell).copied().unwrap_or(false) && !view.blink {
            continue;
        }

        if pane.mc2_art {
            // One pre-composited tile per box state (EF:22468-22544):
            // castable(89+icon), hovered shot-meter(87+icon),
            // owned-unaffordable ghost(91+blended icon). The bake made
            // the treatment choice; the draw is a single quad.
            //
            // NOT POSSESSED = the plain empty box, exactly like retail
            // (EF:22557: SPELL_ICON_PANEL only; the grey 0xA6 relief
            // is retail's "learnable/present" hint, gated on the
            // learn flags 0x3E9/0x403 we don't model yet). The relief
            // tile stays baked (variant 3) for a future opt-in.
            if !owned {
                push_opt(
                    &mut quads,
                    assets.sprite_quad_rect_tint(MC2_SPR_BOX, cell, WHITE),
                );
                // Future learnable-hint opt-in (unfaithful-proactive):
                // if let Some(uv) = assets.pane_tile(spell, 3) {
                //     quads.push(UiQuad { rect: snap(cell), uv, tint: WHITE });
                // }
                continue;
            }
            let variant = if !castable {
                2
            } else if hovered {
                1
            } else {
                0
            };
            if let Some(uv) = assets.pane_tile(spell, variant) {
                quads.push(UiQuad {
                    rect: snap(cell),
                    uv,
                    tint: WHITE,
                });
            }
            // Shot meter on the hovered castable box, verbatim
            // geometry (EF:22516-22529): fill line at (+6,+28),
            // 36 px ruler, 4 tall; one 2×2 dot per whole affordable
            // cast packed column-major (2 rows), max 36.
            if hovered && castable {
                let cost = view.cost[spell].max(1);
                let frac = (view.mana % cost) as f32 / cost as f32;
                let (mx, my) = (cell[0] + 6.0 * g.sx, cell[1] + 28.0 * g.sy);
                quads.push(solid([mx, my, 36.0 * g.sx * frac, 4.0 * g.sy], METER_GREY));
                let casts = ((view.mana / cost) as usize).min(36);
                for d in 0..casts {
                    let (col, row) = ((d / 2) as f32, (d % 2) as f32);
                    quads.push(solid(
                        [
                            mx + col * 2.0 * g.sx,
                            my + row * 2.0 * g.sy,
                            2.0 * g.sx,
                            2.0 * g.sy,
                        ],
                        MANA_WHITE,
                    ));
                }
            }
            // L/R mouse-binding corner tags, as transparent blits
            // (EF:22546-22553); right tag at +boxW−tagLeftW
            // (EF:22452). The alpha tint stands in for the blend.
            // Keyed on CYCLE-RING membership (`array_0x3B5`,
            // EF:22547) plus the equipped hand, and drawn on EVERY
            // box, not only the hovered one — the queued sets must
            // read at a glance (player-ruled; the decompile's
            // `spellOnCursor_50` hover gate notwithstanding — see
            // DEVIATIONS).
            {
                let ring = view.ring.get(spell).copied().unwrap_or(0);
                let tag_tint = [1.0, 1.0, 1.0, 0.75];
                let su = g.sx;
                if ring == 1 || view.bound[0] == Some(spell as u8) {
                    push_opt(
                        &mut quads,
                        assets.sprite_quad_tint(MC2_SPR_TAG_LEFT, cell[0], cell[1], su, tag_tint),
                    );
                }
                if ring == 2 || view.bound[1] == Some(spell as u8) {
                    let tag_w = assets
                        .sprite_dims(MC2_SPR_TAG_LEFT)
                        .map_or(14.0, |(w, _)| w);
                    push_opt(
                        &mut quads,
                        assets.sprite_quad_tint(
                            MC2_SPR_TAG_RIGHT,
                            cell[0] + (g.box_w - tag_w) * g.sx,
                            cell[1],
                            su,
                            tag_tint,
                        ),
                    );
                }
            }
        } else {
            // MC1 art: the book's slab + icon idiom in pane cells.
            push_opt(
                &mut quads,
                assets.sprite_quad_rect_tint(SPR_SLOT_BG as usize, cell, SLAB_DIM),
            );
            let icon = SPR_SPELL_ICON + spell;
            let irect = [
                cell[0],
                cell[1],
                cell[2] * (ICON_W / 64.0),
                cell[3] * (ICON_H / 37.0),
            ];
            push_opt(
                &mut quads,
                if owned {
                    assets.sprite_quad_rect_tint(icon, irect, WHITE)
                } else {
                    assets.sprite_quad_rect_mask(icon, irect, UNOWNED_MASK)
                },
            );
            if owned && !castable {
                quads.push(solid(cell, LOCKED_WASH));
            }
            // Bound-hand corner marks (translucent white triangles
            // pointing into their corner — the MC1-art stand-in for
            // MC2's corner-tag sprites, player-tuned). Cycle-ring
            // members wear the same notch dimmed — the queued set
            // reads at a glance, the equipped spell stays brighter.
            let tab = [1.0, 1.0, 1.0, 0.5];
            let dim = [1.0, 1.0, 1.0, 0.25];
            let member = view.ring.get(spell).copied().unwrap_or(0);
            if view.bound[0] == Some(spell as u8) || member == 1 {
                let t = if view.bound[0] == Some(spell as u8) {
                    tab
                } else {
                    dim
                };
                corner_tri(&mut quads, cell[0], cell[1], g.sx, g.sy, false, t);
            }
            if view.bound[1] == Some(spell as u8) || member == 2 {
                let t = if view.bound[1] == Some(spell as u8) {
                    tab
                } else {
                    dim
                };
                corner_tri(
                    &mut quads,
                    cell[0] + cell[2] - 6.0 * g.sx,
                    cell[1],
                    g.sx,
                    g.sy,
                    true,
                    t,
                );
            }
            if hovered {
                ring(&mut quads, cell, [0.9, 0.85, 0.5, 0.9]);
            }
            // Shot meter on the hovered castable box (book meter
            // idiom).
            if hovered && castable {
                let cost = view.cost[spell].max(1);
                let frac = (view.mana % cost) as f32 / cost as f32;
                let my = cell[1] + cell[3] - 5.0 * g.sy;
                quads.push(solid(
                    [
                        cell[0] + 3.0 * g.sx,
                        my,
                        (cell[2] - 6.0 * g.sx) * frac,
                        2.0 * g.sy,
                    ],
                    METER_GREY,
                ));
            }
        }
    }

    // --- Pane end frames + hovered-slot highlight (MC2 art) ---
    if pane.mc2_art {
        let top = g.f.by(g.grid_top());
        push_opt(
            &mut quads,
            assets.sprite_quad_rect_tint(
                MC2_SPR_EDGE,
                [g.f.cx(0.0), top, g.edge_w * g.sx, 2.0 * g.box_h * g.sy],
                WHITE,
            ),
        );
        push_opt(
            &mut quads,
            assets.sprite_quad_rect_tint(
                MC2_SPR_EDGE,
                [
                    g.f.cx(g.edge_w + pane.cols as f32 * g.box_w),
                    top,
                    g.edge_w * g.sx,
                    2.0 * g.box_h * g.sy,
                ],
                WHITE,
            ),
        );
        if let Some(slot) = drag_slot.or(hover.slot) {
            push_opt(
                &mut quads,
                assets.sprite_quad_rect_tint(MC2_SPR_BOX_FRAME, g.cell(slot, pane.cols), WHITE),
            );
        }
    }

    // --- The 3-level flyout (trace §4) ---
    if let Some(slot) = anchor {
        let spell = pane.order[slot] as usize;
        let max = view.max_level[spell].min(pane.levels - 1);
        // Level under the cursor, clamped to the unlocked max
        // (SelectSpell_6D4F0's clamp, trace §4.3).
        let row = g.sub_cell(slot, pane.cols, pane.levels as f32, 0);
        if cursor.1 >= row[1] && cursor.1 < row[1] + row[3] {
            let l = ((cursor.0 - row[0]) / row[2].max(1.0)).floor();
            if l >= 0.0 && l < pane.levels as f32 {
                hover.level = Some((l as u8).min(max));
            }
        }
        // The gold frame sits on the live-tracked level during a drag,
        // else on the stored selection.
        let chosen = if drag_slot.is_some() {
            hover.level.unwrap_or(view.selected_level[spell].min(max))
        } else {
            view.selected_level[spell].min(max)
        };
        for l in 0..pane.levels {
            let cell = g.sub_cell(slot, pane.cols, pane.levels as f32, l);
            if l > max {
                // Locked: the empty box, no icon (trace §4.2).
                push_opt(
                    &mut quads,
                    assets.sprite_quad_rect_tint(MC2_SPR_SUB_LOCKED, cell, WHITE),
                );
                continue;
            }
            // The retail THREE-STATE flyout tile (EF:22611-28): the
            // castle-pool prerequisite unmet (`canSubSummon`,
            // EF:22602-08) = dark frame + LUT-ghosted icon; pool ok
            // but the hand can't pay one cast (`manaPart = mana /
            // cost`, EF:22609/:22618) = dark frame with the icon
            // still LIT; else the lit tile. The broke term was a
            // known approximation until player retail-verification
            // (2026-08-21) promoted it.
            let li = (l as usize).min(2);
            let pool_ok = view.castable_tier.get(spell).is_some_and(|t| t[li]);
            let variant = if !pool_ok {
                1
            } else if view.cost_tier.get(spell).is_some_and(|c| view.mana < c[li]) {
                2
            } else {
                0
            };
            if let Some(uv) = assets.sub_tile(spell, l as usize, variant) {
                quads.push(UiQuad {
                    rect: snap(cell),
                    uv,
                    tint: WHITE,
                });
            }
            if l == chosen {
                push_opt(
                    &mut quads,
                    assets.sprite_quad_rect_tint(MC2_SPR_SUB_GOLD, cell, WHITE),
                );
            }
            // The per-tier XP progress bar (EF:22633-71): unlocked
            // boxes below the third draw a 54×2 line at (+6,+28) —
            // background CLRD 0, fill CLRD 3840 (0xF00 red). The
            // in-progress level (l == max) fills
            // (xp − xpos1[l]) / (xpos1[l+1] − xpos1[l]); levels
            // already passed draw the full bar.
            if l + 1 < pane.levels {
                let (Some(&xp), Some(lad)) = (view.xp.get(spell), view.xpos.get(spell)) else {
                    continue;
                };
                let bar = [
                    cell[0] + 6.0 * g.sx,
                    cell[1] + 28.0 * g.sy,
                    54.0 * g.sx,
                    2.0 * g.sy,
                ];
                quads.push(solid(bar, XP_BAR_BG));
                let frac = if l < max {
                    1.0
                } else {
                    let (x0, x1) = (lad[l as usize], lad[l as usize + 1]);
                    if x1 > x0 {
                        ((xp - x0) as f32 / (x1 - x0) as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                };
                if frac > 0.0 {
                    quads.push(solid([bar[0], bar[1], bar[2] * frac, bar[3]], XP_BAR_RED));
                }
            }
        }
    }

    (quads, hover)
}

#[cfg(test)]
mod tests {
    /// Cropping must CUT a partial sprite, not squash it: the visible
    /// part keeps its scale and the uv window follows the rect, so the
    /// texels shown are exactly the ones inside the viewport.
    #[test]
    fn clip_crops_rather_than_squashing() {
        use mgc_render::UiQuad;
        let quad = |rect: [f32; 4]| UiQuad {
            rect,
            uv: [100.0, 200.0, 10.0, 20.0],
            tint: [1.0; 4],
        };
        // Fully inside: untouched. Fully outside: dropped.
        let mut q = vec![
            quad([5.0, 5.0, 10.0, 20.0]),
            quad([-40.0, 0.0, 10.0, 20.0]),
            quad([700.0, 0.0, 10.0, 20.0]),
        ];
        super::clip_quads(&mut q, 640.0, 480.0);
        assert_eq!(q.len(), 1, "only the inside quad survives");
        assert_eq!(q[0].uv, [100.0, 200.0, 10.0, 20.0], "inside quad untouched");

        // Half off the left edge: half the width, half the texels, and
        // the uv window advances by the cut.
        let mut q = vec![quad([-5.0, 0.0, 10.0, 20.0])];
        super::clip_quads(&mut q, 640.0, 480.0);
        assert_eq!(q[0].rect, [0.0, 0.0, 5.0, 20.0]);
        assert_eq!(q[0].uv, [105.0, 200.0, 5.0, 20.0]);
        // The scale is preserved — that is what "crop, not squash"
        // means: texels-per-pixel is the same before and after.
        assert_eq!(q[0].uv[2] / q[0].rect[2], 10.0 / 10.0);

        // Off the bottom-right: cut on both axes, origin unchanged.
        let mut q = vec![quad([635.0, 470.0, 10.0, 20.0])];
        super::clip_quads(&mut q, 640.0, 480.0);
        assert_eq!(q[0].rect, [635.0, 470.0, 5.0, 10.0]);
        assert_eq!(q[0].uv, [100.0, 200.0, 5.0, 10.0]);
    }

    /// A 4:3 window has no bars; a wider one bars left/right and a
    /// narrower one top/bottom. In every case the picture is centred
    /// and the cursor round-trips.
    #[test]
    fn letterbox_centres_and_round_trips() {
        let (w, h) = (640.0, 480.0);
        for size in [
            (640.0, 480.0),
            (1280.0, 960.0),
            (1600.0, 900.0),
            (800.0, 900.0),
        ] {
            let (scale, ox, oy) = super::letterbox(size, w, h);
            // The picture fits, and the bars are even on both sides.
            assert!(
                w * scale <= size.0 + 0.5 && h * scale <= size.1 + 0.5,
                "{size:?}"
            );
            assert!((ox * 2.0 - (size.0 - w * scale)).abs() <= 1.0, "{size:?} x");
            assert!((oy * 2.0 - (size.1 - h * scale)).abs() <= 1.0, "{size:?} y");
            // A point in the middle of the picture maps back to itself.
            let mid = (ox + w * scale / 2.0, oy + h * scale / 2.0);
            let back = super::unletterbox(mid, size, w, h);
            assert!(
                (back.0 - w / 2.0).abs() < 1.0 && (back.1 - h / 2.0).abs() < 1.0,
                "{size:?}"
            );
        }
    }

    /// The map's edge-scroll reads "at or BEYOND the picture edge", so
    /// the whole letterbox bar scrolls rather than only the boundary
    /// pixel — otherwise the confined pointer has a dead strip where it
    /// is off the map and nothing happens. It has to hold on the
    /// barred axis whichever one that is: left/right on a wide window,
    /// top/bottom on a squashed one.
    #[test]
    fn cursor_anywhere_in_a_letterbox_bar_reads_as_beyond_the_edge() {
        let (w, h) = (640.0, 480.0);
        // Wide: bars left and right.
        let size = (1600.0, 900.0);
        let (_, ox, _) = super::letterbox(size, w, h);
        assert!(ox > 1.0, "expected side bars, got ox={ox}");
        for x in [0.0, ox / 2.0, ox - 1.0] {
            let (mx, _) = super::unletterbox((x, size.1 / 2.0), size, w, h);
            assert!(
                mx < 1.0,
                "x={x} in the left bar did not read as past the edge"
            );
        }
        for x in [size.0 - 1.0, size.0 - ox / 2.0] {
            let (mx, _) = super::unletterbox((x, size.1 / 2.0), size, w, h);
            assert!(
                mx >= 638.0,
                "x={x} in the right bar did not read as past the edge"
            );
        }
        // Squashed: bars top and bottom, same rule on y.
        let size = (800.0, 900.0);
        let (_, _, oy) = super::letterbox(size, w, h);
        assert!(oy > 1.0, "expected top/bottom bars, got oy={oy}");
        for y in [0.0, oy / 2.0, oy - 1.0] {
            let (_, my) = super::unletterbox((size.0 / 2.0, y), size, w, h);
            assert!(
                my < 1.0,
                "y={y} in the top bar did not read as past the edge"
            );
        }
        for y in [size.1 - 1.0, size.1 - oy / 2.0] {
            let (_, my) = super::unletterbox((size.0 / 2.0, y), size, w, h);
            assert!(
                my >= 478.0,
                "y={y} in the bottom bar did not read as past the edge"
            );
        }
    }

    use super::*;

    /// The invariant that makes the whole non-4:3 law safe to land: at
    /// any 4:3 size every anchor collapses onto the authored native
    /// coordinate, so nothing about the retail presentation moved.
    #[test]
    fn hud_frame_is_the_identity_at_four_by_three() {
        for &(w, h) in &[(640.0, 480.0), (1280.0, 960.0), (320.0, 240.0)] {
            let f = HudFrame::new(w, h);
            let s = w / 640.0;
            assert_eq!(f.s, s);
            for &x in &[0.0, 2.0, 384.0, 510.0, 574.0, 640.0] {
                assert_eq!(f.lx(x), x * s, "left anchor at {w}x{h}");
                assert!((f.rx(x) - x * s).abs() < 1e-3, "right anchor at {w}x{h}");
                assert!((f.cx(x) - x * s).abs() < 1e-3, "center anchor at {w}x{h}");
            }
            for &y in &[0.0, 194.0, 416.0, 480.0] {
                assert_eq!(f.ty(y), y * s);
                assert!((f.by(y) - y * s).abs() < 1e-3, "bottom anchor at {w}x{h}");
            }
        }
    }

    /// Wider than 4:3: the vertical is exact, the slack is horizontal,
    /// and it lands BETWEEN the two anchored groups — the left group
    /// still hugs x=0, the right group still hugs x=w.
    #[test]
    fn wide_screen_anchors_to_the_edges_without_stretching() {
        let (w, h) = (1920.0, 1080.0); // 16:9
        let f = HudFrame::new(w, h);
        assert_eq!(f.s, h / 480.0, "uniform scale keyed off the height");
        // Left group: the radar's 2px inset is still 2 native px in.
        assert_eq!(f.lx(2.0), 2.0 * f.s);
        // Right group: the spell-hand pair still ends 2 native px clear
        // of the RIGHT edge, mirroring the radar's inset.
        assert!((f.rx(638.0) - (w - 2.0 * f.s)).abs() < 1e-3);
        // …and the pair keeps its authored 64-native width apart.
        assert!((f.rx(574.0) - f.rx(510.0) - 64.0 * f.s).abs() < 1e-3);
        // The slack is genuinely in the middle, not eaten by a stretch.
        let gap = f.rx(510.0) - f.lx(510.0);
        assert!(gap > 0.0, "wide screens open a gap between the groups");
        assert!((gap - (w - 640.0 * f.s)).abs() < 1e-3);
    }

    /// Narrower than 4:3: the whole HUD simply shrinks to match the
    /// screen WIDTH, and the vertical slack goes to the anchors.
    #[test]
    fn narrow_screen_shrinks_to_the_width() {
        let (w, h) = (800.0, 800.0); // 1:1
        let f = HudFrame::new(w, h);
        assert_eq!(f.s, w / 640.0, "uniform scale keyed off the width");
        // Horizontal is exact: the strip still spans edge to edge.
        assert_eq!(f.lx(0.0), 0.0);
        assert!((f.rx(640.0) - w).abs() < 1e-3);
        // Vertical slack: the strip stays at the top, the selector pane
        // and the log strip stay at the bottom.
        assert_eq!(f.ty(0.0), 0.0);
        assert!((f.by(480.0) - h).abs() < 1e-3);
        assert!(f.by(416.0) > 416.0 * f.s, "bottom-anchored rows drop");
    }

    /// The spellbook is the RIGID pane: same pixel size at every
    /// aspect, always flush into the bottom-right corner.
    #[test]
    fn spellbook_is_rigid_and_corner_anchored_at_any_aspect() {
        for &(w, h) in &[(1280.0, 960.0), (1920.0, 1080.0), (800.0, 800.0)] {
            let s = HudFrame::new(w, h).s;
            let first = book_cell(w, h, 0);
            let last = book_cell(w, h, 23);
            // Cells keep their authored proportions — never stretched.
            assert!((first[2] - 64.0 * s).abs() < 1e-3, "cell w at {w}x{h}");
            assert!((first[3] - 37.0 * s).abs() < 1e-3, "cell h at {w}x{h}");
            // Last column ends flush at the right edge.
            assert!((last[0] + last[2] - w).abs() < 1e-3, "flush at {w}x{h}");
            // Grid bottom leaves exactly the 64-native log strip.
            assert!(
                (last[1] + last[3] - (h - 64.0 * s)).abs() < 1e-3,
                "log strip at {w}x{h}"
            );
        }
    }

    #[test]
    fn spellbook_grid_is_tightly_packed_at_native_coords() {
        // At native 640×480 the 24 cells sit at (384,194)+(col·64,row·37),
        // 4 cols × 6 rows, with NO gaps — the faithful spellbook packing.
        // Anchors + step.
        let (w, h) = (640.0, 480.0);
        // First cell at the grid origin.
        assert_eq!(book_cell(w, h, 0), [384.0, 194.0, 64.0, 37.0]);
        // End of row 0 (col 3): x = 384 + 3·64 = 576, right edge = 640.
        let c3 = book_cell(w, h, 3);
        assert_eq!(c3, [576.0, 194.0, 64.0, 37.0]);
        assert_eq!(c3[0] + c3[2], 640.0, "row fills to the screen edge");
        // Wraps to the next row at col 0 (k=4): x back to 384, y += 37.
        assert_eq!(book_cell(w, h, 4), [384.0, 231.0, 64.0, 37.0]);
        // Last cell (k=23 = col 3, row 5): bottom edge = 194 + 6·37 = 416.
        let last = book_cell(w, h, 23);
        assert_eq!(last, [576.0, 379.0, 64.0, 37.0]);
        assert_eq!(last[1] + last[3], 416.0, "grid bottom = spellbook base");
        // Tightly packed: adjacent cells share an edge (no gap).
        let a = book_cell(w, h, 0);
        let b = book_cell(w, h, 1);
        assert_eq!(a[0] + a[2], b[0], "columns are gapless");
    }

    #[test]
    fn spellbook_grid_scales_with_resolution() {
        // Cells scale by w/640, h/480 so the layout is resolution-parametric.
        let cell = book_cell(1280.0, 960.0, 0);
        assert_eq!(cell, [768.0, 388.0, 128.0, 74.0], "2× native");
    }
}
