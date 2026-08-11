//! Level entities -> billboards.
//!
//! Two paths: with a live [`mgc_sim::engine::world::World`] (all games
//! since Phase 3.5), [`billboards_from_poses`] consumes the sim's
//! pose snapshot — sprite types, spawn facing and jitter come from
//! the ported spawn handlers' per-event LCG (byte-faithful), and
//! positions move with the mob tick; each pose resolves through its
//! game's own sprite table ([`resolve_pose_sprite`]). The static
//! [`billboards`] path resolves THING records through the MC1
//! (class, model) -> type-index mapping with a per-slot LCG
//! approximation — kept for `--no-terrain-features` comparison
//! renders.

use mgc_formats::{Thing, ThingKind};
use mgc_render::{Billboard, BoltSegment, FireParticle, HealthBar};
use mgc_sim::engine::features::BoltStrike;
use mgc_sim::engine::world::{BlastView, LivePose};
use mgc_sim::ids::GameId;
use mgc_sim::mc1::entities::{Mc1TypePick, SpawnRng, mc1_entity_parts, mc1_entity_type};
use mgc_sim::mc1::sprite_stats::SPRITE_STATS;
use mgc_sim::{HEIGHT_SCALE, MAP_TILES};

/// Engine fixed-point units per tile.
const UNITS_PER_TILE: f32 = 256.0;

/// A live pose's sprite row resolved under the game's own table.
struct PoseSprite {
    sprite_base: u16,
    draw_type: u8,
    /// Billboard height in tiles.
    world_h: f32,
    /// Billboard width in tiles (health-bar sizing; the renderer
    /// derives its own draw width from the frame's pixel aspect).
    world_w: f32,
}

/// MC1/HW: `type_index` = the sprite-STATS row — explicit engine-unit
/// width/height (a 0 height derives from width and the base sprite's
/// pixel aspect, the original's load-time fixup).
///
/// MC2: `type_index` = the sprite-PARAM row (`particlesParameters`,
/// the entity's +90). `word_0` is the TMAPS sprite base;
/// `rot_speed_8` IS the world height in engine units — the renderer
/// draws `projScale * rotSpeed_8 / depth` px tall and re-derives
/// width from the frame's pixel aspect (remc2
/// GameRenderOriginal.cpp:2192-98; our renderer does the same with
/// `world_h`). A 0 height derives from `speed_6` and the pixel
/// aspect, mirroring the load-time cross-fill
/// (EventsFunctions.cpp:44895-903). The draw type is NOT the table's
/// `byte_12` — the loader overwrites it from the TMAPS entry header
/// byte (payload[1] = the flags high byte, :44906), which the bake
/// preserves in `SpriteEntry.flags`.
fn resolve_pose_sprite(
    game: GameId,
    type_index: u16,
    sprite_dims: &impl Fn(u16) -> Option<(u16, u16, u16)>,
) -> Option<PoseSprite> {
    match game {
        GameId::Mc1 | GameId::Mc1Hw => {
            let stats = SPRITE_STATS.get(type_index as usize)?;
            let world_h = if stats.height != 0 {
                stats.height as f32 / UNITS_PER_TILE
            } else {
                let (sw, sh, _) = sprite_dims(stats.sprite_base)?;
                if sw == 0 || stats.width == 0 {
                    return None;
                }
                stats.width as f32 * sh as f32 / sw as f32 / UNITS_PER_TILE
            };
            Some(PoseSprite {
                sprite_base: stats.sprite_base,
                draw_type: stats.draw_type,
                world_h,
                world_w: stats.width as f32 / UNITS_PER_TILE,
            })
        }
        GameId::Mc2 => {
            let param = mgc_sim::mc2::sprite_params::SPRITE_PARAMS.get(type_index as usize)?;
            let (sw, sh, flags) = sprite_dims(param.word_0)?;
            if sw == 0 || sh == 0 {
                return None;
            }
            let world_h = if param.rot_speed_8 != 0 {
                param.rot_speed_8 as f32 / UNITS_PER_TILE
            } else if param.speed_6 != 0 {
                param.speed_6 as f32 * sh as f32 / sw as f32 / UNITS_PER_TILE
            } else {
                return None;
            };
            Some(PoseSprite {
                sprite_base: param.word_0,
                draw_type: (flags >> 8) as u8,
                world_h,
                world_w: world_h * sw as f32 / sh as f32,
            })
        }
    }
}

/// Resolve all drawable entities of a level against the post-feature
/// height plane. `sprite_dims(id)` returns a sprite's pixel size (for
/// the load-time aspect fixup when the stats row has height 0).
pub fn billboards(
    things: &[Thing],
    height: &[u8],
    sprite_dims: impl Fn(u16) -> Option<(u16, u16, u16)>,
) -> Vec<Billboard> {
    let mut out = Vec::new();
    // Per-(class, model) spawn counters (AlternateByCount picks).
    let mut counts = std::collections::HashMap::<(u16, u16), u32>::new();

    for t in things {
        if t.kind != ThingKind::Entity {
            continue;
        }
        let Some(pick) = mc1_entity_type(t.class, t.model) else {
            continue;
        };
        let n = counts.entry((t.class, t.model)).or_default();
        let count = *n;
        *n += 1;

        // Position: tile center (the original spawns at `<<8 | +128`
        // engine units); trees additionally jitter by the LCG.
        let mut rng = SpawnRng(t.slot);
        let mut x = t.x as f32 + 0.5;
        let mut z = t.y as f32 + 0.5;
        let type_index = match pick {
            Mc1TypePick::Const(i) => i,
            Mc1TypePick::RandomBit(even, odd) => {
                // The tree spawner's draw order (sub_37BC0): actLife,
                // x jitter, y jitter, then the variant bit.
                rng.draw(); // actLife
                x += ((rng.draw() & 0x3F) as f32 - 32.0) / UNITS_PER_TILE;
                z += ((rng.draw() & 0x3F) as f32 - 32.0) / UNITS_PER_TILE;
                if rng.draw() & 1 != 0 { odd } else { even }
            }
            Mc1TypePick::RandomSevenSplit(major, minor) => {
                if rng.draw() % 7 < 4 {
                    major
                } else {
                    minor
                }
            }
            Mc1TypePick::AlternateByCount(first, second) => {
                if count.is_multiple_of(2) {
                    first
                } else {
                    second
                }
            }
            Mc1TypePick::Mana => {
                if t.swi_id >= 3 {
                    280
                } else {
                    77
                }
            }
        };

        let yaw = (rng.draw() & 0x7FF) as f32 * std::f32::consts::TAU / 2048.0;
        push_billboard(&mut out, height, &sprite_dims, type_index, x, z, yaw);

        // Multi-part creatures: the original spawns the body segments
        // stacked on the head (state 120) and its movement strings
        // them out from the first tick — a state the player never
        // sees. Until mobs move, settle the body in a trailing line
        // behind the head (approximation; movement will own segment
        // positions).
        const PART_SPACING: f32 = 0.35; // tiles between segments
        let (fx, fz) = (yaw.sin(), -yaw.cos()); // facing (yaw 0 = -Z)
        for (i, &part) in mc1_entity_parts(t.class, t.model).iter().enumerate() {
            let d = PART_SPACING * (i + 1) as f32;
            push_billboard(
                &mut out,
                height,
                &sprite_dims,
                part,
                x - fx * d,
                z - fz * d,
                yaw,
            );
        }
    }
    out
}

/// Any single-tick jump longer than this (tiles) is a teleport —
/// portals, possession warps — and snaps instead of lerping. The
/// fastest legitimate movers (projectiles, falling meteors) cover a
/// few tiles per tick at most.
const SNAP_TILES: f32 = 8.0;

/// Blend two consecutive tick pose snapshots at `alpha` ∈ [0, 1] —
/// the smooth-motion (render interpolation) pass. Presentation only:
/// the sim never sees the blended poses.
///
/// Poses pair by (slot, generation): slot alone aliases across pool
/// reuse — a projectile dying into a same-tick fresh spawn would
/// streak across the map (the balloon stale-slot class) — the pair
/// never does. Unpaired poses (fresh spawns) draw at their current
/// tick position. Transforms lerp — x/z the short way around the
/// 256-tile torus (the camera's `lerp_wrap` rule), yaw the short way
/// around the circle; everything discrete (sprite frame, blend,
/// life_frac, flags) rides the newer snapshot.
pub fn lerp_poses(prev: &[LivePose], cur: &[LivePose], alpha: f32) -> Vec<LivePose> {
    use std::f32::consts::{PI, TAU};
    let by_slot: std::collections::HashMap<u16, &LivePose> =
        prev.iter().map(|p| (p.slot, p)).collect();
    let wrap_delta = |p: f32, q: f32| {
        let mut d = q - p;
        if d > 128.0 {
            d -= 256.0;
        }
        if d < -128.0 {
            d += 256.0;
        }
        d
    };
    cur.iter()
        .map(|b| {
            let mut out = *b;
            let Some(a) = by_slot
                .get(&b.slot)
                .filter(|a| a.generation == b.generation)
            else {
                return out;
            };
            // The retail self-modifying sprite height (the Vissuluth
            // growth ramp) steps once per SIM TICK — the player's
            // retail footage shows the discrete mid-step images.
            // Player-requested presentation polish: lerp it on the
            // same frame alpha as the transforms so the port's growth
            // is continuous. Deliberate deviation, render-path only —
            // the sim keeps retail's +30/tick step exactly.
            if let (Some(pa), Some(pb)) = (a.sprite_h_units, b.sprite_h_units) {
                out.sprite_h_units = Some(pa + (pb - pa) * alpha);
            }
            let (dx, dz, dalt) = (wrap_delta(a.x, b.x), wrap_delta(a.z, b.z), b.alt - a.alt);
            if dx * dx + dz * dz + dalt * dalt > SNAP_TILES * SNAP_TILES {
                return out; // teleport: snap, never streak
            }
            out.x = (a.x + dx * alpha).rem_euclid(256.0);
            out.z = (a.z + dz * alpha).rem_euclid(256.0);
            out.alt = a.alt + dalt * alpha;
            let mut dyaw = b.yaw - a.yaw;
            if dyaw > PI {
                dyaw -= TAU;
            }
            if dyaw < -PI {
                dyaw += TAU;
            }
            out.yaw = (a.yaw + dyaw * alpha).rem_euclid(TAU);
            out
        })
        .collect()
}

/// The live-world path: billboards straight from the sim's pose
/// snapshot — position, altitude, yaw, sprite type and animation frame
/// are all sim-owned (the spawn handlers ran the original's per-event
/// LCG), so nothing is re-derived here. The static `billboards` path
/// above remains for MC2 / `--no-terrain-features` comparison renders.
pub fn billboards_from_poses(
    game: GameId,
    poses: &[LivePose],
    sprite_dims: impl Fn(u16) -> Option<(u16, u16, u16)>,
    enhanced_fire: bool,
    enhanced_lightning: bool,
    dweller_invisibility: bool,
) -> Vec<Billboard> {
    let mut out = Vec::new();
    for p in poses {
        if p.map_only {
            continue; // map presence only (unclaimed MC2 buildings)
        }
        // Enhanced fire: the procedural pass draws these instead — the
        // (10,0/1) fire/explosion cells (their retail sprite would sit
        // hot under the flash→soot lifecycle) and the flame-flying
        // set: the (9,0) fireball, the MC2 (9,28) charged fireball,
        // and the MC2 (10,77) firestorm satellites (the flame + comet
        // trail replaces each core). Classic leaves every retail
        // sprite untouched.
        if enhanced_fire
            && (p.class == 10
                && (matches!(p.model, 0 | 1 | 77)
                    || (matches!(p.model, 6 | 19 | 23) && p.fire_life.is_some()))
                || p.class == 9 && matches!(p.model, 0 | 28))
        {
            continue;
        }
        // Enhanced lightning: the procedural bolt replaces the
        // sprite-216 zigzag flash — (9,9) covers the trail/segment
        // nodes of every beam emitter in both games (the beam entity
        // itself dies the tick it fires and never poses).
        if enhanced_lightning && p.class == 9 && p.model == 9 {
            continue;
        }
        let Some(s) = resolve_pose_sprite(game, p.type_index, &sprite_dims) else {
            continue;
        };
        out.push(Billboard {
            x: p.x,
            y: p.alt,
            z: p.z,
            yaw: p.yaw,
            sprite_base: s.sprite_base,
            draw_type: s.draw_type,
            frame: p.frame,
            // A sprite-param row retail patched IN PLACE overrides the
            // baked `rot_speed_8` (the Vissuluth wait-phase shrink +
            // growth ramp — see `LivePose::sprite_h_units`). Retail's
            // projected height is `projScale * rotSpeed_8 / depth`
            // (GameRenderOriginal.cpp:3770-72), so swapping the field
            // is the whole law; the renderer re-derives width from the
            // frame's pixel aspect exactly as retail does.
            world_h: p.sprite_h_units.map_or(s.world_h, |u| u / UNITS_PER_TILE),
            blend: p.blend,
            // Retail's co-tile paint order, read off the sim's live
            // tile chains (`LivePose::chain_depth`).
            chain_depth: p.chain_depth,
            // Retail proximity concealment (a 19..15-tile slant-
            // distance sphere, retail's own fog band — see
            // mgc_render::Billboard): the MC2 wraith (5,26)
            // unconditionally — retail's short draw radius is what
            // kept its hunting ghost unseen until close, and the
            // port's extended fog had left it exposed map-wide —
            // plus the mana dwellers (5,23) under the
            // `mc2_dweller_invisibility` patch, whose covert design
            // leaned on the same short fog.
            conceal: game == GameId::Mc2
                && p.class == 5
                && (p.model == 26 || (p.model == 23 && dweller_invisibility)),
        });
    }
    out
}

/// Health bars from the live pose set (unfaithful debug overlay,
/// `render.debug.health_bars` / H): one classic red-on-black bar
/// hovering above EVERYTHING destroyable — every entity the sim tags
/// with a `life_frac` (class-5 chain heads, wizard-family castles and
/// carpets, and destructible structures like dwellings). Width tracks
/// the sprite when there is one; structures rendered as models (no
/// billboard sprite) fall back to a fixed bar so they are covered too.
pub fn health_bars_from_poses(
    game: GameId,
    poses: &[LivePose],
    sprite_dims: impl Fn(u16) -> Option<(u16, u16, u16)>,
) -> Vec<HealthBar> {
    let mut out = Vec::new();
    for p in poses {
        let Some(frac) = p.life_frac else {
            continue;
        };
        // Sprite-backed entities float the bar at sprite height and
        // scale its width; entities with no resolvable billboard
        // (dwellings, other structures) still get a bar at a default
        // height/width so the overlay covers all destroyables.
        let (world_h, world_w) = match resolve_pose_sprite(game, p.type_index, &sprite_dims) {
            Some(s) => (s.world_h, s.world_w.clamp(0.6, 2.0)),
            None => (1.5, 1.5),
        };
        out.push(HealthBar {
            x: p.x,
            y: p.alt + world_h + 0.15,
            z: p.z,
            w: world_w,
            frac,
        });
    }
    out
}

