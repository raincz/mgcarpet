//! MC1 creature/scenery spawn handlers and (movement track) the mob
//! state machine — direct ports of remc1's per-model spawn functions.
//! All citations remc1 sub_main.cpp.
//!
//! Spawn dispatch in the original: `dword_96902[class].str_4` →
//! per-class tables str_254D48 (class 2), str_254B84 (class 3),
//! str_255478 (class 5). Every handler allocates from the shared event
//! pool ([`Gen::new_event`]) and rolls only the event's OWN LCG
//! (`rand_29799_4`, seeded `slot + global_rand` — the global stream is
//! read, never advanced, by spawning), so spawn randomness is
//! byte-faithful regardless of spawn order.
//!
//! Fidelity notes:
//! - Class-5 model 0's segment-mana write targets the HEAD (+140,
//!   :44644) where model 3's identical construct writes the SEGMENT
//!   (:44861) — ported literally from the decompile; flagged in
//!   docs/ROADMAP.md (mana-track concern only).
//! - The kraken head (m6) is linked into the tile chain twice
//!   (:45086-:45087); `link` guards on the placed flag in both the
//!   original and this port, so the second call is a no-op.

use crate::engine::features::{
    Gen, HashSilent, Raw48, no_mc1_build_site_chain, no_mc1_vulture_death_mover, no_mc1_wake_dy48,
};
use crate::mc1::behavior::{BEHAVIOR, BehaviorRow};
use crate::mc1::combat::Inbox;
use crate::mc1::sprite_stats::SPRITE_STATS;
use crate::mc1::tables::{COS, SIN};

/// The OUT-OF-POOL sentinel owner/target tag for the human — the
/// fallback [`crate::engine::features::Gen::human_tag`] keeps when no
/// carpet record is seated (a bare test world) or under
/// `MGC_NO_HUMAN_SLOT_TAG`. Since the multiplayer stage 1 the live tag
/// is the carpet record's pool slot, retail's own convention; sim code
/// reads `Gen::human_tag`, never this constant.
pub(crate) const PLAYER_TARGET: u16 = 0xFFFF;

/// The `+146` a class-10 explosion child carries when its probe found
/// NOBODY. Retail's stamp is an unguarded pointer difference
/// (`(v17 - v21) / 164`, :63428), so a null probe records
/// `(0 - entBase) / 164` as a word — a CARPET.EXE link-time constant,
/// the same in every retail instance. Measured off the recording (all
/// 542 mc1l42 (10,23) miss rows and its 13 (10,11) crater rows), never
/// derived. The per-binary caution is now settled by measurement:
/// HIDDEN.EXE links its pool at the same base and records the SAME
/// word (mc1hwl0 t=335 (10,23) slot 589 and t=31088 slot 944 both
/// read 64608), so the stamp is emitted for both binaries.
pub(crate) const MC1_MISS_STAMP: u16 = 64608;

/// `MGC_NO_TURN_TIE=1` restores the pre-dig wrapped-delta turn sign
/// (`(tgt − cur) & 0x7FF <= 1024`), which turns the WRONG way on the
/// exact 180° tie — see [`Gen::turn_sign`]. Kept so one binary can be
/// A/B'd; read once, a whole-process arm.
fn no_turn_tie_fix() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_TURN_TIE").is_some())
}

/// `MGC_NO_DEAF_STATES=1` restores the unconditional damage intake,
/// i.e. runs the mailbox prologue above EVERY live state — see
/// [`Gen::state_is_damage_deaf`]. Kept so one binary can be A/B'd;
/// read once, a whole-process arm.
fn no_deaf_states() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_DEAF_STATES").is_some())
}

/// `MGC_NO_MC1_LEADERLESS_PACK_DEAF=1` restores the mailbox prologue
/// above a pack member that has NO LEADER (round 167). The shared pack
/// routine `sub_1A390` (remc1hw sub_main.cpp:20234) opens on `if (!+52)
/// { state = base + 1; return; }` — HIDDEN.EXE file 0x32d92 `cmpw
/// $0x0,0x34(%ebx)` / 0x32d97 `jne` / 0x32d9d `jmp` to the state write
/// — and the mail read, the segment walk and the `actLife < 0` test all
/// sit BELOW it. A leaderless member therefore spends the tick being
/// demoted to wander and cannot be hurt in it; the pending mail waits
/// for the wander prologue. WITNESS mc1hwl10 t=21968 slot 579 `(5,4)`:
/// state 27 -> 25 with `mail0` (1200, 629) standing and `actLife` 207;
/// the port debited it (-993) and corpsed the militiaman a tick early.
fn no_mc1_leaderless_pack_deaf() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_LEADERLESS_PACK_DEAF").is_some())
}

/// `MGC_NO_MC1HW_TREE_CTOR=1` restores MC1's tree ctor on a Hidden
/// Worlds level (round 167). The two binaries differ: `CARPET.EXE`'s
/// `sub_37BC0` (remc1 sub_main.cpp:44402) draws FOUR times and picks
/// sprite 83 or 84 off the fourth; `HIDDEN.EXE`'s `sub_37F80` (remc1hw
/// :40815) draws THREE times and takes sprite 83 always — file 0x509c3
/// / 0x509f3 / 0x50a14 are the only three `imul $0x24a1,0x4(%ebx)` in
/// the body, and 0x50a4c is `push $0x53` ahead of the sprite setter.
/// One draw and one sprite row: `rand`, `type86` and the extents that
/// follow the sprite (171 against 177). At level init this is round
/// 165's `(2,0)` census leftover (983 rows, 12 HW takes — replay imports
/// the pool, so it never graded); WITNESS at runtime mc1hwl10 t=48554,
/// where a trigger mints the authored tree at slot 6: retail `rand`
/// 1069502566 / `type86` 83, the port 4271901957 / 84.
fn no_mc1hw_tree_ctor() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1HW_TREE_CTOR").is_some())
}

/// `MGC_NO_HIT_TRAILERS=1` restores the pre-dig BLANKET hit abort: the
/// wizard-attacker arms return before the wrapper trailers, m1's idle
/// mover is skipped on a hit tick, and m6's chase dive-clock reset is
/// dropped. Retail aborts the shared CORE only — every prologue exit
/// is a plain return back into the per-model WRAPPER, whose tail then
/// runs regardless. Kept so one binary can be A/B'd; read once, a
/// whole-process arm.
fn no_hit_trailers() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_HIT_TRAILERS").is_some())
}

/// `MGC_NO_M4_SCRATCH_TARGET=1` restores the pre-dig m4 CHASE guards:
/// the `+146 == 0` early return and the `class64 == 0` conjunct in the
/// target-lost test — see [`Gen::militia_chase_body`]. Retail's shared
/// chase core `sub_1A120` (:21655-58) has neither. Kept so one binary
/// can be A/B'd; read once, a whole-process arm.
fn no_m4_scratch_target() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M4_SCRATCH_TARGET").is_some())
}

/// ⭐⭐⭐ THE GRIFFON'S WANDER SCAN IS CLOAK-BLIND. The m8 griffon's
/// WANDER (`sub_1CA50`, creature state 0x31) carries its OWN copy of
/// the class-3 body scan, and that copy **omits the
/// `(+16 & 0x20) == 0` per-node test** that the shared wander
/// `sub_19D70` and the militia's `sub_1B5D0` both carry. The two loops
/// are otherwise the same code; the griffon's is three instructions
/// shorter. Verbatim from `HIDDEN.EXE` (identical in `CARPET.EXE` at
/// the same VAs — file = VA + 0x189F8 / + 0x187F8):
///
/// ```text
/// sub_19D70  (VA 0x19D70, file 0x32768) — the SHARED wander:
///   32975  3b 7c 24 04   cmp   0x4(%esp),%edi    ; d2 > v_28² ?
///   32979  77 51         ja    0x329cc           ;   -> skip node
///   3297b  f6 46 10 20   testb $0x20,0x10(%esi)  ; <- THE CLOAK BIT
///   3297f  75 4b         jne   0x329cc           ;   -> skip node
///   32981  …cone test…
///
/// sub_1CA50  (VA 0x1CA50, file 0x35448) — the m8 GRIFFON wander:
///   35652  3b 7c 24 0c   cmp   0xc(%esp),%edi    ; d2 > v_28² ?
///   35656  77 49         ja    0x356a1           ;   -> skip node
///   35658  …cone test…                           ; <- NO 0x20 TEST
/// ```
///
/// An `f6 4? 10 20` scan over the WHOLE of `sub_1CA50`
/// (0x1CA50..0x1CF60, both binaries) finds **zero** matches;
/// `sub_1B5D0` and `sub_19D70` have exactly one each. The winner gate
/// that follows is unchanged (`+65 <= 1 && wizext[+528]`, HIDDEN
/// 0x356b5-0x356c6 / decompile `reference/remc1/sub_main.cpp:23500`,
/// `reference/remc1hw/sub_main.cpp:22057`).
///
/// So a CLOAKED wizard body still takes part in the griffon's
/// election — and because the gate judges the ELECTION WINNER and
/// never falls past it, the bit swings the outcome BOTH ways:
///
/// - mc1hwl9 t=890, griffon 273: retail's nearest in-cone body is the
///   cloaked carpet 246 (`flags 0x2c`, d²=17.5M) whose wizext `+528`
///   is 0 — the gate refuses it and the whole scan comes home empty,
///   so retail PACKS (`+70` 49 → 51, `+52` = 458, `+146` stays 0).
///   The port skipped 246 for its cloak bit, elected carpet 233
///   (d²=29.8M, `+528` = 200) and CHASED (`+146` 0 → 233).
/// - mc1hwl9 t=2973 (griffon 331) and mc1hwl5 t=2218 (griffon 457)
///   are the mirror: retail's winner IS the cloaked carpet (168 and
///   386, both `flags 0x2c`) and both ARE village-wanted, so retail
///   chases them; the port skipped them and its own winner failed the
///   gate — `+146` stayed 0.
///
/// The human is the chain's out-of-pool member and his cloak is the
/// same `+16 & 0x20` bit (mirrored in `player_invisible`), so it drops
/// out of the griffon's scan too.
///
/// `MGC_NO_MC1_GRIFFON_SCAN_CLOAK_BLIND=1` restores the pre-dig
/// per-node cloak gate on the griffon's scan.
pub(crate) fn griffon_scan_cloak_blind() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_GRIFFON_SCAN_CLOAK_BLIND").is_none())
}

/// `MGC_NO_M15_CHASE_HIT_FALLTHROUGH=1` restores the pre-dig freeze on
/// the castle guard's CHASE hit tick (the shared prologue's blanket
/// `return`) — see the `Inbox::Hit` guard in [`Gen::creature_tick`].
/// Retail's `sub_201D0` (:25789-825) carries its OWN inline mailbox
/// that aborts on DEATH ONLY. Kept so one binary can be A/B'd.
fn no_m15_chase_hit_fallthrough() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M15_CHASE_HIT_FALLTHROUGH").is_some())
}

/// `MGC_NO_M15M16_SCRATCH_TARGET=1` restores the pre-dig guards on the
/// two chase handlers that carry their OWN target read instead of the
/// shared core's: the wyvern's `sub_207E0` (:26144-52) and the castle
/// guard's `sub_201D0` (:25830-34). Neither retail function tests the
/// index or the class before dereferencing `&pool[+146]` — the third
/// and fourth copies of the law `no_m4_scratch_target` names. Kept so
/// one binary can be A/B'd.
fn no_m15m16_scratch_target() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M15M16_SCRATCH_TARGET").is_some())
}

/// `MGC_NO_M4M9_ARM_SCRATCH_TARGET=1` restores the pre-dig `+146 == 0`
/// guards on the two ARM trailers that stamp the filter pair off the
/// target record: the militiaman's `sub_1BC50` (:22759-61) and the
/// mound's `sub_1DCD0` (:24240-48) — see [`Gen::militia_arm`] and
/// [`Gen::m9_enter_chase`]. Both retail functions index the pool with
/// `+146` UNCONDITIONALLY (`*(BYTE *)(pool + 164 * v3 + 64)`), and
/// `sub_1DCD0` runs its `id24` equality test off the SAME raw pointer,
/// so a `+146` of 0 reads the scratch record and stamps its (0, 0) —
/// the FIFTH and SIXTH copies of the law `no_m4_scratch_target` names.
/// Kept so one binary can be A/B'd; read once, a whole-process arm.
fn no_m4m9_arm_scratch_target() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M4M9_ARM_SCRATCH_TARGET").is_some())
}

/// `MGC_NO_MC1_WANTED_DEATH_STATE=1` restores the pre-dig placement of
/// the village-wanted (+528 = 200) arm on the death-TRANSITION tick,
/// inside the damage prologue's `Inbox::Dead` arm. Retail does not arm
/// there at all: the +528 write lives in the four per-model DEATH-STATE
/// handlers, which run one and two ticks LATER and read `+38` as it
/// stands then — griffon `sub_1CF60` (state 0x34 = 48+4,
/// `reference/remc1/sub_main.cpp` :23574-80, HW twin :22131-39),
/// settler `sub_1F5B0` (state 0x4D = 72+5, :25285-92, HW :23842-50),
/// feeder `sub_1FA00` (state 0x52 = 78+4, :25447-62, HW :24004-20,
/// the arm sitting in the NON-absorb else) and feeder `sub_1FEC0`
/// (state 0x59 = 84+5, :25632-38, HW :24189-97). NONE of the four has
/// a single `call` site in `CARPET.EXE` — every one is reached ONLY
/// through the class-5 state table — and each tails into its own
/// family BASE: `sub_1CF60` @VA 0x1CF60 ends `push 0x30; call 0x1a6c0`
/// (48 = m8, the DEATH slot), `sub_1FA00` @0x1FA00 `push 0x4e; call
/// 0x1a6c0` (78 = m13, DEATH), `sub_1F5B0` @0x1F5B0 `push 0x48; call
/// 0x1a800` (72 = m12, CORPSE) and `sub_1FEC0` @0x1FEC0 `push 0x54;
/// call 0x1a800` (84 = m14, CORPSE). The write itself is one
/// instruction, `66 C7 80 10 02 00 00 C8 00` = `mov word
/// [eax+0x210],200`, at file 0x357C2 / 0x37E12 / 0x3827A / 0x38722
/// (`file = VA + 0x187f8`), reached past `mov dl,[eax+0x74a4]` (POOL +
/// 65 = the killer's model) `test dl,dl / cmp dl,1` — the
/// `model <= 1` gate — off `mov eax,[edx+eax*4+0x7503]` (POOL + 160,
/// the wizext). Kept so one binary can be A/B'd; read once, a
/// whole-process arm.
fn no_mc1_wanted_death_state() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WANTED_DEATH_STATE").is_some())
}

/// A/B toggle for THE KRAKEN HEAD'S `+56 = 96` CTOR STAMP (round 154,
/// w154e): set `MGC_NO_MC1_KRAKEN_HEAD_F56` to restore the pre-dig
/// `if model != 6` skip in [`Gen::spawn_worm`]. Citation at the write
/// site.
fn no_mc1_kraken_head_f56() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_KRAKEN_HEAD_F56").is_some())
}

/// `MGC_NO_M4_CHASE_WANTED_TRAILER=1` restores the pre-dig behaviour in
/// which a militiaman's CHASE tick that is FROZEN by the damage inbox
/// skipped `sub_1BB20`'s wanted-timer tail — see
/// [`Gen::militia_chase_wanted_tail`]. Kept so one binary can be A/B'd;
/// read once, a whole-process arm.
fn no_m4_chase_wanted_trailer() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M4_CHASE_WANTED_TRAILER").is_some())
}

/// A/B toggle for **THE WYVERN'S HOUSE HUNT IS A WRAPPER TRAILER**
/// (round 155, w155c): set `MGC_NO_MC1_WYVERN_HOUSE_HUNT_ON_HIT` to
/// restore the pre-dig behaviour in which a wyvern's WANDER tick that
/// is FROZEN by the damage inbox skipped `sub_20710`'s house hunt.
/// Retail (:26033-58, CARPET.EXE 0x38F15 `call sub_19D70` then, with
/// eax never tested, 0x38F1A `mov 0x46(%ebx),%ah` / 0x38F20 `cmp
/// $0x61,%ah`): the shared wander core's damage prologue returns
/// into the wrapper, whose ONLY gate on the hunt is `+70 == 97`.
/// WITNESS mc1l34 pair 14668→14669, wyvern slot 8: militia 466's bolt
/// 289 lands 250 (`+40` 0 → 466), and on that very tick (`+63` 83 =
/// 0 mod v_26+1) retail elects dwelling 24 at (1280,36352) — `+146`
/// 856 → 24, `+70` 97 → 98 — where the port's blanket hit-freeze held
/// 97/856. See [`Gen::wyvern_house_hunt`].
fn no_mc1_wyvern_house_hunt_on_hit() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_WYVERN_HOUSE_HUNT_ON_HIT").is_some())
}

/// Per-tick context the creature handlers need: the seat the walk
/// serves and the world's rule flags.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MobCtx {
    /// The human seat this context's walk serves. The context carries
    /// no human lanes: walkers read every human off his RECORD
    /// ([`Gen::seat_at`] and friends), and what the walk publishes
    /// about him between record reads is his [`Gen::seat_view`]
    /// ([`crate::engine::features::SeatView`]).
    pub(crate) phx: crate::engine::world::Hx,
    /// Conformance replay (`World::strict_retail`): deliberate
    /// gameplay deviations switch off (DEVIATIONS.md law). Carried
    /// here so Gen-side ticks can gate without a Gen field (which
    /// would drag the snapshot codec and the state hash along).
    pub(crate) strict: bool,
    /// The retail-bug patch set (`World::patches`) — same MobCtx
    /// rationale as `strict`. Gated sites read the patched arm as
    /// `ctx.patches.x && !ctx.strict`.
    pub(crate) patches: crate::patches::WorldPatches,
    /// MC2's `setting_30` game-loop counter (engine_support.h:229):
    /// zeroed at level init, incremented in `PlayerEvents_51BB0`
    /// beside `Turn++` (EF:37557) — during the entity pass it equals
    /// the post-increment turn, i.e. `World::mc2_turn` (the cave
    /// carpet tail's corpus-proven additive reads the same value,
    /// EF:59803). The per-entity `rand_0x14 += setting_30` perturb
    /// sites consume it; remc2's `uint8_t` typing is false — the
    /// counter is full-width. Same MobCtx rationale as `strict`.
    pub(crate) mc2_turn: u32,
}

/// Animation frame counts by sprite draw type (`byte_90AD8`, :2716):
/// the 2..=16 animation draw types carry their frame count in the
/// type itself; view-select types have 1.
pub(crate) const FRAME_COUNTS: [u8; 37] = [
    1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, //
    1, 1, 1, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
];

impl Gen {
    /// sub_36FA0_37360 (:43751): assign the sprite-stats type index and
    /// derive extents from the sprite's world size halves.
    pub(crate) fn set_sprite(&mut self, i: usize, t: u16) {
        let s = SPRITE_STATS[t as usize];
        // The row's size as this level's sprite sheet derives it
        // ([`crate::mc1::derive_sprite_stats`]); the static table is
        // the temperate bank's.
        let (width, height) = self
            .assets
            .mc1_sprite_ext
            .get(t as usize)
            .copied()
            .unwrap_or((s.width, s.height));
        let e = &mut self.ent[i];
        e.frame88 = 0;
        e.type86 = t;
        e.frames89 = FRAME_COUNTS.get(s.draw_type as usize).copied().unwrap_or(0);
        e.f78 = height / 2;
        e.f80 = width / 2;
        e.f82 = width / 2;
        e.f84 = height / 2;
    }

    /// sub_370A0_37460 (:43772): [`set_sprite`](Self::set_sprite), then
    /// DOUBLE the three collision half-extents (+80/+82/+84 — never
    /// +78). Because the inner call re-derives the extents first, this
    /// is idempotent: retail calls it on the same entity twice (the
    /// m13 ctor at :46274 and then the firing thunk at :21928) and the
    /// box does not grow to 4x.
    ///
    /// Only three call sites of THIS WRAPPER exist (:21928, :21957,
    /// :46274), all on the m13 arrow path — the sibling m14 boulder
    /// ctor (:46297) deliberately uses the PLAIN setter, so the two
    /// projectiles differ in hitbox as well as in art.
    ///
    /// ⚠ THE DOUBLING ALSO SHIPS INLINE, AND THE WRAPPER'S CALL LIST
    /// DOES NOT FIND IT. Two class-9 ctors open-code the same thing as
    /// `set_sprite` + `extents(2 * +80, 2 * +84)`: the possess lob
    /// sub_39A90 (:45917) and the Mana Magnet bolt sub_3A2F0 (:46385),
    /// both confirmed in CARPET *and* HIDDEN (hw:42038 / hw:42505).
    /// Grep the SHAPE (`sub_37130_374F0(.., 2 * ..)`), not the wrapper.
    pub(crate) fn set_sprite_x2(&mut self, i: usize, t: u16) {
        self.set_sprite(i, t);
        let e = &mut self.ent[i];
        e.f80 *= 2;
        e.f82 *= 2;
        e.f84 *= 2;
    }

    /// sub_37130_374F0 (:43790): explicit extent override.
    pub(crate) fn extents(&mut self, i: usize, horiz: u16, vert: u16) {
        let e = &mut self.ent[i];
        e.f80 = horiz;
        e.f82 = horiz;
        e.f84 = vert;
    }

    /// RefillLife_36DE0_371A0 (:43701).
    pub(crate) fn refill_life(&mut self, i: usize) {
        self.ent[i].act_life = self.ent[i].max_life as i32;
    }

    /// The spawn facing draw shared by most models (:44751-:44755):
    /// `(lcg & 0x7FF) - 1`, written to +34/+30/+32.
    fn spawn_facing(&mut self, i: usize, f: u16) {
        let e = &mut self.ent[i];
        e.f34 = f;
        e.f30 = f;
        e.f32 = f;
        e.f36 = 0;
    }

    // ---- class 2: scenery (str_254D48, :4359) -----------------------------

    /// Class-2 spawn dispatch. All models set `+26 = slot % 11`.
    pub(crate) fn spawn_scenery(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        if model > 5 {
            return None;
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 2;
            e.model65 = model as u8;
            e.f26 = (i % 11) as i16;
        }
        match model {
            // sub_37BC0 (:44402): the tree. Four draws of the event LCG
            // in strict order: a discarded life roll, x jitter, y
            // jitter (±32 units), then the variant bit (83/84) — MC1's;
            // Hidden Worlds stops at three.
            0 => {
                let e = &mut self.ent[i];
                e.tick70 = 0;
                e.f28 = 1;
                let life = self.ent_rand(i) % 0x1388 + 2500;
                self.ent[i].act_life = life as i32; // clobbered by RefillLife below, as the original
                let jx = ((self.ent_rand(i) & 0x3F) as i32 - 32) as i16;
                let jy = ((self.ent_rand(i) & 0x3F) as i32 - 32) as i16;
                self.link(i, x.wrapping_add(jx as u16), y.wrapping_add(jy as u16), z);
                self.refill_life(i);
                // HIDDEN.EXE's twin `sub_37F80` has no variant draw
                // ([`no_mc1hw_tree_ctor`]).
                let t = if self.is_hidden_worlds() && !no_mc1hw_tree_ctor() {
                    83
                } else if self.ent_rand(i) & 1 != 0 {
                    84
                } else {
                    83
                };
                self.set_sprite(i, t);
            }
            // sub_37CF0/37D70/37E00 (:44451-): clear flag bit 3.
            1 | 2 | 3 => {
                let e = &mut self.ent[i];
                e.flags &= !8;
                e.tick70 = [3, 6, 9][model as usize - 1];
                self.link(i, x, y, z);
                self.refill_life(i);
                self.set_sprite(i, [79, 39, 270][model as usize - 1]);
                if model == 2 {
                    self.extents(i, 1024, 1024);
                }
            }
            // sub_37E80/37EF0 (:44526-): both the type-48 marker stone.
            _ => {
                self.ent[i].tick70 = if model == 4 { 12 } else { 15 };
                self.link(i, x, y, z);
                self.refill_life(i);
                self.set_sprite(i, 48);
            }
        }
        Some(i)
    }

    // ---- class 3: balloons / castle (str_254B84, :4367) --------------------

