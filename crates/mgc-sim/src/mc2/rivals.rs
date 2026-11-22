//! MC2 rival wizards — the class-3 model-1 AI carpets on the MC2
//! column: lifecycle (spawn/records/authored castles), the per-tick
//! brain, the casting arm, mortality and respawn. Port of the remc2
//! machinery over the MC1 rival chassis; trace bank:
//! docs/traces/mc2-rivals-brain.md, mc2-rivals-spawn-mortality.md and
//! mc2-rivals-open-closure.md (`EF:` = remc2 EventsFunctions.cpp).
//!
//! The MC2 brain is the MC1 brain function-for-function (the
//! sub_12910 housekeeping/selector/handlers sandwich, the 0x601F hate
//! ledger, the burst gun, the poverty latch). What MC2 keys
//! differently:
//! - the SPELL IDS (heal 5, speed-up 3, possess 1, cloak 0xB,
//!   castle 2) and the recast/attack-priority tables (§7.2/7.3);
//! - the book is the class-15 manifestation entity per spell
//!   ([`Mc2Spellbook`] per rival), granted ONLY at load from the
//!   level's `WizardMapSettings` masks with authored starting tiers;
//!   MC2 has NO runtime spell learning (open-closure §3);
//! - the water/obstacle steer `sub_16580` runs after every state
//!   handler (open-closure §1 — MC1's AI flew over everything);
//! - death scatters the class-15 SPELL TOKENS (re-collectible), the
//!   respawn timer is a flat 1200, and a castle-less dead rival is
//!   BANISHED — the elimination signal the staged objective engine's
//!   case 3/8 reads;
//! - rivals earn NO spell XP — `sub_6D8B0`'s guard is class-3
//!   model-0, the human only (EF:58240-41); a rival's tiers are its
//!   authored map levels, and the per-cast TIER-DOWN walk
//!   (sub_15F20) supplies the tier dynamics.
//!
//! Original asymmetries, ported as traced: the AI at its own castle
//! DISCARDS damage (grace pinned 2, mailbox memset — EF:5400-5414;
//! the at-castle FORWARD is human-only, EF:59961); AI life regen /200
//! home /500 afield (4x the human afield); the AI carpet ignores
//! walls and knockback but — new in MC2 — steers around WATER; target
//! scans are omniscient.
//!
//! Open interim deviations (ours, flagged inline): the hate feed
//! rides damage intake instead of the per-projectile scan sub_159E0
//! (the MC1-column position); the DEFENSE state's disguise VISUAL
//! (retail draws the metamorph creature in place of the AI carpet) is
//! presentation-side unported — the state machine, tier pick,
//! shadowing and speed law are faithful (sub_15FC0/sub_161A0).

use crate::engine::features::{Gen, tile};
use crate::engine::world::{Hx, LifeState, World};
use crate::mc2::behavior::BEHAVIOR;
use crate::mc2::cast::{Mc2Spellbook, NO_MID_BURST_REGEN_PIN};

/// MC2 spell count (spell ids 0..25).
pub const MC2_SPELLS: usize = 26;

/// The hate ledger's neutral baseline (0x601F, EF:5377).
pub(crate) const HATE_NEUTRAL: u16 = 24607;
/// Hate toward a freshly (re)spawned wizard — elevated but decaying
/// (the post-spawn truce, -24609 as unsigned, EF:43850).
const HATE_RESPAWN: u16 = 40927;

/// A/B toggle for THE HUMAN RESPAWN'S POST-DEATH TRUCE: set
/// `MGC_NO_MC2_HUMAN_RESPAWN_TRUCE` to restore the pre-dig behaviour,
/// where only a RIVAL's (re)spawn pushed every other wizard's ledger
/// toward the newcomer to [`HATE_RESPAWN`] and the HUMAN's respawn
/// left the rivals' `hate[0]` running wherever the decay had carried
/// it. `sub_5C950` is ONE function for both wizards (EF:43982) and
/// the truce loop sits BELOW the `IsAiPlayer` fork, ungated
/// (`reference/remc2/remc2/engine/EventsFunctions.cpp:44185-93`; the
/// tree's older `EF:43839` numbering names the same statement):
///
/// ```text
/// for (kx = ...dword_38519; kx > Entities_EA3E4[0]; kx = kx->next_0)
///     if (kx->id_0x1A_26 != v2x->id_0x1A_26) {
///         v32 = kx->model_0x40_64;
///         if (!v32 || v32 == 1)
///             kx->dword_0xA4_164x->array_0x1FC_508[4 + 4 * colour] = -24609;
///     }
/// ```
///
/// SHIPPED NETHERW.EXE, VA 0x5CE22..0x5CE5A (file 0x81622, = VA +
/// 0x24800) — the ONLY `df 9f` immediate in the whole binary:
///
/// ```text
/// 5ce22: 66 8b 50 1a           mov    0x1a(%eax),%dx        ; kx->id
/// 5ce26: 66 3b 53 1a           cmp    0x1a(%ebx),%dx        ; vs respawner's id
/// 5ce2a: 74 26                 je     0x5ce52
/// 5ce2c: 8a 50 40              mov    0x40(%eax),%dl        ; kx->model_0x40_64
/// 5ce2f: 84 d2                 test   %dl,%dl
/// 5ce31: 74 05                 je     0x5ce38               ; model 0 = HUMAN
/// 5ce33: 80 fa 01              cmp    $0x1,%dl
/// 5ce36: 75 1a                 jne    0x5ce52               ; model 1 = RIVAL
/// 5ce38: 8b 93 a4 00 00 00     mov    0xa4(%ebx),%edx
/// 5ce3e: 0f bf 4a 38           movswl 0x38(%edx),%ecx       ; playerColorIndex
/// 5ce42: 8b 90 a4 00 00 00     mov    0xa4(%eax),%edx
/// 5ce48: 66 c7 84 ca 04 02 00  movw   $0x9fdf,0x204(%edx,%ecx,8)
/// 5ce52: 8b 00                 mov    (%eax),%eax           ; kx = kx->next_0
/// 5ce54: 3b 05 e4 a3 01 00     cmp    0x1a3e4,%eax
/// 5ce5a: 77 c6                 ja     0x5ce22
/// ```
///
/// ⚠ DISCRIMINATED ON AN ARGUMENT, NOT SHAPE: the near-identical
/// sibling 24 bytes below (`movw $0x601f,0x1fc(%ecx,%eax,8)` at
/// 0x5cea3) is the RESPAWNER'S OWN ledger going back to
/// [`HATE_NEUTRAL`], and THAT one IS gated — `cmpb $0x1,0x40(%ebx)`
/// at 0x5ce7a, i.e. model 1 only. The truce loop above it has no such
/// test. Both write the same `array_0x1FC_508` at an 8-byte stride
/// (0x204 + 8·colour vs 0x1fc + 8·l with `l` pre-incremented 1..8 —
/// the same eight words).
///
/// See the call site in [`World::mc2_respawn_truce`].
pub(crate) fn no_mc2_human_respawn_truce() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_HUMAN_RESPAWN_TRUCE").is_some())
}

/// A/B toggle for THE TRUCE LOOP'S TWO **RIVAL** CALL PATHS: set
/// `MGC_NO_MC2_RIVAL_TRUCE_ROSTER` to restore the pre-dig behaviour,
/// where a rival's spawn and a rival's respawn stamped
/// [`HATE_RESPAWN`] into **every** entry of `mc2_rivals` instead of
/// walking retail's tick-top class-3 roster.
///
/// ⭐⭐⭐ **A LAW ON ONE CALL PATH IS NOT LANDED.** The roster walk at
/// the bottom of `sub_5C950` (the bytes quoted on
/// [`no_mc2_human_respawn_truce`], shipped NETHERW.EXE VA
/// 0x5CE1A..0x5CE5C = file 0x8161A..0x8165C) reads its members from
/// `dword_38519` — `mov 0x9677(%eax),%eax` at 0x5CE1A, the CLASS-3
/// tick-top chain ([`Gen::wiz_chain`]), whose membership is sampled
/// once per tick at `life_0x8 >= 0` (EF:40281-88). The previous round
/// landed that on the HUMAN respawn only; the two RIVAL paths kept a
/// flat `for other in &mut self.mc2_rivals` loop that knows nothing
/// about life or about the tick top. Two consequences, both measured:
///
/// 1. **LEVEL-START SEATING.** `sub_5C950` is the PlayerAction-1 arm
///    of the input switch (EF:37887), so every wizard is seated inside
///    ONE tick whose class-3 chain was built BEFORE any wizard record
///    existed — the walk visits nobody and no ledger is stamped.
///    The flat loop instead stamped every already-seated rival,
///    producing the TRIANGULAR init-check signature (wiz k wrong
///    toward every colour > k: mc2l22-new 21 rows, mc2l17 15,
///    mc2l12 6, at 40927 minus the settle ticks' decay against
///    retail's 24607).
/// 2. **A RESPAWN WHILE OTHER WIZARDS ARE DEAD.** `dword_38519` only
///    ever holds `life >= 0` records, so a rival that is in its
///    dead-wait (action 3) when another colour comes back takes NO
///    truce. mc2l17 t=1500: wizards 1,2,3,4,6 all hold `life < 0`
///    (−3114/−2737/−2260/−2012/−670) with only wizard 5 and the human
///    alive; colour 3's dead-wait counter hits 0 (`scratch10 1 -> 0`)
///    and it respawns at t=1501. Retail stamps wizard 5 alone; the
///    port stamped 1,2,4,5,6 — the pair census's `wiz 2/4/6 [3]:
///    retail 24607 port 40927` at t=1501, and 2,085 free-run rows
///    behind them.
///
/// ⚠ The respawner itself is excluded by `cmp 0x1a(%ebx),%dx` /
/// `je` at 0x5CE26 — an ID test, which matters on the ALIVE respawn
/// arm (PlayerAction 0xF with a castle, EF:37937), where the
/// respawner IS on the roster.
pub(crate) fn no_mc2_rival_truce_roster() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_TRUCE_ROSTER").is_some())
}

/// `MGC_NO_MC2_WIZ_BLOCK_AT_BIRTH=1` restores the pre-dig ordering, in
/// which `Gen::rival_ents[slot]` — the port's only entity → player
/// block map — was seated at the very END of [`Mc2Ctx::mc2_spawn_rival`],
/// after the authored starting castle had already been built.
///
/// ⭐⭐ RETAIL WIRES THE BACK-POINTER AT WIZARD BIRTH, NOT AT THE END
/// OF THE SPAWN. `sub_5C950` (EF:44058, banner `0005C950`) writes
/// ```text
///   v2x->dword_0xA4_164x = &a1x->dword_0x3E6_2BE4_12228;
/// ```
/// in its opening block, ~130 source lines ABOVE the authored-castle
/// arm (EF:44130-44170), and everything downstream resolves a player
/// through it. In particular the castle HP/CAP ladder `sub_60810`
/// (EF:62092, banner `00060810`) reads the owner's Life scalar as
/// ```text
///   Entities_EA3E4[locEvent->id_0x1A_26]->dword_0xA4_164x->word_0x24A_586
/// ```
/// — a two-hop pointer chase off the CASTLE's owner id, with no roster
/// scan anywhere. Shipped bytes (`NETHERW.EXE`, file = VA + 0x24800),
/// `sub_60810` at file 0x85010 = VA 0x60810:
/// ```text
///   8501c  movswl 0x1a(%ebx),%eax           ; castle->id_0x1A_26
///   85020  mov    0x1a3e4(,%eax,4),%eax     ; Entities_EA3E4[id]
///   85053  mov    0xa4(%eax),%eax           ; ->dword_0xA4_164x
///   85059  mov    0x10(%ebx),%ecx           ; castle->dword_0x10_16
///   8505c  movsbl 0x24e(%ecx,%eax,1),%ecx   ; array_0x24E_590[level]
///   85064  shl    $0x8,%ecx
///   85067  movswl 0x24a(%eax),%eax          ; word_0x24A_586  (Life)
///   8506e  add    $0x100,%ecx
///   85074  imul   %eax,%ecx
///   8507a  sar    $0x8,%ecx                 ; number1
/// ```
/// and `sub_5C950` at file 0x81150 = VA 0x5C950, whose back-pointer
/// write and Life write both precede its call into the ladder:
/// ```text
///   8124d  lea    0x3e6(%edx),%eax          ; &player->dword_0x3E6
///   81259  mov    %eax,0xa4(%ebx)           ; wizard->dword_0xA4_164x
///   ...
///   8143e  mov    0x3612f(%eax),%cx         ; WizardMapSettings.Life
///   81445  test   %cx,%cx
///   81448  je     0x8146a                   ; 0 keeps the 0x100 default
///   8144a  mov    %cx,0x24a(%edx)           ; ->word_0x24A_586
///   ...
///   8158f  call   0x85010                   ; sub_60810, authored castle
/// ```
///
/// The port has no per-entity player pointer: `Gen::mc2_castle_ladder`
/// (mc2/castle.rs) resolves the owner's colour by scanning
/// `Gen::rival_ents` for the owner entity id and falls back to
/// `unwrap_or(0)` — the HUMAN, whose `mc2_life_scale` is the flat 256.
/// Because `rival_ents[slot]` was written only after
/// `mc2_spawn_authored_castle` returned, EVERY authored rival castle
/// took its birth ladder at scale 256 instead of the map header's
/// `WizardMapSettings.Life_0x3612F`.
///
/// Usually invisible, because the castle's FIRST build tick re-runs
/// the ladder (`mc2_castle_upgrade`) once `rival_ents` is complete and
/// overwrites the wrong HP. It survives to record 0 exactly when the
/// level-up is REFUSED — `mc2_castle_build` case 0's
/// `mc2_castle_space_ok` says no, the castle parks at action 4 holding
/// its birth rung. Witness mc2l22 / mc2l22-new slot 476 (level 1,
/// owner wizard 451 = colour 1, `Life_0x3612F` = 254): retail
/// `life`/`max_life` 19843 = `20000 * 254 >> 8`, port 20000.
pub(crate) fn no_mc2_wiz_block_at_birth() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_WIZ_BLOCK_AT_BIRTH").is_some())
}

/// `MGC_NO_MC2_AUTHORED_CASTLE_LINK_Z=1` restores the pre-dig link of
/// the authored starting castle, which used the perimeter-min build
/// datum (`site_z`) as its map-link altitude instead of the ground
/// under the raw caller point. See the citation at the use site in
/// [`Mc2Ctx::mc2_spawn_authored_castle`] — `sub_4AA40` EF:33385/33399/
/// 33400, banner `0004AA40`; shipped `NETHERW.EXE` file 0x6F240 =
/// VA 0x4AA40:
/// ```text
///   6f29a  push   $0x1b398             ; &predictedAxis_EB398ar (RAW)
///   6f2a3  call   0x35440              ; getTerrainAlt_10C40
///   6f2ac  mov    %ax,-0x8(%ebp)       ; v6ar.z <- ground at the RAW pt
///   6f2f2  movsl ; 6f2f3 movsw         ; entity +0x9A <- v6ar (x,y,z)
///   6f357  call   0x6d660              ; sub_48E60 perimeter minimum
///   6f35c  shl    $0x5,%eax            ;   x 32
///   6f362  mov    %ax,0x9e(%ebx)       ; entity +0x9E <- the build datum
///   6f369  lea    -0xc(%ebp),%eax      ; &v6ar — z STILL the raw ground
///   6f36e  call   0x7c570              ; AddEventToMap_57D70(entity, v6ar)
/// ```
pub(crate) fn no_mc2_authored_castle_link_z() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_AUTHORED_CASTLE_LINK_Z").is_some())
}

/// The MC2 AI per-spell recast cooldowns `x_WORD_D3F4C` (EF:1070) —
/// differs wholesale from MC1's table AND is indexed by MC2 spell id.
const AI_RECAST: [u16; MC2_SPELLS] = [
    2, 10, 40, 32, 300, 1, 1, 1, 1, 4, 1, 1, 0, 0, 0, 0, 0, 0, 400, 600, 600, 400, 400, 0, 0, 0,
];

/// Attack-priority walk vs a WIZARD — `unk_D3F80x` (EF:1071).
const ATTACK_WIZARD: [u8; 8] = [0x10, 0x12, 0x09, 0x07, 0x14, 0x15, 0x13, 0x00];
/// Attack-priority walk vs a CASTLE — `unk_D3F89x` (EF:1072).
const ATTACK_CASTLE: [u8; 7] = [0x10, 0x12, 0x07, 0x09, 0x11, 0x14, 0x00];
/// The DEFENSE state's DISGUISE-MODEL table — `unk_D3F91x` (EF:1073):
/// the class-5 creature models Metamorph's tiers turn a wizard into
/// (2 = tier-0 Day bird, 0x13 = tier-0 non-Day, 0x19 = tier 1,
/// 0x10 = tier 2), walked in scan-priority order by `sub_15FC0`
/// (EF:7664-79). These are creature MODELS, not spell ids — do not
/// feed them to the cast path.
const DISGUISE_MODELS: [u8; 4] = [0x02, 0x13, 0x19, 0x10];

/// A/B toggle for the RIVAL TOKEN BACK-REF (`word_0x26_38` on a
/// rival-cast projectile): set `MGC_NO_RIVAL_TOKEN_BACKREF` to restore
/// the pre-dig behaviour, where `mc2_rival_emit` stamped the SPELL
/// INDEX instead of the (15,x) token's pool slot. See the write site
/// in [`World::mc2_rival_emit`] for the citations and the witness.
fn no_rival_token_backref() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_TOKEN_BACKREF").is_some())
}

/// A/B toggle for the CASTLE-LESS SPAWN'S SITE GATE: set
/// `MGC_NO_MC2_RIVAL_CASTLE_ANY_SITE` to restore the pre-dig invented
/// `if site == (0,0) { refuse }` guard on `mc2_rival_cast_castle`.
fn no_mc2_rival_castle_any_site() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_CASTLE_ANY_SITE").is_some())
}

/// ⭐⭐⭐ **THE FIRST CASTLE IS FREE — AND IT IS ALSO UNRESEARCHED.**
/// `MGC_NO_MC2_FREE_CASTLE_UNRESEARCHED=1` restores the pre-dig
/// stage-1 research stamp on [`World::mc2_rival_cast_castle`]'s
/// castle-less arm.
///
/// `array_0x24E_590` — the castle research table, whose `+9` half is
/// the PART-TYPE that `sub_613D0` gates the (10,79) defender pieces on
/// — has **exactly one writer in the shipped `NETHERW.EXE`**:
/// `sub_69AB0`'s mint window, file **0x8E366**
/// `88 8e 4e 02 00 00  mov %cl,0x24e(%esi)` (HP factor) and **0x8E379**
/// `88 94 01 57 02 00 00  mov %dl,0x257(%ecx,%eax,1)` (part type). A
/// whole-image scan for a store to either displacement returns those
/// two instructions and nothing else.
///
/// `sub_14E10`'s case-2 CASTLE-LESS arm — the "first castle is free"
/// leg — is, byte for byte at file **0x396A7-0x396F3**:
/// `6a 02 / 6a 03 / 8d 83 9a 00 00 00 / 50 / e8 d9 52 03 00` (the (3,2)
/// alloc), `66 8b 53 1a / 66 89 50 1a` (the owner id), the slot
/// division, `8b 93 a4 00 00 00 / 66 89 42 3a  mov %ax,0x3a(%edx)`
/// (`CastleEntityIndex_0x3A_58`), `b8 01 00 00 00 / ret`. It never
/// reaches `sub_69AB0`, so a rival's FIRST castle carries
/// `array_0x24E_590[9+1] == 0` for its whole level-1 life, and the
/// level-up's `sub_613D0` walk-down (the gate at file **0x85C34**
/// `0f be 84 06 57 02 00 00  movsbl 0x257(%esi,%eax,1),%eax` /
/// `85 c0 / 75 05` break / `4e` dec, then **0x85C48** `85 f6` /
/// `0f 84 bb 01 00 00` bail) falls through at `v4 == 0` and spawns
/// **NO PIECES AT ALL**.
///
/// ⭐ `sub_613D0` has **exactly two** callers in the image (file
/// 0x84D63 inside `sub_60480`, the level-up; file 0x84EE4 inside
/// `sub_605E0`, the downgrade) and `sub_508E0`, the (10,79) ctor, has
/// **zero** direct callers — it is reached only through the
/// class/model dispatch from that walk. So the walk is the ONLY way a
/// (10,79) is ever minted, and the research byte is its only gate.
///
/// The port stamped stage-1 research here (an "A.5 shortcut"), so the
/// castle's very first level-up minted the stage-1 defender piece.
/// WITNESS mc2l12 t=10016, rival player 3 (ent 164): retail pops
/// exactly two slots — 639 the (3,2) castle, 761 the (10,42) build
/// painter — and the free stack goes 864 -> 862 with next-pop 765; the
/// port popped 765 as well and turned it into a `(10,79)`. Every later
/// allocation on the take was shifted by that one pop. Landing this
/// took mc2l12's bit-exact horizon 10,015 -> 39,625.
///
/// The upgrade leg is unaffected: [`World::mc2_rival_castle_mint`]
/// still stamps `castleLevel + 1`, which is `sub_69AB0`'s own write.
fn no_mc2_free_castle_unresearched() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_FREE_CASTLE_UNRESEARCHED").is_some())
}

/// A/B toggle for THE HOME ARM'S CHAINED CAST WALK: set
/// `MGC_NO_MC2_RIVAL_HOME_CAST_CHAIN` to restore the pre-dig shared
/// [`World::mc2_rival_walk_cast`] at `sub_133B0`'s spell-1 site, which
/// STOPS the tier walk at the first passing tier even when the
/// executor then refuses — leaving the token re-priced at that tier.
///
/// `sub_133B0` (EF:5804) is the ONE retail tier walk that chains the
/// executor into the loop condition:
/// `if (sub_15F20(a1x, k, 1) == 1 && sub_14E10(a1x, 1u)) goto LABEL_6;`
/// Shipped `NETHERW.EXE` file 0x37C08-0x37C3C (VA 0x133B0 + 0x24800),
/// discriminated on the ARGUMENT `push $0x1` / `movsbw 0x41e(%eax),%di`
/// (= `SpellLevels[1]`):
/// ```text
///   37bf7: movsbw 0x41e(%eax),%di   ; k = SpellLevels[1]
///   37c01: push $0x1                ; a3 = spell 1
///   37c06: push %eax                ; a2 = k
///   37c08: call 0x3a720             ; sub_15F20  (probe; SetSpell side effect)
///   37c10: cmp  $0x1,%ax
///   37c14: jne  0x37c38             ; probe failed -> k--
///   37c19: call 0x39610             ; sub_14E10  (the executor)
///   37c21: test %eax,%eax
///   37c23: je   0x37c38             ; ⭐ REFUSED CAST -> k--, KEEP WALKING
///   37c38: dec  %edi
///   37c39: test %di,%di
///   37c3c: jge  0x37c01
/// ```
/// Its near-identical sibling 40 bytes later is the spell-0xB walk at
/// 0x37C4E (`push $0xb` / `movsbw 0x428(%eax),%di`), which calls the
/// same executor at 0x37C66 and then `jmp 0x37c76` — it does NOT test
/// the return. `sub_135C0`'s Possess walk (0x37E2E-0x37E60) breaks out
/// of the loop first (`je 0x37e4c`) and casts OUTSIDE it, so the
/// shared helper stays correct there. Only the Home arm chains.
fn no_mc2_rival_home_cast_chain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_HOME_CAST_CHAIN").is_some())
}

/// A/B toggle for THE HOMELESS HOME ARM: set
/// `MGC_NO_MC2_RIVAL_HOMELESS_SPEED_RUN` to restore the pre-dig
/// paraphrase — the cloak walk, an INVENTED `state = Cruise` write,
/// and nothing else.
///
/// `sub_133B0`'s castle-MISSING arm (`Entities[CastleEntityIndex_0x3A_58]
/// <= Entities[0]`, EF:5771-97 in the tree's current copy — the
/// surrounding `sub_133B0` citations are ~13 lines stale) is FOUR
/// steps, not one. Shipped
/// `NETHERW.EXE` file 0x37CCA-0x37D9F (VA 0x134CA + 0x24800),
/// discriminated on the ARGUMENTS `push $0xb`/`0x428(%eax)` vs
/// `push $0x3`/`0x420(%eax)` (= `SpellLevels[11]` / `SpellLevels[3]`,
/// base 0x41D):
/// ```text
///   37cca: movsbw 0x428(%eax),%si  ; k = SpellLevels[11]  (CLOAK)
///   37cd4: push $0xb / push %eax / push %ebx
///   37cdb: call 0x3a720            ; sub_15F20 probe
///   37ce3: cmp  $0xb,%ax
///   37ce7: jne  0x37cf6            ; miss -> k--
///   37ce9: push $0xb / push %ebx
///   37cec: call 0x39610            ; sub_14E10 -- return NOT tested
///   37cf4: jmp  0x37cfc            ; BREAK, fall through to step 2
///   37d02: movsbw 0x420(%eax),%si  ; j = SpellLevels[3]   (SPEED)
///   37d0c: push $0x3 / push %eax / push %ebx
///   37d13: call 0x3a720            ; sub_15F20 probe
///   37d1b: cmp  $0x3,%ax
///   37d1f: jne  0x37d41            ; miss -> j--
///   37d21: push $0x3 / push %ebx
///   37d24: call 0x39610            ; sub_14E10 -- return NOT tested
///   37d2d: call 0x3ad80            ; sub_16580 (water steer)
///   37d32: mov  $0x1,%ebx          ; ⭐ RETURN 1 whatever the cast did
///   37d47: push $0x3 / push %ebx
///   37d4a: call 0x38ec0            ; sub_146C0(self, 3)
///   37d52: test %eax,%eax / je 0x37d72
///   37d56: cmpw $0x0,0x2e(%eax)    ; word_0x2E_46 (the live window)
///   37d5b: jle  0x37d72
///   37d5d: call 0x3ad80 / mov $0x1,%ebx  ; RETURN 1, still boosting
///   37d72: mov  0x84(%ebx),%ax     ; minSpeed_0x84_132
///   37d7f: mov  %ax,0xc(%esi)      ; speed_0xc_12  = minSpeed
///   37d89: movw $0x1,0xe(%eax)     ; word_0xe_14   = 1  (brake)
///   37d90: call 0x3ad80            ; sub_16580, return 0
/// ```
/// NOTHING in the whole function writes `byte_0x1C1_449`: the state
/// byte is the SELECTOR's alone (the same law the target-gate note on
/// [`World::mc2_rival_state_tick`] already carries), so the `Cruise`
/// write was invented too.
fn no_mc2_rival_homeless_speed_run() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_HOMELESS_SPEED_RUN").is_some())
}

/// A/B toggle for the FRESH-SPAWN PURSE (round 113): set
/// `MGC_NO_MC2_RIVAL_START_PURSE` to restore the pre-dig behaviour — the
/// rival's (3,1) record left at `mana_0x90_144 = 0` from the ctor's
/// memset, so the purse mirror read 0 and the first castle waited on
/// 100/tick regen (mc2l4: cast tick 11 instead of retail's 8).
fn no_mc2_rival_start_purse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_START_PURSE").is_some())
}

/// A/B toggle for the AUTHORED SPELL LEVELS ON ALL 26 SLOTS (round
/// 147, native-init channel): set `MGC_NO_MC2_RIVAL_AUTHORED_LEVELS`
/// to restore the pre-dig behaviour, where `SpellLevels_0x41D` was
/// written only for spells the map both GRANTED and did not BLOCK.
/// Retail's `InitialiseSpells_54A50` takes the AI arm on a bare
/// `IsAiPlayer_0x009_2BE4_11239 == 1` test, ahead of the
/// StartingSpells/BlockedSpells test that computes `setSpell`
/// (EF:39025-29; NETHERW.EXE 0x792e5 `cmpb $0x1,0x9(%edx)` /
/// `je 0x7938f`, arm = 0x793a8 `mov 0x360fb(%ebx,%eax,1),%bl` +
/// 0x793af `mov %bl,0x803(%edi)` — `str_611 + 0x41D = player + 0x803`).
/// WITNESS (init-check record 0): mc2l17 wiz 1 is authored
/// `starting_spell_levels = [1; 26]` with 14 spells granted, and
/// retail's block reads 1 on all 26 while the port read 0 on the
/// other 12; mc2l18 wiz 6 differs on index 25 alone, which is
/// exactly its one blocked spell.
fn no_mc2_rival_authored_spell_levels() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_AUTHORED_LEVELS").is_some())
}

/// A/B toggle for the RIVAL QUICK-SLOT SEAT (round 147, native-init
/// channel): set `MGC_NO_MC2_RIVAL_AUTHORED_HANDS` to restore the
/// pre-dig behaviour, where a natively spawned rival kept
/// `Mc2Spellbook::default()`'s empty `-1/-1` hands for its whole life.
/// `InitialiseSpells_54A50` clears `SpellIndexLeft_0x451` /
/// `SpellIndexRight_0x453` to -1 (EF:38998-99; NETHERW.EXE 0x79280 /
/// 0x7928e `movw $0xffff`) and then seats the FIRST granted spell in
/// the left hand and the SECOND in the right, inside the same
/// 26-iteration walk (EF:39089-96; NETHERW.EXE 0x794d9 `movswl
/// 0x837(%edx),%ebx` / `cmp $-1` / 0x794e5 `mov %ax,0x837(%edx)`,
/// else 0x794ee/0x794fa for 0x839). There is no `break`, so once both
/// are seated the remaining grants change nothing.
/// WITNESS: retail reads 0/1 on all 51 rival seats of the 13 MC2 rival
/// takes; the port read -1/-1 on every one.
/// ⚠ NOTHING IN THE PORT READS A RIVAL'S HANDS TODAY — see the dig
/// note on [`World::mc2_spawn_rival`]; this seats the register for the
/// init census and for the (unported) `sub_68FF0` rival pickup.
fn no_mc2_rival_authored_hands() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_AUTHORED_HANDS").is_some())
}

/// A/B toggle for THE RESPAWN RE-MINT'S KEY (round 148, the
/// "no port home" scoping dig): set `MGC_NO_MC2_RIVAL_REMINT_MARKER`
/// to restore the pre-dig behaviour, where
/// [`World::mc2_rival_respawn`] re-minted the AUTHORED grant set
/// ([`Mc2Rival::known`]) and never cleared an entry the mint could
/// not fill.
///
/// ⭐ **`sub_5CF40` HAS NO "KNOWN" SET AT ALL — THE BOOK SLOT *IS*
/// THE PREDICATE.** The re-mint walks all 26 slots and takes a slot
/// only when `SpellEnabled[i]` is non-zero, which after
/// `sub_5E310`'s death scatter is the BOOLEAN 1 marker of what the
/// wizard actually held when it died (EF:60146) — not what the map
/// authored. And when the mint FAILS (the event pool is exhausted)
/// it writes the slot back to **0**, so the spell is gone for good.
/// Shipped `NETHERW.EXE` (file = VA + 0x24800), `sub_5CF40` at file
/// 0x81740:
/// ```text
///   81760: 66 83 bc 07 33 03 00 00  cmpw $0x0,0x333(%edi,%eax,1) ; SpellEnabled[i]
///   81769: 74 6f                    je   0x817da                 ; empty slot -> skip
///   8176b: 56 / 6a 0f / …           push i / push $0xf           ; mint a (15,i)
///   81772: e8 19 d2 fe ff           call 0x6e990                 ; IfSubtypeCall…4A190
///   8177e: 74 4a                    je   0x817ca                 ; NULL -> the loss arm
///   817aa: 66 89 87 33 03 00 00     mov  %ax,0x333(%edi)         ; slot = the new record
///   817c0: 66 89 41 28              mov  %ax,0x28(%ecx)          ; parentId = the wizard
///   817c4: 80 49 0c 01              orb  $0x1,0xc(%ecx)          ; byte[0] |= 1
///   817ca: 8b 83 a4 00 00 00        mov  0xa4(%ebx),%eax
///   817d0: 66 c7 84 07 33 03 00 00  movw $0x0,0x333(%edi,%eax,1) ; THE LOSS
///   817db: 83 fe 1a                 cmp  $0x1a,%esi              ; 26 slots
/// ```
///
/// The port's [`Mc2Rival::known`] is a non-retail shadow of that one
/// lane, and the two DO come apart: the conformance importer already
/// derives it as `known[s] = book.ent[s] != 0`
/// ([`World::reanchor_mc2_rival_ai`]), the level-start seating leaves
/// `known[s]` true for a spell whose mint failed on a full pool, and
/// `sub_605E0`'s castle-death purge zeroes `book.ent[2]` while the
/// grant stands ([`World::mc2_castle_death_token_purge`]). Round
/// 104 converted the four BRAIN gates from `known` to the
/// manifestation register for exactly this reason
/// (`rival_cascade_token_gate_off`, `rival_raid_castleless_token_off`)
/// — ⭐ A LAW ON ONE CALL PATH IS NOT LANDED: the respawn was the
/// fifth reader and kept asking the grant.
///
/// UNWITNESSED IN THE CORPUS by construction: in replay the importer
/// makes the two keys identical at every anchor, and round 147's
/// free-run census reads ZERO rival `spell_ent` rows over all 40 MC2
/// takes. This is a NATIVE-PLAY law and takes a unit test
/// (docs/CONFORMANCE.md — an ungraded lane can only take one).
fn no_mc2_rival_remint_marker() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_REMINT_MARKER").is_some())
}

/// A/B toggle for the ZERO WEAVE DIRECTION AT SPAWN (round 147,
/// native-init channel): set `MGC_NO_MC2_RIVAL_WEAVE_DIR_ZERO` to
/// restore the pre-dig behaviour, where [`Mc2Rival::new`] invented
/// `weave_dir = 1`. `str_611_byte_0x45C_1116` has exactly one writer
/// in the whole retail engine — `sub_13890`'s tick-0 arm, which
/// stores 1 or 2 (EF:6022/6027) — and exactly one reader, inside that
/// same arm's `if (byte_0x45D)` guard (EF:6006), so the spawn value is
/// UNREADABLE on both sides and this is a state seat, not a
/// behavioural law.
fn no_mc2_rival_weave_dir_zero() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_WEAVE_DIR_ZERO").is_some())
}

/// A/B toggle for the SCRATCH SLOT'S AUTHORED SITE (round 114): set
/// `MGC_NO_MC2_AUTHORED_SCRATCH_SITE` to restore the pre-dig behaviour —
/// the authored castle's row stamps and the build-site scout ran on a
/// function argument and never staged pool slot 0, so a native world's
/// slot 0 kept z = 0 and every rival's first `site_z` was 0 (mc2l4:
/// retail's rival 298 holds `dest_z` 2624 = castle 297's site z through
/// record 0 and its Build hover pins at 1176; the port's descended 8/tick).
fn no_mc2_authored_scratch_site() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_AUTHORED_SCRATCH_SITE").is_some())
}

/// A/B toggle for the COLOUR-STAGGERED CASTLE COOLDOWN (round 113): set
/// `MGC_NO_MC2_CASTLE_COLOUR_COOLDOWN` to restore the pre-dig behaviour
/// (every wizard's Create-Castle recast counter 0 at spawn).
fn no_mc2_castle_colour_cooldown() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_COLOUR_COOLDOWN").is_some())
}

/// A/B toggle for the LIFE-SCALAR IMPORT SEAT: set
/// `MGC_NO_MC2_LIFE_SCALE_IMPORT` to restore the pre-dig behaviour,
/// where `reanchor_mc2_rival_ai` seeded `Mc2Rival::life_scale` but left
/// the World-side `Gen::mc2_life_scale` mirror — the ONLY home the
/// castle HP ladder reads — at its level-init value.
pub(crate) fn no_mc2_life_scale_import() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LIFE_SCALE_IMPORT").is_some())
}

/// A/B toggle for the rival possession TIER PAYLOAD: set
/// `MGC_NO_RIVAL_POSSESS_TIER` to restore the pre-dig behaviour, where
/// the rival funnel stamped the basic bolt's `(10,12)` on the leveled
/// `(9,17)` at every tier.
fn no_rival_possess_tier() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_POSSESS_TIER").is_some())
}

/// A/B toggle for `sub_14030`'s ENTRY REFUSAL (absence 1 in
/// [`World::mc2_rival_pick_wizard`]): set
/// `MGC_NO_RIVAL_ATTACK_CASTLELESS_GATE` to restore the pre-dig
/// behaviour, where a castle-less rival that still owned the castle
/// manifestation went wizard-hunting instead of falling through to the
/// builder arms. EF:6245-46; `NETHERW.EXE` 0x38855-0x38874.
fn rival_attack_castleless_gate_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_ATTACK_CASTLELESS_GATE").is_some())
}

/// ⭐⭐⭐ A/B toggle for `sub_13E40`'s ENTRY REFUSAL — the **FOURTH**
/// `sub_146C0` cascade gate (see [`rival_cascade_token_gate_off`],
/// which enumerated only three). The RAID arm's castle-less refusal
/// asks the MANIFESTATION REGISTER, not the authored `known[2]` grant:
///
/// ```text
/// // EF:6192, sub_13E40
/// if (!sub_164B0(a1x) || !a1x->dword_0xA4_164x->CastleEntityIndex_0x3A_58
///                        && sub_146C0(a1x, 2u))
///     return 0;
/// ```
///
/// Shipped `NETHERW.EXE` 0x38640 (`off = 0x34800 + linear − 0x10000`,
/// `sub_13E40` linear 0x13E40) — the same five instructions
/// `sub_14030` carries at 0x38855:
///
/// ```text
///   38665  8b 86 a4 00 00 00   mov   0xa4(%esi),%eax   ; own wizext
///   3866b  66 83 78 3a 00      cmpw  $0x0,0x3a(%eax)   ; CastleEntityIndex
///   38670  75 13               jne   0x38685           ; has a keep → carry on
///   38672  6a 02               push  $0x2
///   38674  56                  push  %esi
///   38675  e8 46 08 00 00      call  0x38ec0           ; sub_146C0(self, 2)
///   3867d  85 c0               test  %eax,%eax
///   3867f  0f 85 9e 01 00 00   jne   0x38823           ; OWNS THE TOKEN → return 0
/// ```
///
/// A rival that has lost its keep but still holds the castle
/// manifestation is a BUILDER and must not raid. Once the token is
/// gone too it is a beaten wizard with nothing to build, and retail
/// lets it keep raiding the castle it was already on.
///
/// WITNESS — mc2l22 t=17474..17476, rival 611. Its own castle 637
/// dies at t=17474 (`castle_ent 637 -> 0`) and the castle token
/// (slot 15, a `(15,2)`) is purged in the same tick
/// (`spell_ent[2] 15 -> 0`). Retail keeps `ai_state 7` on the human's
/// castle 496 for the whole segment; the port's `known[2]` stayed
/// TRUE, refused arm 4 and re-elected the human WIZARD
/// (`target96 retail 496 / port 424`, with `z` 4265/4261 and `roll`
/// 1803/261 as the graded shadow).
///
/// `MGC_NO_RIVAL_RAID_CASTLELESS_TOKEN=1` restores the `known[2]` form.
fn rival_raid_castleless_token_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_RAID_CASTLELESS_TOKEN").is_some())
}

/// ⭐⭐⭐ A/B toggle for `sub_6B1C0`'s CLOAK EDGE — **AND THE SHIPPED
/// EXE CORRECTS THE LISTING TWICE IN FIVE LINES.** EF:57090-57094
/// reads
///
/// ```c
/// if (a1x->word_0x2E_46 == a1x->word_0x30_48) { … ; a1x->struct_byte_0xc_12_15.byte[0] |= 0x20u; }
/// else if (!(a1x->struct_byte_0xc_12_15.byte[0] & 0x20))  a1x->word_0x2E_46 = 1;
/// ```
///
/// i.e. both the stamp and the test on `a1x`, the TOKEN. In the
/// shipped `NETHERW.EXE` (`sub_6B1C0` linear 0x6B1C0 → file 0x8F9C0,
/// `off = 0x34800 + linear − 0x10000`) `%ebx` is the token and `%esi`
/// is `Entities[token->parentId_0x28_40]` — the CASTER — and BOTH
/// operands are `%esi`:
///
/// ```text
///   8fa03  66 8b 43 2e      mov   0x2e(%ebx),%ax     ; token word_0x2E_46
///   8fa07  66 3b 43 30      cmp   0x30(%ebx),%ax     ; token word_0x30_48
///   8fa0b  75 5f            jne   0x8fa6c
///   …                                                 ; the first-tick arm
///   8fa5e  8a 66 0c         mov   0xc(%esi),%ah      ; CASTER byte[0]
///   8fa61  80 cc 20         or    $0x20,%ah          ; ⇒ RAISE ON THE CASTER
///   8fa67  88 66 0c         mov   %ah,0xc(%esi)
///   8fa6a  eb 0c            jmp   0x8fa78
///   8fa6c  f6 46 0c 20      testb $0x20,0xc(%esi)    ; ⇒ TEST THE CASTER
///   8fa70  75 06            jne   0x8fa78
///   8fa72  66 c7 43 2e 01 00   movw $0x1,0x2e(%ebx)  ; ⇒ COLLAPSE THE WINDOW
///   8fa8a  66 8b 7b 2e      mov   0x2e(%ebx),%di     ; the shared decrement
///   8fa8e  66 4f            dec   %di
///   8fa96  8a 76 0c / 80 e6 df   and $0xdf, CASTER byte[0]   ; the expiry clear
/// ```
///
/// So the cloak bit is raised on the CASTER on the arm tick and read
/// back off the CASTER on every later tick: **the moment anything
/// clears it — and `sub_5F660`'s `sub_5F7B0` → `sub_5F7E0`
/// (EF:60974-90) clears it on the rival's NEXT CAST — the
/// Invisibility window collapses to 0 on its own next body.** The
/// port carried neither half; a banked comment in
/// [`World::mc2_rival_buffs`] even asserted, from the decompile
/// alone, that "`sub_6B1C0` NEVER raises 0x20 … mirroring the
/// Invisibility window onto the wizard's own 0x20 was an INVENTED
/// WRITE". That reading is wrong, and the RECORDING says so.
///
/// WITNESS — mc2l22, rival 530 (ri 3), token 542 `(15,11)`:
/// wizard `flags` **12 → 44 at t=13105** (the Invisibility II cast,
/// `f30` 181 → 183, mana 34516 → 15516) **→ 12 at t=13106**, and the
/// token's `word_0x2E_46` goes **182 → 0 in that one tick**. The port
/// left `flags` at 12 throughout and decremented 182 → 181 → … , so
/// its `sub_68DE0` mid-burst pin held the rival's `manaRegen` at 0
/// FOREVER: retail's `d88` reads 100 from t=13120 and mana climbs
/// +100/tick, the port's purse froze at 15516. The graded head is
/// `slot 530 mana: retail 15616 / port 15516` at t=13121 — Δ exactly
/// one regen quantum, and the divergence that produced it sat 15
/// ticks earlier in the UNGRADED `flags.b0_x20` / `f2e` lanes.
///
/// `MGC_NO_RIVAL_INVIS_CLOAK_EDGE=1` restores the old behaviour.
fn rival_invis_cloak_edge_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_INVIS_CLOAK_EDGE").is_some())
}
/// `sub_6B1C0`'s cloak edge (EF:57090-94, corrected against
/// `NETHERW.EXE` 0x8FA03-0x8FA72 — see
/// [`rival_invis_cloak_edge_off`]), shared by the two rival
/// manifestation call paths. `m` is the `(15,11)` token, `caster` the
/// wizard slot, `first` retail's `word_0x2E_46 == word_0x30_48`.
/// ⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED.
fn mc2_invis_cloak_edge(g: &mut Gen, m: usize, caster: usize, first: bool) {
    if rival_invis_cloak_edge_off() || caster == 0 || caster >= g.ent.len() {
        return;
    }
    if first {
        g.ent[caster].flags |= 0x20;
    } else if g.ent[caster].flags & 0x20 == 0 {
        g.ent[m].f26 = 1;
    }
}

/// A/B toggle for the BULLY ARM's missing `sub_146C0(ix, 2u)` term
/// (absence 2 in [`World::mc2_rival_pick_wizard`]): set
/// ⭐⭐⭐ **`sub_146C0(a1x, s)` IS THE MANIFESTATION REGISTER, NOT THE
/// AUTHORED `known[s]` GRANT — AND THE RIVAL CASCADE ASKS IT AT THREE
/// GATES.** `sub_146C0` (EF:6404-11) is
/// `Entities[a1x->wizext->str_611.SpellsEnabled[s]]`, null-checked
/// against `Entities[0]` — exactly the port's `book.ent[s] != 0`.
/// `sub_14030`'s bully arm already asks it (see
/// [`rival_bully_castle_spell_off`]); `sub_13B00` (the BUILD arm) and
/// `sub_13CE0` (the BALL arm) asked `known[..]` instead. **A law
/// landed on one call path is not landed.**
///
/// Shipped `NETHERW.EXE` 0x38300 = `sub_13B00` (linear 0x13B00,
/// `off = 0x34800 + linear − 0x10000`):
///
/// ```text
///   3830c  8b 87 a4 00 00 00   mov  0xa4(%edi),%eax   ; own wizext
///   38312  66 8b 50 3a         mov  0x3a(%eax),%dx    ; CastleEntityIndex
///   3831c  66 85 d2            test %dx,%dx
///   3831f  0f 85 1d 01 00 00   jne  0x38442           ; has a keep → return 0
///   38325  6a 02               push $0x2
///   38327  57                  push %edi
///   38328  e8 93 0b 00 00      call 0x38ec0           ; sub_146C0(self, 2)
///   38330  85 c0               test %eax,%eax
///   38332  0f 84 0a 01 00 00   je   0x38442           ; NO TOKEN → return 0
///   38338  6a 02               push $0x2
///   3833a  57                  push %edi
///   3833b  e8 f0 1b 00 00      call 0x39f30           ; sub_15730(self, 2)
/// ```
///
/// `sub_146C0` at file 0x38EC0 is `mov 0xa4(%eax),%eax` /
/// `movswl 0x333(%eax,%edx,2),%eax` / `mov 0x1a3e4(,%eax,4),%eax` /
/// `cmp` against `Entities[0]`; `sub_15730` at 0x39F30 calls it and
/// DEREFERENCES the result (`mov 0x8c(%eax),%ebx`) with no null test
/// of its own — the `&&` short-circuit at 0x38332 is load-bearing.
///
/// And `sub_13CE0` (EF:6129-45) is
/// `if (sub_146C0(a1x, 1u)) { v2x = sub_146C0(a1x, 2u);
///  if (v2x) { if (maxMana <= v2x->maxMana) pick; } else pick; }` —
/// both gates the same register, and the ceiling comparison against
/// the castle TOKEN's own `maxMana_0x8C_140`.
///
/// A wizard that has lost BOTH its keep and its Create-Castle token
/// (`sub_605E0`'s level-0 purge, see
/// [`crate::engine::world::mc2_castle_purge_inline_off`]) is a beaten
/// wizard, not a builder: retail drops it straight through to the
/// ball loop. The port re-entered Build every think tick — freezing
/// `target96` and steering at a 4x4 sector corner — and then skipped
/// the ball arm because it measured its ceiling against the ladder
/// (closed separately by [`rival_castle_token_price_off`]).
///
/// WITNESS — mc2l22 t=3838..3840, one tick past the wall the purge
/// ordering law opens. Rival 451's castle 476 died at t=3837 and its
/// token 454 was purged in the same tick; retail keeps re-electing
/// mana balls (`target96` 904 → 975, `ai_state` 13 → 6). The port's
/// `target96` FROZE at 236 from t=3836 and `dump-state … 3839 451
/// --port` printed `dest_x/dest_y retail 0 port 49152` (= 3 << 14,
/// `sub_13B00`'s own `(sector & 3) << 14`) beside `target96 retail
/// 904 port 236`. With the Build arm refused it becomes one row,
/// `target96 retail 904 port 251` — the ball arm, refused by
/// `known[2]`.
///
/// `MGC_NO_RIVAL_CASCADE_TOKEN_GATE=1` restores the `known[..]` form.
fn rival_cascade_token_gate_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_CASCADE_TOKEN_GATE").is_some())
}

/// ⭐⭐⭐ A/B toggle for THE CASTLE TOKEN'S **STAMPED** PRICE. Every
/// affordability gate in the rival cascade reads
/// `sub_146C0(a1x, 2u)->maxMana_0x8C_140` — the number `SetSpell_6D5E0`
/// cached on the class-15 manifestation — and NOT a recomputed ladder
/// rung: `sub_15730` (EF:7159, the BUILD arm's gate through
/// `sub_13B00` EF:6071) is `a1x->maxMana_0x8C_140 >=
/// sub_146C0(a1x, a2)->maxMana_0x8C_140`; `sub_155E0` (EF:7117 with a
/// castle, EF:7128 without — the UPGRADE arm) is
/// `a1x->maxMana_0x8C_140 >= v1x->maxMana_0x8C_140` with
/// `v1x = sub_146C0(a1x, 2u)`; and `sub_13CE0` (EF:6135, the BALL arm)
/// is `a1x->maxMana_0x8C_140 <= v2x->maxMana_0x8C_140`.
///
/// SHIPPED `NETHERW.EXE`, `sub_13CE0` at file **0x384E0** (linear
/// 0x13CE0, `off = 0x34800 + linear - 0x10000`) — the whole arm:
///
/// ```text
///   384e8  6a 01 / 53 / e8 d0 09 00 00   push $1; push %ebx; call 0x38ec0  ; sub_146C0(a1x,1)
///   384f3  85 c0 / 0f 84 bb 00 00 00     no possess token -> return 0
///   384fb  6a 02 / 53 / e8 bd 09 00 00   push $2; push %ebx; call 0x38ec0  ; sub_146C0(a1x,2)
///   38506  85 c0 / 74 5f                 NO castle token -> 0x38569, the unconditional pick
///   3850a  8b 93 8c 00 00 00             mov 0x8c(%ebx),%edx   ; a1x->maxMana
///   38510  3b 90 8c 00 00 00             cmp 0x8c(%eax),%edx   ; vs THE TOKEN's @0x8C
///   38516  0f 8f 98 00 00 00             jg  <skip>            ; proceed iff self <= token
///   3851c  53 / e8 be 0b 00 00           push %ebx; call 0x390e0 ; sub_148E0, the ball pick
/// ```
///
/// and `sub_15730` at file **0x39F30**: `call 0x38ec0` then
/// `mov 0x8c(%ebx),%edx` / `mov 0x8c(%eax),%ebx` / `cmp %ebx,%edx` /
/// `setge %al`. ⭐ THE SHIPPED EXE OUTRANKS THE LISTING, and here it
/// AGREES with it.
///
/// The stamp is NOT the bare rung. `GetSpellManaCost_6D710`
/// (Level.cpp:1761-70) applies the TIER MULTIPLY — ×320>>8 at tier 1,
/// ×384>>8 (= 1.5×) at tier 2 — on top of
/// [`crate::mc2::castle::MC2_CASTLE_COST`], and
/// [`World::mc2_rival_set_spell`] already writes that product to the
/// token's `max_life` (the port's home for @0x8C on a class-15 record).
/// `mc2_castle_ladder_cost` recomputes the rung ALONE, so at spell
/// tier 2 every gate was asked against a price 1.5× too small.
///
/// ⭐⭐ **A CLEAN RATIO IS A MISSING MULTIPLIER** (round 103's castle
/// readiness gate, same shape): mc2l22 t=22736, rival 530 holds
/// `maxMana` 50,836 with castle 556 at rung 3; its token slot 533
/// carries 60,000 (= 40,000 × 384 >> 8) where the ladder says 40,000.
/// Retail's `sub_13CE0` reads 50,836 <= 60,000 TRUE and takes the ball
/// arm (`sub_148E0`, `ai_state` 6 Possess, `target96` 960 — a (10,39)
/// at z 4384, BELOW the rival's floor, so `mc2_rival_hover` steps
/// z −4); the port read 50,836 <= 40,000 FALSE, fell through to arm 9
/// `sub_14530` and elected the (5,23) creature slot 221 at z 6912,
/// climbing — so the hover stepped +4 and the wizard turned after it.
/// That one wrong arm is the whole 22736..22772 reset run.
///
/// `MGC_NO_RIVAL_CASTLE_TOKEN_PRICE=1` restores the ladder recompute.
fn rival_castle_token_price_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_CASTLE_TOKEN_PRICE").is_some())
}

/// `MGC_NO_RIVAL_BULLY_CASTLE_SPELL` to restore the pre-dig two-term
/// conjunction, which read every castle-less poor wizard as a bully
/// target whether or not it still held the castle manifestation.
/// EF:6264-67; `NETHERW.EXE` 0x3896e-0x38982.
fn rival_bully_castle_spell_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_BULLY_CASTLE_SPELL").is_some())
}

/// ⭐⭐⭐ A/B toggle for SHIELD III'S PRE-DECREMENT on the RIVAL
/// column: set `MGC_NO_RIVAL_SHIELD3_PREDECREMENT` (or the human
/// column's `MGC_NO_MC2_SHIELD3_PREDECREMENT`, which restores the
/// shared skeleton on both) to bill Shield III like Shield I/II.
///
/// `sub_6A480`'s two arms are the same four statements in a DIFFERENT
/// ORDER: `life_0x1A == 0` runs `byte[1] |= 0x40 ; sub_68DE0 ;
/// word_0x2E_46--` (EF:56513-15; shipped `NETHERW.EXE` 0x8ecf8 `call
/// 0x8d5e0` then 0x8ecfd `dec`), while `life_0x1A == 1` — the tier-2
/// row, Shield III — runs `byte[2] |= 0x40 ; word_0x2E_46-- ;
/// sub_68DE0` (EF:56525-28; 0x8ed29 `dec` then 0x8ed34 `call`). The
/// debit inside `sub_68DE0` (EF:55569) is keyed on
/// `word_0x2E_46 == word_0x30_48`, so on the pre-decrement arm that
/// compare is FALSE BY CONSTRUCTION: **Shield III's cost is never
/// charged**, the call falls into the else (`if (v2 && manaRegen > 0)
/// manaRegen = 0`), and because `v2` is the POST-decrement counter the
/// tick the window ENDS on pins nothing.
///
/// The body is CASTER-GENERIC (`sub_6A480` resolves its wizard through
/// `parentId_0x28_40`), so this is the exact twin of the human
/// column's law — see [`crate::mc2::cast::no_mc2_shield3_predecrement`]
/// for the full citations and the rsg witnesses. The rival column has
/// its own PAIR-VISIBLE witness: mc2l22 pair 19865→19866, rival 557
/// arms Shield III (`flags 32780 → 4227084` = `|= 0x400000`, whose
/// sole writer in the whole decompile is EF:56526) with retail's mana
/// holding at 11088 where the port paid down to 1088.
///
/// ⚠ THE LAW LIVES ON **BOTH** RIVAL TOKEN PATHS. A level-load book
/// sits ABOVE its wizard, so its debit is pre-consumed in
/// [`World::mc2_rival_cast`] and its pin lands in
/// [`World::mc2_rival_buffs`]; a respawn-minted book sits BELOW and
/// runs its real body in [`World::mc2_rival_token_tick`]. ⭐ A LAW
/// LANDED ON ONE CALL PATH IS NOT LANDED.
fn no_rival_shield3_predecrement() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_RIVAL_SHIELD3_PREDECREMENT").is_some()
            || crate::mc2::cast::no_mc2_shield3_predecrement()
    })
}

/// A/B toggle for **THE RIVAL LIFE REGEN FLOORS AT −1, EXACTLY LIKE
/// THE MC1 TWIN**: set `MGC_NO_MC2_RIVAL_LIFE_FLOOR` to restore the
/// pre-dig behaviour, where [`World::mc2_rival_alive`]'s regen step
/// applied only the `maxLife` CEILING and no floor. `sub_12A70`
/// (EF:5427-5430) is `life += lifeRegen; if (life < -1) life = -1;
/// if (life > maxLife) life = maxLife;` — shipped `NETHERW.EXE`
/// file 0x37478 (`0f bf 80 63 01 00 00` movsx eax,[eax+0x163]),
/// 0x3747f `01 c1`, 0x37481 `89 4b 08`, **0x37484 `83 f9 ff` cmp
/// ecx,-1 / 0x37487 `7d 07` jge / 0x37489 `c7 43 08 ff ff ff ff`
/// mov dword [ebx+8],-1**, then the ceiling at 0x37490-0x3749a.
/// The MC1 twin `sub_132B0` carries the identical shape at
/// `CARPET.EXE` file 0x2BCE9 `83 f9 ff` / 0x2BCEC `7d 07` /
/// 0x2BCEE `c7 43 0c ff ff ff ff` (MC1's `life` sits at +0xC, MC2's
/// at +0x8), and [`World::rival_alive_tick`] has ported it as
/// `.clamp(-1, max)` all along — this column was the laggard.
fn no_mc2_rival_life_floor() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_LIFE_FLOOR").is_some())
}

/// `MGC_NO_MC2_RIVAL_MANA_OVERDRAFT=1` restores the pre-dig
/// affordability floor on the CHARGED-SHIELD quarter, where the
/// wizard paid `min(dmg/4, mana)` and its purse could never go below
/// zero.
///
/// ⭐⭐⭐ **RETAIL'S ONLY MANA FLOOR IS AT THE *END* OF THE TICK, AND
/// THE LETHAL LEG RETURNS BEFORE IT.** `sub_5EFA0`'s charged arm
/// (EF:60682-89) is a plain signed subtract with no guard at all —
///
/// ```text
///   v10 = dword_0x5E_94 / 4;            // the quarter
///   v12 = a1x->mana_0x90_144 - v10;     // NO min, NO clamp
///   a1x->str_0x5E_94.dword_0x5E_94 = v10;
///   a1x->mana_0x90_144 = v12;
/// ```
///
/// — and the floor that normally hides it lives twenty statements
/// later in the CALLER, `sub_12A70` (EF:5456-57, `if (mana < 0)
/// mana = 0;`), past `mana += manaRegen` (EF:5424). When the same
/// letter is lethal, `sub_5EFA0` returns 2 and `sub_12A70` takes
/// `actionIndex_0x45_69 = 2; return 0;` (EF:5430-33) — skipping the
/// regen, the rate fork AND the floor. So a rival wizard killed by a
/// hit its charged shield quartered keeps a **negative purse for the
/// rest of its death**, and the port, whose `Mc2Rival::mana` was
/// `u32`, could not represent it.
///
/// DISASSEMBLED, not inferred. Shipped `NETHERW.EXE`, `sub_5EFA0` =
/// file **0x837A0** (linear 0x5EFA0 + 0x24800); the CHARGED arm is
/// file **0x839B0-0x839DA**:
///
/// ```text
///   839b0: 8b 43 5e             mov  0x5e(%ebx),%eax ; dword_0x5E_94
///   839b3: 89 c2                mov  %eax,%edx
///   839b5: c1 fa 1f             sar  $0x1f,%edx      ; SIGN-extend
///   839b8: c1 e2 02             shl  $0x2,%edx
///   839bb: 1b c2                sbb  %edx,%eax
///   839bd: c1 f8 02             sar  $0x2,%eax       ; ARITHMETIC /4
///   839c0: 8b bb 90 00 00 00    mov  0x90(%ebx),%edi ; mana_0x90_144
///   839c6: 8a 53 0d             mov  0xd(%ebx),%dl
///   839c9: 29 c7                sub  %eax,%edi       ; mana - quarter
///   839cb: 89 43 5e             mov  %eax,0x5e(%ebx) ; letter := quarter
///   839ce: 80 e2 bf             and  $0xbf,%dl       ; byte[1] &= 0xBF
///   839d1: 89 bb 90 00 00 00    mov  %edi,0x90(%ebx) ; STORED RAW
///   839d7: 88 53 0d             mov  %dl,0xd(%ebx)
/// ```
///
/// — `sar`/`sub`/`mov` with **no `cmp`, no `jge`/`jl`, no `cmov`**
/// anywhere between the load and the store, and the `/4` is an
/// ARITHMETIC (sign-aware) shift, not a logical one. Twenty-five
/// bytes later `839f3: mov 0x5e(%ebx),%eax / 839f8: mov 0x8(%ebx),
/// %edx / 839ff: sub %eax,%edx / 83a04: mov %edx,0x8(%ebx)` spends
/// the SAME quartered word on `life_0x8` — which is why both
/// witnesses show `life` and `mana` moving by the identical 400.
///
/// And the ordering in the caller, `sub_12A70` = file 0x37270:
///
/// ```text
///   37402: e8 99 c3 04 00   call 0x837a0        ; sub_5EFA0
///   3740a: 83 f8 02         cmp  $0x2,%eax
///   3740d: 75 30            jne  0x3743f
///   3740f: c6 43 45 02      movb $0x2,0x45(%ebx) ; actionIndex = 2
///   37413: 31 d2            xor  %edx,%edx
///   37415: e9 49 02 00 00   jmp  0x37663         ; THE EPILOGUE
///   …
///   37463: 8b 83 88 00 00 00 mov  0x88(%ebx),%eax ; manaRegen
///   37469: 01 83 90 00 00 00 add  %eax,0x90(%ebx) ; mana += regen
///   …
///   37553: 83 bb 90 00 00 00 00  cmpl $0x0,0x90(%ebx)
///   3755a: 7d 0a                 jge  0x37566
///   3755c: c7 83 90 00 00 00 00  movl $0x0,0x90(%ebx)  ; THE FLOOR
///   37566-3757c:                 the maxMana ceiling
///   37663: 89 d0 / 89 ec / 5d …  the epilogue
/// ```
///
/// 0x3740A's `jmp` lands at 0x37663, **past 0x37463 and past
/// 0x37553** — the regen and the only mana floor in the function.
///
/// ⚠ CALL PATHS. `e8 rel32` scan of the shipped image: `sub_5EFA0`
/// has exactly TWO callers — 0x37402 (this one, inside `sub_12A70`)
/// and 0x828E9, inside the HUMAN's `AddPlayer03_00_5E010`, which
/// DISCARDS the result (`jmp 0x82918`) and re-tests `life_0x8`
/// itself at 0x82939. The human column already carries the
/// overdraft through [`World::debit_mana`]'s `owed` → `mana_delta`
/// hand-off, so its ALIVE path is covered; its own lethal tick is
/// unwitnessed in this corpus (the human's purse is six digits) and
/// is NOT touched by this hunk.
///
/// WITNESS — mc2l16, two independent rival wizards 8,000 ticks
/// apart, both landing on the SAME constant: slot 372 at t=11721
/// (`life 100 -> -300`, `mail0.amt 1600 -> 400`, `mana 0 -> -400`)
/// and slot 389 at t=19784 (`life 360 -> -40`, `mail0.amt 1600 ->
/// 2000`, `mana 0 -> -400`). Both are a 1600-point letter quartered
/// to 400 against an empty purse on the tick it kills; retail's
/// `d88` goes 100 -> 0 in the same instant (an ungraded lane here).
/// The port read 0 at both.
pub(crate) fn no_rival_mana_overdraft() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_MANA_OVERDRAFT").is_some())
}

/// A/B toggle for THE FALL VELOCITY THAT IS NEVER RESET: set
/// `MGC_NO_RIVAL_FALL_CARRY` to restore the pre-dig behaviour, where
/// the lethal branch of `mc2_rival_alive` stamped `f46 = 0` (the
/// wizard's `word_0x2C_44`) alongside `actionIndex = 2`. See the
/// write site for the citations.
fn no_rival_fall_carry() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_FALL_CARRY").is_some())
}

/// A/B toggle for **THE KNOCKBACK BEARING READS A FREED SOURCE**: set
/// `MGC_NO_KNOCK_FROM_DEAD_SOURCE` to restore the pre-dig guard, where
/// the MC2 wizard-damage arm only stamped `yaw_0x1E_30`/`moveBoost` when
/// the mail source was still a LIVE pool record. Retail's gate is
/// `word_0x62_98 != 0` alone (EF:61012/61037). See the write site.
fn no_knock_from_dead_source() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_KNOCK_FROM_DEAD_SOURCE").is_some())
}

/// A/B toggle for **THE DEAD-WAIT DISPATCH CLEARS THE KNOCK MAGNITUDE
/// EVERY TICK**: set `MGC_NO_MC2_DEAD_WAIT_KNOCK_CLEAR` to restore the
/// pre-dig behaviour, where a dead MC2 wizard kept whatever
/// `moveBoost_0x1E_30` its killing letter stamped for the whole 1,200
/// tick respawn countdown. `sub_5E7C0` (EF:60660) opens with
/// `a1x->dword_0xA4_164x->moveBoost_0x1E_30 = 0` **above** its
/// AI/human and castle/banished branches — shipped `NETHERW.EXE` file
/// 0x82FC9 `8b 86 a4 00 00 00` = `mov 0xa4(%esi),%eax` / 0x82FCF
/// `66 c7 40 1e 00 00` = `movw $0x0,0x1e(%eax)`, the FIRST two
/// instructions after the prologue, with the `IsAiPlayer` test only at
/// 0x82FEB. See [`World::mc2_rival_dead_wait`] and the eliminated arm
/// of [`World::mc2_rival_entity_tick`].
fn no_mc2_dead_wait_knock_clear() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_DEAD_WAIT_KNOCK_CLEAR").is_some())
}

/// A/B toggle for **THE RIVAL DUEL LOCK'S DEATH-FALL TETHER**: set
/// `MGC_NO_MC2_RIVAL_DUEL_DEATH_TETHER` to restore the pre-dig
/// behaviour, where a rival wizard had no duel lock register at all
/// and `mc2_rival_carpet_move`'s doc comment listed `sub_5DE30` under
/// "NOT reproduced".
///
/// ⭐⭐⭐ **A RIVAL'S DUEL LOCK IS INERT WHILE IT LIVES AND LIVE WHILE
/// IT DIES — AND THE CALL GRAPH IS WHY.** `sub_5DE30` is the duel
/// enforcement (the leash plus the opponent DRAIN). Scanning
/// `NETHERW.EXE` for `e8 <rel32>` (file = VA + 0x24800) gives the
/// whole graph, and it is three edges wide:
///
/// ```text
///   sub_5DE30  (file 0x82630)  callers: 0x82163  = VA 0x5D963  -> INSIDE sub_5D530
///   sub_5D530  (file 0x81D30)  callers: 0x82934  = VA 0x5E134  -> AddPlayer03_00_5E010
///                                       0x82B1D  = VA 0x5E31D  -> sub_5E310, +0xD = ITS FIRST STATEMENT
///   sub_12A70  (the AI brain)  callers: VA 0x12918 (sub_12910) only
///   sub_146F0  (the AI mover)  callers: VA 0x12C40 (sub_12A70) only
/// ```
///
/// So the enforcement has exactly TWO reachable seats: the live
/// wizard body `AddPlayer03_00_5E010` — which an AI wizard NEVER
/// runs, its action-0 entry being `sub_12A70` → `sub_146F0`, a mover
/// with no duel leg — and `sub_5E310`, THE DEATH FALL, which is one
/// shared function for both columns (the port already models its
/// opening `sub_5D530` call, [`World::mc2_rival_death_fall`]). A
/// rival that holds `word_0x146_326` therefore does nothing with it
/// for its whole life and then, on every tick of its death fall,
/// slews toward its opponent and bills them.
///
/// The body (file 0x82630-0x827E9), with the operands this arm
/// reproduces:
///
/// ```text
///   82644  66 8b 9a 46 01 00 00  mov 0x146(%edx),%bx     ; word_0x146_326 = the lock
///   8265a  39 cb / 0f 86 ..      cmp/jbe 0x827EB         ; lock 0 -> return, NO clear
///   82662  0f bf 82 4a 01 00 00  movswl 0x14a(%edx)      ; word_0x14A_330 = the TIER
///   82676  66 8b 8a 4f 03 00 00  mov 0x34f(%edx),%cx     ; SpellEnabled[14] = manifestation
///   82694  e8 57 a5 ff ff        call 0x7CBF0            ; sub_583F0_distance_3d (3-D)
///   826a6  66 83 7f 2e 00        cmpw $0x0,0x2e(%edi)    ; manifestation charged?
///   826bc  83 7b 08 00 / jl      cmpl $0x0,0x8(%ebx)     ; VICTIM life >= 0  (never the caster's)
///   826c6  ..0x84(%esi)..        minSpeed 80 -> cap 120, divisor 1024/120 = 8
///   8273a  e8 a1 a2 ff ff        call 0x7C9E0            ; tan2 bearing
///   8275e  e8 ed a3 ff ff        call 0x7CB50            ; sub_58350 yaw servo, rate 0x82
///   82785  e8 16 a0 ff ff        call 0x7C7A0            ; MoveEntity_57FA0 onto 0x1B398
///   8278d  8a 40 18              mov 0x18(%eax),%al      ; row.life_0x1A = the DRAIN MODE
///   82793  3c 01 / 72 54         cmp $1 / jb  0x827EB    ; mode 0 -> no drain
///   82797  76 1c                 jbe 0x827B5             ; mode 1 -> mana only
///   82799  3c 02 / 75 4e         cmp $2 / jne 0x827EB    ; mode > 2 -> return
///   827a3  0f bf 80 63 01 00 00  movswl 0x163(%eax)      ; victim lifeRegen_0x163_355
///   827ad  83 c0 02 / 29 c1      life -= lifeRegen + 2   ; NO floor
///   827b5  8b 83 88 00 00 00     victim manaRegen_0x88_136
///   827c1  83 c0 08 / 29 c6      mana -= manaRegen + 8
///   827cc  85 f6 / 7d 1b         test/jge -> movl $0x0   ; the ONE clamp: mana floors at 0
///   827dc  8b 86 a4 00 00 00     mov 0xa4(%esi),%eax     ; the CASTER's own player block
///   827e2  66 c7 80 46 01 00 00  movw $0x0,0x146(%eax)   ; THE CLEAR — the liveness ELSE
/// ```
///
/// The clear is therefore exactly two sites: this `else` (manifestation
/// discharged, out of the tier's range, or the victim dead) and
/// `sub_5C950`'s respawn, file 0x81662 `movw $0x0,0x146(%eax)` beside
/// its 0x81671 twin for `word_0x148_328`. NOTHING clears it at the
/// kill — the lock survives the fatal letter, the whole fall, and the
/// landing payout, and the 26-token scatter's `SpellEnabled[i] = 1`
/// then makes the manifestation test read pool slot 1 (retail's own
/// out-of-bounds class), which is what usually trips the `else` one
/// tick after touchdown.
///
/// ⚠ **UNWITNESSED, AND STRUCTURALLY SO ON THIS COLUMN.** Spell 14 is
/// in none of the AI's pick tables ([`ATTACK_WIZARD`],
/// [`ATTACK_CASTLE`], the Home/Defense immediates), so no retail AI
/// ever stamps its own lock in single player; the register can only
/// arrive through the conformance import, which is why this dig also
/// seats it ([`Mc2RivalAi::duel_target`]) and grades it
/// ([`Mc2Rival::wiz_shadow_lanes`]). Ported on the player's ruling of
/// 2026-09-18 because it is player-visible the moment a lock exists.
pub(crate) fn no_mc2_rival_duel_death_tether() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_DUEL_DEATH_TETHER").is_some())
}

/// A/B toggle for **THE KNOCK BEARING IS STORED RAW, NOT MASKED TO 11
/// BITS**: set `MGC_NO_MC2_KNOCK_DIR_RAW` to restore the pre-dig
/// `& 0x7FF`. `sub_5EFA0` writes the tangent's full 16-bit return
/// straight into `yaw_0x1E_30` — shipped `NETHERW.EXE` file 0x83A19
/// `e8 c2 8f ff ff` = `call 0x7C9E0` (`sub_581E0` tan2) / 0x83A1E
/// `8b 93 a4 00 00 00` / 0x83A27 `66 89 42 20` = `mov %ax,0x20(%edx)`,
/// with no `and` in between — and `angle_of` returns **2048** on the
/// `-dx < -dy` quadrant edge, which the port folded to 0. The consumer
/// (`MoveEntity_57FA0`) masks at use, so this is a register-state law
/// only. Witness mc2l22-new t=3421 wiz 6: retail 2048, port 0.
pub(crate) fn no_mc2_knock_dir_raw() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_KNOCK_DIR_RAW").is_some())
}

/// A/B toggle for **`sub_12A70`'s LETHAL RETURN IS THE HOUSEKEEPING'S,
/// NOT THE BRAIN'S**: set `MGC_NO_RIVAL_DEATH_BRAIN` to restore the
/// pre-dig behaviour, where [`World::mc2_rival_alive`]'s lethal branch
/// returned from the whole wizard body, so neither the state handler
/// nor the selector ran on the death tick. See the call site for the
/// citations and the witness.
fn no_rival_death_brain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_DEATH_BRAIN").is_some())
}

/// A/B toggle for **INVISIBILITY IS A LIVE WINDOW ON THE
/// MANIFESTATION, NOT A LATCH ON THE WIZARD**: set
/// `MGC_NO_RIVAL_INVIS_WINDOW` to restore the pre-dig behaviour, where
/// [`World::mc2_rival_pick_wizard`] dropped a candidate on the port's
/// persistent `player.invisible` / `Mc2Rival::invisible` booleans
/// instead of retail's `sub_15760(ix, 0xB)`. See the call site.
fn rival_invis_window_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_INVIS_WINDOW").is_some())
}

/// A/B toggle for THE (10,57) SIGNATURE IS THE SLOT, NOT THE CASTER:
/// set `MGC_NO_MC2_M57_SIG_IS_SLOT` to restore the pre-dig behaviour,
/// where [`crate::engine::world::World::mc2_target_sig`] summed the
/// port's FUSED `id24` on a fool's-mana sphere — i.e. retail's
/// `parentId_0x28_40`, not its `id_0x1A_26`.
fn no_m57_sig_is_slot() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M57_SIG_IS_SLOT").is_some())
}

/// A/B toggle for the (10,40) GRAVE's MC2 mail-mask home: set
/// `MGC_NO_MC2_GRAVE_MASK_HOME` to restore the pre-dig ctor, which
/// wrote only the MC1 `@0x38` home (`f28`) and left the class-2/10 one
/// (`f56` — the home `import_ent_mc2` seats and `port_ent_lanes_mc2`
/// publishes as `b38`) at zero. Citation and receipts at the call site
/// in [`Gen::mc2_spawn_grave`].
fn no_mc2_grave_mask_home() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_GRAVE_MASK_HOME").is_some())
}

/// A/B toggle for THE RE-DETECT TICK THAT PROBES NOTHING: set
/// `MGC_NO_RIVAL_STEER_REDETECT_DEFER` to restore the pre-dig
/// behaviour, where [`World::mc2_rival_water_steer`] zeroed the avoid
/// counter and re-classified IN THE SAME TICK. See the call site for
/// the citations and the witness.
pub(crate) fn no_rival_steer_redetect_defer() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_STEER_REDETECT_DEFER").is_some())
}

/// A/B toggle for THE STEER'S TARGET IS THE ENTITY WORD, READ RAW:
/// set `MGC_NO_STEER_TARGET_ENTITY_WORD` to restore the pre-dig
/// behaviour, where [`World::mc2_steer_target_tile`] read the BRAIN
/// record's mirrored target behind an invented `flags & 0x400`
/// liveness guard and an invented "8 tiles ahead along the heading"
/// fallback. See the call site for the citations and the witness.
pub(crate) fn no_steer_target_entity_word() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_STEER_TARGET_ENTITY_WORD").is_some())
}

/// ⭐⭐⭐ AN INVENTED GUARD IS FALSE EXACTLY WHEN IT MATTERS — and
/// this is the THIRD copy of the tick-top-roster law in this file,
/// after `dword_38519`'s raid pick and `dword_38523`'s ball chain.
///
/// `sub_15FC0`'s second scan (EF:7664-81) walks the per-model class-5
/// roster `bytearray_38403x[model]` (EF:7669) and re-asks exactly ONE
/// thing: `kx->id_0x1A_26 != a1x->id_0x1A_26`. No life test, no action
/// test and **no `flags & 0x400` test** — all of that is MEMBERSHIP,
/// decided once per frame when `UpdateEntities_57730` rebuilds the
/// roster (EF:39987-40008, `life >= 0` plus the corpse-action skip).
/// And the reap term is absent for a reason: that rebuild is preceded
/// by the frame's own reap pre-walk (EF:39948-56 → `sub_57F20`), which
/// has ALREADY FREED every record flagged `0x400` last frame. So a
/// live `0x400` at scan time can only have been raised during the
/// CURRENT frame — and retail's snapshot still contains it. The port
/// re-scanned the live pool and skipped it, which is the real rule's
/// sign backwards.
///
/// `MGC_NO_RIVAL_DISGUISE_ANCHOR_LIST=1` restores the skip.
fn rival_disguise_anchor_list_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_DISGUISE_ANCHOR_LIST").is_ok_and(|v| v == "1"))
}

/// ⭐⭐⭐ `sub_15FC0`'s DISGUISE-ANCHOR SCAN WALKS THE TICK-TOP
/// PER-MODEL ROSTER CHAIN, NOT THE LIVE POOL.
///
/// EF:7662-73: `for (kx = x_D41A0_BYTEARRAY_4_struct.bytearray_38403x[v9];
/// kx > Entities_EA3E4[0]; kx = kx->next_0)` with ONE filter,
/// `kx->id_0x1A_26 != a1x->id_0x1A_26`. Every life / state / class /
/// model predicate the port carried inline is MEMBERSHIP, sampled once
/// per frame by `UpdateEntities_57730`'s rebuild (EF:39987-40008) —
/// the same law [`rival_disguise_anchor_list_off`] already established
/// for the reap term, and the same law
/// [`crate::engine::features::MobChains`] already models for MC1 and
/// for the MC2 muzzle/summon walks. The tier the winner arms is
/// `v1 = *j`, the CHAIN'S model byte (EF:7676-7708) — never the
/// record's live `model_0x40_64` — which is why a record that changed
/// class mid-frame is still scored, and scored as its tick-top model.
///
/// WITNESS — mc2l22's certification frontier, pair 1644→1645. Rival
/// 557 sits in Defense on anchor 37 (a `(5,2)`). During tick 1644
/// rival 530 (a LOWER pool slot, so earlier in the ascending walk)
/// converts slot 947 from a `(9,13)` into a `(5,2)` summon AT ITS OWN
/// POSITION. The port re-scanned the live pool, found the brand-new
/// `(5,2)` at distance **0** from the reference wizard, and re-pointed
/// `word_0x96_150` 37 → 947 one tick early; retail's class-5 model-2
/// chain was built at the top of 1644, while 947 was still a `(9,13)`,
/// so retail keeps 37 for one more tick and re-points at 1645
/// (`explain mc2l22 1645 557`: `target96 37 -> 947`, `f98 679 -> 1172`).
/// `target96` is UNGRADED, so the whole wall reads as
/// `(3,1) slot 557: z` — the wizard steering at a different anchor.
///
/// `MGC_NO_RIVAL_ANCHOR_ROSTER=1` restores the live-pool walk.
fn rival_anchor_roster_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_ANCHOR_ROSTER").is_ok_and(|v| v == "1"))
}

/// ⭐⭐ `sub_583F0_distance_3d` RETURNS AN INTEGER SQUARE ROOT — A
/// TRUNCATED **LINEAR** DISTANCE — AND THESE TWO SCANS RANKED SQUARES.
///
/// The decompile only DECLARES `sub_583F0` (EventsFunctions.cpp:822);
/// it is never defined, so the port had to guess its units, and here
/// it guessed "squared". The shipped EXE settles it. `NETHERW.EXE`
/// file **0x7CBF0** (linear 0x583F0) builds `dx² + dy² + dz²` from
/// three 16-bit wrapping deltas (`movswl`) and then
/// `push %eax; call 0x96f7a; add $0x4,%esp` — and file **0x96f7a** is
/// a Newton integer square root: `bsr` the operand, seed from the
/// 32-entry `u16` table at file **0x96FB0**
/// (`1,2,2,4,5,8,0xB,0x10,…,0xB504,0xFFFF` — byte-identical to the
/// port's [`crate::mc1::tables::BIT_SQRT`]), then
/// `x = n/g; if x >= g return g; else g = (x+g)>>1`. Verified
/// exhaustively over `0..2^22`, over every `m²−1/m²/m²+1` boundary for
/// `m < 65536` and over 400k random `u32`: it is **exactly
/// `floor(sqrt(n))`**, so [`Gen::isqrt`] reproduces it bit-for-bit.
/// ⚠ Its sibling `EuclideanDistXY_584D0` (file 0x7CCD0) returns
/// `dx² + dy²` RAW — the two helpers in one file disagree about units,
/// which is why "WHICH DISTANCE FUNCTION A SITE CALLS IS A LAW".
///
/// The port already models `sub_583F0` correctly nearly everywhere:
/// [`Gen::mc2_dist3`] (mc2/mobs.rs) is `isqrt(dx²+dy²+dz²)` and ~38
/// call sites use it. The exceptions were both here — the inline
/// closures in [`World::mc2_nearest_wizard`] and in
/// [`World::mc2_rival_pick_defense`]'s anchor scan — and they leak in
/// two distinct ways:
///
/// 1. **TRUNCATION IS THE TIE-BREAKER.** Both scans keep the running
///    best on a STRICT `<` (`cmp %edx,%eax; jae <skip>` at 0x3a867,
///    0x3a8cb, 0x3ab4a), so the FIRST candidate in walk order wins a
///    tie. Retail ties whenever two candidates share an integer
///    bucket; the port, comparing exact squares, splits those ties and
///    can elect a different record.
/// 2. **A `>` GATE IS OFF BY A SQUARE ROOT.** `isqrt(D) >= C` really
///    is `D >= C*C`, and `isqrt(D) < C` really is `D < C*C` — so the
///    port's `>=`/`<` gates were accidentally exact. But
///    `isqrt(D) > C` is `D >= (C+1)²`, NOT `D > C²`. Retail's
///    `sub_15FC0` scan-1 gate is `cmpl $0x1400,-0xc(%ebp); jbe` at
///    file **0x3a87b** (EF:7657) — i.e. drop when `isqrt(D) > 5120`,
///    which keeps the whole band `D ∈ (5120², 5121²]` that the port
///    dropped. `sub_161A0`'s lower band edge `cmp $0xa00,%esi; jbe` at
///    file **0x3ab74** (EF:7801) has the same shape. The upper edges
///    (`jae $0x1400` at 0x3a8fc / 0x3ab68, EF:7676/7799) are exact
///    either way.
///
/// `MGC_NO_MC2_DIST3D_ISQRT=1` restores the squared-space scans.
fn mc2_dist3d_isqrt_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_MC2_DIST3D_ISQRT").is_ok_and(|v| v == "1"))
}

/// A root-space threshold constant re-expressed in whatever key space
/// [`mc2_dist3d_isqrt_off`] currently has the scans ranking in.
fn mc2_d3_gate(c: i64) -> i64 {
    if mc2_dist3d_isqrt_off() { c * c } else { c }
}

/// `sub_583F0_distance_3d` (`NETHERW.EXE` file 0x7CBF0), transcribed.
///
/// ⚠ NOT `Gen::mc2_dist3` (mc2/mobs.rs), which computes the sum in
/// `i32` and takes `dz` as a FULL difference. Retail takes all three
/// deltas as 16-bit wrapping subtractions and sign-extends them
/// (`mov (%esi),%ax; sub (%ecx),%ax; … movswl %dx,%eax`), so the sum
/// is bounded by `3 * 32768² = 0xC0000000` — it overflows `i32` (retail
/// just wraps and the callee reads the operand UNSIGNED) and it
/// overflows `mc2_dist3`'s `i32`, which panics under debug
/// overflow-checks the moment two wizards are far enough apart on both
/// horizontal axes. Doing the sum in `i64` and narrowing to `u32` is
/// the same 32-bit value retail hands `0x96f7a`.
fn mc2_sub_583f0(a: (u16, u16, i16), b: (u16, u16, i16)) -> u32 {
    let dx = (b.0.wrapping_sub(a.0) as i16) as i64;
    let dy = (b.1.wrapping_sub(a.1) as i16) as i64;
    let dz = ((b.2 as u16).wrapping_sub(a.2 as u16) as i16) as i64;
    Gen::isqrt((dx * dx + dy * dy + dz * dz) as u32)
}

/// Metamorph tier for a disguise model (`SetSpell` switch,
/// EF:7685-7711): 2/0x13 → tier 0, 0x19 → 1, 0x10 → 2.
fn mc2_disguise_tier(model: u8) -> u8 {
    match model {
        0x19 => 1,
        0x10 => 2,
        _ => 0,
    }
}
/// Raid-castle offense gate `sub_164B0` (EF:6182 caller).
const OFFENSE_RAID: [u8; 9] = [0x11, 0x10, 0x12, 0x07, 0x09, 0x14, 0x13, 0x15, 0x00];
/// Attack-wizard/balloon/hunt offense gate `sub_15E60` (EF:6233).
const OFFENSE_ATTACK: [u8; 7] = [0x00, 0x07, 0x12, 0x10, 0x14, 0x15, 0x09];

// ---- the water-steer static tables (open-closure §1.0, EF:1074-79).
// Step deltas for the 40-step detour march, indexed by the probe
// code's LOW byte (0xff = -1).
const STEER_DX_L: [i8; 14] = [0, -1, 0, -1, 1, -1, 0, 0, 0, 0, -1, 0, 0, 0];
const STEER_DY_L: [i8; 14] = [0, 0, -1, 0, 0, 0, -1, -1, 1, 1, 0, 1, 0, 0];
const STEER_DX_R: [i8; 14] = [0, 1, 0, 0, -1, 1, -1, 0, 0, 1, 1, 0, 0, 0];
const STEER_DY_R: [i8; 14] = [0, 0, 1, 1, 0, 0, 0, 1, -1, 0, 0, 1, -1, 0];
/// Escape yaw when committed LEFT — `x_WORD_D3FCE` (EF:1078).
const STEER_YAW_L: [u16; 13] = [
    0, 1536, 0, 1536, 512, 1536, 0, 0, 1024, 1024, 1536, 1024, 512,
];
/// Escape yaw when committed RIGHT — `x_WORD_D3FE8` (EF:1079).
const STEER_YAW_R: [u16; 14] = [
    1024, 512, 1024, 1024, 1536, 512, 1536, 1024, 0, 512, 512, 1024, 0, 0,
];

/// The MC2 rival wizard names by color (`WizardsNames_D93A0`,
/// GameUI.h:39; `GetTrueWizardNumber` is identity in single player).
pub const MC2_RIVAL_NAMES: [&str; 8] = [
    "Zanzamar", "Nyphur", "Rahn", "Belix", "Jark", "Elyssia", "Yragore", "Prish",
];

/// The AI wizard's tuning row: the rival ctor PINS `str_D7BD6[67]`
/// (sub_4A9C0 EF:33351), overriding the spawn law 59+model=60. Row 67
/// carries the retail AI band (ceiling ground+768, floor ground+128),
/// turn caps (v_4 5, v_2 256), climb -4 and the 8192 engagement range
/// v_28; row 60's 1792/0/22/4096 are wrong for the brain's consumers.
pub(crate) const WIZARD_ROW: u8 = 67;

/// `MGC_NO_RIVAL_BUFF_ORDER=1` — replay the pre-dig placement, where
/// the manifestation-window countdown stand-in ran BEFORE the rival's
/// brain instead of after the whole wizard body.
pub(crate) fn rival_buff_order_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_BUFF_ORDER").is_ok_and(|v| v == "1"))
}

/// `MGC_NO_RIVAL_REGEN_PIN_LIST=1` — restore the pre-dig behaviour,
/// where the RIVAL column pinned the caster's regen accumulator on
/// every mid-burst tick of every spell, ignoring
/// [`NO_MID_BURST_REGEN_PIN`].
/// `MGC_NO_DEATH_PAYOUT_VICTIMS=1` restores the pre-dig death payout,
/// which rebuilt only the FREE half of `sub_49F90`. Retail's payout
/// `sub_5E310` (EF:60101) is the ONE of NINE `sub_49F90` call sites
/// that never clears `dword_0x11e6` — the other eight do (EF:32995,
/// 38829, 38874, 39401, 39468, 43858, and 61283 conditionally) — so
/// the victim list it builds stays ARMED for the rest of the level,
/// and its last statement is a bare `--dword_0x11e6` (EF:60179).
/// ⚠ PAIR-BLIND BY CONSTRUCTION: the importer overwrites
/// `mc2_recycle.stack` from the recording at every pair boundary
/// (`world/conformance.rs`, `refill = false`), so this wants a UNIT
/// TEST, not a fixture. The switch exists so that test can prove
/// itself non-vacuous. Added 2026-09-04 — the law had landed without
/// one.
fn no_death_payout_victims() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_DEATH_PAYOUT_VICTIMS").is_some())
}

/// A/B toggle for the RIVAL CORPSE'S STUCK NUDGE (round 139): set
/// `MGC_NO_MC2_RIVAL_STUCK_NUDGE` to restore the pre-dig behaviour, in
/// which a refused commit gate left the corpse exactly where it was.
///
/// `sub_5DD50` (EF:60157) is called from exactly ONE place —
/// `sub_5D530`'s `else` on `moveTest_5D0A0` (EF:60077). Shipped
/// `NETHERW.EXE`: file 0x821F3 `e8 a8 f6 ff ff` = `call sub_5D0A0`,
/// 0x821FB `66 85 c0` / 0x821FE `0f 84 ba 00 00 00` = `test ax,ax` /
/// `je 0x822BE`, and 0x822BE `53` / 0x822BF `e8 8c 02 00 00` =
/// `push a1x` / `call sub_5DD50` (file 0x82550 = VA 0x5DD50).
/// The nudge body: 0x825E6 `bf 98 b3 01 00` + 0x825EB `8d 73 4c` +
/// `a5 66 a5` = `predictedAxis_EB398ar = a1x->position_0x4C_76`,
/// 0x825EE `68 80 00 00 00` = **push 128**, 0x825F8 `6a 00` = pitch 0,
/// 0x825FA `66 8b 43 1c` = `yaw_0x1C_28`, `call 0x7C7A0`
/// (`MoveEntity_57FA0`), then 0x82612 `call 0x7C4F0`
/// (`CopyEntityPosition_57CF0`). The latch is the PLAYER struct's
/// `byte_0x261_609` (0x825DF `c6 80 61 02 00 00 01`, cleared at
/// 0x82622) — the recorder's `nudge_latch`.
///
/// ⭐⭐⭐ A LAW ON ONE CALL PATH IS NOT LANDED. `sub_5D530` has exactly
/// TWO callers in the shipped EXE (an `e8 rel32` scan): VA 0x5E134
/// (the alive wizard tick) and VA 0x5E31D — `sub_5E310`, THE DEATH
/// FALL, shared by the human corpse and the rival corpse. The port
/// split that one mover in two: [`crate::flight::mc2_move`] carries
/// the nudge, [`World::mc2_rival_carpet_move`] did not, so a rival
/// that dies over deep water freezes at the refusal point.
fn no_mc2_rival_stuck_nudge() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_STUCK_NUDGE").is_some())
}

/// A/B toggle for the WATER-STEER MARCH CURSOR'S BYTE WRAP (round
/// 139): set `MGC_NO_MC2_STEER_TILE_WRAP` to restore the pre-dig
/// 32-bit march, which let a detour walk off the map into tile
/// x = 269 and then measured its "distance to target" from there.
///
/// `sub_169C0`'s two 40-step detour cursors are `baxis_2d` BYTE PAIRS
/// (`v12`/`v11`) and every step is an 8-bit add. Shipped
/// `NETHERW.EXE`, the LEFT march (`sub_169C0` = file 0x3B1C0):
/// ```text
///   3b26b  8a 82 96 3f 00 00   mov  0x3f96(%edx),%al  ; x_BYTE_D3F96[code]
///   3b271  8a 65 ec            mov  -0x14(%ebp),%ah   ; v12.x  (BYTE)
///   3b274  00 c4               add  %al,%ah           ; 8-BIT ADD
///   3b279  88 65 ec            mov  %ah,-0x14(%ebp)   ; stored back as a BYTE
///   3b27c  8a 82 a4 3f 00 00   mov  0x3fa4(%edx),%al  ; x_BYTE_D3FA4[code]
///   3b282  00 c1               add  %al,%cl           ; v12.y, 8-BIT ADD
///   3b284  88 4d ed            mov  %cl,-0x13(%ebp)
/// ```
/// and the RIGHT march is the same shape at 0x3b2b7-0x3b2d0. The
/// exit-distance compare then loads BOTH exits and the target as
/// ZERO-EXTENDED BYTES — `xor %eax,%eax` / `xor %edx,%edx` at
/// 0x3b304-06 and then `8a 45 e4` / `8a 55 fc` (0x3b308/0x3b30b),
/// `8a 55 e5` / `8a 45 fd` (0x3b31b/0x3b31e), `8a 55 e4` /
/// `8a 45 f0` (0x3b333/0x3b336) — so every coordinate in it is 0..255.
///
/// WITNESS mc2l16 t=4664, THE TAKE'S HORIZON. Rival 389 at tile
/// (247, 57), target tile (239, 47), both probes 0x8. The RIGHT
/// detour marches east and retail's byte cursor wraps 255 -> 0,
/// ending at tile x 13; the port's i32 cursor ended at 269. Areas:
/// left |47-97|*|239-247| = 400 either way, right |239-13|*|47-41|
/// = 1356 wrapped but |239-269|*|47-41| = 180 unwrapped — so retail
/// takes `1` (LEFT, `STEER_YAW_L[8]` = 1024) and the port took `2`
/// (RIGHT, `STEER_YAW_R[8]` = 0). ⭐ The 180-degree look of the head
/// is an ARTEFACT: `STEER_YAW_*` only ever holds {0, 512, 1024,
/// 1536}, so ANY wrong pick reads as a multiple of 90 degrees.
fn no_mc2_steer_tile_wrap() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_STEER_TILE_WRAP").is_some())
}

fn rival_regen_pin_list_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_REGEN_PIN_LIST").is_ok_and(|v| v == "1"))
}

/// `MGC_NO_RIVAL_HUMAN_CHAIN_TOP=1` — restore the pre-dig shape, where
/// `sub_14030`'s (and `sub_15FC0`'s) human candidate was gated on the
/// LIVE `player.state` instead of the tick-top class-3 roster sample
/// `human_wiz_top`. See the witness in [`World::mc2_rival_pick_wizard`].
fn rival_human_chain_top_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_HUMAN_CHAIN_TOP").is_ok_and(|v| v == "1"))
}

/// `MGC_NO_MC2_TARGET_ALIVE_HUMAN_LIVENESS=1` — restore the pre-dig
/// shape, where [`World::mc2_target_alive`]'s `PLAYER_TARGET` arm
/// ANDed a live `player.state == LifeState::Alive` onto the signature
/// test.
///
/// ⭐⭐⭐ **A LAW ON ONE CALL PATH IS NOT LANDED.**
/// [`World::mc2_target_alive`]'s own doc comment already states the
/// law — `sub_14C60` (EF:6707) "is an IDENTITY test, not a LIVENESS
/// test", "THE LIFE AND REAP PREDICATES DO NOT BELONG HERE" — and the
/// pool-target arm one line below obeys it. The `PLAYER_TARGET` arm
/// did not.
///
/// Shipped `NETHERW.EXE`, `sub_14C60` = file 0x39460 (linear 0x14C60 +
/// 0x24800) is SEVEN instructions and there is no life test, no
/// reap test and no human-vs-pool branch in any of them:
///
/// ```text
///   39460: 53 55 89 e5              push %ebx; push %ebp; mov %esp,%ebp
///   39464: 8b 55 10                 mov  0x10(%ebp),%edx   ; a2 = target
///   39467: 52                       push %edx
///   39468: e8 d3 ff ff ff           call 0x39440           ; sub_14C40
///   3946d: 8b 55 0c                 mov  0xc(%ebp),%edx    ; a1 = self
///   39470: 66 8b 9a 98 00 00 00     mov  0x98(%edx),%bx    ; word_0x98_152
///   3947a: 66 39 d8                 cmp  %bx,%ax
///   3947d: 0f 94 c0                 sete %al
/// ```
///
/// and `sub_14C40` (file 0x39440) is `(class_0x3F << 7) + model_0x40 +
/// id_0x1A` — three bytes the human's carpet record keeps unchanged
/// through death, fall and corpse (only a FREE clears `class_0x3F`,
/// and the human is never freed). The AttackWizard handler
/// `sub_13890` (file 0x38090) opens on exactly this gate and on
/// nothing else:
///
/// ```text
///   3809b: 66 8b 83 96 00 00 00     mov  0x96(%ebx),%ax      ; target word
///   380a2: 8b 34 85 e4 a3 01 00     mov  0x1a3e4(,%eax,4),%esi
///   380ab: e8 b0 13 00 00           call 0x39460             ; sub_14C60
///   380b5: 0f 84 2d 02 00 00        je   0x382e8             ; steer + return 0
///   380c3: e8 18 49 04 00           call 0x7c9e0             ; tan2(self+0x4C, tgt+0x4C)
///   380d7: 66 89 43 20              mov  %ax,0x20(%ebx)      ; roll_0x20 = the FACE
///   380cb/0x380d0: push $0x1200 / $0xd00                     ; boost 4608 / arrive 3328
///   380db: e8 b0 13 00 00           call 0x39490             ; sub_14C90, the approach
/// ```
///
/// This is the third consumer of one idea. `human_wiz_top`
/// ([`rival_human_chain_top_off`]) already replaced the live
/// `player.state` with the tick-top roster sample on the two PICK
/// sides (`sub_14030` / `sub_15FC0`); the VALIDITY gate is the arm
/// that never got it, and unlike those two it is not a roster
/// question at all — retail simply does not ask.
///
/// WITNESS mc2l16 t=16472, the take's cheapest open head. The human
/// (slot 303) dies at t=16466 — `life` 960 -> -640, `action45` 0 -> 2
/// — while rival 389 holds `ai_state` 8 / `target96` 303 /
/// `word_0x98_152` 687 (= 303 + 0 + (3 << 7)). Retail's `sub_13890`
/// keeps running on the corpse for six more ticks: it re-faces the
/// falling body every tick and at t=16471 the setpoint finally moves,
/// `roll` 917 -> 915 (= `angle_of(3002 - 1530, 29740 - 25513)`,
/// ATAN[89] = 109). The yaw servo follows one tick later and that is
/// the graded head, `slot 389 heading` retail 915 / port 917. The
/// port's gate went false the instant the human left `Alive`, so
/// `mc2_rival_state_tick` returned before the face: a targeted
/// `MGC_WRITE_TRACE=389:f34` prints its last write at t=16464 and
/// nothing again until the re-anchor. Retail's own re-election lands
/// one tick LATER than the port's drop, at t=16472 (`ai_state` 8 ->
/// 6, `target96` 303 -> 637, `f98` 687 -> 1956) — from the SELECTOR,
/// which does test life, not from this gate.
///
/// ⚠ `roll`/`target96`/`ai_state` are all UNGRADED on a (3,1), so the
/// defect is only ever visible one servo tick late on `heading`.
fn no_mc2_target_alive_human_liveness() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_TARGET_ALIVE_HUMAN_LIVENESS").is_some())
}

/// `MGC_NO_MC2_INTAKE_DROPS_DEAD_HUMAN=1` — restore the round-141
/// shape, where `sub_5EFA0`'s dead-target drop EXEMPTED
/// `PLAYER_TARGET`. See [`World::mc2_rival_intake`]: the drop resolves
/// its target through `Entities[word_0x96_150]` with NO class, model
/// or human test (shipped `NETHERW.EXE` file 0x837b2-0x837e9), so the
/// human's carpet is just a pool record to it. WITNESS
/// `recordings/mc2l22.mgcr` t=10,020 — the human (slot 424) dies and
/// rival 530, undocked and holding him in `sub_13890`, fires one more
/// lightning (spell 7) at the corpse: 1 (10,23) + 82 (9,9) records
/// retail never mints.
fn no_mc2_intake_drops_dead_human() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_INTAKE_DROPS_DEAD_HUMAN").is_some())
}

/// `MGC_NO_RIVAL_WAR_EARLYOUT=1` — restore the pre-dig shape, where
/// `sub_14030`'s WAR arm was folded into the hated/bully score behind
/// the `range + 10` gate instead of returning immediately.
fn rival_war_earlyout_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_WAR_EARLYOUT").is_ok_and(|v| v == "1"))
}

/// `MGC_NO_RIVAL_METAMORPH_CLOAK=1` — restore the pre-dig behaviour,
/// where the RIVAL column's metamorph minted the pose-puppet but never
/// raised the caster's `byte[0] & 0x20` SCAN-INVISIBILITY bit.
///
/// ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED. `sub_6A030` is
/// CASTER-GENERIC: its first-tick block ends
/// `v2x->struct_byte_0xc.byte[0] |= 0x21u` (EF:56335; shipped
/// `NETHERW.EXE` 0x8E92F `or dl,0x21`) and its expiry arm runs
/// `v2x->struct_byte_0xc.byte[0] &= 0xDFu` (EF:56403; 0x8EA50
/// `and cl,0xdf`) — on `Entities[token->parentId_0x28_40]`, whoever
/// that is. Bit 0 is the draw-hide (presentation; retail immediately
/// re-sets it for a NON-human caster, EF:56338-42, and blinks it
/// between puppet and wizard every tick, EF:56352-84). Bit 5 (0x20)
/// is the SAME scan-invisibility bit Invisibility and the death
/// touchdown raise, and every creature scanner filters on it
/// (`sub_1BF90` :9155 → [`crate::mc2::mobs`] `flags & 0x20`,
/// `sub_1DBF0`'s wizard-watch tail :10294, the archer Scan A, and the
/// rival roster's own nearest-wizard pick).
///
/// The human half landed as `MGC_NO_METAMORPH_CLOAK` in
/// `mc2/cast.rs`, but the human carpet is OUT OF THE POOL, so there
/// it is free-run-only. A rival wizard IS a pool record and the bit
/// rides `carpet.flags & 0x20` through the importer
/// (`engine/world/conformance.rs` `b0 & 0x20 -> flags |= 0x20`), so
/// on this column the law is GRADED.
fn rival_metamorph_cloak_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_METAMORPH_CLOAK").is_ok_and(|v| v == "1"))
}

/// `MGC_NO_RIVAL_REBOUND_CLEAR_DEFER=1` restores the pre-dig
/// behaviour, where a rival's REBOUND window dropped `F_REBOUND` off
/// its caster on the same pass the counter reached 0 — one tick early.
/// `sub_6AA00` (EF:56728-62) is the SIBLING SPLIT of `sub_6A480`
/// (EF:56503 / 56533-36): the SHIELD *returns* on an already-spent
/// window and clears at the expiry pass; the REBOUND *clears* on an
/// already-spent window and only drains (`sub_6D880`) at the expiry
/// pass. Mirror images. (Round 104, dig W3-S.)
fn no_rival_rebound_clear_defer() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_REBOUND_CLEAR_DEFER").is_some())
}

/// ⭐⭐⭐ THE RIVAL CAST **ARMS**; THE TOKEN'S OWN BODY **FIRES**, AND
/// IT READS THE CASTER'S POSE LIVE AT ITS OWN POOL SLOT.
///
/// `sub_14E10`'s commit hands every case to `sub_5F660`, whose whole
/// arm is `sub_5F7B0` (EF:60971-77): `word_0x2E_46 = word_0x30_48`,
/// two flag bits, the cloak clear — **no spawn, no yaw read, nothing
/// aimed**. The FIRE is `sub_693F0` (EF:55832, the decompile's own
/// `//spell fire`), the class-15 action body, dispatched at the
/// TOKEN's pool slot:
///
/// ```text
///   v1x = Entities[a1x->parentId_0x28_40];             // the CASTER
///   if (a1x->word_0x2E_46 == a1x->word_0x30_48) {      // first tick
///       v2x = sub_6DCA0(v1x, &v1x->position_0x4C_76, …);
///       v6x->word_0x26_38 = a1x - Entities[0];         // the TOKEN slot
///       v6x->yaw_0x1C_28   = wizext->+0x18 + v1x->yaw_0x1C_28;
///       v6x->pitch_0x1E_30 = wizext->+0x1A + v1x->pitch_0x1E_30;
///       v6x->axis_0x9A_154x = v1x->position_0x4C_76;
///       MoveEntity_57FA0(&v6x->axis_0x9A_154x, …, 0x4000);
///   }
/// ```
///
/// A LEVEL-LOAD book sits ABOVE its wizard (530 → 531..556), so that
/// body runs LATER IN THE SAME FRAME — **after the caster's own steer
/// has re-aimed it.** The port fused the arm and the fire into one
/// call at the CASTER's slot and handed the emit the yaw it had
/// snapshotted at the TOP of `mc2_rival_cast`, i.e. the PRE-steer
/// heading, so every rival projectile left on a stale bearing.
///
/// mc2l22 t=17548→17549 is the witness. Rival 530 fires lightning:
/// retail's beam (slot 765) leaves on yaw **512**, the caster's
/// POST-steer heading, and its `axis_0x9A_154x` reproduces it exactly
/// (49633 + 16384·cos(pitch 2032) = 65997 → 461 = the recorded
/// `dest_x`, `dest_y` = 15389 unchanged). The port left on **148**,
/// the caster's PRE-steer heading, which its own beam acquisition then
/// nudged to 143 — laying all 32 `sub_66750` trail nodes 25° off. Every
/// node's `z` and every node's `rand` were already bit-exact; only
/// `x`/`y` moved. 62 rows.
///
/// `MGC_NO_RIVAL_FIRE_AT_TOKEN_SLOT=1` restores the fused
/// caster-slot fire.
fn rival_fire_at_token_slot_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_FIRE_AT_TOKEN_SLOT").is_ok_and(|v| v == "1"))
}

/// ⭐⭐⭐ "AFTER THE CASTER'S BODY" IS NOT "AT THE TOKEN'S SLOT" — A
/// **SIBLING TOKEN BELOW THE FIRING ONE REWRITES THE CASTER'S SPEED**,
/// AND THE BOOST READS IT.
///
/// [`rival_fire_at_token_slot_off`] moved the deferred fire below the
/// brain, but it still runs INSIDE the caster's own dispatch
/// ([`Self::mc2_rival_buffs`], caster slot). Retail's fire is
/// `sub_693F0`'s first-tick arm at the **TOKEN's** pool slot, and a
/// level-load book lays its 26 tokens in SPELL ORDER directly above the
/// wizard (557 → 558..583), so every token with a LOWER spell index
/// than the firing one has already run its own body in the same walk.
/// One of them writes the caster's `actSpeed_0x82_130`: SPEED's
/// `GetScroll_69DB0` (EF:56245-52 on the armed tick, EF:56266-68 at
/// window close, `minSpeed * sign`) — token slot base+3, i.e. below
/// every offensive spell's token. And `sub_693F0`'s spawn tail is
/// `spawned->actSpeed_0x82_130 += v1x->actSpeed_0x82_130` (EF:55856),
/// read LIVE off the caster.
///
/// mc2l22 pair 2562→2563 is the witness. Rival 557 casts spell 9
/// (`cooldown[9] 0 → 4`, token 567 `word_0x2E_46` 0 → 2) on the very
/// frame its SPEED window closes (token **561** holds 244 at 2562 and
/// 0 at 2563), and that close stamps `actSpeed = minSpeed = 80` over
/// the standing 160. Retail's (9,3) is born at slot 909 with
/// `speed` **462** = 384 + **80** − the class-9 mover's own first
/// step; the port fired at slot 557, three slots BEFORE 561, read the
/// standing **160** and minted **542**. The whole x/y/z head of both
/// the (9,3) and its (10,0) child is that one wrong step length:
/// `pos = caster + speed·sin(yaw)·cos(pitch)` reproduces retail's
/// 430/83 at 462 and the port's 504/98 at 542 to the unit.
///
/// Only the FIRE moves — the window arm, the debit, the buff publish
/// and the countdown stay in the caster-slot stand-in, so the token's
/// own `word_0x2E_46` lane is untouched. The stand-in has already
/// decremented by the time the token's slot is reached, which is why
/// the first-tick test reads `word_0x2E_46 + 1`.
///
/// `MGC_NO_RIVAL_FIRE_AT_OWN_TOKEN_SLOT=1` restores the caster-slot fire.
fn rival_fire_at_own_token_slot_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_FIRE_AT_OWN_TOKEN_SLOT").is_ok_and(|v| v == "1"))
}

/// ⭐⭐⭐ A MANIFESTATION'S CASTER IS ITS OWN `parentId_0x28_40`, NEVER
/// A SCAN OF THE ROSTER'S BOOKS. Every class-15 effect body opens on
/// the SAME two lines — `v1x = Entities_EA3E4[a1x->parentId_0x28_40];
/// if (v1x > Entities_EA3E4[0])` — and hands that record to
/// `sub_68D50` as the purse: the fire `sub_693F0` (EF:56176-78),
/// SHIELD `sub_6A480` (EF:56496-503), REBOUND `sub_6AA00`
/// (EF:56721-28), METEOR `sub_6AB00` (EF:56784), CRATER `sub_6BAB0`
/// (EF:57421). The token names its wizard; the wizard's book is never
/// consulted to answer "whose token is this".
///
/// The port resolved the owner as "the FIRST rival whose
/// `book.ent[spell]` equals this slot", and a rival's book after a
/// DEATH SCATTER is not a slot table at all — `mc2_scatter_spells`
/// leaves BOOLEAN 1s in it (the same marker the `tick70 == 3·spell`
/// gate above already had to disambiguate for a loose jar). So every
/// corpse in the roster claimed pool slot 1, and a live wizard whose
/// respawn-minted book really does sit at slot 1 lost its whole
/// offensive column to the corpse's purse: `mc2_rival_afford` is
/// `sub_68D50`, whose first test is `life_0x8 < 0` (EF:55553), so the
/// body took the refused arm — window collapsed to 0, no fire, no
/// debit — on EVERY tick, forever.
///
/// WITNESS mc2l8 t=7535, the head of 617 excess resets. Rival 139
/// (3,1) holds spell 0's token at pool slot 1 (`owner28` = 139 on
/// both sides, `f2e`/`f30` = 5/5 — the first tick). Retail fires:
/// slot 1 `f2c 0 → 1`, `f2e 5 → 4`, wizard 139 `mana 5290 → 5190`,
/// and the (9,0) is born at slot 289 — the FIRST pop of the tick, the
/// slot the tick-top reap had just pushed. The port matched rival
/// index 0 instead (ent **122**, `life -1168`, book
/// `[1,1,1,1,1,0,0,1,…]`), refused, collapsed `f2e` 5 → 0, spawned
/// nothing, and spent slot 289 on the human's (10,2) puff — the
/// `slot 139 mana: retail N port N+100` + `missing in port: slot N
/// (9,0)/(10,2)/(10,12)` census family, 313 + 157 of the take's 617
/// first-divergence rows.
///
/// The parent match is tried FIRST; a token whose parent names no
/// rival falls back to the old book scan, which is the only shape
/// the pre-law code could ever have resolved.
///
/// `MGC_NO_MC2_TOKEN_PARENT_OWNER=1` restores the bare book scan.
pub(crate) fn no_mc2_token_parent_owner() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_MC2_TOKEN_PARENT_OWNER").is_ok_and(|v| v == "1"))
}

/// ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED — the NATIVE
/// stand-in for the SPEED manifestation body.
///
/// `MGC_NO_RIVAL_SPEED_STANDIN=1` restores the pre-law behaviour, in
/// which `mc2_manifestation_pass` (the only dispatcher of
/// `mc2_rival_manifestation_tick`) was dead in native play because
/// `mc2_token_slot_dispatch()` requires a pooled carpet, so no rival's
/// commanded speed was ever written and every rival sat on its spawn
/// marker for the whole level.
///
/// ⚠ CONFORMANCE-NEUTRAL BY CONSTRUCTION, not by luck: under replay
/// the human IS pooled, `mc2_token_slot_dispatch()` is true, and the
/// stand-in is skipped in favour of the real slot-order dispatch. The
/// law is therefore PAIR-BLIND and REPLAY-BLIND — its pin is the unit
/// test `mc2_rival_native_movement.rs`, never a fixture.
fn mc2_rival_speed_standin_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_SPEED_STANDIN").is_ok_and(|v| v == "1"))
}

/// ⭐⭐⭐ THE WIZARD BUFF STAGE LIVES ON THE WIZARD'S OWN FLAGS WORD,
/// AND THE MC2 RIVAL COLUMN KEPT IT IN A SIDE STRUCT.
///
/// Retail's shield absorb (`sub_5EFA0` EF:60676-93) opens
/// `if (byte[1] & 0x40 || byte[2] & 0x40)` on the VICTIM's
/// `struct_byte_0xc_12_15` — our `flags` — and the deflect gate
/// (`sub_65820` EF:62939 -> `sub_68740` EF:55221) opens on
/// `word[0] & 0x8010`. The two publishers are the manifestation
/// bodies themselves:
///
/// * SHIELD `sub_6A480` (EF:56496): tier `life_0x1A == 0` stamps
///   `parent.byte[1] |= 0x40` (our [`F_SHIELD_CHARGED`]) on EVERY
///   afforded tick; tier 1 stamps `parent.byte[2] |= 0x40` (our
///   [`F_SHIELD_ARMED`]) on the FIRST tick only; the window's last
///   pass runs `parent.dword &= 0xFFBFBFFF`, clearing both.
/// * REBOUND `sub_6AA00` (EF:56721-51): tier 0 stamps
///   `parent.byte[1] |= 0x80` (our [`F_REBOUND`]) on every afforded
///   tick, tier 1 the PRECISE `byte[0] |= 0x10`; a pass with the
///   window already spent runs `parent.word[0] &= 0x7FEF`.
///
/// MC1's rival column has read exactly this word since its own shield
/// dig (`mc1/rivals.rs`: "keyed on the ENTITY's 0x4000 bit — the
/// imported flag, not the port-side buff mirror", and
/// `rival_shield_token_tick`/`rival_rebound_token_tick` publish it);
/// MC2's never crossed. mc1hwl0 t=5593's note in that file records
/// retail publishing carpet 473's `flags` 0x800C -> 0xC00C — the
/// SAME two bits, in the same order, that mc2l22's rival 611 takes at
/// t=3624/3625.
///
/// `MGC_NO_RIVAL_BUFF_BITS=1` restores the pre-dig behaviour (the
/// `Mc2Rival::shield_state` FSM, no entity publish).
pub(crate) fn rival_buff_bits_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MGC_NO_RIVAL_BUFF_BITS").is_ok_and(|v| v == "1"))
}

/// Retail `byte[1] & 0x40` — the shield's CHARGED stage (the next hit
/// is quartered and mana-paid). Positional; the human column's
/// importer already reads the same bit (`conformance.rs`,
/// `player.shield = carpet.flags & 0x4000`).
pub(crate) const F_SHIELD_CHARGED: u32 = 0x4000;
/// Retail `byte[1] & 0x80` — the scatter REBOUND window
/// (`sub_68740`'s `word[0] & 0x8010` gate). Positional; already read
/// on pool victims by [`Gen::mc2_rebound_deflect`].
pub(crate) const F_REBOUND: u32 = 0x8000;
/// Retail `byte[2] & 0x40` — the shield's ARMED stage (the next hit
/// is NULLED and promotes to CHARGED). ⚠ NOT positional: dword bit 22
/// is [`crate::mc2::tail::F_GRABBED`] in the port, so this stage is
/// homed at bit 20.
pub(crate) const F_SHIELD_ARMED: u32 = 1 << 20;

/// ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED — WHICH SPELLS
/// PIN THE REGEN MID-BURST IS AN ENUMERATED LIST, AND THE RIVAL
/// COLUMN NEVER READ IT.
///
/// `sub_68DE0` (EF:55569) is not part of a shared manifestation
/// skeleton: each of the 26 class-15 handlers places its OWN call, and
/// six of them put it INSIDE the `word_0x2E_46 == word_0x30_48`
/// first-tick block instead of after it — `sub_6BCF0` (spell 0x11,
/// EF:57494-57530: the call sits one brace deeper, right after the
/// `sub_6DCA0(..., 0x11u, ...)` spawn), `sub_6BF30` (18), `sub_6C620`
/// (21), `sub_6C870` (22), `sub_6CAC0` (23), `sub_6CFA0` (25). Those
/// six debit on the ARM frame and then leave the purse alone for the
/// rest of the window. The human column already reads this list
/// ([`NO_MID_BURST_REGEN_PIN`], `mc2/cast.rs`) and so does the pair
/// importer (`mc2_applied_mana_delta`); both rival token paths —
/// [`Self::mc2_rival_buffs`]'s above-the-wizard stand-in and
/// [`Self::mc2_rival_token_tick`]'s own-slot body — pinned
/// unconditionally.
///
/// mc2l22 t=1092: rival 7 (slot 611) casts spell 17 for 15,000
/// (`d88` 159 → −15000, charge 200 → 0) and its token 629 opens a
/// 41-tick window. Retail's record holds `d88` back at +159 for
/// t=1093 and pays it — mana 71,005 → 71,164 at t=1094 — while the
/// port zeroed the freshly recomputed 159 at the stand-in and froze
/// the purse for the whole window. That single suppressed quantum was
/// the take's entire horizon wall at 1093.
fn spell_pins_regen(spell: usize) -> bool {
    rival_regen_pin_list_off() || !NO_MID_BURST_REGEN_PIN.contains(&spell)
}

/// `MGC_NO_RIVAL_PIN_AFFORD=1` — the A/B lane for **THE MID-BURST
/// REGEN PIN IS AFFORD-GATED ON THE RIVAL COLUMN TOO** (the human
/// column's own law, `mc2/cast.rs`: "AFFORD-GATED: every retail
/// handler calls sub_68DE0 only on the afford-success path"). Set it
/// to restore the pre-dig behaviour, where both rival token stand-ins
/// pinned the wizard's freshly recomputed regen to 0 on a tick whose
/// `sub_68D50` probe had REFUSED the body.
///
/// ⭐⭐⭐ EVERY MANIFESTATION BODY IS
/// `if (sub_68D50(tok, wiz)) { …publish…; sub_68DE0(tok, wiz); }
///  else { word_0x2E_46 = 1; }` followed by the shared decrement, so
/// the REFUSED arm reaches neither half of `sub_68DE0` — no debit and
/// **no pin**. REBOUND (`sub_6AA00`, EF:56729-62) disassembled from
/// the shipped `NETHERW.EXE` at file `0x8F200`
/// (`off = 0x34800 + linear - 0x10000`, the same rule the SHIELD III
/// citation in `mc2/cast.rs` uses — `sub_68DE0` is `0x8D5E0`,
/// `sub_68D50` is `0x8D550`):
///
/// ```text
///   0x8f257  call 0x8d550            ; sub_68D50
///   0x8f25f  test eax,eax
///   0x8f261  jz   0x8f286            ; REFUSED ->
///   0x8f270  or byte [esi+0xd],0x80  ;   (the publish arm)
///   0x8f27c  call 0x8d5e0            ; sub_68DE0  — AFFORD ARM ONLY
///   0x8f284  jmp  0x8f28c
///   0x8f286  mov word [ebx+0x2e],0x1 ; REFUSED: window = 1, NO call
///   0x8f28c  dec  [ebx+0x2e]         ; the shared decrement
/// ```
///
/// `sub_693F0`, the generic spell body (`0x8DBF0`), carries the same
/// `call 0x8d550 / test / jz -> mov word [reg+0x2e],0x1` skeleton at
/// every one of its arms (0x8dc2c/0x8de79/0x8e2ec …), with each
/// `call 0x8d5e0` inside the taken leg.
///
/// WITNESS (mc2l22, segment anchored at t=9378): rival 530's REBOUND
/// token 539 is refused — the port's own `mc2_rival_cast` collapse
/// leaves `f26 = 1` against `f28 = 251` — and the buffs stand-in read
/// `f26 != f28` and pinned the freshly recomputed +100. Retail's
/// `manaRegen_0x88_136` holds 100 across t=9378/9379/9380 and its
/// purse steps 23800 -> 23900 -> 24000; the port stalled at 23900.
/// The same missing gate is 749 of the take's 775 rival-mana
/// first-divergence segments, each short EXACTLY one regen quantum
/// (644 x 100 afield, 105 x 1000 at the castle).
fn rival_pin_afford_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_PIN_AFFORD").is_some())
}

/// ⭐⭐⭐ A MANIFESTATION BODY RUNS AT THE **TOKEN'S** SLOT, SO IT
/// OUTLIVES ITS CASTER — and its teardown is what CLEARS the buff
/// bits the deflect and absorb gates read.
///
/// `sub_6AA00` (REBOUND, EF:56721-51) and `sub_6A480` (SHIELD,
/// EF:56496-540) both resolve their caster through
/// `Entities[parentId_0x28_40]` and are dispatched from the class-15
/// walk arm at the TOKEN's own pool slot. Nothing in either is
/// conditional on the caster's action state, and `sub_68D50`'s FIRST
/// two statements (EF:55549-52) are `if (caster->mana < 0) return
/// false; if (caster->life_0x8 < 0) return false;` — so the tick a
/// wizard's life goes negative the body takes its REFUSED arm, and
/// the refused arm is a TEARDOWN:
///
/// ```text
///   sub_6AA00 (rebound):  refused -> word_0x2E_46 = 1; --;  sub_6D880
///                         and the CLEAR lives in the function's TOP
///                         arm, `if (word_0x2E_46 <= 0)
///                             caster->word[0] &= 0x7FEF;`
///                         => it lands on the NEXT pass.
///   sub_6A480 (shield):   refused -> word_0x2E_46 = 0  (LABEL_12)
///                         then LABEL_13 `if (!word_0x2E_46) {
///                             caster->dword &= 0xFFBFBFFF;
///                             sub_6D880; }`
///                         => it lands on the SAME pass.
/// ```
/// ⭐⭐ A SIBLING SPLIT: the same teardown, ONE PASS APART, because
/// the rebound body puts its clear above the refusal and the shield
/// body puts it below.
///
/// The port dispatches a rival by action state
/// ([`Self::mc2_rival_entity_tick`]) and a LEVEL-LOAD book's
/// countdown is the caster-slot stand-in inside
/// [`Self::mc2_rival_buffs`], which the death fall never reaches — so
/// a rival that died with a live REBOUND window kept
/// [`F_REBOUND`] set on its corpse FOREVER, and
/// [`crate::Gen::mc2_rebound_deflect`]'s pool arm reads exactly that
/// bit.
///
/// WITNESS (mc2l22, free run, the take's t=3742 wall): rival 611's
/// rebound token is slot 620 (`(15,8)`, `word_0x30_48` = 125). Retail
/// counts it 11 at t=3737, the wizard's life goes 379 -> -1621 at
/// t=3738 and the token collapses 11 -> **0** on that same tick, and
/// at t=3739 the corpse's flags word steps **32780 -> 12** (`0x800C &
/// 0x7FEF`). The port froze the token at 11 and the flags at 32780,
/// so at t=3742 the human's castle-guard arrow — slot 917, a `(9,13)`
/// whose `byte_0x43_67` is 10 — took `sub_68740`'s deflect arm off a
/// three-tick-old corpse where retail's `AddArcherArrow_672E0`
/// call-site gate `!(v3x->word[0] & 0x8010)` (EF:58892) sent it
/// straight to the impact: retail `life 7 -> 6` + `byte[1] |= 4` and
/// 100 into 611's ch0 mailbox, the port `life 7 -> 13`, `id24 -> 611`,
/// `f30 22 -> 1043`, `f32 1914 -> 134`.
/// `sub_6D880` (EF:58215-25) is the queued-tier drain, which is the
/// second row of the same debt: token 620's `word_0x2C_44` is 0 in
/// retail and 1 in the port at t=3741.
/// THE NULL-RECORD POSITION `sub_161A0`'s else arm reads when its
/// class-3 rescan comes up empty (EF:7815-40 — `v6x == 0`, and the
/// shipped code does `lea 0x4c(%edi)` with `edi = 0`, NETHERW.EXE
/// 0x3ac06): linear address 0x4C is the real-mode INTERRUPT VECTOR
/// TABLE, mapped 1:1 under DOS/4GW — `x` = INT 13h's offset word,
/// `y` = INT 13h's segment word, `z` = INT 14h's offset word. Under
/// DOSBox the BIOS vectors point into its callback segment
/// `0xF000`, which is exactly what the recording measures: the two
/// blank-chain Defense ticks on mc2l22 (t=4662 rival 503 roll 318 at
/// (47824, 10966); t=9784 rival 530 roll 303 at (50204, 10724)) fit
/// `y = 0xF000` with `x ∈ [4326, 4428]`, a 32-byte-aligned callback
/// slot inside that window being `0x1100`. `z` is only ever compared
/// against a wizard's altitude (`z > ghost.z + 512`), and every
/// measured tick sits above it.
///
/// This is retail reading OUTSIDE ITS OWN DATA (docs/DEVIATIONS.md
/// "RETAIL'S OWN OUT-OF-BOUNDS READS"). ⚖ PLAYER-RULED 2026-09-04:
/// stays unfaithful, the authored origin (the remc2 `//fix`'s
/// `(0, 0, 0)`) is the default. ⚖ **RE-RULED 2026-09-18 (round 150),
/// on the painter-residue precedent** (`mc2_painter_ctor_extents`:
/// ONE STABLE WITNESSED VALUE ⇒ SEED IT): unlike every other member
/// of the class the words are the same on every recording made on
/// the same DOSBox, and re-measured on the round-149 floor the fitted
/// words take mc2l22 **3 → 1 segment** (the two roster-excused
/// boundaries 4661/9784 go clean), its raw shadow **4 → 0** (the
/// `(3,1)`/`(5,2)` `roll` rows at 4662/9784/9785), the other 39 MC2
/// takes byte-identical, and the t=6160 downstream head that kept the
/// origin the default in round 102 no longer exists. So the fitted
/// words ARE the default; `MGC_NO_MC2_IVT_GHOST=1` restores the
/// authored origin for the A/B. Fixture:
/// `mc2l22/a-blank-defense-rescan-bears-on-the-interrupt-vector-table`
/// (pair 4661→4662, four `z` rows).
fn mc2_ivt_ghost() -> (u16, u16, i16) {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_IVT_GHOST").is_some()) {
        (0, 0, 0)
    } else {
        (0x1100, 0xF000, 0x1100)
    }
}

fn rival_corpse_token_off() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_CORPSE_TOKEN").is_some())
}

/// Per-color config from the level record (the 110-byte
/// `WizardMapSettings_0x360D2` block + the header's authored
/// starting-castle level), resolved by the app.
#[derive(Debug, Clone)]
pub struct Mc2RivalConfig {
    /// `Aggression_0x360D5` — hate pacing, wealth-scaled war
    /// thresholds, opportunism margins.
    pub aggression: u8,
    /// `Perception_0x360DD` — notice rolls + aim-cone width.
    pub perception: u8,
    /// `Reflexes_0x360D9` — decision cadence, turn rate, burst
    /// lockout.
    pub reflexes: u8,
    /// `Life_0x3612F` — 16.8 HP scalar; ALSO the castle-HP factor
    /// (castle-data-tables §2.4). 0 = default 256 (1.0x).
    pub life: u16,
    /// `player_0x2FED9[color]` — authored starting-castle level
    /// (0 = none, N = a castle at level N-1). AI-only in retail
    /// (open-closure §7 — the human never consumes it).
    pub castle_level: u8,
    /// `StartingSpells_0x360E1x` — per-spell grant flag.
    pub start: [bool; MC2_SPELLS],
    /// `byte_0x360FBx` — per-spell starting LEVEL 0..2 (clamped;
    /// EF:38693 writes it straight into the AI's SpellLevels).
    pub start_level: [u8; MC2_SPELLS],
    /// `BlockedSpells_0x36115x` — per-spell deny flag.
    pub blocked: [bool; MC2_SPELLS],
}

/// The MC2 AI brain state (`byte_0x1C1_449`; brain trace §1.2). Same
/// semantic set as MC1's — plus the DEFENSE state MC2 selects as
/// cascade step 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum Mc2AiState {
    /// Fresh spawn: decide immediately (the case-0 double selector
    /// call, EF:5255-56).
    #[default]
    Fresh,
    /// State 1: fly home, cast the castle upgrade (sub_12FF0).
    Upgrade,
    /// State 3: fly to the scouted site, cast the castle (sub_13100).
    Build,
    /// State 6: claim a mana ball with possess-1 (sub_131F0/135C0).
    Possess,
    /// State 7: raid an enemy castle (sub_13710).
    RaidCastle,
    /// State 8: attack an enemy wizard (sub_13830).
    AttackWizard,
    /// State 9: intercept an enemy balloon (same handler).
    RaidBalloon,
    /// State 13: hunt any mana-holding creature.
    HuntMana,
    /// State 11: return home (heal / regroup; sub_133B0).
    Home,
    /// State 12: cruise (sub_13270).
    Cruise,
    /// State 14: reactive defense (sub_161A0) — cascade step 7.
    Defense,
}

impl Mc2AiState {
    /// The retail `byte_0x1C1_449` value the dispatch switches on
    /// (EF:5252-5310). State 4 (`sub_131F0`) is the possess APPROACH
    /// arm — a target-alive check plus the 256/2048 close, no cast —
    /// and shares this port state with 6 (`sub_135C0`, the arm that
    /// casts); 2, 5 and 10 are the decompile's `_nmemneed` stubs and
    /// 15+ never appear, all of which fall through to the bare
    /// selector call — the port's Fresh.
    pub(crate) fn from_retail(v: u8) -> Self {
        match v {
            1 => Mc2AiState::Upgrade,
            3 => Mc2AiState::Build,
            4 | 6 => Mc2AiState::Possess,
            7 => Mc2AiState::RaidCastle,
            8 => Mc2AiState::AttackWizard,
            9 => Mc2AiState::RaidBalloon,
            0xB => Mc2AiState::Home,
            0xC => Mc2AiState::Cruise,
            0xD => Mc2AiState::HuntMana,
            0xE => Mc2AiState::Defense,
            _ => Mc2AiState::Fresh,
        }
    }

    /// The canonical retail byte for this port state — the inverse of
    /// [`Self::from_retail`] on the states that survive the collapse.
    /// `Possess` reports 6 (`sub_135C0`, the arm that casts) because
    /// the port fuses retail's 4 and 6; grade retail's byte through
    /// `norm_retail_ai_state_mc2`, never raw, or every tick a rival
    /// sits in the APPROACH arm reads as a mismatch it is not.
    pub(crate) fn to_retail(self) -> u8 {
        match self {
            Mc2AiState::Fresh => 0,
            Mc2AiState::Upgrade => 1,
            Mc2AiState::Build => 3,
            Mc2AiState::Possess => 6,
            Mc2AiState::RaidCastle => 7,
            Mc2AiState::AttackWizard => 8,
            Mc2AiState::RaidBalloon => 9,
            Mc2AiState::Home => 0xB,
            Mc2AiState::Cruise => 0xC,
            Mc2AiState::HuntMana => 0xD,
            Mc2AiState::Defense => 0xE,
        }
    }
}

/// The retail AI decision lanes for one wizard, as the conformance
/// importer lifts them — the wizard extension's (`type_str_164`)
/// brain half plus the two lanes that ride the wizard ENTITY. Kept
/// as a record because the re-anchor seats fourteen of them.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Mc2RivalAi {
    /// `byte_0x1C1_449`, raw — mapped by [`Mc2AiState::from_retail`].
    pub state: u8,
    /// Entity `word_0x96_150`, already translated to the port's
    /// out-of-pool [`PLAYER_TARGET`] convention for the human.
    pub target: u16,
    /// Entity `word_0x98_152`, the stored target signature.
    pub target_sig: u16,
    /// Entity `axis_0x9A_154x` — the scouted castle site.
    pub site: (u16, u16),
    pub burst: i16,
    pub poverty: i16,
    pub cooldown: [u16; MC2_SPELLS],
    pub hate: [u16; 8],
    pub war: [u16; 8],
    pub weave: u8,
    pub weave_dir: u8,
    pub avoid: u8,
    pub avoid_exit: u8,
    pub aggression: u16,
    pub perception: u16,
    pub reflexes: u16,
    pub life_scale: u16,
    /// The players-block brake word `word_0xe_14` (flight +14) —
    /// seeds the hash-silent [`BrakeWord`] at re-anchor.
    pub brake: i16,
    /// The players-block life-regen register `lifeRegen_0x163_355`.
    pub life_regen: i16,
    /// The players-block knockback register (`yaw_0x1E_30` /
    /// `moveBoost_0x1E_30`). Real state on a rival for longer than on
    /// the human: nothing spends it until the wizard dies (see
    /// [`Mc2Rival::knock_dir`]), so an imported rival that has been
    /// taking fire is already carrying the impulse its death fall
    /// will pay out.
    pub knock_dir: u16,
    pub knock_mag: i16,
    /// The players-block DUEL LOCK words `word_0x146_326` /
    /// `dword_0x142_322` / `word_0x14A_330`. Round 148 measured the
    /// cost of NOT seating these: every candidate pair fixture in the
    /// mc2l6-rsg duel window failed on rival 370's `mana`, because an
    /// imported world could not run the drain at all. A rival's own
    /// lock is only ever nonzero in a recording (the AI never casts
    /// spell 14), and it decides its whole death fall — see
    /// [`no_mc2_rival_duel_death_tether`].
    pub duel_target: u16,
    pub duel_hold: i32,
    pub duel_tier: u8,
}

/// Hash-silent bool (the `CrtRand` pattern — features.rs): carried
/// in a `derive(Hash)` struct without moving any golden pin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BrakeWord(pub bool);

impl std::hash::Hash for BrakeWord {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// Hash-silent bool for the corpse nudge latch (`byte_0x261_609`) —
/// same `CrtRand` pattern as [`BrakeWord`]; a bare field here would
/// re-pin every golden.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NudgeLatch(pub bool);

impl std::hash::Hash for NudgeLatch {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// The rival's DUEL LOCK register triple, hash-silent for the same
/// reason as [`BrakeWord`] and [`NudgeLatch`] ([`Mc2Rival`] is
/// `derive(Hash)`, so a bare field would re-pin every golden).
///
/// `Some((opponent, hold, tier))` mirrors the human's
/// `World::mc2_duel` and carries retail's three player-block words:
/// `word_0x146_326` (the opponent's entity index, or
/// [`PLAYER_TARGET`] for the human), `dword_0x142_322` (the held
/// distance, clamped to [1024, 3072] at the stamp) and
/// `word_0x14A_330` (the tier the SPELLS row 14 lookup uses).
/// `None` IS retail's 0 — a cleared lock reads 0/stale on the other
/// two, which the wizard shadow's lane table already skips.
/// See [`no_mc2_rival_duel_death_tether`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DuelLock(pub Option<(u16, i32, u8)>);

impl std::hash::Hash for DuelLock {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// One live MC2 rival: the player-extension subset the AI machinery
/// needs. Position/yaw/life live on the pool entity (class 3 model 1).
#[derive(Hash, Clone)]
pub(crate) struct Mc2Rival {
    /// Player color (0..=7) — never the human's own (`Gen::human_seat`).
    pub slot: u8,
    /// Wizard entity pool index — also the OWNER TAG on its
    /// projectiles (id24) and claims.
    pub ent: u16,
    /// The per-wizard `str_611` book subset — class-15 manifestation
    /// slots, per-spell XP and levels (shared shape with the human's
    /// [`Mc2Spellbook`]).
    pub(crate) book: Mc2Spellbook,
    /// Spells known across deaths — death reverts the book flags to
    /// boolean (EF:60147), respawn re-mints the manifestations.
    pub known: [bool; MC2_SPELLS],
    /// AI recast cooldowns (`SpellEnabled` doubles as the cooldown
    /// counter in retail, EF:5361-63; ours is a separate array).
    pub(crate) cooldown: [u16; MC2_SPELLS],
    /// Carried mana / ceiling / the regen delta the cast debit rides.
    pub mana: i32,
    pub mana_max: u32,
    pub(crate) mana_delta: i32,
    /// ⭐⭐ THE STORED LIFE-REGEN RATE (`lifeRegen_0x163_355`), the
    /// life twin of `mana_delta`. `sub_12A70` adds it to `life_0x8`
    /// (EF:5425) and re-selects it TWENTY LINES LATER off the
    /// at-castle/dolmen test (EF:5439/5447), exactly like the mana
    /// half two statements above it — so the boosted rate lands the
    /// tick AFTER the rival reaches its castle. The port recomputed
    /// the life half fresh each tick and paid the boost a frame
    /// early: mc2l6 t=784, rival 378 (maxLife 1992) arrives over
    /// castle 174 and retail still regenerates 1992/500 = 3 (life
    /// 737 → 740, `manaRegen` 100 → 1000 in the same statement pair),
    /// then 1992/200 = 9 from t=785 — the port jumped straight to 9.
    pub(crate) life_delta: i32,
    /// Personality (word_0x242/244/246) + the Life scalar (word_0x24A).
    agg: u16,
    per: u16,
    refl: u16,
    pub(crate) life_scale: u16,
    pub state: Mc2AiState,
    /// Hate ledger + war flags per color (array_0x1FC_508).
    pub(crate) hate: [u16; 8],
    pub(crate) war: [bool; 8],
    /// Fireball-family burst counter (word_0x1A2_418): 8 shots then a
    /// negative lockout of (Reflexes-255)/8-1 ticks (EF:6813-15).
    pub(crate) burst: i16,
    /// Poverty latch (word_0x1A4_420): mana < max/4 stops attack
    /// casting; the release is max/4 + 6000, clamped to max/2 only
    /// when the sum overshoots the ceiling (EF:7191-7205).
    pub(crate) poverty: bool,
    /// Current target: entity slot or [`PLAYER_TARGET`]; 0 = none.
    pub(crate) target: u16,
    target_sig: u16,
    /// Scouted castle site (axis_0x9A_154x).
    pub(crate) site: (u16, u16),
    /// The strafe channel (strafeSpeed_0x10_16): decays 4/tick,
    /// stepped at yaw+90. Written 80 by the reactive dodge
    /// (EF:7469) and 3*minSpeed*Reflexes/255 by the combat weave.
    pub(crate) strafe: i16,
    /// The combat-weave micro-FSM (str_611_byte_0x45D_1117, 0..20).
    pub(crate) weave: u8,
    /// The weave's committed direction (str_611_byte_0x45C_1116):
    /// 1 = port (-512), 2 = starboard (+512).
    pub(crate) weave_dir: u8,
    /// The two-stage shield absorb (the wizard byte[1]/byte[2] 0x40
    /// pair, EF:60676-93): 0 spent, 1 armed (next hit nulled +
    /// promotes), 2 charged (next hit quartered, mana-paid, spends).
    shield_state: u8,
    /// The water-steer micro-FSM (byte_0x45E_1118): 0 idle, 1/2
    /// committed left/right, 3..7 frozen arc, >=8 re-detect.
    pub(crate) avoid: u8,
    /// The last chosen steer exit code (byte_0x45E_1119).
    pub(crate) avoid_exit: u8,
    /// Desired speed toward which f126 accelerates 16/tick.
    vdes: i16,
    /// ⭐⭐ THE KNOCKBACK IMPULSE REGISTER, and A LIVING MC2 RIVAL
    /// NEVER SPENDS IT. `sub_5EFA0`'s damage block stamps the pair on
    /// every delivered letter (EF:60697-702): `yaw_0x1E_30 =
    /// tan2(source -> victim)` and `moveBoost_0x1E_30 = damage / 10`
    /// clamped to [0, 80] — the MC1 rival column's own law
    /// ([`crate::mc1::rivals::Rival::knock_dir`], SNAPSHOT 13) one
    /// game over. The only reader is `sub_5D530`'s block 5, and the
    /// alive dispatch does not run `sub_5D530`: `sub_146F0`
    /// (EF:6416-6511) has no moveBoost leg at all. So the register
    /// accumulates while the rival lives and is spent — 4/tick, cap
    /// 128, snapped to 0 below 4 — only by the DEATH FALL.
    /// mc2l6-rsg rival 378 carries 20 (its 200-damage letters) through
    /// its whole life and pays it out over t=1846..1850.
    knock_dir: u16,
    knock_mag: i16,
    /// ⭐⭐⭐ THE DUEL LOCK, AND ONLY THE DEATH FALL EVER READS IT.
    /// Retail's `word_0x146_326`/`dword_0x142_322`/`word_0x14A_330`
    /// on this wizard's player block. `sub_5DE30` is reached from
    /// `sub_5D530` alone, and `sub_5D530` from `AddPlayer03_00_5E010`
    /// (human-only) and `sub_5E310` (the shared death fall) alone —
    /// so a rival carrying a lock enforces nothing while it lives and
    /// then tethers, slews and DRAINS its opponent for every tick of
    /// its fall. See [`no_mc2_rival_duel_death_tether`] for the full
    /// call graph and the shipped bytes.
    pub(crate) duel: DuelLock,
    /// The flight-brake word (`word_0xe_14`): cleared FIRST on every
    /// approach call (EF:6718), set by the arrive branch (EF:6733-35)
    /// and by the can't-boost else (EF:6751-52). The SPEED token's
    /// body reads it to collapse its window (EF:56216-19).
    /// Hash-silent (the CrtRand pattern): a new lane in a
    /// derive(Hash) struct would re-pin every golden.
    v14: BrakeWord,
    /// `sub_5DD50`'s wedged latch — the PLAYER struct's
    /// `byte_0x261_609`, which the nudge sets and its else clears
    /// (recorder lane `nudge_latch`). Only the CAVE re-test reads it
    /// back (`mc2_flight_stuck`), so it is inert off-cave; hash-silent
    /// for the same reason as [`Mc2Rival::v14`].
    nudge_latch: NudgeLatch,
    /// Spawn/at-castle grace (word_0x159_345): mailbox memset while
    /// > 0 (100 at spawn, pinned 2 at the own castle).
    pub(crate) grace: u16,
    /// Dead and castle-less: BANISHED (byte_0x006_2BE4_11236 = 0,
    /// EF:60299) — the objective case-3/8 signal.
    pub eliminated: bool,
    /// Buff flags derived from the manifestations' armed windows.
    pub shield: bool,
    pub invisible: bool,
    pub rebound: bool,
}

impl Mc2Rival {
    fn new(slot: u8, ent: u16, cfg: &Mc2RivalConfig) -> Self {
        Mc2Rival {
            slot,
            ent,
            book: Mc2Spellbook::default(),
            known: [false; MC2_SPELLS],
            cooldown: [0; MC2_SPELLS],
            mana: 1000,
            mana_max: 1000,
            mana_delta: 0,
            life_delta: 0,
            agg: cfg.aggression as u16,
            per: cfg.perception as u16,
            refl: cfg.reflexes as u16,
            life_scale: if cfg.life == 0 { 256 } else { cfg.life },
            state: Mc2AiState::Fresh,
            hate: [HATE_NEUTRAL; 8],
            war: [false; 8],
            burst: 0,
            poverty: false,
            target: 0,
            target_sig: 0,
            site: (0, 0),
            strafe: 0,
            weave: 0,
            // ⭐ THE COMMITTED WEAVE DIRECTION STARTS AT THE MEMSET 0.
            // `str_611_byte_0x45C_1116` has exactly ONE writer in
            // retail — `sub_13890`'s tick-0 arm (EF:6022/6027), which
            // only ever stores 1 or 2 — and `sub_5C950`'s spawn tail
            // does not touch it, so a freshly started rival carries 0
            // until its first whiff weave. The port's `1` was an
            // invention. Measured on all 13 MC2 rival takes: retail's
            // record 0 reads 0 on every seat (51/51).
            weave_dir: if no_mc2_rival_weave_dir_zero() { 1 } else { 0 },
            shield_state: 0,
            avoid: 0,
            avoid_exit: 0,
            vdes: 0,
            knock_dir: 0,
            knock_mag: 0,
            duel: DuelLock(None),
            v14: BrakeWord(false),
            nudge_latch: NudgeLatch(false),
            grace: 100,
            eliminated: false,
            shield: false,
            invisible: false,
            rebound: false,
        }
    }

    /// The knock register, for [`crate::engine::world::World::debug_mc2_rival_knock`].
    #[doc(hidden)]
    pub(crate) fn debug_knock(&self) -> (u16, i16) {
        (self.knock_dir, self.knock_mag)
    }

    /// The book's manifestation slots, for
    /// [`crate::engine::world::World::debug_mc2_rival_book`].
    #[doc(hidden)]
    pub(crate) fn debug_book_ents(&self) -> [u16; MC2_SPELLS] {
        self.book.ent
    }

    /// Decision-cadence period: `64 - Reflexes/4` ticks keyed on the
    /// entity age byte (EF:5460).
    pub(crate) fn think_period(&self) -> u8 {
        (64 - (self.refl / 4) as i32).max(1) as u8
    }

    /// The rival's player-block/brain registers as RETAIL-convention
    /// lanes — the per-rival half of `World::wiz_shadow_mc2`, the MC2
    /// twin of MC1's [`crate::mc1::rivals::Rival::wiz_shadow_lanes`]
    /// (this module owns the private brain fields, so the projection
    /// lives here).
    ///
    /// Lane names match [`mgc_formats::mgcr::RetailPlayerMc2`]'s
    /// fields. `ai_state` is the canonical [`Mc2AiState::to_retail`]
    /// byte (grade retail's through `norm_retail_ai_state_mc2`);
    /// `poverty`, `war` and `brake` are 0/1 because retail keeps a
    /// threshold / a flag word where the port keeps a bool, and
    /// nonzero-ness is the comparable fact. `target`/`target_sig` are
    /// deliberately absent — they ride the wizard ENTITY's graded
    /// `f146`/`f148`, and the entity shadow owns that story.
    pub(crate) fn wiz_shadow_lanes(
        &self,
    ) -> (Vec<(&'static str, i64)>, Vec<(&'static str, Vec<i64>)>) {
        let scalars = vec![
            ("cmd_speed", self.vdes as i64),
            ("strafe", self.strafe as i64),
            ("brake", self.v14.0 as i64),
            ("invuln", self.grace as i64),
            ("ai_state", self.state.to_retail() as i64),
            ("burst", self.burst as i64),
            ("poverty", self.poverty as i64),
            ("aggression", self.agg as i64),
            ("perception", self.per as i64),
            ("reflexes", self.refl as i64),
            ("life_scale", self.life_scale as i64),
            ("weave", self.weave as i64),
            ("weave_dir", self.weave_dir as i64),
            ("avoid", self.avoid as i64),
            ("avoid_exit", self.avoid_exit as i64),
            // Round 147's widening: every register the importer SEATS
            // on the rival (`reanchor_mc2_rival_ai`) and nothing
            // compared. ⚠ `hand_*` is NOT "-1 on both sides": retail
            // seats the first two granted spells at level init
            // (`InitialiseSpells_54A50` — see
            // [`no_mc2_rival_authored_hands`]), which reads 0/1 on
            // every rival seat of every MC2 take.
            ("life_regen", self.life_delta as i64),
            ("duel_target", self.duel.0.map_or(0, |d| d.0) as i64),
            ("duel_hold", self.duel.0.map_or(0, |d| d.1 as i64)),
            ("duel_tier", self.duel.0.map_or(0, |d| d.2) as i64),
            ("knock_dir", self.knock_dir as i64),
            ("knock_mag", self.knock_mag as i64),
            ("hand_left", self.book.left as i64),
            ("hand_right", self.book.right as i64),
        ];
        let arrays = vec![
            ("hate", self.hate.iter().map(|&v| v as i64).collect()),
            ("war", self.war.iter().map(|&v| v as i64).collect()),
            (
                "cooldown",
                self.cooldown.iter().map(|&v| v as i64).collect(),
            ),
            (
                "spell_ent",
                self.book.ent.iter().map(|&v| v as i64).collect(),
            ),
            (
                "levels",
                self.book.levels.iter().map(|&v| v as i64).collect(),
            ),
            ("sel", self.book.sel.iter().map(|&v| v as i64).collect()),
            ("ring", self.book.ring.iter().map(|&v| v as i64).collect()),
            (
                "xp_bank",
                self.book.xp_bank.iter().map(|&v| v as i64).collect(),
            ),
            (
                "xp_vol",
                self.book.xp_vol.iter().map(|&v| v as i64).collect(),
            ),
        ];
        (scalars, arrays)
    }
}

/// ⭐ THE NATIVE WORLD MINTS THE HUMAN'S POOL RECORD (round 112).
/// Retail's `sub_5C950` runs for player 0 like any other: `AddPlayer_4A920`
/// (EF:33326) pops a (3,0) record at the (3,4) start marker — action 0,
/// maxLife 10000, minSpeed 80, `byte_0x38_56 = 29`, id = its own slot,
/// sprite 44, map-linked — and only THEN mints the book's class-15
/// tokens; the rivals follow. The native port kept the human out of the
/// pool (slot 0) and minted the tokens straight after the THINGs, so
/// every later record sat one slot low (26 on a full book), and every
/// slot-seeded law downstream — `rand = slot + global`, `f63 % n`
/// cadences, which free slot a painter pops relative to its castle's
/// walk position — ran on the wrong slot. `terrain-check`: mc2l0's 14
/// load-time (10,0) fires burned the start pad with a shifted stream
/// (18 height / 27 shading cells); mc2l22's (207,157) castle painter
/// popped a slot BELOW its castle, missed tick 0 and stood one rise
/// step short (11 cells). With the record in place the certified
/// in-walk carpet arms (`mc2_carpet_slot != 0`, round 103) now run
/// natively too. Set `MGC_NO_MC2_NATIVE_HUMAN_RECORD=1` for A/B.
pub(crate) fn no_mc2_native_human_record() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_NATIVE_HUMAN_RECORD").is_some())
}

impl World {
    /// `AddPlayer_4A920` (EF:33326) inside `sub_5C950` for player 0:
    /// the human's own pool record, popped at the point retail pops
    /// it — after the THINGs, BEFORE the book's tokens and the rivals
    /// — so every later slot lays out like retail's. See
    /// [`no_mc2_native_human_record`].
    ///
    /// THE REPRESENTATION IS THE IMPORT'S (conformance.rs): the slot
    /// stays CLASS 0 — an empty, pinned record whose `rand` lane is
    /// the live per-entity stream (the death scatter spends it) — and
    /// the pose stays the runner's input, anchored at the slot by the
    /// walk. A live (3,0) record with a frozen pose is WORSE than none:
    /// every class-3 scan (creature targeting, the wizard lists) chases
    /// a carpet parked at the start marker (`mc2_castle` test
    /// `a_rising_castle_executes_what_stands_under_it`: the victims
    /// wandered off toward the marker and the castle rose on nobody).
    /// A world with no marker (a bare test world) keeps slot 0.
    pub(crate) fn mc2_spawn_human_record(&mut self, hx: Hx) {
        if no_mc2_native_human_record() || self.mc2_carpet_slot[hx] != 0 {
            return;
        }
        let seat = self.g.human_seat(hx) as usize;
        if self.start_markers[seat].is_none() {
            return;
        }
        let Some(i) = self.g.new_event() else { return };
        // `new_event` seeded `rand` from the slot like `NewEvent_4A050`;
        // everything else stays the import's `Ent::default()`, class 0.
        let rand = self.g.ent[i].rand;
        self.g.ent[i] = crate::engine::features::Ent::default();
        self.g.ent[i].rand = rand;
        // `sub_5C950`/`AddPlayer_4A920` place player 0 at its own start
        // marker BEFORE `sub_55AB0` reifies the carried book, and the
        // reify mints each token at the CARPET's `position_0x4C_76`
        // (Level.cpp:1319). Without this the `human_pose` register is
        // still the constructor's (0,0,0) when the book is granted.
        // [`crate::engine::features::no_mc2_native_human_start_pose`].
        if !crate::engine::features::no_mc2_native_human_start_pose() {
            if let Some((mx, my)) = self.start_markers[seat] {
                let x = (mx << 8).wrapping_add(128);
                let y = (my << 8).wrapping_add(128);
                let z = (self.g.ground_z(x, y) as i16).wrapping_add(0x100);
                let e = &mut self.g.ent[i];
                e.x = x;
                e.y = y;
                e.z = z;
                self.set_human_pose(hx, (x, y, z));
                // …and his seat in that tile's chain, taken where
                // retail links him: before the book's tokens.
                if crate::mc1::rivals::native_human_seat() {
                    self.g.player_relink(hx, x, y);
                }
            }
        }
        self.mc2_carpet_slot[hx] = i as u16;
        self.g.mc2_pinned[hx] = crate::engine::features::Mc2Pinned(i as u16);
        self.g.set_human_tag_slot(hx, i as u16);
    }

    /// Wire the MC2 level's wizards: colors `1..player_count` spawn
    /// as AI rivals at their (3,4+color) start markers (the
    /// `sub_53160` activation walk under the NumberOfPlayers pump
    /// bound — lifecycle trace §1 + the header-unk09 identification).
    /// The human's colour ([`crate::engine::features::Gen::human_seat`],
    /// 0 unless `--seat`) stays out-of-pool; it gets NO authored
    /// starting castle (open-closure §7).
    pub fn set_mc2_wizards(&mut self, configs: &[Option<Mc2RivalConfig>; 8], player_count: u16) {
        for slot in 0..player_count.min(8) as u8 {
            if self.hx_at_wizard(slot).is_some() {
                continue;
            }
            let Some(cfg) = &configs[slot as usize] else {
                continue;
            };
            self.mc2_spawn_rival(slot, cfg.clone());
        }
    }

    /// Seat the whole MC2 roster in retail's colour order (`sub_53160`
    /// runs `sub_5C950` for colour 0..NumberOfPlayers, EF:38410): at
    /// the human's colour his (3,0) record
    /// ([`World::mc2_spawn_human_record`]) and then his default book
    /// ([`World::mc2_seed_default_spells`]); a rival at every other.
    /// At colour 0 this is exactly the classic sequence — the
    /// constructor's record and book, then `set_mc2_wizards`. A world
    /// built by [`World::new_full_env_seated`] with a nonzero seat
    /// must be seated through here.
    pub fn seat_mc2_wizards(&mut self, configs: &[Option<Mc2RivalConfig>; 8], player_count: u16) {
        // One human at seat 0: the constructor already popped him.
        if self.hx_at_wizard(0).is_some() && self.players.len() == 1 {
            self.set_mc2_wizards(configs, player_count);
            return;
        }
        for slot in 0..player_count.min(8) as u8 {
            if let Some(hx) = self.hx_at_wizard(slot) {
                // Seat 0 may already stand (the constructor's pop and
                // book seed); every other human pops in his turn.
                let fresh = self.mc2_carpet_slot[hx] == 0;
                self.mc2_spawn_human_record(hx);
                if fresh {
                    self.mc2_seed_default_spells(hx);
                }
            } else if let Some(cfg) = &configs[slot as usize] {
                self.mc2_spawn_rival(slot, cfg.clone());
            }
        }
    }

    /// Seat a NETWORK game's roster (the port's netplay): every human
    /// at his colour in colour order — all eight colours, retail's
    /// network game seats the JOINED count, not the header's
    /// NumberOfPlayers — and AI wizards in the first `ai` free colours
    /// the level authors a rival for (retail's network game seats none;
    /// the count is the session's rule). Same per-colour arms as
    /// [`World::seat_mc2_wizards`].
    pub fn seat_mc2_net(&mut self, configs: &[Option<Mc2RivalConfig>; 8], ai: u8) {
        let mut left = ai;
        for slot in 0..8u8 {
            if let Some(hx) = self.hx_at_wizard(slot) {
                let fresh = self.mc2_carpet_slot[hx] == 0;
                self.mc2_spawn_human_record(hx);
                if fresh {
                    self.mc2_seed_default_spells(hx);
                }
            } else if left > 0
                && let Some(cfg) = &configs[slot as usize]
            {
                self.mc2_spawn_rival(slot, cfg.clone());
                left -= 1;
            }
        }
    }

    /// `sub_5C950` (EF:43600), the fresh-spawn arm: the (3,1) carpet
    /// at `array_0x2362[color]` raised ground+0x100, base stats, the
    /// AI personality + Life scalar, the book from the map masks,
    /// and the authored starting castle.
    fn mc2_spawn_rival(&mut self, slot: u8, cfg: Mc2RivalConfig) {
        // Start marker (3,4+color); a color with no marker keeps the
        // memset-0 position (map origin) — the retail authoring
        // contract (lifecycle §1 item 3).
        let (mx, my) = self.start_markers[slot as usize].unwrap_or((0, 0));
        let x = (mx << 8).wrapping_add(128);
        let y = (my << 8).wrapping_add(128);
        let z = (self.g.ground_z(x, y) as i16).wrapping_add(0x100);
        let Some(i) = self.g.new_event() else { return };
        {
            let e = &mut self.g.ent[i];
            e.class64 = 3;
            e.model65 = 1;
            e.tick70 = 1; // action 1 = the AI tick (EF:43696)
            e.max_life = 10000;
            e.row156 = WIZARD_ROW;
            e.f128 = BEHAVIOR[WIZARD_ROW as usize].v_0.max(80);
            e.id24 = i as u16; // self owner-tag (the MC1 convention)
            // byte_0x38_56 = 29: the vulnerability mask both wizard
            // ctors write (AddPlayer_4A920 / sub_4A9C0, EF:33326/33352)
            // — ch0 damage + ch2/ch3/ch4 (claim/steal/grip). Without
            // it f28 stays 0 and `area_write`'s per-channel gate drops
            // EVERY hit at the mailbox: a fireball detonates ON the
            // rival but deals nothing (unkillable). debug_kill injects
            // mail directly, so mortality tests do NOT exercise this.
            e.f28 = 29;
        }
        self.g.link(i, x, y, z);
        // Carpet sprite by color: retail switches on
        // TransformPlayerColorIndex (EF:43732) -> models 273..279
        // (colour 0 keeps 44) — the art families are authored in
        // Transform order (crate::mc2::COLOR_ART). A rival reads the
        // row-44 arm only at colour 0, i.e. when the human flies
        // another (`--seat`); the shared helper is what the replay
        // ghost needs.
        self.g
            .mc2_set_sprite(i, crate::mc2::carpet_sprite_row(slot));
        let mut r = Mc2Rival::new(slot, i as u16, &cfg);
        // Life scalar: wizard maxLife *= Life/256 (EF:43768-72).
        self.g.ent[i].max_life = ((10000u64 * r.life_scale as u64) >> 8).max(1) as u32;
        self.g.refill_life(i);
        // ⭐⭐ THE PURSE STARTS FULL, ON THE ENTITY. `sub_5C950`'s
        // tail (EF:43825-28) is `life = maxLife; mana = maxMana`
        // (`maxMana_0x8C_140 = 1000`, EF:43722) for EVERY wizard the
        // level starts, human and AI alike — the same two statements
        // the respawn arm already carries. The port's fresh arm
        // stopped at the life line: the (3,1) record kept the ctor's
        // memset 0 in `f140`, and since `Mc2Rival::mana` is only a
        // MIRROR of `f140` (re-seeded every tick by
        // `mc2_rival_alive`), the ctor's own `mana: 1000` was
        // overwritten on the first tick and the rival climbed 0 → 1000
        // at 100/tick — its first castle came at tick 11 on mc2l4
        // where retail's rival 2 (record 0: castle 304 + painter 305
        // both ONE dispatch old, f63 = slot + 1) cast on tick 8.
        if !no_mc2_rival_start_purse() {
            let e = &mut self.g.ent[i];
            e.f136 = 1000; // maxMana_0x8C_140 (EF:43722)
            e.f140 = 1000; // `mana_0x90_144 = maxMana` (EF:43826-28)
        }
        // ⭐⭐ AND THE CASTLE RECAST COUNTER IS COLOUR-STAGGERED AT
        // SPAWN. The model-1 arm of the same tail (EF:43847-52) resets
        // the brain state, pins every hate ledger to neutral, and then
        // writes `SpellEnabled[2] = 4 * playerColorIndex` — the
        // per-spell recast counter `sub_12A70` decrements once per
        // tick (EF:5335-38) and `sub_15170`'s case 2 reads as
        // `!SpellEnabled[2]` (EF:7032/7043). So colour 2 cannot cast
        // Create Castle before its 8th tick, colour 1 before its 4th,
        // and the human (colour 0) is never held. That is the whole
        // reason retail's mc2l4 rival 2 sits in Build with a full
        // purse for seven ticks and founds castle 304 on tick 8 — the
        // castle's 3×3 stamp lands on the (10,11) island at (64,0)
        // BEFORE record 0, and terrain-check's port world had no
        // castle there at all (the 9-cell height residual of round
        // 112-4).
        if !no_mc2_castle_colour_cooldown() {
            r.cooldown[2] = 4 * slot as u16;
        }
        // The ladder reads the owner's Life scalar per slot.
        self.g.mc2_life_scale.0[slot as usize] = r.life_scale;
        // …and it reaches that slot through `rival_ents`, the port's
        // stand-in for retail's per-entity player-block back-pointer
        // `dword_0xA4_164x`, which `sub_5C950` wires HERE — at wizard
        // birth, above the authored castle (EF:44058). See
        // [`no_mc2_wiz_block_at_birth`].
        if !no_mc2_wiz_block_at_birth() {
            self.g.rival_ents[slot as usize] = i as u16;
        }
        // The book: `InitialiseSpells_54A50` (EF:38650) — AI grant =
        // granted && !blocked; starting LEVEL = byte_0x360FBx clamped
        // <= 2, written straight into SpellLevels (no XP accrual).
        // spellIndex_D94FF is identity for the 26 real spells
        // (open-closure §4) — raw spell-id indexing.
        //
        // ⭐⭐ THE AUTHORED LEVEL LANDS ON ALL 26 SPELLS, NOT ONLY THE
        // GRANTED ONES. The AI arm is a bare `if (IsAiPlayer == 1)`
        // inside the 26-iteration walk, taken BEFORE the
        // StartingSpells/BlockedSpells test that computes `setSpell`
        // (EF:39025-29, NETHERW.EXE 0x792e5 `cmpb $0x1,0x9(%edx)` /
        // `je 0x7938f`, and the arm itself is the two-instruction
        // 0x793a8 `mov 0x360fb(%ebx,%eax,1),%bl` / 0x793af
        // `mov %bl,0x803(%edi)`). So a spell the map BLOCKS or does
        // not grant still gets its `byte_0x360FBx` level written into
        // `SpellLevels_0x41D`.
        if !no_mc2_rival_authored_spell_levels() {
            for s in 0..MC2_SPELLS {
                r.book.levels[s] = cfg.start_level[s].min(2);
            }
        }
        for s in 0..MC2_SPELLS {
            if cfg.start[s] && !cfg.blocked[s] {
                r.known[s] = true;
                let lvl = cfg.start_level[s].min(2);
                r.book.levels[s] = lvl;
                // ⭐ THE QUICK SLOTS ARE BOUND HERE, IN THE SAME WALK.
                // `InitialiseSpells_54A50` clears both hands to -1 up
                // front (EF:38998-99) and then, on every `setSpell`
                // spell in index order, fills LEFT if it is still -1
                // else RIGHT if THAT is still -1 (EF:39089-96;
                // NETHERW.EXE 0x794d9..0x79500). No `break`: once both
                // are seated the rest of the walk simply skips the
                // test, so the pair is the FIRST TWO granted spells.
                if !no_mc2_rival_authored_hands() {
                    if r.book.left == -1 {
                        r.book.left = s as i8;
                    } else if r.book.right == -1 {
                        r.book.right = s as i8;
                    }
                }
                // ⭐ THE AUTHORED START IS A **LEVEL**, NOT A
                // SELECTION. `InitialiseSpells_54A50` opens by zeroing
                // `array_0x437_1079x.SpellIndex[i]` — the SELECTED
                // tier — for all 26 spells (EF:38658), and the AI arm
                // writes `byte_0x360FBx` into `SpellLevels_0x41D_1053z`
                // ONLY (EF:38692). `sub_55AB0`'s mint then stamps
                // `SetSpell_6D5E0(token, array_0x437[spell])`
                // (Level.cpp:1319) — i.e. TIER 0 — and no AI path ever
                // writes that array again (`sub_6D9C0` only clamps it
                // DOWN, EF:43908-09; the tier dynamics all ride
                // `sub_15F20`'s SetSpell side effect, which does not
                // touch it). Measured on mc2l22, whose seven rivals are
                // authored `starting_spell_levels = [2; 26]`: retail's
                // player blocks read `levels = [2; 26]` and
                // `sel = [0; 26]` at t=0, t=1092 and t=6616, and every
                // one of their 175 (15,x) tokens records the TIER-0
                // cost at t=0 (rival 611 slot 629 = (15,17) mana_max
                // 10000 / mana 476, not tier 2's 15000 / 365).
                r.book.sel[s] = 0;
                if let Some(m) = self.mc2_mint_rival_manifestation(&r, s) {
                    r.book.ent[s] = m as u16;
                }
            }
        }
        // Authored starting castle: AI-only AND Create-Castle-gated
        // (EF:43775-77).
        if cfg.castle_level > 0 && r.known[2] {
            self.mc2_spawn_authored_castle(&mut r, cfg.castle_level);
        }
        // The post-spawn truce: every OTHER wizard ON THE TICK-TOP
        // CLASS-3 ROSTER takes the elevated-but-decaying 40927 toward
        // this newcomer (EF:44185-93). At LEVEL-START seating that
        // roster is empty — the chain is rebuilt before the input
        // handlers run and no wizard record existed yet — so nothing
        // is stamped. `MGC_NO_MC2_RIVAL_TRUCE_ROSTER=1` restores the
        // flat every-rival loop.
        if no_mc2_rival_truce_roster() {
            for other in &mut self.mc2_rivals {
                other.hate[slot as usize] = HATE_RESPAWN;
            }
        } else {
            self.mc2_truce_roster(slot as usize, i as u16);
        }
        // Team resolver for owner recolors (balls/balloons/flags).
        // ⚠ Already seated at birth unless the kill switch is set —
        // see [`no_mc2_wiz_block_at_birth`]. Kept unconditional so the
        // switched-off arm is byte-identical to the pre-dig tree.
        self.g.rival_ents[slot as usize] = i as u16;
        self.mc2_rivals.push(r);
        self.entities_dirty = true;
    }

    /// A rival-owned class-15 spell manifestation: the token entity
    /// in its owned state (3M), wired at the rival's current tier
    /// (the `sub_55AB0` reify + `SetSpell_6D5E0` pair, Level:1305).
    fn mc2_mint_rival_manifestation(&mut self, r: &Mc2Rival, spell: usize) -> Option<usize> {
        let (x, y, z) = {
            let e = &self.g.ent[r.ent as usize];
            (e.x, e.y, e.z)
        };
        let m = self.mc2_new_spell_token(spell as u8, x, y, z)?;
        {
            let e = &mut self.g.ent[m];
            e.tick70 = (spell as u8).wrapping_mul(3); // owned state
            // The reify does NOT arm the re-steal lock — see
            // [`crate::engine::features::no_mc2_pickup_only_steal_lock`].
            if crate::engine::features::no_mc2_pickup_only_steal_lock() {
                e.f54 = 64;
            }
            e.id24 = r.ent;
            e.f26 = 0;
            e.f44 = 0;
        }
        self.mc2_rival_set_spell(m, r.book.sel[spell], r.ent);
        Some(m)
    }

    /// `SetSpell_6D5E0` for a rival-owned manifestation: the human
    /// arm ([`World::mc2_set_spell`]) resolves the castle spell's
    /// ladder cost against the HUMAN castle — this one uses the
    /// owner's own castle level.
    pub(crate) fn mc2_rival_set_spell(&mut self, m: usize, tier: u8, own: u16) {
        self.mc2_rival_set_spell_at(m, tier, own, None)
    }

    /// [`Self::mc2_rival_set_spell`] with an explicit FALLBACK pricing
    /// castle, used when the scan misses — the rival twin of the human
    /// arm's `snap` register snapshot. See
    /// [`crate::engine::features::no_mc2_rival_ladder_price_dying_castle`].
    pub(crate) fn mc2_rival_set_spell_at(
        &mut self,
        m: usize,
        tier: u8,
        own: u16,
        fallback: Option<usize>,
    ) {
        let spell = self.g.ent[m].model65 as usize;
        let Some(row) = self.g.assets.spells.get(spell).copied() else {
            return;
        };
        let count = (row.byte_0 as i16).max(1);
        let t = (tier as i16).min(count - 1).max(0) as usize;
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f44 = (t + 1) as u16;
            return;
        }
        let sub = row.tiers[t];
        let cost = if spell == 2 {
            // `GetSpellManaCost_6D710` (Level.cpp:1714-82) — the ONE
            // cost function, priced against the token's own parent
            // wizard: `SetSpell_6D5E0` calls it as
            // `GetSpellManaCost_6D710(Entities[entity->parentId_0x28_40],
            // model, tier)` (Level.cpp:1525), so a rival is priced off
            // ITS OWN `CastleEntityIndex_0x3A_58`, with the SAME two
            // arms the human column already runs.
            //
            // Retail's shape, in order (L:1717-82):
            //   * no own castle → the tier's RAW `manaCost_6`, with NO
            //     ladder and NO tier multiply (L:1717-27);
            //   * else the rung by the castle's `dword_0x10_16`
            //     (L:1729-55, [`crate::mc2::castle::MC2_CASTLE_COST`]);
            //   * level >= 7 returns the 300M sentinel unmultiplied
            //     (L:1756-60);
            //   * else the ×320>>8 (tier 1) / ×384>>8 (tier 2) TIER
            //     MULTIPLY (L:1761-70).
            // The +3000 re-cast surcharge is deliberately absent on
            // this column: retail's `add3000` is the caster's own
            // `byte_0x1BE_446`, and the ONLY writer is the human's
            // `PlayerAction 0x2A` voluntary demolish (EF:37993-95), so
            // it is provably 0 for every AI wizard.
            //
            // Measured: mc2l22 pair 6615→6616, rival 530's castle
            // token slot 533 — retail `mana_max` 30000 / `mana` 297
            // (= rung 2 20000 ×384>>8, /101), the port 20000 / 198.
            // Retail stamps the same multiply on that token at t=6619
            // (60000 = rung 3 ×1.5) and t=23090 (15000 = rung 1 ×1.5).
            match self.rival_castle(own).or(fallback) {
                None => sub.mana_cost,
                Some(c) => {
                    let lvl = self.g.ent[c].f26.clamp(0, 7) as usize;
                    let mut result = crate::mc2::castle::MC2_CASTLE_COST[lvl];
                    if lvl < 7 {
                        result = match t {
                            1 => (result * 320) >> 8,
                            2 => (result * 384) >> 8,
                            _ => result,
                        };
                    }
                    result.clamp(0, i32::MAX as i64) as i32
                }
            }
        } else {
            sub.mana_cost
        };
        let e = &mut self.g.ent[m];
        e.f71 = t as u8;
        e.f30 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        e.f28 = sub.word_0x18.max(0) as u16;
        e.f59 = (sub.font_type & 1 == 0) as u8;
        e.f136 = sub.max_mana_limit;
        e.max_life = cost.max(0) as u32;
        e.f140 = if e.f28 != 0 {
            cost / e.f28 as i32
        } else {
            cost
        };
    }

    /// The authored starting castle (EF:43779-43819 + castle-builder
    /// §1.2): a (3,2) at the wizard, level = players[color]-1, one
    /// terrain pass per authored level, extents + the HP/CAP ladder
    /// (which reads the owner's Life scalar), FULL stored mana
    /// clamped 320000, sound 30, standing.
    fn mc2_spawn_authored_castle(&mut self, r: &mut Mc2Rival, castle_level: u8) {
        let (wx, wy) = {
            let e = &self.g.ent[r.ent as usize];
            (e.x, e.y)
        };
        let Some(c) = self.g.new_event() else { return };
        {
            let e = &mut self.g.ent[c];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 4; // standing (the buildable steady state)
            e.max_life = 40000;
            // `byte_0x38_56 = 33` (EF:33433) — the castle's damage
            // channel mask; NewEvent leaves it 0.
            if !crate::mc2::mobs::no_mc2_castle_ch_mask() {
                e.f28 = 33;
            }
            e.id24 = r.ent;
            // Even-parity tile-corner snap (the shared castle anchor
            // law, MC1 :44229 = MC2 sub_4A9E0).
            let mut tx = wx >> 8;
            let ty = wy >> 8;
            if (tx.wrapping_add(ty)) & 1 == 1 {
                tx = tx.wrapping_add(1);
            }
            e.dest_x = tx << 8;
            e.dest_y = ty << 8;
        }
        let (sx, sy) = (self.g.ent[c].dest_x, self.g.ent[c].dest_y);
        // The ctor's corner-mean build datum (sub_4AA40 EF:33399) —
        // the painter/leveler read site_z, not the live ground.
        let z = self.g.mc2_castle_site_z((sx >> 8) as u8, (sy >> 8) as u8);
        self.g.ent[c].site_z = z;
        // ⭐⭐ TWO Z DATA, NOT ONE — AND THIS CALL PATH ONLY EVER HAD
        // ONE. `Gen::spawn_castle` (mc1/combat.rs) already carries the
        // law: `sub_4AA40`'s link takes the ctor LOCAL `v6ar`, whose z
        // is `getTerrainAlt_10C40(&predictedAxis_EB398ar)` — the ground
        // under the RAW caller point (EF:33385) — while the
        // perimeter-min `32 * sub_48E60(..)` overwrite lands on the
        // ENTITY's `axis_0x9A_154x.z` one line earlier (EF:33399) and
        // never touches `v6ar`. `AddEventToMap_57D70(v2x, &v6ar)`
        // (EF:33400) then links at the raw ground.
        //
        // The authored-castle arm of `sub_5C950` reaches that same ctor
        // through `IfSubtypeCallCreatingManaSphere_4A190(&v2x->
        // position_0x4C_76, 3, 2)` (EF:44131) — the RAW point is the
        // WIZARD's own position — but the port's copy of the ctor here
        // linked with `site_z`, so an authored castle whose perimeter
        // minimum differs from the ground under its wizard was born at
        // the wrong altitude. Invisible on a take settled far enough
        // for the standing tick's `z = ground_z(x, y)` refresh to run;
        // mc2l22-new is settled 2 ticks and catches it: retail slot 476
        // z 4623, port 4384 (= 32 x 137, the perimeter min).
        let link_z = if no_mc2_authored_castle_link_z() {
            z
        } else {
            self.g.ground_z(wx, wy) as i16
        };
        self.g.link(c, sx, sy, link_z);
        self.g.refill_life(c);
        // The team flag: retail `+90 += TransformPlayerColorIndex`
        // (EF:61133) — flag family 177 + COLOR_ART[slot] (the MC2
        // stage pieces carry the visible castle).
        // Round 110: 177 FLAT, no colour latch — the castle now takes
        // retail's own road: the ctor's build action (5) at level
        // `castle_level - 1`, whose FIRST tick runs `mc2_castle_build`
        // case 0 = the colour one-shot + the level-up commit that
        // paints BUILD00 row `castle_level` (the mint's j-loop below
        // stamps rows 0..castle_level-1 only — row 0 is empty, so a
        // level-1 castle owes its whole 4×4 keep to that first tick,
        // and a level-5 castle its outer walls: mc2l22 record 0 reads
        // 16 / 820 castle tiles where the standing-at-`level-1`
        // shortcut left 0 / 332).
        self.g.mc2_set_sprite(c, 177);
        let lvl = (castle_level - 1).min(7);
        self.g.ent[c].f26 = lvl as i16;
        // One INSTANT BUILD00 pass per authored level (EF:43787-43800):
        // retail loads the scratch slot 0 with the castle's site axis
        // and id, sets its row byte to `j` and runs `sub_36FC0` for
        // every `j in 0..castle_level` — rows 0..=level-1, cumulative,
        // synchronous, no painter record. ⚠ Round 110: this used to be
        // the (10,42) REPAINT painter settled at row `level - 1`, which
        // stamps nothing for castle level 1 (row 0 is a real 4×4 row on
        // MC2) and 332 of a level-5 castle's 820 cells — every authored
        // rival castle on mc2l22 (levels 5/5/5/7) and the level-1
        // stumps on mc2l4/mc2l6 were missing or short in the GENERATED
        // terrain for 100+ rounds (certification imports terrain).
        let id = self.g.ent[c].id24;
        for j in 0..castle_level {
            // Round 114: retail runs each row stamp ON THE SCRATCH SLOT
            // 0 (EF:43792-43802: `Entities[0]->position = castle->axis
            // _0x9A_154x` — x, y AND z —, `model = 0`, `dword_0x10 =
            // 0`, `id = castle id`, `byte_0x46 = j`, then
            // `sub_36FC0(Entities[0])`), and the residue is read: the
            // build-site scout `sub_13B00` (EF:6090) copies slot 0's
            // WHOLE position onto the scouting wizard, z included, so
            // a level's first rival scout inherits the LAST AUTHORED
            // CASTLE'S SITE Z (mc2l4 record 0: slot 0 z 2624 = castle
            // 297's `dest_z`, rival 298's `dest_z` 2624). The port
            // passed the row as an argument and left slot 0's z at 0;
            // the same shape as the castle-downgrade scratch write
            // (`mc2_castle_tick`, ledger 95). `id_0x1A_26` is NOT
            // written — the port fuses it with `parentId_0x28_40` in
            // `id24` and nothing reads the sentinel's id.
            if !no_mc2_authored_scratch_site() {
                let s = &mut self.g.ent[0];
                s.x = sx;
                s.y = sy;
                s.z = z;
                s.model65 = 0;
                s.f26 = 0;
                s.f71 = j;
            }
            self.g.mc2_stamp_build_row_instant((sx, sy, z), j, id);
        }
        // The ctor's state (`sub_4AA40` EF:33377: actionIndex 5 = the
        // build machine, sub-state 0): the first tick commits the
        // level-up to `castle_level`.
        self.g.ent[c].tick70 = 5;
        self.g.ent[c].f59 = 0;
        self.g.ent[c].f50 = 0;
        // Extents + ladder (Life-scaled HP) + the stage pieces.
        self.g.mc2_castle_extents(c, lvl);
        self.g.mc2_castle_ladder(c);
        self.g.mc2_castle_stages(c);
        // Spawns FULL of mana, clamped 320000 (EF:43812-17).
        let cap = self.g.ent[c].f136;
        self.g.ent[c].f140 = cap.clamp(0, 320_000);
        self.g.snd(30, c);
        self.terrain_dirty = true;
        let _ = r;
    }

    // ---- the per-tick brain (sub_12910 EF:5243) --------------------------

    /// Class-3 model<=1 pool dispatch on the MC2 column: resolve the
    /// rival record; a level-authored husk with no record stands.
    pub(crate) fn mc2_rival_entity_tick(&mut self, i: usize) {
        let Some(ri) = self.mc2_rivals.iter().position(|r| r.ent as usize == i) else {
            return;
        };
        if self.mc2_rivals[ri].eliminated {
            // Retail still dispatches the corpse: `sub_5E7C0`'s
            // banished arm, call-free.
            if self.g.ent[i].tick70 == 3 {
                // ⭐⭐⭐ A LAW ON ONE CALL PATH IS NOT LANDED. The
                // knock wipe sits ABOVE `sub_5E7C0`'s AI/castle
                // branches, so the BANISHED corpse takes it too.
                if !no_mc2_dead_wait_knock_clear() {
                    self.mc2_rivals[ri].knock_mag = 0;
                }
                if !crate::engine::features::no_mc2_m27_v34_corpse_transparent() {
                    self.g.m27_v34_transparent(i);
                }
            }
            return;
        }
        match self.g.ent[i].tick70 {
            // Death fall (action 2, sub_5E310 EV:2882).
            2 => self.mc2_rival_death_fall(ri, i),
            // Dead-wait (action 3, sub_5E7C0 EV:2895).
            3 => self.mc2_rival_dead_wait(ri, i),
            // Alive (action 1).
            _ => self.mc2_rival_alive(ri, i),
        }
        // ⭐⭐⭐ …AND THE BOOK KEEPS TICKING ON THE CORPSE. See
        // [`rival_corpse_token_off`]: the buff bodies run at the
        // TOKEN's slot, above this wizard in the walk, and their
        // refused arm is the teardown that clears the caster's
        // deflect/absorb bits.
        self.mc2_rival_corpse_tokens(ri, i);
    }

    /// The refused arms of `sub_6AA00` / `sub_6A480` for a caster
    /// `sub_68D50` has stopped affording — see
    /// [`rival_corpse_token_off`] for the citations and the witness.
    /// Runs AFTER the wizard's own body because a level-load book
    /// sits ABOVE its wizard in the ascending entity walk.
    fn mc2_rival_corpse_tokens(&mut self, ri: usize, i: usize) {
        if rival_corpse_token_off() || rival_buff_bits_off() {
            return;
        }
        // `sub_68D50`'s first two statements (EF:55549-52). Evaluated
        // HERE, at the token's own dispatch point, so a wizard the
        // dead-wait respawned this very tick is affording again and
        // keeps its freshly minted book.
        if self.g.ent[i].act_life >= 0 && self.g.ent[i].f140 >= 0 {
            return;
        }
        let book = self.mc2_rivals[ri].book.ent;
        // ⭐⭐⭐ EVERY CLASS-15 BODY CARRIES THE REFUSED ARM, NOT JUST
        // SHIELD AND REBOUND. The `word_0x2E_46 = 1;` else-arm sits in
        // all of `sub_693F0` (EF:55885), `sub_69640` (POSSESSION L1+,
        // EF:56021), `sub_6A030` (EF:56349), `sub_6A300` (HEAL,
        // EF:56468), `sub_6A5C0` (EF:56690), `sub_6AA00`, `sub_6AB00`,
        // `sub_6AD60`, `sub_6B1C0`, `sub_6B310`, `sub_6B3E0`,
        // `sub_6B610`, `sub_6B870`, `sub_6BAB0`, `sub_6BCF0`,
        // `sub_6BF30`, `sub_6C170`, `sub_6C3E0`, `sub_6C620`,
        // `sub_6C870`, `sub_6CAC0`, `sub_6CD20`, `sub_6CFA0` (EF:56756
        // … 58172), and every one of them is followed by the shared
        // post-decrement + `sub_6D880` drain — so the tick a wizard's
        // life goes negative, EVERY live window in its book collapses
        // to 0 and applies its queued tier. The two exceptions are the
        // two bodies that run at the token's OWN slot on every book:
        // CASTLE (2, `sub_69AB0`'s upgrade lock — its `sub_68D50`
        // failure clears the lock in [`Self::mc2_rival_castle_token_tick`])
        // and SPEED (3, `GetScroll_69DB0` EF:56216-19, whose refused
        // arm is EMPTY — only the decrement runs, at
        // [`Self::mc2_rival_manifestation_tick`]).
        //
        // WITNESS (mc2l22 t=6497, the take's horizon after round 101):
        // rival 557 dies at its own slot (life 1209 -> -391, killer
        // 424) and its brain tail's tier-down walk has ALREADY run
        // `SetSpell(token, 0)` on the possession token 559 with the
        // window live (40), so the tier is QUEUED (`word_0x2C_44` 1).
        // Retail's token then takes `sub_69640`'s refused arm at slot
        // 559: window 40 -> 0, `sub_6D880` drains the queue and
        // `SetSpell` lands tier 0 — `b46 1 -> 0`, `f2a 15 -> 10`, `f30
        // 41 -> 3`, `d88 1000 -> 0`, `mana_max 250 -> 100`, `mana 6 ->
        // 33` (100 / word_0x18 3). The port's list here read `[6, 8]`,
        // so the possession token froze at tier 1 with the queue
        // parked — `dump-state 6497 559 --port --start 6496` printed
        // exactly those seven lanes ≠.
        for s in 0..MC2_SPELLS {
            if s == 2 || s == 3 {
                continue;
            }
            let m = book[s] as usize;
            if m == 0 || m >= self.g.ent.len() {
                continue;
            }
            // ⭐⭐⭐ THE STAND-IN'S SCOPE, NOT JUST ITS BODY. This is
            // [`Self::mc2_rival_buffs`]'s own `if m < own_slot {
            // continue }` rule: a book BELOW its wizard runs its real
            // body at the token's own slot
            // ([`Self::mc2_rival_token_tick`], dispatched from
            // `mc2_manifestation_pass`), and that body already carries
            // the refused arm (`if !afford { f26 = 1 }`) with NO
            // wizard-liveness gate of its own — so a respawn-minted
            // book already tears itself down on a corpse and must not
            // be touched twice.
            if m < i {
                continue;
            }
            // ⭐⭐⭐ AND THE POINTER ONLY RUNS ONE WAY IN RETAIL.
            // `sub_6AA00`/`sub_6A480` are reached from the class-15
            // walk arm at the TOKEN's slot — `if (class_0x3F_63 == 15
            // && action == 3 * model_0x40_64)` (the port's own
            // dispatch condition, `engine/world.rs` `t ==
            // model.wrapping_mul(3)` -> `mc2_manifestation_pass`) —
            // and only THEN resolve the caster through
            // `Entities[parentId_0x28_40]`. Retail never walks
            // caster -> book, so a book slot that has been FREED BY
            // THE DEATH PAYOUT and re-popped as something else is
            // inert there by construction. `Mc2Spellbook::ent` is a
            // port-side register that keeps pointing at the recycled
            // slot, so the record must be re-validated here.
            //
            // MEASURED (mc2l22 t=19,501, `MGC_WRITE_TRACE=1`): rival
            // 451's book still named slot 1, by then a `(5,23)` in
            // action 185, and this pass stamped `f26 54 -> 0`,
            // `f44 118 -> 0` and then `mc2_rival_set_spell` re-minted
            // the creature as a spell token — `max_life 10000 ->
            // 24000`, `f30 230 -> 10000`, `f136 0 -> 400000`,
            // `f140 20600 -> 275`. 2,698 introduced segments, all of
            // them this.
            // ⚠ `(5,23)` is on the `ramp2c` list, where `f44` holds
            // retail's `@0x2C` and not `@0x2A` — a stray `f44` write
            // means something DIFFERENT there.
            {
                let e = &self.g.ent[m];
                if e.class64 != 15 || e.model65 as usize != s || e.tick70 as usize != s * 3 {
                    continue;
                }
            }
            if self.g.ent[m].f26 > 0 {
                // The refused arm. Rebound: `word_0x2E_46 = 1` then
                // the shared post-decrement — i.e. 0. Shield: a bare
                // `word_0x2E_46 = 0` (LABEL_12).
                self.g.ent[m].f26 = 0;
                // `sub_6D880` — the queued-tier drain, reached by
                // both bodies on the tick the window hits 0.
                if self.g.ent[m].f44 != 0 {
                    let queued = (self.g.ent[m].f44 - 1) as u8;
                    self.g.ent[m].f44 = 0;
                    let own = self.mc2_rivals[ri].ent;
                    self.mc2_rival_set_spell(m, queued, own);
                }
                // The SHIELD clears on this same pass (LABEL_13); the
                // REBOUND clear is the function's TOP arm and waits
                // for the next one.
                if s == 6 {
                    self.mc2_rival_clear_buff(ri, s);
                    self.mc2_rivals[ri].shield_state = 0;
                }
                // The other two bodies with an expiry tail past the
                // drain — the same pair [`Self::mc2_rival_buffs`]'s
                // natural expiry runs: METAMORPH (`sub_6A030`
                // EF:56399-408) and INVISIBILITY (`sub_6B1C0`
                // EF:57108-09, the caster's `byte[0] &= 0xDF`).
                if s == 4 {
                    self.mc2_rival_metamorph_expire(m, i);
                }
                if s == 0xB && !rival_metamorph_cloak_off() {
                    self.g.ent[i].flags &= !0x20;
                }
            } else if s == 8 {
                // `sub_6AA00`'s `if (word_0x2E_46 <= 0)` arm —
                // idempotent, and it is what actually drops
                // [`F_REBOUND`] off the corpse.
                self.mc2_rival_clear_buff(ri, s);
            }
        }
        self.entities_dirty = true;
    }

    /// Conformance re-anchor (the MC1 rival-freeze fix's MC2 twin,
    /// engine/world/conformance.rs): point the brain record at the
    /// imported carpet slot and reseed the motion/economy lanes the
    /// per-tick arms consume — without this every imported rival
    /// carpet was a frozen husk (the dispatch above keys on `ent`,
    /// which the world-build seeded with fresh spawn slots).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reanchor_mc2_rival(
        &mut self,
        ri: usize,
        ent: u16,
        vdes: i16,
        strafe: i16,
        grace: u16,
        mana: i32,
        mana_max: u32,
        mana_delta: i32,
    ) {
        let r = &mut self.mc2_rivals[ri];
        r.ent = ent;
        r.eliminated = ent == 0;
        if ent == 0 {
            return;
        }
        r.vdes = vdes;
        r.strafe = strafe;
        r.grace = grace;
        r.mana = mana;
        r.mana_max = mana_max;
        r.mana_delta = mana_delta;
    }

    /// Conformance re-anchor, decision half: reconstruct the wizard-
    /// extension AI lanes so the imported rival resumes mid-decision
    /// instead of re-deciding from a world-build default. The retail
    /// dispatch reads `byte_0x1C1_449` FIRST and the selector writes
    /// it LAST (EF:5252 vs :5517-70), so an un-imported state re-runs
    /// the whole cascade on the pair's tick and the replayed rival
    /// makes its OWN decision — the residue the freeze fix left.
    ///
    /// The target and the scouted site ride the wizard ENTITY
    /// (`word_0x96_150`/`word_0x98_152` and `axis_0x9A_154x`,
    /// EF:6114-15), already imported by `import_ent`; everything else
    /// lives in the player block's `type_str_164` (+998) — see
    /// [`mgc_formats::mgcr::RetailPlayerMc2`] for the lane map.
    ///
    /// The signature is imported RAW rather than recomputed: it is
    /// retail's staleness detector (`sub_14C60` fails the target when
    /// the stored word no longer matches the slot's id+model+class,
    /// EF:6701), so recomputing would silently revive a target retail
    /// had already dropped. The human's carpet is out-of-pool here, so
    /// its target takes the port's [`PLAYER_TARGET`] convention on
    /// both lanes (the alive test ignores the signature for it).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reanchor_mc2_rival_ai(
        &mut self,
        ri: usize,
        ai: &Mc2RivalAi,
        book: &Mc2Spellbook,
    ) {
        let r = &mut self.mc2_rivals[ri];
        if r.eliminated {
            return;
        }
        r.state = Mc2AiState::from_retail(ai.state);
        r.target = ai.target;
        // VERBATIM — retail's `word_0x98_152` is the truth channel
        // for staleness. The old PLAYER_TARGET branch destroyed the
        // evidence: a retaliation write (sub_1BD90 EF:9002) moves
        // the target word ALONE, so a RaidCastle rival whose stored
        // sig still names the human's CASTLE must FAIL the guard and
        // go Fresh (retail's LABEL_12), not index the pool with the
        // sentinel (the mc2l22/mc2l6-rsg segmented-sweep panic).
        r.target_sig = ai.target_sig;
        r.site = ai.site;
        r.burst = ai.burst;
        r.poverty = ai.poverty != 0;
        r.cooldown = ai.cooldown;
        r.hate = ai.hate;
        for (w, &v) in r.war.iter_mut().zip(ai.war.iter()) {
            *w = v != 0;
        }
        r.weave = ai.weave;
        r.weave_dir = ai.weave_dir;
        r.avoid = ai.avoid;
        r.avoid_exit = ai.avoid_exit;
        r.agg = ai.aggression;
        r.per = ai.perception;
        r.refl = ai.reflexes;
        r.life_scale = ai.life_scale;
        // The brake word rides the players block, not the entity — a
        // re-anchored rival must resume retail's approach/brake
        // alternation phase or the whole SPEED window shifts a tick
        // (the mc2l22 every-other-tick pump).
        r.v14 = BrakeWord(ai.brake != 0);
        // The life-regen register is state for the same reason the
        // brake word is: it is applied before it is re-selected.
        r.life_delta = ai.life_regen as i32;
        // The knockback register: unspent until the death fall, so an
        // imported rival resumes owing whatever its last hit stamped.
        r.knock_dir = ai.knock_dir;
        r.knock_mag = ai.knock_mag;
        // The duel lock: retail state the import had no seat for at
        // all until this dig. `word_0x146_326 == 0` IS "no lock", and
        // the other two words are stale-but-present in that case, so
        // only the nonzero register becomes a `Some`.
        r.duel = if no_mc2_rival_duel_death_tether() || ai.duel_target == 0 {
            DuelLock(None)
        } else {
            DuelLock(Some((
                ai.duel_target,
                ai.duel_hold.clamp(1024, 3072),
                ai.duel_tier.min(2),
            )))
        };
        // The book: `SpellsEnabled_0x333` is the live manifestation
        // slot, and DEATH rewrites every owned entry to the boolean
        // marker 1 (EF:60147) — imported verbatim, quirk included,
        // because retail's own dead-window reads index the pool with
        // that 1. `known` is the nonzero test, which is exactly what
        // the marker encodes.
        r.book = *book;
        for s in 0..MC2_SPELLS {
            r.known[s] = book.ent[s] != 0;
        }
        // The record's copy is a MIRROR of the entity word, never a
        // second truth — [`Self::mc2_rival_alive`] re-reads
        // `word_0x96_150` at every dispatch. The conformance import
        // seats both (`import_ent` carries target96 → f146), but the
        // unit rigs call this alone; keep the pair consistent so a
        // seated target survives its first tick.
        let e = self.mc2_rivals[ri].ent as usize;
        if e != 0 && e < self.g.ent.len() {
            self.g.ent[e].f146 = ai.target;
        }
        // ⭐⭐ ONE RETAIL CELL, TWO PORT HOMES, AND THE IMPORTER FILLED
        // ONLY ONE. Retail keeps the wizard's LIFE SCALAR in exactly
        // one place — `Entities[..]->dword_0xA4_164x->word_0x24A_586`
        // — written unconditionally to 256 by `sub_5C950` (EF:43720)
        // and overwritten with the map header's authored value only
        // `if (Life_0x3612F)` on the AI arm (EF:43771-72). It is read
        // from the CASTLE's side by the HP ladder `sub_60810`
        // (EF:61704, `20000 * number1 >> 8` at EF:61710), which is why
        // the port mirrors it into `Gen::mc2_life_scale` — `Mc2Rival`
        // is out of reach there. Both runtime writers keep the pair in
        // sync; this seat did not, so a rival whose scalar had been
        // reset before the anchor tick came back with the map value.
        // WITNESS mc2l6-rsg anchored at 28444: the recording's own
        // rival-AI channel carries `life_scale` 82 / 256 / 90 for
        // players 1/2/3, while `Gen::mc2_life_scale` still read
        // [256, 82, 51, 90] — so rival 378's brand-new castle levelled
        // to `20000 * 51 >> 8 = 3984` where retail has 20000 (t=28707).
        // ⚠ COUNT-NEUTRAL ON THE FULL-TAKE PATH (the free run reaches a
        // rival respawn before the 28444 anchor and the respawn writer
        // happens to fix the mirror); witnessed only under an isolated
        // anchor. Landed as a structural import-seat correction — it is
        // measured identical on both horizons and both whole-take
        // censuses. Ledger ROUND 99 dig 99-15 law 2.
        if !no_mc2_life_scale_import() {
            let slot = self.mc2_rivals[ri].slot as usize;
            if slot < self.g.mc2_life_scale.0.len() {
                self.g.mc2_life_scale.0[slot] = ai.life_scale;
            }
        }
    }

    /// Housekeeping `sub_12A70` (EF:5320) + the state dispatch.
    pub(crate) fn mc2_rival_alive(&mut self, ri: usize, i: usize) {
        // ⭐⭐⭐ AND SO IS THE PURSE — see
        // [`crate::mc2::cast::no_mc2_wiz_purse_is_entity`] for the
        // citations (EF:5426 / EF:55274) and the mc2l22 t=3629/3630
        // witness. `Mc2Rival::mana` is a MIRROR of `f140`, not its
        // master: re-seed it here so an entity-side write landed by
        // any other slot's handler survives this wizard's own tick.
        if !crate::mc2::cast::no_mc2_wiz_purse_is_entity() {
            self.mc2_rivals[ri].mana = if no_rival_mana_overdraft() {
                self.g.ent[i].f140.max(0)
            } else {
                self.g.ent[i].f140
            };
        }
        // ⭐ THE ENTITY WORD IS THE TARGET; the brain record only
        // MIRRORS it. Retail's AI reads `a1x->word_0x96_150` fresh on
        // every use (sub_13890 EF:5952, sub_12FF0 EF:5585, the
        // selector's `sub_14C60` gates), so a write landed by anything
        // else since our last pick is already in force. The only such
        // writer is the impact re-point `sub_686D0` (mc2/proj.rs), and
        // it runs at the BOLT's slot — earlier in the walk than a
        // higher-slot wizard, later than a lower-slot one, which this
        // read-at-dispatch reproduces exactly. The SIGNATURE is
        // deliberately not touched: that mismatch is the mechanism.
        {
            let t = self.g.ent[i].f146;
            self.mc2_rivals[ri].target = t;
        }
        // Burst lockout recovery (EF:5357).
        if self.mc2_rivals[ri].burst < 0 {
            self.mc2_rivals[ri].burst += 1;
        }
        // Recast cooldowns (EF:5361-63).
        for c in self.mc2_rivals[ri].cooldown.iter_mut() {
            *c = c.saturating_sub(1);
        }
        self.mc2_rival_hate_decay(ri);

        // At the own castle: grace pinned 2 + the mailbox memset —
        // the AI DISCARDS damage at home (EF:5397-5414; the FORWARD
        // into the castle is human-only, EF:59961).
        // ⭐ THE FOURTH READER OF ONE PREDICATE. `sub_106C0(rival,
        // castle)` (EF:5396) is the FULL `sub_10630` (EF:3712-16):
        // SUMMED extents, strict `<`, z leg included — which is
        // exactly `Gen::ent_overlap`. The MC1 human (`regen_boost`),
        // the MC1 rival (mc1/rivals.rs) and the MC2 human all run it
        // already; this rival copy was the last bare `<= f80/f82`
        // test, and it reproduced the SAME symptom the human lane was
        // fixed for at mc1l0 t=1827 — retail +1000/tick vs port +100,
        // because the point test drops the RIVAL's own extent band.
        // mc2l6-rival-spells-galore t=148: dx 1755 sits in
        // [castle 1664, castle+rival 1787), so retail is still at
        // home and regens 1000 where we regened 100; at t=149 that
        // is the difference between affording SPEED (240) and not
        // (80). Retail's own release at t=150 (dx 1983 > 1787)
        // confirms the summed box.
        let castle = self.rival_castle(self.mc2_rivals[ri].ent);
        let at_castle = castle.is_some_and(|c| self.g.ent_overlap(i, c));
        // ⭐⭐⭐ **THE AT-CASTLE PIN IS AN ASSIGNMENT, NOT A FLOOR** —
        // `if (v2) word_0x159_345 = 2` (EF:5398-99), and `v2` is
        // initialised 0 at the top of `sub_12A70` (EF:5356) and raised
        // only by the castle-overlap test one line above. So docking
        // does not merely *keep* a wizard's grace alive, it CLOBBERS a
        // larger one down to 2 — and the very next statement's `--`
        // parks it at 1 for as long as the wizard stays home.
        //
        // That matters exactly once per life, and it is the tick a
        // rival respawns: `sub_5C950` re-anchors it AT ITS OWN CASTLE
        // with `word_0x159_345 = 100` (session 83 law D), so the first
        // `sub_12A70` after the respawn finds it docked and throws
        // 98 ticks of that grace away. mc2l6-rsg rival 378 respawns at
        // t=3084 with 100 and reads 1 from t=3085 on; the port's
        // `.max(2)` kept the whole 100 and ran the mailbox memset —
        // no `sub_5EFA0` at all — for 99 ticks it had no business
        // being immune for.
        //
        // ⭐ The MC2 HUMAN column has had the plain assignment since
        // it was written (`world.rs`, the `grace = 2` beside the same
        // memset). This is the session-82 shape once more: the rival
        // column missing what another column of this port already had
        // right.
        if at_castle {
            self.mc2_rivals[ri].grace = 2;
        }
        if self.mc2_rivals[ri].grace > 0 {
            self.mc2_rivals[ri].grace -= 1;
            self.g.ent[i].mail = [(0, 0); 6];
        } else {
            self.mc2_rival_intake(ri, i);
            if self.g.ent[i].act_life < 0 {
                // Lethal -> action 2, the death fall (EF:5416-19).
                self.g.ent[i].tick70 = 2;
                // ⭐⭐⭐ THE FALL VELOCITY IS NEVER RESET — IT
                // ACCUMULATES ACROSS LIVES. The lethal branch is
                // EXACTLY two instructions in the shipped
                // `NETHERW.EXE` (file 0x3740A-0x37415, linear
                // 0x12BAA): `cmp $0x2,%eax; jne; movb $0x2,0x45(%ebx);
                // xor %edx,%edx; jmp <epilogue>` — it stamps
                // `actionIndex_0x45_69` and returns, and there is NO
                // write to `word_0x2C_44` anywhere on it (EF:5416-19).
                // `sub_5E310`'s gravity leg (EF:60081-90) then reads
                // whatever @0x2C already held, so every death RESUMES
                // the previous death's terminal velocity. Witness
                // (`state.struct_b64` @0x2C, mc2l6-rsg rival 378):
                // 0 while alive -> the t=1846 fall walks it to -76 ->
                // it HOLDS -76 through the whole next life (respawn
                // t=3084) -> the t=14996 death resumes at -76 and
                // steps -78/-80/-82/-84 -> holds -84 to t=20747, and
                // so on through -86/-90/-104/-112 over six deaths.
                // The invented `f46 = 0` here made every fall but the
                // FIRST start from rest: retail's z steps 1114 -> 1034
                // at t=14997 (-76 of carried velocity plus the mover's
                // -4), the port stepped 1114 -> 1110 — the take's
                // horizon, and 15,229 gated ticks in four segments.
                //
                // ⭐⭐⭐ AND IT IS A SPLIT IN A SIBLING PAIR, PROVEN
                // BYTE-FOR-BYTE ON BOTH ARMS. The HUMAN's lethal
                // transition `AddPlayer03_00_5E010` (EF:60036-40, file
                // 0x82949-0x82953) DOES clear it -- `movb $0x2,0x45
                // (%ebx)` at 0x82949 followed by `movw $0x0,0x2c(%ebx)`
                // at 0x82953 -- while this one carries only the first
                // of those two stores. The port had copied the human
                // arm's reset onto the rival arm and made the pair
                // uniform. `world.rs`'s `player.fall_speed = 0` is
                // therefore CORRECT and must not be changed to match.
                if no_rival_fall_carry() {
                    self.g.ent[i].f46 = 0;
                }
                self.g.snd(16, i); // death sound 16 (EF:60039)
                // ⭐⭐⭐ **THIS `return` IS `sub_12A70`'s, NOT THE
                // BRAIN'S — THE STATE HANDLER AND THE SELECTOR STILL
                // RUN ON THE DEATH TICK.** Retail's wizard body is TWO
                // functions and the port fused them. `sub_12910`
                // (EF:5243) is:
                //
                // ```text
                //   sub_12A70(event);                        // result DISCARDED
                //   switch (event->…->byte_0x1C1_449) { … }   // state handler
                //   result = sub_12E70(event);                // the SELECTOR
                // ```
                //
                // and the lethal leg lives inside `sub_12A70`, whose
                // `return 0` merely hands control BACK to `sub_12910`.
                // Shipped `NETHERW.EXE`, `sub_12910` = file 0x37110
                // (linear 0x12910 + 0x24800; `sub_12A70` = 0x37270 and
                // `sub_12E70` = 0x37670 confirm the region constant):
                //
                // ```text
                //   37110: 53                push %ebx
                //   37111: 55                push %ebp
                //   37112: 89 e5             mov  %esp,%ebp
                //   37114: 8b 5d 0c          mov  0xc(%ebp),%ebx
                //   37117: 53                push %ebx
                //   37118: e8 53 01 00 00    call 0x37270      ; sub_12A70
                //   3711d: 8b 83 a4 00 00 00 mov  0xa4(%ebx),%eax
                //   37123: 8a 80 c1 01 00 00 mov  0x1c1(%eax),%al
                //   3712c: 3c 0e             cmp  $0xe,%al
                //   3712e: 0f 87 27 01 00 00 ja   0x3725b        ; default
                // ```
                //
                // — **the instruction right after the `call` reloads
                // `%eax` from the entity**, so `sub_12A70`'s return
                // value is clobbered before anything could test it, and
                // all fifteen switch arms (and the default at 0x3725b)
                // end `call 0x37670` = `sub_12E70`, the selector. The
                // lethal leg itself is at file 0x3740A: `cmp $0x2,%eax;
                // jne 0x3743f; movb $0x2,0x45(%ebx); xor %edx,%edx;
                // jmp 0x37663` — and 0x37663 is `sub_12A70`'s OWN
                // epilogue (`sub_12E70` begins at 0x37670). What the
                // leg really skips is only `sub_146F0` (0x38EF0, the
                // mover) and the regen / cadence / altitude-clamp tail
                // below it — which is what stays skipped here.
                //
                // WITNESS (mc2l6-rival-spells-galore, rival 378, the
                // take's own recording): at t=26,634 it takes lethal
                // damage — `life_0x8` 10000 → −29, `actionIndex_0x45_69`
                // 1 → 2 — and ON THAT SAME TICK retail's player block
                // steps `ai_state` 8 → 11 with `target96` 343 (the
                // human) → 772 (its OWN castle), because `sub_12E70`
                // still ran and `sub_13DC0`'s `maxLife/2 <= life` test
                // (EF:6168) is now false. Its aim setpoint `roll_0x20`
                // takes one last step 1009 → 1003 out of the state-8
                // handler and then FREEZES there for the 1,214 ticks
                // the corpse lies on the ground. The port's setpoint
                // froze one tick early at 1009 — retail's t=26,633
                // value — so when the respawn at t=27,849 re-ran the
                // yaw servo it tracked 1009 instead of 1003 and
                // `heading` read 1009 where retail read 1006. That was
                // the take's whole horizon wall, and `dump-state …
                // 27848 378 --port` printed exactly the two
                // consequences: `roll 1003/1009`, `target96 772/343`.
                //
                // ⚠ INVISIBLE TO EVERY CENSUS: `ai_state` is a SHADOW
                // lane and `target96`/`roll` are ungraded on a (3,1),
                // so the debt rode 1,214 bit-exact ticks before it
                // surfaced on a graded lane.
                if !no_rival_death_brain() {
                    self.mc2_rival_brain_tail(ri, i);
                }
                return;
            }
        }

        // Movement filter/step `sub_146F0` (EF:6415).
        self.mc2_rival_movement(ri, i);

        // ⭐ THE CAST-CHARGE METER IS EVERY WIZARD'S, NOT THE
        // HUMAN'S. `sub_12A70`'s regen block steps it one statement
        // after the movement call and one before `mana +=
        // manaRegen` (EF:5423-25) — the SAME `byte_0x154_340` the
        // human's carpet tick steps (world.rs, EF:5424-25), the same
        // 200 ceiling. The port stepped `wiz_charge[0]` only, so
        // every rival's meter sat frozen at whatever the import
        // seeded: mc2l6 wiz 1/2/3 part from retail at t=1 and never
        // rejoin (593 rows apiece over 600 ticks — the first thing
        // `wiz_shadow_mc2` ever said). ⭐ A LAW LANDED ON ONE CALL
        // PATH IS NOT LANDED; this is the session-73 shape again,
        // one column short.
        {
            let c = &mut self.wiz_charge[self.mc2_rivals[ri].slot as usize];
            if *c < 200 {
                *c += 1;
            }
        }

        // Regen (EF:5424-5455): delta applied first, then the rate
        // recompute — home /200 (mana min 1000), afield /2000 mana
        // (min 100) and /500 life. The dolmen-shrine flag (+17 0x10,
        // our 0x1000 — stamped by AddDolmen02_02's sweep) rides the
        // same fork and is consumed (cleared) by the fast branch
        // (EF:5438-45).
        let at_shrine = self.g.ent[i].flags & 0x1000 != 0;
        {
            let r = &mut self.mc2_rivals[ri];
            let stepped = r.mana as i64 + r.mana_delta as i64;
            r.mana = stepped.clamp(0, r.mana_max as i64) as i32;
            r.mana_delta = if at_castle || at_shrine {
                ((r.mana_max / 200) as i32).max(1000)
            } else {
                ((r.mana_max / 2000) as i32).max(100)
            };
            // Retail's purse LIVES on the entity (`mana_0x90_144 +=
            // manaRegen` EF:5424, clamped at EF:5456-59) — the life
            // half of the same block already writes the record; the
            // mana half must too (mc2l4 t=16 slot 292: +1000/tick at
            // the castle, the port's entity frozen at the import).
            self.g.ent[i].f140 = r.mana;
        }
        if at_castle || at_shrine {
            self.g.ent[i].flags &= !0x1000;
        }
        {
            // ⭐⭐ APPLIED FIRST, RE-SELECTED AFTER — see
            // [`Mc2Rival::life_delta`]. Retail's `life_0x8 +=
            // lifeRegen_0x163_355` (EF:5425) runs on the rate the
            // PREVIOUS tick stored; the fork that recomputes it
            // (EF:5439/5447) is twenty lines further down, past the
            // mana clamp, and shares its `v2` at-castle test with the
            // mana half already written above.
            let max = self.g.ent[i].max_life as i32;
            // ⭐ AND THE FLOOR IS RETAIL'S TOO. EF:5427-5430 is
            // `life += lifeRegen; if (life < -1) life = -1; if (life >
            // maxLife) life = maxLife;` — the port had the `+=` and the
            // ceiling and stopped one statement short. The MC1 twin
            // (`rival_alive_tick`, `.clamp(-1, max)`, remc1 :17992-98)
            // has carried both bounds since its own dig. Byte-proved on
            // BOTH shipped binaries — see
            // [`no_mc2_rival_life_floor`].
            let floor: i32 = if no_mc2_rival_life_floor() {
                i32::MIN
            } else {
                -1
            };
            let e = &mut self.g.ent[i];
            e.act_life = (e.act_life + self.mc2_rivals[ri].life_delta).clamp(floor, max);
            self.mc2_rivals[ri].life_delta = if at_castle || at_shrine {
                max / 200
            } else {
                max / 500
            };
        }

        // ⭐⭐⭐ THE TOKEN BODY RUNS **AFTER** THE WHOLE WIZARD BODY,
        // BRAIN INCLUDED — see the call below, moved out of here.
        if rival_buff_order_off() {
            self.mc2_rival_buffs(ri);
        }

        // Decision-cadence work (EF:5458-77): the reactive
        // anti-projectile defense (sub_15CB0 chain — open-closure
        // §3) + heal-when-hurt (spell 5).
        let think = self.g.ent[i].f63 % self.mc2_rivals[ri].think_period() == 0;
        if think {
            self.mc2_rival_react_defense(ri, i);
            if self.g.ent[i].act_life < self.g.ent[i].max_life as i32 {
                self.mc2_rival_walk_cast(ri, i, 5);
            }
        }

        // Altitude hard clamp to the behavior-row band (EF:5482-86).
        {
            let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
            let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
            let z = &mut self.g.ent[i].z;
            *z = (*z).clamp(
                ground.saturating_add(row.v_12),
                ground.saturating_add(row.v_10),
            );
        }

        // State handler, then the selector re-runs (every handler
        // tail-calls sub_12E70; Fresh runs it twice, EF:5255-56);
        // the water steer closes every state handler (EF:7879).
        let _ = think;
        self.mc2_rival_brain_tail(ri, i);
        // ⭐⭐⭐ THE MANIFESTATION COUNTDOWN RUNS **AFTER** THE WHOLE
        // WIZARD BODY, BRAIN INCLUDED. Retail's countdown lives in the
        // class-15 action body (`sub_693F0` / `GetScroll_69DB0`) at the
        // TOKEN's own pool slot, and a level-load book sits ABOVE its
        // wizard (584 → 585..611), so the ascending entity walk reaches
        // it only once the wizard is finished — exactly the reasoning
        // [`Self::mc2_rival_heal_stand_in`] already carried for spell 5
        // alone. This caster-slot stand-in ran FIRST, so the executor's
        // window gate (`sub_5F660`'s "refuse while `word_0x2E_46` is
        // live", [`Self::mc2_rival_cast`]) read a window ALREADY ONE
        // TICK SHORT and every zero-recast rival spell re-fired ONE
        // TICK EARLY. mc2l22 rival 584's earthquake token 602: retail's
        // recorded `word_0x2E_46` counts 21…1,0 and re-arms on the pair
        // that reads 0 (t=2514, 2535 — period 21); the port refused at
        // 2512 and cast at BOTH 2513 and 2514, minting one extra (9,2)
        // per cycle and shifting every later free-stack pop by one for
        // the rest of the take. The allocator itself was never wrong:
        // its reap timing, push order and pop order are bit-exact with
        // retail's at that pair.
        if !rival_buff_order_off() {
            self.mc2_rival_buffs(ri);
        }
        // The heal token's caster-slot stand-in runs LAST — retail's
        // token sits ABOVE the wizard (451 → 457) and its `sub_6A300`
        // body lands after the whole wizard body, i.e. after the
        // EF:5468-75 heal-when-hurt cast has re-armed it. See
        // [`Self::mc2_rival_heal_stand_in`].
        self.mc2_rival_heal_stand_in(ri);
        self.entities_dirty = true;
    }

    /// `sub_12910`'s continuation (EF:5252-5310): the state-handler
    /// switch, then `sub_12E70` — the half of the wizard body that is
    /// NOT inside `sub_12A70` and therefore survives its lethal early
    /// return. Factored out so the live path and the death tick run
    /// byte-identical code.
    fn mc2_rival_brain_tail(&mut self, ri: usize, i: usize) {
        // `sub_12E70` recomputes the decision cadence itself off
        // `byte_0x3E_62` (EF:5527) rather than inheriting `sub_12A70`'s
        // — the same expression on the same two unchanged operands.
        let think = self.g.ent[i].f63 % self.mc2_rivals[ri].think_period() == 0;
        let fresh = self.mc2_rivals[ri].state == Mc2AiState::Fresh;
        self.mc2_rival_state_tick(ri, i, think);
        self.mc2_rival_water_steer(ri, i);
        self.mc2_rival_selector(ri, i, think);
        if fresh {
            self.mc2_rival_selector(ri, i, think);
        }
    }

    /// `sub_15760(a1x, s)` (EF:7163) — **"is spell `s` LIVE on this
    /// wizard right now"**: `sub_146C0` (EF:6690) resolves
    /// `Entities[wizext->SpellsEnabled_0x333_819[s]]` and returns null
    /// unless it is above `Entities[0]`, then the window word
    /// `word_0x2E_46` (the port's `Ent::f26`) must be **strictly
    /// positive**. Two dereferences; no wizard-side flag anywhere.
    ///
    /// ⚠ The dead-wizard book marker 1 (EF:60147) is honoured
    /// VERBATIM, exactly as the import seat already honours it: retail
    /// indexes the pool with that 1 and reads slot 1's window.
    fn mc2_spell_window_live(&self, book_slot: u16) -> bool {
        let s = book_slot as usize;
        s != 0 && s < self.g.ent.len() && self.g.ent[s].f26 > 0
    }

    /// Hate regression toward neutral (EF:5377-93): below rises by
    /// agg+1, above decays by 256-agg — the war flag pins it.
    ///
    /// FROM-BINARY CORROBORATED (NETHERW.EXE `sub_12A70`, linear
    /// 0x12A70 = file 0x37270 by the banked LE recipe `0x34800 +
    /// (linear − 0x10000)`). remc2's transcription reads
    /// `array_0x1FC_508[4·i]` on the right-hand side while writing
    /// `[4·i+4]` — an eight-byte-shifted accumulator
    /// (`hate[p] = agg + 1 + hate[p−1]`) that would make the ladder
    /// leak across pairs. **It is a decompiler artifact.** The shipped
    /// loop reads and writes the SAME element:
    ///
    /// ```text
    ///   lea  esi,[ecx+eax]          ; esi = playerRec + 8·i
    ///   mov  cx,[esi+0x204]         ; READ hate[i]
    ///   cmp  cx,0x601f
    ///   jnc  .above                 ; unsigned >= neutral
    ///   mov  ax,[eax+0x242]         ; aggression (per-PLAYER, not indexed)
    ///   inc  eax
    ///   add  ecx,eax                ; hate[i] + agg + 1
    ///   mov  [esi+0x204],cx         ; WRITE hate[i]
    /// ```
    ///
    /// …then `cmp/jna` clamps down to 0x601F; the `.above` arm tests
    /// `word [esi+0x206]` (the war flag, the pair record's second
    /// word) and only on zero does `sub [esi+0x204],ax` with
    /// `ax = 0x100 − agg`, clamping back UP to 0x601F. Both
    /// comparisons are UNSIGNED and strict — exactly the per-player
    /// form below. No port change owed.
    fn mc2_rival_hate_decay(&mut self, ri: usize) {
        let (agg, war) = (self.mc2_rivals[ri].agg, self.mc2_rivals[ri].war);
        for (p, h) in self.mc2_rivals[ri].hate.iter_mut().enumerate() {
            if *h < HATE_NEUTRAL {
                *h = (*h + agg + 1).min(HATE_NEUTRAL);
            } else if *h > HATE_NEUTRAL && !war[p] {
                *h = h.saturating_sub(256 - agg).max(HATE_NEUTRAL);
            }
        }
    }

    /// The shared damage intake `sub_5EFA0` (EF:60613) on the rival's
    /// mailbox: steal channel, shield quarter paid by mana, killer
    /// latch. Hate feed rides here (APPROX: retail's per-projectile
    /// scan sub_159E0 — same inputs, slightly earlier; the MC1-column
    /// position).
    fn mc2_rival_intake(&mut self, ri: usize, i: usize) {
        // ⭐⭐⭐ THE DEAD-TARGET DROP IS A **WRITE HERE**, NOT A
        // PREDICATE IN THE GATE. `sub_5EFA0`'s second block
        // (EF:60634-39) is
        //
        //     if (a1x->word_0x96_150) {
        //         v2x = Entities[a1x->word_0x96_150];
        //         if (v2x > Entities[0]
        //             && (v2x->life_0x8 <= 0 || v2x->byte[1] & 4))
        //             a1x->word_0x96_150 = 0;
        //     }
        //
        // — the ONLY liveness/reap test retail applies to a wizard's
        // target. `sub_14C60`, which every state handler opens with,
        // is a pure signature compare and knows nothing about life
        // (see [`Self::mc2_target_alive`]). The port had folded this
        // write into that read, and the two are NOT the same law
        // because this one is **gated by the dock**: `sub_12A70`
        // reaches `sub_5EFA0` only down the `word_0x159_345 == 0`
        // arm (EF:5400-19), so a wizard sitting in its castle's grace
        // window keeps a dead target — and keeps ACTING on it.
        //
        // mc2l6-rsg pins both halves one tick apart. Rival 370's
        // disguise anchor, the (5,19) at slot 213, drops to life 0 at
        // t=4493 and −200 at t=4494:
        //   t=4494 grace 1 → 0, so NO `sub_5EFA0` — target stays 213,
        //          the signature still matches, and `sub_161A0` runs
        //          in full (two entity-LCG draws for the tier-0
        //          heading wiggle, tan2 re-aim, 3·minSpeed speed).
        //   t=4495 grace 0, `sub_5EFA0` runs and zeroes the word —
        //          `sub_14C60` now reads the `Entities[0]` sentinel,
        //          goes false, and the handler no-ops for good.
        // The port's predicate answered "dead" on BOTH ticks.
        //
        // The stale signature is deliberately left standing: retail
        // never touches `word_0x98_152` here, and it does not need to
        // — target 0 is already unmatchable.
        let t = self.mc2_rivals[ri].target;
        // ⭐⭐⭐ RETAIL DROPS A DEAD TARGET HERE AND THE HUMAN IS JUST
        // A POOL RECORD TO THIS BLOCK — shipped `NETHERW.EXE` file
        // 0x837b2-0x837e9 is
        //   mov  0x96(%ebx),%dx      ; word_0x96_150
        //   test %dx,%dx / je        ; no target
        //   mov  0x1a3e4(,%eax,4),%eax  ; Entities[target]
        //   cmp  %edx,%eax / jbe     ; <= Entities[0]
        //   cmpl $0x0,0x8(%eax)      ; life_0x8 <= 0
        //   testb $0x4,0xd(%eax)     ; byte[1] & 4 (reap)
        //   movw $0x0,0x96(%ebx)     ; word_0x96_150 = 0
        // reached through `Entities[word_0x96_150]` with NO class,
        // model or human test.
        //
        // ⚠⚠⚠ THIS WAS PARKED IN ROUND 141 AS A FIDELITY GAP THAT
        // "MEASURES EXACTLY ZERO ACROSS THE WHOLE 49-TAKE CORPUS", AND
        // THAT CLAIM WAS FALSE. Round 141 removed the port's
        // `player.state == Alive` predicate from `mc2_target_alive`
        // (correctly — `sub_14C60` IS a pure identity compare) and did
        // not land the WRITE that replaces it, on a measurement that
        // could not have been taken against the post-removal tree.
        // **mc2l22 was one of those 49 takes and it is the witness**:
        // it de-certified silently (8 seg / 0 dev / END -> 11 / 3 /
        // horizon 10,019) and stayed that way through round 142, which
        // three round-143 digs then hit independently. At t=10,020 the
        // human (slot 424) dies and rival 530 — undocked, holding
        // `PLAYER_TARGET` in `sub_13890` — fires ONE more lightning at
        // the corpse: 1 (10,23) + 82 (9,9) chain nodes retail never
        // mints. mc2l16 does not show it because ITS rival 389 is
        // DOCKED (`invuln` 1) across the only window that would, so
        // retail genuinely keeps facing that corpse there — the dock
        // is the discriminator, and it is why one take can look like
        // proof that the other refutes.
        // ⭐⭐⭐ A MEASURED ZERO IS ONLY EVIDENCE IF IT WAS MEASURED ON
        // THE TREE THE CHANGE PRODUCES.
        // `MGC_NO_MC2_INTAKE_DROPS_DEAD_HUMAN=1` restores the parked
        // shape (see the switch's doc comment).
        if t != 0 {
            let dead = if let Some(hx) = self.g.hx_of_tag(t) {
                !no_mc2_intake_drops_dead_human() && self.human_life(hx) <= 0
            } else {
                let e = &self.g.ent[t as usize];
                e.act_life <= 0 || e.flags & 0x400 != 0
            };
            if dead {
                self.mc2_rivals[ri].target = 0;
                self.g.ent[i].f146 = 0;
            }
        }
        // ch4 duel letter FIRST (EF:60643-57) — retail's own statement
        // order, and the block that turns a landed (10,26) marker into
        // the caster's lock one tick after the marker posted it.
        self.mc2_duel_stamp(i);
        // ch3 steal-mana (EF:60666 -> `sub_61050` EF:62076). The
        // channel carries the steal TIER INDEX (`dword_0x70_112`, the
        // (10,25) burst's `byte_0x46_70`), not an amount: the drain
        // re-reads `SPELLS[13].subspell[tier]` and, for
        // `life_0x1A == 0` (L1/L2), moves `subSpellIndex_2`
        // (2000/4000) FLAT — `caster.mana += amt; victim.mana -= amt`,
        // then victim clamped to [0, max], then caster (EF:62202-20).
        // The caster is credited the WHOLE amount whatever the victim
        // held: mc2l6-rsg t=8675, rival 378 at 1600 is drained by the
        // human's L2 burst (tier 1) — 378 reads 0 (+100 regen the same
        // tick), the human +4000 (1436526 → 1440526). Only the SOURCE
        // word is cleared (EF:62227); the tier stays as residue
        // (378's `mail3.amt` reads 1 from t=8675 on). Steal XP goes to
        // the caster (EF:62123, `sub_6D8B0`'s model-0 guard makes it
        // human-only). The L3 castle-percent arm (`life_0x1A` 1/2,
        // EF:62135-88) now runs too — see
        // [`World::mc2_steal_resolve`], which returns 0 for it
        // because retail reaches LABEL_23 with `v33 == 0`: the tier-3
        // steal takes 10% off the VICTIM'S CASTLE and scatters it as
        // (10,39) spheres round the CASTER'S castle, moving NOTHING
        // between the two purses. Witnessed once in the whole corpus:
        // mc2l6-rsg t=19303→19304 (castle 466 8580 → 7722, two
        // spheres of 500 + 358, both purses untouched).
        let (steal_tier, steal_src) = self.g.ent[i].mail[3];
        if steal_src != 0 {
            let caster_is_wizard = self.g.is_human_tag(steal_src)
                || ((steal_src as usize) < self.g.ent.len()
                    && self.g.ent[steal_src as usize].class64 == 3);
            if caster_is_wizard {
                let victim = self.mc2_rivals[ri].ent;
                let amt = self.mc2_steal_resolve(victim, steal_src, steal_tier.min(2) as u8);
                if amt != 0 {
                    self.credit_wizard_mana(steal_src, amt);
                    self.mc2_rivals[ri].mana =
                        (self.mc2_rivals[ri].mana as i64 - amt as i64).max(0) as i32;
                }
                self.mc2_rivals[ri].mana = self.mc2_rivals[ri]
                    .mana
                    .min(self.mc2_rivals[ri].mana_max as i32);
                if self.g.is_human_tag(steal_src) {
                    self.g.mc2_cast_xp.0.push((steal_src, 13, 1));
                }
            }
            self.g.ent[i].mail[3].1 = 0; // source only (EF:62227)
        }
        // ch0 damage.
        //
        // ⭐⭐⭐ THE GATE IS THE **SOURCE** WORD, NOT THE AMOUNT.
        // `sub_5EFA0`'s whole damage block opens `v8 =
        // str_0x5E_94.word_0x62_98; if (v8)` (EF:60673) and never looks
        // at `dword_0x5E_94` until it is inside — it cannot, because the
        // body immediately dereferences `Entities[v8]` for the knockback
        // bearing (`tan2(source → self)`, EF:60697-99). So an amount
        // parked with source 0 is not a pending hit at all: it is inert
        // bytes, and it stays parked forever.
        //
        // mc2l6-rsg: rival 370 carries `mail0` (300, src 0) from t=0 to
        // the end of the take. Retail never delivers it; the port's
        // `src == 0 && amt == 0` gate let the amount alone open the
        // block, spending 300 life and one entity LCG draw on the
        // FIRST tick of every pair cut from that recording — which is
        // why no mc2l6 fixture could be cut clean (SESSION 75 banked it
        // after law B fell back to a unit pin).
        // `a1x->word_0x26_38 = 0` — retail's FIRST statement, before
        // any of the guards (EF:60633). The attacker word is a
        // one-tick flag, re-published below only when a letter is
        // actually delivered.
        self.g.ent[i].f40 = 0;
        let src = self.g.ent[i].mail[0].1;
        if src == 0 {
            return;
        }
        self.g.ent[i].f40 = src; // word_0x26_38 = v8 (EF:60674)
        // ⭐⭐⭐ THE WIZARD'S READER CLEARS THE **SOURCE** AND NEVER THE
        // AMOUNT — and even the source only when the hit was NOT
        // lethal. `sub_5EFA0`'s ch0 block ends (EF:60714-25)
        //
        // ```text
        //   if (a1x->life_0x8 < 0) { a1x->word_0x24_36 = …; v1 = 2; }
        //   if (v1 != 2) { v1 = 1; sub_5EF70(a1x);
        //                  a1x->str_0x5E_94.word_0x62_98 = 0; }   // :60725
        // ```
        //
        // and there is NO `dword_0x5E_94 = 0` anywhere in it. The
        // wizard is the OUTLIER of MC2's three intakes: the castle
        // (`sub_609E0`, EF:61748-49) and the house
        // (`CompareEvent08_38B00`, EF:28270-71) each wipe amount AND
        // source, and this body had been written to THEIR shape. It
        // matters because MC2 point damage is written with the INVERSE
        // protocol — `sub_11900` (EF:4381-84) OVERWRITES a pending box
        // and ACCUMULATES onto a CONSUMED one, i.e.
        // [`Gen::mail_write_single`] — so leaving the amount standing
        // is exactly what makes melee on a wizard SNOWBALL. It is the
        // same law the human's inbox already carries
        // (`engine/world.rs`, ":55734 source only") and the same one
        // [`Gen::mc2_melee_write`] documents from mc2l3 t=2704; it had
        // simply never crossed to the rival column.
        //
        // mc2l22 slot 477 (rival "Rahn") is the witness — every letter
        // is one (5,2) grunt's `subSpellIndex_0x2A_42 = 200`
        // (EF:9780/9794/9808) and the box climbs 200 → 400 → 600 →
        // 800 → 1000 → 1200 → 1400 → 1600 → 1800 → 2000 at t=22 / 46 /
        // 48 / 51 / 54 / 57 / 58 / 69 / 71 / 72, with the life drop
        // matching the WHOLE running total every time (t=46: 9335 −
        // 400 + 18 regen = 8953; the port wiped the residue, took 200
        // and read 9153 — the take's first divergence). The tenth
        // letter kills him at t=72 (life −625), and THAT tick's box
        // keeps `src = 24` standing — the lethal branch skips the
        // clear — until the spawn memset wipes the block at t=73.
        //
        // ⚠ The `dmg <= 0` early return that used to sit here is an
        // invented guard (retail runs the whole block on a zero
        // letter: shield promotion, knockback, hit LCG draw, sound,
        // source clear) and with the amount preserved it would be
        // load-bearing in the wrong direction — returning WITHOUT
        // clearing the source jams a zero-amount letter in the box
        // forever.
        //
        // Shield: the two-stage absorb (EF:60676-95): an ARMED shield
        // NULLS the hit outright and promotes to CHARGED; a CHARGED
        // shield quarters the hit, pays the quarter from mana and is
        // spent. Retail writes the absorbed value BACK into
        // `dword_0x5E_94` (EF:60684 charged / EF:60692 armed), so the
        // shield shrinks the RESIDUE too and the next letter
        // accumulates onto the absorbed amount, not the raw one — the
        // human column's own note, ":55704 writes the QUARTERED value
        // back into +90". (Retail calls the shield-XP award here too —
        // a structural no-op for rivals through sub_6D8B0's model-0
        // guard.)
        //
        // ⭐⭐⭐ AND THE STAGE LIVES ON THE ENTITY, NOT IN A SIDE
        // STRUCT. Retail's gate is literally
        // `if (v9 & 0x40 || a1x->byte[2] & 0x40)` on the VICTIM's own
        // `struct_byte_0xc_12_15` (EF:60676) — our `flags`
        // [`F_SHIELD_CHARGED`] / [`F_SHIELD_ARMED`] — and the charged
        // arm ends `byte[1] = v11 & 0xBF` (EF:60688), the armed arm
        // `dword &= 0xFFBFBFFF; byte[1] = v13 | 0x40` (EF:60690-93).
        // The port kept the stage in `Mc2Rival::shield_state`, a lane
        // the pair importer cannot seed, so EVERY pair cut from a
        // recording ran the wizard's intake UNSHIELDED however the
        // recorded flags read — mc2l22 rival 611 holds `flags` 0xC00C
        // from t=3625 (a tier-0 shield re-arming 0x4000 every tick,
        // `sub_6A480` EF:56513) and quarters every 2000-point letter
        // to 500 (life −481 with the +19 regen, t=3637/3638/3642/…),
        // while the port spent the whole 2000 (−1981) for 82 life rows
        // across the take. It is the same hole the human column
        // closed with `player.shield = carpet.flags & 0x4000`
        // (conformance.rs) and the same word MC1's rival intake has
        // read since its own shield dig.
        if !rival_buff_bits_off() {
            let f = self.g.ent[i].flags;
            if f & (F_SHIELD_CHARGED | F_SHIELD_ARMED) != 0 {
                if f & F_SHIELD_CHARGED != 0 {
                    let q = (self.g.ent[i].mail[0].0.min(i32::MAX as u32) as i32) / 4;
                    let pay = if no_rival_mana_overdraft() {
                        q.max(0).min(self.mc2_rivals[ri].mana)
                    } else {
                        q.max(0)
                    };
                    self.mc2_rivals[ri].mana -= pay;
                    self.g.ent[i].f140 = self.mc2_rivals[ri].mana;
                    self.g.ent[i].mail[0].0 = q.max(0) as u32; // = v10 (EF:60684)
                    self.g.ent[i].flags &= !F_SHIELD_CHARGED; // byte[1] & 0xBF
                    self.mc2_rivals[ri].shield_state = 0;
                } else {
                    // `dword &= 0xFFBFBFFF` then `byte[1] |= 0x40`.
                    self.g.ent[i].flags &= !(F_SHIELD_CHARGED | F_SHIELD_ARMED);
                    self.g.ent[i].mail[0].0 = 0; // dword_0x5E_94 = 0 (EF:60692)
                    self.g.ent[i].flags |= F_SHIELD_CHARGED;
                    self.mc2_rivals[ri].shield_state = 2;
                }
            }
        } else if self.mc2_rivals[ri].shield {
            match self.mc2_rivals[ri].shield_state {
                1 => {
                    self.mc2_rivals[ri].shield_state = 2;
                    self.g.ent[i].mail[0].0 = 0; // dword_0x5E_94 = 0 (EF:60692)
                }
                2 => {
                    let q = (self.g.ent[i].mail[0].0.min(i32::MAX as u32) as i32) / 4;
                    let pay = if no_rival_mana_overdraft() {
                        q.max(0).min(self.mc2_rivals[ri].mana)
                    } else {
                        q.max(0)
                    };
                    self.mc2_rivals[ri].mana -= pay;
                    self.g.ent[i].mail[0].0 = q.max(0) as u32; // = v10 (EF:60684)
                    self.mc2_rivals[ri].shield_state = 0;
                }
                _ => {}
            }
        }
        let dmg = self.g.ent[i].mail[0].0.min(i32::MAX as u32) as i32;
        self.g.ent[i].act_life -= dmg;
        // ⭐⭐ THE KNOCKBACK REGISTER (EF:60697-702), and it is the MC1
        // rival column's own law (mc1/rivals.rs, SNAPSHOT 13) crossing
        // to MC2 for the first time. Retail stamps the bearing from
        // the SOURCE to the VICTIM and the magnitude from the RAW
        // letter — the post-shield `dword_0x5E_94`, which is the same
        // word `life_0x8 -=` just consumed:
        //
        // ```text
        //   yaw_0x1E_30    = tan2(&v15x->position, &a1x->position);
        //   fov_0x22_34    = radix_tan(...);            // unread here
        //   moveBoost_0x1E_30 = dword_0x5E_94 / 10;     // clamp 0..80
        // ```
        //
        // Nothing on the alive path reads it back (`sub_146F0` has no
        // moveBoost leg), so this write is pure deferred state until
        // the wizard's death fall runs `sub_5D530`.
        // ⚠ Retail's source is always a pool entity; the port stamps
        // human-fired mail with [`PLAYER_TARGET`], so that case reads
        // the pinned human pose — the MC1 twin's own note, verbatim.
        //
        // ⭐⭐⭐ AND THE SOURCE NEED NOT STILL BE ALIVE. The only gate
        // retail has is `if (v8)` on `word_0x62_98` itself (EF:61012);
        // `v15x = Entities_EA3E4[v14]` (EF:61037) is a bare table index
        // with NO `class_0x3F_63` test, and `position_0x4C_76` survives
        // a free (`sub_49FE0` clears the class byte alone), so the
        // bearing is taken from the dead projectile's LAST position.
        // The proof that retail dereferences a freed record here is
        // eight lines down in its own arm: the flood-killer suppression
        // reads `v15x->class_0x3F_63 == 10 && v15x->model_0x40_64 == 67`
        // off the same pointer. This arm's `class64 != 0` was an
        // INVENTED GUARD, and it is silent until the fatal hit comes
        // from a projectile that expires on the very tick it lands —
        // then the register keeps the PREVIOUS hit's bearing and the
        // whole death fall flies the wrong way.
        // Witness mc2l4 t=8541: rival 292 is killed by slot 247, a
        // (0,2) husk already freed that tick but still parked at
        // (31866, 60047). Retail stamps bearing 1314 from it; the port
        // held the stale 385 and the t=8542 death fall landed the
        // corpse — and with it the grave, the revived (10,1) and the
        // whole class-15 token scatter — a constant (+137, -81) off.
        let attacker = if let Some(hx) = self.g.hx_of_tag(src) {
            Some((self.human_pose(hx).0, self.human_pose(hx).1))
        } else {
            let s = src as usize;
            (s != 0 && s < self.g.ent.len())
                .then(|| (self.g.ent[s].x, self.g.ent[s].y))
                .filter(|_| !no_knock_from_dead_source() || self.g.ent[s].class64 != 0)
        };
        if let Some((ax, ay)) = attacker {
            let (vx, vy) = (self.g.ent[i].x, self.g.ent[i].y);
            let r = &mut self.mc2_rivals[ri];
            // ⚠ NO 11-BIT MASK — see [`no_mc2_knock_dir_raw`].
            let bearing = Gen::angle_between(ax, ay, vx, vy);
            r.knock_dir = if no_mc2_knock_dir_raw() {
                bearing & 0x7FF
            } else {
                bearing
            };
            r.knock_mag = (dmg / 10).clamp(0, 80) as i16;
        }
        // ⭐⭐ THE KILLER LATCH BELONGS TO THE LETHAL BRANCH ALONE —
        // MC1's own law (`Gen::mail_inbox`, :21365-66 and its three
        // siblings) one column over, and MC2 spells it out at
        // EF:60712-16: `if (life_0x8 < 0) { word_0x24_36 = mail0.src;
        // if the killer is a (10,67) → 0; }`. Latching it on EVERY hit
        // leaves a live wizard carrying a killer id retail zeroes,
        // which is what mc2l6-rsg's rival 370 held from its first
        // scratch to the end of the take.
        if self.g.ent[i].act_life < 0 {
            let hearse = self
                .g
                .ent
                .get(src as usize)
                .is_some_and(|k| k.class64 == 10 && k.model65 == 67);
            self.g.ent[i].f38 = if hearse { 0 } else { src };
        }
        // Wizard hit sound rand 54..57 on the entity LCG (EF:60712-13).
        let hs = 54 + (self.g.ent_rand(i) & 3) as u8;
        self.g.snd(hs, i);
        // The SURVIVE branch alone clears the source (EF:60721-25).
        // The lethal branch returns with the letter armed, and the
        // dead wizard's next pass bails at the `life_0x8 >= 0` gate
        // (EF:60641) before it could ever reach this line, so the
        // letter stands until the spawn-grace memset. mc2l22 t=72:
        // slot 477 dies at −625 with `mail0.src` still 24; at t=73 the
        // block reads (0, 0).
        if self.g.ent[i].act_life >= 0 {
            self.g.ent[i].mail[0].1 = 0;
        }
        // ⚠ NO HATE FEED HERE. The damage mailbox is not the ledger's
        // input — `sub_5EFA0`'s wizard-damage arm (EF:60676-716) never
        // touches `array_0x1FC_508`. MC2 feeds hate exactly where MC1
        // does, from the once-per-projectile chain sweep
        // [`Self::mc2_proj_hate_sweep`].
    }

    /// Ledger bump, clamped to the u16 the retail array holds
    /// (EF:7393-96 / :7414-17 — every arm clamps 0..0xFFFF).
    /// ⚠ NO WAR CHECK HERE: retail raises the flag ONLY in the
    /// CASTLE arm (EF:7402-03), exactly like MC1's sweep.
    pub(crate) fn mc2_rival_add_hate(&mut self, ri: usize, shooter: u8, amount: u16) {
        if shooter as usize >= 8 || self.mc2_rivals[ri].slot == shooter {
            return;
        }
        let r = &mut self.mc2_rivals[ri];
        r.hate[shooter as usize] = r.hate[shooter as usize].saturating_add(amount);
    }

    /// The castle-arm war latch (EF:7402-03): `hate > 50000 -
    /// shooterMaxMana/10 * X/255` with X =
    /// `v2x->dword_0xA4_164x->word_0x242_578` — and **v2x is the
    /// CASTLE**, not its owner. A castle's `dword_0xA4_164` is not a
    /// player block: every non-player record is bound to the shared
    /// dummy `unk_F42B0x` (Level.cpp:208/1244 — a BSS `type_str_164`
    /// at 0x2C52B0; dump-state `ptr_a4` reads 2904752 on castle 392,
    /// worm 390 and bolt 834 alike, 3516640 on wizard 383), whose
    /// +0x242 is 0. The wealth term therefore multiplies by ZERO and
    /// the threshold is the FLAT 50000 — the same degenerate form as
    /// MC1's `rival_war_check`. NETHERW.EXE @0x15B07-4A: `mov eax,
    /// [ecx+0xa4]; movsx eax,word [eax+0x242]` (ecx = v2x), `idiv 10`,
    /// `imul`, `idiv 255`, `mov edx,0xc350; sub edx,eax; cmp ebx,edx;
    /// jng` — signed, unclamped; with the port's wealth-scaled form
    /// a rich shooter drove the threshold to 0 and EVERY castle hit
    /// latched war.
    ///
    /// mc2l6-rsg t=6753 is the witness: the human's (9,4) bolt 834
    /// (maxMana 1,438,596 ⇒ the port's threshold 0) lands on rival
    /// 383's castle 392; hate[0] takes +5000 (40495 → 45495) and
    /// retail's war[0] STAYS 0 — the decay then runs (45495 → 45479
    /// the same tick, 16/tick to 40983 by t=7034) where the port
    /// latched war, pinned hate at 45495 and read `war[0]` in
    /// `sub_14030` for 256 ticks. (45495 < 50000 bounds the dummy's
    /// X at ≤ 7; the BSS zero is the only writer-free value.)
    fn mc2_rival_war_check(&mut self, ri: usize, shooter: u8) {
        if shooter as usize >= 8 || self.mc2_rivals[ri].slot == shooter {
            return;
        }
        let r = &mut self.mc2_rivals[ri];
        if r.hate[shooter as usize] as u32 > 50_000 {
            r.war[shooter as usize] = true;
        }
    }

    /// `sub_159E0` (EF:7319) — MC2'S PER-PROJECTILE HATE/WAR LEDGER,
    /// and the twin the port never wired. It is called once per tick
    /// from `UpdateEntities_57730` (EF:40112), between the tick-top
    /// chain rebuild and the entity walk, and it walks
    /// `dword_38531` — the CLASS-9 tick-top roster (`proj_chain`),
    /// not the pool.
    ///
    /// Each record is ledgered ONCE: `byte[1] |= 0x20` (our 0x2000) is
    /// stamped the first tick the bolt has BOTH a class-3 owner and a
    /// non-zero `word_0x96_150`, whether or not a table arm then
    /// applies (EF:7347). The bump is keyed on the PROJECTILE model,
    /// and — ⚠ unlike MC1's — the heavy set is {3,4,11} ONLY: model 16
    /// takes the light rate here because retail's ladder is
    /// `<0xA ? (3..4 ? heavy : light) : (>0xA ? (11 ? heavy : light) :
    /// nothing)` (EF:7376-88 / :7404-19), so model 10 alone scores
    /// zero. Castle victim (class 3 model 2) → 5000/1000 AND the war
    /// check; any other wizard → 3000/500 and no war check; a claimed
    /// mana sphere ((10,39) hit by a possess lob, model 1) → the
    /// claimant's ledger takes `ball_mana / 4` (EF:7358-68).
    ///
    /// ⭐ WHAT THE PORT HAD INSTEAD was a hate feed on the DAMAGE
    /// MAILBOX — flat +3000 per damaging hit, war-checked every time.
    /// Three ways wrong: it fires per damage TICK rather than once per
    /// projectile, it ignores the model ladder, and it latches war off
    /// the wizard arm that retail leaves alone. mc2l6-rsg is the
    /// witness on all three: the port had wiz 1/2/3 pinned at hate
    /// 27607 with `war` latched from t=387/464/558 where retail's
    /// three ledgers sit at the 24607 neutral with war CLEAR — and a
    /// latched war pins the decay (EF:5388), so the port could never
    /// come back down.
    ///
    /// Human-victim writes go to retail's human T160 tables, which
    /// nothing consumes (the human has no AI); the port keeps no such
    /// store, so those arms latch the mark and stop — MC1's ruling.
    pub(crate) fn mc2_proj_hate_sweep(&mut self) {
        for k in 0..self.g.proj_chain.visible_len() {
            let i = self.g.proj_chain.list[k] as usize;
            let e = &self.g.ent[i];
            if e.flags & 0x2000 != 0 {
                continue;
            }
            let (own, tgt, pmodel) = (e.id24, e.f146, e.model65);
            let owner_ok = self.g.is_human_tag(own)
                || (own != 0
                    && (own as usize) < self.g.ent.len()
                    && self.g.ent[own as usize].class64 == 3);
            if !owner_ok || tgt == 0 {
                continue;
            }
            self.g.ent[i].flags |= 0x2000; // ledgered (EF:7347)
            let Some(shooter) = self.owner_slot_of_source(own) else {
                continue;
            };
            if self.g.is_human_tag(tgt) || tgt as usize >= self.g.ent.len() {
                continue; // human tables unmodeled (see above)
            }
            let t = &self.g.ent[tgt as usize];
            let (tclass, tmodel) = (t.class64, t.model65);
            if tclass == 3 {
                let Some(victim) = self.owner_slot_of_source(tgt) else {
                    continue;
                };
                let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == victim) else {
                    continue;
                };
                if tmodel == 2 {
                    let bonus = match pmodel {
                        3 | 4 | 11 => 5000,
                        10 => 0,
                        _ => 1000,
                    };
                    self.mc2_rival_add_hate(ri, shooter, bonus);
                    self.mc2_rival_war_check(ri, shooter);
                } else {
                    let bonus = match pmodel {
                        3 | 4 | 11 => 3000,
                        10 => 0,
                        _ => 500,
                    };
                    self.mc2_rival_add_hate(ri, shooter, bonus);
                }
            } else if tclass == 10 && pmodel == 1 && tmodel == 39 {
                // The claimed-sphere arm: the claimant's OWN block
                // takes the bump (`v10x`, EF:7359-68), keyed by the
                // shooter's colour like every other arm.
                let claimant = t.f144;
                let mana = t.f140.max(0) as u32;
                if claimant == 0 {
                    continue;
                }
                let claimant_ok = self.g.is_human_tag(claimant)
                    || ((claimant as usize) < self.g.ent.len()
                        && self.g.ent[claimant as usize].class64 == 3);
                if !claimant_ok {
                    continue;
                }
                let Some(victim) = self.owner_slot_of_source(claimant) else {
                    continue;
                };
                let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == victim) else {
                    continue;
                };
                let bump = (mana / 4).min(u16::MAX as u32) as u16;
                self.mc2_rival_add_hate(ri, shooter, bump);
            }
        }
    }

    /// AI carpet movement `sub_146F0` (EF:6415): band-settle,
    /// always-level forward step, the strafe channel (decay 4/tick),
    /// accel 16/tick, Reflexes-scaled turn clamped to the row caps.
    /// No wall gate — the water steer is the only obstacle law.
    ///
    /// ⭐ ITS FIRST STATEMENT IS THE ONE-SHOT STOP VETO (EF:6441-44):
    /// `if (byte[1] & 8) { byte[1] &= 0xF7; } else { <the body> }` —
    /// the `sub_580E0` band, the step, the strafe spend, the ±16
    /// actSpeed servo AND the turn all sit inside the `else`. The
    /// flag is retail's `byte[1] |= 8`, the port's `F_STOP`; on a
    /// wizard it is armed by the whirlwind's victim pass
    /// (`sub_33340` EF:24329/24360, mc2/tail.rs) at the HEAD's walk
    /// slot, so a caught rival is parked on the eye for a tick and
    /// its own body must not undo that. The same veto already headed
    /// the corpse mover (`mc2_rival_carpet_move`, `sub_5D530`
    /// EF:59616-19), the creature core (`sub_1B8C0` EF:8786) and the
    /// ball (`TransformArcherToMana_35940` EF:26062) — this was the
    /// one mover on the port that read the flag on nobody.
    /// mc2l6-rival-spells-galore 10087→10088, rival 378 born inside
    /// the human's (10,22) head 159: mid-walk (`--at-slot 200`) the
    /// port's 378 is bit-exact with retail's final row (x 7331 y
    /// 26685 z 210 yaw 1774 speed -3, `b1_x8` set); by the end of
    /// the tick this body had stepped it -3 along 1774 (x 7333 y
    /// 26688), sunk the band (z 206), servoed the speed to 13 and
    /// turned it to 1760 — and left `b1_x8` armed forever, since
    /// nothing on the alive path consumed it.
    fn mc2_rival_movement(&mut self, ri: usize, i: usize) {
        if self.g.ent[i].flags & super::mobs::F_STOP != 0 {
            self.g.ent[i].flags &= !super::mobs::F_STOP;
            return;
        }
        let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
        let (v12, v14) = (row.v_12, row.v_14);
        let (v2, v4) = (row.v_2, row.v_4);
        let ground = self.g.ground_z(self.g.ent[i].x, self.g.ent[i].y) as i16;
        {
            let e = &mut self.g.ent[i];
            // sub_580E0 (EF:40372, via sub_146F0 EF:6454): the
            // TWO-branch altitude servo — the full v_14 step whenever
            // above the terrain itself, then the ground+v_12 floor
            // clamp. The a4 slot (v_10) is passed but DEAD in retail,
            // same as the balloon servo (castle.rs).
            if e.z > ground {
                e.z = e.z.saturating_add(v14);
            }
            if e.z <= ground.saturating_add(v12) {
                e.z = ground.saturating_add(v12);
            }
        }
        let (yaw, speed, strafe) = {
            let e = &self.g.ent[i];
            (e.f30, e.f126, self.mc2_rivals[ri].strafe)
        };
        let mut pos = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        Gen::polar_step(&mut pos, yaw, 0, speed);
        if strafe != 0 {
            Gen::polar_step(&mut pos, yaw.wrapping_add(0x200) & 0x7FF, 0, strafe);
            self.mc2_rivals[ri].strafe -= 4 * strafe.signum();
        }
        self.g.move_relink(i, pos.0, pos.1, pos.2);
        {
            let vdes = self.mc2_rivals[ri].vdes;
            let e = &mut self.g.ent[i];
            e.f126 += 16 * (vdes - e.f126).signum();
            // Turn toward the setpoint: err / (8 + (255-Reflexes)/16),
            // clamped to the row's [v_4, v_2] caps (EF:6488-6501).
            // Retail applies sign × cap with NO min(err) and then
            // SNAPS to the target when the numeric-ordered compare
            // says the step crossed it (EF:6502-10) — the same
            // seam-blind clamp the MC1 twin carries (sub_14EB0
            // :18835-57, mc1/rivals.rs). mc2l1 t=22 slot 138:
            // retail turns 0 → 1980 in ONE tick (2043 crossed 1980,
            // snap), where a min(err) step left 2043.
            let err = Gen::angdist(e.f30, e.f34) as i32;
            let div = 8 + ((255 - self.mc2_rivals[ri].refl as i32) / 16);
            let step = (err / div).clamp(v4 as i32, v2 as i32) as i16;
            let old = e.f30;
            let des = e.f34;
            let new = (old as i32 + (Gen::turn_sign(old, des) * step) as i32) as u16 & 0x7FF;
            e.f30 = new;
            if (old < des && new > des) || (old > des && des > new) {
                e.f30 = des;
            }
        }
    }

    // ---- the water / obstacle steer (sub_16580 EF:7879 +
    // ---- open-closure §1) -------------------------------------------

    /// Is the tile at pixel (x, y) + tile delta (dx, dy) deep water
    /// (`mapTerrainType == 8` — the ONLY obstacle type)?
    fn mc2_steer_water(&self, tx: i32, ty: i32) -> bool {
        self.g.t.tile_type[tile(tx as u8, ty as u8)] == 8
    }

    /// `sub_16730` (EF:7955) / `sub_16CA0` (EF:8245) — the
    /// four-neighbour probe at an arbitrary tile cursor. Returns the
    /// packed `(exit<<8 | mask)` word: mask bits 1=N, 2=E, 4=S, 8=W
    /// (N-else-S first; the fwd side of the handedness next). The
    /// diagonal escape keys on the remembered exit code.
    fn mc2_steer_probe(&self, tx: i32, ty: i32, right: bool, exit_mem: u8) -> u16 {
        let mut mask: u16 = 0;
        if self.mc2_steer_water(tx, ty - 1) {
            mask = 1;
        } else if self.mc2_steer_water(tx, ty + 1) {
            mask = 4;
        }
        // Forward side then back side, short-circuiting like retail.
        let (fwd, back) = if right { (1, -1) } else { (-1, 1) };
        if self.mc2_steer_water(tx + fwd, ty) {
            mask |= if fwd == 1 { 2 } else { 8 };
            return mask;
        }
        if self.mc2_steer_water(tx + back, ty) {
            mask |= if back == 1 { 2 } else { 8 };
            return mask;
        }
        if mask != 0 {
            return mask;
        }
        // Diagonal escapes, keyed on the FSM's remembered exit.
        let diag = |dx: i32, dy: i32| self.mc2_steer_water(tx + dx, ty + dy);
        if right {
            match exit_mem {
                1 | 9 if diag(-1, -1) => 1544, // (6<<8)|8
                2 | 3 if diag(1, -1) => 3073,  // (12<<8)|1
                4 | 6 if diag(1, 1) => 2306,   // (9<<8)|2
                8 | 0xC if diag(-1, 1) => 772, // (3<<8)|4
                _ => 0,
            }
        } else {
            match exit_mem {
                1 | 3 if diag(1, -1) => 770,   // (3<<8)|2
                2 | 6 if diag(1, 1) => 1540,   // (6<<8)|4
                4 if diag(-1, 1) => 3080,      // (12<<8)|8
                8 | 9 if diag(-1, -1) => 2305, // (9<<8)|1
                _ => 0,
            }
        }
    }

    /// `sub_16E70` (EF:8403) — Bresenham tile raycast: does the line
    /// from (x0,y0) to (x1,y1) cross water?
    fn mc2_steer_crosses_water(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let mut err = dx - dy;
        for _ in 0..512 {
            if self.mc2_steer_water(x, y) {
                return true;
            }
            if x == x1 && y == y1 {
                return false;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
        false
    }

    /// ⭐⭐⭐ THE WATER STEER'S TARGET IS THE **ENTITY** WORD, READ RAW —
    /// AND `Entities_EA3E4[0]` IS A LEGAL ANSWER.
    ///
    /// `sub_169C0`'s fifth statement is a bare, untested
    /// `v1x = Entities_EA3E4[a1x->word_0x96_150];` (EF:8147),
    /// dereferenced two lines later for the case-1/case-2 raycast
    /// `sub_16E70` (EF:8228/:8238) — and it runs BEFORE the `avoid`
    /// switch, so it is on every arm. Shipped `NETHERW.EXE`
    /// (`sub_169C0` at file 0x3B1C0):
    /// ```text
    ///   3b1e8  mov 0x96(%ebx),%ax          ; word_0x96_150, zero-extended
    ///   3b1f1  mov 0x1a3e4(,%eax,4),%eax   ; Entities_EA3E4[word]
    ///   3b200  movswl 0x4c(%eax),%ecx      ; target x   — NO test between
    ///   3b20a  movswl 0x4e(%eax),%eax      ; target y
    ///   3b214  mov 0xa4(%ebx),%eax         ; ...only NOW the avoid switch
    /// ```
    /// The port asked THREE questions retail does not: it read the
    /// BRAIN record's mirrored `mc2_rivals[ri].target` instead of the
    /// entity's own `f146`; it required `flags & 0x400 == 0`; and on
    /// either refusal it invented an aim point 8 tiles ahead along the
    /// current heading. ⭐ **SLOT 0 IS A LAW SURFACE — third instance
    /// in the campaign** (round 99's castle scratch, round 101's
    /// castle-less hated-ball anchor, and now this).
    ///
    /// WITNESS mc2l22 t=11343, the take's free-run wall. Rival 503,
    /// `avoid=1 / avoid_exit=8`, post-move tile (91,181): the probe
    /// `sub_16730` takes the case-8/9 diagonal escape and returns 2305
    /// = `(9<<8)|1`, so the high byte is non-zero and case 1 MUST ray.
    /// The intake's dead-target drop (`sub_5EFA0` EF:60634-39) had just
    /// zeroed 503's target word that same tick, so retail rays at
    /// SLOT 0, crosses water, keeps `result = 1`, and case 1 snaps
    /// `yaw = STEER_YAW_L[1] = 1536` with the EF:7936-42 brake
    /// (`speed 80 → 0`). The port's invented heading fallback aimed at
    /// (91,189), found no water, and froze with no yaw write at all.
    /// Horizon 11,342 → 11,724. `MGC_NO_STEER_TARGET_ENTITY_WORD=1`.
    fn mc2_steer_target_tile(&self, ri: usize, i: usize) -> (i32, i32) {
        if no_steer_target_entity_word() {
            let t = self.mc2_rivals[ri].target;
            if let Some(hx) = self.g.hx_of_tag(t) {
                return (
                    (self.human_pose(hx).0 >> 8) as i32,
                    (self.human_pose(hx).1 >> 8) as i32,
                );
            }
            if t != 0 && (t as usize) < self.g.ent.len() {
                let e = &self.g.ent[t as usize];
                if e.flags & 0x400 == 0 {
                    return ((e.x >> 8) as i32, (e.y >> 8) as i32);
                }
            }
            let e = &self.g.ent[i];
            let mut fwd = (e.x, e.y, e.z);
            Gen::polar_step(&mut fwd, e.f30, 0, 8 * 256);
            return ((fwd.0 >> 8) as i32, (fwd.1 >> 8) as i32);
        }
        // EF:8147, verbatim: `Entities_EA3E4[a1x->word_0x96_150]`.
        // The only translation is the port's out-of-pool human, whose
        // slot the importer stores as `PLAYER_TARGET`.
        let t = self.g.ent[i].f146;
        if let Some(hx) = self.g.hx_of_tag(t) {
            return (
                (self.human_pose(hx).0 >> 8) as i32,
                (self.human_pose(hx).1 >> 8) as i32,
            );
        }
        // An out-of-pool word is retail reading past `Entities_EA3E4`;
        // fall back to the same slot-0 record a ZERO word names rather
        // than fabricating a live slot.
        let e = self.g.ent.get(t as usize).unwrap_or(&self.g.ent[0]);
        ((e.x >> 8) as i32, (e.y >> 8) as i32)
    }

    /// `sub_169C0` (EF:8111) — the situation classifier: 0 clear,
    /// 1/2 commit left/right, 3 freeze. Fresh obstacles ray-march
    /// BOTH detours 40 steps and pick the exit nearer the target.
    fn mc2_steer_classify(&mut self, ri: usize, i: usize) -> u8 {
        let (wx, wy) = ((self.g.ent[i].x >> 8) as i32, (self.g.ent[i].y >> 8) as i32);
        let (tx, ty) = self.mc2_steer_target_tile(ri, i);
        let exit_mem = self.mc2_rivals[ri].avoid_exit;
        match self.mc2_rivals[ri].avoid {
            0 => {
                let mut left = self.mc2_steer_probe(wx, wy, false, exit_mem);
                let mut right = self.mc2_steer_probe(wx, wy, true, exit_mem);
                if left == 0 && right == 0 {
                    return 0;
                }
                // March both detours 40 steps (EF:8143-8166); index
                // the step tables by the code's LOW byte (0 = hold).
                // ⭐⭐⭐ THE CURSOR IS A BYTE PAIR (`baxis_2d v12`/`v11`)
                // AND EVERY STEP IS AN 8-BIT ADD — see
                // [`no_mc2_steer_tile_wrap`] for the shipped bytes.
                let wrap = !no_mc2_steer_tile_wrap();
                let step = |v: i32, d: i8| {
                    if wrap {
                        (v as u8).wrapping_add(d as u8) as i32
                    } else {
                        v + d as i32
                    }
                };
                let (mut lx, mut ly) = (wx, wy);
                let (mut lex, mut ley) = (wx, wy);
                if left != 0 {
                    for _ in 0..0x28 {
                        let idx = (left & 0xFF) as usize % 14;
                        lx = step(lx, STEER_DX_L[idx]);
                        ly = step(ly, STEER_DY_L[idx]);
                        (lex, ley) = (lx, ly);
                        left = self.mc2_steer_probe(lx, ly, false, (left & 0xFF) as u8);
                    }
                }
                let (mut rx, mut ry) = (wx, wy);
                let (mut rex, mut rey) = (wx, wy);
                if right != 0 {
                    for _ in 0..0x28 {
                        let idx = (right & 0xFF) as usize % 14;
                        rx = step(rx, STEER_DX_R[idx]);
                        ry = step(ry, STEER_DY_R[idx]);
                        (rex, rey) = (rx, ry);
                        right = self.mc2_steer_probe(rx, ry, true, (right & 0xFF) as u8);
                    }
                }
                let pick = if left != 0 && right != 0 {
                    // Rect-area proxy for "exit nearer the target"
                    // (EF:8168-73).
                    if (ty - ley).abs() * (tx - lex).abs() > (tx - rex).abs() * (ty - rey).abs() {
                        2
                    } else {
                        1
                    }
                } else if left == 0 {
                    2
                } else {
                    1
                };
                self.mc2_rivals[ri].avoid = pick;
                pick
            }
            1 => {
                let w = self.mc2_steer_probe(wx, wy, false, exit_mem);
                if w & 0xFF00 == 0 || self.mc2_steer_crosses_water(wx, wy, tx, ty) {
                    1
                } else {
                    self.mc2_rivals[ri].avoid = 3;
                    3
                }
            }
            2 => {
                let w = self.mc2_steer_probe(wx, wy, true, exit_mem);
                if w & 0xFF00 == 0 || self.mc2_steer_crosses_water(wx, wy, tx, ty) {
                    2
                } else {
                    self.mc2_rivals[ri].avoid = 3;
                    3
                }
            }
            s => s, // 3..8 handled by the caller
        }
    }

    /// `sub_16580` (EF:7879) — the post-state steer: classify, snap
    /// yaw to the escape table, zero speed on any turn, hold the arc
    /// ~5 ticks, re-detect.
    fn mc2_rival_water_steer(&mut self, ri: usize, i: usize) {
        let fsm = self.mc2_rivals[ri].avoid;
        // ⭐⭐⭐ ROUND 98 — **WHICH ARM THE CLASSIFY CALL LIVES IN IS
        // THE LAW.** `sub_16580`'s entry is TWO nested tests, not one.
        // The outer one (EF:7895) sends `avoid <= 2 || avoid >= 8`
        // into the same block — but the block opens with a SECOND
        // test the port had folded away (EF:7897-7900):
        //     if (avoid <= 7) v4 = sub_169C0(a1x); else v4 = 0;
        // so the RE-DETECT TICK CALLS NO CLASSIFIER AT ALL. `v4 == 0`
        // falls into `case 0`, which zeroes the counter and returns
        // (EF:7908-10) — the fresh probe happens on the NEXT tick,
        // when `avoid` is 0 and both tests pass.
        //
        // Byte-for-byte in the shipped `NETHERW.EXE` (file
        // 0x3adb9-0x3adca, linear 0x16589; `sub_169C0` is the call at
        // 0x3adcd = linear 0x169C0):
        //     3adb9  8b 83 a4 00 00 00     mov  0xa4(%ebx),%eax
        //     3adbf  80 b8 5e 04 00 00 07  cmpb $0x7,0x45e(%eax)
        //     3adc6  7e 04                 jle  0x3adcc   ; -> call
        //     3adc8  31 c0                 xor  %eax,%eax ; v4 = 0
        //     3adca  eb 0b                 jmp  <switch>
        //     3adcc  53 / e8 ee 03 00 00   push %ebx; call 0x3b1c0
        // and `case 0` at 0x3adee: `movb $0x0,0x45e(%eax)` + `xor
        // %eax,%eax` + return.
        //
        // The port pre-zeroed `avoid` and then classified in the same
        // tick, so the frozen arc re-detected ONE TICK EARLY — and on
        // that tick the fresh probe snapped `yaw` to the escape table
        // and braked the wizard to a stop.
        //
        // WITNESS mc2l22 2035 -> 2036: retail's own changelog reads
        // `player 1 (ent 451): avoid 8 -> 0` and `player 3 (ent 503):
        // avoid 8 -> 0` with NO steer — rival 503's `yaw` takes the
        // ordinary +12 servo step 1140 -> 1152 and `actSpeed` holds
        // 80. The port steered instead: `yaw` -> 512 (a
        // `x_WORD_D3FE8` escape rung) and `actSpeed` -> 0 (the
        // EF:7936-42 brake). That IS the take's frontier signature
        // `(3,1)slot503:heading,speed`.
        let class = if fsm >= 8 && !no_rival_steer_redetect_defer() {
            0
        } else if fsm <= 2 || fsm >= 8 {
            if fsm >= 8 {
                self.mc2_rivals[ri].avoid = 0;
            }
            self.mc2_steer_classify(ri, i)
        } else {
            3
        };
        let (wx, wy) = ((self.g.ent[i].x >> 8) as i32, (self.g.ent[i].y >> 8) as i32);
        let exit_mem = self.mc2_rivals[ri].avoid_exit;
        let new_yaw = match class {
            0 => {
                self.mc2_rivals[ri].avoid = 0;
                return;
            }
            1 => {
                let w = self.mc2_steer_probe(wx, wy, false, exit_mem);
                let lo = (w & 0xFF) as usize;
                if lo == 0 || lo >= STEER_YAW_L.len() {
                    return;
                }
                self.mc2_rivals[ri].avoid_exit = lo as u8;
                STEER_YAW_L[lo]
            }
            2 => {
                let w = self.mc2_steer_probe(wx, wy, true, exit_mem);
                let lo = (w & 0xFF) as usize;
                if lo == 0 || lo >= STEER_YAW_R.len() {
                    return;
                }
                self.mc2_rivals[ri].avoid_exit = lo as u8;
                STEER_YAW_R[lo]
            }
            _ => {
                // Frozen arc: coast, count toward re-detect.
                self.mc2_rivals[ri].avoid = self.mc2_rivals[ri].avoid.saturating_add(1);
                return;
            }
        };
        let e = &mut self.g.ent[i];
        if e.f30 != new_yaw {
            // A turn = full stop AND THE BRAKE WORD. `sub_16580`'s
            // exit arm (EF:7936-7942):
            //   if (v1 != a1x->yaw_0x1C_28) {
            //       a1x->actSpeed_0x82_130 = 0;
            //       a1x->dword_0xA4_164x->speed_0xc_12 = 0;
            //       a1x->dword_0xA4_164x->word_0xe_14 = 1;
            //   }
            // The brake is what the SPEED token reads at its own slot
            // later in the frame (`GetScroll_69DB0` EF:56216-19:
            // `word_0xe_14` → `word_0x2E_46 = 1`, body skipped, the
            // decrement lands on 0 and pays the `minSpeed·sign`
            // restore). Without it the port's token ran its full body
            // on a tick retail skips: mc2l22 t=1245→1246, rival 530
            // (`avoid_exit` 1 → 9 = this arm's `+1119 = v8` write):
            // retail's token 534 collapses 287 → 0 and `cmd_speed`
            // 160 → 80; the port kept the window at 286, wrote speed
            // 160 and minted the (10,2) slipstream puff at slot 808 —
            // the ONE extra pop that shifted the castle guards' three
            // (9,13) arrows and the human's (10,0) muzzle puff one
            // slot each (the t=1245 `extra (10,0) 731` head).
            e.f126 = 0;
            self.mc2_rivals[ri].vdes = 0;
            self.mc2_rivals[ri].v14 = BrakeWord(true);
        }
        e.f30 = new_yaw;
        e.f34 = new_yaw; // realign the steering setpoint
    }

    // ---- buffs + reactive defense -----------------------------------------

    /// The manifestations' armed windows (the rival's class-15
    /// entities are inert in the world tick — their cast windows count
    /// down here). Retail's class-15 action maintains `word_0x2E_46`
    /// as a live countdown on EVERY spell's manifestation — the
    /// readiness gates (EF:6997/7014/7065) rely on it expiring, so the
    /// homing set {1,9,0x10,0x12,0x13,0x15} re-arms after `f28` ticks
    /// like retail instead of locking for the rival's whole life. Buff
    /// flags read the post-decrement window; Heal (5) heals while
    /// armed.
    /// The duel enforcement's opponent DRAIN (`sub_5DE30`
    /// EF:59930-43): mode >= 1 drains mana by the opponent's regen
    /// rate plus 8 per tick; mode == 2 also drains life by the
    /// regen plus 2.
    ///
    /// ⭐⭐⭐ **BOTH TERMS ARE THE VICTIM'S *STORED* REGEN REGISTERS,
    /// READ RAW.** `v11 = mana_0x90_144 - (manaRegen_0x88_136 + 8)`
    /// (EF:59941) and `life_0x8 -= lifeRegen_0x163_355 + 2`
    /// (EF:59939) — signed, no floor, and the only clamp is mana at 0
    /// from below. The port put a `max(0)` on the mana term and
    /// recomputed the life term as the afield `maxLife/500`, and both
    /// were inventions.
    ///
    /// The mana floor is the one that bites, because
    /// `manaRegen_0x88_136` is ALSO the lane a cast debit rides:
    /// `sub_68DE0`'s first-tick arm parks `-maxMana_0x8C_140` there
    /// (the manifestation's cost) and the world tick spends it two
    /// statements before re-selecting the rate. So on a casting tick
    /// the drain term goes NEGATIVE and retail's subtraction becomes
    /// a CREDIT that cancels the debit exactly — a dueled rival's
    /// purse falls a flat 8 per tick whatever it is casting. Floor
    /// the term at 0 and the credit never lands, so the debit is paid
    /// twice over: mc2l6-rsg t=2165, rival 370 under the human's duel
    /// with a 100-mana manifestation opening — retail 588 → 580
    /// (drain −92, regen −100), the port 588 → 488.
    pub(crate) fn mc2_duel_drain(&mut self, opp: u16, mode: u8) {
        // ⭐⭐⭐ THE VICTIM CAN BE THE HUMAN, and until the rival duel
        // lock landed nothing could reach this arm: the only holder
        // of a lock was the carpet, and a wizard never duels itself.
        // Retail's drain is written against `v2x`, the OPPONENT
        // entity, with no column test anywhere in `sub_5DE30` —
        // `mana_0x90_144`/`manaRegen_0x88_136` off the entity and
        // `lifeRegen_0x163_355` off its player block, exactly as the
        // rival arm below reads them. See
        // [`no_mc2_rival_duel_death_tether`].
        if let Some(hx) = self.hx_of_tag(opp) {
            let d = self.players[hx].mana_delta as i64 + 8;
            let m = self.human_mana(hx) as i64 - d;
            self.set_human_mana(hx, m.max(0) as u32);
            if mode == 2 {
                self.set_human_life(hx, self.human_life(hx) - (self.players[hx].life_rate + 2));
            }
            return;
        }
        let Some(ri) = self.mc2_rivals.iter().position(|r| r.ent == opp) else {
            return;
        };
        let r = &mut self.mc2_rivals[ri];
        let d = r.mana_delta as i64 + 8;
        r.mana = (r.mana as i64 - d).max(0) as i32;
        // Retail's purse IS `mana_0x90_144` — publish, exactly as
        // `mc2_rival_leech_apply` already does at its own drain (see
        // [`crate::mc2::cast::no_mc2_wiz_purse_is_entity`]).
        if !crate::mc2::cast::no_mc2_wiz_purse_is_entity() {
            let m = self.mc2_rivals[ri].mana;
            let t = opp as usize;
            if t < self.g.ent.len() {
                self.g.ent[t].f140 = m;
            }
        }
        if mode == 2 {
            let life_delta = self.mc2_rivals[ri].life_delta;
            let a = opp as usize;
            self.g.ent[a].act_life -= life_delta + 2;
        }
    }

    /// A RIVAL's owned class-15 token, dispatched at the TOKEN's own
    /// pool slot (`mc2_manifestation_pass`) — retail's caster-generic
    /// body (`GetScroll_69DB0`, model-1 arm EF:56205-12). SPEED (3)
    /// is the landed arm, the MC2 twin of MC1's
    /// `rival_speed_token_tick`:
    /// - brake collapse: `word_0xe_14` set → window = 1 (EF:56216-19);
    /// - sustain (EF:56239-45): BOTH speed columns =
    ///   sign · minSpeed · (subSpell + first-tick);
    /// - the (10,2) slipstream puff every 4th tick on the TOKEN's
    ///   phase byte, pre-increment (EF:56246-50), at the caster's
    ///   settled pose, id = the caster, life ×4;
    /// - `sub_68DE0` (EF:56261), INSIDE the effect arm: the first tick
    ///   pins the debit (`-maxMana`, or stacked onto a negative
    ///   delta), every later live tick pins a positive regen to 0.
    ///   The debit lives HERE and not at the cast commit because the
    ///   brake collapse above it runs first: `sub_13710` calls
    ///   `sub_16580` (the avoid FSM) AFTER `sub_14C90` armed the
    ///   token, and an avoid turn raises `word_0xe_14` (EF:7936-42)
    ///   between the arm and this body — so a first tick that reads
    ///   the brake collapses to 1 → 0 and NEVER pays. mc2l22 t=10,
    ///   rival 451 raiding castle 502 from its own castle's 1000/tick
    ///   regen band: retail arms (`cooldown[3]` 31 → 32), turns, and
    ///   the token 455 dies unpaid (`d88` stays +100, mana 1100 →
    ///   1200 at t=11 → casts again); the port had pre-consumed the
    ///   −1000 at the cast site, read 100 at t=11, failed `sub_15170`'s
    ///   purse and braked instead — the `(3,1) slot 451 speed/mana`
    ///   free-run head at pair 10→11 (PAIR-CLEAN: the importer
    ///   restores the delta from `d88`).
    /// - shared countdown + the 1× signed restore at expiry
    ///   (EF:56263-68), queued-tier drain like the buffs pass.
    ///
    /// Other spells keep their carpet-slot machinery in
    /// [`Self::mc2_rival_buffs`].
    pub(crate) fn mc2_rival_manifestation_tick(&mut self, spell: usize, m: usize, ri: usize) {
        if spell != 3 || self.g.ent[m].f26 <= 0 {
            return;
        }
        let i = self.mc2_rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return;
        }
        let sign: i16 = if self.mc2_rivals[ri].vdes < 0 { -1 } else { 1 };
        let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28;
        // `if (!sub_68D50(a1x, v1x) || v1x->…word_0xe_14) { if (brake)
        // word_0x2E_46 = 1; } else { …the effect arm… }` (EF:56216-19).
        // The refused arm is EMPTY: no speed write, no puff, no debit
        // pin — only the trailing decrement (EF:56262) still runs. A
        // wizard killed EARLIER IN THE SAME WALK (its tokens sit above
        // it) reads `life < 0` here: mc2l6-rsg t=7062, rival 383 dies
        // at its own slot, token 387 holds the arm-tick 240 and pops
        // nothing, where the port wrote 160 and minted a (10,2).
        let afford = self.mc2_rival_afford(ri, m);
        if !afford || self.mc2_rivals[ri].v14.0 {
            if self.mc2_rivals[ri].v14.0 {
                self.g.ent[m].f26 = 1; // the brake collapse (EF:56218)
            }
        } else {
            let factor = self.g.ent[m].f30 as i16 + i16::from(first);
            let v12 = sign * self.g.ent[i].f128 * factor;
            self.mc2_rivals[ri].vdes = v12;
            self.g.ent[i].f126 = v12;
            if self.g.ent[m].f63 & 3 == 0 {
                let (cx, cy, cz, own_id) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z, e.id24)
                };
                if let Some(p) = self.g.mc2_spawn_speed_puff(cx, cy, cz) {
                    self.g.ent[p].act_life *= 4;
                    self.g.ent[p].id24 = own_id;
                }
            }
            // `sub_68DE0(a1x, v1x)` (EF:56261) — both halves, at the
            // token's own slot, after the puff, before the decrement.
            let cost = self.g.ent[m].max_life as i32;
            let r = &mut self.mc2_rivals[ri];
            if first {
                r.mana_delta = if r.mana_delta >= 0 {
                    -cost
                } else {
                    r.mana_delta - cost
                };
            } else if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        }
        self.g.ent[m].f26 -= 1;
        if self.g.ent[m].f26 == 0 {
            let base = sign * self.g.ent[i].f128;
            self.mc2_rivals[ri].vdes = base;
            self.g.ent[i].f126 = base;
            self.g.ent[m].flags &= !0x80;
            if self.g.ent[m].f44 != 0 {
                let queued = (self.g.ent[m].f44 - 1) as u8;
                self.g.ent[m].f44 = 0;
                let own = self.mc2_rivals[ri].ent;
                self.mc2_rival_set_spell(m, queued, own);
            }
        }
    }

    /// `sub_6A300` (EF:56430) on the RIVAL column — the same body the
    /// human's [`World::mc2_heal_token_tick`] runs, because retail's
    /// is caster-generic: it resolves its wizard through the token's
    /// `parentId_0x28_40` and touches only that record's `life_0x8` /
    /// `maxLife_0x4` / `mana_0x90_144` / `manaRegen_0x88_136`.
    ///
    /// What this replaces was flagged APPROX in place: a flat
    /// `maxLife/20` per armed tick (MC1's 5%), no `life < maxLife`
    /// admission, no purse test and NO COST AT ALL. Retail:
    /// - admits on `sub_68D50` AND `mana >= maxMana_0x8C_140`, and
    ///   re-tests that purse leg EVERY tick (not just the arm one) —
    ///   a rival that runs dry mid-window collapses it (`+46 = 1`);
    /// - heals `maxLife * subSpellIndex_0x2A_42 / 100` and pays the
    ///   FULL cost onto the regen delta, but ONLY on a tick that
    ///   actually heals — a rival at full life burns nothing and
    ///   keeps the window, resuming the moment it is hurt;
    /// - awards the caster XP on the first HEALING tick.
    ///
    /// ⚠ UNWITNESSED ON THIS COLUMN. No graded take reaches a rival
    /// heal — mc2l6's three rivals hold {0,1,2,3,4,7,9,13,12,14} and
    /// none holds 5; mc2l22 is the take that does (all seven rivals
    /// own it) and its horizon is 0. Landed on the decompile alone,
    /// as the same citation as the human arm, not as a reconstruction.
    #[cfg(test)]
    pub(crate) fn mc2_rival_heal_tick_for_test(&mut self, ri: usize, m: usize) {
        self.mc2_rival_heal_tick(ri, m);
    }

    /// `sub_6A030`'s expiry arm (EF:56399-56408): the linked pose-puppet
    /// is disabled (`DisableEntityDrawing04_57F10` — the tick-top reap
    /// takes it next frame), the token's `word_0x96_150` link is
    /// cleared, and THE CASTER'S CLOAK LIFTS.
    ///
    /// ⚠ STALE COMMENT, CORRECTED. This doc used to call the wizard's
    /// own byte[0] `0x20`/`0x01` bits "presentation lanes the port does
    /// not model". That is TRUE of `0x01` (the draw-hide, which retail
    /// blinks between puppet and wizard every tick for a non-human
    /// caster, EF:56352-84) and FALSE of `0x20`: bit 5 is the
    /// SCAN-INVISIBILITY bit every creature scanner filters on. Retail
    /// clears it here with `v2x->struct_byte_0xc.byte[0] &= 0xDFu`
    /// (EF:56403; shipped `NETHERW.EXE` 0x8EA50 `and cl,0xdf`) on
    /// `Entities[token->parentId_0x28_40]` — the CASTER, human or
    /// rival. See [`rival_metamorph_cloak_off`].
    fn mc2_rival_metamorph_expire(&mut self, m: usize, i: usize) {
        let c = self.g.ent[m].f146 as usize;
        if c != 0 && c < self.g.ent.len() && self.g.ent[c].class64 == 5 {
            self.g.ent[c].flags |= 0x400;
        }
        self.g.ent[m].f146 = 0;
        if !rival_metamorph_cloak_off() && i != 0 && i < self.g.ent.len() {
            self.g.ent[i].flags &= !0x20;
        }
    }

    fn mc2_rival_heal_tick(&mut self, ri: usize, m: usize) {
        let i = self.mc2_rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return;
        }
        let cost = self.g.ent[m].max_life;
        let admitted = self.mc2_rival_afford(ri, m) && self.mc2_rivals[ri].mana >= cost as i32;
        if !admitted {
            self.g.ent[m].f26 = 1; // :56466 — the release
            return;
        }
        let max = self.g.ent[i].max_life as i32;
        if self.g.ent[i].act_life >= max {
            return; // full life: no heal, no debit, window stays open
        }
        let step = (max as i64 * self.g.ent[m].f30 as i64 / 100) as i32;
        self.g.ent[i].act_life = (self.g.ent[i].act_life + step).min(max);
        let c = cost.min(i32::MAX as u32) as i32;
        let r = &mut self.mc2_rivals[ri];
        r.mana_delta = if r.mana_delta >= 0 {
            -c
        } else {
            r.mana_delta - c
        };
    }

    /// The HEAL token's caster-slot stand-in (the level-load book's
    /// token sits above its wizard, so `mc2_manifestation_pass` never
    /// dispatches it — `mc2_rival_buffs` is the stand-in for every
    /// other spell). It runs ONCE per frame and AFTER the brain, which
    /// is the two things `sub_6A300` at slot 457 does that the old
    /// placement did not:
    ///
    /// ⭐⭐⭐ **A RE-ARMED WINDOW IS STILL ONE TOKEN BODY.** The AI's
    /// heal recast cooldown is 1 (`AI_RECAST[5]`), so a hurt rival
    /// re-casts heal EVERY think tick: `sub_5F7B0` (EF:60976) stamps
    /// `word_0x2E_46 = word_0x30_48` (11) at the wizard's slot, and
    /// the token at its own higher slot sees 11 == 11, runs the
    /// first-tick body ONCE (sound 25, XP, `life += maxLife·sub/100`,
    /// `manaRegen = −cost`) and decrements to 10 — which is why
    /// mc2l22's `@0x2E` sits pinned at 10 across t=728-730 while life
    /// climbs 99/tick and `d88` reads −500 every tick. The port ran
    /// the mid-window body in `mc2_rival_buffs` on the imported 10
    /// (→ 9) and then the cast arm's same-frame first tick on the
    /// fresh 11 (→ 10): TWO heals, one `f2e`. mc2l22 pair 728→729,
    /// rival 451 (maxLife 9921, sub 1): retail 9639 −16 +19 +99 =
    /// 9741, the port 9840 — +99 on 1,246 pairs across six rivals.
    ///
    /// ⭐ **THE BRAIN READS PRE-HEAL LIFE.** `sub_12A70`'s
    /// `if (life_0x8 < maxLife_0x4)` (EF:5468) runs at the wizard's
    /// slot; the token's heal lands later in the frame. Running the
    /// stand-in before the brain let the port's brain see the healed
    /// value and skip the cast retail makes.
    pub(crate) fn mc2_rival_heal_stand_in(&mut self, ri: usize) {
        let own_slot = self.mc2_rivals[ri].ent as usize;
        let m = self.mc2_rivals[ri].book.ent[5] as usize;
        // A respawn-minted token sits BELOW the wizard and runs its
        // real body at its own slot (`mc2_rival_token_tick`).
        if m == 0 || m < own_slot || m >= self.g.ent.len() || self.g.ent[m].f26 <= 0 {
            return;
        }
        // Body before the shared decrement, as `sub_6A300` has it
        // (EF:56445-66 then :56468-71).
        self.mc2_rival_heal_tick(ri, m);
        self.g.ent[m].f26 -= 1;
        if self.g.ent[m].f26 == 0 && self.g.ent[m].f44 != 0 {
            // The queued-tier drain at expiry, as in `mc2_rival_buffs`.
            let queued = (self.g.ent[m].f44 - 1) as u8;
            self.g.ent[m].f44 = 0;
            let own = self.mc2_rivals[ri].ent;
            self.mc2_rival_set_spell(m, queued, own);
        }
    }

    /// The m26 WRAITH's mana leech on a wizard — `sub_28FF0`
    /// EF:19331-34, applied at the wraith's walk slot (see
    /// `Gen::m26_tick` and the consumer in `World`'s creature walk):
    ///
    /// ```text
    ///   v9 = v6x->mana_0x90_144 - (v6x->manaRegen_0x88_136 + 14);
    ///   v6x->mana_0x90_144 = v9;
    ///   if (v9 < 0) v6x->mana_0x90_144 = 0;
    /// ```
    ///
    /// ⭐⭐⭐ BOTH OPERANDS ARE THE VICTIM'S OWN RECORD, READ LIVE —
    /// the stored regen register (`mana_delta`: +100 afield, 1000 at
    /// home, NEGATIVE on a cast/heal tick, when the leech becomes a
    /// CREDIT exactly as the duel drain does), floor 0 and NO ceiling
    /// (the wizard's own step clamps at maxMana, EF:5456-59).
    /// mc2l22: rival 451 at t=1210-1221 under one wraith (slot 233,
    /// action 210, target 451) — retail 9500 → 9486 → 9472 (+100 regen
    /// − 114), the port +100 flat; rival 584 at t=2497 under three
    /// (233/236/265) — retail −242 = 100 − 3·114.
    pub(crate) fn mc2_rival_leech_apply(&mut self, victim: u16) {
        // ⭐⭐⭐ THE HUMAN IS BILLED BY THE SAME STATEMENT. Retail
        // has ONE drain (EF:19331-34) on whatever wizard record
        // `word_0x96_150` names; the human's carpet is a pool record
        // like any other. `player.mana_delta` is the port's home for
        // retail's `manaRegen_0x88_136` (`conformance.rs` seats it
        // from the carpet's `d88` through `mc2_applied_mana_delta`),
        // and `mc2_mana_tail` runs at the carpet's OWN pool slot, so
        // a wraith below the carpet reads the pre-step register and
        // one above reads the freshly selected rate — retail's phase.
        // Floor 0 and NO ceiling, exactly as the rival arm: on a
        // cast/heal tick the register is NEGATIVE and the leech is a
        // CREDIT (mc2l22 t=5655→5656, wraith 490 on carpet 424:
        // `d88` −100, retail 118312 + 1369 − (−100 + 14) = 119767).
        if let Some(hx) = self.hx_of_tag(victim) {
            let d = self.players[hx].mana_delta as i64 + 14;
            self.set_human_mana(
                hx,
                (self.human_mana(hx) as i64 - d).clamp(0, u32::MAX as i64) as u32,
            );
            return;
        }
        let Some(ri) = self.mc2_rivals.iter().position(|r| r.ent == victim) else {
            return;
        };
        let r = &mut self.mc2_rivals[ri];
        let d = r.mana_delta as i64 + 14;
        r.mana = (r.mana as i64 - d).clamp(0, i32::MAX as i64) as i32;
        // Retail's purse LIVES on the entity; keep the mirror the obs
        // lane and every other reader see in step, at this slot.
        let t = victim as usize;
        if t < self.g.ent.len() {
            self.g.ent[t].f140 = r.mana;
        }
    }

    /// `sub_68D50` (EF:55548-66) on the rival column, statement for
    /// statement: `mana_0x90_144 < 0` → false; `life_0x8 < 0` → false
    /// (a wizard killed EARLIER IN THE SAME WALK — its tokens sit
    /// above it — refuses every effect body from that frame on); the
    /// token's castle-upkeep word (`manaRegen_0x88_136`, our `f136`)
    /// covered by the rival's own castle store; and — on the ARM tick
    /// only — the purse against the full cost. The purse is a `u32`
    /// (retail's is signed but `sub_12A70` floors it at 0 every tick,
    /// EF:5453-54), so the `mana < 0` leg is unreachable here.
    /// `wizext->byte_0x1BF_447` — THE INVISIBILITY STRENGTH REGISTER,
    /// derived rather than stored. `sub_6B1C0`'s first-tick arm writes
    /// `SPELLS[11].subspell[token.byte_0x46_70].life_0x1A` and its
    /// expiry tail writes 0 (EF:57088/57110, `NETHERW.EXE` 0x8FA55 /
    /// 0x8FAA6), so the register IS the live window's tier strength.
    /// Its only reader is `sub_5F7E0`'s cloak gate. See
    /// [`rival_invis_cloak_edge_off`].
    fn mc2_rival_invis_strength(&self, ri: usize) -> u8 {
        let m = self.mc2_rivals[ri].book.ent[0xB] as usize;
        if m == 0 || m >= self.g.ent.len() || self.g.ent[m].f26 <= 0 {
            return 0;
        }
        let idx = self.g.ent[m].f71 as usize;
        self.g
            .assets
            .spells
            .get(0xB)
            .and_then(|r| r.tiers.get(idx).copied())
            .map_or(0, |t| t.life)
            .max(0) as u8
    }

    fn mc2_rival_afford(&self, ri: usize, m: usize) -> bool {
        let i = self.mc2_rivals[ri].ent as usize;
        if i != 0 && i < self.g.ent.len() && self.g.ent[i].act_life < 0 {
            return false; // `if (locEvent2->life_0x8 < 0) return false;` EF:55553
        }
        let e = &self.g.ent[m];
        if e.f136 > 0 {
            let ok = self
                .rival_castle(self.mc2_rivals[ri].ent)
                .is_some_and(|c| self.g.ent[c].f140 >= e.f136);
            if !ok {
                return false;
            }
        }
        if e.f26 as u16 == e.f28.max(1) {
            return self.mc2_rivals[ri].mana >= e.max_life as i32;
        }
        true
    }

    /// ⭐⭐⭐ THE BUFF WINDOW IS PUBLISHED ONTO THE CASTER'S FLAGS
    /// WORD, EVERY AFFORDED TICK — `sub_6A480` (SHIELD, EF:56496-529)
    /// and `sub_6AA00` (REBOUND, EF:56721-51) both resolve their
    /// caster through `Entities[parentId_0x28_40]` and stamp a bit on
    /// it; nothing about either is human-only. The readers are
    /// `sub_5EFA0`'s absorb (`byte[1] & 0x40 || byte[2] & 0x40`,
    /// EF:60676) and `sub_68740`'s deflect gate (`word[0] & 0x8010`,
    /// EF:62939) — both of which the port already had, keyed on
    /// `flags`, with nothing on the rival column ever writing them.
    ///
    /// The tier is retail's `SPELLS[model].subspell[byte_0x46_70]
    /// .life_0x1A` (the port's `Mc2SubSpell::life` at the token's
    /// `f71`), NOT the tier index: shield tier 0 re-stamps CHARGED on
    /// every tick, tier 1 stamps ARMED on the FIRST tick only, and
    /// rebound tier 0 stamps the scatter bit (tier 1 is the PRECISE
    /// `byte[0] |= 0x10`, which the port homes in
    /// `Gen::mc2_rebound_precise` on the human column only and does
    /// not yet model per-rival).
    ///
    /// ⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED: called from
    /// BOTH [`Self::mc2_rival_buffs`] (a book above its wizard) and
    /// [`Self::mc2_rival_token_tick`] (a respawn-minted book below
    /// it).
    pub(crate) fn mc2_rival_publish_buff(&mut self, ri: usize, s: usize, m: usize) {
        if rival_buff_bits_off() || !matches!(s, 6 | 8) {
            return;
        }
        let own = self.mc2_rivals[ri].ent as usize;
        if own == 0 || own >= self.g.ent.len() || m == 0 || m >= self.g.ent.len() {
            return;
        }
        // `sub_68D50(a1x, v2x)` guards BOTH publishes (EF:56508/56740).
        if !self.mc2_rival_afford(ri, m) {
            return;
        }
        let idx = self.g.ent[m].f71 as usize;
        let tier = self
            .g
            .assets
            .spells
            .get(s)
            .and_then(|r| r.tiers.get(idx).copied())
            .map_or(0, |t| t.life);
        let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28;
        if s == 6 {
            match tier {
                0 => self.g.ent[own].flags |= F_SHIELD_CHARGED,
                1 if first => self.g.ent[own].flags |= F_SHIELD_ARMED,
                _ => {}
            }
        } else if tier == 0 {
            self.g.ent[own].flags |= F_REBOUND;
        }
    }

    /// The window's own teardown: `sub_6A480`'s
    /// `parent.dword &= 0xFFBFBFFF` (EF:56534, both shield stages) and
    /// `sub_6AA00`'s `parent.word[0] &= 0x7FEF` (EF:56735, the rebound
    /// window plus the PRECISE bit).
    /// ⚠ Retail runs the rebound clear on the pass AFTER the window
    /// hits 0 (its `word_0x2E_46 <= 0` arm), and the port's token
    /// loops skip a spent window entirely, so it is collapsed onto the
    /// expiry edge here — one tick early on that bit alone.
    pub(crate) fn mc2_rival_clear_buff(&mut self, ri: usize, s: usize) {
        if rival_buff_bits_off() || !matches!(s, 6 | 8) {
            return;
        }
        let own = self.mc2_rivals[ri].ent as usize;
        if own == 0 || own >= self.g.ent.len() {
            return;
        }
        if s == 6 {
            self.g.ent[own].flags &= !(F_SHIELD_CHARGED | F_SHIELD_ARMED);
        } else {
            self.g.ent[own].flags &= !F_REBOUND;
        }
    }

    pub(crate) fn mc2_rival_buffs(&mut self, ri: usize) {
        // Does this world dispatch tokens at their own slots (a pooled
        // carpet)? A world property — asked of the local seat.
        let hx = self.local_seat();
        let book = self.mc2_rivals[ri].book.ent;
        let own = self.mc2_rivals[ri].ent;
        // The class-15 manifestation body is CASTER-GENERIC in
        // retail (GetScroll_69DB0 resolves its caster through
        // parentId and explicitly handles the rival wizard model,
        // EF:56205-12); the port's body pass is keyed on the HUMAN
        // book (`mc2_manifestation_pass`). The rival-visible arm
        // landed here is the MID-BURST REGEN PIN (sub_68DE0's else
        // arm, EF:56216-19): a live window past its first tick pins
        // a positive regen to 0 (mc2l1 t=195/238/248/258 — retail
        // HOLDS after the debit; the port regenned straight back).
        // SPEED (3) is EXCLUDED here: its whole body — countdown,
        // speed writes, the f63&3 puff, the brake collapse — runs
        // at the TOKEN's own walk slot (`mc2_rival_manifestation_tick`,
        // dispatched from `mc2_manifestation_pass`), which is what
        // gives the puff cadence its retail pre-increment phase; a
        // second countdown here would double-decrement (the carpet
        // slot sits below the token in every take measured).
        // ⚠ …and "below the token" is only true of a LEVEL-LOAD book.
        // A respawn re-mints onto the lowest free slots, so those
        // tokens sit BELOW the wizard and are walked BEFORE it — they
        // run their real body at their own slot
        // ([`Self::mc2_rival_token_tick`], dispatched from
        // `mc2_manifestation_pass`) and must not be counted twice here.
        let own_slot = self.mc2_rivals[ri].ent as usize;
        let mut mid_burst = false;
        for (s, m) in book.iter().enumerate() {
            if s == 3 {
                // ⚠ IT STILL HAS TO RUN IN NATIVE PLAY, for the same
                // reason the CASTLE arm below does. SPEED's body is
                // what PUBLISHES a wizard's commanded speed —
                // `GetScroll_69DB0` writes `dword_0xA4_164x->speed_0xc_12`
                // on the armed tick (EF:56245-52) and again at window
                // close (EF:56266-68, `minSpeed * sign` = base cruise) —
                // and it is CASTER-GENERIC: it walks
                // `Entities_EA3E4[a1x->parentId_0x28_40]` and its
                // `model_0x40_64 == 1` arm IS the rival arm. Nothing in
                // it is conditional on the caster being the pooled human.
                // The approach helper `sub_14C90` (EF:6729-46) writes no
                // speed of its own — its boost arm only CASTS spell 3 —
                // and the rival spawn `sub_5C950` (EF:43716) seeds
                // `speed_0xc_12 = 0`. So this body is the ONLY lift a
                // rival ever gets, and with `mc2_manifestation_pass`
                // dead in native play every native rival sat on its
                // spawn marker forever, casting correctly and never
                // moving. Measured: 1200 native ticks on mc2:22 moved
                // the seven rivals 0.0/2.1/0.0/0.0/0.0/0.0/0.0 tiles
                // without this, 157.6/157.8/193.8/89.4/187.0/70.0/40.3
                // with it.
                let m = *m as usize;
                if !mc2_rival_speed_standin_off()
                    && !self.mc2_token_slot_dispatch(hx)
                    && m != 0
                    && m < self.g.ent.len()
                    && self.g.ent[m].class64 == 15
                    && self.g.ent[m].model65 == 3
                {
                    self.mc2_rival_manifestation_tick(3, m, ri);
                }
                continue;
            }
            // CASTLE (2) is EXCLUDED for the same reason the human's
            // [`World::mc2_manifestation_tick`] returns before its
            // countdown: the castle manifestation's window is not a
            // timed cast but `sub_5F890`'s UPGRADE LOCK, pinned at
            // `word_0x30_48 - 1` and released by an explicit
            // `sub_5F890(*, 0)` at the CASTLE's pass — never counted
            // down. mc2l6-rsg slot 373 holds retail's 100 flat from
            // t=4310 to t>6350.
            if s == 2 {
                // CASTLE (2) is `sub_69AB0`'s own body, not a timed
                // cast — the window is `sub_5F890`'s UPGRADE LOCK,
                // pinned at `word_0x30_48 - 1` and released by an
                // explicit `sub_5F890(*, 0)` at the CASTLE's pass,
                // never counted down. mc2l6-rsg slot 373 holds
                // retail's 100 flat from t=4310 to t>6350.
                //
                // ⚠ IT STILL HAS TO RUN. Retail dispatches it at the
                // TOKEN's slot, which is
                // [`World::mc2_manifestation_pass`] — and that pass
                // is dead in NATIVE play (no pooled carpet). Without
                // this stand-in the cast arms a lock that nothing
                // ever mints against, so a native rival takes its
                // upgrade lock once and never builds again.
                let m = *m as usize;
                if !self.mc2_token_slot_dispatch(hx)
                    && m != 0
                    && m < self.g.ent.len()
                    && self.g.ent[m].class64 == 15
                    && self.g.ent[m].model65 == 2
                {
                    self.mc2_rival_castle_token_tick(m, ri);
                }
                continue;
            }
            let m = *m as usize;
            if m != 0 && m < own_slot {
                continue;
            }
            // HEAL (5) runs `sub_6A300`, the ONE effect state that never
            // calls `sub_68DE0` — no mid-burst pin, its own per-healing-
            // tick debit — and its stand-in runs AFTER the brain
            // ([`Self::mc2_rival_heal_stand_in`]): a window the brain
            // re-arms this frame gets ONE body, retail's.
            if s == 5 {
                continue;
            }
            if m != 0 && self.g.ent[m].f26 <= 0 && s == 8 && !no_rival_rebound_clear_defer() {
                // `sub_6AA00`'s TOP arm on an already-spent window —
                // the deferred clear (EF:56730-32). The twin of the
                // `else if s == 8` leg the death-collapse walk carries.
                self.mc2_rival_clear_buff(ri, s);
            }
            if m != 0 && self.g.ent[m].f26 > 0 {
                if self.g.ent[m].f26 as u16 != self.g.ent[m].f28
                    && spell_pins_regen(s)
                    // ⭐ SHIELD III pins on the POST-decrement counter
                    // (EF:56525-28), so the tick its window ENDS on
                    // pins nothing — the wizard's freshly recomputed
                    // regen stands. See
                    // [`no_rival_shield3_predecrement`].
                    && !(self.g.ent[m].f26 <= 1 && self.mc2_shield3_predecrement(s, m))
                    // ⭐⭐⭐ …AND THE PIN IS `sub_68DE0`, WHICH LIVES
                    // INSIDE THE `sub_68D50` ARM. A refused body takes
                    // the `word_0x2E_46 = 1` else and reaches NEITHER
                    // half of it — see [`rival_pin_afford_off`]. The
                    // human column has carried exactly this gate since
                    // its own mid-burst dig (`mc2/cast.rs`: "every
                    // retail handler calls sub_68DE0 only on the
                    // afford-success path"); the rival stand-in pinned
                    // unconditionally, so a rival the collapse arm had
                    // already refused lost one regen quantum anyway.
                    && (rival_pin_afford_off() || self.mc2_rival_afford(ri, m))
                {
                    mid_burst = true;
                }
                self.mc2_rival_publish_buff(ri, s, m);
                // ⭐⭐⭐ THE DEFERRED FIRE — `sub_693F0`'s first-tick
                // arm at the TOKEN's own slot (EF:55845-55882). See
                // [`rival_fire_at_token_slot_off`]: this stand-in runs
                // AFTER the whole wizard body, brain and steer
                // included, which is exactly where a level-load book's
                // token sits in the walk. `word_0x2E_46 ==
                // word_0x30_48` is retail's own first-tick test; the
                // arm in `mc2_rival_cast` writes `f28.max(1)`, so that
                // is the value to compare against. `sub_68D50` guards
                // it (`mc2_rival_afford`), the same gate the fused
                // caster-slot emit carried.
                if !rival_fire_at_token_slot_off()
                    && !rival_buff_order_off()
                    // ⭐⭐⭐ …AND THE FIRE ITSELF MOVES ON, to the
                    // TOKEN's own slot, whenever the class-15 walk
                    // arm is live — see
                    // [`rival_fire_at_own_token_slot_off`]. NATIVE
                    // play has no pooled carpet, so
                    // `mc2_token_slot_dispatch()` is false there and
                    // this stand-in stays the only body that fires.
                    && (rival_fire_at_own_token_slot_off()
                        || !self.mc2_token_slot_dispatch(hx))
                    && self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1)
                    && self.mc2_rival_afford(ri, m)
                {
                    let own_ent = self.mc2_rivals[ri].ent as usize;
                    if own_ent != 0 && own_ent < self.g.ent.len() {
                        let (yaw, pitch) = {
                            let e = &self.g.ent[own_ent];
                            (e.f30, e.f32)
                        };
                        self.mc2_rival_emit(ri, own_ent, s, yaw, pitch);
                    }
                }
                // ⭐⭐⭐ INVISIBILITY'S CLOAK EDGE — the raise on the
                // CASTER and the collapse that reads it back. See
                // [`rival_invis_cloak_edge_off`]; the retail arms sit
                // inside `sub_68D50`'s success branch.
                if s == 0xB && self.mc2_rival_afford(ri, m) {
                    let first_b = self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1);
                    mc2_invis_cloak_edge(&mut self.g, m, own_slot, first_b);
                    // ⭐ A LAW ON ONE CALL PATH IS NOT LANDED: the
                    // cloak edge's third write is the CASTER's spawn
                    // grace (`word_0x159_345 = 0`, NETHERW.EXE
                    // 0x8FA46), and the human's twin is
                    // `mc2_spell_fire`'s own `0xB` arm. See
                    // [`crate::mc2::cast::no_mc2_invis_clears_grace`].
                    if first_b && !crate::mc2::cast::no_mc2_invis_clears_grace() {
                        self.mc2_rivals[ri].grace = 0;
                    }
                }
                self.g.ent[m].f26 -= 1;
                if self.g.ent[m].f26 == 0 {
                    // Shield window expiry drops the absorb stages.
                    // The REBOUND's does NOT — `sub_6AA00`'s expiry
                    // tail is `sub_6D880` alone (EF:56760-61) and its
                    // clear lives in the `<= 0` arm one pass later.
                    // ⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED:
                    // this caster-slot stand-in runs a book that sits
                    // ABOVE its wizard, `Self::mc2_rival_token_tick`
                    // one that sits below, and the death-collapse walk
                    // above already carries the split.
                    if s != 8 || no_rival_rebound_clear_defer() {
                        self.mc2_rival_clear_buff(ri, s);
                    }
                    if s == 6 {
                        self.mc2_rivals[ri].shield_state = 0;
                    }
                    // Metamorph expiry disables the pose-puppet
                    // (`sub_6A030` EF:56399-56408).
                    if s == 4 {
                        self.mc2_rival_metamorph_expire(m, own as usize);
                    }
                    // `sub_6B1C0`'s expiry (EF:57108-09):
                    // `v1x->byte[0] &= 0xDFu` on the CASTER, plus
                    // `wizext->byte_0x1BF_447 = 0`. Invisibility never
                    // RAISES that bit (see the note in
                    // [`Self::mc2_rival_buffs`]) — this clear is
                    // unconditional, so an Invisibility window that
                    // ends inside a metamorph window drops the
                    // metamorph cloak with it.
                    if s == 0xB && !rival_metamorph_cloak_off() {
                        let w = own as usize;
                        if w != 0 && w < self.g.ent.len() {
                            self.g.ent[w].flags &= !0x20;
                        }
                    }
                    // A tier queued mid-effect (`SetSpell`'s f44 =
                    // t+1 stash) applies at window expiry — the
                    // retail word_0x2C_44 drain (Level:1505-18; the
                    // defense state's disguise re-pick relies on it).
                    if self.g.ent[m].f44 != 0 {
                        let queued = (self.g.ent[m].f44 - 1) as u8;
                        self.g.ent[m].f44 = 0;
                        self.mc2_rival_set_spell(m, queued, own);
                    }
                }
            }
        }
        if mid_burst {
            let r = &mut self.mc2_rivals[ri];
            if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        }
        let live = |g: &Gen, spell: usize| -> bool {
            let m = book[spell] as usize;
            m != 0 && g.ent[m].f26 > 0
        };
        let shield = live(&self.g, 6);
        let rebound = live(&self.g, 8);
        let invisible = live(&self.g, 0xB);

        // (3 = the approach boost window, read by the movers.)
        // (5 = heal, now `mc2_rival_heal_tick` in the countdown loop
        // above — retail's own body, run before its own decrement.)
        {
            let r = &mut self.mc2_rivals[ri];
            r.shield = shield;
            r.invisible = invisible;
            r.rebound = rebound;
        }
        // ⭐⭐⭐ THE SCAN-INVISIBILITY BIT IS AN EDGE, AND INVISIBILITY
        // IS NOT ONE OF ITS WRITERS. Grepping every
        // `byte[0] |= 0x20` in the decompile finds EXACTLY THREE
        // raises on a wizard record — EF:56336 (`|= 0x21`, the
        // metamorph mint), EF:60177 and EF:60195 (the death
        // touchdown, already landed at the corpse-fall below). The
        // Invisibility handler `sub_6B1C0` raises 0x20 on the TOKEN
        // (`a1x`, EF:57090) as its own already-applied latch, sets
        // `wizext->byte_0x1BF_447` (the strength), and touches the
        // CASTER's byte[0] only to CLEAR it at expiry (EF:57108).
        // Mirroring the Invisibility window onto the wizard's own
        // 0x20 was therefore an INVENTED WRITE that hid a rival from
        // every creature scan retail lets through.
        //
        // The rest of the lifecycle is edges too:
        //   * `sub_6A030` mint raises (see `mc2_rival_emit` s == 4),
        //     but only inside the puppet-allocation success arm;
        //   * `sub_6A030` expiry clears (`mc2_rival_metamorph_expire`);
        //   * `sub_6B1C0` expiry clears (below);
        //   * ⭐ EVERY CAST CLEARS IT. `sub_5F660`'s arm calls
        //     `sub_5F7B0` (EF:60974-80) which tail-calls `sub_5F7E0`
        //     (EF:60983-90): `if (strength < 2 || (strength <= 2 &&
        //     token.model != 1)) caster->byte[0] &= 0xDF`. So a
        //     metamorph cloak lasts only until the rival's next cast —
        //     which is why mc2l22 t=4729 shows rival 530 mid-window,
        //     puppet 903 linked, and byte[0] & 0x20 CLEAR.
        let i = self.mc2_rivals[ri].ent as usize;
        if rival_metamorph_cloak_off() && self.g.ent[i].tick70 == 1 {
            if invisible {
                self.g.ent[i].flags |= 0x20;
            } else {
                self.g.ent[i].flags &= !0x20;
            }
        }
    }

    /// The reactive anti-projectile defense (`sub_15CB0` +
    /// `sub_15D20` + `sub_15D40`, open-closure §3): nearest class-9
    /// entity homing on me within 5120² -> strafe 80 (only when not
    /// mid water-steer); within 1024² -> cast by threat model
    /// (0|3 -> rebound 8 else shield 6; 4 -> shield 6).
    fn mc2_rival_react_defense(&mut self, ri: usize, i: usize) {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        // ⭐ THE SCAN WALKS THE TICK-TOP CLASS-9 ROSTER, NOT THE POOL —
        // `sub_15CB0` iterates `x_D41A0_BYTEARRAY_4_struct.dword_38531`
        // through `next_0` (EF:7447-53), and that head is rebuilt once
        // per tick by `UpdateEntities_57730`'s case-9 arm (EF:40011-17),
        // *above* the entity walk that dispatches the wizards
        // (EF:40116-80) and *below* the same function's opening reap
        // pass (EF:39948-55, every `byte[1] & 4` ghost freed). So the
        // membership sample is the class-9 population as of the tick
        // head, with NO life and NO flags test of its own — the MC2
        // face of the law `rival_defense` already runs on the MC1
        // column ([`crate::engine::features::TickChain`]).
        //
        // mc2l6-rsg t=843 is the witness. The human's fire stream is
        // recycling one slot every tick: slot 646's spent (9,9) is a
        // `byte[1] & 4` ghost at the t=842 boundary, the tick-843 reap
        // frees it, the ctor pops that very slot back, and the new bolt
        // — homing on rival 370 at 453 units — is live in the pool by
        // the time the walk reaches slot 370. Retail's roster was
        // sampled while that slot was FREE, so `sub_15CB0` returns 0
        // and the rival never strafes; the port's pool scan found the
        // newborn, took `strafe = 80` and pushed 370 an eighth of a
        // tile off on both axes at t=844. Retail does strafe wiz 1 —
        // at t=892, the next cadence tick, on slot 619's (9,13), which
        // WAS alive at the tick head.
        //
        // The election is UNGATED and its key UNSIGNED: retail seeds
        // `v2 = -1` and tests `>= 0x1900000` on the WINNER only.
        let mut best: Option<(usize, u32)> = None;
        for k in 0..self.g.proj_chain.visible_len() {
            let j = self.g.proj_chain.list[k] as usize;
            let e = &self.g.ent[j];
            if e.f146 != me {
                continue;
            }
            let d2 = Gen::dist2_sq(px, py, e.x, e.y) as u32;
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((j, d2));
            }
        }
        let best = best.filter(|&(_, d)| d < 0x190_0000);
        if std::env::var_os("MGC_JINK_TRACE").is_some() {
            let cand: Vec<(u16, u16, u16, u16, u32)> = (0..self.g.proj_chain.visible_len())
                .map(|k| {
                    let j = self.g.proj_chain.list[k] as usize;
                    let e = &self.g.ent[j];
                    (
                        j as u16,
                        e.f146,
                        e.x,
                        e.y,
                        Gen::dist2_sq(px, py, e.x, e.y) as u32,
                    )
                })
                .collect();
            eprintln!(
                "[jink/mc2] t={} ri={ri} me={me} best={best:?} cand={cand:?} me_pos=({px},{py})",
                crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        let Some((threat, d2)) = best else { return };
        if self.mc2_rivals[ri].avoid == 0 {
            self.mc2_rivals[ri].strafe = 80;
        }
        if d2 < 0x10_0000 {
            let model = self.g.ent[threat].model65;
            match model {
                0 | 3 => {
                    // Rebound tier-walk; shield falls back ONLY when
                    // no rebound tier PROBED castable (the a1-capture
                    // law, EF:7518-43) — a probe that passed but a
                    // cast that then whiffed does NOT re-open the
                    // shield fallback.
                    let mut probed8 = false;
                    let mut tier = self.mc2_rivals[ri].book.levels[8] as i16;
                    while tier >= 0 {
                        if self.mc2_rival_tier_probe(ri, tier, 8) == 8 {
                            probed8 = true;
                            self.mc2_rival_cast(ri, i, 8);
                            break;
                        }
                        tier -= 1;
                    }
                    if !probed8 {
                        self.mc2_rival_walk_cast(ri, i, 6);
                    }
                }
                4 => {
                    self.mc2_rival_walk_cast(ri, i, 6);
                }
                _ => {}
            }
        }
    }

    // ---- the decision selector cascade (sub_12E70 EF:5495) ----------------

    fn mc2_rival_selector(&mut self, ri: usize, i: usize, think: bool) {
        // 1. Need a castle (sub_13B00 EF:6056) — every tick.
        let castle = self.rival_castle(self.mc2_rivals[ri].ent);
        // ⭐⭐⭐ THE BUILDER GATE IS THE MANIFESTATION REGISTER, NOT
        // THE KNOWN FLAG. `sub_13B00`'s entry (EF:6071) is
        // `!a1x->dword_0xA4_164x->CastleEntityIndex_0x3A_58
        //   && sub_146C0(a1x, 2u) && sub_15730(a1x, 2u)`.
        // Disassembly, witness and the third gate:
        // [`rival_cascade_token_gate_off`].
        let builder = if rival_cascade_token_gate_off() {
            self.mc2_rivals[ri].known[2]
        } else {
            self.mc2_rivals[ri].book.ent[2] != 0
        };
        if castle.is_none()
            && builder
            && self.mc2_rival_afford_castle(ri)
            && self.mc2_rival_scout_site(ri, i)
        {
            self.mc2_rivals[ri].state = Mc2AiState::Build;
            return;
        }
        // 2. Flee home hurt (sub_13DC0 EF:6163) — every tick. The
        // steer target = the OWN castle (EF:6174-75 — the water
        // detour scanner walks toward it).
        if let Some(c) = castle {
            if self.g.ent[i].act_life < (self.g.ent[i].max_life / 2) as i32 {
                self.mc2_set_rival_state(ri, Mc2AiState::Home, c as u16);
                return;
            }
        }
        if !think {
            return;
        }
        // 3. Upgrade the castle (sub_13C50 EF:6107 → sub_155E0
        // EF:7106-30). Retail's gate ORDER is load-bearing: the
        // sub_11A10 space probe — which QUAD-STAMPS the castle
        // (yaw 0/fov 256, the tick's last write on those lanes) —
        // runs BEFORE the affordability test (EF:7115 then :7119),
        // so a broke rival still stamps its castle on every think
        // tick (mc2l4 t=59/113/315 slot 297/304: retail 0 vs the
        // port's ctor -8192 whenever afford short-circuited the
        // probe; t=167/719 agree exactly when afford passes). The
        // manifestation-exists gate (sub_146C0(a1x,2)) precedes the
        // probe; the port's f26<7 guard stays but AFTER the stamp
        // (retail has none — at rung 7 the 300M cost fails afford
        // anyway, post-stamp). The conjunction's VALUE is unchanged
        // apart from the known[2] gate; only the stamp side effect
        // moves.
        if let Some(c) = castle {
            if self.mc2_rivals[ri].cooldown[2] == 0
                && self.g.ent[c].tick70 == 4
                && self.g.ent[c].f50 == 0
                && self.mc2_rivals[ri].book.ent[2] != 0
                && self.g.mc2_castle_space_ok(c)
                && self.g.ent[c].f26 < 7
                && self.mc2_rival_afford_castle(ri)
                && {
                    // ⭐⭐⭐ THE LAST GATE OF sub_155E0 IS THE PERCEPTION
                    // CONE (EF:7121-25): `sub_582B0(yaw_0x1C, roll_0x20)
                    // < ((((255 - per) >> 2) + 20) << 11) / 360` — the
                    // wizard must already be FACING its setpoint before
                    // the selector takes state 1. The port carried the
                    // cone only in the cast kernel (`mc2_rival_cast_ready`,
                    // sub_15170), so a rival that could not aim still
                    // re-entered Upgrade on every think tick and never
                    // fell through to arms 4-5. mc2l6-rsg t=7034: rival
                    // 383 (Perception 255 ⇒ cone 113) sits at yaw 1334 /
                    // roll 891 (443 off) with castle 749 at rung 1 and
                    // 11690 ≥ 10000 affordable; retail stamps the probe
                    // quad (749 ayaw −8192 → 0), fails HERE, and takes
                    // state 8 on the human (`sub_14030`), casting SPEED
                    // at 7035 (cmd_speed 240, cooldown[3] 32); the port
                    // stayed in state 1 hovering the keep.
                    let cone = ((255 - self.mc2_rivals[ri].per as u32) / 4 + 20) * 2048 / 360;
                    let e = &self.g.ent[i];
                    (Gen::angdist(e.f30, e.f34) as u32) < cone
                }
            {
                // Steer target = the own castle (EF:6114-15).
                self.mc2_set_rival_state(ri, Mc2AiState::Upgrade, c as u16);
                return;
            }
        }
        // 4. Raid an enemy castle (sub_13E40 EF:6182).
        if self.mc2_rival_owns_any(ri, &OFFENSE_RAID) && self.mc2_rival_pick_castle(ri, i) {
            return;
        }
        // 5. Attack an enemy wizard (sub_14030 EF:6233).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_wizard(ri, i) {
            return;
        }
        // 6. Intercept a fat enemy balloon (sub_14250 EF:6292).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_balloon(ri, i) {
            return;
        }
        // 7. Reactive defense (sub_15FC0 EF:7616) — the MC2-native
        // cascade placement (brain §1.3): a live enemy wizard close
        // by flips to the dodge state.
        if self.mc2_rival_pick_defense(ri, i) {
            return;
        }
        // 8. Claim mana balls (sub_13CE0 EF:6122): needs possess-1;
        // with the castle spell known, only while the ceiling is at
        // or under the castle spell's CURRENT ladder cost — the
        // economy loop re-opens after every upgrade.
        // ⭐⭐⭐ THE SAME `sub_146C0` SUBSTITUTION, SECOND CALL SITE —
        // `sub_13CE0` (EF:6129-45) gates on the POSSESS token and the
        // CASTLE token, and a wizard whose castle token has been
        // purged takes the unconditional `else`. See
        // [`rival_cascade_token_gate_off`]. The `maxMana <=
        // v2x->maxMana` term now reads the token's STAMPED price
        // ([`rival_castle_token_price_off`]) — round 104; it is only
        // reachable when the token EXISTS.
        if self.mc2_rival_ball_arm_open(ri) && self.mc2_rival_pick_ball(ri, i) {
            return;
        }
        // 9. Hunt any mana holder (sub_14530 EF:6341).
        if self.mc2_rival_owns_any(ri, &OFFENSE_ATTACK) && self.mc2_rival_pick_mana(ri, i) {
            return;
        }
        // 10. Idle (sub_14630 EF:6383): hurt + castle → home (steer
        // target = the castle, EF:6394-96); else cruise.
        if let (Some(c), true) = (
            castle,
            self.g.ent[i].act_life < self.g.ent[i].max_life as i32,
        ) {
            self.mc2_set_rival_state(ri, Mc2AiState::Home, c as u16);
        } else {
            self.mc2_rivals[ri].state = Mc2AiState::Cruise;
        }
    }

    /// `sub_13CE0`'s GATE (EF:6129-45, the ceiling at :6135), minus
    /// the pick — cascade arm 8:
    /// `if (sub_146C0(a1x, 1u)) { v2x = sub_146C0(a1x, 2u);
    /// if (v2x) { if (a1x->maxMana_0x8C_140 <= v2x->maxMana_0x8C_140)
    /// … } else … }`. Both tokens come off the manifestation register
    /// ([`rival_cascade_token_gate_off`]) and the ceiling is measured
    /// against the castle token's **STAMPED** price, not a recomputed
    /// ladder rung ([`rival_castle_token_price_off`]).
    fn mc2_rival_ball_arm_open(&self, ri: usize) -> bool {
        let owns_possess_token = if rival_cascade_token_gate_off() {
            self.mc2_rivals[ri].known[1]
        } else {
            self.mc2_rivals[ri].book.ent[1] != 0
        };
        let owns_castle_token = if rival_cascade_token_gate_off() {
            self.mc2_rivals[ri].known[2]
        } else {
            self.mc2_rivals[ri].book.ent[2] != 0
        };
        owns_possess_token
            && (!owns_castle_token || {
                let cost = self
                    .mc2_rival_castle_token_price(ri)
                    .unwrap_or_else(|| self.mc2_castle_ladder_cost(ri).max(0) as u32);
                self.mc2_rivals[ri].mana_max <= cost
            })
    }

    /// Test hooks for the two cascade gates
    /// [`rival_castle_token_price_off`] moves (round 104).
    #[cfg(test)]
    pub(crate) fn mc2_rival_ball_arm_open_for_test(&self, ri: usize) -> bool {
        self.mc2_rival_ball_arm_open(ri)
    }

    #[cfg(test)]
    pub(crate) fn mc2_rival_afford_castle_for_test(&self, ri: usize) -> bool {
        self.mc2_rival_afford_castle(ri)
    }

    pub(crate) fn mc2_set_rival_state(&mut self, ri: usize, s: Mc2AiState, target: u16) {
        if std::env::var_os("MGC_K1_TRACE").is_some() && self.mc2_rivals[ri].state != s {
            eprintln!(
                "[k1/state] t={} ri={ri} ent={} {:?} -> {s:?} target {} -> {target}",
                crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                self.mc2_rivals[ri].ent,
                self.mc2_rivals[ri].state,
                self.mc2_rivals[ri].target,
            );
        }
        self.mc2_rivals[ri].state = s;
        self.mc2_rivals[ri].target = target;
        self.mc2_rivals[ri].target_sig = self.mc2_target_sig(target);
        // EF:6114-15 and every goal-predicate sibling (6140/6174/
        // 6224/6259/6283/6332/6394, retaliation 9002): the pick
        // writes the pair onto the WIZARD ENTITY — word_0x96_150 is
        // where the cast's aim pitch reads the target back
        // (sub_14E10 EF:6810/6860). The import already reads the
        // entity (reanchor doc); the sim never wrote it back, so
        // free-run f146 held 0 and the landed pitch law aimed at
        // the scratch slot (mc2l1 t=193: retail 141, port 0). The
        // target is mirrored VERBATIM (it already carries the
        // PLAYER_TARGET convention the importer produces); the
        // stale-target drop at state_tick does NOT come through
        // this funnel — retail never zeroes the word.
        let e = self.mc2_rivals[ri].ent as usize;
        if e != 0 && e < self.g.ent.len() {
            self.g.ent[e].f146 = target;
        }
    }

    /// Target signature `sub_14C40` (EF:6701): id + model + class<<7.
    fn mc2_target_sig(&self, target: u16) -> u16 {
        if target == 0 {
            return 0;
        }
        if let Some(hx) = self.g.hx_of_tag(target) {
            // The human carpet's RETAIL signature (sub_14C40
            // EF:6702: id + model + (class << 7); the carpet's
            // id_0x1A is its own slot, class 3 model 0 — measured
            // 727 = 343+384 / 808 = 424+384). Self-consistent on a
            // native world (slot 0 → 384) because the write side
            // uses this same function.
            return self.mc2_carpet_slot[hx].wrapping_add(3u16 << 7);
        }
        let e = &self.g.ent[target as usize];
        // ⭐ THE ID IS STORED TRANSLATED; THE SUM IS RETAIL'S. The
        // importer maps the human's pool slot to `PLAYER_TARGET` in
        // `id24` on every non-class-11 record (conformance.rs
        // `import_ent_mc2`, `id24: tr(r.id24)`), and a castle's
        // `id_0x1A_26` IS its owner wizard's id (mc2-castle-builder.md
        // §"BIND owner", `v4x->id_0x1A_26 = a1x->id_0x1A_26`). Retail's
        // `sub_14C40` (EF:6701) sums the RAW id, so the stored
        // `word_0x98_152` for the human's castle is `human + 2 +
        // (3 << 7)` (mc2l6-rsg: 343 + 386 = 729), while the sentinel
        // summed to 0xFFFF + 386 = 385 and `sub_14C60` (EF:6707) went
        // FALSE on every tick a rival raided the human's castle —
        // `sub_13710` (EF:5878-80) fell out at its first line: no
        // `roll` tan2 (EF:5881), no approach/brake, no castle-attack
        // pick, no `sub_14E10` cast (the pitch aim EF:6808, burst
        // EF:6809-11, the arm `sub_5F660`, `cooldown[s]` EF:6814).
        // mc2l6-rsg t=6430..6479: retail's setpoint tracks 889 → 668
        // while the port holds the imported 908 (the 246 ±1
        // INHERITED heading segments); t=6478→6479 and 7090→7091:
        // retail casts spell 0 (pitch 397 → 1734 / 1666 → 1937,
        // burst 7 → −26, cooldown[0] 2, token slot 3 `f2e` 5) and the
        // port does nothing. Native play is unaffected either way —
        // the write side (`mc2_set_rival_state`) uses this same
        // function — but only the untranslated sum matches the
        // recorded `f98`, so the import-restored signature admits the
        // human's castle exactly when retail's does.
        // ⭐⭐⭐ AND ON THE (10,57) FOOL'S-MANA SPHERE THE PORT'S
        // `id24` IS NOT AN `@0x1A` AT ALL — IT IS RETAIL'S `@0x28`.
        // `sub_14C40` sums `id_0x1A_26` and nothing else. Shipped
        // `NETHERW.EXE` file 0x39440 (linear 0x14C40, +0x24800):
        //   53 55 89 e5              push %ebx; push %ebp; mov %esp,%ebp
        //   8b 5d 0c                 mov  0xc(%ebp),%ebx      ; a1
        //   66 0f be 43 3f           movsbw 0x3f(%ebx),%ax    ; class
        //   66 0f be 53 40           movsbw 0x40(%ebx),%dx    ; model
        //   c1 e0 07 / 01 d0         shl $0x7,%eax; add %edx,%eax
        //   66 03 43 1a              add  0x1a(%ebx),%ax      ; + @0x1A
        //   5d 5b c3                 ret
        // and `sub_14C60` right below it at 0x39460 is the single
        // `cmp %bx,%ax` against `0x98(%edx)`. `@0x1A` is whatever
        // `NewEvent_4A050` left there — the record's OWN SLOT INDEX:
        // file 0x6E915-0x6E946 computes `(ptr - base) / 0xA8` and
        // stores it with `66 89 43 1a  mov %ax,0x1a(%ebx)`.
        //
        // The sphere's ONLY creation site in the whole decompile is
        // `sub_6C870` EF:57894-57905 (`grep ", 10, 57)"` = one hit),
        // and its owner stamp is `v1x->parentId_0x28_40 =
        // v2x->id_0x1A_26` — it never touches the new record's own
        // `@0x1A`. The importer nevertheless fuses that `@0x28` into
        // `id24` (conformance.rs, the `c == 10 && m == 57 &&
        // id24 != slot` arm of `translated`, which is also why the
        // `--port` dump prints `f1a` as `—` on a sphere), so summing
        // `id24` yields `caster + model + class<<7` where retail sums
        // `slot + model + class<<7`.
        //
        // ⭐ THE ENUMERATED `id_0x1A_26 =` WRITE LIST IS WHAT SCOPES
        // THIS. Of the seats whose `@0x28` the importer fuses, five
        // COPY the parent's `@0x1A` one statement later — the pyramid
        // summon (EF:13415), the summon-army creature (EF:29611), the
        // metamorph puppet (EF:56329), the (9,24) carrier (EF:57669)
        // and the (10,72) ring node (EF:59170) — so there the port's
        // `id24` IS retail's `@0x1A` and this function was already
        // right. Two do NOT: the (10,57) sphere and the class-15
        // manifestation (`sub_5CF40` EF:59395-99, `parentId` only).
        // A third, the (10,42) build painter (EF:61193-94), copies
        // the CASTLE's `@0x1A` (= the owner wizard's id) while its
        // `@0x28` is the castle SLOT, so its fused `id24` is a third
        // word again. Only the sphere is reachable as a rival TARGET:
        // `mc2_rival_pick_ball` walks the class-10 chain of models
        // 39/40/**57**, and the other five pickers take castles
        // (3,2), carpets (3,0)/(3,1), class-5 mob-chain creatures and
        // class-9 projectiles — none of them fused. The manifestation
        // and the painter are therefore left alone deliberately:
        // UNREACHABLE, hence unwitnessed, hence not landed.
        //
        // mc2l6-rival-spells-galore is the witness. Rival 378 holds
        // the (10,57) at slot 159; retail's recorded `f1a` on 159 is
        // 159 and the `word_0x98_152` the import restores onto 378 is
        // 1496 = 159 + 57 + (10 << 7), while the port summed
        // 343 + 57 + 1280 = 1680 (343 = the human caster, mapped back
        // through the `PLAYER_TARGET` branch above). `sub_14C60` was
        // therefore FALSE on every tick, so `sub_135C0`'s state-6 body
        // fell out at its first line: across t=29832..29869 retail
        // re-aims `roll` 1284 → 1286 → 1288 → 1291 onto the drifting
        // sphere while the port's sits FROZEN at 1281 (a targeted
        // `MGC_WRITE_TRACE=378:f34` over the window prints not one
        // write) and never approaches, never casts possess. Ten ±1
        // `slot 378 heading` segments — 724/723, 826/825, 912/911,
        // 983/982, 1068/1067, 1114/1113, 1161/1160, 1185/1184,
        // 1206/1205, 1230/1229 — which are the yaw servo tracking a
        // stale setpoint, not a servo defect.
        //
        // ⚠ IT IS THE IMPORT THAT EXPOSES IT, NOT THE FREE RUN. The
        // pick side (`mc2_set_rival_state`) stamps this same function,
        // so a native pick and a native check agree on the wrong
        // number and grade clean; only `reanchor_mc2_rival_ai`'s
        // VERBATIM `r.target_sig = ai.target_sig` puts retail's real
        // word in front of it. Both horizons are unmoved.
        let id = if e.class64 == 10 && e.model65 == 57 && !no_m57_sig_is_slot() {
            target
        } else if let Some(hx) = self.g.hx_of_tag(e.id24) {
            self.mc2_carpet_slot[hx]
        } else {
            e.id24
        };
        id.wrapping_add(e.model65 as u16)
            .wrapping_add((e.class64 as u16) << 7)
    }

    /// The target-validity gate `sub_14C60` (EF:6707): **one
    /// statement**, `sub_14C40(a2x) == a1x->word_0x98_152`. It is an
    /// IDENTITY test, not a LIVENESS test.
    ///
    /// ⭐ THE LIFE AND REAP PREDICATES DO NOT BELONG HERE. Retail asks
    /// only whether the slot still holds the same
    /// `id + model + (class<<7)` it held when the pick stamped it. A
    /// creature that has DIED — or been reap-flagged — keeps all three
    /// bytes until the record is actually FREED, and freeing is what
    /// clears `class_0x3F_63` and so breaks the signature by itself.
    /// The liveness test retail *does* have is a WRITE one function
    /// away, in `sub_5EFA0` (see [`Self::mc2_rival_intake`]) — and it
    /// is skipped whenever the wizard is docked, which is precisely
    /// the phase this predicate could not express.
    ///
    /// mc2l6-rsg t=4494 is the witness: rival 370's disguise anchor,
    /// the (5,19) at slot 213, steps life 0 → −200 (action 154 → 156)
    /// on the tick before 370's own walk slot, while 370 spends the
    /// last tick of its castle grace. Its signature is unchanged at
    /// 872 (= 213 + 19 + (5 << 7)) and retail's `sub_161A0` runs in
    /// full. The port's gate went false and `mc2_rival_state_tick`
    /// returned before any of it.
    fn mc2_target_alive(&self, target: u16, sig: u16) -> bool {
        if target == 0 {
            return false;
        }
        if let Some(hx) = self.hx_of_tag(target) {
            // Signature-honouring, like every pool target: an
            // imported sig that names something else (the human's
            // CASTLE after a retaliation re-point) must fail here.
            //
            // ⭐⭐⭐ AND NOTHING ELSE — THE HUMAN ARM CARRIED THE
            // LIVENESS TEST THE DOC ABOVE SAYS DOES NOT BELONG HERE.
            // See [`no_mc2_target_alive_human_liveness`].
            if no_mc2_target_alive_human_liveness() && self.human_state(hx) != LifeState::Alive {
                return false;
            }
            return self.mc2_target_sig(self.g.human_tag[hx].0) == sig;
        }
        self.mc2_target_sig(target) == sig
    }

    /// Any of the listed spells owned.
    fn mc2_rival_owns_any(&self, ri: usize, set: &[u8]) -> bool {
        set.iter()
            .any(|&s| self.mc2_rivals[ri].book.ent[s as usize] != 0)
    }

    /// The castle spell's **STAMPED** price —
    /// `sub_146C0(a1x, 2u)->maxMana_0x8C_140`, which
    /// [`Self::mc2_rival_set_spell`] parks in the class-15 token's
    /// `max_life` complete with `GetSpellManaCost_6D710`'s tier
    /// multiply. `None` when the wizard holds no castle token (the
    /// caller's `sub_146C0` null arm). See
    /// [`rival_castle_token_price_off`].
    fn mc2_rival_castle_token_price(&self, ri: usize) -> Option<u32> {
        if rival_castle_token_price_off() {
            return None;
        }
        let m = self.mc2_rivals[ri].book.ent[2] as usize;
        if m == 0 || m >= self.g.ent.len() {
            return None;
        }
        Some(self.g.ent[m].max_life)
    }

    /// The castle-spell affordability gate, `sub_15730` (EF:7159):
    /// `a1x->maxMana_0x8C_140 >= sub_146C0(a1x, 2u)->maxMana_0x8C_140`
    /// — the TOKEN's stamped price, not a fresh ladder rung. The
    /// ladder is only the pre-law fallback (and the no-token case,
    /// where every retail caller has already short-circuited).
    fn mc2_rival_afford_castle(&self, ri: usize) -> bool {
        if let Some(p) = self.mc2_rival_castle_token_price(ri) {
            return self.mc2_rivals[ri].mana_max >= p;
        }
        self.mc2_rivals[ri].mana_max as i32 >= self.mc2_castle_ladder_cost(ri)
    }

    /// The AI's castle affordability price. ONE definition per game —
    /// [`crate::mc2::castle::MC2_CASTLE_COST`]; this was the second
    /// hand-rolled copy whose rung 7 read `0x3E8` (= 1000) instead of
    /// retail's 300,000,000 sentinel. See the note at
    /// `mc2_rival_set_spell` for the tier-multiply scope still owed.
    fn mc2_castle_ladder_cost(&self, ri: usize) -> i32 {
        let lvl = self
            .rival_castle(self.mc2_rivals[ri].ent)
            .map_or(0, |c| self.g.ent[c].f26.clamp(0, 7) as usize);
        crate::mc2::castle::MC2_CASTLE_COST[lvl] as i32
    }

    /// Castle-site scout (sub_13B00 EF:6056-6103): walk the 4x4
    /// sector grid from the OWN sector (+x inner, +y outer, wrapping
    /// mod 4); a sector CORNER qualifies when the nearest foreign
    /// castle is over 12288 away in CHEBYSHEV max(|dx|,|dy|). The
    /// FIRST qualifying corner wins — no water veto, no +128 centre
    /// offset, no second candidate (the duplicated check in the
    /// decompile is a loop-unroll artifact, not a second candidate).
    ///
    /// ⭐⭐⭐ TWO METRICS, AND RETAIL GATES THE ELECTION WINNER.
    /// EF:6087 is `sub_14B10(v1x, 2)` — an ELECTION (EF:6615-63: walk
    /// the class-3 roster chain `dword_38519`, skip records whose
    /// `id_0x1A_26` matches the scout's own, keep `model_0x40_64 == 2`,
    /// rank by `EuclideanDistXY_584D0`, strict `<` so the first of a
    /// tie holds) — and ONLY that winner is handed to `sub_583B0`. The
    /// port had folded both into one in-loop `min` of the GATE metric,
    /// the standing "retail gates the winner, not the candidates"
    /// shape: a Euclidean-nearer castle that clears the Chebyshev bar
    /// must still veto a corner that some farther castle sits closer
    /// to in Chebyshev.
    ///
    /// ⭐⭐⭐ AND BOTH METRICS SUBTRACT IN 16 BITS. `NETHERW.EXE`
    /// @0x2393B0 is `mov ax,[ecx] / sub ax,[esi] / cwde / cdq / xor /
    /// sub` per axis, then `cmp/jnl`: the axis difference is taken as a
    /// **16-bit** subtraction and SIGN-EXTENDED before `abs`, so the
    /// map wraps. remc2 declares `axis_3d::x/y` as `uint16_t`, which
    /// silently promotes the operands to `int` and loses the `cwde` —
    /// and the port's own doc line here used to cite that declaration
    /// as the law ("UNSIGNED axes … the brain-trace metric OPEN is
    /// CLOSED"). ⭐ THE SHIPPED EXE OUTRANKS THE LISTING.
    /// `EuclideanDistXY_584D0` (Maths.cpp:1043) already casts each
    /// delta to `int16_t`, so the election agreed all along.
    ///
    /// mc2l6-rsg t=4310 is the witness. Rival 370 loses its castle and
    /// re-scouts from sector (0,1). Corner (0,16384) elects the HUMAN's
    /// castle at (61952,17920) — `dx` wraps to −3584 — and 3584 fails
    /// the 12288 bar, so the corner is vetoed; corner (16384,16384) is
    /// castle 174's own tile; corner **(32768,16384)** elects 174 at
    /// |dx| = 16384 and passes. The port's unsigned min-over-all read
    /// 61952 at the very first corner and planted the site at x = 0.
    ///
    /// The scan-start sector is NOT the unsigned `pos >> 14`: retail
    /// derives it on the position cast to SIGNED int16 with the
    /// round-toward-zero correction (EF:6076/:6079 —
    /// `(int16_t)(pos - (sign<<14) - sign) >> 14`, sign = the -1/0
    /// indicator), then truncates to a byte. In the upper coordinate
    /// bands that DIFFERS from the unsigned shift: band 2
    /// (0x8000..0xBFFF) starts the scan at corner 3, band 3
    /// (0xC000..0xFFFF) at corner 0. Load-bearing on mc2:04 — Rahn
    /// starts at (64,255), and the signed form points the first
    /// candidate across the y-wrap at tile (64,0), the authored
    /// crater pad on HIS OWN island; the unsigned form scanned from
    /// (64,192) and planted the castle in the open sea.
    pub(crate) fn mc2_rival_scout_site(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        // EF:6074-79 — the signed-trunc sector, kept as the raw byte
        // (the `(x_BYTE)v13 + i` addition wraps mod 256, then & 3).
        let sector = |v: u16| -> u16 {
            let c = v as i16;
            let s: i16 = if c < 0 { -1 } else { 0 };
            (((c - (s << 14) - s) >> 14) as u8) as u16
        };
        let (sx, sy) = (sector(self.g.ent[i].x), sector(self.g.ent[i].y));
        for row in 0..4u16 {
            for col in 0..4u16 {
                let tx = (sx.wrapping_add(col) & 3) << 14;
                let ty = (sy.wrapping_add(row) & 3) << 14;
                // EF:6084-85: the candidate corner is built IN the
                // scratch slot 0 (`v1x->position.x/y = corner`) and
                // both metrics run on `v1x`; the last candidate tested
                // is slot 0's residue (mc2l4 record 0: x 16384, y 0 =
                // rival 298's winning corner, z untouched). Round 114.
                if !no_mc2_authored_scratch_site() && self.g.ent.len() > 1 {
                    self.g.ent[0].x = tx;
                    self.g.ent[0].y = ty;
                }
                // The 16-bit axis deltas both metrics run on.
                let d16 = |a: u16, b: u16| a.wrapping_sub(b) as i16 as i32;
                // `sub_14B10(v1x, 2)`: elect the Euclidean-nearest
                // foreign castle, ties to the first walked. (The
                // squared sum is retail's 32-bit `int` product pair, so
                // it wraps into the unsigned compare exactly as there.)
                let mut best: Option<usize> = None;
                let mut bd = u32::MAX;
                // ⭐⭐ THE CANDIDATE SET IS `dword_38519`, THE TICK-TOP
                // CLASS-3 ROSTER. `sub_14B10` (EF:6649-59) walks
                // `x_D41A0_BYTEARRAY_4_struct.dword_38519` via
                // `->next_0` and re-asks only `id != a1x->id` and
                // `model == a2`. Membership was sampled once, at the
                // tick top, under `life_0x8 >= 0` ALONE (EF:39972-85) —
                // no `flags & 0x400` test of any kind. The port's
                // `flags & 0x400` was an INVENTED liveness stand-in for
                // a membership rule it could not express, and it has
                // the sign of the real rule backwards: a record flagged
                // MID-tick is still a chain member for the rest of the
                // frame (the 0x400 reap is the NEXT tick's top
                // pre-walk, EF:39948-56), while a castle sitting at
                // `life < 0` is NOT a member however clear its flags.
                for ci in 0..self.g.wiz_chain.visible_len() {
                    let j = self.g.wiz_chain.list[ci] as usize;
                    let e = &self.g.ent[j];
                    if e.model65 == 2 && e.id24 != me {
                        let (dx, dy) = (d16(e.x, tx), d16(e.y, ty));
                        let d = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) as u32;
                        if d < bd {
                            bd = d;
                            best = Some(j);
                        }
                    }
                }
                // `sub_583B0` on the winner alone — and `!v3x` (no
                // castle on the chain at all) qualifies the corner.
                let near = best.map_or(i32::MAX, |j| {
                    let e = &self.g.ent[j];
                    d16(e.x, tx).abs().max(d16(e.y, ty).abs())
                });
                if near > 0x3000 {
                    self.mc2_rivals[ri].site = (tx, ty);
                    // EF:6090 `a1x->axis_0x9A_154x = v1x->position` —
                    // the pick is copied onto the WIZARD ENTITY, whole
                    // position: x/y are the corner, z is the SCRATCH
                    // slot 0's residual z (sub_13B00 writes only the
                    // scratch's x/y; the copy takes its z as-is). The
                    // reanchor reads these lanes back (`site: (e.dest_x,
                    // e.dest_y)`), the whiff-hover reads the entity's
                    // site_z home, and the obs projects dest_x/dest_y —
                    // the same write-back asymmetry the f146 target
                    // mirror closed.
                    if i != 0 && i < self.g.ent.len() {
                        self.g.ent[i].dest_x = tx;
                        self.g.ent[i].dest_y = ty;
                        self.g.ent[i].site_z = self.g.ent[0].z;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// `sub_106C0(Entities[castle->id_0x1A_26], castle)` — "is this
    /// castle's OWNER standing on it", the summed-extents box with the
    /// z leg (`sub_10630` EF:3712-16 = [`Gen::ent_overlap`]). The
    /// human owns no pool slot in the port, so slot 0 runs the same
    /// box against the carpet's own measured extents (121 lateral,
    /// the 100 lift / 100 half-height `player_overlap` already uses).
    fn mc2_owner_at_castle(&self, owner: u8, c: usize) -> bool {
        if let Some(hx) = self.hx_at_wizard(owner) {
            if self.human_state(hx) != LifeState::Alive {
                return false;
            }
            let (hx, hy, hz) = self.human_pose(hx);
            let e = &self.g.ent[c];
            let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
            return wd(e.x, hx) < e.f80 as i32 + crate::mc1::combat::MC2_PLAYER_HW
                && wd(e.y, hy) < e.f82 as i32 + crate::mc1::combat::MC2_PLAYER_HW
                && ((e.z as i32 + e.f78 as i16 as i32)
                    - (hz as i32 + crate::mc1::combat::PLAYER_HH))
                    .abs()
                    < e.f84 as i32 + crate::mc1::combat::PLAYER_HH;
        }
        let Some(ent) = self
            .mc2_rivals
            .iter()
            .find(|r| r.slot == owner)
            .map(|r| r.ent)
        else {
            return false;
        };
        (ent as usize) < self.g.ent.len() && self.g.ent_overlap(ent as usize, c)
    }

    /// The hate gate: hate[slot] over the wealth-scaled threshold.
    fn mc2_hate_over(&self, ri: usize, slot: u8, wealth: u32) -> bool {
        let r = &self.mc2_rivals[ri];
        let threshold = 50_000u32.saturating_sub(wealth / 10 * r.agg as u32 / 255);
        r.hate[slot as usize] as u32 > threshold
    }

    /// Enemy-castle pick (sub_13E40 EF:6182): hated-and-undefended
    /// (owner > 7680 from its castle and not physically at it) OR
    /// plain poorer (my stored >> theirs + 640*(255-agg)), nearest
    /// within the behavior-row range.
    pub(crate) fn mc2_rival_pick_castle(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let my_castle = self.rival_castle(me);
        // ⭐⭐⭐ THE RAID ARM'S CASTLE-LESS REFUSAL IS THE **FOURTH**
        // `sub_146C0` GATE — see [`rival_raid_castleless_token_off`].
        // `sub_13E40`'s entry (EF:6192) is
        // `if (!sub_164B0(a1x) || !a1x->dword_0xA4_164x->
        //     CastleEntityIndex_0x3A_58 && sub_146C0(a1x, 2u)) return 0;`
        // — byte-identical in shape to `sub_14030`'s
        // ([`rival_attack_castleless_gate_off`]). The port asked the
        // authored `known[2]` GRANT instead of the manifestation
        // register, so a rival whose keep AND castle token both died
        // stayed refused here forever and fell through to arm 5.
        let castle_token = if rival_raid_castleless_token_off() {
            self.mc2_rivals[ri].known[2]
        } else {
            self.mc2_rivals[ri].book.ent[2] != 0
        };
        if my_castle.is_none() && castle_token {
            return false;
        }
        let my_stored = my_castle.map_or(0, |c| self.g.ent[c].f140.max(0) as u32);
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let mut best: Option<(u16, i32)> = None;
        // ⭐⭐ THE CANDIDATE SET IS `dword_38519` (EF:6196), NOT THE
        // POOL: `for (ix = dword_38519; ix > Entities[0]; ix =
        // ix->next_0) if (ix->id_0x1A_26 != a1x->id_0x1A_26 &&
        // ix->model_0x40_64 == 2)`. Membership is the tick-top sample
        // `class == 3 && life_0x8 >= 0` (EF:39972-85) — no reap-flag
        // test — so the port's `flags & 0x400` was an invented
        // liveness filter standing in for a rule it cannot express,
        // and it MISSES the rule that is actually there: a castle at
        // `life < 0` is off the chain and is not a raid candidate.
        for ci in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[ci] as usize;
            let e = &self.g.ent[j];
            if e.model65 != 2 || e.id24 == me {
                continue;
            }
            let Some(owner) = self.owner_slot(e.id24) else {
                continue;
            };
            let hated = self.mc2_hate_over(ri, owner, self.wizard_wealth(owner));
            // ⭐⭐ UNDEFENDED IS **TWO** TESTS, AND THE SECOND IS THE
            // SUMMED-EXTENTS BOX. EF:6202-04 reads
            // `dist2(owner, castle) > 0x3840000 && !sub_106C0(owner,
            // castle)` — the port kept only the 7680² radius, which
            // looks like the stronger of the two and is not. A castle
            // grown to rung 6 measures 6784 in `array_0x52_82`, so
            // `sub_106C0`'s box reaches 6784 + 121 = 6905 PER AXIS:
            // its corner sits at 9765 units, a full 2085 BEYOND the
            // radius gate. An owner parked diagonally off its own
            // keep is therefore "far" and still "at" it.
            //
            // mc2l6-rsg t=952 is the witness and it is the whole head.
            // The human sits 8067 from castle 63 (65,062,037 > the
            // 58,982,400 gate) but at dx 4706 / dy 6551, INSIDE the
            // 6905 box on both axes — so retail's conjunction fails,
            // `sub_13E40` finds no candidate at all (rival 370's own
            // castle holds 0 mana, so the poorer arm cannot fire
            // either) and the cascade falls through to arm 5. Retail
            // takes AttackWizard on the human and shoots a (9,0) at
            // t=953; the port raided the castle instead and fired
            // nothing. Same predicate, same summed-box law as the
            // rival's own at-castle regen probe (`mc2_rival_tick`).
            let undefended = self
                .wizard_pos(owner)
                .is_none_or(|(wx, wy, _)| Gen::dist2_sq(e.x, e.y, wx, wy) > 7680 * 7680)
                && !self.mc2_owner_at_castle(owner, j);
            let poorer = (e.f140.max(0) as u32)
                .saturating_add(640 * (255 - self.mc2_rivals[ri].agg as u32))
                < my_stored;
            if !(hated && undefended) && !poorer {
                continue;
            }
            // ⭐ A CASTLE THAT HAS ALREADY DIED THIS TICK IS NOT A RAID
            // CANDIDATE — MEASURED, MECHANISM OPEN. mc2l22 t=825:
            // castle 496 (3,2) takes a rung DOWNGRADE at its own slot
            // (`life 53200 -> -1`, `action45 4 -> 6`; it revives at
            // t=826 with life 39999, max_life 60000 -> 40000). Rival
            // 611 — a LATER slot in the same walk — then abandons
            // state 7 (`target96 496 -> 424`, `f98 810 -> 808`,
            // `ai_state 7 -> 8`) instead of re-picking it, and picks
            // the human. The port kept raiding 496 and ran the
            // RaidCastle handler on a target retail had abandoned.
            // This guard reproduces the recording and is worth +268
            // ticks of mc2l22 horizon (825 -> 1093), census-neutral on
            // mc2l6-rsg (419 -> 419 byte-identical).
            //
            // ⚠ THE MECHANISM IS NOT THIS FUNCTION, AND THE LEDGER
            // SAYS SO. `sub_13E40` (NETHERW.EXE 0x38640) was
            // disassembled instruction-for-instruction and carries NO
            // life test: the chain walk `[0x41a4] -> [+0x9677]`
            // (`dword_38519`), `cmp byte [ebx+0x40],2`, the hate arm,
            // the `640*(255-agg)` poorer arm, the `[esi+0xa0]->[+0x1c]`
            // squared ring, `mov [esi+0x96],ax`. And the chain itself
            // is a genuine TICK-TOP snapshot — `UpdateEntities_57730`
            // (EF:39968-85) rebuilds `dword_38519` with `life >= 0` in
            // one pass BEFORE the walk, so 496 (alive at tick top) is
            // still in it. Recorded gameplay outranks the listing, so
            // the behaviour is landed here and the PATH is an open dig.
            //
            // ⭐ WHAT THE NEXT DIG ALREADY RULED OUT (session 96):
            // • NOT a dead-target arm in the state-7 handler.
            //   `sub_13710`'s first statement IS a target test —
            //   `sub_14C60(a1x, Entities[word_0x96_150])` (EF:6707) =
            //   `sub_14C40(target) == word_0x98_152`, and `sub_14C40`
            //   (EF:6701) is `id_0x1A_26 + model_0x40_64 + (class<<7)`,
            //   which DECODES this take's `f98` exactly (castle 496:
            //   424 + 2 + 384 = 810; the human wizard 424: 424 + 0 +
            //   384 = 808). But it is a SIGNATURE test, and a rung
            //   downgrade changes neither class, model nor `f1a`, so
            //   it still PASSES on 496 — and its failure arm
            //   (`goto LABEL_12; sub_16580; return 0`) cannot change
            //   the state anyway: `sub_12910` (EF:5243) DISCARDS every
            //   handler's return and calls `sub_12E70` unconditionally
            //   afterwards. Only `sub_12E70`'s re-pick writes
            //   `byte_0x1C1_449`.
            // • NOT one of this function's own predicates. Measured
            //   with a probe on mc2l22 pair 824→825: for rival 611
            //   castle 496 is the ONLY candidate inside the ring
            //   (d² 4,441,049 vs `v_28²` 67,108,864 — every other
            //   castle is 40x-400x beyond it), `hated` is true and the
            //   poorer arm is true by a mile (496 holds 69,184 vs
            //   611's own castle 637 at 317,400, so
            //   `69,184 + 640*(255-agg) < 317,400` for EVERY agg).
            //   Retail computes those from the same numbers, so
            //   EF:6198-6210 cannot be what rejected it.
            // ⇒ the only exclusion rule left is CHAIN MEMBERSHIP, and
            //   `dword_38519`'s only class-3 filter is `life_0x8 >= 0`
            //   (EF:39972-85) — i.e. this guard states retail's rule
            //   correctly and is sitting one level too low. The open
            //   question is purely WHEN retail's chain is rebuilt
            //   relative to the dispatch walk that kills 496, since a
            //   pure tick-top rebuild would keep it. ⚠ One real
            //   mid-tick chain-clearing path exists and is worth
            //   checking on other heads: `NewEvent_4A050`'s RECYCLE
            //   arm (Events.cpp:584-89, taken when the free stack is
            //   exhausted) zeroes ALL FIVE roster chain heads before
            //   seizing a live slot. It is not the cause here (mc2l22
            //   t=825 pops the free stack, 304 -> 302).
            // ⚠ STALE LEAD, CHECKED AND CLOSED (session 96):
            //   `sub_13E40`'s FIRST gate `!sub_164B0(a1x)` (EF:7865 —
            //   the rival must own one of spells {0,7,9,0x10,0x11,
            //   0x12,0x13,0x14,0x15} before it raids at all) IS
            //   modelled, at the CALL SITE: the cascade's arm 4 reads
            //   `mc2_rival_owns_any(ri, &OFFENSE_RAID)` and arm 5 the
            //   `sub_15E60` twin `OFFENSE_ATTACK`, and that is the
            //   only production caller. `sub_146C0(a1x, s)` is
            //   `SpellsEnabled[s] > Entities[0]`, i.e. the port's
            //   `book.ent[s] != 0` — the same test.
            // ⭐⭐⭐ RESOLVED (this session): THE MECHANISM WAS NEVER IN
            // THIS FUNCTION. `sub_13E40` carries no life test of its
            // own; the exclusion is chain MEMBERSHIP, and
            // `self.g.wiz_chain` above already samples it at the tick
            // top. What made t=825 look like a live-life rule was the
            // PORT stamping the Shift+L demolish AFTER the chain
            // rebuild — retail's `PlayerEvents_51BB0` case 0x2A
            // (EF:37991-97) runs in the INPUT pass, ahead of
            // `UpdateEntities_57730` (see
            // [`crate::engine::world::mc2_demolish_phase_off`]). With
            // the stamp back in its own phase the chain excludes
            // castle 496 unaided. And the live read is affirmatively
            // WRONG one tick later: at mc2l22 t=1197 castle 502 dies
            // to MAILBOX damage mid-walk (`f24 0 -> 503`) and retail's
            // rivals 530/557 keep raiding it for that entire tick,
            // only flipping `ai_state 7 -> 14` at t=1198. That was the
            // certification wall this removes.
            if e.act_life < 0 && crate::engine::world::mc2_demolish_phase_off() {
                continue;
            }
            let d = Gen::dist2_sq(px, py, e.x, e.y);
            // STRICT: retail rejects the winner at `>= v5*v5`
            // (EF:6222/6281/6339), so equality is OUT of range.
            if d < range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::RaidCastle, t);
            true
        } else {
            false
        }
    }

    /// Enemy-wizard pick (sub_14030 EF:6233): war | hated | bully the
    /// homeless rich (32*(255-agg) margin); invisible targets are
    /// skipped; nearest within range+10.
    fn mc2_rival_pick_wizard(&mut self, ri: usize, i: usize) -> bool {
        // ⭐⭐⭐ ABSENCE 1 — THE CASTLE-LESS BUILDER DOES NOT GO
        // HUNTING. `sub_14030`'s entry is a TWO-clause refusal:
        // `if (!sub_15E60(a1x) || !a1x->dword_0xA4_164x->
        // CastleEntityIndex_0x3A_58 && sub_146C0(a1x, 2u)) return 0;`
        // (EF:6245-46). The port carried only the first clause, at the
        // call site (`mc2_rival_owns_any(&OFFENSE_ATTACK)`). Shipped
        // `NETHERW.EXE` 0x38855-0x38874 (`off = 0x34800 + linear -
        // 0x10000`, `sub_14030` linear 0x14030 → file 0x38830):
        //   0x38855 mov 0xa4(%esi),%eax      ; own wizext
        //   0x3885b cmpw $0x0,0x3a(%eax)     ; CastleEntityIndex
        //   0x38860 jne  0x38875             ; has one → carry on
        //   0x38862 push $0x2 / push %esi
        //   0x38865 call 0x38ec0             ; sub_146C0(self, 2)
        //   0x3886f jne  0x38a3e             ; owns it → return 0
        // A rival that has lost its keep but still holds the castle
        // manifestation is a BUILDER: it must fall through to arms
        // 6-10, not commit to AttackWizard.
        if !rival_attack_castleless_gate_off()
            && self.rival_castle(self.mc2_rivals[ri].ent).is_none()
            && self.mc2_rivals[ri].book.ent[2] != 0
        {
            return false;
        }
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32 + 10;
        let my_mana = self.mc2_rivals[ri].mana.max(0) as u32;
        let agg = self.mc2_rivals[ri].agg as u32;
        // The candidate walk, in `dword_38519` order — the roster
        // chain is TAIL-appended over an ascending pool sweep
        // (EF:39976-83), so the human's wizard record (the lowest
        // wizard slot in every MC2 take) comes first and the rivals
        // follow in slot order, which is exactly this enumeration.
        // `(tgt, x, y, castle_less, mana, war, hated, owns_castle_spell)`;
        // invisible records are dropped here (`!sub_15760(ix, 0xB)`,
        // EF:6252).
        let mut cands: Vec<(u16, u16, u16, bool, u32, bool, bool, bool)> = Vec::new();
        // ⭐⭐⭐ ABSENCE 3 — INVISIBILITY IS A LIVE WINDOW ON THE
        // SPELL'S MANIFESTATION, NOT A LATCH ON THE WIZARD.
        // `sub_14030`'s per-candidate skip is `!sub_15760(ix, 0xBu)`
        // (EF:6253), and `sub_15760` (EF:7163) is
        // `sub_146C0(ix, 0xB) && result->word_0x2E_46 > 0` — the
        // spell-11 manifestation must EXIST *and* still be inside its
        // window. `sub_15760(_, 0xB)` has exactly ONE call site in the
        // whole decompile, so the predicate is scoped to this picker
        // alone. The port asked two persistent booleans instead —
        // `player.invisible` for the human and `Mc2Rival::invisible`
        // for a rival — and they stay set long after retail's window
        // has run out.
        //
        // WITNESS (mc2l6-rival-spells-galore, the take's own
        // recording): the human's spell-11 manifestation is slot 81
        // ((15,11)), and its `word_0x2E_46` reads **0** at t=27,879 /
        // 27,920 / 27,931 / 27,932 alike. Retail therefore keeps the
        // human on `sub_14030`'s candidate chain throughout and rival
        // 378 holds `ai_state` 8 / `target96` 343 from t=27,879 past
        // the wall. The port's `player.invisible` went true between
        // 27,879 and 27,931, emptied the candidate list (`ncand = 0`,
        // measured), refused arm 5, and fell through to arm 8 —
        // `mc2_rival_pick_ball` elected mana ball 67, whose bearing
        // froze the aim setpoint at `roll` 1414 against retail's 1726.
        // `dump-state … 27932 378 --port` printed exactly those two
        // rows and nothing else.
        // Every human seat's candidacy (see `mc2_nearest_wizard`): his
        // record slot, roster seat and liveness, each judged where his
        // record sits in the tick-top `dword_38519` walk.
        let intact = self.g.wiz_chain.cut == usize::MAX
            || self.g.wiz_chain.visible_len() == self.g.wiz_chain.list.len();
        let seats: Vec<(Hx, u16, bool, bool)> = self
            .players
            .ixs()
            .map(|hx| {
                let human_invis = self.ghost
                    || if rival_invis_window_off() {
                        self.human_invisible(hx)
                    } else {
                        self.mc2_spell_window_live(self.mc2_book[hx].ent[11])
                    };
                let pinned = self.mc2_carpet_slot[hx];
                let human_top = if rival_human_chain_top_off() {
                    self.human_state(hx) == LifeState::Alive
                } else {
                    self.human_wiz_top[hx]
                };
                let on_chain = self.g.human_rides_chain(hx) && pinned != 0;
                let live = if on_chain {
                    !human_invis
                } else {
                    human_top && !human_invis && (pinned == 0 || intact)
                };
                (hx, pinned, on_chain, live)
            })
            .collect();
        let human_cand = |w: &Self, hx: Hx| {
            (
                w.g.human_tag[hx].0,
                w.human_pose(hx).0,
                w.human_pose(hx).1,
                w.seat_castle(hx).is_none(),
                w.human_mana(hx),
                w.mc2_rivals[ri].war[w.g.human_seat(hx) as usize],
                w.mc2_hate_over(ri, w.g.human_seat(hx), w.human_mana_max(hx)),
                w.mc2_book[hx].ent[2] != 0,
            )
        };
        let mut done = vec![false; seats.len()];
        for (k, &(hx, pinned, _, live)) in seats.iter().enumerate() {
            if pinned == 0 && live {
                cands.push(human_cand(self, hx));
                done[k] = true;
            }
        }
        let roster: Vec<u16> = self.g.wiz_chain.list[..self.g.wiz_chain.visible_len()].to_vec();
        for j in roster {
            let mut his_record = false;
            for (k, &(hx, pinned, on_chain, live)) in seats.iter().enumerate() {
                if on_chain {
                    if j == pinned {
                        if live {
                            cands.push(human_cand(self, hx));
                        }
                        done[k] = true;
                        his_record = true;
                    }
                } else if pinned != 0 && j > pinned && !done[k] && live {
                    cands.push(human_cand(self, hx));
                    done[k] = true;
                }
            }
            if his_record {
                continue;
            }
            if j as usize == i || seats.iter().any(|s| s.1 != 0 && j == s.1) {
                continue;
            }
            let Some(oj) = (0..self.mc2_rivals.len()).find(|&o| {
                o != ri && !self.mc2_rivals[o].eliminated && self.mc2_rivals[o].ent == j
            }) else {
                continue;
            };
            let (slot, ent, mana_max, mana, invis, has_castle_spell) = {
                let o = &self.mc2_rivals[oj];
                (
                    o.slot,
                    o.ent,
                    o.mana_max,
                    o.mana,
                    o.invisible,
                    o.book.ent[2] != 0,
                )
            };
            // The same `!sub_15760(ix, 0xB)` skip, off the rival's
            // own book rather than the port's latch.
            let invis = if rival_invis_window_off() {
                invis
            } else {
                let _ = invis;
                self.mc2_spell_window_live(self.mc2_rivals[oj].book.ent[11])
            };
            if invis {
                continue;
            }
            let e = &self.g.ent[ent as usize];
            if e.tick70 != 1 {
                continue;
            }
            let (ex, ey) = (e.x, e.y);
            cands.push((
                ent,
                ex,
                ey,
                self.rival_castle(ent).is_none(),
                mana.max(0) as u32,
                self.mc2_rivals[ri].war[slot as usize],
                self.mc2_hate_over(ri, slot, mana_max),
                has_castle_spell,
            ));
        }
        for (k, &(hx, pinned, on_chain, live)) in seats.iter().enumerate() {
            if pinned != 0 && !on_chain && !done[k] && live {
                cands.push(human_cand(self, hx));
            }
        }
        // ⭐⭐⭐ WAR IS AN EARLY-OUT, NOT A SCORE. `sub_14030`'s war
        // arm (EF:6256-60) is `if (array_0x1FC_508[4*colorIdx + 5] ==
        // 1) { word_0x96_150 = ix; word_0x98_152 = sub_14C40(ix);
        // return 1; }` — it fires on the FIRST chain member at war
        // with this rival, with NO distance ring and NO nearest-wins
        // tournament; the `v_28 + 10` gate at EF:6280-83 guards only
        // the hated/bully winner picked below it. The port folded
        // `war` in as a third qualifier behind the same range gate, so
        // a declared war beyond the behavior row's engagement radius
        // silently fell through to arms 6-10 (balloon / defense /
        // ball / hunt / idle) where retail commits to AttackWizard.
        // `MGC_NO_RIVAL_WAR_EARLYOUT=1` restores the folded form.
        if !rival_war_earlyout_off() {
            if let Some(&(t, ..)) = cands.iter().find(|c| c.5) {
                self.mc2_set_rival_state(ri, Mc2AiState::AttackWizard, t);
                return true;
            }
        }
        let mut best: Option<(u16, i32)> = None;
        for &(tgt, x, y, castle_less, mana, war, hated, owns_castle_spell) in &cands {
            // ⭐⭐⭐ ABSENCE 2 — "THE HOMELESS RICH" IS A THREE-TERM
            // CONJUNCTION, AND THE PORT CARRIED TWO. The bully arm is
            // `!ix->dword_0xA4_164x->CastleEntityIndex_0x3A_58
            //  && sub_146C0(ix, 2u)
            //  && ix->mana_0x90_144 + 32*(255 - agg) < a1x->mana_0x90_144`
            // (EF:6264-67). The middle term — **the VICTIM must still
            // own the castle MANIFESTATION** — was missing, so a
            // wizard that has lost its keep AND its castle token (a
            // beaten wizard, not a hoarding builder) still read as a
            // bully target. Shipped `NETHERW.EXE` 0x3896e-0x38982:
            //   0x3896e cmpw $0x0,0x3a(%ecx)   ; victim's castle idx
            //   0x38973 jne  0x389cb           ; has one → skip
            //   0x38975 push $0x2 / push %ebx
            //   0x38978 call 0x38ec0           ; sub_146C0(victim, 2)
            //   0x38982 je   0x389cb           ; does NOT own → SKIP
            //   0x38984-0x389ad the `shl $0x5` mana margin
            // WITNESS — mc2l22 pair 3836→3837: rival 451's castle 476
            // dies, and its own `spell_ent[2]` goes 454 → 0 in the
            // same tick. Retail's rivals 503/557/584 drop out of
            // RaidCastle (`ai_state 7 → 6`, the ball loop, cascade
            // step 8) and sit there until they flip to Defense at
            // t=3859; the PORT commits all three to AttackWizard
            // (`ai_state` 8) on 451 and therefore never reaches
            // cascade step 7 at all — which is why the whole
            // `mc2_rival_pick_defense` metamorph re-tier family
            // (20 of the take's 26 `(15,4) mana_max` rows) never
            // fires. `MGC_NO_RIVAL_BULLY_CASTLE_SPELL=1` restores the
            // two-term form.
            let bully = castle_less
                && (owns_castle_spell || rival_bully_castle_spell_off())
                && mana.saturating_add(32 * (255 - agg)) < my_mana;
            if !war && !hated && !bully {
                continue;
            }
            let d = Gen::dist2_sq(px, py, x, y);
            // STRICT: retail rejects the winner at `>= v5*v5`
            // (EF:6222/6281/6339), so equality is OUT of range.
            if d < range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((tgt, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::AttackWizard, t);
            true
        } else {
            false
        }
    }

    /// Test hook for [`Self::mc2_rival_pick_wizard`] (the round-104
    /// tick-top human-candidacy pin).
    #[cfg(test)]
    pub(crate) fn mc2_rival_pick_wizard_for_test(&mut self, ri: usize, i: usize) -> bool {
        self.mc2_rival_pick_wizard(ri, i)
    }

    /// Enemy-balloon pick (sub_14250 EF:6292): hated owner, cargo
    /// over 10*(275-agg), not sitting at its own castle.
    pub(crate) fn mc2_rival_pick_balloon(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let range = BEHAVIOR[self.g.ent[i].row156 as usize].v_28 as i32;
        let cargo_gate = 10 * (275 - self.mc2_rivals[ri].agg as u32);
        let mut best: Option<(u16, i32)> = None;
        // ⭐⭐ THE CANDIDATE SET IS `dword_38519` (EF:6311), NOT THE
        // POOL — the same chain and the same tick-top `life >= 0`
        // membership as `mc2_rival_pick_castle` above; the only
        // per-candidate tests retail keeps are `id != own` and
        // `model == 3`.
        for ci in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[ci] as usize;
            let e = &self.g.ent[j];
            if e.model65 != 3 || e.id24 == me {
                continue;
            }
            let Some(owner) = self.owner_slot(e.id24) else {
                continue;
            };
            if !self.mc2_hate_over(ri, owner, self.wizard_wealth(owner)) {
                continue;
            }
            if (e.f140.max(0) as u32) <= cargo_gate {
                continue;
            }
            // ⭐ "NOT PARKED OVER ITS OWN KEEP" IS THE SUMMED-EXTENTS
            // BOX, NOT A RADIUS. EF:6316 is
            // `!sub_106C0(balloon, Entities[ownerWizext->CastleEntityIndex])`
            // — the port had an invented 2048² circle, which on a
            // grown castle (extents 6784) is more than three times too
            // small: every human balloon loitering inside its own keep
            // read as a raidable target. Same law as the `undefended`
            // conjunction above and as the rival's at-castle regen
            // probe; a castle-less owner indexes retail's scratch
            // record and overlaps nothing, which is the `is_some_and`
            // here.
            let home = self.rival_castle(e.id24);
            if home.is_some_and(|c| self.g.ent_overlap(j, c)) {
                continue;
            }
            let d = Gen::dist2_sq(px, py, e.x, e.y);
            // STRICT: retail rejects the winner at `>= v5*v5`
            // (EF:6222/6281/6339), so equality is OUT of range.
            if d < range.saturating_mul(range) && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j as u16, d));
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::RaidBalloon, t);
            true
        } else {
            false
        }
    }

    /// Nearest live enemy wizard by 3-D distance — the shared scan of
    /// `sub_15FC0`/`sub_161A0` (EF:7644-59/7782-98): class-3 model 0|1
    /// on a foreign team. NO invisibility filter (retail has none
    /// here) and no hostility gate. Returns (target, pos, dist) with
    /// the distance in `sub_583F0`'s own units — a TRUNCATED integer
    /// square root, see [`mc2_dist3d_isqrt_off`]; the kill switch
    /// restores the old squared key.
    /// Test hook for [`Self::mc2_nearest_wizard`] (the round-102 chain-blank pin).
    #[cfg(test)]
    pub(crate) fn mc2_nearest_wizard_for_test(
        &self,
        ri: usize,
        i: usize,
    ) -> Option<(u16, (u16, u16, i16), i64)> {
        self.mc2_nearest_wizard(ri, i)
    }

    fn mc2_nearest_wizard(&self, _ri: usize, i: usize) -> Option<(u16, (u16, u16, i16), i64)> {
        let (px, py, pz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        let d3 = |x: u16, y: u16, z: i16| -> i64 {
            if mc2_dist3d_isqrt_off() {
                let dx = (px.wrapping_sub(x) as i16) as i64;
                let dy = (py.wrapping_sub(y) as i16) as i64;
                let dz = pz as i64 - z as i64;
                dx * dx + dy * dy + dz * dz
            } else {
                // `sub_583F0` itself (file 0x7CBF0 -> isqrt 0x96f7a).
                mc2_sub_583f0((px, py, pz), (x, y, z)) as i64
            }
        };
        let mut best: Option<(u16, (u16, u16, i16), i64)> = None;
        // ⭐⭐⭐ THE SCAN WALKS THE TICK-TOP CLASS-3 CHAIN — AND THAT HEAD
        // CAN BE BLANK. `sub_161A0` iterates
        // `x_D41A0_BYTEARRAY_4_struct.dword_38519` through `next_0`
        // (EF:7782-98; shipped NETHERW.EXE 0x3ab11 `mov 0x9677(%esi),%esi`
        // / 0x3ab55 `cmp 0x1a3e4,%esi`), the class-3 roster the tick-top
        // sweep rebuilt from every `life_0x8 >= 0` class-3 record
        // (EF:39968-85), filtered to models 0|1 and `id != own`. That
        // head is one of the five `NewEvent_4A050`'s recycle arm BLANKS
        // (Events.cpp:583-87; [`Gen::new_event`]'s `wiz_chain.cut = 0`),
        // so on a tick whose pool is FULL every seizure earlier in the
        // walk leaves this rescan with NO candidate at all — `v6x == 0`
        // — and the handler takes its else arm against a NULL record.
        // The port walked its rival array (never blank) and always found
        // somebody.
        //
        // WITNESS: mc2l22 t=4662 (round 101's parked wall — pool free
        // 1 → 0 on the tick, both Defense rivals 503/530 "one servo step
        // off", 503's final roll 318 "the bearing to the origin") and
        // t=9784 (pool 999/999, the disposition-14 wave seizing 61
        // victims before slot 530 dispatches: retail roll 303 / z 5196
        // against the port's 1871 / 5204). Both are the blanked chain:
        // retail's else arm aimed at whatever lies at linear 0x4C — see
        // [`mc2_ivt_ghost`].
        //
        // Under a pooled human (the conformance seat) the carpet is a
        // chain member at its own slot and is measured at the live
        // `human_pose`; a native world (no pooled carpet) keeps the
        // human as the leading candidate, as before.
        // ⚠ The pooled human is a ZEROED HUSK in our pool (the
        // conformance seat pins its slot; `import_ent_mc2`), so it never
        // joins `wiz_chain` — it is walked HERE at the position its slot
        // would hold in retail's chain (ascending pool sweep), and only
        // while the chain is INTACT: a blanked head hides the carpet's
        // record exactly like every other member. A native world (no
        // pooled carpet) keeps the human as the leading candidate.
        let own_id = self.g.ent[i].id24;
        let chain = &self.g.wiz_chain;
        let intact = chain.cut == usize::MAX || chain.visible_len() == chain.list.len();
        // Per seat: (seat, his record slot, on the roster, scannable).
        // The SAME tick-top membership sample as the arm-5 picker's
        // (see [`Self::mc2_rival_pick_wizard`]) — this is `sub_15FC0`/
        // `sub_161A0`'s half of the one `dword_38519` walk, so a human
        // who dies at his own slot stays scannable for the rest of the
        // tick here too. Ghost mode: retail has no cloak test on this
        // scan, so the cheat's own gate stands in. On the roster the
        // record carries the membership and the cut.
        let seats: Vec<(Hx, u16, bool, bool)> = self
            .players
            .ixs()
            .map(|hx| {
                let pinned = self.mc2_carpet_slot[hx];
                let human_top = if rival_human_chain_top_off() {
                    self.human_state(hx) == LifeState::Alive
                } else {
                    self.human_wiz_top[hx]
                };
                let on_chain = self.g.human_rides_chain(hx) && pinned != 0;
                let live = if on_chain {
                    !self.ghost
                } else {
                    human_top && !self.ghost && (pinned == 0 || intact)
                };
                (hx, pinned, on_chain, live)
            })
            .collect();
        let mut done = vec![false; seats.len()];
        let consider_human =
            |k: usize, done: &mut Vec<bool>, best: &mut Option<(u16, (u16, u16, i16), i64)>| {
                let (hx, _, _, live) = seats[k];
                if done[k] || !live {
                    return;
                }
                done[k] = true;
                let (hpx, hpy, hpz) = self.human_pose(hx);
                let d = d3(hpx, hpy, hpz);
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    *best = Some((self.g.human_tag[hx].0, (hpx, hpy, hpz), d));
                }
            };
        for k in 0..seats.len() {
            if seats[k].1 == 0 {
                consider_human(k, &mut done, &mut best);
            }
        }
        let roster = &chain.list[..chain.visible_len()];
        for &j in roster {
            let mut his_record = false;
            for k in 0..seats.len() {
                let (_, pinned, on_chain, _) = seats[k];
                if on_chain {
                    if j == pinned {
                        consider_human(k, &mut done, &mut best);
                        his_record = true;
                    }
                } else if pinned != 0 && j > pinned {
                    consider_human(k, &mut done, &mut best);
                }
            }
            if his_record {
                continue;
            }
            let j = j as usize;
            if j == i || seats.iter().any(|s| s.1 != 0 && j == s.1 as usize) {
                continue;
            }
            let e = &self.g.ent[j];
            if e.class64 != 3 || e.model65 > 1 || e.id24 == own_id {
                continue;
            }
            let d = d3(e.x, e.y, e.z);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((j as u16, (e.x, e.y, e.z), d));
            }
        }
        for k in 0..seats.len() {
            if seats[k].1 != 0 && !seats[k].2 {
                consider_human(k, &mut done, &mut best);
            }
        }
        best
    }

    /// DEFENSE selector (sub_15FC0 EF:7616, cascade step 7): the
    /// metamorph MIMICRY pick — blend into the local fauna. Requires
    /// Metamorph (4) owned; finds the nearest enemy wizard within
    /// 0x1400 (3-D), then the nearest disguisable creature (the
    /// [`DISGUISE_MODELS`] table, walked in order) within 0x1400 OF
    /// THAT WIZARD; pre-arms the matching metamorph tier on the
    /// manifestation (SetSpell — mid-effect queues via f44) and
    /// targets the CREATURE (the disguise anchor). A live disguise
    /// window with a valid target signature holds the state without
    /// rescanning (EF:7639/7717). No hostility filter — any foreign
    /// wizard nearby triggers the mimicry (no war gate here).
    pub(crate) fn mc2_rival_pick_defense(&mut self, ri: usize, i: usize) -> bool {
        let m4 = self.mc2_rivals[ri].book.ent[4] as usize;
        if m4 == 0 {
            return false; // metamorph not owned (EF:7641-43)
        }
        if self.g.ent[m4].f26 > 0
            && self.mc2_target_alive(self.mc2_rivals[ri].target, self.mc2_rivals[ri].target_sig)
        {
            self.mc2_rivals[ri].state = Mc2AiState::Defense;
            return true;
        }
        let Some((_, (wx, wy, wz), wd)) = self.mc2_nearest_wizard(ri, i) else {
            return false;
        };
        // ⭐ `cmpl $0x1400,-0xc(%ebp); jbe` at file 0x3a87b (EF:7657)
        // on a TRUNCATED root — so the retained band's far edge is
        // `isqrt(D) <= 5120`, i.e. `D < 5121²`, not `D <= 5120²`.
        if wd > mc2_d3_gate(0x1400) {
            return false; // no wizard close enough (EF:7660-63)
        }
        // Scan 2: the disguise anchor — nearest table-model creature
        // to the WIZARD (not to self, EF:7673). Corpse states are
        // excluded like retail's per-model alive lists (EF:39987-40008
        // skip actions 0xB4/0xE8/0xEA).
        let me = self.mc2_rivals[ri].ent;
        let mut best: Option<(usize, i64, u8)> = None;
        // ⭐⭐⭐ THE CANDIDATE SET IS THE TICK-TOP PER-MODEL ROSTER
        // CHAIN (`bytearray_38403x[model]`, EF:7662), not the live
        // pool — see [`rival_anchor_roster_off`] for the law, the
        // EXE/EF citations and the mc2l22 t=1645 witness. Membership
        // (class, model, `act_life >= 0`, the corpse-state skip) is
        // the tick-top sample [`crate::engine::features::MobChains`]
        // already keeps; retail's own walk filters on NOTHING but
        // `id_0x1A_26 != a1x->id_0x1A_26`.
        let roster = !rival_anchor_roster_off();
        for &dm in &DISGUISE_MODELS {
            let cands: Vec<u16> = if roster {
                self.g.mob_chains.visible(dm as usize).to_vec()
            } else {
                (1..self.g.ent.len() as u16).collect()
            };
            for &jw in &cands {
                let j = jw as usize;
                let e = &self.g.ent[j];
                if e.id24 == me {
                    continue;
                }
                if !roster
                    && (e.class64 != 5
                        || e.model65 != dm
                        // ⚠ NOT retail's: the roster carries no reap
                        // term (see [`rival_disguise_anchor_list_off`]).
                        // Kept only as the kill switch's restore path.
                        || (rival_disguise_anchor_list_off() && e.flags & 0x400 != 0)
                        // ⚠ `act_life`/`tick70` ARE retail's membership
                        // rule (EF:39990-40001) — read LIVE here, which
                        // is the very thing the roster walk fixes.
                        || e.act_life < 0
                        || matches!(e.tick70, 0xB4 | 0xE8 | 0xEA))
                {
                    continue;
                }
                let d = if mc2_dist3d_isqrt_off() {
                    let dx = (wx.wrapping_sub(e.x) as i16) as i64;
                    let dy = (wy.wrapping_sub(e.y) as i16) as i64;
                    let dz = wz as i64 - e.z as i64;
                    dx * dx + dy * dy + dz * dz
                } else {
                    // `sub_583F0` (file 0x7CBF0) — TRUNCATED root, so
                    // two candidates in one integer bucket TIE and the
                    // first in chain order keeps the seat
                    // (`cmp %edx,%eax; jae` at file 0x3a8cb).
                    mc2_sub_583f0((wx, wy, wz), (e.x, e.y, e.z)) as i64
                };
                if best.is_none_or(|(_, bd, _)| d < bd) {
                    best = Some((j, d, dm));
                }
            }
        }
        let Some((anchor, d, dm)) = best else {
            return false;
        };
        // `cmpl $0x1400,-0x10(%ebp); jae` at file 0x3a8fc (EF:7676) —
        // `isqrt(D) >= C` IS `D >= C*C`, so this edge was exact either
        // way; it moves only through the tie-break above.
        if d >= mc2_d3_gate(0x1400) {
            return false; // no plausible fauna near the threat
        }
        self.mc2_rival_set_spell(m4, mc2_disguise_tier(dm), me);
        self.mc2_set_rival_state(ri, Mc2AiState::Defense, anchor as u16);
        true
    }

    /// Mana-ball pick (sub_148E0 EF:6518-6609): walk the class-10
    /// sphere chain — the (10,39)/(10,40)
    /// spheres in pool order, then the (10,57) randoms (retail's
    /// list is built in that order). A model-57 sphere BREAKS the
    /// whole walk on a Perception roll, keeping the best so far (a
    /// failed roll evaluates it like any other ball). Skip own
    /// claims. A ball owned by a NOT-hated wizard is taken only
    /// when isolated — the nearest wizard TO THE BALL (self
    /// excluded) beyond 5120; NO wizard in the world skips it too
    /// (the retail quirk) — and not parked at the nearest non-own
    /// castle (bbox overlap). A HATED owner's balls skip both tests
    /// and rank from the rival's OWN castle (castle-less anchors to
    /// self — retail reads the Entities[0] sentinel there,
    /// documented idealization). Unowned balls rank from self. An
    /// empty walk falls back to the nearest class-5 model-22
    /// flying-chain ball not already owned.
    fn mc2_rival_pick_ball(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        let per = self.mc2_rivals[ri].per;
        let own_castle = self.rival_castle(me);
        // ⭐⭐⭐ A CASTLE-LESS HATED RANKING READS POOL SLOT 0. Retail's
        // `v15x = Entities[wizext->CastleEntityIndex_0x3A_58]` (EF:6539)
        // is `Entities[0]` for a wizard with no castle, and the hated
        // arm's `EuclideanDistXY(&v15x->position, ball)` (EF:6583)
        // measures from THAT record — the pool sentinel whose x/y is
        // whatever last staged through it: the castle-downgrade
        // scratch (`sub_605E0`, the last castle anywhere to lose a
        // level — round 99's law already writes `ent[0]` here) or a
        // build-site scout's candidate corner (`sub_13B00`). The port
        // anchored to SELF instead ("documented idealization").
        // mc2l22 t=5917: castle-less rival 451 at (25823,60850) hates
        // the human and re-picks between the human's balls 43 at
        // (19793,18797) and 621 at (24542,3984); from itself 621 is
        // 76.8M and 43 is 588M, from slot 0 = castle 529's site
        // (17152,25344) 43 is 49.8M and 621 is 511M. Retail's target96
        // reads 43; the port's read 621, and the heading it turned
        // toward that ball was the 5919..5934 reset cluster.
        let anchor = own_castle
            .map(|c| (self.g.ent[c].x, self.g.ent[c].y))
            .unwrap_or((self.g.ent[0].x, self.g.ent[0].y));
        let mut best: Option<(u16, i32)> = None;
        // ⭐⭐⭐ THE CANDIDATE SET IS THE `dword_38523` CHAIN, AND IT
        // IS **ONE INTERLEAVED WALK**. EF:6538 is
        // `v2x = x_D41A0_BYTEARRAY_4_struct.dword_38523`, stepped by
        // `->next_0` — the tick-top class-10 list built by the case-10
        // arm of `UpdateEntities_57730` (EF:40023-62), which links
        // models 39/40 AND 57 into the SAME chain in ascending slot
        // order under no life test and no `flags & 0x400` test (a
        // 0x400 record was already freed by that function's own
        // top pre-walk, EF:39948-56 → `sub_57F20`). The port ran TWO
        // model-filtered passes over the pool, so it evaluated EVERY
        // (10,39)/(10,40) before it ever rolled the 57-break — and the
        // break is exactly what makes chain order load-bearing:
        // retail STOPS at the first non-own model-57 whose Perception
        // roll lands and keeps the best of the PREFIX only.
        //
        // mc2l6-rsg, rival 378 (Perception 108, castle 814) is the
        // witness. Its elected ball sits at chain index 0..11 on every
        // possess pick, and on ELEVEN of thirteen of them it is
        // strictly FARTHER than the globally-nearest (10,39)/(10,40):
        // t=18099 target 159 at 369,889,412 against slot 740's
        // 43,629,120; t=29823 target 159 at 129,114,954 against slot
        // 777's 3,457,640; t=19227 target 67 at 1,042,176,925 against
        // slot 787's 252,409,545. A two-pass walk's `best` after pass 0
        // IS that global minimum and pass 1 can only lower it, so the
        // port could not reach any of those targets under ANY roll.
        'walk: for ci in 0..self.g.ball_chain.visible_len() {
            let j = self.g.ball_chain.list[ci] as usize;
            let (model, claim, bx, by) = {
                let e = &self.g.ent[j];
                (e.model65, e.f144, e.x, e.y)
            };
            // The own-claim skip runs BEFORE the roll (EF:6541-42), so
            // a rival's own spheres never spend a break.
            if claim == me {
                continue;
            }
            // ⭐ THE 57-BREAK ROLLS THE **GLOBAL** WATCOM STREAM.
            // EF:6544-46 is `v3 = a1x->wizext->word_0x244_580;
            // if (watcomrand() % 255 < v3) break;` — the same CRT
            // `rand()` as the anti-rebound plan roll below
            // (EF:7212) and as MC1's :19507-08, NOT the caster's
            // entity LCG. Spending `ent_rand` here steps a GRADED
            // lane (`EntObsMc2::rand`) once per model-57 node the
            // walk touches — on mc2l6-rsg's ball chain that is a
            // 2.4-draw expectation per pick tick at Perception 108
            // where retail spends none, so every later `rand`
            // comparison on that wizard shatters even when the
            // elected ball agrees.
            if model == 57 && ((self.g.watcom_rand() % 255) as u16) < per {
                // PATCH `mc2_fools_mana_skip`: the perceptive wizard
                // sees through THIS decoy and keeps looking; retail
                // abandons the whole search (see `WorldPatches`).
                if self.patches.mc2_fools_mana_skip && !self.strict_retail {
                    continue 'walk;
                }
                break 'walk; // the 57-break (EF:6544-49)
            }
            let score = if let Some(o) = self.owner_slot(claim) {
                let hated = self.mc2_hate_over(ri, o, self.wizard_wealth(o));
                if !hated {
                    // Nearest wizard to the BALL, self excluded.
                    let mut wnear: Option<i32> = None;
                    for hx in self
                        .players
                        .ixs()
                        .filter(|&hx| self.human_state(hx) == LifeState::Alive)
                    {
                        let d = Gen::dist2_sq(bx, by, self.human_pose(hx).0, self.human_pose(hx).1);
                        if wnear.is_none_or(|w| d < w) {
                            wnear = Some(d);
                        }
                    }
                    for oj in 0..self.mc2_rivals.len() {
                        if oj == ri || self.mc2_rivals[oj].eliminated {
                            continue;
                        }
                        let oe = &self.g.ent[self.mc2_rivals[oj].ent as usize];
                        if oe.tick70 != 1 {
                            continue;
                        }
                        let d = Gen::dist2_sq(bx, by, oe.x, oe.y);
                        if wnear.is_none_or(|w| d < w) {
                            wnear = Some(d);
                        }
                    }
                    let Some(wd) = wnear else {
                        continue; // no wizard at all → skip (EF:6560-62)
                    };
                    if wd <= 5120 * 5120 {
                        continue; // guarded (EF:6565-69)
                    }
                    // At-castle skip vs the nearest non-own castle —
                    // `sub_14BD0` (EF:6678-92), which walks
                    // `dword_38519` like every other class-3 election
                    // and re-asks only `id != ball->id`,
                    // `id != own_castle->id` and `model == 2`. Same
                    // tick-top `life >= 0` membership, same absence of
                    // any `flags & 0x400` test.
                    let mut cnear: Option<(usize, i32)> = None;
                    for cj in 0..self.g.wiz_chain.visible_len() {
                        let k = self.g.wiz_chain.list[cj] as usize;
                        let c = &self.g.ent[k];
                        if c.model65 == 2 && own_castle != Some(k) {
                            let d = Gen::dist2_sq(bx, by, c.x, c.y);
                            if cnear.is_none_or(|(_, bd)| d < bd) {
                                cnear = Some((k, d));
                            }
                        }
                    }
                    // ⭐⭐⭐ THE PARK TEST IS `sub_106C0` = `sub_10630`
                    // (EF:3712-17): THREE axes, BOTH records' extents,
                    // STRICT `<` — x: |dx| < ball.pitch + castle.pitch,
                    // y: |dy| < ball.roll + castle.roll, and z:
                    // |ball.z + ball.yaw − castle.z − castle.yaw| <
                    // ball.fov + castle.fov, all on the `array_0x52`
                    // quad (port f80/f82/f78/f84). The castle's quad is
                    // TWO-VALUED: the even-phase standing tick stamps
                    // yaw −8192 / fov 0x4000 (a z window of ±16k, so
                    // the z leg is moot), but a castle in its shake
                    // countdown (`word_0x30_48` ≠ 0) skips that block
                    // and holds `SetShiftByCastle`'s 0 / 256, and then
                    // the z leg DECIDES. The port's castle-only 2-D
                    // `<=` box said "parked" wherever retail's z leg
                    // said "free". mc2l22 t=5201: rival 530 re-picks a
                    // ball; retail elects 987 (claimed by 451, isolated,
                    // 1.3M from 530) — it sits inside castle 610's
                    // 6784×6784 box but 1587 below its z with the castle
                    // mid-countdown (fov 256) — where the port skipped
                    // it and settled on 905 (22M away), then spent the
                    // next fourteen ticks flying the wrong way (the
                    // 5202..5214 reset cluster: z +4, speed 240 vs the
                    // braking 192/128, extra (10,2) casts).
                    if let Some((k, _)) = cnear {
                        let (b, c) = (&self.g.ent[j], &self.g.ent[k]);
                        let dx = (bx.wrapping_sub(c.x) as i16).unsigned_abs() as i32;
                        let dy = (by.wrapping_sub(c.y) as i16).unsigned_abs() as i32;
                        let dz =
                            (b.z as i32 + b.f78 as i16 as i32 - c.z as i32 - c.f78 as i16 as i32)
                                .abs();
                        if dx < b.f80 as i16 as i32 + c.f80 as i16 as i32
                            && dy < b.f82 as i16 as i32 + c.f82 as i16 as i32
                            && dz < b.f84 as i16 as i32 + c.f84 as i16 as i32
                        {
                            continue; // parked at a castle (EF:6570-71)
                        }
                    }
                    Gen::dist2_sq(px, py, bx, by)
                } else {
                    Gen::dist2_sq(anchor.0, anchor.1, bx, by)
                }
            } else {
                Gen::dist2_sq(px, py, bx, by)
            };
            if best.is_none_or(|(_, bd)| score < bd) {
                best = Some((j as u16, score));
            }
        }
        // Second-chain fallback: the wild flying-chain balls
        // (class 5 model 22 — EF:6593-6604).
        //
        // ⭐⭐⭐ THE CANDIDATE SET IS THE MODEL-22 ROSTER CHAIN, NOT THE
        // POOL. EF:6595 is `for (ix = bytearray_38403x[88 / 4]; ix >
        // Entities[0]; ix = ix->next_0)` — the tick-top per-model
        // roster (EF:39987-40008: class 5, `life >= 0`, `actionIndex`
        // not 0xB4 / 0xE8 / 0xEA), the same chain `mc2_rival_pick_mana`
        // walks; the only per-candidate tests are EF:6597's
        // `model != 57` (vacuous on this chain) and
        // `playerEntityIndex_0x94 != a1x->id_0x1A`. A worm's TAIL
        // SEGMENTS ride action 0xB4 and are never members; the pool
        // walk admitted them, and a segment trailing its head is
        // always the nearer record. mc2l22 t=1: the six possess-capable
        // rivals (477/503/530/557/584/611) elect the nearest HEAD
        // (action 176) in retail — 390/345/360/375/345/375 — and the
        // nearest 0xB4 segment in the port (403/358/374/384/351/389);
        // `roll` (the tan2 setpoint) parts at t=2 and `heading` at
        // t=3 on every one of them.
        if best.is_none() {
            for k in 0..self.g.mob_chains.visible(22).len() {
                let j = self.g.mob_chains.visible(22)[k] as usize;
                let e = &self.g.ent[j];
                if e.f144 == me {
                    continue;
                }
                let d = Gen::dist2_sq(px, py, e.x, e.y);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::Possess, t);
            true
        } else {
            false
        }
    }

    /// Mana-holder hunt (sub_14530 EF:6341): any other-team creature
    /// with mana, nearest to the own castle (or self), no range cap.
    ///
    /// ⭐⭐⭐ THE CANDIDATE SET IS THE 29 PER-MODEL ROSTER CHAINS, NOT
    /// THE POOL. EF:6357-59 is `for (i = 0; i < 29; i++) for (jx =
    /// bytearray_38403x[i]; jx > Entities[0]; jx = jx->next_0)`, and
    /// the ONLY per-candidate tests inside are `jx->id_0x1A_26 !=
    /// a1x->id_0x1A_26 && jx->mana_0x90_144 > 0`. Every liveness
    /// question is already settled at BUILD time by the tick-top sweep
    /// (EF:39987-40008: class 5, `life >= 0`, and `actionIndex` not
    /// 0xB4 / 0xE8 / 0xEA), which is why the body carries no life or
    /// reap test of its own — the port's `flags & 0x400` and
    /// `act_life < 0` filters were standing in for a membership rule
    /// they cannot express, and its pool walk ADMITTED the three
    /// excluded action states outright.
    ///
    /// mc2l6-rsg t=5289 is the witness, and it is the excluded-state
    /// half: rival 383 (castle 392 at 61952,41472) hunts among a pair
    /// of model-3 creatures a few hundred units apart — slot 290 at
    /// (3353,41786), `action45` 25, and slot 294 at (3225,41244),
    /// `action45` **0xE8**. By `EuclideanDistXY_584D0` from the castle
    /// 294 is the nearer of the two (46 414 465 against 290's
    /// 48 220 565), so the pool walk elected it; retail's roster never
    /// contains it and 290 stands. The port then held the wrong
    /// creature for 34 860 ticks.
    /// ⭐ THIS IS SESSION 71's MC1 LAW (`str_36382x`) ON A NEW SITE.
    pub(crate) fn mc2_rival_pick_mana(&mut self, ri: usize, i: usize) -> bool {
        let me = self.mc2_rivals[ri].ent;
        let anchor = self
            .rival_castle(me)
            .map(|c| (self.g.ent[c].x, self.g.ent[c].y))
            .unwrap_or((self.g.ent[i].x, self.g.ent[i].y));
        let mut best: Option<(u16, i32)> = None;
        for m in 0..self.g.mob_chains.list.len() {
            for k in 0..self.g.mob_chains.visible(m).len() {
                let j = self.g.mob_chains.visible(m)[k] as usize;
                let e = &self.g.ent[j];
                if e.id24 == me || e.f140 <= 0 {
                    continue;
                }
                let d = Gen::dist2_sq(anchor.0, anchor.1, e.x, e.y);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j as u16, d));
                }
            }
        }
        if let Some((t, _)) = best {
            self.mc2_set_rival_state(ri, Mc2AiState::HuntMana, t);
            true
        } else {
            false
        }
    }

    // ---- state handlers ----------------------------------------------------

    pub(crate) fn mc2_rival_state_tick(&mut self, ri: usize, i: usize, think: bool) {
        let needs_target = matches!(
            self.mc2_rivals[ri].state,
            Mc2AiState::Upgrade
                | Mc2AiState::Possess
                | Mc2AiState::RaidCastle
                | Mc2AiState::AttackWizard
                | Mc2AiState::RaidBalloon
                | Mc2AiState::HuntMana
                | Mc2AiState::Defense
        );
        // ⭐ A STALE TARGET SKIPS THE HANDLER — IT DOES NOT CLEAR THE
        // STATE. Every target-carrying handler opens with the same two
        // statements, `v1x = Entities[word_0x96_150]` then
        // `sub_14C60(a1x, v1x)` (= `sub_14C40(v1x) == word_0x98_152`),
        // and every one of their FALSE arms is the same two lines:
        // `sub_16580(a1x); return 0;` — sub_12FF0 EF:5586/5609,
        // sub_131F0 :5664-70, sub_135C0 :5831-36, sub_13710
        // :5880/5905, sub_13890 :5953/6050, sub_161A0 :7744. NOT ONE of
        // them writes `byte_0x1C1_449` or zeroes `word_0x96_150`. The
        // state stands, dead target and all, until the SELECTOR
        // re-arbitrates on the next cadence tick.
        //
        // The port dropped to `Fresh` instead, which is retail's state
        // BYTE 0 — a distinct dispatch arm (`sub_12910` case 0 runs the
        // selector TWICE, EF:5255-56). So one dead target both misread
        // the rival's state for up to a whole think period AND spent an
        // extra arbitration. mc2l6-rsg is the witness: rival 370 sits
        // in retail's state 8 across t=294..330, 419..439, 447..488,
        // 700..744 and 796..842 while the port reads 0 — five windows
        // whose lengths are exactly "until the next `f63 % 49 == 0`".
        // The t=953 head is where it finally costs a shot: retail's 370
        // holds the human (`target96` 343) and fires a (9,0) at slot 40;
        // the port had re-arbitrated onto castle 63 and fired nothing.
        if needs_target
            && !self.mc2_target_alive(self.mc2_rivals[ri].target, self.mc2_rivals[ri].target_sig)
        {
            return;
        }
        match self.mc2_rivals[ri].state {
            Mc2AiState::Fresh => {}
            // Fly home, hover castle+512, cast the upgrade (sub_12FF0
            // EF:5579; approach 512/2048, speed-up en route).
            //
            // ⭐⭐⭐ THE HOME CASTLE IS THE **TARGET WORD**, BEHIND THE
            // SAME GATE AS EVERY OTHER HANDLER. `sub_13C50` (EF:6110-15)
            // takes state 1 by writing `word_0x96_150 = castle` and
            // `word_0x98_152 = sub_14C40(castle)`, and `sub_12FF0`
            // opens exactly like its siblings: `v1x =
            // Entities[word_0x96_150]; if (!sub_14C60(a1x, v1x)) goto
            // LABEL_14` (EF:5585-86 — steer, return 0, NO roll stamp,
            // NO approach). The port resolved the castle by OWNER
            // (`rival_castle`) and left Upgrade out of the gate list
            // above, so the one tick the intake's dead-target drop
            // (`sub_5EFA0` EF:60634-39, [`Self::mc2_rival_intake`])
            // zeroes the word, retail's handler no-ops and the port's
            // still flew home. The invented `Fresh` fallback on a
            // missing castle is gone with it: retail never writes the
            // state byte from a handler (see the note above).
            //
            // WITNESS (mc2l22 t=6519, the free run's next head after
            // the corpse-token law): the human knocks rival 611's
            // castle 637 down a level — `life 739 -> -861` at t=6518,
            // revived at 40000 at its own slot on 6519, ABOVE 611 in
            // the walk. 611's intake at 6519 reads `life_0x8 <= 0`
            // and drops the target; retail's roll HOLDS 1998 (the
            // 6518 stamp) and the selector takes state 7 on castle
            // 529. The port re-stamped `roll` 1998 -> 1995 (the fresh
            // bearing to 637 from the post-move position) and the
            // servo followed it at 6520: `heading` retail 1998 / port
            // 1995 — an INHERITED head, pair-clean from a 6519 import
            // because `roll` is not graded on a (3,1).
            Mc2AiState::Upgrade => {
                let c = self.mc2_rivals[ri].target as usize;
                if c == 0 || c >= self.g.ent.len() {
                    return;
                }
                let (cx, cy, cz) = {
                    let e = &self.g.ent[c];
                    (e.x, e.y, e.z)
                };
                if self.mc2_rival_approach(ri, i, cx, cy, 512, 2048) {
                    // ⭐ THE HOVER IS THE CAST'S **FAIL** ARM, NOT ITS
                    // PRELUDE. `sub_12FF0` (EF:5590-5606) walks the
                    // tiers, then `if (!sub_14E10(a1x, 2u)) { speed_0xc
                    // = 0; z += v_14 * sign(z - (castle.z + 512)); }`
                    // — a tick whose cast LANDS takes no z step at
                    // all (it falls to LABEL_14: steer, return 0).
                    // The port stepped z BEFORE every cast, so the
                    // one tick the upgrade fires it sat 4 above
                    // retail, and the (9,10) ball minted off that z
                    // (`wizard.z + afov`) carried the 4 with it.
                    // mc2l6-rsg t=6429: rival 383 lands its Create
                    // Castle on the very tick the `sub_12A70` tail
                    // floor (EF:5485-86: ground 450 + v_12 128 = 578)
                    // lifts it; retail reads 578 / ball 678, the port
                    // 582 / 682, and from t=6430 (cast on cooldown,
                    // the fail arm) the two agree again — the "+4 on
                    // the first tick only" tell. Same shape as the
                    // Build / Possess / RaidCastle / Attack arms
                    // below, which already hover only on the whiff.
                    if !self.mc2_rival_walk_cast(ri, i, 2) {
                        self.mc2_rivals[ri].vdes = 0;
                        self.mc2_rival_hover(i, cz.saturating_add(512));
                    }
                }
            }
            // Fly to the scouted site, plant (sub_13100 EF:5620;
            // approach 2048/4096). A whiffed cast stops and hovers at
            // the SITE's stored z + 512 (`axis_0x9A_154x.z`, the
            // entity's site_z home — retail's scout copies the
            // scratch slot's whole position, z included).
            Mc2AiState::Build => {
                // sub_13100 has NO castle-exists exit: a rival whose
                // castle landed keeps hovering the site until the
                // selector's think cadence re-arbitrates (sub_12E70's
                // sub_13B00 goes false, the priority walk runs on the
                // f63 % (64 - agg/4) tick — mc2l4 t=1..4 hovers over
                // a live castle, state flips only at t=5).
                let (sx, sy) = self.mc2_rivals[ri].site;
                if self.mc2_rival_approach(ri, i, sx, sy, 2048, 4096) {
                    if !self.mc2_rival_walk_cast(ri, i, 2) {
                        self.mc2_rivals[ri].vdes = 0;
                        let sz = self.g.ent[i].site_z;
                        self.mc2_rival_hover(i, sz.saturating_add(512));
                    }
                }
            }
            // Claim the ball with possess-1 (sub_135C0 EF:5822-68;
            // approach 1024/3072): tier-walk the possess cast in the
            // act zone; a SUCCESSFUL cast aimed strictly under 0x1C
            // writes the claim directly (EF:5849-50 — the rival-side
            // guarantee; the projectile's stamp delivery remains the
            // general law). A whiffed cast hovers at ball z+512. No
            // internal "claimed → done" exit — the selector
            // re-arbitrates and the ball pick skips own claims.
            Mc2AiState::Possess => {
                let t = self.mc2_rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                self.mc2_rival_face(i, tx, ty);
                if self.mc2_rival_approach(ri, i, tx, ty, 1024, 3072) {
                    if self.mc2_rival_walk_cast(ri, i, 1) {
                        let aim = Gen::angdist(
                            self.g.ent[i].f30,
                            Gen::angle_between(self.g.ent[i].x, self.g.ent[i].y, tx, ty),
                        );
                        if aim < 0x1C {
                            self.g.ent[t].f144 = self.mc2_rivals[ri].ent;
                        }
                    } else {
                        self.mc2_rivals[ri].vdes = 0;
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Castle raid (sub_13710 EF:5872-5913; approach
            // 2048/3584): INSIDE the cast ring the castle-walk pick
            // fires ON CADENCE; a whiffed or unavailable cast hovers
            // at castle z+512. NO ownership write — retail never
            // claims the raided castle.
            Mc2AiState::RaidCastle => {
                let t = self.mc2_rivals[ri].target as usize;
                let (tx, ty, tz) = {
                    let e = &self.g.ent[t];
                    (e.x, e.y, e.z)
                };
                self.mc2_rival_face(i, tx, ty);
                let arrived = self.mc2_rival_approach(ri, i, tx, ty, 2048, 3584);
                if arrived && think {
                    let pick = self.mc2_rival_attack_pick(ri, false);
                    let cast_ok = match pick {
                        Some(s) => self.mc2_rival_cast(ri, i, s),
                        None => false,
                    };
                    if !cast_ok {
                        self.mc2_rivals[ri].vdes = 0;
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Wizard / balloon / mana-holder attack (sub_13890
            // EF:5937-6050; approach 3328/4608): the pick + cast run
            // only INSIDE the ring with burst budget; a landed cast
            // de-latches the war toward ANY wizard target
            // (EF:5966-68); the whiff path stops, weaves (wizard
            // targets only) and z-tracks the target + 512.
            Mc2AiState::AttackWizard | Mc2AiState::RaidBalloon | Mc2AiState::HuntMana => {
                let (tx, ty, tz) = match self.mc2_rivals[ri].target {
                    ht if let Some(hx) = self.g.hx_of_tag(ht) => self.human_pose(hx),
                    t => {
                        let e = &self.g.ent[t as usize];
                        (e.x, e.y, e.z)
                    }
                };
                self.mc2_rival_face(i, tx, ty);
                let arrived = self.mc2_rival_approach(ri, i, tx, ty, 3328, 4608);
                if arrived && self.mc2_rivals[ri].burst >= 0 {
                    let fired = match self.mc2_rival_attack_pick(ri, true) {
                        Some(s) => self.mc2_rival_cast(ri, i, s),
                        None => false,
                    };
                    if fired {
                        let slot = match self.mc2_rivals[ri].target {
                            ht if self.g.is_human_tag(ht) => {
                                self.g.hx_of_tag(ht).map(|hx| self.g.human_seat(hx))
                            }
                            t => self.mc2_rivals.iter().find(|r| r.ent == t).map(|r| r.slot),
                        };
                        if let Some(s) = slot {
                            self.mc2_rivals[ri].war[s as usize] = false;
                        }
                    } else {
                        self.mc2_rivals[ri].vdes = 0;
                        // ⭐⭐ THE WEAVE GATE IS THE TARGET'S **MODEL
                        // BYTE**, NOT ITS IDENTITY. NETHERW.EXE 0x1396A
                        // reads `[esi+0x40]` off `Entities[word_0x96_150]`
                        // and takes the weave on model 0 or 1 — no class
                        // test, no roster lookup. The two wizard shapes
                        // ARE (3,0) and (3,1), which is why the identity
                        // reading held up until a rival hunted a
                        // non-wizard that shares the model: mc2l1 t=449,
                        // rival 138 in state 0xD onto the (5,1) mana
                        // creature at slot 60 — retail rolls the entity
                        // LCG and snaps the yaw 105 → 607, the port sat
                        // still. Model 0 stands in for the human, whose
                        // record retail reaches through its real slot.
                        let weave_target = if crate::engine::features::no_mc2_weave_model_gate() {
                            (self.g.is_human_tag(self.mc2_rivals[ri].target))
                                || self
                                    .mc2_rivals
                                    .iter()
                                    .any(|r| r.ent == self.mc2_rivals[ri].target)
                        } else {
                            match self.mc2_rivals[ri].target {
                                ht if self.g.is_human_tag(ht) => true,
                                t => matches!(self.g.ent[t as usize].model65, 0 | 1),
                            }
                        };
                        if weave_target {
                            self.mc2_rival_weave(ri, i);
                        }
                        self.mc2_rival_hover(i, tz.saturating_add(512));
                    }
                }
            }
            // Home (sub_133B0 EF:5745; approach 256/2048): cloak-0xB
            // while fleeing; heal up at the castle.
            Mc2AiState::Home => {
                let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) else {
                    // The castle-MISSING arm (EF:5771-97; NETHERW.EXE
                    // 0x37CCA-0x37D9F) — see
                    // [`no_mc2_rival_homeless_speed_run`]. Four steps,
                    // of which the port had only the first.
                    self.mc2_rival_walk_cast(ri, i, 0xB);
                    if no_mc2_rival_homeless_speed_run() {
                        self.mc2_rivals[ri].state = Mc2AiState::Cruise;
                        return;
                    }
                    // Step 2: the SPEED tier walk. The executor's
                    // return is NOT tested (0x37d24 -> 0x37d32
                    // `mov $0x1,%ebx`), so a probe HIT ends the tick
                    // whether or not the cast landed.
                    let mut tier = self.mc2_rivals[ri].book.levels[3] as i16;
                    while tier >= 0 {
                        if self.mc2_rival_tier_probe(ri, tier, 3) == 3 {
                            self.mc2_rival_cast(ri, i, 3);
                            return;
                        }
                        tier -= 1;
                    }
                    // Step 3: already boosting -> nothing else.
                    if self.mc2_spell_window_live(self.mc2_rivals[ri].book.ent[3]) {
                        return;
                    }
                    // Step 4: cruise at the row's base speed, braked.
                    self.mc2_rivals[ri].vdes = self.g.ent[i].f128;
                    self.mc2_rivals[ri].v14 = BrakeWord(true);
                    return;
                };
                let (cx, cy) = (self.g.ent[c].x, self.g.ent[c].y);
                // ⭐ THE LONG WAY HOME IS A MANA RUN. Retail's very
                // first statement inside the castle-EXISTS arm
                // (EF:5787-94) is `if (EuclideanDistXY(self, castle) >
                // 0x6400000)` → walk spell 1 (the Possession /
                // Mana Magnet / Mana Lock family) down from its level
                // and, ON A SUCCESSFUL CAST, `goto LABEL_6` — which
                // RETURNS 1 past the cloak walk and past the approach
                // entirely. So a rival flying home from far away
                // spends the tick casting instead of steering, and
                // that is the mc2l6 t=587 head: rival 378 (ai_state
                // 0xB the whole window, castle 174 at the map centre,
                // itself out at x=64384) mints the basic (9,1) at the
                // just-freed slot 561 with charge 90 -> 0, burst 6 ->
                // 7 and cooldown[1] 0 -> 10. The port's Home arm was a
                // paraphrase that started at the cloak walk, so the
                // rival never chose the spell at all.
                //
                // The distance is `EuclideanDistXY_584D0`
                // (Maths:1043) — dist² with BOTH deltas SIGN-CAST to
                // int16, which is `Gen::dist2_sq` verbatim; at t=586
                // the -48000 dx wraps to +17536 and the comparison
                // clears the threshold by 3x either way.
                //
                // ⭐⭐⭐ Retail's loop is `if (probe == 1 && cast(1))
                // goto`, i.e. it keeps walking DOWN on a refused cast
                // where `walk_cast` stops. The refusals `sub_14E10`
                // can raise here (the 0xAA cone, the burst lockout)
                // ARE tier-independent — but the token's PRICE STAMP
                // is not, and that is the graded lane: retail's walk
                // re-stamps tier 0 on the way down, the port left the
                // refusal's tier on the token. 21 of mc2l12's heads.
                // [`Self::mc2_rival_walk_cast_chained`] is the site's
                // own shape; every other retail walk breaks first.
                if Gen::dist2_sq(self.g.ent[i].x, self.g.ent[i].y, cx, cy) as u32 > 0x640_0000
                    && self.mc2_rival_walk_cast_chained(ri, i, 1)
                {
                    return;
                }
                self.mc2_rival_walk_cast(ri, i, 0xB);
                self.mc2_rival_approach(ri, i, cx, cy, 256, 2048);
                if self.g.ent[i].act_life >= self.g.ent[i].max_life as i32 {
                    self.mc2_rivals[ri].state = Mc2AiState::Fresh;
                }
            }
            // Cruise (sub_13270 EF:5680-5740): a Perception-rolled
            // Fool's-Mana lure when not hovering over the own castle
            // (the tier pick carries retail's verbatim SpellIndex[2]
            // quirk — a single probe at the CASTLE spell's level,
            // EF:5698/5705); else keep the speed-up boost topped
            // (readiness has no cooldown for 3 — the live window is
            // the only re-cast gate); else plain cruise at minSpeed.
            Mc2AiState::Cruise => {
                let me = self.mc2_rivals[ri].ent;
                // ⭐ THE CRUISE LURE ROLL IS THE **GLOBAL** WATCOM
                // STREAM TOO, and it is the LAST of `sub_13270`'s
                // three siblings still on the entity LCG. EF:5691-94
                // verbatim:
                //     v1 = a1x->dword_0xA4_164x->word_0x244_580;
                //     v2 = 0;
                //     if (watcomrand() % 255 < v1)
                // — the same CRT `rand()` as the 57-break above
                // (EF:6547) and the anti-rebound plan roll below
                // (EF:7212), and as MC1's :19507-08. It is drawn
                // UNCONDITIONALLY on entry, so `ent_rand` here steps a
                // GRADED lane (`EntObsMc2::rand`) once EVERY tick a
                // rival cruises, and every later `rand` comparison on
                // that wizard shatters even when the lure decision
                // agrees.
                if ((self.g.watcom_rand() % 255) as u16) < self.mc2_rivals[ri].per {
                    let over_castle = self.rival_castle(me).is_some_and(|c| {
                        let (ex, ey) = (self.g.ent[i].x, self.g.ent[i].y);
                        let e = &self.g.ent[c];
                        ((ex.wrapping_sub(e.x) as i16).unsigned_abs()) <= e.f80
                            && ((ey.wrapping_sub(e.y) as i16).unsigned_abs()) <= e.f82
                    });
                    if !over_castle {
                        let quirk_tier = self.mc2_rivals[ri].book.levels[2] as i16;
                        if self.mc2_rival_tier_probe(ri, quirk_tier, 0x16) == 0x16
                            && self.mc2_rival_cast(ri, i, 0x16)
                        {
                            return;
                        }
                    }
                }
                let m3 = self.mc2_rivals[ri].book.ent[3] as usize;
                let boosted = m3 != 0 && self.g.ent[m3].f26 > 0;
                if !boosted && self.mc2_rival_cast_ready(ri, 3) {
                    self.mc2_rival_cast(ri, i, 3);
                    return;
                }
                if !boosted {
                    self.mc2_rivals[ri].vdes = self.g.ent[i].f128;
                }
            }
            // Defense (sub_161A0 EF:7724): the metamorph DISGUISE
            // posture — refresh the cast, shadow the anchor creature
            // at z+512 (retail's duplicated climb-step block = two
            // steps/tick), tier-0 heading wiggle, engage a mid-band
            // (0xA00..0x1400) wizard, disguise-scaled speed. No
            // internal band exit — the selector owns leaving the
            // state. The disguise VISUAL (drawing the anchor model in
            // place of the carpet) is presentation-side, APPROX
            // unported — but the CLOAK is not: the metamorph window
            // this state refreshes raises the caster's scan-invisible
            // `byte[0] & 0x20` (`sub_6A030` EF:56335), which is
            // modelled in [`Self::mc2_rival_emit`]'s `s == 4` arm.
            Mc2AiState::Defense => {
                let (tx, ty, tz) = match self.mc2_rivals[ri].target {
                    ht if let Some(hx) = self.g.hx_of_tag(ht) => self.human_pose(hx),
                    t => {
                        let e = &self.g.ent[t as usize];
                        (e.x, e.y, e.z)
                    }
                };
                // Refresh the disguise (readiness gates: window clear,
                // the 300-tick cooldown, mana, the 0xE3 cone).
                self.mc2_rival_cast(ri, i, 4);
                // Shadow the anchor (EF:7748-71 — the z block runs
                // twice verbatim).
                self.mc2_rival_face(i, tx, ty);
                let step = BEHAVIOR[self.g.ent[i].row156 as usize].v_14.abs().max(1);
                for _ in 0..2 {
                    let want = tz.saturating_add(512);
                    let e = &mut self.g.ent[i];
                    if e.z < want {
                        e.z = e.z.saturating_add(step);
                    } else if e.z > want {
                        e.z = e.z.saturating_sub(step);
                    }
                }
                // Tier-0 (bird) heading wiggle: two LCG draws in
                // retail order (EF:7774-80).
                let m4 = self.mc2_rivals[ri].book.ent[4] as usize;
                let tier0 = m4 != 0 && self.g.ent[m4].f71 == 0;
                if tier0 {
                    let r1 = self.g.ent_rand(i);
                    let v5 = 2 * ((r1 % 0x9D) / 79); // {0, 2}
                    let r2 = self.g.ent_rand(i);
                    let jink = (v5 as i32 - 1) * (r2 % 0x55) as i32;
                    let e = &mut self.g.ent[i];
                    e.f34 = (e.f34 as i32 + jink) as u16 & 0x7FF;
                }
                // Wizard rescan: the mid-band flips to an engage —
                // retarget the WIZARD + attack pick (EF:7799-7809).
                let wiz = self.mc2_nearest_wizard(ri, i);
                let mut in_band = false;
                if let Some((wt, _, wd)) = wiz {
                    // file 0x3ab68 `cmp $0x1400,%esi; jae` (exact) and
                    // 0x3ab74 `cmp $0xa00,%esi; jbe` on the TRUNCATED
                    // root — the near edge is `D >= 2561²`, not
                    // `D > 2560²` (EF:7799-7801).
                    if wd > mc2_d3_gate(0xA00) && wd < mc2_d3_gate(0x1400) {
                        in_band = true;
                        self.mc2_set_rival_state(ri, Mc2AiState::Defense, wt);
                        self.mc2_rivals[ri].vdes = 0;
                        // ⭐ THE BRAKE WORD IS THE SECOND HALF OF THIS
                        // WRITE. EF:7804-05 is the PAIR
                        // `speed_0xc_12 = 0; word_0xe_14 = 1;` — and
                        // `sub_161A0` never calls `sub_14C90`, so this
                        // is the ONLY site that raises the brake in
                        // Defense. NETHERW.EXE 0x3abbf/0x3abcb:
                        // `movw $0x0,0xc(%edx)` then `movw $0x1,0xe(%edx)`.
                        // `word_0xe_14` is what the SPEED token reads at
                        // its own slot later in the frame
                        // (`GetScroll_69DB0` EF:56216-19): brake set ->
                        // `word_0x2E_46 = 1`, the body skipped, the
                        // decrement lands on 0 and the expiry arm pays
                        // the `minSpeed * sign` restore. Without it the
                        // port ran the token's full body on a tick
                        // retail refuses — mc2l22 pair 1675->1676,
                        // rival 503 in Defense with token 507
                        // (subSpellIndex 2, minSpeed 80): retail
                        // collapses the window 190 -> 0 and restores
                        // actSpeed 80; the port kept 189 and stamped
                        // 80 * 2 = 160.
                        self.mc2_rivals[ri].v14 = BrakeWord(true);
                        if let Some(s) = self.mc2_rival_attack_pick(ri, true) {
                            if self.mc2_rival_cast(ri, i, s) {
                                return; // steer + selector still run
                            }
                        }
                    }
                }
                if !in_band {
                    // Face + z-track the wizard (overrides the anchor
                    // heading, EF:7815-40); none → the NULL record's
                    // position — see [`mc2_ivt_ghost`] (default: the IVT
                    // words retail actually reads; `MGC_NO_MC2_IVT_GHOST`:
                    // the remc2 //fix's authored origin).
                    let (wx, wy, wz) = wiz.map(|(_, p, _)| p).unwrap_or_else(mc2_ivt_ghost);
                    self.mc2_rival_face(i, wx, wy);
                    let want = wz.saturating_add(512);
                    let e = &mut self.g.ent[i];
                    if e.z < want {
                        e.z = e.z.saturating_add(step);
                    } else if e.z > want {
                        e.z = e.z.saturating_sub(step);
                    }
                }
                // Disguise-scaled speed (EF:7842-49): the tier-0 bird
                // flies 3x minSpeed, tiers 1/2 plain minSpeed.
                //
                // ⭐ IT IS THE **ACTUAL** SPEED, NOT THE SETPOINT.
                // EF:7848 is `a1x->actSpeed_0x82_130 = v13` on the
                // ENTITY — the same lane `mc2_rival_weave` already
                // pulses directly — where the port wrote `vdes`
                // (`speed_0xc_12`, the AI's commanded speed). The two
                // are not interchangeable here: the mid-band engage
                // arm three statements above sets `speed_0xc_12 = 0`
                // and `word_0xe_14 = 1` (EF:7804-05) and retail LEAVES
                // that 0 standing while stamping 240 on the entity, so
                // the port's write silently undid the brake it had
                // just armed and then crawled the real speed back at
                // the servo's 16/tick.
                //
                // ⭐ AND THE WRITE IS GATED ON OWNING THE TOKEN AT ALL.
                // `v14x = sub_146C0(a1x, 4u)` (EF:7772) and EF:7842 is
                // `if (v14x)`: a rival with no metamorph manifestation
                // takes NO speed write on this path. The port's `else`
                // stamped plain minSpeed for it.
                //
                // mc2l6-rsg t=4477: rival 370 in Defense, cmd_speed 0
                // and brake 1, `minSpeed` 80 — retail's entity speed
                // steps 0 → 240 in the one tick.
                if m4 != 0 {
                    let min = self.g.ent[i].f128;
                    self.g.ent[i].f126 = if tier0 { min.saturating_mul(3) } else { min };
                }
            }
        }
    }

    /// The combat whiff weave (sub_13890 EF:5980-6034), WIZARD
    /// targets only: tick 0 rolls the committed direction on the
    /// entity LCG and snaps the ACTUAL yaw ±512; ticks 1-2 jink the
    /// setpoint ±512 and pulse the actual speed to
    /// 3·minSpeed·Reflexes/255; ticks 3..19 coast; 20 restarts.
    fn mc2_rival_weave(&mut self, ri: usize, i: usize) {
        let cnt = self.mc2_rivals[ri].weave;
        match cnt {
            0 => {
                let r = self.g.ent_rand(i);
                let dir = if (r % 255) >= 127 { 2u8 } else { 1 };
                self.mc2_rivals[ri].weave_dir = dir;
                let e = &mut self.g.ent[i];
                e.f30 = if dir == 2 {
                    e.f30.wrapping_add(512) & 0x7FF
                } else {
                    e.f30.wrapping_sub(512) & 0x7FF
                };
                self.mc2_rivals[ri].weave = 1;
            }
            1..=2 => {
                let jink: i32 = if self.mc2_rivals[ri].weave_dir == 1 {
                    -512
                } else {
                    512
                };
                let refl = self.mc2_rivals[ri].refl as i32;
                let e = &mut self.g.ent[i];
                e.f34 = ((e.f34 as i32 + jink) & 0x7FF) as u16;
                e.f126 = (3 * e.f128 as i32 * refl / 255) as i16;
                self.mc2_rivals[ri].weave = cnt + 1;
            }
            3..=19 => self.mc2_rivals[ri].weave = cnt + 1,
            _ => self.mc2_rivals[ri].weave = 0,
        }
    }

    /// Shared travel helper (sub_14C90 EF:6713): inside arriveR ->
    /// stop, done; else min speed, and beyond boostR cast the
    /// speed-up (spell 3 — the MC2 remap).
    /// sub_14C90 (EF:6713-53). The brake word `v14` is cleared FIRST
    /// on every call (EF:6718); the arrive branch sets speed 0 +
    /// brake (EF:6733-35); beyond the boost ring with SPEED owned,
    /// the cast fires on a free window and the flight target is
    /// LEFT ALONE (the token writes it at its own slot); otherwise
    /// speed = minSpeed + brake (EF:6751-52).
    fn mc2_rival_approach(
        &mut self,
        ri: usize,
        i: usize,
        tx: u16,
        ty: u16,
        arrive: i32,
        boost: i32,
    ) -> bool {
        self.mc2_rivals[ri].v14 = BrakeWord(false);
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        // ⭐⭐ THE RING IS COMPARED IN LINEAR UNITS, NOT SQUARED ONES.
        // `sub_14C90` (EF:6713) tests `EuclideanDistXYZ_58490(self,
        // target)` against a3/a4 raw — and that function
        // (Maths:738-745) ends in `sub_7277A_radix_3d`, the FLOOR
        // integer sqrt. Squaring the radius instead loses the whole
        // band `[R², (R+1)²)`, where retail's floor still reads R:
        // the port's ring is up to one unit tight, every tick, on
        // every approach.
        //
        // It bites exactly as rarely as that sounds and exactly as
        // hard. mc2l6 t=623, rival 383 attacking the human at ring
        // 3328: dx -1904, dy -2730, d² = 11,078,116, which sits 2,532
        // above 3328² and 4,124 below 3329². Retail's isqrt reads
        // 3328, ARRIVES, brakes to speed 0, resets the weave 20 -> 0
        // and runs the combat z-track (914 -> 908); the port read
        // "not arrived", kept minSpeed 80, held weave at 20 and
        // skipped the z leg (914 -> 912) — the graded `(3,1) slot 383
        // z` head, and three ungraded wiz lanes with it.
        //
        // ⚠ NOT the same function as the Home handler's
        // `EuclideanDistXY_584D0` twelve lines up: that one returns
        // the RAW dist² and is compared against a squared constant
        // (0x6400000). Retail uses both, and which one a site calls
        // is the law.
        let d = crate::mc2::morph::dist2d(px, py, tx as i32, ty as i32);
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
        if d <= arrive {
            self.mc2_rivals[ri].vdes = 0;
            self.mc2_rivals[ri].v14 = BrakeWord(true);
            return true;
        }
        let m3 = self.mc2_rivals[ri].book.ent[3] as usize;
        if d > boost && self.mc2_rival_cast_ready(ri, 3) {
            // ⭐⭐⭐ THE LIVE-WINDOW REFUSAL IS A GATE **INSIDE** THE
            // BOOST ARM, AND ITS ELSE IS EMPTY. Retail's branch
            // selector is `if (v6 > a4 && sub_15170(a1x, 3u))`, and
            // `sub_15170`'s case 3 (EF:7050-63) checks the
            // manifestation, the `maxManaLimit` ceiling and the purse
            // — NOT the window; the window test is the separate
            // `if (!sub_156F0(a1x, 3u)) sub_14E10(a1x, 3u)` INSIDE
            // that arm (EF:6729-31/6744-46). So a rival already
            // boosting takes the boost arm, casts nothing, AND DOES
            // NOT BRAKE.
            //
            // Folding the window into the selector is what sent a
            // boosting rival down the brake else — and the brake word
            // is exactly what makes the speed token collapse itself
            // (`f26 = 1` then the decrement, EF:56218), so the port
            // pumped: cast, brake, collapse, cast, brake… mc2l6
            // t=764, rival 383 running home at ring 2048: retail
            // holds `cmd_speed` 160 with `cooldown[3]` counting 32 →
            // 31 → 30 and the token's `word_0x2E_46` walking 301 →
            // 299, where the port alternated 240/80 and re-stamped
            // the cooldown every other tick (`speed` retail 160 port
            // 80 — the graded head).
            if m3 == 0 || self.g.ent[m3].f26 <= 0 {
                self.mc2_rival_cast(ri, i, 3);
            }
        } else {
            self.mc2_rivals[ri].vdes = self.g.ent[i].f128;
            self.mc2_rivals[ri].v14 = BrakeWord(true);
        }
        false
    }

    fn mc2_rival_face(&mut self, i: usize, tx: u16, ty: u16) {
        let (px, py) = (self.g.ent[i].x, self.g.ent[i].y);
        self.g.ent[i].f34 = Gen::angle_between(px, py, tx, ty);
    }

    /// Combat hover toward target z + 512 by the row's v_14 step.
    fn mc2_rival_hover(&mut self, i: usize, tz: i16) {
        let row = &BEHAVIOR[self.g.ent[i].row156 as usize];
        let step = row.v_14.abs().max(1);
        let e = &mut self.g.ent[i];
        if e.z < tz {
            e.z = e.z.saturating_add(step);
        } else if e.z > tz {
            e.z = e.z.saturating_sub(step);
        }
    }

    /// The attack-spell picker (sub_15790 EF:7175 wizard /
    /// sub_15910 EF:7246 castle): the poverty hysteresis, the
    /// anti-rebound lightning preference, then the priority walk
    /// with the per-spell TIER-DOWN (sub_15F20) — every spell is
    /// probed at every tier and a refused/unaffordable tier just
    /// keeps walking. The winning probe leaves the manifestation
    /// retuned to the passing tier; the caller's cast fires at it.
    /// There is no "affordable by ceiling → save up and WAIT" hold.
    pub(crate) fn mc2_rival_attack_pick(&mut self, ri: usize, vs_wizard: bool) -> Option<usize> {
        // The poverty latch (EF:7190-7205): enter under maxMana/4;
        // release at maxMana/4 + 6000, clamped to maxMana/2 ONLY
        // when the sum overshoots the ceiling (NOT an unconditional
        // min — that would be wrong for mid wealth).
        {
            let r = &mut self.mc2_rivals[ri];
            if r.mana < (r.mana_max / 4) as i32 {
                r.poverty = true;
            } else if r.poverty {
                let mut release = r.mana_max / 4 + 6000;
                if release >= r.mana_max {
                    release = r.mana_max / 2;
                }
                if r.mana >= release as i32 {
                    r.poverty = false;
                }
            }
            if r.poverty {
                return None;
            }
        }
        let walk = |w: &mut Self, s: usize| -> bool {
            let mut tier = w.mc2_rivals[ri].book.levels[s] as i16;
            while tier >= 0 {
                if w.mc2_rival_tier_probe(ri, tier, s) == s as i32 {
                    return true;
                }
                tier -= 1;
            }
            false
        };
        if vs_wizard {
            // Anti-rebound (EF:7209-19): a target visibly holding a
            // live rebound (8) prefers a lightning (7) tier-walk,
            // Perception% of the time.
            let target_buffed = match (
                self.mc2_rivals[ri].target,
                self.hx_of_tag(self.mc2_rivals[ri].target),
            ) {
                (_, Some(hx)) => self.players[hx].rebound,
                (t, None) => self
                    .mc2_rivals
                    .iter()
                    .find(|r| r.ent == t)
                    .is_some_and(|r| r.rebound),
            };
            if target_buffed {
                // ⭐ THE ROLL IS THE **GLOBAL** WATCOM STREAM, NOT THE
                // WIZARD'S ENTITY LCG. EF:7212 is literally
                // `if (watcomrand() % 255 < v5)` — the same CRT
                // `rand()` the MC1 column has drawn here since its own
                // t=2788 head (`mc1/rivals.rs`, :19507-08). Spending
                // `ent_rand` instead steals a step from a GRADED lane,
                // so every later `rand` comparison on that wizard
                // shatters even when the pick itself agrees.
                //
                // mc2l6-rsg t=4559: rival 378 attacks the human, who
                // is holding a live Rebound, and casts spell 0 either
                // way — retail's entity `rand` stands at 60615 across
                // the tick while the port's stepped to 2566.
                //
                // ⚠ `crt_rand`'s phase has no capture channel, so the
                // roll's OUTCOME is best-effort here; keeping the
                // entity stream honest is the part that is measurable,
                // and it is the part that propagates.
                //
                // ⭐ MC1's twin site was called unrecoverable for the
                // same reason and was NOT: see
                // `mc1::rivals::mc1_crt_draw_at_label49`. The phase
                // was never the problem — a phantom draw at the wrong
                // site was, and the seed is Watcom's default 1. MC2 is
                // worth the same treatment: enumerate ITS draw sites
                // (`watcomrand` in EF), check each against the
                // decompile's short-circuits, then fit the outcome on
                // recorded casts.
                let roll = (self.g.watcom_rand() % 255) as u16;
                if roll < self.mc2_rivals[ri].per && walk(self, 7) {
                    return Some(7);
                }
            }
            let target_is_wizard = (self.g.is_human_tag(self.mc2_rivals[ri].target))
                || self
                    .g
                    .ent
                    .get(self.mc2_rivals[ri].target as usize)
                    .is_some_and(|e| e.class64 == 3 && e.model65 <= 1);
            for &s in &ATTACK_WIZARD {
                // Spell 0x13 only against a wizard body (EF:7230-37).
                if s == 0x13 && !target_is_wizard {
                    continue;
                }
                if walk(self, s as usize) {
                    return Some(s as usize);
                }
            }
        } else {
            for &s in &ATTACK_CASTLE {
                if walk(self, s as usize) {
                    return Some(s as usize);
                }
            }
        }
        None
    }

    // ---- the cast arm (readiness sub_15170 EF:6887 + executor
    // ---- sub_14E10 EF:6759) -----------------------------------------------

    /// Readiness `sub_15170` (EF:6888-7095) — the per-spell-CLASS
    /// gate table. Common to every class: owned + the tier's ceiling
    /// unlock (maxMana >= maxManaLimit) + affordable now (the castle
    /// reads the ladder fresh). Per class:
    /// - {0,7,0xD,0xE,0x16}: cooldown + the Perception cone;
    /// - {1,9,0x10,0x12,0x13,0x15}: + the armed-window refusal;
    /// - 2 with a castle: armed + cooldown + cone + the space check
    ///   (sub_11A10); without one: cooldown only — the first castle
    ///   is aim-free (EF:7046-49);
    /// - 3 speed-up: NO cooldown check at all (EF:7051-63);
    /// - {4,6,8,0xB} buff/self: armed + cooldown, no cone (EF:7078);
    /// - the rest (5,0xA,0xC,0xF,0x11,0x14,0x17,0x18,0x19): cooldown
    ///   only (the LABEL_43 generic arm).
    ///
    /// The cone is yaw-vs-setpoint (`sub_582B0(yaw, roll)` — the
    /// state handlers keep f34 on the target): (255-P)/4+20 degrees.
    fn mc2_rival_cast_ready(&mut self, ri: usize, s: usize) -> bool {
        let r = &self.mc2_rivals[ri];
        let m = r.book.ent[s] as usize;
        if m == 0 {
            return false;
        }
        let e = &self.g.ent[m];
        if (r.mana_max as i64) < e.f136 as i64 {
            return false; // the maxManaLimit ceiling gate
        }
        // ⭐⭐⭐ AND THE CASTLE READS THE **STAMPED** PRICE, LIKE EVERY
        // OTHER CLASS. Every arm of `sub_15170` spends the SAME
        // expression — `a1x->mana_0x90_144 < v<n>x->maxMana_0x8C_140`
        // — on the manifestation the book handed it, case 2 included
        // (EF:7035 with a castle, EF:7046 without). The token's
        // `@0x8C` is whatever `SetSpell_6D5E0` last wrote, and the ONE
        // caller that matters here is `sub_15F20`, which stamps the
        // tier's price IMMEDIATELY BEFORE calling this gate
        // (EF:7595-96) — so the price under test is the ladder rung
        // TIMES THE TIER MULTIPLIER, never the bare rung. Recomputing
        // it from the castle level dropped the ×320>>8 / ×384>>8 and
        // priced every tier-1/2 probe at the tier-0 rung, so the
        // tier-down walk stopped one or two rungs too high and left
        // the token re-priced there.
        // mc2l22 t=10495: rival 611's Upgrade walk starts at tier 2
        // over its level-1 castle 637; retail's gate reads the freshly
        // stamped 15000 against a purse under it, fails tier 2 and
        // tier 1, and casts at tier 0 — leaving token 15 at `mana_max`
        // 10000 / `mana` 99. The port read the bare rung 10000, passed
        // at tier 2 and left 15000 / 148. Same row at t=21772 (rival
        // 584, token 46) and t=28428 (rival 557, token 6, rung 4:
        // retail 80000 / 792, port 120000 / 1188).
        let cost = e.max_life as i64;
        // The affordability gate sits BEFORE the dispatch for every
        // spell but the castle: retail's case-2 kernel runs its
        // `mana >= manifestation cost` test AFTER the sub_11A10 space
        // probe (EF:7028-41), so a broke rival still stamps the
        // castle's quad — the case-2 arm gates itself below.
        if s != 2 && (r.mana as i64) < cost {
            return false;
        }
        let armed = e.f26 > 0;
        let cooling = r.cooldown[s] != 0;
        let cone_ok = {
            let cone = ((255 - r.per as u32) / 4 + 20) * 2048 / 360;
            let e = &self.g.ent[r.ent as usize];
            (Gen::angdist(e.f30, e.f34) as u32) < cone
        };
        let ent = r.ent;
        match s {
            0 | 7 | 0xD | 0xE | 0x16 => !cooling && cone_ok,
            1 | 9 | 0x10 | 0x12 | 0x13 | 0x15 => !armed && !cooling && cone_ok,
            2 => match self.rival_castle(ent) {
                // The space probe (sub_11A10 — it QUAD-STAMPS the
                // castle) runs as soon as the armed/cooling gates
                // pass, BEFORE the mana/cone gates, and retail has NO
                // action gate here (EF:7028-41): the probe fires on a
                // RISING castle too (mc2l4 t=1 castle 304's
                // yaw-0/fov-256 stamp is exactly this probe).
                Some(c) => {
                    !armed
                        && !cooling
                        && self.g.mc2_castle_space_ok(c)
                        && self.mc2_rivals[ri].mana as i64 >= cost
                        && cone_ok
                }
                None => !cooling && self.mc2_rivals[ri].mana as i64 >= cost,
            },
            3 => true,
            4 | 6 | 8 | 0xB => !armed && !cooling,
            _ => !cooling,
        }
    }

    #[cfg(test)]
    pub(crate) fn mc2_rival_cast_ready_for_test(&mut self, ri: usize, s: usize) -> bool {
        self.mc2_rival_cast_ready(ri, s)
    }

    /// `sub_15F20` (EF:7581-7611) — the tier-down probe: retune the
    /// manifestation to `tier` FIRST (retail's SetSpell side effect
    /// happens even when the probe then fails — a live window queues
    /// via f44 instead), then the readiness gate + the tier's raw
    /// table costs. Returns the spell id when castable at this tier,
    /// -1 when only mana blocks it, 0 otherwise.
    fn mc2_rival_tier_probe(&mut self, ri: usize, tier: i16, s: usize) -> i32 {
        let Some(row) = self.g.assets.spells.get(s).copied() else {
            return 0;
        };
        if tier < 0 || row.byte_0 as i16 <= tier {
            return 0;
        }
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        if m == 0 {
            return 0;
        }
        let own = self.mc2_rivals[ri].ent;
        self.mc2_rival_set_spell(m, tier as u8, own);
        if !self.mc2_rival_cast_ready(ri, s) {
            return 0;
        }
        let sub = row.tiers[(tier as usize).min(2)];
        let r = &self.mc2_rivals[ri];
        if (r.mana_max as i64) < sub.max_mana_limit as i64 || (r.mana as i64) < sub.mana_cost as i64
        {
            return -1;
        }
        s as i32
    }

    /// The tier-down cast walk every retail pick site shares
    /// (`for k = SpellLevels[s]; k >= 0; k--` + cast on the first
    /// passing tier — EF:5470/5591/5759/5840...).
    pub(crate) fn mc2_rival_walk_cast(&mut self, ri: usize, i: usize, s: usize) -> bool {
        let mut tier = self.mc2_rivals[ri].book.levels[s] as i16;
        while tier >= 0 {
            if self.mc2_rival_tier_probe(ri, tier, s) == s as i32 {
                return self.mc2_rival_cast(ri, i, s);
            }
            tier -= 1;
        }
        false
    }

    /// ⭐⭐⭐ THE ONE SITE THAT CHAINS THE EXECUTOR INTO THE LOOP
    /// CONDITION. `sub_133B0`'s spell-1 walk is
    /// `if (sub_15F20(a1x, k, 1) == 1 && sub_14E10(a1x, 1u)) goto
    /// LABEL_6;` (EF:5804) — a REFUSED cast falls through to `k--`
    /// and the walk keeps descending, so the probe's `SetSpell` side
    /// effect runs again at every lower tier and the token ends the
    /// tick re-priced at tier 0, not at the tier the refusal happened
    /// on. [`Self::mc2_rival_walk_cast`] returns the refusal instead
    /// and leaves the stamp behind.
    ///
    /// The old shared-helper comment argued the two agree because
    /// every refusal `sub_14E10` can raise here (the 0xAA cone, the
    /// burst lockout) is tier-INDEPENDENT. That is true of the CAST
    /// outcome and false of the TOKEN'S PRICE STAMP, which is the
    /// graded lane: `mana_0x90_144` / `maxMana_0x8C_140` on the
    /// class-15 record.
    ///
    /// WITNESS mc2l12, rival 136's spell-1 token slot 138: retail
    /// holds 33/100 (tier 0) and flashes 6/250 (tier 1) for exactly
    /// the one tick the cast lands (t=1003, 1013, ... 1870). The port
    /// held 6/250 for every tick the executor refused — t=1000-1002
    /// (the 0xAA cone), 1288-89, 1331-37 and 1830-37 (`burst` negative
    /// after the 8-shot lockout), 1749 — which is 21 ticks and
    /// exactly the take's 21 `(15,1)` heads, first at the horizon
    /// t=1000.
    ///
    /// See [`no_mc2_rival_home_cast_chain`] for the EXE bytes and for
    /// the two near-identical siblings that do NOT chain.
    pub(crate) fn mc2_rival_walk_cast_chained(&mut self, ri: usize, i: usize, s: usize) -> bool {
        if no_mc2_rival_home_cast_chain() {
            return self.mc2_rival_walk_cast(ri, i, s);
        }
        let mut tier = self.mc2_rivals[ri].book.levels[s] as i16;
        while tier >= 0 {
            if self.mc2_rival_tier_probe(ri, tier, s) == s as i32 && self.mc2_rival_cast(ri, i, s) {
                return true;
            }
            tier -= 1;
        }
        false
    }

    /// The commit (sub_14E10 EF:6759): burst gun on the precision
    /// family, aim pitch at the target, arm the recast cooldown,
    /// debit through the regen delta, emit through the shared MC2
    /// class-9 spawners. Spell 0xF is never AI-cast (EF:6885).
    pub(crate) fn mc2_rival_cast(&mut self, ri: usize, i: usize, s: usize) -> bool {
        if s >= MC2_SPELLS || s == 0xF {
            return false;
        }
        if !self.mc2_rival_cast_ready(ri, s) {
            return false;
        }
        // ⭐⭐⭐ THE CAST CONE IS `yaw_0x1C` vs THE **STORED** AIM
        // REGISTER `roll_0x20`, NOT A FRESH BEARING TO THE TARGET.
        // Both aiming arms of `sub_14E10` read exactly
        // `sub_582B0(a1x->yaw_0x1C_28, a1x->roll_0x20_32)`
        // (EF:6801 for {0,1,7,0x16} at 0xAA, EF:6845 for
        // {4,9,0xD,0xE,0x12,0x13,0x15} at 0xE3) — the same roll/fov
        // desired-aim pair the steering writes, one statement per
        // handler, and NEVER an on-the-spot `angle_between`. The port
        // recomputed the bearing from its own steer target, which is a
        // different number the moment the cast site runs BEFORE the
        // handler's own aim stamp.
        //
        // mc2l6-rsg t=1130 is the witness and the shape is session
        // 78's law D again — a STORED register read as a fresh
        // recompute. Rival 370 is in Home with `word_0x96_150` = its
        // own castle (`sub_14630` EF:6394-96 stamps the castle as the
        // target), and `sub_133B0`'s opportunistic spell-1 cast fires
        // ABOVE the `sub_14C60` block that re-stamps `roll` — so
        // retail tests yaw 254 against roll 254, angdist 0, casts, and
        // `goto LABEL_6` returns past the approach so `roll` is never
        // re-stamped that tick. The port aimed a fresh 1596 at the
        // castle, read angdist 706, refused at the 0xAA gate, fell
        // through to the approach and stamped roll 1596 — one missing
        // (10,0) at slot 833 plus the whole `roll` lane.
        // ⭐ `mc2_rival_cast_ready`'s own cone already read the pair
        // correctly (`f30` vs `f34`); this was the second copy.
        let (yaw, aim) = {
            let e = &self.g.ent[i];
            (e.f30, e.f34)
        };
        match s {
            // Precision-aimed burst family {0,1,7,0x16} (EF:6797):
            // cone 0xAA, the shared burst counter.
            0 | 1 | 7 | 0x16 => {
                if self.mc2_rivals[ri].burst < 0 || Gen::angdist(yaw, aim) >= 0xAA {
                    return false;
                }
                // EF:6810 — the aim pitch is written onto the CASTER,
                // from the entity's OWN f146 target (see
                // [`Self::mc2_rival_aim_pitch`]); the write precedes
                // the fire, so even a whiffed cast leaves the stamp.
                self.g.ent[i].f32 = self.mc2_rival_aim_pitch(i);
                self.mc2_rivals[ri].burst += 1;
                if self.mc2_rivals[ri].burst >= 8 {
                    self.mc2_rivals[ri].burst =
                        ((self.mc2_rivals[ri].refl as i32 - 255) / 8 - 1) as i16;
                }
            }
            // Homing-aimed {4,9,0xD,0xE,0x12,0x13,0x15} (EF:6841):
            // the wider 0xE3 cone, the same caster pitch write
            // (EF:6860).
            4 | 9 | 0xD | 0xE | 0x12 | 0x13 | 0x15 => {
                if Gen::angdist(yaw, aim) >= 0xE3 {
                    return false;
                }
                self.g.ent[i].f32 = self.mc2_rival_aim_pitch(i);
            }
            // Every other case fires with the caster's standing
            // pitch — retail writes NO pitch outside the two aiming
            // families (EF:6870+).
            _ => {}
        }
        // Create Castle routes to the build/upgrade arm (case 2,
        // EF:6820 — the same body as the human's).
        if s == 2 {
            return self.mc2_rival_cast_castle(ri, i);
        }
        // ⭐⭐⭐ THE EXECUTOR HAS ITS OWN WINDOW GATE, AND IT IS NOT THE
        // READINESS TABLE'S. `sub_14E10` (EF:6759) runs `sub_15170`
        // first, then hands EVERY case to `sub_5F660(a1x, tok, 0)`
        // (EF:6816/6826/6844/6861/6875) and stamps the recast cooldown
        // ONLY when that returned 1 (`!= 1 → return 0`). `sub_5F660`'s
        // per-model switch (EF:60889-60948) refuses a LIVE window
        // before the mana test / `sub_5F7B0` re-arm:
        //   case 0   : tier >= 2 && window      → LABEL_16 (refuse)
        //   case 7   : tier >= 1 && window      → LABEL_23 (refuse)
        //   4/6/8/0xB/0xC/0xE, rival caster     → LABEL_16 (refuse)
        //   9/0xA/0xD/0xF/0x10..0x18 (LABEL_16): `if (word_0x2E_46)
        //                                         goto LABEL_23`
        //   3 (speed), 5 (heal)                 → default: re-arm freely
        // The human column has carried exactly this switch since the
        // cast-gate dig (`mc2_cast_gate`, cast.rs); the rival funnel
        // only had the READINESS copy (`sub_15170`'s armed leg), so
        // every spell in `sub_15170`'s generic arm with a 0 recast
        // cooldown — 0x11 EARTHQUAKE, 0x14, 0x16, 0x17, 0x18, 0xA, 0xC,
        // 0xD, and fireball T3 / lightning T2+ — re-armed and RE-FIRED
        // every tick its window was live. mc2l22 t=2512→2513: rival
        // 584's earthquake token 602 reads window 1/21 in retail and
        // 20/21 in the port, the port mints a (9,2) at 988 that retail
        // does not, and the whole free stack shifts by one for 733
        // ticks. A refused executor casts NOTHING this tick — no
        // cooldown, no debit, no fire — exactly `sub_14E10`'s
        // `return 0`; the aim-pitch/burst writes above it stand
        // (EF:6810 precedes EF:6816).
        {
            let m = self.mc2_rivals[ri].book.ent[s] as usize;
            let armed = m != 0 && m < self.g.ent.len() && self.g.ent[m].f26 > 0;
            let tier = if m != 0 && m < self.g.ent.len() {
                self.g.ent[m].f71
            } else {
                0
            };
            let refused = armed
                && match s {
                    0 => tier >= 2,
                    7 => tier >= 1,
                    4 | 6 | 8 | 0xB | 0xC | 0xE => true,
                    9 | 0xA | 0xD | 0xF | 0x10..=0x18 => true,
                    _ => false,
                };
            if refused {
                return false;
            }
        }
        // Arm the recast cooldown + debit the full tier cost through
        // the regen delta (the chassis mana law).
        //
        // ⚠ THE DEBIT IS NOT PART OF THE ARM. `sub_5F7B0` (EF:60974)
        // only writes `word_0x2E_46 = word_0x30_48` and two flag bits;
        // the payment is each effect body's own first-tick
        // `sub_68DE0`, which lands in the same frame because the token
        // sits above the caster (see the arm note below). Stamping it
        // here is the port's stand-in for that — and it is WRONG for
        // HEAL, the one body that never calls `sub_68DE0` at all
        // (`sub_6A300` bills itself, per HEALING tick). Charging both
        // would double-bill the arm tick.
        self.mc2_rivals[ri].cooldown[s] = AI_RECAST[s];
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        let cost = self.g.ent[m].max_life as i32;
        // ⭐⭐⭐ **WHETHER THE FIRST TICK IS THIS FRAME IS A SLOT
        // COMPARISON, NOT A CONSTANT.** Retail's arm (`sub_5F7B0`
        // EF:60976) writes `word_0x2E_46 = word_0x30_48` and nothing
        // else; the FIRE, the `sub_68DE0` debit and the countdown all
        // live in the token's own action body (`sub_693F0`), which
        // runs at the TOKEN's pool slot. A wizard minted at level load
        // owns slots directly above itself, so the token is dispatched
        // later in the very frame the arm lands — which is the
        // same-frame first tick this arm emulates by pre-consuming.
        // **A RESPAWNED wizard's book is re-minted onto the LOWEST
        // free slots** (SESSION 83 law D: `sub_5C950` opens on
        // `sub_49F90`), so its tokens sit BELOW it and have already
        // been walked this frame — the first tick, the fire and the
        // debit all land on the NEXT one. mc2l6-rsg t=3258: rival 378
        // (respawned at 3084, book at 3/4/6/7) stamps its spell-1 cast
        // — retail's own `burst` and `cooldown[1]` move on that tick —
        // and the (9,1) is not born until t=3259, at slot 39, the slot
        // the port had already spent on a puff a frame earlier.
        let token_later = m > i;
        // ⭐⭐⭐ THE FIRST TICK OPENS ON `sub_68D50`, AND THE ARM DOES NOT.
        // Every manifestation body is `if (sub_68D50(tok, wiz)) { first
        // tick: fire + sub_68DE0 debit } else { word_0x2E_46 = 1 }` —
        // meteor `sub_6AB00` EF:56784-56830, crater `sub_6BAB0`
        // EF:57421-57470, earthquake EF:57494-57540, speed
        // `GetScroll_69DB0` EF:56216. `sub_68D50` (EF:55548) refuses
        // when the token's `manaRegen_0x88_136` — SetSpell's copy of
        // the tier's `maxManaLimit_A` — is nonzero and the caster has
        // no castle or the castle's store is below it. `sub_15170`
        // only tests the WIZARD's maxMana against that ceiling
        // (EF:6982), so retail arms (cooldown stamped, window =
        // duration) and then the token body fires NOTHING, debits
        // NOTHING, collapses the window to 1 → 0, and the brain
        // re-arms next tick — forever. mc2l22 t=5557→5558: rival 530
        // `cooldown[9] 0 → 4`, `d88 1000 → 100`, NO birth in retail;
        // the port fired the (9,3) at 925 and debited 4000.
        // t=6600→6602: rival 611's crater token 194 carries `d88`
        // 150000 and reads window 41/41 on every retail snapshot; the
        // port's `first` fired a (9,5) and debited 9100 EVERY tick and
        // flipped its poverty latch — how a "dumb" rival gets stuck.
        // `mc2_rival_afford` IS `sub_68D50` on this column.
        let afford = self.mc2_rival_afford(ri, m);
        // SPEED (3) is EXCLUDED from the pre-consume: its body runs
        // at the TOKEN's slot (`mc2_rival_manifestation_tick`) in
        // every book layout, and that body pays `sub_68DE0` itself —
        // inside the effect arm, BELOW the `word_0xe_14` collapse, so
        // an avoid-FSM brake raised between this arm and the token's
        // slot (`sub_13710` → `sub_16580`, EF:7936-42) kills the
        // first tick unpaid. Debiting here paid it anyway (mc2l22
        // t=10, rival 451 — see the token body's note).
        // ⭐⭐⭐ …EXCEPT SHIELD III, WHICH DECREMENTS BEFORE IT BILLS.
        // `sub_6A480`'s tier-2 arm runs `word_0x2E_46--` BEFORE
        // `sub_68DE0` (EF:56525-28; `NETHERW.EXE` 0x8ed29 `dec` /
        // 0x8ed34 `call`), so the debit's `word_0x2E_46 ==
        // word_0x30_48` test is FALSE BY CONSTRUCTION and the call
        // falls into the else — the regen pin, keyed on the
        // POST-decrement counter. This is the same-frame first tick
        // the pre-consume above stands in for, so the substitution
        // belongs here too. See [`no_rival_shield3_predecrement`].
        let shield3_predecrement = self.mc2_shield3_predecrement(s, m);
        if s != 3 && s != 5 && token_later && afford {
            if shield3_predecrement {
                // The stand-in's decrement lands the counter on
                // `f28.max(1) - 1`; a non-zero `v2` pins the regen.
                if self.g.ent[m].f28.max(1) > 1 {
                    let r = &mut self.mc2_rivals[ri];
                    if r.mana_delta > 0 {
                        r.mana_delta = 0;
                    }
                }
            } else {
                let r = &mut self.mc2_rivals[ri];
                r.mana_delta = if r.mana_delta >= 0 {
                    -cost
                } else {
                    r.mana_delta - cost
                };
            }
        }
        // Arm the manifestation window (buffs/heal read it).
        //
        // ⭐⭐ THE CAST FRAME **IS** THE WINDOW'S FIRST TICK. Retail's
        // arm is a bare `word_0x2E_46 = word_0x30_48` (`sub_5F7B0`
        // EF:60976) at the CASTER's walk slot — and the token's own
        // pool slot sits ABOVE the wizard's (MC2 mints the wizard then
        // its 26 manifestations: mc2l6's rival 370 owns 371-377), so
        // the class-15 handler runs LATER IN THAT SAME FRAME. It reads
        // `word_0x2E_46 == word_0x30_48`, stamps `sub_68DE0`'s
        // first-tick debit, and decrements. Retail's capture at the
        // cast tick is therefore ALREADY `word_0x30_48 - 1`.
        //
        // The port stamps that first-tick debit right above, but
        // `mc2_rival_buffs` runs at the RIVAL's slot — below every
        // token — so arming the full duration here bought the window
        // one extra tick and shifted the whole mid-burst regen pin a
        // frame late. mc2l6 t=244 fireball (`f30` 5): retail's counter
        // reads 4/3/2/1/0 at t=244..248 and pins `d88` to 0 across
        // 245-248, back to +100 at 249; the port read 5/4/3/2/1/0 and
        // spent t=245 on its own `f26 == f28` first-tick arm, which
        // skips the pin — so rival 370 regenerated 900 -> 1000 at
        // t=246 while retail held 900 (the horizon head).
        //
        // ⚠ SPELL 3 IS EXCLUDED. Speed's body genuinely runs at the
        // token's own slot (`mc2_rival_manifestation_tick`, dispatched
        // from `mc2_manifestation_pass`), so it already gets retail's
        // same-frame first tick — including the one-factor-hotter
        // spike that `first` keys on. Pre-consuming here would eat it.
        let arm = self.g.ent[m].f28.max(1) as i16;
        self.g.ent[m].f26 = arm;
        // ⭐ CASTING BREAKS THE CLOAK. `sub_5F660`'s arm is
        // `sub_5F7B0(token, caster, handbits)` (EF:60974-80) — the
        // very `word_0x2E_46 = word_0x30_48` written above — and it
        // tail-calls `sub_5F7E0` (EF:60983-90), which clears the
        // CASTER's `byte[0] & 0x20` unless the wizard's invisibility
        // strength `byte_0x1BF_447` is >= 3 (or == 2 while the armed
        // token's model is 1). ⚠ APPROXIMATED AS ALWAYS-CLEAR on this
        // column: the strength byte is only ever non-zero while an
        // Invisibility window is live, and Invisibility never raises
        // the wizard's own 0x20 in the first place, so the only cloak
        // the gate could protect is a metamorph one held by a rival
        // simultaneously invisible at tier >= 2.
        // ⭐⭐⭐ …AND THE STRENGTH GATE IS REAL, because
        // [`rival_invis_cloak_edge_off`] gives `byte_0x1BF_447` a
        // value to hold. `sub_5F7E0`, shipped `NETHERW.EXE` 0x83FE0
        // (`%edx` = arg2 = the CASTER, `%eax` = arg1 = the ARMED
        // TOKEN):
        //   83fec  mov  0x1bf(%eax),%al   ; caster wizext strength
        //   83ff2  cmp  $0x2,%al
        //   83ff4  jb   0x84008           ; < 2  -> CLEAR
        //   83ff6  jbe  0x83ffe           ; == 2 -> ask the token
        //   83ff8  cmp  $0x3,%al / je / ret   ; >= 3 -> NO CLEAR
        //   83ffe  mov  0x40(%eax),%al / cmp $0x1 / je  ; model 1 -> NO CLEAR
        //   84008  andb $0xdf,0xc(%edx)   ; the clear
        // ⭐ THE REGISTER NEEDS NO IMPORT SEAT: retail sets it in
        // `sub_6B1C0`'s first-tick arm and zeroes it at expiry, so it
        // is exactly `SPELLS[11].subspell[token.byte_0x46_70]
        // .life_0x1A` while the window is live and 0 otherwise —
        // DERIVED, not stored. (mc2l22 t=28013: rival 557 arms
        // Invisibility TIER 2 and retail's `flags` stay 44 for the
        // whole 183-tick window across every later cast, where
        // rival 530's TIER 1 at t=13105 is uncloaked the very next
        // tick.)
        if !rival_metamorph_cloak_off() {
            let strength = self.mc2_rival_invis_strength(ri);
            let armed_model = self.g.ent[m].model65;
            if rival_invis_cloak_edge_off() || strength < 2 || (strength <= 2 && armed_model != 1) {
                self.g.ent[i].flags &= !0x20;
            }
        }
        // HEAL (5) is EXCLUDED too: its whole body — first tick or
        // not — is `mc2_rival_heal_stand_in`, which runs after this
        // arm in the same frame exactly where retail's token slot
        // would, and decrements for itself. Pre-consuming here made
        // a re-armed window pay two bodies for one `f2e` step.
        if s != 3 && s != 5 && token_later && !afford {
            // The else arm: `word_0x2E_46 = 1`, then the shared
            // decrement lands it on 0.
            self.g.ent[m].f26 = 1;
        }
        // ⚠ THE PRE-CONSUME IS THE STAND-IN'S OWN DECREMENT. With the
        // countdown moved BELOW the brain (see `mc2_rival_buffs`'s call
        // site) the stand-in genuinely runs after this arm in the same
        // frame, exactly where retail's token slot is — so doing it
        // here too would spend the window twice.
        if rival_buff_order_off() && s != 3 && s != 5 && token_later {
            self.g.ent[m].f26 -= 1;
        }
        if s == 6 {
            // A fresh shield starts ARMED (the byte[2] 0x40 stage).
            self.mc2_rivals[ri].shield_state = 1;
        }
        // The fire reads the caster's STANDING pitch (`sub_5F660`
        // consumes `pitch_0x1E` — fresh from the stamp above in the
        // aiming families, untouched otherwise). The old Euclidean
        // `pitch_toward` at the AI brain's target aimed at the WRONG
        // entity on the wrong metric (mc2l1 t=193: f146=141, the
        // mana sphere).
        // ⭐⭐⭐ THE FIRE IS NOT PART OF THE ARM — see
        // [`rival_fire_at_token_slot_off`]. `sub_5F7B0` writes the
        // window and nothing else; `sub_693F0` (EF:55832) spawns off
        // the CASTER's LIVE `yaw_0x1C_28`/`pitch_0x1E_30` at the
        // TOKEN's own pool slot, which for a level-load book is after
        // the caster's steer in the same frame. The deferred fire
        // lands in [`Self::mc2_rival_buffs`], which already runs at
        // that phase (its own call site was moved below the brain for
        // exactly this reason), so the emit reads the POST-steer
        // heading the way retail's does.
        //
        // ⚠ The snapshot above is still right for the CONE — retail's
        // `sub_14E10` tests `sub_582B0(yaw_0x1C, roll_0x20)` at the
        // caster's slot, before the steer. Only the EMIT moved.
        let pitch = self.g.ent[i].f32;
        if token_later && afford && (rival_fire_at_token_slot_off() || rival_buff_order_off()) {
            self.mc2_rival_emit(ri, i, s, yaw, pitch);
        }
        true
    }

    /// A rival token whose pool slot sits BELOW its caster's, run at
    /// its OWN slot — retail's `sub_693F0`/`GetScroll_69DB0` body,
    /// which is where the fire, the `sub_68DE0` debit and the
    /// countdown have always lived. Only a RESPAWN-minted book can be
    /// down here (see the slot note in [`Self::mc2_rival_cast`]); a
    /// level-load book sits above its wizard and keeps running through
    /// [`Self::mc2_rival_buffs`]'s caster-slot stand-in, byte for
    /// byte as before.
    /// The FIRE half of `sub_693F0`'s first-tick arm, run at the
    /// TOKEN's own pool slot for a book that sits ABOVE its wizard —
    /// see [`rival_fire_at_own_token_slot_off`] for the law and its
    /// witness. Dispatched from [`World::mc2_manifestation_pass`].
    ///
    /// The caster-slot stand-in in [`Self::mc2_rival_buffs`] still
    /// owns the debit, the buff publish, the countdown and the
    /// expiry, and it has ALREADY decremented `word_0x2E_46` by the
    /// time the walk reaches this slot — so retail's own first-tick
    /// test `word_0x2E_46 == word_0x30_48` reads one lower here. The
    /// `sub_68D50` gate is re-evaluated on that pre-decrement window,
    /// leg for leg with [`Self::mc2_rival_afford`], so the two paths
    /// admit exactly the same casts.
    pub(crate) fn mc2_rival_token_fire_at_slot(&mut self, s: usize, m: usize, ri: usize) {
        if rival_fire_at_own_token_slot_off()
            || rival_fire_at_token_slot_off()
            || rival_buff_order_off()
            // CASTLE (2) has no fire, and HEAL (5) is excluded from
            // the stand-in's countdown loop entirely.
            || s == 2
            || s == 5
        {
            return;
        }
        // The stand-in only reaches its fire on a window it found
        // LIVE (`word_0x2E_46 > 0`) and on the arm tick
        // (`== word_0x30_48`); post-decrement that is `+ 1`.
        let win = self.g.ent[m].f26 as i32 + 1;
        if win <= 0 || win as u16 != self.g.ent[m].f28.max(1) {
            return;
        }
        let i = self.mc2_rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return;
        }
        // `sub_68D50` (EF:55548-66), on the PRE-decrement window.
        if self.g.ent[i].act_life < 0 {
            return;
        }
        let upkeep = self.g.ent[m].f136;
        if upkeep > 0
            && !self
                .rival_castle(self.mc2_rivals[ri].ent)
                .is_some_and(|c| self.g.ent[c].f140 >= upkeep)
        {
            return;
        }
        if self.mc2_rivals[ri].mana < self.g.ent[m].max_life as i32 {
            return;
        }
        let (yaw, pitch) = {
            let e = &self.g.ent[i];
            (e.f30, e.f32)
        };
        self.mc2_rival_emit(ri, i, s, yaw, pitch);
    }

    /// `sub_6A480`'s arm selector — the spell row's `life_0x1A` for the
    /// token's CURRENT tier (`byte_0x46_70`, the port's `f71`). Row
    /// value 1 is SHIELD III, the arm that decrements `word_0x2E_46`
    /// BEFORE it calls `sub_68DE0` (EF:56525-28). See
    /// [`no_rival_shield3_predecrement`] for the whole law.
    pub(crate) fn mc2_shield3_predecrement(&self, s: usize, m: usize) -> bool {
        s == 6
            && !no_rival_shield3_predecrement()
            && m != 0
            && m < self.g.ent.len()
            && self
                .g
                .assets
                .spells
                .get(s)
                .map(|r| r.tiers[(self.g.ent[m].f71 as usize).min(2)].life)
                == Some(1)
    }

    pub(crate) fn mc2_rival_token_tick(&mut self, s: usize, m: usize, ri: usize) {
        // CASTLE (2) is `sub_5F890`'s upgrade lock, not a timed cast —
        // the same exclusion `mc2_rival_buffs` carries, repeated here
        // because a respawn-minted book reaches its body down THIS
        // path instead.
        if s == 2 {
            return;
        }
        if self.g.ent[m].f26 <= 0 {
            // ⭐⭐⭐ `sub_6AA00`'s `<= 0` ARM IS NOT A RETURN — IT IS
            // THE CLEAR (EF:56728-32):
            //     if (a1x->word_0x2E_46 <= 0)
            //         v1x->…word[0] &= 0x7FEFu;
            //     else { …publish…; if (!--word_0x2E_46) sub_6D880(a1x); }
            // Its sibling `sub_6A480` (SHIELD) is the MIRROR IMAGE —
            // `if (parent <= Entities[0] || word_0x2E_46 <= 0) goto
            // LABEL_15;` (EF:56503, a bare return) with the clear at
            // `LABEL_13: if (!word_0x2E_46) { parent.dword &=
            // 0xFFBFBFFF; sub_6D880(a1x); }` (EF:56533-36). So the
            // REBOUND window survives EXACTLY ONE PASS past the tick
            // its counter hits 0, and the shield does not. The port
            // carried the shield's shape on both.
            // WITNESS mc2l22 t=23732: rival 557's rebound token is
            // slot 12 (class 15, model 8, action 24, owner28 557) and
            // its `word_0x2E_46` runs 251 → 250 → 0 across
            // 23730/23731/23732 (`dump-state <t> 12`, lane `f2e`) as
            // the wizard dies at 23731 and `sub_68D50` starts refusing.
            // Retail's 557 holds `flags 32780` (= 0x800C) through BOTH
            // 23731 and 23732 and only reads 12 at 23733. The port
            // dropped 0x8000 during 23732 — at the wizard's own walk
            // slot, BELOW the human's (9,0) fireball at slot 997 — so
            // `sub_65C20`'s `word[0] & 0x8010` window test missed, the
            // bolt struck the CORPSE instead of flying on, and the
            // strike minted the census's `extra in port: slot 770`.
            if s == 8 && !no_rival_rebound_clear_defer() {
                self.mc2_rival_clear_buff(ri, s);
            }
            return;
        }
        let i = self.mc2_rivals[ri].ent as usize;
        if i == 0 || i >= self.g.ent.len() {
            return;
        }
        let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28;
        // ⭐⭐⭐ SHIELD III DECREMENTS BEFORE IT BILLS — see
        // [`no_rival_shield3_predecrement`] for the whole law
        // (EF:56525-28 / `NETHERW.EXE` 0x8ed29 `dec` before 0x8ed34
        // `call`, against EF:56513-15 / 0x8ecf8 `call` before 0x8ecfd
        // `dec`). `sub_6A480` is CASTER-GENERIC, so the rival column
        // carries the human column's law verbatim.
        let shield3_predecrement = self.mc2_shield3_predecrement(s, m);
        // `sub_68D50` opens the body (see the note in
        // `mc2_rival_cast`); refused → `word_0x2E_46 = 1` and fall
        // through to the decrement.
        let afford = self.mc2_rival_afford(ri, m);
        if !afford {
            self.g.ent[m].f26 = 1;
        } else if first {
            // `sub_693F0` EF:56?-: the fire is the `word_0x2E_46 ==
            // word_0x30_48` arm, and the caster's yaw/pitch are read
            // LIVE here — a frame after the arm stamped them.
            if shield3_predecrement {
                // `sub_68DE0` with `v2 = f26 - 1`: the
                // `v2 == word_0x30_48` debit is unreachable, so this
                // takes the else — the pin — on the ARM tick.
                if self.g.ent[m].f26 > 1 {
                    let r = &mut self.mc2_rivals[ri];
                    if r.mana_delta > 0 {
                        r.mana_delta = 0;
                    }
                }
            } else if s != 5 {
                let cost = self.g.ent[m].max_life as i32;
                let r = &mut self.mc2_rivals[ri];
                r.mana_delta = if r.mana_delta >= 0 {
                    -cost
                } else {
                    r.mana_delta - cost
                };
            }
            let (yaw, pitch) = {
                let e = &self.g.ent[i];
                (e.f30, e.f32)
            };
            self.mc2_rival_emit(ri, i, s, yaw, pitch);
        }
        if s == 5 {
            self.mc2_rival_heal_tick(ri, m);
        } else if !first
            && spell_pins_regen(s)
            // …and Shield III's pin reads the POST-decrement counter,
            // so the tick the window ENDS on pins nothing.
            && !(shield3_predecrement && self.g.ent[m].f26 <= 1)
            // ⭐⭐⭐ …AND `sub_68DE0` IS UNREACHABLE FROM THE REFUSED
            // ARM — the twin of the gate in [`Self::mc2_rival_buffs`].
            // See [`rival_pin_afford_off`]. (A LAW LANDED ON ONE CALL
            // PATH IS NOT LANDED: a respawn-minted book reaches its
            // body down THIS path instead.)
            && (rival_pin_afford_off() || afford)
        {
            // `sub_68DE0`'s else arm — the mid-burst regen pin, on the
            // twenty handlers that actually place the call there (see
            // [`spell_pins_regen`]).
            let r = &mut self.mc2_rivals[ri];
            if r.mana_delta > 0 {
                r.mana_delta = 0;
            }
        }
        // ⭐⭐⭐ INVISIBILITY'S CLOAK EDGE, SECOND CALL PATH — see
        // [`rival_invis_cloak_edge_off`]. `afford` is `sub_68D50`; the
        // refused arm above already collapsed the window.
        if s == 0xB && afford {
            mc2_invis_cloak_edge(&mut self.g, m, i, first);
        }
        self.mc2_rival_publish_buff(ri, s, m);
        self.g.ent[m].f26 -= 1;
        if self.g.ent[m].f26 == 0 {
            // …and the other half of the same split: `sub_6AA00`'s
            // expiry tail is `if (!v4) sub_6D880(a1x);` (EF:56760-61)
            // with NO flag write — the rebound clear waits for the
            // next pass's `<= 0` arm above. Only the SHIELD (and the
            // metamorph/invisibility tails below) clear here.
            if s != 8 || no_rival_rebound_clear_defer() {
                self.mc2_rival_clear_buff(ri, s);
            }
            if s == 6 {
                self.mc2_rivals[ri].shield_state = 0;
            }
            if s == 4 {
                self.mc2_rival_metamorph_expire(m, self.mc2_rivals[ri].ent as usize);
            }
            // `sub_6B1C0` expiry (EF:57108) — see the twin in
            // [`Self::mc2_rival_buffs`].
            if s == 0xB && !rival_metamorph_cloak_off() {
                let w = self.mc2_rivals[ri].ent as usize;
                if w != 0 && w < self.g.ent.len() {
                    self.g.ent[w].flags &= !0x20;
                }
            }
            if self.g.ent[m].f44 != 0 {
                let queued = (self.g.ent[m].f44 - 1) as u8;
                self.g.ent[m].f44 = 0;
                let own = self.mc2_rivals[ri].ent;
                self.mc2_rival_set_spell(m, queued, own);
            }
        }
    }

    /// EF:6810/6860 — `radix_tan(caster, Entities[caster+0x96])`:
    /// both aiming families aim at the entity the CASTER's own f146
    /// walk-slot register holds, NOT the AI brain's target pick
    /// (mc2l1 t=193: f146 = 141, the mana sphere it was feeding on).
    /// The read is raw and corpse-tolerant, like the register itself.
    fn mc2_rival_aim_pitch(&self, i: usize) -> u16 {
        let e = &self.g.ent[i];
        let tp = if let Some(hx) = self.g.hx_of_tag(e.f146) {
            self.human_pose(hx)
        } else {
            let t = (e.f146 as usize).min(self.g.ent.len() - 1);
            let te = &self.g.ent[t];
            (te.x, te.y, te.z)
        };
        Gen::mc2_radix_tan((e.x, e.y, e.z), tp)
    }

    /// Case 2 — Create Castle (EF:6820): with a castle, the upgrade
    /// request through the shared mail[5] token protocol; without,
    /// the DIRECT (3,2) spawn at the scouted site — MC2 runtime AI
    /// castles build the real thing (no MC1 free-plant).
    fn mc2_rival_cast_castle(&mut self, ri: usize, i: usize) -> bool {
        // Affordability re-check FIRST — a whiffed attempt must not
        // burn the recast cooldown (the cost is re-read fresh off the
        // ladder, not the manifestation's stale stamp). The cooldown
        // arms ONLY after a successful
        // upgrade fire (EF:6828); the first-castle direct spawn
        // never arms it at all (EF:6831-40).
        let cost = self.mc2_castle_ladder_cost(ri);
        if (self.mc2_rivals[ri].mana as i64) < cost as i64 {
            return false;
        }
        if let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) {
            if !self.g.mc2_castle_space_ok(c) {
                return false;
            }
            // ⭐⭐⭐ THE UPGRADE LEG IS AN **ARM**, NOT A MAIL WRITE.
            // Retail's case 2 WITH a castle is a bare
            // `sub_5F660(a1x, v2x, 0)` (EF:6825) — byte for byte the
            // same statement as case 3 — and every commit that
            // follows lives in the class-15 record's OWN action-6
            // body ([`Self::mc2_rival_castle_token_tick`]), which
            // runs later in the same frame whenever the token sits
            // above its wizard. The port short-circuited the whole
            // path and hand-wrote `castle.mail[5] = (10, owner)`,
            // with a comment calling the (9,10) ball ride "cosmetic,
            // APPROX skipped like the MC1 column". It is not:
            // mc2l6-rsg t=6429 mints slot 8 as a (9,10) owned by
            // rival 383 — target96 392, speed 464, scratch10 84 —
            // where the port left slot 8 on the free stack, and the
            // pool allocation alone forks every later spawn.
            //
            // `sub_5F660`'s model-2 arm (EF:60907-13): a LIVE upgrade
            // lock refuses outright (the buzz — `word_0x2E_46 > 0`
            // falls straight to LABEL_23 with the result still 0),
            // and the purse test is the caster's mana against the
            // manifestation's OWN `maxMana_0x8C_140`, which is the
            // ladder rung `SetSpell` cached there (10000 at the
            // witness = `MC2_CASTLE_COST[1]`, the same number
            // `mc2_castle_ladder_cost` computes). The PAYMENT is not
            // here either — it is the mint's own `sub_68DE0`.
            let m = self.mc2_rivals[ri].book.ent[2] as usize;
            if m == 0 || m >= self.g.ent.len() {
                return false;
            }
            if self.g.ent[m].f26 > 0 {
                return false;
            }
            if self.mc2_rivals[ri].mana < self.g.ent[m].max_life as i32 {
                return false;
            }
            // `sub_5F7B0` (EF:60974) — the bare window write.
            self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16;
            self.mc2_rivals[ri].cooldown[2] = AI_RECAST[2].max(1);
            return true;
        }
        // Castle-less: the direct (3,2) spawn at the site (EF:6833
        // IfSubtypeCallCreatingManaSphere(&axis_0x9A_154x, 3, 2)),
        // paid in full — the build machinery takes it from there.
        // ⭐⭐ AN INVENTED GUARD, AND FALSE EXACTLY WHEN IT MATTERS.
        // `sub_14E10`'s case-2 castle-less arm (EF:6831-40) is, byte
        // for byte at NETHERW.EXE 0x396A7 (verified by the main
        // session 2026-09-04):
        //     6a 02              push 0x2                (model 2)
        //     6a 03              push 0x3                (class 3)
        //     8d 83 9a 00 00 00  lea eax,[ebx+0x9a]      (&axis_0x9A_154)
        //     50                 push eax
        //     e8 d9 52 03 00     call 0x6E990            (_4A190)
        //     85 c0 / 74 2c      test eax,eax / je       (null alloc only)
        // — the axis is loaded as a POINTER and never read, let alone
        // tested. Retail builds at whatever `axis_0x9A_154` holds,
        // including the map origin, and the only refusal is a failed
        // allocation. WITNESS mc2l6-rsg t=28706: rival 378's
        // `dest_x/dest_y` are (0,0) (a never-scouted axis), retail's
        // own `explain` mints slot 316 as a (3,2) at x=y=z=0 and moves
        // `player 2 castle_ent 0 -> 316`, and the port refused here.
        // Ledger ROUND 99 dig 99-15 law 1.
        let (sx, sy) = self.mc2_rivals[ri].site;
        if no_mc2_rival_castle_any_site() && sx == 0 && sy == 0 {
            return false;
        }
        // ⭐ EF:6833's `IfSubtypeCallCreatingManaSphere(&axis, 3, 2)`
        // IS `sub_4AA40` — the SHARED (3,2) ctor the port already
        // transcribes exactly in `Gen::spawn_castle`. This arm used to
        // hand-roll a lesser copy of it and got three things wrong:
        //   · it LINKED at `site_z`, where retail links at the ground
        //     under the RAW landing point and keeps the perimeter-min
        //     only in `+0x9E` (the two-z law spelled out in
        //     `spawn_castle`) — mc2l6 t=70 slot 174 z retail 1536
        //     (= 6*256) / port 1184, t=104 slot 62 z 64 / 0;
        //   · it stamped `177 + COLOR_ART[slot]`, where the ctor ends
        //     in `SetEntityIndexAndRot_49CD0(v2x, 177)` FLAT (EF:33402)
        //     — the team colour is the build machine's own deferred
        //     latch one tick later (see `mc2_castle_build` case 0), so
        //     stamping it here put the recolor a whole tick early AND
        //     mispriced the derived extents quad (apitch/aroll retail
        //     194 / port 217 at t=104);
        //   · it omitted `+28 = 33` and the `frames89` stamp (the
        //     ungraded `b38`/`b5d` lanes: retail 33/1, port 0/0).
        // EF:6836 assigns the owner AFTER the ctor returns.
        let Some(c) = self.g.spawn_castle(sx, sy) else {
            return false;
        };
        self.g.ent[c].id24 = self.mc2_rivals[ri].ent;
        // ⭐⭐ THE FIRST CASTLE IS FREE. The castle-less arm (EF:6831-40)
        // is the ONE case-2 leg that never reaches `sub_5F660` — and
        // `sub_5F660` is where BOTH the manifestation arm and the mana
        // debit live (`sub_5F7B0` past the `mana < maxMana` refusal,
        // EF:60948-56). The upgrade leg above calls it and pays; this
        // one spawns the (3,2), stamps the owner and the castle index,
        // and returns 1 — no arm, no cooldown, NO DEBIT. The port
        // already had the cooldown half of that right (see the comment
        // at the top of this fn) and still charged for the spawn, which
        // is the mc2l6 t=71 head: rival 378 pays its whole 1000 purse
        // for its first castle and retail's stays pinned at 1000
        // (mana_max) with d88 +100 for the rest of the take.
        // Affordability is still GATED — retail's `sub_15170` refuses
        // the attempt below `manaCost_6` / the manifestation's own
        // `maxMana_0x8C_140` (EF:6929-33) — it is only the PAYMENT that
        // never happens.
        let _ = cost;
        self.g.snd(30, c);
        // ⭐⭐⭐ NO RESEARCH. The castle-less arm (file 0x396A7-0x396F3)
        // writes the owner id and `CastleEntityIndex_0x3A_58` and
        // nothing else — it never reaches `sub_69AB0`, the image's ONLY
        // writer of `array_0x24E_590` (files 0x8E366 / 0x8E379). A
        // rival's FIRST castle therefore stands UNRESEARCHED:
        // `sub_613D0`'s walk-down reads `array_0x24E_590[9+1] == 0`,
        // falls to `v4 == 0` and spawns no (10,79) defender piece at
        // its first level-up. See [`no_mc2_free_castle_unresearched`]
        // for the full citation and the mc2l12 t=10016 witness.
        if no_mc2_free_castle_unresearched() {
            let own = self.mc2_rivals[ri].ent;
            let tier = self.mc2_rival_castle_tier(ri);
            self.g.mc2_research_stamp(own, 1, tier);
        }
        self.entities_dirty = true;
        let _ = i;
        true
    }

    /// ⭐⭐⭐ `sub_69AB0` (EF:56086) ON THE RIVAL COLUMN — the CASTLE
    /// manifestation's own action-6 body, run at the **TOKEN's** pool
    /// slot exactly like every other class-15 handler.
    ///
    /// Retail never counts this window down: the mint latches it at
    /// `word_0x30_48 - 1` and only an explicit `sub_5F890(*, 0)` at the
    /// CASTLE's own pass releases it (see
    /// [`World::mc2_castle_lock_stamp`], whose rival half session 86
    /// already landed). What runs every tick is the solvency re-test —
    /// `sub_68D50` failing CLEARS the lock outright (EF:56163) — and
    /// the `word_0x36_54` cooldown decrement.
    ///
    /// ⚠ The slot is load-bearing. The mint reads the wizard's pose
    /// AFTER its own dispatch has moved it: at mc2l6-rsg t=6429 the
    /// ball is born at (61530, 41612), rival 383's POST-move position,
    /// and banks charge 84 — the value 383's walk had just stepped up
    /// from 83, which the mint then zeroes. Both are wrong by a frame
    /// from the caster's slot.
    pub(crate) fn mc2_rival_castle_token_tick(&mut self, m: usize, ri: usize) {
        let i = self.mc2_rivals[ri].ent as usize;
        if self.g.ent[m].f26 <= 0 {
            // `sub_6D880` (EF:58215) — the deferred tier applies once
            // the lock is down, the same drain the release edge runs.
            if self.g.ent[m].f44 != 0 {
                let queued = (self.g.ent[m].f44 - 1) as u8;
                self.g.ent[m].f44 = 0;
                let own = self.mc2_rivals[ri].ent;
                self.mc2_rival_set_spell(m, queued, own);
            }
        } else if i != 0 && i < self.g.ent.len() {
            if !self.mc2_rival_afford(ri, m) {
                self.g.ent[m].f26 = 0;
            } else if self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1) {
                self.mc2_rival_castle_mint(m, ri, i);
            }
        }
        if self.g.ent[m].f54 > 0 {
            self.g.ent[m].f54 -= 1;
        }
    }

    /// The mint proper — `sub_69AB0`'s `word_0x2E_46 == word_0x30_48`
    /// arm (EF:56118-58), stamp for stamp.
    fn mc2_rival_castle_mint(&mut self, m: usize, ri: usize, i: usize) {
        let own = self.mc2_rivals[ri].ent;
        let castle = self.rival_castle(own);
        // EF:56118-21 — the research write for `castleLevel + 1`
        // (retail indexes `array_0x24E_590` off the castle's
        // `dword_0x10_16`, our `f26`), off the manifestation's own
        // tier row. It runs BEFORE the spawn, so a pool-full mint
        // still researches.
        let stage = castle.map_or(1, |c| (self.g.ent[c].f26 + 1).clamp(1, 7)) as u8;
        let tier = self.g.ent[m].f71;
        self.g.mc2_research_stamp(own, stage, tier);
        // `sub_68DE0` (EF:56122) — the first-tick debit, and the cost
        // is the manifestation's own `maxMana_0x8C_140`, not a freshly
        // recomputed ladder rung. Retail's wizard 383 carries
        // `manaRegen_0x88_136` = −10000 out of t=6429.
        let cost = self.g.ent[m].max_life.min(i32::MAX as u32) as i32;
        {
            let r = &mut self.mc2_rivals[ri];
            r.mana_delta = if r.mana_delta >= 0 {
                -cost
            } else {
                r.mana_delta - cost
            };
        }
        // ⭐⭐ THE BALL IS MINTED **AT THE WIZARD**, NOT AT A HAND
        // MUZZLE. `_4A190(&v1x->position_0x4C_76, 9, 10)` takes the
        // wizard's own axis, and the `sub_68E50` call that follows is
        // DEAD on this path: the arm ran `sub_5F7B0(.., a3 = 0)`,
        // which clears the caster's two hand bits, and both of
        // `sub_68E50`'s offset arms are gated on one of them
        // (EF:55617/55643). Every AI cast passes a3 = 0 — `sub_5F660`
        // forces it for a model-1 caster outright (EF:60889) — so a
        // rival's ball never leaves a hand. mc2l6-rsg t=6429: ball
        // (61530, 41612) is rival 383's position to the unit.
        let (wx, wy, wz, wspeed, wfov, wyaw, wpitch, wid) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f126, e.f84 as i16, e.f30, e.f32, e.id24)
        };
        let Some(pr) = self.g.mc2_spawn_cast_proj(10, wx, wy, wz) else {
            return;
        };
        // EF:56129 — the lock latches HERE, in the mint.
        self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16 - 1;
        // The token's @0x2A payload lives in the port's `f30` on a
        // class-15 record (`port_ent_lanes_mc2` maps it to the `f2a`
        // lane); `f44` there is the deferred-tier word @0x2C.
        let (tok_sub, tok_mana) = (self.g.ent[m].f30, self.g.ent[m].f140);
        let charge = std::mem::take(&mut self.wiz_charge[self.mc2_rivals[ri].slot as usize]);
        {
            let e = &mut self.g.ent[pr];
            // EF:56130 — the caster boost rides @0x82 alone, RAW and
            // unclamped like every hand-written class-15 tail.
            e.f126 += wspeed;
            e.f40 = m as u16; // @0x26 = the token's own slot
            e.f44 = tok_sub; // @0x2A = the token's @0x2A
            e.id24 = wid; // @0x1A = the wizard's id
            e.z = e.z.wrapping_add(wfov); // EF:56135
            e.f140 = tok_mana; // @0x90 = the token's purse, verbatim
            e.f26 = charge as i16; // @0x10 = the banked cast charge
            e.f30 = wyaw; // EF:56155-56
            e.f32 = wpitch;
        }
        if let Some(c) = castle {
            // EF:56150-52 — the UPGRADE delivery: the ball morphs into
            // the (10,43) token and homes on the castle.
            let e = &mut self.g.ent[pr];
            e.f68 = 10;
            e.f69 = 43;
            e.f146 = c as u16;
        } else {
            // EF:56143-49 — the castle died between the arm and the
            // mint: fall back to the CREATE landing, 4096 ahead at
            // ground level, morphing into the (3,2).
            let mut t = (wx, wy, 0i16);
            Gen::polar_step(&mut t, wyaw, 0, 4096);
            let gz = self.g.ground_z(t.0, t.1) as i16;
            let e = &mut self.g.ent[pr];
            e.f68 = 3;
            e.f69 = 2;
            e.dest_x = t.0;
            e.dest_y = t.1;
            e.site_z = gz;
        }
        self.g.snd(15, pr); // EF:56157
        self.entities_dirty = true;
    }

    /// The rival's live castle-spell tier — the book manifestation's
    /// f71 (`byte_0x46_70`), 0 when spell 2 is unowned.
    fn mc2_rival_castle_tier(&self, ri: usize) -> u8 {
        let m = self.mc2_rivals[ri].book.ent[2] as usize;
        if m != 0 { self.g.ent[m].f71 } else { 0 }
    }

    /// The per-spell emission through the shared MC2 class-9
    /// spawners (the sub_5F660 router's downstream), owner = the
    /// rival's entity — homing, damage payloads and the impact-XP
    /// mail all serve it unchanged.
    fn mc2_rival_emit(&mut self, ri: usize, i: usize, s: usize, yaw: u16, pitch: u16) {
        let m = self.mc2_rivals[ri].book.ent[s] as usize;
        let tier = self.g.ent[m].f71 as usize;
        let row = self.g.assets.spells.get(s).copied();
        let mut sub = row.map(|r| r.tiers[tier.min(2)]).unwrap_or_default();
        if matches!(s, 21 | 25) && sub.life > 0 {
            sub.sub_spell /= sub.life as i32;
        }
        // The would-be projectile subtype: the sub_6DCA0 band + the
        // direct class-9 arms (possess 1/17 / summon 24 / mine 29 /
        // alliance 25). Rivals run the SAME `sub_69640` machine as the
        // human, so possession picks its entity off the tier's
        // `life_0x1A` too: 0 → the basic **(9,1)** (`sub_69900`
        // EF:56039), 1..3 → the leveled (9,17) (EF:55950). Hardcoding
        // 17 left 49 (9,17)-extra rows on the mc2l4 0+4000 window
        // after the human's arm was fixed.
        let band = World::mc2_dispatch_arm(s, sub.life);
        // The direct arms are the ones that do NOT go through
        // `sub_6DCA0` — each has its own hand-written spawn tail, and
        // the speed law below turns on exactly this split.
        let direct = band.is_none();
        let arm = band.map(|a| (a.subtype, a.impact, a.charge)).or(match s {
            // ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED — the
            // POSSESSION TIER PAYLOAD. `sub_69640` (EF:55915) is the
            // shared body: `life_0x1A == 0` → `sub_69900` mints the
            // basic (9,1) with `byte_0x44_68 = 12` (EF:56046), and
            // `1 <= life <= 3` mints the leveled (9,17) whose
            // `byte_0x44_68` is written 54 at life 1 and 69 at life 2
            // (EF:55957-64) and LEFT AT THE CTOR DEFAULT at life 3;
            // `byte_0x43_67 = 10` unconditionally (EF:55965). `life >
            // 3` fails the `<= 3u` gate and casts NOTHING at all.
            // The human's arm (mc2/cast.rs, spell 1) has carried the
            // whole table for sessions; the rival funnel hardcoded the
            // BASIC bolt's (10,12) on both tiers, so rival 611's Mana
            // Lock bolt at mc2l22 slot 718 flew with b44 12 where
            // retail records 69 — and its victimless expiry then took
            // `mc2_proj_impact`'s unconditional (10,12) ground-miss
            // arm instead of the (10,54)/(10,69) struck-gated one.
            1 if !no_rival_possess_tier() => {
                let plife = row.map_or(tier as i8, |_| sub.life);
                match plife {
                    0 => Some((1u8, (10u8, 12u8), false)),
                    1 => Some((17, (10, 54), false)),
                    2 => Some((17, (10, 69), false)),
                    3 => Some((17, (10, 0), false)),
                    _ => None,
                }
            }
            1 if sub.life == 0 => Some((1u8, (10u8, 12u8), false)),
            1 => Some((17u8, (10u8, 12u8), false)),
            0x13 => Some((24, (10, 0), false)),
            0x17 => Some((29, (10, 0), false)),
            0x18 => Some((25, (10, 0), false)),
            _ => None,
        });
        // Cast sound (EF:44233 family).
        let snd = match s {
            0 => Some(9u8),
            1 => Some(40),
            3 => Some(19),
            5 => Some(25),
            7 => Some(23),
            0x13 | 0x18 => Some(9),
            _ if arm.is_some() => Some(15),
            _ => None,
        };
        if let Some(id) = snd {
            self.g.snd(id, i);
        }
        // Fools-mana conjures the (10,57) random sphere in place. The
        // sphere is a TRAP (`sub_36680` EF:26615, the (10,57) tick's
        // own law — docs/spell-audit/fools-mana.md), so it must carry
        // the caster as parentId (`sub_6C870` EF:57905): that is the
        // ONLY skip arm, and without it the rival springs its own bait.
        // ⭐⭐⭐ FOOL'S MANA IS A SIX-SPHERE THROW, NOT ONE GROUND
        // SPHERE. `sub_6C870` (EF:57868-57920) is the class-15 action-66
        // body: on the window's first tick, `if (free > 6)`
        // (`sub_4A810_get_0x35plus() > 6`), it loops SIX
        // `_4A190(&caster->pos, 10, 57)` and stamps each sphere:
        //   actSpeed = (tokenLCG & 0x7F) + clamp(4·caster.actSpeed,
        //              140, 280)                              (EF:57897-57904)
        //   sub_68E50(caster, sphere, token) — muzzle; DEAD on a rival
        //              (hand bits cleared by `sub_5F7B0(..,0)`), so the
        //              sphere sits at the CASTER's own position, caster z
        //   parentId_0x28 = caster.id_0x1A                    (EF:57905)
        //   subSpellIndex_0x2A (f44) = tier subSpellIndex_2   (EF:57906)
        //   byte_0x46_70 (f71)       = tier life_0x1A         (EF:57907)
        //   playerEntityIndex (f144) = 0, then = caster id +
        //              SetManaSphereColorAndRot when life_0x1A >= 3
        //   yaw   = (caster.yaw − 85 + tokenLCG % 0xAA + wizext+0x18)
        //           & 0x7FF                                   (EF:57914-18)
        //   pitch = caster.pitch + wizext+0x1A                 (EF:57919)
        // then sound 11 at the last sphere (EF:57923). mc2l22
        // t=9785→9786: rival 557 pays 25000 and retail births SIX
        // (10,57) at 900/901/904/920/924/938 (owner28 557, pitch 1925,
        // speed 328, b38 3 = tier 3 colour) where the port minted ONE
        // at ground z with no speed and no colour. The port's wizard
        // record carries no wizext +0x18/+0x1A for a rival (retail
        // writes them from the HUMAN input block only, EF:38065-66),
        // so those terms are 0 here. Both LCG draws are the TOKEN's
        // own (`a1x->rand`), not the sphere's and not the caster's.
        if s == 0x16 {
            if self.g.free.len() <= 6 {
                return;
            }
            let (px, py, pz, cyaw, cpitch, cspeed, cid) = {
                let e = &self.g.ent[i];
                (e.x, e.y, e.z, e.f30, e.f32, e.f126, e.id24)
            };
            let payload = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            let tier_life = sub.life.max(0) as u8;
            let base = (4 * cspeed as i32).clamp(140, 280);
            let mut last = 0usize;
            for _ in 0..6 {
                let Some(sp) = self.g.mc2_spawn_mana_sphere(57, px, py, pz) else {
                    break;
                };
                let r1 = self.g.ent_rand(m);
                let speed = ((r1 & 0x7F) as i32 + base) as i16;
                let r2 = self.g.ent_rand(m);
                let yaw = cyaw.wrapping_sub(85).wrapping_add((r2 % 0xAA) as u16) & 0x7FF;
                let e = &mut self.g.ent[sp];
                e.f126 = speed;
                e.id24 = cid;
                e.f44 = payload;
                e.f71 = tier_life;
                e.f144 = 0;
                e.f30 = yaw;
                e.f32 = cpitch;
                if tier_life >= 3 {
                    e.f144 = cid;
                    self.g.ball_resize(sp);
                }
                last = sp;
            }
            if last != 0 {
                self.g.snd(11, last);
            }
            return;
        }
        let Some((subtype, impact, charge)) = arm else {
            // Buff/self spells have no projectile — the armed window
            // on the manifestation IS the effect — EXCEPT METAMORPH.
            // ⭐⭐⭐ A "COSMETIC" ENTITY IS STILL AN ALLOCATION.
            // `sub_6A030` (EF:56294) is caster-generic: on the window's
            // first tick it mints `_4A190(&caster.pos, 5, tier.life_0x1A)`
            // — a class-5 creature at the WIZARD's position — and stamps
            // it the pose-PUPPET: `StageVar2_0x49 = 12`, `action = 8*M+7`,
            // `parentId_0x28 = caster`, `id_0x1A = caster.id`, links it
            // through the TOKEN's `word_0x96_150` (EF:56316-56332), then
            // sound 60. At expiry (EF:56399-56408) the linked puppet is
            // `DisableEntityDrawing04_57F10`'d (our 0x400 reap flag) and
            // the link cleared. The human column has carried both halves
            // (`mc2_cast_metamorph`, `mc2_cast_expire` case 4, cast.rs);
            // the rival funnel returned here with "no projectile" and the
            // Defense arm's own comment called the disguise
            // "presentation-side, APPROX unported". It is a pool record:
            // mc2l22 t=1198→1199 retail births (5,2) at 673 (owner 557)
            // and 953 (owner 530), t=1249→1250 (5,2) at 729 (owner 503,
            // life 3000, action 23 = 8·2+7, sv2 12) — every one `missing`
            // in the port, every later spawn one free-stack pop off.
            if s == 4 {
                let model = sub.life.max(0) as u8;
                let (px, py, pz) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z)
                };
                let owner = self.mc2_rivals[ri].ent;
                if let Some(c) = self.g.mc2_spawn_creature_model(model, px, py, pz) {
                    if std::env::var_os("MGC_K1_TRACE").is_some() {
                        eprintln!(
                            "[k1/meta] t={} ri={ri} ent={i} owner={owner} slot={c} model={model}",
                            crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                        );
                    }
                    let e = &mut self.g.ent[c];
                    e.site_z = 12; // StageVar2 = 12: the pose-puppet
                    e.tick70 = model.wrapping_mul(8).wrapping_add(7);
                    e.id24 = owner; // @0x1A = @0x28 = the casting wizard
                    // See
                    // [`crate::engine::features::no_mc2_morph_keeps_ctor_10`]
                    // — the class-5 ctor's `slot % 100` stays.
                    if crate::engine::features::no_mc2_morph_keeps_ctor_10() {
                        e.f26 = 0;
                    }
                    self.g.ent[m].f146 = c as u16; // word_0x96_150 link
                    // ⭐⭐⭐ THE METAMORPH CLOAK IS THE INVISIBILITY BIT,
                    // AND `sub_6A030` IS CASTER-GENERIC. The first-tick
                    // block's last write on the WIZARD is
                    // `v2x->struct_byte_0xc.byte[0] |= 0x21u` (EF:56335;
                    // shipped 0x8E92F `or dl,0x21`) — bit 0 the
                    // draw-hide, bit 5 (0x20) the SAME
                    // scan-invisibility bit Invisibility and the death
                    // touchdown raise, which every creature scanner
                    // filters on. Colour-blind: the human/AI test right
                    // below it (EF:56338-42) only re-arranges bit 0
                    // between puppet and wizard. Landed on the human
                    // column as `MGC_NO_METAMORPH_CLOAK`
                    // (`mc2/cast.rs`), where the out-of-pool carpet
                    // makes it free-run-only; a rival wizard is a POOL
                    // RECORD, so here it is graded.
                    //
                    // ⭐⭐⭐ A FAILED ALLOCATION IS NOT A CLOAK. The
                    // write sits INSIDE `if (v4x)` — the
                    // `_4A190(&caster.pos, 5, tier.life_0x1A)` success
                    // arm (EF:56325-48) — so a metamorph cast on a FULL
                    // POOL mints no puppet, raises no cloak, and still
                    // counts its window down. mc2l22 pair 4728→4729 is
                    // the witness: free stack EMPTY, rival 530 mid
                    // metamorph window, and retail's byte[0] & 0x20 on
                    // 530 is CLEAR.
                    if !rival_metamorph_cloak_off() {
                        self.g.ent[i].flags |= 0x20;
                    }
                    self.g.snd(60, i);
                }
                self.entities_dirty = true;
            }
            return;
        };
        let (ex, ey, ez, speed, half) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f126, e.f78 as i16)
        };
        // ⭐ THE ARMY AND ALLIANCE ARMS NEITHER LIFT NOR RE-PRICE.
        // `sub_6C170` (EF:57659-81) and `sub_6CD20` (EF:58062-86) are
        // the only two direct `_4A190` arms with NEITHER the
        // `position_0x4C_76.z += array_0x52_82.fov` lift NOR the
        // `mana_0x90_144 = <token>->mana_0x90_144` copy their five
        // siblings carry (fire block EF:55865-66, mine EF:57991-92,
        // possession EF:56055-56 / EF:55970-71). They spawn at
        // `&<caster>->position_0x4C_76`, run `sub_68E50` (a 256-unit
        // LATERAL step at pitch 0) and stop — so the bolt launches at
        // the caster's OWN z and keeps the class-9 ctor's
        // `mana_0x90_144 = 50`. Measured on the HUMAN column
        // (mc2l6-rsg pair 15630→15631 slot 6: z retail 1145 / port
        // 1245, mana retail 50 / port 33); the rival funnel is the
        // same body and takes the same law.
        let bare = matches!(subtype, 24 | 25);
        let mz = if bare { ez } else { ez.wrapping_add(half) };
        let owner = self.mc2_rivals[ri].ent;
        let target = self.mc2_rivals[ri].target;
        // ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED — THE
        // LIGHTNING TWIN FAN. `sub_6A5C0` (EF:56596-56656) is the
        // class-15 model-7 body for EVERY caster: on the window's
        // first tick it loops `(life_0x1A != 1) + 1` spawns through
        // `sub_6DCA0`, hands the caster's charge bank to the FIRST
        // (the meter is zeroed by that spawn, EF:56620-21), stores
        // `yaw ± 113` (+113 on the first, −113 on the second, masked
        // 0x7FF) to `yaw_0x1C_28` ALONE after the unfanned muzzle
        // step, and cross-links the pair via `word_0x34_52`
        // (EF:57002-06; the port home is `f54`). The human column has carried this since
        // `mc2_spell_fire`'s fan; the rival funnel minted one bolt.
        // mc2l22 t=4636→4637: rival 584 fires its tier-2 lightning
        // (token 592 `b46` 2) at castle 556 and retail births TWO
        // (9,12) at 901 (bank 1, `f34` 853) and 853 (bank 0, `f34`
        // 901); the port minted 901 only, and the (10,0) revive that
        // should have taken 715 took 853 — the take's horizon.
        let fan: &[u16] = if s == 7 && sub.life == 2 {
            &[113, 113u16.wrapping_neg()]
        } else {
            &[0]
        };
        let mut twin: Option<usize> = None;
        for &off in fan {
            let Some(p) = self.g.mc2_spawn_cast_proj(subtype, ex, ey, mz) else {
                continue;
            };
            // Every retail cast site copies the hand token's mana onto
            // the spawned projectile (sub_69900 EF:56056 for the basic
            // possession bolt) — rivals run the SAME machine; the human
            // arm in mc2_launch already carries this (mc2l4 t=83 slot
            // 306: retail 33 = the token's purse, not the ctor's 50).
            let token_mana = self.g.ent[m].f140;
            // The token's `subSpellIndex_0x2A_42` (class-15 home = f30)
            // — the leveled possession bolt's reach root, below.
            let token_sub = self.g.ent[m].f30 as i32;
            let charge_bank =
                std::mem::take(&mut self.wiz_charge[self.mc2_rivals[ri].slot as usize]);
            {
                let e = &mut self.g.ent[p];
                e.id24 = owner;
                e.f68 = impact.0;
                e.f69 = impact.1;
                if !bare {
                    e.f140 = token_mana;
                }
                // ⭐⭐⭐ AN ABSENCE IN AN ENUMERATED LIST, ON THE TWIN CALL
                // PATH. The human column landed this in round 98
                // (`mc2/cast.rs` `mc2_launch`, `MGC_NO_MC2_LAUNCH_2A_ABSENCE`)
                // and the rival funnel is the SAME retail body — these
                // class-15 handlers are caster-generic, taking the caster
                // as `Entities[token->parentId_0x28_40]`, so there is no
                // separate "rival" thunk to differ from.
                // `sub_69900` (EF:56039-70), the leveled possession block
                // in `sub_69640` (EF:55950-84) and the summon army
                // `sub_6C170` (EF:57637-77) contain NO
                // `subSpellIndex_0x2A_42` write at all, so their flyers
                // keep `NewEvent_4A050`'s ctor **100** (Events.cpp:569).
                // BYTE-VERIFIED: `sub_69900` in the shipped `NETHERW.EXE`
                // is file 0x8E100-0x8E240 and its complete store list on
                // the spawned record is @0x82, @0x43 (=10), @0x26, @0x44
                // (=12), @0x1a, @0x50, @0x90, @0x10 (=0xc8), @0x9a, @0x1c
                // and @0x1e — there is no `0x2a(%ebx)` anywhere in it.
                // Census witness (rsg pairs 0..2000, `MGC_RAW_SHADOW=1`):
                // 143 `(9,1) f2a` rows, retail 100 / port 10.
                // ⭐ …AND THE FOURTH MEMBER, THE STEAL-MANA BOLT (9,8),
                // on this twin call path too — `sub_6DCA0`'s `a3 <= 0xD`
                // arm is the one band arm that writes neither `@0x2A`
                // nor `@0x46`, and `sub_6B3E0` (caster-generic, so
                // rivals run the same body) adds no `@0x2A` write.
                // Full citation on [`crate::mc2::cast::no_steal_2a_absence`].
                let absent_2a = matches!(subtype, 1 | 17 | 24)
                    || (subtype == 8 && !crate::mc2::cast::no_steal_2a_absence());
                if !absent_2a || crate::mc2::cast::no_rival_launch_2a_absence() {
                    e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
                }
                if charge {
                    e.f71 = sub.life.max(0) as u8;
                }
                // Steal Mana: the token's TIER INDEX rides the bolt's
                // `byte_0x46_70` (`sub_6B3E0` EF:57213 — rivals run the
                // same class-15 body), not `sub.life`. The (10,25) burst
                // copies it (EF:63559) and `sub_61050` indexes
                // `SPELLS[13]` by it.
                if s == 13 {
                    e.f71 = tier as u8;
                }
                e.f30 = yaw;
                e.f32 = pitch;
                // ⭐⭐⭐ THE CLASS-9 FLYER IS BORN WITH `roll`/`fov` AT
                // ZERO — the twin call path of the human column's own
                // absence (`mc2/cast.rs`, `MGC_NO_MC2_LAUNCH_ROLL_ABSENCE`,
                // which carries the full citation). The port's `f34`/`f36`
                // ARE retail's `roll_0x20_32`/`fov_0x22_34`
                // (`import_ent_mc2`: `f34: r.roll`), and not one of the
                // twenty class-15 fire handlers — nor the band spawner
                // `sub_6DCA0`, nor `NewEvent_4A050`'s eleven seeded
                // defaults — writes either word on the spawned flyer.
                if crate::mc2::cast::no_launch_roll_absence() {
                    e.f34 = yaw;
                    e.f36 = pitch;
                }
                // ⭐⭐⭐ THE DIRECT ARMS TAKE THE CASTER BOOST **RAW** — the
                // [384, 0x2000] clamp is `sub_6DCA0`'s ALONE (EF:44224-31,
                // the band spawner's tail). Every hand-written class-15
                // handler instead opens with a bare
                // `spawned->actSpeed_0x82_130 += caster->actSpeed_0x82_130`
                // and never clamps: possession basic (`sub_69900`
                // EF:56048) and leveled (EF:55953), summon (EF:57662),
                // mine (EF:57984), alliance (EF:58065). So a bolt cast by
                // a REVERSING wizard is born BELOW its own `min_speed` and
                // stays there — the class-9 mover never floors it back up.
                //
                // The human column has carried exactly this since the
                // mc2l4 t=13 witness (`mc2_spell_fire` case 1 re-stamps
                // `384 + p.speed` after `mc2_launch` clamped); the rival
                // funnel is one shared body for both halves and kept the
                // clamp — A LAW LANDED ON ONE CALL PATH IS NOT LANDED.
                // mc2l6-rsg t=1130: rival 370 fires its (9,1) at speed −8
                // (its own `d88` had just flipped to −100), so retail's
                // slot 853 is born at 384 − 8 = **376** against
                // `min_speed` 384 where the port minted 384 — and the
                // whole x/y/z head is that one missing step-length.
                //
                // ⚠ The `.max(0)` on the band leg is inert, not faithful:
                // retail passes `a5 = v1x->actSpeed` RAW (EF:55854 and the
                // eleven `sub_6DCA0` siblings) and every class-9 creator's
                // base speed is 384, so `clamp(384, ..)` swallows the
                // difference on every existing row. Left alone rather than
                // changed without a witness.
                let boosted = if direct {
                    e.f126 as i32 + speed as i32
                } else {
                    (e.f126 as i32 + speed.max(0) as i32).clamp(384, 0x2000)
                };
                e.f126 = boosted as i16;
                // ⭐⭐⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED —
                // `word_0x26_38` IS THE (15,x) TOKEN'S **POOL SLOT**, NOT
                // A SPELL INDEX. The human arm ([`crate::mc2::cast`]'s
                // `no_token_slot_backref`) and this file's own (9,10)
                // upgrade-ball tail (`e.f40 = m as u16` above) already
                // stamp the token slot; the shared rival funnel kept the
                // retired spell index, so every rival bolt carried
                // `s` where retail records the token.
                //
                // Retail DEREFERENCES the lane at both readers:
                //   • the impact XP award `sub_6D8B0(a1x->id,
                //     Entities_EA3E4[a1x->word_0x26_38]->model_0x40_64, 1)`
                //     (EF:62985) — slot and index are interchangeable
                //     there only because a (15,N) token's model IS N;
                //   • the Magic Mine swallow `sub_68AC0` (EF:55441-44),
                //     which reads `v5x->model_0x40_64` AND
                //     `v5x->byte_0x46_70` off the record and re-arms
                //     `v5x->word_0x2E_46 = 1` — UNREACHABLE from an index,
                //     which is why the lane must carry the slot.
                // [`Gen::mc2_token_model`] resolves it back for the first.
                //
                // WITNESS (mc2l6-rival-spells-galore, `dump-state --port`
                // t=341 --start 340): rival 378's newborn (9,0) at slot 60
                // records `@0x26` = **379**, which is exactly 378's own
                // (15,0) hand token (`owner28` 378, `@0x2A` 250 and mana
                // 20 — both already copied onto the bolt); the port wrote
                // 0. Whole-take raw-shadow census of pairs 0..2000:
                // (9,0) f26 187 rows (e.g. t=243 slot 63 retail 371 port
                // 0) and (9,1) f26 33 rows (t=586 slot 561 retail 380
                // port 1 — the possession bolt, spell index 1).
                //
                // `MGC_NO_RIVAL_TOKEN_BACKREF=1` restores the spell index.
                e.f40 = if no_rival_token_backref() {
                    s as u16
                } else {
                    m as u16
                };
                // The CHARGE BANK, rival column: every retail effect-state
                // spawner writes `v6x->dword_0x10_16 = caster wizext
                // byte_0x154_340` and ZEROES the meter (EF:55869-70 for
                // the fireball and seventeen siblings) — the basic
                // possession bolt instead stamps a FLAT 200 and keeps the
                // zero (`sub_69900` EF:56057-58). The human's `mc2_launch`
                // has carried both halves for sessions; the rival funnel
                // banked nothing, so mc2l6 t=687's (9,0) at slot 643 read
                // `scratch10` 0 where retail banked 60. @0x10 is the
                // port's `f26` (`port_ent_lanes_mc2` maps it to the
                // `scratch10` lane).
                // ⭐ …AND THE LEVELED POSSESSION BOLT IS NEITHER: ITS
                // @0x10 IS THE SQUARED REACH, ON THIS CALL PATH TOO.
                // `sub_69640`'s (9,17) arm (EF:56329) writes
                // `(token->subSpellIndex_0x2A_42 << 8)²` and only
                // ZEROES the caster's charge meter — it never banks it
                // on the bolt. The rival funnel wrote the bank here, so
                // a rival's leveled bolt carried a completely different
                // word. Homed as the ROOT, published squared — see
                // [`crate::mc2::cast::no_posses_reach_root`].
                e.f26 = if subtype == 1 {
                    200
                } else if subtype == 17 && !crate::mc2::cast::no_posses_reach_root() {
                    token_sub as i16
                } else {
                    charge_bank as i16
                };
                // NO f146 hand-off: no site in retail's rival cast
                // chain writes word_0x96_150 onto the spawned bolt
                // (sub_14E10 writes only the caster's pitch; sub_5F660/
                // sub_5F7B0 touch only the window word and flags) — the
                // rival possession bolt carries target96 = 0 and flies
                // a straight frozen-pitch ray (mc2l1 t=193-195 slot
                // 151: z steps exactly −56/tick). The old stamp let the
                // class-9 homing tick re-aim it at the AI's pick and
                // forked the whole trajectory.
            }
            // ⭐⭐⭐ THE LAUNCH AIM POINT, ON THE TWIN CALL PATH. A LAW
            // LANDED ON ONE CALL PATH IS NOT LANDED: every class-15 fire
            // handler copies the CASTER's own pre-lift
            // `position_0x4C_76` into the flyer's `axis_0x9A_154x` and
            // steps it along the launch bearing, and those handlers are
            // caster-generic (`Entities[token->parentId_0x28_40]`), so a
            // rival's bolt carries it exactly like the human's. The reach
            // table and both shipped-EXE confirmations live in
            // [`crate::mc2::cast::mc2_launch_axis_reach`].
            // The bearing is the wizext aim offsets plus the caster's
            // yaw/pitch; a RIVAL carries no wizext @0x18/@0x1A (retail
            // writes them from the human input block only, EF:38065-66),
            // so the emit's own `yaw`/`pitch` are exactly retail's terms.
            // The position is the caster's, NOT the muzzle and NOT the
            // fov-lifted spawn z.
            // WITNESS (rsg t=341 slot 60, `MGC_RAW_SHADOW=1`): caster 378
            // at (64341, 16649, 360), yaw 1324, pitch 10 — retail records
            // dest (51308, 26565, -142) where -142 = 360 -
            // 16384*sin(10/2048*2pi) and the horizontal leg is
            // 16384*cos(pitch) = 16376. The port left all three at 0.
            // The reach's spell-1 arm is keyed on the SPAWNED class-9
            // model (`sub_69900` 10240 vs `sub_69640` 0x4000) — see
            // [`crate::mc2::cast::no_mc2_posses_axis_reach_by_subtype`].
            // The rival funnel has the minted record in hand, so it
            // names the spawner directly.
            if let Some((reach, use_pitch, ground_snap)) =
                crate::mc2::cast::mc2_launch_axis_reach(s, sub.life, self.g.ent[p].model65)
            {
                if !crate::mc2::cast::no_rival_launch_axis() {
                    let mut dest = (ex, ey, ez);
                    Gen::polar_step(&mut dest, yaw, if use_pitch { pitch } else { 0 }, reach);
                    // The TERRAIN-TAIL EIGHT end
                    // `axis.z = getTerrainAlt_10C40(&axis)` (EF:57383 &c.).
                    if ground_snap {
                        dest.2 = self.g.ground_z(dest.0, dest.1) as i16;
                    }
                    let e = &mut self.g.ent[p];
                    e.dest_x = dest.0;
                    e.dest_y = dest.1;
                    e.site_z = dest.2;
                }
            }
            if off != 0 {
                // The fan lands on `yaw_0x1C_28` alone, AFTER the
                // muzzle step and the axis reach took the unfanned
                // bearing (EF:56629-42).
                self.g.ent[p].f30 = yaw.wrapping_add(off) & 0x7FF;
            }
            if let Some(t) = twin {
                // ⭐⭐⭐ THE CROSS-LINK'S HOME IS **f54** (@0x34), ON
                // THE TWIN CALL PATH TOO — `sub_6A5C0` is
                // caster-generic, so the rival funnel writes the same
                // `word_0x34_52`. Citation, shipped bytes and the
                // 177+177-row (9,12) census on
                // [`crate::mc2::cast::no_mc2_m12_handle_home`].
                if crate::mc2::cast::no_mc2_m12_handle_home() {
                    self.g.ent[p].f52 = t as u16;
                    self.g.ent[t].f52 = p as u16;
                } else {
                    self.g.ent[p].f54 = t as u16;
                    self.g.ent[t].f54 = p as u16;
                }
            }
            twin = Some(p);
        }
        if twin.is_none() {
            return;
        }
        if let Some(hx) = self.hx_of_tag(target) {
            // Being targeted arms the danger music.
            self.g.player_danger[hx] = 100;
        }
        self.entities_dirty = true;
    }

    // NOTE: retail rivals have NO spell-XP progression —
    // `sub_6D8B0`'s guard is class-3 model-0, the human only
    // (EF:58240-41). A rival's spell tiers are its authored map
    // levels for life; the per-cast TIER-DOWN walk
    // (`mc2_rival_tier_probe`) supplies all tier dynamics. Do NOT add
    // an XP relevel ladder for rivals.

    /// Test hook: the rival's book as (owned, level) rows plus its
    /// castle's (stored, cap) mana — `None` castle-less. The lifecycle
    /// tests pin the authored grant/tier law and the spawns-full
    /// castle bank with this.
    #[doc(hidden)]
    pub fn debug_mc2_rival_economy(
        &self,
        slot: u8,
    ) -> Option<([(bool, u8); MC2_SPELLS], Option<(i32, i32)>)> {
        let r = self.mc2_rivals.iter().find(|r| r.slot == slot)?;
        let mut book = [(false, 0u8); MC2_SPELLS];
        for (s, row) in book.iter_mut().enumerate() {
            *row = (r.book.ent[s] != 0, r.book.levels[s]);
        }
        let bank = self
            .rival_castle(r.ent)
            .map(|c| (self.g.ent[c].f140, self.g.ent[c].f136));
        Some((book, bank))
    }

    /// Test hook for the AUTHORED SEED lane: everything the `.mgcl`
    /// wizard block feeds into a rival before a single tick of play —
    /// `(life_scale, ent.max_life, ent.life, world mirror, agg, per,
    /// refl)`.
    ///
    /// ⚠⚠ NOTHING IN THE GRADED CORPUS CAN SEE THESE. Replay
    /// re-imports the rival-AI channel (`life_scale`, personality) and
    /// the entity's `max_life`/`life` at every anchor, so a port that
    /// dropped the authored handicap entirely — every rival starting
    /// at full strength in real play — would give ZERO divergences
    /// corpus-wide. Round 131 W8: the handicap is also PERMANENTLY
    /// DISCARDED on a rival's first death (`sub_5C950` resets
    /// `word_0x24A_586 = 256` / `maxLife = 10000`, and the authored
    /// `Life_0x3612F` read sits inside the NEW-ENTITY arm), so the
    /// seed is only ever observable at t=0.
    #[doc(hidden)]
    pub fn debug_mc2_rival_life_seed(
        &self,
        slot: u8,
    ) -> Option<(u16, u32, i32, u16, u16, u16, u16)> {
        let r = self.mc2_rivals.iter().find(|r| r.slot == slot)?;
        let e = &self.g.ent[r.ent as usize];
        Some((
            r.life_scale,
            e.max_life,
            e.act_life,
            self.g.mc2_life_scale.0[slot as usize],
            r.agg,
            r.per,
            r.refl,
        ))
    }

    /// Test hook: relocate a rival's wizard record to a tile position
    /// and hand back its pool slot.
    ///
    /// ⚠ THE WAY OUT OF THE AT-CASTLE MAILBOX MEMSET, which is
    /// retail's own arm and not a rig artifact: a wizard overlapping
    /// its own castle dumps its inbox into the castle and pins
    /// `word_0x159_345` to 2, whose handler memsets `str_0x5E_94` and
    /// SKIPS `sub_5EFA0` entirely (`AddPlayer03_00_5E010`
    /// EF:59962-88). A DOCKED WIZARD READS NO MAIL AT ALL — no
    /// damage, no steal, no duel grip — so any test that needs a
    /// rival to receive a letter has to get it off its own doorstep
    /// first.
    #[doc(hidden)]
    pub fn debug_place_mc2_rival(&mut self, slot: u8, tx: f32, ty: f32) -> Option<usize> {
        let ri = self.mc2_rivals.iter().position(|r| r.slot == slot)?;
        let i = self.mc2_rivals[ri].ent as usize;
        let (x, y) = (
            (tx.rem_euclid(256.0) * 256.0) as u16,
            (ty.rem_euclid(256.0) * 256.0) as u16,
        );
        let z = self.g.ent[i].z;
        self.g.move_relink(i, x, y, z);
        Some(i)
    }

    /// Test hook: zero a rival's grace and hand it a lethal hit from
    /// nothing (the mc1 `debug_kill_player` shape).
    #[doc(hidden)]
    pub fn debug_kill_mc2_rival(&mut self, slot: u8) {
        if let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == slot) {
            self.mc2_rivals[ri].grace = 0;
            let i = self.mc2_rivals[ri].ent as usize;
            self.g.ent[i].mail[0] = (u32::MAX / 4, 1);
        }
    }

    /// Test hook: seat the rival's knock register directly, so a test
    /// can pin the dead-wait wipe without having to construct a fall
    /// short enough to leave an impulse unspent.
    #[doc(hidden)]
    pub fn debug_arm_mc2_rival_knock(&mut self, slot: u8, dir: u16, mag: i16) {
        if let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == slot) {
            self.mc2_rivals[ri].knock_dir = dir;
            self.mc2_rivals[ri].knock_mag = mag;
        }
    }

    /// Test hook: is the rival's wizard record in the DEAD-WAIT arm
    /// (`actionIndex_0x45_69 == 3`, `sub_5E7C0`)? The death fall
    /// (action 2) has to reach the floor first.
    #[doc(hidden)]
    pub fn debug_mc2_rival_dead_waiting(&self, slot: u8) -> bool {
        self.mc2_rivals
            .iter()
            .find(|r| r.slot == slot)
            .and_then(|r| self.g.ent.get(r.ent as usize))
            .is_some_and(|e| e.tick70 == 3)
    }

    /// Test hook: hand a rival wizard a damage letter from an
    /// ARBITRARY pool source (`str_0x5E_94.word_0x62_98`), so a test
    /// can pin WHICH record the knockback bearing is taken off.
    #[doc(hidden)]
    pub fn debug_hit_mc2_rival(&mut self, slot: u8, src: u16, amount: u32) {
        if let Some(ri) = self.mc2_rivals.iter().position(|r| r.slot == slot) {
            self.mc2_rivals[ri].grace = 0;
            let i = self.mc2_rivals[ri].ent as usize;
            self.g.ent[i].mail[0] = (amount, src);
        }
    }

    /// Test hook: retail's free (`sub_49FE0`) as the knockback bearing
    /// sees it — the CLASS BYTE alone goes to zero and
    /// `position_0x4C_76` stands. That is the exact state mc2l4's slot
    /// 247 is in on the tick its hit kills rival 292.
    #[doc(hidden)]
    pub fn debug_mc2_free_in_place(&mut self, slot: usize) {
        if let Some(e) = self.g.ent.get_mut(slot) {
            e.class64 = 0;
        }
    }

    /// Test hook: every live class-9 flyer as
    /// `(slot, model, id24, word_0x26_38)`. The TOKEN BACK-REF lane
    /// (`f40` port-side) is invisible to the obs projection, so this is
    /// the only way a test can pin it — see the citations at the
    /// `e.f40` write in [`World::mc2_rival_emit`].
    #[doc(hidden)]
    pub fn debug_mc2_proj_backref(&self) -> Vec<(u16, u8, u16, u16)> {
        self.g
            .ent
            .iter()
            .enumerate()
            .filter(|(_, e)| e.class64 == 9 && e.act_life >= 0)
            .map(|(i, e)| (i as u16, e.model65, e.id24, e.f40))
            .collect()
    }

    /// Test hook: is `slot` a live class-15 manifestation token?
    #[doc(hidden)]
    pub fn debug_mc2_is_token(&self, slot: u16) -> bool {
        self.g
            .ent
            .get(slot as usize)
            .is_some_and(|e| e.class64 == 15)
    }

    // ---- mortality (sub_5E310 EV:2882 + sub_5E7C0 EV:2895) -----------------

    /// `sub_5D530` (EF:59610) DRIVEN BY A RIVAL — the CARPET mover,
    /// which is what a dead wizard flies with, and a different
    /// function from the alive brain's `sub_146F0` (EF:6416). Only the
    /// death fall reaches it on this column, so the two movers'
    /// disagreements are all corpse laws:
    ///
    /// - the **actSpeed servo runs BEFORE the step**, where `sub_146F0`
    ///   steps first and servos after — so a corpse covers its NEW
    ///   speed, not its old one (mc2l6-rsg t=1851 onward: 93 -> 77
    ///   moves 77, 77 -> 93 moves 93, exactly inverted from the same
    ///   wizard's alive ticks);
    /// - the pose accumulators are the HUMAN CURSOR's
    ///   (`rollDelta_0x4_4`/`pitchDelta_0x6_6` -> `roll_0x155_341`/
    ///   `pitch_0x157_343`, EF:59622-35). Nothing writes a rival's
    ///   (`PlayerEvents_51BB0` is the local player alone), so the yaw
    ///   integration is a no-op AND `pitch_0x1E_30` is STAMPED 0 from
    ///   the empty accumulator — the visible half of this law and the
    ///   head's own lane (rival 378 carries the 2007 its last cast
    ///   aimed with until the mover clears it);
    /// - the ramp only writes the move pitch `pitch_0x24_36` when the
    ///   aim pitch is nonzero (EF:59653-65), so a rival's stays 0 and
    ///   the step is flat;
    /// - the **strafe is spent but NOT decayed** here (EF:59681-93 has
    ///   no `-= 4 * sign` — that lives in `sub_146F0` alone);
    /// - the **knockback impulse is paid out** (EF:59695-711), which
    ///   is the only place a rival ever spends one — see
    ///   [`Mc2Rival::knock_dir`];
    /// - the commit gate is `moveTest_5D0A0` and the vertical is the
    ///   row's buoyancy `v_14` above the clearance band, the clearance
    ///   floor below it.
    ///
    /// - and `sub_5DE30`, THE DUEL ENFORCEMENT, is block 7
    ///   ([`Self::mc2_rival_duel_enforce`]). This arm's old comment
    ///   filed it under "not reproduced … no rival duel cast is
    ///   ported", which confused the STAMP with the ENFORCEMENT: the
    ///   register is retail state the import restores, and this mover
    ///   is the only place any wizard column ever reads it off a
    ///   rival. See [`no_mc2_rival_duel_death_tether`].
    ///
    /// NOT reproduced, and inert on this column: the one-shot
    /// `xAdd/yAdd/zAdd` mailbox and the water counter (neither is
    /// modelled per-rival), and the slow/mobilize scalings and their
    /// counters (a rival carries no debuff state).
    /// `sub_5DE30` (file 0x82630) DRIVEN BY A RIVAL — the duel
    /// enforcement, the state half and the leash, off the rival's own
    /// lock register instead of the human's `World::mc2_duel`. The
    /// arithmetic is [`World::mc2_duel_enforce`]'s verbatim (3-D
    /// metric, cap 120, divisor 8, SIGNED clamp — the caster is shoved
    /// back out inside the held distance), and so is the liveness set:
    /// exactly three tests, manifestation charged / victim `life >= 0`
    /// / in the tier's range, with the caster's OWN life never asked.
    ///
    /// ⭐ THE VICTIM MAY BE THE HUMAN, who has no pool record when the
    /// carpet is out of pool — hence the [`PLAYER_TARGET`] fork on the
    /// pose and the liveness read. Everything else is one body.
    ///
    /// Called from [`Self::mc2_rival_carpet_move`], which on this
    /// column is reached ONLY by the death fall — see
    /// [`no_mc2_rival_duel_death_tether`].
    fn mc2_rival_duel_enforce(
        &mut self,
        ri: usize,
        i: usize,
        pos: (u16, u16, i16),
    ) -> Option<(u16, i16)> {
        if no_mc2_rival_duel_death_tether() {
            return None;
        }
        let (opp, hold, tier) = self.mc2_rivals[ri].duel.0?;
        // `SpellEnabled[14]` on the CASTER's own book, and the death
        // scatter's boolean 1 marker is imported verbatim — so a
        // landed corpse indexes pool slot 1 here exactly as retail
        // does (file 0x826A6 `cmpw $0x0,0x2e(%edi)`).
        let m = self.mc2_rivals[ri].book.ent[14] as usize;
        let live = m != 0 && self.g.ent.get(m).is_some_and(|e| e.f26 != 0);
        let (alive, vpos) = if let Some(hx) = self.g.hx_of_tag(opp) {
            (self.human_life(hx) >= 0, self.human_pose(hx))
        } else {
            match self.g.ent.get(opp as usize) {
                None => (false, (0, 0, 0)),
                Some(e) => (e.act_life >= 0, (e.x, e.y, e.z)),
            }
        };
        let dist = Gen::mc2_dist3(pos, vpos) as i32;
        let (range, mode) = self.mc2_duel_tier(tier as usize);
        if !live || !alive || dist >= range {
            // The liveness `else`, file 0x827DC-0x827E9.
            self.mc2_rivals[ri].duel = DuelLock(None);
            return None;
        }
        let cap = 3 * 80 / 2; // `3 * minSpeed_0x84_132 / 2`, the wizard's constant 80
        let pull = ((dist - hold) / (1024 / cap)).clamp(-cap, cap) as i16;
        let bearing = Gen::angle_between(pos.0, pos.1, vpos.0, vpos.1);
        if mode >= 1 {
            self.mc2_duel_drain(opp, mode);
        }
        let _ = i;
        Some((bearing, pull))
    }

    fn mc2_rival_carpet_move(&mut self, ri: usize, i: usize) {
        // The one-shot move veto (EF:59615-19): byte[1] & 8 skips the
        // whole mover and clears itself.
        if self.g.ent[i].flags & (1 << 26) != 0 {
            self.g.ent[i].flags &= !(1 << 26);
            return;
        }
        // Block 0's tail: the published aim pitch is the (empty)
        // accumulator, `pitch_0x1E_30 = pitch_0x157_343 & 0x7FF`.
        self.g.ent[i].f32 = 0;
        // Block 1: the ±16 sign servo toward the commanded speed.
        {
            let vdes = self.mc2_rivals[ri].vdes;
            let e = &mut self.g.ent[i];
            let d = vdes - e.f126;
            if d != 0 {
                e.f126 += d.signum() * 16;
            }
        }
        let (x, y, z, mut yaw) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f30)
        };
        let mut cand = (x, y, z);
        // Block 3: the forward step, at the flat move pitch.
        Gen::polar_step(&mut cand, yaw, 0, self.g.ent[i].f126);
        // Block 4: strafe at yaw+512 — spent, never decayed.
        let strafe = self.mc2_rivals[ri].strafe;
        if strafe != 0 {
            Gen::polar_step(&mut cand, yaw.wrapping_add(512) & 0x7FF, 0, strafe);
        }
        // Block 5: the knockback impulse — cap 128, decay 4 toward
        // zero, snapped to 0 below 4.
        {
            let r = &mut self.mc2_rivals[ri];
            if r.knock_mag != 0 {
                let mag = r.knock_mag.min(128);
                r.knock_mag = mag;
                let dir = r.knock_dir;
                Gen::polar_step(&mut cand, dir, 0, mag);
                let r = &mut self.mc2_rivals[ri];
                r.knock_mag -= if mag <= 0 { -4 } else { 4 };
                if r.knock_mag.abs() < 4 {
                    r.knock_mag = 0;
                }
            }
        }
        // Block 7: `sub_5DE30` (file 0x82163, the call INSIDE
        // `sub_5D530`) — the duel enforcement, at retail's own seat
        // between the knock payout and the commit gate. Blocks 6 and
        // 8 (the one-shot xAdd mailbox / water counter, and the
        // debuff decay) are the two this column still does not model,
        // and neither touches the leash.
        if let Some((bearing, pull)) = self.mc2_rival_duel_enforce(ri, i, (x, y, z)) {
            // The yaw servo writes `a1x->yaw_0x1C_28` back (file
            // 0x8276D `mov %ax,0x1c(%esi)`), so the gate's own turn
            // below composes onto the SERVOED heading, not the one
            // this tick started with.
            let step = Gen::turn_step(yaw, bearing, 0x82);
            yaw = ((yaw as i32 + step as i32) & 0x7FF) as u16;
            self.g.ent[i].f30 = yaw;
            // …and the step's heading is the RAW bearing, at the
            // published aim pitch (`pitch_0x1E_30`, block 0's tail
            // stamped it 0 on this column).
            let pitch = self.g.ent[i].f32;
            Gen::polar_step(&mut cand, bearing, pitch, pull);
        }
        // Block 9: the commit gate and the vertical resolution.
        let row = BEHAVIOR[self.g.ent[i].row156 as usize];
        let clr = row.v_12 as i32;
        let fov = self.g.ent[i].f84 as i32;
        let out = self.g.mc2_flight_gate(fov, clr, (x, y, z), cand, false);
        let Some((p, dyaw)) = out.pass else {
            // `sub_5D530`'s `else` (EF:60077, file 0x821FE `je 0x822BE`
            // -> 0x822BF `call sub_5DD50`) — the un-gated 128-unit
            // shove out of whatever the corpse is wedged in. The human
            // column has carried it since the flight model landed
            // (`crate::flight::mc2_move`); this column is the SAME
            // retail function's other caller.
            self.mc2_rival_stuck_nudge(ri, i, fov, clr);
            return;
        };
        self.g.ent[i].f30 = ((yaw as i32 + dyaw as i32) & 0x7FF) as u16;
        let g = self.g.ground_z(p.0, p.1) as i32;
        let mut nz = p.2 as i32;
        if nz > g + clr {
            nz += row.v_14 as i32;
        }
        if nz >= g + clr {
            if let Some(c) = self.g.is_cave().then(|| self.g.ceiling_z(p.0, p.1) - 384) {
                nz = nz.min(c as i32);
            }
        } else {
            nz = g + clr;
        }
        let nz = nz.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.g.move_relink(i, p.0, p.1, nz);
    }

    /// `sub_5DD50` (EF:60157) DRIVEN BY A RIVAL — the wedged shove
    /// `sub_5D530` runs whenever `moveTest_5D0A0` refuses the commit.
    /// Verbatim: deep water at the CURRENT position (or a sealed cave
    /// tile, or — latched — a live ceiling collision) latches
    /// `byte_0x261_609` and steps the position 128 units along the
    /// entity's OWN yaw at pitch 0, publishing through
    /// `CopyEntityPosition_57CF0` (= [`Gen::move_relink`], which is
    /// also what writes `predictedAxis`, so the death puff follows the
    /// shove). Anything else clears the latch. See
    /// [`no_mc2_rival_stuck_nudge`] for the shipped bytes.
    fn mc2_rival_stuck_nudge(&mut self, ri: usize, i: usize, fov: i32, clr: i32) {
        if no_mc2_rival_stuck_nudge() {
            return;
        }
        let (x, y, z, yaw) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z, e.f30)
        };
        let latched = self.mc2_rivals[ri].nudge_latch.0;
        // A rival is never the ghost-cheat carpet.
        if self.g.mc2_flight_stuck(fov, clr, (x, y, z), latched, false) {
            self.mc2_rivals[ri].nudge_latch = NudgeLatch(true);
            let mut a = (x, y, z);
            Gen::polar_step(&mut a, yaw, 0, 128);
            self.g.move_relink(i, a.0, a.1, a.2);
        } else {
            self.mc2_rivals[ri].nudge_latch = NudgeLatch(false);
        }
    }

    /// Action 2 — the death fall (sub_5E310 EF:60074-60099).
    ///
    /// ⭐⭐⭐ ITS FIRST STATEMENT IS `sub_5D530(a1x)` — the corpse KEEPS
    /// FLYING. This arm had been Z-ONLY ("no polar drift"), so a dead
    /// rival froze where it was hit and hit the floor the same tick,
    /// scattering its spell tokens and planting its grave at the point
    /// of death. Retail's rival 378 leaves mc2l6-rsg t=1845 at
    /// (1827, 17717, 1629) and is still gliding nine ticks later at
    /// (2646, 17668, 1521) — eight tiles downrange, and the whole
    /// death payout with it. The MC2 human column has carried the
    /// same opening since the mc2l3 fall (`step_player_flight_mc2`);
    /// this is that law one wizard over.
    ///
    /// Then the gravity leg, which is Z-ONLY and is the ONLY thing on
    /// this path that is: integrate the OLD velocity, then -2/tick,
    /// terminal -256, positive (upward) velocity zeroed immediately;
    /// floor = ground + the tuning row's v_12. The (10,1) owner-
    /// flagged death puff goes at `predictedAxis_EB398ar` — the
    /// position the MOVER settled, sampled BEFORE the gravity leg
    /// rewrites it (EF:60092, the MC1/MC2 human twin's `fall_pre_z`).
    /// EXACT floor contact runs the payout.
    fn mc2_rival_death_fall(&mut self, ri: usize, i: usize) {
        self.mc2_rival_carpet_move(ri, i);
        let (x, y, pre_z) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        let ground = self.g.ground_z(x, y) as i16;
        let floor = ground.saturating_add(BEHAVIOR[self.g.ent[i].row156 as usize].v_12);
        {
            let e = &mut self.g.ent[i];
            let mut z = e.z.saturating_add(e.f46);
            e.f46 = (e.f46 - 2).clamp(-256, 0);
            if z < floor {
                z = floor;
            }
            e.z = z;
        }
        let z = self.g.ent[i].z;
        // ⭐⭐⭐ THE PUFF GOES AT RETAIL'S GLOBAL SCRATCH AXIS, NOT AT
        // THE CORPSE — `sub_5E310` pushes `0x1b398` (shipped
        // `NETHERW.EXE` file 0x82BDD `68 98 b3 01 00`, quoted in full
        // at `World::mc2_player_fall`). ⭐ ONE BODY, TWO CALL PATHS:
        // the human's dispatch and this one, so the rival column takes
        // the same read. It is a no-op on an ordinary fall — the mover
        // above ends in `move_relink`, which IS the port's model of
        // `CopyEntityPosition_57CF0(a1x, &predictedAxis)` — and it
        // differs exactly where `sub_5D530` did not run: the
        // `byte[1] & 8` whirlwind veto at the head of
        // `mc2_rival_carpet_move`, and a flight-gate rejection.
        let (px, py, pz) = if crate::mc2::roster::no_mc2_fall_puff_pred_axis() {
            (x, y, pre_z)
        } else {
            self.g.mc2_pred_axis.0
        };
        // The (10,1) death puff (EF:60092-97), owner-flagged.
        if let Some(s) = self.g.mc2_spawn_big_explosion(px, py, pz) {
            self.g.ent[s].flags |= 0x80;
            self.g.ent[s].id24 = self.mc2_rivals[ri].ent;
        }
        if z == floor {
            self.mc2_rival_death_impact(ri, i);
        }
        self.entities_dirty = true;
    }

    /// The landing payout (EF:60096-60177): kill credit, the 26
    /// SPELL-TOKEN scatter (class-15, re-collectible, lifetime
    /// rand%90+200 at +-256), the (10,40) grave, the owned-sphere
    /// re-point, the 1200 respawn timer, husk hidden.
    fn mc2_rival_death_impact(&mut self, ri: usize, i: usize) {
        // ⭐ THE PAYOUT OPENS ON A FREE-STACK REBUILD (`sub_49F90`,
        // EF:60116) — Level.cpp:1294-1300's descending 999→1 collect,
        // so the grave and the death puffs come off the LOWEST free
        // slots, not the incremental stack's order. The MC2 human
        // payout and the respawn already call it (`mc2_rebuild_free`,
        // whose own doc names EF:60101 as a site); the rival column
        // never did, so retail's grave at slot 1 and its (10,0)s at
        // 3/4/6 landed on 760/217/756/801 here.
        self.g.mc2_rebuild_free(&self.mc2_carpet_slots());
        // ⭐⭐⭐ AND THE VICTIM HALF OF THAT SAME REBUILD. `sub_49F90`
        // (Level.cpp:1271-1302) has TWO halves — the free collect
        // (:1298-1300) and the VICTIM collect (:1291-92, the descending
        // 999->1 push of every live record with `byte[2] & 2`). There are
        // NINE `sub_49F90` call sites and SEVEN clear `dword_0x11e6`
        // straight after (EF:32995, 38829, 38874, 39401, 39468, 43858,
        // and 61283 conditionally). **`sub_5E310`, the DEATH PAYOUT
        // (EF:60101), is the one that does NOT** — so the victim list it
        // builds stays ARMED for the rest of the level.
        // ⭐ AN ENUMERATED CALL-SITE LIST DECIDED IT: 7 vs 2, and the odd
        // one out is the law. Witness mc2l22: the list arms at t=73 (a
        // rival death), is whittled by `sub_57F20`'s swap-removes
        // 34->33->31->24->22->18->15->9 by t~150, and holds NINE for the
        // next thousand ticks — `dump-state 1113 665 --port` showed
        // `free stack: retail 120 / port 120` IDENTICAL but
        // `recycle stack: retail len 9 / port len 0`.
        if !no_death_payout_victims() {
            self.g.rebuild_recycle(0x2_0000);
        }
        // Kill credit: the killer wizard's per-color tally; the
        // (10,67) flood killer is suppressed (EF:60716 — no credit).
        let killer = self.g.ent[i].f38;
        let flood_kill = self
            .g
            .ent
            .get(killer as usize)
            .is_some_and(|e| e.class64 == 10 && e.model65 == 67);
        if !flood_kill {
            if let Some(k) = self.owner_slot_of_source(killer) {
                self.kill_tally[k as usize][self.mc2_rivals[ri].slot as usize] += 1;
                // The HUMAN's tally (retail's local index — seat 0 in
                // single player, his own seat under `--seat`).
                if let Some(hx) = self.hx_at_wizard(k) {
                    self.g.kills[hx] = self.g.kills[hx].saturating_add(1);
                }
            }
        }
        let slot = self.mc2_rivals[ri].slot;
        self.rival_deaths.push(slot);
        // The death broadcast (retail lang 374 "has died.") — the MC2
        // wizard name table (WizardsNames_D93A0), NOT the MC1 one.
        let name = MC2_RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
        // Notification life 100 (retail's toast countdown).
        self.set_notification(format!("{name} has died."), 100, [0xFF, 0, 0]);
        // The mailbox memset (EF:60123) — 36 bytes, the WHOLE
        // `str_0x5E_94`. It is the only thing that ever clears the
        // letter the fatal hit left parked: `sub_5EFA0`'s own consume
        // is on the SURVIVE path alone, so a corpse carries its
        // killer's amount and source all the way down the fall
        // (mc2l6-rsg rival 378 holds (200, 343) for the whole 37-tick
        // glide) and drops it here.
        self.g.ent[i].mail = [(0, 0); 6];
        // The SPELL-TOKEN scatter (EF:60137-62): every owned
        // manifestation detaches into a loose pickup token, scattered
        // +-256 around the CORPSE, lifetime rand%90+200.
        //
        // ⭐⭐ ONE RUNNING STREAM, THE DYING WIZARD'S OWN
        // (`a1x->rand_0x14_20`, EF:60153-61): three draws per token,
        // CONTINUING across all 26 — the MC2 human's own law
        // (`mc2_scatter_spells`), which this column had as 26 restarts
        // of each TOKEN's private seed. Unlike the human the rival IS
        // a pool record, so the real stream is right here and no
        // DEVIATION stand-in is needed. ⚠ SIXTEEN-BIT STEPS —
        // `rand_0x14_20` is a `uint16_t`, and `% 0x5A` reads the whole
        // word, so a 32-bit helper gets x/y right and life wrong.
        //
        // ⚠ And the writes are exactly four: flag bit 0 cleared,
        // `actionIndex++` (an INCREMENT off whatever the armed state
        // was, which is `3*model` on an idle manifestation), the
        // position, the life. Retail touches NEITHER the owner
        // (`parentId_0x28_40` keeps naming the dead wizard — the
        // graded `owner` lane on mc2l6-rsg slots 379-382) nor the
        // armed-window word.
        //
        // The book entry becomes a BOOLEAN 1, not 0 (EF:60146) — that
        // marker is the memory of what the wizard knew, and the
        // respawn re-mints exactly the non-zero entries.
        let (cx, cy, cz) = {
            let e = &self.g.ent[i];
            (e.x, e.y, e.z)
        };
        for s in 0..MC2_SPELLS {
            let m = self.mc2_rivals[ri].book.ent[s] as usize;
            if m == 0 {
                self.mc2_rivals[ri].book.ent[s] = 0;
                continue;
            }
            self.mc2_rivals[ri].book.ent[s] = 1;
            let r1 = self.g.ent_rand(i);
            let r2 = self.g.ent_rand(i);
            let life = (self.g.ent_rand(i) % 0x5A + 200) as i32;
            let jx = cx.wrapping_add((r1 & 0x1FF) as u16).wrapping_sub(256);
            let jy = cy.wrapping_add((r2 & 0x1FF) as u16).wrapping_sub(256);
            {
                let e = &mut self.g.ent[m];
                e.flags &= !1;
                e.tick70 = e.tick70.wrapping_add(1); // loose token
                e.act_life = life;
            }
            self.g.move_relink(m, jx, jy, cz);
        }
        // The grave (10,40) + the owned (10,39) sphere re-point
        // (EF:60164-77). The grave stands as the census anchor for
        // the dead wizard's loose spheres: re-owning them to the
        // grave means a wizard who later possesses the grave
        // (grave_tick, action 42) inherits the dead wizard's mana.
        //
        // ⚠ Its axis is the CORPSE's own `position_0x4C_76`, which at
        // landing sits a clearance band (`v_12`) above the terrain —
        // not the ground point this arm was re-deriving. And action 3
        // + the FLAT 1200 respawn timer (EF:60168-70) are INSIDE the
        // spawn's success arm: a wizard who dies with the pool full
        // stays in the death fall.
        if let Some(gv) = self.g.mc2_spawn_grave(cx, cy, cz) {
            let me = self.mc2_rivals[ri].ent;
            for j in 1..self.g.ent.len() {
                let e = &mut self.g.ent[j];
                if e.class64 == 10 && e.model65 == 39 && e.flags & 0x400 == 0 && e.f144 == me {
                    e.f144 = gv as u16;
                }
            }
            let e = &mut self.g.ent[i];
            e.tick70 = 3;
            e.f26 = 1200;
        }
        // The husk hides (EF:60176) — an OR alone. Clearing the
        // collide bit as well was this arm's own invention.
        self.g.ent[i].flags |= 0x20;
        // The payout's LAST statement is the bare `--dword_0x11e6`
        // (EF:60179) — it pops one entry off the victim list it just
        // built, and never resets the head. Arithmetic that closes the
        // law: with the `byte[2] & 2` stamp but without this pop, the
        // rebuild yields 35 victims on mc2l22 t=73; with it, 34 —
        // retail's exact recorded count, and the nine surviving to
        // t=1113 are exactly retail's nine.
        if !no_death_payout_victims() {
            self.g.mc2_recycle.stack.pop();
        }
        self.entities_dirty = true;
    }

    /// Action 3 — dead-wait (sub_5E7C0 EF:60254): with a castle the
    /// timer counts down to a respawn AT the castle; castle-less =
    /// BANISHED (checked every tick — losing the castle mid-wait
    /// converts to elimination).
    fn mc2_rival_dead_wait(&mut self, ri: usize, i: usize) {
        // ⭐⭐⭐ THE KNOCK MAGNITUDE IS WIPED EVERY DEAD TICK, ABOVE
        // EVERY BRANCH — `sub_5E7C0`'s first statement (EF:60660,
        // shipped file 0x82FCF `movw $0x0,0x1e(%eax)`). See
        // [`no_mc2_dead_wait_knock_clear`]. The bearing (+32) is NOT
        // touched here; only the respawn `sub_5C950` clears that.
        if !no_mc2_dead_wait_knock_clear() {
            self.mc2_rivals[ri].knock_mag = 0;
        }
        // The two call-free arms of `sub_5E7C0` (banished, countdown)
        // leave both hydra `v34` dwords alone — see
        // [`crate::engine::features::no_mc2_m27_v34_corpse_transparent`].
        let quiet = self.rival_castle(self.mc2_rivals[ri].ent).is_none() || self.g.ent[i].f26 > 0;
        if quiet && !crate::engine::features::no_mc2_m27_v34_corpse_transparent() {
            self.g.m27_v34_transparent(i);
        }
        if self.rival_castle(self.mc2_rivals[ri].ent).is_none() {
            // The FINAL-death broadcast (retail lang 283, sub_5E7C0
            // EF:60282-97: printed once on the elimination edge —
            // the byte_0x006 guard — with toast countdown 200; the
            // per-death "has died." already fired at corpse-fall).
            if !self.mc2_rivals[ri].eliminated {
                let slot = self.mc2_rivals[ri].slot;
                let name = MC2_RIVAL_NAMES.get(slot as usize).copied().unwrap_or("?");
                self.set_notification(
                    format!("{name} has been banished from the realm."),
                    200,
                    [0xFF, 0, 0],
                );
            }
            self.mc2_rivals[ri].eliminated = true;
            return;
        }
        if self.g.ent[i].f26 > 0 {
            self.g.ent[i].f26 -= 1;
            return;
        }
        self.mc2_rival_respawn(ri, i);
    }

    /// The respawn (the sub_5C950 REUSE arm, EF:43694-43706): re-
    /// anchor at the castle, full life/mana, grace 100, re-mint the
    /// remembered book at the recorded tiers, brain reset, truce.
    ///
    /// ⭐⭐⭐ **THE MAP'S PER-WIZARD DIFFICULTY SCALING IS INIT-ONLY, SO
    /// A RESPAWNED RIVAL COMES BACK RAW.** `sub_5C950` is one function
    /// for both the mint and the reuse, and everything that reads
    /// `WizardMapSettings_0x360D2` — Aggression, Perception, Reflexes
    /// and the Life scalar `word_0x24A_586` that divides the wizard's
    /// maxLife — sits inside `if (v9)`, the NEW-ENTITY arm
    /// (EF:43757-72). The reuse arm runs only the three statements
    /// above it: `word_0x24A_586 = 256`, `maxMana_0x8C_140 = 1000`,
    /// `maxLife_0x4 = 10000` (EF:43703-05), and the tail then does
    /// `life = maxLife; mana = maxMana`. A rival authored at Life 51
    /// therefore spawns with maxLife `10000·51>>8` = 1992 and comes
    /// back from its FIRST death with 10000 — five times the wizard it
    /// was — while its mana ceiling goes the other way, wiped from
    /// whatever the castle ladder had grown it to back down to 1000.
    /// The port re-filled the OLD ceiling on both lanes. mc2l6-rsg
    /// t=3084, rival 378: retail maxLife/life 1992/−25 → 10000/10000,
    /// `word_0x24A_586` 51 → 256, maxMana 1300 → 1000.
    ///
    /// ⭐⭐ **AND THE ANCHOR IS THE CASTLE'S OWN AXIS, ALL THREE
    /// COMPONENTS.** The authored spawn point is sampled and lifted to
    /// terrain+0x100 first, but `v35x = *castle_position` (EF:43690)
    /// then overwrites the WHOLE `axis_3d`, z included — so a rival
    /// re-materialises at its castle's altitude, not at the ground
    /// under it. Retail 2720 where the port's terrain+256 gave 2976.
    ///
    /// ⚠ The two zero-writes this arm used to make were the wrong
    /// record's: retail's `speed_0xc_12 = 0` and `yaw_0x1E_30 = 0` are
    /// the PLAYER struct's commanded-speed and knock registers, not
    /// the entity's `actSpeed_0x82_130` or its @0x2C — the corpse's
    /// fall velocity and its last actSpeed both survive the respawn.
    pub(crate) fn mc2_rival_respawn(&mut self, ri: usize, i: usize) {
        let Some(c) = self.rival_castle(self.mc2_rivals[ri].ent) else {
            return;
        };
        // `sub_5C950`'s FIRST statement is `sub_49F90()` (EF:43672) —
        // the same free-stack rebuild the death payout opens on, so the
        // re-minted book takes the LOWEST free slots (retail 3/4/6/7 at
        // mc2l6-rsg t=3084, where the un-rebuilt stack would have
        // handed out 672 and up).
        self.g.mc2_rebuild_free(&self.mc2_carpet_slots());
        let (cx, cy, z) = (self.g.ent[c].x, self.g.ent[c].y, self.g.ent[c].z);
        {
            let e = &mut self.g.ent[i];
            e.flags = (e.flags & !0x20) | 8;
            e.tick70 = 1;
            e.max_life = 10000;
            e.f136 = 1000; // maxMana_0x8C_140 (EF:43704)
            e.f140 = 1000; // `mana_0x90_144 = maxMana` in the tail
        }
        self.mc2_rivals[ri].life_scale = 256; // word_0x24A_586 (EF:43703)
        // `sub_5C950`'s duel-lock clear (EF:44194-95) — shipped
        // `NETHERW.EXE` file 0x81662 `66 c7 80 46 01 00 00 00 00` =
        // `movw $0x0,0x146(%eax)`, with its `word_0x148_328` twin at
        // 0x81671. The OTHER of the register's only two clears; see
        // [`no_mc2_rival_duel_death_tether`].
        self.mc2_rivals[ri].duel = DuelLock(None);
        let slot = self.mc2_rivals[ri].slot;
        // The same tail's model-1 arm (EF:43847-52) runs on the reuse
        // path too: `SpellEnabled[2] = 4 * playerColorIndex` — see
        // the fresh arm in `mc2_spawn_rival`.
        if !no_mc2_castle_colour_cooldown() {
            self.mc2_rivals[ri].cooldown[2] = 4 * slot as u16;
        }
        self.g.mc2_life_scale.0[slot as usize] = 256;
        self.g.move_relink(i, cx, cy, z);
        self.g.refill_life(i);
        // ⭐⭐ **THE BOOK IS RE-MINTED UNCONDITIONALLY, AND THE OLD
        // TOKENS ARE JUST ORPHANED.** `sub_5CF40` (EF:43770-84) walks
        // all 26 slots and, for every non-zero `SpellEnabled[i]`,
        // creates a FRESH class-15 and OVERWRITES the entry with it —
        // there is no "is the old one still alive" test, because the
        // entry it overwrites is the old token's slot index. The death
        // scatter never freed those tokens (SESSION 82 law C: it writes
        // four fields and leaves `parentId_0x28_40` naming the dead
        // wizard), so retail's world simply accumulates the loose ones.
        // The port's `book.ent[s] == 0` guard meant a rival that died
        // once respawned with an EMPTY book — 72 missing class-15s at
        // mc2l6-rsg t=3084.
        //
        // ⭐⭐⭐ **AND THE SLOT IT WALKS IS THE BOOK'S OWN, NOT THE
        // AUTHORED GRANT.** `sub_5CF40`'s gate is
        // `cmpw $0x0,0x333(%edi,%eax,1)` (file 0x81760) — the death
        // scatter's BOOLEAN 1 marker, i.e. what the wizard actually
        // held when it died — and the NULL-mint arm at file 0x817d0
        // writes the slot back to **0**, losing the spell for good.
        // The port asked [`Mc2Rival::known`], a non-retail shadow the
        // four round-104 brain gates were already converted away from.
        // See [`no_mc2_rival_remint_marker`].
        let marker = no_mc2_rival_remint_marker();
        for s in 0..MC2_SPELLS {
            let held = if marker {
                self.mc2_rivals[ri].known[s]
            } else {
                self.mc2_rivals[ri].book.ent[s] != 0
            };
            if held {
                let r = &self.mc2_rivals[ri];
                let ent = r.ent;
                let sel = r.book.sel[s];
                let (x, y, zz) = {
                    let e = &self.g.ent[i];
                    (e.x, e.y, e.z)
                };
                if let Some(m) = self.mc2_new_spell_token(s as u8, x, y, zz) {
                    {
                        let e = &mut self.g.ent[m];
                        e.tick70 = (s as u8).wrapping_mul(3);
                        // The respawn re-mint is a reify too — no lock.
                        // [`crate::engine::features::no_mc2_pickup_only_steal_lock`].
                        if crate::engine::features::no_mc2_pickup_only_steal_lock() {
                            e.f54 = 64;
                        }
                        e.id24 = ent;
                        e.f26 = 0;
                        e.f44 = 0;
                    }
                    self.mc2_rival_set_spell(m, sel, ent);
                    self.mc2_rivals[ri].book.ent[s] = m as u16;
                } else if !marker {
                    // `movw $0x0,0x333(%edi,%eax,1)` (file 0x817d0) —
                    // a pool-exhausted re-mint LOSES the spell. The
                    // port's `known` shadow has to follow it, or the
                    // next respawn would resurrect a book slot retail
                    // has already zeroed.
                    self.mc2_rivals[ri].book.ent[s] = 0;
                    self.mc2_rivals[ri].known[s] = false;
                }
            }
        }
        {
            let r = &mut self.mc2_rivals[ri];
            // maxMana wiped to the base 1000 — the mana progression
            // does NOT survive death (EF:43722).
            r.mana = 1000;
            r.mana_max = 1000;
            // ⚠ `manaRegen_0x88_136` is NOT touched: `sub_5C950` never
            // writes it, and the dead-wait action the respawn runs
            // inside is not the alive action whose tail re-selects it —
            // so the rate the wizard died holding is the rate its first
            // live tick spends. mc2l6-rsg t=3085: retail carries the
            // afield 100 across the seam (mana 1000 → 1100) where a
            // zeroed delta left the purse flat.
            r.grace = 100;
            // ⭐⭐⭐ **THE REUSE ARM'S ZERO-WRITES ARE AN ENUMERATED
            // LIST, AND THE COMBAT FSMs ARE NOT ON IT.** `sub_5C950`
            // writes exactly `word_0x159_345`, `word_0x24C_588`,
            // `dword_0x16D_365`, `speed_0xc_12`, `yaw_0x1E_30`,
            // `fov_0x22_34`, `moveBoost_0x1E_30`,
            // `strafeSpeed_0x10_16`, `memset(&str_0x1AC_428, 0, 18)`,
            // `word_0x24A_586`, `word_0x146_326`/`word_0x148_328`,
            // and — only under `model_0x40_64 == 1` — the brain byte
            // `byte_0x1C1_449`, the eight hate words and
            // `SpellEnabled[2]` (EF:43694-43843). The burst counter
            // `word_0x1A2_418` and the poverty latch `word_0x1A4_420`
            // sit BELOW the 18-byte memset's 0x1AC..0x1BD window; the
            // weave and water-steer FSMs live in `str_611` at
            // 0x45C..0x45F; and the target `word_0x96_150` is an
            // ENTITY field the reuse arm never re-enters. **A rival
            // comes back from death mid-weave, mid-burst, still
            // pointed at whatever it last picked.**
            //
            // The port zeroed all six, and `weave = 0` is the one that
            // bites: tick 0 of `mc2_rival_weave` is the arm that DRAWS
            // THE ENTITY LCG and snaps the actual yaw ±512, where
            // ticks 3..19 only coast. mc2l6-rsg rival 378 respawns at
            // t=3084 holding `weave` 8 / `burst` 2 / `target` 174 and
            // coasts; the port re-rolled a fresh jink, spent a draw
            // nothing else spends and steered off — a debt that rode
            // 285 ungraded ticks before it surfaced as the wizard's
            // own pitch/z at t=3369.
            r.state = Mc2AiState::Fresh;
            r.strafe = 0;
            // `speed_0xc_12`/`yaw_0x1E_30`/`moveBoost_0x1E_30` are all
            // PLAYER-struct registers (EF:43698-43701) — the commanded
            // speed and the banked knock, not anything on the entity.
            r.vdes = 0;
            r.knock_dir = 0;
            r.knock_mag = 0;
            // Own hate ledger back to neutral (EF:43848-50); the WAR
            // latches survive death — retail never clears them here.
            r.hate = [HATE_NEUTRAL; 8];
            // Cooldowns are KEPT except the castle slot, staggered
            // by color: SpellEnabled[2] = 4·color (EF:43851) — else
            // every respawn rebuilds its castle at once.
            r.cooldown[2] = 4 * r.slot as u16;
        }
        // ⭐⭐⭐ AND THE RESPAWN DISARMS THE VICTIM STACK IT RE-RANKED.
        // `sub_5C950`'s last statement before the speed-index reset is
        // `D41A0_0.dword_0x11e6 = -1` (EF:43858) — the eighth of the
        // nine `sub_49F90` sites to drop the victim half on the floor;
        // only the death payout `sub_5E310` (EF:60101) leaves it armed.
        // The opening rebuild above collected every live `byte[2] & 2`
        // record — on mc2l22 that is 77 (5,15) creatures — and without
        // this clear the port kept them ARMED across the whole life:
        // t=4957 (rival respawn) to t=5141, where a lightning beam's
        // trail emptied the free stack (retail 81→0, recycle 0→0,
        // spawns dropped) and the port seized slot 21 — a LIVE (5,15)
        // in both worlds — for a (9,9) node. Retail's recorded recycle
        // stack stays empty through the whole window.
        self.g.mc2_recycle.stack.clear();
        // The post-respawn truce toward this color — the tick-top
        // class-3 roster, NOT the rival vector: a wizard still in its
        // own dead-wait is off `dword_38519` and takes no truce
        // (EF:44185-93). `MGC_NO_MC2_RIVAL_TRUCE_ROSTER=1` restores
        // the flat every-rival loop.
        let slot = self.mc2_rivals[ri].slot as usize;
        if no_mc2_rival_truce_roster() {
            for (oj, o) in self.mc2_rivals.iter_mut().enumerate() {
                if oj != ri {
                    o.hate[slot] = HATE_RESPAWN;
                }
            }
        } else {
            let own = self.mc2_rivals[ri].ent;
            self.mc2_truce_roster(slot, own);
        }
        self.entities_dirty = true;
    }

    /// ⭐⭐⭐ **THE POST-DEATH TRUCE IS NOT A RIVAL-ONLY LAW.**
    /// `sub_5C950` is ONE routine for every wizard, and its truce loop
    /// (EF:44185-93, shipped NETHERW.EXE 0x5CE22) sits below the
    /// `IsAiPlayer` fork with no gate: whoever just respawned, every
    /// OTHER class-3 model-0/1 record on the tick-top roster
    /// `dword_38519` ([`Gen::wiz_chain`]) takes
    /// `hate[respawner colour] = 0x9FDF` = [`HATE_RESPAWN`]. The port
    /// had the write on the two RIVAL call paths only
    /// ([`Self::mc2_spawn_rival`] and the respawn above), so a HUMAN
    /// respawn left every rival's `hate[0]` wherever
    /// [`Self::mc2_rival_hate_decay`] had carried it — the §5 class,
    /// a known-correct law that never reached one call path.
    ///
    /// ⚠ THE ROSTER, NOT THE RIVAL VECTOR. `dword_38519` only ever
    /// holds `life_0x8 >= 0` records (EF:39975), so a rival that is
    /// DEAD when the human respawns takes no truce at all.
    ///
    /// WITNESS (mc2l16 t=7934, the human's respawn; the rivals' own
    /// decay runs later in the same tick, `-(256 - Aggression)`):
    ///
    /// | wiz | t=7933 | port (no truce) | retail t=7934 |
    /// |-----|--------|-----------------|---------------|
    /// | 1   | 24607  | 24607           | 40914 = 40927 − 13 |
    /// | 4   | 45303  | 45279 = −24      | 40903 = 40927 − 24 |
    /// | 5   | 54505  | 54459 = −46      | 40881 = 40927 − 46 |
    ///
    /// The debt is INVISIBLE for 1,131 boundaries — `hate` is an
    /// ungraded lane — and surfaces at t=9066, where rival 372's
    /// target election (`sub_14030`) picks 452 where retail picks 389
    /// and the wizard's `z` steps −4 instead of −8.
    pub(crate) fn mc2_respawn_truce(&mut self, colour: usize) {
        if no_mc2_human_respawn_truce() || colour >= 8 {
            return;
        }
        // The human carpet is not in `mc2_rivals`, so nothing on the
        // roster can be the respawner here; pass the sentinel.
        self.mc2_truce_roster(colour, u16::MAX);
    }

    /// The truce loop itself (shipped NETHERW.EXE 0x5CE1A..0x5CE5C):
    /// walk the TICK-TOP class-3 chain `dword_38519`
    /// ([`Gen::wiz_chain`]) and stamp `hate[colour] = 0x9FDF` on every
    /// member whose `model_0x40_64` is 0 (the human) or 1 (a rival),
    /// skipping the record whose `id_0x1A_26` matches the
    /// respawner's. Membership is `life_0x8 >= 0` sampled at the tick
    /// top, so a wizard in its dead-wait takes no truce, and at
    /// level-start seating the chain is still empty. The port's
    /// `wiz_chain` is already the class-3 arm of the same sweep, so
    /// the model test is implicit — only wizards are in `mc2_rivals`.
    ///
    /// See [`no_mc2_rival_truce_roster`] for the two rival call paths
    /// and [`no_mc2_human_respawn_truce`] for the human one.
    pub(crate) fn mc2_truce_roster(&mut self, colour: usize, own_ent: u16) {
        if colour >= 8 {
            return;
        }
        for c in 0..self.g.wiz_chain.visible_len() {
            let j = self.g.wiz_chain.list[c] as usize;
            if j == own_ent as usize {
                continue;
            }
            if let Some(r) = self.mc2_rivals.iter_mut().find(|r| r.ent as usize == j) {
                r.hate[colour] = HATE_RESPAWN;
            }
        }
    }
}

impl Gen {
    /// The (10,40) wizard grave: the census anchor the dead wizard's
    /// (10,39) spheres re-point to (EF:60164). Its action body is
    /// `sub_36AE0` (EF:26835) = action 42 = the shared `grave_tick`
    /// (byte-exact with MC1 `spawn_grave`/`grave_tick`, features.rs):
    /// a class-3 wizard's ch1 possession claim inherits EVERYTHING the
    /// grave owns (its re-pointed mana spheres) then despawns it, so
    /// possessing the corpse reclaims the dead wizard's loose mana.
    /// KEEP targetable bit 8 (the possess bolt must be able to hit it)
    /// and set `f28 = 2` (the ch1 claim channel), matching MC1.
    pub(crate) fn mc2_spawn_grave(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let s = self.new_event()?;
        {
            let e = &mut self.ent[s];
            e.class64 = 10;
            e.model65 = 40;
            e.tick70 = 42;
            e.f26 = (s % 11) as i16;
            e.f28 = 2;
            // ⚠⚠ …AND THE MASK'S **SECOND** HOME, THE MC2 ONE.
            // `sub_501D0` (the (10,40) ctor, `EventsFunctions.cpp`
            // signature line 36711, banner `(000501D0)`) is
            // ```text
            // event->actionIndex_0x45_69 = 0x2A;
            // event->class_0x3F_63 = 0xA;   event->model_0x40_64 = 0x28;
            // event->dword_0x10_16 = (event - Entities) % 11;
            // event->byte_0x38_56 = 2;
            // ```
            // `byte_0x38_56` is `@0x38`, which the port keeps at `f28`
            // for most classes but at **`f56` for class 2/10** —
            // `import_ent_mc2` seats BOTH (`f28: r.b38`, and
            // `f56: if matches!(class, 2 | 10) { r.b38 }`) and
            // `port_ent_lanes_mc2` publishes `f56` on the `b38` lane
            // for class 2/10. This is the exact sibling of the law
            // `Gen::spawn_mana_ball` already carries ("⚠⚠ THE MASK HAS
            // TWO HOMES AND MC2's WAS EMPTY", mc1/combat.rs) — the
            // grave ctor set only the MC1 home, so every free-run MC2
            // grave published mask 0 where retail publishes 2.
            // Census: `(10,40) b38` = **290,065 rows over 29 takes**,
            // every one `retail 2 / port 0` (mc2l8 slot 1 from t=2920,
            // mc2l6 slot 1 from t=1205). `MGC_NO_MC2_GRAVE_MASK_HOME=1`
            // reverts. State-only: `area_write`'s admit test reads the
            // MC1 home `f28`, which is already right.
            if !no_mc2_grave_mask_home() {
                e.f56 = 2;
            }
        }
        self.link(s, x, y, z);
        self.refill_life(s);
        self.mc2_set_sprite(s, 65);
        Some(s)
    }
}

// ------------------------------------------------------------ snapshot

use crate::snapshot::{Reader, Snap, SnapshotError, Writer, snap_enum};

snap_enum!(
    Mc2AiState,
    "Mc2AiState",
    0 => Mc2AiState::Fresh,
    1 => Mc2AiState::Upgrade,
    2 => Mc2AiState::Build,
    3 => Mc2AiState::Possess,
    4 => Mc2AiState::RaidCastle,
    5 => Mc2AiState::AttackWizard,
    6 => Mc2AiState::RaidBalloon,
    7 => Mc2AiState::HuntMana,
    8 => Mc2AiState::Home,
    9 => Mc2AiState::Cruise,
    10 => Mc2AiState::Defense,
);

impl Snap for Mc2Rival {
    fn put(&self, w: &mut Writer) {
        let Mc2Rival {
            slot,
            ent,
            book,
            known,
            cooldown,
            mana,
            mana_max,
            mana_delta,
            life_delta,
            agg,
            per,
            refl,
            life_scale,
            state,
            hate,
            war,
            burst,
            poverty,
            target,
            target_sig,
            site,
            strafe,
            weave,
            weave_dir,
            shield_state,
            avoid,
            avoid_exit,
            vdes,
            knock_dir,
            knock_mag,
            duel,
            v14,
            nudge_latch,
            grace,
            eliminated,
            shield,
            invisible,
            rebound,
        } = self;
        w.put(slot);
        w.put(ent);
        w.put(book);
        w.put(known);
        w.put(cooldown);
        w.put(mana);
        w.put(mana_max);
        w.put(mana_delta);
        w.put(life_delta);
        w.put(agg);
        w.put(per);
        w.put(refl);
        w.put(life_scale);
        w.put(state);
        w.put(hate);
        w.put(war);
        w.put(burst);
        w.put(poverty);
        w.put(target);
        w.put(target_sig);
        w.put(site);
        w.put(strafe);
        w.put(weave);
        w.put(weave_dir);
        w.put(shield_state);
        w.put(avoid);
        w.put(avoid_exit);
        w.put(vdes);
        w.put(knock_dir);
        w.put(knock_mag);
        // SNAPSHOT 24 — retail persists the lock too
        // (`engine_support.cpp:678`: `S164SC(dword_0x142_322, 4);
        // S164SC(word_0x146_326, 2);`).
        w.put(&duel.0);
        w.put(&v14.0);
        w.put(&nudge_latch.0);
        w.put(grace);
        w.put(eliminated);
        w.put(shield);
        w.put(invisible);
        w.put(rebound);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Mc2Rival {
            slot: r.get()?,
            ent: r.get()?,
            book: r.get()?,
            known: r.get()?,
            cooldown: r.get()?,
            mana: r.get()?,
            mana_max: r.get()?,
            mana_delta: r.get()?,
            life_delta: r.get()?,
            agg: r.get()?,
            per: r.get()?,
            refl: r.get()?,
            life_scale: r.get()?,
            state: r.get()?,
            hate: r.get()?,
            war: r.get()?,
            burst: r.get()?,
            poverty: r.get()?,
            target: r.get()?,
            target_sig: r.get()?,
            site: r.get()?,
            strafe: r.get()?,
            weave: r.get()?,
            weave_dir: r.get()?,
            shield_state: r.get()?,
            avoid: r.get()?,
            avoid_exit: r.get()?,
            vdes: r.get()?,
            knock_dir: r.get()?,
            knock_mag: r.get()?,
            duel: DuelLock(r.get()?),
            v14: BrakeWord(r.get()?),
            nudge_latch: NudgeLatch(r.get()?),
            grace: r.get()?,
            eliminated: r.get()?,
            shield: r.get()?,
            invisible: r.get()?,
            rebound: r.get()?,
        })
    }
}

#[cfg(test)]
mod grave_tests {
    use crate::chassis::ChassisParams;
    use crate::engine::features::{FeatureAssets, Gen, Planes};
    use crate::verbs::VerbSet;

    fn flat_mc2_gen() -> Gen {
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
        Gen::new(planes, assets, 1, ChassisParams::MC2, VerbSet::MC2)
    }

    /// ⚠⚠ THE MAIL-CHANNEL ADMIT MASK HAS TWO HOMES AND THE GRAVE'S
    /// MC2 ONE WAS EMPTY — the exact sibling of the law
    /// `Gen::spawn_mana_ball` already carries. `sub_501D0` writes
    /// `byte_0x38_56 = 2`; `@0x38` lives in `f28` for most classes but
    /// in `f56` for class 2/10, which is the home `import_ent_mc2`
    /// seats and `port_ent_lanes_mc2` publishes as `b38`. Census:
    /// `(10,40) b38` = 290,065 rows over 29 takes, all
    /// `retail 2 / port 0`.
    /// FAILS under `MGC_NO_MC2_GRAVE_MASK_HOME=1`; the `f28`, `f26`
    /// and action assertions are the POSITIVE CONTROL.
    #[test]
    fn the_wizard_grave_stamps_both_homes_of_its_admit_mask() {
        let mut g = flat_mc2_gen();
        let s = g
            .mc2_spawn_grave(40 << 8, 40 << 8, 100)
            .expect("grave slot");
        assert_eq!(g.ent[s].f56, 2, "byte_0x38_56 = 2, the MC2 class-10 home");
        // POSITIVE CONTROL — the rest of `sub_501D0`.
        assert_eq!(g.ent[s].f28, 2, "…and the MC1 home the ctor already had");
        assert_eq!(g.ent[s].f26, (s % 11) as i16, "dword_0x10_16 = slot % 11");
        assert_eq!(g.ent[s].tick70, 42, "actionIndex_0x45_69 = 0x2A");
        assert_eq!((g.ent[s].class64, g.ent[s].model65), (10, 40));
    }
}

/// The human at a colour other than 0 (`Gen::human_seat`) — MC2's
/// twin of `mc1::rivals::human_seat_tests`, on baked arena level 53
/// (eight authored seats, NumberOfPlayers 8). Self-skips without
/// baked data.
#[cfg(test)]
mod human_seat_tests {
    use super::Mc2RivalConfig;
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::World;
    use crate::ids::GameId;
    use crate::{FlightInput, Simulation, ThrustModel};

    struct Arena {
        planes: Planes,
        things: Vec<mgc_formats::Thing>,
        seed: u32,
        assets: FeatureAssets,
        stages: Vec<(i8, i16, i16, i16)>,
        vars: Vec<(i8, i8, u8, u8, u32)>,
        configs: [Option<Mc2RivalConfig>; 8],
        count: u16,
    }

    fn arena() -> Option<Arena> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../baked");
        let lp = root.join("mc2/level-053.mgcl");
        if !lp.exists() {
            eprintln!("skip: baked mc2 level 53 not present");
            return None;
        }
        let pkg: mgc_formats::LevelPackage =
            mgc_formats::mgcl::read(std::fs::File::open(lp).ok()?).ok()?;
        let bundle = mgc_formats::bundle::Bundle::load(&root.join("assets/mc2-day")).ok()?;
        let assets = FeatureAssets::parse(
            bundle.search.as_ref()?,
            bundle.build_tab.as_ref()?,
            bundle.build_dat.as_ref()?,
        )
        .ok()?
        .with_bldgprm(bundle.bldgprm.as_deref().unwrap_or_default());
        let assets = match bundle.spells.as_deref() {
            Some(sp) => assets.with_spells(sp).ok()?,
            None => assets,
        };
        let terrain = pkg.terrain.as_ref()?;
        let planes = Planes {
            height: terrain.height.clone(),
            tile_type: terrain.tile_type.clone(),
            shading: terrain.shading.clone()?,
            angle: terrain.angle.clone()?,
            ceiling: terrain.ceiling.clone().unwrap_or_default(),
        };
        let (stages, vars) = pkg.stages.as_ref().map_or((Vec::new(), Vec::new()), |st| {
            (
                st.checkpoints
                    .iter()
                    .map(|c| (c.index, c.stage, c.x, c.y))
                    .collect(),
                st.variables
                    .iter()
                    .map(|v| (v.index, v.stage, v.x, v.y, v.data))
                    .collect(),
            )
        });
        // The app's `mc2_rival_configs`, every colour.
        let (wiz, h) = (pkg.wizards.as_ref()?, pkg.header.as_ref()?);
        let mut configs: [Option<Mc2RivalConfig>; 8] = Default::default();
        for (slot, c) in wiz.wizards.iter().enumerate().take(8) {
            let (Some(reflexes), Some(perception)) = (c.reflexes, c.perception) else {
                continue;
            };
            let mut start = [false; 26];
            let mut start_level = [0u8; 26];
            let mut blocked = [false; 26];
            for s in 0..26 {
                start[s] = c.starting_spells.get(s).copied().unwrap_or(0) != 0;
                start_level[s] = c.starting_spell_levels.get(s).copied().unwrap_or(0).min(2);
                blocked[s] = c.blocked_spells.get(s).copied().unwrap_or(0) != 0;
            }
            configs[slot] = Some(Mc2RivalConfig {
                aggression: c.aggression.clamp(0, 255) as u8,
                perception: perception.clamp(0, 255) as u8,
                reflexes: reflexes.clamp(0, 255) as u8,
                life: c.life.unwrap_or(0).max(0) as u16,
                castle_level: h.players[slot].max(0) as u8,
                start,
                start_level,
                blocked,
            });
        }
        Some(Arena {
            planes,
            things: pkg.things.things.clone(),
            seed: pkg.gen_params.as_ref().map_or(0, |g| g.seed),
            assets,
            stages,
            vars,
            configs,
            count: h.number_of_players.clamp(1, 8) as u16,
        })
    }

    /// The app's build order: constructor, stage board, roster.
    fn build(a: &Arena, seat: u8, classic: bool) -> World {
        let mut w = if classic {
            World::new_full_env(
                a.planes.clone(),
                &a.things,
                a.seed,
                a.assets.clone(),
                GameId::Mc2.chassis(),
                GameId::Mc2,
                false,
            )
        } else {
            World::new_full_env_seated(
                a.planes.clone(),
                &a.things,
                a.seed,
                a.assets.clone(),
                GameId::Mc2.chassis(),
                GameId::Mc2,
                false,
                seat,
            )
        };
        if !a.stages.is_empty() {
            w.set_mc2_stages(&a.stages);
        }
        if !a.vars.is_empty() {
            w.set_mc2_stagevars(&a.vars);
        }
        if classic {
            w.set_mc2_wizards(&a.configs, a.count);
        } else {
            w.seat_mc2_wizards(&a.configs, a.count);
        }
        w
    }

    fn sim(w: World) -> Simulation {
        let mut s = Simulation::with_world(w);
        s.thrust_model[crate::engine::world::Hx::FIRST] = ThrustModel::Mc1;
        s.sync_carpet_from_flyer(crate::engine::world::Hx::FIRST);
        s
    }

    /// THE DEV-SPELLS TOGGLE RE-STAMPS THE BOOK (player report
    /// 2026-11-22): the instrument's castle-upkeep bypass lives on each
    /// manifestation (`SetSpell`), so a spell whose gated top tier was
    /// selected BEFORE the toggle must lose its gate when the cheat goes
    /// on (it stayed blocked until re-selected) and get it back when the
    /// cheat goes off; a tier picked only through the cheat drops back
    /// to the earned one.
    #[test]
    fn the_dev_spells_toggle_restamps_held_spells() {
        use crate::engine::world::Hx;
        let Some(a) = arena() else { return };
        let mut w = build(&a, 0, true);
        let hx = Hx::FIRST;
        // A spell/tier with a castle-pool prerequisite.
        let Some((s, t, req)) = (0..26usize).find_map(|s| {
            let row = w.g.assets.spells.get(s)?;
            let n = (row.byte_0 as usize).clamp(1, 3);
            (1..n).find_map(|t| {
                let r = row.tiers[t].max_mana_limit;
                (r > 0).then_some((s, t, r))
            })
        }) else {
            eprintln!("skip: no gated tier in the arena's SPELLS table");
            return;
        };
        // Earn it honestly (XP way past the ladder), select it.
        w.mc2_grant_plausible(hx, &[(s as u8, 1 << 30)]);
        assert!(w.mc2_book[hx].levels[s] as usize >= t, "tier {t} earned");
        w.mc2_select_spell(hx, s as u8, t as u8, 0);
        let m = w.mc2_book[hx].ent[s] as usize;
        assert_eq!(w.g.ent[m].f136, req, "the real castle-pool gate");
        w.set_dev_spells(hx, true);
        assert_eq!(w.g.ent[m].f136, 0, "the cheat lifts an already-set gate");
        w.set_dev_spells(hx, false);
        assert_eq!(w.g.ent[m].f136, req, "and restores it when it goes off");

        // A cheat-only tier falls back to the earned one.
        let mut w = build(&a, 0, true);
        w.mc2_grant_plausible(hx, &[(s as u8, 0)]);
        let earned = w.mc2_book[hx].levels[s];
        assert!((earned as usize) < t);
        w.set_dev_spells(hx, true);
        w.mc2_select_spell(hx, s as u8, t as u8, 0);
        assert_eq!(w.mc2_book[hx].sel[s] as usize, t);
        w.set_dev_spells(hx, false);
        assert_eq!(w.mc2_book[hx].sel[s], earned, "the cheat's tier is not kept");
        let m = w.mc2_book[hx].ent[s] as usize;
        assert_eq!(w.g.ent[m].f71, earned, "and the manifestation follows");
    }

    /// PATCH `mc2_fools_mana_skip`. A rival at Perception 255 (every
    /// roll lands) walks a sphere chain of [fool's sphere, real ball].
    /// Retail's 57-break abandons the walk at the fool's sphere, so the
    /// real ball is never elected; patched, the rival skips the decoy
    /// and takes the ball.
    #[test]
    fn a_perceptive_rival_skips_fools_mana_only_under_the_patch() {
        let Some(a) = arena() else { return };
        for patched in [false, true] {
            let mut w = build(&a, 0, true);
            w.set_patches(crate::WorldPatches {
                mc2_fools_mana_skip: patched,
                ..crate::WorldPatches::RETAIL
            });
            let ri = (0..w.mc2_rivals.len())
                .find(|&r| !w.mc2_rivals[r].eliminated)
                .expect("a live rival");
            w.mc2_rivals[ri].per = 255;
            let i = w.mc2_rivals[ri].ent as usize;
            let (x, y, z) = (w.g.ent[i].x, w.g.ent[i].y, w.g.ent[i].z);
            let fool = w
                .g
                .mc2_spawn_mana_sphere(57, x.wrapping_add(4 << 8), y, z)
                .expect("fool's sphere");
            let ball = w
                .g
                .spawn_mana_ball(x.wrapping_add(8 << 8), y, z)
                .expect("mana ball");
            assert_eq!(w.g.ent[fool].model65, 57);
            // Chain order is the test: the decoy first.
            w.g.ball_chain.list = vec![fool as u16, ball as u16];
            w.g.ball_chain.cut = usize::MAX;
            w.mc2_rival_pick_ball(ri, i);
            let elected = w.mc2_rivals[ri].target as usize == ball
                && w.mc2_rivals[ri].state == super::Mc2AiState::Possess;
            assert_eq!(
                elected, patched,
                "patched={patched}: the real ball is elected only under the patch"
            );
        }
    }

    #[test]
    fn colour_zero_is_the_classic_build() {
        let Some(a) = arena() else { return };
        let (mut c, mut s) = (sim(build(&a, 0, true)), sim(build(&a, 0, false)));
        assert_eq!(c.state_hash(), s.state_hash(), "fresh build");
        let fly = FlightInput {
            thrust: 1.0,
            stick_x: 40,
            ..Default::default()
        };
        for t in 1..=300 {
            c.step(&fly);
            s.step(&fly);
            assert_eq!(c.state_hash(), s.state_hash(), "tick {t}");
        }
    }

    #[test]
    fn human_flies_colour_four_and_colour_zero_is_a_rival() {
        let Some(a) = arena() else { return };
        assert_eq!(a.count, 8, "level 53 seats eight wizards");
        let w = build(&a, 4, false);
        assert_eq!(w.g.human_seat(crate::engine::world::Hx::FIRST), 4);
        let (mx, my) = w.start_markers[4].expect("level 53 authors colour 4");
        assert_eq!(
            (
                w.human_pose(crate::engine::world::Hx::FIRST).0,
                w.human_pose(crate::engine::world::Hx::FIRST).1
            ),
            ((mx << 8) | 128, (my << 8) | 128)
        );
        let mut seats: Vec<u8> = w.mc2_rivals.iter().map(|r| r.slot).collect();
        seats.sort_unstable();
        assert_eq!(seats, vec![0, 1, 2, 3, 5, 6, 7]);
        // `sub_5C950` per colour in order: the human's record between
        // colours 3 and 5.
        let ent_of = |s: u8| w.mc2_rivals.iter().find(|r| r.slot == s).unwrap().ent;
        let hs = w.mc2_carpet_slot[crate::engine::world::Hx::FIRST];
        assert!(
            ent_of(3) < hs && hs < ent_of(5),
            "{} < {hs} < {}",
            ent_of(3),
            ent_of(5)
        );
        assert_eq!(
            w.g.owner_team(w.g.human_tag[crate::engine::world::Hx::FIRST].0),
            Some(4)
        );
        assert_eq!(w.g.owner_team(ent_of(0)), Some(0));
        assert_eq!(w.g.rival_ents[0], ent_of(0));
    }

    #[test]
    fn colour_four_plays_and_round_trips() {
        let Some(a) = arena() else { return };
        let mut live = sim(build(&a, 4, false));
        let fly = FlightInput {
            thrust: 1.0,
            stick_x: -30,
            fire_left: true,
            ..Default::default()
        };
        for _ in 0..600 {
            live.step(&fly);
        }
        let w = live.world.as_ref().unwrap();
        assert!(
            !w.mc2_rivals
                .iter()
                .find(|r| r.slot == 0)
                .unwrap()
                .eliminated
        );
        let bytes = live.snapshot();
        let mut restored = sim(build(&a, 4, false));
        restored.restore(&bytes).expect("restore");
        assert_eq!(live.state_hash(), restored.state_hash());
        for t in 1..=300 {
            live.step(&fly);
            restored.step(&fly);
            assert_eq!(live.state_hash(), restored.state_hash(), "tick {t}");
        }
        assert_ne!(
            sim(build(&a, 4, false)).state_hash(),
            sim(build(&a, 0, false)).state_hash()
        );
    }
}