/// The team color pairs `byte_99B58[16]` (remc1 :5740): per team,
/// even entry = the bright/solid color (projectiles, blink-A), odd =
/// the darker alternate (creatures, blink-B). Raw palette indices,
/// exactly as plotted; row = the wizard slot (0 = the human, whose
/// even entry 0xB7 is a near-white lavender in the game palette).
const TEAM_COLORS: [(u8, u8); 8] = [
    (0xB7, 0x71),
    (0x7D, 0x7A),
    (0x9D, 0x9A),
    (0x07, 0x5A),
    (0x1D, 0x1B),
    (0xDD, 0xDA),
    (0x3C, 0x39),
    (0x10, 0x0E),
];
#[cfg(test)]
const TEAM0_EVEN: u8 = TEAM_COLORS[0].0;
#[cfg(test)]
const TEAM0_ODD: u8 = TEAM_COLORS[0].1;

/// Icon patches for the map's UI-sprite markers (cropped from the
/// composited HSPR atlas): castle = sprite 58+team, balloon = 66+team
/// (remc1 sub_48710 :57230/:57234); the advertised-trigger X/O
/// markers 83/84 (`sub_48710` case 0xB :57386-401) — see `exit_x`/
/// `exit_o` and `exit_marker_stamps`.
#[derive(Default)]
pub struct MapIcons {
    /// Castle stamps 58..=65 by team slot.
    pub castle: [Option<mgc_render::MapStamp>; 8],
    /// Balloon stamps 66..=73 by team slot.
    pub balloon: [Option<mgc_render::MapStamp>; 8],
    /// Spell icons by spell id (game-aware source sprite; see
    /// `ui::spell_icon_sprite`), shrunk to marker size — the
    /// expose-jar-spells debug stamps, consumed only when that
    /// option is on.
    pub spell: Vec<Option<mgc_render::MapStamp>>,
    /// Advertised-trigger map markers: the class-11 X = HUD-bank
    /// sprite 83, the O = sprite 84 (remc1 `sub_48710` case 0xB;
    /// remc2 GameUI.cpp:2049-53 — drawn centered, colour baked into
    /// the sprite). Loaded for both games (83/84 baked for each).
    pub exit_x: Option<mgc_render::MapStamp>,
    pub exit_o: Option<mgc_render::MapStamp>,
    /// The marker icon-swap's miniature world-sprite stamps
    /// (`map_marker_icons`, deliberate deviation), keyed by the
    /// pose's `type_index`: spell jars/tokens. Built per level from
    /// the load population — a family with no icon keeps its dot.
    pub jar_icons: std::collections::HashMap<u16, mgc_render::MapStamp>,
    /// Icon-swap stamps for the dolmen/shrine/statue statics, keyed
    /// like [`Self::jar_icons`].
    pub static_icons: std::collections::HashMap<u16, mgc_render::MapStamp>,
}

/// Which icon-swap table a dot family belongs in (`map_marker_icons`,
/// player-chosen scope 2026-08-07): spell jars/tokens (MC1/HW class
/// 12 red AND blue, MC2 class-12/15 tokens), and the dolmen/shrine/
/// statue statics. Per game those are: MC1's statues — the near-black
/// models 1/3 (sprites 79/270) — plus its dolmen regen shrine, model
/// 2 (sprite 39; its dot is the tree-colored scenery 28, which is
/// exactly why it deserves an icon); MC2's blinking marker stone
/// (2,1) and dolmen (2,2). Trees and the sprite-48 trail-marker
/// stones keep their retail dots.
pub enum SwapFamily {
    Jar,
    Static,
}

pub fn icon_swap_family(game: GameId, class: u8, model: u8) -> Option<SwapFamily> {
    match (game, class, model) {
        (_, 12 | 15, _) => Some(SwapFamily::Jar),
        (GameId::Mc2, 2, 1 | 2) => Some(SwapFamily::Static),
        (GameId::Mc1 | GameId::Mc1Hw, 2, 1..=3) => Some(SwapFamily::Static),
        _ => None,
    }
}

/// The atlas sprite id a pose's type row resolves to — what the
/// marker icon-swap crops (frame 0). The same source rows as
/// `resolve_pose_sprite`, without the size/draw-type resolution.
pub fn pose_sprite_id(game: GameId, type_index: u16) -> Option<u16> {
    match game {
        GameId::Mc1 | GameId::Mc1Hw => Some(SPRITE_STATS.get(type_index as usize)?.sprite_base),
        GameId::Mc2 => Some(
            mgc_sim::mc2::sprite_params::SPRITE_PARAMS
                .get(type_index as usize)?
                .word_0,
        ),
    }
}

/// The MC2 map environment — selects the minimap's team-colour table
/// and map-type colours (`sub_48120` rebuilds `playersColors_E88E0x`
/// per `MapType`, remc2 EventsFunctions.cpp:32180-32262; the v90/v91/
/// v92 map-type colours are GameUI.cpp:1043-1063).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mc2MapEnv {
    #[default]
    Day,
    Night,
    Cave,
}

/// `playersColors_E88E0x[8][{bright, dark}]` — 8-bit palette indices
/// (EventsFunctions.cpp: day :32236-59, night :32188-32231, cave =
/// night except wizard 0 :32206-31). Column [2] (0x7B shared) is the
/// name-text colour, unused by the dot pass.
const MC2_TEAM_DAY: [(u8, u8); 8] = [
    (0x60, 0x64),
    (0x7B, 0x77),
    (0x1C, 0x18),
    (0x5B, 0x57),
    (0x9A, 0x97),
    (0xDB, 0xD8),
    (0x76, 0xA0),
    (0x3D, 0x3A),
];
const MC2_TEAM_NIGHT: [(u8, u8); 8] = [
    (0xA4, 0xAA),
    (0x77, 0x7D),
    (0xC0, 0xC6),
    (0x58, 0x5D),
    (0x97, 0x9D),
    (0xD7, 0xDD),
    (0x69, 0x62),
    (0xC9, 0xCF),
];

/// A 12-bit `0xRGB` minimap colour code (`MapColourIndexs.h`)
/// resolved through the level palette. Retail routes these through
/// `CLRD-0.DAT` — which is exactly a PRECOMPUTED nearest-palette-
/// index table for the 4096 codes (loaded Basic.cpp:330, indexed
/// GameUI.cpp:1166 etc.) — so the live quantization here is the same
/// mapping without a bake. (RGB nibble order: the enum's own names
/// align with the shared marker conventions — SPELLS 0xF00 red,
/// CIVILIANS 0x00F blue, CREATURE 0xFFF white.)
fn mc2_clrd(palette: &[[u8; 4]; 256], code: u16) -> u8 {
    let n = |v: u16| {
        let v = (v & 0xF) as u8;
        (v << 4) | v
    };
    nearest_palette_index(palette, [n(code >> 8), n(code >> 4), n(code)])
}

/// The MC2 minimap entity law — `DrawMinimapEntities_B_61A00` (remc2
/// GameUI.cpp:951, entity switch :1134-1411), the dot/colour rules on
/// our full-map view (the rotating-radar projection stays MC1-shaped;
/// only the colours follow MC2).
///
/// INTERIM stand-ins (banked): the MSPRD bitmap stamps — class-11
/// models 0x0C/0x1F (map X-markers 83/84), the class-3 castle flag
/// (+58) and balloon (+66) families — draw as 2x2 dots until the
/// MSPRD bank is baked; the castle "rope" line (:1089-1130) joins the
/// guide-path machinery when MC2 castles land; the Beyond-Sight
/// enemy-wizard reveal (:1492-1529) waits for MC2 rivals.
fn mc2_map_dots(
    poses: &[LivePose],
    palette: &[[u8; 4]; 256],
    env: Mc2MapEnv,
    turn: u32,
    icon_swapped: &std::collections::HashSet<u16>,
) -> Vec<mgc_render::MapDot> {
    let team_tab = match env {
        Mc2MapEnv::Day => MC2_TEAM_DAY,
        Mc2MapEnv::Night | Mc2MapEnv::Cave => {
            let mut t = MC2_TEAM_NIGHT;
            if env == Mc2MapEnv::Cave {
                t[0] = (0xE0, 0x58);
            }
            t
        }
    };
    // Map-type colours (GameUI.cpp:1043-63): v92 = the unit fill,
    // v91/v90 = building/marker fallbacks.
    let (v92, v91, v90) = match env {
        Mc2MapEnv::Day => (mc2_clrd(palette, 0), 0xE8, 0x1C),
        Mc2MapEnv::Night => (mc2_clrd(palette, 4095), 0xE8, 0x84),
        Mc2MapEnv::Cave => (mc2_clrd(palette, 4095), 0x1C, mc2_clrd(palette, 240)),
    };
    // Blink phases `colorIndex_121[k] = (Turn / k) & 1`
    // (EventsFunctions.cpp:37563-66).
    let blink3 = (turn / 3) & 1 == 1;
    let blink2 = (turn / 2) & 1 == 1;
    let team = |t: Option<u8>| t.map(|t| team_tab[(t as usize).min(7)]);
    // LABEL_56 (GameUI.cpp:1291-96): owner wizard → team bright, else
    // the UNPOSSESSED_BUILDING2 code.
    let by_owner = |t: Option<u8>| {
        team(t)
            .map(|(bright, _)| bright)
            .unwrap_or_else(|| mc2_clrd(palette, 0xF0F))
    };
    // LABEL_173 (:1303-16): linked wizard → the blink pair, else v91.
    let linked_blink = |t: Option<u8>| {
        team(t)
            .map(|(bright, dark)| if blink3 { bright } else { dark })
            .unwrap_or(v91)
    };

    let mut out = Vec::new();
    for p in poses {
        if p.segment {
            continue;
        }
        // Marker icon-swap: this family draws as a miniature stamp
        // instead (`map_stamps_from_poses`); the set holds exactly
        // the type rows an icon was built for.
        if icon_swapped.contains(&p.type_index) {
            continue;
        }
        let mut size = 1u8;
        let color = match (p.class, p.model) {
            // Scenery: tree v90 (:1147-62); marker stone blinks the
            // MARKER_STONE code (:1163-70); dolmen blinks the
            // UNPOSSESSED_BUILDING code against v90 (:1171-79).
            (2, 0) => v90,
            (2, 1) => {
                if blink3 {
                    mc2_clrd(palette, 0x88)
                } else {
                    continue;
                }
            }
            (2, 2) => {
                if blink2 {
                    mc2_clrd(palette, 0x888)
                } else {
                    v90
                }
            }
            (2, _) => continue,
            // Castle: retail = the +58 MSPRD flag stamp (:1188-95);
            // 2x2 team dot until the stamp bank bakes. Wizard bodies
            // (own = the player arrow; enemies need Beyond Sight) and
            // balloons skip.
            (3, 2) => {
                size = 2;
                by_owner(p.team)
            }
            (3, _) => continue,
            // Units (:1219-53): wizard-owned → team dark; wild
            // civilians (12..=14) → CIVILIANS; every other wild
            // creature → the map-type fill.
            (5, _) if p.team.is_some() => team(p.team).unwrap().1,
            (5, 12..=14) => mc2_clrd(palette, 15),
            (5, _) => v92,
            (9, _) => by_owner(p.team),
            // Class 10 (:1256-1332): 0x12 and 0x56/0x57 skip; the
            // portal (34) grows 2x2; buildings (45) and the flag
            // models blink the owner pair; 0x4E is own-only.
            (10, 0x12) => continue,
            (10, 34) => {
                size = 2;
                by_owner(p.team)
            }
            (10, 45) => {
                if p.team.is_some() {
                    linked_blink(p.team)
                } else {
                    by_owner(p.team)
                }
            }
            (10, 0x27..=0x39) => linked_blink(p.team),
            (10, 0x4E) => {
                if !p.player_owned {
                    continue;
                }
                let (b, d) = team_tab[0];
                if blink3 { b } else { d }
            }
            (10, 0x56 | 0x57) => continue,
            (10, _) => by_owner(p.team),
            // Switch X-markers (models 0x0C/0x1F → MSPRD stamps
            // 83/84, :1385-92): 2x2 white until the stamps bake.
            // Every other switch is undrawn (:1341-84).
            (11, 0x0C | 0x1F) => {
                size = 2;
                mc2_clrd(palette, 4095)
            }
            (11, _) => continue,
            // Spells + class-15 (:1396-1402).
            (12 | 15, _) => mc2_clrd(palette, 3840),
            // The class-14 model 5 blinker (:1403-09).
            (14, 5) => {
                if blink3 {
                    mc2_clrd(palette, 3840)
                } else {
                    mc2_clrd(palette, 4095)
                }
            }
            _ => continue,
        };
        out.push(mgc_render::MapDot {
            x: p.x,
            z: p.z,
            color,
            size,
        });
    }
    out
}