    /// Class-3 spawn dispatch; models 4..=11 are the player-start
    /// position markers (no entity — `sub_37720`.. write
    /// `str_9177[colour]`, which `World::spawn_from_thing_at` records
    /// in `start_markers` at every disposition fire), 12+ nothing.
    pub(crate) fn spawn_class3(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        if model > 3 {
            return None;
        }
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 3;
            e.model65 = model as u8;
        }
        match model {
            // sub_37820/sub_378A0 (:44180/:44201): the wizard carpet —
            // model 0 = the HUMAN player's entity (row 7), model 1 =
            // an AI wizard (row 8, re-sets +24 to its own slot). No
            // facing draw. Wizard AI/flight is the Phase-5 track —
            // level-authored ones stand and render.
            0 | 1 => {
                let e = &mut self.ent[i];
                e.tick70 = model as u8;
                e.max_life = 10000;
                e.f128 = 80;
                e.f28 = 29;
                e.row156 = if model == 0 { 7 } else { 8 };
                if model == 1 {
                    e.id24 = i as u16;
                }
                self.link(i, x, y, z);
                self.refill_life(i);
                self.set_sprite(i, 44);
            }
            // sub_37920 (:44229): the castle. Spawn position snaps to a
            // tile corner of even parity; +150/152 keep the snapped
            // position (the castle's anchor) and +154 the ground at
            // the RAW pre-snap axis (:44251/:44256 — one sub_11F50,
            // used for both the link z and the site datum; the
            // caller's z is ignored). The transform's painter mints
            // AT the +150 triple (sub_47020 :56100), so a zero +154
            // is a painter born at z 0 (mc1l5 t=17645, the post-raze
            // rebuild's first level-up at site (0,0)).
            2 => {
                let e = &mut self.ent[i];
                e.tick70 = 5;
                e.max_life = 40000;
                e.f26 = 0;
                e.f28 = 33;
                let mut tx = x >> 8;
                let ty = y >> 8;
                if (tx.wrapping_add(ty)) & 1 == 1 {
                    tx = tx.wrapping_add(1);
                }
                let (sx, sy) = (tx << 8, ty << 8);
                let gz = self.ground_z(x, y) as i16;
                self.ent[i].dest_x = sx;
                self.ent[i].dest_y = sy;
                self.ent[i].site_z = gz;
                self.link(i, sx, sy, gz);
                self.refill_life(i);
                self.set_sprite(i, 177);
            }
            // sub_37A00 (:44266).
            _ => {
                let e = &mut self.ent[i];
                e.tick70 = 7;
                e.max_life = 10000;
                e.f126 = 48;
                e.f136 = 10000;
                e.f140 = 0;
                e.f28 = 1;
                e.row156 = 9;
                self.link(i, x, y, z);
                self.refill_life(i);
                self.set_sprite(i, 169);
            }
        }
        Some(i)
    }

    // ---- class 5: creatures (str_255478, :4420) ----------------------------

    /// Class-5 spawn dispatch, models 0..=16 (17+ hit the table's null
    /// terminator — no spawn). Returns the head slot.
    pub(crate) fn spawn_creature(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        match model {
            0 => self.spawn_worm(0, x, y, z),
            3 => self.spawn_worm(3, x, y, z),
            6 => self.spawn_worm(6, x, y, z),
            1..=16 => self.spawn_simple_creature(model, x, y, z),
            _ => None,
        }
    }

    /// The single-entity creature spawns (:44664-:45640), one table
    /// row per model; the shared shape is NewEvent + state/speeds/life +
    /// mana + facing draw + bookkeeping + place + RefillLife +
    /// sprite/extents.
    fn spawn_simple_creature(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        // Per-model constants (sub_38270..sub_396E0):
        //   state, life, act_speed, max_speed, accel, row, f44,
        //   mana mode, facing mode, type pick, f58 mode, f26 override,
        //   extent override (128,128).
        struct C {
            state: u8,
            life: u32,
            act: i16,
            max: i16,
            accel: i16,
            row: u8,
            f44: u16,
        }
        let c = match model {
            1 => C {
                state: 7,
                life: 2000,
                act: 50,
                max: 100,
                accel: 16,
                row: 13,
                f44: 100,
            },
            2 => C {
                state: 13,
                life: 3000,
                act: 35,
                max: 70,
                accel: 30,
                row: 14,
                f44: 350,
            },
            4 => C {
                state: 25,
                life: 1000,
                act: 30,
                max: 30,
                accel: 0,
                // Row 16, not 0: remc1's m4 ctor (sub_386DE) could not resolve
                // the row symbol and substituted unk_98F38[0]; the unresolved
                // declaration survives commented out as `//int unk_99138;//fix`
                // directly above it, and unk_99138 self-identifies as row 16.
                // Every other single-body ctor maps model n -> row 12+n, and
                // row 16 is referenced by no constructor anywhere.
                row: 16,
                f44: 500,
            },
            5 => C {
                state: 31,
                life: 5000,
                act: 30,
                max: 30,
                accel: 3,
                row: 17,
                f44: 500,
            },
            7 => C {
                state: 43,
                life: 0,
                act: 20,
                max: 20,
                accel: 3,
                row: 19,
                f44: 500,
            },
            8 => C {
                state: 49,
                life: 10000,
                act: 40,
                max: 40,
                accel: 20,
                row: 20,
                f44: 1000,
            },
            // State 54, NOT 55 — breaks the 6n+1 family pattern (:45258).
            9 => C {
                state: 54,
                life: 1000,
                act: 20,
                max: 20,
                accel: 0,
                row: 21,
                f44: 500,
            },
            10 => C {
                state: 61,
                life: 2000,
                act: 60,
                max: 60,
                accel: 20,
                row: 22,
                f44: 500,
            },
            // State 66, NOT 67 (:45364).
            11 => C {
                state: 66,
                life: 20000,
                act: 60,
                max: 60,
                accel: 20,
                row: 23,
                f44: 500,
            },
            12 => C {
                state: 73,
                life: 1000,
                act: 40,
                max: 40,
                accel: 20,
                row: 10,
                f44: 500,
            },
            13 => C {
                state: 79,
                life: 1000,
                act: 40,
                max: 40,
                accel: 20,
                row: 10,
                f44: 500,
            },
            14 => C {
                state: 85,
                life: 1000,
                act: 40,
                max: 40,
                accel: 20,
                row: 10,
                f44: 500,
            },
            15 => C {
                state: 91,
                life: 1000,
                act: 30,
                max: 30,
                accel: 0,
                row: 24,
                f44: 500,
            },
            16 => C {
                state: 97,
                life: 100000,
                act: 60,
                max: 60,
                accel: 20,
                row: 25,
                f44: 500,
            },
            _ => return None,
        };
        let i = self.new_event()?;

        // Model 7 (sub_38C60 :45123): life and sprite alternate on the
        // per-model spawn ordinal's parity (sub_38C00 :45101).
        let ordinal = self.spawn_count[model as usize];
        let life = if model == 7 {
            if ordinal & 1 != 0 { 4000 } else { 2000 }
        } else {
            c.life
        };

        {
            let e = &mut self.ent[i];
            e.class64 = 5;
            e.model65 = model as u8;
            e.tick70 = c.state;
            e.max_life = life;
            e.f126 = c.act;
            e.f128 = c.max;
            e.f130 = c.accel;
            e.f44 = c.f44;
            e.row156 = c.row;
            e.f66 = 3;
            // Model 2 is the ONLY creature that narrows the second
            // filter byte (sub_38370 :44744, `+67 = 0`) — a census of
            // all sixteen class-5 ctors finds no other `+67` write, so
            // every other model keeps NewEvent's 0xFF wildcard. The bee
            // therefore stings the HUMAN wizard alone (class 3 model 0)
            // where its siblings admit any class-3 body, rivals
            // included.
            if model == 2 {
                e.f67 = 0;
            }
        }

        // Mana: most models sub_36F90 (+140 = life/2, :43741); the
        // m5 growth creature and the m12/13/14 villagers are explicit.
        match model {
            5 => {
                self.ent[i].f140 = 500;
                self.ent[i].f136 = 12000;
            }
            12 | 13 | 14 => self.ent[i].f140 = 0,
            15 => self.ent[i].f140 = 0,
            _ => self.ent[i].f140 = (life >> 1) as i32,
        }
        if model == 11 {
            // :45370: +136 = 2 * (+140).
            self.ent[i].f136 = 2 * self.ent[i].f140;
        }

        // Facing draw — the event LCG's first draw. m1/m15 draw
        // nothing (facing 0); m9 rolls % 0x832 (:45264).
        let facing = match model {
            1 | 15 => 0u16,
            9 => (self.ent_rand(i) % 0x832).wrapping_sub(1) as u16,
            _ => (self.ent_rand(i) & 0x7FF).wrapping_sub(1) as u16,
        };
        self.spawn_facing(i, facing);
        self.ent[i].f28 = 1;

        // Bookkeeping: +26 state timer, +63 = per-model spawn ordinal
        // (counter incremented), +58 scan phase from the behavior
        // row's word 26.
        let v26 = BEHAVIOR[c.row as usize].v_26;
        {
            let e = &mut self.ent[i];
            e.f26 = match model {
                9 => (i % 10) as i16 + 29,
                11 | 16 => 0,
                12 | 13 | 14 => 2,
                _ => (i % 100) as i16,
            };
            e.f63 = ordinal;
        }
        self.spawn_count[model as usize] = ordinal.wrapping_add(1);
        self.ent[i].f58 = match model {
            1 => (v26 & 0xFF) + 1,
            // The phase-spread family by ROW, not by intuition: the
            // ctor census finds the `v26 - (ord % v26) + 4` seed on
            // rows 14 (:44753), 16 (:44934), 17 (:45004), 21 (:45283)
            // and 24 (:45629) — m5's row-17 site was misfiled here
            // under the flat 64 (mc1l32 t=33135: the village dwellers
            // at slots 54/56 seed 30/29 = 30 - ord + 4, and the port's
            // 64 woke them 34 ticks long, flipping the crab chase at
            // 33144 via the pack scan).
            2 | 4 | 5 | 9 | 15 => v26 - (ordinal as i16 % v26) + 4,
            _ => 64,
        };
        if model == 15 {
            self.ent[i].flags |= 0x20000; // :45622, +18 |= 2
        }

        self.link(i, x, y, z);
        // The m9 ctor GROUND-SNAPS its z after the place — sub_38E70
        // ends `+76 = sub_11F50(+72)` (:45289), overwriting the z the
        // link just stored, exactly the standing-fire ctor's shape
        // (:46640). A ctor census finds it on model 9 ALONE of the
        // seventeen class-5 ctors. mc1l5 t=4478: the hidden mound
        // converts militia 780 mid-descent (z 337, ground 334) and
        // retail's newborn burrower lands ON the ground.
        if model == 9 {
            let (px, py) = (self.ent[i].x, self.ent[i].y);
            self.ent[i].z = self.ground_z(px, py) as i16;
        }
        // m7 sets +12 = +8 inline instead of RefillLife (:45118) —
        // identical result.
        self.refill_life(i);

        // Sprite type; m13 draws the event LCG a second time here
        // (:45505): % 7 in 0..3 → 217, else 218.
        let t = match model {
            1 => 86,
            2 => 3,
            4 | 15 => 0,
            5 => 185,
            7 => {
                if ordinal & 1 != 0 {
                    85
                } else {
                    199
                }
            }
            8 => 47,
            9 => 220,
            10 => 208,
            11 => 200,
            12 => 221,
            13 => {
                if self.ent_rand(i) % 7 < 4 {
                    217
                } else {
                    218
                }
            }
            14 => 219,
            16 => 207,
            _ => unreachable!(),
        };
        self.set_sprite(i, t);
        if model == 7 {
            self.ent[i].f71 = if ordinal & 1 != 0 { 1 } else { 2 };
        }
        // All simple creatures override the horizontal extents to a
        // half-tile square (sub_37130(128,128)) — except m1.
        if model != 1 {
            self.extents(i, 128, 128);
        }
        Some(i)
    }

    // ---- movement core -----------------------------------------------------

    /// sub_11810 (:16879): terrain capability bit by the tile's type
    /// byte; a creature may stand on a tile iff its behavior row's
    /// v_20 mask has the bit set.
    pub(crate) fn cap_bit(&self, x: u16, y: u16) -> u32 {
        let t = self.t.tile_type[(((y >> 8) as usize) << 8) | (x >> 8) as usize];
        match t {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 0x10,
            5 => 0x20,
            8 => 0x100,
            9 => 0x200,
            10 => 0x100000,
            11 => 0x200000,
            12 => 0x400000,
            13 | 14 => 0,
            15..=20 | 28..=34 => 0x400,
            21 | 22 | 24 => 0x20000,
            23 => 0x40000,
            25 | 27 => 0x80000,
            26 => 0x10000,
            _ => 0x800000,
        }
    }

    /// sub_19650 (:21149): local roughness — max corner-height cross
    /// difference of the tile under the position, raw height bytes.
    pub(crate) fn roughness(&self, x: u16, y: u16) -> i32 {
        let (tx, ty) = ((x >> 8) as u8, (y >> 8) as u8);
        let h = |dx: u8, dy: u8| {
            self.t.height[(((ty.wrapping_add(dy)) as usize) << 8) | tx.wrapping_add(dx) as usize]
                as i32
        };
        let (h00, h10, h01, h11) = (h(0, 0), h(1, 0), h(0, 1), h(1, 1));
        (h00 + h01 - h10 - h11)
            .abs()
            .max((h00 + h10 - h01 - h11).abs())
    }

    /// MC2 `sub_1B830` (Terrain.cpp:1602) — the WIDE roughness probe:
    /// the same max-cross formula as `roughness` (MC2's
    /// `sub_1B7A0_tile_compare`, Terrain.cpp:1578) but on the
    /// 2-tile-span stencil (tx-1,ty-1)/(tx+1,ty-1)/(tx+1,ty+1)/
    /// (tx-1,ty+1), so on a planar slope it reads exactly DOUBLE.
    /// The m27 hydra's ground mover and emerge probe are its ONLY
    /// callers (EF:20912, EF:20926, EF:19498) — every other retail
    /// roughness site calls the tight `sub_1B7A0` above.
    /// The listing's fourth corner (`uindex._axis_2d.x += 2`,
    /// Terrain.cpp:1620) is a hand-conversion error: the shipped
    /// NETHERW.EXE walks BACK (`sub al,0x2` at linear 0x1B86C, file
    /// 0x4006C) to tx-1, which is what the function's own ASCII
    /// stencil draws.
    pub(crate) fn roughness_wide(&self, x: u16, y: u16) -> i32 {
        let (tx, ty) = ((x >> 8) as u8, (y >> 8) as u8);
        let (xm, xp) = (tx.wrapping_sub(1), tx.wrapping_add(1));
        let (ym, yp) = (ty.wrapping_sub(1), ty.wrapping_add(1));
        let h = |hx: u8, hy: u8| self.t.height[((hy as usize) << 8) | hx as usize] as i32;
        let (p1, p2, p3, p4) = (h(xm, ym), h(xp, ym), h(xp, yp), h(xm, yp));
        (p1 + p4 - p2 - p3).abs().max((p1 + p2 - p4 - p3).abs())
    }

    /// sub_42000_42340 (:52576): altitude clamp toward the behavior
    /// band [ground+v_12, ground+v_10] with step v_14 (quarter step
    /// inside the band, hard floor below).
    pub(crate) fn alt_clamp(z: &mut i16, ground: i16, row: &BehaviorRow) {
        if *z > ground.wrapping_add(row.v_10) {
            *z = z.wrapping_add(row.v_14);
        } else if *z > ground.wrapping_add(row.v_12) {
            *z = z.wrapping_add((25 * row.v_14 as i32 / 100) as i16);
        }
        if *z < ground.wrapping_add(row.v_12) {
            *z = ground.wrapping_add(row.v_12);
        }
    }

    /// sub_41EC0_42200 (:52523): polar step — dist along (yaw, pitch)
    /// on the 16.16 sine tables; yaw 0 = -y (north), positive pitch
    /// steps downward (z -= dist·sin).
    pub(crate) fn polar_step(pos: &mut (u16, u16, i16), yaw: u16, pitch: u16, dist: i16) {
        if dist == 0 {
            return;
        }
        let yaw = (yaw & 0x7FF) as usize;
        let pitch = (pitch & 0x7FF) as usize;
        let (horiz, dz) = if pitch != 0 {
            (
                ((dist as i32 * COS[pitch]) >> 16),
                ((dist as i32 * SIN[pitch]) >> 16),
            )
        } else {
            (dist as i32, 0)
        };
        pos.2 = pos.2.wrapping_sub(dz as i16);
        pos.0 = pos.0.wrapping_add(((horiz * SIN[yaw]) >> 16) as u16);
        pos.1 = pos.1.wrapping_sub(((horiz * COS[yaw]) >> 16) as u16);
    }

    /// sub_42210 (:52652): angular distance on 11-bit angles.
    pub(crate) fn angdist(a: u16, b: u16) -> u16 {
        let d = a.wrapping_sub(b) & 0x7FF;
        if d > 1024 { 2048 - d } else { d }
    }

    /// sub_42240_42580 (:52664, MC2's twin `sub_582F0` Sound.cpp:6580 —
    /// identical bodies): which WAY to turn from `cur` toward `tgt`,
    /// −1/0/+1.
    ///
    /// THE 180° TIE-BREAK IS NOT SYMMETRIC. Retail takes the plain
    /// integer difference of the two masked angles and only unwraps it
    /// when `abs(v3) > 1024` — **strictly** greater. So a target exactly
    /// 1024 (180°) away keeps the RAW sign: `tgt` numerically below
    /// `cur` turns NEGATIVE, above turns POSITIVE. Deriving the sign
    /// from the wrapped delta instead (`(tgt−cur) & 0x7FF <= 1024`)
    /// agrees everywhere except that one tie, where it turns the wrong
    /// way by a full `2·cap` (CONFORMANCE-FINDINGS.md §"THE (5,23)
    /// RETRY-LEG PAIR IS THE 180° TURN TIE-BREAK") — the antipodal move
    /// -core retry (`yaw0 + 0x400`) lands on it every time it fires
    /// against a creature already facing its wander target.
    pub(crate) fn turn_sign(cur: u16, tgt: u16) -> i16 {
        if no_turn_tie_fix() {
            return if tgt.wrapping_sub(cur) & 0x7FF <= 1024 {
                1
            } else {
                -1
            };
        }
        let mut v3 = (tgt & 0x7FF) as i32 - (cur & 0x7FF) as i32;
        if v3.abs() > 1024 {
            v3 -= if v3 >= 0 { 2048 } else { -2048 };
        }
        i16::from(v3 > 0) - i16::from(v3 < 0)
    }

    /// sub_422A0_425E0 (:52689): rate-limited turn from `cur` toward
    /// `tgt`, capped at the row's v_2 (v_4 is passed but dead).
    pub(crate) fn turn_step(cur: u16, tgt: u16, cap: i16) -> i16 {
        if cur == tgt {
            return 0;
        }
        let d = Self::angdist(cur, tgt) as i16;
        Self::turn_sign(cur, tgt) * d.min(cap)
    }

    /// One candidate probe of the movement core: clamp + step from the
    /// current position, then the block test (terrain mask + local
    /// roughness; crossing into a new tile only).
    fn move_probe(
        &self,
        i: usize,
        yaw: u16,
        row: &BehaviorRow,
        first: bool,
    ) -> Option<(u16, u16, i16)> {
        let e = &self.ent[i];
        let mut tmp = (e.x, e.y, e.z);
        let ground = self.ground_z(e.x, e.y) as i16;
        Self::alt_clamp(&mut tmp.2, ground, row);
        Self::polar_step(&mut tmp, yaw, 0, e.f126);
        // The same-tile shortcut applies ONLY to the first candidate
        // (:21225-30) — the three retry headings test the mask
        // unconditionally (:21252/:21274/:21291). This is what kills
        // a BEACHED KRAKEN (row 18's v_20 = water-only): terrain
        // raised under it → the next boundary crossing fails all
        // four candidates → life = -1. Extending the shortcut to
        // every candidate lets it bounce forever inside the land tile.
        if first && e.x >> 8 == tmp.0 >> 8 && e.y >> 8 == tmp.1 >> 8 {
            return Some(tmp);
        }
        // sub_11640 mode 1: capability mask; then roughness < v_16.
        if self.cap_bit(tmp.0, tmp.1) & !row.v_20 != 0 {
            return None;
        }
        if self.roughness(tmp.0, tmp.1) >= row.v_16 as i32 {
            return None;
        }
        Some(tmp)
    }

    /// Movement core sub_196E0 (:21182): altitude clamp → polar step →
    /// wall rule with three retry headings (±341 ≈ ±60°, then
    /// reversed) — all four blocked kills the creature (life = -1,
    /// the emergent behavior the carpet inherits differently). Commits
    /// via move_relink, then turns toward +34 capped at v_2.
    fn creature_move(&mut self, i: usize) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let v31 = self.ent[i].f30;
        let candidates = [
            v31,
            v31.wrapping_add(341) & 0x7FF,
            v31.wrapping_sub(341) & 0x7FF,
            v31.wrapping_add(1024) & 0x7FF,
        ];
        let mut committed = false;
        for (k, &yaw) in candidates.iter().enumerate() {
            if k > 0 {
                // Failed candidates leave +30 mutated (:21239).
                self.ent[i].f30 = yaw;
            }
            if let Some(tmp) = self.move_probe(i, yaw, row, k == 0) {
                self.move_relink(i, tmp.0, tmp.1, tmp.2);
                committed = true;
                break;
            }
        }
        if !committed {
            // :21293 — the walled-in kill. It does NOT end the tick:
            // no retail handler re-tests its own life after the mover
            // (:21503 wander, :21654 chase, :21769 pack, :23057 m5 eat,
            // :23205/:24188 m7/m9 chases, :23747 m9 hidden, :24469/
            // :24632 genie, :25068/:25380/:25556 villagers, :26145
            // wyvern — every one falls straight into its think/scan/
            // fire body), so the dying tick still draws, re-bears and
            // SHOOTS; the death is taken by the NEXT tick's prologue.
            // mc1l3 t=5289: militia 130 walls in mid-chase (all four
            // candidates refused, +30 left at the reverse probe) and
            // retail still mints its 500-damage dart at the human.
            self.ent[i].act_life = -1;
            return;
        }
        let e = &self.ent[i];
        let turn = Self::turn_step(e.f30, e.f34, row.v_2);
        self.ent[i].f30 = (self.ent[i].f30 as i32 + turn as i32) as u16 & 0x7FF;
    }

    /// The human player's commit gate sub_45410_45750 (:55065):
    /// type-8 wall tiles are horizontally impassable for the carpet at
    /// ANY altitude (`sub_11810 == 0x100` — only the wall type maps to
    /// exactly that mask; the human row 7 clears bit 0x100 while every
    /// flying creature row allows it). A blocked move retries along
    /// the two cardinals adjacent to the move bearing (floor, then
    /// ceil multiple of 512), each stepped from the CURRENT position
    /// scaled by angular proximity `dist·(512-Δ)>>9` — the original's
    /// wall slide; both blocked → the whole move is discarded (None).
    /// The routine's unconditional trailing z-floor (ground + row
    /// v_12) stays with the flyer's own clamp for now (Phase 5).
    ///
    /// CAVE ARM (MC2, Phase 4.5): sealed bit3 tiles block like walls
    /// — retail's MC2 commit gate refuses any move onto a sealed
    /// tile (`moveTest_5D0A0` EF:59594-97). The full headroom
    /// steer-search (EF:59515-93) belongs to the real MC2 commit
    /// gate (Phase 4.4); until then the MC1 cardinal slide stands in
    /// for the steer.
    pub(crate) fn player_wall_gate(
        &self,
        cur: (u16, u16, i16),
        prop: (u16, u16, i16),
    ) -> Option<(u16, u16, i16)> {
        let (ok, slid) = self.player_wall_slide(cur, prop);
        ok.then_some(slid)
    }

    /// sub_45410's FULL contract: the verdict AND the axis the routine
    /// leaves in the global scratch `word_AE454_AE444` (:14798).
    ///
    /// ⚠⚠ THE SCRATCH SURVIVES A REFUSAL, AND SOMETHING READS IT.
    /// Retail never restores the scratch when both cardinals fail — it
    /// re-seeds from the pose (:55092), writes the SECOND cardinal's
    /// step into the scratch (:55094-98), tests it, and sets `v10 = 0`
    /// (:55099-100) while leaving that step standing. Only the COMMIT
    /// is conditional (:55250-52 `if (v26) sub_41C70_41FB0(a1, &scr)`).
    /// The carpet's death-fall handler then spawns its fire trail at
    /// the scratch, not at the pose (:55478 — see the caller in
    /// `world.rs`), so a corpse sliding along a wall drops its trail
    /// on the blocked side. Returning `Option` here threw the axis
    /// away and cost mc1l32-quick its t=19134 head.
    ///
    /// The routine's trailing z-floor (:55103-05) is UNCONDITIONAL and
    /// outside every branch, so it applies to the refused scratch too;
    /// it is applied by the caller (`flight::mc1_move`), which owns
    /// the `ground` closure. HW is line-identical (hw :51133-51174).
    pub(crate) fn player_wall_slide(
        &self,
        cur: (u16, u16, i16),
        prop: (u16, u16, i16),
    ) -> (bool, (u16, u16, i16)) {
        let blocked = |x: u16, y: u16| {
            self.cap_bit(x, y) == 0x100
                || (self.is_cave()
                    && self.t.angle[crate::engine::features::tile((x >> 8) as u8, (y >> 8) as u8)]
                        & 8
                        != 0)
        };
        if !blocked(prop.0, prop.1) {
            return (true, prop);
        }
        let v1 = Self::angle_between(cur.0, cur.1, prop.0, prop.1);
        // sub_42340 (3D distance) and sub_42180 (vertical bearing).
        let dh2 = Self::dist2_sq(cur.0, cur.1, prop.0, prop.1);
        let dz = prop.2.wrapping_sub(cur.2) as i32;
        let v7 = Self::isqrt((dh2 as u32).wrapping_add((dz * dz) as u32)) as i32;
        let v8 = Self::pitch_toward(cur.2, prop.2, Self::isqrt(dh2 as u32) as i32);
        // Each pass re-seeds the scratch from the POSE (:55082,
        // :55092) before stepping, so the second cardinal is measured
        // from `cur`, never from the first attempt.
        let mut slid = cur;
        for cardinal in [(v1 >> 9) << 9, ((v1 >> 9).wrapping_add(1) << 9) & 0x7FF] {
            let scaled = (v7 * (512 - Self::angdist(v1, cardinal) as i32)) >> 9;
            slid = cur;
            Self::polar_step(&mut slid, cardinal, v8, scaled as i16);
            if !blocked(slid.0, slid.1) {
                return (true, slid);
            }
        }
        // Refused — but the SECOND cardinal's step is what retail left
        // in the scratch, and that is what the trail spawn reads.
        (false, slid)
    }

    // ---- the six state primitives (:21311-:21871) --------------------------

    /// Squared 2D distance in engine units (16-bit wrapping deltas).
    ///
    /// ⭐ ELECTION KEYS ARE UNSIGNED. Every retail nearest-X election
    /// seeds `unsigned int v = -1` and compares this sum UNSIGNED
    /// (sub_19D70 :21516, sub_143A0 :18505, sub_15340 :18989, …), so
    /// the one attainable sign-bit value — dx = dy = -32768, the exact
    /// half-map diagonal, 2·2³⁰ = `i32::MIN` — reads as the LARGEST
    /// key: it loses every election and fails every `<= v_28²` range
    /// pre-gate. A signed key elected the phantom "nearest" instead
    /// (the landed scout law, mc1l49 t=3298). Election sites therefore
    /// cast this `as u32`; the WINNER's range gate stays per-site
    /// (signed `int` recomputes at :18531/:18910, unsigned at :19789).
    pub(crate) fn dist2_sq(ax: u16, ay: u16, bx: u16, by: u16) -> i32 {
        let dx = bx.wrapping_sub(ax) as i16 as i32;
        let dy = by.wrapping_sub(ay) as i16 as i32;
        dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
    }

    /// Pack scan (inside IDLE :21384 / asleep WANDER, and the militia's
    /// own copy :22651-84): nearest same-model packless creature within
    /// v_28² and the v_30 facing cone becomes the leader; state →
    /// base+3.
    ///
    /// THE CANDIDATE SET IS THE TICK-TOP CHAIN, not the pool. Retail
    /// walks `var_u32_36462[model]` head-to-tail through `->next`
    /// (:22653-77) — the per-model roster rebuilt once at the top of
    /// the tick (:52287-313) — so a creature BORN THIS TICK is not yet
    /// a member and cannot be seen, by itself or by anyone else. The
    /// port's pool scan saw newborns and paired them instantly: mc1l2's
    /// village collapse at t=4935 evacuates militia into slots 285/286/
    /// 287 in ONE tick, and retail leaves all three unpaired (they only
    /// find each other later — 286 packs onto 287 at t=5054), where the
    /// port packed 287 onto 286 on their shared birth tick. That single
    /// wrong pairing is the whole mc1l2 free-run break: a packed
    /// militiaman follows its leader instead of running the two-draw
    /// wander, so its own LCG stops advancing and every later roll on
    /// that entity is off by the draws it never made.
    ///
    /// The chain rebuild already applies the `act_life >= 0` and
    /// `tick70 != 120` membership gates, which is why retail's walk
    /// re-tests neither — only `+52 == 0` and identity (:22660).
    fn pack_scan(&mut self, i: usize, base: u8) {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let (ex, ey, yaw, model) = (e.x, e.y, e.f30, e.model65);
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        let cone = row.v_30 as u16;
        let mut best: Option<(usize, u32)> = None;
        for &member in self.mob_chains.visible(model as usize) {
            let j = member as usize;
            let c = &self.ent[j];
            if j == i || c.f52 != 0 {
                continue;
            }
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if d2 > r2 {
                continue;
            }
            if Self::angdist(yaw, Self::angle_between(ex, ey, c.x, c.y)) >= cone {
                continue;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        if let Some((j, _)) = best {
            self.ent[i].f52 = j as u16;
            self.ent[i].tick70 = base + 3;
        }
    }

    /// Scan A (sub_19D70 :21519-42): the nearest bucket[0] body within
    /// this creature's v_28² range and v_30 facing cone. Retail's
    /// bucket[0] (`var_u32_36462[0]`, rebuilt at :52253 from every live
    /// class-3 entity) holds the wizard CARPETS *and* the CASTLES, and
    /// the shared creature scans carry no +65 body filter — so a castle
    /// (class 3, model 2) is as valid a chase target as a carpet (the
    /// m9 mound already hunts castles off this same list, :23752).
    ///
    /// bucket[0] holds every LIVE class-3 body regardless of model —
    /// the case-3 rebuild gate is `actLife>=0 && (flags & 0x10)==0`, and
    /// 0x10 is a dead bit that nothing ever sets (:52254), so the four
    /// class-3 models all qualify: 0 human carpet, 1 rival carpet, 2
    /// castle (:44245, maxLife 40000), 3 mana balloon (:44277, maxLife
    /// 10000). Retail even extends the m9 mound's attack reach by a
    /// castle's footprint (:24201) — castle-attacking is deliberate.
    ///
    /// The human lives outside the pool, so it is the first candidate
    /// (via `ctx`, with the invisibility gate — spell 12's +16 0x20 bit
    /// mirrored in `player_invisible` — and the undead-army owner gate:
    /// a creature the human OWNS never targets it); rival carpets,
    /// castles and balloons are
    /// the pool members (model 0 would be a second human body — never
    /// spawned, since ours is out-of-pool — so it is skipped). A
    /// creature never targets its own owner or its own castle/balloon
    /// (the +24 exclusion, verbatim for the m9/m15 scans and a kept
    /// extension of the port's existing human gate for the rest). A body
    /// being removed/captured carries the 0x20 bit and is skipped,
    /// exactly as retail's per-node gate (0x420 folds in the port's
    /// 0x400 removed/dead bit). Ascending pool index matches bucket[0]'s
    /// rebuild order for the tie-break; the out-of-pool human, which has
    /// no index, wins ties by going first. Returns [`PLAYER_TARGET`] or
    /// a pool entity index; `None` when the cone holds no body.
    ///
    /// `bodies_only` is retail's `+65 <= 1` gate: the genie's aggro scan
    /// (:24487) restricts to wizard CARPETS (model 1), skipping castles
    /// and balloons; the wyvern/crab/mound/guard scans pass `false` and
    /// take the whole list.
    ///
    /// `wanted_only` is the m4 militia (:22613) / m8 griffon (:23500)
    /// hostility gate: the winner must be a wizard whose village-wanted
    /// timer is live (the human's `player_aggro`, a rival's
    /// `rival_wanted` slot — see [`Gen::village_wanted`]).
    ///
    /// ⭐ GATE PLACEMENT FORKS BY CALLER, and it is load-bearing. The
    /// genie's `+65 <= 1` is a PER-CANDIDATE filter (:24487 tests it
    /// inside the loop before distance), so its election falls past a
    /// nearer castle to a farther carpet. The m4/m8 scans instead elect
    /// the nearest class-3 body on range+cone ALONE and apply BOTH
    /// gates to the ELECTION WINNER (base :22613/:23500, hw 21170-72/
    /// 22057 — `if (v25 && *(v25+65) <= 1 && *(*(v25+160)+528))`): a
    /// nearer balloon/castle/un-wanted carpet wins the election, fails
    /// the gate, and the whole scan yields NOTHING — the caller drops
    /// to its next rung (the militia's burrower hunt, the wander's
    /// pack-up). mc1hwl0 t=3286: griffon 79's cadence tick has the
    /// human wanted (aggro 197) at d≈5904 and a mana balloon at d≈1375;
    /// retail elects the balloon, refuses it, and PACKS with griffon 72
    /// (f52=72, state base+3) where the in-loop port skipped the
    /// balloon and CHASED the human (state base+2).
    pub(crate) fn nearest_wizard_target(
        &self,
        i: usize,
        ctx: &MobCtx,
        bodies_only: bool,
        wanted_only: bool,
    ) -> Option<u16> {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        let cone = row.v_30 as u16;
        let (ex, ey, ef30, owner) = (e.x, e.y, e.f30, e.id24);
        // ⭐⭐⭐ The m8 GRIFFON's wander (`sub_1CA50`, state 0x31) is
        // the one copy of this scan with NO per-node `+16 & 0x20`
        // test — see [`griffon_scan_cloak_blind`] for the shipped
        // bytes beside `sub_19D70`'s and `sub_1B5D0`'s. It is reached
        // only from [`Gen::mob_wander`], and only there with
        // `wanted_only`, so the model tells the caller apart.
        let cloak_gate = !(wanted_only && e.model65 == 8 && griffon_scan_cloak_blind());

        let mut best: Option<u16> = None;
        let mut best_d2 = u32::MAX;

        // The human wizard (bucket[0]'s out-of-pool member). The
        // rebuild gate applies to him like any other class-3 body:
        // bucket[0] admits a class-3 record only if `actLife >= 0 &&
        // !(flags & 0x10)` (:52254 — CARPET.EXE VA `0x41959:
        // 83 78 0c 00` = `cmp DWORD [eax+0xc],0` / `0x4195D: 7c 24`
        // jl / `0x4195F: f6 40 10 10` = `test BYTE [eax+0x10],0x10`,
        // then `0x41973: 89 83 6e 8e 00 00` publishes the head).
        // ⭐ BUT THAT GATE RUNS AT THE TICK TOP, ONCE, AND THE SCAN
        // ONLY WALKS THE RESULT. `sub_1FF60`'s loop
        // (VA `0x200F1..0x2018A`) re-tests the owner tag, the 3-D
        // range, the cloak bit (`0x20129: f6 43 10 20` — the ONLY
        // per-node flag test) and the cone; an exhaustive scan of
        // that range finds NO reference to `actLife` (`+0xc`) at
        // all. So a wizard who dies DURING a tick stays acquirable
        // by every creature processed later in that same tick, and
        // membership must be sampled with `dead_top`, never the
        // live `dead` (which `World` republishes mid-walk at the
        // carpet's slot). WITNESS mc1l48-nodeath t=4892: the human
        // (slot 681) takes a fatal `(9,0)` from id 641 at the tick
        // TOP (`act_life 380 → −20`); five guards correctly break
        // their chases on the live read (746/832/903/905/943,
        // `f70 92→91`) and guard 829 ACQUIRES HIM ANYWAY
        // (`f146 0→681`, `f126 30→0`, `type86 0→1`), where the port
        // left it wandering. ⚠ The comment this replaces asserted
        // the opposite — "from the fatal hit onward no scan can
        // acquire him" — a passing doc comment encoding a false
        // claim (round 115's law). It is true from the NEXT tick on.
        // The five other MC1 `view.dead` reads (:1379, :1748, :2119,
        // :2388, :3940) are chase TARGET-LOSS tests, which retail
        // does read live off the victim record; they stay as they
        // are.
        // …and his seat must be inside the chain's visible prefix
        // ([`Gen::mc1_human_on_wiz_chain`] — the seizure blank / sever
        // law, the same test the fireball acquire makes).
        // (With the human ON the roster he is met at his own slot in
        // the walk below — [`crate::engine::features::human_on_wiz_chain`].)
        // Every seat off the roster is judged ahead of the walk; on it,
        // at his own record.
        for hx in self.human_tag.ixs() {
            if !self.human_rides_chain(hx)
                && !self.seat_dead_top(hx)
                && !(self.player_cloaked(hx) && cloak_gate)
                && self.human_tag[hx].0 != owner
                && self.mc1_human_on_wiz_chain(hx)
            {
                let (hpx, hpy, _) = self.seat_at(ctx, hx);
                let d2 = Self::dist2_sq(ex, ey, hpx, hpy) as u32;
                if d2 <= r2
                    && d2 < best_d2
                    && Self::angdist(ef30, Self::angle_between(ex, ey, hpx, hpy)) < cone
                {
                    best = Some(self.human_tag[hx].0);
                    best_d2 = d2;
                }
            }
        }

        // Pool bodies: the TICK-TOP bucket[0] roster ([`Gen::wiz_chain`]),
        // minus the out-of-pool human's model 0 — rival carpets (1),
        // castles (2), mana balloons (3). Under `bodies_only` (the
        // genie's +65<=1 gate) only the rival carpets qualify; castles
        // and balloons are skipped.
        //
        // ⭐ THE LIFE TEST IS THE CHAIN BUILD, NOT A GUARD HERE. Retail's
        // per-node gate is `(+16 & 0x20) == 0` and nothing else
        // (:21524) — liveness was sampled once, at the tick-top rebuild
        // (:52253). So a body that dies MID-tick stays acquirable for
        // the rest of that tick, the same "a soft kill is not a free"
        // shape the class-9 despawn and the balloon self-kill already
        // wear; the port's old `act_life < 0` / `0x400` conjuncts were
        // the stand-in for the snapshot it did not have. (A record that
        // entered the tick already flagged is REAPED above this sweep,
        // so it is not in the roster at all.)
        for c in 0..self.wiz_chain.visible_len() {
            let j = self.wiz_chain.list[c] as usize;
            let c = &self.ent[j];
            // The genie's `+65 <= 1` is the only IN-LOOP model gate
            // (:24487); the m4/m8 scans (`wanted_only`) elect ungated
            // and test the winner below.
            let seat = self.hx_of_rec(j);
            let human = seat.is_some();
            if (c.model65 == 0 && !human) || (bodies_only && !wanted_only && c.model65 > 1) {
                continue;
            }
            // The human at his slot: his tag, the cloak register, the
            // published pose — the inputs his old pre-pass read.
            let (cid, cloaked, cx, cy) = if let Some(hx) = seat {
                let (hpx, hpy, _) = self.seat_at(ctx, hx);
                (self.human_tag[hx].0, self.player_cloaked(hx), hpx, hpy)
            } else {
                (c.id24, c.flags & 0x20 != 0, c.x, c.y)
            };
            if (cloak_gate && cloaked) || owner == cid {
                continue;
            }
            let d2 = Self::dist2_sq(ex, ey, cx, cy) as u32;
            if d2 <= r2
                && d2 < best_d2
                && Self::angdist(ef30, Self::angle_between(ex, ey, cx, cy)) < cone
            {
                best = Some(if human { cid } else { j as u16 });
                best_d2 = d2;
            }
        }
        // The m4/m8 winner gate (:22613/:23500): the nearest body must
        // BE a wanted wizard, or the scan comes home empty-handed —
        // there is no falling past the winner to the second-nearest.
        if wanted_only {
            match best.map(|j| (j, self.hx_of_tag(j))) {
                Some((_, Some(hx))) => {
                    if self.player_aggro[hx] <= 0 {
                        return None;
                    }
                }
                Some((j, None)) => {
                    let c = &self.ent[j as usize];
                    if c.model65 > 1 || self.village_wanted(c.id24) <= 0 {
                        return None;
                    }
                }
                None => {}
            }
        }
        best
    }

    /// The player slot (0 = human, 1..=7 = rival) that owns `id` — an
    /// owner tag: [`PLAYER_TARGET`] for the human, else a wizard entity
    /// index (a rival carpet is its own owner tag). `None` if `id` names
    /// no live wizard.
    fn wizard_slot_of(&self, id: u16) -> Option<usize> {
        if let Some(hx) = self.hx_of_tag(id) {
            return Some(self.human_seat(hx) as usize);
        }
        self.rival_ents.iter().position(|&e| e != 0 && e == id)
    }

    /// A wizard's village-wanted timer (retail's +528), by owner tag:
    /// the human reads `player_aggro`, a rival its `rival_wanted` slot.
    /// 0 for a non-wizard tag.
    pub(crate) fn village_wanted(&self, id: u16) -> i16 {
        match self
            .wizard_slot_of(id)
            .map(|s| (s, self.hx_at_wizard(s as u8)))
        {
            Some((_, Some(hx))) => self.player_aggro[hx],
            Some((s, None)) => self.rival_wanted[s],
            None => 0,
        }
    }

    /// Flag a wizard village-wanted for 200 ticks (the +528 = 200
    /// writers): the human raises `player_aggro`, a rival its
    /// `rival_wanted` slot. A non-wizard tag is a no-op.
    #[track_caller]
    pub(crate) fn flag_village_wanted(&mut self, id: u16) {
        if let Some(t) = crate::mail_trace() {
            eprintln!(
                "[wanted] t={t} id={id} from {}",
                std::panic::Location::caller()
            );
        }
        match self
            .wizard_slot_of(id)
            .map(|s| (s, self.hx_at_wizard(s as u8)))
        {
            Some((_, Some(hx))) => self.player_aggro[hx] = 200,
            Some((s, None)) => self.rival_wanted[s] = 200,
            None => {}
        }
    }

    /// IDLE sub_19B10 (:21311): stationary; every v_26 ticks a pack
    /// scan. (The damage inbox runs in `creature_tick` before
    /// dispatch, as the original's per-handler prologue.)
    fn mob_idle(&mut self, i: usize, base: u8) {
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 == 0 {
            self.pack_scan(i, base);
        }
    }

    /// WANDER sub_19D70 (:21421): move every tick; every v_26 ticks
    /// the two-draw yaw jitter (:21506 — d1 picks the sign via % 157,
    /// d2's low byte + 85 the magnitude), then — ONLY WHEN AWAKE
    /// (:21514, `if (+58)`) — the wizard scan (Scan A, the class-3
    /// hunt list :21519-42), falling back to the same-owner pack scan
    /// (Scan B :21546-73) when no wizard is in range/cone. EVERY
    /// awake creature runs both scans — the engine has no per-model
    /// aggro list. The m8 griffon alone gates Scan A on the wanted
    /// timer (sub_1CA50 :23500): it chases a wizard only while that
    /// wizard's +528 is live and re-arms it on the pounce (:23503),
    /// staying peaceful until a village marks the wizard — and that
    /// gate rules the ELECTION WINNER, not the candidates (see
    /// `nearest_wizard_target`): a nearer balloon/castle wins the
    /// election, fails the gate, and the griffon packs up instead.
    /// Asleep creatures never scan (getting this backwards packs whole
    /// distant crowds up onto the unbounded pack accel).
    fn mob_wander(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.creature_move(i);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 == 0 {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let mag = ((d2 & 0xFF) + 85) as i32;
            let sign = if d1 % 157 >= 79 { 1 } else { -1 };
            self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
            if self.ent[i].f58 != 0 {
                // m8 alone runs the wanted-gated bodies scan; everyone
                // else takes the whole class-3 list ungated.
                let griffon = self.ent[i].model65 == 8;
                if let Some(t) = self.nearest_wizard_target(i, ctx, griffon, griffon) {
                    self.ent[i].f146 = t;
                    self.ent[i].tick70 = base + 2;
                    if griffon {
                        self.flag_village_wanted(t); // re-arm +528, :23503
                    }
                } else {
                    self.pack_scan(i, base);
                }
            }
        }
    }

    /// m1's WANDER-wrapper trailer (`sub_1B200` :22260-88): after the
    /// shared two-scan, the SAME v_26 cadence walks the TICK-TOP ball
    /// chain for GRAVES — model 40 on the models-39|40 chain — nearest
    /// by 2-D distance within v_28², NO facing cone, NO awake or cloak
    /// test — and a find OVERRIDES whatever the shared scan just
    /// picked: chase = the grave, state = base+0, the vulture's moving
    /// idle (the glide-in). mc1l4 t=6888: the rival's three-tick-old
    /// grave out-pulls the human the shared Scan A had locked.
    fn m1_grave_hunt(&mut self, i: usize, base: u8) {
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        if (e.f63 as i16) % row.v_26 != 0 {
            return;
        }
        let (ex, ey) = (e.x, e.y);
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.ball_chain.visible_len() {
            let j = self.ball_chain.list[k] as usize;
            let c = &self.ent[j];
            if c.model65 != 40 {
                continue;
            }
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if d2 <= r2 && best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        if let Some((j, _)) = best {
            self.ent[i].f146 = j as u16;
            self.ent[i].tick70 = base;
        }
    }

    /// CHASE sub_1A120 (:21580): move; bearing to the target every 4th
    /// tick; every v_26 ticks either drop back to WANDER when the 3D
    /// distance reaches v_28 (un-squared — asymmetric with the scan's
    /// entry test, verbatim) or fire the per-model attack thunk
    /// (:21665-72). m6 arms a burst counter instead; the burst spawns
    /// run every tick while armed. (m2/m8/m11/m16 chase through their
    /// own wrappers/handlers above.)
    ///
    /// Returns retail's `sub_1A120` result: true ONLY on the tick the
    /// per-model thunk actually connected (:21668-69) — every other
    /// exit returns the zero-initialised `v15`. Wrappers trail on it;
    /// m7's `m7_chase` is the port's consumer.
    fn mob_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) -> bool {
        let model = self.ent[i].model65;
        self.creature_move(i);
        let tgt = self.ent[i].f146;
        // tf66/tf67 = the target's OWN filter fields (the player
        // entity keeps NewEvent's -1/-1): m6/m8 copy them onto their
        // beams (:23261-64, :22156-60) — hit-anything vs the player.
        // The position is a RAW struct read with no validity test
        // (:21657 dereferences +146 before anything looks at the
        // target's life) — a dead target's coordinates still steer.
        // The lost test reads the TARGET ENTITY with no special case
        // for the human (:21658 dereferences +146 whoever it names),
        // so a wizard who has just taken his fatal hit fails
        // `+12 < 0` on the very next chase tick and the creature
        // drops back to WANDER. Ours is out of the pool, so `dead`
        // stands in for his `actLife < 0` — without it every chaser
        // stayed latched onto the corpse through the death fall, the
        // dead hold and the respawn (the reported "monsters keep
        // attacking the body" bug).
        let (tx, ty, tz, tf66, tf67, lost) = if let Some(hx) = self.hx_of_tag(tgt) {
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                0xFFu8,
                0xFFu8,
                self.seat_dead(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            // ⭐ A `+146` OF 0 IS THE SCRATCH RECORD, AND IT STEERS THE
            // EXIT BEARING. Retail forms `v12 = &pool[+146]` with NO
            // validity test (:21655) and re-bears off it (:21657)
            // BEFORE the lost test reads `v12`'s own life and flags
            // (:21658) — so slot 0's coordinates go into `+34` on the
            // way out. That is not an exotic path: the PACK-DEATH
            // HANDOFF hands a survivor `+146 = the dier's +40`
            // (:21746), and `+40` is 0 whenever the fatal change did
            // not arrive through that entity's own `+94` mailbox slot
            // on the handoff tick (:21716). mc1l3 t=498 (slot 417
            // hands 421 over) and mc1l4 t=406 (slot 56 hands 39 over)
            // are the same law in two takes: the survivor aims at
            // slot 0 and only then drops back to `base + 1`. The early
            // return skipped the re-bear and left `+34` stale.
            // The bound is memory safety only.
            if t >= self.ent.len() {
                self.ent[i].tick70 = base + 1;
                return false;
            }
            // Target lost (:21658-61). Retail's pair is `+12 < 0 ||
            // (+17 & 4)` — dead OR DESTROY-FLAGGED — and NOTHING
            // else: no class test. A scratch/freed target's verdict
            // comes off its record BYTES, so an all-zeros slot 0
            // reads NOT-lost and the chaser keeps hunting the origin
            // (mc1l32 t=33144: the pack-recruited villager holds
            // state 44 on `+146 = 0` for a whole v_26 window, target
            // yaw = the bearing to (0,0)). mc1l3/l4's scratch read
            // lost through its own 0x400 — a prior collapse's mark,
            // which retail never clears — not through a class test;
            // the port's old `class64 == 0` conjunct was a stand-in
            // for its demolish stamp zeroing the scratch flags
            // (removed with it). The verdict is taken here but
            // applied only AFTER the shared re-bear.
            let c = &self.ent[t];
            let lost = c.act_life < 0 || c.flags & 0x400 != 0;
            (c.x, c.y, c.z, c.f66, c.f67, lost)
        };
        // Re-aim cadence. The shared chase re-bears every 4th tick
        // (:21654 `(+63 & 3) == 0`) BEFORE the target-lost test — the
        // exit tick still aims at the corpse (mc1l0 t=2217: the
        // vulture leaves its dead castle target bearing 107, the
        // bearing to the ruin, not its stale 163). m9, which drives
        // its own chase in retail, tests FIRST and uses a DECIMAL
        // period (sub_1DA60 :24190-97 `+63 % 10`) — a rooted mound
        // therefore swings onto a moving target noticeably more
        // slowly than the shared families do, and never re-aims on
        // its own exit tick.
        let e = &self.ent[i];
        if model != 9 && e.f63 & 3 == 0 {
            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
        }
        if lost {
            self.ent[i].tick70 = base + 1;
            return false;
        }
        let e = &self.ent[i];
        if model == 9 && (e.f63 as i16) % 10 == 0 {
            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
        }
        // m6's buffet drag (:23215-31): the counter +26 cycles 1..41
        // then -90 — 41 ON ticks per 132-tick cycle. Each ON tick
        // re-arms the victim's knock fields (Type_160 v_24 dir /
        // v_22 = 80): a per-tick pull TOWARD the kraken, applied by
        // the human move. These are DIRECT struct writes, not a
        // mailbox — spawn grace does not shield them (the "tractor
        // beam"). v_26 = 256 is written but read by nothing.
        if model == 6 {
            // Retail compares the PRE-increment value (:23219-22), so
            // 41 is still an ON tick before the reset to -90.
            let old = self.ent[i].f26;
            self.ent[i].f26 = old + 1;
            if old > 40 {
                self.ent[i].f26 = -90;
            }
            // ⭐ THE BUFFET DRAGS ANY WIZARD, NOT JUST THE HUMAN (w154j):
            // the three stores go through the TARGET record's `+160`
            // pointer whoever `+146` names (CARPET.EXE 0x1C6EE `mov
            // 0xa0(%edi),%edx` with no class test, then 0x1C6F7 `+24`,
            // 0x1C701 `+26 = 256`, 0x1C719 `+22 = 80`). The bearing is
            // `sub_42150(kraken+72, target+72)` off the RAW target
            // record with `(hi + 4) & 7` — `+0x400`, 11-bit. A rival's
            // wizext lives on `World::rivals`, so the arm posts the
            // bearing on [`Gen::mc1_buffet_post`] and the walk drains
            // it right after this dispatch (retail's phase: the write
            // is visible to every later slot of the same tick). The
            // port used to buffet the human only, leaving a tethered
            // rival's `knock_mag`/`knock_dir` at whatever its last
            // damage letter armed (mc1l14 t=2262..2276 wiz 1: retail
            // 80 flat / 447→458, port 40 / 609).
            if self.ent[i].f26 > 0
                && let Some(hx) = self.hx_of_tag(tgt)
            {
                let (kx, ky) = (self.ent[i].x, self.ent[i].y);
                let (tpx, tpy, _) = self.seat_at(ctx, hx);
                let dir = Self::angle_between(kx, ky, tpx, tpy).wrapping_add(0x400) & 0x7FF;
                self.player_knock[hx] = (dir, 80);
                // Victim buffet cue (:23223) — the kraken tether's
                // distinctive "resonance". Sound 42 has its OWN mixer
                // case (sub_55370 :64625, priority-1, same group as
                // 3/9/40/43); retail PLAYS it. (An earlier note here
                // wrongly claimed it hits the default-drop — corrected,
                // and the mixer policy now admits 42.)
                self.snd(42, i);
            } else if self.ent[i].f26 > 0
                && !crate::mc1::rivals::no_mc1_kraken_buffet_rival()
                && self.wizard_slot_of(tgt).is_some_and(|s| s != 0)
            {
                let (kx, ky) = (self.ent[i].x, self.ent[i].y);
                let dir = Self::angle_between(kx, ky, tx, ty).wrapping_add(0x400) & 0x7FF;
                self.mc1_buffet_post = HashSilent((tgt, dir));
                self.snd(42, i);
            }
        }
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        if (e.f63 as i16) % row.v_26 == 0 {
            let dz = tz.wrapping_sub(e.z) as i32;
            let sq = Self::dist2_sq(e.x, e.y, tx, ty).wrapping_add(dz.wrapping_mul(dz));
            // m9 drives its own chase in retail (sub_1DA60), whose only
            // difference from the shared gate is that a CASTLE target
            // widens the keep-chasing radius by the castle's own extent
            // (:24201-02) — the same `+80 + v_28` the mound's castle
            // hunt acquires on (:23770). Without it the port acquired a
            // castle at a distance its own drop-out test then rejected,
            // so a mound flapped chase/hidden on every v_26 tick.
            let mut v28 = row.v_28 as u32;
            if model == 9 && !self.is_human_tag(tgt) {
                let t = tgt as usize;
                if self.ent[t].class64 == 3 && self.ent[t].model65 == 2 {
                    v28 += self.ent[t].f80 as u32;
                }
            }
            if Self::isqrt(sq as u32) >= v28 {
                self.ent[i].tick70 = base + 1;
                // LABEL_30 (:23239) is a RETURN out of the handler, not
                // a fall-through: the drop-out tick never reaches the
                // spit block below.
                return false;
            } else if model == 6 {
                // Kraken: growl + arm the 5-bolt spit (:23240-42).
                // Sound 37 sits BEHIND the range gate — an out-of-range
                // cadence tick bails silent (goto LABEL_30, no growl).
                self.snd(37, i);
                self.ent[i].f71 = 5;
            } else {
                return self.attack_thunk(i, model, tgt, tx, ty, tz, tf66, tf67);
            }
        }
        // m6's spit burst (:23243-66): while +71 > 0, one lightning
        // beam per tick, filter copied from the target's own fields,
        // beam row [6] (:23259 — inert in flight, no homing).
        // THE ARM AND THE FIRST BOLT SHARE A TICK: retail's burst block
        // sits BELOW the cadence gate that writes +71 = 5 (:23241), so
        // the growl tick immediately spends one charge (5 → 4) and lays
        // a beam. The port used to test +71 first, which cost the burst
        // its opening bolt on every single cadence tick.
        if model == 6 && self.ent[i].f71 > 0 {
            self.ent[i].f71 -= 1;
            let (x, y, z, owner, f84) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24, e.f84)
            };
            if let Some(p) = self.spawn_zigzag(x, y, z) {
                let (owner, tf66, tf67, tgt, lift) =
                    self.mc1_arm_live(p, i, (owner, tf66, tf67, tgt, f84 as i16), false, 1);
                self.arm_projectile(p, owner, tf66, tf67, tgt, tx, ty, tz, 800, 23, lift);
                self.ent[p].row156 = 6;
            }
        }
        false
    }

    /// m2's CHASE wrapper sub_1B3C0 (:22335): the sting cooldown +26
    /// counts down BEFORE the shared chase; the tick it expires the
    /// bee LUNGES at 3x max speed (:22346-47) — the retail
    /// "no-escape" burst. The sting itself (in the melee thunk)
    /// recoils and re-arms the cooldown; leaving the chase state
    /// resets speed to base (:22363-66).
    /// sub_1B3C0's PRE-WORK (:22342-54) — the wrapper's first two acts,
    /// hoisted out of [`Gen::bee_chase`] because retail's damage
    /// prologue does NOT sit above them: the mail read, chain scan,
    /// lethal test and state-abort all live inside the shared core
    /// `sub_1A120` (:21598-21651), which the wrapper only reaches at
    /// :22355. So on the tick a bee is hit or dies, retail has already
    /// run the sting countdown and the z step. The port's centralized
    /// [`Gen::inbox`] returns before the handler and lost both — the
    /// mc1l2 (5,2) z + f26 family, all of them bee DEATH ticks.
    /// Runs on hit and already-dead-on-entry ticks too (:22342-54 is
    /// unconditional), though no row here exercises those arms.
    /// THE DAMAGE-DEAF STATES. The mailbox prologue (:21330-81) opens
    /// *most* live state handlers, not all of them: four open straight
    /// on body work and never touch `str_29885_90` at all, so an entity
    /// in one of them cannot be hurt for the state's whole duration —
    /// `actLife` is never debited, no attacker is latched into
    /// +40/+38/+146, and because the AREA protocol accumulates while
    /// its source stays pending (`mail_write`), the unread amount
    /// SNOWBALLS until a state that *does* carry the prologue finally
    /// reads it.
    ///
    /// - `(5,0)` `sub_1BD10` :22775-78 — a bare promote to wander.
    /// - `(9,0)` `sub_1CFF0` :23591-623 — the materialize countdown.
    /// - `(11,0)` `sub_1DE40` :24317-84 — the blink cycle.
    /// - `(12,0)` `sub_1EA40` :24835-992 — the build attempt.
    ///
    /// Corpus proof is the m9 pair, mc1l32 t=2032/2040 slot 4: a mound
    /// materializing inside the player's fire holds `act_life` 1000
    /// with its mailbox climbing 400 → 800 → 1200 and its source
    /// pinned at the human — nothing ever read it — while the port
    /// debited the whole snowball at once (600, then -200) and
    /// promoted the mound into a chase on the human. `sub_1CFF0` is
    /// byte-identical in the HW twin (remc1hw :22148-81).
    fn state_is_damage_deaf(&self, model: u8, role: u8) -> bool {
        !no_deaf_states() && matches!((model, role), (5, 0) | (9, 0) | (11, 0) | (12, 0))
    }

    fn m2_chase_prework(&mut self, i: usize, ctx: &MobCtx) {
        let v1 = self.ent[i].f26;
        if v1 != 0 {
            self.ent[i].f26 = v1 - 1;
            if v1 == 1 {
                self.ent[i].f126 = 3 * self.ent[i].f128;
            }
        }
        // The vertical aim (:22349-54): a DIRECT z step of |v_14|
        // toward the target's altitude, BEFORE the shared chase — so
        // it lands even on ticks whose damage prologue bails, and the
        // target read has no validity test (f146 == 0 reads the
        // scratch slot; a dead target still steers). The mover's alt
        // clamp then fights it: net +24/tick climbing in-band, a hard
        // ceiling at ground + v_10, and the short bob cycle about the
        // victim's altitude once level. The read takes the OLD +146 —
        // the Hit arm's retarget lands after, as in retail (:22349
        // reads +146 before sub_1A120 rewrites it at :21642).
        let tgt = self.ent[i].f146;
        let tz = if let Some(hx) = self.hx_of_tag(tgt) {
            self.seat_at(ctx, hx).2
        } else {
            self.ent.get(tgt as usize).map_or(0, |e| e.z)
        };
        let d = self.ent[i].z as i32 - tz as i32;
        let sign = i32::from(d > 0) - i32::from(d < 0);
        let v14 = BEHAVIOR[self.ent[i].row156 as usize].v_14 as i32;
        self.ent[i].z = self.ent[i].z.wrapping_add((sign * v14) as i16);
    }

    fn bee_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        // The pre-work already ran in `creature_tick`, above the intake.
        self.mob_chase(i, base, ctx);
        if self.ent[i].tick70 != base + 2 {
            self.chase_exit_trailer(i, 2);
        }
    }

    /// m2's promotion arm (sub_1B350 :22319 / sub_1B370 :22327-31 /
    /// sub_1B4C0 :22374): the tick a non-chase handler promotes the
    /// bee to CHASE, +26 arms to 1 — the next bee_chase tick expires
    /// it into the 3x acquisition lunge. Only the WANDER promotion
    /// buzzes (sound 13); idle and pack arm silently.
    fn m2_lunge_arm(&mut self, i: usize, base: u8, buzz: bool) {
        if self.ent[i].tick70 == base + 2 {
            if buzz {
                self.snd(13, i);
            }
            self.ent[i].f26 = 1;
        }
    }

    /// m7's CHASE sub_1C960 (:23319, twin remc1hw :21876): the
    /// boulder-thrower's DUG-IN cycle, and the family's only speed
    /// bound. Firing plants it — sprite 85 -> 198, speed dropped to
    /// the ACCEL (+130 = 3, a crawl) and a 30-tick timer armed
    /// (:23339-45); the timer expiring un-plants it and restores
    /// +128 (:23327-32); and leaving CHASE in the planted pose
    /// restores it too (:23346-55), which is the RESTORE the port
    /// lacked. Without the third arm an m7 that inherited a pack
    /// catch-up (+126 = leader +126 + leader +130) carried the
    /// inflated speed out of the chase with nothing to re-baseline
    /// it — the mc1l5 corpus scores exactly that, `speed` 23 against
    /// retail's 20, alongside the 20<->3 toggle rows this restores.
    /// Only the ODD-ordinal variant participates: the ctor's parity
    /// arm gives the even one sprite 199 (:45101-13), which matches
    /// neither pose test, so it never plants and never restores.
    /// m7's CHASE HEAD (:23325-32) — the dug-in timer decrement and
    /// the takeoff flip sit ABOVE the shared core's damage prologue
    /// (retail runs them before `sub_1A120`), so a hit tick still
    /// counts the timer down. Hoisted into `creature_tick` with the
    /// m2/m8/m9 preworks: mc1l5 t=23065 slot 55 eats the fire wall's
    /// −400 letter every tick and retail's f26 steps 9→8 where the
    /// port's intake abort froze it — the phase slip behind the whole
    /// (5,7) 20-vs-3 flee-speed family.
    /// m7's WANDER HEAD (sub_1C900 :23305-12): every `v_26`-th tick
    /// (`f63 % v_26 == 0`) a wanderer with nonzero life SNAPS TO
    /// FULL. The decompile's arithmetic is a mangled bool (`v1 =
    /// (max>>6) > max`, always 0) but its effective behavior is the
    /// measured one — mc1l5 t=22881 slot 57: 3200 → 4000 in ONE tick
    /// (a 1/64 regen would read 3262). Sits above the shared core's
    /// damage prologue like the other family preworks — a hit tick
    /// still heals first.
    fn m7_wander_prework(&mut self, i: usize) {
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
        if (self.ent[i].f63 as i16) % v26 == 0 && self.ent[i].act_life != 0 {
            self.ent[i].act_life = self.ent[i].max_life as i32;
        }
    }

    fn m7_chase_prework(&mut self, i: usize) {
        let v1 = self.ent[i].f26;
        if v1 != 0 {
            self.ent[i].f26 = v1 - 1;
            if v1 == 1 && self.ent[i].type86 == 198 {
                self.set_sprite(i, 85);
                self.ent[i].f126 = self.ent[i].f128;
            }
        }
    }

    fn m7_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        if self.mob_chase(i, base, ctx) && self.ent[i].type86 == 85 {
            self.set_sprite(i, 198);
            self.ent[i].f26 = 30;
            self.ent[i].f126 = self.ent[i].f130;
        }
        // The exit trailer runs on the SAME tick the chase breaks —
        // the shared chase has already written the new state.
        if self.ent[i].tick70 != base + 2 {
            self.chase_exit_trailer(i, 7);
        }
    }

    /// m7's promotion arm (sub_1C900 :23315 / sub_1CA00 :23361): the
    /// tick a non-chase handler promotes the thrower, +26 arms to 1 —
    /// the same shape as the bee's, and what lets a chase entered
    /// while still planted un-plant on its first `m7_chase` tick.
    fn m7_arm(&mut self, i: usize, base: u8) {
        if self.ent[i].tick70 == base + 2 {
            self.ent[i].f26 = 1;
        }
    }

    /// m8's CHASE PRE-WORK (sub_1CE30 :23550-52): restore full speed
    /// while the cooldown runs, then re-set the DEFLECTION bit. Both
    /// statements sit ABOVE the `sub_1A120` call (:23553), i.e. above
    /// the shared damage prologue, so a griffon that takes a hit on a
    /// chase tick STILL ratchets its speed back to +128 and STILL
    /// re-raises 0x8000 — the abort happens inside the core, below
    /// them. Hoisted into `creature_tick` for exactly that reason
    /// (the m2 pre-work's twin); mc1l42 t=20162 is the proof: seven
    /// griffons in state 50 eat the crater's 240 and retail shows
    /// every one at flags 0x800C / +126 40 while the port's blanket
    /// hit-abort left them 0x000C / 60.
    fn griffon_chase_prework(&mut self, i: usize) {
        if self.ent[i].f26 != 0 {
            self.ent[i].f126 = self.ent[i].f128;
        }
        self.ent[i].flags |= 0x8000;
    }

    /// m8's CHASE sub_1CE30 (:23546): the pre-work above (already run
    /// in `creature_tick`, above the intake) — the speed restore and
    /// the DEFLECTION bit re-set EVERY tick (:23552 — the only
    /// creature that raises it, and only the rival Rebound token ever
    /// clears it: fireballs/meteors bounce off an attacking griffon
    /// for good; beams — lightning — never full-deflect, which is why
    /// lightning stays the counter), then the shared chase with the
    /// 4000-damage beam thunk, plus the screech throttle (sound 38)
    /// every v_26 (:23563-65). The provoking hit lands BEFORE the
    /// first chase tick sets the bit (the first meteor connects).
    fn griffon_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.mob_chase(i, base, ctx);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
        if (self.ent[i].f63 as i16) % v26 == 0 {
            self.snd(38, i);
        }
    }

    /// m16's CHASE sub_207E0 (:26062) — the wyvern's own handler, NOT
    /// the shared sub_1A120. Bearing every 8th tick, and only when
    /// the target is a wizard or beyond 0x200 3D (:26146-49 — over a
    /// house it stops re-aiming instead of orbiting); target
    /// dead/expired → back to the hunt (:26152); while the burst
    /// counter +26 runs, one strongly homing 3000-damage fireball PER
    /// TICK from 4x launch height with the wyvern's own +66/+67
    /// filter (:26154-77); every v_26 a SQUARED 2D range drop-out
    /// (unlike the shared chase's un-squared 3D test), the roar at
    /// 2*v_26 (sound 39) and the 0xE3-cone burst re-arm to 15
    /// (:26178-90).
    fn wyvern_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.creature_move(i);
        let tgt = self.ent[i].f146;
        let (tx, ty, tz, tclass, tdead) = if let Some(hx) = self.hx_of_tag(tgt) {
            // :26152-53 dereferences pool[+146] whoever it names —
            // the human's carpet included; the dead test is the same
            // +12<0 || +17&4 (mc1l49 t=3059: both wyverns exit chase
            // on the death tick where the port kept firing).
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                3u8,
                self.seat_dead(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            // ⭐ A `+146` OF 0 IS THE SCRATCH RECORD HERE TOO — the
            // THIRD copy of the law [`Gen::mob_chase`] and
            // [`Gen::militia_chase_body`] already carry. `sub_207E0`
            // forms `v11 = &pool[+146]` at :26144 with NO validity
            // test of any kind, re-bears off it at :26146-49 and only
            // then runs the lost test `+12 < 0 || (+17 & 4)` at
            // :26152. Retail DOES read `+64` in this handler, but as
            // `*(v11 + 64) == 3` INSIDE the re-aim gate (the "target
            // is a wizard" arm), never as a target-lost conjunct — so
            // the port's `class64 == 0` here was a paraphrase of the
            // wrong test, and the `t == 0` arm was invented outright.
            // A wyvern handed 0 keeps hunting the ORIGIN: it moves,
            // re-bears to (0,0) — slot 0's class is not 3 and the
            // origin is far, so the gate opens — and stays in chase
            // for as long as the scratch record reads alive.
            if t >= self.ent.len() || (t == 0 && no_m15m16_scratch_target()) {
                self.ent[i].tick70 = base + 1;
                return;
            }
            let c = &self.ent[t];
            let dead = (no_m15m16_scratch_target() && c.class64 == 0)
                || c.act_life < 0
                || c.flags & 0x400 != 0;
            (c.x, c.y, c.z, c.class64, dead)
        };
        if self.ent[i].f63 & 7 == 0 {
            let e = &self.ent[i];
            let dz = tz.wrapping_sub(e.z) as i32;
            let d = Self::isqrt(
                Self::dist2_sq(e.x, e.y, tx, ty).wrapping_add(dz.wrapping_mul(dz)) as u32,
            );
            if tclass == 3 || d >= 0x200 {
                let e = &self.ent[i];
                self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
            }
        }
        if tdead {
            self.ent[i].tick70 = base + 1;
            return;
        }
        if self.ent[i].f26 > 0 {
            self.ent[i].f26 -= 1;
            let (x, y, z, owner, f84, f66, f67) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24, e.f84, e.f66, e.f67)
            };
            if let Some(p) = self.spawn_fireball(x, y, z) {
                self.ent[p].row156 = 2; // unk_98F38[2], turn 0x71
                self.ent[p].f140 = 60000;
                let (owner, f66, f67, tgt, lift) =
                    self.mc1_arm_live(p, i, (owner, f66, f67, tgt, 4 * f84 as i16), true, 4);
                self.arm_projectile(p, owner, f66, f67, tgt, tx, ty, tz, 3000, 0, lift);
            }
        }
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (v26, v28) = (row.v_26.max(1), row.v_28 as i32);
        if (self.ent[i].f63 as i16) % v26 == 0 {
            let e = &self.ent[i];
            if Self::dist2_sq(e.x, e.y, tx, ty) >= v28 * v28 {
                self.ent[i].tick70 = base + 1;
                return;
            }
            if (self.ent[i].f63 as i16) % (2 * v26) == 0 {
                self.snd(39, i);
            }
            let e = &self.ent[i];
            let bearing = Self::angle_between(e.x, e.y, tx, ty);
            if Self::angdist(e.f30, bearing) < 0xE3 {
                self.ent[i].f26 = 15;
            }
        }
    }

    /// sub_20710's custom layer over the shared wander (:26033-58):
    /// every v_26+1 ticks (offset from the scan cadence) the nearest
    /// HOUSE on the +36470 roster ([`Gen::bldg_chain`]) within v_28²
    /// becomes the chase target — pure 2D nearest-in-radius, NO
    /// facing cone, NO invisibility gate, and no life or 0x400 test
    /// either: the walk at :26041-48 carries NOTHING but the distance
    /// pair, so class/model/liveness come entirely from ROSTER
    /// MEMBERSHIP. Wyverns wreck dwellings on sight — but only the
    /// ones that already existed when the tick began.
    fn wyvern_house_hunt(&mut self, i: usize, base: u8) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let period = row.v_26 + 1;
        if (self.ent[i].f63 as i16) % period != 0 {
            return;
        }
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.bldg_chain.visible_len() {
            let j = self.bldg_chain.list[k] as usize;
            let c = &self.ent[j];
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if d2 <= r2 && best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        if let Some((j, _)) = best {
            self.ent[i].f146 = j as u16;
            self.ent[i].tick70 = base + 2;
        }
    }

    // ---- model 11, the genie (states 66-71, :24317-24770) ------------------

    /// m11 IDLE sub_1DE40 (:24317) — the blink cycle. While +26 runs
    /// it counts down; on expiry (sound 21) the phase bit picks the
    /// exit: SET → drop the target AND its pending ch0 mail and
    /// TELEPORT by a per-axis LCG offset ((rand % 0x3C) << 8) + 12800
    /// (toroidal map) into WANDER; CLEAR → straight into CHASE with
    /// the target intact. At +26 == 0 it lays the 12-puff (10,1)
    /// sparkle ring on a 3x4 grid of 40-unit cells, re-arms +26 = 1
    /// and toggles the phase — ring, then blink, alternating.
    ///
    /// **THE PHASE BIT IS +16 byte[0] BIT 0** (`& 1` at :24336, `^ 1`
    /// at :24383), not the port-local 0x2000 that used to stand in for
    /// it. The bit is RECORDED, so the stand-in cost the port the
    /// whole cycle under import: retail's genie carries the phase in
    /// bit 0, an imported record never has 0x2000, and the port's
    /// genie therefore read CLEAR forever — it took the chase exit on
    /// every expiry and NEVER blinked. mc1l42 slot 101 shows it as a
    /// matched pair: flags want 13 got 8204 on the toggle tick, then
    /// x/y/rand on the next (retail teleports whole tiles away, the
    /// port stands still). Bit 0 is ALSO retail's billboard hide (the
    /// `& 0x21` walk, :36830), so the phased-out genie is genuinely
    /// unseen: retail's state 67 carries it on every recorded row
    /// (`hide_bit_census`), and `World::live_poses` exports it map-only.
    fn genie_idle(&mut self, i: usize, base: u8) {
        let v1 = self.ent[i].f26;
        if v1 != 0 {
            self.ent[i].f26 = v1 - 1;
            if v1 == 1 {
                self.snd(21, i);
                if self.ent[i].flags & 1 != 0 {
                    self.ent[i].f146 = 0;
                    // :24339 — the blink also drops the pending ch0
                    // damage SOURCE, so a hit landed this tick never
                    // reaches the chase-state mail read.
                    self.ent[i].mail[0].1 = 0;
                    let d1 = self.ent_rand(i);
                    let d2 = self.ent_rand(i);
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x, e.y, e.z)
                    };
                    let nx = x.wrapping_add((((d1 % 0x3C) << 8) + 12800) as u16);
                    let ny = y.wrapping_add((((d2 % 0x3C) << 8) + 12800) as u16);
                    self.move_relink(i, nx, ny, z);
                    self.ent[i].tick70 = base + 1;
                } else {
                    self.ent[i].tick70 = base + 2;
                }
            }
        } else {
            // The sparkle ring (:24361-84); each puff carries the
            // genie as owner (+24) and the original's +18 bit 0.
            let (x, y, z, id) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24)
            };
            for k in (0..12u16).rev() {
                let px = x.wrapping_add(40 * (k % 3));
                let py = y.wrapping_add(40 * (k / 3));
                if let Some(p) = self.spawn_effect(1, px, py, z) {
                    self.ent[p].id24 = id;
                    self.ent[p].flags |= 0x10000; // +18 |= 1 (:24377-78)
                }
            }
            self.ent[i].f26 = 1;
            self.ent[i].flags ^= 1; // :24383 — byte[0] ^= 1
        }
    }

    /// sub_1E770 (:24733): the AMBUSH BLINK — with a target held,
    /// zero the blink timer, drop to IDLE (whose ring/blink cycle
    /// alternates back into chase) and TELEPORT to the point one
    /// actSpeed<<6 step (60<<6 = 15 tiles) AHEAD of the target along
    /// the TARGET's own heading, at the target's altitude.
    fn genie_ambush(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        let tgt = self.ent[i].f146;
        if tgt == 0 {
            return;
        }
        let (tx, ty, tz, tyaw) = if let Some(hx) = self.hx_of_tag(tgt) {
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                self.seat_yaw(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            if t >= self.ent.len() {
                return;
            }
            let c = &self.ent[t];
            (c.x, c.y, c.z, c.f30)
        };
        self.ent[i].f26 = 0;
        self.ent[i].tick70 = base;
        let mut pos = (tx, ty, tz);
        let step = ((self.ent[i].f126 as i32) << 6).clamp(i16::MIN as i32, i16::MAX as i32);
        Self::polar_step(&mut pos, tyaw, 0, step as i16);
        self.move_relink(i, pos.0, pos.1, pos.2);
    }

    /// sub_1E720 (:24724): blink home — clear the target and timer,
    /// back to IDLE, sound 11.
    fn genie_home(&mut self, i: usize, base: u8) {
        self.ent[i].f146 = 0;
        self.ent[i].f26 = 0;
        self.ent[i].tick70 = base;
        self.snd(11, i);
    }

    /// sub_1E810 (:24751): eat a loose mana ball — while below max
    /// mana, the nearest class-10 m39 ball within v_28² is absorbed
    /// (+140 += ball's, ball unclaimed + destroyed) with a (10,0)
    /// explosion puff at the spot and sound 11. The other half of
    /// "genies steal mana": they drain the map economy too.
    ///
    /// The candidates are the TICK-TOP BALL CHAIN's, not the pool's
    /// ([`crate::engine::features::no_mc1_genie_eat_chain`]): one ball
    /// feeds every genie that reaches it in the tick.
    fn genie_eat_ball(&mut self, i: usize) {
        if self.ent[i].f140 >= self.ent[i].f136 {
            return;
        }
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let mut best: Option<(usize, u32)> = None;
        if crate::engine::features::no_mc1_genie_eat_chain() {
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 10 || c.model65 != 39 || c.flags & 0x400 != 0 {
                    continue;
                }
                let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
                if d2 <= r2 && best.is_none_or(|(_, bd)| d2 < bd) {
                    best = Some((j, d2));
                }
            }
        } else {
            // The walk is the TICK-TOP BALL CHAIN (+36466), per-node
            // gate `+65 == 39` ALONE (HIDDEN.EXE file 0x37249 `cmpb
            // $0x27,0x41(%eax)`): no 0x400, no class test. A ball a
            // genie ate earlier THIS tick is still a member, so the
            // next genie in reach eats it again — the full +140 lands
            // a second time with its own (10,0) puff.
            for k in 0..self.ball_chain.visible_len() {
                let j = self.ball_chain.list[k] as usize;
                let c = &self.ent[j];
                if c.model65 != 39 {
                    continue;
                }
                let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
                if d2 <= r2 && best.is_none_or(|(_, bd)| d2 < bd) {
                    best = Some((j, d2));
                }
            }
        }
        if let Some((t, _)) = best {
            self.ent[i].f140 += self.ent[t].f140;
            self.ent[t].f144 = 0;
            self.ent[t].flags |= 0x400;
            let (bx, by, bz) = (self.ent[t].x, self.ent[t].y, self.ent[t].z);
            let id = self.ent[i].id24;
            if let Some(p) = self.spawn_effect(0, bx, by, bz) {
                self.ent[p].id24 = id;
                self.ent[p].flags |= 0x10000; // +18 |= 1 (:24793-94)
            }
            self.snd(11, i);
        }
    }

    /// m11 WANDER sub_1DFE0 (:24388) — a full active handler, not a
    /// caster-phase nop: move, then every v_26 the SELF-HEAL
    /// (+maxLife>>6, clamped), the awake- and quarter-life-gated
    /// wizard scan (range v_28 + cone v_30 + invisibility, model +65<=1
    /// — human + rival CARPETS, not castles/balloons; the doc's old
    /// "owner ≤ 1" misread the +65 byte) → AMBUSH BLINK, else eat a
    /// mana ball; the standard two-draw yaw jitter; and above 3/4 life
    /// the MANA HUNT (:24523-46) — the first wizard holding ANY mana, no
    /// range or cone gate, → ambush. (Hit-retaliation is the inbox's.)
    fn genie_wander(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.creature_move(i);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
        if (self.ent[i].f63 as i16) % v26 != 0 {
            return;
        }
        {
            let e = &mut self.ent[i];
            e.act_life += (e.max_life >> 6) as i32;
            if e.act_life < -1 {
                e.act_life = -1;
            }
            if e.act_life > e.max_life as i32 {
                e.act_life = e.max_life as i32;
            }
        }
        // Aggro scan (:24485): the nearest wizard BODY — human or rival
        // carpet (the +65<=1 gate, so no castles/balloons) — within
        // range+cone → ambush-blink; nothing in reach → eat a mana ball.
        if self.ent[i].f58 != 0 && self.ent[i].act_life > (self.ent[i].max_life >> 2) as i32 {
            if let Some(t) = self.nearest_wizard_target(i, ctx, true, false) {
                self.ent[i].f146 = t;
                self.genie_ambush(i, base, ctx);
            } else {
                self.genie_eat_ball(i);
            }
        }
        let d1 = self.ent_rand(i);
        let d2 = self.ent_rand(i);
        let mag = ((d2 & 0xFF) + 85) as i32;
        let sign = if d1 % 157 >= 79 { 1 } else { -1 };
        self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
        // Mana hunt (:24523): above 3/4 life, the FIRST wizard body
        // (human or rival carpet) holding any mana — no range or cone
        // gate — becomes the ambush target.
        if self.ent[i].act_life > (self.ent[i].max_life - (self.ent[i].max_life >> 2)) as i32 {
            if let Some(t) = self.first_wizard_with_mana(i, ctx) {
                self.ent[i].f146 = t;
                self.genie_ambush(i, base, ctx);
            }
        }
    }

    /// The genie mana hunt's target pick (:24527): the FIRST wizard body
    /// (human or rival CARPET, the +65<=1 gate) that is holding mana
    /// (+140 != 0) and not cloaked/hidden (0x20 clear). No range or cone
    /// — a genie above 3/4 life blinks straight onto the nearest mana it
    /// can smell. The out-of-pool human (usually bucket[0]'s lowest
    /// index) is tested first via `ctx`; rivals follow in pool order.
    fn first_wizard_with_mana(&self, i: usize, ctx: &MobCtx) -> Option<u16> {
        // Retail's loop walks the tick-top class-3 chain (:24527),
        // built with actLife >= 0 (:52254) — MEMBERSHIP is the
        // liveness filter, and a dead human is off the chain
        // (mc1hwl0 t=23503/23504: both wandering genies elect the
        // corpse in the port, retail can't see it). Roster-membership
        // phase: the tick-top pdead, not the mid-walk republish.
        for hx in self.human_tag.ixs() {
            if !self.player_cloaked(hx)
                && self.human_tag[hx].0 != self.ent[i].id24
                && self.seat_purse(ctx, hx) != 0
                && !self.seat_dead_top(hx)
            {
                return Some(self.human_tag[hx].0);
            }
        }
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 == 3
                && c.model65 == 1
                && c.act_life >= 0
                && c.flags & 0x20 == 0
                && c.f140 != 0
            {
                return Some(j as u16);
            }
        }
        None
    }

    /// m11 CHASE sub_1E380 (:24554): move; at or above half life the
    /// bearing update every 8th tick (target dead/expired → eat a
    /// ball + blink home); below half life the BREAK-OFF blink home.
    /// Every v_26: 3D range ≥ v_28 → blink home; the chatter (sound
    /// 11) every 8*v_26; else the 3000-payload steal seeker (the
    /// attack thunk; the +26 counter the original bumps per window is
    /// vestigial — both decompiled branches spawn identically).
    ///
    /// THE BLINK HOME IS NOT A RETURN. `sub_1E720` (:24724) is a plain
    /// call in both the dead-target arm (:24643-44) and the break-off
    /// arm (:24649); only the RANGE drop-out returns (:24658). So a
    /// genie that breaks off still falls into the every-v_26 block and
    /// fires ONE PARTING SEEKER, and because the target pointer `v9`
    /// was taken once at :24633 — before `sub_1E720` zeroed +146 —
    /// that seeker is aimed at the target it just abandoned. The
    /// signature is +26: `sub_1E720` writes 0 and the `+26++` at
    /// :24666 leaves 1. mc1l42 t=2133 and t=14481, slot 101: retail
    /// hands back +26 = 1 with a (9,8) seeker chasing 331; the port
    /// returned early and left +26 = 0 with no shot. (This RETIRES the
    /// "we return" deviation that used to sit on this comment — it was
    /// a ruling on a GRADED lane.)
    fn genie_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.creature_move(i);
        let tgt = self.ent[i].f146;
        // `v9` (:24633) is resolved ONCE, ahead of every branch, and
        // every later read — the re-bear, the range drop-out, the
        // seeker's launch bearing — goes through it. A +146 of 0 lands
        // on the scratch record and a freed slot keeps its stale
        // coordinates; retail tests only `+12 >= 0 && !(+17&4) &&
        // +64` (:24636) and treats a failure as a dead target, not as
        // a reason to leave the handler.
        let (tx, ty, tz, tdead) = if let Some(hx) = self.hx_of_tag(tgt) {
            // The human's carpet IS a pool record at :24636 — the
            // dead test applies to it too (mc1hwl0 t=23461 slot 697:
            // retail eats a ball + blinks home on the death tick,
            // mana 10000→10500, f146/f26 → 0).
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                self.seat_dead(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            if t >= self.ent.len() {
                self.genie_home(i, base);
                return; // port guard: retail would read raw memory
            }
            let c = &self.ent[t];
            (
                c.x,
                c.y,
                c.z,
                c.act_life < 0 || c.flags & 0x400 != 0 || c.class64 == 0,
            )
        };
        if self.ent[i].act_life >= (self.ent[i].max_life >> 1) as i32 {
            if !tdead {
                if self.ent[i].f63 & 7 == 0 {
                    let e = &self.ent[i];
                    self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
                }
            } else {
                self.genie_eat_ball(i);
                self.genie_home(i, base);
            }
        } else {
            self.genie_home(i, base);
        }
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26.max(1);
        if (self.ent[i].f63 as i16) % v26 == 0 {
            let e = &self.ent[i];
            let dz = tz.wrapping_sub(e.z) as i32;
            let sq = Self::dist2_sq(e.x, e.y, tx, ty).wrapping_add(dz.wrapping_mul(dz));
            if Self::isqrt(sq as u32) >= BEHAVIOR[self.ent[i].row156 as usize].v_28 as u32 {
                self.genie_home(i, base);
                return;
            }
            if (self.ent[i].f63 as i16) % (8 * v26) == 0 {
                self.snd(11, i);
            }
            self.ent[i].f26 += 1;
            // ⭐ THE PARTING SHOT IS BORN UNTARGETED. :24697 stamps the
            // seeker's +146 from the caster's LIVE +146 — which the
            // break-off arms above have just had `sub_1E720` zero
            // (:24724) — while the launch BEARING still rides the
            // stale `v9` resolved at :24633. So a genie that breaks
            // off looses a seeker that carries no target and must
            // acquire in its own first `sub_530C0` tick (the
            // acquire-or-track fork, :63071-84), which is what leaves
            // retail's newborn reading flags 6 (bit 1 latched), +26 16
            // (sub_54520's entry clamp) and +30 == +34. Passing the
            // stale `tgt` here handed it a target it never had and
            // suppressed the acquire entirely — mc1l42 t=2133 slot 378
            // and t=14481 slot 991, and the free replay's t=2134 wall.
            let live_target = self.ent[i].f146;
            self.attack_thunk(i, 11, live_target, tx, ty, tz, 0, 0);
        }
    }

    /// m5's regen tail (sub_1BF60/sub_1C110 :22959-65, :22976-82):
    /// life += maxlife>>7 per tick while below max.
    fn m5_regen(&mut self, i: usize) {
        let e = &mut self.ent[i];
        if e.act_life < e.max_life as i32 {
            e.act_life += (e.max_life >> 7) as i32;
        }
    }

    /// sub_38820_38BA0 (:44943): the crab GROWS — size = clamp(mana /
    /// (maxmana/8), 0, 7) picks sprite 185+size (extents follow the
    /// new sprite's stats); a size-up adds 5000 max life (unrefilled).
    fn m5_grow(&mut self, i: usize) {
        let e = &self.ent[i];
        let step = (e.f136 >> 3).max(1);
        let size = (e.f140 / step).clamp(0, 7) as i16;
        if size > e.type86 as i16 - 185 {
            self.ent[i].max_life += 5000;
        }
        self.set_sprite(i, (185 + size) as u16);
    }

    /// m5 WANDER sub_1BF60 (:22775): move (NO yaw-jitter draws — the
    /// crab's wander is a custom handler); every v_26: wizard scan →
    /// CHASE, else steer toward / close on the targeted mana ball
    /// (within maxSpeed<<7 → EAT state, +26 = 15), else acquire the
    /// nearest ball and lay an egg when 500 over max mana.
    fn m5_wander(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.creature_move(i);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 == 0 {
            if let Some(t) = self.nearest_wizard_target(i, ctx, false, false) {
                self.ent[i].f146 = t;
                self.ent[i].tick70 = base + 2;
            } else if self.ent[i].f146 != 0 {
                let t = self.ent[i].f146 as usize;
                // :22907-08 — the steer arm re-tests +64/+65 ALONE: no
                // 0x400 conjunct, a soft-killed ball stays a target
                // until the reap wipes its class.
                let is_ball =
                    t < self.ent.len() && self.ent[t].class64 == 10 && self.ent[t].model65 == 39;
                if is_ball {
                    let e = &self.ent[i];
                    let (bx, by) = (self.ent[t].x, self.ent[t].y);
                    let d = Self::isqrt(Self::dist2_sq(e.x, e.y, bx, by) as u32);
                    if d > (e.f128 as u32) << 7 {
                        self.ent[i].f34 = Self::angle_between(e.x, e.y, bx, by);
                    } else {
                        self.ent[i].f26 = 15;
                        self.ent[i].tick70 = base + 3; // EAT (state 0x21)
                    }
                } else {
                    self.ent[i].f146 = 0;
                }
            } else {
                // Nearest loose ball, any range (:22928-45) — the
                // walk is the TICK-TOP BALL CHAIN (+36466, bucket[1]),
                // per-node gate `+65 == 39` ALONE: no 0x400, no life
                // test — a ball eaten mid-tick still scores, a ball
                // born mid-tick is invisible (the chain-vs-pool bug in
                // its fourth costume).
                let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                let mut best: Option<(usize, u32)> = None;
                let n = self.ball_chain.visible_len();
                for k in 0..n {
                    let j = self.ball_chain.list[k] as usize;
                    let c = &self.ent[j];
                    if c.model65 != 39 {
                        continue;
                    }
                    let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
                    if best.is_none_or(|(_, bd)| d2 < bd) {
                        best = Some((j, d2));
                    }
                }
                if let Some((j, _)) = best {
                    self.ent[i].f146 = j as u16;
                }

                // Egg-laying (:22945-55): 500 over max mana buys a
                // class-10 m52 egg (1 own-LCG draw). The creator's
                // f26=600 is overwritten here with the real
                // 100..190-tick hatch timer; the egg then incubates
                // (`tick_egg_incubate`) and hatches a wild m5 crab
                // (`tick_egg_hatch`).
                if self.ent[i].f136 + 500 < self.ent[i].f140 {
                    let (x, y, z) = {
                        let e = &self.ent[i];
                        (e.x, e.y, e.z)
                    };
                    if let Some(egg) = self.spawn_creator(52, x, y, z) {
                        let d = self.ent_rand(i);
                        self.ent[egg].f26 = (10 * (d % 10) + 100) as i16;
                        self.ent[i].f140 -= 500;
                    }
                }
            }
        }
        self.m5_regen(i);
    }

    /// m5 EAT sub_1C170 (:22986): close on the ball at the +26 think
    /// period (15, dropping to 3 inside 20·maxSpeed); within
    /// 5·maxSpeed: absorb its mana, destroy it, GROW, back to wander.
    fn m5_eat(&mut self, i: usize, base: u8) {
        self.creature_move(i);
        let period = self.ent[i].f26.max(1);
        if (self.ent[i].f63 as i16) % period != 0 {
            return;
        }
        let t = self.ent[i].f146 as usize;
        // :23063 — class/model ALONE again (no 0x400; slot-0 scratch
        // fails the class test, covering the f146==0 case).
        let is_ball =
            t != 0 && t < self.ent.len() && self.ent[t].class64 == 10 && self.ent[t].model65 == 39;
        if !is_ball {
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = base + 1;
            return;
        }
        let e = &self.ent[i];
        let (bx, by) = (self.ent[t].x, self.ent[t].y);
        // :23068 sub_423D0 — the eat range is 2-D (x/y alone; the
        // port's added z term made a ball under the crab read too far
        // — mc1l32 t=22114: retail absorbs, the port kept chasing and
        // sat one 5000-step of growth behind for the rest of the era).
        let d2 = Self::dist2_sq(e.x, e.y, bx, by);
        let dist = Self::isqrt(d2 as u32);
        let max = self.ent[i].f128 as u32;
        if dist > 5 * max {
            if dist <= 20 * max {
                self.ent[i].f26 = 3;
            }
            let e = &self.ent[i];
            self.ent[i].f34 = Self::angle_between(e.x, e.y, bx, by);
        } else {
            self.ent[i].f146 = 0;
            self.ent[i].f140 += self.ent[t].f140;
            self.ent[t].f144 = 0;
            self.ent[t].flags |= 0x400;
            self.ent[i].tick70 = base + 1;
            self.m5_grow(i);
        }
    }

    /// sub_1BC50 (:22744): the militiaman shoulders his dart — ONE LCG
    /// draw (sprite 206 on 11/20 else 1), STOPS (speed 0) and takes the
    /// target's own class/model as his projectile filter. Retail runs
    /// this from the three non-chase handlers on the PROMOTION tick
    /// (idle :22432, wander :22690, pack :22725), not from the chase —
    /// the mc1l5 corpus scores the one-tick lag the port's in-chase arm
    /// produced on `speed`, `sclass`, `smodel` and `rand` alike.
    fn militia_arm(&mut self, i: usize) {
        let tgt = self.ent[i].f146;
        // The human is out of pool here, so his class/model pair is
        // named directly (the wizard-body class 3, model 0) exactly as
        // the chase read it before — retail keeps his POOL SLOT in
        // `+146` and reads (3, 0) straight off it.
        //
        // :22759-61 has no index test of any kind: `+146` of 0 reads
        // the SCRATCH RECORD at pool slot 0 and stamps its class/model,
        // which is (0, 0) while the slot is unstaged. The `t != 0` arm
        // was invented, and it is what mc1hwl1 t=3684 and mc1l37
        // t=2742..2878 score as `sclass: retail 0 port 3`.
        let (tc, tm) = match tgt as usize {
            _ if self.is_human_tag(tgt) => (3u8, 0u8),
            t if t < self.ent.len() && !(t == 0 && no_m4m9_arm_scratch_target()) => {
                (self.ent[t].class64, self.ent[t].model65)
            }
            _ => (3u8, 0u8),
        };
        let d = self.ent_rand(i);
        self.ent[i].f126 = 0;
        self.set_sprite(i, if d % 20 <= 10 { 206 } else { 1 });
        self.ent[i].f66 = tc;
        self.ent[i].f67 = tm;
    }

    /// sub_1BCE0 (:22766): leaving the chase puts the dart away —
    /// the WALK SPEED restored (+126 = +128), the unarmed sprite and
    /// the hit-anything filter. This is the militia's only speed
    /// restore anywhere in the engine; without it a militiaman who
    /// had chased once stayed pinned at speed 0 for the rest of the
    /// level and never wandered again.
    fn militia_disarm(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.set_sprite(i, 0);
        self.ent[i].f66 = 3;
        self.ent[i].f67 = 0xFF;
    }

    /// m4 CHASE (sub_1BB20 :22690): the militiaman stands his ground
    /// and shoots. Every v_26 in range fires the sub_1A990 dart and
    /// refreshes the wizard's wanted timer (:22714). Break state is
    /// base+1 (25) — the shared chase's own `a2 + 1` (:21657/:21661) —
    /// and ANY break runs the disarm trailer on the same tick.
    fn militia_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.militia_chase_body(i, base, ctx);
        // sub_1BB20's trailer (:22699-702)...
        if self.ent[i].tick70 != base + 2 {
            self.chase_exit_trailer(i, 4);
        }
        // ...and its wanted-timer tail (:22703-14), textually below the
        // core exactly like the m0 bob and the m5 regen.
        self.militia_chase_wanted_tail(i, base);
    }

    fn militia_chase_body(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        let tgt = self.ent[i].f146;
        // ⭐ THE LOST TEST IS A VERDICT, NOT AN EXIT — it is taken here
        // and applied only BELOW THE RE-BEAR, because `sub_1BB20` has
        // no body of its own: it is `sub_1A120(a1x, 24, sub_1A990)`
        // (:22696), and the shared chase reads the target's axis
        // unguarded, moves, re-bears every 4th tick and only THEN
        // tests `+12 < 0 || (+17 & 4)` (:21654-61). So a militiaman
        // whose target dies still turns onto the corpse on his exit
        // tick — the same law [`Gen::mob_chase`] already carries, and
        // the one place in the family that had kept the early return.
        // mc1l2 t=8283: the rival wizard slot 300 is mid-death-fall at
        // `act_life -280`, retail's militia posts `+34 = 691` on the
        // way out and ours held its stale 693 for another tick.
        let (tx, ty, tz, lost) = if let Some(hx) = self.hx_of_tag(tgt) {
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                self.seat_dead(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            // ⭐ A `+146` OF 0 IS THE SCRATCH RECORD HERE TOO — the
            // law [`Gen::mob_chase`] already carries, landed on ONE
            // call path. m4's chase IS the shared core (`sub_1BB20`
            // :22698 = `sub_1A120(a1x, 24, sub_1A990)`), and the core
            // forms `v12 = &pool[+146]` with NO validity test at all
            // (:21655, hw :20211): it MOVES first (:21654), re-bears
            // off `v12` (:21657) and only then tests `+12 < 0 ||
            // (+17 & 4)` (:21658) — no index test, no class test. So a
            // militiaman handed `+146 = 0` (the pack-death handoff
            // :21746 passes the dier's `+40`, which is 0 whenever the
            // fatal change did not arrive through its own +94 mailbox)
            // keeps hunting the ORIGIN: he walks, and his target yaw
            // becomes the bearing to (0,0). The port's `t == 0` early
            // return and `class64 == 0` conjunct froze him instead and
            // dropped him back to 25 — mc1hwl1 t=346 slot 33 and
            // mc1l37 t=2735 slot 847 are the same tick shape in two
            // games. The remaining bound is memory safety only.
            if t >= self.ent.len() || (t == 0 && no_m4_scratch_target()) {
                self.ent[i].tick70 = base + 1;
                return;
            }
            let c = &self.ent[t];
            let lost = (no_m4_scratch_target() && c.class64 == 0)
                || c.act_life < 0
                || c.flags & 0x400 != 0;
            (c.x, c.y, c.z, lost)
        };
        // Retail runs the movement core (sub_196E0 :21654, via
        // sub_1A120) every alive tick — at chase speed 0 it only
        // altitude-clamps, settling a militiaman a collapse spawned
        // above ground so the 3D range gate below can reach the target
        // instead of failing on the stale spawn height.
        self.creature_move(i);
        let e = &self.ent[i];
        if e.f63 & 3 == 0 {
            // The shared chase's re-bear writes the TARGET heading
            // only (:21654) — the movement core's capped turn walks
            // +30 toward it, ±v_2 a tick even at chase speed 0
            // (mc1l0 t=5052: retail steps 1392→1414 toward the STALE
            // 1710 on the very tick the re-bear posts 1027; snapping
            // +30 = +34 here jumped the whole 365).
            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
        }
        if lost {
            self.ent[i].tick70 = base + 1;
            return;
        }
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        if (e.f63 as i16) % row.v_26 == 0 {
            let dz = tz.wrapping_sub(e.z) as i32;
            let sq = Self::dist2_sq(e.x, e.y, tx, ty).wrapping_add(dz.wrapping_mul(dz));
            if Self::isqrt(sq as u32) >= row.v_28 as u32 {
                self.ent[i].tick70 = base + 1;
            } else {
                self.attack_thunk(i, 4, tgt, tx, ty, tz, 0, 0);
            }
        }
    }

    /// `sub_1BB20`'s own tail (:22703-14), the militia CHASE wrapper's
    /// wanted-timer cadence: every `v_26` ticks a chasing militiaman
    /// re-stamps his target's `+528` to 200, outside the range gate and
    /// only for a carpet-borne target (`model <= 1`; the out-of-pool
    /// player IS the carpet).
    ///
    /// ⭐ IT IS A WRAPPER TRAILER, SO THE DAMAGE FREEZE DOES NOT REACH
    /// IT. `sub_1BB20` is `sub_1A120(a1x, 24, sub_1A990)` and *then*
    /// this tail (:22696-714): every early exit the shared core takes
    /// out of its damage prologue — the class-3 retarget
    /// `+146 = +40; return 0` (:21641-44) and the plain `return v15`
    /// a non-wizard hit falls to (:21649) — is a return INTO the
    /// wrapper, which then runs this. Only a LETHAL hit escapes it,
    /// because that arm moves `+70` to `a2 + 4` and the wrapper's own
    /// `if (+70 != 26)` (:22699) diverts to `sub_1BCE0` first. This is
    /// the same law the m0 bob, the m1 idle trailer and the m5 regen
    /// already carry in [`Gen::creature_tick`]'s `Inbox::Hit` arm; the
    /// m4 chase's `+528` cadence was the one wrapper tail left out of
    /// it.
    ///
    /// mc1l37 t=9270 is the witness: militiaman 867 is in chase 26 on
    /// the human with `+63 = 180` (row 16, `v_26 = 30`) when the human's
    /// 240 lands (`+94 = 523`, `act_life 520 → 280`). Retail's core
    /// aborts on the retarget, the wrapper still stamps
    /// `wiz 0 +528 = 200` — read raw at struct offset 14954, `aggro`
    /// 0 → 200 — and 16 ticks later the militiaman at slot 183 passes
    /// rung 1 of the idle acquisition ladder and takes the human
    /// (`+146 = 523`, `type86 = 206`, speed 0). The port, frozen out of
    /// the tail, held `player_aggro` at 0 and slot 183 wandered on.
    fn militia_chase_wanted_tail(&mut self, i: usize, base: u8) {
        // The wrapper's `if (+70 != 26)` diverts to `sub_1BCE0` before
        // the cadence is ever read (:22699-702).
        if self.ent[i].tick70 != base + 2 {
            return;
        }
        let e = &self.ent[i];
        let tgt = e.f146;
        if (e.f63 as i16) % BEHAVIOR[e.row156 as usize].v_26 != 0 {
            return;
        }
        // Retail indexes its fixed pool array with the raw `+146`; the
        // bound is memory safety only (the FROZEN path never reaches
        // the body's own index guard).
        if self.is_human_tag(tgt) || self.ent.get(tgt as usize).is_some_and(|c| c.model65 <= 1) {
            self.flag_village_wanted(tgt);
        }
    }

    /// m4 IDLE, state 25 (sub_1B5D0 :22436): the movement core, the
    /// unarmed-look / filter restore, then — every v_26, and only when
    /// +146 is EMPTY (the anchor arm below eats the tick otherwise) —
    /// the wander jitter and every
    /// 4·v_26 a TWO-rung acquisition ladder — (1) a wizard on the
    /// village wanted list (+528 ≠ 0, the hostility gate) within aggro
    /// range, (2) the nearest burrower (m9), NO gate — villagers fight
    /// burrowers on their own. There is no third rung: the port used to
    /// invent a house walk-in here (see the pair-up note below).
    /// Retail runs `sub_196E0` (`creature_move`) on
    /// the ALIVE path every tick (:22541) — the sole carrier of the
    /// altitude clamp `sub_42000`, so a militiaman spawned above ground
    /// by a village collapse settles onto it (every behavior row's
    /// v_14 is negative), and the idle speed (f128 = 30) makes him
    /// wander with the same two-draw yaw jitter as `mob_wander`
    /// (:22572-79). When neither rung lands, the idle pair-up scan
    /// (:22661-90) runs UNCONDITIONALLY — identical to the shared
    /// `pack_scan`, so a lone militiaman falls in behind the nearest
    /// packless sibling (state 0x1B).
    fn militia_idle(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.militia_idle_body(i, base, ctx);
        // sub_1B5D0's trailer (:22689-90): acquiring a target arms him
        // on the SAME tick, before the first chase tick runs.
        if self.ent[i].tick70 == base + 2 {
            self.militia_arm(i);
        }
    }

    fn militia_idle_body(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        // The walk-in-flag zero (:22482) is HOISTED to creature_tick's
        // pre-work block: retail writes it as sub_1B5D0's first
        // statement ABOVE the inline intake, so it must land on hit
        // and death ticks too — this body only runs on Quiet ticks
        // (the mc1hwl0 t=27516 born-into-fire militia was the
        // witness). +26 is only ever set during the one-tick hop from
        // the house branch below into the silent-absorb death slot.
        // (The unarmed-look restore that used to sit here was the
        // port's stand-in for the missing sub_1BCE0 disarm trailer;
        // retail's sub_1B5D0 writes neither sprite nor filter, and the
        // trailer now runs on the chase-exit tick where it belongs.)
        self.creature_move(i);
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (v26, r) = (row.v_26, row.v_28 as i32);
        if (self.ent[i].f63 as i16) % v26 != 0 {
            return;
        }
        // THE +146 ANCHOR ARM (:22546-68), the whole every-v_26 block's
        // FIRST test and an `if/else` over the jitter — not a step
        // beside it. A militiaman carrying ANY target consumes the tick
        // here: a house (class 10 / model 45) is walked toward while the
        // 3-D distance stays above 0x1000 and absorbed below it
        // (:22551-63), and anything else — the stale wizard a chase left
        // behind — is simply CLEARED (:22567). Either way there is no
        // yaw jitter, no acquisition ladder and no pair-up scan on that
        // tick. The clear is the reachable half: the shared chase's
        // range/lost exits drop a militiaman back to 25 with +146 still
        // naming his wizard, and the very next multiple of v_26 spends
        // itself forgetting him. mc1l42 t=471 slot 204 (and t=564/981):
        // retail zeroes +146 = 331, never draws and holds heading 38,
        // while the port jittered, re-acquired the human and armed.
        // The walk-in half stays unreachable — nothing in the port or
        // the original ever puts a house in an m4's +146 (see the
        // pair-up note below) — but it is retail's code, verbatim.
        let anchor = self.ent[i].f146;
        if anchor != 0 {
            let house = (anchor as usize) < self.ent.len()
                && self.ent[anchor as usize].class64 == 10
                && self.ent[anchor as usize].model65 == 45;
            if !house {
                self.ent[i].f146 = 0;
                return;
            }
            let (e, h) = (&self.ent[i], &self.ent[anchor as usize]);
            let dz = h.z.wrapping_sub(e.z) as i32;
            let d = Self::isqrt(
                Self::dist2_sq(e.x, e.y, h.x, h.y).wrapping_add(dz.wrapping_mul(dz)) as u32,
            );
            if d > 0x1000 {
                let (e, h) = (&self.ent[i], &self.ent[anchor as usize]);
                self.ent[i].f34 = Self::angle_between(e.x, e.y, h.x, h.y);
            } else {
                // The silent walk-in: state 0x1C is m4's DEATH slot, and
                // +26 = 1 is the absorb flag `mob_death` reads.
                self.ent[i].tick70 = base + 4;
                self.ent[i].f26 = 1;
                self.ent[anchor as usize].f26 = self.ent[anchor as usize].f26.wrapping_add(1);
            }
            return;
        }
        // Wander re-heading (:22572-79, identical to `mob_wander`):
        // d1 picks the sign via % 157, d2's low byte + 85 the size.
        let d1 = self.ent_rand(i);
        let d2 = self.ent_rand(i);
        let mag = ((d2 & 0xFF) + 85) as i32;
        let sign = if d1 % 157 >= 79 { 1 } else { -1 };
        self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
        if (self.ent[i].f63 as i16) % (4 * v26) != 0 {
            return;
        }
        // The nearest class-3 body, elected on range+cone alone; the
        // WINNER must then be a wizard body (+65<=1) on its own
        // village-wanted list (+160->+528, :22613 gates the winner,
        // not the candidates — see `nearest_wizard_target`). A refused
        // winner falls through to the burrower hunt below.
        if let Some(t) = self.nearest_wizard_target(i, ctx, true, true) {
            self.ent[i].f146 = t;
            self.ent[i].tick70 = base + 2;
            return;
        }
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let r2 = (r * r) as u32;
        // ⭐ The burrower rung walks the TICK-TOP m9 roster (:22624-31
        // walks `+36382 + 4*9`, the model-indexed chain the tick-head
        // sweep rebuilt), NOT the live pool: membership was sampled
        // once at the tick top, so a burrower born MID-tick is
        // invisible to this tick's ladder, and one that died mid-tick
        // is still a candidate — the walker carries no life test of
        // its own (the bucket[0] law's model-9 coat). mc1l5 t=4442:
        // one trigger tick mints 68 m9s and 32 m4s; the port's live
        // scan let newborn militia 774 acquire newborn burrower 757
        // and ARM in its birth frame — five graded lanes on one slot
        // — where retail's ladder walks the empty tick-top chain and
        // stays idle.
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.mob_chains.list.get(9).map_or(0, |l| l.len()) {
            let j = self.mob_chains.list[9][k] as usize;
            let c = &self.ent[j];
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if d2 <= r2 && best.is_none_or(|(_, b)| d2 < b) {
                best = Some((j, d2));
            }
        }
        if let Some((j, _)) = best {
            self.ent[i].f146 = j as u16;
            self.ent[i].tick70 = base + 2;
            return;
        }
        // A barren ladder falls STRAIGHT through to the pair-up: there
        // is no house rung. Bucket 9 (`+36382 + 4*9`) is the model-9
        // list — the per-tick rebuild routes class-10 model 45 only to
        // `var_u32_36462[2]` (+36470) and lets nothing but class 5 into
        // the model-indexed array (:52287-52313) — so the `class != 10
        // || model != 45` guard at :22643 is never-false shared-template
        // dead code. And even granting a house could win scan B, retail
        // would then do NOTHING: the guard skips the chase arm and the
        // non-zero winner skips the pack scan. Retail's only militia
        // walk-in is the +146 ANCHOR arm (:22546-63), which no writer
        // can reach — nothing ever puts a house in an m4's +146. The
        // port's invented house rung stamped `house.f26 += 1` and
        // `f26 = 1` where retail's last write of the tick is `+26 = 0`
        // (:22482), which is the whole mc1l2 (5,4)+(10,45) family (18
        // paired ticks) and the same signature in mc1l5. Retail itself
        // packs up mid-family: slot 286 goes f52 0 → 287 at t=5054→5055.
        self.pack_scan(i, base);
    }

    /// Nearest m45 house on the original's +36470 list — the TICK-TOP
    /// roster [`Gen::bldg_chain`], not the live pool, and carrying no
    /// life or flags test of its own (the walk at :25241 has NO
    /// per-candidate guard at all) — scored the way m12 SEEK scores
    /// it (:25241-49):
    /// `sub_42340_42680` (:52721-27), the THREE-axis distance, compared
    /// as a TRUNCATED isqrt under a strict `<` — so equal-rounding
    /// candidates resolve to the earlier entry. The `d != 0` skip is
    /// retail's own `if (v10 && v10 < v1)`.
    ///
    /// SEEK is the only caller. The m4 militia's 2-D `dx*dx + dy*dy`
    /// scan (:22628-31) used to share a helper here, but that whole
    /// ladder rung was the port's own invention and is gone — see
    /// [`Gen::militia_idle`].
    fn nearest_building_3d(&self, x: u16, y: u16, z: i16) -> Option<usize> {
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.bldg_chain.visible_len() {
            let j = self.bldg_chain.list[k] as usize;
            let c = &self.ent[j];
            let dz = c.z.wrapping_sub(z) as i32;
            let sum = Self::dist2_sq(x, y, c.x, c.y).wrapping_add(dz.wrapping_mul(dz));
            let d = Self::isqrt(sum as u32);
            if d != 0 && best.is_none_or(|(_, b)| d < b) {
                best = Some((j, d));
            }
        }
        best.map(|(j, _)| j)
    }

    /// m12 settler WANDER, state 73 (sub_1EED0 :24994): jitter-walk;
    /// +26 runs down one per think tick — at 0 → +26 = 1, SEEK (75).
    fn m12_wander(&mut self, i: usize) {
        self.creature_move(i);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 == 0 {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let mag = ((d2 & 0xFF) + 85) as i32;
            let sign = if d1 % 157 >= 79 { 1 } else { -1 };
            self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
            // :25077-84 — the test reads the PRE-decrement +26, so from
            // the ctor's 2 the settler spends THREE wander think-ticks,
            // not two. Testing post-decrement left our stream two
            // ent_rand draws ahead of retail's at BUILD time, which
            // rerolls btype and the side jitter.
            let pre = self.ent[i].f26;
            self.ent[i].f26 = pre - 1;
            if pre == 0 {
                self.ent[i].f26 = 1;
                self.ent[i].tick70 = 75;
            }
        }
    }

    /// m12 SEEK, state 75 (sub_1F390 :25198): the nearest house on
    /// the m45 list (state-51 sites included — settlers cluster
    /// around construction) → APPROACH; none on the map → wander
    /// forever (villages only grow around existing buildings).
    fn m12_seek(&mut self, i: usize) {
        let (x, y, z) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        if let Some(b) = self.nearest_building_3d(x, y, z) {
            self.ent[i].f146 = b as u16;
            self.ent[i].f26 = 10;
            self.ent[i].tick70 = 74;
        } else {
            self.ent[i].f26 = 5;
            self.ent[i].tick70 = 73;
        }
    }

    /// m12 APPROACH, state 74 (sub_1F120 :25101): steer to the anchor
    /// house; +26 runs down every v_26/2 ticks (target gone or
    /// patience out → wander); inside 0xA00 → BUILD with +26 = 0.
    fn m12_approach(&mut self, i: usize) {
        // :25164 — the walk runs BEFORE the think gate and on EVERY
        // tick, on last tick's heading; only the re-aim and the
        // proximity promotion sit inside the gate.
        self.creature_move(i);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        // :25165 — C precedence makes retail's `f63 % v_26 / 2` read
        // `(f63 % v_26) / 2`, NOT `f63 % (v_26 / 2)`: the think fires
        // whenever the remainder is 0 or 1, i.e. twice as often.
        if (self.ent[i].f63 as i16) % v26.max(1) / 2 != 0 {
            return;
        }
        // :25166 — retail indexes the anchor unconditionally; the only
        // validity test is the class BYTE at +64 being zero (a freed
        // slot), and it is folded into the patience test below. There
        // is no top-of-function guard and +146 is never cleared, so a
        // bailing settler keeps its stale anchor. The bounds clamp is
        // ours: retail's pool index cannot leave the table.
        let t = (self.ent[i].f146 as usize).min(self.ent.len() - 1);
        // :25168-70 — pre-decrement test, as in m12_wander.
        let pre = self.ent[i].f26;
        self.ent[i].f26 = pre - 1;
        if pre == 0 || self.ent[t].class64 == 0 {
            self.ent[i].f26 = 5;
            self.ent[i].tick70 = 73;
            // :25172 — NO return: retail falls through, so the
            // proximity test below can still promote to BUILD on the
            // very tick patience ran out.
        }
        let (ex, ey, ez) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (bx, by, bz) = (self.ent[t].x, self.ent[t].y, self.ent[t].z);
        self.ent[i].f34 = Self::angle_between(ex, ey, bx, by);
        // :25176 — sub_42340_42680 (:52721) is a THREE-axis distance,
        // and the 0xA00 is compared against the rooted value.
        let dz = bz.wrapping_sub(ez) as i16 as i32;
        let d3 = Self::dist2_sq(ex, ey, bx, by).wrapping_add(dz.wrapping_mul(dz));
        if Self::isqrt(d3 as u32) < 0xA00 {
            self.ent[i].f26 = 0;
            self.ent[i].tick70 = 72;
        }
    }

    /// m12 BUILD, state 72 (sub_1EA40 :24835): one site attempt per
    /// tick against the anchor house +146 — attempt # = the side
    /// (E/W/S/N), three settler-LCG draws each (type (rand&7)+25 =
    /// tent..house range, gap roll, perpendicular jitter). Water
    /// aborts to wander (+26 = 2); a rough or overlapping site just
    /// burns the attempt; the fifth entry resets (+26 = 1) to
    /// wander. Success spawns the (10,45) site in state 51 — the
    /// SAME 30-tick construction the features pass runs — and the
    /// settler retires into villager-feeder state 79: model stays
    /// 12, dispatch is state-based, exactly the original's trick.
    fn m12_build(&mut self, i: usize) {
        let a = self.ent[i].f146 as usize;
        let anchor_ok =
            a != 0 && a < self.ent.len() && self.ent[a].class64 == 10 && self.ent[a].model65 == 45;
        if !anchor_ok {
            self.ent[i].f26 = 5;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = 73;
            return;
        }
        let pre = self.ent[i].f26;
        self.ent[i].f26 = pre + 1;
        if pre >= 4 {
            self.ent[i].f26 = 1;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = 73;
            return;
        }
        let d = self.ent_rand(i);
        let btype = ((d & 7) + 25) as u16;
        let def = self.assets.build_tab[btype as usize % self.assets.build_tab.len()];
        // sub_1E9B0 (:24815): inflated footprint halves — the house
        // spacing margin.
        let half_x = ((def.w as i32) << 8) / 2 + 768;
        let half_y = ((def.h as i32) << 8) / 2 + 768;
        let (ax, ay, az, af80, af82) = {
            let e = &self.ent[a];
            (e.x, e.y, e.z, e.f80 as i32, e.f82 as i32)
        };
        let d1 = (self.ent_rand(i) % 3) as i32;
        let d2 = (self.ent_rand(i) % 3) as i32;
        let (mut px, mut py) = (ax as i32, ay as i32);
        match self.ent[i].f26 {
            1 => {
                px += af80 + half_x + (d1 << 8) + 256;
                py += (d2 << 8) - 1280;
            }
            2 => {
                px -= af80 + half_x + (d1 << 8) + 256;
                py += (d2 << 8) - 1280;
            }
            3 => {
                px += (d1 << 8) - 1280;
                py += af82 + half_y + (d2 << 8) + 256;
            }
            _ => {
                px += (d1 << 8) - 1280;
                py -= af82 + half_y + (d2 << 8) + 256;
            }
        }
        let (px, py) = (px as u16, py as u16);
        if self.on_water_pub(px, py) {
            self.ent[i].f26 = 2;
            self.ent[i].f146 = 0;
            self.ent[i].tick70 = 73;
            return;
        }
        // Flatness (sub_1E920/sub_35EA0): 4-corner max−min under the
        // 15/16 threshold.
        let thr = if (half_y >> 7) + (half_x >> 7) > 4 {
            16
        } else {
            15
        };
        if self.site_roughness(px, py, (half_x >> 8) as u8, (half_y >> 8) as u8) >= thr {
            return;
        }
        // Overlap vs every house, then every castle (:24940-75).
        //
        // ⭐⭐⭐ AND BOTH SCANS WALK A TICK-TOP ROSTER CHAIN, NOT THE
        // LIVE POOL. `HIDDEN.EXE 0x1ED41: mov ebx,[ebx+0x8e76]` is the
        // +36470 HOUSE head ([`Gen::bldg_chain`]) and `0x1EDB3: mov
        // ebx,[ebx+0x8e6e]` the +36462 CLASS-3 head
        // ([`Gen::wiz_chain`]) under `0x1EDC9: cmpb $0x2,0x41(%ebx)`
        // = the castle; each is stepped by `mov ebx,[ebx]`, the
        // `->next` word at +0, to the pool sentinel. `CARPET.EXE`
        // carries the identical bytes at the identical VAs. Neither
        // loop tests a flag — chain MEMBERSHIP is the whole gate, so
        // the port's `flags & 0x400` skip went with the pool scan.
        // See [`no_mc1_build_site_chain`] for what it cost.
        if no_mc1_build_site_chain() {
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                let house = c.class64 == 10 && c.model65 == 45;
                let castle = c.class64 == 3 && c.model65 == 2;
                if !(house || castle) || c.flags & 0x400 != 0 {
                    continue;
                }
                let dx = (c.x.wrapping_sub(px) as i16 as i32).abs();
                let dy = (c.y.wrapping_sub(py) as i16 as i32).abs();
                if dx <= c.f80 as i32 + half_x && dy <= c.f82 as i32 + half_y {
                    return;
                }
            }
        } else {
            for k in 0..self.bldg_chain.visible_len() {
                let c = &self.ent[self.bldg_chain.list[k] as usize];
                let dx = (c.x.wrapping_sub(px) as i16 as i32).abs();
                let dy = (c.y.wrapping_sub(py) as i16 as i32).abs();
                if dx <= c.f80 as i32 + half_x && dy <= c.f82 as i32 + half_y {
                    return;
                }
            }
            for k in 0..self.wiz_chain.visible_len() {
                let c = &self.ent[self.wiz_chain.list[k] as usize];
                if c.model65 != 2 {
                    continue;
                }
                let dx = (c.x.wrapping_sub(px) as i16 as i32).abs();
                let dy = (c.y.wrapping_sub(py) as i16 as i32).abs();
                if dx <= c.f80 as i32 + half_x && dy <= c.f82 as i32 + half_y {
                    return;
                }
            }
        }
        // Site accepted: the house goes up, the settler settles.
        if let Some(b) = self.spawn_creator(45, px, py, az) {
            self.snd(10, i); // construction gong (:24983)
            self.building_fixup(b, btype);
            self.ent[b].tick70 = 51;
        }
        self.ent[i].f146 = 0;
        self.ent[i].tick70 = 79;
    }

    /// sub_1E920/sub_35EA0 (:24802/:36260): 4-corner max−min height
    /// of the prospective footprint (spans in tiles), with the parity
    /// nudge on the start corner.
    /// ⭐ SHARED WITH MC2: `sub_22640` (EF:13906-16) is the same
    /// routine, called from `sub_22760`'s site test at EF:14036-40.
    pub(crate) fn site_roughness(&self, x: u16, y: u16, w_tiles: u8, h_tiles: u8) -> i32 {
        let mut v4 = ((x >> 8) as u8).wrapping_sub(w_tiles >> 1);
        let v5 = ((y >> 8) as u8).wrapping_sub(h_tiles >> 1);
        if (v4 as u16 + v5 as u16) % 2 == 1 {
            v4 = v4.wrapping_add(1);
        }
        let h = |cx: u8, cy: u8| self.t.height[crate::engine::features::tile(cx, cy)] as i32;
        let c = [
            h(v4, v5),
            h(v4.wrapping_add(w_tiles), v5),
            h(v4.wrapping_add(w_tiles), v5.wrapping_add(h_tiles)),
            h(v4, v5.wrapping_add(h_tiles)),
        ];
        *c.iter().max().unwrap() - *c.iter().min().unwrap()
    }

    /// m13/m14 feeder wander (sub_1F640 :25296 / sub_1FAC0 :25472):
    /// with a house target — steer home from beyond the 0x800 door
    /// radius (rooted THREE-axis distance, sub_42340_42680, and this
    /// runs BEFORE the fullness test: a full home keeps PULLING its
    /// villager back — the village leash), inside it walk in the door
    /// if there is room (death slot with +26 = 1 = silent absorb,
    /// house occupants++) else drop the anchor and slow to the accel
    /// speed (:25398-402); without one — jitter-walk and acquire the
    /// nearest house every v_26, speeding up to max for the walk home
    /// (:25436-38) (`distant`: m14 only ever anchors to a village
    /// farther than 0xE100000 dist² — unsigned 32-bit 2-D math,
    /// verbatim — the cross-map migrant stream).
    pub(crate) fn feeder_wander(&mut self, i: usize, base: u8, distant: bool) {
        self.creature_move(i);
        // One think gate wraps BOTH arms (:25382) — a stale/invalid
        // anchor is also only dropped on a think tick.
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 != 0 {
            return;
        }
        let t = self.ent[i].f146 as usize;
        if t != 0 {
            let valid =
                t < self.ent.len() && self.ent[t].class64 == 10 && self.ent[t].model65 == 45;
            if valid {
                let (ex, ey, ez) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                let (bx, by, bz) = {
                    let e = &self.ent[t];
                    (e.x, e.y, e.z)
                };
                let dz = bz.wrapping_sub(ez) as i32;
                let d3 = Self::dist2_sq(ex, ey, bx, by).wrapping_add(dz.wrapping_mul(dz));
                if Self::isqrt(d3 as u32) > 0x800 {
                    self.ent[i].f34 = Self::angle_between(ex, ey, bx, by);
                    return;
                }
                if self.ent[t].f128 > self.ent[t].f26 {
                    self.ent[t].f26 += 1;
                    self.ent[i].f26 = 1;
                    self.ent[i].tick70 = base + 4; // walks in the door
                    return;
                }
            }
            // Full or invalid: drop the anchor, wander at the accel
            // speed (:25399-401 LABEL_36) until a new home is taken.
            self.ent[i].f146 = 0;
            self.ent[i].f126 = self.ent[i].f130;
            return;
        }
        let d1 = self.ent_rand(i);
        let d2 = self.ent_rand(i);
        let mag = ((d2 & 0xFF) + 85) as i32;
        let sign = if d1 % 157 >= 79 { 1 } else { -1 };
        self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
        // Acquire (:25420-38): nearest house by unsigned 2-D squared
        // distance; the m14 migrant arm filters INSIDE the loop — the
        // nearest house BEYOND the threshold, not "nothing if the
        // nearest is inside it".
        let (ex, ey) = (self.ent[i].x, self.ent[i].y);
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.bldg_chain.visible_len() {
            let j = self.bldg_chain.list[k] as usize;
            let c = &self.ent[j];
            let dx = c.x.wrapping_sub(ex) as i16 as i32;
            let dy = c.y.wrapping_sub(ey) as i16 as i32;
            let d2 = (dx * dx) as u32 + (dy * dy) as u32;
            if distant && d2 <= 0xE100000 {
                continue;
            }
            if best.is_none_or(|(_, b)| d2 < b) {
                best = Some((j, d2));
            }
        }
        if let Some((b, _)) = best {
            self.ent[i].f146 = b as u16;
            // Head home at the max speed (:25437, +126 = +128).
            self.ent[i].f126 = self.ent[i].f128;
        }
    }

    /// The per-model attack thunks CHASE fires in range. Constants per
    /// the banked combat trace (docs/ROADMAP.md); projectile damage
    /// rides +44, explosions on +68/+69, owner immunity on +24.
    ///
    /// The return is retail's thunk return, which `sub_1A120` passes
    /// straight out (:21668-69) so a chase WRAPPER can trail the tick
    /// that actually connected: true = the attack happened (the
    /// projectile got a pool slot, or the melee reach test passed).
    /// m7's `sub_1C960` is the consumer — the m1/m2/m8 trailers are
    /// folded into their own arms below, where the same gate is
    /// already in scope.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn attack_thunk(
        &mut self,
        i: usize,
        model: u8,
        tgt: u16,
        tx: u16,
        ty: u16,
        tz: i16,
        tf66: u8,
        tf67: u8,
    ) -> bool {
        let (x, y, z, owner, f44, f84) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f44, e.f84)
        };
        // Every thunk spawns at the shooter's own z and lifts the
        // projectile's +76 AFTER the bearings are computed (see
        // arm_projectile) — never aim from the lifted muzzle.
        let lift = f84 as i16;
        // The SHOOTER's own filter pair rides every thunk in the
        // engine except m8's, which takes the TARGET's (sub_1AEE0
        // :22155-60): sub_1A8E0 :21895-98, sub_1A990 :21952-55,
        // sub_1AB70 :22005-06, sub_1AE30 :22122-25, sub_1AA40
        // :21951-52, m15 :25857-58 all write `+66/+67 = a1x->+66/+67`.
        // For most creatures that IS the shared (3, 0xFF) the port
        // used to hardcode — the ctor sets +66 = 3 and NewEvent
        // defaults +67 = 0xFF — but m4 and m9 OVERWRITE the pair with
        // their target's class/model on the chase-entry trailer
        // (sub_1BC50 / sub_1DCD0), so their shots inherit a NARROWED
        // filter. A mound besieging a castle fires (3, 2) bolts that
        // pass straight through the player, a rival carpet and a mana
        // balloon; ours were (3, 0xFF) and collided with all three —
        // `filter_admits` explicitly tests the human as (3, 0), so the
        // port let a castle-aimed bolt hit the wizard flying past.
        let (sf66, sf67) = {
            let e = &self.ent[i];
            (e.f66, e.f67)
        };
        match model {
            // sub_1A8E0 (:21874): the 500-damage straight fireball.
            0 | 3 => {
                if let Some(p) = self.spawn_fireball(x, y, z) {
                    self.ent[p].row156 = 6; // turn 0: no homing
                    let (owner, sf66, sf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                    self.arm_projectile(p, owner, sf66, sf67, tgt, tx, ty, tz, 500, 0, lift);
                    self.snd(8, i); // :22182/:22406
                    return true;
                }
                false
            }
            // sub_1AB10 (:21962): melee within 1024 units, m2 recoils.
            // (No cooldown gate — the thunk fires whenever the shared
            // chase cadence lands it in range; the bee's +26 only
            // drives the recoil/lunge cycle in bee_chase.)
            1 | 2 => {
                let d2 = Self::dist2_sq(x, y, tx, ty);
                let dz = tz.wrapping_sub(z) as i32;
                if Self::isqrt(d2.wrapping_add(dz.wrapping_mul(dz)) as u32) < 1024 {
                    let t = self.target_of_tag(tgt);
                    // sub_12B50 (:21970) — the INVERTED single-target
                    // protocol: melee accumulates onto the victim's
                    // stale amount, so repeated bites snowball.
                    self.mail_write_single(t, 0, f44 as u32, owner);
                    self.snd(if model == 2 { 13 } else { 7 }, i); // :22294/:22358
                    if model == 2 {
                        // Recoil + cooldown (:22356-62).
                        self.ent[i].f126 = -self.ent[i].f130;
                        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
                        self.ent[i].f26 = 3 * v26;
                    }
                    return true;
                }
                false
            }
            // sub_1A990 (:21907): the 250-damage straight bolt.
            4 | 10 => {
                if let Some(p) = self.spawn_bolt(x, y, z) {
                    let (owner, sf66, sf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                    self.arm_projectile(p, owner, sf66, sf67, tgt, tx, ty, tz, 250, 0, lift);
                    return true;
                }
                false
            }
            // sub_1AB70 (:21976): m5's mana-scaled multishot. The
            // roll bound is 7·mana/maxMana UNCLAMPED, so an
            // over-filled purse rolls past the last case and the
            // switch's DEFAULT fires NOTHING (v4 ≥ 7 falls through
            // and returns 0 — mc1l32-quick t=15188 lands exactly
            // v4 = 7). Sound 32 is the CALLER's, gated on the
            // return (:22972-75) — a silent default tick.
            5 => {
                let mut fired = false;
                let mana = self.ent[i].f140;
                let maxmana = self.ent[i].f136.max(1);
                let v2 = (7 * mana / maxmana).max(0) as u32;
                let v4 = if v2 != 0 {
                    (self.ent_rand(i) % (100 * v2)) / 100
                } else {
                    0
                };
                let n = (v2 as i32).clamp(1, 5);
                match v4 {
                    0 => {
                        for k in 0..n {
                            // :21990-22006 — the muzzle is re-read off
                            // the shooter's `+72` for EVERY shot in the
                            // volley, so a shot that seized the shooter
                            // re-homes the rest onto its own zeroed axis
                            // ([`crate::mc1::combat::no_mc1_self_seize`]).
                            let (x, y, z) = self.mc1_muzzle(i, (x, y, z));
                            if let Some(p) = self.spawn_fireball(x, y, z) {
                                self.ent[p].row156 = (6 - k).max(0) as u8;
                                let (owner, sf66, sf67, tgt, lift) = self.mc1_arm_live(
                                    p,
                                    i,
                                    (owner, sf66, sf67, tgt, lift),
                                    true,
                                    1,
                                );
                                self.arm_projectile(
                                    p, owner, sf66, sf67, tgt, tx, ty, tz, 400, 0, lift,
                                );
                                fired = true;
                            }
                        }
                    }
                    1 | 2 => {
                        // :22050 — each zigzag binds row 6−i (i runs
                        // 1..v5, rows 5..2), the same descending
                        // ladder as case 0's; the spawn default is
                        // NOT it.
                        for k in 1..n.max(1) {
                            let (x, y, z) = self.mc1_muzzle(i, (x, y, z));
                            if let Some(p) = self.spawn_zigzag(x, y, z) {
                                self.ent[p].row156 = (6 - k).max(0) as u8;
                                let (owner, sf66, sf67, tgt, lift) = self.mc1_arm_live(
                                    p,
                                    i,
                                    (owner, sf66, sf67, tgt, lift),
                                    true,
                                    1,
                                );
                                self.arm_projectile(
                                    p, owner, sf66, sf67, tgt, tx, ty, tz, 800, 23, lift,
                                );
                                fired = true;
                            }
                        }
                    }
                    3..=6 => {
                        if let Some(p) = self.spawn_trail_bolt(x, y, z) {
                            self.ent[p].row156 = 3;
                            let (owner, sf66, sf67, tgt, lift) =
                                self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                            self.arm_projectile(
                                p, owner, sf66, sf67, tgt, tx, ty, tz, 8000, 17, lift,
                            );
                            fired = true;
                        }
                    }
                    // v4 ≥ 7: the switch has no arm — nothing fires,
                    // the rand step still burned.
                    _ => {}
                }
                if fired {
                    self.snd(32, i);
                }
                fired
            }
            // sub_1AE30 (:22101): m7's 780-damage slow bolt (class-9
            // m14, the generic homing flight): the ctor binds row [6]
            // (:22122) and pre-targets the bolt with the thrower's
            // own +146 (:22122-23) — arm_projectile's `tgt` carries
            // it. OPEN (DEVIATIONS boulder entry): retail copies the
            // thrower's +66/+67 filter pair; we pass the shared
            // (3, 0xFF).
            7 => {
                if let Some(p) = self.spawn_slow_bolt(x, y, z) {
                    let (owner, sf66, sf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                    self.arm_projectile(p, owner, sf66, sf67, tgt, tx, ty, tz, 780, 0, lift);
                    self.ent[p].row156 = 6;
                    return true;
                }
                false
            }
            // sub_1AEE0 (:22134): m8's 4000-damage beam, filter
            // copied from the target's own fields, row [6] (:22155).
            // A landed attack refreshes the victim's wanted timer
            // (+528 = 200, sub_1CE30 :23557-60).
            8 => {
                if let Some(p) = self.spawn_zigzag(x, y, z) {
                    let (owner, tf66, tf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, tf66, tf67, tgt, lift), false, 1);
                    self.arm_projectile(p, owner, tf66, tf67, tgt, tx, ty, tz, 4000, 23, lift);
                    self.ent[p].row156 = 6;
                    self.snd(38, i); // :23555
                    self.flag_village_wanted(tgt);
                    return true;
                }
                false
            }
            // sub_1AA40 (:21935): m9's bolt — 600 with segments, else
            // 400. (Aimed at the TARGET; the transcription's
            // self-aim at :21947-48 is a decompile casualty.)
            // (`sub_1AA40` is `void` — m9 drives its own chase and
            // never reads a return; the value here is unobserved.)
            9 => {
                let dmg = if self.ent[i].f144 != 0 { 600 } else { 400 };
                if let Some(p) = self.spawn_bolt(x, y, z) {
                    let (owner, sf66, sf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                    self.arm_projectile(p, owner, sf66, sf67, tgt, tx, ty, tz, dmg, 0, lift);
                    // m9 alone re-skins its bolt (:21957): row 203 =
                    // sprite family base 215 where 195 is base 193 —
                    // same 45x60 size, same 5-view fold, so this is
                    // PURELY the billboard. Its arrow sound is retail
                    // asset reuse and stays (see DEVIATIONS.md).
                    self.set_sprite_x2(p, 203);
                    return true;
                }
                false
            }
            // sub_1E380 (:24554): m11's 3000-payload wizard-seeker
            // (explodes into the ch3 mana-steal flash, wizards only).
            // The lone thunk that writes NO filter at all (:24683-700
            // stamps +68/+69/+24/+44/+26/+76/+146/+30/+32 and nothing
            // else), and sub_39E40 (:46104-28) writes none either — so
            // the seeker flies on NewEvent's WILDCARD pair, −1/−1
            // (:43875-76), not the shared (3, 0xFF) that used to sit
            // here. It is the widest filter in the game: the seeker
            // collides with everything and dies silently on anything
            // that is not a wizard. mc1l42 reads sclass 255 on every
            // one of them (1,138 rows against the port's 3).
            11 => {
                if let Some(p) = self.spawn_seeker(x, y, z) {
                    self.ent[p].f26 = 20;
                    let (owner, _, _, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, 0xFF, 0xFF, tgt, lift), false, 1);
                    self.arm_projectile(p, owner, 0xFF, 0xFF, tgt, tx, ty, tz, 3000, 25, lift);
                    self.snd(9, i); // :24700
                    return true;
                }
                false
            }
            // m15 (:25846-59): a bare bolt — no +44 override, so the
            // NewEvent default 100 rides.
            15 => {
                if let Some(p) = self.spawn_bolt(x, y, z) {
                    let dflt = self.ent[p].f44;
                    let (owner, sf66, sf67, tgt, lift) =
                        self.mc1_arm_live(p, i, (owner, sf66, sf67, tgt, lift), true, 1);
                    self.arm_projectile(p, owner, sf66, sf67, tgt, tx, ty, tz, dflt, 0, lift);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// PACK sub_1A390 (:21677): mirror the leader — follow its
    /// heading, join its hunts, chain to its leader — with same-model
    /// separation and a per-v_26 speed bump of the leader's accel.
    /// `restore` = the `mc1_pack_speed_restore` patch arm: a follower
    /// leaving the pack (leader lost, leader joined a chase) puts its
    /// own `+128` back first — see [`crate::patches::WorldPatches`].
    fn mob_pack(&mut self, i: usize, base: u8, restore: bool) {
        let l = self.ent[i].f52 as usize;
        if l == 0 {
            if restore {
                self.ent[i].f126 = self.ent[i].f128;
            }
            self.ent[i].tick70 = base + 1;
            return;
        }
        self.creature_move(i);
        let e = &self.ent[i];
        let row = &BEHAVIOR[e.row156 as usize];
        if (e.f63 as i16) % row.v_26 != 0 {
            return;
        }
        // Only the follow cases (leader idling/wandering/packing) fall
        // through to separation + accel; joining a chase and the
        // default both RETURN (:21781, :21793 — running the accel on
        // those paths too was part of the runaway).
        match (self.ent[l].tick70 as i16) - base as i16 {
            0 | 1 => {
                let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                let (lx, ly) = (self.ent[l].x, self.ent[l].y);
                self.ent[i].f34 = Self::angle_between(ex, ey, lx, ly);
            }
            2 => {
                if restore {
                    self.ent[i].f126 = self.ent[i].f128;
                }
                self.ent[i].f146 = self.ent[l].f146;
                self.ent[i].f52 = 0;
                self.ent[i].tick70 = base + 2;
                return;
            }
            3 => {
                // Leader is packing too: chain to the grand-leader.
                self.ent[i].f52 = self.ent[l].f52;
                let g = self.ent[i].f52 as usize;
                if g != 0 {
                    let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                    let (gx, gy) = (self.ent[g].x, self.ent[g].y);
                    self.ent[i].f34 = Self::angle_between(ex, ey, gx, gy);
                }
            }
            _ => {
                // Retail (:21793): link cleared, speed kept — the
                // orphan wanders one catch-up step hot and seeds the
                // next generation of the ratchet. The patch restores.
                if restore {
                    self.ent[i].f126 = self.ent[i].f128;
                }
                self.ent[i].f52 = 0;
                self.ent[i].tick70 = base + 1;
                return;
            }
        }
        // Separation (:21796): first same-model neighbor within a tile
        // square points us away from it. ⚠ The box is MOVSX-SIGNED
        // PER COORDINATE (the binary at obj1 0x1d5eb: `movsx` each
        // 16-bit x/y, then a 32-BIT subtract — never a 16-bit wrapped
        // difference): a pair straddling the signed midline 0x8000
        // reads ~65k apart and NEVER separates. mc1l1 pinned it with
        // five straddle skips (t=1924/1933/4003/4004/4433, all with
        // own/member on opposite sides of y=0x8000, |wrapped dy| <
        // 256 every time) against 436 same-side fires — the same
        // movsx-signed law as the ch0 area window. The id24 self-skip
        // and the full-chain first-hit walk are byte-verified against
        // the binary.
        //
        // ⭐⭐ IT WALKS THE PER-MODEL ROSTER CHAIN, NOT THE ARRAY.
        // remc1 :21796 loads `str_36382x[+65]` — the [`MobChains`]
        // head — and follows `+0` (`v13 = *(_DWORD *)v13`) to the
        // pool base; remc1hw :20346 is byte-identical, so this is two
        // witnesses. The only per-member test retail applies is the
        // `+24 != own id` self-skip: class, model, state and life are
        // MEMBERSHIP, sampled at tick top by the roster rebuild, not
        // re-read live. A full-array walk gets that wrong in both
        // directions (it sees a creature the tick-top rebuild did not,
        // and it hides one that died mid-tick), and — the head that
        // caught it — it is BLIND TO THE SEIZURE BLANK: mc1hwl0
        // t=42648 exhausts the pool (free 10 → 0, recycle 793 → 352,
        // 441 records seized for the eruption plume), and the MC1
        // seizure memsets all 20 heads (:43885-91), so every roster
        // walk dispatched after the seizing slot sees an EMPTY list.
        // Retail's militia 924 therefore keeps its leader bearing to
        // slot 34 (`+34` = 728) while the array scan found militia 236
        // one tile east and turned away from it (1575).
        let e = &self.ent[i];
        let (ex, ey, id, model) = (e.x, e.y, e.id24, e.model65);
        for k in 0..self.mob_chains.visible(model as usize).len() {
            let j = self.mob_chains.visible(model as usize)[k] as usize;
            let (cx, cy, cid) = {
                let c = &self.ent[j];
                (c.x, c.y, c.id24)
            };
            if cid == id {
                continue;
            }
            let dx = ((ex as i16 as i32) - (cx as i16 as i32)).abs();
            let dy = ((ey as i16 as i32) - (cy as i16 as i32)).abs();
            if dx < 256 && dy < 256 {
                self.ent[i].f34 = Self::angle_between(cx, cy, ex, ey);
                break;
            }
        }
        // Catch-up (:21814): member speed = LEADER's speed + accel —
        // a bounded "fly slightly faster than the leader". The remc1
        // source line reads `a1x->+126 += v3x->+130`, but the dead
        // decompiler temp preserved above it reads BOTH operands from
        // the LEADER (`v10 = v3x->+130 + v3x->+126`) — a dead temp of
        // the += form would read the member's +126. The original
        // computed the leader sum; the += is a maintainer mis-fix
        // whose unbounded accumulation is exactly the runaway (IDLE's
        // pack scan is NOT awake-gated, so distant idle crowds pack
        // up and would ratchet forever). The bee's retail "no escape"
        // is the 3x lunge in bee_chase, not this line.
        //
        // ⛔ DO NOT "fix" this toward the `+=`. Three independent
        // reasons, in increasing order of authority:
        //   1. remc1 :21813 and remc1hw :20370 both keep the ORIGINAL
        //      decompiler line commented out directly above the live
        //      rewrite — and it is the SET form, byte for byte. Those
        //      two files share a maintainer, so they are ONE witness;
        //      the cross-binary check is remc2 EF:9482, a different
        //      lineage, which carries the SET form too.
        //   2. An unbiased `dump-state` sweep of the mc1l5 recording
        //      over every live class-5 entity finds NO creature above
        //      its own +128 at t=5000/12000/17000 except one bee mid
        //      3x lunge. The `+=` would have filled that take with
        //      inflated creatures. Recorded gameplay outranks the
        //      decompile.
        //   3. There is no cap on this path in EITHER engine: +128 and
        //      +130 are write-once (ctors only) and the mover passes
        //      +126 verbatim (sub_196E0 :21182 -> sub_41EC0 :52523).
        //      What bounds retail is the per-model chase-exit RESTORE
        //      (m2 :22366, m4 :22768, m7 :23353, m9 :24257, m15
        //      :25901), which is why those are ported rather than a
        //      clamp added here — a `.min(+128)` would be measurably
        //      WRONG, retail carries m2 +126 = 95 against +128 = 70
        //      for 62 creature-ticks in mc1l5 alone.
        self.ent[i].f126 = self.ent[l].f126.wrapping_add(self.ent[l].f130);
    }

    /// DEATH sub_1A6C0 (:21820): one tick — body segments become
    /// corpses (any segment's killer propagates to the head), kill
    /// credit, then self to CORPSE. A militia (m4), feeder (m13/m14)
    /// or retired settler that walked into a house set +26 = 1 and
    /// despawns silently instead — no corpse, so no mana ball, no
    /// death-flame, no 400-dmg fire onto the dwelling it just entered.
    /// Retail decides this in the per-model death slots, each of which
    /// gates on +26 regardless of how the slot was reached (walk-in or
    /// combat): militia sub_1BC10 (:22729), m13 sub_1FA00 (:25447),
    /// m14 sub_1FE90 (:25623). Keyed on the DISPATCH model (base/6),
    /// NOT model65 — a settler walks in still carrying model65 = 12 yet
    /// dispatches through the m13 slot (state 79 -> death 82); gating on
    /// model65 leaves militia and settlers falling through to the corpse
    /// path, whose 400-dmg fire drives the village-churn destruction.
    ///
    /// `chain_revalidate` = PATCH `mc1_segment_chain_revalidate`
    /// (docs/DEVIATIONS.md): the segment walk below stops at the first
    /// `+54` link that no longer names a class-5 member of THIS body
    /// (same `+24`). Retail's walk (sub_1A6C0 :21792, `CARPET.EXE` file
    /// 0x32EB8-0x32F26: `mov 0x36(%ebx),%bx` → `call sub_424F0`, which
    /// is a bare `mov %al,0x46(%edx)`) tests nothing but `+54 != 0`, so
    /// a head that dies a SECOND time stamps `base + 5` onto whatever
    /// record now occupies a reaped segment's slot.
    /// The PACK arm's write through `+52` (:21746 lethal, :21755-65
    /// hit): the partner clears its own `+52`, takes `target` as its
    /// quarry and enters CHASE (`base + 2`). Retail's only gate is
    /// `+52 != 0`, and nothing clears a member's `+52` when its leader
    /// dies and the slot is re-minted.
    ///
    /// `revalidate` = PATCH `mc1_segment_chain_revalidate`, the same
    /// ghost-mana class as the segment walk in [`Self::mob_death`]:
    /// the write lands only on a class-5 record of the member's own
    /// `model65` — the identity `pack_scan` linked it by. WITNESS
    /// `ghostmana.mgcr` (mc1 level 9, a port take on retail patches):
    /// a (5,10) leader dies at t=3489, its slot is re-minted as the
    /// player's (10,0) fireball fire at t=3494, and at t=3496 a dying
    /// member stamps `60 + 2 = 62` onto it. Class-10 state 62 is the
    /// mana-sphere handler, so the fire falls and rolls as an
    /// uncollectable "ball" for the rest of the level (again at
    /// t=3577, slot 131).
    pub(crate) fn pack_partner_retarget(
        &mut self,
        i: usize,
        base: u8,
        target: u16,
        revalidate: bool,
    ) {
        let l = self.ent[i].f52 as usize;
        if l == 0 || l >= self.ent.len() {
            return;
        }
        if revalidate && (self.ent[l].class64 != 5 || self.ent[l].model65 != self.ent[i].model65) {
            return;
        }
        self.ent[l].f52 = 0;
        self.ent[l].f146 = target;
        self.ent[l].tick70 = base + 2;
    }

    pub(crate) fn mob_death(&mut self, i: usize, base: u8, chain_revalidate: bool) {
        if matches!(base / 6, 4 | 13 | 14) && self.ent[i].f26 != 0 {
            self.ent[i].flags |= 0x400;
            return;
        }
        // ⭐ THE VILLAGE-WANTED ARM IS A DEATH-STATE HANDLER, NOT A
        // DEATH-TRANSITION TRAILER. The griffon's `sub_1CF60` (state
        // 0x34 = 48 + 4, :23574-80) and the feeder's `sub_1FA00`
        // (state 0x52 = 78 + 4, :25447-62) each stamp
        // `pool[+38].+160->u16_528 = 200` and THEN call
        // `sub_1A6C0` — so the write runs on the FIRST tick spent IN
        // the death state, one tick after the prologue put the record
        // there, and reads `+38` as it stands then. m13's copy sits in
        // the NON-absorb `else`, so a feeder that walked into a house
        // (+26 != 0) marks nobody — the early return above.
        // Both arms precede the body, so they take the PRE-propagation
        // `+38`, before the segment chain below can overwrite it.
        if matches!(base / 6, 8 | 13) && !no_mc1_wanted_death_state() {
            self.flag_village_wanted(self.ent[i].f38);
        }
        // ⭐ PATCH `mc1_segment_chain_revalidate` — THE KRAKEN'S
        // PHANTOM MANA BALL (player-reported 2026-09-19,
        // `recordings/mc1l42-new.mgcr`). The head's `+54` is never
        // cleared when a segment's corpse drops its ball and is reaped;
        // a pack-mate's blind `+52` write (the PACK arm of the damage
        // prologue) can revive the head's CORPSE to chase, it dies
        // again, and this walk then stamps `+70 = base + 5` onto the
        // recycled slot. For the kraken (base 36) that is 41 — on a
        // class-10 record, the MANA BALL handler — so a (10,0) corpse
        // fire becomes a permanent, unpossessable model-0 "ball".
        // Retail arm: the blind walk. Patched: stop at a non-member.
        let own = self.ent[i].id24;
        let mut s = self.ent[i].f54 as usize;
        while s != 0 {
            if chain_revalidate && (self.ent[s].class64 != 5 || self.ent[s].id24 != own) {
                break;
            }
            self.ent[s].tick70 = base + 5;
            if self.ent[s].f38 != 0 {
                self.ent[i].f38 = self.ent[s].f38;
            }
            s = self.ent[s].f54 as usize;
        }
        // The stats ledger's every-death tally (not retail — see
        // `engine::stats`); after the chain inherit, so a segment's
        // killer bills the head.
        self.stats_note_death(i);
        // Kill credit (:21840-50): the human player, chain heads only,
        // spell-track models excluded. The reward itself is the ball.
        if let Some(hx) = self.hx_of_tag(self.ent[i].f38)
            && self.ent[i].id24 == i as u16
            && !matches!(self.ent[i].model65, 9 | 12 | 13 | 14 | 15)
        {
            self.kills[hx] += 1;
        }
        self.ent[i].tick70 = base + 5;
    }

    /// CORPSE sub_1A800 (:21855), on every 8th phase tick: drop the
    /// mana ball (sub_27690) and the death-flame puff, then despawn.
    /// Every worm segment corpses independently — each drops its own.
    fn mob_corpse(&mut self, i: usize, base: u8) {
        // The settler's `sub_1F5B0` (state 0x4D = 72 + 5, :25285-92)
        // and the second feeder slot's `sub_1FEC0` (state 0x59 =
        // 84 + 5, :25632-38) are CORPSE handlers that arm +528 and
        // then call `sub_1A800` — so their write is TWO ticks after
        // the kill, it is UNGATED by the every-8th-tick drop below,
        // and it RE-FIRES on every corpse tick the record survives.
        // (mc1hwl2: the settler at slot 136, killed by the human's
        // castle footprint at t=372, holds retail's `aggro` at 199
        // across BOTH t=375 and t=376 — a 200 re-stamp against the
        // per-tick decay — then lets it fall to 198 at t=377.)
        if matches!(base / 6, 12 | 14) && !no_mc1_wanted_death_state() {
            self.flag_village_wanted(self.ent[i].f38);
        }
        if self.ent[i].f63 & 7 == 0 {
            self.corpse_drop(i);
            self.corpse_puff(i);
            self.ent[i].flags |= 0x400;
        }
    }

    /// sub_42510_42850 (:52763): one animation-frame step; true =
    /// already finished (does not wrap).
    pub(crate) fn anim_advance(&mut self, i: usize) -> bool {
        if self.ent[i].frame88 >= self.ent[i].frames89 {
            true
        } else {
            self.ent[i].frame88 += 1;
            false
        }
    }

    // ---- model 9, the burrower (states 54/55, :23591-:23920) ---------------

    /// sub_1DD50 (:24255): the hidden-mound disguise — and the mound's
    /// speed RESTORE. Retail runs it from exactly two places: the
    /// ctor's emergence (:23619) and the chase EXIT trailer (:24212,
    /// `if (+70 != 56)`).
    fn m9_disguise(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.set_sprite(i, 201);
        self.ent[i].f66 = 3;
        self.ent[i].f67 = 0xFF; // sModel = -1
        self.ent[i].f26 = 50;
        self.ent[i].f71 = 0;
    }

    /// sub_1DCD0 (:24236): the mound's chase-ENTRY trailer, run by the
    /// hidden (:23922) and pack (:24220) handlers on the promotion
    /// tick. A mound that acquired its own owner's body drops straight
    /// back to hidden; otherwise it STOPS (+126 = 0 — retail's
    /// burrower fights rooted, it never walks in the warrior form),
    /// pops the type-202 disguise and takes the target's class/model
    /// as its bolt filter.
    fn m9_enter_chase(&mut self, i: usize) {
        let tgt = self.ent[i].f146;
        // :24240-48 stages ONE pointer `v1 = pool + 164 * +146` and runs
        // both the `id24` equality test and the filter stamp off it, with
        // no index test — so a `+146` of 0 tests against the SCRATCH
        // record's id and stamps its (0, 0). The `t != 0` arm was
        // invented, and it short-circuited the id test as well as the
        // stamp.
        let (tc, tm) = match tgt as usize {
            _ if self.is_human_tag(tgt) => (3u8, 0u8),
            t if t < self.ent.len() && !(t == 0 && no_m4m9_arm_scratch_target()) => {
                if self.ent[i].id24 == self.ent[t].id24 {
                    self.ent[i].tick70 = 55; // 0x37, back to hidden
                    return;
                }
                (self.ent[t].class64, self.ent[t].model65)
            }
            _ => (3u8, 0u8),
        };
        self.ent[i].f126 = 0;
        self.set_sprite(i, 202);
        self.ent[i].f66 = tc;
        self.ent[i].f67 = tm;
    }

    /// Spawn state 54, sub_1CFF0 (:23591): the materialize sequence —
    /// the spawn form (type 220, the player's "blue flame") counts
    /// down, swaps to the 16-frame transform animation (type 237) at
    /// 17, steps its frames every other tick, then settles into the
    /// type-201 mound at state 55.
    fn m9_emerge(&mut self, i: usize) {
        let v1 = self.ent[i].f26;
        self.ent[i].f26 = v1.wrapping_sub(1);
        if v1 != 0 {
            if v1 == 17 {
                self.set_sprite(i, 237);
            } else if v1 - 1 < 16 && (v1 - 1) % 2 == 0 {
                self.anim_advance(i);
            }
        } else {
            self.m9_disguise(i);
            self.ent[i].tick70 = 55;
            self.ent[i].f26 = 400;
            self.ent[i].f71 = 0;
        }
    }

    /// The mound's roam/CONVERT tail (surfaced sub_1D060 :23834-23917,
    /// buried sub_1D6D0 :24030-24116): the undead-army growth. Every
    /// v_26 ticks a mound with nothing to chase eats the nearest
    /// civilian and mints a fresh (5,9) at its feet. The victim menu
    /// cycles on the mound's own clock — `f63 / v_26 % 3` → m4 village
    /// militia / m12 settler / m13 feeder (:23837; m14/m15 are never
    /// on it) — with NO owner or team filter: a mound eats its own
    /// wizard's villagers too. Nearest is XY-only within v_28 (8
    /// tiles); the kill needs the 3-D reach ≤ 0x600 (:23904/:24103).
    /// The victim is destroy-flagged raw (sub_41E80 — no death state,
    /// no corpse, no mana ball, no kill credit) and the newborn runs
    /// the ctor's state-54 emergence, never born buried. Owner stamp
    /// = the parent's id24; the surfaced arm gates it on the owner
    /// actually being a wizard body (:23912), the buried arm stamps
    /// unconditionally (:24112 — a wild mound passes its own slot
    /// index on, a genuine retail quirk kept as-is).
    ///
    /// PATCH `skeletons_convert_traders`: the menu rotates `% 4` and
    /// adds the m14 migrant trader, which retail's `% 3` never reaches.
    fn m9_convert(&mut self, i: usize, buried: bool) {
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let menu: &[u16] = if self.skeletons_convert_traders.0 {
            &[4, 12, 13, 14]
        } else {
            &[4, 12, 13]
        };
        let victim_model = menu[((self.ent[i].f63 as i16 / row.v_26) % menu.len() as i16) as usize];
        let (ex, ey, ez, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        let r2 = ((row.v_28 as i32) * (row.v_28 as i32)) as u32;
        // THE VICTIM SCAN WALKS THE TICK-TOP PER-MODEL ROSTER
        // (`+36382 + 4*model`, :23887-98) WITH NO PER-NODE GATES —
        // nearest-within-v_28² is the whole test. The tick-top rebuild
        // already applied `act_life >= 0 && +70 != 120`, and retail
        // re-tests NEITHER: a militiaman another mound converted (and
        // soft-killed) EARLIER THIS TICK is still a valid victim, and
        // the second mound mints a second burrower on the same corpse
        // (mc1l5 t=4576: mounds 733 and 790 both convert militia 792,
        // burrowers 796 + 813 born at one axis; sub_41E80 re-kills the
        // corpse idempotently). The port's pool scan with live
        // life/0x400/state-120 re-tests lost the second mint — the
        // chain-vs-pool bug in its per-node-gate-set costume.
        let mut best: Option<(usize, u32)> = None;
        for &member in self.mob_chains.visible(victim_model as usize) {
            let j = member as usize;
            let c = &self.ent[j];
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if d2 <= r2 && best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        let Some((v, _)) = best else { return };
        let (vx, vy, vz) = (self.ent[v].x, self.ent[v].y, self.ent[v].z);
        let dz = vz.wrapping_sub(ez) as i32;
        let sq = Self::dist2_sq(ex, ey, vx, vy).wrapping_add(dz.wrapping_mul(dz));
        if Self::isqrt(sq as u32) > 0x600 {
            return;
        }
        self.ent[v].flags |= 0x400;
        if let Some(n) = self.spawn_creature(9, vx, vy, vz) {
            let owner_is_wizard =
                (own as usize) < self.ent.len() && self.ent[own as usize].class64 == 3;
            if buried || owner_is_wizard {
                self.ent[n].id24 = own;
            }
        }
    }

    /// Hidden state 55, sub_1D060 (:23627): the mound lurks — burrow
    /// timer (bury as type 245 when the countdown runs out and the
    /// player is away), burrow-walk + every v_26 a CASTLE hunt
    /// (nearest class-3 model-2; within its extent + v_28 → chase),
    /// the standard yaw jitter when no castle exists, then — whenever
    /// no castle chase was taken — the awake-gated WIZARD scan
    /// (:23796-23833) and, if that declines too, the `m9_convert`
    /// tail. Buried mode (sub_1D6D0): the wizard entering the 24-tile
    /// wake gate arms a −50 countdown and the mound rises again
    /// (sub_1DDB0); asleep, it runs the convert tail underground.
    fn m9_hidden(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.m9_hidden_body(i, base, ctx);
        // sub_1D060's trailer (:23921-22): promoting to CHASE runs the
        // entry trailer on the SAME tick.
        if self.ent[i].tick70 == base + 2 {
            self.m9_enter_chase(i);
        }
    }

    /// m9's HIDDEN PRE-WORK (sub_1D060 :23682-98): the burrow
    /// countdown (bury as type 245 on the tick it reaches 1) and then
    /// — for a mound still on the surface — the AWAKE RE-ARM,
    /// `if (+58) +26 = 400` (:23697-98). Both sit above the mailbox
    /// prologue at :23700, so a mound that takes its promoting hit
    /// still ticks the timer and still re-arms: the burrow clock only
    /// ever runs down while the player is far, and a hit tick is by
    /// definition a tick with someone near. mc1l42 t=20721 slot 34 is
    /// the family: retail hands the promoted mound +26 = 400, the
    /// port's blanket hit-abort left the imported 399 standing (and at
    /// t=23732 left the chase-exit disguise's 50).
    ///
    /// Scoped to `+71 == 0` — retail's countdown is a no-op for a
    /// buried mound (its +26 is the NEGATIVE unbury clock) and the
    /// buried branch at :23691-95 jumps past the re-arm to the entry
    /// trailer, so hoisting only the surfaced head is exact.
    fn m9_hidden_prework(&mut self, i: usize) {
        if self.ent[i].f71 != 0 {
            return;
        }
        let v1 = self.ent[i].f26;
        if v1 > 0 {
            self.ent[i].f26 = v1 - 1;
            if v1 == 1 {
                // sub_1DD90: bury.
                self.set_sprite(i, 245);
                self.ent[i].f71 = 1;
                return; // :23691-95 — the bury tick skips the re-arm
            }
        }
        if self.ent[i].f58 != 0 {
            self.ent[i].f26 = 400; // player near: stay surfaced
        }
    }

    fn m9_hidden_body(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        // The countdown / bury / awake re-arm head already ran in
        // `creature_tick`, above the intake.
        if self.ent[i].f71 != 0 {
            // Buried, sub_1D6D0 (:23926). Damage pops it into CHASE
            // via the shared prologue (retail's own inbox arm). Core
            // machinery: a running unbury countdown advances FIRST
            // (:24016-22), else an awake trigger — the wizard inside
            // the 24-tile wake gate — arms it at −50 (:24024-28), so
            // a buried mound rises ~1 s after the player flies near.
            let v8 = self.ent[i].f26;
            if v8 < 0 {
                self.ent[i].f26 = v8 + 1;
                if v8 == -1 {
                    // sub_1DDB0 (:24273): rise back to the mound.
                    self.ent[i].f71 = 0;
                    self.ent[i].f26 = 400;
                    self.set_sprite(i, 201);
                }
            } else if self.ent[i].f58 != 0 {
                self.ent[i].f26 = -50;
            } else if (self.ent[i].f63 as i16) % BEHAVIOR[self.ent[i].row156 as usize].v_26 == 0 {
                // The ASLEEP roam/convert scan (:24030-116): buried
                // mounds grow the army only while the player is far —
                // the village is gone by the time you fly back.
                self.m9_convert(i, true);
            }
            return;
        }
        self.creature_move(i);
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        if (self.ent[i].f63 as i16) % row.v_26 != 0 {
            return;
        }
        // Nearest castle — a walk of BUCKET[0] (:23752 reads
        // `var_u32_36462[0]`), filtered to `+65 == 2 && +24 != own`
        // and nothing else, at unbounded radius. ⭐ THE MEMBERSHIP IS
        // THE TICK-TOP SNAPSHOT, so there is NO life test here: a
        // castle that dies mid-tick is still this cadence's answer.
        // mc1l4 t=1017 is the receipt — castle 71 takes its fatal hit
        // earlier in that very tick, and all four (5,9) mounds still
        // write its bearing (`+34` holds 1174/1102/1114/1137) where a
        // live-pool scan loses it and falls to the two-draw wander
        // jitter instead.
        let e = &self.ent[i];
        let (ex, ey, ez, id) = (e.x, e.y, e.z, e.id24);
        let mut best: Option<(usize, u32)> = None;
        for c in 0..self.wiz_chain.visible_len() {
            let j = self.wiz_chain.list[c] as usize;
            let c = &self.ent[j];
            if c.model65 != 2 || c.id24 == id {
                continue;
            }
            let d2 = Self::dist2_sq(ex, ey, c.x, c.y) as u32;
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        let mut chased = false;
        if let Some((j, _)) = best {
            let (cx, cy, cz) = (self.ent[j].x, self.ent[j].y, self.ent[j].z);
            self.ent[i].f34 = Self::angle_between(ex, ey, cx, cy);
            let dz = cz.wrapping_sub(ez) as i32;
            let sq = Self::dist2_sq(ex, ey, cx, cy).wrapping_add(dz.wrapping_mul(dz));
            let range = self.ent[j].f80 as u32 + row.v_28 as u32;
            if Self::isqrt(sq as u32) <= range {
                self.ent[i].f146 = j as u16;
                self.ent[i].tick70 = base + 2;
                chased = true;
            }
        } else {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let mag = ((d2 & 0xFF) + 85) as i32;
            let sign = if d1 % 157 >= 79 { 1 } else { -1 };
            self.ent[i].f34 = ((self.ent[i].f34 as i32 + sign * mag) & 0x7FF) as u16;
        }
        // The `if (!v46)` wizard scan (:23796-23833): a castle found
        // but out of range falls through here too (no jitter, like
        // the original). Same range/cone/invisibility gates as the
        // shared wander scan.
        let mut engaged = chased;
        if !chased && self.ent[i].f58 != 0 {
            if let Some(t) = self.nearest_wizard_target(i, ctx, false, false) {
                self.ent[i].f146 = t;
                self.ent[i].tick70 = base + 2;
                engaged = true;
            }
        }
        // The last-resort convert tail (:23834-: the `if (!v46)`
        // branch) — only when neither the castle hunt nor the wizard
        // scan took a target this cadence tick.
        if !engaged {
            self.m9_convert(i, false);
        }
    }

    /// Flyer altitude oscillator sub_1B120 (:22206) — model 0's
    /// wander/chase/pack wrappers; +26 doubles as vertical speed.
    fn flyer_bob(&mut self, i: usize) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let ground = self.ground_z(x, y) as i16;
        let e = &mut self.ent[i];
        e.z = e.z.wrapping_add(e.f26);
        e.f26 -= 5;
        if e.z < ground.wrapping_add(256) {
            e.f26 = 150;
        }
    }

    /// m1's IDLE wrapper tail, sub_1B160 :22228-45 — the mover
    /// `sub_196E0`, then the re-aim/drop. Both sit BELOW the shared
    /// `sub_19B10`, so a hit or death tick still runs the mover; the
    /// re-aim's `+70 == 6` gate is what turns IT off once the prologue
    /// has promoted the bird, which is why the two need separating.
    /// (`only_the_vulture_moves_while_idle`: ten wrappers call the
    /// shared idle, exactly one also calls the mover.)
    fn m1_idle_trailer(&mut self, i: usize, base: u8) {
        self.creature_move(i);
        // :22232 — the retarget runs only if the pack scan (or the
        // damage prologue) did not promote, and only on the think tick.
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if self.ent[i].tick70 == base && (self.ent[i].f63 as i16) % v26 == 0 {
            let t = self.ent[i].f146 as usize;
            // :22235 — a target whose record went class 0 is dropped
            // and the bird falls back to wander. The f146 = 0 case
            // lands here too: slot 0 is the sentinel and reads class 0.
            if t < self.ent.len() && self.ent[t].class64 != 0 {
                let (ax, ay) = (self.ent[i].x, self.ent[i].y);
                let (bx, by) = (self.ent[t].x, self.ent[t].y);
                self.ent[i].f34 = Self::angle_between(ax, ay, bx, by);
            } else {
                self.ent[i].f146 = 0;
                self.ent[i].tick70 = base + 1;
            }
        }
    }

    /// Body segment state 120, sub_19550 (:21107): rigid follow —
    /// awake segments sit at distance +56 behind their leader along
    /// the exact bearing (position derived from the leader every
    /// tick); asleep ones collapse onto it every 4th tick.
    ///
    /// Patch option `mc1_fix_dragon_tail` (`follow_asleep`, player-
    /// ruled 2026-09-07): the rigid follow runs whether or not the
    /// segment is awake, so a distant dragon's tail trails its head
    /// instead of collapsing onto it — retail's every-4th-tick
    /// collapse is a period CPU save behind the 24-tile awake radius,
    /// and it is the only reason anyone raises `awake_range` (which
    /// wakes the whole ecology). The damage intake stays awake-gated:
    /// asleep creatures do not take mail damage.
    fn segment_follow(&mut self, i: usize, follow_asleep: bool) {
        let l = self.ent[i].f52 as usize;
        // Orphan (:21116-17): a leader no longer class 5 flags the
        // segment dead (sub_41E80 = 0x400) and FALLS THROUGH — the
        // orphan still follows the stale leader slot this tick
        // (f52 == 0 follows the scratch slot, as retail reads it).
        if self.ent[l].class64 != 5 {
            self.ent[i].flags |= 0x400;
        }
        let (lx, ly, lz) = (self.ent[l].x, self.ent[l].y, self.ent[l].z);
        let awake = self.ent[i].f58 != 0;
        if awake || follow_asleep {
            let e = &self.ent[i];
            let yaw = Self::angle_between(e.x, e.y, lx, ly);
            // Vertical bearing sub_42180 (:52644).
            let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, lx, ly) as u32) as i16;
            let pitch = Self::angle_of(e.z.wrapping_sub(lz), dh.wrapping_neg());
            self.ent[i].f30 = yaw;
            self.ent[i].f32 = pitch;
            let mut tmp = (lx, ly, lz);
            let d = self.ent[i].f56 as i16;
            Self::polar_step(&mut tmp, yaw, pitch, -d);
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
            // Damage intake AFTER the move (:21127-37): apply pending
            // ch0 and latch the attacker in +40 ALONE — or CLEAR the
            // latch on a quiet tick (the head's chain walk inherits
            // +40; +38 is the lethal branch's alone).
            if !awake {
                // (the patched asleep follow: no intake, as retail)
            } else if self.ent[i].mail[0].1 != 0 {
                let (amt, src) = self.ent[i].mail[0];
                self.ent[i].act_life -= amt as i32;
                self.ent[i].mail[0].1 = 0;
                self.ent[i].f40 = src;
            } else {
                self.ent[i].f40 = 0;
            }
        } else if self.ent[i].f63 & 3 == 0 {
            self.move_relink(i, lx, ly, lz);
            self.ent[i].f30 = self.ent[l].f30;
        }
    }

    /// Model 15's grid-walker movement sub_20480 (:25906): every 8th
    /// phase tick a weighted 4-way heading vote (die on forbidden
    /// terrain); every 16th a lane snap to tile centers; same-model
    /// repulsion; then a gated move (aligned, or a 55% coin).
    /// The vote's 4-entry weight table lives at a code/data alias the
    /// decompile can't express (`*(_DWORD*)sub_1FF40`) — EXTRACTED
    /// from the retail binary (CARPET.EXE obj1 VA 0x1FF40, file
    /// 0x38738: `58 1b 58 1b 0a 00 58 1b`): score = rand % w + 2 per
    /// candidate k = heading + 512k, so straight/right/left run a
    /// huge-range roll while the REVERSAL (k=2, w=10) caps at 11 —
    /// a guard almost never turns around on a free pad. The uniform
    /// stand-in [16,16,16,16] kept the draw COUNT (streams aligned)
    /// but flipped the CHOICE wherever a big roll landed: mc1l0
    /// t=2541, guard 500 reversed onto the vote the free run forked
    /// on — THE 2542 free-run wall.
    fn grid_walk(&mut self, i: usize, base: u8) {
        const WEIGHTS: [u32; 4] = [7000, 7000, 10, 7000];
        let row = BEHAVIOR[self.ent[i].row156 as usize];
        if self.ent[i].f63 % 8 == 0 {
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            if self.cap_bit(x, y) & !row.v_20 != 0 {
                self.ent[i].tick70 = base + 4; // state 94: die on bad ground
                return;
            }
            let v31 = self.ent[i].f30;
            let mut best_score = 1u32;
            for k in 0..4u16 {
                let cand = v31.wrapping_add(512 * k) & 0x7FF;
                let e = &self.ent[i];
                let mut tmp = (e.x, e.y, e.z);
                Self::polar_step(&mut tmp, cand, 0, 256);
                let r = self.ent_rand(i);
                let free = self.cap_bit(tmp.0, tmp.1) & !row.v_20 == 0;
                let score = (r % WEIGHTS[k as usize] + 2) * free as u32;
                if score > best_score {
                    best_score = score;
                    self.ent[i].f30 = cand;
                }
            }
        }
        let e = &self.ent[i];
        let mut tmp = (e.x, e.y, e.z);
        if e.f63 % 16 == 0 {
            match (e.f30.wrapping_sub(256) >> 9) & 3 {
                0 | 2 => tmp.1 = (tmp.1 & !255) + 128,
                _ => tmp.0 = (tmp.0 & !255) + 128,
            }
        }
        // Same-model repulsion (:25984) — the exact lifted twin of
        // mob_pack's :21796 walk, movsx-signed box included (see the
        // pack walk: each coordinate sign-extends BEFORE the 32-bit
        // subtract, so midline-0x8000 straddlers never repel).
        let (ex, ey, id, model) = (e.x, e.y, e.id24, e.model65);
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 5 || c.model65 != model || c.tick70 == 120 || c.act_life < 0 {
                continue;
            }
            if c.id24 == id {
                continue;
            }
            let dx = ((ex as i16 as i32) - (c.x as i16 as i32)).abs();
            let dy = ((ey as i16 as i32) - (c.y as i16 as i32)).abs();
            if dx < 256 && dy < 256 {
                self.ent[i].f34 = Self::angle_between(c.x, c.y, ex, ey);
                break;
            }
        }
        let e = &self.ent[i];
        let aligned = e.f34 == e.f30;
        if aligned || self.ent_rand(i) % 20 <= 10 {
            let e = &self.ent[i];
            let (yaw, speed) = (e.f30, e.f126);
            Self::polar_step(&mut tmp, yaw, 0, speed);
            let ground = self.ground_z(tmp.0, tmp.1) as i16;
            Self::alt_clamp(&mut tmp.2, ground, &row);
            self.move_relink(i, tmp.0, tmp.1, tmp.2);
        }
    }

    /// m15 castle-guard WANDER, state 91 (sub_1FF60 :25662): the grid-
    /// walk movement (sub_20480, always), then — every v_26 ticks while
    /// awake — the wizard-acquisition scan (:25733-64, the shared
    /// sub_19D70 scan A reduced to the player: v_28² range + v_30 cone +
    /// the +16/0x20 invisibility skip + the owner gate). Acquiring the
    /// wizard promotes to the STATIONARY chase (92) and runs the
    /// chase-entry trailer (sub_20410) on that tick. Without this scan
    /// the castle's L3+ archers patrolled forever and never engaged —
    /// rival guards were harmless. HW shares this handler verbatim.
    fn guard_wander(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        self.grid_walk(i, base);
        let v26 = BEHAVIOR[self.ent[i].row156 as usize].v_26;
        if (self.ent[i].f63 as i16) % v26 == 0 && self.ent[i].f58 != 0 {
            if let Some(t) = self.nearest_wizard_target(i, ctx, false, false) {
                self.ent[i].f146 = t;
                self.ent[i].tick70 = base + 2;
            }
        }
        if self.ent[i].tick70 == base + 2 {
            self.guard_enter_chase(i);
        }
    }

    /// sub_20410 (:25885): the guard stops (speed 0) and rolls the alert
    /// sprite — ONE LCG draw, armed 206 on 11/20 else 1.
    fn guard_enter_chase(&mut self, i: usize) {
        let r = self.ent_rand(i);
        self.ent[i].f126 = 0;
        self.set_sprite(i, if r % 20 <= 10 { 206 } else { 1 });
    }

    /// sub_20450 (:25899): leaving the chase restores the walk speed
    /// (f128) and the unarmed sprite.
    fn guard_exit_chase(&mut self, i: usize) {
        self.ent[i].f126 = self.ent[i].f128;
        self.set_sprite(i, 0);
    }

    /// The per-model CHASE-ENTRY trailers, run on the tick the
    /// promotion lands. Retail hangs these off the individual state
    /// handlers, so the coverage is NOT uniform and the `role` gate is
    /// load-bearing: m7's idle slot (sub_1C8F0 :23294) is a bare
    /// shared idle with no trailer, and m15's pack slot (sub_203E0
    /// :25867) a bare shared pack. (m15's idle slot 90 — sub_1FF50, a
    /// bare shared idle too — is kept on the list: nothing in the port
    /// or the original ever parks a guard there, so the extra arm is
    /// unreachable, and the m15 lane measures clean as it stands.)
    /// The per-model CHASE-EXIT trailers — the speed RESTORES. Every
    /// one hangs off its handler's tail as `if (+70 != chase)
    /// <trailer>`, so it fires on the tick the chase breaks for ANY
    /// reason: target lost, out of range, or the creature's own DEATH.
    /// The death case is easy to miss because retail's damage prologue
    /// sits INSIDE each handler and `goto`s the trailer instead of
    /// returning (m9 sub_1DA60 :24184 `goto LABEL_31`; m2/m4/m7 reach
    /// it through sub_1A120's plain `return v15`) — the mc1l5
    /// recording shows it plainly: slot 348 goes act_life -1 at
    /// t=6241 and STILL gets +126 = 20, type 201 and its filter
    /// restored at t=6242.
    fn chase_exit_trailer(&mut self, i: usize, model: u8) {
        match model {
            2 => self.ent[i].f126 = self.ent[i].f128, // sub_1B3C0 :22363-66
            4 => self.militia_disarm(i),              // sub_1BCE0
            7 => {
                // sub_1C960 :23346-55 — the planted pose only.
                if self.ent[i].type86 == 198 {
                    self.set_sprite(i, 85);
                    self.ent[i].f126 = self.ent[i].f128;
                }
            }
            9 => self.m9_disguise(i),       // sub_1DD50
            15 => self.guard_exit_chase(i), // sub_20450
            _ => {}
        }
    }

    fn chase_entry_trailer(&mut self, i: usize, model: u8, role: u8) {
        match (model, role) {
            // m2's lunge arm, the same trailer in three costumes:
            // sub_1B350 :22319-20 (idle, silent), sub_1B370 :22327-31
            // (wander, and ONLY the wander buzzes) and sub_1B4C0
            // :22374-75 (pack, silent). It was reachable only through
            // the two dispatch wrappers, so a bee PROMOTED BY DAMAGE
            // never armed: mc1l42 t=19459 slot 230 and t=21217 slot
            // 222 both take their 400 in the pack slot and retail
            // hands back +26 = 1 where the port kept the running
            // pack counter (79 / 30).
            (2, 1) => {
                self.snd(13, i);
                self.ent[i].f26 = 1;
            }
            (2, 0 | 3) => self.ent[i].f26 = 1,
            (4, 0 | 1 | 3) => self.militia_arm(i), // sub_1BC50
            (7, 1 | 3) => self.ent[i].f26 = 1,     // sub_1C900/sub_1CA00
            (9, 1 | 3) => self.m9_enter_chase(i),  // sub_1DCD0
            (15, 0 | 1) => self.guard_enter_chase(i), // sub_20410
            _ => {}
        }
    }

    /// m15 castle-guard CHASE, state 92 (sub_201D0 :25771): STATIONARY —
    /// face the target every 4th tick (heading f34 only, never a step or
    /// a move core), break back to WANDER (91) on target loss or when
    /// the 3D distance reaches v_28, else fire the (9,13) bolt every
    /// v_26 (the existing m15 thunk — class-9 m13, filter 3/0xFF, launch
    /// z + f84, default 100 damage). Any break runs the exit trailer.
    fn guard_chase(&mut self, i: usize, base: u8, ctx: &MobCtx) {
        let tgt = self.ent[i].f146;
        let (tx, ty, tz, alive) = if let Some(hx) = self.hx_of_tag(tgt) {
            // Same live-record law as the wyvern/genie chases: the
            // human's carpet is a pool record to retail's target-loss
            // test, so a dead human breaks the guard back to WANDER.
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2,
                !self.seat_dead(ctx, hx),
            )
        } else {
            let t = tgt as usize;
            // ⭐ …and the FOURTH copy. `sub_201D0` :25830 forms
            // `v8 = &pool[+146]` with no index or class test, re-bears
            // off `v8 + 72` at :25832-33 and tests `+12 < 0 ||
            // (+17 & 4)` at :25834. A guard handed 0 aims at the
            // ORIGIN and reads the scratch record's life; the port's
            // "use my own position, target lost" stand-in froze it and
            // broke it back to WANDER instead. Bounds only, otherwise.
            if t >= self.ent.len() || (t == 0 && no_m15m16_scratch_target()) {
                let e = &self.ent[i];
                (e.x, e.y, e.z, false)
            } else {
                let c = &self.ent[t];
                (c.x, c.y, c.z, c.act_life >= 0 && c.flags & 0x400 == 0)
            }
        };
        // Aim (heading only, :25832-33).
        if self.ent[i].f63 & 3 == 0 {
            let e = &self.ent[i];
            self.ent[i].f34 = Self::angle_between(e.x, e.y, tx, ty);
        }
        if !alive {
            self.ent[i].tick70 = base + 1; // target lost (:25834)
        } else {
            let e = &self.ent[i];
            let row = &BEHAVIOR[e.row156 as usize];
            if (e.f63 as i16) % row.v_26 == 0 {
                let dz = tz.wrapping_sub(e.z) as i32;
                let sq = Self::dist2_sq(e.x, e.y, tx, ty).wrapping_add(dz.wrapping_mul(dz));
                if Self::isqrt(sq as u32) >= row.v_28 as u32 {
                    self.ent[i].tick70 = base + 1; // out of range (:25841)
                } else {
                    self.attack_thunk(i, 15, tgt, tx, ty, tz, 0, 0);
                }
            }
        }
        // Exit trailer (:25862): restore the walk speed on any break.
        if self.ent[i].tick70 != base + 2 {
            self.chase_exit_trailer(i, 15);
        }
    }

    // ---- dispatch -----------------------------------------------------------

    /// The awake pre-pass sub_54F80 (:64300), run before dispatch:
    /// awake creatures count down (+58, mirrored into their body
    /// segments); asleep ones re-arm to 16 (segments 18) when the
    /// player is within 24 tiles (2D dist² < 0x2400000 — sub_42410
    /// :52748 reads only x/y; altitude never gates waking. remc2's
    /// sub_68C70 uses the same 2D distance, corroborated by the
    /// synchronized remc1 body).
    ///
    /// MANA BALLS ride the same pass on their own list: a ball within
    /// the same 24-tile radius of the HUMAN (:64352 reads the single
    /// local-player index; rivals never wake balls) re-arms +58 = 16
    /// (:64361), so near-wizard mana rolls downhill in a 16-of-17-tick
    /// duty cycle — "the mana runs away when approached" is retail
    /// law. Corpus: mc1hwl0 full-take scan, 7,571 re-arm events, all
    /// writing exactly 16, per-slot period exactly 17, hard cutoff at
    /// 24.0 tiles. +58 is a raw BYTE in retail — the import widens i8
    /// (a fresh ball's 0x80 arrives −128), so the countdown masks.
    ///
    /// ⭐ MEMBERSHIP IS THE CHAIN BUILD, NOT A GUARD INSIDE THE PASS.
    /// The tick head `sub_41780_41AC0` rebuilds the awake chains every
    /// tick (:52246-320) and then calls `sub_54F00_55430` (:64266) —
    /// its ONLY call site (:52325). A class-5 creature joins its model
    /// bucket (`str_36382x[+65]`, :52266) only while `act_life >= 0 &&
    /// +70 != 120`, so the walker's dead arm (:64283-84, `+58 = -6`)
    /// is UNREACHABLE: retail never stamps a dead creature, it simply
    /// stops walking it, and +58 FREEZES at its last live countdown.
    /// (The port used to stamp the byte -6 as 250 here, which the raw
    /// shadow saw as ~1,000 rows/take of `retail 2-12, port 250` — and
    /// a creature that died asleep froze at 0 in retail while the port
    /// read back a nonzero counter through every `f58 != 0` gate.)
    /// The ball list is built separately and keyed on the MODEL, not
    /// the state: class 10 with `+65` in 39..=40 (:52287-96 →
    /// `var_u32_36462[1]`), walked at :64288-93 with NO life gate.
    /// The port used to scope balls to `+70 == 41`, the SETTLED state
    /// — a rolling ball is 42 and retail counts it down all the same
    /// (raw shadow: 275 mc1l42 rows of `retail 249, port 250`, a fresh
    /// ball's allocator -6 that only retail was decrementing).
    #[cfg(test)]
    pub(crate) fn mob_awake_pass(&mut self, ctx: &MobCtx) {
        self.mob_awake_pass_refs(ctx, &[(self.view(ctx.phx).x, self.view(ctx.phx).y)]);
    }

    /// [`Self::mob_awake_pass`] against every human seat's reference
    /// pose (`refs`, one per seat): a creature wakes off the FIRST seat
    /// inside its gate — one seat is retail's own `playerIndex` read.
    pub(crate) fn mob_awake_pass_refs(&mut self, ctx: &MobCtx, refs: &[(u16, u16)]) {
        for i in 1..self.ent.len() {
            let e = &self.ent[i];
            let creature = e.class64 == 5 && e.act_life >= 0 && e.tick70 != 120;
            let ball = e.class64 == 10 && matches!(e.model65, 39 | 40);
            if !creature && !ball {
                continue;
            }
            let v = (e.f58 & 0xFF) as u8;
            if v > 0 {
                let v = v - 1;
                self.ent[i].f58 = v as i16;
                let mut s = self.ent[i].f54 as usize;
                while s != 0 {
                    self.ent[s].f58 = v as i16;
                    s = self.ent[s].f54 as usize;
                }
            } else if e.f59 > 0 {
                self.ent[i].f59 -= 1;
            } else if (ball && ctx.patches.map_wide_ball_rolling && !ctx.strict)
                // The pre-pass REFERENCE poses the walk hands in
                // (`World::mc2_wake_pose`'s phase), not a slot read.
                || refs.iter().any(|&(px, py)| Self::dist2_sq(e.x, e.y, px, py) < self.chassis.awake_gate_sq)
            {
                // Patch option `map_wide_ball_rolling`: the BALL rows
                // re-arm without the 24-tile radius, so every ball
                // rolls to its resting place at retail's own
                // 16-of-17-tick duty cycle instead of visibly "running
                // away" when the human walks into wake range. Balls
                // only — creatures keep the distance gate (this is NOT
                // `awake_range = 0`, which wakes the whole ecology).
                //
                // Retail's wake arm stamps `+48` FIRST (:64353-59,
                // `CARPET.EXE` 0x6D7F8: `push %edx; call
                // Distance_410CE; mov %ax,0x30(%esi)`): `edx` is
                // `sub_42410`'s leftover `dy²`, so the word is the
                // absolute Y-leg to the human carpet, not the
                // distance. Write-only — nothing in the binary reads
                // a creature's or a ball's `+48` — so it lands in the
                // hash-silent `raw48`. Citation and the reader
                // enumeration on [`no_mc1_wake_dy48`].
                if !no_mc1_wake_dy48() {
                    // The waking seat's Y-leg (the first inside the gate;
                    // a map-wide ball takes seat 0's).
                    let (ex, ey) = (self.ent[i].x, self.ent[i].y);
                    let gate = self.chassis.awake_gate_sq;
                    let py = refs
                        .iter()
                        .find(|&&(px, py)| Self::dist2_sq(ex, ey, px, py) < gate)
                        .or(refs.first())
                        .map_or(0, |r| r.1);
                    let dy = py.wrapping_sub(e.y) as i16 as i32;
                    self.ent[i].raw48 = Raw48(Self::isqrt(dy.wrapping_mul(dy) as u32) as u16);
                }
                self.ent[i].f58 = 16;
                self.ent[i].f59 = 0;
                let mut s = self.ent[i].f54 as usize;
                while s != 0 {
                    self.ent[s].f58 = 18;
                    s = self.ent[s].f54 as usize;
                }
            }
        }
    }

    /// Class-5 per-state dispatch (str_254DCC, :4687). Family blocks
    /// of 6 per model: base+0 IDLE, +1 WANDER, +2 CHASE, +3 PACK,
    /// +4 DEATH, +5 CORPSE; state 120 = body segment. Custom family
    /// behavior beyond movement (disguises, mana hunts, house
    /// building, ranged/teleport casters) is the AI/combat track —
    /// those states stand still here; every simplification is flagged
    /// in docs/ROADMAP.md.
    pub(crate) fn creature_tick(&mut self, i: usize, ctx: &MobCtx) {
        let s = self.ent[i].tick70;
        if s == 120 {
            return self.segment_follow(i, ctx.patches.mc1_fix_dragon_tail && !ctx.strict);
        }
        if s > 101 {
            return; // parked states (data10 = 0)
        }
        let base = s - s % 6;
        let model = s / 6;
        let role = s % 6;
        match role {
            4 => {
                return self.mob_death(
                    i,
                    base,
                    ctx.patches.mc1_segment_chain_revalidate && !ctx.strict,
                );
            }
            5 => return self.mob_corpse(i, base),
            _ => {}
        }
        // Per-model wrapper PRE-WORK that retail runs ABOVE its damage
        // prologue. The prologue is not the handler's first act — it
        // lives inside the shared core `sub_1A120` (:21598-21651), so
        // anything a wrapper does before calling that core still lands
        // on hit and death ticks. m2's (:22342-54, the mc1l2 (5,2)
        // family), m8's (:23550-52) and m9's hidden head (:23682-98 —
        // both mc1l42 families) are
        // hoisted here. m6's `+126 = 30` (sub_1C4F0 :23146, likewise
        // the handler's first statement) is the same class of pre-work
        // and is DELIBERATELY still below the intake at its own site —
        // the banked "HIT-ABORT RESTRUCTURE" spec lists it separately
        // and no corpus row has demanded it yet.
        if (model, role) == (2, 2) {
            self.m2_chase_prework(i, ctx);
        }
        if (model, role) == (7, 1) {
            self.m7_wander_prework(i);
        }
        if (model, role) == (7, 2) {
            self.m7_chase_prework(i);
        }
        if (model, role) == (8, 2) {
            self.griffon_chase_prework(i);
        }
        if (model, role) == (9, 1) {
            self.m9_hidden_prework(i);
        }
        // m4 IDLE's walk-in-flag zero is sub_1B5D0's FIRST statement,
        // ABOVE its inline intake (:22482, hw :21039) — so it lands on
        // hit AND death ticks too. Zeroing it inside the idle body
        // (which Hit/Dead intakes never reach) let a militia born into
        // fire die with the ctor stagger (+26 = slot % 100) standing,
        // and sub_1BC10's absorb gate (:22730) swallowed the corpse —
        // no (10,39) ball. mc1hwl0 t=27513-16 slot 777: popped by a
        // torched house, hit on its birth tick, dead two later; retail
        // corpses at state 29 and mints ball 876 (f140=500).
        if (model, role) == (4, 1) {
            self.ent[i].f26 = 0;
        }
        // The damage inbox block opening every live state handler
        // (:21330-81): apply pending damage, dispatch death/aggro.
        // Families 8/12/13/14 mark the attacker instead of chasing
        // (:25057-63 — the "under attack" memory, wizard-AI track).
        //
        // ...except it does NOT open every live state handler, and a
        // state without it is DAMAGE-DEAF (see `state_is_damage_deaf`).
        // …and a fifth, by DATA rather than by state: a pack member
        // with no leader leaves `sub_1A390` at its first statement
        // ([`no_mc1_leaderless_pack_deaf`]). m5 / m12 / m13 / m14 own
        // their pack slot and never reach that routine.
        let leaderless_pack = role == 3
            && !matches!(model, 5 | 12 | 13 | 14)
            && self.ent[i].f52 == 0
            && !no_mc1_leaderless_pack_deaf();
        let intake = if self.state_is_damage_deaf(model, role) || leaderless_pack {
            Inbox::Quiet
        } else {
            self.inbox(i)
        };
        match intake {
            Inbox::Dead => {
                if role == 3 {
                    // Pack-member death (:21746): the leader retargets
                    // the killer and rejoins the hunt.
                    //
                    // ⚠ THE HANDOFF WRITES THROUGH `+52` BLIND. Retail
                    // takes `v3x = &pool[a1x->+52]` (:21702) with no
                    // class, model, life or flags test — the only gate
                    // on the whole path is `+52 != 0` (:21695) — and
                    // then stamps whatever record now occupies that
                    // slot. The port's `class64 == 5` conjunct was
                    // invented, and it cost mc1l2 its last graded row:
                    // at t=8290 the militiaman at slot 285 dies still
                    // pointing at slot 287, which had been reaped and
                    // re-minted as a `(10,0)` FIRE. Retail stamps the
                    // fire — `+146 = 295` (the human), `+52 = 0`,
                    // `+70 = 24+2 = 26` — and the ascending walk then
                    // reaches 287 and dispatches it as class-10 state
                    // 26 (`sub_263C0` :28949: `+26++`, pre-decrement,
                    // soft kill) instead of a fire. `f26 = 1` is that
                    // dispatch, not a fourth write.
                    //
                    // The bound is memory safety, not behaviour:
                    // retail indexes a fixed 1000-record C array with a
                    // raw u16 and would read garbage where the port's
                    // `Vec` would panic. Do NOT re-introduce a
                    // behavioural test on the RETAIL arm — the patched
                    // arm's test lives in `pack_partner_retarget`.
                    //
                    // :21746 reads `+40`; the lethal branch of the
                    // inbox has just copied it into `+38` (:21737-38),
                    // so the two are equal here.
                    let killer = self.ent[i].f40;
                    self.pack_partner_retarget(
                        i,
                        base,
                        killer,
                        ctx.patches.mc1_segment_chain_revalidate && !ctx.strict,
                    );
                }
                // Killing village folk puts the wizard on the wanted
                // list (m12 :25291, m13 :25459, m14 :25638) — and so
                // does killing a griffon (sub_1CF60 :23578-80): the
                // flock avenges it. NO m4 site exists — the +528=200
                // census finds death arms for 8/12/13/14 alone, and a
                // MILITIA death arms nobody (mc1l32 t=45218: three
                // militia burn with f38=14 and retail's wanted stays
                // 0; the port's invented "m4 corpse analog" armed the
                // human and the 45231/45249 pack scans acquired a
                // carpet retail never marked).
                //
                // ⚠ NONE OF THE FOUR IS ON THIS TICK. All four writes
                // are per-model DEATH/CORPSE STATE handlers (0x34,
                // 0x4D, 0x52, 0x59) — see [`Gen::mob_death`] and
                // [`Gen::mob_corpse`]. Arming here read `+38` one tick
                // too early, at the one moment the prologue itself has
                // just clobbered it: the `else { +40 = 0 }` above
                // (:21341-44) then `+38 = +40` (:21363-66) wipes the
                // killer stamp of every OUT-OF-BAND kill — the castle
                // footprint sweep `sub_40E20` (:51751-58) writes
                // `life = -1, +38 = +40 = owner` directly — so the
                // arm fired with `+38 = 0` and marked nobody. Retail's
                // handler runs a tick later, after the painter's next
                // pass has RE-STAMPED the credit.
                if no_mc1_wanted_death_state() && matches!(model, 8 | 12 | 13 | 14) {
                    self.flag_village_wanted(self.ent[i].f38);
                }
                self.ent[i].tick70 = base + 4;
                // Dying IS leaving the chase: retail's prologue falls
                // through to the handler's own exit trailer on this
                // very tick.
                if role == 2 {
                    self.chase_exit_trailer(i, model);
                }
                // The m0 wrappers (sub_1B070/1B090/1B0E0) run the
                // z-bob sub_1B120 as an UNCONDITIONAL tail — the
                // death-transition tick still rises (mc1l1 t=3810:
                // the killed worm head bobs +130 while the prologue
                // demotes state 2 → 4; the hold starts one tick
                // later, in the death state itself).
                if model == 0 && matches!(role, 1..=3) {
                    self.flyer_bob(i);
                }
                // ...and so does the vulture's IDLE mover: sub_1B160
                // (:22227-28, CARPET.EXE calls at 0x1B168/0x1B171) is
                // `sub_19B10(a1x, 6); sub_196E0(a1x);` with no test
                // between the two calls, so the core's lethal exit
                // (`sub_424F0(a1x, 6 + 4)` :21415) returns into a wrapper
                // that still steps, turns and ground-snaps the bird;
                // only the re-aim after it is gated on `+70 == 6`
                // (0x1B17C `cmp ah,6`), which `m1_idle_trailer`
                // mirrors with its `tick70 == base` test. mc1l35
                // t=4522 slot 65: retail moves 47087/46736/1259 →
                // 47046/46684/1258 and turns 1832 → 1854 on the very
                // tick `+70` goes 6 → 10; the port held the pre-tick
                // pose. The HIT arm below has run this trailer since
                // mc1l32 t=31567 — the DEATH arm is the same wrapper.
                if (model, role) == (1, 0) && !no_mc1_vulture_death_mover() {
                    self.m1_idle_trailer(i, base);
                }
                // ...and m5's regen trailer runs on the DEATH tick
                // too: the fatal hit returns out of the shared core
                // back into sub_1BF60/sub_1C110, whose unconditional
                // `if (act < max) act += max >> 7` then credits the
                // fresh corpse (act is deep negative, trivially
                // < max). mc1l32 pins it four ways — every crab death
                // overshoot reads |port| high by exactly max >> 7:
                // t=29840 slot 324 retail -1007 = -1124 + 117
                // (max 15000), t=27365 slot 323 Δ78 (10000), t=27371
                // slot 115 Δ39 (5000), t=30954 slot 63 Δ312 (40000).
                if model == 5 && matches!(role, 1 | 2) {
                    self.m5_regen(i);
                }
                return;
            }
            // ⭐ THE CASTLE GUARD'S CHASE IS NOT ON THE SHARED
            // PROLOGUE. m15 state 92 is `sub_201D0` (:25771), and its
            // mailbox block (:25789-825) is its OWN, inline: it debits
            // `actLife`, latches `+40`, walks the `+54` chain — and
            // then branches on `v2 > 1` ONLY. A non-lethal hit (v2 ==
            // 1) falls straight through to the aim at :25832 and the
            // bolt at :25845, and there is NO retarget arm anywhere in
            // the function: the guard keeps the target it had.
            //
            // Both halves of the shared prologue are therefore wrong
            // here — the blanket `return` (which freezes the every-4th-
            // tick re-bear and swallows the bolt) and the class-3
            // retarget. mc1l49 t=4655 slot 114 is the receipt: the
            // guard takes 800 (mail0 800 from 331, act_life 1000 → 200,
            // `+40` 0 → 331) on a tick whose `+63` is 12, and retail
            // still re-bears `+34` 1256 → 1259 while keeping `+146` at
            // 907. The port held 1256 — the take's whole horizon.
            //
            // m15's OTHER slots stay on the shared path and are right
            // as they stand: the WANDER `sub_1FF60` (:25718-27) does
            // `if (v1) { … goto LABEL_34 }` — freeze — with the class-3
            // retarget, and the PACK `sub_203E0` (:25869) is a bare
            // `sub_1A390(a1x, 0x5A)`.
            Inbox::Hit(_) if (model, role) == (15, 2) && !no_m15_chase_hit_fallthrough() => {}
            Inbox::Hit(src) => {
                // The "under attack" mark the villager families write
                // instead of chasing (m12 :25057-63, m13/m14 twins)
                // feeds the wanted timer. m8 is NOT among them: the
                // griffon's hit prologue (:23446-58) tests the
                // attacker for class 3 and goes straight to chase —
                // no +528 write anywhere in it (its +528 arms are the
                // beam emission :23560 and the death avenge :23580).
                // With rival wanted never decaying, the invented m8
                // arm here held wiz 1 wanted through mc1hwl0
                // t=20677-20690 where retail reads 0.
                if matches!(model, 12 | 13 | 14) {
                    self.flag_village_wanted(src);
                }
                // The villager families' hit tick ends HERE: retail's
                // movement core + wander sit in the ELSE of the
                // damage test (m12 :25057-67, m13/m14 twins), so the
                // marked tick freezes the walker — no move, no turn,
                // no wander draws (mc1l0 t=5026: settler 640 holds
                // (246.35,237.62) heading 1412 on the tick the fire's
                // 400 lands; the fall-through walked and re-aimed).
                if matches!(model, 12 | 13 | 14) {
                    return;
                }
                // m8 DOES retaliate — its IDLE promotes a hit-by-
                // wizard griffon straight into attack (sub_1CA50
                // :23455-58); only the villager families merely mark
                // the attacker (:25057-63) without chasing.
                //
                // The m9 mound's HIDDEN slot is the one prologue in
                // the family with NO class gate: sub_1D060 :23732-38
                // and its buried twin sub_1D6D0 :24004-07 both do a
                // bare `+146 = +40; state 0x38`, where everything
                // sharing sub_19B10/sub_1A120 first tests the
                // attacker's class for 3 (and m9's own CHASE prologue
                // :24177-79 keeps that test). So a lurking mound turns
                // on ANY attacker, a militiaman included — mc1l5
                // t=4655 slot 819 takes 250 and retaliates onto the
                // class-5 model-4 in slot 776, which the port, gated
                // on wizards, simply ignored.
                let mound_hidden = model == 9 && role == 1;
                if (mound_hidden || self.attacker_is_wizard(src)) && !matches!(model, 12 | 13 | 14)
                {
                    match role {
                        0 | 1 => {
                            self.ent[i].f146 = src;
                            if model == 11 {
                                // The genie's retaliation blinks
                                // ahead of the attacker (sub_1DFE0
                                // :24459-62 → sub_1E770).
                                self.genie_ambush(i, base, ctx);
                            } else {
                                self.ent[i].tick70 = base + 2;
                                // Every family with a chase-entry
                                // trailer runs it on the retaliate path
                                // too: retail's damage prologue lives
                                // INSIDE each handler and falls through
                                // to the handler's own trailer (m4
                                // sub_1B5D0 :22522 → :22689, m15
                                // sub_1FF60 LABEL_33 → LABEL_34, and
                                // the m7/m9 twins).
                                self.chase_entry_trailer(i, model, role);
                            }
                            // NO `return` — every arm here falls out to
                            // the wrapper TRAILERS below. Retail's
                            // prologue exits are plain returns back INTO
                            // the wrapper, so a hit that PROMOTES the
                            // creature still runs the tail its wrapper
                            // holds (mc1l5 t=5475, mc1l32 t=19430).
                            if no_hit_trailers() {
                                return;
                            }
                        }
                        2 => {
                            // CHASE just retargets (:21639-41) — except
                            // the kraken, whose own chase handler resets
                            // its dive clock on the wizard-hit branch
                            // first: sub_1C4F0 :23192-94 writes
                            // `+26 = -10` beside the retarget, ten ticks
                            // of quiet before the next surface wake
                            // (mc1l42 t=6263 slot 183: retail -10, port
                            // -57 — the pair's only unexplained row).
                            if model == 6 && !no_hit_trailers() {
                                self.ent[i].f26 = -10;
                            }
                            self.ent[i].f146 = src;
                            if no_hit_trailers() {
                                return;
                            }
                        }
                        3 => {
                            // PACK: leader and member both retarget
                            // (:21755-65) — the SAME blind write
                            // through `+52` as the lethal arm above,
                            // and retail clears the PARTNER's `+52`
                            // (:21758) as well as its own.
                            self.pack_partner_retarget(
                                i,
                                base,
                                src,
                                ctx.patches.mc1_segment_chain_revalidate && !ctx.strict,
                            );
                            self.ent[i].f146 = src;
                            self.ent[i].f52 = 0;
                            self.ent[i].tick70 = base + 2;
                            // `mc1_pack_speed_restore`: the hit takes
                            // the follower out of the pack too — own
                            // max back BEFORE the entry trailer, so
                            // m4's arm (speed 0) still wins.
                            if ctx.patches.mc1_pack_speed_restore && !ctx.strict {
                                self.ent[i].f126 = self.ent[i].f128;
                            }
                            // The per-model PACK wrappers trail the
                            // shared sub_1A390 the same way (m4
                            // :22724-25, m7 :23362-64, m9 :24219-20).
                            self.chase_entry_trailer(i, model, role);
                            if no_hit_trailers() {
                                return;
                            }
                        }
                        _ => {}
                    }
                }
                // The shared prologues freeze EVERY hit tick, not just
                // the villager families': idle (:21365-80), chase
                // (:21673-77) and pack (:21745-52) all return out of
                // the `if (v4)` arm with the move/aim/wander body
                // never reached — a non-wizard hit (fire, crush) holds
                // the walker in place while the damage lands (mc1l1
                // t=5450: vulture 200 takes a 400 fire hit mid-pack
                // and retail's boundary shows position, heading and
                // aim all frozen; the old wizard-only scoping let the
                // port walk on). The wizard-attacker arms above are
                // freezes too — none of them moves.
                //
                // THE WRAPPER TRAILERS BELOW ARE NOT PART OF THAT
                // FREEZE, and they run on EVERY path through this arm,
                // the promote-and-retarget ones included: retail's
                // prologue exits are plain returns out of the shared
                // CORE (sub_19B10/19D70/1A120/1A390) back into the
                // per-model WRAPPER, which then runs its tail whatever
                // the core did. The m0 bob is the first of them
                // (sub_1B070/1B090/1B0E0 :22172-90 all end
                // `sub_1B120(a1x);` with no guard) — mc1l5 t=5475 slot
                // 271, a worm promoted by wizard 650's 400, still bobs
                // `z += +26` / `+26 -= 5` (retail f26 20 z 3083, port
                // 25 / 3058).
                if model == 0 && matches!(role, 1..=3) {
                    self.flyer_bob(i);
                }
                // m1's IDLE wrapper trails the shared idle with the
                // MOVER (sub_1B160 :22228 — `sub_19B10(a1x, 6);
                // sub_196E0(a1x);`), textually below the core exactly
                // like the m0 bob, and the re-aim after it is gated on
                // the bird still being idle so a promoting hit skips
                // only the re-aim. mc1l32 t=31567 slot 28: the vulture
                // takes 1000 from wizard 14 in state 6 and retail still
                // steps it 178 units (its own +126) and turns
                // 1938 → 1960 while the port left it bit-identical.
                if (model, role) == (1, 0) && !no_hit_trailers() {
                    self.m1_idle_trailer(i, base);
                }
                // ...and m5's REGEN is a wrapper trailer too, so it
                // survives the freeze exactly like the bob: retail's
                // sub_1BF60 (:22959-65) and sub_1C110 (:22976-82) call
                // the shared handler and THEN run
                // `if act < max { act += max >> 7 }` unconditionally —
                // the abort happens inside sub_1A120, below them. This
                // is the banked HIT-ABORT RESTRUCTURE's item 4 ("a hit
                // skips the CORE; wrapper pre-work and TRAILERS still
                // run"), and it proves the port's blanket return
                // OVER-aborts. mc1l32 t=23132: 16 crabs in state 32
                // take the blast ring's 800 and retail freezes their
                // movement exactly as the port does, yet every one of
                // them still lands its regen — retail sits above the
                // port by precisely `max_life >> 7` (39 / 78 / 117 for
                // max 5000 / 10000 / 15000). It rides the WIZARD path
                // too, now that that path falls through: mc1l32
                // t=19430 slot 323, retail 4658 vs port 4619.
                // The DEATH arm runs the same trailer (see
                // `Inbox::Dead` above) — retail's wrapper tail is
                // unconditional, so it credits the fresh corpse; the
                // l32 death-overshoot family is the receipt.
                if model == 5 && matches!(role, 1 | 2) {
                    self.m5_regen(i);
                }
                // ...and so is the militia CHASE wrapper's wanted-timer
                // cadence (sub_1BB20 :22703-14) — the tail sits below
                // `sub_1A120` in the wrapper, so every non-lethal exit
                // the core's damage prologue takes still reaches it.
                // See [`Gen::militia_chase_wanted_tail`].
                if (model, role) == (4, 2) && !no_hit_trailers() && !no_m4_chase_wanted_trailer() {
                    self.militia_chase_wanted_tail(i, base);
                }
                // ...and so is the wyvern WANDER wrapper's house hunt
                // (sub_20710 :26033-58): `sub_19D70(a1x, 96)` returns
                // out of its damage prologue and the wrapper tests
                // `+70 == 97` and nothing else, so a non-promoting hit
                // tick still elects the nearest dwelling. mc1l34
                // t=14669 slot 8. See [`no_mc1_wyvern_house_hunt_on_hit`].
                if (model, role) == (16, 1)
                    && self.ent[i].tick70 == base + 1
                    && !no_hit_trailers()
                    && !no_mc1_wyvern_house_hunt_on_hit()
                {
                    self.wyvern_house_hunt(i, base);
                }
                return;
            }
            Inbox::Quiet => {}
        }
        // The kraken pins its speed on every movement tick, but the
        // three slots do it at DIFFERENT points: the chase (sub_1C4F0
        // :23146) writes it as its first statement, while the wander
        // (sub_1C4A0 :23118) and the pack (sub_1C880 :23276) write it
        // as their LAST — which is what keeps m6 out of the pack
        // catch-up's reach, since :21814's inflated +126 is stamped
        // back to 30 before the tick ends instead of being left
        // standing for the follower's next read.
        if model == 6 && role == 2 {
            self.ent[i].f126 = 30;
        }
        match (model, role) {
            // -- m4, the VILLAGE MILITIA (the "mimic" reading was
            // half the story): stand-and-shoot with the +528 wanted-
            // timer hostility gate, armed/unarmed sprite swaps and
            // the walk-back-into-a-house exit. State 24 (sub_1B5A0
            // :22428) is the shared idle plus the promotion arm; the
            // chase breaks to 25, not here. State 27 = the pair-up
            // pack (sub_1BBE0), which arms on promotion the same way.
            (4, 0) => {
                self.mob_idle(i, base);
                if self.ent[i].tick70 == base + 2 {
                    self.militia_arm(i);
                }
            }
            (4, 1) => self.militia_idle(i, base, ctx),
            (4, 2) => self.militia_chase(i, base, ctx),
            (4, 3) => {
                self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict);
                if self.ent[i].tick70 == base + 2 {
                    self.militia_arm(i); // sub_1BBE0 :22724-25
                }
            }

            // -- idles --
            // m5's spawn state falls straight through to wander
            // (:22775); m9 = the materialize sequence; m12's idle
            // slot 72 = the BUILD state; m11 = the blink cycle;
            // m13/14/15 idles are custom/parked nops.
            (5, 0) => self.ent[i].tick70 = base + 1,
            (9, 0) => self.m9_emerge(i),
            (11, 0) => self.genie_idle(i, base),
            (12, 0) => self.m12_build(i),
            (13 | 14 | 15, 0) => {}
            // m2's idle wrapper sub_1B350 (:22316): shared idle, then
            // the acquisition-lunge arm.
            (2, 0) => {
                self.mob_idle(i, base);
                self.m2_lunge_arm(i, base, false);
            }
            // THE VULTURE IS THE ONLY CREATURE THAT MOVES WHILE IDLE.
            // Its wrapper sub_1B160 (:22222-46) calls the shared idle
            // and then `sub_196E0` — the mover — as a wrapper TRAILER,
            // then re-aims. Ten idle wrappers exist and this is the
            // only one that moves: the other nine (bases 0/12/18/24/
            // 36/42/48/60/96) are 3-5 line bodies with no mover at
            // all. It is also the only one that MAKES SENSE, m1 being
            // a glider that cannot hover. remc1hw :20779-801 is
            // byte-identical, so this is two witnesses, not one.
            //
            // mc1l32 t=23132 slot 28 caught it: retail stepped the
            // bird 98 units — exactly its own `f126` — along
            // `f30 = 1288` with ZERO LCG draws, while the port left it
            // bit-identical. (The paired `target_yaw` row on slot 31
            // is the downstream knock-on: `f52 = 28`, so its
            // bearing-to-leader moved only because the leader did.)
            // The shared idle `sub_19B10` :21311-419 ends at the pack
            // scan with no mover, so `mob_idle` was never the culprit.
            //
            // The mover sits in the WRAPPER, textually after
            // `sub_19B10` returns, so retail runs it on a HIT tick too
            // — the same trailer-survives-the-freeze shape the m5 regen
            // row proves. `m1_idle_trailer` is therefore shared with
            // the `Inbox::Hit` arm (mc1l32 t=31567).
            (1, 0) => {
                self.mob_idle(i, base);
                self.m1_idle_trailer(i, base);
            }
            (_, 0) => self.mob_idle(i, base),

            // -- wanders --
            (0, 1) => {
                self.mob_wander(i, base, ctx);
                self.flyer_bob(i);
            }
            // m5, the crab: mana-hunting wander + EAT in the family's
            // pack slot (state 0x21) + regen — growth feeds straight
            // into the mana-scaled multishot.
            (5, 1) => self.m5_wander(i, base, ctx),
            (5, 3) => self.m5_eat(i, base),
            (5, 2) => {
                self.mob_chase(i, base, ctx);
                self.m5_regen(i);
            }
            (9, 1) => self.m9_hidden(i, base, ctx),
            // m7's wander wrapper sub_1C900 (:23300): the shared scan,
            // and the promotion tick arms the dug-in timer.
            (7, 1) => {
                self.mob_wander(i, base, ctx);
                self.m7_arm(i, base);
            }
            (11, 1) => self.genie_wander(i, base, ctx),
            (15, 1) => self.guard_wander(i, base, ctx),
            // The villager families' custom hunts.
            (12, 1) => self.m12_wander(i),
            (13, 1) => self.feeder_wander(i, base, false),
            (14, 1) => self.feeder_wander(i, base, true),
            // Every remaining model runs the shared awake-gated
            // two-scan — the engine has no per-model aggro list. m8's
            // CHASE promotion is gated on the target wizard's wanted
            // timer inside `mob_wander` (sub_1CA50 :23500 — the griffon
            // stays peaceful until a village marks the wizard); m16
            // layers the house hunt on top of the shared scans
            // (sub_20710 :26033) when it is still wandering afterwards.
            // m2's wander wrapper sub_1B370 (:22324-32): the shared
            // scan, and the promotion tick buzzes AND arms the lunge.
            (2, 1) => {
                self.mob_wander(i, base, ctx);
                self.m2_lunge_arm(i, base, true);
            }
            // m1's wander wrapper sub_1B200 (:22260-88): the shared
            // scan, then the vulture's GRAVE hunt as a trailer.
            (1, 1) => {
                self.mob_wander(i, base, ctx);
                self.m1_grave_hunt(i, base);
            }
            (_, 1) => {
                self.mob_wander(i, base, ctx);
                if model == 16 && self.ent[i].tick70 == base + 1 {
                    self.wyvern_house_hunt(i, base);
                }
            }

            // -- chases --
            (0, 2) => {
                self.mob_chase(i, base, ctx);
                self.flyer_bob(i);
            }
            (2, 2) => self.bee_chase(i, base, ctx),
            (8, 2) => self.griffon_chase(i, base, ctx),
            (7, 2) => self.m7_chase(i, base, ctx),
            (9, 2) => {
                // sub_1DA60: the pose and the ROOTED speed are set by
                // the entry trailer, not re-stamped per tick; leaving
                // the chase restores both (:24211-12).
                self.mob_chase(i, base, ctx);
                if self.ent[i].tick70 != base + 2 {
                    self.chase_exit_trailer(i, 9);
                }
            }
            (11, 2) => self.genie_chase(i, base, ctx),
            // m12's chase slot 74 = the house APPROACH.
            (12, 2) => self.m12_approach(i),
            (15, 2) => self.guard_chase(i, base, ctx),
            (16, 2) => self.wyvern_chase(i, base, ctx),
            (_, 2) => {
                self.mob_chase(i, base, ctx);
            }

            // -- packs --
            (0, 3) => {
                self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict);
                self.flyer_bob(i);
            }
            // m12's pack slot 75 = the house SEEK; m13/m14's pack
            // slots stay parked (unreferenced in the trace).
            (12, 3) => self.m12_seek(i),
            (13, 3) | (14, 3) => {}
            // m2's pack wrapper sub_1B4C0 (:22371-76): silent arm.
            (2, 3) => {
                self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict);
                self.m2_lunge_arm(i, base, false);
            }
            // m7's pack wrapper sub_1CA00 (:23359).
            (7, 3) => {
                self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict);
                self.m7_arm(i, base);
            }
            // m9's pack wrapper sub_1DC80 (:24216).
            (9, 3) => {
                self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict);
                if self.ent[i].tick70 == base + 2 {
                    self.m9_enter_chase(i);
                }
            }
            (_, 3) => self.mob_pack(i, base, ctx.patches.mc1_pack_speed_restore && !ctx.strict),

            _ => unreachable!(),
        }
        // The kraken's wander and pack slots pin the speed on the way
        // OUT (sub_1C4A0 :23118 / sub_1C880 :23276) — see the chase's
        // pre-write above.
        if model == 6 && matches!(role, 1 | 3) {
            self.ent[i].f126 = 30;
        }
    }
    /// Multipart spawns — worms m0 (sub_38030 :44570) / m3 (sub_384B0
    /// :44799) with 16 segments, kraken m6 (sub_389E0 :45015) with 2.
    /// Segments are byte-copies of the head (inheriting its LCG state,
    /// no draws of their own) at state 120, chained via +52 (toward
    /// head) / +54 (toward tail).
    fn spawn_worm(&mut self, model: u16, x: u16, y: u16, z: i16) -> Option<usize> {
        // :44586 (m0) / :45028 (m6): guard on 16 free pool slots.
        // m3's ctor sub_384B0 has NO guard (:44815 goes straight to
        // NewEvent; the HW sibling agrees) — under pool pressure
        // retail raises an m3 head with a partial chain where a
        // blanket-guarded port raised nothing, shifting every later
        // slot allocation.
        if model != 3 && self.free.len() < 16 {
            return None;
        }
        let (state, max_speed, row, head_type): (u8, i16, u8, u16) = match model {
            0 => (1, 80, 12, 40),
            3 => (19, 64, 15, 88),
            _ => (37, 80, 18, 49),
        };
        let seg_count = if model == 6 { 2 } else { 16 };

        let head = self.new_event()?;
        let ordinal = self.spawn_count[model as usize];
        {
            let e = &mut self.ent[head];
            e.class64 = 5;
            e.model65 = model as u8;
            e.tick70 = state;
            e.max_life = 9000;
            e.f126 = 30;
            e.f128 = max_speed;
            e.f130 = 16;
            e.row156 = row;
            e.f66 = 3;
            e.f28 = 1;
            // sub_36F90 then the explicit pool writes (:44601-:44605):
            e.f136 = 4500;
            e.f140 = if model == 6 { 1500 } else { 2250 };
        }
        let facing = (self.ent_rand(head) & 0x7FF).wrapping_sub(1) as u16;
        self.spawn_facing(head, facing);
        let v26 = BEHAVIOR[row as usize].v_26;
        {
            let e = &mut self.ent[head];
            e.f26 = (head % 100) as i16;
            e.f63 = ordinal;
            e.f58 = if model == 6 {
                64
            } else {
                v26 - (ordinal as i16 % v26) + 4
            };
            // ⭐ THE KRAKEN HEAD STAMPS `+56 = 96` LIKE THE WORMS DO
            // (round 154, w154e). All three multipart ctors write it
            // on the HEAD: m0 `sub_38030` :44615, m3 `sub_384B0`
            // :44842 and m6 `sub_389E0` :45057 — `CARPET.EXE` file
            // 0x512A4 (VA 0x38AAC) `66 C7 43 38 60 00` = `mov word
            // [ebx+0x38],0x60`, between the `+36 = 0` and `+28 = 1`
            // stores. The segments then `qmemcpy` the head and
            // overwrite their own `+56` with `4·+80` (:45086), so the
            // head's 96 is never read back — `+56` is an unnamed
            // `stub2eb` in the record and the only class-5 reader in
            // the whole listing is the segment follow's `-+56` at
            // :21125, on the SEGMENT's own copy. A write-only ctor
            // stamp: it moves no graded lane (mc1l15 / mc1l49 hold
            // END either way) and only the raw shadow's `(5,6) f56`
            // lane (852,694 free-run rows / 15 takes in round 153's
            // census; mc1l49 28,342 rows / 28 slots, mc1l15 268 / 2).
            // `MGC_NO_MC1_KRAKEN_HEAD_F56=1` restores the old skip.
            if model != 6 || !no_mc1_kraken_head_f56() {
                e.f56 = 96;
            }
        }
        self.spawn_count[model as usize] = ordinal.wrapping_add(1);

        let mut prev = head;
        for si in 0..seg_count {
            let Some(seg) = self.new_event() else { break };
            // qmemcpy(seg, head, 164) — retail re-establishes only the
            // chain/anim fields below; +24 KEEPS the head's id (the
            // mc1l0 corpus: every segment carries the head slot),
            // which is also what keeps the kill credit head-only
            // (mob_death's `id24 == own slot` test).
            self.ent[seg] = self.ent[head];
            self.ent[seg].flags &= !4; // not yet placed
            self.ent[seg].f52 = prev as u16;
            self.ent[prev].f54 = seg as u16;
            self.ent[seg].f54 = 0;
            self.ent[seg].tick70 = 120;
            match model {
                // Decompile-literal m0 quirk (:44644): the write lands
                // on the HEAD's +140, not the segment's.
                0 => self.ent[head].f140 = self.ent[head].f136 / 32,
                3 => self.ent[seg].f140 = self.ent[head].f136 / 32,
                _ => self.ent[seg].f140 = self.ent[head].f136 / 3,
            }
            self.ent[seg].f63 = si as u8;
            let seg_type = match model {
                0 => 19 + si as u16,
                3 => 89 + si as u16,
                _ => {
                    if si == 0 {
                        50
                    } else {
                        193
                    }
                }
            };
            self.set_sprite(seg, seg_type);
            self.ent[seg].f56 = if model == 6 {
                4 * self.ent[seg].f80
            } else {
                self.ent[seg].f80
            };
            self.link(seg, x, y, z);
            self.refill_life(seg);
            prev = seg;
        }

        self.link(head, x, y, z); // m6 calls this twice; guarded no-op
        self.refill_life(head);
        self.set_sprite(head, head_type);
        Some(head)
    }
}

#[cfg(test)]
mod tests {
    use super::{MobCtx, PLAYER_TARGET};
    use crate::chassis::ChassisParams;
    use crate::engine::features::{FeatureAssets, Gen, Planes};
    use crate::patches::WorldPatches;
    use crate::verbs::VerbSet;

    /// A flat MC1 `Gen` — the mob state machine needs terrain under it
    /// (`creature_move`) and nothing else. Same shape as
    /// `mc2::stagevars::tests::flat_gen`.
    fn flat_gen() -> Gen {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        Gen::new(planes, assets, 1, ChassisParams::MC1, VerbSet::MC1)
    }

    fn ctx(g: &mut crate::engine::features::Gen) -> MobCtx {
        *g.view_mut(crate::engine::world::Hx::FIRST) = crate::engine::features::SeatView {
            x: 200 << 8,
            y: 200 << 8,
            z: 100,
            ..Default::default()
        };
        MobCtx {
            phx: crate::engine::world::Hx::FIRST,
            strict: false,
            patches: WorldPatches::RETAIL,
            mc2_turn: 0,
        }
    }

    /// Round 167: `sub_1E810` (remc1hw sub_main.cpp:23308) walks the
    /// TICK-TOP BALL CHAIN and tests `+65 == 39` alone (HIDDEN.EXE file
    /// 0x37249), and the destroy it calls only sets `+17 |= 4`. A ball
    /// eaten this tick is still on the chain, so the next genie in reach
    /// takes its whole `+140` again and mints its own `(10,0)` puff.
    /// WITNESS mc1hwl17 t=45101 (genies 9, 10, 11 on ball 442, 500 each)
    /// and mc1hwl20 t=23717. A ball born AFTER the rebuild is on no
    /// chain and feeds nobody. `MGC_NO_MC1_GENIE_EAT_CHAIN=1` fails the
    /// second genie's assert (the pool scan skips the `0x400` ball).
    #[test]
    fn one_ball_feeds_every_genie_that_reaches_it_in_the_tick() {
        let mut g = flat_gen();
        let a = g.spawn_creature(11, 100 << 8, 100 << 8, 100).unwrap();
        let b = g.spawn_creature(11, 100 << 8, 101 << 8, 100).unwrap();
        for &i in &[a, b] {
            g.ent[i].f136 = 20_000;
            g.ent[i].f140 = 10_000;
        }
        let ball = g.new_event().unwrap();
        {
            let e = &mut g.ent[ball];
            e.class64 = 10;
            e.model65 = 39;
            e.x = (100 << 8) + 64;
            e.y = 100 << 8;
            e.f140 = 500;
            e.f144 = PLAYER_TARGET;
        }
        g.rebuild_ball_chain();
        let puffs = |g: &Gen| {
            g.ent
                .iter()
                .filter(|e| e.class64 == 10 && e.model65 == 0)
                .count()
        };
        g.genie_eat_ball(a);
        assert_eq!(g.ent[a].f140, 10_500, "the first genie eats the ball");
        assert_ne!(g.ent[ball].flags & 0x400, 0, "the ball is soft-killed");
        assert_eq!(g.ent[ball].f144, 0, "and unclaimed");
        assert_eq!(puffs(&g), 1);
        g.genie_eat_ball(b);
        assert_eq!(
            g.ent[b].f140, 10_500,
            "the soft-killed ball is still on the tick-top chain"
        );
        assert_eq!(puffs(&g), 2, "each eat mints its own puff");

        // A ball minted after the rebuild is on no chain.
        let mut g = flat_gen();
        let a = g.spawn_creature(11, 100 << 8, 100 << 8, 100).unwrap();
        g.ent[a].f136 = 20_000;
        g.ent[a].f140 = 10_000;
        g.rebuild_ball_chain();
        let late = g.new_event().unwrap();
        {
            let e = &mut g.ent[late];
            e.class64 = 10;
            e.model65 = 39;
            e.x = (100 << 8) + 64;
            e.y = 100 << 8;
            e.f140 = 500;
        }
        g.genie_eat_ball(a);
        assert_eq!(g.ent[a].f140, 10_000, "a mid-tick ball is invisible");
    }