/// Map dots from the live pose set — the verbatim color switch of
/// remc1 sub_48710_48A50 (:57184-:57292); body segments hidden like
/// the original's state-120 exclusion. `turn` = the sim tick
/// (MC1's claimed-ball blink derives its ~4 Hz phase from it; MC2's
/// `colorIndex_121` divides it directly). `owned_buildings` = our
/// MC2-style enhancement: dwellings mark like MC2's (unclaimed pink,
/// possessed blinking the owner pair); off, no building marks at all.
///
/// MC2 worlds dispatch to [`mc2_map_dots`] — the real
/// DrawMinimapEntities_B_61A00 law.
pub fn map_dots_from_poses(
    game: GameId,
    poses: &[LivePose],
    palette: &[[u8; 4]; 256],
    owned_buildings: bool,
    env: Mc2MapEnv,
    turn: u32,
    icon_swapped: &std::collections::HashSet<u16>,
) -> Vec<mgc_render::MapDot> {
    if game == GameId::Mc2 {
        return mc2_map_dots(poses, palette, env, turn, icon_swapped);
    }
    let blink = (turn >> 3) & 1 == 0;
    // MC2's linked-building flash phase (`colorIndex_121[3]`, the
    // same one mc2_map_dots runs): the dwelling-marker enhancement
    // blinks at MC2's cadence, not the ~2.7x slower MC1 claimed-ball
    // phase above (player ruling — the option IS MC2's behavior).
    let blink3 = (turn / 3) & 1 == 1;
    // The engine's computed colors go through its 16x16x16 RGB LUT
    // (byte_AD167_AD157). The cube is RED-major and pre-incremented
    // (build loop :41950-77: R strides 256, G 16, B 1, entry values
    // 3 + 4·level), so LUT[n] decodes 6-bit RGB (3+4·((n-1)>>8),
    // 3+4·(((n-1)>>4)&15), 3+4·((n-1)&15)): [1] = near-black (wild
    // creatures), [16] = the vivid violet-blue (villagers — the
    // "village speckles" on the retail map are the VILLAGERS, not
    // the houses), [3856] = (n-1 = 0xF0F) bright magenta — the same
    // RGB444 code MC2's UNPOSSESSED_BUILDING2 resolves.
    let near_black = nearest_palette_index(palette, vga(3, 3, 3));
    let villager_blue = nearest_palette_index(palette, vga(3, 3, 63));
    let wild_magenta = nearest_palette_index(palette, vga(63, 3, 63));
    let red = nearest_palette_index(palette, vga(63, 3, 7));
    const SCENERY: u8 = 28;
    const WILD_BALL: u8 = 232; // v74 = -24 (:57291)

    let mut out = Vec::new();
    for p in poses {
        if p.segment {
            continue;
        }
        // Marker icon-swap: the family draws as a miniature stamp
        // instead (same rule as the MC2 walk above).
        if icon_swapped.contains(&p.type_index) {
            continue;
        }
        let team = p.team.map(|t| TEAM_COLORS[(t as usize).min(7)]);
        let owner_color = team.map(|(v, _)| v).unwrap_or(wild_magenta);
        let mut size = 1u8;
        let color = match (p.class, p.model) {
            // Charred trees leave the map (v29 stays 0, :57219).
            (2, 0) if matches!(p.type_index, 226 | 227) => continue,
            // Models 1/3 = the settings-gated near-black family
            // (:57195-57210); the rest plain scenery 28.
            (2, 1 | 3) => near_black,
            (2, _) => SCENERY,
            // Castle/balloon draw as icon STAMPS, not dots.
            (3, _) => continue,
            (5, 12..=14) if team.is_none() => villager_blue,
            // :57252 (the team pair's odd entry).
            (5, _) if team.is_some() => team.unwrap().1,
            (5, _) => near_black,
            (9, _) => owner_color,
            // Portal vortex: the 2x2 grown dot (v60 = 2, :57270).
            (10, 34) => {
                size = 2;
                owner_color
            }
            // The mana magnet: a bright white dot for its whole
            // 128-tick life (player retail-verified) — the map is the
            // only place the invisible (10,54) shows at all.
            (10, 54) => nearest_palette_index(palette, vga(63, 63, 63)),
            // Mana balls: wild = 232; claimed model 39 BLINKS the team
            // pair on the global flash phase (:57282-92). Model 40
            // carries no phase term — it falls through LABEL_32 like
            // any other class-10 (steady even entry / wild magenta).
            //
            // MC2's (10,57) fool's-mana sphere plots on the SAME arm:
            // the whole spell is that a decoy is indistinguishable
            // from neutral ground mana, so it must not get its own
            // tell. (Native m57 wore model 39 until the OPEN-6 fix, so
            // this arm is where it already drew — the 57 keeps that.)
            (10, 39 | 57) => {
                if let Some((v, b)) = team {
                    if blink { v } else { b }
                } else {
                    WILD_BALL
                }
            }
            // Dwellings: retail marks NO buildings on the map
            // (player-certified from original gameplay; the
            // decompile's LABEL_32 house arm notwithstanding — see
            // docs/DEVIATIONS.md), so the faithful default plots
            // nothing. The `owned_buildings` enhancement brings MC2's
            // law over: unclaimed = the steady magenta 0xF0F code,
            // possessed = the owner pair blinking at MC2's flash
            // cadence, 1px like MC2's markers.
            (10, 45) => {
                if !owned_buildings {
                    continue;
                }
                if let Some((v, b)) = team {
                    if blink3 { v } else { b }
                } else {
                    wild_magenta
                }
            }
            (10, _) => owner_color,
            (12, _) => red,
            _ => continue,
        };
        out.push(mgc_render::MapDot {
            x: p.x,
            z: p.z,
            color,
            size,
        });
    }
    out
}

/// Icon stamps from the live pose set — remc1 :57224-37 draws these
/// as UI sprites instead of dots. Retail rule (sub_48710): EVERY
/// castle stamps unconditionally with its team's sprite [58+team];
/// balloons [66+team] only when own or Beyond Sight is live (v59,
/// :57232-35). `beyond_sight` also reveals rival WIZARD positions —
/// retail draws their NAME there in team color (:57413-48); until
/// the DrawText path lands, a 2x2 team-color marker dot stands in
/// (banked with the font track).
pub fn map_stamps_from_poses(
    game: GameId,
    poses: &[LivePose],
    icons: &MapIcons,
    beyond_sight: bool,
    expose_jar_spells: bool,
    marker_icons: bool,
) -> Vec<mgc_render::MapStamp> {
    let mut out = Vec::new();
    for p in poses {
        let team = p.team.map(|t| (t as usize).min(7)).unwrap_or(0);
        // MC2's stamp art families (castle 58+k, balloon 66+k) are
        // authored in TransformPlayerColorIndex order — retail's
        // castle marker is `Transform(owner) + 58` (GameUI.cpp:1193).
        // MC1's stamp family is raw slot order (sub_48710 [58+team]).
        let team = match game {
            GameId::Mc2 => mgc_sim::mc2::color_art(team as u8) as usize,
            _ => team,
        };
        let icon = match (p.class, p.model) {
            (3, 2) => icons.castle[team].as_ref(),
            (3, 3) if p.team == Some(0) || beyond_sight => icons.balloon[team].as_ref(),
            // expose-jar-spells: pickable jars (MC1 class 12, MC2
            // class-15 tokens; owned manifestations never reach the
            // pose list) tag with their spell's icon. The debug
            // option OUTRANKS the marker icon-swap for jars (player
            // ruling): spell icon + the retail dot, never the jar
            // miniature too.
            (12 | 15, m) if expose_jar_spells => {
                icons.spell.get(m as usize).and_then(Option::as_ref)
            }
            // Marker icon-swap (map_marker_icons): the jar/dolmen/
            // statue families wear miniatures of their own world
            // sprite; the tables hold exactly the type rows an icon
            // was built for, so anything else keeps its dot.
            (12 | 15, _) if marker_icons => icons.jar_icons.get(&p.type_index),
            (2, _) if marker_icons => icons.static_icons.get(&p.type_index),
            // (MC2 exit X/O markers are NOT pose-driven — hidden
            // markers must plot too; see `exit_marker_stamps`.)
            _ => None,
        };
        if let Some(i) = icon {
            let mut s = *i;
            s.x = p.x;
            s.z = p.z;
            out.push(s);
        }
    }
    out
}

/// The advertised-trigger map markers from the sim's trigger census
/// (`advertised_marker_poses`, plotted from level start, gone once
/// tripped): the class-11 X sprite 83 (MC1 flight-path breadcrumb
/// models 9/10/11/12; MC2's model-12 checkpoint trip) and the O
/// sprite 84 (model 31 — MC1 advertise-only, MC2's secret trip).
/// Iteration order puts a co-located O over an X like retail's entity
/// walk.
pub fn exit_marker_stamps(
    markers: &[(f32, f32, u8)],
    icons: &MapIcons,
) -> Vec<mgc_render::MapStamp> {
    markers
        .iter()
        .filter_map(|&(x, z, model)| {
            let icon = match model {
                9..=12 => icons.exit_x.as_ref(),
                31 => icons.exit_o.as_ref(),
                _ => None,
            }?;
            let mut s = *icon;
            s.x = x;
            s.z = z;
            Some(s)
        })
        .collect()
}

/// The expose-jar-spells world markers: every pickable spell jar's
/// `(x, alt, z, spell id)` — MC1 class 12 (pre-placed, red or blue,
/// and death-scattered) plus MC2's class-15 tokens. model65 = spell
/// id (off_987DE dispatch, docs/traces/mc1-blue-jars.md).
pub fn jar_markers_from_poses(poses: &[LivePose]) -> Vec<(f32, f32, f32, u8)> {
    poses
        .iter()
        .filter(|p| matches!(p.class, 12 | 15))
        .map(|p| (p.x, p.alt, p.z, p.model))
        .collect()
}

/// Dynamic point lights `(x, alt, z, intensity)` for the renderer's
/// terrain light pass. Retail (remc2 `AddEvent2_847D0`, EF:47172)
/// hand-attaches lights to exactly seven spawn ctors — the fireball
/// projectiles (9,0)/(9,9)/(10,23), the explosions (10,0)/(10,1) at
/// intensity 128, and the standing fire (10,6) at 80 — never a flag
/// or class rule. Intensity here is normalized to the 128 spell
/// baseline; the caller gates on Night/Cave (retail's MapType gate —
/// day shade tables invert, added rows would darken) and the
/// `light_sources` option. Capped at 16 (retail: 50 cell-grid slots;
/// our per-pixel pass keeps a uniform-friendly cap).
pub fn lights_from_poses(poses: &[LivePose]) -> Vec<[f32; 4]> {
    poses
        .iter()
        .filter_map(|p| {
            let intensity = match (p.class, p.model) {
                (9, 0) | (9, 9) | (10, 0) | (10, 1) | (10, 23) => 1.0,
                (10, 6) => 80.0 / 128.0,
                _ => return None,
            };
            Some([p.x, p.alt, p.z, intensity])
        })
        .take(16)
        .collect()
}

/// One blast ring's world pitch in tiles: the `(10,17)` driver places
/// ring cells at 160 sim units per ring index (:28707), and a tile is
/// 256 units.
const RING_PITCH: f32 = 160.0 / 256.0;

/// PROTOTYPE: one tracked blast in the render-side fire ledger — a live
/// `(10,17)` driver, or a recently dead one whose smoke is still
/// choreographed (the driver despawns ticks before the rim smoke
/// clears, so the ledger must outlive it).
///
/// The comb law lives here: driver pass `p` (1-based; `elapsed` counts
/// completed passes) fires ring `(2(p-1)) mod 11` — the EVEN radii
/// sweep outward first (wave 1, 6 passes), then the ODD radii back-fill
/// the burnt interior (wave 2, 5 passes), and a driver that outlives
/// one full 11-pass comb re-burns the same rings CYCLICALLY (the ring
/// table wraps). `passes` is what distinguishes every blast kind: the
/// MC1 meteor/volcano runs 11 (pre-decrement), the MC2 tiered meteor
/// 2/5/10 (its tier fuse — a short fuse scales BOTH duration and final
/// radius), and the MC2 doomsday death sphere 70 (a rolling firestorm
/// cycling the comb ~6 times).
#[derive(Debug, Clone, Copy)]
pub struct LedgerBlast {
    pub slot: u16,
    pub generation: u32,
    pub x: f32,
    pub z: f32,
    pub plane_z: f32,
    /// Ring passes completed (whole ticks; the frame's smooth-motion
    /// alpha is added at emission time). Keeps advancing after the
    /// driver despawns.
    pub elapsed: f32,
    /// Total passes the driver runs (MC1 pre-decrement: max_life + 1).
    pub passes: f32,
}

impl LedgerBlast {
    /// One full comb cycle — the driver's ring table is mod 11.
    pub const CYCLE: f32 = 11.0;

    /// The comb cycle containing blast time `t`: (cycle start as a
    /// pass offset, passes this cycle runs — capped at a full comb).
    pub fn cycle_at(&self, t: f32) -> (f32, f32) {
        let base = (((t - 1.0) / Self::CYCLE).floor()).max(0.0) * Self::CYCLE;
        (base, (self.passes - base).clamp(0.0, Self::CYCLE))
    }

    /// The cycle-local time at which a `cp`-pass cycle's wave-1 front
    /// stops (pass 6 fires ring 10, or the cycle's final pass if it
    /// dies sooner — the MC2 low tiers).
    pub fn wave1_end_of(cp: f32) -> f32 {
        cp.min(6.0)
    }

    /// A `cp`-pass cycle's final wave-1 fire radius in tiles.
    pub fn wave1_max_of(cp: f32) -> f32 {
        RING_PITCH * 2.0 * (Self::wave1_end_of(cp) - 1.0).max(0.0)
    }

    /// The blast's overall fire extent (its first cycle's wave-1 max —
    /// later cycles never exceed it): crater membership, preview.
    pub fn wave1_max(&self) -> f32 {
        Self::wave1_max_of(self.passes.min(Self::CYCLE))
    }
}

/// PROTOTYPE: the render-side blast ledger. Latches every live blast
/// driver each sim tick and keeps aging it after the driver despawns,
/// until the last rim smoke is gone — the procedural flame walls, the
/// shockwave, and the crater-cell smoke all read blasts from here.
#[derive(Debug, Default)]
pub struct BlastLedger {
    entries: Vec<LedgerBlast>,
}

impl BlastLedger {
    /// Advance every tracked blast by `steps` sim ticks, then refresh /
    /// insert from the live driver views (authoritative while alive).
    /// Call once per sim tick with steps = 1.
    pub fn update(&mut self, views: &[BlastView], steps: f32) {
        for e in &mut self.entries {
            e.elapsed += steps;
        }
        for v in views {
            if let Some(e) = self
                .entries
                .iter_mut()
                .find(|e| e.slot == v.slot && e.generation == v.generation)
            {
                e.elapsed = v.elapsed;
                e.x = v.x;
                e.z = v.z;
                e.plane_z = v.plane_z;
            } else {
                self.entries.push(LedgerBlast {
                    slot: v.slot,
                    generation: v.generation,
                    x: v.x,
                    z: v.z,
                    plane_z: v.plane_z,
                    elapsed: v.elapsed,
                    passes: v.passes as f32,
                });
            }
        }
        // Retire once the rim smoke horizon has passed: the longest-
        // lived cells (life 14) are born at wave 1's end (~pass 6).
        self.entries.retain(|e| e.elapsed < e.passes + 16.0);
    }

    /// Drop everything (level restart / session teardown).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn blasts(&self) -> &[LedgerBlast] {
        &self.entries
    }

    /// A preview ledger from hand-built entries (headless stills).
    pub fn synthetic(entries: Vec<LedgerBlast>) -> Self {
        Self { entries }
    }

    /// The tracked blast whose crater covers (x, z), torus-aware:
    /// nearest center within its wave-1 extent plus `margin` tiles.
    /// Returns (outward dir x, outward dir z, distance, blast).
    fn crater_at(&self, x: f32, z: f32, margin: f32) -> Option<(f32, f32, f32, &LedgerBlast)> {
        let full = MAP_TILES as f32;
        let wrap = |d: f32| {
            let mut d = d;
            if d > full / 2.0 {
                d -= full;
            }
            if d < -full / 2.0 {
                d += full;
            }
            d
        };
        let mut best: Option<(f32, f32, f32, &LedgerBlast)> = None;
        for b in &self.entries {
            let (dx, dz) = (wrap(x - b.x), wrap(z - b.z));
            let dist = (dx * dx + dz * dz).sqrt();
            if dist > b.wave1_max() + margin {
                continue;
            }
            if best.is_none_or(|(_, _, d, _)| dist < d) {
                // Outward radial direction; an on-center cell gets a
                // stable arbitrary heading instead of NaN.
                let (ox, oz) = if dist > 0.05 {
                    (dx / dist, dz / dist)
                } else {
                    (1.0, 0.0)
                };
                best = Some((ox, oz, dist, b));
            }
        }
        best
    }
}

/// Terrain altitude at a tile position (torus-wrapped nearest sample).
fn terrain_at(height: &[u8], x: f32, z: f32) -> f32 {
    let map = MAP_TILES;
    let tx = (x.floor() as i32).rem_euclid(map as i32) as usize;
    let tz = (z.floor() as i32).rem_euclid(map as i32) as usize;
    height[tz * map + tx] as f32 * HEIGHT_SCALE
}