    /// Round 167 ([`super::no_mc1_leaderless_pack_deaf`]): `sub_1A390`
    /// opens on `if (!+52) { state = base + 1; return; }` (HIDDEN.EXE
    /// file 0x32d92), ABOVE its mail read and its `actLife < 0` test.
    /// A pack member with no leader is demoted to wander and the
    /// pending hit waits for the wander prologue. WITNESS mc1hwl10
    /// t=21968 slot 579. `MGC_NO_MC1_LEADERLESS_PACK_DEAF=1` fails the
    /// state assert (the prologue debits 1200 and corpses the record).
    #[test]
    fn a_leaderless_pack_member_is_demoted_before_it_can_be_hurt() {
        let mut g = flat_gen();
        let m = g.spawn_creature(4, 100 << 8, 100 << 8, 100).unwrap();
        g.ent[m].tick70 = 27; // m4 PACK = 4*6 + 3
        g.ent[m].f52 = 0;
        g.ent[m].f58 = 16; // awake: the prologue WOULD read the mail
        g.ent[m].act_life = 207;
        g.ent[m].mail[0] = (1200, 629);
        let rig_ctx = ctx(&mut g);
        g.creature_tick(m, &rig_ctx);
        assert_eq!(g.ent[m].tick70, 25, "demoted to wander");
        assert_eq!(g.ent[m].act_life, 207, "the hit is not read this tick");
        assert_eq!(g.ent[m].mail[0], (1200, 629), "…and stays pending");
        assert_eq!(g.ent[m].f38, 0, "no killer latched");

        // A member WITH a leader still runs the prologue.
        let mut g = flat_gen();
        let l = g.spawn_creature(4, 100 << 8, 100 << 8, 100).unwrap();
        let m = g.spawn_creature(4, 101 << 8, 100 << 8, 100).unwrap();
        g.ent[m].tick70 = 27;
        g.ent[m].f52 = l as u16;
        g.ent[m].f58 = 16;
        g.ent[m].act_life = 207;
        g.ent[m].mail[0] = (1200, 629);
        let rig_ctx = ctx(&mut g);
        g.creature_tick(m, &rig_ctx);
        assert_eq!(g.ent[m].act_life, 207 - 1200, "led: the mail lands");
        assert_eq!(g.ent[m].tick70, 28, "…and the member corpses");
    }

    /// Round 167 ([`super::no_mc1hw_tree_ctor`]): `HIDDEN.EXE`'s tree
    /// ctor `sub_37F80` draws three times and takes sprite 83 always
    /// (file 0x50a4c `push $0x53`); `CARPET.EXE`'s `sub_37BC0` draws a
    /// fourth time for the 83 / 84 variant. WITNESS mc1hwl10 t=48554
    /// slot 6. `MGC_NO_MC1HW_TREE_CTOR=1` fails the HW draw count.
    #[test]
    fn the_hidden_worlds_tree_draws_three_times_and_wears_sprite_83() {
        let lcg = |r: u32| r.wrapping_mul(9377).wrapping_add(9439);
        for (verbs, draws) in [(VerbSet::MC1HW, 3), (VerbSet::MC1, 4)] {
            let mut saw84 = false;
            for seed in 0..64u32 {
                let planes = Planes {
                    height: vec![100; 0x10000],
                    tile_type: vec![5; 0x10000],
                    shading: vec![32; 0x10000],
                    angle: vec![5; 0x10000],
                    ceiling: Vec::new(),
                };
                let assets = FeatureAssets {
                    rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
                    build_tab: Vec::new(),
                    build_dat: Vec::new(),
                    bldgprm: Vec::new(),
                    spells: Vec::new(),
                    mc2_sprite_ext: Vec::new(),
                    mc1_sprite_ext: Vec::new(),
                };
                let mut g = Gen::new(planes, assets, 1, ChassisParams::MC1, verbs);
                g.rand = seed;
                let t = g.spawn_scenery(0, 100 << 8, 100 << 8, 100).unwrap();
                // The allocator seeds `+4` = slot + the world LCG.
                let mut r = (t as u32).wrapping_add(seed);
                for _ in 0..draws {
                    r = lcg(r);
                }
                assert_eq!(g.ent[t].rand, r, "{draws} draws, seed {seed}");
                if draws == 3 {
                    assert_eq!(g.ent[t].type86, 83, "HW has no variant");
                } else {
                    let want = if r & 1 != 0 { 84 } else { 83 };
                    assert_eq!(g.ent[t].type86, want, "MC1 picks off the 4th");
                    saw84 |= want == 84;
                }
            }
            assert_eq!(saw84, draws == 4, "non-vacuity: MC1 reaches 84");
        }
    }