/// PROTOTYPE (throwaway): turn fireball projectiles into a cloud of
/// glowing fire particles — a hot head cluster plus a tapering comet
/// trail streamed backward along the projectile's velocity. Stateless
/// by motion law (trail = head − velocity·age), so it needs no history
/// buffer; `time` (wall seconds) drives smooth turbulence and shimmer.
///
/// Emits for the MC1/MC2 fireball projectile (class 9, model 0) and its
/// impact burst (class 10, models 0/1) — the same (class, model) set
/// the dynamic-light pass already recognizes.
pub fn fire_particles_from_poses(
    prev: &[LivePose],
    cur: &[LivePose],
    blasts: &BlastLedger,
    alpha: f32,
    time: f32,
) -> Vec<FireParticle> {
    use std::collections::HashMap;
    use std::f32::consts::TAU;
    let by_slot: HashMap<u16, &LivePose> = prev.iter().map(|p| (p.slot, p)).collect();
    let wrap_delta = |p: f32, q: f32| {
        let mut d = q - p;
        if d > 128.0 {
            d -= 256.0;
        }
        if d < -128.0 {
            d += 256.0;
        }
        d
    };
    // Cheap deterministic hash -> [0,1) for per-particle phase constants.
    let hash = |a: u32| {
        let mut x = a.wrapping_mul(0x9E37_79B9);
        x ^= x >> 15;
        x = x.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 13;
        (x & 0xFFFF) as f32 / 65536.0
    };

    let mut out = Vec::new();
    for c in cur {
        // The flame-flying set: (9,0) = the fireball — MC1's, MC2's
        // L1 AND every MC2 creature/rival bolt (they share the model);
        // (9,28) = MC2's charged/repeat fireball body; (10,77) = the
        // MC2 firestorm's 25 orbiting fireball satellites (same retail
        // sprite 340 — the whole tumbling constellation burns).
        let is_projectile =
            c.class == 9 && matches!(c.model, 0 | 28) || c.class == 10 && c.model == 77;
        // Burning-in-place set: the (10,0/1) fire/explosion, plus the
        // standing fires — (10,6) in both games (MC1: Wall of Fire
        // curtain / ground patches / burning trees; MC2: tree burn)
        // and MC2's (10,19) volcano/dome fire spray. Models 6/19 key
        // on the pose carrying fire_life (the world gates model 19
        // off for MC1, where 19 is the volcano smoke plume).
        // (10,23) = the lightning hit-blast, both games — a small,
        // quick fire burst at the beam terminus (player-requested
        // enhancement; the retail sprite is suppressed like the rest).
        let is_impact = c.class == 10
            && (matches!(c.model, 0 | 1)
                || (matches!(c.model, 6 | 19 | 23) && c.fire_life.is_some()));
        if !is_projectile && !is_impact {
            continue;
        }

        // Lerped head position + per-tick velocity (from the prev/cur
        // pair, matched on slot+generation like lerp_poses).
        let (mut hx, mut hz, mut hy) = (c.x, c.z, c.alt);
        let (mut vx, mut vz, mut vy) = (0.0f32, 0.0f32, 0.0f32);
        if let Some(a) = by_slot
            .get(&c.slot)
            .filter(|a| a.generation == c.generation)
        {
            let (dx, dz, dy) = (wrap_delta(a.x, c.x), wrap_delta(a.z, c.z), c.alt - a.alt);
            if dx * dx + dz * dz + dy * dy < SNAP_TILES * SNAP_TILES {
                hx = (a.x + dx * alpha).rem_euclid(256.0);
                hz = (a.z + dz * alpha).rem_euclid(256.0);
                hy = a.alt + dy * alpha;
                vx = dx;
                vz = dz;
                vy = dy;
            }
        }
        // Center the flame near the sprite's visual middle.
        hy += 0.4;

        // Velocity direction (fall back to facing when barely moving).
        let vspeed = (vx * vx + vz * vz + vy * vy).sqrt();
        let (dirx, dirz, diry) = if vspeed > 0.02 {
            (vx / vspeed, vz / vspeed, vy / vspeed)
        } else {
            (c.yaw.sin(), -c.yaw.cos(), 0.0)
        };
        // A horizontal perpendicular for lateral dance.
        let perp = (-dirz, dirx);
        let seed_base = c.slot as u32 * 131 + c.generation;

        // One proportional size knob: scales every disc AND every
        // positional offset, so the whole flame shrinks without changing
        // shape. 0.45 keeps the head disc (plus its cluster jitter)
        // inside one tile — the shader window fades every disc to zero
        // before its quad edge, so nothing spills past its footprint.
        let scale = 0.45f32;

        // --- Impact cell: TEMPORAL decay -----------------------------
        // A CRATER cell (inside a tracked blast's footprint) draws
        // NOTHING here — the ledger-driven emitter synthesizes the
        // whole crater (walls + smoke) procedurally, and the sim cell
        // exists only for fidelity. A STANDALONE fire (a fireball hit,
        // a burning corpse's flame trail, any genuine ground fire — no
        // blast nearby) BURNS: retail drew its flame sprite for the
        // whole cell life, so the flame here holds for most of it —
        // upright licking fire in place — and only the tail of the
        // life cools into rising smoke. Age is driven by the sim's
        // per-cell `fire_life` (0 fresh → 1 dying); the 1-tick (10,1)
        // explosion seeder runs the same law compressed = a quick
        // bright burst.
        if is_impact {
            if blasts.crater_at(hx, hz, 1.2).is_some() {
                continue;
            }
            let (elapsed_cur, maxlife) = c.fire_life.unwrap_or((0.0, 1.0));
            // SUB-TICK animation: advance the cell's age fractionally with
            // the smooth-motion alpha, so the burn→smoke lifecycle plays
            // out continuously across the frames between ticks (the sim
            // steps once per tick; the render fills in the gap). A cell
            // present last tick ages elapsed_prev→elapsed_cur (they differ
            // by 1); a cell born this tick holds at its fresh value.
            let prev_life = by_slot
                .get(&c.slot)
                .filter(|a| a.generation == c.generation)
                .and_then(|a| a.fire_life);
            let fresh = prev_life.is_none();
            let elapsed = match prev_life {
                Some((ep, _)) => ep + (elapsed_cur - ep) * alpha,
                None => elapsed_cur,
            };
            let age = (elapsed / maxlife).clamp(0.0, 1.0);
            let (smoke, fade) = if matches!(c.model, 6 | 19) {
                // Standing fire / fire spray: act_life is routinely
                // overridden below the nominal 240 (curtain sheets 1,
                // patches 14, ground waves 30, MC2 tree burns 130..189,
                // MC2 trail sprays 10), so age-since-birth is
                // unknowable — this law runs on REMAINING life: full
                // flame until the last ~4 ticks, then smoke out (a
                // long tree burn just burns longer, as retail did).
                // `fresh` keeps a newborn short-fused sheet (the
                // rising Wall of Fire curtain) burning on its birth
                // tick instead of being born mid-smoke.
                const TAIL: f32 = 4.0;
                let remaining = (maxlife - elapsed).max(0.0);
                let u = if fresh {
                    0.0
                } else {
                    (1.0 - remaining / TAIL).clamp(0.0, 1.0)
                };
                (u * u * (3.0 - 2.0 * u), 1.0 - u * 0.9)
            } else {
                // Burn → smoke by LIFE FRACTION (not an absolute
                // flash): full flame through ~40% of the life, then a
                // smooth hand-over — the back half is a real smoke
                // puff (the fireball-in-the-face experience), not
                // just a fade tail.
                let s = ((age - 0.42) / 0.28).clamp(0.0, 1.0);
                (s * s * (3.0 - 2.0 * s), (1.0 - age * 1.05).clamp(0.0, 1.0))
            };
            if fade <= 0.02 {
                continue;
            }
            // TREE fires ENGULF their tree: among the model-6 standing
            // fires only tree burns carry a long re-seeded life
            // (130..189 remaining vs ≤30 for curtain sheets / patches
            // / ground waves), so size eases up with REMAINING life —
            // a fresh tree wears a full crown of flame that visibly
            // dies down to a normal fire as the tree chars out.
            let engulf = if c.model == 6 {
                1.0 + ((maxlife - elapsed - 25.0) / 55.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            // The 1-tick explosion seeder detonates bigger and hotter
            // than a steady ground fire; the standing fire's sheet
            // burns a touch larger than a ground fire cell.
            let boom = match c.model {
                1 => 1.45f32,
                6 | 19 => 1.25 * engulf,
                // The lightning hit-blast is a compact hit marker
                // (retail extents ~200 units ≈ 0.8 tile), not a
                // fireball crater — scaled well down so it reads as a
                // sharp crack at the beam end.
                23 => 0.65,
                _ => 1.0,
            };
            // Three licks in place: jittered around the cell, taller
            // than wide while burning, billowing out and rising as
            // they turn to smoke.
            for j in 0..3u32 {
                let ph = hash(seed_base ^ (j * 2657));
                let lick = (time * 8.0 + ph * 40.0 + j as f32 * 2.1).sin();
                let jx = (ph - 0.5) * 0.5 + 0.08 * lick;
                let jz = (hash(seed_base ^ (j * 977)) - 0.5) * 0.5 - 0.08 * lick;
                // The engulfing crown grows UPWARD (into the canopy),
                // not into the ground.
                let rise =
                    (0.12 * j as f32) * engulf + (0.55 + 0.1 * lick) * smoke + 0.3 * (engulf - 1.0);
                out.push(FireParticle {
                    x: hx + jx * boom,
                    z: hz + jz * boom,
                    y: hy + rise,
                    w: (0.62 + 0.12 * ph + 0.55 * smoke) * boom,
                    h: (0.88 + 0.16 * ph + 0.4 * smoke) * boom,
                    // Flicker while burning, cool to soot as it smokes.
                    heat: ((0.88 + 0.12 * lick) * (1.0 - smoke)).clamp(0.0, 1.0),
                    alpha: fade * (0.85 - 0.18 * smoke),
                    seed: ph * TAU + time * 4.0,
                });
            }
            continue;
        }

        // --- Projectile: hot head cluster -----------------------------
        // Styled per projectile kind: `g` scales the flame's diameter
        // (discs + lateral spread), the trail count its reach.
        let style = flame_style(c.class, c.model);
        // A stretched firestorm's satellites grow their girth with
        // the hub's envelope (pose flame_scale, 1.0 elsewhere); the
        // trail count stays — length would smother the lattice.
        let g = scale * style.girth * c.flame_scale;
        let core_n: u32 = 5;
        let trail_n: u32 = (16.0 * style.length).round() as u32;
        for j in 0..core_n {
            let ph = hash(seed_base ^ (j * 2657));
            let sw = (time * 9.0 + ph * 40.0).sin();
            let cw = (time * 7.0 + ph * 31.0).cos();
            let off = 0.18 * g;
            out.push(FireParticle {
                x: hx + perp.0 * off * sw + dirx * off * 0.3 * cw,
                z: hz + perp.1 * off * sw + dirz * off * 0.3 * cw,
                y: hy + 0.22 * g * cw,
                w: (1.05 + 0.2 * ph) * g,
                h: (1.3 + 0.25 * ph) * g,
                heat: 0.94 + 0.06 * ph,
                alpha: 0.62,
                seed: ph * TAU + time * 4.0,
            });
        }

        // --- Comet trail: fire near the head, dying into SOOT --------
        // The wake streams backward along -velocity, at a FIXED per-step
        // spacing (so `length` extends the reach without thinning it).
        // The near half is flame; past the midpoint the particles cool
        // to smoke (heat→0, which the shader renders as grey soot),
        // BILLOW (grow), RISE, and dissipate (alpha→0). A held rapid-
        // fire stream spaces its balls ~1 tile apart, so the stretched
        // trails overlap several successors deep and the stream reads
        // as one continuous flamethrower jet, no seams.
        for i in 1..=trail_n {
            let age = i as f32 / trail_n as f32; // 0..1
            let back = i as f32 * 0.42 * scale;
            let ph = hash(seed_base ^ (i * 8191));
            // Smoke regime ramps in past the midpoint (smoothstep).
            let smoke = {
                let t = ((age - 0.45) / 0.55).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            // Turbulence: lateral sway + rise, both stronger for smoke.
            let sway = (time * 5.0 + ph * 25.0 + i as f32 * 0.9).sin() * (0.15 + 0.6 * age) * g;
            let rise = ((time * 4.0 + ph * 17.0).sin() * 0.12 + 0.28 * age + 0.6 * smoke) * g;
            out.push(FireParticle {
                x: hx - dirx * back + perp.0 * sway,
                z: hz - dirz * back + perp.1 * sway,
                y: hy - diry * back + rise,
                // Flame tapers, then smoke billows back outward.
                w: ((1.2 - 0.7 * age + 1.1 * smoke) * g).max(0.16),
                h: ((1.35 - 0.75 * age + 1.0 * smoke) * g).max(0.2),
                // Cool steadily to ~0 (full soot) by the tail.
                heat: (1.0 - 1.25 * age).max(0.03),
                // Fade the smoke out as it dissipates.
                alpha: (0.72 - 0.62 * age).max(0.05) * (1.0 - 0.5 * smoke),
                seed: ph * TAU + time * 3.0,
            });
        }
    }
    out
}

/// A parameterized projectile-flame look: `girth` scales the flame's
/// diameter (every disc and lateral offset), `length` the comet trail's
/// reach along the heading (per-step spacing is fixed, so a longer
/// trail keeps its density). 1.0/1.0 = the original prototype look.
/// Spawn bigger or smaller flames for new purposes by adding a row to
/// [`flame_style`].
#[derive(Debug, Clone, Copy)]
pub struct FlameStyle {
    pub girth: f32,
    pub length: f32,
}

impl FlameStyle {
    /// The fireball: slimmer than the prototype (it claimed too much
    /// screen at gameplay range) but half again as LONG, so a held
    /// rapid-fire stream (~1 tile between balls) melts into one
    /// continuous flamethrower jet.
    pub const FIREBALL: FlameStyle = FlameStyle {
        girth: 0.75,
        length: 1.5,
    };
    /// A firestorm satellite: 25 of these tumble on a ~1-2-tile orb,
    /// so each burns small and SHORT-tailed — the constellation's own
    /// density supplies the rage; full fireball trails would smother
    /// it in smoke.
    pub const FIRESTORM: FlameStyle = FlameStyle {
        girth: 0.6,
        length: 0.6,
    };
}

/// The flame look for a projectile (class, model) — the same set
/// [`fire_particles_from_poses`] flies; future flaming projectiles
/// add arms here.
fn flame_style(class: u8, model: u8) -> FlameStyle {
    match (class, model) {
        (9, 0 | 28) => FlameStyle::FIREBALL,
        (10, 77) => FlameStyle::FIRESTORM,
        _ => FlameStyle {
            girth: 1.0,
            length: 1.0,
        },
    }
}

/// PROTOTYPE procedural flame walls: the crater's expanding flame front,
/// synthesized per frame from the blast ledger instead of the discrete
/// tick-spawned ring cells (which step ~1.25 tiles/tick — the chop this
/// replaces). Two walls per blast, mirroring the driver's real comb:
/// wave 1 = the outward even-radii sweep, wave 2 = the odd back-fill
/// re-burn crossing the interior afterwards. Each wall is a continuous
/// band of hot leading discs with a soot trail, terrain-draped like the
/// shockwave, sub-tick smooth, with total time = the driver's real pass
/// schedule. The per-blast angular sample set is FIXED (sized for the
/// final radius), so each sample behaves like a tiny outward-flying
/// projectile — stable identity, no re-sampling shimmer; they bunch
/// into one bright ball at detonation and fan out with the front.
///
/// Behind the walls, the crater SMOKE is synthesized here too: one
/// virtual ring of soot cells per completed pass, laid out where
/// `blast_ring_tick` spawns its real (sim-faithful, sprite-suppressed)
/// fire cells, with a radius-dependent virtual lifetime — inner cells
/// clear fast, the rim's smoke lingers well past the driver's death.
/// Fully render-side, so the sim keeps retail cell lifetimes exactly.
pub fn crater_particles(
    ledger: &BlastLedger,
    height: &[u8],
    alpha: f32,
    time: f32,
) -> Vec<FireParticle> {
    use std::f32::consts::TAU;
    // Angular spacing of wall samples at their FINAL radius (tiles).
    const SPACING: f32 = 0.55;
    // How long a wall lingers (fading) after its front stops (ticks).
    const FADE_T: f32 = 1.3;
    let hash = |a: u32| {
        let mut x = a.wrapping_mul(0x9E37_79B9);
        x ^= x >> 15;
        x = x.wrapping_mul(0x85EB_CA6B);
        x ^= x >> 13;
        (x & 0xFFFF) as f32 / 65536.0
    };
    let mut out = Vec::new();
    for b in ledger.blasts() {
        let t = b.elapsed + alpha;
        let seed_base = b.slot as u32 * 131 + b.generation;
        // Walls are computed per COMB CYCLE (a >11-pass driver — the
        // doomsday sphere — re-runs the two-wave sweep every 11
        // passes). The previous cycle is evaluated too: its wave-2
        // fade tail overhangs the next cycle's start.
        let (cbase, cp) = b.cycle_at(t);
        let mut cycles = [(cbase, cp), (0.0, 0.0)];
        if cbase >= LedgerBlast::CYCLE {
            cycles[1] = (cbase - LedgerBlast::CYCLE, LedgerBlast::CYCLE);
        }
        for (ci, &(cb, cp)) in cycles.iter().enumerate().filter(|(_, c)| c.1 > 0.0) {
            let tl = t - cb; // cycle-local time
            let w1_end = LedgerBlast::wave1_end_of(cp);
            let r1 = RING_PITCH * 2.0 * (tl - 1.0).clamp(0.0, w1_end - 1.0);
            // Wave 2 (odd back-fill) exists only in a cycle running
            // past pass 6; it eases out of the centre over the tick
            // before its first ring.
            let wave2 = (cp >= 7.0 && tl >= 6.0).then(|| {
                let tt = (tl - 7.0).clamp(-1.0, cp - 7.0);
                (
                    (RING_PITCH * (2.0 * tt + 1.0)).max(0.0),
                    RING_PITCH * (2.0 * (cp - 7.0) + 1.0),
                    cp,
                    // The re-burn crosses ground wave 1 already torched:
                    // visibly weaker, more ember than blaze.
                    0.72,
                )
            });
            // (front radius now, final radius, cycle-local end, strength)
            let waves = [
                Some((r1, LedgerBlast::wave1_max_of(cp), w1_end, 1.0)),
                wave2,
            ];
            for (wi, wave) in waves.into_iter().enumerate() {
                let wi = (wi + ci * 2) as u32; // decorrelate cycle seeds
                let Some((r, r_max, end, strength)) = wave else {
                    continue;
                };
                if r <= 0.05 || r_max <= 0.05 {
                    continue;
                }
                // Fade out over FADE_T after the front finishes; the
                // rim hand-over goes to the lingering crater smoke.
                let fade = (1.0 - (tl - end) / FADE_T).clamp(0.0, 1.0);
                if fade <= 0.02 {
                    continue;
                }
                // Bunched samples at small radii overlap heavily — thin
                // them so the newborn wall reads as one bright ball,
                // not a blown-out core.
                let converge = (r / (r + 1.2)).clamp(0.25, 1.0);
                let n = ((TAU * r_max / SPACING) as u32).max(10);
                for k in 0..n {
                    let ph = hash(seed_base ^ (k * 7919) ^ (wi * 331));
                    let ang = (k as f32 + ph * 0.7) / n as f32 * TAU;
                    let (ox, oz) = (ang.cos(), ang.sin());
                    let flick = 0.8 + 0.2 * (time * 11.0 + ph * 40.0).sin();
                    // Two stacked discs per sample on the radial axis:
                    // a hot LEADING head just past the front and a
                    // cooling trail behind it — the same outer-hot →
                    // inner-soot gradient the smoke streaks carry, so
                    // heavy tangential overlap reinforces the direction
                    // instead of washing it out.
                    for (axis, heat, aw, ah, al) in [
                        (0.30, (0.9 + 0.1 * ph) * flick, 1.05, 1.15, 0.9),
                        (-0.75, 0.28 * flick, 1.3, 1.2, 0.72),
                    ] {
                        let rad = (r + axis + (ph - 0.5) * 0.3).max(0.05);
                        let x = (b.x + ox * rad).rem_euclid(MAP_TILES as f32);
                        let z = (b.z + oz * rad).rem_euclid(MAP_TILES as f32);
                        let y = b.plane_z.max(terrain_at(height, x, z)) + 0.4;
                        out.push(FireParticle {
                            x,
                            z,
                            y: y + 0.12 * (time * 6.0 + ph * 20.0).sin(),
                            w: aw,
                            h: ah,
                            // The post-front fade also COOLS to soot:
                            // the dying wall must read as fire burning
                            // out into the smoke, not a lingering hot
                            // ring — on a short-fused blast (MC2 low
                            // tiers) that ring flashed like a phantom
                            // second wave.
                            heat: (heat * strength * fade).clamp(0.0, 1.0),
                            alpha: al * fade * strength * converge,
                            seed: ph * TAU + time * 4.0,
                        });
                    }
                }
            }
        }

        // --- Crater smoke: virtual soot cells, comb-scheduled --------
        // One ring per completed pass, in the driver's real firing
        // order. Each virtual cell fades in over its first tick (the
        // wall sweeping over it is its flash), drifts inward, and
        // dissolves over a radius-dependent lifetime: inner ≈ 3 ticks
        // (the crater centre clears while the blast still burns), rim
        // ≈ 14 (its smoke lingers ~8 ticks past the driver's death).
        for p in 1..=(t.floor().clamp(0.0, b.passes) as u32) {
            let r = (2 * (p - 1)) % 11; // the cyclic ring table
            let vlife = (3 + r).min(14) as f32;
            let elapsed = t - p as f32;
            if elapsed > 14.0 {
                continue; // long past dissolve (long drivers: old cycles)
            }
            let age = (elapsed / vlife).clamp(0.0, 1.0);
            // Dissolve a bit faster than the full life (fewer stacked
            // puffs in the aged inner pile), fade in over ~0.8 ticks.
            let fade = (1.0 - age * 1.25).clamp(0.0, 1.0) * (elapsed / 0.8).clamp(0.0, 1.0);
            if fade <= 0.02 {
                continue;
            }
            let rt = RING_PITCH * r as f32;
            let inward = age * 0.9;
            let n = (8 * r).max(4);
            for k in 0..n {
                let cs = seed_base ^ (p * 8887) ^ (k * 271);
                let ph = hash(cs);
                let ang = (k as f32 + ph) / n as f32 * TAU;
                let (ox, oz) = (ang.cos(), ang.sin());
                // Two discs stacked on the radial axis (outer + inner
                // soot): the streak layout the cell smoke always had,
                // sliding toward the crater centre as it ages.
                for j in 0..2u32 {
                    let s = j as f32;
                    let ph2 = hash(cs ^ (j * 2657));
                    let wob = (time * 7.0 + ph2 * 30.0).sin();
                    let axis = 0.4 - s * 1.3 - inward;
                    let jit = (ph2 - 0.5) * (0.3 + 0.35 * age);
                    let rad = rt + (ph - 0.5) * 0.5 + axis * 1.2;
                    let x = (b.x + ox * rad - oz * jit * 1.2).rem_euclid(MAP_TILES as f32);
                    let z = (b.z + oz * rad + ox * jit * 1.2).rem_euclid(MAP_TILES as f32);
                    let y = b.plane_z.max(terrain_at(height, x, z)) + 0.4;
                    out.push(FireParticle {
                        x,
                        z,
                        y: y + 0.1 * age + 0.05 * wob,
                        w: (1.05 - 0.1 * s + 0.35 * age) * 1.2,
                        h: (1.1 - 0.1 * s + 0.3 * age) * 1.2,
                        heat: 0.0,
                        alpha: fade * (0.95 - 0.06 * s),
                        seed: ph2 * TAU + time * 4.0,
                    });
                }
            }
        }
    }
    out
}

/// PROTOTYPE meteor shockwave: ONE soft translucent ring per tracked
/// blast — the pressure wave DETACHING from the fire at the end of the
/// wave-1 sweep. It does not lead the flame: it materializes at the
/// fire's edge just as the front stops expanding, then runs on outward
/// as the blast's continuation — the explosion's momentum outliving its
/// flame — decelerating and fading. Scale and timing derive from the
/// blast's own pass schedule, so a short-fused MC2 tier makes a small,
/// quick puff at its own (smaller) fire edge.
/// The ring is DRAPED over the terrain: its altitude is `max(plane_z,
/// terrain_z)` — a flat floor at the detonation plane that climbs only
/// where terrain rises above it (peaks/walls), never sinking below.
/// Emitted as [`FireParticle`]s with a heat sentinel (`< 0`) that the
/// fire shader renders as a cool vapor band instead of flame.
pub fn shockwave_particles(
    ledger: &BlastLedger,
    height: &[u8],
    alpha: f32,
    time: f32,
) -> Vec<FireParticle> {
    use std::f32::consts::TAU;
    // The band fades in slightly BEFORE the front stops (ticks) — the
    // hand-over reads as continuous rather than a pop at the end.
    const START_LEAD: f32 = 0.6;
    // How long the detached wave runs on past the fire edge (ticks).
    const RUN_T: f32 = 2.4;
    let mut out = Vec::new();
    for b in ledger.blasts() {
        // Per comb cycle: a >11-pass driver (the doomsday sphere)
        // detaches a fresh pressure ring at the end of EVERY cycle's
        // wave-1 sweep. (The wave finishes ~3 ticks before the cycle
        // does, so cycles never overlap.)
        let t = b.elapsed + alpha;
        let (cbase, cp) = b.cycle_at(t);
        let end_local = LedgerBlast::wave1_end_of(cp);
        if end_local <= 1.0 {
            continue; // degenerate cycle tail: no front to detach from
        }
        let w1_max = LedgerBlast::wave1_max_of(cp);
        // Run distance scales with the blast: a tier-1 MC2 puff sends
        // a small short wave, the full meteor its ~2.7-tile ring.
        let run_dist = 0.8 + 0.3 * w1_max;
        let t0 = cbase + end_local - START_LEAD;
        let phase = ((t - t0) / RUN_T).clamp(0.0, 1.0);
        // Born straddling the wall's final front (inner rim inside the
        // fire — the continuation), then the center runs outward with
        // an ease-out: fast on detachment, decelerating as it dies.
        let run = 1.0 - (1.0 - phase) * (1.0 - phase);
        let center = w1_max - 0.2 + run_dist * run;
        // Quick fade-in out of the flame, slow fade-out as it runs.
        let fade = (phase / 0.15).clamp(0.0, 1.0) * (1.0 - phase * phase);
        if phase <= 0.0 || fade <= 0.02 || center < 0.4 {
            continue;
        }
        // Fine angular sampling → continuous band regardless of size.
        let n = (24.0 + center * 12.0) as u32;
        for k in 0..n {
            let ang = k as f32 / n as f32 * TAU;
            let (ox, oz) = (ang.cos(), ang.sin());
            let x = (b.x + ox * center).rem_euclid(MAP_TILES as f32);
            let z = (b.z + oz * center).rem_euclid(MAP_TILES as f32);
            let y = b.plane_z.max(terrain_at(height, x, z)) + 0.12;
            out.push(FireParticle {
                x,
                y,
                z,
                // ≈1.3-tile band radially; a touch flatter than round.
                w: 0.85,
                h: 0.7,
                heat: -1.0,        // shockwave sentinel (grey vapor band)
                alpha: fade * 0.3, // much subtler than v1
                seed: ang * 3.0 + time,
            });
        }
    }
    out
}

/// PROTOTYPE overdraw cap (`MGC_FIRE_CAP`, particles per ~1.2-tile ground
/// cell; default 6, 0 = off): a massive blast stacks hundreds of big
/// overlapping quads, and past a handful of layers the extra ones add
/// almost nothing but fill cost. This keeps the first `max_per` particles
/// whose centers fall in each ground cell and drops the rest — so the
/// sparse flame front and shockwave pass untouched while the dense inner
/// pile is bounded. The crater cells are stationary and generation order
/// is deterministic, so the kept set is stable frame-to-frame (no flicker).
/// DIAGNOSTIC (`MGC_FIRE_DEBUG`): once every ~60 calls, print a
/// one-line health summary of the assembled fire set — the tool for
/// catching the sticky, global fire-corruption "tripwire" live. A
/// poisoned process-lifetime scalar shows up as a non-finite count > 0,
/// or a `seed`/position/size extreme far outside the normal range
/// (seed carries `effect_time*4`, so a runaway clock reads as a huge
/// |seed|; a blown-up finite value reads as a huge pos/size).
pub fn debug_fire_stats(particles: &[FireParticle], effect_time: f32) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static TICK: AtomicU32 = AtomicU32::new(0);
    if TICK.fetch_add(1, Ordering::Relaxed) % 60 != 0 {
        return;
    }
    let mut nonfinite = 0usize;
    let (mut max_pos, mut max_size, mut max_seed, mut max_alpha) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for p in particles {
        let fields = [p.x, p.y, p.z, p.w, p.h, p.heat, p.alpha, p.seed];
        if fields.iter().any(|v| !v.is_finite()) {
            nonfinite += 1;
        }
        max_pos = max_pos.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
        max_size = max_size.max(p.w.abs()).max(p.h.abs());
        max_seed = max_seed.max(p.seed.abs());
        max_alpha = max_alpha.max(p.alpha);
    }
    println!(
        "FIRE n={} t={:.1} nonfinite={} maxpos={:.1} maxsize={:.2} maxseed={:.1} maxalpha={:.2}",
        particles.len(),
        effect_time,
        nonfinite,
        max_pos,
        max_size,
        max_seed,
        max_alpha,
    );
}

pub fn cap_particle_density(particles: Vec<FireParticle>) -> Vec<FireParticle> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static CAP: OnceLock<usize> = OnceLock::new();
    let max_per = *CAP.get_or_init(|| {
        std::env::var("MGC_FIRE_CAP")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(6)
    });
    if max_per == 0 {
        return particles;
    }
    const CELL: f32 = 1.2;
    let mut counts: HashMap<(i32, i32), usize> = HashMap::new();
    let mut kept = Vec::with_capacity(particles.len());
    for p in particles {
        let key = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
        let c = counts.entry(key).or_insert(0);
        if *c < max_per {
            *c += 1;
            kept.push(p);
        }
    }
    kept
}

/// The Beyond-Sight rival position markers (interim for the retail
/// name labels, :57413-48): a 2x2 dot in the rival's team color at
/// each live, non-cloaked rival wizard.
pub fn rival_markers(
    rivals: &[mgc_sim::engine::world::RivalView],
    beyond_sight: Option<u8>,
) -> Vec<mgc_render::MapDot> {
    let Some(tier) = beyond_sight else {
        return Vec::new();
    };
    // Tier 0 excludes Invisible rivals; tier ≥ 1 (Mana-Lock sight)
    // reveals them too (docs/spell-audit/beyond-sight.md).
    rivals
        .iter()
        .filter(|r| r.alive && (tier >= 1 || !r.invisible))
        .map(|r| mgc_render::MapDot {
            x: r.x,
            z: r.z,
            color: TEAM_COLORS[(r.slot as usize).min(7)].1,
            size: 2,
        })
        .collect()
}

/// Resolve one type index to a billboard at a world position; skips
/// rows whose size cannot be resolved (missing sprite dims).
/// The replay GHOST (`--replay`): the recorded pose drawn as a
/// translucent billboard (blend 2 — the 33%-opaque raster mode) of
/// the wizard-carpet sprite, feet at the recorded altitude like any
/// flying pose.
pub fn ghost_billboard(
    game: GameId,
    type_index: u16,
    x: f32,
    alt: f32,
    z: f32,
    yaw: f32,
    sprite_dims: &impl Fn(u16) -> Option<(u16, u16, u16)>,
) -> Option<Billboard> {
    let s = resolve_pose_sprite(game, type_index, sprite_dims)?;
    Some(Billboard {
        x: x.rem_euclid(MAP_TILES as f32),
        y: alt,
        z: z.rem_euclid(MAP_TILES as f32),
        yaw,
        sprite_base: s.sprite_base,
        draw_type: s.draw_type,
        frame: 0,
        world_h: s.world_h,
        blend: 2,
        // An instrument, not a retail entity — never distance-hidden,
        // and on no tile chain, so it takes the neutral co-tile rank.
        conceal: false,
        chain_depth: 0.5,
    })
}

fn push_billboard(
    out: &mut Vec<Billboard>,
    height: &[u8],
    sprite_dims: &impl Fn(u16) -> Option<(u16, u16, u16)>,
    type_index: u16,
    x: f32,
    z: f32,
    yaw: f32,
) {
    let Some(stats) = SPRITE_STATS.get(type_index as usize) else {
        return;
    };
    // World height; a 0 height derives from width and the base
    // sprite's pixel aspect (the original's load-time fixup).
    let world_h = if stats.height != 0 {
        stats.height as f32 / UNITS_PER_TILE
    } else {
        let Some((sw, sh, _)) = sprite_dims(stats.sprite_base) else {
            return;
        };
        if sw == 0 || stats.width == 0 {
            return;
        }
        stats.width as f32 * sh as f32 / sw as f32 / UNITS_PER_TILE
    };

    out.push(Billboard {
        x: x.rem_euclid(MAP_TILES as f32),
        y: ground_height(height, x, z),
        z: z.rem_euclid(MAP_TILES as f32),
        yaw,
        sprite_base: stats.sprite_base,
        draw_type: stats.draw_type,
        frame: 0,
        world_h,
        blend: 0,
        conceal: false,
        // The static THING-list path (`--no-terrain-features`
        // comparison renders): no live pool, so no tile chain to
        // read — every sprite takes the neutral co-tile rank and the
        // co-tile tie falls back to submission order, as before.
        chain_depth: 0.5,
    });
}

/// Entity dots for the overhead map, one pixel per entity, colored as
/// the original's map overlay (remc1 sub_48710_48A50, :57050): the
/// draw switches on LIVE entity class — trees/scenery (live class 2,
/// spawned state 0) = raw palette index 28; wild creatures =
/// near-black, wizard-owned creatures = the owner's team color
/// (byte_99B58; team 0 = the player's blue family), villagers (class
/// 5 models 12-14) = dark green; pre-placed mana/spell pickup jars
/// (class 12) = bright red — the vital red dots. "Computed" colors go
/// through the engine's 16x16x16 RGB->palette LUT (`byte_AD167_AD157`,
/// nearest-palette-match of RGB(3+4r, 3+4g, 3+4b) in 6-bit VGA), which
/// [`nearest_palette_index`] reproduces against the bundle palette.
///
/// Not replicated yet:
/// - Castle markers are team-colored UI-SPRITE ICONS in the original
///   (begSprTab 58+team / 66+team; balloons 83/84) — pending the
///   HSPR/UI-sprite bake; until then castles get a team-blue dot.
/// - Runtime loose mana balls (the orange / blinking claimed dots)
///   are live-class-2 models 1/3 entities spawned at runtime, not
///   level records — they land with mana mechanics.
/// - Dot blinking, the 2x2 grown dot of one creature sub-case, rival
///   name labels (runtime state, not placement).
pub fn map_dots(things: &[Thing], palette: &[[u8; 4]; 256]) -> Vec<mgc_render::MapDot> {
    let near_black = nearest_palette_index(palette, vga(7, 3, 3));
    let dark_green = nearest_palette_index(palette, vga(3, 7, 3));
    let red = nearest_palette_index(palette, vga(63, 3, 7));
    const SCENERY: u8 = 28;
    const PLAYER_TEAM_BLUE: u8 = 0x71; // byte_99B58[1 + 2*0]

    let mut out = Vec::new();
    for t in things {
        if t.kind != ThingKind::Entity {
            continue;
        }
        let color = match (t.class, t.model) {
            (2, _) => SCENERY,
            // Castle markers only (the original draws live class 3
            // from model 2 up; models 0/1 are the player balloon —
            // the map's center cross — and 3 needs rivals revealed).
            (3, 2) => PLAYER_TEAM_BLUE,
            (5, 12..=14) => dark_green,
            (5, _) => near_black,
            (12, _) => red,
            _ => continue,
        };
        out.push(mgc_render::MapDot {
            x: t.x as f32 + 0.5,
            z: t.y as f32 + 0.5,
            color,
            size: 1,
        });
    }
    out
}

/// Expand a 6-bit VGA triple the way the bundle palette was baked.
fn vga(r: u8, g: u8, b: u8) -> [u8; 3] {
    [
        (r << 2) | (r >> 4),
        (g << 2) | (g >> 4),
        (b << 2) | (b >> 4),
    ]
}

/// Per-slot map-roster color pair `(box, text)` as RGBA, resolved
/// through the level palette. MC1: the `byte_99B58` team pairs (even =
/// box tint, odd = text — sub_22880 :27087-88). MC2: the map-env
/// `playersColors_E88E0x` table, `[0]` = border, `[1]` = text
/// (DrawSorcererScores_2D1D0 EF:22277-78); night/cave use the night
/// table with the cave's wizard-0 override, same as the dot pass.
pub fn roster_team_colors(
    game: GameId,
    env: Mc2MapEnv,
    palette: &[[u8; 4]; 256],
) -> [([f32; 4], [f32; 4]); 8] {
    let rgba = |idx: u8| pal_ui_rgba(palette, idx);
    let tab = if game == GameId::Mc2 {
        match env {
            Mc2MapEnv::Day => MC2_TEAM_DAY,
            Mc2MapEnv::Night | Mc2MapEnv::Cave => {
                let mut t = MC2_TEAM_NIGHT;
                if env == Mc2MapEnv::Cave {
                    t[0] = (0xE0, 0x58);
                }
                t
            }
        }
    } else {
        TEAM_COLORS
    };
    tab.map(|(a, b)| (rgba(a), rgba(b)))
}

/// One sRGB byte decoded to linear. Solid UI tints are written RAW
/// onto the sRGB swapchain (the shader returns the tint, the surface
/// encodes) — so a palette byte fed straight through gets
/// gamma-encoded TWICE and washes out lighter than the retail
/// framebuffer. Decode here so the display round-trips back to the
/// palette color. (Atlas SPRITES are unaffected: their textures are
/// sRGB-typed and decode on sample.)
fn srgb_lin(b: u8) -> f32 {
    let c = b as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A palette index as a linear RGBA solid-UI tint (see [`srgb_lin`]).
fn pal_ui_rgba(palette: &[[u8; 4]; 256], idx: u8) -> [f32; 4] {
    let p = palette[idx as usize];
    [srgb_lin(p[0]), srgb_lin(p[1]), srgb_lin(p[2]), 1.0]
}

/// The rival tag's box chrome (`DrawSorcererNameAndHealthBar_2CB30`,
/// remc2 GameRenderHD.cpp:2824-32): box background + top/left bevel +
/// bottom/right bevel come from the map-type building-parameter row
/// `str_D94F0_bldgprmbuffer[MapType][{0,2,3}]`
/// (Type_D94F0_Bldgprmbuffer.cpp:3), the empty health-bar backdrop
/// from palette index 0 (`m_ptrColorPalette[0]`).
pub struct TagChrome {
    pub bg: [f32; 4],
    pub bevel_tl: [f32; 4],
    pub bevel_br: [f32; 4],
    pub bar_empty: [f32; 4],
}

/// Resolve the tag chrome. MC2 resolves the retail indices through
/// the LIVE level palette (retail reads its active palette buffer).
/// MC1 has no MC2 palette in reach, so the opt-in tag carries the Day
/// row pre-sampled from the retail palette (PALD-0.DAT: 0xAA =
/// (101,101,101), 0x63 = (158,162,178), 0x0D = (32,28,24), index 0 =
/// black).
pub fn rival_tag_chrome(game: GameId, env: Mc2MapEnv, palette: &[[u8; 4]; 256]) -> TagChrome {
    if game == GameId::Mc2 {
        let (bg, tl, br) = match env {
            Mc2MapEnv::Day => (0xAA, 0x63, 0x0D),
            Mc2MapEnv::Night => (0x33, 0x11, 0x3B),
            Mc2MapEnv::Cave => (0x33, 0x88, 0x3B),
        };
        TagChrome {
            bg: pal_ui_rgba(palette, bg),
            bevel_tl: pal_ui_rgba(palette, tl),
            bevel_br: pal_ui_rgba(palette, br),
            bar_empty: pal_ui_rgba(palette, 0),
        }
    } else {
        let rgb = |r, g, b| [srgb_lin(r), srgb_lin(g), srgb_lin(b), 1.0];
        TagChrome {
            bg: rgb(101, 101, 101),
            bevel_tl: rgb(158, 162, 178),
            bevel_br: rgb(32, 28, 24),
            bar_empty: rgb(0, 0, 0),
        }
    }
}

/// Nearest palette entry by squared RGB distance (the engine's
/// `sub_5CC70_5D180` palette-match used to build its RGB LUT).
fn nearest_palette_index(palette: &[[u8; 4]; 256], rgb: [u8; 3]) -> u8 {
    let mut best = (0usize, u32::MAX);
    for (i, p) in palette.iter().enumerate() {
        let d = p[..3]
            .iter()
            .zip(rgb)
            .map(|(&a, b)| {
                let d = a as i32 - b as i32;
                (d * d) as u32
            })
            .sum();
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0 as u8
}

/// The single-player start: the class-3 model-4 marker in BOTH games
/// (player start #0 of 8; the original's marker spawner copies its
/// position into the per-player start table, sub_37720 :44068 — every
/// shipped MC2 single-player level authors exactly one). MC2's
/// (10, 0x52) records are cave ROOM CARVERS (GenerateEvents pass 1,
/// remc2 Events.cpp:162-170 → PrepareEvents case 0x52 = authored box
/// extents), NOT wizard starts — the (3, 4) marker is the only start
/// path. Returns tile-center
/// coordinates. Neither game stores an orientation (both wizards spawn
/// at engine yaw 0 = our north); altitude re-derives at spawn from
/// ground height (MC2 places at terrain alt exactly — hover is flight
/// physics, not spawn state).
pub fn player_start(_game: GameId, things: &[Thing]) -> Option<(f32, f32)> {
    things
        .iter()
        .find(|t| t.kind == ThingKind::Entity && t.class == 3 && t.model == 4)
        .map(|t| (t.x as f32 + 0.5, t.y as f32 + 0.5))
}

/// Spawn altitude above ground (the original's `sub_11F50 + 1` hover;
/// exact engine scaling still to pin down in the flight-model port).
pub const START_HOVER: f32 = 1.0;

/// Ground altitude at a position (public for spawn placement).
pub fn ground_at(height: &[u8], x: f32, z: f32) -> f32 {
    ground_height(height, x, z)
}

/// Bilinear ground altitude from the corner-based height grid.
fn ground_height(height: &[u8], x: f32, z: f32) -> f32 {
    if height.len() != MAP_TILES * MAP_TILES {
        return 0.0;
    }
    let n = MAP_TILES;
    let (fx, fz) = (x.rem_euclid(n as f32), z.rem_euclid(n as f32));
    let (x0, z0) = (fx.floor() as usize % n, fz.floor() as usize % n);
    let (x1, z1) = ((x0 + 1) % n, (z0 + 1) % n);
    let (tx, tz) = (fx.fract(), fz.fract());
    let h = |xx: usize, zz: usize| height[zz * n + xx] as f32 * HEIGHT_SCALE;
    let top = h(x0, z0) * (1.0 - tx) + h(x1, z0) * tx;
    let bot = h(x0, z1) * (1.0 - tx) + h(x1, z1) * tx;
    top * (1.0 - tz) + bot * tz
}

// ---- PROTOTYPE enhanced lightning ----------------------------------------

/// One latched strike aging across render frames. Geometry is FROZEN
/// per strike (a real channel doesn't wander — the stream dances
/// because each RAPID re-fire is a new strike); only intensity
/// animates.
struct LedgerBolt {
    start: [f32; 3],
    end: [f32; 3],
    /// Whole sim ticks since the strike (sub-tick comes from alpha).
    age: f32,
    seed: u32,
}

/// The strike ledger (sibling of [`BlastLedger`]): latches the sim's
/// hash-quiet [`BoltStrike`] feed each tick and ages strikes across
/// frames so the envelope (leader → return stroke → decay) can
/// overlap successive strikes into a continuous stream.
#[derive(Default)]
pub struct BoltLedger {
    bolts: Vec<LedgerBolt>,
    counter: u32,
}

/// Total strike life in ticks (24 Hz): leader + stroke + decay.
const BOLT_LIFE: f32 = 3.6;
/// Leader phase end — the channel grows muzzle→target until here.
const BOLT_LEADER_END: f32 = 0.65;
/// Return-stroke end — full brightness until here, then decay.
const BOLT_STROKE_END: f32 = 1.6;
/// Ribbon half-width of the main channel, in tiles.
const BOLT_CORE_W: f32 = 0.13;

impl BoltLedger {
    /// One sim tick: age everything, retire the dead, latch the new.
    pub fn update(&mut self, strikes: Vec<BoltStrike>, steps: f32) {
        for b in &mut self.bolts {
            b.age += steps;
        }
        self.bolts.retain(|b| b.age < BOLT_LIFE);
        let conv =
            |(x, y, z): (u16, u16, i16)| [x as f32 / 256.0, z as f32 / 256.0, y as f32 / 256.0];
        // Resolve `end` to the nearest torus image of `start` (256-tile
        // wrap on x/z): a strike whose sim coords straddle the map seam
        // (a beam marching across the u16 edge — `disp` is a wrapping_add)
        // would otherwise convert to endpoints ~256 tiles apart, and
        // `bolt_channel` would draw a channel clear across the map. Pinning
        // `end` beside `start` keeps every bolt a short, coherent primitive
        // so the renderer's rigid anchor wrap places it correctly.
        let torus = |start: [f32; 3], mut end: [f32; 3]| {
            for a in [0usize, 2] {
                let d = end[a] - start[a];
                if d > 128.0 {
                    end[a] -= 256.0;
                } else if d < -128.0 {
                    end[a] += 256.0;
                }
            }
            end
        };
        for s in strikes {
            self.counter = self.counter.wrapping_add(1);
            let start = conv(s.start);
            self.bolts.push(LedgerBolt {
                start,
                end: torus(start, conv(s.end)),
                age: 0.0,
                seed: self
                    .counter
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(s.owner as u32),
            });
        }
    }

    pub fn clear(&mut self) {
        self.bolts.clear();
        self.counter = 0;
    }
}

/// Deterministic hash → [0,1) (the fire emitter's phase hash).
fn hash01(a: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65536.0
}

fn v_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn v_scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn v_len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn v_norm(a: [f32; 3]) -> [f32; 3] {
    let l = v_len(a).max(1e-6);
    v_scale(a, 1.0 / l)
}
fn v_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn v_lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Fractal channel: recursive midpoint displacement between two world
/// points — long straight-ish runs broken by kinks with detail at
/// every scale, the actual shape of a lightning channel (the retail
/// ±1 random-walk zigzag is uniform noise by comparison). Offsets are
/// drawn in the chord's perpendicular plane from the strike seed, so
/// the shape is FROZEN for the strike's life. Endpoint-pinned: the
/// bolt genuinely starts at the muzzle and ends on the victim.
fn bolt_channel(start: [f32; 3], end: [f32; 3], seed: u32, depth: u32) -> Vec<[f32; 3]> {
    let chord = v_sub(end, start);
    let clen = v_len(chord);
    let n = v_norm(chord);
    // Perpendicular basis (u horizontal-ish, v the other axis).
    let helper = if n[1].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = v_norm(v_cross(n, helper));
    let v = v_cross(n, u);
    let mut pts = vec![start, end];
    for d in 0..depth {
        let amp = clen * 0.16 * 0.5f32.powi(d as i32);
        let mut next = Vec::with_capacity(pts.len() * 2 - 1);
        for (i, pair) in pts.windows(2).enumerate() {
            let ih = (i as u32).wrapping_mul(2_654_435_761);
            let h1 = hash01(seed ^ (d << 12) ^ ih) - 0.5;
            let h2 = hash01(seed ^ (d << 12) ^ ih ^ 0x5555) - 0.5;
            let mid = v_lerp(pair[0], pair[1], 0.5);
            let off = v_add(v_scale(u, h1 * amp), v_scale(v, h2 * amp * 0.6));
            next.push(pair[0]);
            next.push(v_add(mid, off));
        }
        next.push(*pts.last().unwrap());
        pts = next;
    }
    pts
}

/// Build the frame's bolt segments from the ledger: per strike, the
/// envelope picks the phase —
/// - LEADER (0..0.65 ticks): a thin dim channel GROWS muzzle→target
///   (the buildup);
/// - RETURN STROKE (0.65..1.6): full-brightness core + branches, with
///   high-frequency flicker;
/// - DECAY (1.6..3.6): branches die first, the core thins and fades.
///
/// Held RAPID fire lands a fresh strike every tick, so envelopes
/// overlap — the new leader climbs while the old stroke decays; the
/// stream stays continuous and dances between re-strikes.
pub fn bolt_segments(ledger: &BoltLedger, alpha: f32, time: f32) -> Vec<BoltSegment> {
    let mut out = Vec::new();
    for b in &ledger.bolts {
        let t = b.age + alpha.clamp(0.0, 1.0);
        if t >= BOLT_LIFE {
            continue;
        }
        let (energy, width_k, fade, grow, branches) = if t < BOLT_LEADER_END {
            (0.35, 0.5, 0.9, t / BOLT_LEADER_END, false)
        } else if t < BOLT_STROKE_END {
            (1.0, 1.0, 1.0, 1.0, true)
        } else {
            let k = 1.0 - (t - BOLT_STROKE_END) / (BOLT_LIFE - BOLT_STROKE_END);
            (k * k, 0.55 + 0.45 * k, 0.4 + 0.6 * k, 1.0, k > 0.55)
        };
        let pts = bolt_channel(b.start, b.end, b.seed, 5);
        let segs = pts.len() - 1;
        let drawn = ((segs as f32) * grow).ceil().max(1.0) as usize;
        let anchor = [b.start[0], b.start[2]];
        let emit = |out: &mut Vec<BoltSegment>, p0: [f32; 3], p1: [f32; 3], w: f32, e: f32, idx| {
            out.push(BoltSegment {
                p0,
                p1,
                // The strike origin, shared by every segment/branch of
                // this bolt — the renderer wraps the bolt to the camera
                // as a rigid unit around it (see `BoltSegment::anchor`).
                anchor,
                width: w,
                energy: e,
                alpha: fade,
                seed: hash01(b.seed ^ (idx as u32).wrapping_mul(977)) * std::f32::consts::TAU
                    + time * 6.0,
            });
        };
        for (i, pair) in pts.windows(2).take(drawn).enumerate() {
            let mut p1 = pair[1];
            // Partial tip while the leader grows.
            if i + 1 == drawn && grow < 1.0 {
                let frac = (segs as f32 * grow) - i as f32;
                p1 = v_lerp(pair[0], pair[1], frac.clamp(0.05, 1.0));
            }
            emit(&mut out, pair[0], p1, BOLT_CORE_W * width_k, energy, i);
        }
        if branches {
            // 2-3 side branches forked off upper-level kinks: dimmer,
            // thinner, dying with the stroke — the thing that reads
            // "lightning" instead of "noodle". One level only.
            let count = 2 + (b.seed & 1) as usize;
            for k in 0..count {
                let h = hash01(b.seed ^ (k as u32).wrapping_mul(7919));
                // Fork in the UPPER part of the channel only (12-45%
                // along): real side leaders split early and die off;
                // a branch near the terminus reads as a misfire.
                let at = ((0.12 + 0.33 * h) * segs as f32) as usize;
                let p = pts[at.min(pts.len() - 2)];
                let dir = v_norm(v_sub(pts[at.min(pts.len() - 2) + 1], p));
                let helper = if dir[1].abs() > 0.9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                };
                let lat = v_norm(v_cross(dir, helper));
                let sign = if hash01(b.seed ^ (k as u32).wrapping_mul(104_729)) < 0.5 {
                    -1.0
                } else {
                    1.0
                };
                // Veer well OFF the channel heading — a branch that
                // parallels the main bolt toward the target reads as
                // a second (missing) bolt.
                let bdir = v_norm(v_add(
                    v_scale(dir, 0.4),
                    v_scale(lat, sign * (0.6 + 0.4 * h)),
                ));
                // Short: a fraction of the remaining distance, hard
                // capped — side leaders exhaust quickly.
                let blen = (v_len(v_sub(b.end, p)) * (0.10 + 0.10 * h)).min(1.6);
                let bend = v_add(p, v_scale(bdir, blen));
                let bpts = bolt_channel(p, bend, b.seed ^ 0xB1A5 ^ (k as u32) << 16, 3);
                for (i, pair) in bpts.windows(2).enumerate() {
                    emit(
                        &mut out,
                        pair[0],
                        pair[1],
                        BOLT_CORE_W * width_k * 0.42,
                        energy * 0.34,
                        i + 64 + k * 16,
                    );
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strike envelope: the leader GROWS from the muzzle (partial
    /// channel, endpoint short of the target), the return stroke is
    /// full-length and endpoint-pinned on the target, decay fades and
    /// eventually retires the strike.
    #[test]
    fn bolt_envelope_grows_then_pins_then_dies() {
        let mut led = BoltLedger::default();
        let strike = BoltStrike {
            start: (256, 256, 100),
            end: (2560, 256, 100),
            owner: 0xFFFF,
        };
        led.update(vec![strike], 1.0);
        // Mid-leader (age 0 + alpha 0.3): channel truncated.
        let segs = bolt_segments(&led, 0.3, 0.0);
        assert!(!segs.is_empty(), "the leader draws");
        let far = segs.iter().map(|s| s.p1[0]).fold(f32::MIN, f32::max);
        assert!(
            far < 9.5,
            "mid-leader the channel has not reached the target (far x {far})"
        );
        // Return stroke (age 1 + alpha 0.2): full length, endpoint
        // pinned on the victim (end x = 2560/256 = 10 tiles).
        led.update(Vec::new(), 1.0);
        let segs = bolt_segments(&led, 0.2, 0.0);
        let far = segs.iter().map(|s| s.p1[0]).fold(f32::MIN, f32::max);
        assert!(
            (far - 10.0).abs() < 0.05,
            "the stroke ends ON the target (far x {far})"
        );
        let peak = segs.iter().map(|s| s.energy).fold(0.0f32, f32::max);
        assert!(peak >= 0.99, "return stroke at full energy");
        // Past the life: retired.
        led.update(Vec::new(), 3.0);
        assert!(
            bolt_segments(&led, 0.0, 0.0).is_empty(),
            "the strike retires after its life"
        );
    }

    /// A beam whose sim coords straddle the u16 map seam (start at tile
    /// 255, end at tile 2 — truly ~3 tiles apart across the 256-tile wrap,
    /// not 253) must resolve `end` beside `start` so the channel stays a
    /// short coherent primitive. Non-vacuity: without the torus resolution
    /// in `update`, `bolt_channel` would run 255→2, a span clear across
    /// the whole map, failing the assert.
    #[test]
    fn seam_crossing_strike_stays_a_short_coherent_channel() {
        let mut led = BoltLedger::default();
        let strike = BoltStrike {
            start: (65280, 256, 100), // world x = 255.0
            end: (512, 256, 100),     // world x = 2.0 raw → 258.0 resolved
            owner: 7,
        };
        led.update(vec![strike], 1.0);
        // Return stroke (age 1 + alpha): full-length channel.
        led.update(Vec::new(), 1.0);
        let segs = bolt_segments(&led, 0.2, 0.0);
        assert!(!segs.is_empty(), "the stroke draws");
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for s in &segs {
            for x in [s.p0[0], s.p1[0]] {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        assert!(
            hi - lo < 12.0,
            "the seam-crossing bolt stays short (span {} tiles)",
            hi - lo
        );
        assert!(
            hi > 250.0,
            "endpoint pinned beside the start across the seam (max x {hi})"
        );
        // The anchor equals the strike origin (tile 255), shared by every
        // segment — what lets the renderer wrap the bolt as a rigid unit.
        assert!(segs.iter().all(|s| (s.anchor[0] - 255.0).abs() < 1e-3));
    }

    fn pose(class: u8, model: u8, owned: bool, type_index: u16) -> LivePose {
        LivePose {
            slot: 1,
            generation: 1,
            class,
            model,
            type_index,
            frame: 0,
            x: 10.0,
            z: 10.0,
            alt: 1.0,
            yaw: 0.0,
            segment: false,
            chain_depth: 0.5,
            life_frac: None,
            fire_life: None,
            player_owned: owned,
            team: owned.then_some(0),
            blend: 0,
            map_only: false,
            flame_scale: 1.0,
            sprite_h_units: None,
        }
    }

    /// The smooth-motion lerp's edges: plain blend, the torus seam,
    /// the (slot, generation) identity guard, the teleport snap, and
    /// the yaw short-arc.
    #[test]
    fn lerp_poses_edges() {
        use std::f32::consts::TAU;
        let at = |slot: u16, generation: u32, x: f32, z: f32, alt: f32, yaw: f32| {
            let mut p = pose(5, 0, false, 0);
            p.slot = slot;
            p.generation = generation;
            p.x = x;
            p.z = z;
            p.alt = alt;
            p.yaw = yaw;
            p
        };

        // Plain midpoint blend.
        let out = lerp_poses(
            &[at(3, 1, 10.0, 20.0, 1.0, 0.0)],
            &[at(3, 1, 12.0, 20.0, 2.0, 0.0)],
            0.5,
        );
        assert!((out[0].x - 11.0).abs() < 1e-4 && (out[0].alt - 1.5).abs() < 1e-4);

        // Torus seam: 255.5 → 0.5 goes the short way through 0.0.
        let out = lerp_poses(
            &[at(3, 1, 255.5, 10.0, 1.0, 0.0)],
            &[at(3, 1, 0.5, 10.0, 1.0, 0.0)],
            0.5,
        );
        assert!(out[0].x.abs() < 1e-4, "seam lerp landed at {}", out[0].x);

        // Generation mismatch (slot reused): no lerp, draw at cur.
        let out = lerp_poses(
            &[at(3, 1, 10.0, 10.0, 1.0, 0.0)],
            &[at(3, 2, 100.0, 100.0, 1.0, 0.0)],
            0.5,
        );
        assert_eq!((out[0].x, out[0].z), (100.0, 100.0));

        // Teleport (jump beyond SNAP_TILES): snap to cur, no streak.
        let out = lerp_poses(
            &[at(3, 1, 10.0, 10.0, 1.0, 0.0)],
            &[at(3, 1, 40.0, 10.0, 1.0, 0.0)],
            0.5,
        );
        assert_eq!(out[0].x, 40.0);

        // Yaw crosses the 0/TAU seam the short way.
        let out = lerp_poses(
            &[at(3, 1, 10.0, 10.0, 1.0, TAU - 0.1)],
            &[at(3, 1, 10.0, 10.0, 1.0, 0.1)],
            0.5,
        );
        assert!(
            out[0].yaw.abs() < 1e-4 || (out[0].yaw - TAU).abs() < 1e-4,
            "yaw took the long way: {}",
            out[0].yaw
        );

        // A pose absent from prev (fresh spawn) draws at cur.
        let out = lerp_poses(&[], &[at(7, 1, 50.0, 50.0, 1.0, 0.0)], 0.25);
        assert_eq!((out[0].x, out[0].z), (50.0, 50.0));
    }

    /// The verbatim sub_48710 color switch (:57184-:57292), plus the
    /// dwelling-marker enhancement arms.
    #[test]
    fn map_dot_color_switch() {
        // Distinct anchors for the LUT-computed colors: [1] the
        // 0xF0F magenta (wild class-9/10), [2] the LUT[16]
        // villager violet-blue.
        let mut pal = [[0u8; 4]; 256];
        pal[1] = [255, 12, 255, 255];
        pal[2] = [12, 12, 255, 255];
        // blink true ↔ turn 0, false ↔ turn 8 ((turn >> 3) & 1 == 0).
        let dots = |p: LivePose, owned_buildings: bool, blink: bool| {
            map_dots_from_poses(
                GameId::Mc1,
                &[p],
                &pal,
                owned_buildings,
                Mc2MapEnv::Day,
                if blink { 0 } else { 8 },
                &Default::default(),
            )
        };

        // Player projectiles = the team-0 even entry; wild = the
        // LUT[3856] magenta (RED-major cube: n-1 = 0xF0F).
        assert_eq!(
            dots(pose(9, 0, true, 42), false, false)[0].color,
            TEAM0_EVEN
        );
        assert_eq!(dots(pose(9, 0, false, 42), false, false)[0].color, 1);
        // Wild villagers = the LUT[16] violet-blue (the retail map's
        // village speckles).
        assert_eq!(dots(pose(5, 13, false, 0), false, false)[0].color, 2);
        // Claimed model-39 balls blink the team pair on the phase;
        // model 40 has no phase term (LABEL_32): steady even entry.
        assert_eq!(
            dots(pose(10, 39, true, 105), false, true)[0].color,
            TEAM0_EVEN
        );
        assert_eq!(
            dots(pose(10, 39, true, 105), false, false)[0].color,
            TEAM0_ODD
        );
        assert_eq!(
            dots(pose(10, 40, true, 105), false, true)[0].color,
            TEAM0_EVEN
        );
        assert_eq!(
            dots(pose(10, 40, true, 105), false, false)[0].color,
            TEAM0_EVEN
        );
        // Wild balls = the raw 232 (:57291).
        assert_eq!(dots(pose(10, 39, false, 52), false, false)[0].color, 232);
        // Portals draw the 2x2 grown dot (:57270).
        assert_eq!(dots(pose(10, 34, false, 223), false, false)[0].size, 2);
        // Charred trees leave the map (:57219).
        assert!(dots(pose(2, 0, false, 226), false, false).is_empty());
        assert_eq!(dots(pose(2, 0, false, 83), false, false).len(), 1);
        // Castles/balloons are icon stamps, never dots.
        assert!(dots(pose(3, 2, true, 0), false, false).is_empty());

        // Dwellings, faithful default: NO marker, claimed or not
        // (player-certified retail behavior).
        let mut unclaimed = pose(10, 45, false, 105);
        unclaimed.map_only = true; // as live_poses_mc1 exports them
        assert!(dots(unclaimed, false, false).is_empty());
        assert!(dots(pose(10, 45, true, 105), false, false).is_empty());
        // The MC2-style enhancement: unclaimed = steady magenta on
        // every phase; possessed = the owner pair blinking at MC2's
        // (turn / 3) & 1 cadence, NOT the slower claimed-ball phase;
        // all 1px.
        let at_turn = |p: LivePose, turn: u32| {
            map_dots_from_poses(
                GameId::Mc1,
                &[p],
                &pal,
                true,
                Mc2MapEnv::Day,
                turn,
                &Default::default(),
            )
        };
        for turn in 0..8 {
            assert_eq!(at_turn(unclaimed, turn)[0].color, 1);
            assert_eq!(at_turn(unclaimed, turn)[0].size, 1);
        }
        let claimed = pose(10, 45, true, 105);
        assert_eq!(at_turn(claimed, 3)[0].color, TEAM0_EVEN);
        assert_eq!(at_turn(claimed, 0)[0].color, TEAM0_ODD);
        // Distinct from the ball phase: turn 8 flips the ball blink
        // but sits on the dark half of the building phase.
        assert_eq!(at_turn(claimed, 8)[0].color, TEAM0_ODD);
        assert_eq!(at_turn(claimed, 3)[0].size, 1);
    }

    /// The MC2 minimap law (DrawMinimapEntities_B_61A00, remc2
    /// GameUI.cpp:1134-1411): 12-bit codes through the palette,
    /// team pairs from playersColors_E88E0x, blink phases Turn/k.
    #[test]
    fn mc2_map_dot_law() {
        // A palette with distinct anchors for the 12-bit codes:
        // [1] blue (CIVILIANS 0x00F), [2] red (SPELLS 0xF00),
        // [3] white (CREATURE 0xFFF), [4] magenta (0xF0F).
        let mut pal = [[0u8; 4]; 256];
        pal[1] = [0, 0, 255, 255];
        pal[2] = [255, 0, 0, 255];
        pal[3] = [255, 255, 255, 255];
        pal[4] = [255, 0, 255, 255];
        let dots = |p: LivePose, turn: u32| {
            map_dots_from_poses(
                GameId::Mc2,
                &[p],
                &pal,
                false,
                Mc2MapEnv::Night,
                turn,
                &Default::default(),
            )
        };

        // Wild civilians (12..=14) = CIVILIANS blue (:1228-37).
        assert_eq!(dots(pose(5, 13, false, 0), 0)[0].color, 1);
        // Every other wild creature = the night map-type fill (white
        // v92, :1246-53 + :1052-55).
        assert_eq!(dots(pose(5, 1, false, 0), 0)[0].color, 3);
        // Wizard-owned units = the team DARK column (:1222-26).
        assert_eq!(dots(pose(5, 4, true, 0), 0)[0].color, MC2_TEAM_NIGHT[0].1);
        // Wild buildings = UNPOSSESSED_BUILDING2 magenta (LABEL_56,
        // :1291-96); owned ones blink the team pair (:1273-86).
        assert_eq!(dots(pose(10, 45, false, 0), 0)[0].color, 4);
        assert_eq!(dots(pose(10, 45, true, 0), 3)[0].color, MC2_TEAM_NIGHT[0].0);
        assert_eq!(dots(pose(10, 45, true, 0), 0)[0].color, MC2_TEAM_NIGHT[0].1);
        // Spells = SPELLS red (:1396-1402).
        assert_eq!(dots(pose(12, 0, false, 0), 0)[0].color, 2);
        // The marker stone blinks phase 3 on/off (:1163-70).
        assert_eq!(dots(pose(2, 1, false, 0), 3).len(), 1);
        assert!(dots(pose(2, 1, false, 0), 0).is_empty());
        // Route explosions' class-10 model 0x12 is skipped (:1262-63);
        // the portal (34) grows 2x2 (:1264-67).
        assert!(dots(pose(10, 0x12, false, 0), 0).is_empty());
        assert_eq!(dots(pose(10, 34, false, 0), 0)[0].size, 2);
        // Switches: only the X-marker models draw (:1341-92).
        assert!(dots(pose(11, 0, false, 0), 0).is_empty());
        assert_eq!(dots(pose(11, 0x0C, false, 0), 0)[0].size, 2);
    }

    /// The marker icon-swap (`map_marker_icons`, deliberate
    /// deviation): a type row in the swap set loses its DOT in both
    /// games' walks and gains a miniature STAMP; expose-jar-spells
    /// (debug) OUTRANKS the swap for jars (player ruling 2026-08-07:
    /// spell icon + retail dot, never the jar miniature too); a
    /// family with no built icon keeps its dot.
    #[test]
    fn marker_icon_swap_moves_families_between_layers() {
        let pal = [[0u8; 4]; 256];
        let mut swapped = std::collections::HashSet::new();
        swapped.insert(42u16);
        let dots = |game, p: LivePose| {
            map_dots_from_poses(game, &[p], &pal, false, Mc2MapEnv::Day, 0, &swapped)
        };
        // MC1 jar (class 12) with an icon: no dot; without: the dot.
        assert!(dots(GameId::Mc1, pose(12, 3, false, 42)).is_empty());
        assert_eq!(dots(GameId::Mc1, pose(12, 3, false, 43)).len(), 1);
        // The MC2 walk honors the same set (dolmen (2,2), on its
        // visible blink phase — turn 0 is (turn/2)&1 == 0 → v90).
        assert!(dots(GameId::Mc2, pose(2, 2, false, 42)).is_empty());
        assert_eq!(dots(GameId::Mc2, pose(2, 2, false, 43)).len(), 1);

        // Stamps: the miniature keyed by type row, gated on the
        // toggle, outranked by the debug spell icon.
        let mini = mgc_render::MapStamp {
            x: 0.0,
            z: 0.0,
            w: 12,
            h: 12,
            uv: [0.0, 400.0, 24.0, 24.0],
            anchor: [0.5, 1.0],
        };
        let spell_icon = mgc_render::MapStamp {
            x: 0.0,
            z: 0.0,
            w: 8,
            h: 8,
            uv: [64.0, 0.0, 8.0, 8.0],
            anchor: [0.5, 1.0],
        };
        let mut icons = MapIcons::default();
        icons.jar_icons.insert(42, mini);
        icons.static_icons.insert(7, mini);
        icons.spell = vec![Some(spell_icon); 26];
        let stamps = |p: LivePose, expose: bool, swap: bool| {
            map_stamps_from_poses(GameId::Mc1, &[p], &icons, false, expose, swap)
        };
        let jar = pose(12, 3, false, 42);
        // Toggle on, debug off → the jar miniature.
        let s = stamps(jar, false, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].uv, mini.uv);
        // Debug on outranks → the spell icon, never the miniature.
        let s = stamps(jar, true, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].uv, spell_icon.uv);
        // Both off → no stamp at all (the dot layer's business).
        assert!(stamps(jar, false, false).is_empty());
        // Statics resolve through their own table.
        let s = stamps(pose(2, 1, false, 7), false, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].uv, mini.uv);
        // An unkeyed static stays dotted, not stamped.
        assert!(stamps(pose(2, 1, false, 8), false, true).is_empty());
    }

    /// The MC2 billboard size law (remc2 GameRenderOriginal.cpp
    /// :2192-98 + the loader cross-fill :44895-903): `rot_speed_8` =
    /// world height in engine units; width from the frame's pixel
    /// aspect; draw type from the TMAPS header byte (flags >> 8).
    #[test]
    fn mc2_billboard_size_law() {
        // Row 43: word_0 0x52, rot_speed_8 0x96 = 150 (the row
        // remc2's own table authors at Type_WORD_D951C.cpp:47;
        // indexing verified 0-based against the vendored source).
        let dims = |id: u16| (id == 0x52).then_some((32u16, 64u16, 0x1200u16));
        let s = resolve_pose_sprite(GameId::Mc2, 43, &dims).unwrap();
        assert_eq!(s.sprite_base, 0x52);
        assert_eq!(s.draw_type, 0x12, "draw type = the TMAPS header byte");
        assert!((s.world_h - 150.0 / 256.0).abs() < 1e-6);
        assert!(
            (s.world_w - s.world_h * 0.5).abs() < 1e-6,
            "width = pixel aspect"
        );
        // MC1 rows still resolve through SPRITE_STATS untouched.
        let any_dims = |_: u16| Some((32u16, 64u16, 0u16));
        let mc1 = resolve_pose_sprite(GameId::Mc1, 43, &any_dims).unwrap();
        assert_eq!(mc1.sprite_base, SPRITE_STATS[43].sprite_base);
    }

    /// Retail proximity concealment marking: the MC2 wraith (5,26)
    /// carries it unconditionally (its retail concealment was the
    /// short draw radius, not a flag), the mana dweller (5,23) only
    /// under the `mc2_dweller_invisibility` patch, and nothing else —
    /// MC1's (5,26) is a different creature and never concealed.
    #[test]
    fn conceal_marks_the_wraith_always_and_dwellers_under_the_patch() {
        let dims = |id: u16| (id == 0x52).then_some((32u16, 64u16, 0x1200u16));
        let poses = [
            pose(5, 26, false, 43),
            pose(5, 23, false, 43),
            pose(5, 2, false, 43),
        ];
        let conceals = |patch: bool| {
            billboards_from_poses(GameId::Mc2, &poses, dims, false, false, patch)
                .iter()
                .map(|b| b.conceal)
                .collect::<Vec<_>>()
        };
        assert_eq!(conceals(false), [true, false, false]);
        assert_eq!(conceals(true), [true, true, false]);
        let any_dims = |_: u16| Some((32u16, 64u16, 0u16));
        let mc1 = billboards_from_poses(GameId::Mc1, &poses, any_dims, false, false, true);
        assert!(mc1.iter().all(|b| !b.conceal), "MC1 never conceals");
    }

    /// The rival tag chrome: MC2 resolves the retail bldgprmbuffer
    /// indices through the LIVE level palette (sRGB-decoded — the
    /// double-encode trap); MC1's opt-in carries the pre-sampled Day
    /// constants and ignores the palette entirely.
    #[test]
    fn rival_tag_chrome_resolution() {
        let mut pal = [[0u8; 4]; 256];
        pal[0xAA] = [10, 20, 30, 255];
        pal[0x63] = [40, 50, 60, 255];
        pal[0x0D] = [70, 80, 90, 255];
        pal[0x33] = [1, 2, 3, 255];
        pal[0] = [255, 255, 255, 255];
        let day = rival_tag_chrome(GameId::Mc2, Mc2MapEnv::Day, &pal);
        assert_eq!(day.bg, pal_ui_rgba(&pal, 0xAA));
        assert_eq!(day.bevel_tl, pal_ui_rgba(&pal, 0x63));
        assert_eq!(day.bevel_br, pal_ui_rgba(&pal, 0x0D));
        assert_eq!(
            day.bar_empty,
            pal_ui_rgba(&pal, 0),
            "empty bar = palette[0]"
        );
        let night = rival_tag_chrome(GameId::Mc2, Mc2MapEnv::Night, &pal);
        assert_eq!(night.bg, pal_ui_rgba(&pal, 0x33), "night row selected");
        let mc1 = rival_tag_chrome(GameId::Mc1, Mc2MapEnv::Day, &pal);
        assert_eq!(
            mc1.bg,
            [srgb_lin(101), srgb_lin(101), srgb_lin(101), 1.0],
            "MC1 uses the PALD-0.DAT-sampled constants, not the live palette"
        );
    }

    /// Player-start resolution against the real level-000 package
    /// (self-skips without baked data): MC2 falls back to the
    /// MC1-shaped (3,4) marker — level-000 authors that one.
    #[test]
    fn mc2_player_start_level_000() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../baked/mc2/level-000.mgcl");
        let Ok(f) = std::fs::File::open(p) else {
            eprintln!("skipped: baked mc2 data not present");
            return;
        };
        let pkg: mgc_formats::LevelPackage = mgc_formats::mgcl::read(f).unwrap();
        assert_eq!(
            player_start(GameId::Mc2, &pkg.things.things),
            Some((77.5, 222.5)),
            "the (3,4) fallback start marker"
        );
    }
}