    /// A mound in the PACK slot whose leader is already chasing, with
    /// the leader's `+146` reading 0 — the pack-death handoff
    /// (`sub_1A390` :21777-81 copies the leader's `+146` verbatim,
    /// clears `+52` and promotes to `base + 2`), which is exactly how
    /// the mc1hwl1 t=345 militiaman acquires his own `+146 = 0`. The
    /// `(9, 3)` wrapper `sub_1DC80` (:24215-21 — `sub_1A390(a1x,
    /// 0x36)` then `if (+70 == 56) sub_1DCD0(a1x)`) runs the
    /// chase-entry trailer on the SAME tick.
    fn packed_mound(g: &mut Gen, tgt_of_leader: u16) -> usize {
        let leader = g.spawn_creature(9, 100 << 8, 100 << 8, 100).unwrap();
        let mound = g.spawn_creature(9, 101 << 8, 100 << 8, 100).unwrap();
        g.ent[leader].tick70 = 56; // m9 CHASE = 9*6 + 2
        g.ent[leader].f146 = tgt_of_leader;
        g.ent[mound].tick70 = 57; // m9 PACK = 9*6 + 3
        g.ent[mound].f52 = leader as u16;
        g.ent[mound].f63 = 0; // on the v_26 cadence
        // The type-201 disguise pose the entry trailer overwrites.
        g.ent[mound].f126 = 20;
        g.ent[mound].f66 = 3;
        g.ent[mound].f67 = 0xFF;
        mound
    }

    /// ⭐⭐⭐ **THE MOUND'S CHASE-ENTRY TRAILER INDEXES THE POOL WITH
    /// `+146` UNCONDITIONALLY, SO A TARGET OF 0 IS THE SCRATCH
    /// RECORD** — law L1's SECOND call path (`sub_1DCD0`), the half
    /// the recording corpus never captured.
    ///
    /// `reference/remc1/sub_main.cpp` :24240-48 stages ONE pointer
    /// `v1 = pool_base + 164 * a1x->+146` and then runs BOTH the
    /// `id24` equality test (`if (a1x->+24 == v1->+24) { +70 = 55;
    /// return; }`, the sibling test below) and the filter stamp
    /// (`+66 = v1->+64`, `+67 = v1->+65`) off it. There is no index
    /// test, no class test and no player special case anywhere on the
    /// path — the militia twin `sub_1BC50` (:22759-61,
    /// `sClass = *(BYTE *)(pool() + 164 * v3 + 29859)`, 29859 = POOL
    /// 29795 + 64) is the same idiom and is already corpus-pinned by
    /// `conformance/fixtures/mc1hwl1/an-arm-trailer-stamps-the-scratch-record-s-filter.mgcr`
    /// (mc1hwl1 t=3684, slot 305: retail stamps `f66 = 0`).
    /// Pool slot 0 is the engine's SCRATCH RECORD: unstaged it reads
    /// class64 = 0 / model65 = 0, so the mound stamps (0, 0) — not the
    /// wizard-body (3, 0) the port's invented `t != 0` fallthrough
    /// produced.
    ///
    /// THE SHIPPED BYTES SETTLE IT (`CARPET.EXE`, file = VA + 0x187f8;
    /// `sub_1DCD0` begins at file 0x364c8 = VA 0x1DCD0). `+146` is
    /// loaded ZERO-EXTENDED with no test at all —
    /// `31 d2 / 66 8b 93 92 00 00 00` (`xor edx,edx` /
    /// `mov dx,[ebx+0x92]`) at file 0x364d0 — scaled by 164 through
    /// the `lea` chain and folded into ONE staged pointer by
    /// `01 c6` (`add esi,eax`) at 0x364fc. Both consumers then read
    /// off `%esi`: the id test `66 8b 53 18 / 66 3b 56 18`
    /// (`mov dx,[ebx+0x18]` / `cmp dx,[esi+0x18]`) at 0x364fe, whose
    /// taken branch is `6a 37 / 53 / e8 …` (`push 0x37` = state 55)
    /// at 0x36508, and the stamp `8a 46 40 / 88 43 42`
    /// (`+66 = [esi+0x40]`) at 0x36527 with `8a 46 41 / 88 43 43`
    /// (`+67 = [esi+0x41]`) at 0x3652d. There is no `test` or `cmp`
    /// against the index anywhere between the load and either use.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_M4M9_ARM_SCRATCH_TARGET=1` puts the
    /// invented guard back and the stamp reads (3, 0).
    #[test]
    fn a_mound_entering_chase_on_a_null_target_stamps_the_scratch_filter() {
        let mut g = flat_gen();
        assert_eq!(
            (g.ent[0].class64, g.ent[0].model65),
            (0, 0),
            "the scratch record is unstaged — retail reads class64 = 0, model65 = 0 there"
        );
        let mound = packed_mound(&mut g, 0);
        assert_ne!(
            g.ent[mound].id24, g.ent[0].id24,
            "this leg must not trip the sibling id test"
        );
        let rig_ctx = ctx(&mut g);
        g.creature_tick(mound, &rig_ctx);
        assert_eq!(
            g.ent[mound].f146, 0,
            "the pack handoff copied the leader's null target"
        );
        assert_eq!(g.ent[mound].tick70, 56, "and promoted the mound to CHASE");
        assert_eq!(
            (g.ent[mound].f66, g.ent[mound].f67),
            (0, 0),
            "the bolt filter is the SCRATCH record's (0, 0), not a hardcoded (3, 0)"
        );
        assert_eq!(
            g.ent[mound].f126, 0,
            "…and the burrower fights rooted (+126 = 0)"
        );

        // CONTROLS — the arm is not simply always (0, 0).
        let mut g = flat_gen();
        let victim = g.spawn_creature(4, 102 << 8, 100 << 8, 100).unwrap();
        let mound = packed_mound(&mut g, victim as u16);
        let rig_ctx = ctx(&mut g);
        g.creature_tick(mound, &rig_ctx);
        assert_eq!(
            (g.ent[mound].f66, g.ent[mound].f67),
            (5, 4),
            "a live militia target stamps its own (5, 4)"
        );
        let mut g = flat_gen();
        let mound = packed_mound(&mut g, PLAYER_TARGET);
        let rig_ctx = ctx(&mut g);
        g.creature_tick(mound, &rig_ctx);
        assert_eq!(
            (g.ent[mound].f66, g.ent[mound].f67),
            (3, 0),
            "the out-of-pool human is still named directly"
        );
    }

    /// The OTHER half of `sub_1DCD0`'s single staged pointer: the
    /// `id24` equality test at :24242-46 runs off `v1` too, so a mound
    /// whose `+146` is 0 tests itself against POOL SLOT 0's id and, on
    /// a match, drops straight back to hidden (`+70 = 0x37` = 55)
    /// without stamping anything. The port's invented `t != 0` guard
    /// short-circuited the test as well as the stamp, so this path was
    /// unreachable.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_M4M9_ARM_SCRATCH_TARGET=1` — the
    /// mound then stays in CHASE (`tick70` 56) with the hardcoded
    /// (3, 0) stamped over its disguise filter.
    #[test]
    fn a_mound_s_id_test_runs_against_the_scratch_record_too() {
        let mut g = flat_gen();
        let mound = packed_mound(&mut g, 0);
        // "a mound that acquired its own owner's body" — here the
        // scratch record's id, which is what retail actually compares.
        g.ent[mound].id24 = g.ent[0].id24;
        let rig_ctx = ctx(&mut g);
        g.creature_tick(mound, &rig_ctx);
        assert_eq!(
            g.ent[mound].tick70, 55,
            "matching the scratch record's id drops the mound straight back to HIDDEN"
        );
        assert_eq!(
            (g.ent[mound].f66, g.ent[mound].f67),
            (3, 0xFF),
            "the early return leaves the disguise filter standing — no stamp at all"
        );
        assert_eq!(g.ent[mound].f126, 20, "and no rooting either");
    }

    /// The kraken ctor `sub_389E0` stamps `+56 = 96` on the HEAD like
    /// its two worm siblings (:45057, `CARPET.EXE` file 0x512A4 `mov
    /// word [ebx+0x38],0x60`); the segments overwrite their copy with
    /// `4·+80` (:45086). Ungraded lane — the raw shadow's `(5,6) f56`
    /// (852,694 free-run rows / 15 takes in round 153's census).
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_MC1_KRAKEN_HEAD_F56=1` — the head
    /// reads 0.
    #[test]
    fn the_kraken_head_stamps_plus56_96_like_the_worms() {
        let mut g = flat_gen();
        let head = g.spawn_creature(6, 100 << 8, 100 << 8, 100).unwrap();
        assert_eq!((g.ent[head].class64, g.ent[head].model65), (5, 6));
        assert_eq!(g.ent[head].f56, 96, "the head's +56 is the ctor's 96");
        let seg = g.ent[head].f54 as usize;
        assert_ne!(seg, 0, "two segments hang off +54");
        assert_eq!(
            g.ent[seg].f56,
            4 * g.ent[seg].f80,
            "a segment's +56 is its own 4·+80, not the copied 96"
        );
        // Control: the worm head carries the same stamp.
        let worm = g.spawn_creature(0, 120 << 8, 120 << 8, 100).unwrap();
        assert_eq!(g.ent[worm].f56, 96);
    }

    /// PATCH `skeletons_convert_traders`: on the fourth window of the
    /// patched rotation a skeleton converts an m14 trader beside it;
    /// retail's `% 3` reads that same clock as the militia window and
    /// leaves the trader alone. Control: an m13 villager on its own
    /// window is converted in both arms.
    #[test]
    fn a_skeleton_converts_a_trader_only_when_patched() {
        for patched in [false, true] {
            let mut g = flat_gen();
            g.skeletons_convert_traders = crate::engine::features::HashSilent(patched);
            let sk = g
                .spawn_creature(9, 100 << 8, 100 << 8, 100)
                .expect("skeleton");
            let v26 = super::BEHAVIOR[g.ent[sk].row156 as usize].v_26;
            assert!(3 * v26 < 256, "test premise: window 3 fits the u8 clock");
            g.ent[sk].f63 = (3 * v26) as u8;
            let tr = g
                .spawn_creature(14, (100 << 8) + 300, 100 << 8, 100)
                .expect("trader");
            g.ent[tr].z = g.ent[sk].z; // inside the 3-D reach (0x600)
            g.rebuild_mob_chains();
            let before = (1..g.ent.len())
                .filter(|&s| g.ent[s].class64 == 5 && g.ent[s].model65 == 9)
                .count();
            g.m9_convert(sk, false);
            let after = (1..g.ent.len())
                .filter(|&s| g.ent[s].class64 == 5 && g.ent[s].model65 == 9)
                .count();
            assert_eq!(
                g.ent[tr].flags & 0x400 != 0,
                patched,
                "patched={patched}: trader consumed"
            );
            assert_eq!(
                after - before,
                patched as usize,
                "patched={patched}: one new skeleton"
            );

            // Control: window 2 is the villager's in both rotations.
            let mut g = flat_gen();
            g.skeletons_convert_traders = crate::engine::features::HashSilent(patched);
            let sk = g
                .spawn_creature(9, 100 << 8, 100 << 8, 100)
                .expect("skeleton");
            g.ent[sk].f63 = (2 * v26) as u8;
            let vi = g
                .spawn_creature(13, (100 << 8) + 300, 100 << 8, 100)
                .expect("villager");
            g.ent[vi].z = g.ent[sk].z;
            g.rebuild_mob_chains();
            g.m9_convert(sk, false);
            assert_ne!(
                g.ent[vi].flags & 0x400,
                0,
                "patched={patched}: villager consumed"
            );
        }
    }
}
