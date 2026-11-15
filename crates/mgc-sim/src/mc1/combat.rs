//! MC1 combat: damage mailboxes, class-9 projectiles, class-10
//! combat effects (fire/explosion, fire-spreader, splash, blast ring,
//! hit-flash, mana-steal flash, mana ball) and the corpse pipeline —
//! ports of remc1 sub_main.cpp. Full specs in docs/ROADMAP.md
//! ("Combat, damage, death & corpses", "Fireball / repeat fireball").
//!
//! Deviations from the decompile:
//! - `sub_12B50`'s inverted accumulate/overwrite is NOT ported; the
//!   direct write uses the area writers' protocol (:17301-05)
//!   (deliberate: suspect transcription swap, like :21814).
//! - The m9 ranged thunk aims at the TARGET, not the atan2(0,0)
//!   self-aim (:21947-48) (deliberate: decompile casualty).
//! - (RETIRED) Aim assist now runs sub_54A90's exact distance-weighted
//!   score, 2-D range and v_28 class-3 pre-gate in every acquire
//!   subtype (see `Gen::acquire_score`); the Δyaw² + Δpitch²
//!   approximation is gone.
//! - The m9 lightning BEAM (sub_535E0 :63272) is a full port (one-tick
//!   hitscan walk + state-14 segment chain, confirmed vs remc2
//!   sub_66750); the explosion's +146 stamps hit-or-0 where the
//!   original writes garbage on a miss (deliberate).
//! - Class-9 model 14 / state 15 (the Troll & Ape boulder): remc1's
//!   class-9 tick table is truncated, but CARPET.EXE's relocated
//!   table binds 0xF to the bare sub_52770 thunk — the boulder runs
//!   the generic homing flight (no fire trail, no acquire for m14),
//!   pre-targeted by its throw ctor. It must NOT alias onto state
//!   13, whose first-tick roll is the arrow quartet.
//! - Mana-shield reflection (+17 bit 7) is ported but nothing sets the
//!   flag yet (OPEN: wizard shields are the spell track).

use crate::engine::features::{
    Ent, Gen, lcg32, no_mc1_castle_ball_stepback_moves_ball, no_mc1_eruption_counter_reread, tile,
};
use crate::engine::world::Hx;
use crate::mc1::behavior::BEHAVIOR;
use crate::mc1::mobs::{MC1_MISS_STAMP, MobCtx};
use crate::mc1::sprite_stats::SPRITE_STATS;
use crate::verbs::{CorpseVerb, TargetingVerb, VerbKind};

/// A/B toggle for **THE MANA MAGNET HOMES ON GRAVES TOO** (round 156,
/// w156a): set `MGC_NO_MC1_MAGNET_HOMES_ON_GRAVES` to restore the
/// pre-dig model-39-only filter on the m17 magnet bolt's acquire walk.
/// Retail: `sub_54520` case 0x11 (remc1hw `sub_54520_548B0`, the
/// `case 0x11:` arm after the `//----- (00054520)` body, hw:60386-405)
/// walks the tick-top ball roster `var_u32_36462[1]` and tests ONLY
/// `+58` — no model, class, owner or claim test. CARPET.EXE jump
/// table at VA 0x544CC (+65 index, `cmp $0x13,%al` at 0x54533) sends
/// case 0x11 to VA 0x548FF (file 0x6D0F7): `mov 0x8e72(%edi),%edi`
/// (roster head, +36466) / 0x54914 `cmpb $0x0,0x3a(%edi)` (+58) / `je`
/// next / `push 0x71; push 0x71; push edi; push esi; call sub_54A90` /
/// `cmp %ebp,%eax; jae` (unsigned best) / 0x54930 `mov (%edi),%edi`
/// (next). The roster holds class-10 m39 balls AND m40 graves
/// (`rebuild_ball_chain`), so an awake grave inside the 0x71 cone is a
/// homing target. (The IMPACT scan `sub_11C00` is m39-only and stays
/// so — [`Gen::possess_victim_at`].) Witness mc1l27 pair 37366→37367:
/// the human's magnet bolt 836 elects the (10,40) grave 258 in retail;
/// the port skipped it and homed on the (10,39) ball 546.
pub(crate) fn no_mc1_magnet_homes_on_graves() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_MAGNET_HOMES_ON_GRAVES").is_some())
}

/// `MGC_NO_BALL_MERGE_FIX=1` restores BOTH pre-dig halves of the
/// mana-sphere merge — the whole-pool partner scan (instead of
/// retail's `sub_11D10`/`sub_10A50` map-tile ring walk) and the MC2
/// soft-kill of the absorbed donor (instead of retail's hard
/// `sub_57F20` free) — so one binary can be A/B'd. Read once: the
/// value is a whole-process arm, never a per-run input.
fn no_ball_merge_fix() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_BALL_MERGE_FIX").is_some())
}

/// `MGC_NO_EFFECT_RING_LIVE_OPERANDS=1` restores the pre-dig HOISTED
/// operands in the four class-10 ring sprayers (`blast_ring_tick`,
/// `spreader_tick`, `napalm_tick`, `napalm_tick_hw`). Retail drives all
/// four loops off ONE register holding the sprayer's pool pointer and
/// re-reads `+72/+74/+76` (position) per iteration, `+24`/`+30`/`+44`/`+26`
/// AFTER each allocation, and `+26` again after the loop — so when the
/// allocator SEIZES the sprayer's own record for one of its children the
/// rest of the spray reads the CHILD's fields. See `blast_ring_tick`.
fn no_effect_ring_live_operands() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_EFFECT_RING_LIVE_OPERANDS").is_some())
}

/// `MGC_NO_MC1_TRAIL_SELF_SEIZE=1` restores the pre-dig HOISTED
/// operands of the meteor's fire-trail wrapper. Retail's `sub_53070`
/// (remc1 `sub_main.cpp` :63021-38) passes the ctor a POINTER to the
/// bolt's own position, not a copy: `CARPET.EXE` file 0x6B881
/// `8d 43 48` (`lea eax,[ebx+0x48]`, +72) / `50` / `e8 5e 43 fe ff`
/// (`call sub_373F0` = the `(10,1)` ctor `sub_3A510`), and `+24` is
/// read off `%ebx` only AFTER it returns (`66 8b 5b 18` at 0x6B898).
/// `sub_3A510` (file 0x52D08) calls `NewEvent_372C0` FIRST
/// (`e8 aa cd ff ff` at 0x52D09) and only then `sub_41CF0(new, a1)`
/// (`e8 9b 77 00 00` at 0x52D48), which reads x/y/z through `a1`.
/// So when the free stack is dry and the allocator seizes the BOLT
/// ITSELF, `a1` aliases the freshly memset child: the seeder links
/// at (0,0,0) and takes its own-slot `+24` stamp. WITNESS mc1l47
/// pair 16904→16905: the human's (9,3) meteor at slot 977 seizes
/// itself for its trail; retail's (10,1) reads x/y/z 0, `id24` 977,
/// the port's hoisted copies posed it at the bolt (x 31455, z 1795)
/// owned by the human (29).
///
/// ⭐⭐ IT IS A CLASS, NOT A TRAIL LAW (w159e follow-up). `sub_373F0`
/// (file 0x4FBE8) has 106 call sites in `CARPET.EXE`; every one that
/// pushes `lea 0x48(%reg)` of the CALLER's own record hands every
/// ctor behind it the same pointer, and every class-9/10 ctor body
/// the port models is `NewEvent` → field stores → `sub_41CF0(new,
/// a1)` (or a raw `+72` copy), so a self-seized caller always yields
/// a child posed at its own zeroed (0,0,0). Every read of the
/// caller's `+24`/`+30`/`+44`/`+68/+69`/… after the allocation then
/// reads the CHILD, which makes the plain copy-stamps identity
/// stores, and the caller's closing `sub_41E80(a1)` reaps the child.
/// [`Gen::mc1_self_seized`] is the shared pose half; each call site
/// re-reads its post-allocation operands live. The site census and
/// the unreachable (wizard-caster) half are on that helper.
///
/// `MGC_NO_MC1_SELF_SEIZE=1` is the class-wide name; the original
/// `MGC_NO_MC1_TRAIL_SELF_SEIZE=1` is honoured as a synonym.
pub(crate) fn no_mc1_self_seize() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var_os("MGC_NO_MC1_SELF_SEIZE").is_some()
            || std::env::var_os("MGC_NO_MC1_TRAIL_SELF_SEIZE").is_some()
    })
}

/// `MGC_NO_MC1_BEAM_SELF_SEIZE=1` — the A/B arm for the LIGHTNING
/// BEAM's half of the self-seizure class ([`no_mc1_self_seize`]),
/// dug in round 163 (w163b) on the mc1hw storm takes.
///
/// ⭐⭐⭐ THE BEAM IS THE ONE SELF-SEIZURE SITE WHOSE `sub_41E80` RUNS
/// **BEFORE** THE ALLOCATION, SO THE REBORN CHILD SURVIVES. Every
/// other site in the class reaps the child it seized with its closing
/// `sub_41E80(a1)`. `sub_535E0_53920` (the m9 beam, :63272-63449) has
/// **no** `+16 |= 0x400` of its own at all: the death bit is the last
/// statement of the FLIGHT step `sub_534C0_53800` (:63267-68,
/// HIDDEN.EXE 0x6C2F5 `cmpb $0,(%esp)` / `je` / `call 0x5ABB8`, and
/// 0x5ABB8 = HW VA 0x421C0 = `orb $4,0x11(%eax)`), which runs before
/// the segment chain is laid. When the chain's `NewEvent_37680` hands
/// back the beam's OWN record the ctor memsets it — clearing that
/// death bit (`movl $8,0x10(%ebx)` at 0x5014B) and stamping
/// `+24 = own slot` (`mov %ax,0x18(%ebx)` at 0x5015E, after
/// `idiv $0xa4`) — and the beam's record ends the tick ALIVE, as one
/// of its own state-14 segments.
///
/// And because the chain's owner copy is a MEMORY re-read of the
/// beam's `+24` once per segment (HIDDEN.EXE 0x6C412/0x6C416,
/// inside the loop that closes at 0x6C5AD) and again at the endpoint
/// blast (0x6C61F/0x6C623), every segment laid after the seizure —
/// and the blast — inherits the beam's SLOT NUMBER as its owner. The
/// port hoisted `owner` once before the loop and set the death bit
/// after it, so it stamped the caster instead and re-killed the
/// reborn segment.
///
/// WITNESS (guard probe on, so this is not the volcano): mc1hwl15
/// t=6434, storm cloud 318's second bolt at slot 872 — retail `flags`
/// 12 / `id` 872 on the beam's slot and `id` 872 on the 12 segments
/// plus the (10,0) endpoint at 925; the port read 1036 / 120
/// (`PLAYER_TARGET`) on all of them. Same shape at mc1hwl15 t=6440
/// (beam 920), mc1hwl13 t=15048 (beam 876) and mc1hwl11 t=41680
/// (beam 992) — the head slot's `id` is always its own slot number.
pub(crate) fn no_mc1_beam_self_seize() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_BEAM_SELF_SEIZE").is_some())
}

/// `MGC_NO_PARTNER_SOFTKILL_FIX=1` restores the port's invented
/// `flags & 0x400 == 0` clause in [`Gen::ball_merge_candidates`]'
/// tile-ring walk. Retail's `sub_11D10` has NO reap test: an
/// exhaustive scan of `CARPET.EXE 0x2A508..0x2A63D` finds no
/// `test`/`cmp` against the flags word (`+16`/`+17`) anywhere in the
/// per-node loop, whose whole predicate is the seeker's `+66`/`+67`
/// membership, the `+24` id self-exclusion and the `sub_11950` AABB.
/// A soft kill is therefore not a free HERE either — the same law the
/// `sub_46CA0` balloon pick, MC1 `castle_absorb`, MC2
/// `mc2_castle_absorb` and `mc2_aura_tick` already carry. A law on one
/// call path is not landed. The MC2 twin `sub_10A50` (`NETHERW.EXE`
/// `0x352D9..0x3532B`) is byte-for-byte the same shape.
fn no_partner_softkill_fix() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_PARTNER_SOFTKILL_FIX").is_some())
}

/// `MGC_NO_M57_MERGE=1` restores the pre-dig (10,57) arm: the fool's
/// sphere never merges at all (the `is_fool` early return in
/// [`Gen::ball_merge_candidates`]) and the partner filter is the
/// hardcoded `(10,39)` pair instead of retail's seeker-parameterised
/// `xtype`/`xsubtype` test. A/B arm only.
/// A/B toggle for the fool's-sphere merge PARENT test: set
/// `MGC_NO_M57_PARENT_ZERO` to restore the pre-dig behaviour, where
/// `sub_36F30`'s `@0x28` comparison ran on the port's `id24` (seeded
/// to the slot by `new_event`) instead of retail's raw 0.
pub(crate) fn no_m57_parent_zero() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_PARENT_ZERO").is_some())
}

/// `MGC_NO_MC2_AREA_WINDOW=1` restores the pre-dig behaviour of
/// [`Gen::area_write`]'s player probe, in which an invented `mc2 ||`
/// short-circuit skipped the TILE-WINDOW gate on all three MC2 area
/// primitives. Retail has no player arm at all: `sub_116A0`
/// (`NETHERW.EXE` 0x35EA0..0x360F7) reaches EVERY victim through
/// `mapEntityIndex_15B4E0` — centre `(pos+128)>>8` at 0x35F52, radius
/// `(pitch+255)>>8` at 0x35F87, bucket `movswl 0x8b4e0(,%eax,2)` at
/// 0x35FDB, `ret` at 0x360F7 — and the AABB `sub_106C0` is the SECOND
/// gate, never the first. Added 2026-09-04: the law had landed without
/// a switch, so its fixture's A/B required a source revert.
pub(crate) fn no_mc2_area_window() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_AREA_WINDOW").is_some())
}

/// `MGC_NO_MC2_AREA_WINDOW_CH34=1` — the A/B arm for the SAME window
/// gate on `sub_10C80`'s ch3/ch4 (steal/duel) arm, which returns
/// before the ring pass and so never inherited it. Set it to restore
/// the pre-dig pure-AABB human probe on those two channels.
pub(crate) fn no_mc2_area_window_ch34() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_AREA_WINDOW_CH34").is_some())
}

/// A/B toggle for the MC2 POSSESSION-PULSE FORCE FLAG: set
/// `MGC_NO_MC2_CLAIM_PULSE_FORCE` to restore the pre-dig behaviour,
/// where the (10,12) WEAK claim pulse broadcast its ctor's `+44`
/// (64000) on ch1 and MC2's ball intake read that as a FORCED claim,
/// stealing through the `byte[2] & 0x20` claim lock. Citation at the
/// write site in [`Gen::possess_flash_tick`].
pub(crate) fn mc2_claim_pulse_force_law() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CLAIM_PULSE_FORCE").is_none())
}

pub(crate) fn no_m57_merge() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_MERGE").is_some())
}

/// ⭐⭐⭐ THE VOLCANO'S PLUME HANDOVER LIVES INSIDE THE NEW PLUME'S
/// NULL GUARD. `sub_25EC0` (:28782-93) spawns the replacement
/// `(10,19)` FIRST and only then, `if (v8)`, stamps its owner,
/// soft-kills the record named by the old `+38` register and
/// publishes the new one. When the free stack is dry and
/// `sub_373F0_377B0` refuses, retail reap-flags NOBODY and the `+38`
/// register KEEPS its stale slot — it is never zeroed. The port ran
/// the blind kill unconditionally and then wrote `plume = 0` on a
/// refusal, so an exhausted pool made the volcano soft-kill a live
/// bystander (mc1l49 t=14798: free stack 39 → 0, the new volcano at
/// slot 956 reap-flagged slot 235, a `(10,0)` fire with `act_life 3`
/// that retail burns for seven more ticks) and then forgot its plume.
/// This is the same fork `proj_m12_tick` documents for the storm
/// cloud's bloom — the kill is INSIDE the guard, not beside it.
/// `MGC_NO_MC1_PLUME_SPAWN_GUARD=1` restores the pre-dig order.
pub(crate) fn plume_handover_is_guarded() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_PLUME_SPAWN_GUARD").is_none())
}

/// ⭐ **THE VOLCANO'S BLIND KICK WRITES A TOKEN'S SPELL LEVEL, NOT
/// ITS BURST** (round 159, w159a). `sub_25EC0` (:28778-81; CARPET.EXE
/// file 0x3E7B2-BC: `cmp %eax,%edx; jbe; movw $0xfa,0x1a(%edx)`)
/// stores 250 into the RAW `+26` word of whatever record the stale
/// `+36` register names — a pointer-above-base test and nothing else.
/// When that record is a live class-12 manifestation, retail's `+26`
/// is the token's spell level; the port homes the token's `+48`
/// burst counter in `Ent::f26` (the importer's re-home,
/// `conformance.rs` `f26: if r.class64 == 12 { r.f48 }`), so the
/// port's blind `f26 = 250` ARMED the burst instead. mc1l45 t=16952:
/// the volcano at slot 707 kicks wiz 3 (ent 496)'s Retreat token 169
/// `(12,21)` — retail `+26` 0 → 250 with `+48` staying 0; the port
/// armed `+48` 250, the backwards-speed handler ran its v_14 kill and
/// expiry snap and wrote the wizard's `f126 = −f128 = −80` against
/// retail's 0 (segment 3's INHERITED head: pair-clean because the
/// graded diff cannot see a token's raw `+26`/`+48` split). Set
/// `MGC_NO_MC1_VOLCANO_KICK_TOKEN_LEVEL=1` to restore the blind
/// `f26` store.
pub(crate) fn volcano_kick_spares_token_burst() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_VOLCANO_KICK_TOKEN_LEVEL").is_none())
}

/// ⭐⭐ **`sub_37710_37AD0` RETURNS THE FREE STACK'S LENGTH, NOT ITS
/// LENGTH PLUS ONE** (round 161, w161c). The whole body is
/// `return *(_DWORD *)(base + 40) + 1;` (remc1 :44061 / remc1hw
/// :40471; CARPET.EXE file 0x4FF08 `a1 00e40100 | 8b 40 28 | 40 | c3`,
/// HIDDEN.EXE file 0x504C8 `a1 f0e30100 | 8b 40 28 | 40 | c3` — file
/// offset = VA + 0x189F8, verified by the `e8 1f0c0100` call from
/// `sub_26E90` at 0x3F8A4), and `var_u32_40` is the free stack's TOP
/// INDEX, not its count: `sub_37220_375E0` (:43838-55) initialises it
/// to **−1** and pushes with `var_u32_593[++var_u32_40]`, while
/// `NewEvent_372C0` (:43867) pops on `var_u32_40 >= 0` and returns
/// `var_u32_593[var_u32_40--]`. So the probe's `+ 1` converts the top
/// index back into a LENGTH, and a port that adds a second `+ 1` to a
/// `Vec::len()` is one too high. The tree's other three runtime
/// readers already spell it as a plain length — the m0/m6 worm guard
/// (`free.len() < 16`, :44586/:45028), the castle leveler's
/// any-slot test (:56142) and the mana-spill ejector's cap
/// (:56194) — only [`Gen::undead_army_tick`] carried the extra one.
///
/// WITNESS mc1hwl3 t=695 (segment 0's only head, 37 field rows over
/// seven slots): the free stack holds exactly **7** when the (10,36)
/// spawner ticks, so retail raises `N = 7` skeletons on a
/// `2048/7 = 292` step from angle 0 — headings 144/436/728/1024/
/// 1316/1608/1900 after the `+0x400` flip — and hands each one
/// `10000 % (10000/7) = 4` mana. The port took `N = 8`, stepped by
/// `2048/8 = 256`, paid `10000 % 1250 = 0` mana, and starved on the
/// eighth `spawn_creature`, so it raised the same seven slots with
/// every position, heading, target yaw and purse wrong. One decision,
/// thirty-seven rows.
///
/// Set `MGC_NO_MC1_UNDEAD_RING_POOL_DEPTH=1` to restore the pre-dig
/// `free.len() + 1`.
pub(crate) fn undead_ring_size_is_the_free_length() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_UNDEAD_RING_POOL_DEPTH").is_none())
}

/// A/B toggle for the MC2 BALL-MERGE OWNER LADDER: set
/// `MGC_NO_MC2_BALL_OWNER_LADDER` to restore the pre-dig
/// approximation, in which two OWNED spheres resolved the survivor's
/// colour on the BALL manas (`fj > fi`) instead of retail
/// `sub_36D50`'s class-10 arms + the owner wizards'
/// `maxMana_0x8C_140` contest. Citation at the call site in
/// [`Gen::mc2_ball_owner_contest`].
pub(crate) fn no_mc2_ball_owner_ladder() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BALL_OWNER_LADDER").is_some())
}

/// `MGC_NO_M57_IMPACT_PAIR=1` reverts the (10,57) ctor's
/// `byte_0x43_67 = 10` / `byte_0x44_68 = 1` stamp (`sub_50130`
/// EF:36642-44), which the shared `CreateManaSphere_500C0` twin does
/// not carry. See the call site in [`Gen::mc2_spawn_mana_sphere`].
pub(crate) fn no_m57_impact_pair() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_IMPACT_PAIR").is_some())
}

/// `MGC_NO_M57_RECLAIM_BIT=1` reverts the (10,57) ctor's
/// `struct_byte_0xc_12_15.byte[2] |= 2` recycle-victim membership
/// (`sub_50130` EF:36645), which the shared `CreateManaSphere_500C0`
/// twin does not carry. See [`Gen::mc2_spawn_mana_sphere`].
pub(crate) fn no_m57_reclaim_bit() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_RECLAIM_BIT").is_some())
}

/// `MGC_NO_M57_TRAP_FALLTHROUGH=1` restores the pre-dig (10,57) claim
/// arm, which returned out of `ball_tick` whenever the ch1 latch was
/// set. Retail's `else if (w68 && sub_36680(a1x))` only skips the
/// mover when `sub_36680` returns NONZERO — see the call site in
/// [`Gen::ball_tick`]. A/B arm only.
pub(crate) fn no_m57_trap_fallthrough() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_TRAP_FALLTHROUGH").is_some())
}

/// A/B toggle for **THE MANA BALL's OWNER TAG IS A RAW POOL SEAT**
/// (round 161, w161a): set `MGC_NO_MC1_BALL_OWNER_SEAT` to restore the
/// pre-dig derive, which resolved `+144` through [`Gen::owner_team`]
/// (the `PLAYER_TARGET` sentinel plus the eight registered wizard
/// seats) and fell back to the unowned family 52 for everything else.
/// Retail `sub_274D0` indexes the POOL at `+144` and tests only
/// `pool[+144].+64 == 3`, then reads that record's `+160` extension at
/// `+48`; a class-3 NON-wizard (the (3,3) mana balloon) shares the
/// engine's default extension, whose `+48` is 0, so it colours the
/// ball with the player-0 family 105. Decompile remc1
/// `sub_main.cpp:29586-29625` = remc1hw `sub_main.cpp:28130-28169`;
/// shipped bytes `CARPET.EXE` file 0x3FCC8 / `HIDDEN.EXE` file
/// 0x3FEC8, quoted at the call site in [`Gen::ball_resize`]. Witness
/// mc1hwl5 t=25348..25486, ball 866 `type86` retail 112 / port 59.
/// MC1 family only — MC2's sphere derive is a different routine.
fn no_mc1_ball_owner_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_BALL_OWNER_SEAT").is_some())
}

/// `MGC_NO_MC1_CH4_AMOUNT_KEEP=1` restores the pre-dig MC1 ch4 intake,
/// which cleared the WHOLE `mail[4]` pair. Retail MC1's `sub_27030`
/// clears the SOURCE word `+118` only — shipped `CARPET.EXE`
/// `0x3f900` (VA `0x27108`): `66 89 7b 76  mov %di,0x76(%ebx)` with
/// `%di = 0`, and NOTHING in `0x27076..0x27116` touches `+114`
/// (`0x72`). Its ch1 twin two blocks up DOES clear both
/// (`0x27069: 66 c7 43 64 00 00 movw $0x0,0x64(%ebx)` = source `+100`,
/// then `0x2706f: c7 43 60 00 00 00 00 movl $0x0,0x60(%ebx)` = amount
/// `+96`), so the asymmetry is deliberate and per-channel, exactly
/// like the MC2 twin EF:26109 the port already models.
fn no_mc1_ch4_amount_keep() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_CH4_AMOUNT_KEEP").is_some())
}

/// `MGC_NO_M57_TICK_RESIZE=1` restores the pre-dig (10,57) tail, which
/// ran the (10,39) BALL's PER-TICK `SetManaSphereColorAndRot_36920` on
/// the fool's sphere as well.
///
/// `sub_35FB0` (EF:26318-26614), the m57's action-0x3E tick, calls it
/// from exactly ONE place — the merge arm at EF:26595 — and the
/// shipped `NETHERW.EXE` agrees. A byte scan of every `E8 rel32` in
/// the image finds precisely FIVE call sites targeting linear
/// `0x36920`:
///
/// ```text
///   file 0x5A73E / lin 0x35F3E   TransformArcherToMana_35940 — the BALL's per-tick
///   file 0x5ADDC / lin 0x365DC   sub_35FB0 — the m57's MERGE, the only one
///   file 0x74922, 0x749C2        the two sphere ctors
///   file 0x911AC                 sub_6C870, the Fool's Mana cast
/// ```
///
/// and the merge site sits *inside* the `jz`:
///
/// ```text
///   0x5ADC4  53 E8 86A4FDFF        push ebx ; call 0x10A50   (find merge partner)
///   0x5ADCA  83C404 85C0 7413      add esp,4 ; test eax,eax ; jz +0x13
///   0x5ADD1  50 53 E8 58090000     push eax ; push ebx ; call 0x36F30  (the merge)
///   0x5ADD9  83C408
///   0x5ADDC  53 E8 3F030000        push ebx ; call 0x36920  (the resize)
/// ```
///
/// An ENUMERATED LIST — the absence is the law. A/B arm only.
pub(crate) fn no_m57_tick_resize() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M57_TICK_RESIZE").is_some())
}

/// `MGC_NO_CASTLE_BIND_LEVEL=1` restores the pre-dig at-castle probe:
/// [`World::regen_boost`] took the bare `seat_castle()` scan, so the
/// castle-rate boost armed on the tick the (3,2) record is BORN.
/// Retail's probe is gated on the wizard's BOUND castle register
/// `wizext+50` (`:55346-52`), which is written only by the LEVEL-UP
/// COMMIT `sub_47960_47CA0` (`:56484`, the same statement block that
/// increments the castle's `+26`) and cleared by the last downgrade
/// (`sub_47A70_47DB0` `:56534`, `if (!(a1+26)) owner->var_50 = 0`) —
/// i.e. `var_50 != 0` ⟺ the bound castle's LEVEL is ≥ 1. A/B arm
/// only.
pub(crate) fn no_castle_bind_level() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_CASTLE_BIND_LEVEL").is_some())
}

/// `MGC_NO_CASTLE_BIND_REGISTER=1` restores the bare pool scan at
/// every reader routed through [`World::player_castle_bound`] — the
/// generalisation of the at-castle law to the whole `wizext+50`
/// reader list (the demolish `:55838`, the token gate `:64923`, the
/// castle mail redirect `:55357`, the objective `:52122`, the
/// respawn seat `:54859` and its `sub_47DD0` re-price `:55034`, the
/// teleport resolve `:65574`, the create-vs-upgrade split `:65893`,
/// the shield token's store check, the win trigger `:67299` and the
/// HUD panel `:27214`). A/B arm only.
pub(crate) fn no_castle_bind_register() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_CASTLE_BIND_REGISTER").is_some())
}

/// A/B toggle for THE SHOT-STATS MODEL GATE (round 154, w154f; round
/// 153 finding #5): set `MGC_NO_MC1_SHOT_STATS_MODEL_GATE` to restore
/// the pre-dig `proj_explode`, which bumped the human's `shots`/`hits`
/// (`Type_160+343/+347`) on EVERY human-owned class-9 detonation.
/// Retail's one writer `sub_526C0` (:62585-62612, CARPET.EXE VA
/// 0x526CC-0x526F2: `cmp $3,%al; jb → cmp $1,%al; ja ret` / `cmp
/// $7; jb ret` / `cmp $9; jbe ok` / `cmp $0x13; je ok`) counts only
/// bolt models 0, 1, 3, 7, 8, 9 and 19 — the spell bolts whose ctor
/// row is a shot: fireball, possession, lightning, the m7/m8/m9
/// family and m19 — and returns before `shots++` for every other
/// model (the (9,16) beam segment, the m2 quake, the m4-6 lobs, the
/// castle ball…). mc1l49 t=633/638/641: three (9,16) detonations
/// read retail `shots` 61 / `hits` 10 flat against the port's 62 /
/// 11. The owner test (`class 3, model 0`, 0x5271D-0x52727) is the
/// human's `PLAYER_TARGET` on both sides.
pub(crate) fn no_mc1_shot_stats_model_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_SHOT_STATS_MODEL_GATE").is_some())
}

/// A/B toggle for HIDDEN WORLDS' SHOT GATE (round 168): set
/// `MGC_NO_MC1HW_SHOT_STATS_M16` to keep `CARPET.EXE`'s model list on
/// an HW level. `HIDDEN.EXE`'s `sub_526C0` twin (VA 0x52A00, remc1hw
/// sub_main.cpp:58673) is not the same gate: file 0x6b407 `cmp
/// $0x7,%al; jb` (0, 1 and 3 below it) / 0x6b40b `cmp $0x9,%al; jbe
/// ok` / 0x6b40f `cmp $0x10,%al; jb ret; jbe ok` / 0x6b419 `cmp
/// $0x13,%al; je ok` — model **16**, the beam segment, IS a shot
/// there, where `CARPET.EXE` returns on it (the mc1l49 t=633 witness
/// of [`no_mc1_shot_stats_model_gate`]). WITNESS mc1hwl9-crashed
/// t=808: the human's `shots` 36 -> 37 and `hits` 6 -> 7 on a (9,16)
/// detonation the port left uncounted; 13 takes' counters ran one or
/// more short from their first beam on (255,353 `shots` rows).
fn no_mc1hw_shot_stats_m16() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1HW_SHOT_STATS_M16").is_some())
}

/// A/B toggle for THE UNDEAD-ARMY BOLT'S `+44` (round 168): set
/// `MGC_NO_MC1_UNDEAD_BOLT_44` to restore the spell table's damage word
/// (100) on the (9,11) bolt. The token machine `sub_57800` (remc1
/// sub_main.cpp:65926; one function for the human and the AI) writes
/// the word three times: the token's own `+44`, then `+44 = 1`
/// (:65964), then — when the owner's `wizext+50` register is nonzero
/// and that record's `+136` covers the token's `+136` (:65967-71) —
/// the low word of the token's price. The bolt hands the word to its
/// (10,36) child through the generic explode, whose first tick
/// overwrites it, so it shows only on the bolt's flight and the
/// spawner's birth boundary. WITNESS mc1hwl0 t=47086, bolt slot 906:
/// the human's castle 340 stores 30,000,000, `+44` reads 13000.
fn no_mc1_undead_bolt_44() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_UNDEAD_BOLT_44").is_some())
}

/// A/B toggle for THE SILENT END'S SHOT (round 168): set
/// `MGC_NO_MC1_SILENT_END_SHOT` to stop scoring the no-spawn end of
/// the state 7 / 8 flight handler and the duel dart's tail. Citation
/// and the mc1l32-quick t=9962 / mc1l48 t=59118 witnesses at
/// [`Gen::mc1_shot_stats_silent`].
fn no_mc1_silent_end_shot() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_SILENT_END_SHOT").is_some())
}

/// A/B toggle for THE HIT-STAT AIM LATCH (round 154, w154f; round 153
/// finding #5): set `MGC_NO_MC1_HIT_STAT_AIM_LATCH` to restore the
/// pre-dig `hits` test, which read the bolt's `+146` AT THE
/// DETONATION and compared it against the struck record's slot OR
/// its `id24`. Every retail flight handler latches the aimed record's
/// POINTER at its ENTRY — `v2 = pool + 164 * +146` is the first
/// statement of `sub_52ED0` (:62952-54), `sub_52B30` (:62807-08),
/// `sub_52770` (:62644-45) and the m8/m9 twins (:63073, :63653), BEFORE
/// the `+16 & 2` first-tick acquisition (`sub_54520`, :62961) — and
/// hands that pointer to `sub_526C0`, whose hit test is `pool < struck
/// && aimed > pool && struck.id24 == aimed.id24` (CARPET.EXE
/// 0x52740-0x52750: `cmp %edi,%edx; jae` / `cmp %edx,%ecx; jbe` /
/// `mov 0x18(%edi),%dx; cmp 0x18(%ecx),%dx`). So a bolt that acquires
/// and strikes on its FIRST stepped tick (mc1l49 t=250: the possession
/// lob born on top of ball 928, `+146` 0 → 928 in the same dispatch)
/// scores a SHOT and no HIT — the aimed pointer was the null record
/// — and a struck record counts as a hit whenever it shares the aimed
/// record's OWNER id, not only when it is the aimed record itself.
/// The latch is [`Gen::mc1_aim_latch`], stamped by `proj_tick` at
/// dispatch entry.
pub(crate) fn no_mc1_hit_stat_aim_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_HIT_STAT_AIM_LATCH").is_some())
}

/// A/B toggle for THE BALLOON ALERT SINK (round 154, w154f; round 153
/// finding #6): set `MGC_NO_MC1_BALLOON_ALERT_SINK` to restore the
/// pre-dig `balloon_tick`, which armed the human's balloon-under-attack
/// HUD flash (`Type_160+393 = 4`) on every processed hit on an own
/// balloon. Retail's `sub_481D0` (:56820-31) writes `*(a1+160)+393`
/// through the BALLOON'S OWN `+160` (CARPET.EXE 0x481F4 `mov
/// 0xa0(%eax),%edx` / 0x481FD `movb $0x4,0x189(%edx)`), and a pool
/// record's `+160` is the allocator's static sink `unk_B7330`
/// (`NewEvent_372C0` :43878, 0x373BF `movl $0x27330,0xa0(%ebx)`) —
/// only a WIZARD record is ever re-pointed at its player block
/// (:54866). So the flash lands in a dummy `Type_160` nobody draws:
/// retail's balloon panel NEVER flashes, and `+393` reads 0 on every
/// record of every take (round 153: 8,651 pair rows / 36 takes,
/// retail 0 vs port 3). ⚠ A retail bug the port reproduces; a PATCH
/// to re-enable the flash would be a player ruling.
pub(crate) fn no_mc1_balloon_alert_sink() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_BALLOON_ALERT_SINK").is_some())
}

/// A/B toggle for THE HUD ALERT CADENCE (round 154, w154f; round 153
/// finding #6): set `MGC_NO_MC1_ALERT_HUD_CADENCE` to restore the
/// pre-dig tick tail, which decremented `castle_alert` /
/// `player_alert` / `balloon_alert` (`Type_160+391..393`) once per
/// tick unconditionally. Retail's ONLY decrements are in the HUD
/// panel draw `sub_22E50` (:27217-20 castle, :27287-90 balloon,
/// :27347-50 self; CARPET.EXE 0x22F5B-0x22F8F / 0x2330E-0x23347 /
/// 0x235CB `cmpb $0,0x187(%edi)` … `cmpb $0,0x5e(%eax)` … `decb
/// 0x187(%eax)`), gated on the blink bit `str_93[1] = frame & 1`
/// (:48552-53, the command processor's per-frame counter `+13341`,
/// bumped BEFORE the tick function the recorder samples ahead of),
/// so a flash counts 4,4,3,3,2,2,1,1 — eight frames, not four — and
/// only while the HUD is drawn at all: view mode 0/3, the carpet's
/// `actLife >= 0` (:26414 / :26454), the castle panel only while
/// `+50` names a live castle with `+26 > 0` (0x22F48-0x22F55), the
/// balloon panel only while `+50` names one. Witness mc1l49 t=84-96:
/// retail 4,4,3,4,3,2,1 on the even ticks against the port's
/// 3,3,2,3,2,1,0 (every odd tick agrees). See
/// [`World::mc1_alert_cadence`].
pub(crate) fn no_mc1_alert_hud_cadence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_ALERT_HUD_CADENCE").is_some())
}

/// A/B toggle for THE DANGER CLOCK'S WALK SEAT (round 154, w154f;
/// round 153 finding #6): set `MGC_NO_MC1_DANGER_WALK_SEAT` to
/// restore the pre-dig tick tail, which decremented the danger-music
/// countdown `Type_160 v_46` once per tick in every carpet state.
/// Retail's one decrement is in the carpet MOVER `sub_455D0`
/// (:55282-92, gated on `var_48 == local player`), which only the
/// state-0 flight handler `sub_45C90` (:55380) and the state-2 FALL
/// `sub_45FC0` (:55463) call — the state-3 dead-wait `sub_46480`
/// never moves, so the clock HOLDS through it (mc1l49 t=3080-85:
/// retail 77 flat, the port 76; t=3389-3404: 79 vs 78). And it runs
/// at the carpet's own walk slot, after that dispatch's mail drain
/// re-arms it to 100 (:55637) and before every higher-slot
/// projectile acquisition (:64013), which the tail seat read one
/// tick late. Now in the carpet dispatch, MC1 only; MC2's
/// `sub_5EFA0` twin keeps the tail.
pub(crate) fn no_mc1_danger_walk_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_DANGER_WALK_SEAT").is_some())
}

/// A/B toggle for THE DANGER ARM'S ACQUIRE CASES (round 154, w154f):
/// set `MGC_NO_MC1_DANGER_ACQUIRE_CASES` to restore the pre-dig
/// acquire, which armed the danger music (`sub_46520`, `v_46 = 100`)
/// on EVERY human lock. `sub_54520` calls it from two of its blocks —
/// models 0/3/4 (:64013, CARPET.EXE 0x54807) and 7/8/B/C (:64095,
/// 0x54A6D) — and NOT from the lightning's case 9 (:64125-91,
/// 0x54654-0x54681 stamps `+146`, calls `sub_52500` and returns 1)
/// nor the possess case 1. A/B arm only.
pub(crate) fn no_mc1_danger_acquire_cases() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_DANGER_ACQUIRE_CASES").is_some())
}

/// A/B toggle for THE SHOT-STATS ALLOCATION GUARD (round 154, w154f):
/// set `MGC_NO_MC1_SHOT_STATS_ALLOC_GUARD` to restore the pre-dig
/// seat, which scored a human detonation BEFORE its effect spawn.
/// Every retail detonation arm calls `sub_526C0` as the first
/// statement of its `if (effect = sub_373F0(…))` block (generic
/// :62762-64, m0 :62925-27, m1 :63002-04, m8 :63193-95 / :63206-08,
/// lightning :63426-29, :63770-72, :63906-08 / :63915-17), so a
/// detonation the dry pool cannot give an effect record scores
/// nothing that tick and re-detonates: mc1l49 t=8009-13, the human's
/// (9,3) at slot 914 (free stack 0) flies at life −1…−5 and `shots`
/// 445 → 446 lands with the t=8013 allocation, four ticks after the
/// port's pre-spawn bump.
pub(crate) fn no_mc1_shot_stats_alloc_guard() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_SHOT_STATS_ALLOC_GUARD").is_some())
}

/// `MGC_NO_PROBE_ID_UNFUSE=1` restores the pre-dig victim-probe
/// self-gate that compared the port's FUSED `id24` on both sides.
/// A/B arm only — see [`Gen::probe_self_id`].
pub(crate) fn no_probe_id_unfuse() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_PROBE_ID_UNFUSE").is_some())
}

/// `MGC_NO_PROBE_WINDOW_PLAYER=1` restores the projectile probe's
/// unconditional player AABB arm — the pre-dig [`Gen::victim_scan`]
/// tail that reached the human from anywhere its box overlapped,
/// ignoring the cell window `sub_11980` actually walks. A/B arm only.
fn no_probe_window_player() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_PROBE_WINDOW_PLAYER").is_some())
}

/// `MGC_NO_PLAYER_CHAIN_SEAT=1` restores the pre-dig placement of the
/// out-of-pool human INSIDE his own tile chain — the flat
/// `MGC_NO_PLAYER_CELL_TAIL` head/tail choice, where he was pinned to
/// one END of the chain instead of holding the SEAT retail's linked
/// carpet actually occupies (see
/// [`crate::engine::features::PlayerChain`]). MC1 column only. A/B
/// arm only.
fn no_player_chain_seat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_PLAYER_CHAIN_SEAT").is_some())
}

/// `MGC_NO_M8_ACQUIRE=1` restores the pre-dig [`Gen::proj_m8_tick`]
/// head: no `sub_54520` acquire fork for an untargeted steal seeker
/// and the `.clamp(-2, 2)` speed servo. A/B arm only.
fn no_m8_acquire() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_M8_ACQUIRE").is_some())
}

/// `MGC_NO_MC1_FLIGHT_SPEED_STEP=1` restores the pre-dig CLAMPED
/// class-9 flight speed servo, `+126 += (+128 - +126).clamp(-2, 2)`.
///
/// ⭐ RETAIL'S SERVO IS A **SIGN TIMES TWO**, AND IT NEVER CLAMPS.
/// `CARPET.EXE` 0x539DA (file 0x6C1D2) is the shape, read off the
/// shipped LE binary:
///
/// ```text
///     movswl 0x80(%ebx),%ecx      ; minSpeed
///     movswl 0x7e(%ebx),%eax      ; actSpeed
///     sub    %eax,%ecx            ; gap
///     jne    .diff
///     xor    %eax,%eax            ; gap == 0 -> 0
///     jmp    .apply
///  .diff:
///     cltd / xor %edx,%eax / sub %edx,%eax   ; |gap|
///     idiv   %edi                 ; gap / |gap| = SIGN
///  .apply:
///     lea    0x0(,%eax,2),%ecx    ; * 2
///     add / mov %ax,(%esi)        ; +126 += 2 * sign   -- NO CLAMP
/// ```
///
/// The two forms agree whenever the gap is 0 or |gap| >= 2, and
/// differ ONLY when |gap| == 1: retail steps 2, OVERSHOOTS, and then
/// oscillates about minSpeed forever, while `.clamp` steps 1 and
/// pins. An odd gap needs an odd input, which is why it hid.
///
/// ⭐⭐⭐ IT IS THE **WHOLE CLASS-9 FLIGHT FAMILY**, NOT ONE HANDLER.
/// The class-9 dispatch table (`CARPET.EXE` file 0x9CF34, stride 14,
/// `{u32 tag, u16 state, u32 handler, u32 1}`, 21 rows 0x00-0x14)
/// names five servo-bearing handlers, and a scan of the shipped
/// binary for the `99 31 d0 29 d0` abs idiom followed by a scale-by-2
/// finds EXACTLY those five sites and no others:
///
/// ```text
///   VA 0x52802  sub_52770  (:62671)  states 02/04/05/06/0B/0F/10/11/14 via sub_53060
///   VA 0x53152  sub_530C0  (:63097)  states 07 (via sub_530B0) and 08
///   VA 0x539DA  sub_53980  (:63478)  state 0A, the TARGETED castle-ball arm
///   VA 0x53C2A  sub_53B50  (:63571)  state 0A's untargeted tail call
///   VA 0x53E52  sub_53DC0  (:63680)  state 0C, the storm carrier
/// ```
///
/// Not one of the five clamps. ⭐⭐ **`HIDDEN.EXE` CARRIES THE SAME
/// FIVE, AND THE PORT SHARES THIS CODE WITH HW.** The same scan over
/// the HW binary finds exactly five, at the SAME offset inside each
/// twin of the decompile's `sub_MC1_HW` pairs — `sub_52AB0` 0x52B42,
/// `sub_53400` 0x53492, `sub_53CC0` 0x53D1A, `sub_53E90` 0x53F6A,
/// `sub_54100` 0x54192 — and none of those clamps either.
///
/// The MC2 twin landed first as
/// [`crate::engine::features::no_mc2_castle_ball_speed_step`]
/// (`NETHERW.EXE` 0x66B73 with `lea esi,[eax*2+0x0]` and 0x66DEB with
/// `add %eax,%eax` — same semantics, two encodings), and the MC1
/// `sub_530C0` site landed earlier still under `MGC_NO_M8_ACQUIRE`;
/// this switch covers the family, that site included.
fn no_mc1_flight_speed_step() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_FLIGHT_SPEED_STEP").is_some())
}

/// Retail's class-9 flight speed servo: `2 * SIGN(minSpeed - actSpeed)`.
/// See [`no_mc1_flight_speed_step`] for the bytes and the five sites.
#[inline]
fn flight_speed_step(gap: i16) -> i16 {
    if no_mc1_flight_speed_step() {
        gap.clamp(-2, 2)
    } else {
        2 * gap.signum()
    }
}

/// `MGC_NO_ACQUIRE_HUMAN_BUCKET0=1` restores the pre-dig
/// [`Gen::aim_assist_mc1_cone2`] head: the out-of-pool human is a
/// class-3 acquire candidate on every tick, alive or not. A/B arm
/// only. See the `view.dead_top` term at the Scan-A gate.
fn no_acquire_human_bucket0() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_ACQUIRE_HUMAN_BUCKET0").is_some())
}

/// The player carpet's half-extents (sprite 44 stats halves — the
/// same constants the trigger/portal overlap uses).
pub(crate) const PLAYER_HW: i32 = (SPRITE_STATS[44].width / 2) as i32;
pub(crate) const PLAYER_HH: i32 = (SPRITE_STATS[44].height / 2) as i32;
/// The MC2 carpet's half-width. MC2 keeps no world extents in its
/// sprite table (`mc2::sprite_params` — each ctor sets them through
/// `SetEntityShiftRot_49EA0`), so this is the boot-derived value the
/// carpet record actually carries: row 44 authors `speed_6 = 0`, boot
/// fills it from the TMAPS geometry as `width * rotSpeed_8 / height`
/// = 242, and `SetEntityIndexAndRot_49CD0` halves it. Measured on the
/// mc2l0 carpet record (apitch = aroll = 121). The vertical pair is
/// 100/100 — the same as MC1's, so [`PLAYER_HH`] serves both games.
pub(crate) const MC2_PLAYER_HW: i32 = 121;

/// Candidate set of the pure crosshair preview — the sub_54520
/// subtype blocks the player's own spells can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AimPreviewSet {
    /// Blocks 0/3/4 + the beam's one-shot snap: awake creatures +
    /// the class-3 list (fireball, meteor, volcano, lightning).
    Creatures,
    /// Block 1: unowned mana balls + houses (possess).
    Possess,
    /// Blocks 7/8/B/C: the class-3 list alone (duel, steal, undead).
    Wizards,
}

/// A mailbox recipient: a pool event or the out-of-pool player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MailTarget {
    Pool(usize),
    /// A human seat's out-of-pool mailbox (`Gen::pmail`).
    Player(Hx),
}

/// The inbox verdict a state handler dispatches on (hitflag 0/1/2).
pub(crate) enum Inbox {
    Quiet,
    Hit(u16),
    Dead,
}

/// Which Rebound deflection arm a bolt family carries. sub_52B30's
/// fireball arm (:62847-88) deflects ANY impact pair with the ±45
/// scatter, and an unaffordable deflection flies straight through
/// untouched. sub_52770's generic arm (:62705-50) deflects only the
/// (10,1)/(10,17) impact descriptors — Hidden Worlds inserts (10,53),
/// the homing meteor's pair, the single compare distinguishing the two
/// shipped binaries' handlers (CARPET.EXE 0x528B6-C3 vs HIDDEN.EXE
/// 0x52BF9-0x52C03) — with the ±22 scatter, and ANY refusal (pair gate
/// or mana) lands as a plain hit on the deflector. Wall of Fire's bolt
/// carries (10,53), so it punches through Rebound in base MC1 (the
/// anti-griffon spell) while the HW meteor deflects.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DeflectLaw {
    Fireball,
    Generic,
}

/// A/B toggle for THE DEFLECTION'S TARGET-PITCH MIRROR (round 154,
/// w154k; the `(9,x) f36` residue w154b left — mc1l15 233 pair rows,
/// mc1hwl0 160, mc1l49 158, spells-galore 56, every one a Rebound
/// deflection tick): set `MGC_NO_MC1_DEFLECT_PITCH_MIRROR` to restore
/// the pre-dig deflect arms, which reversed the live pitch (`+32`) and
/// left the TARGET pitch (`+36`) holding the pre-deflection value, so
/// the record read retail `p` against port `2048 − p` for the tick
/// (mc1hwl0 t=4494 slot 820 (9,16): retail 2008, port 40). Retail's
/// deflect block computes the reversed pitch ONCE and stores it to
/// BOTH words — `v14 = -(sub_42240(0,+32) * sub_42210(0,+32));
/// BYTE1(v14) &= 7; +36 = v14; +32 = v14` (:62727-32 generic,
/// :62867-72 fireball) — and the same store sits in all four
/// deflect-family handlers: CARPET.EXE VA 0x52963 (`sub_52770`),
/// 0x52D2E (`sub_52B30`), 0x532B3 (`sub_530C0`), 0x53FB6 (`sub_53DC0`),
/// each `f7 d8 / 80 e4 07 / 66 89 43 24 / 8b 53 04 / 66 89 43 20`
/// (`neg %eax; and $7,%ah; mov %ax,0x24(%ebx); …; mov %ax,0x20(%ebx)`);
/// HIDDEN.EXE has the same four (0x52EA8/0x5326E/0x537F8/0x544FB).
/// The `sub_530C0`/`sub_53DC0` copies are dead for their own bolts
/// (the m8 seeker carries `+69 = 25`, outside the `{1,17,53}` pair
/// gate), so the port's two live arms — both in
/// [`Gen::proj_move_and_hit`] — are every reachable site. Record
/// fidelity only: the next tick's homer rewrites `+36` before any
/// reader sees it, so no graded lane moves.
pub(crate) fn no_mc1_deflect_pitch_mirror() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC1_DEFLECT_PITCH_MIRROR").is_some())
}

impl Gen {
    // ---- mailbox writes ---------------------------------------------------

    /// The AREA write protocol, open-coded identically at every area
    /// writer in both games (MC1 `sub_120B0`/`sub_124F0`/`sub_127E0`
    /// :17466-70 and twins; MC2 `EF:4022-25`; the MC1 at-castle ch0
    /// redirect :55357-60 too): accumulate while a source is pending,
    /// overwrite a stale amount.
    ///
    /// Readers in both games clear the SOURCE and never the amount
    /// (:55734 / :21337 / EF:5407), so a consumed mailbox keeps its
    /// last amount as residue — under this order the next write simply
    /// overwrites it, and the residue is inert.
    ///
    /// ⚠ It is NOT inert under [`Gen::mail_write_single`], MC1's
    /// point-damage writer, whose branches are the exact INVERSE.
    pub(crate) fn mail_write(&mut self, tgt: MailTarget, ch: usize, amt: u32, src: u16) {
        if ch == 0
            && matches!(tgt, MailTarget::Pool(s) if s == crate::mail_trace_slot())
                | matches!(tgt, MailTarget::Player(_))
            && let Some(t) = crate::mail_trace()
        {
            let who = if matches!(tgt, MailTarget::Player(_)) {
                "player"
            } else {
                "slot"
            };
            eprintln!("[mail] t={t} AREA->{who} amt={amt} src={src}");
        }
        let m = match tgt {
            MailTarget::Pool(i) => &mut self.ent[i].mail[ch],
            MailTarget::Player(hx) => &mut self.pmail_mut(hx)[ch],
        };
        if m.1 != 0 {
            m.0 = m.0.wrapping_add(amt);
        } else {
            m.0 = amt;
        }
        m.1 = src;
    }

    /// `sub_12B50` (:17604-07) — MC1's SINGLE-TARGET write, and its
    /// branches are the INVERSE of the area protocol above: it
    /// OVERWRITES while a source is still pending and ACCUMULATES onto
    /// the stale amount once a reader has cleared the source. Because
    /// readers leave the amount standing, point damage in MC1
    /// SNOWBALLS: each hit lands on top of the residue of the previous
    /// one.
    ///
    /// Exactly two callers in the whole binary — the creature melee
    /// thunk `sub_1AB10` (:21970) and the class-3 arm of the proximity
    /// sweep at :31296. Every other MC1 damage path is an area write.
    ///
    /// Measured on mc1l0: one 100-damage melee (`+44`) onto a 400
    /// residue costs the player 500 life at t=3230, and 600 at t=3235
    /// on the 500 that left behind — both exact. The other branch is
    /// pinned by the t=565-570 castle window, where the source stays
    /// pending across four writes and the amounts record
    /// 1200/800/1200/400 with no compounding at all.
    pub(crate) fn mail_write_single(&mut self, tgt: MailTarget, ch: usize, amt: u32, src: u16) {
        if ch == 0
            && matches!(tgt, MailTarget::Pool(s) if s == crate::mail_trace_slot())
                | matches!(tgt, MailTarget::Player(_))
            && let Some(t) = crate::mail_trace()
        {
            let who = if matches!(tgt, MailTarget::Player(_)) {
                "player"
            } else {
                "slot"
            };
            eprintln!("[mail] t={t} SINGLE->{who} amt={amt} src={src}");
        }
        let m = match tgt {
            MailTarget::Pool(i) => &mut self.ent[i].mail[ch],
            MailTarget::Player(hx) => &mut self.pmail_mut(hx)[ch],
        };
        if m.1 != 0 {
            m.0 = amt;
        } else {
            m.0 = m.0.wrapping_add(amt);
        }
        m.1 = src;
    }

    /// sub_118C0 (:16963) between two pool events: extents SUM per
    /// axis, z centered by each half-height (+78). +78 is SIGNED —
    /// the castle's 0xE000 z-center marker (sub_37150 :43798) means
    /// −8192, not 57344; widening it unsigned orphans every castle
    /// out of the z test.
    pub(crate) fn ent_overlap(&self, a: usize, b: usize) -> bool {
        let (ea, eb) = (&self.ent[a], &self.ent[b]);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        // ⭐ EVERY EXTENT IS READ SIGNED, not just +78. See
        // [`crate::engine::features::no_mc1_aabb_signed_extents`].
        let ext = Self::aabb_ext;
        wd(ea.x, eb.x) < ext(ea.f80) + ext(eb.f80)
            && wd(ea.y, eb.y) < ext(ea.f82) + ext(eb.f82)
            && ((ea.z as i32 + ea.f78 as i16 as i32) - (eb.z as i32 + eb.f78 as i16 as i32)).abs()
                < ext(ea.f84) + ext(eb.f84)
    }

    /// The AABB test's extent operand: `movswl` in both shipped
    /// binaries (CARPET.EXE `sub_118C0` at file 0x2A0CC/0x2A0D0
    /// `movswl 0x2(%ebx)` / `movswl 0x2(%ecx)`, 0x2A0E8/0x2A0EC for +82,
    /// 0x2A106/0x2A10A for +84; NETHERW.EXE `sub_106C0` at 0x34EFF/
    /// 0x34F03 and 0x34F1D/0x34F21), so an extent past 0x7FFF is
    /// NEGATIVE and the summed box collapses.
    #[inline]
    pub(crate) fn aabb_ext(v: u16) -> i32 {
        if crate::engine::features::no_mc1_aabb_signed_extents() {
            v as i32
        } else {
            v as i16 as i32
        }
    }

    /// sub_118C0 against the player carpet.
    ///
    /// ⚠ THE CARPET'S HALF-WIDTH IS PER-GAME. `PLAYER_HW` is MC1's
    /// sprite-44 box (width 0xEE ⇒ 119); MC2's carpet record measures
    /// **121**, and the port already reads that value at
    /// `World::mc2_regen_boost` (the dolmen/castle latch) and in the
    /// switch-volume sum — sprite-params row 44 authors `speed_6 = 0`
    /// and BOOT derives it from the TMAPS geometry as
    /// `width * rotSpeed_8 / height` = 121·200/100 = 242, which
    /// `SetEntityIndexAndRot_49CD0` (:32841) halves into the extent
    /// quad. This is the third reader of that one lane and the last
    /// to get the MC2 value.
    ///
    /// The VERTICAL terms need no split: MC1's sprite-44 height 0xC8
    /// halves to 100, and the MC2 carpet's `ayaw` lift and `afov`
    /// half-extent are both 100 too, so `PLAYER_HH` is already right
    /// on both columns.
    ///
    /// Corpus row mc2l0 t=4104, and it is a ONE-UNIT row: the (9,13)
    /// archer arrow at slot 149 probes PRE-move (`AddArcherArrow_672E0`
    /// calls `sub_10780` before any `CopyEntityPosition` — the
    /// opposite of the fireball, do not unify them) from
    /// (49664, 52830, 583) against the carpet at (49500, 52705, 482).
    /// |Δx| = 164 clears the summed half-width 44 + 121 = 165 by one
    /// unit and fails MC1's 44 + 119 = 163. Retail hit, snapped the
    /// arrow onto the victim's RAISED position (`sub_65580`: z + f78 =
    /// 482 + 100 = 582, the recorded landing) and knocked the carpet;
    /// the port's arrow flew on and the pose forked on y.
    pub(crate) fn player_overlap(&self, i: usize, ctx: &MobCtx) -> bool {
        self.player_overlap_at(i, self.human_at(ctx))
    }

    /// [`Self::player_overlap`] against an explicit carpet pose — the
    /// carpet dispatch's own pre-move at-castle probe, which reads a
    /// warp destination its record does not hold yet.
    pub(crate) fn player_overlap_at(&self, i: usize, (px, py, pz): (u16, u16, i16)) -> bool {
        let e = &self.ent[i];
        let hw = if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            MC2_PLAYER_HW
        } else {
            PLAYER_HW
        };
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        let ext = Self::aabb_ext; // signed, like `ent_overlap`
        wd(e.x, px) < ext(e.f80) + hw
            && wd(e.y, py) < ext(e.f82) + hw
            && ((e.z as i32 + e.f78 as i16 as i32) - (pz as i32 + PLAYER_HH)).abs()
                < ext(e.f84) + PLAYER_HH
    }

    /// The writer's +66/+67 target filter (-1/-1 = wildcard).
    pub(crate) fn filter_admits(f66: u8, f67: u8, class: u8, model: u8) -> bool {
        (f66 == 0xFF || f66 == class) && (f67 == 0xFF || f67 == model)
    }

    /// sub_120B0 (:17235) / sub_124F0 (:17399) / sub_127E0 (:17502):
    /// the channel-N area write around event `i`. Gates per
    /// candidate: owner immunity (+24 equality — the engine's only
    /// friendly-fire rule), the damageable flag (+16&8), the
    /// vulnerability mask (+28 bit ch), the writer's +66/+67 filter,
    /// AABB overlap; the tile scan skips class-3 model 2 (:17372) —
    /// castles get their own ch0 pre-pass instead (:17325-34): every
    /// overlapping castle on ANOTHER team takes the mail (this is
    /// how mob-death fire cells fell castles), and under the 127E0
    /// variant (`shake`) EVERY castle in range — own included — arms
    /// its 30-tick blast-shake repaint (:17522). `building_tenth` =
    /// the 124F0 variant where class-2 model-0 TREES take amt/10
    /// (:17465 — the discount that keeps area spells from vaporizing
    /// forests; village buildings are class-10 m45 and take full
    /// amounts).
    /// Returns the number of mails written (retail's sub_124F0-family
    /// and MC2's sub_10C80/sub_116A0 return the hit count — the
    /// spellbook reports and the (10,9) earthquake gate consume it;
    /// MC1 callers ignore it).
    ///
    /// ⭐ `#[track_caller]` IS AN INSTRUMENT, NOT A BEHAVIOUR. It costs
    /// a hidden `&Location` argument and lets the `^player-post` line
    /// below name **which spell body** posted the mail. Every area
    /// spell in both games funnels through this one function, so
    /// without it the trace says "some (10,0) posted 250" and the dig
    /// then greps for the amount; with it the line reads
    /// `<- crates/mgc-sim/src/mc2/mobs.rs:2237:22` and the search is
    /// over. (Round 99 needed exactly that twice.) The attribute
    /// changes no value the sim computes and no branch it takes —
    /// measured: the 28-take corpus sweep, the 422-fixture suite and a
    /// whole-take `verify-deltas --csv` are all BYTE-IDENTICAL with
    /// and without this hunk.
    #[track_caller]
    pub(crate) fn area_write(
        &mut self,
        i: usize,
        ch: usize,
        amt: u32,
        ctx: &MobCtx,
        building_tenth: bool,
        shake: bool,
    ) -> u32 {
        self.area_write_opt(i, ch, amt, Some(ctx), building_tenth, shake)
    }

    /// [`Self::area_write`] with the out-of-pool player arm OPTIONAL:
    /// `ctx = None` is the level-load fixpoint (`sub_36620`), where
    /// retail's writers run the same tile-map walk over the half-built
    /// pool but no human record exists yet to be billed (it is seated
    /// after GenerateFeatures). See
    /// [`crate::engine::features::no_mc1_load_pass_area_mail`].
    #[track_caller]
    pub(crate) fn area_write_opt(
        &mut self,
        i: usize,
        ch: usize,
        amt: u32,
        ctx: Option<&MobCtx>,
        building_tenth: bool,
        shake: bool,
    ) -> u32 {
        let mut count = 0u32;
        let (wx, wy, id, f66, f67) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24, e.f66, e.f67)
        };
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // Does this call run MC2's BUILDING FOOTPRINT pass (below)?
        // It is `sub_10C80`'s ch0 arm alone — and where it runs, the
        // tile scan must skip (10,45) so the two never double up.
        // ⭐⭐⭐ MC2 HAS THREE ch0 AREA PRIMITIVES AND ONLY ONE WALKS
        // THE BUILDING LIST. `sub_10C80` (EF:3953) has both halves —
        // the `dword_38527` pass (EF:4076-4105) and the matching
        // `(class != 10 || model != 45)` ring exclusion (EF:4135).
        // `sub_11400` (EF:4208) has NEITHER, and `sub_116A0` (EF:4305)
        // has neither either. The shipped EXE agrees: `sub_10C80` @
        // 0x35480 loads `0x9677` (38519) AND `0x967f` (38527) and
        // compares `$0x2d`; `sub_11400` @ 0x35c00 loads only `0x9677`
        // and never compares 0x2d.
        //
        // `sub_11400`'s ONLY two call sites in the binary are the
        // (10,6) standing fire's tick, EF:23111 and EF:23152 — a
        // bijection with the port's only two `building_tenth` MC2
        // callers (`mc2/effects.rs` in `mc2_fire6_tick`). So
        // `building_tenth` IS `sub_11400`'s fingerprint, and a fire
        // reaches a building through the TILE CHAIN alone, at the
        // building's anchor cell — not through its summed AABB.
        // The port paid every overlapped building 50 a tick forever:
        // mc2l22 t=9745 slot 873 retail life flat 8000, port 7950,
        // and 98.3% of the family's life divergence is a multiple of
        // the (10,6) ctor's 50. Turning `fp_pass` off here is
        // self-completing — the tile ring's (10,45) skip is already
        // gated on it, so the anchor-cell reach comes back with it.
        let fp_pass = ch == 0 && !shake && !building_tenth && mc2;
        // The castle pre-pass (ch0 only) — :17322-33, and it is a walk
        // of BUCKET[0] ([`Gen::wiz_chain`], the tick-top class-3
        // roster) filtered to `+65 == 2`, not a pool sweep. Same
        // membership law as the m9 castle hunt and Scan A: liveness was
        // sampled once at the tick top, so a castle that dies mid-tick
        // keeps taking ch0 for the rest of that tick and a castle
        // already dead at the tick top takes NONE — where the port's
        // pool walk, which had no life test at all, delivered to both.
        // (mc1l4: 45 `(3,2) mail0.amt` shadow rows inside the bit-exact
        // window, e.g. t=1312 slot 436 retail 500 port 1050 — one
        // extra fire's 550 accumulated into a box retail left for the
        // spreader alone.) ⚠ This pass carries NO `& 8` damageable
        // test, no `+28` channel mask and no `+66/+67` filter — the
        // three the tile-ring walk below does carry. A castle is
        // reachable by anything.
        if ch == 0 {
            let mut hits: Vec<usize> = Vec::new();
            for c in 0..self.wiz_chain.visible_len() {
                let j = self.wiz_chain.list[c] as usize;
                if self.ent[j].model65 == 2 && j != i && self.ent_overlap(i, j) {
                    hits.push(j);
                }
            }
            for j in hits {
                if shake {
                    self.ent[j].f50 = 30;
                }
                if self.ent[j].id24 != id {
                    self.mail_write(MailTarget::Pool(j), 0, amt, id);
                    count += 1;
                    if std::env::var_os("MGC_AREA_TRACE").is_some() {
                        let e = &self.ent[i];
                        eprintln!(
                            "[area] poster {i} ({},{}) at ({},{},{}) f26={} f80={} f84={} amt={amt} -> castle {j}",
                            e.class64, e.model65, e.x, e.y, e.z, e.f26, e.f80, e.f84
                        );
                    }
                }
            }
        }
        // ---- MC2 PASS 2: THE BUILDING FOOTPRINT LIST ---------------
        //
        // `sub_10C80`'s ch0 arm runs THREE passes, not two: between
        // the castle list and the tile scan sits a walk of
        // `dword_38527` — the (10,45) BUILDING list (built at
        // EF:40043-52, the `model <= 0x2D` arm) — at EF:4076-4105.
        // Every building whose 2-D box the writer overlaps samples one
        // cell of its BUILD00 footprint mask, and a solid cell takes
        // the mail. `CompareAxisWithShift_10750` (EF:3733) is
        // `ent_overlap` MINUS the z term, so this is
        // [`Gen::mc2_overlap_xy`], and the pass has **no owner
        // immunity, no damageable flag, no vulnerability mask, no
        // +66/+67 filter, no life or collapse-mark test and no z
        // test** — "damage registers anywhere within the perimeter",
        // literally. `sub_116A0` (the shake variant) has no such pass,
        // which is why this is gated on `!shake`; MC1's `sub_120B0`
        // has none either.
        //
        // Without it a building was reachable only through the tile
        // chain, where it is linked at its ANCHOR alone
        // (`AddEventToMap_57D70` EF:40313 single-links, exactly like
        // our `Gen::link` — the multi-link theory is refuted). A
        // ground fire's 3×3 window at the anchor reaches 4 of the
        // main tower's 2,024 footprint cells; retail lands all 2,024.
        // The "damage snaps to the flag" report is the anchor hit
        // being the port's ONLY hit — the snap itself is faithful.
        //
        // ⚠ The mask row is BUILD00, not the sprite table remc2
        // guessed: the raw expression is `**filearray[24] + 6*idx +
        // 4`, a 6-byte TAB record with w at +4 and h at +5, and the
        // building ctor `sub_49A30` (EF:32765) reads the same row
        // through `filearrayindex_BUILD00DATTAB`. Its `>> 4` area lift
        // is the same 4x slip MC1's dwelling cap carries (see
        // `a_dwelling_carries_the_z_center_marker_sprite_and_area_cap`):
        // the recording's `min_speed` 6 on a 5x5 row is `(w*h) >> 2`,
        // which is also what pins the table as un-halved — retail's
        // `resolution == 1` halving is not in force for these takes.
        //
        // ⭐⭐ **AND THE MASK PROBE IS RETAIL'S OWN DEAD COMPUTATION.**
        // The top-left is taken from the WRITER (`a1x`), not from the
        // building, and the index then subtracts it from the WRITER
        // again — so the whole expression cancels to a per-ROW
        // constant, `(w>>1) - bump + (h>>1)*w`, the mask's own centre
        // cell. The only writer-dependence left is the parity bump,
        // which picks the centre or the cell left of it. THE CORPUS
        // ARBITRATES AND IT IS UNAMBIGUOUS: mc2l0 t=3192→3193, four
        // ground fires overlap building 196 (row 37, a 5x5 mask at
        // tile 171,207) at `d = (-1,1)`, `(-1,2)`, `(2,-2)` and
        // `(2,2)` — three of them OUTSIDE the footprint entirely —
        // and retail pays all four (1600 = 4x400). Probing the
        // building's own corner pays exactly the one writer standing
        // on a solid cell, and that 400-per-fire shortfall was the
        // whole residue left after the roster law below. Landing this
        // moves mc2l0's free horizon 3221 → 3240.
        //
        // So the pass is, in effect, "every building whose summed AABB
        // the writer overlaps" — the footprint mask never discriminates
        // by position. It is a latent retail bug and it is now the
        // port's law, exactly like the balloon mover's blind absorb
        // (DEVIATIONS §castle_balloons).
        if fp_pass {
            let (wtx, wty) = ((wx >> 8) as u8, (wy >> 8) as u8);
            let mut hits: Vec<usize> = Vec::new();
            // Retail walks the TICK-TOP roster `dword_38527`
            // ([`Gen::bldg_chain`]), not the live pool — so a record
            // that only BECOMES a building mid-tick is unreachable
            // by ch0 for the rest of that tick, and one killed
            // mid-tick keeps taking it. The pool walk here delivered
            // to both: mc2l0's ten village buildings complete at
            // t=3192 (action 51 → 52) and the port's burning-village
            // fires started paying them the same tick, so by the
            // construction window's end at t=3221 every inbox was
            // 400-multiples high (slot 123: retail 1600, port 2400)
            // and the drain at 3222 forked ten lives at once — the
            // take's whole wall. The chain-vs-pool costume, MC2 ch0
            // face.
            for c in 0..self.bldg_chain.visible_len() {
                let j = self.bldg_chain.list[c] as usize;
                let c = &self.ent[j];
                if !self.mc2_overlap_xy(i, j) {
                    continue;
                }
                let Some(def) = self.assets.build_tab.get(c.f71 as usize).copied() else {
                    continue;
                };
                // Retail's own expression, verbatim and with its dead
                // subtraction intact (EF:4082-88). `v22`/`v20` are
                // plain ints there, so nothing wraps at a byte.
                let mut v22 = wtx as i32 - (def.w as i32 >> 1);
                let v20 = wty as i32 - (def.h as i32 >> 1);
                if (v20 + v22) & 1 != 0 {
                    v22 += 1;
                }
                let off = def.offset as i64
                    + 2 * ((wtx as i32 - v22) + (wty as i32 - v20) * def.w as i32) as i64;
                if off < 0 {
                    continue;
                }
                // ⭐⭐ A ZERO-SIZE TEMPLATE IS NOT A SKIP — IT IS A
                // PARITY TEST, and the port's `def.w == 0 { continue }`
                // was a defensive guard with no line behind it.
                // EF:4082-88 has no size test at all. Put w = h = 0
                // through the expression and the row/column terms both
                // vanish: `v22 = wtx`, `v20 = wty`, so the index is
                // 0 when `wtx + wty` is EVEN and −2 when it is ODD.
                // A 0x0 building therefore takes ch0 from writers on
                // even-parity tiles and not from odd ones.
                //
                // It is reachable because BUILD.TAB has a 77th entry —
                // row 76, `offset 80064 / 0x0`, where 80064 is exactly
                // BUILD.DAT's length — and the villager lottery can
                // raise it (see `Assets::with_bldgprm` for the other
                // half of that phantom template). So its `data`
                // pointer sits one past the blob, and the two indices
                // are: −2 → BUILD.DAT's last byte, 0xFF, a MISS; and
                // 0 → the byte AFTER the buffer, which is the one
                // thing here we cannot read off disk.
                //
                // ⚠ THE CORPUS DECIDES IT, AND IT IS A HIT. mc2l3
                // t=15419: the (10,45) built at 14611 stands at
                // (46080, 17920) = tile (180, 70), sum 250, EVEN; the
                // (10,0) burst on top of it broadcasts 160 and
                // retail's building takes it (`life` 170000 → 169840,
                // `word_0x26_38` → the human). So an index past the
                // blob reads non-0xFF and delivers.
                let cell = self.assets.build_dat.get(off as usize).copied();
                if cell != Some(0xFF) {
                    hits.push(j);
                }
            }
            for j in hits {
                self.mail_write(MailTarget::Pool(j), 0, amt, id);
                count += 1;
            }
        }
        // ---- MC2 CHANNELS 3/4: `sub_10C80`'s CLASS-3 ARM ------------
        //
        // `if (a2 < 3 || a2 > 4)` (EF:3995) splits the MC2 primitive:
        // channels 3 (steal) and 4 (duel) run their OWN walk
        // (EF:4034-4060), and it is not the ring pass below with a
        // class filter bolted on — it has NONE of that pass's tests.
        // Verbatim:
        //
        //     if (a1x->id != nx->id && nx->class == 3 && sub_106C0(a1x, nx)) {
        //         v11x = nx + a2;                       // channel a2
        //         if (!v11x->word_0x62_98) {            // SOURCE FREE?
        //             v11x->dword_0x5E_94 = a3;         // the payload
        //             v3++;
        //             v11x->word_0x62_98 = a1x->id;     // the source
        //         }
        //     }
        //
        // No `byte[0] & 8` damageable test, no `+28` channel mask, no
        // `+66/+67` filter, no castle exclusion — and the write is
        // FREE-ONLY: a pending letter is left alone, never
        // accumulated onto (the ch0 protocol above is the opposite).
        // Window = `array_0x52_82.pitch` (= f80) rounded up, centre
        // `(pos + 128) >> 8` (EF:4035-38), overlap = `sub_106C0` =
        // the 3-D `sub_10630` box (EF:3712) = `ent_overlap`.
        //
        // The human is a class-3 record in retail's tile chain, and
        // since the multiplayer stage-2 tail so is the port's: his
        // linked record is billed in the walk below. Without one the
        // same free-only write goes to his mailbox block under the
        // AABB probe MC2 already uses for every channel.
        //
        // mc2l6-rsg t=8675: the human's Steal-Mana burst (slot 102,
        // f71 = tier 1) stamps rival 378 `mail3 = (1, 343)` and 378's
        // own dispatch drains 4000 the same tick (`sub_61050`).
        if mc2 && matches!(ch, 3 | 4) {
            let r = (self.ent[i].f80 as i32 + 255) >> 8;
            let (cx, cy) = ((wx as i32 + 128) >> 8, (wy as i32 + 128) >> 8);
            let mut victims: Vec<usize> = Vec::new();
            // His linked (3,0) record is billed in the walk like any
            // member ([`crate::engine::features::human_area_at_record`]);
            // without it his own arm below handles him.
            for dy in -r..=r {
                for dx in -r..=r {
                    let mut j = self.map_entity[tile((cx + dx) as u8, (cy + dy) as u8)] as usize;
                    while j != 0 {
                        let next = self.ent[j].next20 as usize;
                        if self.ent[j].id24 != id
                            && self.ent[j].class64 == 3
                            && self
                                .hx_of_rec(j)
                                .is_none_or(|h| self.human_area_at_record(h))
                            && self.ent_overlap(i, j)
                        {
                            victims.push(j);
                        }
                        j = next;
                    }
                }
            }
            for j in victims {
                if self.ent[j].mail[ch].1 == 0 {
                    self.ent[j].mail[ch] = (amt, id);
                    count += 1;
                }
            }
            // ⭐⭐⭐ AND THE WINDOW GATE BELONGS HERE TOO — THE SAME
            // FUNCTION, TWO ARMS, ONE GATED. The ch1+ ring pass below
            // already carries it (session 96's `mc2 ||` law); this
            // early-returning ch3/ch4 arm never got it, and it is the
            // arm that walks `mapEntityIndex` MOST literally — the
            // loop 20 lines up is a bare 2r+1 square of tile chains,
            // so the human, being a pool record in retail, is
            // unreachable from outside it however well the AABB
            // overlaps. The AABB is the SECOND gate, never the first.
            //
            // mc2l24 t=7913: the newborn (10,25) Steal-Mana burst at
            // slot 351 stands at (32128, 29312) with pitch 512, so
            // `v10 = 2` and the window is tiles x 124..=128; the
            // carpet, which moved at its OWN walk slot 116 earlier in
            // this very tick, is linked at (31723 >> 8) = **123** —
            // one tile short — while its box (|Δx| 405 < 512 + 121,
            // |Δy| 385, |Δz| 606 < 512 + 100) says yes on all three
            // axes. Retail's burst finds nobody and keeps its life
            // (8 → 7); the port billed the human, and
            // `mc2_blast25_tick`'s `hits != 0 ⇒ act_life = 0` killed
            // the burst on its first tick.
            let Some(ctx) = ctx else {
                return count;
            };
            for hx in self.human_tag.ixs() {
                if self.human_area_at_record(hx) {
                    continue;
                }
                let seat = self.seat_at(ctx, hx);
                let player_in_window = no_mc2_area_window() || no_mc2_area_window_ch34() || {
                    let (ptx, pty) = ((seat.0 >> 8) as u8, (seat.1 >> 8) as u8);
                    (-r..=r).any(|dx| (cx + dx) as u8 == ptx)
                        && (-r..=r).any(|dy| (cy + dy) as u8 == pty)
                };
                if self.human_tag[hx].0 != id
                    && player_in_window
                    && self.player_overlap_at(i, seat)
                    && self.pmail(hx)[ch].1 == 0
                {
                    self.pmail_mut(hx)[ch] = (amt, id);
                    count += 1;
                }
            }
            return count;
        }
        // THE SCAN RADIUS. Retail is the x half-extent rounded UP,
        // `(+80 + 255) >> 8`, in every variant of both games (MC1
        // sub_120B0 :17267 and :17342, sub_124F0 :17431, sub_127E0
        // :17539; MC2 sub_10C80 EF:3995/4032/4120) — the `__CFSHL__`
        // / `my_sign32` fixups wrapped around them are DEAD, the
        // extent field is uint16 so the sum never goes negative.
        //
        // ⭐⭐ AND THERE IS NO FLOOR UNDER IT, IN EITHER GAME. A
        // zero-extent writer runs `for i = -0; i <= 0` and scans its
        // OWN TILE ALONE — one tile, and on ch0 not even the tile it
        // stands on, because the window centre is `(pos + 128) >> 8`
        // while the map links at `pos >> 8`. The `.max(1)` that used
        // to sit here was OURS; it handed such a writer a 3x3 and
        // stole a tick of grace from every victim standing on an
        // impact point.
        //
        // BOTH GAMES PAY THE SAME RECEIPT, AND IT IS THE SAME SPRITE.
        // MC1, mc1l32 t=29834: a (10,17) blast ring's first dispatched
        // tick runs with entry f26 = 0, so field 260's 1000 reaches
        // crab 324 a tick early in the port (retail 883 v port 1883).
        // MC2, mc2l6 t=761: the human's (10,17) meteor is born at
        // (63924, 20732) — rival 383's own tile, (249, 80) — with the
        // quad still all zeros; `SetEntityShiftRot_49EA0(a1x, (768*0 -
        // …) >> 2, 512)` (EF:23861) leaves `array_0x52_82.pitch` at 0,
        // so retail's `v24 = (0 + 255) >> 8 = 0` window is the single
        // cell (250, 81) and 383 — standing exactly on the poster, box
        // overlapping on all three axes — takes NOTHING. Its 2,000
        // arrives one tick later at ring 2, where `v24` is finally 2:
        // life 3,144 → 3,151 (regen alone) across t=761 and 3,151 →
        // 1,158 across t=762.
        //
        // ⚠ The MC2 floor was held as one half of a compensating pair
        // with the arrow's direct hit (§THE HELD-BACK AREA FIXES,
        // 2026-08-12) — if an arrow stops damaging, the anti-tunnel
        // chord march on that path is the half to fix, not this line.
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        // Pass 2 OWNS the buildings, so the tile scan must not also
        // find them: `&& (class != 10 || model != 45)` sits at
        // EF:4135 right beside the castle exclusion, and for the same
        // reason. Only in the variant that runs pass 2 — `sub_116A0`
        // carries neither, `sub_11400` (the (10,6) standing fire's own
        // writer) carries neither, and MC1 has neither.
        let mut victims: Vec<(usize, u32)> = Vec::new();
        // THE WINDOW CENTRE, AND IT IS NOT THE SAME ON CHANNEL 0.
        // Channels 1+ round to the NEAREST tile, `(pos + 128) >> 8`
        // (MC1 sub_120B0 :17260-72, MC2 sub_10C80 EF:3995-96) — that
        // rounding is what the l0 t=91 tent claim needs, its flash at
        // y=70.63 sweeping tile row 73.
        //
        // MC1's ch0 arm biases the window one tile BACK instead:
        // `(pos - 128) / 256`, a TRUNCATING divide of the coordinate
        // loaded SIGN-EXTENDED. CARPET.EXE (VA 0x1215B/0x12172,
        // 0x1259B/0x125B2, 0x1288D/0x128A4 — all three variants,
        // byte-identical) does `movsx` from the u16 position and then
        // the `sar 31/shl 8/sbb/sar 8` signed-division idiom, so the
        // truncation is toward ZERO from both sides: on the west/north
        // half of the map (pos < 0x8000) the familiar one-tile-back
        // bias, and on the east/south half (pos as i16 negative) the
        // toward-zero rounding flips it to the NEAREST-UP centre
        // `(pos + 127) >> 8`. The listing types the coordinate
        // unsigned, which hides the movsx and reads as a plain back
        // bias everywhere — mc1l0 t=2811 is the measured refutation:
        // the (10,0) fire at x=37278 (tile 145.62) centres tile 146,
        // its 3x3 window missing the burning tree's tile (144,25),
        // while the (10,6) flame at x=37234 (tile 145.44) centres 145
        // and grinds the same tree 5/tick.
        //
        // MC2 does NOT do this: `sub_10C80`'s ch0 arm centres on
        // `(pos + 128) >> 8` like every other channel (EF:4118-19).
        // The ch1+ arm is sign-agnostic in BOTH games: the binary's
        // `movsx` + `add 0x80` + `sar 8` (VA 0x12329) is a FLOOR
        // divide, which commutes with the u8 tile wrap.
        let centre = |p: u16| -> i32 {
            if ch == 0 && !mc2 {
                (p as i16 as i32 - 128) / 256
            } else {
                (p as i32 + 128) >> 8
            }
        };
        let (ctx_, cty_) = (centre(wx), centre(wy));
        for dy in -r..=r {
            for dx in -r..=r {
                let tx = (ctx_ + dx) as u8;
                let ty = (cty_ + dy) as u8;
                let mut j = self.map_entity[tile(tx, ty)] as usize;
                while j != 0 {
                    let c = &self.ent[j];
                    let next = c.next20 as usize;
                    // ⭐⭐⭐ A LAW ON ONE CALL PATH IS NOT LANDED —
                    // `sub_10C80`'s owner immunity is the SAME
                    // `@0x1A != @0x1A` test as `sub_10780`'s
                    // self-exclusion — `sub_10C80`'s ch0 tile pass is
                    // `a1x->id_0x1A_26 != v8x->id_0x1A_26 && …`, and
                    // its ch1+ and ch3/ch4 arms repeat it verbatim.
                    // Shipped `NETHERW.EXE`, the ch0 arm at file
                    // **0x3572e**:
                    //   3572e  66 8b 43 1a  mov 0x1a(%ebx),%ax  ; POSTER @0x1A
                    //   35732  66 3b 46 1a  cmp 0x1a(%esi),%ax  ; VICTIM @0x1A
                    //   35754  8a 56 38     mov 0x38(%esi),%dl  ; …then the mask
                    //   3575f  f6 46 0c 08  testb $0x8,0xc(%esi); …and byte[0]&8
                    // (the ch1+ arm repeats the pair at 0x357fa), so
                    // it needs the SAME
                    // fused-`id24` unfuse [`Self::probe_self_id`]
                    // already carries for the victim probe. Without
                    // it every `@0x28`-fused record — the (10,57)
                    // fool's-mana sphere above all — is immune to
                    // every area writer its OWNER launched.
                    // WITNESS mc2l6-rsg t=11664: the human's own
                    // fireball detonates on his (10,57) at slot 547,
                    // the (10,0) impact at slot 510 area-writes its
                    // inherited `subSpellIndex` 160 over the sphere's
                    // tile — retail compares `343 != 547` and posts
                    // `mail0 = (160, 343)`, which `sub_35FB0` NEVER
                    // reads, so it stands for the rest of the take.
                    // The port compared `65535 != 65535` (both the
                    // human) and posted nothing: `(10,57) mail0.amt`
                    // 188,838 rows + `mail0.src` 187,978 rows on that
                    // one take, the second-largest entity block of the
                    // round-148 census.
                    // `MGC_NO_MC2_AREA_ID_UNFUSE=1` reverts.
                    let cand_id = if crate::mc2::mobs::no_mc2_area_id_unfuse() {
                        c.id24
                    } else {
                        self.probe_self_id(j)
                    };
                    if cand_id != id
                        && c.flags & 8 != 0
                        && c.f28 & (1 << ch) != 0
                        && Self::filter_admits(f66, f67, c.class64, c.model65)
                        && !(ch == 0 && c.class64 == 3 && c.model65 == 2)
                        && !(fp_pass && c.class64 == 10 && c.model65 == 45)
                        && self.ent_overlap(i, j)
                    {
                        let a = if building_tenth && c.class64 == 2 && c.model65 == 0 {
                            amt / 10
                        } else {
                            amt
                        };
                        victims.push((j, a));
                    }
                    j = next;
                }
            }
        }
        for (j, a) in victims {
            if j == crate::mail_trace_slot()
                && let Some(t) = crate::mail_trace()
            {
                let e = &self.ent[i];
                eprintln!(
                    "[mail] t={t} AREA poster {i} ({},{}) act={} at ({},{},{}) ch={ch} amt={a} src={id}",
                    e.class64, e.model65, e.tick70, e.x, e.y, e.z
                );
            }
            self.mail_write(MailTarget::Pool(j), ch, a, id);
            count += 1;
        }
        // The player probe (the human wizard is outside the pool; the
        // original reaches it through the same grid). BECAUSE it is
        // the same grid, MC1's probe carries the tile-scan's window
        // gate too: retail has no separate player arm — the carpet is
        // a pool record linked at its plain (x>>8,y>>8) tile, so an
        // area writer reaches it ONLY when its scan window (the ch0
        // one-tile-back bias included) covers that tile. Pinned by the
        // mc1l0 t=565-570 castle window: at t=568 fires 692/694
        // overlap the carpet's AABB but their windows stop one tile
        // short of tile (117,96) — retail's recorded residue is 3×400,
        // not 5 (the t=606 castle-gulp Δ=800, whole story).
        // ⭐⭐⭐ MC2 CARRIES THE WINDOW GATE TOO — its area writers
        // have NO player arm at all. `sub_10C80` (EF:3953),
        // `sub_11400` (EF:4208) and `sub_116A0` (EF:4305) reach EVERY
        // victim through `mapEntityIndex_15B4E0`, so the human's own
        // pool record is billed only when the writer's
        // `[-v10..v10]` tile window covers the tile it is LINKED at
        // (`sub_41CF0`: `(x>>8, y>>8)`) — the AABB `sub_106C0` is the
        // SECOND gate, never the first. Verified in the shipped
        // binary: sub_116A0 = NETHERW.EXE 0x35EA0..0x360F7 and its
        // whole body is the castle-list walk + the grid window
        // (centre `(pos+128)/256` at 0x35F52-84, radius
        // `(pitch+255)/256` at 0x35F87-9E, bucket
        // `movsx eax,[eax*2+0x8B4E0]` at 0x35FDB, list step
        // `[ebx+0x16]` at 0x360B4) — then `ret`. No carpet arm.
        //
        // The exemption this replaces (`mc2 ||`) was an INVENTED
        // widening: it let any MC2 area writer bill the wizard from
        // outside its own scan, and it is false exactly when it
        // matters. mc2l24 t=134: the (10,9) dome in slot 91 at
        // (33920,50048) has pitch 2816, so v10 = 11 and its window is
        // tiles y 185..=207 — the wizard at y=47193 is linked at tile
        // 184, ONE TILE SHORT, while its AABB (|dy| 2855 < 2816+121)
        // says yes. Retail bills nothing; the port billed 1200 and
        // the knock (1200/10 clamped 80) threw the carpet off pose.
        // `MGC_NO_MC2_AREA_WINDOW=1` restores the pre-dig `mc2 ||`
        // widening for the A/B (see `no_mc2_area_window`).
        let Some(ctx) = ctx else {
            return count;
        };
        // Every seat in turn — a human is spared only his OWN writes
        // (the owner test the walk above applies to every member).
        for hx in self.human_tag.ixs() {
            // His linked record took this write in the walk above when it
            // is a pool victim ([`crate::engine::features::no_human_pool_victim`]).
            if self.human_pool_victim(hx) {
                continue;
            }
            let seat = self.seat_at(ctx, hx);
            let player_in_window = no_mc2_area_window() && mc2 || {
                let (ptx, pty) = ((seat.0 >> 8) as u8, (seat.1 >> 8) as u8);
                (-r..=r).any(|dx| (ctx_ + dx) as u8 == ptx)
                    && (-r..=r).any(|dy| (cty_ + dy) as u8 == pty)
            };
            if self.human_tag[hx].0 != id
                && player_in_window
                && Self::filter_admits(f66, f67, 3, 0)
                && self.player_overlap_at(i, seat)
            {
                if ch == 0
                    && let Some(t) = crate::mail_trace()
                {
                    let e = &self.ent[i];
                    eprintln!(
                        "[mail] t={t} ^player-post from slot {i} ({},{}) at ({},{},{}) f80={} f84={} f78={} ctx=({},{},{}) amt={amt} <- {}",
                        e.class64,
                        e.model65,
                        e.x,
                        e.y,
                        e.z,
                        e.f80,
                        e.f84,
                        e.f78,
                        seat.0,
                        seat.1,
                        seat.2,
                        std::panic::Location::caller()
                    );
                }
                self.mail_write(MailTarget::Player(hx), ch, amt, id);
                count += 1;
            }
        }
        count
    }

    // ---- the creature inbox (the block opening every state handler) -------

    /// :21330-67: apply pending ch0 damage (awake only), inherit the
    /// weakest body segment's life, latch attacker (+40) and killer
    /// (+38), and report the hitflag.
    pub(crate) fn inbox(&mut self, i: usize) -> Inbox {
        let mut hit = 0u8;
        if self.ent[i].f58 != 0 {
            if self.ent[i].mail[0].1 != 0 {
                let (amt, src) = self.ent[i].mail[0];
                self.ent[i].act_life -= amt as i32;
                self.ent[i].mail[0].1 = 0; // amount stays stale (:21337)
                self.ent[i].f40 = src;
                hit = 1;
            } else {
                self.ent[i].f40 = 0;
            }
            let mut s = self.ent[i].f54 as usize;
            while s != 0 {
                if self.ent[s].act_life < self.ent[i].act_life {
                    self.ent[i].act_life = self.ent[s].act_life;
                    self.ent[i].f40 = self.ent[s].f40;
                    hit = 1;
                    break;
                }
                s = self.ent[s].f54 as usize;
            }
        }
        if self.ent[i].act_life < 0 {
            hit = 2;
            // The killer latch belongs to the LETHAL branch alone
            // (:21365-66 / :21485-86 / :21631-32 / :21737-38).
            self.ent[i].f38 = self.ent[i].f40;
        }
        match hit {
            1 => Inbox::Hit(self.ent[i].f40),
            2 => Inbox::Dead,
            _ => Inbox::Quiet,
        }
    }

    /// Aggro test on a mailbox source: only class-3 (wizard-family)
    /// attackers provoke a chase (:21370-76).
    pub(crate) fn attacker_is_wizard(&self, src: u16) -> bool {
        if self.is_human_tag(src) {
            return true;
        }
        let s = src as usize;
        s != 0 && s < self.ent.len() && self.ent[s].class64 == 3
    }

    // ---- class-9 projectiles ----------------------------------------------

    /// The shared class-9 init shape (str_255870 :4463): 8.8 position,
    /// not hittable (+16 &= ~8), refilled life, sprite-derived extents.
    /// `speed`/`life`/`row`/`sprite` per the model column; state = the
    /// model's flight state.
    #[allow(clippy::too_many_arguments)]
    fn spawn_projectile(
        &mut self,
        model: u8,
        state: u8,
        x: u16,
        y: u16,
        z: i16,
        speed: i16,
        life: u32,
        row: u8,
        sprite: u16,
    ) -> Option<usize> {
        let p = self.new_event()?;
        {
            let e = &mut self.ent[p];
            e.class64 = 9;
            e.model65 = model;
            e.tick70 = state;
            e.f126 = speed;
            e.f128 = speed;
            e.max_life = life;
            // Every per-model ctor writes +140 = 50 (:45877..:46120,
            // :46293+) EXCEPT the m13 bolt / m14 slow bolt / m15
            // boulder (sub_3A0C0/sub_3A1A0/sub_3A210 carry no +140
            // write — the l2 guard-arrow corpus reads 0 at spawn).
            if !matches!(model, 13 | 14 | 15) {
                e.f140 = 50;
            }
            e.row156 = row;
            e.flags &= !8;
        }
        self.link(p, x, y, z);
        self.refill_life(p);
        self.set_sprite(p, sprite);
        Some(p)
    }

    /// ⭐ THE MANIFESTATION BLOCK STAMPS THE DEST TRIPLE ON EVERY
    /// BOLT, OFF THE CASTER'S RECORD. Sixteen token machines
    /// (`sub_56090` … `sub_58240`, :65029-66420) mint a class-9 bolt
    /// and every one of them closes the mint with the same three
    /// lines — `*(bolt+150) = *(caster+72); *(bolt+154) = *(caster+76);
    /// sub_41EC0(bolt+150, caster+30, <pitch>, <reach>)` — the
    /// CASTER's raw axis (+72/+74/+76, not the muzzle, not the lifted
    /// z) projected along its LIVE aim. The machine is ONE routine
    /// for the human and the AI (the token's `+42` → the caster record
    /// as the walk holds it), so the stamp is the caster's pool record
    /// at the token's slot in the walk, warp included (w152l's law).
    /// Three shapes, read off CARPET.EXE (file = VA + 0x187F8; the
    /// `68 <imm32>` push before each `call sub_41EC0`):
    ///
    /// | reach            | pitch | arms (VA of the push)                       |
    /// |------------------|-------|---------------------------------------------|
    /// | `0x4000`         | +32   | fireball 0x561DD / 0x5838D (spells 0/23),   |
    /// |                  |       | steal mana 0x57376, lightning 0x575A2,      |
    /// |                  |       | undead 0x57966, storm 0x57AFE, magnet       |
    /// |                  |       | 0x57CCD, wall of fire 0x57E8D, global death |
    /// |                  |       | 0x581ED                                      |
    /// | `10240` (0x2800) | +32   | possess 0x56635, meteor 0x56A72, duel 0x57135|
    /// | `4096` (0x1000)  | **0** | earthquake 0x568CD, volcano 0x56C0D, crater  |
    /// |                  |       | 0x56DCD, castle CREATE 0x57762 — then `+154 =|
    /// |                  |       | sub_11F50(+150)`: the GROUND under the dest  |
    ///
    /// HIDDEN.EXE carries the same sixteen pushes with ONE immediate
    /// changed: wall of fire is `0x2800` (file 0x70D72) — the fork
    /// `cast_firewall` already documents. The castle UPGRADE arm
    /// (:65904-08) stamps nothing (`+146` = the bound castle instead),
    /// and the creature shooters (`sub_1A8E0` :21874 …) never touch
    /// `+150` — a creature's bolt keeps NewEvent's 0.
    ///
    /// The port stamped five arms (possess/meteor/duel/lightning on
    /// the human side, the duel on the rival's, storm/firewall in
    /// their own machines) and left the rest at the ctor's 0 — round
    /// 153's `(9,x) dest_x/dest_y/site_z` census: 1.05M free rows /
    /// 39 takes, 239k pair. mc1l2 t=2 slot 197 (rival 300's fireball):
    /// retail (36378, 5825, 3253) = (37760, 21872, 254) stepped 0x4000
    /// along (2020, 1988); t=2380 slot 108 (its possess lob): (55029,
    /// 53144, −6234) = (47553, 52128, 689) stepped 10240 along (556,
    /// 242). Nothing in the fireball/lob flights reads the triple back
    /// (the firewall bolt's homing tail is the one consumer, already
    /// stamped) — a record-fidelity law, shadow lanes only.
    ///
    /// `MGC_NO_MC1_BOLT_DEST_STAMP=1` leaves the newly covered arms at
    /// the ctor's 0 (the arms that stamped before this law keep their
    /// own switches or none).
    pub(crate) fn mc1_stamp_bolt_dest(
        &mut self,
        pr: usize,
        caster: (u16, u16, i16),
        yaw: u16,
        pitch: u16,
        spell: usize,
    ) {
        if crate::engine::world::no_mc1_bolt_dest_stamp() {
            return;
        }
        let (reach, pitched) = match spell {
            0 | 13 | 15 | 17 | 18 | 19 | 22 | 23 => (0x4000, true),
            20 => (
                if self.is_hidden_worlds() {
                    10240
                } else {
                    0x4000
                },
                true,
            ),
            3 | 7 | 11 => (10240, true),
            6 | 8 | 9 | 16 => (4096, false),
            _ => return,
        };
        let mut d = caster;
        Self::polar_step(&mut d, yaw, if pitched { pitch } else { 0 }, reach);
        if !pitched {
            d.2 = self.ground_z(d.0, d.1) as i16;
        }
        let e = &mut self.ent[pr];
        e.dest_x = d.0;
        e.dest_y = d.1;
        e.site_z = d.2;
    }

    /// sub_39A10 (:45861): the fireball. Base speed 384, life 21
    /// ticks, homing row [5] (thunks override), sprite 42.
    pub(crate) fn spawn_fireball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(0, 0, x, y, z, 384, 21, 5, 42)
    }

    /// sub_39BC0 (:45954): the m3 trail bolt (meteor). Row [1].
    pub(crate) fn spawn_trail_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(3, 3, x, y, z, 384, 21, 1, 76)
    }

    /// sub_39E40 (:46104): the m8 wizard-seeker. Row [4] (yaw 0x100).
    pub(crate) fn spawn_seeker(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(8, 8, x, y, z, 384, 21, 4, 214)
    }

    /// sub_39EC0 (:46135): the m9 zigzag lightning. Life 9.
    pub(crate) fn spawn_zigzag(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(9, 9, x, y, z, 384, 9, 4, 216)
    }

    /// sub_3A0C0 (:46256): the m13 straight bolt. Life 13, default
    /// row/damage (NewEvent's +44 = 100 unless the thunk overrides).
    pub(crate) fn spawn_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let p = self.spawn_projectile(13, 13, x, y, z, 384, 13, 0, 195)?;
        // The ctor's sprite call is the DOUBLING setter (:46274), not
        // the plain one every other class-9 ctor uses — the arrow
        // carries twice the collision half-extents (44/44/60 rather
        // than 22/22/30 for its 45x60 row).
        self.set_sprite_x2(p, 195);
        Some(p)
    }

    /// sub_3A390 (:46392): the m18 GLOBAL DEATH carrier. Fireball-
    /// shaped ctor (speed 384, life 0x2000/384 = 21, row [5],
    /// sprite 42), state 19 — and the 21-tick life is DEAD WEIGHT,
    /// because state 19 is [`Gen::death_relay_tick`], a one-tick
    /// relay that never reads it (retail's records carry act_life
    /// frozen at 21 for the carrier's whole one-tick existence:
    /// mc1l0-sg t=3877 slot 962).
    pub(crate) fn spawn_bomb_fuse(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(18, 19, x, y, z, 384, 21, 5, 42)
    }

    /// sub_54480_54810 (:63928-40 = hw:60038-50): the class-9
    /// **state-19 relay**. Global Death's carrier does not fly, does
    /// not tick down and does not detonate — it mints its `+68/+69`
    /// child at its OWN `+72`, copies four words in, and reaps
    /// ITSELF, all on the first walk that reaches it.
    ///
    /// ⭐ THIS ROW WAS NEVER "MISSING" — IT WAS NEVER WIRED UP. The
    /// body sits verbatim in BOTH listings; only remc1's transcribed
    /// class-9 state table stops short of it (14 rows of 21). Read
    /// off the shipped binaries: CARPET.EXE file 0x9CF34 row 0x13 →
    /// 0x54480, HIDDEN.EXE 0x9D134 row 0x13 → 0x54810. The port's old
    /// arm was a reconstruction — a caster-riding 21-tick fuse — from
    /// the only thing observable without the table: *the blast lands
    /// around the caster*. It does, because the carrier is born at
    /// the caster and dies there the same tick.
    ///
    /// ⚠ NO `sub_526C0`. The relay does not score, so it must NOT go
    /// through [`Gen::proj_explode`] — that would bill the caster a
    /// shot and a hit the wizard lane grades.
    ///
    /// The self-reap is INSIDE the null guard (:63932-39): a child
    /// the pool refuses leaves the carrier alive to try again next
    /// tick — the same refused-bloom shape as the ball merge's.
    fn death_relay_tick(&mut self, i: usize) -> bool {
        let (x, y, z, class, model, own, yaw, pitch, dmg) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f68, e.f69, e.id24, e.f30, e.f32, e.f44)
        };
        // `sub_373F0_377B0` is the whole class/model ctor table; the
        // only carrier that reaches this row is the cast arm's
        // (10,55) stamp (:66269-70), so the class-10 effect ctors are
        // the reachable half.
        let child = match class {
            10 => self.spawn_effect(model, x, y, z),
            _ => None,
        };
        if let Some(c) = child {
            // :63933-38 — four stamps read off the carrier after the
            // allocation: identity stores on a self-seizure, and the
            // reap then lands on the child ([`no_mc1_self_seize`]).
            if !self.mc1_self_seized(c, i) {
                let e = &mut self.ent[c];
                e.id24 = own;
                e.f30 = yaw;
                e.f32 = pitch;
                e.f44 = dmg;
            }
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// sub_3A1A0 (:46281): m7's slow bolt — state 15, the generic
    /// flight (see the dispatch note at [`Gen::proj_tick`]).
    pub(crate) fn spawn_slow_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(14, 15, x, y, z, 128, 32, 0, 196)
    }

    /// The player-spell payload projectiles (c9 m1 possess / m2
    /// earthquake / m4 volcano / m5 crater / m7 duel / m11 undead /
    /// m17 magnet): fireball-shaped init, state = model, dispatched
    /// to [`Gen::proj_payload_tick`] — except the MAGNET bolt (m17),
    /// which runs possession's state-1 flight: its ctor writes state
    /// 18 (:46371), past remc1's 14-entry class-9 state table, and
    /// the m1 flight is the behavior-matched stand-in. Inside it the
    /// m17 bolt = the m1 homing skeleton in state 18: the HW listing
    /// carries the case-0x11 acquire arm remc1's dropped
    /// (hw:60386-405 — walks ONLY the ball chain, no owner
    /// exclusion, no danger call, cone 0x71/0x71 =
    /// aim_assist_possess_mc1's magnet arm; corpus witness
    /// mc1l0-sg t=2723 slot 46, retail +70 = 18). Its contact scan
    /// is model-39-ONLY (sub_11C00 :17083, not possession's
    /// 39/40/45 sub_11AC0).
    /// Sprites per the class-9 rows in `mc1_entities` — the magnet
    /// bolt shares possession's sprite 209 (both ctors call
    /// sub_36FA0(entity, 209), :45916/:46384: distinct models, one
    /// look).
    pub(crate) fn spawn_spell_lob(&mut self, model: u8, x: u16, y: u16, z: i16) -> Option<usize> {
        let sprite = match model {
            1 | 17 => 209,
            2 => 211,
            4 => 210,
            5 => 211,
            7 => 213,
            11 => 281,
            _ => return None,
        };
        // Retail's magnet bolt flies in state 18 (measured, stable
        // for its whole life) — native mint and import must agree.
        let state = if model == 17 { 18 } else { model };
        // The possess lob AND the magnet bolt are the family's short
        // fuses: sub_39A90 (:45908) and sub_3A2F0 (:46375) both
        // compute life 4096/speed = 10 where every sibling
        // (:45861..:46135) takes 0x2000/speed = 21 (mc1l0 pair 63:
        // retail lob 9/10 vs the port's old fireball-shaped 20/21).
        let life = if model == 1 || model == 17 { 10 } else { 21 };
        // Homing rows straight off the ctors: m1/m17 sit on row [2]
        // (yaw/pitch caps 113/113, :45908/:46376); the payload lobs
        // m2/m4/m5/m7/m11 on row [1] (caps 22/22, :45941..:46220).
        // The port used to hand every lob row 0 — inert while the
        // homer hardcoded its caps, live now that the tracked arm
        // reads BEHAVIOR[row156].
        let row = if model == 1 || model == 17 { 2 } else { 1 };
        let p = self.spawn_projectile(model, state, x, y, z, 384, life, row, sprite)?;
        // ⭐ THE TWO SPRITE-209 LOBS CARRY A DOUBLE-SIZE HITBOX. Both
        // ctors close with an INLINE `sub_37130_374F0(v2, 2 * +80,
        // 2 * +84)` after the plain sprite bind — sub_39A90 :45917
        // (possess, hw:42038) and sub_3A2F0 :46385 (magnet,
        // hw:42505), identical in both binaries. Every other class-9
        // spell ctor in the family leaves the sprite's own halves
        // alone. Sprite 209 derives 90/90/75, so these two fly at
        // 180/180/150 — and the whole flight hangs off it, because
        // the contact scan is `ent_overlap`'s SUMMED extents: at half
        // size the bolt passes straight through the ball it was fired
        // at. mc1l0-sg slot 46 held retail's 180/180/150 vs the
        // port's 90/90/75 from its first tick (t=2723) and never
        // touched down; retail's terminated at t=2733 into the
        // (10,12)+(10,54) pair.
        //
        // Not `sub_370A0_37460`: same arithmetic, different shape —
        // see [`Gen::set_sprite_x2`].
        if model == 1 || model == 17 {
            self.set_sprite_x2(p, sprite);
        }
        Some(p)
    }

    /// Vertical bearing (sub_42180 :52644): the pitch whose polar step
    /// descends from `fz` toward `tz` over horizontal distance `dh`.
    ///
    /// ⭐ THE RUN IS TRUNCATED TO i16 BEFORE IT IS NEGATED, NOT
    /// CLAMPED. `sub_42180` is `sub_40F87(a1->z - a2->z, -(__int16)
    /// sub_423D0(a1, a2))` (:52646-48) and `sub_423D0` (:52739-44)
    /// returns an UNSIGNED isqrt that reaches 46340 for a full-map
    /// diagonal. Past 32767 the `(__int16)` cast goes negative and the
    /// unary minus hands `sub_40F87` a POSITIVE run, which lands the
    /// bearing in the 1024−ε quadrant (nose UP and BACKWARDS) instead
    /// of the 0+ε one — retail's homer aims the LONG way round at any
    /// tracker further than 32767 away. The port clamped to 0x7FFF and
    /// so kept aiming forward, turning the servo the opposite way at
    /// its full row cap every tick.
    ///
    /// This is the (9,0) fireball's whole residue on mc1l42: the
    /// one-shot tracker never re-validates (see [`Self::home`]), so a
    /// fireball whose +146 slot gets recycled into something across
    /// the map homes on garbage at 35537 units — retail pitches DOWN
    /// 22/tick, the port pitched UP 22/tick, and x/y/z followed
    /// (t=2137-42 slot 371, t=6051-56, t=12274-80, t=14485-89).
    pub(crate) fn pitch_toward(fz: i16, tz: i16, dh: i32) -> u16 {
        Self::angle_of(fz.wrapping_sub(tz), (dh as i16).wrapping_neg())
    }

    /// The acquire score, sub_54A90 :64212-17 (its castle twin
    /// sub_54BD0 :64261 is term-for-term identical): the 2-D ground
    /// distance decomposed onto the angular-error axes. The 16.16 cos
    /// terms come down `>>16`, the sin terms `>>14` through an i16
    /// truncation — a unit of angular miss costs ~4x its on-axis
    /// projection (16x squared), but DISTANCE multiplies everything:
    /// between two candidates inside the cone, the closer one wins
    /// unless the farther is much straighter ahead. Lower is better;
    /// the caller's compare is unsigned strictly-less (retail seeds
    /// best = -1 and rejects with -1, both = u32::MAX; the port
    /// gates cone/range before scoring instead).
    fn acquire_score(dist: i32, dy: usize, dp: usize) -> u32 {
        use crate::mc1::tables::{COS, SIN};
        let v8 = dist * COS[dy];
        let v9 = dist * SIN[dy];
        let v10 = dist * COS[dp];
        let v11 = ((SIN[dp] * dist) >> 14) as i16 as i32;
        ((v10 >> 16) * (v10 >> 16)
            + (v8 >> 16) * (v8 >> 16)
            + ((v9 >> 14) as i16 as i32) * ((v9 >> 14) as i16 as i32)
            + v11 * v11) as u32
    }

    /// The 3-D point distance (sub_42340 :52721): wrapping i16
    /// deltas, i32 wrapping square-sum, Newton isqrt — the acquire's
    /// class-3 pre-gate metric, measured at the nodes' RAW positions
    /// (+72, no aim lift).
    fn dist3d(ax: u16, ay: u16, az: i16, bx: u16, by: u16, bz: i16) -> i32 {
        let dx = (bx as i16).wrapping_sub(ax as i16) as i32;
        let dy = (by as i16).wrapping_sub(ay as i16) as i32;
        let dz = bz.wrapping_sub(az) as i32;
        let sum = (dx * dx).wrapping_add(dy * dy).wrapping_add(dz * dz);
        Self::isqrt(sum as u32) as i32
    }

    /// Aim a fresh projectile from an attacker at a target point
    /// (sub_42150/42180 pair) and stamp the combat fields the thunks
    /// share: owner, filter, homing target, damage, explosion.
    ///
    /// Every retail thunk computes BOTH bearings from the SHOOTER's
    /// +72 position struct — the muzzle lift lands on the
    /// projectile's +76 separately (`+76 += +84`, :21893/:21922/
    /// :21949/:22120/:22153/:23257/:25855, ×4 at :26171, and the
    /// seeker's pre-bearing +76 write at :24693 still aims off the
    /// shooter) — so the launch pitch is aimed from the UNLIFTED z.
    /// The caller spawns at the shooter's z and passes the lift here.
    /// +34/+36 are NOT written: NewEvent zeroes them and only the
    /// first homing/arm tick fills them (corpus t=3051/3081/4194 —
    /// a creature bolt born behind the walk cursor surfaces with
    /// target_yaw 0).
    #[allow(clippy::too_many_arguments)]
    /// The muzzle a repeated spawn reads: retail re-reads the
    /// emitter's `+72` per shot, so once a shot has seized the
    /// emitter the rest of the volley is laid around the CHILD's
    /// zeroed axis. Hoisted copies stay under the kill switch.
    pub(crate) fn mc1_muzzle(&self, src: usize, hoisted: (u16, u16, i16)) -> (u16, u16, i16) {
        if no_mc1_self_seize() || matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            return hoisted;
        }
        let e = &self.ent[src];
        (e.x, e.y, e.z)
    }

    /// THE CREATURE THUNKS' POST-ALLOCATION OPERANDS. Every retail
    /// shooter thunk (`sub_1A8E0` :21890-98, `sub_1A990` :21947-55,
    /// `sub_1AA40`, `sub_1AB70` :22005-06, `sub_1AE30` :22122-25,
    /// `sub_1AEE0` :22155-60, `sub_1E380` :24683-700, m15 :25857-58)
    /// mints its bolt with a POINTER to the shooter's `+72` and only
    /// THEN reads `+24` (owner), `+146` (target), `+84` (the muzzle
    /// lift) and — for the thunks that copy the shooter's pair —
    /// `+66/+67`. On a self-seizure those reads land on the bolt
    /// itself, so it flies owner = its own slot, target 0, lift = its
    /// own `+84`, with the shooter's filter replaced by the ctor's.
    /// The BEARING needs no fixing: retail aims from the shooter's
    /// `+72`, the port from the bolt's, and after
    /// [`Gen::mc1_self_seized`] zeroed the pose those are the same
    /// record. `shooter_filter` is false for m8/m6 (which copy the
    /// TARGET's pair, :22155-60) and for m11 (which writes none);
    /// `lift_mul` is 4 for the m16 wyvern's quadruple lift.
    pub(crate) fn mc1_arm_live(
        &mut self,
        p: usize,
        src: usize,
        ops: (u16, u8, u8, u16, i16),
        shooter_filter: bool,
        lift_mul: i16,
    ) -> (u16, u8, u8, u16, i16) {
        if !self.mc1_self_seized(p, src) {
            return ops;
        }
        let e = &self.ent[p];
        (
            e.id24,
            if shooter_filter { e.f66 } else { ops.1 },
            if shooter_filter { e.f67 } else { ops.2 },
            e.f146,
            lift_mul * e.f84 as i16,
        )
    }

    pub(crate) fn arm_projectile(
        &mut self,
        p: usize,
        owner: u16,
        f66: u8,
        f67: u8,
        target: u16,
        tx: u16,
        ty: u16,
        tz: i16,
        f44: u16,
        expl_model: u8,
        lift: i16,
    ) {
        let (px, py, pz) = (self.ent[p].x, self.ent[p].y, self.ent[p].z);
        let yaw = Self::angle_between(px, py, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
        let pitch = Self::pitch_toward(pz, tz, dh);
        let e = &mut self.ent[p];
        e.id24 = owner;
        e.f66 = f66;
        e.f67 = f67;
        e.f146 = target;
        e.f30 = yaw;
        e.f32 = pitch;
        e.z = e.z.wrapping_add(lift);
        e.f44 = f44;
        e.f68 = 10;
        e.f69 = expl_model;
    }

    /// The TargetingVerb seam (crate::verbs) — the acquire subtypes
    /// dispatch here. MC2's own acquire column lives in mc2::mobs;
    /// this dispatcher is only reached from MC1-spell paths, where an
    /// MC2 world serves the MC1 scan and notes the fallback (the
    /// pinned frankenstein ledger).
    fn aim_assist(&mut self, i: usize, ctx: &MobCtx) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_mc1(i, ctx),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_mc1(i, ctx);
            }
        }
    }

    /// Is this the Hidden Worlds engine? HW's entire live sim delta is
    /// the original's single compiled `IsHiddenWord` bool (two branches:
    /// the model-16 homing meteor and the napalm-geometry fork). We
    /// carry it as the one HW-distinct verb — the targeting column —
    /// rather than a parallel flag; every HW branch reads it here. If HW
    /// ever needs a divergence on a column that also varies for MC2,
    /// promote this to a dedicated field.
    pub(crate) fn is_hidden_worlds(&self) -> bool {
        matches!(self.verbs.targeting, TargetingVerb::Mc1Hw)
    }

    /// One-time target acquisition sub_54520 (:63943): the class-3
    /// significant list (within the OWNER row's v_28) and the awake
    /// creature buckets, inside a ±0x71 yaw AND pitch cone within
    /// 2-D ground distance 5120, best by sub_54A90's
    /// distance-weighted score ([`Self::acquire_score`]).
    ///
    /// **THE LIGHTNING BEAM (model 9) IS ITS OWN CASE.** `sub_54520`
    /// switches on `+65`, and case 9 (:64125, remc1hw :60256 —
    /// identical) scores the CREATURE buckets at `(0x71, 0x200)`: the
    /// yaw wedge is the usual ±20°, but the PITCH cone is ±90°, i.e.
    /// effectively unbounded. Wizards/castles keep `(0x71, 0x71)`.
    /// That one constant is the Lightning Storm: the (10,38) cloud
    /// fires its (9,9) bolts at a fixed pitch 56 (≈10° down) from
    /// 1024 above the ground, and the bolt's reach is only
    /// `life/3 + 1` steps of 384 — it can NEVER reach the ground on
    /// its own. Every kill comes from the acquire snapping the beam
    /// onto a creature. Under the shared ±0x71 pitch cone the flock
    /// underneath the cloud is outside the wedge, nothing locks, and
    /// the bolts sail out level "just above the monsters" — the
    /// reported bug. Retail's ±0x200 makes the storm the flock killer
    /// it is remembered as.
    fn aim_assist_mc1(&mut self, i: usize, ctx: &MobCtx) {
        let creature_pitch = if self.ent[i].model65 == 9 {
            0x200
        } else {
            0x71
        };
        self.aim_assist_mc1_cone2(i, ctx, 0x71, 0x71, Some(creature_pitch));
    }

    /// [`Self::aim_assist_mc1`] with an explicit acquire cone. The base
    /// MC1 scan is `0x71`/`0x71`; Hidden Worlds' Fire Storm child (model
    /// 16, acquire switch case 0x10, remc1hw :60322) widens the YAW cone
    /// to `0x100` while the pitch stays `0x71` on BOTH candidate lists.
    /// APPROX: case 0x10 scans the spatial buckets for any awake entity;
    /// we reuse the shared creature+wizard+player candidate set (the
    /// meaningful enemy set), only widening the cone.
    fn aim_assist_mc1_cone(&mut self, i: usize, ctx: &MobCtx, yaw_cone: u32, pitch_cone: u32) {
        self.aim_assist_mc1_cone2(i, ctx, yaw_cone, pitch_cone, Some(pitch_cone));
    }

    /// [`Self::aim_assist_mc1_cone`] with the CREATURE-bucket pitch cone
    /// split out: `sub_54520` case 9 (the lightning beam) is the one
    /// subtype that scores creatures on a different cone than the
    /// wizard/castle list. `creature_pitch: None` is the
    /// significant-list-only shape (blocks 7/8/B/C) — no creature
    /// sweep at all.
    fn aim_assist_mc1_cone2(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        yaw_cone: u32,
        pitch_cone: u32,
        creature_pitch: Option<u32>,
    ) {
        // sub_54520's entry clamp (:63975-76), BEFORE the model
        // switch: the acquire caps the projectile's +26 at 16 —
        // every ctor stamp above that (the possess lob's 200, a
        // fireball's high charge) is cut on the one-shot acquire
        // tick. The mc1l0 (9,1) f26 family, 234 rows.
        if self.ent[i].f26 > 16 {
            self.ent[i].f26 = 16;
        }
        let (px, py, pz, yaw, pitch, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.id24)
        };
        // The class-3 list's 3-D pre-gate (:64018-19): dist from the
        // bolt to the node's RAW +72 position vs the OWNER's row v_28
        // (`v28 = pool[164*+24] → +156 → +28` — the human wizard's
        // row 7 and a rival's row 8 both carry 8192; a creature
        // caster gates at its own row's reach). The beam alone (case
        // 9, :64137) gates on its own `f128 x max_life` instead.
        let sig_gate = if self.ent[i].model65 == 9 {
            (self.ent[i].f128 as i32).wrapping_mul(self.ent[i].max_life as i32)
        } else {
            let row = if self.is_human_tag(own) || own as usize >= self.ent.len() {
                7
            } else {
                self.ent[own as usize].row156
            };
            BEHAVIOR[row as usize].v_28 as i32
        };
        let mut best: Option<(u16, u32, u16, u16)> = None; // (slot, score, yaw, pitch)
        // sub_54A90's measurement order (:64196-217): yaw wedge,
        // pitch wedge, then the 2-D ground range — sub_423D0 has NO
        // z term (:52739) — and the score off the SAME truncated
        // ground distance.
        let consider = |tx: u16,
                        ty: u16,
                        tz: i16,
                        slot: u16,
                        pcone: u32,
                        best: &mut Option<(u16, u32, u16, u16)>| {
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dy = Self::angdist(yaw, ty_yaw) as u32;
            if dy > yaw_cone {
                return;
            }
            let dist = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
            let ty_pitch = Self::pitch_toward(pz, tz, dist);
            let dp = Self::angdist(pitch, ty_pitch) as u32;
            if dp > pcone || dist > 5120 {
                return;
            }
            let score = Self::acquire_score(dist, dy as usize, dp as usize);
            // Strictly-less: on a score tie the earlier candidate
            // wins, matching the original's scan order.
            if best.is_none() || best.is_some_and(|(_, bs, _, _)| score < bs) {
                *best = Some((slot, score, ty_yaw, ty_pitch));
            }
        };
        // The significant-entity list FIRST (:64016-37): bucket[0]
        // holds every live class-3 body — rival carpets, CASTLES and
        // mana BALLOONS; the walk carries NO model filter (the
        // membership ruling on [`Self::nearest_wizard_target`]).
        // ⭐ MEMBERSHIP IS THE TICK-TOP SNAPSHOT ([`TickChain`],
        // bucket[0] law): the per-node gates re-read only the owner
        // tag and the cloak bit (+16 0x20) plus the 3-D sig gate at
        // the node's RAW position — NO life re-test, so a wizard that
        // died mid-tick stays a lock candidate for the rest of the
        // tick (mc1l4 t=6865: the pelting stream's next ball locks the
        // rival's corpse the very tick it fell). Wizards/balloons
        // score through the generic scorer sub_54A90, whose sub_524C0
        // bracket lifts the aim z by +78; castles route to the castle
        // scorer sub_54BD0 (:64231 — same cones/range/score) at the
        // RAW position: the lift explicitly skips model 2. That is
        // how retail meteors fall a rival's castle out from under a
        // camping wizard (mc1hwl0 slot 522, chase=522). The
        // out-of-pool human goes first (the Scan-A tie-break ruling),
        // cloak-gated on spell 12's mirror (:65689-90).
        // ⭐⭐⭐ AND THE HUMAN IS ON BUCKET[0] ONLY WHILE HE IS ALIVE.
        // Retail's human wizard IS a pool class-3 record (mc1l49 slot
        // 569), so his membership is decided by the SAME tick-top
        // sweep as every other carpet: `sub_41780_41AC0`'s case-3 arm
        // (:52253-62) links a record onto `var_u32_36462[0]` only when
        // `actLife >= 0 && (+16 & 0x10) == 0`. VERIFIED in the shipped
        // bytes — CARPET.EXE 0x5A151 `83 78 0c 00` `cmpl $0,0xc(eax)`
        // / `7c 24` `jl` / `f6 40 10 10` `testb $0x10,0x10(eax)` /
        // `75 1e` `jne`, then `89 83 6e 8e 00 00` writes the head at
        // `+0x8e6e` (= 36462) — and the acquire's own walk reads that
        // very head (`mov 0x8e6e(%ebx),%ebx`, CARPET.EXE 0x6CEB9 in
        // `sub_54520`'s case-0/3/4 arm). Ours is out of pool, so his
        // seat rides the TICK-TOP latch `view.dead_top`, never the
        // live `dead` — exactly as [`Gen::nearest_wizard_target`]
        // already does for the same list.
        //
        // Without it a fireball cast after the human fell locked onto
        // his corpse and bent 23/34 off the muzzle bearing for the
        // rest of its flight: mc1l49 pair 34531→34532 (the human died
        // at t=34530, `actLife` −970) — slot 981 `chase` retail 0 port
        // 569, `heading` 260 vs 283, `target_yaw` 260 vs 283 — and
        // pair 35089→35090, slot 949 `heading` 921 vs 887. Between
        // them they gated 2,581 ticks of mc1l49.
        // ⭐ …AND ONLY WHILE HIS SEAT IS INSIDE THE CHAIN'S VISIBLE
        // PREFIX ([`Gen::mc1_human_on_wiz_chain`]): a seizure blank or
        // a sever below him this tick hides him from `sub_54520`'s
        // walk of `var_u32_36462[0]` exactly as it hides a pooled
        // carpet (mc1l19 t=14743/45/47, the pool at 999).
        // (On the roster he is met at his own slot in the walk below.)
        let on_chain = |g: &Self, hx: Hx| g.human_rides_chain(hx) && !no_acquire_human_bucket0();
        // Every seat off the roster ahead of the walk; on it, at his record.
        for hx in self.human_tag.ixs() {
            let (hpx, hpy, hpz) = self.seat_at(ctx, hx);
            if !on_chain(self, hx)
                && self.human_tag[hx].0 != own
                && (!self.seat_dead_top(hx) || no_acquire_human_bucket0())
                && !self.player_cloaked(hx)
                && self.mc1_human_on_wiz_chain(hx)
                && Self::dist3d(px, py, pz, hpx, hpy, hpz) <= sig_gate
            {
                consider(
                    hpx,
                    hpy,
                    hpz.wrapping_add(PLAYER_HH as i16),
                    self.human_tag[hx].0,
                    pitch_cone,
                    &mut best,
                );
            }
        }
        for k in 0..self.wiz_chain.visible_len() {
            let j = self.wiz_chain.list[k] as usize;
            let c = &self.ent[j];
            if let Some(hx) = self.hx_of_rec(j) {
                let (hpx, hpy, hpz) = self.seat_at(ctx, hx);
                if !on_chain(self, hx)
                    || self.human_tag[hx].0 == own
                    || self.player_cloaked(hx)
                    || Self::dist3d(px, py, pz, hpx, hpy, hpz) > sig_gate
                {
                    continue;
                }
                consider(
                    hpx,
                    hpy,
                    hpz.wrapping_add(PLAYER_HH as i16),
                    self.human_tag[hx].0,
                    pitch_cone,
                    &mut best,
                );
                continue;
            }
            if c.model65 == 0 {
                continue; // the human's body, handled above
            }
            if c.flags & 0x20 != 0 || c.id24 == own {
                continue;
            }
            if Self::dist3d(px, py, pz, c.x, c.y, c.z) > sig_gate {
                continue;
            }
            consider(c.x, c.y, c.aim_z(), j as u16, pitch_cone, &mut best);
        }
        // Then the creature buckets (:63990-64007) with the SAME
        // running best — a creature must strictly BEAT the class-3
        // pick. The 20 chains are keyed by MODEL and walked
        // model-major; MEMBERSHIP is the TICK-TOP snapshot
        // ([`MobChains`]) — a segment the castle crush promoted to a
        // corpse state mid-tick stays invisible to this sweep until
        // the next rebuild (mc1l1 t=4130: the fireball flies straight,
        // f146 = 0). Per-node gates read LIVE: awake (+58) and not
        // the shooter's own (:63995-96) — NO cloak, NO act/state
        // re-test, NO distance beyond the scorer's own 5120.
        if let Some(creature_pitch) = creature_pitch {
            for m in 0..20usize {
                let chain: Vec<u16> = self.mob_chains.visible(m).to_vec();
                for j in chain {
                    let c = &self.ent[j as usize];
                    if c.f58 == 0 || c.id24 == own {
                        continue;
                    }
                    consider(c.x, c.y, c.aim_z(), j, creature_pitch, &mut best);
                }
            }
        }
        if let Some((slot, _, ty_yaw, ty_pitch)) = best {
            self.ent[i].f146 = slot;
            self.ent[i].f34 = ty_yaw;
            self.ent[i].f36 = ty_pitch;
            // Being targeted arms the danger music (:64013/:64095 —
            // acquire of a class-3 m0 human calls sub_46520) — in the
            // 0/3/4 block and the 7/8/B/C block ONLY. The LIGHTNING's
            // case 9 (:64125-64191, CARPET.EXE 0x54654-0x54681: `+146
            // = slot; call sub_52500; mov $1,%eax; ret`) has no such
            // call — `sub_54520` holds exactly two `call sub_46520`
            // sites, 0x54807 and 0x54A6D, neither in case 9 (HW's
            // :60257-60321 likewise; its case 0x10 :60354 does arm).
            // mc1l49 t=5673: rival 646's (9,9) at slot 797 locks the
            // carpet above its walk slot and retail's `danger` reads
            // 99 (the earlier arm, one step down), the port's re-arm
            // 100. `MGC_NO_MC1_DANGER_ACQUIRE_CASES` re-arms on 9.
            if let Some(hx) = self.hx_of_tag(slot)
                && (self.ent[i].model65 != 9 || no_mc1_danger_acquire_cases())
            {
                self.player_danger[hx] = 100;
            }
        }
    }

    /// Read-only twin of the acquire family below for the crosshair
    /// instrument (P-class `crosshair` option): identical candidate
    /// filters, cone (±0x71 yaw AND pitch), 2-D range (≤ 5120) and
    /// min-score pick as [`Self::aim_assist`] /
    /// [`Self::aim_assist_wizards`] / [`Self::aim_assist_possess`] —
    /// but NO entity writes, NO `player_danger` arming and NO LCG
    /// draws, so it is safe to run every frame without touching
    /// simulation state. The caster is the human player
    /// (own = PLAYER_TARGET), so the mob scans' player-candidate arm
    /// never applies. Returns the acquired slot.
    pub(crate) fn aim_preview_scan(
        &self,
        hx: Hx,
        px: u16,
        py: u16,
        pz: i16,
        yaw: u16,
        pitch: u16,
        set: AimPreviewSet,
    ) -> Option<u16> {
        let own = self.human_tag[hx].0;
        let mut best: Option<(u16, u32)> = None;
        // sub_54A90's measurement, mirroring the live consider: yaw
        // wedge, pitch wedge, 2-D ground range, weighted score.
        let mut consider = |tx: u16, ty: u16, tz: i16, slot: u16| {
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dy = Self::angdist(yaw, ty_yaw) as u32;
            if dy > 0x71 {
                return;
            }
            let dist = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
            let ty_pitch = Self::pitch_toward(pz, tz, dist);
            let dp = Self::angdist(pitch, ty_pitch) as u32;
            if dp > 0x71 || dist > 5120 {
                return;
            }
            let score = Self::acquire_score(dist, dy as usize, dp as usize);
            if best.is_none_or(|(_, bs)| score < bs) {
                best = Some((slot, score));
            }
        };
        if set == AimPreviewSet::Possess {
            // Mirror of aim_assist_possess: unowned/unclaimed awake
            // mana balls (m39/40) + anyone else's houses (m45).
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 10 || c.flags & 0x400 != 0 {
                    continue;
                }
                let candidate = match c.model65 {
                    39 | 40 => c.f58 != 0 && c.f144 != own && c.id24 != own,
                    45 => c.f144 != own && c.id24 != own,
                    _ => false,
                };
                if candidate {
                    consider(c.x, c.y, c.aim_z(), j as u16);
                }
            }
            return best.map(|(slot, _)| slot);
        }
        // Both remaining sets walk the class-3 significant list FIRST
        // (mirror of aim_assist_mc1_cone2): every live body — rival
        // carpets, castles, balloons — not cloaked/removed, inside
        // the human wizard row's v_28 at the RAW position; model 2
        // scores at the flag z (no +78 lift), the rest lifted.
        let sig_gate = BEHAVIOR[7].v_28 as i32;
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 != 3 || c.model65 == 0 {
                continue;
            }
            if c.act_life < 0 || c.flags & 0x420 != 0 || c.id24 == own {
                continue;
            }
            if Self::dist3d(px, py, pz, c.x, c.y, c.z) > sig_gate {
                continue;
            }
            consider(c.x, c.y, c.aim_z(), j as u16);
        }
        // The Creatures set adds the model-major bucket sweep.
        if set == AimPreviewSet::Creatures {
            for m in 0..20u8 {
                for j in 1..self.ent.len() {
                    let c = &self.ent[j];
                    if c.class64 != 5 || c.model65 != m || c.tick70 == 120 || c.act_life < 0 {
                        continue;
                    }
                    if c.f58 == 0 || c.id24 == own {
                        continue;
                    }
                    consider(c.x, c.y, c.aim_z(), j as u16);
                }
            }
        }
        best.map(|(slot, _)| slot)
    }

    /// The wizard-only acquire subtype's TargetingVerb seam (see
    /// [`Self::aim_assist`]).
    fn aim_assist_wizards(&mut self, i: usize, ctx: &MobCtx) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_wizards_mc1(i, ctx),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_wizards_mc1(i, ctx);
            }
        }
    }

    /// The significant-list-only acquire (sub_54520 blocks 7/8/B/C —
    /// duel m7, steal m8, undead m11): the same class-3 walk, gates,
    /// cone, v_28 pre-gate and score as [`Self::aim_assist`], minus
    /// the creature-bucket sweep. The decompile carries NO model
    /// filter here either (:64100-118 — castles and balloons are
    /// candidates alongside the carpets; the `+65 == 2` split calls
    /// the same scorer in both arms).
    fn aim_assist_wizards_mc1(&mut self, i: usize, ctx: &MobCtx) {
        self.aim_assist_mc1_cone2(i, ctx, 0x71, 0x71, None);
    }

    /// sub_52550 (:62534): per-tick homing — recompute bearing to the
    /// target (z-centered via the
    /// [`crate::engine::features::Ent::aim_z`] model-2 bracket:
    /// castles home at the FLAG, not 8192 under the base) and turn
    /// yaw/pitch capped at the row's v_2/v_6.
    ///
    /// THE TRACKER NEVER RE-VALIDATES (:62543-55): retail computes the
    /// bearing to whatever `pool[164 * +146]` holds — a corpse waiting
    /// on the reaper, even a slot recycled into a different entity
    /// (mc1l0 t=1818-30: two lobs track slots that are live
    /// PROJECTILES by then, and retail steers onto them). The port
    /// used to clear +146 on a dead/empty slot — the 133-pair
    /// `chase → 0` family. Only the out-of-range guard stays
    /// (defensive; a retail +146 is always a pool index).
    /// Test seam for [`Self::home`] (the per-tick homing step is
    /// otherwise only reachable through a full flight tick).
    #[cfg(test)]
    pub(crate) fn home_for_test(&mut self, i: usize, ctx: &MobCtx) -> bool {
        self.home(i, ctx)
    }

    /// Test seam for [`Self::proj_move_and_hit`]'s generic arm (the
    /// strike's move-then-probe-then-snap chain choreography is
    /// otherwise only reachable through a full flight tick).
    #[cfg(test)]
    pub(crate) fn proj_strike_for_test(&mut self, i: usize, ctx: &MobCtx) -> bool {
        self.mc1_aim_latch = crate::engine::features::HashSilent(self.ent[i].f146);
        self.proj_move_and_hit(i, ctx, false, false, DeflectLaw::Generic)
    }

    fn home(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let tgt = self.ent[i].f146;
        let (tx, ty, tz) = if let Some(hx) = self.hx_of_tag(tgt) {
            (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2.wrapping_add(PLAYER_HH as i16),
            )
        } else {
            let t = tgt as usize;
            if t == 0 || t >= self.ent.len() {
                return false;
            }
            let c = &self.ent[t];
            (c.x, c.y, c.aim_z())
        };
        let e = &self.ent[i];
        let yaw = Self::angle_between(e.x, e.y, tx, ty);
        let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, tx, ty) as u32) as i32;
        // The aim lift is an IN-PLACE bracket on the MEASURED record
        // (sub_524C0 :62509 writes +76, sub_52550 reads a1's own +72
        // between the lift and the un-lift at :62542-56), not the
        // pure `z + f78` this port models. The two agree for every
        // distinct (shooter, target) pair and diverge on the ALIAS:
        // when +146 holds the shooter's OWN slot the lift raises the
        // shooter too, so dz is exactly 0 over dh 0 and homing at
        // yourself is a NO-OP. Reading the target lifted against an
        // unlifted self gave dz = +f78 over dh = 0 = pitch 1536
        // (straight up), and the lob climbed 113/tick — mc1l2 slot
        // 317, t=2300..2310, a lob born into a freed ball's slot that
        // the possess acquire then scored against itself.
        let fz = if tgt as usize == i { e.aim_z() } else { e.z };
        let pitch = Self::pitch_toward(fz, tz, dh);
        // ⚠ THE ROW LIVES IN THE ENGINE'S OWN TABLE. `row156` is an
        // index into whichever behavior table the column owns —
        // `unk_98F38` (0-based) on MC1, `str_D7BD6` (ABSOLUTE, base 59)
        // on MC2 — and MC2 records reach this shared homer. Reading
        // MC1's 31-row table with an MC2 index was a silent wrong-row
        // read before the allocator seeded row 59, and an out-of-bounds
        // panic after it.
        let (v2, v6) = if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            let row = &crate::mc2::behavior::BEHAVIOR[e.row156 as usize];
            (row.v_2, row.v_6)
        } else {
            let row = &BEHAVIOR[e.row156 as usize];
            (row.v_2, row.v_6)
        };
        self.ent[i].f34 = yaw;
        self.ent[i].f36 = pitch;
        let ty_ = Self::turn_step(self.ent[i].f30, yaw, v2);
        self.ent[i].f30 = (self.ent[i].f30 as i32 + ty_ as i32) as u16 & 0x7FF;
        let tp = Self::turn_step(self.ent[i].f32, pitch, v6);
        self.ent[i].f32 = (self.ent[i].f32 as i32 + tp as i32) as u16 & 0x7FF;
        true
    }

    /// ⭐⭐⭐ `sub_10780`'s SELF-EXCLUSION COMPARES `@0x1A` ON BOTH
    /// SIDES — `a1x->id_0x1A_26 != v5x->id_0x1A_26` (EF:3769) — and
    /// the port's `id24` is a **FUSION** of retail's `@0x1A` (the
    /// record's own id, `NewEvent`'s default = its slot) and `@0x28`
    /// (`parentId`, the owner). `obs_project_mc2` already enumerates
    /// the families where the fusion holds the OWNER (world/
    /// conformance.rs `let translated`): class-15 tokens, the (10,42)
    /// and (10,57) spheres, and the (5,·) pyramid puppets. For those
    /// records the probe was reading the OWNER where retail reads the
    /// SELF ID, so any projectile sharing their owner flew straight
    /// through them.
    ///
    /// mc2l6-rsg t=11664 is the witness: the human's fireball at slot
    /// 55 steps onto his OWN Fool's-Mana sphere (10,57) at slot 547
    /// — all three AABB axes pass by 64/50/103 against half-sums of
    /// 132/132/134 — and retail detonates, teleporting the bolt onto
    /// the sphere's raised centre (`sub_65580`: z 1281 + box.fov 84 =
    /// **1365**, retail's recorded value) and minting the (10,0)
    /// impact at slot 510. Retail compares `343 != 547`; the port
    /// compared `343 != 343` and skipped, so slot 510 went to that
    /// tick's newly cast fireball and slot 729 — retail's — never
    /// existed.
    ///
    /// The (5,·) puppet arm needs a parent-class lookup and is not
    /// exercised by this take; it is left for a measured follow-up.
    ///
    /// ⭐⭐ AND THE MAGIC MINE (10,78) IS THE NEXT MEMBER. Its ctor
    /// `sub_50840` never touches `@0x1A` (EF:36960-82 — fourteen field
    /// writes, none of them the id) and the carrier tail that arms it,
    /// `sub_67960`, stamps the owner into `word_0x32_50` INSTEAD
    /// (EF:59356), so retail's mine keeps `NewEvent`'s own-slot id:
    /// mc2l6-rsg t=13544 slot 178 reads `f1a` **178** against the
    /// port's fused **343**. Without the unfuse the caster's own bolt
    /// compares `343 != 343` and flies through the mine, and
    /// `sub_68AC0` — the whole Magic Mine mechanic
    /// ([`Gen::mc2_mine_swallow`]) — is never handed it.
    fn probe_self_id(&self, j: usize) -> u16 {
        let c = &self.ent[j];
        if !no_probe_id_unfuse()
            && matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
            && c.id24 as usize != j
            && (matches!((c.class64, c.model65), (15, _) | (10, 42) | (10, 57))
                || ((c.class64, c.model65) == (10, 78) && !crate::mc2::proj::no_mine_swallow()))
        {
            return j as u16;
        }
        c.id24
    }

    /// `sub_11980` / `sub_10780` as retail runs it: the SEARCH.DAT ring
    /// of tile chains, first overlapping member passing the owner,
    /// damageable, class-14 and `+66/+67` gates. With the human's record
    /// a pool victim this IS the probe; otherwise [`Self::victim_scan`]
    /// adds the out-of-pool player arms around the same walk.
    fn victim_scan_pool(&self, i: usize, ctx: &MobCtx) -> Option<usize> {
        let (wx, wy, id, f66, f67) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24, e.f66, e.f67)
        };
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        for &t in &self.probe_window(wx, wy, r, ctx.strict) {
            let mut j = self.map_entity[t] as usize;
            while j != 0 {
                let c = &self.ent[j];
                if self.probe_self_id(j) != id
                    && c.flags & 8 != 0
                    && (ctx.strict || c.class64 != 14)
                    && Self::filter_admits(f66, f67, c.class64, c.model65)
                    && self.ent_overlap(i, j)
                {
                    return Some(j);
                }
                j = c.next20 as usize;
            }
        }
        None
    }

    /// sub_11980 (:16988) from a projectile: first overlapped victim
    /// in the surrounding cells passing the filter/owner/damageable
    /// gates. The out-of-pool player rides the same cell window (see
    /// the tail).
    ///
    /// Geometry identical to its `sub_11AC0` sibling below, and for
    /// the same reason — both walk the SEARCH.DAT ring iterator, not
    /// a square: `sub_11410(0, (+80 + 255) >> 8)` at :16999 over a
    /// centre rounded to the NEAREST tile, `(+72 + 128) >> 8`
    /// (:17000-01). The MC2 twin `sub_10780` is byte-identical
    /// (`AddE7EE0x_10080(0, …)`, EF:3700-04). The port truncated the
    /// centre and walked a square with a `.max(1)` floor, so a
    /// zero-extent bolt swept nine tiles instead of its own, and
    /// every probe sat up to a tile behind the shot.
    fn victim_scan(&self, i: usize, ctx: &MobCtx) -> Option<MailTarget> {
        // ⭐ His linked record carries the damageable bit: the probe is
        // retail's plain chain walk and meets him there
        // ([`crate::engine::features::no_human_pool_victim`]). With
        // every seat a linked pool victim that is the whole scan;
        // otherwise the out-of-pool arm below serves the first seat
        // that is not (the others still meet the walk at their records).
        let Some(hx) = self.human_tag.ixs().find(|&h| !self.human_pool_victim(h)) else {
            return self.victim_scan_pool(i, ctx).map(MailTarget::Pool);
        };
        let seat = self.seat_at(ctx, hx);
        let (wx, wy, id, f66, f67) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24, e.f66, e.f67)
        };
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        let cells = self.probe_window(wx, wy, r, ctx.strict);
        // ⚠ THE OUT-OF-POOL HUMAN JOINS NO TILE CHAIN, so he is
        // checked AS HIS CELL COMES UP in the ring walk, ahead of
        // whatever the chain there holds (the `mc2_piece_scan`
        // pattern). Retail's carpet is an ordinary linked record and
        // the probe returns the FIRST overlapping record in SEARCH.DAT
        // ring order — a post-loop player arm loses every tiebreak
        // where a pool victim sits in a LATER cell: mc1hwl0-pd t=451,
        // the kraken beam's step-4 probe from rounded cell (99,182) —
        // the human's cell (98,181) is ring-1 index 0, the rival beam
        // segment 584's cell (99,181) is index 1, and the post-pass
        // handed the bolt to 584 (chain-snap 118/162/170 off on the
        // (9,9), chase 583-for-472 on the (10,23)). The two escape
        // arms below keep their post-pass semantics.
        let ptile = tile((seat.0 >> 8) as u8, (seat.1 >> 8) as u8);
        let player_ordered = !no_probe_window_player()
            && !(!ctx.strict && matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2));
        // ⭐⭐⭐ INSIDE HIS OWN CELL THE HUMAN IS THE CHAIN'S TAIL, NOT
        // ITS HEAD. `AddEventToMap_57D70` (EF:40315-27) is a HEAD
        // insertion — `oldMapEntity_0x16_22 = mapEntityIndex[cell]`
        // then `mapEntityIndex[cell] = entity` — and
        // `CopyEntityPosition_57CF0` (EF:40282-99) relinks only when
        // the tile CHANGES, so a tile chain is ordered
        // most-recently-entered FIRST and a parked record sinks to the
        // tail. The carpet is an ordinary member of it, so anything
        // that flew into the human's tile after he settled there is
        // walked BEFORE him.
        let player_cell_tail = !crate::mc2::mobs::no_player_cell_tail();
        // ⭐⭐⭐ AND NEITHER END IS THE LAW: THE HUMAN HOLDS A SEAT.
        // Head-insertion orders a chain most-recently-entered FIRST,
        // so retail's linked carpet sits wherever his last tile entry
        // put him — AHEAD of everything parked in the tile since
        // before he arrived, BEHIND everything that flew in after.
        // `Gen::player_chain` carries that seat for the out-of-pool
        // human (see [`crate::engine::features::PlayerChain`]).
        //
        // mc1l48 t=6593→6594 is the witness both END rules fail on.
        // The rival's homing fireball at slot 255 steps into cell
        // (26,138), whose chain runs 319 → 307 → **681 (the human)**
        // → 306 → … → 708 → 874 (his castle). The bolt's +66/+67 is
        // (3, −1), so only class-3 members are candidates: the human,
        // the (3,3) balloon 708 and the castle 874. Retail's
        // `sub_11980` returns the human — he is the first class-3 in
        // walk order — and detonates on his carpet at (6714, 35439,
        // 2929+100). The TAIL rule walked past him to the castle and
        // teleported the bolt onto (6656, 35328, 2912) instead, which
        // is where the SAME tick's other fireball (slot 318, one tile
        // further out and 234 units off the human in y) legitimately
        // lands. The seat gets both right in one walk.
        //
        // MC1 COLUMN ONLY. An imported MC2 world keeps its carpet as
        // a LIVE pool record (`import_ent_mc2` does not hole it out),
        // so the human is already in the chain there and the tail
        // rule is what keeps the redundant player arm behind him —
        // a seat would double-represent him.
        let seat_law = !no_player_chain_seat()
            && !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        for &t in &cells {
            let player_here = player_ordered
                && t == ptile
                && self.human_tag[hx].0 != id
                && Self::filter_admits(f66, f67, 3, 0);
            let seat_here = seat_law && player_here && self.player_chain[hx].cell == t;
            if player_here && !seat_here && !player_cell_tail && self.player_overlap_at(i, seat) {
                return Some(MailTarget::Player(hx));
            }
            let mut j = self.map_entity[t] as usize;
            loop {
                // The seat: his own linked record, or (no record) the
                // GAP before his successor (0 = the chain tail, so this
                // fires on the terminating j == 0 pass). A seat that no
                // longer names a member of this chain falls through to
                // the tail arm below.
                if seat_here && self.human_seat_at(j) && self.player_overlap_at(i, seat) {
                    return Some(MailTarget::Player(hx));
                }
                if j == 0 {
                    break;
                }
                let c = &self.ent[j];
                // Class-14 map objects (MC2 XP scrolls, mouth/
                // checkpoint markers) are OBSERVABLE pass-through
                // in NATIVE play: retail's probe admits them
                // mechanically (the (14,5) ctor keeps byte[0]&8,
                // EF:37315/37365, and a player bolt's xtype is the
                // −1 wildcard) but its ≈0-box, own-cell,
                // endpoint-only probe rarely reaches the scroll's
                // 768/1280 PICKUP box (EF:63127-28 +
                // Events.cpp:132 ring 0). Our anti-tunneling ring +
                // chord-march (below/mc2 proj) WOULD reach it — the
                // player's "fireballs detonate on scrolls / scrolls
                // steal autoaim" report — so the guard restores the
                // retail observable (2026-07-16 scroll trace;
                // MC1 has no class-14, goldens untouched).
                //
                // ⚠ STRICT-RETAIL REPLAY IS EXEMPT — the SAME
                // treatment DEVIATIONS §262 gave the march and the
                // square, and for the same reason: this guard exists
                // only to compensate for THEM, and under `strict`
                // neither is live, so gating here would be gating
                // against retail's own geometry. "Rarely reaches"
                // is not "never": mc2l0 t=9216 detonates FIVE (9,0)
                // fireballs at once on the (14,5) cluster minted the
                // tick before (slots 58/59/60 at 768/768/1280,
                // ground z 0), each landing at victim z + its ayaw
                // 37 — the port flew all five straight through.
                if self.probe_self_id(j) != id
                    && c.flags & 8 != 0
                    && (ctx.strict || c.class64 != 14)
                    && Self::filter_admits(f66, f67, c.class64, c.model65)
                    && self.ent_overlap(i, j)
                {
                    return Some(MailTarget::Pool(j));
                }
                j = c.next20 as usize;
            }
            if player_here && (seat_here || player_cell_tail) && self.player_overlap_at(i, seat) {
                return Some(MailTarget::Player(hx));
            }
        }
        // The player probe. `sub_11980` has NO player arm: it returns
        // pool records only (:17020-27), so retail reaches the human
        // exactly like every other victim — through the tile chain,
        // where the carpet sits as an ordinary linked record. So the
        // probe's CELL WINDOW gates it, the same law [`Self::area_write`]
        // already carries: the carpet is linked at its plain
        // `(x>>8, y>>8)` tile (`sub_41CF0` :52468, truncated), while
        // the probe centres on the ROUNDED cell (:17000-01) and walks
        // the SEARCH.DAT ring from there — so the two grids are half a
        // tile out of step and a bolt whose ring stops one cell short
        // sails straight past a carpet its AABB overlaps.
        //
        // The port's unconditional AABB arm is what killed the genie's
        // mana-steal seeker a tick early all over mc1l42 (~20 events,
        // the level's largest single family, and the free replay's
        // t=1162 wall): exemplar t=4418 slot 373, seeker stepping to
        // (6643, 35222) = rounded cell (26, 138), carpet 331 at its
        // POST-walk sample (6731, 35054) = linked cell (26, 136) —
        // two rows below a ring that reaches 137. All three AABB axes
        // pass by 88/168/4 against half-sums of 194/194/175, and
        // retail still does not hit: retail's scan never sees the
        // record. The port hit, took the `MailTarget::Player` arm,
        // teleported onto the carpet and spawned its (10,25) flash a
        // tick early.
        //
        // MC2 NATIVE PLAY keeps the pure AABB probe, in step with
        // `area_write`: its window is the inflated square with the
        // `.max(1)` floor (see [`Self::probe_window`]), one half of a
        // compensating pair with the chord march, so gating it there
        // would be gating a window that is not retail's. Under
        // `strict` that square IS the ring, so the cell gate is
        // retail's again and MC2 rejoins the shared arm.
        // Post-pass ONLY for the ordered-walk escapes: the
        // `MGC_NO_PROBE_WINDOW_PLAYER` instrument and MC2 NATIVE play
        // (whose inflated `.max(1)` square + chord march is the
        // documented compensating pair — the cell-ordered arm above
        // would order against a window that is not retail's).
        if !player_ordered
            && self.human_tag[hx].0 != id
            && Self::filter_admits(f66, f67, 3, 0)
            && self.player_overlap_at(i, seat)
        {
            return Some(MailTarget::Player(hx));
        }
        None
    }

    /// The CLAIM/possession candidate test `sub_108B0` (EF:3766)'s
    /// whitelist body. The possession projectile (action 18) does NOT
    /// collide with every solid like the generic `victim_scan`
    /// (`sub_10780`) — it detonates ONLY on entities it could claim
    /// and flies straight through everything else. Whitelist (verbatim
    /// sub_108B0, EF:3826-58): worm heads (5,22); the 512/random mana
    /// spheres (10,39)/(10,40); the foreign-owned sphere variant
    /// (10,57) when its parent tag differs from the caster; and
    /// buildings (10,45) ONLY when POSSESSABLE — `bldgprm.flags & 8
    /// == 0`. The un-possessable factory / terrain-modification
    /// buildings (level-001 cross sinks, level-000 spires) and every
    /// wizard / marker keep the bit set or fall off the list, so
    /// possession passes through them (NOT the generic probe, which
    /// would consume the shot on those sinks). Retail's accept filter
    /// (EF:3862-67) is TWO-armed: the creator half (`id_0x1A_26` →
    /// `id24`) AND the claim-owner half (`playerEntityIndex_0x94_148`
    /// → `f144`, the field both claim intakes write) — a ball or
    /// building the caster already possesses does NOT eat the bolt;
    /// it flies through to the unclaimed field behind. A
    /// rival-claimed target fails neither half and stays claimable —
    /// PLAYER RETAIL-CERTIFIED 2026-07-27 for ALL tiers including
    /// the tier-1 Mana Magnet (a briefly-tried tier-1 carve-out that
    /// detonated on own-claimed spheres was refuted by the player's
    /// own retail discriminator run: it does NOT explode on already
    /// possessed mana; the "sticks to my mana" feel is the aura
    /// piling the claimed spheres onto the detonation point).
    fn claim_admits(&self, j: usize, own: u16) -> bool {
        let c = &self.ent[j];
        if c.flags & 8 == 0 {
            return false;
        }
        match (c.class64, c.model65) {
            (5, 22) => c.id24 != own && c.f144 != own,
            (10, 39) | (10, 40) => c.id24 != own && c.f144 != own,
            // The (10,57) foreign sphere: gated on the PARENT TAG
            // alone, no id/owner re-check (sub_108B0's early-return
            // arm, EF:3846 `v8x->parentId_0x28_40 != a1x->id_0x1A_26`).
            //
            // That parent tag is retail's `@0x28`, and the port's home
            // for `@0x28` is `id24` — the importer fuses `id_0x1A` and
            // `parentId_0x28` into it (world/conformance.rs `owner28`)
            // and `mc2_fools_retaliate` reads the same lane for its
            // owner-skip. `f40` is `@0x26`, an unrelated latch; the
            // old test there was a live carve-out failure the moment a
            // CAST decoy (whose parentId is the caster) met its own
            // caster's possess bolt — retail flies through, the port
            // detonated. Until OPEN-6 stamped the native model this
            // arm only ever saw IMPORTED spheres, whose `@0x26` and
            // `@0x28` both read 0, which is why it never showed.
            (10, 57) => c.id24 != own,
            (10, 45) => {
                c.id24 != own
                    && c.f144 != own
                    && self
                        .assets
                        .bldgprm
                        .get(c.f71 as usize)
                        .is_none_or(|b| b.flags & 8 == 0)
            }
            _ => false,
        }
    }

    /// The possession victim probe `sub_108B0` (EF:3766): the same
    /// tile-chain sweep as [`Self::victim_scan`] but under the
    /// claim whitelist ([`Self::claim_admits`]) — and with NO player
    /// probe (sub_108B0 never reaches the human wizard; you cannot
    /// possess a wizard). Ring-iterator geometry on BOTH games,
    /// verbatim: `AddE7EE0x_10080(0, (+82 + 255) >> 8)` over the
    /// `(pos + 128) >> 8` centre (EF:3798-3801) — NOT
    /// [`Self::probe_window`]'s MC2 square: that square is one half
    /// of the generic march's compensating pair, and the claim path
    /// probes ONCE at the endpoint like retail, so the square's
    /// truncated centre reads one cell short of the ring on grazing
    /// geometry (mc2l0 t=65: the (9,1) bolt's endpoint sits two
    /// x-cells from the (10,45) building's chain cell — the
    /// 2x2-anchored ring reaches it, the ±1 square does not).
    fn claim_victim_scan(&self, i: usize) -> Option<MailTarget> {
        let (wx, wy, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.id24)
        };
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        let cx = ((wx as i32 + 128) >> 8) as u8;
        let cy = ((wy as i32 + 128) >> 8) as u8;
        for t in self
            .ring_cells(0, r)
            .into_iter()
            .map(|(dx, dy)| tile(cx.wrapping_add(dx), cy.wrapping_add(dy)))
        {
            let mut j = self.map_entity[t] as usize;
            while j != 0 {
                let next = self.ent[j].next20 as usize;
                if self.claim_admits(j, id) && self.ent_overlap(i, j) {
                    return Some(MailTarget::Pool(j));
                }
                j = next;
            }
        }
        None
    }

    /// The projectile probe's cell window — the ONE seam where the
    /// two games part (§THE HELD-BACK AREA FIXES, landed for MC1):
    ///
    /// - **MC1/HW: retail's geometry exactly.** `victim_scan`
    ///   (sub_11980 :16999-17001) and its possession sibling walk
    ///   `sub_11410(0, (+80 + 255) >> 8)` — the SEARCH.DAT ring
    ///   iterator, forward-biased 2x2-anchored shells with the
    ///   last-cell drop — over the centre rounded to the NEAREST
    ///   tile (`(+72 + 128) >> 8`). No radius floor: a zero-extent
    ///   bolt probes ring 0's own 2x2 block alone. Retail gets away
    ///   with the narrow window because the MC1 mover probes ONCE,
    ///   at the end of the move — which the port's MC1 movers also
    ///   do (endpoint-only `victim_scan_at`).
    ///
    /// - **MC2 NATIVE PLAY: the port's inflated square window stays.**
    ///   The MC2 mover ray-marches the chord in ≤128-unit sub-steps
    ///   (the documented anti-tunnel deviation for zero-width sprite
    ///   boxes), and that march and the truncated-centre square with
    ///   its `.max(1)` floor are ONE compensating family — measured
    ///   2026-08-12: giving MC2 the retail ring cost five pinned
    ///   fixtures (fools-trap muzzle, meteor homing lock, arrow
    ///   collateral, two muzzle-admission guards) and mc2l4 t=621.
    ///   They come out together or not at all (the +1/+2 mc2l4/
    ///   mc2l30 ring pairs are forfeited with it, documented there).
    ///
    /// - **MC2 UNDER `strict` (conformance replay): retail's geometry,
    ///   like every other game.** The compensating pair comes out
    ///   TOGETHER exactly as the deviation requires — the march
    ///   collapses to retail's single endpoint probe in
    ///   `mc2_proj_flight`, and the window becomes the ring — so the
    ///   strict lane carries neither half and the native lane carries
    ///   both. Measured mc2l0 t=3992: the (9,0) fireball's chord from
    ///   (50460, 53582) to (50708, 53901) passes the (5,4) archer at
    ///   slot 142 without either endpoint overlapping it (post-move
    ///   |Δx| = 195 and pre-move |Δy| = 313, both against half-sums of
    ///   176), but sub-step 2 of 4 sits at (50584, 53741) — inside on
    ///   all three axes. The port burst and minted a (10,0) into slot
    ///   48; retail flew past and terrain-contacted a tick later.
    fn probe_window(&self, wx: u16, wy: u16, r: i32, strict: bool) -> Vec<usize> {
        if !strict && matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            let r = r.max(1);
            let mut out = Vec::with_capacity(((2 * r + 1) * (2 * r + 1)) as usize);
            for dy in -r..=r {
                for dx in -r..=r {
                    let tx = ((wx >> 8) as i32 + dx) as u8;
                    let ty = ((wy >> 8) as i32 + dy) as u8;
                    out.push(tile(tx, ty));
                }
            }
            out
        } else {
            let cx = ((wx as i32 + 128) >> 8) as u8;
            let cy = ((wy as i32 + 128) >> 8) as u8;
            self.ring_cells(0, r)
                .into_iter()
                .map(|(dx, dy)| tile(cx.wrapping_add(dx), cy.wrapping_add(dy)))
                .collect()
        }
    }

    /// [`Self::claim_victim_scan`] at a temporary probe position (the
    /// marched-substep companion of [`Self::victim_scan_at`]).
    pub(crate) fn claim_victim_scan_at(
        &mut self,
        i: usize,
        tmp: (u16, u16, i16),
    ) -> Option<MailTarget> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let v = self.claim_victim_scan(i);
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        v
    }

    /// The Undead-Army bolt's `+44` as `sub_57800` leaves it (:65962-71):
    /// 1, or the token's price when wizard `wiz`'s castle register
    /// names a record whose `+136` covers it — the register is an index
    /// test alone. `None` under [`no_mc1_undead_bolt_44`].
    pub(crate) fn mc1_undead_bolt_44(&self, wiz: usize, price: u32) -> Option<u16> {
        if no_mc1_undead_bolt_44() {
            return None;
        }
        let reg = self.castle_reg[wiz & 7] as usize;
        let housed = reg != 0 && reg < self.ent.len() && self.ent[reg].f136 as u32 >= price;
        Some(if housed { price as u16 } else { 1 })
    }

    /// `sub_526C0`'s model gate (:62591-98, CARPET.EXE 0x526CC-0x526F2):
    /// which class-9 models are SHOTS for the human's `+343/+347`
    /// counters — 0, 1, 3, 7, 8, 9 and 19; every other model returns
    /// before `shots++`. `MGC_NO_MC1_SHOT_STATS_MODEL_GATE` counts all.
    /// `HIDDEN.EXE`'s twin adds model 16 ([`no_mc1hw_shot_stats_m16`]).
    fn mc1_shot_counts(&self, model: u8) -> bool {
        no_mc1_shot_stats_model_gate()
            || matches!(model, 0 | 1 | 3 | 7..=9 | 19)
            || (model == 16 && self.is_hidden_worlds() && !no_mc1hw_shot_stats_m16())
    }

    /// `sub_526C0` (:62585-612) for a human-owned detonation: the
    /// model gate first — only bolt models 0/1/3/7/8/9/19 are shots —
    /// then `shots++`, then the hit test against the aimed pointer the
    /// HANDLER latched at dispatch entry (`Gen::mc1_aim_latch`), not
    /// the live `+146`. See `no_mc1_shot_stats_model_gate` /
    /// `no_mc1_hit_stat_aim_latch` for the citations.
    fn mc1_shot_stats(&mut self, i: usize, struck: Option<MailTarget>) {
        if !self.mc1_shot_counts(self.ent[i].model65) {
            return;
        }
        let aimed = if no_mc1_hit_stat_aim_latch() {
            self.ent[i].f146
        } else {
            self.mc1_aim_latch.0
        };
        let hit = self.mc1_shot_hit(struck, aimed);
        self.mc1_note_shot(self.ent[i].id24, self.ent[i].model65, hit);
    }

    /// `sub_526C0(a1, 0, ..)` — THE SILENT END IS A SHOT (round 168).
    /// `sub_530C0`'s tail (remc1 sub_main.cpp:63186-63210, the state
    /// 7 / 8 handler: the duel dart, the steal ball and every REBOUNDED
    /// bolt) forks on the struck record: a wizard mints the child and
    /// scores inside the allocation guard (:63193-95); ANYTHING ELSE —
    /// a creature, the ground, the life running out — is
    /// `sub_526C0(a1, 0, v21); sub_41E80(a1)` (:63208-09), no spawn
    /// and no guard. A null struck record can never be a hit, so it is
    /// `shots++` alone, behind the same model gate and owner test.
    /// WITNESS mc1l32-quick t=9962: the human's rebounded fireball
    /// (slot 2, `+70` 8) flies its life out at z 8759 and `shots`
    /// reads 1226 -> 1227 with no effect born; the port's three
    /// silent ends scored nothing. `MGC_NO_MC1_SILENT_END_SHOT=1`
    /// restores that.
    fn mc1_shot_stats_silent(&mut self, i: usize) {
        if no_mc1_silent_end_shot() {
            return;
        }
        let (own, model) = (self.ent[i].id24, self.ent[i].model65);
        if self.is_human_tag(own) && self.mc1_shot_counts(model) {
            self.mc1_note_shot(own, model, false);
        }
    }

    /// Bill one gated shot: retail's `shots++` / `hits++`, plus the
    /// ENHANCED accuracy tally in the stats ledger, which leaves out
    /// the possession lob (model 1). Player ruling 2026-09-24: it gets
    /// lobbed wholesale into mana-ball nests and dwellings, so counting
    /// it made Accuracy meaningless. The ledger is hash-silent.
    fn mc1_note_shot(&mut self, owner: u16, model: u8, hit: bool) {
        // The shooter's own seat (every caller gates on a human owner).
        let Some(hx) = self.hx_of_tag(owner) else {
            return;
        };
        self.shots[hx] += 1;
        if hit {
            self.hits[hx] += 1;
        }
        if model != 1 {
            let led = &mut self.stats.0;
            led.offensive_shots += 1;
            led.offensive_hits += hit as u32;
        }
    }

    /// `sub_526C0`'s hit test (:62608-11, CARPET.EXE 0x52740-0x52750):
    /// `pool < struck && aimed > pool && struck.id24 == aimed.id24` —
    /// the struck record shares the aimed record's OWNER id; a null
    /// aim (`+146 == 0`, the pool's record 0) never scores. The
    /// out-of-pool human is never his own bolt's victim. Under
    /// `MGC_NO_MC1_HIT_STAT_AIM_LATCH` the pre-dig shape (aimed slot
    /// against the struck record's slot OR id24) is restored.
    fn mc1_shot_hit(&self, struck: Option<MailTarget>, aimed: u16) -> bool {
        let Some(MailTarget::Pool(j)) = struck else {
            return false;
        };
        if no_mc1_hit_stat_aim_latch() {
            return aimed == self.ent[j].id24 || aimed == j as u16;
        }
        aimed != 0
            && j != 0
            && self
                .ent
                .get(aimed as usize)
                .is_some_and(|a| a.id24 == self.ent[j].id24)
    }

    /// The explode tail shared by the flight handlers: accuracy stats
    /// (sub_526C0 :62585), spawn the +68/+69 effect, despawn. The
    /// generic sub_52770 path (:62759-72) also copies +44 and the
    /// victim; sub_52B30 (fireball) does NOT (:62928-30) — the fire's
    /// own 400 is the fireball's real damage.
    fn proj_explode(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        struck: Option<MailTarget>,
        copy_f44: bool,
        stamp_victim: bool,
    ) {
        let (x, y, z, owner, yaw, pitch, f44, f69) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30, e.f32, e.f44, e.f69)
        };
        // The accuracy stats (`sub_526C0`) sit INSIDE the effect
        // allocation guard in every retail arm — see the `if (result)
        // { sub_526C0(…); …; sub_41E80(a1) }` blocks below the spawn —
        // so they land after `spawn_effect`, and a starved detonation
        // scores nothing (`mc1_shot_stats` below). The pre-dig
        // pre-spawn seat survives under the switch.
        let stats_pre_spawn = self.is_human_tag(owner) && no_mc1_shot_stats_alloc_guard();
        if stats_pre_spawn {
            self.mc1_shot_stats(i, struck);
        }
        // Mana Magnet bolt (m17): the real state-18 handler
        // sub_542B0_54640 (hw:59951-60035, byte-identical at
        // :63841-63925 but unwired past remc1's truncated class-9
        // table) detonates on a ball strike, GROUND CONTACT, or life
        // expiry alike — a miss still drops the pair (both spawns
        // are invisible, so an empty-field miss still LOOKS like a
        // fizzle, but loose mana near the landing spot gets pulled;
        // this supersedes the earlier fizzle-on-miss reading). The
        // detonation is a TWO-SPAWN: the (10,12) possession flash
        // FIRST (hw:59993), then the +68/+69 (10,54) magnet
        // (hw:60013) — both stamped with the bolt's owner/heading.
        // The flash is retail's own wildcard possession flash: its
        // ~8-tick channel-1 AREA claim (sub_25760 → sub_120B0) is
        // gated by the victims' +28 bit-1 susceptibility, which
        // admits balls (3) and graves (2) but not houses (33) — the
        // player's "possesses the struck balls simultaneously with
        // creating the magnet". The pulled remainder outside the
        // flash box claims by MERGING (owned-beats-unowned,
        // sub_277D0 :29717); the ch4 pull itself never claims.
        let magnet_bolt = self.ent[i].class64 == 9 && self.ent[i].model65 == 17;
        if magnet_bolt {
            if let Some(fl) = self.spawn_effect(12, x, y, z) {
                // :63905-11: a flash that seized the bolt makes the
                // three stamps identity stores ([`Gen::mc1_self_seized`]).
                if !self.mc1_self_seized(fl, i) {
                    let e = &mut self.ent[fl];
                    e.id24 = owner;
                    e.f30 = yaw;
                    e.f32 = pitch;
                }
            }
        }
        // ⭐ Every operand below is read AFTER the flash's allocation
        // in retail (`+72` and `+68/+69` are the second ctor call's
        // arguments, :63913), so a flash that seized the bolt hands
        // the payload spawn the FLASH's own fields — a (10,0) at
        // (0,0,0), `+69` being the fresh record's 0.
        let (x, y, z, owner, yaw, pitch, f44, f69) = if no_mc1_self_seize() {
            (x, y, z, owner, yaw, pitch, f44, f69)
        } else {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30, e.f32, e.f44, e.f69)
        };
        // ⭐ RETAIL'S DESPAWN SITS INSIDE THE CHILD-ALLOCATION GUARD.
        // Every class-9 detonation arm has the shape
        // `if ((fx = sub_373F0_377B0(...))) { …score…; sub_41E80(a1); }`
        // — m8 :63193/:63203, generic :62762/:62772, m0 :62925/:62931,
        // m1 :63002/:63008, :63933/:63934 — so a detonation that
        // CANNOT ALLOCATE its effect does not spawn, does not score
        // and DOES NOT DIE: it re-detonates next tick. That it is
        // deliberate rather than an artifact is settled twenty lines
        // away in the same function, where the WATER arm (:63175-81)
        // guards its splash spawn and then kills UNCONDITIONALLY.
        //
        // ⚠ THE GUARD IS ON THE ALLOCATION, NOT ON THE OPTION. Retail's
        // `sub_373F0_377B0` (:43917) also nulls on an UNREGISTERED ctor
        // row, and `spawn_effect`'s `_` arm nulls on a `+69` this port
        // has not modelled — a port gap, not retail's law. Gating on
        // `is_some()` would make any projectile with an unmodelled
        // `+69` IMMORTAL, detonating and scoring every tick forever.
        // `Gen::exhausted` is bumped once per failed `new_event` and
        // nowhere else, so it is the honest witness; sample it AFTER
        // the magnet flash above, or a starved flash would mask a
        // later unmodelled `+69`.
        let starved0 = self.exhausted;
        let child = self.spawn_effect(f69, x, y, z);
        // A detonation that seized its own bolt: the child sits at
        // (0,0,0); `sub_526C0` reads the CHILD's `+65`/`+24` (not a
        // wizard's shot); the `+24/+30/+32/+44` stamps are identity
        // stores; the `+146` victim stamp (`v20[73]`, off the probe
        // register) still lands; and the closing `sub_41E80(a1)`
        // reaps the child. See [`no_mc1_self_seize`].
        let seized = child.is_some_and(|fx| self.mc1_self_seized(fx, i));
        if let Some(fx) = child {
            // `sub_526C0` (:62585-612), first statement of every
            // allocation-guarded detonation block (generic :62762-64,
            // m0 :62925-27, m1 :63002-04, m8 :63193-95 / :63206-08,
            // :63770-72, :63906-08 / :63915-17): a POOL-STARVED
            // detonation neither spawns, dies, nor SCORES — it
            // re-detonates next tick and scores once, when the effect
            // allocates. mc1l49 t=8009-13: the human's (9,3) at slot
            // 914 flies on at life −1…−5 through a dry pool and retail
            // bumps `shots` 445 → 446 only at the t=8013 allocation;
            // the port's pre-spawn bump read 446 four ticks early and
            // stayed one high.
            if self.is_human_tag(owner) && !stats_pre_spawn && !seized {
                self.mc1_shot_stats(i, struck);
            }
            let e = &mut self.ent[fx];
            if !seized {
                e.id24 = owner;
                e.f30 = yaw;
                e.f32 = pitch;
            }
            // The child carries the struck victim's SLOT in +146
            // — sub_52770's explode block ONLY (:58859-64 `v20[73] =
            // victim`, the states-3/17 generic family); the m0/m1
            // explode (:59015/:59092) writes owner/yaw/pitch alone.
            // Provenance only — no effect handler reads it — but it
            // is an observable lane (the mc1hwl0 clouds carry
            // chase=522).
            // The write is a raw pointer-to-index of whatever the
            // probe returned, so the HUMAN CARPET stamps like any
            // other victim — mc1l42's steal flashes all read chase =
            // the carpet slot, never 0. `PLAYER_TARGET` is the port's
            // name for that slot; the projection untranslates it.
            // ...and a MISS stamps too. `v19[73] = (v17 - v21) / 164`
            // (:63428) is an UNGUARDED pointer difference — the very
            // next statement guards `v17` for the shielded-wizard
            // quartering (:63437-47), so the author knew it could be
            // null and left this one bare. A null probe therefore
            // yields `(0 - entBase) / 164` truncated to a word, which
            // in a DOS binary with no ASLR is a LINK-TIME CONSTANT,
            // identical in every retail instance.
            //
            // It is measured from the recording, not derived: mc1l42
            // reads 64608 on all 542 (10,23) miss rows and on the 13
            // (10,11) crater rows. Reproducing it is deliberate — the
            // lane is GRADED (the obs `chase` column), so declining to
            // emit it put a permanent floor under any certified run,
            // which is the one thing a registered deviation may not do
            // (docs/DEVIATIONS.md, ruling 2026-08-17).
            if stamp_victim {
                match struck {
                    Some(MailTarget::Pool(j)) => e.f146 = j as u16,
                    Some(MailTarget::Player(hx)) => e.f146 = self.human_tag[hx].0,
                    // The per-binary caution this arm used to carry is
                    // SETTLED: HIDDEN.EXE links its pool at the same
                    // base and records the same word (mc1hwl0 t=335
                    // slot 589, t=31088 slot 944 — see
                    // [`MC1_MISS_STAMP`]), so the stamp is emitted for
                    // both binaries and the old `Option` hedge is gone.
                    None => e.f146 = MC1_MISS_STAMP,
                }
            }
            if copy_f44 && !seized {
                e.f44 = f44;
            }
        }
        let _ = ctx;
        if child.is_some() || self.exhausted == starved0 {
            self.ent[i].flags |= 0x400;
        }
    }

    /// Class-9 flight dispatch by state (str_25573C :4838).
    pub(crate) fn proj_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // The aimed record's pointer, latched at dispatch entry the way
        // every retail flight handler's first statement does
        // (`v2 = pool + 164 * +146`, :62952-54) — BEFORE the first-tick
        // acquisition rewrites `+146`. See [`Gen::mc1_aim_latch`].
        self.mc1_aim_latch = crate::engine::features::HashSilent(self.ent[i].f146);
        match self.ent[i].tick70 {
            0 => self.proj_m0_tick(i, ctx),
            1 => self.proj_m1_tick(i, ctx),
            // Global Death's m18 carrier (state 19): the one-tick
            // relay sub_54480_54810 — mint the (10,55) field, reap
            // self. Takes no ctx; it never moves.
            19 => self.death_relay_tick(i),
            3 => self.proj_generic_tick(i, ctx, true),
            8 => self.proj_m8_tick(i, ctx),
            9 => self.proj_m9_tick(i, ctx),
            10 => self.proj_castle_ball_tick(i, ctx),
            12 => self.proj_m12_tick(i, ctx),
            13 => self.proj_bolt_tick(i, ctx),
            // The Troll/Ape boulder — CARPET.EXE's relocated class-9
            // table binds state 0xF to the sub_52770 thunk 0x53060,
            // the BARE generic flight (the fire-trail wrapper is
            // state 3's own thunk sub_53070 :63021 — the boulder
            // drops no trail). Silent in flight (the arrow roll is
            // state 13's alone); it speaks through its (10,0) impact
            // (sub_3A490 :46454, sound 3 :28114), which inherits the
            // thrown +44 = 780 (:22112). The throw ctor sub_1AE30
            // pre-targets it with the thrower's own +146 (:22122-23)
            // — a thrown boulder HOMES like any generic bolt.
            15 => self.proj_generic_tick(i, ctx, false),
            17 => self.proj_firewall_tick(i, ctx),
            // Player-spell payload projectiles (spell track). The
            // m17 magnet bolt is NOT here — it rides possession's
            // state-1 flight (see spawn_spell_lob).
            2 | 4 | 5 | 7 | 11 => self.proj_payload_tick(i, ctx),
            // Beam segment (state 14; remc1's table is truncated here
            // — lifecycle reconstructed from the slot-order life trick
            // :63349-53): kill on the PRE-decrement value so every
            // segment renders exactly one frame regardless of whether
            // its slot ticks before or after the beam's.
            // Decrement THEN test (the l32 corpus: every dying
            // segment reads act_life −2, never −1 — the post-
            // decrement kill), below −1. Death frames are identical
            // to the pre-decrement form; only the residual value in
            // the record differs, and the recording pins it.
            14 => {
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life < -1 {
                    self.ent[i].flags |= 0x400;
                }
                false
            }
            // States 6/16 stay INERT, not killed: remc1's table
            // carries row 6 (sub_53060, unported) and the truncated
            // listing leaves 16 unresolved — no corpus witness
            // either way, so their no-op stands.
            6 | 16 => false,
            // State 18 IS the m17 magnet bolt's flight (HW listing
            // hw:60386-405 carries the case-0x11 arm remc1's dropped;
            // docs/DEVIATIONS.md names the state-18 flight): the m1
            // homing skeleton whose acquire walks ONLY the ball
            // chain, no owner exclusion, no danger call, cone
            // 0x71/0x71 — aim_assist_possess_mc1's magnet arm.
            // mc1l0-sg t=2723-2733 slot 46: chase/f26/flags/life all
            // diverged while this arm sat inert.
            18 => self.proj_m1_tick(i, ctx),
            // ⭐ THE WALKER SOFT-KILLS A STATE WITH NO TABLE ROW. The
            // main walk direct-indexes `table[class][state]` and
            // requires the row's own state word to match (`data4 ==
            // +70`); a miss is the "STATE-ID does not match" arm —
            // `sub_41E80` (:212A70 dispatch, the soft-kill bit), NO
            // handler, and NO `+63` tick, because retail's phase
            // increment (:52406) sits INSIDE the dispatched branch.
            //
            // ⭐⭐⭐ `sub_41E80_421C0` (:52508-11) IS RETAIL'S SOLE
            // WRITER OF THE REAP BIT, settled in the shipped bytes:
            // CARPET.EXE 0x5A678 and HIDDEN.EXE 0x5ABB8 are the same
            // five-instruction leaf, `8b 44 24 04 / 80 48 11 04 /
            // c3` — `mov eax,[esp+4]; or byte [eax+0x11],4; ret`,
            // i.e. `+17 |= 4` = our `flags |= 0x400`. The opcode
            // sequence `80 48 11 04` occurs EXACTLY ONCE in each
            // binary, with 99 direct `call rel32` sites (the
            // decompile lists 106 calls across 84 functions), and the
            // bit is NEVER cleared — the only `byte[1]` clears in
            // either listing are `&= 0xFCu` (bits 8/9). ⚠ a grep for
            // `*(_BYTE *)(x + 17)` will NOT find it: the decompile
            // spells the write `var_29811_16.byte[1] |= 4u`.
            // The port's shared walk increments +63 after this
            // returns, so the un-dispatched tick compensates here to
            // keep the lane at retail's frozen value.
            //
            // Reachable through the death handoff's blind `+52` stamp
            // (sub_1A6C0 :21702, [`Gen::mob_death`]'s war story): the
            // dying packmate writes chase = base+2 into whatever
            // occupies the stale slot. mc1hwl0 t=18600: griffon 162
            // dies still pointing at slot 109 — long since re-minted
            // as the castle guard's (9,13) arrow — and stamps it
            // f146 = 17, +70 = 50. Retail's next walk finds no
            // class-9 row 50, reap-flags the arrow un-ticked
            // (t=18601: flags 8198 → 9222, f63 parked at 118) and
            // the tick-top reap frees it at 18602; the port's old
            // no-op arm ticked f63 forever and the arrow never died.
            _ => {
                self.ent[i].flags |= 0x400;
                self.ent[i].f63 = self.ent[i].f63.wrapping_sub(1);
                false
            }
        }
    }

    /// sub_52B30 (:62779): the fireball. Returns terrain_dirty.
    fn proj_m0_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // ⭐ ONE-SHOT ACQUISITION, exactly as in `sub_52770`'s
        // prologue — `sub_52B30` carries the SAME latch (:62811-15):
        // untargeted and `(+16 & 2) == 0` → set the bit, scan once
        // (model 0 is an acquire case), and on a HIT turn yaw by AT
        // MOST 34 toward the pick with pitch taken outright
        // (`+32 = +36`, :62817-24); on a MISS mirror the live heading
        // into the aim fields and never scan again.
        //
        // The port re-ran the scan EVERY untargeted tick and applied
        // the 34-step every tick with it, so a fireball launched wide
        // kept hunting for its whole life and bent onto anything that
        // wandered into the ±0x71 cone. Retail commits at the muzzle:
        // miss the cone at launch and the shot flies straight. This is
        // the same law as the meteor's, and the fireball is where it
        // is felt — it is the most-cast spell in the game.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                self.aim_assist(i, ctx);
                if self.ent[i].f146 != 0 {
                    let t = Self::turn_step(self.ent[i].f30, self.ent[i].f34, 34);
                    // The 34-step is stored RAW (:62824 — no mask): a
                    // step past 0/2048 parks an out-of-range u16 in
                    // +30 (corpus t=2739: 65512 = −24). Every consumer
                    // masks on read; the next homing write
                    // canonicalizes.
                    self.ent[i].f30 = (self.ent[i].f30 as i32 + t as i32) as u16;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            self.home(i, ctx);
        }
        self.proj_move_and_hit(i, ctx, false, false, DeflectLaw::Fireball)
    }

    /// sub_52ED0 (:62937): the POSSESS lob (c9 m1). Its flight z is
    /// clamped UP to the terrain each tick (:62975-77 — the lob skims
    /// rising ground), its acquisition scans ONLY mana balls and
    /// houses (sub_54520 case 1, :64040-77 — never creatures or
    /// wizards), and its victim scan is the dedicated sub_11AC0
    /// (:17033): class-10 models 39/40/45 only, skipping entities the
    /// shooter already owns or claimed. Any end detonates into the
    /// (10,12) ch1-claim flash.
    fn proj_m1_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // ONE acquisition roll on the first untargeted tick — the
        // +16&2 latch (:62952-60), same idiom as the HW dart and the
        // castle ball. A lob that finds nothing flies straight and
        // never re-acquires.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                self.aim_assist_possess(i);
            }
        } else {
            // The tracked arm is the SHARED homer `sub_52550`
            // (:62971), row-capped on BOTH axes — the lob's ctor row
            // [2] turns 113/113 a tick (:45908). An earlier port
            // homer hardcoded a 34 yaw cap and snapped pitch outright
            // — the (9,1) ±79 heading staircase.
            self.home(i, ctx);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let g = self.ground_z(tmp.0, tmp.1) as i16;
        if tmp.2 < g {
            tmp.2 = g; // ground clamp (:62975-77)
        }
        let hit = self.possess_victim_at(i, tmp);
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        // RECONSTRUCTION BRIDGE (m17 only): the magnet bolt claims a
        // dwelling it PASSES THROUGH in flight (player retail-
        // verified — the pass-through and the exact-flag-hit claims
        // are one mechanism). The decompiled chain has NO path that
        // can claim a (10,45) at all: the (10,12) flash writes ch1
        // and retail dwellings listen on ch0 only (+28 = 33), and an
        // exhaustive sweep proved every flight call pure (sub_11C00 /
        // sub_11AC0 / steer / move / the flash "mover" = a frame
        // counter) — the write was reconstructed away. Bridge: an
        // in-flight direct ch1 touch on overlapped dwellings, gated
        // like the possess scan (:17067 — not own by +24 or +144);
        // the port's built houses carry the ch1 intake.
        if self.ent[i].model65 == 17 {
            let own = self.ent[i].id24;
            let (bx, by) = (self.ent[i].x, self.ent[i].y);
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let tx = ((bx >> 8) as i32 + dx) as u8;
                    let ty = ((by >> 8) as i32 + dy) as u8;
                    let mut j = self.map_entity[tile(tx, ty)] as usize;
                    while j != 0 {
                        let c = &self.ent[j];
                        let next = c.next20 as usize;
                        if c.class64 == 10
                            && c.model65 == 45
                            && c.flags & 8 != 0
                            && c.id24 != own
                            && c.f144 != own
                            && self.ent_overlap(i, j)
                        {
                            self.mail_write(MailTarget::Pool(j), 1, 0, own);
                        }
                        j = next;
                    }
                }
            }
        }
        if let Some(j) = hit {
            // The HIT tick detonates before the life decrement (the
            // l0 impact record keeps life 5, corpus t=69), parking
            // the lob AT the victim's AIM point — x/y and the z+f78
            // bracket (the tent lands the record at 896 − 8192 =
            // −7296).
            let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
            self.move_relink(i, jx, jy, jz);
            self.proj_explode(i, ctx, Some(MailTarget::Pool(j)), false, false);
            return false;
        }
        self.ent[i].act_life -= 1;
        if self.ent[i].act_life < 0 {
            self.proj_explode(i, ctx, None, false, false);
        }
        false
    }

    /// The possess-acquire subtype's TargetingVerb seam (see
    /// [`Self::aim_assist`]).
    fn aim_assist_possess(&mut self, i: usize) {
        match self.verbs.targeting {
            TargetingVerb::Mc1 | TargetingVerb::Mc1Hw => self.aim_assist_possess_mc1(i),
            TargetingVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Targeting);
                self.aim_assist_possess_mc1(i);
            }
        }
    }

    /// sub_54520 case 1 (:64040-77): possess acquisition — the awake
    /// (+58 != 0) mana balls (m39/40) and houses (m45) not already
    /// CLAIMED by the shooter (+144 only — the creator +24 half of the
    /// gate is impact-only, :17067), inside the ±0x71 yaw+pitch cone
    /// within 2-D distance 5120 (sub_423D0 has no z term). Best by
    /// sub_54A90's score (:64212-17): the distance decomposed onto the
    /// angular-error axes — 16.16 cos terms >>16, sin terms >>14
    /// through an i16 truncation (~4x misalignment weight) — compared
    /// UNSIGNED (the -1 reject sentinel = u32::MAX). Snaps the heading
    /// on success.
    fn aim_assist_possess_mc1(&mut self, i: usize) {
        // sub_54520's entry clamp (:63975-76) — shared by every
        // acquire case; see `aim_assist_mc1_cone2`. This is what
        // turns the possess lob's ctor +26 = 200 into the corpus 16.
        if self.ent[i].f26 > 16 {
            self.ent[i].f26 = 16;
        }
        let (px, py, pz, yaw, pitch, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f32, e.id24)
        };
        // The Mana Magnet bolt (m17) HOMES — sub_54520 case 0x11
        // (hw:60386-60405; remc1's reconstructed switch is TRUNCATED
        // past case 9, which read as "no case 17 → straight flight"
        // until the player's retail playtest refuted it): the
        // ball roster only — m39 balls AND m40 graves, never the
        // dwellings list (mc1l27 t=37367) — awake-gated
        // (+58) and NOTHING else — no team gate, no claim gate, so
        // caster-claimed balls are homing targets too
        // (player retail-verified). Same 0x71/0x71 cone + 5120 range
        // score (sub_54A90) as possession's case 1; possession keeps
        // its +144-vs-+24 skip (hw:60169-60207) and its second
        // graves/dwellings list.
        let magnet = self.ent[i].model65 == 17;
        // The candidate roster is the TICK-HEAD ball/grave chain
        // (:64043/:64054 walk `var_u32_36462[1]` — the very list the
        // magnet stamp reads), severed at any mid-tick record reuse
        // ([`TickChain`]), then the m45 dwelling list (:64058-71) —
        // NOT the live pool. Retail's list gates are +144/+58 ALONE
        // (no class, life, or reap-mark test), so a chain member that
        // died mid-tick stays a stale-byte candidate, and a ball
        // spawned mid-tick is invisible until the next rebuild.
        // Measured: mc1l0 pair 604→605 (chase 104-vs-714 — the lob
        // reusing ball 642's record sees only the chain prefix; its
        // sibling in old projectile slot 61 three ticks earlier saw
        // the intact chain and faithfully chased 714). The MC2
        // fallback keeps the live-pool walk — its list law is
        // unmeasured and its reap timing differs (strict-scoped).
        let mc2_fallback = self.verbs.targeting == TargetingVerb::Mc2;
        let mut best: Option<(u16, u32, u16, u16)> = None;
        let consider = |c: &Ent, j: usize, best: &mut Option<(u16, u32, u16, u16)>| {
            let (tx, ty, tz) = (c.x, c.y, c.aim_z());
            let ty_yaw = Self::angle_between(px, py, tx, ty);
            let dy = Self::angdist(yaw, ty_yaw) as usize;
            if dy > 0x71 {
                return;
            }
            let dist = Self::isqrt(Self::dist2_sq(px, py, tx, ty) as u32) as i32;
            // sub_54A90 brackets the CANDIDATE in place (:64203-09),
            // so a self-candidate lifts the scorer's own z too —
            // bearing (0, 0), distance 0, score 0, an outright win.
            // The possess walk alone is exposed to the alias: it
            // gates on +144 (:64045) where the class-3 and creature
            // sweeps skip the shooter via `c.id24 == own`.
            let fz = if j == i { c.aim_z() } else { pz };
            let ty_pitch = Self::pitch_toward(fz, tz, dist);
            let dp = Self::angdist(pitch, ty_pitch) as usize;
            if dp > 0x71 || dist > 5120 {
                return;
            }
            let score = Self::acquire_score(dist, dy, dp);
            if best.is_none() || best.is_some_and(|(_, bs, _, _)| score < bs) {
                *best = Some((j as u16, score, ty_yaw, ty_pitch));
            }
        };
        if mc2_fallback {
            for j in 1..self.ent.len() {
                let c = &self.ent[j];
                if c.class64 != 10 || c.flags & 0x400 != 0 {
                    continue;
                }
                let candidate = match c.model65 {
                    39 => c.f58 != 0 && (magnet || c.f144 != own),
                    40 | 45 => !magnet && c.f144 != own && c.f58 != 0,
                    _ => false,
                };
                if candidate {
                    consider(c, j, &mut best);
                }
            }
        } else {
            for k in 0..self.ball_chain.visible_len() {
                let j = self.ball_chain.list[k] as usize;
                let c = &self.ent[j];
                // The magnet takes the WHOLE ball roster — m39 balls
                // AND m40 graves — behind the +58 gate alone
                // (hw:60386-60405, CARPET.EXE 0x548FF; see
                // [`no_mc1_magnet_homes_on_graves`]) and skips the claim
                // gate; possess takes the whole chain behind the shared
                // +144/+58 pair (:64045-49).
                if magnet && c.model65 != 39 && no_mc1_magnet_homes_on_graves() {
                    continue;
                }
                if c.f58 == 0 || (!magnet && c.f144 == own) {
                    continue;
                }
                consider(c, j, &mut best);
            }
            if !magnet {
                // The dwelling list (:64058-71) — chain semantics
                // unmodeled (m45 records are never reused mid-tick
                // in the corpus); the live walk with the port's
                // conservative liveness gates stands in.
                for j in 1..self.ent.len() {
                    let c = &self.ent[j];
                    if c.class64 == 10
                        && c.model65 == 45
                        && c.flags & 0x400 == 0
                        && c.f144 != own
                        && c.f58 != 0
                    {
                        consider(c, j, &mut best);
                    }
                }
            }
        }
        if let Some((slot, _, ty_yaw, ty_pitch)) = best {
            let e = &mut self.ent[i];
            e.f146 = slot;
            e.f30 = ty_yaw;
            e.f32 = ty_pitch;
            e.f34 = ty_yaw;
            e.f36 = ty_pitch;
        }
    }

    /// sub_11AC0 (:17033): the possess victim scan — class-10 models
    /// 39/40/45 only, not the shooter's own or already-claimed
    /// (:17067 gates on BOTH +24 and +144), AABB. The Mana Magnet
    /// bolt (m17) instead uses retail's balls-only sibling sub_11C00
    /// (:17109-12, called from the state-18 handler hw:59994): model
    /// 39 + collidable + overlap and NOTHING else — no owner, team,
    /// or claim filter. Crucially the bolt therefore strikes balls
    /// the caster ALREADY CLAIMED — the spell's core economy: strike
    /// your claimed ball, the pulled wild remainder merges into it
    /// and adopts the owner. (An earlier port gate excluded
    /// own-claimed balls — the bolt flew through your pile and
    /// grounded beyond it.)
    fn possess_victim_at(&mut self, i: usize, tmp: (u16, u16, i16)) -> Option<usize> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let own = self.ent[i].id24;
        let balls_only = self.ent[i].model65 == 17;
        let mut found = None;
        // sub_11AC0's geometry exactly: the scan center is the
        // NEAREST tile (`(+72 + 128) >> 8`, :17046-47) and the
        // neighborhood is the SEARCH.DAT ring iterator (sub_11410
        // rings 0..=(f80+255)>>8) — the retail rings are 2x2-anchored
        // shells (ring 1 spans dx,dy −1..2), which is how the l0 tent
        // two tiles up-range still meets the lob's radius-1 scan
        // (the t=69/t=78 impacts; big-extent victims overlap from
        // well outside a square window).
        let r = (self.ent[i].f80 as i32 + 255) >> 8;
        let cells = self.ring_cells(0, r);
        let cx = ((tmp.0 as i32 + 128) >> 8) as u8;
        let cy = ((tmp.1 as i32 + 128) >> 8) as u8;
        for (dx, dy) in cells {
            let tx = cx.wrapping_add(dx);
            let ty = cy.wrapping_add(dy);
            let mut j = self.map_entity[tile(tx, ty)] as usize;
            while j != 0 {
                let c = &self.ent[j];
                if c.flags & 8 != 0
                    && c.class64 == 10
                    && (c.model65 == 39 || (!balls_only && matches!(c.model65, 40 | 45)))
                    && (balls_only || (c.id24 != own && c.f144 != own))
                    && self.ent_overlap(i, j)
                {
                    found = Some(j);
                    break;
                }
                j = c.next20 as usize;
            }
            if found.is_some() {
                break;
            }
        }
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        found
    }

    /// sub_53DC0 (:63628): the storm-carrier flight (c9 m12) — the
    /// Lightning Storm's projectile. Speed eases ±2, homes on an
    /// acquired class-3 target (none exist for us yet → straight
    /// flight); on ANY end but water it becomes the (10,38) storm
    /// cloud, passing owner/heading/victim/damage and the (9,9)
    /// bolt spec down (:63767-83).
    fn proj_m12_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :63653-76 — the TARGET TEST OPENS THE HANDLER, above the
        // speed servo, and the untargeted arm is sub_52770's one-shot
        // acquire prologue verbatim: latch flags bit 1, call
        // sub_54520 ONCE, snap +30/+32 from the acquired +34/+36 on a
        // win and mirror the live heading back on a miss.
        //
        // ⚠ THE STORM CARRIER IS A REAL ACQUIRER, NOT A `default:`.
        // `sub_54520_548B0`'s switch on +65 lists model 12 EXPLICITLY:
        // the case labels are :63979 (0/3/4), :64040 (1), :64078-81
        // (7, 8, 0xB, **0xC**), :64125 (9), :64185 (default) — and
        // 0xC is 12, inside the significant-list block. So the carrier
        // walks the class-3 list, scores each candidate through
        // `sub_54A90_54FC0(a1, cand, 0x71, 0x71)` (:64104/:64110),
        // stamps the winner into +146 and snaps (:64092-93). It is the
        // same block models 7/8/0xB ride, which is exactly what
        // [`Self::aim_assist_wizards_mc1`] already implements, and
        // sub_54520's entry clamp on +26 (:63975-76, above the switch)
        // rides inside it. mc1l42 t=27215: retail's carrier reads
        // flags 6 and +26 = 16 off a 101-tick charge, ours 4 and 0.
        if self.ent[i].f146 != 0 {
            self.home(i, ctx);
        } else if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.aim_assist_wizards(i, ctx);
            if self.ent[i].f146 != 0 {
                self.ent[i].f30 = self.ent[i].f34;
                self.ent[i].f32 = self.ent[i].f36;
            } else {
                self.ent[i].f34 = self.ent[i].f30;
                self.ent[i].f36 = self.ent[i].f32;
            }
        }
        let e = &mut self.ent[i];
        e.f126 += flight_speed_step(e.f128 - e.f126);
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        // :63683-85 — the stepped point is COMMITTED AND RELINKED
        // before the victim probe (`sub_41C70_41FB0` then
        // `sub_11980(a1)` off the entity's own +72), and it is
        // committed RAW: the ground read at :63689 lands in the
        // SCRATCH's z, never the entity's, so a carrier that steps
        // into a hill keeps the buried z. mc1l42 t=27217: retail's
        // carrier ends at 3775 with the ground at 3871, and the
        // (10,38) cloud it raises inherits that buried z (3839, not
        // 3935). Same law as the castle ball / crater grounding.
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        let hit = self.victim_scan(i, ctx);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let grounded = ground > tmp.2;
        // :63692-98 — the life countdown lives INSIDE the airborne
        // arm, so a touchdown never reaches it and the carrier that
        // blooms on contact is recorded one tick "younger" (mc1l42
        // t=27217 life 3 not 2; t=27236 life 5 not 4 — the second
        // storm grounds on its very first tick and never spends one).
        if hit.is_none() {
            if !grounded {
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life >= 0 {
                    return false;
                }
            } else if self.ent[i].model65 != 4 && self.on_water_pub(tmp.0, tmp.1) {
                self.splash_and_die(i); // stormless water end (:63699-709)
                return false;
            }
        }
        // :63759-61 — a struck carrier parks on the victim's aim
        // point (the +76/+78 sub_524C0 bracket) before it blooms, so
        // the cloud is laid on the victim, not at the step endpoint.
        match hit {
            Some(MailTarget::Pool(j)) => {
                let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                self.move_relink(i, jx, jy, jz);
            }
            Some(MailTarget::Player(hx)) => {
                let (px, py, pz) = self.seat_at(ctx, hx);
                self.move_relink(i, px, py, pz.wrapping_add(PLAYER_HH as i16));
            }
            None => {}
        }
        let (x, y, z, own, f44, f30, f32) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f44, e.f30, e.f32)
        };
        // The cloud's +146 is written by the SAME unguarded pointer
        // difference as the explode children's (:63778 `*(v19 + 146)
        // = (v6 - base) / 164`, the :63428 twin): `v6` is the probe
        // result, and the probe returning NULL is never guarded, so a
        // MISS records `(0 - entBase) / 164` truncated to a word —
        // the link-time constant [`MC1_MISS_STAMP`], not 0. This is
        // the citation the site was missing: mc1l42's storm clouds
        // read chase = 64608 on every miss (t=27217 slot 99). ⚠ the
        // constant is PER BINARY; HIDDEN.EXE links its pool
        // elsewhere and has no corpus witness, so HW keeps
        // NewEvent's 0.
        if let Some(s) = self.spawn_effect(38, x, y, z) {
            // :63767-83 — `+24/+30/+32/+44/+68/+69` are copied off the
            // carrier after the allocation: identity stores when the
            // cloud seized the carrier itself ([`no_mc1_self_seize`]),
            // which leaves the fresh record's `+68/+69` = 10/0. The
            // `+146` probe stamp still lands; the reap below hits the
            // cloud.
            let seized = self.mc1_self_seized(s, i);
            let e = &mut self.ent[s];
            if !seized {
                e.id24 = own;
                e.f30 = f30;
                e.f32 = f32;
                e.f44 = f44;
                e.f68 = 9;
                e.f69 = 9;
            }
            e.f146 = match hit {
                Some(MailTarget::Pool(j)) => j as u16,
                Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                None => MC1_MISS_STAMP,
            };
            // ⭐⭐ A REFUSED BLOOM LEAVES THE CARRIER ALIVE. The
            // self-kill is INSIDE the cloud's null guard: HIDDEN
            // 0x54441 `test %eax,%eax` / 0x54443 `je 0x544ba` jumps
            // straight to the epilogue, past the kill at 0x544b2
            // (`call 0x421C0` — a two-instruction leaf that is just
            // `orb $0x4,0x11(%eax)`, i.e. flags |= 0x400). So when
            // `new_event` refuses — free stack AND recycle stack both
            // dry — the carrier is NOT marked dead: it stays alive
            // and re-tries its bloom on the following ticks.
            //
            // ⚠ THE FORK IS DELIBERATE, SO DO NOT GENERALISE IT.
            // Twenty bytes earlier in this very function the WATER
            // arm spawns its splash and kills UNCONDITIONALLY: its
            // `je 0x54407` skips only the +24 copy and the kill at
            // 0x54408 stands outside (the port's `splash_and_die`
            // already matches that shape). Every other
            // `flags |= 0x400` beside a spawn needs its own check
            // against the binary — retail is not consistent here.
            //
            // CARPET.EXE has the same two shapes. mc1hwl0 t=37110:
            // the pool goes 5 → 0 on the tick, retail's slot 983
            // keeps flags 6 and its `act_life`/`f126`/pose steps
            // match the port exactly on every other lane.
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// sub_39F40 (:46166): the castle ball (c9 m10) — sprite 18,
    /// speed 384, life 0x2000/384 = 21, row [1] (:46185 — the same
    /// 22/22-capped row the payload lobs ride; the recorded upgrade
    /// ball's model_ptr resolves to it).
    pub(crate) fn spawn_castle_ball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(10, 10, x, y, z, 384, 21, 1, 18)
    }

    /// sub_3A040 (:46226): the storm carrier (c9 m12) — sprite 216,
    /// speed 384, life 2048/384 = 5, **row [1]** (`+156 =
    /// &unk_98F38[1]` :46246; HW's twin sub_3A3C0 writes the same
    /// address literally, `&unk_98F58` = base + 32, hw:42366). The
    /// port handed it row 0 — inert until the homing arm started
    /// reading `BEHAVIOR[row156]` for its turn caps, and then worth
    /// exactly the difference between row 0's caps and row 1's 22/22:
    /// mc1hwl0 t=21997→21998, retail's carrier eases `+30` 1216 →
    /// 1194 (a 22-capped swing at a `+34` of 1191) where the port
    /// snapped the whole 25 to 1191 and carried 3 units of position
    /// with it.
    ///
    /// ⭐ ONLY THE FREE RUN CAN SEE A WRONG NATIVE ROW — the importer
    /// decodes `row156` from the recorded `+156` pointer, so pair
    /// mode reads retail's row and the tick comes out clean. Same
    /// blind spot the HW firewall-bolt row fork (m16) was found
    /// through.
    pub(crate) fn spawn_storm_carrier(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        self.spawn_projectile(12, 12, x, y, z, 384, 5, 1, 216)
    }

    /// sub_3A270 (:46330): the Wall of Fire bolt (c9 m16, state 17)
    /// — fireball sprite 42, speed 384, life 21. HW swaps the ctor
    /// for sub_3A5F0 (hw:42451), byte-identical except sprite 76
    /// (hw:42474) — the big meteor bitmap. The sprite literal also
    /// sizes the hitbox: SPRITE_STATS row 76 is 420x350 vs 42's
    /// 88x100, so the swap is look AND collision.
    pub(crate) fn spawn_firewall_bolt(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let hw = self.is_hidden_worlds();
        let sprite = if hw { 76 } else { 42 };
        // Behavior ROW 5 (sub_3A270 :46349 `+156 = &unk_98F38[5]`) —
        // its v_2 = 5 is the homing tail's whole turn authority
        // (mc1l5 t=23389: retail eases 664 → 669 where row 0's 56
        // swung the port to 720).
        //
        // ⭐ HW'S BOLT IS A DIFFERENT ROW: `sub_3A5F0` (hw:42461) is
        // the same ctor with `+156 = &unk_98F78` — 0x98F78 against
        // the array base 0x98F38 is 0x40, i.e. **row 2** — and
        // sprite 76 instead of 42 (the sprite was already forked
        // here; the row was not). Row 5's 5-unit yaw cap held HW's
        // bolt back on the one tick its homing needed a real swing:
        // mc1hwl0 t=2750→2751, retail turns `+30` 167 → 178 straight
        // onto `+34` where the port stopped at 172, and the whole
        // `(10,53)` detonation inherited the bad heading.
        // ⭐ THE IMPORTER MEASURES THIS — `row156` is decoded from the
        // recorded `+156` pointer (`(ptr − behavior_base) / 32`), so
        // a wrong native row is invisible to PAIR mode and only the
        // free run can see it.
        let row = if hw { 2 } else { 5 };
        self.spawn_projectile(16, 17, x, y, z, 384, 21, row, sprite)
    }

    /// The m16 firewall flight (state 17): generic ease + move, plus
    /// — ON HW ONLY — the FIRE TRAIL that gives the spell its name.
    ///
    /// ⭐⭐ HW's state-17 handler is `sub_54600` (hw:59929), which is
    /// `sub_52770_52AB0` **wrapped**: run the generic flight, and if
    /// the bolt survived it (`if (+64)` — a detonating bolt frees its
    /// own record) drop a `(10,0)` FIRE at the bolt's post-move
    /// position, stamped `+16 |= 0x80`, `+18 |= 1` and `+24 = the
    /// bolt's owner`. Base MC1 has no such wrapper anywhere in its
    /// listing — its state 17 is the bare `sub_52770` — so the wall
    /// that actually lays a wall of fire is the HW one. The port's
    /// old "no fire trail" reading was right for base MC1 and wrong
    /// for the game this corpus records: mc1hwl0 t=2746, the human's
    /// bolt is born at slot 822 and retail's very next allocation is
    /// the `(10,0)` at 788, on the bolt's own first flight tick, at
    /// the bolt's exact x/y/z with `id24` = the caster.
    /// ⚠ `sub_54600` is UNREFERENCED in the HW listing — the class-9
    /// state table is data, not source. The corpus is what binds it
    /// to state 17.
    ///
    /// The state-17 handler sub_52770_52AB0 copies the
    /// bolt's +44 into the +68/+69 explosion (:62770, hw:58859) —
    /// BOTH games. remc1's truncated class-9 state table hid the
    /// base-MC1 copy for a while (the question sat banked); the
    /// mc1l5 take settled it: victims under the recorded wall lose
    /// EXACTLY 191/tick = 24464/128 = the copied spell damage over
    /// the cloud's maxLife — the cloud is the wall's ONLY damage
    /// source (its 225 flames are stamped decorative, see
    /// `napalm_tick`). HW keeps its ROW damage (5000 over 6 ticks ≈
    /// 833/tick, the "3 guaranteed hits" law; only the HW model-53
    /// rebound reflect defends, hw:58806).
    fn proj_firewall_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let e = &mut self.ent[i];
        e.f126 += flight_speed_step(e.f128 - e.f126);
        // The m16 child runs the acquire cone in BOTH variants, but
        // the YAW cone forks: base MC1's jump table (CARPET.EXE
        // 0x544CC[0x10] = 0x54682, read off the shipped LE binary —
        // the remc1 listing's switch recovery dropped the alias)
        // routes case 16 onto the SHARED case-0/3/4 arm at yaw 0x71;
        // HW forks a dedicated arm (HIDDEN.EXE 0x5485C[0x10] =
        // 0x54BB0, `mov $0x100,%edi` — remc1hw :60322) that widens
        // yaw to 0x100. Pitch is 0x71 in both. MC2's independent
        // decompile carries the same split (EF:54934), and the
        // corpus brackets it: mc1l5 t=23382's pick at dy 44 (must
        // acquire) vs mc1l49 t=1099/1140 at dy 186/125 (retail
        // MISSED both) pin base MC1 inside [44,124] ∋ 0x71. The
        // list SHAPE is the shared arm's exactly (significant list
        // + 20 creature buckets under the owner-row v_28 pre-gate).
        //
        // Acquisition is ONE-SHOT, latched on flags bit 2 even on a
        // miss (remc1hw :58731-49): a miss flies straight forever, a
        // hit SNAPS the live heading to the pick (f30/f32 = f34/f36,
        // :58742-43). Only the post-lock tracker eases (sub_52550,
        // :58754 = home()). Same idiom as the m9 beam (proj_m9_tick).
        //
        // ⚠ THE LATCH ITSELF IS SHARED — the `sub_52770` prologue
        // (:62640-60, and see [`Gen::proj_generic_tick`], the other
        // half of the same retail function).
        // The acquire and the tracker are the TWO ARMS OF ONE
        // IF/ELSE on the PRE-acquire +146 (sub_52770 prologue
        // :62644-62): the tick that ACQUIRES snaps and stops there —
        // home() is the ELSE arm only. Running both in the acquire
        // tick masked the snap's raw pitch through the tracker's
        // & 0x7FF store (mc1l49 t=983 slot 854: retail keeps the
        // acquire's raw 2048; home()'s zero-step store wrapped it
        // to 0). Same shape as `Gen::proj_generic_tick`.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                // The sub_54520 head clamp (:63945-46) — the banked
                // charge rides +26 only until the first acquire tick.
                if self.ent[i].f26 > 16 {
                    self.ent[i].f26 = 16;
                }
                let yaw_cone = if self.is_hidden_worlds() { 0x100 } else { 0x71 };
                self.aim_assist_mc1_cone(i, ctx, yaw_cone, 0x71);
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            self.home(i, ctx);
        }
        let hit = self.proj_move_and_hit(i, ctx, true, true, DeflectLaw::Generic);
        // hw:59934-45 — the trail, gated on the bolt still being a
        // live record after the flight (retail reads `+64`, the class
        // byte its own free clears).
        if self.is_hidden_worlds() && self.ent[i].class64 != 0 {
            let (own, x, y, z) = {
                let e = &self.ent[i];
                (e.id24, e.x, e.y, e.z)
            };
            if let Some(f) = self.spawn_effect(0, x, y, z) {
                // ⭐ THE TRAIL CAN SEIZE ITS OWN BOLT — the HW site of
                // [`no_mc1_self_seize`], the one the class census
                // listed as exposed. `sub_54600` hands the ctor a
                // POINTER to the bolt's `+72` and reads `+24` after it
                // returns (`HIDDEN.EXE` file 0x6D011 `8d 43 48` lea
                // 0x48(%ebx) / 0x6D015 call / 0x6D028 `66 8b 5b 18`
                // mov 0x18(%ebx),%bx), so on a dry free stack the fire
                // is born at (0,0,0) wearing its own slot stamp.
                // WITNESS mc1hwl5 t=6188: the human's bolt at slot 912
                // (free 0, recycle 102 → 74) becomes retail's `(10,0)`
                // at x/y/z 0 with `id24` 912; the port's hoisted
                // copies posed it at the bolt, owned by the human
                // (341). Found through the captured entity index
                // (round 166): retail's chain for cell (0,0) held 912.
                let seized = self.mc1_self_seized(f, i);
                let e = &mut self.ent[f];
                e.flags |= 0x80; // +16 |= 0x80
                e.flags |= 0x1_0000; // +18 |= 1
                if !seized {
                    e.id24 = own;
                }
            }
        }
        hit
    }

    /// sub_53980/sub_53B50 (:63453/:63525): the castle ball's flight
    /// — steered at the +150 ground target (dest_x/dest_y). The
    /// LAUNCH tick latches, runs the placement scan at the spawn spot
    /// (the hand muzzle under the `castle_latch_bug` retail arm) and
    /// RETURNS — no move (:63612-21; the recorded ball sits latched
    /// and unmoved at its first boundary). A launch failure is a
    /// silent despawn.
    ///
    /// The landing law is the `castle_latch_bug` patch fork
    /// (mc1l32-castle-bug.mgcr; MC1/HW only — MC2 keeps the pre-arm
    /// behavior under both arms, its EF lineage unverified):
    /// - RETAIL arm (:63588-90, the short-circuit `ground > z ||
    ///   life < 0 || !scan`): a terrain touchdown or expiry builds
    ///   the castle at the contact point UNSCANNED; the scan re-runs
    ///   only on airborne ticks, where a failure stops the ball —
    ///   flip 180°, one step back with the live pitch (:63601-04) —
    ///   and still builds. Once launched, a castle always rises.
    /// - PATCHED arm: the landing always re-scans; a refused site is
    ///   displaced one step back (the pre-arm port behavior).
    ///
    /// APPROX: snap-steer in place of the original's eased turn.
    fn proj_castle_ball_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let mc1 = !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let patched = ctx.patches.castle_latch_bug && !ctx.strict;
        let one_castle = ctx.patches.one_castle_per_wizard && !ctx.strict;
        // sub_53980's dispatch is on the TARGET, not the model: a
        // ball with a homing slot in +146 (the upgrade cast stamps
        // the bound castle, :65906-08) runs the HOMING arm —
        // sub_52610 every tick, speed ease, arrival on plain
        // overlap, morph into (+68, +69) — and NEVER touches the
        // launch latch (mc1l0 t=2174-77: flags hold 4 through the
        // flight; the port's old latch write was the flags+2 row
        // family). +146 = 0 falls through to the sub_53B50
        // create-castle arm below.
        if mc1 && self.ent[i].f146 != 0 {
            return self.castle_ball_homing_tick(i, one_castle);
        }
        // The UPGRADE variant (+69 = 43, :65904-08) skips the
        // placement scans — it flies at the OWN castle and morphs
        // into the (10,43) token there (sub_53980 has no launch
        // latch: the upgrade ball moves from its first tick).
        let upgrade = self.ent[i].f69 == 43;
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let (x, y) = (self.ent[i].x, self.ent[i].y);
            if !upgrade && !self.castle_site_ok(i, x, y) {
                // The launch failure releases the owner's charge pin
                // BEFORE the despawn (:63614-16, sub_46D20(ball, 0))
                // — the refused site costs the mana but frees the
                // hand for the recast.
                let own = self.ent[i].id24;
                self.release_castle_charge_pin(own);
                self.ent[i].flags |= 0x400;
                return false;
            }
            if mc1 && !upgrade {
                return false;
            }
        }
        // The launch speed boost eases away: +126 walks 2/tick toward
        // the ctor +128 (:63565-67 and the :63472-76 upgrade twin).
        if mc1 {
            let e = &mut self.ent[i];
            e.f126 += flight_speed_step(e.f128 - e.f126);
        }
        let (px, py, pz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (dx, dy) = (self.ent[i].dest_x, self.ent[i].dest_y);
        let tz = self.ground_z(dx, dy) as i16;
        // EASED steering (sub_53B50 :63548-65 via sub_422A0 with the
        // behavior-row caps): the ball leaves along the wizard's aim
        // and turns toward the ground target at row-0 rates — the aim
        // pitch shapes the early arc (NOT snap-steer, which ignores
        // the aim).
        let tgt_yaw = Self::angle_between(px, py, dx, dy);
        let dh = Self::isqrt(Self::dist2_sq(px, py, dx, dy) as u32) as i32;
        let tgt_pitch = Self::pitch_toward(pz, tz, dh);
        let row = &BEHAVIOR[self.ent[i].row156 as usize];
        let (v2, v6) = (row.v_2, row.v_6);
        {
            let e = &mut self.ent[i];
            e.f34 = tgt_yaw;
            e.f36 = tgt_pitch;
            let ty = Self::turn_step(e.f30, tgt_yaw, v2);
            e.f30 = (e.f30 as i32 + ty as i32) as u16 & 0x7FF;
            let tp = Self::turn_step(e.f32, tgt_pitch, v6);
            e.f32 = (e.f32 as i32 + tp as i32) as u16 & 0x7FF;
        }
        let (yaw, pitch) = (self.ent[i].f30, self.ent[i].f32);
        let mut tmp = (px, py, pz);
        let speed = self.ent[i].f126;
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let mut grounded = ground > tmp.2;
        // Retail keeps the STEPPED z through the move (:63577-79 —
        // the recorded landing tick shows z 7344 under ground 7808);
        // the ctor'd castle takes its own ground datum. MC2 keeps the
        // pre-arm ground clamp.
        let move_z = if mc1 || !grounded { tmp.2 } else { ground };
        self.move_relink(i, tmp.0, tmp.1, move_z);
        // The with-castle flight lands on OVERLAP with the linked
        // castle — the ball snaps onto it and morphs (:63484-88);
        // the castle's 0x4000 z-extent makes any overflight count.
        if upgrade {
            let c = self.ent[i].f146 as usize;
            if c != 0
                && self.ent[c].class64 == 3
                && self.ent[c].flags & 0x400 == 0
                && self.ent_overlap(i, c)
            {
                let (cx, cy, cz) = (self.ent[c].x, self.ent[c].y, self.ent[c].z);
                self.move_relink(i, cx, cy, cz);
                tmp = (cx, cy, cz);
                grounded = true;
            }
        }
        // The life countdown runs on AIRBORNE ticks only (:63586-88
        // short-circuits the decrement behind the ground test); the
        // pre-arm MC2 path keeps the unconditional decrement.
        if !mc1 || !grounded {
            self.ent[i].act_life -= 1;
        }
        let mut land = grounded || self.ent[i].act_life < 0;
        // The RETAIL arm's airborne tripwire (:63588-90): the scan
        // runs only while still flying; a failure stops the ball
        // here and builds displaced.
        let mut stepback = false;
        if mc1 && !patched && !land && !upgrade && !self.castle_site_ok(i, tmp.0, tmp.1) {
            land = true;
            stepback = true;
        }
        if land {
            let own = self.ent[i].id24;
            if upgrade {
                // Morph into the (10,43) upgrade token at the castle
                // (:63606-08): owner stamp ONLY — retail never writes
                // the token's +146; the delivery resolves the castle
                // through the owner's bound slot (sub_293D0).
                let z = self.ent[i].z;
                if let Some(t) = self.spawn_creator(43, tmp.0, tmp.1, z) {
                    self.ent[t].id24 = own;
                }
                self.ent[i].flags |= 0x400;
                return false;
            }
            let (mut bx, mut by) = (tmp.0, tmp.1);
            // RETAIL arm: touchdown/expiry build unscanned; only the
            // tripwire displaces. PATCHED arm (and pre-arm MC2): the
            // landing always re-scans, a refusal displaces.
            let displace = if mc1 && !patched {
                stepback
            } else {
                !self.castle_site_ok(i, bx, by)
            };
            if displace {
                let back = yaw.wrapping_add(0x400) & 0x7FF;
                // ⭐ THE STEP-BACK MOVES THE BALL ITSELF, AND ITS z IS
                // SEEDED LIVE. `sub_53B50_53E90`'s tripwire branch
                // (CARPET.EXE `0x6C531` `cmp BYTE [esp+4],0` / `74 42`)
                // re-seeds the scratch axis from the ball's OWN
                // `+72/+76` — `0x6C53D lea esi,[ebx+0x48]` then
                // `0x6C540 a5` (x,y) AND `0x6C541 66 a5` (**z**) —
                // polar-steps it with `yaw+0x400` (`0x6C553 add ah,4`
                // / `0x6C556 and ah,7`), the LIVE pitch
                // (`0x6C54A mov ax,[ebx+0x20]`) and the live speed
                // (`0x6C543 movsx eax,[ebx+0x7e]`), then writes it back
                // THROUGH THE BALL: `0x6C571 53` = `push ebx` into
                // `0x6C572 e8 f1 de fe ff` → `sub_41C70_41FB0` (move +
                // relink). Only THEN does the ctor run, and it reads
                // the ball's own axis (`0x6C584 lea eax,[ebx+0x48]` →
                // `0x6C588` → `sub_373F0_377B0`) — so the castle is
                // built at the ball's NEW position and the ball's last
                // recorded x/y/z are the stepped-back ones. The port
                // kept the result in locals for `spawn_castle` only and
                // seeded z with 0, so the ball's z never took the
                // second (downward) pitch component. WITNESS
                // mc1l48-nodeath t=473→474 slot 972 (yaw 527, pitch 31,
                // speed 458): retail (1388,29804,3466) → (1387,29805,
                // 3380); the port stopped at the FORWARD step
                // (1842,29825,3423), and 3423 − 43 = 3380 — the pitch's
                // vertical component taken once instead of twice.
                // Decompile :63598-611. MC2 keeps the flat step.
                let mut t = (bx, by, if mc1 { self.ent[i].z } else { 0 });
                // The step back carries the live pitch (:63601-04);
                // pre-arm MC2 keeps the flat step.
                Self::polar_step(&mut t, back, if mc1 { pitch } else { 0 }, speed);
                bx = t.0;
                by = t.1;
                if mc1 && !no_mc1_castle_ball_stepback_moves_ball() {
                    self.move_relink(i, t.0, t.1, t.2);
                }
            }
            // ONE CASTLE PER WIZARD (patched arm only). Retail's
            // plain create arm carries NO owner test whatsoever —
            // land two balls far enough apart that sub_12F70 does not
            // refuse the site and BOTH build, splitting the owner's
            // brain (`WorldPatches::one_castle_per_wizard`). MC2's
            // arm at least reads a register; this one reads nothing.
            // Refuse the way MC2's guard does: despawn the ball.
            // ⭐ THE CREATE ARM'S KILL IS INSIDE `if (v2)` — the ball
            // dies ONLY when the castle ctor returned a record
            // (sub_53B50 :63606-11; CARPET.EXE 0x6C588 `call sub_373F0`
            // / 0x6C590 `test eax,eax` / 0x6C592 `je 0x6c5a5` skips
            // BOTH the owner stamp and the 0x6C59D reap call). On a
            // DRY POOL (`sub_373F0` → 0) the landed ball keeps flying:
            // it re-steers, re-lands and retries the ctor every tick
            // until a slot frees. No pin release on this arm (the
            // homing arm's :63513-15 `sub_46D20` has no twin here).
            // WITNESS mc1l25 t=2616-2619 slot 778 (pool 999 live): the
            // grounded ball's life freezes at 17 (the :63586 short-
            // circuit) and retail's flags hold 6 for four ticks while
            // the port reap-flagged it at the first landing; retail
            // builds at t=2621 when the pool has room. See
            // [`crate::engine::features::no_mc1_castle_ball_dry_pool_retry`].
            let mut built = true;
            if !(one_castle && self.castle_owned_by(own)) {
                if let Some(c) = self.spawn_castle(bx, by) {
                    self.ent[c].id24 = own;
                    // Claim owner (+144) — the mana census counts the
                    // castle's stored mana into the owner's ceiling.
                    self.ent[c].f144 = own;
                } else {
                    built = false;
                }
            }
            if built || !mc1 || crate::engine::features::no_mc1_castle_ball_dry_pool_retry() {
                self.ent[i].flags |= 0x400;
            }
        }
        false
    }

    /// Does `own` already hold a castle? ANY live, un-reaped (3,2)
    /// stamped with that owner, AT ANY LEVEL — the patched arm's
    /// answer to "do you already have one"
    /// (`WorldPatches::one_castle_per_wizard`). Deliberately not
    /// retail's test on either arm: retail MC1 asks nothing at all on
    /// the plain create and demands `f26 > 0` on the delivery, and
    /// retail MC2 reads a register written a tick late. All three
    /// leave a window; a pool scan with no level test leaves none.
    pub(crate) fn castle_owned_by(&self, own: u16) -> bool {
        (1..self.ent.len()).any(|c| {
            let e = &self.ent[c];
            e.class64 == 3 && e.model65 == 2 && e.id24 == own && e.flags & 0x400 == 0
        })
    }

    /// sub_53980's +146 arm (:63459-63518): the HOMING castle ball.
    /// sub_52610 homing every tick (the twin WITHOUT the aim-lift
    /// wrap — the bearing runs to the target's RAW z; masked), the
    /// ±2 speed ease, one polar step, then: plain OVERLAP with
    /// whatever +146 holds (blind — no class or dead guard,
    /// :63484-88) teleports the ball onto the target and delivers;
    /// otherwise a terrain touch delivers in place, and only an
    /// AIRBORNE tick pays life (:63494-96 short-circuits the
    /// decrement behind the ground test). Delivery morphs the ball
    /// into (+68, +69) at its current position, owner-stamped
    /// (:63506-11) — refused outright for a class-3 morph when the
    /// owner already holds a BOUND castle (:63500-04, wizext+50;
    /// stand-in = the owner's established (3,2) like the upgrade
    /// token's) — and a FULL POOL releases the owner's m16
    /// manifestation charge pin instead of killing the ball
    /// (:63513-15, sub_46D20(pool[+24], 0): the ball lives and
    /// retries next tick).
    fn castle_ball_homing_tick(&mut self, i: usize, one_castle: bool) -> bool {
        let tgt = self.ent[i].f146 as usize;
        if tgt < self.ent.len() {
            let (tx, ty, tz) = {
                let c = &self.ent[tgt];
                (c.x, c.y, c.z)
            };
            let e = &self.ent[i];
            let yaw = Self::angle_between(e.x, e.y, tx, ty);
            let dh = Self::isqrt(Self::dist2_sq(e.x, e.y, tx, ty) as u32) as i32;
            let pitch = Self::pitch_toward(e.z, tz, dh);
            let row = &BEHAVIOR[e.row156 as usize];
            let (v2, v6) = (row.v_2, row.v_6);
            self.ent[i].f34 = yaw;
            self.ent[i].f36 = pitch;
            let ty_ = Self::turn_step(self.ent[i].f30, yaw, v2);
            self.ent[i].f30 = (self.ent[i].f30 as i32 + ty_ as i32) as u16 & 0x7FF;
            let tp = Self::turn_step(self.ent[i].f32, pitch, v6);
            self.ent[i].f32 = (self.ent[i].f32 as i32 + tp as i32) as u16 & 0x7FF;
        }
        {
            let e = &mut self.ent[i];
            e.f126 += flight_speed_step(e.f128 - e.f126);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        let done = if tgt != 0 && tgt < self.ent.len() && self.ent_overlap(i, tgt) {
            let (cx, cy, cz) = {
                let c = &self.ent[tgt];
                (c.x, c.y, c.z)
            };
            self.move_relink(i, cx, cy, cz);
            true
        } else {
            let ground = self.ground_z(self.ent[i].x, self.ent[i].y) as i16;
            if ground > self.ent[i].z {
                true
            } else {
                self.ent[i].act_life -= 1;
                self.ent[i].act_life < 0
            }
        };
        if done {
            let own = self.ent[i].id24;
            let (f68, f69) = (self.ent[i].f68, self.ent[i].f69);
            let (x, y, z) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            if std::env::var_os("MGC_CASTLE_PIN_TRACE").is_some() {
                eprintln!(
                    "[pin] t={} homing ball {i} done: own={own} f68={f68} f69={f69} \
                     at ({x},{y},{z}) life={}",
                    crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed),
                    self.ent[i].act_life
                );
            }
            // `e.f26 > 0` is retail's own level test (:63500-04), and
            // it is the second half of the split window: a castle that
            // has LANDED but not yet transformed reads as unowned, so
            // a delivery arriving before the first transform builds
            // again. The patched arm drops the level test — any live
            // (3,2) of this owner counts (`WorldPatches::
            // one_castle_per_wizard`).
            let bound = |s: &Self| {
                (1..s.ent.len()).any(|c| {
                    let e = &s.ent[c];
                    e.class64 == 3
                        && e.model65 == 2
                        && e.id24 == own
                        && (one_castle || e.f26 > 0)
                        && e.flags & 0x400 == 0
                })
            };
            if f68 == 3 && bound(self) {
                self.ent[i].flags |= 0x400;
            } else {
                let spawned = if f68 == 3 {
                    let c = self.spawn_castle(x, y);
                    if let Some(c) = c {
                        self.ent[c].id24 = own;
                        self.ent[c].f144 = own;
                    }
                    c
                } else {
                    let t = self.spawn_creator(f69 as u16, x, y, z);
                    if let Some(t) = t {
                        self.ent[t].id24 = own;
                    }
                    t
                };
                if std::env::var_os("MGC_CASTLE_PIN_TRACE").is_some() {
                    eprintln!(
                        "[pin] t={} homing ball {i}: morph spawned={spawned:?}",
                        crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed)
                    );
                }
                if spawned.is_some() {
                    self.ent[i].flags |= 0x400;
                } else {
                    // Pool full: release the pin, the ball lives and
                    // retries (:63513-15).
                    self.release_castle_charge_pin(own);
                }
            }
        }
        false
    }

    /// sub_12F70 (:17786): the castle placement scan — fails when
    /// another castle (c3 m2) is within its own extents+2048 on both
    /// axes (`abs16(dx) <= f80 + 2048` — the probe's extents play no
    /// part; MC1-faithful, MC2 keeps the pre-arm wider margin), or
    /// any tile of the 8x8 block at (tx-8..tx-1, ty-8..ty-1) — the
    /// original's asymmetric NW-only window, ported verbatim: it
    /// never samples the anchor tile itself nor anything south/east
    /// of it, which is half of the `castle_latch_bug` maze cheese —
    /// carries the protection bit.
    pub(crate) fn castle_site_ok(&self, i: usize, x: u16, y: u16) -> bool {
        let mc1 = !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        let (f80, f82) = if mc1 {
            (0, 0)
        } else {
            (self.ent[i].f80 as i32, self.ent[i].f82 as i32)
        };
        let slack = i32::from(mc1);
        let wd = |p: u16, q: u16| (p.wrapping_sub(q) as i16 as i32).abs();
        for j in 1..self.ent.len() {
            let c = &self.ent[j];
            if c.class64 == 3
                && c.model65 == 2
                && c.flags & 0x400 == 0
                && wd(c.x, x) < c.f80 as i32 + f80 + 2048 + slack
                && wd(c.y, y) < c.f82 as i32 + f82 + 2048 + slack
            {
                return false;
            }
        }
        let (tx, ty) = ((x >> 8) as i32, (y >> 8) as i32);
        for dy in -8..0i32 {
            for dx in -8..0i32 {
                if self.t.angle[tile((tx + dx) as u8, (ty + dy) as u8)] & 0x80 != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// sub_37920 (:44229): the class-3 model-2 CASTLE entity —
    /// grid-snapped with (tx+ty) even parity, state 5 machine
    /// (sub-state f59 = 0 → the level-up arm builds level 1),
    /// sprite 177, life 40000. The visible castle is painted
    /// terrain; this entity is the anchor/state machine.
    pub(crate) fn spawn_castle(&mut self, x: u16, y: u16) -> Option<usize> {
        // Snap = TRUNCATION in both ctors (sub_37920's HIBYTE /
        // sub_4AA40's `>>= 8`), then the parity +1 on x.
        let mut cx = (x >> 8) as u8;
        let cy = (y >> 8) as u8;
        if (cx as u16 + cy as u16) % 2 == 1 {
            cx = cx.wrapping_add(1); // parity snap (:44246-52)
        }
        let (px, py) = ((cx as u16) << 8, (cy as u16) << 8);
        // ⭐⭐ TWO Z DATA, NOT ONE — AND ONLY MC2 SPLITS THEM.
        //
        // Both ctors open identically: take the RAW caller axis, read
        // the ground under it, parity-snap x/y to the tile corner,
        // link. MC1 (`sub_37920_37CE0` remc1:44229) keeps ONE number
        // for both jobs, and does so BY STACK ACCIDENT: `v4 =
        // sub_11F50(&raw)` (:44250) is the ground at the raw point, it
        // is written to +154 (:44256), and the link
        // `sub_41CF0_42030(v2, (axis_3d*)&v3)` (:44257) reads an
        // axis_3d at `&v3` whose z WORD IS `v4` — v3 sits at
        // [ebp-14h], v4 at [ebp-10h] (:44233-34), and `axis_3d`'s z is
        // at byte offset 4 (remc1 Basic.h:41-45). Same value, both
        // lanes (mc1l0 t=562: site (114,96) carries z 797, the
        // mid-tile ground, not the corner's 736).
        //
        // MC2 (`sub_4AA40` EF:33362) uses a REAL `axis_3d` local, and
        // that is the whole difference:
        //     v6ar.z = getTerrainAlt_10C40(&predictedAxis_EB398ar); // :33385
        //     v2x->axis_0x9A_154x = v6ar;                           // :33390
        //     v2x->axis_0x9A_154x.z = 32 * sub_48E60(..);           // :33399
        //     AddEventToMap_57D70(v2x, &v6ar);                      // :33400
        // The perimeter-min overwrite lands on the ENTITY's +0x9E and
        // never touches `v6ar`, so the record is LINKED at the ground
        // under the RAW landing point while its painter/leveler datum
        // keeps the perimeter minimum. Retail carries BOTH at once.
        //
        // Measured at the two birth ticks where the data DIFFER:
        //   mc2l0  t=7224 slot 4   retail z 864 / dest_z 0
        //   mc2l30 t=234  slot 126 retail z 256 / dest_z 0
        // and at three where they COINCIDE on sea-level ground, which
        // is why one number passed for so long: spells-galore t=1029
        // slot 266, mc2l3 t=244 slot 127, mc2l1 t=2069 slot 146.
        let link_z = self.ground_z(x, y) as i16;
        // The build datum (+154 / site_z): MC1 = that same ground;
        // MC2's ctor (EF:33399) = 32 x the perimeter-MIN over the
        // BUILD00 row-1 footprint at the snapped site.
        let z = match self.verbs.movement {
            crate::verbs::MovementVerb::Mc2 => self.mc2_castle_site_z(cx, cy),
            _ => link_z,
        };
        let s = self.new_event()?;
        {
            let e = &mut self.ent[s];
            e.class64 = 3;
            e.model65 = 2;
            e.tick70 = 5;
            e.f59 = 0;
            e.f26 = 0;
            e.max_life = 40000;
            // The site echo (+150, :44255) — retail's build workers
            // resolve their castle through it.
            e.dest_x = px;
            e.dest_y = py;
            // Build-site z (+154): the painter/leveler datum. The
            // entity z (+76) is refreshed to live ground per tick —
            // the flag rides the painted tower.
            e.site_z = z;
            // Channel mask (+28 = 33, ch0+ch5 — sub_37920 :44247).
            e.f28 = 33;
        }
        // ⚠ `link_z`, NOT `z`. MC2's `AddEventToMap_57D70(v2x, &v6ar)`
        // (EF:33400) links with the ctor LOCAL, whose z is still the
        // raw-point ground — the perimeter-min write one line earlier
        // hit the entity field only. On MC1 the two are one number.
        self.link(s, px, py, link_z);
        self.refill_life(s);
        // ⭐ THE SPRITE/EXTENT STAMP IS PER-GAME, AND IT IS THE ONLY
        // LINE OF THIS CTOR THAT IS. MC1's `sub_37920_37CE0` ends in
        // `sub_36FA0_37360(event, 177)` (remc1 sub_main.cpp:44259) —
        // the SPRITE_STATS row, 369 x 400, quad {200, 184, 184, 200}.
        // MC2's `sub_4AA40` ends in `SetEntityIndexAndRot_49CD0(v2x,
        // 177)` (EF:33402) — the `particlesParameters_D951C` row,
        // whose (speed_6, rotSpeed_8) pair is DERIVED AT BOOT from the
        // DAY bank's sprite 96 (38 x 39 → 38*400/39 = 389), quad
        // {200, 194, 194, 200}. Same literal 177, two different tables.
        // ⚠ The two tables COLLIDE at 184 for this row, because MC2's
        // night/cave banks ship sprite 96 at 36 wide (36*400/39 = 369)
        // — exactly what MC1's table carries. That is why this read as
        // the already-closed dwelling day-source family (ledger
        // 2026-08-01 ①) rather than as a wrong-table bug.
        // Measured at the castle's BIRTH tick, three takes, zero
        // counterexamples: spells-galore t=1029 slot 266, mc2l1 t=2069
        // slot 146, mc2l30 t=234 slot 126 — retail apitch/aroll 194,
        // port 184, every other lane equal. Live for one tick only
        // (`mc2_castle_extents_ent` overwrites the quad on the
        // castle's first dispatch), but the birth tick is the boundary
        // the free run dies on.
        match self.verbs.movement {
            crate::verbs::MovementVerb::Mc2 => {
                self.mc2_set_sprite(s, 177);
                // `SetEntityIndex_49C90`'s third line (EF:32834):
                // `byte_0x5D_93 = x_BYTE_D8A2E[params[177].byte_12]`.
                // `x_BYTE_D8A2E` (EF:2297) is byte-identical to MC1's
                // FRAME_COUNTS and row 177's draw type is 0, so this is
                // retail's own 1 (dump-state t=1029 slot 266: b5d = 1).
                // `mc2_set_sprite` does not carry the lane yet — retail
                // REWRITES every row's `byte_12` at boot from the
                // decompressed tmap header (EF:44906), like
                // speed_6/rotSpeed_8, so the static column is not
                // trustworthy table-wide and the general stamp is a
                // separate law. Kept local so the swap does not drop
                // this record from retail's 1 to the default zero.
                self.ent[s].frames89 = crate::mc1::mobs::FRAME_COUNTS
                    [crate::mc2::sprite_params::SPRITE_PARAMS[177].byte_12 as usize];
            }
            _ => self.set_sprite(s, 177),
        }
        Some(s)
    }

    /// sub_52770 (:62618): the generic flight (m3 trail bolt) — speed
    /// eases ±2 toward +128, homing, explode copies +44 + victim.
    /// `fire_trail`: m3 drops a damage-suppressed fire-seeder per tick
    /// (:63027-38).
    fn proj_generic_tick(&mut self, i: usize, ctx: &MobCtx, fire_trail: bool) -> bool {
        let e = &mut self.ent[i];
        e.f126 += flight_speed_step(e.f128 - e.f126);
        // ⭐ ACQUISITION IS ONE-SHOT, AND IT SNAPS. `sub_52770` opens
        // by testing the target slot (+146): with a target it goes
        // straight to the tracker, and WITHOUT one it runs the acquire
        // exactly once, latched on flags bit 2 and set win or lose
        // (:62640-60):
        //
        //   if ((flags & 2) == 0) {
        //       flags |= 2;
        //       if (sub_54520(self)) { +30 = +34; +32 = +36; }   // SNAP
        //       else                 { +34 = +30; +36 = +32; }   // mirror
        //   }
        //
        // A hit SNAPS the live heading onto the pick and only then
        // hands over to the per-tick tracker; a MISS mirrors the live
        // heading into the aim fields and the bolt flies straight for
        // the rest of its life, never scanning again. The `else`
        // mirror also runs for the models `sub_54520` declines —
        // m14's `default: return 0` (:64185) — which is why the
        // acquire CALL is unconditional here and only the scan is
        // model-gated.
        //
        // The port used to re-scan EVERY tick while untargeted and
        // never snap, which let a meteor lock onto something that
        // drifted into its cone long after launch (or onto a creature
        // that merely WOKE UP mid-flight — the creature buckets are
        // gated on the awake counter +58, :63996, and retail samples
        // that gate once, at launch), and then ease onto it in a long
        // lazy curve instead of re-pointing. Player-reported as
        // meteors "curving weirdly"; the LONG curve itself is
        // faithful — `sub_52550` tracks the target's live position
        // every tick with no range, lifetime or line-of-sight bound
        // ([`Gen::home`]) — but retail commits to its victim at the
        // muzzle.
        //
        // ⚠ ROOT CAUSE, and the same shape as the castle-extents and
        // building-degradation misses: retail's `sub_52770` is ONE
        // function that the port split in two, and the latch was
        // ported into the `proj_firewall_tick` half only, where a
        // comment claimed it was Hidden Worlds' — it is not, it is
        // right here in base remc1. **The port's function boundaries
        // are not retail's.**
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                // The acquire switch's live cases (:63979 block 0/3/4)
                // — the retail meteor SNAPS to a bee in the cone and
                // the blast ring does the cluster.
                if matches!(self.ent[i].model65, 0 | 3 | 4) {
                    self.aim_assist(i, ctx);
                }
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            // The tracker is the ELSE arm: the tick that acquires
            // snaps and stops there, and easing starts the tick after.
            self.home(i, ctx);
        }
        let r = self.proj_move_and_hit(i, ctx, true, true, DeflectLaw::Generic);
        // :63027-38 — the m3 trail wrapper (sub_53070) mints the
        // seeder AFTER the core returns: at the POST-step position,
        // and on the detonation tick too (`+64` still reads 10
        // through the soft kill — only a hard free skips it, the +64
        // gate). The port minted it pre-move, so every trail puff was
        // born one flight step behind (mc1l32's (10,1) x/y/z family,
        // ~1,450 pairs) and the dying tick's payload/puff free-stack
        // pops landed in swapped slots (t=24700: retail field@53
        // puff@20, port inverted).
        if fire_trail && self.ent[i].class64 != 0 {
            let (x, y, z, owner) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24)
            };
            if let Some(s) = self.spawn_effect(1, x, y, z) {
                // ⭐ THE TRAIL CAN SEIZE ITS OWN BOLT — see
                // [`no_mc1_self_seize`]. Retail hands the ctor a
                // POINTER to the bolt's `+72` and reads `+24` after the
                // allocator returns, so a self-seized bolt yields a
                // seeder linked at its own freshly-zeroed (0,0,0) and
                // owned by its own slot stamp.
                let alias = self.mc1_self_seized(s, i);
                // +16|=0x80, +18|=1: the seeder's fires inherit the
                // no-damage bit — a decorative trail (:63033-38).
                self.ent[s].flags |= 0x80 | 0x10000;
                if !alias {
                    self.ent[s].id24 = owner;
                }
            }
        }
        r
    }

    /// sub_530C0 (:63048): m11's bolt — explodes ONLY on wizard-family
    /// victims (class 3 model ≤ 1 / the player); every other end is a
    /// silent despawn (:63188-210).
    fn proj_m8_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // sub_530C0 opens on the SAME acquire-or-track fork every
        // other flight handler carries (:63071-84), and the port ran
        // only the tracker half. `+146 == 0` means the seeker was
        // launched UNTARGETED; retail then latches +16 bit 1 and runs
        // the ONE-SHOT acquire — `sub_54520` block 8, the
        // significant-list-only scan ([`Self::aim_assist_wizards`]) —
        // snapping the live heading onto the pick, or mirroring the
        // heading into the aim fields when nothing scores.
        //
        // Retail launches m8s untargeted from BOTH of its two sites:
        // the wizard's Steal Mana cast (`sub_57250` :65740-62 stamps
        // +68/+69/+44/+24/+76/+140/+26/+150/+30/+32 and never +146)
        // and the genie's PARTING shot, which reads the caster's LIVE
        // +146 at :24697 — `sub_1E720` has just zeroed it — while the
        // launch bearing still comes off the stale `v9`. So an
        // untargeted seeker never acquired, never latched the bit, and
        // never took `sub_54520`'s entry clamp of +26 to 16 (:63975-76).
        // mc1l42 t=2133 slot 378, the genie at slot 101 dropping its
        // target that very tick (+146 331 → 0): retail hands back
        // flags 6 / +26 16 / +30 1321 / +146 331, the port flags 4 /
        // +26 20 / +30 1322.
        if self.ent[i].f146 == 0 && !no_m8_acquire() {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                self.aim_assist_wizards(i, ctx);
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else if self.ent[i].f146 != 0 {
            self.home(i, ctx);
        }
        // :63097-99 — and the whole flight family (:62671, :63478,
        // :63571, :63680): the servo step is `2 * SIGN(+128 - +126)`,
        // not the gap clamped to ±2. On an odd gap of 1 retail steps 2
        // and OVERSHOOTS; the port stalled one short and never
        // converged. (The sibling handlers still carry the clamp form
        // — same law, other territory.)
        let e = &mut self.ent[i];
        e.f126 += if no_m8_acquire() {
            (e.f128 - e.f126).clamp(-2, 2)
        } else {
            flight_speed_step(e.f128 - e.f126)
        };
        // Move — and RELINK, before the probe (:63103-05: `sub_41C70`
        // to the stepped point, then `sub_11980` at the moved self).
        // The step out of and back into a tile re-heads the record in
        // its chain; endpoint-only probing dropped those chain ops
        // (the t=7785 lineage — see `proj_move_and_hit`).
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        if let Some(v) = self.victim_scan(i, ctx) {
            let wizard = match v {
                MailTarget::Player(_) => true,
                MailTarget::Pool(j) => self.ent[j].class64 == 3 && self.ent[j].model65 <= 1,
            };
            // THE HIT TELEPORTS ONTO THE VICTIM (:63154-56), exactly
            // like the generic arm: `sub_524C0` lifts the victim's +76
            // by its own +78 in place, `sub_41C70(a1, victim+72)`
            // relinks the seeker there, `sub_524E0` puts the lift back
            // — the model-2 castle exemption rides in [`Ent::aim_z`].
            // The stepped point is NEVER where a hit lands, and the
            // flash is born at the seeker's post-snap position: mc1l42
            // t=4→5, retail seeker AND flash at 896/21376/3668 =
            // carpet 331's 896/21376/3568 + its +78 of 100, against
            // the port's stepped 890/21343/3672.
            match v {
                MailTarget::Pool(j) => {
                    let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                    self.move_relink(i, jx, jy, jz);
                }
                MailTarget::Player(hx) => {
                    let (px, py, pz) = self.seat_at(ctx, hx);
                    self.move_relink(i, px, py, pz.wrapping_add(PLAYER_HH as i16));
                }
            }
            if wizard {
                // The child carries the struck victim in +146
                // (:63201 `v19[73] = victim`) — the mana-steal flash
                // reads chase 331 on every recorded strike.
                self.proj_explode(i, ctx, Some(v), true, true);
            } else {
                // :63208-09 — a shot, then the kill.
                self.mc1_shot_stats_silent(i);
                self.ent[i].flags |= 0x400;
            }
            return false;
        }
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        if ground <= tmp.2 {
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                self.mc1_shot_stats_silent(i);
                self.ent[i].flags |= 0x400; // silent timeout
            }
        } else {
            // Terrain block: no revert — the water test, the splash
            // and the silent end all read the point the seeker flew
            // TO (:63161-83), where the pre-probe move left it. The
            // splash arm leaves the detonation flag clear (:63175-80):
            // no shot.
            if self.on_water_pub(tmp.0, tmp.1) {
                self.splash_and_die(i);
            } else {
                self.mc1_shot_stats_silent(i);
                self.ent[i].flags |= 0x400; // silent ground end
            }
        }
        false
    }

    /// sub_535E0 (:63272): the lightning BEAM — resolves in ONE tick.
    /// The flight walks to termination inside the handler in 384-unit
    /// steps (life counts STEPS, not ticks; victim snap / terrain
    /// stop / expiry; NO water splash, NO deflection), then the beam
    /// redraws itself as a chain of short-lived state-14 segment
    /// entities along a ±1 random walk (8 sub-steps per flight step)
    /// and explodes at the segment-walk endpoint. The kraken fires
    /// one beam per burst tick — a beam re-laid every tick, not a
    /// traveling ball.
    fn proj_m9_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        self.ent[i].f126 = self.ent[i].f128;
        let spawn = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        // THE BEAM LEAVES THE TILE CHAIN BEFORE IT FLIES (:63311) and
        // never rejoins: sub_534C0's flight writes +72/+76 RAW
        // (:63247-48, :63253-54), not through the move-relink every
        // other projectile uses, so the record spends its whole
        // resolution — and its whole afterlife, since the beam dies on
        // this same tick — unlinked. Retail's beam is therefore
        // invisible to every scan that walks the tile lists, and its
        // death flags read 0x400 with the link bit CLEAR.
        self.unlink(i);
        // sub_534C0 (:63216): one-time aim assist only while
        // untargeted (+146 == 0 — fire sites that pre-lock +146 also
        // pre-aim +30/+32 at the target, closing this gate); snap to
        // the acquired angles, no per-tick easing, no homing ever.
        // The snap runs inside the FIRST flight call (:63312), BEFORE
        // the chain heading is saved.
        if self.ent[i].f146 == 0 && self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.aim_assist(i, ctx);
            if self.ent[i].f146 != 0 {
                self.ent[i].f30 = self.ent[i].f34;
                self.ent[i].f32 = self.ent[i].f36;
            } else {
                // :63238-40 — the MISS branch is a real store: the
                // aim pair mirrors the live heading. (The storm's
                // launcher used to pre-seed +34/+36 and stand in for
                // this; the launcher now matches retail and leaves
                // them at NewEvent 0, so the copy has to live here,
                // where the original puts it.)
                self.ent[i].f34 = self.ent[i].f30;
                self.ent[i].f36 = self.ent[i].f32;
            }
        }
        // Yaw/pitch are saved AFTER the snap (:63313-14) and restored
        // for the segment chain (:63327-28) — the visible chain and
        // the endpoint explosion follow the AIMED heading, which is
        // why the retail bolt points at (and lands on) its victim.
        let (yaw0, pitch0) = (self.ent[i].f30, self.ent[i].f32);
        let mut steps: i32 = 0;
        loop {
            steps += 1;
            let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            let (yaw, pitch, speed) = {
                let e = &self.ent[i];
                (e.f30, e.f32, e.f126)
            };
            Self::polar_step(&mut tmp, yaw, pitch, speed);
            if let Some(v) = self.victim_scan_at(i, tmp, ctx) {
                // Snap to the victim's exact position — no +78
                // half-height, unlike the fireball (:63252-56).
                let p = match v {
                    MailTarget::Pool(j) => (self.ent[j].x, self.ent[j].y, self.ent[j].z),
                    MailTarget::Player(hx) => self.seat_at(ctx, hx),
                };
                let e = &mut self.ent[i];
                (e.x, e.y, e.z) = p;
                break;
            }
            let e = &mut self.ent[i];
            (e.x, e.y, e.z) = tmp;
            if self.ground_z(tmp.0, tmp.1) as i16 > tmp.2 {
                break; // terrain stop — sub_534C0 has no water case
            }
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                break; // expired midair (≤ 10 steps for life 9)
            }
        }
        // ⭐⭐⭐ THE BEAM DIES AT THE END OF THE FLIGHT, NOT AT THE END
        // OF THE HANDLER. `sub_41E80_421C0(a1)` is the LAST statement
        // of the flight step `sub_534C0_53800` (:63267-68), not of
        // `sub_535E0_53920` — which has no `+16 |= 0x400` anywhere
        // (its whole body is `return v18`). HIDDEN.EXE `0x6C2F5:
        // 80 3c 24 00` `cmpb $0,(%esp)` / `74 09 je 0x6C304` /
        // `53 push %ebx` / `e8 b7 e8 fe ff call 0x5ABB8`, and
        // 0x5ABB8 (= HW VA 0x421C0) is exactly
        // `8b 44 24 04 / 80 48 11 04` = `orb $4,0x11(%eax)`. The
        // beam's own flight loop condition re-reads that very bit
        // (`8a 63 11 mov 0x11(%ebx),%ah` / `f6 c4 04 test $4,%ah`
        // at 0x6C355-0x6C35C), so the bit is set BEFORE the segment
        // chain is laid. Ours set it after the chain, which matters
        // only on the self-seizure below — and there it is the whole
        // divergence. See [`no_mc1_beam_self_seize`].
        if !no_mc1_beam_self_seize() {
            self.ent[i].flags |= 0x400;
        }
        self.ent[i].f30 = yaw0;
        self.ent[i].f32 = pitch0;
        // ---- the segment chain (:63329-63420): 8·steps+1 segments
        // along the straight spawn-heading path, sub-step = speed/8.
        let beam_slot = i;
        let owner0 = self.ent[i].id24;
        let live_owner = !no_mc1_beam_self_seize();
        let substep = self.ent[i].f126 / 8; // v33 = 48
        let scale = (substep / 4) as i32; // offset unit = 12
        let mut delta = (0u16, 0u16, 0i16);
        Self::polar_step(&mut delta, yaw0, pitch0, substep);
        let mut base = spawn;
        let mut disp = spawn;
        let (mut v32, mut v31): (i32, i32) = (0, 0);
        let mut v30 = steps * 8;
        loop {
            if let Some(s) = self.new_event() {
                // NewEvent defaults kept (hittable bit SET, speed 16,
                // +44 100, filter -1). Slot-order life: a slot that
                // ticks later this frame gets 0, an already-ticked
                // one -1 — one rendered frame each under the
                // state-14 pre-decrement test (:63345-56).
                {
                    // ⭐ `+24` IS A MEMORY RE-READ, ONCE PER SEGMENT.
                    // HIDDEN.EXE 0x6C412 `66 8b 43 18`
                    // `mov 0x18(%ebx),%ax` / 0x6C416 `66 89 46 18`
                    // `mov %ax,0x18(%esi)` sits INSIDE the chain loop
                    // (which closes at 0x6C5AD `jge 0x6C3FB`), so the
                    // segment that seizes the beam's own record — and
                    // is therefore re-stamped by `NewEvent` with
                    // `+24 = own slot` (0x5015E `66 89 43 18`) —
                    // changes the owner every later segment and the
                    // endpoint blast inherit. See
                    // [`no_mc1_beam_self_seize`].
                    let owner = if live_owner { self.ent[i].id24 } else { owner0 };
                    let e = &mut self.ent[s];
                    e.class64 = 9;
                    e.model65 = 9;
                    e.tick70 = 14;
                    e.id24 = owner;
                }
                self.link(s, disp.0, disp.1, disp.2);
                self.set_sprite(s, 216);
                // The slot-order life lands in BOTH halves of the
                // pair (l32 corpus: segment max_life is 0/−1 in
                // lockstep with act_life, never a refill value).
                let sl = if s >= beam_slot { 0 } else { -1 };
                self.ent[s].act_life = sl;
                self.ent[s].max_life = sl as u32;
            }
            // Amplitude pinches toward the endpoint (:63358-62).
            let amp = (v30 / 2).clamp(0, 8);
            // Offset walk v32 (applied) then phantom walk v31 (its
            // draws only advance the RNG — confirmed in BOTH
            // decompiles, remc2 sub_66750): ±1 steps with p(+1) =
            // 78/157; draws CONDITIONAL on being inside ±amp, out-of-
            // band offsets pull back deterministically (:63363-92).
            for w in [&mut v32, &mut v31] {
                if *w <= amp {
                    if *w >= -amp {
                        let d = self.ent_rand(i);
                        *w += 2 * ((d % 0x9D) / 79) as i32 - 1;
                    } else {
                        *w += 1;
                    }
                } else {
                    *w -= 1;
                }
            }
            // Advance; the display point offsets by v32·12 in BOTH z
            // and the yaw+0x200 horizontal perpendicular — a diagonal
            // zigzag plane, max ±96 units (:63394-412).
            base.0 = base.0.wrapping_add(delta.0);
            base.1 = base.1.wrapping_add(delta.1);
            base.2 = base.2.wrapping_add(delta.2);
            let off = (v32 * scale) as i16;
            disp = (base.0, base.1, base.2.wrapping_add(off));
            let mut p = (disp.0, disp.1, 0i16);
            // ⭐ THE PERPENDICULAR DIRECTION IS ANOTHER LIVE `+30`
            // READ, not the saved `yaw0`: HIDDEN.EXE 0x6C582
            // `66 8b 43 1e mov 0x1e(%ebx),%ax` / 0x6C586
            // `80 c4 02 add $2,%ah` / 0x6C58B `80 e4 07 and $7,%ah`
            // — inside the chain loop. After a self-seizure the
            // beam's `+30` is the memset 0, so retail's zigzag plane
            // swings to a fixed 0x200 for every remaining segment
            // (the residual x/y rows on slots 874..893).
            let zig_yaw = if live_owner { self.ent[i].f30 } else { yaw0 };
            Self::polar_step(&mut p, zig_yaw.wrapping_add(0x200) & 0x7FF, 0, off);
            disp.0 = p.0;
            disp.1 = p.1;
            v30 -= 1;
            if v30 < 0 {
                break;
            }
        }
        // ---- endpoint (:63421-49) ----
        // The victim the endpoint reads is a FRESH scan (:63422
        // `sub_11980(a1)`), taken after the chain is laid and from the
        // beam's own resolved position — not the verdict the flight
        // loop broke on. The two normally agree (a beam that connected
        // is sitting on its victim), but a beam that stopped on
        // terrain or ran out of life re-scans where it stopped, and a
        // scan that finds nothing is what leaves the +146 stamp below
        // unfed. Own-chain segments are invisible to it: they carry
        // the beam's own +24, and the scan's first test is `+24 !=
        // ours` ([`Gen::victim_scan`]).
        let hit = self.victim_scan(i, ctx);
        // …and the ENDPOINT reads `+24` again: HIDDEN.EXE 0x6C61F
        // `66 8b 43 18` / 0x6C623 `66 89 46 18` — the same
        // `mov 0x18(%ebx),%ax` pair, this time into the blast. So the
        // blast's `+24`, and `sub_526C0_52A00(a1, …)`'s accuracy
        // scoring, both see the POST-chain owner.
        let owner = if live_owner { self.ent[i].id24 } else { owner0 };
        // `v19[15] = *(a1+30)` / `v19[16] = *(a1+32)` (0x6C627/0x6C62B
        // and 0x6C635/0x6C63F) are live reads too — a seized beam's
        // blast therefore carries the ctor's 0/0, not the flight
        // heading (mc1hwl15 t=6434 slot 925 `heading`/`pitch`).
        let (yaw_out, pitch_out) = if live_owner {
            (self.ent[i].f30, self.ent[i].f32)
        } else {
            (yaw0, pitch0)
        };
        let (f69, f44, f140, f146) = {
            let e = &self.ent[i];
            (e.f69, e.f44, e.f140, e.f146)
        };
        let stats_pre_spawn = self.is_human_tag(owner) && no_mc1_shot_stats_alloc_guard();
        if stats_pre_spawn && self.mc1_shot_counts(self.ent[i].model65) {
            let scored = self.mc1_shot_hit(hit, f146);
            self.mc1_note_shot(owner, self.ent[i].model65, scored);
        }
        // Enhanced-lightning presentation feed: the resolved strike,
        // muzzle → chain endpoint (hash-silent, drained by the
        // frontend).
        if self.bolt_fx.0.len() < 256 {
            self.bolt_fx.0.push(crate::engine::features::BoltStrike {
                start: spawn,
                end: disp,
                owner,
            });
        }
        // The explosion lands at the SEGMENT-WALK endpoint, not the
        // beam's snapped position. Shielded (+17 bit7) class-3
        // victims with mana ≥ +140/4 quarter the payload — no drain,
        // no deflection (:63435-47). +146 is the unguarded pointer
        // difference, so a scan that found nothing records the
        // link-time constant [`MC1_MISS_STAMP`] rather than 0.
        if let Some(fx) = self.spawn_effect(f69, disp.0, disp.1, disp.2) {
            // Accuracy stats `sub_526C0` (:62585): human-owned shots
            // only, and INSIDE the effect-allocation guard (:63426-29
            // `if (v18) { sub_526C0(a1, v17, v29); …}`) like every
            // other detonation arm — see `proj_explode`. The
            // lightning's aimed pointer is taken at the ENDPOINT
            // (:63423, after the flight, which never re-acquires), so
            // the live `+146` is the latched value here — no
            // `mc1_aim_latch` read. The model gate still applies.
            // :63428-47 read `+24/+30/+32/+140/+44` off the beam AFTER
            // the allocation, so a blast that seized its own beam reads
            // its OWN fields: `sub_526C0` sees a class-10 owner and
            // scores nothing, the stamps are identity stores, and the
            // quartering tests the blast's `+140` and quarters the
            // blast's own `+44` ([`no_mc1_self_seize`]). The ctor
            // pointer here is the stack endpoint `&v25`, so the POSE
            // is the caller's copy either way.
            let seized = self.mc1_seized_caller(fx, i);
            let (owner, f140, f44) = if seized {
                let e = &self.ent[fx];
                (e.id24, e.f140, e.f44)
            } else {
                (owner, f140, f44)
            };
            if self.is_human_tag(owner)
                && !stats_pre_spawn
                && self.mc1_shot_counts(self.ent[i].model65)
            {
                let scored = self.mc1_shot_hit(hit, f146);
                self.mc1_note_shot(owner, self.ent[i].model65, scored);
            }
            let quartered = match hit {
                Some(MailTarget::Pool(j)) => {
                    self.ent[j].flags & 0x8000 != 0
                        && self.ent[j].class64 == 3
                        && f140 / 4 <= self.ent[j].f140
                }
                // The human IS retail's own class-3 pool record
                // (:63437-38): +17 bit 7 is the Rebound bit the port
                // mirrors as `player_rebound`, +140 is the mana purse
                // (class64 == 3 holds by construction — the carpet).
                // No drain, no deflection: only the payload quarters
                // (:63440 — the ONE `+44 >> 2` site in the listing;
                // the deflect family :62705/:63109/:63714 is ported
                // separately). Witness: mc1l32-quick t=18133, beam
                // slot 48 endpoint blast slot 70 — retail f44 200 vs
                // the port's 800, knock 20 vs 80, the t=18134 pose
                // head's whole 59.4-unit residual at dir 258.
                Some(MailTarget::Player(hx)) => {
                    self.player_rebound(hx) && (f140 / 4).max(0) as u32 <= self.seat_purse(ctx, hx)
                }
                None => false,
            };
            let e = &mut self.ent[fx];
            if !seized {
                e.id24 = owner;
                e.f30 = yaw_out;
                e.f32 = pitch_out;
            }
            e.f146 = match hit {
                Some(MailTarget::Pool(j)) => j as u16,
                Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                None => MC1_MISS_STAMP,
            };
            e.f44 = if quartered { f44 >> 2 } else { f44 };
        }
        // (The death bit was set by the FLIGHT, above — retail's
        // `sub_535E0_53920` has none of its own. The pre-dig arm
        // keeps it here.)
        if no_mc1_beam_self_seize() {
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// sub_54180 (:63789): the straight bolt (m13) — first-tick LCG
    /// sound roll (the `arrow1`..`arrow4` quartet), direct ch0 area
    /// write on any end. Retail reuses the arrow samples across every
    /// user of this state — the skeleton/archer creatures m4/m9/m10
    /// and the castle guard m15 — including m9, whose projectile wears
    /// a different billboard (sprite 203, :21947). That reuse IS
    /// faithful; only the boulder was wrongly borrowing it.
    fn proj_bolt_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let d = self.ent_rand(i); // :63795
            self.snd(33 + (d & 3) as u8, i);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        // ⭐ THE ARROW SCANS FROM WHERE IT STANDS, NOT WHERE IT LANDS.
        // Every other class-9 flight handler relinks onto the stepped
        // point BEFORE the probe — `sub_41C70(a1, &scratch)` then
        // `sub_11980(a1)` at :62703-06 (generic), :62872-75 (fireball),
        // :63133-36, :63713-16, and the direct +72/+76 store at
        // :63275-78. sub_54180 is the family's ONE exception: it steps
        // only the global scratch `word_AE454_AE444` and hands
        // `sub_11980` an `a1` still parked at the tick's start
        // (:63801-05; the HW twin sub_544D0 is byte-identical,
        // hw:59890-93). Since sub_11980 reads the victim window off
        // `a1 + 72/74/80` (:16998-17001) and never touches the
        // scratch, the arrow's contact test trails its flight by a
        // full 384-unit step, while the ground test below still runs
        // on the stepped point. Scanning the endpoint made the arrow
        // connect a tick EARLY — mc1l42 t=525: the militia's arrow
        // steps clean THROUGH the human carpet's box and lives on at
        // act_life 5 → 4. Only the y axis of sub_118C0 (:16963-77)
        // separates the two probe points: from the pre-step
        // (18444, 26662) the carpet at (18505, 26355) is 307 away
        // against a combined +82 of 44 + 119, a miss; from the
        // stepped (18582, 26307) it is 48 away, a hit. Retail's arrow
        // in fact never connects at all — it outruns the carpet and
        // expires unmoved at t=531. The whole (9,13) residue was
        // pairs of this shape: port dies at t, retail at t+1.
        let hit = self.victim_scan(i, ctx);
        // End of flight (:63806-26): the airborne survival arm is the
        // ONLY step onto tmp. A grounding step kills at the PRE-step
        // pose with the life decrement skipped (the decrement lives
        // inside the airborne branch, on the pre-decrement test);
        // expiry kills unmoved too; a hit parks the bolt at the
        // victim's aim point (the +76/+78 sub_524C0 bracket).
        if (self.ground_z(tmp.0, tmp.1) as i16) <= tmp.2 {
            let was = self.ent[i].act_life;
            self.ent[i].act_life = was - 1;
            if was != 0 && hit.is_none() {
                self.move_relink(i, tmp.0, tmp.1, tmp.2);
                return false;
            }
        }
        match hit {
            Some(MailTarget::Pool(j)) => {
                let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                self.move_relink(i, jx, jy, jz);
            }
            Some(MailTarget::Player(hx)) => {
                let (px, py, pz) = self.seat_at(ctx, hx);
                self.move_relink(i, px, py, pz.wrapping_add(PLAYER_HH as i16));
            }
            None => {}
        }
        let amt = self.ent[i].f44 as u32;
        self.area_write(i, 0, amt, ctx, false, false);
        self.ent[i].flags |= 0x400;
        false
    }

    /// The player-spell payload flight. APPROX(original: c9 m1/m2/m4/
    /// m5/m7/m11/m17 have their own states past remc1's transcribed
    /// table): m13-bolt-shaped straight flight at the cast pitch (the
    /// down-arc arrives via the cast's pitch bias); on any end
    /// (victim / ground / expiry) the per-model payload fires.
    ///
    /// ⭐ THE STATES ARE NOW NAMED, NOT GUESSED. The class-9 dispatch
    /// table decoded out of the shipped EXEs (CARPET.EXE file
    /// 0x9CF34 / HIDDEN.EXE 0x9D134; 21 rows 0x00-0x14, where remc1's
    /// `str_25573C` :4838 stops at 0x0D) binds every state this arm
    /// serves to the BARE GENERIC `sub_53060` -> `sub_52770`:
    ///     02 53060 · 04 53060 · 05 53060 · 06 53060 · 0B 53060
    ///     0F 53060 · 10 53060 · 11 53060(CARPET)/44600(HIDDEN) · 14 53060
    /// — with state 07 alone going to `sub_530B0` -> `sub_530C0`
    /// (which state 08 shares, and which the port already runs as
    /// `proj_m8_tick`). So the faithful end state is to delete this
    /// function and route 2/4/5/6/0xB/0x10/0x14 to
    /// `proj_generic_tick` and 7 to `proj_m8_tick`. It is BANKED, not
    /// done: it swaps `spell_payload`'s per-model switch for the
    /// generic `+68/+69` explode, which the MC1 cast arms never stamp
    /// (retail :65465-77 does), and adds the deflection block to
    /// every payload lob — high blast radius across mc1l42/l3/l4/l5.
    /// The end bracket below is retail's own, verbatim.
    fn proj_payload_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // These states run the engine's generic homing flight
        // (sub_52770), so they carry ITS one-shot prologue too
        // (:62640-60, see [`Gen::proj_generic_tick`]): the acquire is
        // attempted ONCE, latched on flags bit 2, and a hit SNAPS the
        // live heading onto the pick. The sub_54520 subtype switch
        // then picks the candidate set — m4 (volcano) sits in the
        // 0/3/4 creature block; m7/m11 take the significant-list-only
        // block 7/8/B/C (rival carpets, castles, balloons); m2/m5/m17
        // are `default:` and never acquire, taking the miss arm's
        // mirror.
        // Homing runs from the tick after, once +146 holds a target.
        if self.ent[i].f146 == 0 {
            if self.ent[i].flags & 2 == 0 {
                self.ent[i].flags |= 2;
                // sub_54520's entry clamp (:63975-76) sits ABOVE the
                // model switch, so it caps +26 at 16 even on the
                // `default:` models that acquire nothing — the crater
                // bolt carries the caster's charge meter (up to 200)
                // and retail records 16 on every single one. Our
                // clamp lived inside the aim-assist bodies, which
                // m2/m5/m17 never reach.
                if self.ent[i].f26 > 16 {
                    self.ent[i].f26 = 16;
                }
                match self.ent[i].model65 {
                    4 => self.aim_assist(i, ctx),
                    7 | 11 => self.aim_assist_wizards(i, ctx),
                    _ => {}
                }
                if self.ent[i].f146 != 0 {
                    self.ent[i].f30 = self.ent[i].f34;
                    self.ent[i].f32 = self.ent[i].f36;
                } else {
                    self.ent[i].f34 = self.ent[i].f30;
                    self.ent[i].f36 = self.ent[i].f32;
                }
            }
        } else {
            self.home(i, ctx);
        }
        // The launch-boost servo (sub_52770's own :63565-67, the twin
        // the castle ball already runs): +126 walks 2/tick toward the
        // ctor +128, so a lob launched at carpet speed accelerates
        // back to its row base over the flight. The cast arm bumps
        // +126 alone, never +128 (see `World::cast_projectile`).
        // mc1l42 slot 259 pins the cadence exactly: 306, 308, 310 …
        // 324 across the crater bolt's nine airborne ticks.
        {
            let e = &mut self.ent[i];
            e.f126 += flight_speed_step(e.f128 - e.f126);
        }
        let mut tmp = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        let hit = self.victim_scan_at(i, tmp, ctx);
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        let grounded = ground > tmp.2;
        // The grounding step is NOT clamped to the terrain — the bolt
        // keeps the z its polar step put it at, under the ground, and
        // the payload detonates from there (the castle ball's
        // recorded "z 7344 under ground 7808", docs/DEVIATIONS.md).
        // mc1l42 t=20159: retail's crater bolt ends at 5596 with the
        // ground at 5664, and its (10,11) inherits that buried z.
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        // The end test is retail's own three-armed bracket
        // (sub_52770 :62678-702 / CARPET.EXE 0x52a2d-0x52a8c), not a
        // short-circuited disjunction. The whole no-victim block sits
        // under `if (!v5)`, so BOTH the life countdown and the water
        // splash are unreachable on a strike tick:
        //
        //   no victim, airborne  -> --life, survive while >= 0
        //   no victim, grounded  -> model 4 detonates; else a type-0
        //                           tile DROWNS the bolt (splash, no
        //                           detonation), any other explodes
        //   victim               -> PARK on the victim's aim point,
        //                           then explode there
        //
        // The life countdown running on AIRBORNE ticks only is the
        // castle ball's :63586-90 twin, so a touchdown is recorded one
        // tick "younger" (mc1l42 t=20159 life 12, not 11 — every
        // crater detonation in the take reads the same way).
        let detonate = match hit {
            None => {
                if !grounded {
                    self.ent[i].act_life -= 1;
                    self.ent[i].act_life < 0
                } else if self.ent[i].model65 != 4 && self.on_water_pub(tmp.0, tmp.1) {
                    // :62690-701 (0x52a44-0x52a7a) — the same water
                    // arm `proj_move_and_hit` and `proj_m8_tick`
                    // already carry, and the generic has no revert:
                    // probe, splash and reap all read the STEPPED
                    // position. The volcano lob (model 4) is exempt
                    // and detonates over water like on land.
                    // mc1l0-sg t=2178/2206: the Undead Army lob
                    // drowns on tile (55,34) and retail records a
                    // lone (10,5) splash where we raised an eight-
                    // skeleton ring off a (10,36) spawner.
                    self.splash_and_die(i);
                    return false;
                } else {
                    true
                }
            }
            Some(v) => {
                // :62751-55 (0x52d42) — `sub_524C0(victim);
                // sub_41C70(bolt, victim+72); sub_524E0(victim)`:
                // the strike PARKS the bolt on the victim's aim point
                // inside the +76/+78 lift bracket, and the explode
                // then mints the child THERE. mc1l0-sg t=1619: the
                // volcano lob strikes building slot 6 (z 544, +78
                // −8192) and both records land at 16640/13312/−7648,
                // exactly `victim.z + victim.f78`. Same idiom as
                // `proj_move_and_hit`'s teleport arm above.
                match v {
                    MailTarget::Pool(j) => {
                        let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                        self.move_relink(i, jx, jy, jz);
                    }
                    MailTarget::Player(hx) => {
                        let (px, py, pz) = self.seat_at(ctx, hx);
                        self.move_relink(i, px, py, pz.wrapping_add(PLAYER_HH as i16));
                    }
                }
                true
            }
        };
        if detonate {
            // ⭐ THE REAP IS GATED ON THE CHILD. `sub_52770`'s explode
            // block (:62757-72) is `v19 = sub_373F0_377B0(a1+72,
            // a1->+68, a1->+69); if (v19) { … sub_41E80_421C0(a1); }`
            // — the soft kill is the LAST statement INSIDE the null
            // check, so a detonation whose child allocation FAILS
            // leaves the bolt alive and it retries on the next tick.
            // WITNESS mc1l49 t=54341: the pool is dry (free 0 /
            // recycle 0 for three ticks), the human's volcano lob at
            // slot 857 parks on rival castle 940 at (0, 16384, 6048)
            // and retail leaves `flags 0x2006` standing through 54341
            // AND 54342, raising `0x400` only at t=54343 — the first
            // tick a slot frees, where the `(10,9)` hill is minted at
            // slot 363. The port reaped on the parking tick.
            // `MGC_NO_MC1_PAYLOAD_CHILD_REAP_GATE=1` restores it.
            // NO damage mail. Retail's payload strike writes nothing
            // to the victim: sub_52770's whole victim arm (:62705-55)
            // only parks the bolt, and the one call it makes past the
            // explode — sub_526C0 (:62585-614) — bumps the caster's
            // shot/hit COUNTERS (+343/+347) and nothing else. The
            // damage rides the CHILD, whose +44 is the bolt's.
            // mc1l0-sg t=1619: retail's slot 6 keeps mail 0 and life
            // 1104 through t=1620, then takes the (10,9) child's
            // 1000/tick from t=1621 (1104 -> 104 -> -896). Our mail
            // spent that 1000 a tick early and then again per child.
            let minted = self.spell_payload(i, hit);
            if minted || crate::engine::features::no_mc1_payload_child_reap_gate() {
                self.ent[i].flags |= 0x400;
            }
        }
        false
    }

    /// The per-model detonation payloads of the player-spell
    /// projectiles (each cite = the traced cast arm's effect).
    /// `hit` is the detonation's own victim probe — the generic
    /// explode stamps it into the child's `+146` unguarded, so it
    /// rides down here too (see the crater arm).
    /// Returns whether the bolt may now be reaped — i.e. whether the
    /// child the arm attempts was actually minted. `sub_52770`'s
    /// explode is ONE call, `sub_373F0_377B0(a1+72, a1->+68, a1->+69)`
    /// (:62757), and every stamp AND the closing `sub_41E80_421C0(a1)`
    /// soft kill (:62772) sit INSIDE its `if (result)` — see
    /// [`crate::engine::features::no_mc1_payload_child_reap_gate`]. An
    /// arm that attempts no spawn (the duel dart's miss, :63208-09, and
    /// the models with no child at all) returns `true`: retail kills
    /// those unconditionally.
    fn spell_payload(&mut self, i: usize, hit: Option<MailTarget>) -> bool {
        let (x, y, z, model) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.model65)
        };
        let own = self.ent[i].id24;
        match model {
            // Earthquake (:65314): the authentic (10,15) crevice
            // walker — random start heading off its own LCG, ±45
            // wander, a 10-tick m11 digger per step (the rumble is
            // the diggers' loop-10). The child is a GENERIC-EXPLODE
            // child like the crater's (:62759-71): laid at the
            // BOLT's own axis (z included), owner + heading + pitch
            // + `+44` copied off the bolt, `+146` the unguarded
            // pointer diff (the miss stamp on a ground death).
            // mc1l0-sg t=1080/1133: the bare-at-ground mint dropped
            // every one of those lanes at once.
            2 => {
                let (yaw, pitch, bolt_f44) = {
                    let e = &self.ent[i];
                    (e.f30, e.f32, e.f44)
                };
                let Some(w) = self.spawn_creator(15, x, y, z) else {
                    return false;
                };
                {
                    let e = &mut self.ent[w];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f32 = pitch;
                    e.f44 = bolt_f44;
                    e.f146 = match hit {
                        Some(MailTarget::Pool(j)) => j as u16,
                        Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                        None => MC1_MISS_STAMP,
                    };
                }
            }
            // Volcano (:65432): the growing hill + pit IS the
            // authentic model (trace :65466, effect c10 m9); the
            // finished cone spawns the model-18 eruption driver
            // ([`Gen::eruption_tick`]). Same generic-explode child
            // stamps as arm 2 (mc1l0-sg t=1176/1483).
            4 => {
                let (yaw, pitch, bolt_f44) = {
                    let e = &self.ent[i];
                    (e.f30, e.f32, e.f44)
                };
                let Some(h) = self.spawn_creator(9, x, y, z) else {
                    return false;
                };
                {
                    let e = &mut self.ent[h];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f32 = pitch;
                    e.f44 = bolt_f44;
                    e.f146 = match hit {
                        Some(MailTarget::Pool(j)) => j as u16,
                        Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                        None => MC1_MISS_STAMP,
                    };
                }
            }
            // Crater (:65491): the expanding bowl (authentic:
            // effect c10 m11). The detonation is the generic explode
            // shape — the child is laid at the BOLT's own axis (all
            // three components: the buried grounding z, not the
            // terrain under it) and carries the bolt's owner, heading
            // and pitch, exactly as `proj_explode`'s children do.
            // mc1l42's thirteen craters read the pair off retail:
            // heading/pitch = the bolt's +30/+32 on every one, z =
            // the bolt's own (5596 under ground 5664 at t=20159).
            5 => {
                let (yaw, pitch) = {
                    let e = &self.ent[i];
                    (e.f30, e.f32)
                };
                // ...and the child's `+146` too. The explode's stamp
                // is the raw unguarded pointer difference (:63428 /
                // :63778), so a detonation that probed NOTHING —
                // which every lobbed crater does, it dies on the
                // ground — records the link-time constant
                // [`MC1_MISS_STAMP`] rather than 0. mc1l42 reads
                // 64608 on all THIRTEEN craters; the lane is the
                // graded obs `chase`, so leaving it at 0 was a floor
                // under the certified run.
                // ⭐ AND THE CHILD INHERITS THE BOLT'S `+44`. It is the
                // last line of the same five-line explode block the
                // `+146` stamp above comes from (`v20[22] = *(a1+44)`,
                // :62770 / :63201), so the effect ctor's own potency is
                // OVERWRITTEN by the casting spell's damage: the crater
                // bowl's ctor writes 200 (sub_3A9A0 :46775) and every
                // player-cast crater then runs on Crater's 6000
                // (SPELLS[9].damage). The digger broadcasts +44 whole
                // on its FIRST tick and +44/25 thereafter
                // (`Gen::tick_digger`), so the port's un-inherited 200
                // paid 200 + 8/tick where retail pays 6000 + 240/tick —
                // mc1l42 t=20161: nine griffons carry retail mail
                // (6000, 331) and land on life 4000, the port's on
                // 9800. Ungraded (the obs carries no +44), so it read
                // clean in pair mode for the whole campaign and only
                // ever broke the free run.
                let bolt_f44 = self.ent[i].f44;
                let Some(c) = self.spawn_creator(11, x, y, z) else {
                    return false;
                };
                {
                    let e = &mut self.ent[c];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f32 = pitch;
                    e.f44 = bolt_f44;
                    e.f146 = match hit {
                        Some(MailTarget::Pool(j)) => j as u16,
                        Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                        None => MC1_MISS_STAMP,
                    };
                }
            }
            // Duel to the Death — the DART's own detonation tail,
            // `sub_530C0_53400` :63186-63210. remc1's class-9 table
            // row 0x0007 (:4846) is `sub_530B0` (:63042), a one-line
            // `return sub_530C0_53400(a1)` — state 7 IS state 8, the
            // steal ball's handler (see [`Gen::proj_payload_tick`]'s
            // banked note), and this is its LABEL_25_26 fork:
            //
            // ```c
            // if (v6 && v6->class == 3 && v6->model <= 1) {   // :63188
            //   v19 = spawn(a1+72, a1->+68, a1->+69);         // (10,26)
            //   if (v19) { sub_526C0(a1, v6, ..);
            //     v19[12] = a1->+24; v19[15] = a1->+30;       // :63198-99
            //     v19[16] = a1->+32; v19[73] = victim_slot;   // :63200-01
            //     v19[22] = a1->+44; kill(a1); }              // :63202-03
            // } else { sub_526C0(a1, 0, ..); kill(a1); }      // :63208-09
            // ```
            //
            // ⭐⭐ THREE DEPARTURES, ALL IN THOSE TEN LINES.
            // (1) **The test is on the entity STRUCK THIS TICK** —
            //     `v6` is `sub_11980`'s probe result (:63105), not the
            //     homing lock `+146`. A dart locked onto a wizard that
            //     stops on a creature or on the ground is a MISS.
            // (2) **The miss arm spawns NOTHING.** `sub_526C0_52A00(a1,
            //     0, ..)` then `sub_41E80` — there is no `+68/+69`
            //     call on that branch, so the port's `(10,23)` hit
            //     flash was an entity retail never mints (mc1l48
            //     t=62999 records the port's extra one, slot 630).
            //     Its MC2 twin is traced identically in
            //     docs/spell-audit/duel.md ("a duel dart that misses,
            //     expires or stops on terrain leaves nothing at all").
            // (3) The child takes the DART's `+30`/`+32`/`+146`/`+44`
            //     — the ctor's 200 is overwritten by `v19[22]`.
            //     mc1l48 t=59118 slot 20 records `f30 1469, f32 71,
            //     f146 712, f44 100` on the tether's birth tick, and
            //     100 is the dart's own `+44`.
            7 if !crate::engine::world::mc1_duel_dart_exact_off() => {
                let struck = match hit {
                    Some(MailTarget::Pool(j)) => Some(j as u16),
                    // The out-of-pool carpet: retail's `v6` is his
                    // class-3 model-0 record, so he passes :63188.
                    Some(MailTarget::Player(hx)) => Some(self.human_tag[hx].0),
                    None => None,
                };
                let is_wizard = match hit {
                    Some(MailTarget::Pool(j)) => {
                        self.ent[j].class64 == 3 && self.ent[j].model65 <= 1
                    }
                    Some(MailTarget::Player(_)) => true,
                    None => false,
                };
                if is_wizard {
                    let (yaw, pitch, bolt_f44) = {
                        let e = &self.ent[i];
                        (e.f30, e.f32, e.f44)
                    };
                    let Some(t) = self.spawn_effect(26, x, y, z) else {
                        return false;
                    };
                    // :63195 — `sub_526C0(a1, v6, v21)`, first
                    // statement inside the allocation guard (mc1l48
                    // t=59118: `shots` 2528 -> 2529, `hits` 279 -> 280
                    // on the tether's birth tick).
                    if self.is_human_tag(own) && !no_mc1_silent_end_shot() {
                        self.mc1_shot_stats(i, hit);
                    }
                    {
                        let e = &mut self.ent[t];
                        e.id24 = own;
                        e.f30 = yaw;
                        e.f32 = pitch;
                        e.f146 = struck.unwrap_or(0);
                        e.f44 = bolt_f44;
                    }
                } else {
                    // :63208 — the miss arm: a shot, no spawn.
                    self.mc1_shot_stats_silent(i);
                }
            }
            // The pre-dig arm, kept under
            // `MGC_NO_MC1_DUEL_DART_EXACT=1`.
            7 => {
                let victim = self.ent[i].f146;
                let is_wizard = self.is_human_tag(victim)
                    || (victim != 0
                        && self.ent[victim as usize].class64 == 3
                        && self.ent[victim as usize].model65 <= 1);
                if is_wizard {
                    if let Some(t) = self.spawn_effect(26, x, y, z) {
                        self.ent[t].id24 = own;
                        self.ent[t].f146 = victim;
                        self.ent[t].f44 = 200;
                    }
                } else if let Some(f) = self.spawn_effect(23, x, y, z) {
                    self.ent[f].id24 = own;
                }
            }
            // Undead Army: the bolt's detonation does NOT raise the
            // ring itself. Its `+68/+69` = 10/36 (the cast at :65959-60)
            // makes the generic-explode child the (10,36) UNDEAD-ARMY
            // SPAWNER `sub_3B3E0` (:47370), and the eight skeletons are
            // raised by THAT record's own state-38 tick
            // ([`Gen::undead_army_tick`], sub_26E90 :29353) later in the
            // same slot walk. The distinction is not cosmetic: the
            // spawner takes a pool slot of its own, so an inline ring
            // hands the FIRST skeleton the free-stack pop that belongs
            // to the spawner and shifts every later one — mc1hwl0
            // t=47080 raised its eight on 967/950/934/753/478/476/372/12
            // where retail spends 967 on the (10,36) and runs the ring
            // 950…12 + 937. The child stamps are the generic explode's
            // (:62763-70), the same set arms 2 and 4 carry.
            11 => {
                let (yaw, pitch, bolt_f44) = {
                    let e = &self.ent[i];
                    (e.f30, e.f32, e.f44)
                };
                let Some(s) = self.spawn_effect(36, x, y, z) else {
                    return false;
                };
                {
                    let e = &mut self.ent[s];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f32 = pitch;
                    e.f44 = bolt_f44;
                    e.f146 = match hit {
                        Some(MailTarget::Pool(j)) => j as u16,
                        Some(MailTarget::Player(hx)) => self.human_tag[hx].0,
                        None => MC1_MISS_STAMP,
                    };
                }
            }
            _ => {}
        }
        true
    }

    /// sub_26E90 (:29353), class-10 state 38 — the UNDEAD ARMY.
    /// The spawner overwrites its own `+44` with 10000 (the mana
    /// purse it hands out), sizes the ring from the FREE POOL
    /// (`sub_37710` :44061 = `free.len()` — see
    /// [`undead_ring_size_is_the_free_length`], clamped to 8), caps it
    /// at 64 live skeletons per owner, raises them on a 512-unit ring
    /// at `k·2048/N` facing radial+180°, and marks itself dead.
    ///
    /// ⭐ THE RING SIZE IS THE POOL DEPTH. A hardcoded 8 is right only
    /// while the pool is deep; under pressure retail raises fewer, and
    /// the pops it does not take shift every later allocation.
    ///
    /// ⭐ THE LIVE CENSUS WALKS THE MODEL-9 ROSTER CHAIN (:29373 loads
    /// `str_36382x[9]` at wizext 36418 = 36382 + 4·9), and its ONLY
    /// per-member test is `+144 == owner` — liveness is MEMBERSHIP,
    /// sampled at tick top, never re-read. A full-array walk with a
    /// live `flags & 0x400` test disagrees in both directions, and is
    /// blind to the seizure blank.
    ///
    /// ⚠ THE OWNER GOES ON `+144` ALONE. remc1 :29399 writes no `+24`,
    /// and mc1hwl0 t=47080 settles the long-standing "transcription
    /// slip" suspicion beside it: all eight recorded skeletons carry
    /// `+24` = their OWN slot, i.e. NewEvent's default, against the
    /// port's caster id. Recorded gameplay outranks the reading.
    fn undead_army_tick(&mut self, i: usize) -> bool {
        self.ent[i].f44 = 10000;
        // :29367-72 — the pool probe, negative-clamped then capped at
        // 8. (Retail's `(__int16)` narrowings are unreachable at any
        // real pool size; kept as the shape, not as live arithmetic.)
        // `sub_37710_37AD0` is `top_index + 1` = the stack's LENGTH;
        // the port's own `Vec` already holds that, so there is no
        // second `+ 1` (see `undead_ring_size_is_the_free_length`).
        let mut n = self.free.len() as i32 + i32::from(!undead_ring_size_is_the_free_length());
        if n & 0x8000 != 0 {
            n = 0;
        }
        if n > 8 {
            n = 8;
        }
        let own = self.ent[i].id24;
        let live = {
            let chain = self.mob_chains.visible(9);
            chain
                .iter()
                .filter(|&&j| self.ent[j as usize].f144 == own)
                .count() as i32
        };
        if n > 64 - live {
            n = 64 - live;
        }
        if n > 0 {
            let (bx, by, bz) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            };
            // :29384-85 — the purse and its per-skeleton share, both
            // int16. Each skeleton banks the REMAINDER of what is left
            // (`v8 % v11`), so a ring that divides 10000 evenly hands
            // out zero mana all the way round — the "skeletons drop no
            // corpse ball" reading, but as arithmetic, not a gate.
            let mut purse: i16 = 10000;
            let share = 10000i16 / n as i16;
            let step = (2048i32 / n) as u16;
            let mut ang: u16 = 0;
            for _ in 0..n {
                let mut pos = (bx, by, bz);
                Self::polar_step(&mut pos, ang, 0, 512);
                let sz = self.ground_z(pos.0, pos.1) as i16;
                if let Some(s) = self.spawn_creature(9, pos.0, pos.1, sz) {
                    // :29396-98 — the +180° flip is done BYTEWISE
                    // (`HIBYTE += 4 & 7`), which is `(ang + 0x400) &
                    // 0x7FF` for every in-range angle.
                    let facing = (ang & 0xFF) | (((ang >> 8).wrapping_add(4) & 7) << 8);
                    let e = &mut self.ent[s];
                    e.f140 = (purse % share) as i32;
                    e.f144 = own;
                    e.f30 = facing;
                    e.f34 = facing;
                    purse -= share;
                }
                ang = ang.wrapping_add(step);
            }
        }
        // :29411 — the spawner kills itself unconditionally, outside
        // the `+44` gate.
        self.ent[i].flags |= 0x400;
        false
    }

    /// sub_25EC0 (:28731): the volcano eruption driver (m18, state
    /// 18). Counter +26 runs the machine; maxLife (10000) never
    /// counts down:
    /// - counter 0: eruption start — always activates, registers as
    ///   THE erupting volcano (kicking any previous one to counter
    ///   250), swaps the global (10,19) plume, and fires the
    ///   once-per-eruption blast fireball ((10,17) payload, pitch
    ///   -386, life 1) at the rotating heading (:28778-823).
    /// - counters 1..126: activate at p=1/5, except every 16th tick
    ///   (counter&0xF == 0) which never does (:28768-71). Every
    ///   activation lobs ONE ballistic (10,16) lava bomb and turns
    ///   the heading by 0x500 (:28795-804).
    /// - an activation at 127 is the CLEAN death: clears the global
    ///   register (:28825-29). Missing that 1/5 roll leaves the
    ///   register pointing at a dead-idle volcano — the authentic
    ///   no-more-eruptions-anywhere quirk.
    /// - counter > 2500: dormant; p=1/100 per tick to re-arm to 0,
    ///   only while NO volcano is registered (:28750-66).
    /// - every activation (and every re-arm) dies instead if the
    ///   ground height under the driver changed (:28773-77).
    ///
    /// No driver-level sound: eruption audio = the bombs' seeded
    /// fires (crackle 3) + the blast ring (30).
    fn eruption_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let c = self.ent[i].f26;
        let (x, y, z, own) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        // ⭐⭐ THE RE-ARM IS NOT A TICK OF ITS OWN. Retail's dormant
        // block (:28749-64) has NO return: it clears `+26` and falls
        // straight into the activation test below, whose `|| !+26`
        // arm is now TRUE — so the tick that wakes a volcano is also
        // the tick that erupts. Ours returned after the re-arm and
        // paid for the whole eruption one tick later, on a driver LCG
        // that had already moved on, which is a divergence that never
        // heals: mc1hwl0 t=35674, driver slot 754 at f26 2597, retail
        // borns the plume (10,19) at 518, the lava bomb (10,16) at
        // 452 and the blast (9,0) at 364 while ours borns nothing and
        // leaves `+30` 0x500 short (38400 vs 39680).
        //
        // The counter is likewise ONE unconditional `++` at the very
        // bottom (:28831), reached by every path but the two deaths —
        // not a per-branch write, and not clamped. The old
        // `f26 < i16::MAX - 1` guard was invented; retail lets the
        // i16 wrap, which is what eventually drags an orphaned
        // register's volcano (see the missed-1/5-roll quirk above)
        // back under 128 and starts it lobbing again.
        if c > 2500 {
            let d = self.ent_rand(i);
            if d % 100 == 0 && self.erupting == 0 {
                // :28756-58 — the probe WRITES the sample into `+76`
                // and only then compares, so a volcano that dies here
                // dies already snapped to the new ground.
                let g = self.ground_z(x, y) as i16;
                self.ent[i].z = g;
                if g != z {
                    self.ent[i].flags |= 0x400;
                    return false;
                }
                self.ent[i].f26 = 0;
            }
        }
        // Re-read: the block above may have just re-armed us.
        let c = self.ent[i].f26;
        let fire = if c != 0 && c < 128 && c & 0xF != 0 {
            self.ent_rand(i) % 5 == 0
        } else {
            c == 0
        };
        if fire {
            // :28770-76 — the same write-then-compare probe.
            let g = self.ground_z(x, y) as i16;
            self.ent[i].z = g;
            if g != z {
                self.ent[i].flags |= 0x400; // deformed under: dead
                return false;
            }
            if c == 0 {
                // Register self; kick the previous eruption (:28778-92).
                //
                // ⚠ BOTH REGISTER WRITES ARE BLIND. Retail's only
                // gate on either is `slot != 0` (:28779 and :28791
                // are the same `> pool base` pointer test) — no
                // class, no model, no life — and it stamps whatever
                // record now occupies that slot. The port's
                // `(10,18)` / `(10,19)` conjuncts were invented, and
                // the plume one cost mc1hwl0 its t=35674 head: the
                // previous plume's slot 404 had been reaped and
                // re-minted as a `(10,0)` FIRE, and retail
                // reap-flags the fire (flags 196742 → 197766) where
                // the port left it burning. Same freed-slot
                // stale-bytes law as the pack-death handoff through
                // `+52`; the `!= 0` bound is memory safety only.
                let prev = self.erupting as usize;
                // PATCH `volcano_register_revalidate` (retail bug, see
                // the patch doc): both register writes land only on the
                // record the register was meant to name. Retail's blind
                // kick turns a re-minted CASTLE into a level-250 castle
                // whose downgrade ladder reads the build table out of
                // bounds and hangs the game at level 245 (mc1l26-froze,
                // mc1l45-froze), and a stale register naming THIS slot
                // self-kicks the new driver (mc1l49 t=29062); the blind
                // plume kill below soft-kills whatever inherited the old
                // plume's slot (mc1l45: a rival's Fireball token).
                let revalidate = ctx.patches.volcano_register_revalidate
                    && (!ctx.strict || crate::engine::features::force_volcano_guard())
                    // …or the take itself was recorded on the guarded
                    // binary and says so (round 166).
                    || crate::engine::features::take_volcano_guard();
                let kick_ok = !revalidate
                    || prev != i
                        && prev < self.ent.len()
                        && self.ent[prev].class64 == 10
                        && self.ent[prev].model65 == 18;
                let plume_ok = |g: &Self, pl: usize| {
                    !revalidate || g.ent[pl].class64 == 10 && g.ent[pl].model65 == 19
                };
                if prev != 0 && prev < self.ent.len() && kick_ok {
                    // The kick is a RAW `+26` store (file 0x3E7B6
                    // `66 c7 42 1a fa 00`). On a live class-12 token
                    // the port homes retail's `+48` burst counter in
                    // `f26` — retail's `+26` there is the SPELL LEVEL,
                    // a lane the port does not model — so the store
                    // must not land in `f26`. See
                    // [`volcano_kick_spares_token_burst`].
                    if !(self.ent[prev].class64 == 12 && volcano_kick_spares_token_burst()) {
                        self.ent[prev].f26 = 250;
                    }
                }
                self.erupting = i as u16;
                let g = self.ground_z(x, y) as i16;
                if plume_handover_is_guarded() {
                    // ⭐⭐⭐ :28782-93 — THE HANDOVER IS INSIDE THE
                    // SPAWN'S NULL GUARD. Retail mints the new
                    // `(10,19)` first and only `if (v8)` does it stamp
                    // `+24`, soft-kill the old `+38` and publish the
                    // new one. A refusal (free stack AND recycle stack
                    // dry) therefore kills NOBODY and leaves `+38`
                    // naming the old slot — the register is never
                    // zeroed. See [`plume_handover_is_guarded`].
                    if let Some(p) = self.spawn_effect(19, x, y, g) {
                        self.ent[p].id24 = own;
                        let pl = self.plume as usize;
                        if pl != 0 && pl < self.ent.len() && plume_ok(self, pl) {
                            self.ent[pl].flags |= 0x400;
                            self.volcano_blind.0.push(pl as u16);
                        }
                        self.plume = p as u16;
                    }
                } else {
                    let pl = self.plume as usize;
                    if pl != 0 && pl < self.ent.len() && plume_ok(self, pl) {
                        self.ent[pl].flags |= 0x400;
                        self.volcano_blind.0.push(pl as u16);
                    }
                    self.plume = match self.spawn_effect(19, x, y, g) {
                        Some(p) => {
                            self.ent[p].id24 = own;
                            p as u16
                        }
                        None => 0,
                    };
                }
            }
            // One ballistic lava bomb per activation (:28795-802):
            // the ctor's own three draws ride the BOMB's fresh LCG,
            // then ONE step of the DRIVER's LCG lands in BOTH records
            // (:28800-02 — the seed passes on STEPPED, and only when
            // the spawn succeeded).
            if let Some(b) = self.spawn_lava_bomb(x, y) {
                self.ent[b].id24 = own;
                let v = self.ent_rand(i);
                self.ent[b].rand = v;
            }
            // Heading advances 0x500 per activation (:28804).
            self.ent[i].f30 = self.ent[i].f30.wrapping_add(0x500);
            // ⭐ RETAIL RE-READS `+26` FROM MEMORY HERE, IT DOES NOT
            // REUSE THE ENTRY VALUE (:28803 `v13 = *(a1+26)`;
            // CARPET.EXE `0x3E88A: 66 8b 7b 1a` = `mov 0x1a(%ebx),%di`,
            // then `0x3E891 test %di,%di` / `0x3E894 75 77 jne 0x3E90D`
            // — the blast is SKIPPED straight to the death check).
            // It matters because the eruption-start block's register
            // kick `*(prev+26) = 250` (`0x3E7B6: 66 c7 42 1a fa 00`) is
            // guarded ONLY by `slot != 0` (`0x3E7B4: 76 06`), so when
            // the stale global `erupting` register names the very pool
            // slot just recycled into THIS driver, retail SELF-KICKS
            // its own `+26` to 250 — and then mints no blast, reaps
            // itself on its first tick, and clears the register it set
            // two instructions earlier. WITNESS mc1l49 t=29062:
            // `erupting=968` is a long-stale reap-flagged (10,6)
            // (`act_life −2`); the pool recycles 968 into the new
            // (10,18) driver and retail records `f26 0 → 251`,
            // `flags → 1024`, popping ONE slot all tick. The port
            // cached `c` at entry, minted a spurious (9,0), and pushed
            // retail's (10,13) from slot 168 to 169.
            let c = if no_mc1_eruption_counter_reread() {
                c
            } else {
                self.ent[i].f26
            };
            if c == 0 {
                // The eruption-start blast fireball (:28805-23):
                // owner, the driver's heading (high byte & 7), pitch
                // RAW −386 (:28814 — the u16 65150; every consumer
                // masks on read), life 1, the (10,17) fire-field
                // detonation, and the +150 DESTINATION = the driver's
                // position stepped 1536 along the launch yaw with the
                // ground sampled under it (:28819-23). `+34`/`+36`
                // and the acquire latch stay NewEvent-clear — the
                // ball's own first tick runs the one-shot acquire
                // like any fireball (mc1l3 t=2304: the pair graded
                // the old pre-aimed, pre-latched, masked-pitch mint).
                let yaw = self.ent[i].f30 & 0x7FF;
                let mut dest = (x, y, z);
                Self::polar_step(&mut dest, yaw, 0, 1536);
                let dg = self.ground_z(dest.0, dest.1) as i16;
                if let Some(p) = self.spawn_fireball(x, y, z) {
                    let e = &mut self.ent[p];
                    e.id24 = own;
                    e.f30 = yaw;
                    e.f32 = -386i16 as u16;
                    e.f68 = 10;
                    e.f69 = 17;
                    e.act_life = 1;
                    e.dest_x = dest.0;
                    e.dest_y = dest.1;
                    e.site_z = dg;
                }
            }
            // :28825 / `0x3E90D: 66 83 7b 1a 7f` = `cmpw $0x7f,
            // 0x1a(%ebx)` — a FRESH memory read, not the cached entry
            // value (`c` above is already the re-read).
            if c >= 127 {
                self.erupting = 0; // the clean death (:28825-29)
                self.ent[i].flags |= 0x400;
                // …and STILL falls to the counter below (:28831 sits
                // outside the fire block): the reap-flagged driver
                // records `+26` one higher.
            }
        }
        // :28831 `++*(a1+26)` / `0x3E928: 66 ff 43 1a` = `incw
        // 0x1a(%ebx)` — an IN-MEMORY increment, not `entry_c + 1`.
        self.ent[i].f26 = if no_mc1_eruption_counter_reread() {
            c.wrapping_add(1)
        } else {
            self.ent[i].f26.wrapping_add(1)
        };
        false
    }

    /// sub_3ACC0 (:46958): the (10,16) lava bomb — draws IN ORDER
    /// off its own LCG: life = %100+100, speed = %50 (held), vz =
    /// 256 up, yaw = rand & 0x7FF; speed applies as +52; spawned
    /// map-linked at ground+64 with the horizontal velocity vector
    /// pre-advanced into +150/+152 (our dest_x/dest_y), sprite 210.
    pub(crate) fn spawn_lava_bomb(&mut self, x: u16, y: u16) -> Option<usize> {
        let b = self.new_event()?;
        {
            let e = &mut self.ent[b];
            e.class64 = 10;
            e.model65 = 16;
            e.tick70 = 16;
            e.f44 = 200;
            e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
            let d1 = lcg32(&mut e.rand);
            e.max_life = d1 % 0x64 + 100;
            let d2 = lcg32(&mut e.rand);
            e.f46 = 256;
            let d3 = lcg32(&mut e.rand);
            e.f30 = (d3 & 0x7FF) as u16;
            e.f126 = (d2 % 0x32) as i16 + 52;
        }
        let gz = (self.ground_z(x, y) + 64) as i16;
        self.link(b, x, y, gz);
        {
            let (yaw, speed) = (self.ent[b].f30, self.ent[b].f126);
            let mut v = (0u16, 0u16, 0i16);
            Self::polar_step(&mut v, yaw, 0, speed);
            let e = &mut self.ent[b];
            e.dest_x = v.0;
            e.dest_y = v.1;
        }
        self.refill_life(b);
        self.set_sprite(b, 210);
        Some(b)
    }

    /// sub_25A60 (:28573): the lava bomb's ballistic flight —
    /// per-axis velocity clamp ±80, gravity -28/tick (vz clamped
    /// [-384, 256]), ground bounce vz = -vz/4, water splash, and at
    /// rest a 30-tick standing fire at 3x damage (if none already
    /// burns on the cell), then downhill roll under 250/256
    /// friction. Slope roll APPROX: central-difference gradient in
    /// place of sub_41F50's table.
    fn lava_bomb_tick(&mut self, i: usize) -> bool {
        // :28592-94 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28597-99 — the family's `& 2` latch, set with no other
        // effect on this model (a pure mark, but a GRADED lane).
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
        }
        // :28600-07 — the held velocity clamps IN PLACE, before the
        // step (the downhill add below stores UNclamped; this is
        // where it comes back into range).
        let vx = (self.ent[i].dest_x as i16).clamp(-80, 80);
        let vy = (self.ent[i].dest_y as i16).clamp(-80, 80);
        self.ent[i].dest_x = vx as u16;
        self.ent[i].dest_y = vy as u16;
        // :28612-19 — the RAW vz applies to z FIRST, THEN decays 28
        // with the [-384, 256] clamp (the port had the order flipped:
        // one whole decay early, 28 units short on every flight tick).
        let (x0, y0, z0) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let x = x0.wrapping_add(vx as u16);
        let y = y0.wrapping_add(vy as u16);
        let mut z = z0.wrapping_add(self.ent[i].f46);
        self.ent[i].f46 = (self.ent[i].f46 - 28).clamp(-384, 256);
        let g = self.ground_z(x, y) as i16;
        if g > z {
            z = g;
            // The bounce reads the DECAYED value (:28626-27),
            // quarter toward zero.
            self.ent[i].f46 = -(self.ent[i].f46 / 4);
            // :28628 — the water probe runs at the OLD position
            // (`sub_11810(a1 + 72)`), the splash spawns at the new —
            // and the kill lands ONLY if the pool granted the splash
            // (:28630-33: the `if (v9)` wraps both writes; a refused
            // splash leaves the bomb flying). No return either way:
            // the splash arm falls through to the +26 increment, the
            // relink and the grounded roll like every other arm —
            // retail never re-tests its own life after a kill (mc1l3
            // t=2416 slot 624: the dying bomb still counts f26 2→3;
            // 11 pair rows, one per drowned bomb).
            if self.on_water_pub(x0, y0) {
                let own = self.ent[i].id24;
                if let Some(s) = self.spawn_effect(5, x, y, z) {
                    self.ent[s].id24 = own;
                    self.ent[i].flags |= 0x400;
                }
            } else {
                // Seed a standing fire on EVERY unburnt ground contact
                // (:28637-47) — not only at rest: life 30, damage 3× the
                // FRESH fire's own ctor +44 (50 → 150; retail reads the
                // just-spawned fire's +44 at :28642, NOT the bomb's 200,
                // a dead store) — and the SEED RESETS the bomb's +26.
                // The existence probe is `sub_11E50` (:17179): the 2×2
                // RECENTRED window (`(pos−128)>>8` and +1 on both axes),
                // class/model match AND 3-D distance ≤ 128 — with NO
                // 0x400 test, a soft-killed fire still counts (mc1l3
                // t=2341: the single-tile, 0x400-excluded probe seeded
                // an extra fire one cell over from a live one).
                let mut burning = false;
                let px = (x.wrapping_sub(128) >> 8) as u8;
                let py = (y.wrapping_sub(128) >> 8) as u8;
                'probe: for dy in 0..2u8 {
                    for dx in 0..2u8 {
                        let mut j = self.map_entity[tile(px.wrapping_add(dx), py.wrapping_add(dy))]
                            as usize;
                        while j != 0 {
                            let c = &self.ent[j];
                            if c.class64 == 10
                                && c.model65 == 6
                                && Self::dist3d(x, y, z, c.x, c.y, c.z) <= 0x80
                            {
                                burning = true;
                                break 'probe;
                            }
                            j = c.next20 as usize;
                        }
                    }
                }
                if !burning {
                    let own = self.ent[i].id24;
                    if let Some(f) = self.spawn_effect(6, x, y, z) {
                        self.ent[f].id24 = own;
                        self.ent[f].act_life = 30;
                        self.ent[f].f44 *= 3;
                        self.ent[i].f26 = 0;
                    }
                }
                // :28650-51 — rest is the SIGNED test: any bounce at or
                // below +28 (including every downward value) parks vz.
                // Land contacts only — the splash arm has no rest park.
                if self.ent[i].f46 <= 28 {
                    self.ent[i].f46 = 0;
                }
            }
        }
        self.ent[i].f26 += 1;
        self.move_relink(i, x, y, z);
        if g == z {
            // Downhill roll (:28655-67): the 2x2 CELL-CORNER raw
            // heightmap differential (`sub_41F50` :52547 — byte
            // units, no scaling) adds straight into the held
            // velocity, then 250/256 friction rounding toward zero;
            // the result stores UNclamped (next tick's top clamp
            // brings it back).
            let (tx, ty) = ((x >> 8) as u8, (y >> 8) as u8);
            let h = |dx: u8, dy: u8| {
                self.t.height[tile(tx.wrapping_add(dx), ty.wrapping_add(dy))] as i16
            };
            let sx = (h(0, 0) + h(0, 1)) - (h(1, 0) + h(1, 1));
            let sy = (h(0, 0) + h(1, 0)) - (h(0, 1) + h(1, 1));
            let nvx = (self.ent[i].dest_x as i16).wrapping_add(sx);
            let nvy = (self.ent[i].dest_y as i16).wrapping_add(sy);
            self.ent[i].dest_x = ((250 * nvx as i32) / 256) as i16 as u16;
            self.ent[i].dest_y = ((250 * nvy as i32) / 256) as i16 as u16;
        }
        false
    }

    /// sub_26140 (:28834), class-10 state 19 — the (10,19) eruption
    /// plume, a 240-tick emitter riding the crater. TRACED (it was
    /// "untraced: life countdown + animation only"): pre-decrement life
    /// like the whole class-10 family, then a per-tick SMOKE SPRAY —
    /// the radius-0 ring (the 2x2 recentre block, 3 cells after the
    /// iterator's dropped-last quirk), each cell rolling the same
    /// ~50% skip test the fire spreader uses and, on a pass, a ±64
    /// jitter pair; on ODD post-decrement life ticks each passing cell
    /// emits FOUR (10,13) puffs at yaws {v, v+0x200, v+0x400, v+0x600}
    /// where v alternates 0/0x100 every other pair of ticks, so the
    /// column corkscrews. The plume then re-seats on the ground.
    /// Retail runs NO animation step here (sprite 228 is static) and
    /// the retail rand cadence is 3/5/7/9 draws a tick — the port drew
    /// zero, which is the whole (10,19) `rand` column in mc1hwl0.
    ///
    /// Retail closes the handler with an UNCONDITIONAL
    /// `sub_120B0(a1, 0, +44)` — a 200 ch0 write per tick over the
    /// ctor's 512 extents, i.e. the plume is a damage field for its
    /// whole 240-tick life, and the write runs on the death tick too
    /// (the call sits after the free, outside the life branch).
    /// Witness: mc1l5 t=16212 — the standing volcano's plume overlaps
    /// Vodor at ~800 units, life 10000 → 9820 (−200 letter + 20
    /// regen), knock 20/396 bearing away from the vent.
    fn plume_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            // ⚠ THE PLUME REGISTER IS NOT CLEARED HERE. Retail's
            // death arm is `sub_41E80_421C0(a1x)` and nothing else
            // (:28891-92) — `+38` keeps naming this slot until the
            // NEXT eruption start overwrites it, straight through the
            // reap and the slot's re-mint. That is what makes the
            // start's blind `+38` reap-flag (see `eruption_tick`)
            // land on a stranger: mc1hwl0 t=35634 reaps plume 404,
            // the pool re-mints 404 as a `(10,0)` fire, and the
            // t=35674 eruption flags the FIRE. Ours cleared the
            // register on death, so the start found 0 and flagged
            // nothing — an UNGRADED global whose only visible
            // consequence is 40 ticks downstream.
            self.ent[i].flags |= 0x400;
        } else {
            self.ent[i].f26 = 0;
            let (x, y, z, owner) = {
                let e = &self.ent[i];
                (e.x, e.y, e.z, e.id24)
            };
            for (dx, dy) in self.ring_cells_pub(0, 0) {
                // The skip test and the jitter pair are the spreader's
                // (:28860-70): the draw order is skip, then jitter ONLY
                // on the spawn branch.
                let s = self.ent_rand(i);
                if s % 0x9D < 79 {
                    continue;
                }
                let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
                let px = x.wrapping_add((192 * dx as i32 + j1 - 96) as u16);
                let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
                let py = y.wrapping_add((192 * dy as i32 + j2 - 96) as u16);
                if self.ent[i].act_life & 1 == 0 {
                    continue;
                }
                let mut yaw = (((self.ent[i].act_life / 2) & 1) << 8) as u16;
                while yaw < 0x800 {
                    if let Some(p) = self.spawn_effect(13, px, py, z) {
                        let e = &mut self.ent[p];
                        e.id24 = owner;
                        e.f30 = yaw;
                    }
                    yaw = yaw.wrapping_add(512);
                }
            }
            self.ent[i].z = self.ground_z(x, y) as i16;
        }
        let amt = self.ent[i].f44 as u32;
        self.area_write(i, 0, amt, ctx, false, false);
        false
    }

    /// sub_257B0 (:28443), class-10 state 13 — the RISING SMOKE PUFF
    /// (remc1hw :26987, byte-identical). Pre-decrement life like the
    /// whole class-10 family; then each tick it
    /// - decays the rise speed +126 by 4, clamped to [64, 128], and
    ///   lifts z by the clamped value, never below the ground under
    ///   its CURRENT cell (the sample precedes the drift);
    /// - for its first 15 ticks (+26 < 16) drifts 30 units flat along
    ///   its own +30 yaw and steps the sprite type on even +26;
    /// - in its last 6 ticks steps the sprite type back down, but only
    ///   while it is above the ctor's base row 67.
    ///
    /// WITHOUT THIS ARM the state fell through world.rs's class-10
    /// catch-all (the terrain-feature dispatch) and every imported puff
    /// self-killed one tick after import: (10,13) was the single
    /// largest unexplained family in the mc1hw corpus.
    ///
    /// `m14` = state 14 (sub_258A0, the mana-scatter puff): the same
    /// body with one tail change — the last-6-ticks sprite walk-down
    /// is UNCONDITIONAL (no base-row-67 floor). Unserviced, the
    /// authored (10,14) puffs of mc1l1's trigger scatters froze and
    /// drifted on every graded pair (~1650 rows, the biggest family
    /// of the take).
    fn smoke_puff_tick(&mut self, i: usize, m14: bool) -> bool {
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let speed = (self.ent[i].f126 - 4).clamp(64, 128);
        self.ent[i].f126 = speed;
        let mut p = (x, y, z.wrapping_add(speed));
        let g = self.ground_z(x, y) as i16;
        if p.2 < g {
            p.2 = g;
        }
        let v5 = self.ent[i].f26 + 1;
        self.ent[i].f26 = v5;
        if v5 < 16 {
            let yaw = self.ent[i].f30;
            Self::polar_step(&mut p, yaw, 0, 30);
            if v5 & 1 == 0 {
                self.ent[i].type86 = self.ent[i].type86.wrapping_add(1);
            }
        }
        if self.ent[i].act_life < 6 && (m14 || self.ent[i].type86 > 67) {
            self.ent[i].type86 = self.ent[i].type86.wrapping_sub(1);
        }
        self.move_relink(i, p.0, p.1, p.2);
        false
    }

    /// sub_26D20 (:29279), state 40: the lightning STORM cloud.
    /// Rises 64/tick until 1024 above the terrain (doing nothing
    /// else while climbing), then holds that altitude and fires TWO
    /// (9,9) bolts per tick in opposite random directions (pitch 56
    /// down, yaw flipped 0x400 between them), each with a third of
    /// the bolt life, the storm's 2000 damage, and the (10,23)
    /// endpoint flash; thunder 23 per firing tick. Life 32 ticks of
    /// fire (~66 bolts).
    fn storm_cloud_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let _ = ctx;
        let (x, y, z) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let g = self.ground_z(x, y) as i16;
        // :29296-306 — BOTH altitude corrections set the `v1` skip
        // flag: the tick the cloud is pulled DOWN onto the ceiling
        // (drifted terrain, a cloud born high) fires nothing either.
        // ⚠ THEY ARE SEQUENTIAL, NOT EXCLUSIVE. The climb writes +76
        // in place and the ceiling test then re-reads that NEW z, so
        // a cloud within 64 of the ceiling climbs PAST it and is
        // pulled straight back to ground+1024 in the SAME tick — it
        // never records the overshoot. The port returned after the
        // climb and spent a second tick coming back down, leaving the
        // cloud up to 63 units high for one frame (mc1l42 t=27234:
        // retail 4895 = ground+1024, ours 4927 = the raw z+64).
        let mut nz = z;
        let mut held = false;
        if nz < g.wrapping_add(1024) {
            nz = nz.wrapping_add(64);
            held = true;
        }
        if nz > g.wrapping_add(1024) {
            nz = g.wrapping_add(1024);
            held = true;
        }
        if held {
            self.move_relink(i, x, y, nz);
            return false;
        }
        // :29311-13 — PRE-decrement life test, as across the whole
        // class-10 effect family: 33 bolt ticks, not 32.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        let d = self.ent_rand(i);
        self.ent[i].f32 = 56;
        self.ent[i].f30 = (d & 0x7FF) as u16;
        for _ in 0..2 {
            // Yaw flips 180° BEFORE each launch (:29321-23).
            self.ent[i].f30 = self.ent[i].f30.wrapping_add(0x400) & 0x7FF;
            let (yaw, pitch, f44, own) = {
                let e = &self.ent[i];
                (e.f30, e.f32, e.f44, e.id24)
            };
            // :29325-31 — retail builds a `z + f78` point in the shared
            // temp and then passes the cloud's OWN position struct to
            // the creator: the +78 lift is a DEAD STORE. Adding it put
            // every bolt a sprite half-height above where retail lays
            // it (and above the flock the beam is meant to strike).
            let (bx, by, bz) = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
            if let Some(b) = self.spawn_zigzag(bx, by, bz) {
                let e = &mut self.ent[b];
                e.id24 = own;
                e.act_life /= 3; // shorter beams (:29334)
                // Retail writes only +30/+32; the beam's own acquire
                // (sub_534C0) fills +34/+36 on a lock and copies the
                // live heading into them on a miss, so pre-seeding them
                // would only matter if it could shadow that — it
                // cannot, and the original leaves them at NewEvent 0.
                e.f30 = yaw;
                e.f32 = pitch;
                e.f68 = 10;
                e.f69 = 23;
                e.f44 = f44;
            }
        }
        self.snd(23, i); // :29343
        false
    }

    /// sub_25760 (:28426), state 12: the possess detonation — a ch1
    /// claim broadcast every tick of its 8-tick life over the 512
    /// extents; balls and built houses consume the SENDER field.
    fn possess_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28433-36 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28432 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28437 — the anim step runs before the ch1 write, and the
        // broadcast amount is the flash's OWN `+44` (`sub_120B0(a1x,
        // 1, +44)`), i.e. the ctor's 64000. MC1's ch1 intake reads
        // the SOURCE alone (:29439-48), so the amount is inert here;
        // it is the MC2 twin that reads it as the claim's FORCE flag
        // (EF:4200 `dword_0x64_100`), and MC2 has its own writers —
        // this handler only reaches an MC2 world down the MC1-spell
        // fallback, where every arm is already approximate.
        self.anim_advance(i);
        // ⭐⭐⭐ MC2: THE CH1 AMOUNT IS `sub_112D0`'s FORCE ARGUMENT,
        // NOT THE PULSE'S `+44`. Retail's possession pulse is a
        // SIBLING PAIR that differs in exactly one literal —
        // `PossesHitMana_320E0` (EF:23546, the (10,12) WEAK claim)
        // calls `sub_112D0(entity, 0)` and `sub_32120` (EF:23559, the
        // (10,70) FORCED steal) calls `sub_112D0(entity, 1)` — and
        // `sub_112D0` (EF:4162) stamps the victim
        //     v5x->str_0x5E_94.word_0x68_104  = a1x->id_0x1A_26;
        //     v5x->str_0x5E_94.dword_0x64_100 = a2;
        // i.e. the ch1 AMOUNT **is** that 0/1 flag, verbatim. The MC2
        // ball intake reads it as the claim's force
        // (EF:26069-94 `if (dword_0x64_100)` → steal past the
        // `byte[2] & 0x20` claim lock AND set it; else the weak arm,
        // which a locked sphere refuses).
        //
        // The port already had the FORCED half right —
        // `mc2_steal_pulse_tick` (mc2/effects.rs) passes 1 — but
        // routed the WEAK (10,12) pulse through THIS handler, which
        // broadcasts MC1's `sub_120B0(a1x, 1, +44)` amount, the
        // ctor's 64000. Nonzero = forced, so **every weak possession
        // pulse stole through the claim lock**. mc2l22 pair
        // 22887→22888: the (10,12) at slot 465 stamps sphere 919
        // `mail1 = (64000, human)` and 919's own tick then takes the
        // forced arm — `player_ent` 530 -> the human and the
        // owner-keyed sprite row `f5a` 128 -> 112, where retail's
        // locked sphere keeps rival 530. `MGC_NO_MC2_CLAIM_PULSE_FORCE=1`
        // restores the pre-dig 64000.
        //
        // MC1 keeps `+44`: `sub_120B0(a1x, 1, +44)` (:28437) really
        // does broadcast the ctor value, and MC1's ch1 intake
        // (:29439-48) reads the SOURCE alone, so the amount is inert
        // there — a different retail primitive, not a shared one.
        let amt = if mc2_claim_pulse_force_law()
            && matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
        {
            0
        } else {
            self.ent[i].f44 as u32
        };
        self.area_write(i, 1, amt, ctx, false, false);
        false
    }

    /// Move + hit scan + terrain shared by the bolt flights — the
    /// common body of sub_52B30 (:62779-936, the m0 fireball) and
    /// sub_52770 (:62618-776, the generic family the class-9 table
    /// routes states 2-6/0xB/0xF/0x10/0x11/0x14 into). Returns
    /// terrain_dirty (always false here — craters come from the
    /// explosion). `law` picks the caller's Rebound deflection arm;
    /// the two are NOT interchangeable (see [`DeflectLaw`]).
    fn proj_move_and_hit(
        &mut self,
        i: usize,
        ctx: &MobCtx,
        copy_f44: bool,
        stamp_victim: bool,
        law: DeflectLaw,
    ) -> bool {
        let start = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        let mut tmp = start;
        let (yaw, pitch, speed) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut tmp, yaw, pitch, speed);
        // Retail MOVES FIRST and probes at its own moved position —
        // `sub_41C70` then `sub_11980` (:62675-76 generic, :62843-44
        // fireball with the pre-step `v21` saved for its terrain
        // revert); a strike then moves AGAIN onto the victim. Each
        // move is a tile-chain relink when it crosses a tile edge, so
        // a step that leaves and re-enters a tile RE-HEADS the record
        // even though its net tile never changed. The old
        // endpoint-only `victim_scan_at` probe dropped every one of
        // those chain ops: mc1l32's free run carried 69k silent
        // next20/prev22 shadow rows from t=7785 (a fireball strike's
        // re-head) and paid at t=29922, where the (9,14) bolt at 986
        // walked a differently-ordered village chain, struck the
        // wrong first match, and the human never took the recorded
        // 780 — the t=29923 knock reads retail mag 74, port 0.
        self.move_relink(i, tmp.0, tmp.1, tmp.2);
        if let Some(v) = self.victim_scan(i, ctx) {
            // Rebound (+17 bit 7): mana-shield deflection. The human
            // carpet's bit is the Rebound spell (14, :65774 — the
            // ported deflection-bit semantics). The Generic arm only
            // deflects whitelisted impact pairs (:62705-21): a bolt
            // failing the pair gate — or the mana check — lands as a
            // PLAIN HIT on the deflector (:62751-55), unlike the
            // Fireball arm's fly-through.
            let rebound = match v {
                MailTarget::Pool(j) => self.ent[j].flags & 0x8000 != 0,
                MailTarget::Player(hx) => self.player_rebound(hx),
            };
            let gate_ok = rebound
                && match law {
                    DeflectLaw::Fireball => true,
                    DeflectLaw::Generic => {
                        let e = &self.ent[i];
                        e.f68 == 10
                            && (e.f69 == 1
                                || e.f69 == 17
                                || (e.f69 == 53 && self.is_hidden_worlds()))
                    }
                };
            // Scatter around the reversed heading: ±45 (:62877) vs
            // ±22 (:62740).
            let (modulus, half) = match law {
                DeflectLaw::Fireball => (0x5B, 45i32),
                DeflectLaw::Generic => (0x2D, 22i32),
            };
            if rebound {
                match v {
                    MailTarget::Pool(j) => {
                        let quarter = (self.ent[i].f140 / 4).max(0);
                        if gate_ok && quarter <= self.ent[j].f140 {
                            // Sound 28 rides INSIDE the deflect branch
                            // (:62861/:62723 — positional at the
                            // DEFLECTOR, sub_55370(victim, -1, 28)); a
                            // refused deflection is silent.
                            self.snd(28, j);
                            self.ent[j].f140 -= quarter;
                            let deflector_id = self.ent[j].id24;
                            let shooter = self.ent[i].id24;
                            let human_shooter =
                                self.hx_of_tag(shooter).map(|hx| self.human_tag[hx].0);
                            let d = self.ent_rand(i);
                            let e = &mut self.ent[i];
                            e.f34 = e.f30.wrapping_add(0x400) & 0x7FF;
                            // The scattered heading is stored RAW
                            // (:62740/:62877 — no mask): a draw below
                            // `half` off a near-zero reversed yaw
                            // parks a NEGATIVE u16 in +30 for a tick
                            // (corpus t=2739: 65512 = −24). Every
                            // consumer masks on read; the next homing
                            // write canonicalizes.
                            e.f30 = (e.f34 as i32 + (d % modulus) as i32 - half) as u16;
                            e.f32 = e.f32.wrapping_neg() & 0x7FF;
                            // The reversed pitch is stored to BOTH
                            // words (:62727-32 / :62867-72, VA
                            // 0x52963 / 0x52D2E).
                            if !no_mc1_deflect_pitch_mirror() {
                                e.f36 = e.f32;
                            }
                            e.f146 = human_shooter.unwrap_or(shooter);
                            e.id24 = deflector_id;
                            e.act_life = e.max_life as i32;
                            // Relink at the deflector, LIFTED by its
                            // +84 (:62885-88 — victim z + victim+84).
                            let (jx, jy, jz) = (
                                self.ent[j].x,
                                self.ent[j].y,
                                self.ent[j].z.wrapping_add(self.ent[j].f84 as i16),
                            );
                            self.move_relink(i, jx, jy, jz);
                            return false;
                        }
                        if law == DeflectLaw::Fireball {
                            // Afford-fail (:62859's false arm — v24
                            // stays clear): NO hit at all. No sound, no
                            // debit, no explosion — the bolt keeps its
                            // stepped position (already moved above)
                            // and flies straight through.
                            return false;
                        }
                        // Generic refusal falls through to the plain
                        // hit below (:62751-55).
                    }
                    MailTarget::Player(hx) => {
                        // Afford gate (:62706/:62859, unsigned
                        // compare): the quarter of the projectile's
                        // `+140` must fit the DEFLECTOR's purse or
                        // the deflection refuses. The human's purse
                        // is his record's live `+140` at the
                        // projectile's walk slot (retail's read), less
                        // whatever this tick's earlier deflections
                        // already owe (the debit drains post-walk).
                        let quarter = (self.ent[i].f140 / 4).max(0) as u32;
                        let purse = self
                            .seat_purse_read(ctx, hx)
                            .saturating_sub(self.player_deflect_debit[hx].0);
                        if gate_ok && quarter <= purse {
                            self.snd(28, i); // deflection twang (:62861)
                            // The deflector PAYS the quarter (:62725
                            // and twins — the debit lands at the
                            // PROJECTILE's walk slot): accumulated
                            // here, drained by the wizard pass / tick
                            // tail into the human purse.
                            self.player_deflect_debit[hx].0 += quarter;
                            // The projectile reverses heading and swaps
                            // owner to the player, re-homing on its
                            // shooter.
                            let shooter = self.ent[i].id24;
                            let d = self.ent_rand(i);
                            let e = &mut self.ent[i];
                            e.f34 = e.f30.wrapping_add(0x400) & 0x7FF;
                            // Raw store, as in the pool arm above.
                            e.f30 = (e.f34 as i32 + (d % modulus) as i32 - half) as u16;
                            e.f32 = e.f32.wrapping_neg() & 0x7FF;
                            // `+36 = +32`, as in the pool arm.
                            if !no_mc1_deflect_pitch_mirror() {
                                e.f36 = e.f32;
                            }
                            e.f146 = shooter;
                            e.id24 = self.human_tag[hx].0;
                            e.act_life = e.max_life as i32;
                            // Victim z + victim +84 (:62885-88), as in
                            // the pool arm — the carpet's +84 is
                            // PLAYER_HH (sprite 44 height/2).
                            self.move_relink(
                                i,
                                self.seat_at(ctx, hx).0,
                                self.seat_at(ctx, hx).1,
                                self.seat_at(ctx, hx).2.wrapping_add(PLAYER_HH as i16),
                            );
                            return false;
                        }
                        if law == DeflectLaw::Fireball {
                            // Afford-fail fly-through (:62859's false
                            // arm), as in the pool arm above: no hit,
                            // no sound, no debit.
                            return false;
                        }
                        // Generic refusal: the bolt hits the rebounding
                        // player like any other (:62751-55).
                    }
                }
            }
            // Teleport onto the victim, explode there (:62852-55).
            match v {
                MailTarget::Pool(j) => {
                    // The +78 aim lift skips castles (sub_524C0's
                    // model-2 guard, [`Ent::aim_z`]): a castle strike
                    // lands at the flag, not 8192 under the mound.
                    let (jx, jy, jz) = (self.ent[j].x, self.ent[j].y, self.ent[j].aim_z());
                    self.move_relink(i, jx, jy, jz);
                }
                MailTarget::Player(hx) => {
                    // The same +78 lift as the pool arm's `aim_z` —
                    // the carpet's +78 is PLAYER_HH (sprite 44
                    // height/2), and the model-2 castle guard can
                    // never apply to the human.
                    let (px, py, pz) = self.seat_at(ctx, hx);
                    self.move_relink(i, px, py, pz.wrapping_add(PLAYER_HH as i16));
                }
            }
            self.proj_explode(i, ctx, Some(v), copy_f44, stamp_victim);
            return false;
        }
        let ground = self.ground_z(tmp.0, tmp.1) as i16;
        if ground <= tmp.2 {
            self.ent[i].act_life -= 1;
            if self.ent[i].act_life < 0 {
                self.proj_explode(i, ctx, None, copy_f44, stamp_victim); // midair expiry
            }
        } else {
            // Terrain impact. The position law differs by function:
            // the FIREBALL (:62899-908) REVERTS to the pre-step
            // position (`sub_41C70(a1, &v21)` — a second relink) —
            // its water test, splash and detonation all happen at the
            // point it flew FROM; the GENERIC (:62680-701) has no
            // revert — it keeps the stepped position for all three.
            // Both exempt model 4 (the volcano lob) from the splash:
            // over water it detonates like on land.
            if law == DeflectLaw::Fireball {
                self.move_relink(i, start.0, start.1, start.2);
            }
            let (ix, iy) = (self.ent[i].x, self.ent[i].y);
            if self.ent[i].model65 != 4 && self.on_water_pub(ix, iy) {
                self.splash_and_die(i); // :62916-21, no explosion/crater
            } else {
                self.proj_explode(i, ctx, None, copy_f44, stamp_victim);
            }
        }
        false
    }

    /// The victim scan evaluated at a prospective position (the
    /// original moves first and scans at the new position).
    pub(crate) fn victim_scan_at(
        &mut self,
        i: usize,
        tmp: (u16, u16, i16),
        ctx: &MobCtx,
    ) -> Option<MailTarget> {
        let old = (self.ent[i].x, self.ent[i].y, self.ent[i].z);
        self.ent[i].x = tmp.0;
        self.ent[i].y = tmp.1;
        self.ent[i].z = tmp.2;
        let v = self.victim_scan(i, ctx);
        self.ent[i].x = old.0;
        self.ent[i].y = old.1;
        self.ent[i].z = old.2;
        v
    }

    fn splash_and_die(&mut self, i: usize) {
        let (x, y, z, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if let Some(s) = self.spawn_effect(5, x, y, z) {
            // :62695-97 / :62910-12 / :63176-78 / :63704-06: `+24` is
            // read off the bolt after the allocation — identity on a
            // self-seizure, and the `sub_41E80(a1)` below then reaps
            // the splash itself ([`no_mc1_self_seize`]).
            if !self.mc1_self_seized(s, i) {
                self.ent[s].id24 = owner;
            }
        }
        self.ent[i].flags |= 0x400;
    }

    /// sub_11810 (:16880) `& 1`: the TILE-TYPE water probe — retail's
    /// projectile splashes, tree trunks, site scans and the ambient
    /// loop all switch on `type == 0`. Its sibling sub_11760 (`angle
    /// nibble == 0`, our `on_water`) is a DIFFERENT law: shore/wave
    /// cells (type 45, nibble 0) are land here and water there —
    /// check the caller's retail anchor before picking one.
    pub(crate) fn on_water_pub(&self, x: u16, y: u16) -> bool {
        self.t.tile_type[(((y >> 8) as usize) << 8) | (x >> 8) as usize] == 0
    }

    /// The register half alone: did the allocator hand `src`'s own
    /// slot back as `child`? For the sites whose ctor pointer is a
    /// STACK local (`lea 0x8(%esp)`, e.g. the m9 lightning's endpoint
    /// `&v25`, :63424) — the pose is the caller's copy, but every
    /// post-allocation read of `src` still reads the child.
    pub(crate) fn mc1_seized_caller(&self, child: usize, src: usize) -> bool {
        child == src
            && !no_mc1_self_seize()
            && !matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2)
    }

    /// ⭐ THE SHARED POSE HALF OF [`no_mc1_self_seize`]: `child` came
    /// back from a ctor that retail handed a POINTER into `src`'s own
    /// record (`lea 0x48(%reg)` → `call sub_373F0`, or a direct ctor
    /// call with the same pointer). If the dry free stack made the
    /// allocator seize `src` itself, the ctor read its position
    /// through that pointer AFTER `NewEvent_372C0` memset the record,
    /// so the child sits at (0,0,0) — linked there if the ctor links,
    /// raw-written if it does not. Returns whether the alias fired;
    /// the caller must then read every post-allocation operand of
    /// `src` LIVE (they are the child's own fields), never from a
    /// pre-allocation copy.
    ///
    /// MC1 column only: MC2's ctors and allocator are their own lane.
    ///
    /// SITE CENSUS — `sub_373F0` (file 0x4FBE8) has 106 call sites in
    /// `CARPET.EXE`; 62 push a record pointer (`lea 0x48(%reg)` /
    /// `lea 0x96(%reg)`), the rest a stack axis (`lea 0x8(%esp)`) or
    /// the global scratch `word_AE454`, which alias only through the
    /// post-allocation OPERAND reads, never the pose.
    ///
    /// * MODELLED HERE — `sub_53070` trail (:63029), the class-9
    ///   detonations that share [`Gen::proj_explode`] (`sub_52770`
    ///   :62759, `sub_52B30` :62923, `sub_52ED0` :63000, `sub_530C0`
    ///   :63190, `sub_542B0` :63905/:63913), the state-19 relay
    ///   `sub_54480` :63932 (`death_relay_tick`), the water splashes
    ///   :62695/:62910/:63176/:63704 (`splash_and_die`), the storm
    ///   bloom `sub_53DC0` :63767, the m9 lightning's endpoint blast
    ///   :63424 (stack axis, operands only), the creature shooter
    ///   thunks :21884/:21916/:21943/:22026/:22051/:22074/:22111/
    ///   :22144/:23248/:25848/:26159 ([`Gen::mc1_arm_live`]), the
    ///   corpse's puff :21866 and ball `sub_27690` :29675, the
    ///   standing fire's exhaust `sub_252D0` :28230, and the HW-only
    ///   firewall trail `sub_54600` (hw:59934, `proj_firewall_tick`;
    ///   witness mc1hwl5 t=6188, round 166).
    /// * UNREACHABLE — the sixteen cast token machines
    ///   (`sub_56090`…`sub_58240`, :65058-66325) and
    ///   `sub_44D30`/`sub_45FC0`/`sub_155F0`/`sub_3C9D0`: their
    ///   pointer is a WIZARD's record. MC1's recycle stack is a
    ///   rebuild over the live records carrying `0x20400`
    ///   (sacrificable | reap-flagged — `mc1/rivals.rs`), a wizard
    ///   record carries neither, and it is minted once at level load
    ///   (`sub_44D30`'s null-record branch) and never freed — the
    ///   respawn at :55616 hands the SAME record back — so no stack
    ///   can ever name its slot.
    /// * STILL EXPOSED, NOT CONVERTED (w159e ran out of round; each
    ///   needs its own operand audit, none has a corpus witness) —
    ///   the volcano driver `sub_25EC0` :28784/:28795/:28807 (plume,
    ///   smoke and lava bomb off the vent's `+72`), the quake crevice
    ///   `sub_25990` :28562 and canyon head `sub_26920` :29135, the
    ///   tree's death flame `sub_49890` :57681 (it also reads `+94`,
    ///   which the port models as the mail source, and takes an LCG
    ///   draw on the caller AFTER the allocation), its siblings
    ///   :57707/:57744/:57762, the castle's `sub_47020`/`sub_47080`
    ///   :56105/:56124 (`+150`, so the child reads the castle's
    ///   zeroed SITE triple) and `sub_47400` :56343/:56428, and the
    ///   m6 spit `sub_1BD20` :22947.
    pub(crate) fn mc1_self_seized(&mut self, child: usize, src: usize) -> bool {
        if !self.mc1_seized_caller(child, src) {
            return false;
        }
        if self.ent[child].flags & 4 != 0 {
            self.unlink(child);
            self.link(child, 0, 0, 0);
        } else {
            let e = &mut self.ent[child];
            e.x = 0;
            e.y = 0;
            e.z = 0;
        }
        true
    }

    // ---- class-10 combat effects -------------------------------------------

    /// The class-10 effect inits (states = the original's +70 writes).
    pub(crate) fn spawn_effect(&mut self, model: u8, x: u16, y: u16, z: i16) -> Option<usize> {
        // On the MC2 column the shared-lineage effects resolve into
        // their NATIVE ctors — the ground fire (0) and the explosion
        // seeder (1) are the same entity in both engines (life 8/1,
        // damage 400, sprite 7/41, extents 128) but tick through the
        // per-game arms (MC2: sub_30D50 worn-path repaints + ring
        // cluster). Without this, an MC1-fallback fireball on an MC2
        // world spawns an MC1-shaped fire that the game-keyed dispatch
        // feeds to the MC2 handler (damage field mismatch → silent
        // fire).
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            match model {
                0 => return self.mc2_spawn_fire(x, y, z),
                1 => return self.mc2_spawn_big_explosion(x, y, z),
                _ => {}
            }
        }
        // sub_3B970 (:47672): the (10,54) mana MAGNET — reached here
        // as the Mana Magnet bolt's +69 detonation (:66084-85); the
        // caller stamps the owner like on every effect.
        if model == 54 {
            return self.spawn_mana_magnet(x, y, z, 0);
        }
        let s = self.new_event()?;
        self.ent[s].class64 = 10;
        self.ent[s].model65 = model;
        match model {
            // sub_3A490 (:46454): the fire/explosion. Damage 400.
            0 => {
                let e = &mut self.ent[s];
                e.tick70 = 0;
                e.max_life = 8;
                e.f44 = 400;
                e.f28 = 0;
                e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 7);
                self.extents(s, 128, 128);
            }
            // sub_3A510 (:46482): the fire-spreader / corpse flame.
            1 => {
                let e = &mut self.ent[s];
                e.tick70 = 1;
                e.max_life = 1;
                e.f44 = 400;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 41);
            }
            // sub_3A570 (str_255D0C[2]): the ambient puff — life 8,
            // silent, spriteless and UNLINKED (raw position write, no
            // grid insert), zero extents. The arctic wizard ambience
            // emits these constantly on HW; the tick is the bare
            // family decrement (sub_252B0).
            2 => {
                let e = &mut self.ent[s];
                e.tick70 = 2;
                e.max_life = 8;
                e.f26 = 0;
                e.flags = (e.flags & !0x2_0009) | 0x2_0001;
                e.x = x;
                e.y = y;
                e.z = z;
                self.refill_life(s);
            }
            // sub_3A5D0 (str_255D0C[3]): the smoke puff — life 7,
            // f44/f26 zeroed, linked, sprite 36, silent, no extents;
            // tick = the bare family decrement (sub_253F0).
            3 => {
                let e = &mut self.ent[s];
                e.tick70 = 3;
                e.max_life = 7;
                e.f44 = 0;
                e.f26 = 0;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 36);
            }
            // sub_3AAA0 (str_255D0C[13]): the RISING SMOKE PUFF. Two
            // creators, both untraced until now: the dying standing
            // fire's 1-in-7 exhaust (sub_252D0 :28224) and the volcano
            // plume's ring spray (sub_26140 :28874). Life rand%23+17,
            // rise speed rand%53+51 (the state-13 tick decays it 4 a
            // tick toward the 64 floor), sprite 67, the (10,13) filter
            // pair, +18 bit1. Its class-10 twin model 14 (sub_3AB40,
            // arm below) IS authored in MC1: level THING records fired
            // by trigger dispositions mint it (mc1l1's t=344 mana
            // scatter — the earlier "NO MC1 creator" note only meant
            // no code-side caller, which is true of the whole creator
            // table).
            13 => {
                let e = &mut self.ent[s];
                e.tick70 = 13;
                let d1 = lcg32(&mut e.rand);
                e.max_life = d1 % 0x17 + 17;
                e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
                let d2 = lcg32(&mut e.rand);
                e.f66 = 10;
                e.f67 = 13;
                e.f126 = (d2 % 0x35 + 51) as i16;
                // Retail's ctor order: link, sprite, THEN refill.
                self.link(s, x, y, z);
                self.set_sprite(s, 67);
                self.refill_life(s);
            }
            // sub_3AB40 (:46860): the mana-scatter puff twin — the
            // same two-draw ctor with its own numbers: life rand%33
            // +28, filter pair (10,14), sprite 9.
            14 => {
                let e = &mut self.ent[s];
                e.tick70 = 14;
                let d1 = lcg32(&mut e.rand);
                e.max_life = d1 % 0x21 + 28;
                e.flags = (e.flags & !(8 | 0x20000)) | 0x20000;
                let d2 = lcg32(&mut e.rand);
                e.f66 = 10;
                e.f67 = 14;
                e.f126 = (d2 % 0x35 + 51) as i16;
                self.link(s, x, y, z);
                self.set_sprite(s, 9);
                self.refill_life(s);
            }
            // The standing fire / ground wave (state 6, sub_3A730 ctor
            // → sub_252D0 tick): life 240, 50 ch0 per tick via the /10
            // writer, sprite 228 (the flame-size family +86 walks ±1),
            // and the damage extents `sub_37130_374F0(272, 1536)`
            // (:46643) — a ~1-tile-wide, 6-tile-tall AABB so burning
            // trees/lava actually torch fly-by carpets, creatures and
            // neighbor trees (WITHOUT it the fire had zero extents and
            // overlapped nothing → ambient fires dealt no damage). Tree
            // deaths override life and set the f46 trunk offset.
            // sub_3A730 (:46620): the STANDING fire. ⭐ The ctor's
            // last position write is a GROUND SNAP — `v2[38] =
            // sub_11F50(a1)` (:46640), word 38 = `+76` = z, sampled at
            // the CALLER's axis and overwriting the z the link just
            // stored. A standing fire is born ON THE GROUND, never at
            // its spawner's altitude, and the sibling ctors 4/5/7
            // (:46600/:46608/:46672) all close the same way.
            // mc1l4 t=1224: three trees burn on ground the (10,0) fire
            // in slot 63 scorched 68 units DOWN earlier in that same
            // tick, so their records still read the pre-dig 2157 while
            // the flames retail plants read 2089.
            6 => {
                let e = &mut self.ent[s];
                e.tick70 = 6;
                e.max_life = 240;
                e.f44 = 50;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                let (px, py) = (self.ent[s].x, self.ent[s].y);
                self.ent[s].z = self.ground_z(px, py) as i16;
                self.refill_life(s);
                self.set_sprite(s, 228);
                self.extents(s, 272, 1536);
            }
            // sub_3A6B0 (:46560 region): the water splash. Grounded.
            5 => {
                let e = &mut self.ent[s];
                e.tick70 = 5;
                e.max_life = 8;
                e.f44 = 0;
                e.flags &= !8;
                e.flags |= 0x20000;
                self.link(s, x, y, z);
                let (px, py) = (self.ent[s].x, self.ent[s].y);
                self.ent[s].z = self.ground_z(px, py) as i16;
                self.refill_life(s);
                self.set_sprite(s, 244);
            }
            // (10,26) ctor `sub_3AF80` (:47116-32): the duel tether —
            // life 8, sprite row 284, `+44` = 200 (the explode tail
            // overwrites it with the DART's), and ⭐ **THE 512/512
            // EXTENTS**: the ctor's last line is
            // `sub_37130_374F0(v2, 512, 512)` (:47132), the plain
            // setter `+80 = +82 = a2; +84 = a3` (:43790-95) — NOT the
            // doubling `sub_370A0_37460`. The port bound sprite 284's
            // own halves (150/150/75) and stopped, so the ch4 grip box
            // was a THIRD of retail's 4-tile reach. mc1l48 t=59118
            // slot 20 records 512/512/512 on the birth tick.
            26 => {
                let e = &mut self.ent[s];
                e.tick70 = 26;
                e.max_life = 8;
                e.f44 = 200;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 284);
                if !crate::engine::world::mc1_tether_static_off() {
                    self.extents(s, 512, 512);
                }
            }
            // sub_3AC70 (:46935): the invisible fire-ring blast driver.
            // sub_3AC70 (:46935): the eruption blast fire-field —
            // life 10, damage 3000, and a RAW position write: the
            // ctor never tile-links it (mc1l3 t=2306 graded the
            // port's linked flags 0x4).
            17 => {
                let e = &mut self.ent[s];
                e.tick70 = 17;
                e.max_life = 10;
                e.f44 = 3000;
                e.flags &= !8;
                e.x = x;
                e.y = y;
                e.z = z;
                self.refill_life(s);
            }
            // sub_3AA10 (:46790): the POSSESS detonation flash —
            // an 8-tick ch1 claim broadcast over 512-unit extents.
            // `+44 = -1536` (:46804) is a SIGNED word in a u16 field,
            // i.e. 64000, and the recording reads exactly that on
            // every flash in the corpus (2,654 raw-shadow rows across
            // mc1l0/l1/l2/l42 against the port's 0). Nothing consumes
            // the amount — MC1's ch1 intake reads the SOURCE alone
            // (:29439-48) and the flash tick re-broadcasts it eight
            // times — so this is a field-value law, not a behaviour
            // one; carry it because the record does.
            12 => {
                let e = &mut self.ent[s];
                e.tick70 = 12;
                e.max_life = 8;
                e.f44 = (-1536i16) as u16;
                // Corpus: every fresh flash record reads flags 0x5 —
                // the ctor sets bit 1 like the (10,19) plume's.
                e.flags = (e.flags & !8) | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                // The ctor's sub_36FA0(41) — the visible claim
                // sparkle (extents then overridden to 512).
                self.set_sprite(s, 41);
                self.extents(s, 512, 512);
            }
            // sub_3AE00 (:47034): the volcano's (10,19) smoke/fire
            // plume — a 240-tick visual at the crater (sprite 228,
            // the flame family), no damage (+18 bit1 set).
            19 => {
                let e = &mut self.ent[s];
                e.tick70 = 19;
                e.max_life = 240;
                e.f44 = 200;
                e.flags = (e.flags & !8) | 0x20000 | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 228);
                self.extents(s, 512, 512);
            }
            // The Wall of Fire NAPALM cloud (state 58 — NOT 53;
            // class-10 state 53 is the building collapse walker). The
            // model-53 creator was SWAPPED between builds (both spell-20
            // paths detonate the m16 bolt into this (10,53) via the
            // +68=10/+69=53 descriptor — trace SURVEY-MC1HW §3/§7):
            // - base MC1 `sub_3B8E0` (:47639): a persistent low-damage
            //   wall — life 128, f44 100, random yaw, extents 1024/0x4000.
            // - Hidden Worlds `sub_3BC60` (remc1hw :43766): a brief,
            //   devastating expanding-ring detonation — life 6, f44 3000,
            //   NO extents (the state-58 HW handler re-derives them each
            //   tick) and NO yaw LCG draw (stream-faithful).
            53 => {
                let hw = self.is_hidden_worlds();
                let e = &mut self.ent[s];
                e.tick70 = 58;
                e.f26 = 0;
                e.flags &= !8;
                if hw {
                    e.max_life = 6;
                    e.f44 = 3000;
                    // hw:43776-77 — the position lands DIRECT here
                    // too (`+72 = *a1`, `+76 = a1[4]`, no
                    // `sub_41CF0`), exactly like the base-MC1 twin
                    // below: the HW cloud is OFF THE TILE CHAIN and
                    // its recorded `flags` is 0, not the linker's 4
                    // (mc1hwl0 t=2750, slot 494).
                    e.x = x;
                    e.y = y;
                    e.z = z;
                } else {
                    e.max_life = 128;
                    // :47654 — the ctor's speed word, and :47664-66:
                    // the position lands DIRECT (+72/+76, no sub_41CF0
                    // anywhere in sub_3B8E0) with `+16 |= 1` — the
                    // recorded cloud reads flags exactly 1, speed 256,
                    // OFF the tile chain (mc1l5 t=23404, slot 772).
                    e.f126 = 256;
                    e.f44 = 100;
                    let d = lcg32(&mut e.rand);
                    e.f30 = (d & 0x7FF) as u16;
                    e.x = x;
                    e.y = y;
                    e.z = z;
                    e.flags |= 1;
                    e.f80 = 1024;
                    e.f82 = 1024;
                    e.f84 = 0x4000;
                }
                self.refill_life(s);
            }
            // sub_3BA00 (:47705): the GLOBAL DEATH field (state 60).
            // +26 = 32 = the priming tick-tock; +44 = 100 (the
            // detonation copy overrides with the spell's 7000). The
            // ctor's life 19 / speed 256 / random heading / extents
            // (1024, 0x4000) are DEAD WEIGHT for the state-60
            // handler (verbatim anyway); the flat plane lives in
            // the sweep's 2D distance. No sprite — the spell is
            // authentically invisible.
            55 => {
                let e = &mut self.ent[s];
                e.tick70 = 60;
                e.max_life = 19;
                e.f44 = 100;
                e.f26 = 32;
                e.f126 = 256;
                let d = lcg32(&mut e.rand);
                e.f30 = (d & 0x7FF) as u16;
                // :47712 clears bit 3, :47729 sets bit 0, and the
                // position is a RAW `+72`/`+76` store (:47726-28) —
                // the field never enters a tile chain. Same ctor shape
                // as its sibling the (10,54) magnet
                // ([`Gen::spawn_mana_magnet`]).
                e.flags &= !8;
                e.flags |= 1;
                e.f80 = 1024;
                e.f82 = 1024;
                e.f84 = 0x4000;
                e.x = x;
                e.y = y;
                e.z = z;
                self.refill_life(s);
            }
            // sub_3B460 (:47396): the lightning STORM cloud — note
            // state 40 (not 38), life 32, sprite 272. The caller
            // copies heading/target/damage/bolt-spec from the (9,12)
            // storm projectile (:63775-81).
            38 => {
                let e = &mut self.ent[s];
                e.tick70 = 40;
                e.max_life = 32;
                // The ctor EDITS the flag word rather than clearing
                // it (:47409-11 `v3 = v1[16] & 0xF7` → `+16 = v3`) —
                // the same mask-then-set shape as the (10,23) flash
                // and the (10,11) crater bowl, and it drops
                // NewEvent's hittable bit 3. A raining storm cloud is
                // NOT a thing other projectiles can detonate on:
                // retail's clouds read flags 4, ours read 12
                // (mc1l42 t=27217 slot 99).
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 272);
                self.extents(s, 512, 512);
            }
            // sub_3AE80 (:47062): the bolt hit-flash (one-shot ch0).
            23 => {
                let e = &mut self.ent[s];
                e.tick70 = 23;
                e.max_life = 8;
                e.f44 = 25;
                // The ctor's flag word is a mask-then-set pair
                // (:47076-79): `+16 &= 0xFFFDFFF7` drops NewEvent's
                // hittable bit 3 as well as 0x20000, and `+18 |= 2`
                // puts 0x20000 straight back. The port kept bit 3 —
                // every retail flash reads 0x20005, ours 0x2000D.
                e.flags &= !8;
                e.flags |= 0x20000 | 1;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 7);
                self.extents(s, 200, 200);
                self.blast_hits.set(s, false);
            }
            // sub_3B3E0 (:47370): the (10,36) UNDEAD-ARMY spawner —
            // life 8, sprite 41, extents 512, and `+44` = −1536 which
            // its own state-38 tick overwrites with 10000 before it
            // reads it ([`Gen::undead_army_tick`]). The ctor's `+44`
            // is observable only on a spawner minted at a slot BELOW
            // the detonating bolt's, which never gets to tick in the
            // same walk.
            36 => {
                let e = &mut self.ent[s];
                e.tick70 = 38;
                e.max_life = 8;
                e.f44 = (-1536i16) as u16;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 41);
                self.extents(s, 512, 512);
            }
            // sub_3AF00 (:47090): m11's mana-steal flash (ch3).
            25 => {
                let e = &mut self.ent[s];
                e.tick70 = 25;
                e.max_life = 8;
                e.f44 = 2000;
                e.flags &= !8;
                self.link(s, x, y, z);
                self.refill_life(s);
                self.set_sprite(s, 283);
                self.extents(s, 512, 512);
            }
            _ => {
                self.free_entity(s);
                return None;
            }
        }
        Some(s)
    }

    /// sub_3B5A0 (:47443): the mana ball (state 41). Callers override
    /// +140/+144; the tick re-derives the size sprite every turn.
    /// Both games' ctors stamp the source pair AND a base speed —
    /// MC1 +66/+67 = 10/39, +126 = 32 (:47456-57, :47463); MC2
    /// xtype/xsubtype = 10/39, actSpeed = 32 (CreateManaSphere
    /// EF:36614-17). The mc1l0 corpus pins both: every corpse-drop
    /// ball reads sclass/smodel 10/39, and an unstamped port ball
    /// sat at the NewEvent default 16 where retail's varied.
    pub(crate) fn spawn_mana_ball(&mut self, x: u16, y: u16, z: i16) -> Option<usize> {
        let b = self.new_event()?;
        {
            let e = &mut self.ent[b];
            e.class64 = 10;
            e.model65 = 39;
            e.f66 = 10;
            e.f67 = 39;
            e.tick70 = 41;
            e.f140 = 512;
            e.f46 = 128;
            e.f126 = 32;
            e.f28 = 3;
            e.f58 = 0x80;
        }
        // ⚠⚠ THE MASK HAS TWO HOMES AND MC2's WAS EMPTY. Both ctors
        // stamp the mail-channel admit mask 3 (`(1 << ch) & mask`),
        // but MC1 keeps it at +28 and MC2 at **@0x38** — retail's
        // `CreateManaSphere_500C0` writes `byte_0x38_56 = 3`
        // (EF:36617), which the importer homes in `f56` for class
        // 2/10 and the roster publishes as the graded `b38` lane. The
        // shared ctor set only MC1's `f28`, so every free-run MC2
        // sphere carried mask 0 where retail carries 3. Corpus: mc2l3
        // t=9816 borns eleven spheres in one tick and `explain` shows
        // ALL ELEVEN taking `b38 0 -> 3`.
        if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
            self.ent[b].f56 = 3;
        }
        self.link(b, x, y, z);
        self.refill_life(b);
        self.ball_resize(b);
        Some(b)
    }

    /// dword_900A4 (:2215): the ball size-class thresholds.
    const BALL_SIZES: [i32; 7] = [256, 512, 1024, 2048, 4096, 9192, 18384];

    /// sub_274D0 (:29574): ball sprite = family base + size class by
    /// carried mana (8 classes; > 36768 = the dragon-drop boulder);
    /// nonzero sizes halve the extents (sub_370E0 :43781). Family 52
    /// = unowned; the owner palette families (105 + 8·player-slot)
    /// are the mana-collection track (our claims use the
    /// PLAYER_TARGET sentinel, not a pool wizard).
    pub(crate) fn ball_resize(&mut self, i: usize) {
        let mana = self.ent[i].f140;
        let mut size = 7usize;
        for (k, t) in Self::BALL_SIZES.iter().enumerate() {
            if mana <= *t {
                size = k;
                break;
            }
        }
        // Owner recolor (:29627-32): claimed balls swap to the owner
        // wizard's color row (base 105 + 8*color, wizext var_48);
        // unowned/wild stay on the neutral 52 row. MC1 art is in raw
        // slot order; MC2's sphere families are authored in Transform
        // order (GetManaSphereIndexFromId EF:26800 routes through
        // TransformPlayerColorIndex — crate::mc2::COLOR_ART).
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // ✅ RESOLVED — and the note that used to stand here was
        // WRONG TWICE, which is why it hid the law for a whole corpus.
        // It read: "a RIVAL-claimed sphere renders the NEUTRAL family
        // in retail (sprite 56 = 52+4 with a live class-3 owner in
        // +148) … the neutral derive's mechanism is unresolved.
        // Conformance-invisible: the sprite lane isn't compared."
        // (1) `f5a` IS GRADED (`conformance.rs`, `("f5a", r.f5a)`) —
        //     and on mc2l6-rsg it was the take's certification wall.
        // (2) The mechanism was never in this colour derive, which is
        //     bit-exact against `GetManaSphereColorIndexFromEntityId_369F0`
        //     and `GetManaSphereIndexFromId_36A50`. It is in WHICH TICK
        //     FUNCTION CALLS THIS: the (10,57) has no per-tick resize
        //     at all (see [`no_m57_tick_resize`]), so the observation
        //     "sprite 56 with a live class-3 owner" was the m57 simply
        //     keeping its unowned ctor stamp. PHASE, NOT ARITHMETIC.
        // The team-colour derive below is correct for the (10,39).
        // ⭐⭐⭐ MC1's `+144` IS A RAW POOL SEAT, AND THE ONLY TEST IS
        // `class == 3`. `sub_274D0` does
        //
        //     v3 = pool + 164 * event->+144;
        //     if (v3 > pool && *(BYTE *)(v3 + 64) == 3)
        //         switch (*(WORD *)(*(DWORD *)(v3 + 160) + 48)) { … }
        //     else v1 = 52;
        //
        // (remc1 sub_main.cpp:29586-29625, remc1hw sub_main.cpp:28130-
        // 28169 — the two bodies are the same statements). It never
        // consults a wizard register: it INDEXES THE POOL at the seat,
        // accepts ANY class-3 occupant, and reads that record's
        // `+160` "wizext" pointer at `+48` for the palette family.
        // Only wizards carry a real extension; every other entity
        // shares the engine's default one, whose `+48` word reads 0 —
        // so a class-3 NON-wizard colours the ball with family 105,
        // the player-0 row.
        //
        // The port resolved the seat through [`Gen::owner_team`],
        // i.e. only `PLAYER_TARGET` and the eight registered wizard
        // seats, and answered "unowned" (52) for anything else. Two
        // holes, both closed here: a class-3 non-wizard in the seat
        // (retail 105, port 52) and a REGISTERED seat whose record has
        // since been recycled to another class (retail 52, port 105+).
        //
        // WITNESS mc1hwl5 t=25348..25486, ball 866 (`+140` 240,277 →
        // size 7, `+144` = 7): slot 7 is recycled from a dead (0,9)
        // corpse into a (3,3) MANA BALLOON at t=25348 and retail's
        // very next resize jumps the row 59 → 112 (52+7 → 105+7) and
        // holds it; the port stayed on 59 for all 61 heads. The
        // balloon's own owner is castle 411 = player 3 (ball 253 in
        // the same take reads `+144` 411 → row 132 = 129+3), so the
        // 105 is NOT the balloon's owner colour — it is the default
        // extension's zero.
        //
        // SHIPPED BYTES — `CARPET.EXE` file 0x3FCC8 (VA 0x274D0, file
        // = VA + 0x187F8) and `HIDDEN.EXE` file 0x3FEC8 (VA 0x274D0,
        // file = VA + 0x189F8, pinned by this body's own
        // `call sub_36FA0_37360`: CARPET `e8 00 fa 00 00` → 0x4F798 =
        // VA 0x36FA0, HIDDEN `e8 c0 fd 00 00` → 0x4FD58 = VA 0x37360)
        // are byte-identical apart from the pool-base global
        // (`8b 15 00 e4 01 00` vs `8b 15 f0 e3 01 00`) and that rel32
        // — HW does NOT fork the derive:
        // ```text
        //   3ff20  39 d0              cmp    %edx,%eax      ; seat != 0
        //   3ff22  76 58              jbe    0x3ff7c        ; → 52
        //   3ff24  80 78 40 03        cmpb   $0x3,0x40(%eax); class 3?
        //   3ff28  75 52              jne    0x3ff7c        ; → 52
        //   3ff2a  8b 80 a0 00 00 00  mov    0xa0(%eax),%eax; +160
        //   3ff30  66 8b 40 30        mov    0x30(%eax),%ax ; +48
        //   3ff34  40                 inc    %eax
        //   3ff35  66 3d 08 00        cmp    $0x8,%ax
        //   3ff39  77 46              ja     0x3ff81        ; stale cx
        //   3ff3b  98                 cwtl
        //   3ff3c  2e ff 24 85 a8 74 01 00  jmp *%cs:0x174a8(,%eax,4)
        //   3ff44  b9 69 00 00 00     mov    $0x69,%ecx     ; 105
        //   …                                               ; +8 each
        //   3ff7c  b9 34 00 00 00     mov    $0x34,%ecx     ; 52
        // ```
        // (`+48 == 0xFFFF` lands on table entry 0 = the 52 arm, which
        // is the decompile's `case 0xFFFF: goto LABEL_15`.)
        //
        // MC2 keeps its own derive — `GetManaSphereColorIndexFromEntityId_369F0`
        // is a different routine and is not re-scoped here.
        let own = self.ent[i].f144;
        let team = if mc2 || no_mc1_ball_owner_seat() {
            self.owner_team(own)
        } else if let Some(hx) = self.hx_of_tag(own) {
            // The port's human tag. Retail carries the human's own
            // pool slot in `+144` (the conformance importer's `tr()`
            // rewrites it), and that record is the class-3 carpet, so
            // the seat test passes and its wizext reads player 0.
            // With several humans each is his own wizard's colour (a
            // second human's balls drew in the host's — audit
            // 2026-10-17); one human stays retail's player 0.
            Some(if self.human_seat.len() > 1 {
                self.human_seat[hx].0
            } else {
                0
            })
        } else if own != 0 && (own as usize) < self.ent.len() && self.ent[own as usize].class64 == 3
        {
            // A registered wizard seat answers with its player slot;
            // any other class-3 occupant reads the shared default
            // extension, i.e. 0.
            Some(self.owner_team(own).unwrap_or(0))
        } else {
            None
        };
        let base = match team {
            Some(team) => {
                let art = if mc2 {
                    crate::mc2::color_art(team)
                } else {
                    team
                };
                105 + 8 * art as usize
            }
            None => 52,
        };
        let ty = (base + size) as u16;
        if self.ent[i].type86 != ty {
            self.set_sprite(i, ty);
            if mc2 {
                // SetManaSphereColorAndRot (EF:26744-77): every MC2
                // re-sprite overwrites the applied quad with the
                // per-size ROTATION constant — 14·(size+1), except
                // 13 at size 0 — replacing the art extents the
                // sprite setter derives.
                const ROT: [u16; 8] = [13, 28, 42, 56, 70, 84, 98, 112];
                let r = ROT[size.min(7)];
                let e = &mut self.ent[i];
                e.f78 = r;
                e.f80 = r;
                e.f82 = r;
                e.f84 = r;
            } else if size != 0 {
                let e = &mut self.ent[i];
                e.f80 /= 2;
                e.f82 /= 2;
                e.f84 /= 2;
            }
        }
    }

    /// Class-10 combat-effect dispatch. Returns terrain_dirty.
    pub(crate) fn effect_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        match self.ent[i].tick70 {
            0 => self.fire_tick(i, ctx),
            1 => self.spreader_tick(i),
            // sub_252B0 / sub_253F0 (states 2/3): the ambient and
            // smoke puffs — the bare family decrement (PRE-decrement
            // life test like the whole class-10 family), no anim
            // step, no sound. Without these arms an imported puff
            // fell through to the terrain-feature dispatch's
            // self-kill catch-all and died a tick after import.
            2 | 3 => {
                let life = self.ent[i].act_life;
                self.ent[i].act_life = life - 1;
                if life < 0 {
                    self.ent[i].flags |= 0x400;
                }
                false
            }
            6 => self.standing_fire_tick(i, ctx),
            13 => self.smoke_puff_tick(i, false),
            // sub_258A0 (:28489), the mana-scatter puff twin.
            14 => self.smoke_puff_tick(i, true),
            5 => {
                // :28285-87 — PRE-decrement life test (class-10
                // family): the splash animates 9 ticks, not 8.
                let life = self.ent[i].act_life;
                self.ent[i].act_life = life - 1;
                if life < 0 {
                    // :28294 — retail frees and returns here: no anim
                    // step and no sound on the death tick.
                    self.ent[i].flags |= 0x400;
                    return false;
                }
                self.anim_advance(i);
                // :28288-91 — the one-shot splash sound, latched on the
                // same `& 2` bit the rest of the family uses.
                if self.ent[i].flags & 2 == 0 {
                    self.ent[i].flags |= 2;
                    self.snd(27, i);
                }
                false
            }
            12 => self.possess_flash_tick(i, ctx),
            16 => self.lava_bomb_tick(i),
            17 => self.blast_ring_tick(i, ctx),
            18 => self.eruption_tick(i, ctx),
            38 => self.undead_army_tick(i),
            19 => self.plume_tick(i, ctx),
            23 => self.hit_flash_tick(i, ctx),
            26 => self.duel_tether_tick(i, ctx),
            25 => self.steal_flash_tick(i, ctx),
            40 => self.storm_cloud_tick(i, ctx),
            41 => self.ball_tick(i, ctx),
            // Action 0x3E = the (10,57) RANDOM-VALUE mana sphere
            // (`sub_35FB0` EF:26318). Its physics core — the settle
            // gate `byte_0x39_57`, gravity `word_0x2C_44 -= 16` clamp
            // −128, the −impact/4 terrain bounce zeroed at ≤16, and
            // the grounded downhill-roll damping — is byte-identical
            // to the (10,39) ball's action 0x29 handler
            // (`TransformArcherToMana_35940` EF:26015), which the port
            // already services via `ball_tick`. The two retail handlers
            // differ only in the collection path (m57 has the
            // `word_0x68_104` spawn-(10,0) despawn branch, the ball has
            // the owner-transfer/sound-4 code) — neither runs while the
            // sphere is falling. Only imported m57 spheres ever carry
            // this action (native spawns them with 0x29 via
            // `spawn_mana_ball`); action 62 is m57-exclusive in the
            // corpus, so routing it here services the level-start
            // gravity fall without touching any native golden.
            62 => self.ball_tick(i, ctx),
            42 => {
                self.grave_tick(i);
                false
            }
            85 => self.mc2_mine_tick(i, ctx), // Magic Mine (10,78), action 0x55
            58 => self.napalm_tick(i, ctx),
            59 => {
                self.mana_magnet_tick(i);
                false
            }
            60 => self.death_field_tick(i, ctx),
            _ => false,
        }
    }

    /// sub_263C0 (:28949-62), class-10 state 26 — the DUEL TETHER: a
    /// STATIONARY 8-tick, 4-tile grip box planted where the dart
    /// struck. `+26++`, `life--`, reap below zero, else the anim step
    /// and one ch4 AREA write of its own `+44` over the ctor's
    /// 512/512 extents. Its `+44` is the DART's, not the ctor's 200
    /// — the explode tail overwrites it (`v19[22]`, :63202). The
    /// victim's intake latches the CASTER-side pull (:55663-82), and
    /// `sub_120B0` names the caster through the letter's source, the
    /// writer's own `+24` (:17309/:17381). See
    /// [`crate::engine::world::mc1_tether_static_off`].
    fn duel_tether_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28956-58 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28955 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28959 — the anim step, then the ch4 AREA write. That is
        // the whole handler.
        self.anim_advance(i);
        let amt = self.ent[i].f44 as u32;
        if !crate::engine::world::mc1_tether_static_off() {
            // ⭐⭐ **THE TETHER NEVER MOVES AND NEVER NAMES A VICTIM.**
            // `sub_263C0` (:28949-62, and it carries remc1's
            // `//SYNCHRONIZED WITH REMC1` marker) is twelve lines:
            // `+26++`, `life--`, reap below zero, else `sub_42510`
            // (the anim step) and `sub_120B0(a1x, 4u, +44)` — the
            // plain channel-4 AREA write over the tether's own
            // extents, which its ctor `sub_3AF80` (:47116) sets to
            // 512/512 via `sub_37130_374F0(v2, 512, 512)`. There is
            // no position write, no `+146` read and no victim-death
            // test anywhere in it. The pre-dig transport followed the
            // victim with `move_relink` and mailed him directly, so a
            // duelled wizard stayed gripped however far he flew, and
            // the tether's whole pose lane was ours rather than
            // retail's (mc1l48 t=59119-59127 records slot 20 parked
            // at (10575, 30298, 2096) for its entire eight-tick life
            // while the port walked it after rival 712).
            self.area_write(i, 4, amt, ctx, false, false);
        } else {
            let victim = self.ent[i].f146;
            if let Some(hx) = self.hx_of_tag(victim) {
                let (x, y, z) = self.seat_at(ctx, hx);
                self.move_relink(i, x, y, z);
            } else if victim != 0 {
                let v = &self.ent[victim as usize];
                if v.flags & 0x400 != 0 || v.act_life < 0 {
                    self.ent[i].flags |= 0x400;
                    return false;
                }
                let (x, y, z) = (v.x, v.y, v.z);
                self.mail_write(MailTarget::Pool(victim as usize), 4, amt, i as u16);
                self.move_relink(i, x, y, z);
            }
        }
        false
    }

    /// sub_299D0 (:31263), class-10 STATE 60 — the real GLOBAL DEATH
    /// field. LAW: the class-10 table is keyed by STATE, not MODEL
    /// (model-keying lands on state 55's terrain-raising volcano
    /// riser; cross-check against the napalm cloud's state 58 →
    /// sub_29780). Verbatim: while +26 (32 from the ctor) runs, tick
    /// it down with sound 43 (the audible priming tick-tock); then
    /// ONE full-pool sweep — every enemy entity within 0xA00 (10
    /// tiles) by PURE 2D DISTANCE (sub_423D0 is x/y only: an infinite
    /// vertical kill cylinder): class 2/5 die instantly (life = -1,
    /// no kill credit, no explosion effect), class 3 take the +44
    /// (7000) on ch0, own-team skipped, and an in-range class-9/10
    /// re-arms the field's OWN life to 0 (verbatim quirk,
    /// inconsequential — it frees this tick regardless). Finish:
    /// sound 44 at the field AND at the owner, the sub_44BE0(owner, 3)
    /// full-screen PALETTE FLASH — the violet wash, armed only when the
    /// field's owner is the local player ([`crate::engine::features::PalFlash`])
    /// — then free. NO terrain change, NO drift, NO entity visual: the
    /// screen flash IS the spell's only sighting, and the ctor's
    /// speed/heading/extents are dead weight.
    fn death_field_tick(&mut self, i: usize, _ctx: &MobCtx) -> bool {
        if self.ent[i].f26 > 0 {
            self.ent[i].f26 -= 1;
            self.snd(43, i);
            return false;
        }
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        if pre >= 0 {
            let (fx, fy, own, amt) = {
                let e = &self.ent[i];
                (e.x, e.y, e.id24, e.f44 as u32)
            };
            for j in 1..self.ent.len() {
                // (the human's (3,0) record is reached through his
                // own mail channel, not as a pool letter — yet)
                if j == i || self.hx_of_rec(j).is_some() {
                    continue;
                }
                let (class, team) = (self.ent[j].class64, self.ent[j].id24);
                if class == 0 || team == own {
                    continue;
                }
                let d2 = Self::dist2_sq(fx, fy, self.ent[j].x, self.ent[j].y);
                if Self::isqrt(d2 as u32) >= 0xA00 {
                    continue;
                }
                match class {
                    2 | 5 => {
                        self.ent[j].act_life = -1;
                        // Retail stamps no killer; the stats ledger
                        // bills the field's owner.
                        if class == 5 {
                            self.stats.0.hint(j, own);
                        }
                    }
                    // sub_12B50 (:31296) — the field's wizard arm is
                    // the binary's OTHER single-target write, so the
                    // 7000 stacks onto a stale amount rather than
                    // replacing it.
                    3 => self.mail_write_single(MailTarget::Pool(j), 0, amt, own),
                    9 | 10 => self.ent[i].act_life = 0,
                    _ => {}
                }
            }
            self.snd(44, i);
            if let Some(hx) = self.hx_of_tag(own) {
                self.snd_player(hx, 44);
                // sub_44BE0(owner, 3): row 3 = red +48 / blue
                // saturated over the untouched green — the violet
                // flash. Gated on the owner being the local player,
                // exactly as sub_44BE0's slot compare is.
                self.pal_flash.arm_seat(3, hx);
            }
        }
        self.ent[i].flags |= 0x400;
        false
    }

    /// sub_29780 (:31140), class-10 state 58 (the m53 Wall of Fire
    /// cloud). The original branches on `IsHiddenWord`:
    /// - base MC1 (`!IsHiddenWord`, below): 15 waves of standing flames
    ///   over the impact ring (112-unit pitch over SEARCH rings 0..1,
    ///   ±64 jitter, the -96 2x2-center recenter): wave 0 = a persistent
    ///   14-tick ground fire patch, waves 1..14 = 1-tick flame sheets
    ///   climbing 128 units per wave — the rising fire curtain. The
    ///   cloud's own ch0 write is +44/maxLife; the flames' inherited
    ///   100/tick is the damage.
    /// - Hidden Worlds ([`Self::napalm_tick_hw`]): a different geometry —
    ///   one EXPANDING (10,0) ring per tick (160-unit pitch), stepped
    ///   `(var26+2)%7`, until `actLife` runs out; sound 30 once. The
    ///   `IsHiddenWord=true` else-branch (remc1hw :29740; the HW path,
    ///   NOT a multiplayer branch — SURVEY-MC1HW §2).
    fn napalm_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.is_hidden_worlds() {
            return self.napalm_tick_hw(i, ctx);
        }
        // Retail burns the cloud's own life down every tick
        // (:31150-52) — inert under the 15-wave cap, but the mc1l5
        // take shows the decrement in every recorded (10,53) pair.
        {
            let e = &mut self.ent[i];
            e.act_life -= 1;
            if e.act_life < 0 {
                e.flags |= 0x400;
                return false;
            }
            e.f80 = 512;
            e.f82 = 512;
            e.f84 = 2048;
        }
        // The wall's ONLY damage: the cloud's single f44/maxLife
        // write — 24464/128 = 191/tick with the bolt-copied +44,
        // exactly the victim life slope the mc1l5 take records.
        let amt = self.ent[i].f44 as u32 / self.ent[i].max_life.max(1);
        self.area_write(i, 0, amt, ctx, false, false);
        // Live operands, as in `blast_ring_tick`: :31164-79 re-reads
        // `+72/+74/+76` per cell and `+24`/`+44`/`+26` after the
        // allocation, and the wave counter at :31184 is read live.
        let live = !no_effect_ring_live_operands();
        let hoisted = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f44, e.f26)
        };
        let wave = hoisted.5;
        let cells = self.ring_cells_pub(0, 1);
        // :31160 — ONE cloud-LCG step after the iterator opens,
        // BEFORE the per-cell jitter pairs. Without it every flame's
        // jitter reads one draw early and the cloud's own rand lane
        // trails — the mc1l5 t=23404-23430 family: 15 re-popped free
        // slots a tick carrying fractionally shifted corpse x/y.
        self.ent_rand(i);
        for (dx, dy) in cells {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let (x, y, z) = if live {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            } else {
                (hoisted.0, hoisted.1, hoisted.2)
            };
            let fx = x.wrapping_add((112 * dx as i32 + (d1 % 0x81) as i32 - 64 - 96) as u16);
            let fy = y.wrapping_add((112 * dy as i32 + (d2 % 0x81) as i32 - 64 - 96) as u16);
            if let Some(f) = self.spawn_effect(6, fx, fy, z) {
                // :31167-79 — every operand below is read off the cloud
                // AFTER the allocation, so a seizing child reads itself.
                let (own, f44, wave) = if live {
                    let e = &self.ent[i];
                    (e.id24, e.f44, e.f26)
                } else {
                    (hoisted.3, hoisted.4, hoisted.5)
                };
                let e = &mut self.ent[f];
                e.id24 = own;
                // Inherited, not the flame ctor's 50 (:31168) —
                // inert under the decorative stamp, but the field
                // is what the take records on every wall flame.
                e.f44 = f44;
                e.act_life = if wave == 0 { 14 } else { 1 };
                e.type86 += 7;
                e.f26 = 7; // :31180 — an ASSIGN, not an accumulate
                e.f46 = wave * 128;
                // :31169 — +18 bit0 (0x10000): NO ch0 broadcast, the
                // flames are pure light show (without it all 15 live
                // cells accumulate ~100 each into ONE mailbox read ≈
                // 6,000/tick — the reported griffon instakill; retail
                // can never one-shot: 10,000 life / 191 ≈ 53 ticks).
                // +16 bit7 (0x80): no smoke-puff LCG draw — the port's
                // extra rand pulls desynced every wanderer downstream
                // of a wall cast in the take.
                e.flags |= 0x10080;
            }
        }
        let wave = if live { self.ent[i].f26 } else { wave };
        self.ent[i].f26 = wave + 1;
        if wave >= 14 {
            self.ent[i].flags |= 0x400;
        }
        false
    }

    /// The Hidden Worlds Wall-of-Fire cloud (sub_29780 `IsHiddenWord`
    /// else-branch, remc1hw :29740). Where base MC1 stacks rising waves,
    /// HW paints ONE expanding ground ring per tick: the (10,0) fire on
    /// a 160-unit grid at radius `var26` (`+26`), the radius stepped
    /// `(var26+2)%7` so it sweeps 0,2,4,6,1,3,5, running until the
    /// cloud's `actLife` expires (no wave cap — the spawner's life is the
    /// terminator). Sound 30 plays once (the `+16` bit-1 latch plus a
    /// persistent 0x10000 marker set together on the first surviving
    /// tick). The cloud's own extent tracks the ring (192·var26 wide =
    /// `(768·var26)>>2`, 512 tall); each child is a full 512³ (10,0)
    /// flame inheriting the cloud's owner and yaw, keeping the (10,0)
    /// ctor's own life/damage.
    ///
    /// NOTE (SURVEY-MC1HW §7 — emit chain UNTRACED): the observable
    /// damage/duration follow the cloud's spawn params (`f44`/`max_life`)
    /// and WHICH creator HW's Fire Storm routes through (`sub_3B8E0`
    /// life-128/f44-100 vs `sub_3BC60` life-6/f44-3000), and whether HW
    /// spell-20 spawns a napalm cloud at all beside the homing meteor.
    /// This handler is faithful for any params; only the trigger is open.
    fn napalm_tick_hw(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // Class-10 PRE-decrement family (the sub_24F60..sub_26D20
        // batch law): the life test reads the value BEFORE the
        // decrement, so a 6-life cloud burns SEVEN ticks (pre-values
        // 6..0) — corpus-pinned on mc1hwl0 slot 522: every napalm
        // burst is 7 × 833 = 5831 per cloud, not 6 × 833.
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        if pre < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 0x10002;
            self.snd(30, i);
        }
        let var26 = self.ent[i].f26;
        self.extents(i, 192u16.wrapping_mul(var26 as u16), 512);
        let amt = self.ent[i].f44 as u32 / self.ent[i].max_life.max(1);
        self.area_write(i, 0, amt, ctx, false, false);
        // Live operands, as in `blast_ring_tick` (hw:29757-70 and the
        // hw:29772 ring step).
        let live = !no_effect_ring_live_operands();
        let hoisted = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24, e.f30)
        };
        // ⭐ THE ITERATOR'S OWN DRAW (hw:29753): retail steps the LCG
        // ONCE between `sub_11410` and the first `sub_114B0`, before
        // any cell is read — the base-MC1 arm above has always
        // carried it and the HW twin was missing it, so every HW wall
        // painted its ring off a stream one step behind and every
        // downstream consumer of the cloud's `+4` desynced
        // (mc1hwl0 t=2751, cloud 494's first paint).
        self.ent_rand(i);
        for (dx, dy) in self.ring_cells_pub(var26 as i32, var26 as i32) {
            let d1 = self.ent_rand(i);
            let d2 = self.ent_rand(i);
            let (x, y, z) = if live {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            } else {
                (hoisted.0, hoisted.1, hoisted.2)
            };
            let fx = x.wrapping_add((160 * dx as i32 + (d1 % 0x81) as i32 - 64 - 96) as u16);
            let fy = y.wrapping_add((160 * dy as i32 + (d2 % 0x81) as i32 - 64 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                let (own, yaw) = if live {
                    let e = &self.ent[i];
                    (e.id24, e.f30)
                } else {
                    (hoisted.3, hoisted.4)
                };
                let e = &mut self.ent[f];
                e.id24 = own;
                e.f30 = yaw; // child copies the cloud's yaw (var30)
                e.flags |= 0x10080;
                e.f80 = 512;
                e.f82 = 512;
                e.f84 = 512;
                e.f26 = 0;
            }
        }
        let var26 = if live { self.ent[i].f26 } else { var26 };
        self.ent[i].f26 = (var26 + 2) % 7;
        false
    }

    /// sub_252D0 (:28199), class-10 state 6: the STANDING fire (tree
    /// burn / ground wave). The flame sprite family walks +86 up 7
    /// steps then back down over the last 12 ticks; the fire rides
    /// ground + f46 (3/4 up a burning tree's trunk), dies on water,
    /// and — while +18 bit0 (0x10000) is clear — broadcasts +44 ch0
    /// through the /10 tree-discount writer EVERY tick, so burning
    /// trees torch their neighbors (~5/tick) and forests chain-burn.
    /// Deviation: the original also spits a (10,13) smoke puff on
    /// 1/7 of shrink ticks — the LCG draw is kept for stream parity,
    /// the puff itself is skipped (decorative).
    fn standing_fire_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let pre = self.ent[i].act_life;
        self.ent[i].act_life = pre - 1;
        let mut done = pre < 0;
        if !done {
            // sub_44C10 player-distance bookkeeping omitted (HUD/AI).
            if self.ent[i].act_life < 12 {
                if self.ent[i].f26 > 0 {
                    self.ent[i].f26 -= 1;
                    self.ent[i].type86 -= 1;
                    if self.ent[i].flags & 0x80 == 0 {
                        let d = self.ent_rand(i);
                        if d % 7 == 0 {
                            // The (10,13) exhaust puff (:28224-33).
                            // +26 = 100 parks it PAST the tick's
                            // 16-tick drift window, so a fire's smoke
                            // rises straight and never walks its
                            // sprite up; life is overridden to 15
                            // (max_life keeps the ctor's roll) and the
                            // sprite starts two rows above the ctor's
                            // 67.
                            let (fx, fy, fz, own) = {
                                let e = &self.ent[i];
                                (e.x, e.y, e.z, e.id24)
                            };
                            if let Some(p) = self.spawn_effect(13, fx, fy, fz) {
                                // :28230-37 — `+24` is read off the fire
                                // AFTER the allocation (identity on a
                                // self-seizure; `+86 += 2` already reads
                                // the child). See [`no_mc1_self_seize`].
                                let seized = self.mc1_self_seized(p, i);
                                let e = &mut self.ent[p];
                                e.f26 = 100;
                                e.act_life = 15;
                                if !seized {
                                    e.id24 = own;
                                }
                                e.type86 = e.type86.wrapping_add(2);
                            }
                        }
                    }
                }
            } else if self.ent[i].f26 <= 6 {
                self.ent[i].f26 += 1;
                self.ent[i].type86 += 1;
            }
            let (x, y, f46) = {
                let e = &self.ent[i];
                (e.x, e.y, e.f46)
            };
            self.ent[i].z = (self.ground_z(x, y) as i16).wrapping_add(f46);
            if self.on_water_pub(x, y) {
                done = true;
            }
        }
        if done {
            self.ent[i].flags |= 0x400;
        }
        // The damage write runs even on the death tick (:28255-56
        // falls through LABEL_11).
        if self.ent[i].flags & 0x10000 == 0 {
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 0, amt, ctx, true, false);
        }
        false
    }

    /// sub_49890/499C0/49A50 (:57662-57790), class-2 model 0 — the
    /// TREE. State 0: ch0 intake; death sparks a (10,6) standing fire
    /// owned by the attacker, riding 3/4 up the trunk, with ONE
    /// tree-LCG draw setting rand%60+130 as BOTH the fire's life and
    /// the tree's burn timer; the tree goes un-hittable, state 1.
    /// State 1: burn down; below 60 → state 2 + the charred sprite
    /// (83→226, 84→227). All states follow the ground and splash-die
    /// on water. (Pool-full fire spawn skips the draw and retries
    /// next tick, as the original.)
    pub(crate) fn tree_tick(&mut self, i: usize, splash_water: bool) {
        match self.ent[i].tick70 {
            0 => {
                self.ent[i].flags |= 0x20000; // +18 |= 2 (:57674)
                if self.ent[i].mail[0].1 != 0 {
                    let (amt, src) = self.ent[i].mail[0];
                    self.ent[i].mail[0].1 = 0;
                    self.ent[i].act_life -= amt as i32;
                    if self.ent[i].act_life < 0 {
                        let (x, y, z, f84) = {
                            let e = &self.ent[i];
                            (e.x, e.y, e.z, e.f84)
                        };
                        if let Some(f) = self.spawn_effect(6, x, y, z) {
                            // ⭐ THE SECOND `+24` HOP IS NOT THE
                            // IDENTITY. :57683 reads
                            // `pool[a1->+94].+24` — the OWNER TAG OF
                            // THE RECORD THAT BROADCAST, not the
                            // broadcasting slot — and it reads it
                            // blind, through a slot the reap may
                            // already have freed (the freed-slot
                            // stale-bytes law; retail keeps the whole
                            // record until the slot is re-minted).
                            // mc1l4 t=1239: the killer is (10,1)
                            // spreader 224, dead and freed since the
                            // tick top, whose `+24` still names the
                            // (10,0) fire 390 that seeded it — so
                            // retail's flame belongs to 390 and the
                            // port's belonged to 224, and every ch0
                            // and XP credit downstream reads it.
                            // The human's carpet is out of pool, so
                            // its sentinel IS its own tag.
                            self.ent[f].id24 = if self.is_human_tag(src) {
                                src
                            } else {
                                self.ent[src as usize].id24
                            };
                            self.ent[f].f46 = (3 * f84 as i32 / 4) as i16;
                            let d = self.ent_rand(i);
                            let burn = (d % 60 + 130) as i32;
                            self.ent[f].act_life = burn;
                            self.ent[i].act_life = burn;
                            self.ent[i].flags &= !8; // no longer hittable
                            self.ent[i].tick70 = 1;
                            // `sub_41CC0_42000(_, a1, a1 + 72)` (:57698,
                            // the fn's SOLE call site) — re-head the
                            // TREE so the flame, head-linked an
                            // instruction ago, paints after it and
                            // therefore in FRONT of it. Behaviour-inert
                            // here: the line above just cleared the
                            // tree's hittable bit, so the re-headed tree
                            // satisfies no scan predicate, and a relink
                            // preserves relative order for every other
                            // member of the tile.
                            self.relink_head(i);
                        }
                    }
                }
                self.tree_ground_water(i, splash_water);
            }
            1 => {
                self.ent[i].act_life -= 1;
                if self.ent[i].act_life < 60 {
                    self.ent[i].tick70 = 2;
                    match self.ent[i].type86 {
                        83 => self.set_sprite(i, 226),
                        84 => self.set_sprite(i, 227),
                        _ => {}
                    }
                }
                self.tree_ground_water(i, splash_water);
            }
            _ => self.tree_ground_water(i, splash_water),
        }
    }

    /// The tree handlers' shared tail (:57703-11): z follows the live
    /// ground; water under the trunk → splash (owner passed on) and
    /// despawn.
    fn tree_ground_water(&mut self, i: usize, splash_water: bool) {
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
        if splash_water && self.on_water_pub(x, y) {
            let owner = self.ent[i].id24;
            let z = self.ent[i].z;
            if let Some(s) = self.spawn_effect(5, x, y, z) {
                self.ent[s].id24 = owner;
            }
            self.ent[i].flags |= 0x400;
        }
    }

    /// sub_49AA0_49DE0 / sub_49B50_49E90 (:57770/:57805), class-2
    /// states 3/9 — the standing stone and the bad stone: the static
    /// draw bit (+18 |= 2), then the per-tick terrain snap that rides
    /// deforming ground. No water arm — statics stand in the sea
    /// (only trees splash-die).
    pub(crate) fn static_snap_tick(&mut self, i: usize) {
        self.ent[i].flags |= 0x20000;
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        self.ent[i].z = self.ground_z(x, y) as i16;
    }

    /// sub_24F60 (:28047): the fire. One ch0 broadcast + terrain
    /// reaction on the first active tick, then flicker/anim out.
    fn fire_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        if self.ent[i].f26 & 3 != 0 {
            self.ent[i].f26 -= 1;
            return false;
        }
        // :28068-70 — PRE-decrement life test (class-10 family): every
        // fire burns one tick longer than the post form allowed.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28073 — the ONE ground sample, taken BEFORE the terrain
        // reaction and held through it: a fire that scorches its own
        // cell still gates and z-rules against the PRE-dig ground
        // (both retail binaries and the MC2 twin pass this v3 into the
        // z rule). Re-sampling after the dig flips the clamp direction
        // on a fire spawned under freshly painter-raised ground.
        let (x, y) = (self.ent[i].x, self.ent[i].y);
        let g = self.ground_z(x, y);
        let mut dirty = false;
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            if self.ent[i].flags & 0x10000 == 0 {
                let amt = self.ent[i].f44 as u32;
                self.area_write(i, 0, amt, ctx, false, false);
            }
            // Terrain reaction (:28083-104): burn conversions, else a
            // small scorch crater on flat, low, dry ground.
            let z = self.ent[i].z;
            // The reaction cell is ROUNDED (:28075-77 `(x+128)>>8` —
            // the MC2 twin already had this); the water probe stays on
            // the plain `>>8` cell AND is the ANGLE probe sub_11760
            // (:28098), not the tile-type one — a shore cell (type 45,
            // angle nibble 0) is WATER to this gate. mc1l0 t=4290: the
            // type probe scorched a wave cell retail leaves alone.
            let t = tile(
                (x.wrapping_add(128) >> 8) as u8,
                (y.wrapping_add(128) >> 8) as u8,
            );
            let ty = self.t.tile_type[t];
            let conv = match ty {
                26 => Some(0x14),
                10 => Some(0x15),
                11 => Some(0x16),
                _ => None,
            };
            if let Some(c) = conv {
                // The real sub_33800 paint call (:28086-92) — the
                // damage-stage TYPES come from PAINT_BC (10/11/12), NOT
                // the paint code (writing the code as the type =
                // wrong texture). a1/a2 are leftover registers in the
                // original; they only seed corner_orient ties.
                self.paint(0, 0, t, c);
                dirty = true;
            } else if ty != 0
                && !(6..=0x22).contains(&ty)
                && self.t.angle[t] & 7 != 1
                && (z as i32 - g) <= 128
                && !self.on_water(x, y)
            {
                let d = self.ent_rand(i);
                self.dig_scorch(i, -((d % 7) as i16));
                dirty = true;
            }
            let d2 = self.ent_rand(i);
            self.ent[i].f46 = ((d2 % 0x41) as i32 - 32) as i16;
            self.snd(3, i); // :28118
        }
        // z rule sub_42000_42340 (:52576-601, called :28116 with
        // (ground, 0, 0, flicker)): ABOVE ground the fire drifts by
        // the fixed flicker delta each tick; below ground it clamps
        // UP to ground; at ground it stays. The original never pulls
        // a fire down to terrain — a midair explosion (max-range
        // fireball expiry, the meteor's trail) stays at altitude.
        let g = g as i16;
        if self.ent[i].z > g {
            self.ent[i].z = self.ent[i].z.wrapping_add(self.ent[i].f46);
        }
        if self.ent[i].z < g {
            self.ent[i].z = g;
        }
        self.anim_advance(i);
        dirty
    }

    /// sub_25130 (:28127): the fire-spreader — one ring of fires at
    /// radius +26 (0 = the single corpse flame), then gone.
    fn spreader_tick(&mut self, i: usize) -> bool {
        // :28142-48 — the life test reads the PRE-decrement value, so a
        // life-1 puff ticks TWICE before it is freed.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        // :28149-53 — the `& 2` latch guards ONLY the one-shot sound.
        // The ring spawn below runs on EVERY tick, exactly as the
        // sibling blast_ring_tick does; hoisting the whole body under
        // this latch halved the corpse flame (one pass instead of two)
        // and with it every "castle as weapon" crush.
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            self.snd(3, i); // :28152
        }
        // The same live-operand law as `blast_ring_tick`: sub_25130 keeps
        // `a1` in one register and re-reads `+72/+74/+76` per cell
        // (:28167-70) and `+24`/`+30`/`+16` AFTER the allocation
        // (:28173-76), so a self-seizure re-homes the rest of the spray.
        let live = !no_effect_ring_live_operands();
        let hoisted = {
            let e = &self.ent[i];
            (
                e.x,
                e.y,
                e.z,
                e.id24,
                e.f30,
                e.f26.max(0) as i32,
                e.flags & 0x10000,
            )
        };
        let radius = hoisted.5;
        let cells = self.ring_cells_pub(radius, radius);
        for (dx, dy) in cells {
            // :28161-63 — the per-cell draw is the SKIP TEST alone: spawn
            // iff `2·(v5 % 0x9D / 79) − 1 > 0`, i.e. `v5 % 157 >= 79`
            // (~50%). The `& 1` low-bit test picked a DIFFERENT set of
            // cells even for the same rand value.
            let s = self.ent_rand(i);
            if s % 0x9D < 79 {
                continue;
            }
            // :28165-70 — the jitter pair is drawn ONLY on the spawn
            // branch. Rolling it unconditionally (once per skipped cell
            // too) desynced the ring's rand stream, so every downstream
            // cell's skip decision — and the corpse-flame fire SET —
            // diverged from retail (57 missing / 210 extra (10,0)).
            let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
            // x - 96 + 192·dx + jitter (:28167-70), 2x2-center recenter.
            let (x, y, z) = if live {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            } else {
                (hoisted.0, hoisted.1, hoisted.2)
            };
            let fx = x.wrapping_add((192 * dx as i32 + j1 - 96) as u16);
            let fy = y.wrapping_add((192 * dy as i32 + j2 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                let (owner, aim, inherit) = if live {
                    let e = &self.ent[i];
                    (e.id24, e.f30, e.flags & 0x10000)
                } else {
                    (hoisted.3, hoisted.4, hoisted.6)
                };
                self.ent[f].id24 = owner;
                self.ent[f].f30 = aim; // :28176 — inherit the spreader's f30
                self.ent[f].flags |= 0x80 | inherit;
            }
        }
        false
    }

    /// sub_25CE0 (:28671): the growing fire-ring blast — per-tick ch0
    /// at +44/maxLife, a ring of fires per tick, radius (+2) % 11.
    fn blast_ring_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28685-88 — the life test reads the PRE-decrement value, so the
        // ring runs one more pass than the post-decrement form allows.
        // Measured 9 -> 10 passes, 376 -> 417 fires; the per-tick ch0
        // write is f44/max_life, so the ring was landing 90% of its
        // authored damage.
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2 | 0x10000;
            self.snd(30, i);
        }
        let radius = self.ent[i].f26.max(0) as i32;
        {
            // Half-extents 192·ring, z 512 (:28696-97) — no floor; the
            // AABB damage test sums both parties' extents, so ring 0
            // still hits a victim on the impact point.
            let e = &mut self.ent[i];
            e.f80 = (768 * radius / 4) as u16;
            e.f82 = e.f80;
            e.f84 = 512;
        }
        let per_tick = (self.ent[i].f44 as u32) / self.ent[i].max_life.max(1);
        self.area_write(i, 0, per_tick, ctx, false, false);
        // ⭐⭐ THE SPRAYER'S OWN RECORD IS A LEGAL ALLOCATION VICTIM.
        // `a1x` never leaves `%ebx` in the shipped routine (CARPET.EXE
        // file 0x3e4d8 = VA 0x25CE0 + 0x187f8): the position operands are
        // re-read INSIDE the loop — `movswl 0x48(%ebx),%esi` at 0x3e5cd
        // (+72 x), `movswl 0x4a(%ebx),%eax` at 0x3e5ff (+74 y),
        // `mov 0x4c(%ebx),%ax` at 0x3e614 (+76 z) — and `+24`/`+30` are
        // read AFTER the allocator returns (`mov 0x18(%ebx),%ax` at
        // 0x3e632 and `mov 0x1e(%ebx),%ax` at 0x3e63a, the call at
        // 0x3e624 being sub_373F0_377B0 = file 0x4fbe8), with the ring
        // step reading `+26` off `%ebx` once more after the loop
        // (`mov 0x1a(%ebx),%dx` at 0x3e68b). When the free stack is empty
        // `new_event` SEIZES a recycle victim, and that victim can be the
        // ring itself: `%ebx` then aliases the child fire, so the rest of
        // the spray is laid around the CHILD's position, every member
        // takes the child's `+24` (which `new_event` stamped = its own
        // slot), and the ring step lands on the child's `+26` = 0 -> 2.
        // mc1l37 t=5670 is exactly that: ring slot 69, f26 = 5, seized by
        // its own first child; retail stamps `id24` = 69 on all 26 fires
        // and shifts members 2..26 by the (682, 402) child-minus-ring
        // delta, while the port's hoisted copies kept the ring's owner
        // (504), its position, and wrote `(5+2)%11 = 7` over the child's
        // `+26`. Same shape at t=23620 (ring slot 702, owner 523).
        let hoisted = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        let live = !no_effect_ring_live_operands();
        let _ = self.ent_rand(i); // pre-loop draw (:28699)
        let cells = self.ring_cells_pub(radius, radius);
        for (dx, dy) in cells {
            // x - 96 + 160·dx + rand%0x81 - 64 (:28707-09): the -96
            // recenters the ring table's 2x2 zero block.
            let (x, y, z) = if live {
                let e = &self.ent[i];
                (e.x, e.y, e.z)
            } else {
                (hoisted.0, hoisted.1, hoisted.2)
            };
            let j1 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let j2 = (self.ent_rand(i) % 0x81) as i32 - 64;
            let fx = x.wrapping_add((160 * dx as i32 + j1 - 96) as u16);
            let fy = y.wrapping_add((160 * dy as i32 + j2 - 96) as u16);
            if let Some(f) = self.spawn_effect(0, fx, fy, z) {
                // :28716 / 0x3e632 — read AFTER the allocation, so a
                // seizing child self-assigns the ctor's own-slot stamp.
                let owner = if live { self.ent[i].id24 } else { hoisted.3 };
                self.ent[f].id24 = owner;
                // :28717 — the ring's children inherit its +30 exactly
                // as the spreader's do (:28176 above). The port set
                // id24/flags/extents/+26 and dropped this one line, so
                // every blast-ring fire was born heading 0: mc1l32
                // t=23132 shows 75 newborn (10,0) rows, all children of
                // one (10,17) ring, all `heading: retail 724 port 0`
                // — 724 being the ring's own f30.
                self.ent[f].f30 = self.ent[i].f30;
                self.ent[f].flags |= 0x80 | 0x10000;
                self.extents(f, 512, 512);
                self.ent[f].f26 = 0;
            }
        }
        // 0x3e68b — `+26` is re-read off `%ebx` after the loop, NOT the
        // radius the ring opened with.
        let step = if live {
            self.ent[i].f26.max(0) as i32
        } else {
            radius
        };
        self.ent[i].f26 = ((step + 2) % 11) as i16;
        false
    }

    /// sub_262D0 (:28898): the bolt hit-flash — one ch0 write and the
    /// thunder-crack 24 (:28911), brief.
    fn hit_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28906-08 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28905 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            self.blast_hits.set(i, false);
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let amt = self.ent[i].f44 as u32;
            let hits = self.area_write(i, 0, amt, ctx, false, false);
            self.blast_hits.set(i, hits != 0);
            self.snd(24, i);
            self.ent[i].act_life = 1;
        }
        // ⚠ NO ANIMATION STEP. The class-10 flash family is NOT uniform
        // here: `sub_25760` (:28437, possess), `sub_26360` (:28937,
        // steal) and `sub_263C0` (:28959, tether) each open their live
        // arm with `sub_42510_42850`, and THIS one does not (:28908-19
        // is the whole live arm — the ch0 write, sound 24, the life
        // pin, the flag). A 2026-07-21 audit batch read the family as
        // uniform and added the step here too; the raw shadow measured
        // the cost as 2,092 `(10,23) frame88` rows on mc1l42, `retail
        // 0 port 1` on every hit flash in the take.
        false
    }

    /// sub_26360 (:28924): m11's mana-steal flash — one ch3 write.
    fn steal_flash_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        // :28933-35 — the life test reads the PRE-decrement value: the
        // whole class-10 effect family is pre-decrement in retail (the
        // class-9 flight handlers genuinely are not), so this runs one
        // more tick than the post-decrement form allows.
        // :28932 — retail bumps +26 every tick, BEFORE the life test, so
        // it counts even on the tick the flash dies.
        self.ent[i].f26 = self.ent[i].f26.wrapping_add(1);
        let life = self.ent[i].act_life;
        self.ent[i].act_life = life - 1;
        if life < 0 {
            self.ent[i].flags |= 0x400;
            return false;
        }
        if self.ent[i].flags & 2 == 0 {
            self.ent[i].flags |= 2;
            let amt = self.ent[i].f44 as u32;
            self.area_write(i, 3, amt, ctx, false, false);
        }
        self.anim_advance(i);
        false
    }

    /// The merge partner search: MC1 `sub_11D10` (:17127) and MC2
    /// `sub_10A50` (EF:3876) are the SAME routine, and they are a
    /// **map-tile ring walk, not a pool scan**. Base tile =
    /// `((pos + 128) >> 8) & 0xFF` — ROUNDED, not floored; ring count
    /// = `(applied_pitch + 255) >> 8` (the searcher's own +80 extent
    /// in tiles, no `.max(1)` — the area writers' `.max(1)` is a
    /// different routine); tiles are visited ring by ring outwards
    /// (`sub_11410`/`sub_10080` seed the walker at ring 0, `sub_114B0`/
    /// `sub_10130` yield each ring's tile offsets) and each tile's
    /// `mapEntityIndex` chain is walked; the FIRST admissible hit
    /// wins and the walk stops.
    ///
    /// This is why the doomsday fountain's shore pile merges one tick
    /// LATER than a pool scan does: mc2l24 slot 845 (a settled sphere
    /// at 55.97/228.99, +80 = 112 ⇒ ring 1 around tile 56/229) does
    /// NOT see slot 795 when it steps to 54.98/227.98 (tile 54/227 is
    /// outside the ring) even though the AABBs already overlap, and
    /// absorbs it only at 55.23/228.23 (tile 55/228). The pool scan
    /// merged it a tick early — a `missing:10,39` every time.
    ///
    /// Retail's admission is `+66/+67` (`filter_admits`) + `id !=
    /// id` + the AABB; every ball ctor stamps `xtype/xsubtype` =
    /// (10,39), so the explicit family test below IS that filter and
    /// keeps working for native balls (which carry no +66/+67). The
    /// port-only exclusions (fool's-mana spheres, soft-killed
    /// records) ride along. ⚠ The soft-kill exclusion models MC2's
    /// unlink-at-disable; MC1's sub_11D10 has NO disable gate — a
    /// castle-absorbed (still-linked) ball is retail-admissible for
    /// one tick and its mana would DUPE into the survivor. Kept
    /// port-wide until a corpus row asks for retail's loophole.
    ///
    /// RESIDUAL: the order WITHIN one ring comes from a data table
    /// (`bitmaps_E9980x`) the decompile does not carry; raster order
    /// stands in. It only decides which of two simultaneously
    /// overlapping partners is absorbed first.
    /// The MC1 ball-merge OWNER CONTEST (`sub_277D0` :29700-73),
    /// lifted out of the merge so it can be pinned directly: the pair
    /// channel restores `+144` every tick and is blind to it.
    pub(crate) fn mc1_ball_owner_contest(&mut self, i: usize, j: usize) {
        // MC1 owner rule (`sub_277D0` :29700): OWNED BEATS
        // UNOWNED — an unowned survivor ADOPTS the absorbed
        // ball's owner (:29717; this is how magnet-pulled
        // balls become claimed as they coalesce into the
        // claimed one). A class-10 owner (a grave's bank
        // tag) loses to a real owner (:29734-50); two
        // DIFFERENT real owners contest on the owner
        // wizards' +136 (:29755-73: strictly larger keeps
        // the survivor's owner, else the absorbed side
        // wins). Port note: MC1 wizard ents don't carry a
        // +136 bank (only castles do) and the human has no
        // pool entity, so both sides resolve 0 and the
        // contest falls to retail's else-arm (absorbed
        // side's owner) — structure faithful, operands
        // approximated. Mana is ALWAYS additive: the
        // reconstruction's two `*=` branches (:29750,
        // :29773) are transcription slips (every sibling
        // branch is `+=`).
        let (oi, oj) = (self.ent[i].f144, self.ent[j].f144);
        let is_c10 = |g: &Self, o: u16| {
            !self.is_human_tag(o) && (o as usize) < g.ent.len() && g.ent[o as usize].class64 == 10
        };
        // ⭐ THE CONTEST OPERAND IS THE OWNER WIZARD'S OWN
        // `+136` — its mana CEILING, read off its pool
        // record (:29760-66). The human's carpet is out of
        // pool here, so its ceiling arrives through the
        // ctx; a rival's is mirrored onto its entity by
        // the rival pass. Reading 0 for BOTH (the earlier
        // approximation) collapsed every contest onto
        // retail's else-arm, which hands the ball to the
        // ABSORBED side unconditionally. mc1l4 t=2722:
        // ball 432 lands on the human (358) in retail and
        // on Vodor (365) in the port, and the 9,500 of
        // ceiling it carries moves with it — both
        // wizards' `mana_max` part on the next census.
        let w136 = |g: &Self, o: u16| {
            // The OWNER's ceiling: the walkers' `ctx` is seat 0's, so
            // with several humans it weighed another player's
            // (audit 2026-10-17).
            if let Some(hx) = self.hx_of_tag(o) {
                g.human_ceiling_of(hx).min(i32::MAX as u32) as i32
            } else if (o as usize) < g.ent.len() {
                g.ent[o as usize].f136
            } else {
                0
            }
        };
        if oi == 0 {
            self.ent[i].f144 = oj;
        } else if oj != 0 && oi != oj {
            let (ci, cj) = (is_c10(self, oi), is_c10(self, oj));
            // Two distinct retail branches that share an
            // outcome: the class-10-loses arm and the
            // lost +136 contest — kept separate to match
            // the trace.
            #[allow(clippy::if_same_then_else)]
            if ci && !cj {
                self.ent[i].f144 = oj;
            } else if !ci && !cj && w136(self, oi) <= w136(self, oj) {
                self.ent[i].f144 = oj;
            }
        }
    }

    /// The MC2 ball-merge OWNER LADDER (`sub_36D50` EF:26919-26996,
    /// shipped `NETHERW.EXE` file 0x5B550-0x5B71D), the both-owned
    /// tail only — the caller has already handled the two unowned arms
    /// (EF:26933/:26944, the FIRST of which is the only one that
    /// carries the absorbed ball's `byte[2] & 0x20` claim lock
    /// across).
    ///
    /// ⭐⭐⭐ THE CONTEST OPERAND IS THE OWNER RECORD'S
    /// `maxMana_0x8C_140` — NOT the ball manas. The port scored the
    /// contest on `fj > fi` ("the larger contributor"), a documented
    /// approximation that stood because no graded take had two owned
    /// spheres merge. mc2l22 has ten of them and every one is a
    /// `player_ent_idx` + `f5a` pair row (`f5a` is
    /// `ball_resize`'s `mc2_ball_color(f144) + size`, so the colour
    /// base moves with the owner and the row is free evidence).
    ///
    /// ⭐⭐ AND THE CLASS-10 ARMS COME FIRST (EF:26951/:26956/:26964,
    /// EXE `cmp byte [edx+0x3f],0xa` at 0x5B603, 0x5B629, 0x5B662). A
    /// `(10,40)` mana-pit bank tag is a legal `playerEntityIndex`, and
    /// its `maxMana` is ENORMOUS — slot 61 at mc2l22 t=21958 holds
    /// 1,068,282 against the human's 930,914, slot 102 at t=23251
    /// holds 17,453,411 against rival 530's 9,494 — so a bare maxMana
    /// contest hands every merge to the pit. Retail asks the CLASS
    /// first: a class-10 owner loses to any non-class-10 owner, and
    /// two class-10 owners leave the survivor's own owner standing.
    ///
    /// ⚠ The maxMana test is STRICT `>` on the SURVIVOR's side and
    /// SIGNED (EF:26978; EXE 0x5B6AC `mov esi,[edx+0x8c]; cmp
    /// esi,[eax+0x8c]; jng`), so equal ceilings hand the sphere to the
    /// ABSORBED side — the same asymmetry MC1's `sub_277D0` twin
    /// carries. `v3x == v4x` (EF:26973, EXE 0x5B68E `cmp edx,eax`) is
    /// the same-owner short-circuit; retail recovers the index by
    /// `(ptr - base)/168` (EXE 0x5B6E4 `idiv esi` with esi = 0xA8),
    /// which is the `playerEntityIndex` it already held.
    ///
    /// ⚠ The human is `PLAYER_TARGET`, not a pool index: retail reads
    /// its ceiling off the carpet record `Entities_EA3E4[424]`, the
    /// port off the player column (`view.mana_max`) — the same
    /// indirection MC1's [`Gen::mc1_ball_owner_contest`] uses.
    // Four arms resolve to `oi` on purpose: each is a separate retail
    // branch with its own EF citation, kept one-to-one.
    #[allow(clippy::if_same_then_else)]
    fn mc2_ball_owner_contest(&self, oi: u16, oj: u16, fi: i32, fj: i32) -> u16 {
        if no_mc2_ball_owner_ladder() {
            return if fj > fi { oj } else { oi };
        }
        let is_c10 = |g: &Self, o: u16| {
            !self.is_human_tag(o) && (o as usize) < g.ent.len() && g.ent[o as usize].class64 == 10
        };
        let w136 = |g: &Self, o: u16| -> i32 {
            // The OWNER's ceiling: the walkers' `ctx` is seat 0's, so
            // with several humans it weighed another player's
            // (audit 2026-10-17).
            if let Some(hx) = self.hx_of_tag(o) {
                g.human_ceiling_of(hx).min(i32::MAX as u32) as i32
            } else if (o as usize) < g.ent.len() {
                g.ent[o as usize].f136
            } else {
                0
            }
        };
        let (ci, cj) = (is_c10(self, oi), is_c10(self, oj));
        if ci && cj {
            oi // EF:26951 — neither side is a wizard: survivor keeps its tag
        } else if ci {
            oj // EF:26956 — a bank tag loses to a real owner
        } else if cj {
            oi // EF:26964 — ... and symmetrically
        } else if oi == oj {
            oi // EF:26973 `v3x == v4x`
        } else if w136(self, oi) > w136(self, oj) {
            oi // EF:26978 — STRICT: a tie hands it to the absorbed side
        } else {
            oj
        }
    }

    fn ball_merge_candidates(
        &self,
        i: usize,
        decaying: bool,
        grounded: bool,
        is_fool: bool,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        // ⭐⭐⭐ THE (10,57) MERGES, AND IT MERGES WHILE DECAYING.
        // Retail's m57 tick `sub_35FB0` reaches the SAME partner
        // search the ball does — EF:26591-96 `v27x = sub_10A50(a1x);
        // if (v27x) { sub_36F30(a1x, v27x);
        // SetManaSphereColorAndRot_36920(a1x); }` — and the shipped
        // EXE (file 0x5adc5 `call 0x35250` = sub_10A50, 0x5add3
        // `call 0x5b730` = sub_36F30, 0x5addc `call 0x5b120` =
        // SetManaSphereColorAndRot) carries NO `byte[1] & 0x20` test
        // in front of it: the ball's decay gate (EF:26268) is the
        // BALL's alone, so a decaying fool's sphere still absorbs.
        // The `is_fool` early return was an invented guard and it is
        // 70% of mc2l6-rsg's (10,57) residue (mana/f5a/applied_yaw/
        // applied_pitch rows plus the un-freed partner as `extra`).
        if no_m57_merge() {
            if decaying || !grounded || is_fool {
                return out;
            }
        } else if !grounded || (decaying && !is_fool) {
            return out;
        }
        if no_ball_merge_fix() {
            // The pre-dig arm (A/B only): a whole-pool scan in slot
            // order.
            out.extend(1..self.ent.len());
            out.retain(|&j| {
                j != i
                    && self.ent[j].class64 == 10
                    && self.ent[j].model65 == 39
                    && self.ent[j].tick70 != 62
                    && self.ent[j].flags & 0x400 == 0
            });
            return out;
        }
        let (bx, by, rings) = {
            let e = &self.ent[i];
            (
                ((e.x as u32 + 128) >> 8) as u8,
                ((e.y as u32 + 128) >> 8) as u8,
                ((e.f80 as i32 + 255) >> 8).max(0),
            )
        };
        // ⭐⭐⭐ THE MEMBERSHIP TEST IS A PARAMETER, NOT A CONSTANT.
        // Both partner searches read the SEEKER's OWN `xtype`/
        // `xsubtype` (@0x41/@0x42 = `f66`/`f67`) against the
        // candidate's class/model, with `-1` as a wildcard on either
        // half — MC2 `sub_10A50` (EF:3908-15; shipped EXE file
        // 0x352d9-0x35301: `movsx eax,[esi+0x41]` vs `[ebx+0x3f]`,
        // `[esi+0x42]` vs `[ebx+0x40]`) and MC1 `sub_11D10`
        // (sub_main.cpp:17161-64, `sClass_29861_66` /
        // `sModel_29862_67`) are the same four lines. The port pinned
        // the pair to `(10, 39)`, which is right for every sphere ctor
        // that stamps `xsubtype = 39` and WRONG for `sub_50130`
        // (EF:36631; EXE file 0x74953 `mov BYTE PTR [eax+0x42],0x39`
        // = 57), whose fool's sphere therefore looks for m57 partners
        // and no others. The extra `tick70 != 62` clause has to go
        // with it: it is exactly the m57 lane, so keeping it would
        // still hide every m57 partner from an m57 seeker. The
        // (class, model) test alone already keeps the two families
        // apart in both directions.
        let (sc, sm) = {
            let e = &self.ent[i];
            (e.f66, e.f67)
        };
        let admits = |c: &crate::engine::features::Ent| {
            if no_m57_merge() {
                return c.model65 == 39 && c.tick70 != 62;
            }
            sc == 0xFF || (sc == c.class64 && (sm == 0xFF || sm == c.model65))
        };
        // ⭐⭐⭐ THE RINGS ARE A DATA FILE, NOT A FORMULA. Retail's
        // partner search `sub_11D10` (:17127-73) walks rings through
        // the iterator `sub_11410`/`sub_114B0` (:16697/:16732), whose
        // offsets are READ FROM `DATA/SEARCH.DAT` (loaded by
        // sub_11540, :16783-815) — a 32x32 Euclidean-band ring image.
        // Those shells are 2x2-ANCHORED, not Chebyshev squares:
        //   ring 0 = (0,0) (+1,0) (0,+1) (+1,+1)   — a 2x2 BLOCK
        //   ring 1 = the 12 cells of the −1..+2 border
        // So retail reaches one column/row FURTHER in +x/+y than a
        // square of the same index, and at larger radii it is ROUND
        // where a square visits corners retail never does. The scan
        // takes the FIRST admissible overlap and stops, so the ORDER
        // is as load-bearing as the membership.
        //
        // This was the last hand-rolled Chebyshev ring in the sim;
        // the certified sibling scan `sub_11AC0` (see `ball_impact`
        // above) has used [`Gen::ring_cells`] — which is the real
        // SEARCH.DAT iterator, carrying retail's dropped-last-cell
        // off-by-one — all along, with this exact centre and radius
        // arithmetic.
        //
        // mc1l32-quick t=34181 measures the difference to the unit:
        // ball 693 at base cell (192,244) finds slot 777 at offset
        // (+1,+1) — retail's RING 0 — while the square model reaches
        // slot 567 at (−1,0) first and absorbs the wrong partner
        // (mana 9000/2390 retail vs 4430/5760 port), which then
        // displaced the tick's new (10,0) spawn from slot 567 to 897.
        for (dx, dy) in self.ring_cells(0, rings) {
            let tx = bx.wrapping_add(dx);
            let ty = by.wrapping_add(dy);
            let mut j = self.map_entity[tile(tx, ty)] as usize;
            while j != 0 {
                let c = &self.ent[j];
                let next = c.next20 as usize;
                // ⭐ NO REAP TEST. Retail's `sub_11D10` per-node
                // predicate is exactly three clauses — the SEEKER's own
                // `+66`/`+67` membership with `-1` wildcards
                // (`CARPET.EXE 0x2A598: 0f be 46 42` /
                // `0x2A59C: 83 f8 ff` / `0x2A5A4: 3a 43 40`), the id
                // self-exclusion (`0x2A5C2: 66 8b 46 18` /
                // `0x2A5C6: 66 3b 43 18`) and the AABB
                // `sub_11950` (`0x2A5CC`) — and an exhaustive scan of
                // `0x2A508..0x2A63D` finds NO test against the flags
                // word anywhere in the loop. So "a soft kill is not a
                // free" reaches the merge partner search too: a ball a
                // castle drank earlier in the same pool walk
                // (`castle_absorb`/`sub_46DB0`, whose only mark is
                // `flags |= 0x400`, the hard reclaim `sub_41E90`
                // running at the NEXT tick's top) keeps its class,
                // model, tile links and `+140`, and a ball landing at a
                // HIGHER pool slot still finds it, banks its mana,
                // adopts its extents and hard-frees it — retail banks
                // the same ball TWICE in one tick.
                // WITNESS mc1l48-nodeath t=19822: castle 772 drinks
                // ball 744's 12,784 (`+140` 252,374 → 265,158), then
                // ball 944 absorbs the SAME 744 — `+140` 14 → 12,798,
                // `type86` 113 → 119, extents 25/30/30/25 →
                // 175/107/107/87, `next20` 744 → 0 — and frees it
                // (free stack 697 → 698). The port's invented
                // exclusion left 744 alive-but-flagged and 944 on 14.
                // ⭐ PREDICTED AND BANKED before a corpus row existed:
                // `docs/CONFORMANCE-FINDINGS.md` "Kept port-only:
                // `ball_merge_candidates`' 0x400 exclusion … would DUPE
                // the mana … undug, no corpus row." The row has
                // arrived. The MC2 twin `sub_10A50` (`NETHERW.EXE`
                // `0x352D9..0x3532B`) has no flags test either.
                if j != i && admits(c) && (!no_partner_softkill_fix() || c.flags & 0x400 == 0) {
                    out.push(j);
                }
                j = next;
            }
        }
        out
    }

    /// sub_27030 (:29416): the mana ball — claim intake, launch-arc
    /// physics (gravity 16, quarter-bounce, 250/256 friction, ±64
    /// clamp), merge on overlap (sub_277D0 :29700).
    fn ball_tick(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let mc2 = matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2);
        // ⭐⭐ NO DISABLE TEST AT THE SPHERE'S OWN HEAD EITHER, ON
        // EITHER GAME. MC1's walk gates on CLASS alone (:52351) and
        // `sub_27030` has no disable check — mc1l0 t=1234, castle 663
        // banks ball 754 (:56032) and the flagged ball still
        // slides/decays at its own slot before the next tick-top reap
        // frees it. MC2's `TransformArcherToMana_35940` (EF:26015) is
        // the same shape: EF:26062-65 tests `byte[1] & 8` — the STALL
        // bit, handled immediately below — and NOTHING else.
        //
        // The MC2-only early-out that used to sit here claimed to
        // mirror "retail's UpdateEntities gate", but that loop
        // (EF:40116-80) has no disable test at all; the only one is
        // the tick-top reaper (EF:39948-56). The certified MC1 twin
        // omitting the check was the tell — a certified twin omitting
        // a call is the proof. mc2l3 t=438/543/564/684/780/963/1175/
        // 1228: a sphere swallowed by a balloon at a LOWER pool slot
        // still runs its collector-tether arm that same tick, z += 32
        // straight into the balloon that just ate it, and only the
        // next tick's top-of-frame reaper frees the slot.
        //
        // ⚠ This guard is scoped by MovementVerb::Mc2, not GameId —
        // "anything running the MC2 mover". Native MC2 relies on it
        // being gone: world.rs' end-of-iteration `free_slot` already
        // implements run-then-free (DEVIATIONS.md), and spheres were
        // the one class exempted from native MC2's own documented
        // rule.
        // MC2 stall skip (retail byte[1] & 8 → import bit 26,
        // EF:26062-65): a one-shot whole-tick skip — intakes, modes
        // and the decay tail included. Native MC2 never arms it on
        // spheres; the conformance import carries it.
        if mc2 && self.ent[i].flags & (1 << 26) != 0 {
            self.ent[i].flags &= !(1 << 26);
            if self.ent[i].model65 == 39
                && !crate::engine::features::no_mc2_m27_v34_sphere_transparent()
            {
                self.m27_v34_transparent(i);
            }
            return false;
        }
        // MC2 (10,57) — the RANDOM-VALUE sphere. Its retail tick is
        // `sub_35FB0` (EF:26318), NOT the (10,39) ball's
        // `TransformArcherToMana_35940` (EF:26015), and the two differ
        // in exactly one place: the claim intake. The ball TRANSFERS
        // ownership (EF:26069-94, the arm below); the (10,57) instead
        // runs the FOOL'S-MANA trap
        //
        //     else if (w68 && sub_36680(a1x))          // EF:26362
        //     { _4A190(&pos, 10, 0); DisableEntityDrawing04(a1x); }
        //
        // — the retaliation homes the claimer and the sphere is
        // consumed with a (10,0) poof (docs/spell-audit/fools-mana.md
        // §2b). There is NO owner precondition in `sub_36680`: the ONLY
        // skip is `parentId == claimer`, so an AUTHORED ground sphere
        // (parentId 0, `byte_0x46_70` = the NewEvent default 0) is a
        // live tier-0 trap for everyone. mc2l24 proves it end to end —
        // all 21 authored start spheres carry b46=0/owner28=0, and each
        // one dies the tick after the human's (10,12) possess pulse
        // stamps w68=116, leaving a co-located (10,0) poof and a (9,0)
        // fireball with `word_0x96_150 = 116` (homing the player).
        //
        // Retail field homes, all of them already carried by the
        // conformance importer: tier = f71 (@0x46), payload = f44
        // (@0x2A), counter = f26 (@0x10), parentId = id24 (@0x28 fused),
        // and the claim LATCH is the ch1 mail source itself (@0x68) —
        // `sub_36680` clears it only on the owner branch, so a mid-trap
        // sphere stays latched (and frozen: retail's else-if means a
        // claimed sphere runs no physics that tick either way).
        //
        // Discriminator: retail's m57 ctor `sub_50130` stamps action
        // 0x3E while every other sphere takes 0x29; the port's native
        // spawner keeps the (10,39) family model but now carries that
        // action, so `model 57 || action 62` covers both the imported
        // and the native sphere. MC1 balls are action 41 → untouched.
        let is_fool = mc2 && (self.ent[i].model65 == 57 || self.ent[i].tick70 == 62);
        if is_fool && self.ent[i].mail[1].1 != 0 {
            let spent = self.mc2_fools_retaliate(i, ctx);
            if spent {
                // EF:26363-65: the consume poof, then the soft kill
                // (tick-top reap) — the sphere survives this tick in
                // the pool exactly as retail's disabled entity does.
                let (x, y, z) = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                self.mc2_spawn_fire(x, y, z);
                self.ent[i].flags |= 0x400;
            }
            // ⭐⭐⭐ THE `&&` IS SHORT-CIRCUIT AND BOTH ARMS FALL INTO
            // THE MOVER. Retail's chain is
            //     if (byte[1] & 8) …
            //     else if (w68 && sub_36680(a1x)) { poof; disable; }
            //     else { the whole mover }
            // so a LATCHED sphere whose `sub_36680` returns 0 — every
            // tier-1 tick before the 8th, the first tier-2/3 tick, the
            // owner-clears arm, and the tier > 3 fallthrough — runs
            // the mover THAT SAME TICK. Shipped NETHERW.EXE
            // 0x5a7d6-0x5a7e8:
            //     cmp word [ebx+0x68],0x0 ; jz  0x5a80a
            //     push ebx ; call 0x5ae80 ; test al,al ; jz 0x5a80a
            // — BOTH `jz` land on 0x5a80a, the `cmp word [ebx+0x7a]`
            // that opens the mover. The port's unconditional
            // `return false` here was an INVENTED GUARD (its comment
            // "retail's else-if never reaches the mover" reads the
            // `&&` as if only `w68` gated it), and it FROZE every
            // mid-trap sphere: mc2l6-rsg pair 13600→13601, sphere 316
            // carries dest (0,4) with `b39` 16 and retail rolls it
            // y 16895 → 16899 / z 1349 → 1350 while the port held
            // both. `MGC_NO_M57_TRAP_FALLTHROUGH=1` restores it.
            if spent || no_m57_trap_fallthrough() {
                return false;
            }
        }
        // ch1 collection claim (:29439-45): the ball takes the
        // claimant as owner — only on an owner CHANGE (the possess
        // flash re-broadcasts for 8 ticks; the guard keeps the claim
        // chime single). The mail AMOUNT is MC2 retail's
        // `dword_0x64_100` force flag (the ball twin EF:26069-94,
        // byte-for-byte the house protocol): a FORCED claim (Mana
        // Lock's (10,70) pulse) steals unconditionally and sets the
        // claim lock; a weak claim bounces off a locked ball. MC1 has
        // no forced writer, so its balls never lock — every MC1 claim
        // runs the weak arm exactly as before.
        let mut claimed = false;
        if !is_fool && self.ent[i].mail[1].1 != 0 {
            let (force, src) = self.ent[i].mail[1];
            self.ent[i].mail[1] = (0, 0);
            // The force/lock protocol is MC2's alone (the ball twin
            // EF:26069-94 reads `dword_0x64_100` and stamps byte[2] |=
            // 0x20). Retail MC1's intake (:29439-48) reads the SOURCE
            // only — the possess flash parks a nonzero ch1 amount that
            // nothing consumes, and a claim on owner change is
            // unconditional. Reading the amount as force here locked
            // MC1 balls with a port-only flag bit (the mc1l0 (10,39)
            // flags family, want 12 got 0x2000000C).
            if src != self.ent[i].f144
                && (!mc2 || force != 0 || self.ent[i].flags & crate::mc2::mobs::F_CLAIM_LOCK == 0)
            {
                self.ent[i].f144 = src;
                self.ent[i].flags &= !0x40;
                if mc2 && force != 0 {
                    self.ent[i].flags |= crate::mc2::mobs::F_CLAIM_LOCK;
                }
                // The chime anchors at the CLAIMANT, not the ball
                // (:29444 sub_55370(claimant, -1, 4)) — the player-
                // gated id 4 is heard exactly when YOU claim.
                if let Some(hx) = self.hx_of_tag(src) {
                    self.snd_player(hx, 4);
                }
                // NO intake recolor, EITHER game: the re-derive lives
                // only in the moving arm — MC1's sub_274D0 inside
                // `else if (+58)` (:29518-69), MC2's
                // SetManaSphereColorAndRot inside `else if (byte57 ||
                // v35)` (EF:26287) — so a ball claimed while SETTLED
                // keeps its stale row (mc1l0 t=601 slot 131: retail
                // holds neutral 52 across the human's claim; every
                // certified take carries the family). MC2's v36 latch
                // (EF:26074) only overrides the DECAY gate on the
                // moving tail — carried as `claimed` below.
                claimed = true;
            }
        }
        // ch4 attract (:29451-62): the (10,54) magnet tagged this
        // ball (+118 = magnet slot, the ch4 mail source: +114/+118
        // ARE the channel-4 amount/source pair, +90+6·4/+94+6·4) —
        // aim at it and add a magnitude-4 impulse onto the velocity
        // accumulator, then acknowledge. Against the ±64 clamp and
        // 250/256 friction below this shapes the retail stream. The
        // pull NEVER claims (the ch4 amount is read by nothing;
        // player-confirmed): claim = the bolt's localized impact
        // flash + the merge's owned-beats-unowned adoption.
        let mut kicked = false;
        if self.ent[i].mail[4].1 != 0 {
            let (amt, m) = self.ent[i].mail[4];
            let m = m as usize;
            // ⭐ BOTH GAMES CLEAR THE SOURCE ONLY. EF:26109 (ball) /
            // EF:26383 (m57) for MC2; and MC1's `sub_27030` is the
            // same shape — shipped `CARPET.EXE` `0x3f900`
            // (VA `0x27108`) is `66 89 7b 76  mov %di,0x76(%ebx)`
            // with `%di` zeroed at `0x270cb`, and an exhaustive read
            // of the whole intake `0x27076..0x27116` finds NO write to
            // `+114` (`0x72`), the ch4 AMOUNT. The ch1 intake right
            // above it clears BOTH halves (`0x27069` source `+100`,
            // `0x2706f` amount `+96` as a dword), so the port's
            // "MC1 clears the pair" arm was reading one channel's law
            // onto another. Raw shadow: every magnet-pulled MC1 ball
            // carries `mail4.amt = 100` for the rest of its life in
            // retail (mc1l48-nodeath t=20517 slot 771, t=23379 slot
            // 920, t=24123 slot 634 — retail 100, port 0) while the
            // port zeroed it on the intake tick. MC1 reads the amount
            // nowhere, so this moves no graded lane; it is a raw-lane
            // fidelity law only (`+114` is absent from `EntObsMc1`).
            if no_mc1_ch4_amount_keep() && !mc2 {
                self.ent[i].mail[4] = (0, 0);
            } else {
                self.ent[i].mail[4].1 = 0;
            }
            // Retail MC2's ch4 intake (w7A, EF:26097-110) forces one
            // moving tick even on a settled sphere (the v35 latch).
            kicked = true;
            if m < self.ent.len() {
                let (bx, by) = (self.ent[i].x, self.ent[i].y);
                let (mx, my) = (self.ent[m].x, self.ent[m].y);
                // The aim IS a heading write (:29453 `+30 =
                // sub_42150(...)`; the MC2 twin's attract intake
                // writes yaw_0x1C the same way, EF:26101). The mc1l0
                // (10,39) heading family — 1,279 rows in ~128-tick
                // windows after each castle teardown — is this write
                // tracking the ball→magnet bearing while the pull
                // lasts; the port only applied the impulse. Stored
                // RAW: retail's atan2 returns 0..2048 INCLUSIVE and
                // +30 keeps the full-turn 2048 (corpus t=1385/2336).
                // Masked only for the table index — SIN/COS are len
                // 2048.
                let raw = Self::angle_between(bx, by, mx, my);
                self.ent[i].f30 = raw;
                if mc2 {
                    // ⭐ MC2 REPLACES THE ACCUMULATOR, MC1 NUDGES IT.
                    // `TransformArcherToMana_35940` EF:26102-08 (and the
                    // (10,57) twin `sub_35FB0` EF:26376-82):
                    //
                    //     predictedAxis = {0,0,0};
                    //     MoveEntity_57FA0(&predictedAxis, yaw, 0, word_0x76_118);
                    //     axis_0x9A_154x.x = predictedAxis.x;
                    //     axis_0x9A_154x.y = predictedAxis.y;
                    //
                    // — the velocity becomes polar(yaw, AMOUNT) outright,
                    // where AMOUNT is the aura's `min(dist, 42)` (sub_38D80
                    // EF:28369-73). The MC1 arm below adds a fixed 4-unit
                    // impulse and never reads the amount. mc2l22 pair
                    // 8290→8291, slot 484 (yaw 1639, w76 42, dest_in
                    // (−38,−11)): retail steps (−40,−13) → (49690,48978);
                    // the +4 arm stepped (−42,−12) → (49688,48979) — the
                    // (10,39) x/y family on every aura-dragged tick.
                    // The m57 twin also zeroes actSpeed first (EF:26373).
                    if is_fool {
                        self.ent[i].f126 = 0;
                    }
                    let mut v = (0u16, 0u16, 0i16);
                    Self::polar_step(&mut v, raw, 0, amt as i16);
                    self.ent[i].dest_x = v.0;
                    self.ent[i].dest_y = v.1;
                } else {
                    let dir = (raw & 0x7FF) as usize;
                    let ivx = ((4 * crate::mc1::tables::SIN[dir]) >> 16) as i16;
                    let ivy = (-((4 * crate::mc1::tables::COS[dir]) >> 16)) as i16;
                    let e = &mut self.ent[i];
                    e.dest_x = (e.dest_x as i16).wrapping_add(ivx) as u16;
                    e.dest_y = (e.dest_y as i16).wrapping_add(ivy) as u16;
                }
            }
        }
        // MC2's HOMING intake (`word_0x7A_122`, EF:26097-110) — the
        // (10,54) aura's half of the one-tick handshake. Retail stamps
        // every unclaimed sphere in range EVERY tick (sub_38D80
        // EF:28364-75, `if (!w7A)`) and the SPHERE clears the stamp
        // here, at the head of its own tick, latching `v35` — which is
        // what lets a pull drag a sphere whose settle counter has
        // already run out (the moving gate is `byte_0x39_57 || v35`,
        // EF:26173). The port's field home for +122 is the aura claim
        // map, and the aura collapses the +118 pull speed into the
        // dest velocity it writes there (documented in
        // [`Self::mc2_aura_tick`]).
        //
        // ⚠ Releasing HERE and not on the moving tail is the whole
        // fix: a settled sphere returns early below, so a tail-only
        // release left the claim latched forever — the aura then
        // skipped that sphere for the rest of the level and the mana
        // stopped dead short of the eye, twitching back to life only
        // when the player wandered close enough for the awake pass to
        // re-arm +58. That is precisely the reported regression.
        if mc2 && let Some(aura) = self.mc2_aura_claim.0.remove(&(i as u16)) {
            kicked = true;
            // EF:26372 — the m57's w7A intake ALSO zeroes actSpeed
            // (the ball's EF:26097-99 does not). The port splits that
            // one retail intake across two arms (mail[4] above and the
            // aura claim map here), so both halves must carry it or a
            // pulled sphere keeps flying.
            if is_fool {
                self.ent[i].f126 = 0;
            }
            // ⭐ AND THE SPHERE WRITES ITS OWN HEADING BEFORE IT FLIES
            // — `yaw_0x1C_28 = sub_581E0_maybe_tan2(&a1x->position,
            // &Entities[w7A]->position)` (EF:26101), and only THEN
            // does it set its dest from that bearing. Exactly the MC1
            // ch4 twin twenty lines above, which has had this write
            // since the mc1l0 (10,39) heading family was closed; the
            // MC2 half collapsed the claim into the dest velocity and
            // threw the aura slot away with `.is_some()`, so every
            // dragged sphere kept heading 0 (mc2l3 t=9816 slot 168:
            // retail 960, port 0 — the (10,39) `heading` family).
            // Stored RAW like the MC1 arm: retail's atan2 returns
            // 0..2048 inclusive and every consumer masks on read.
            let aura = aura as usize;
            if aura < self.ent.len() {
                let (ax, ay) = (self.ent[aura].x, self.ent[aura].y);
                let (bx, by) = (self.ent[i].x, self.ent[i].y);
                self.ent[i].f30 = Self::angle_between(bx, by, ax, ay);
            }
        }
        // Collector tether (flag 0x40): the ball FLIES to its
        // collector (+146) instead of running ground physics
        // (:29464-90; the MC2 twins EF:26111-72 for the (10,39) ball
        // and EF:26385-447 for the (10,57) sphere are the same code).
        // Every tethered tick re-arms the +46 lift at 128 (the release
        // pop) and turns +30 to the collector; ≥16 out the ball steps
        // horizontally at 16/tick, under 16 it snaps over the
        // collector and z-servos into the hover band [collector z,
        // +512]: +step/tick from below, −step/tick from more than 512
        // ABOVE — without the descend arm an overhead ball deadlocks
        // the pickup (the balloon parks under it forever). Ground-
        // clamped; the band sits inside the absorb window (balloon
        // half-height 400), so the collector side's ent_overlap
        // finishes the pickup. Past 1024 the ball drops the tether
        // itself; a tethered tick never runs ball physics (retail's
        // else-if), even on the tick the tether clears. The reach
        // test is retail's `EuclideanDistXYZ_58490`, which despite
        // its name sums X and Y ONLY (utilities/Maths.cpp:738) — a
        // grounded ball under a hovering collector is "at" it and
        // z-servos up.
        //
        // Retail admits exactly TWO collector kinds (EF:26115-27) and
        // drops the grab for anything else:
        //   - the (3,3) mana balloon → z step 32, a constant;
        //   - the MC2 (5,23) mana leviathan → z step = the COLLECTOR's
        //     own `word_0x2C_44`, the siphon ramp its arm seeds at 18
        //     and bumps +10 every tick it holds the grab (:18238,
        //     :18270), so a siphoned ball accelerates upward until the
        //     dweller's swallow test fires. Our column homes retail's
        //     0x2C at f46 on SPHERES (the launch/gravity lane) but at
        //     f44 on class-5 creatures (mc2/mobs.rs field map), so the
        //     cross-read below is f44. MC2-only: MC1's ball tick has
        //     no leviathan.
        // ⚠ THE COLLECTOR TEST IS CLASS + MODEL, AND NOTHING ELSE.
        // `if (ent[+146].+64 == 3 && ent[+146].+65 == 3)` (:29469) and
        // the MC2 twin's identical switch (EF:26115-27) — no liveness
        // test, no 0x400 test. SOFT KILL IS NOT A FREE: the record
        // keeps its class, model and links for the rest of the tick,
        // so a balloon culled by its castle's dispatcher at a LOWER
        // pool slot is still a valid collector when the ball's own
        // handler runs later in the same pass. mc1l5 t=713: castle 301
        // culls balloon 325 over quota (flags 12 -> 0x40c, act_life
        // still 8600), and ball 362 keeps its tether bit AND its
        // 16/tick step for that whole tick — retail flags 76, port 12.
        // (`b != 0` stays: retail's +146 = 0 indexes the class-0
        // scratch record, so the class test fails there anyway; the
        // bounds check is ours, guarding a Vec retail indexes raw.)
        if self.ent[i].flags & 0x40 != 0 {
            // EF:26388 — the (10,57) tether arm opens by ZEROING
            // actSpeed, one statement the (10,39) twin (EF:26113) does
            // not have. A sphere grabbed mid-throw drops its launch
            // speed for good, so it can never resume the flight arm
            // when the tether later strays.
            if is_fool {
                self.ent[i].f126 = 0;
            }
            let b = self.ent[i].f146 as usize;
            let live = b != 0 && b < self.ent.len();
            let step = if live && self.ent[b].class64 == 3 && self.ent[b].model65 == 3 {
                Some(32i16)
            } else if live && mc2 && self.ent[b].class64 == 5 && self.ent[b].model65 == 23 {
                Some(self.ent[b].f44 as i16)
            } else {
                None
            };
            if let Some(step) = step {
                self.ent[i].f46 = 128;
                let (bx, by, bz) = {
                    let e = &self.ent[b];
                    (e.x, e.y, e.z)
                };
                let mut pos = {
                    let e = &self.ent[i];
                    (e.x, e.y, e.z)
                };
                let yaw = Self::angle_between(pos.0, pos.1, bx, by);
                self.ent[i].f30 = yaw;
                let d = Self::isqrt(Self::dist2_sq(pos.0, pos.1, bx, by) as u32) as i32;
                if d <= 1024 {
                    if d >= 16 {
                        Self::polar_step(&mut pos, yaw, 0, 16);
                    } else {
                        pos.0 = bx;
                        pos.1 = by;
                        if pos.2 as i32 >= bz as i32 {
                            if pos.2 as i32 > bz as i32 + 512 {
                                pos.2 = pos.2.wrapping_sub(step);
                            }
                        } else {
                            pos.2 = pos.2.wrapping_add(step);
                        }
                    }
                    let ground = self.ground_z(pos.0, pos.1) as i16;
                    if ground > pos.2 {
                        pos.2 = ground;
                    }
                    self.move_relink(i, pos.0, pos.1, pos.2);
                } else {
                    self.ent[i].flags &= !0x40; // strayed: the ball side lets go
                }
            } else {
                self.ent[i].flags &= !0x40; // dangling tether
            }
            // No re-derive on a tethered tick, either game: MC1's
            // first arm (:29464-90) and MC2's (EF:26111-72) both end
            // without it — the sprite row rides stale until the ball
            // next runs the moving arm.
            //
            // ⭐⭐⭐ **BUT THE DECAY TAIL IS NOT PART OF THE MODE
            // BRANCH, AND THE TETHER ARM FALLS INTO IT TOO.** This
            // arm used to `return` outright. `TransformArcherToMana_
            // 35940`'s three arms all converge on the
            // `if (byte[1] & 0x20) { --life_0x8; … }` tail
            // (EF:26311-26330), which sits at the SAME brace level as
            // the `if (byte[0] & 0x40)` head. `NETHERW.EXE` settles it
            // — the tail is file 0x5A746 (VA 0x35F46,
            // `f6 43 0d 20` / `74 52` / `8b 43 08` / `48` /
            // `89 43 08`, then `83 f8 06` `72 0e` `76 22`
            // `83 f8 0c` `74 12` and `call 0x7c710` =
            // `DisableEntityDrawing04_57F10` on zero) and every one
            // of the tether arm's three exits JUMPS to it:
            // ```text
            //   5a2b7  80 63 0c bf   andb $0xbf,0xc(%ebx)   ; drop tether
            //   5a2bb  e9 86 04 00 00  jmp  0x5a746
            //   5a30e  80 63 0c bf   andb $0xbf,0xc(%ebx)   ; d > 1024
            //   5a312  e9 2f 04 00 00  jmp  0x5a746
            //   5a3d0  e8 1b 21 02 00  call 0x7c4f0          ; CopyEntityPosition
            //   5a3d8  e9 69 03 00 00  jmp  0x5a746
            // ```
            // (the frozen arm's `5a3e7 0f 84 59 03 00 00 je 0x5a746`
            // is the fourth, which the port already had.)
            //
            // WITNESS — mc2l24-crazy t=73775, the take's last head and
            // 24 of its 25 remaining segments. The pyramid at slot 6
            // dies, its endgame arm stamps `max_life = 140`,
            // `byte[1] |= 0x20` and `life = 140` on every `(10,39|40|
            // 57)`, and retail's spheres read `life 139` at that same
            // boundary because their own movers run later in the tick
            // and take the first tick off. Slots 682 and 829 are the
            // two that were TETHERED (`flags 76` — bit 6 set, a
            // balloon collector), so the port skipped their decrement
            // and held `140`, then trailed retail by exactly one for
            // the rest of the countdown — one boundary per tick,
            // t=73775..73798.
            // `MGC_NO_MC2_BALL_TETHER_DECAY` restores the early
            // return. MC1 is untouched in practice: nothing in MC1
            // sets bit 13, so the tail is a no-op there.
            if !crate::engine::features::no_mc2_ball_tether_decay() {
                self.ball_decay_tail(i);
            }
            return false;
        }
        // ⭐⭐⭐ THE (10,57) THROWN-SPHERE FLIGHT ARM — `sub_35FB0`
        // EF:26457-26524, the ONE structural difference between the
        // m57 tick and the (10,39) ball's `TransformArcherToMana_35940`
        // that this handler services. The ball goes straight from the
        // tether arm to the settle arm:
        //
        //     if (byte[0] & 0x40) { tether }            // EF:26111
        //     else if (byte_0x39_57 || v35) { settle }  // EF:26173
        //
        // while the m57 nests a THIRD arm inside the else:
        //
        //     else { v13 = actSpeed;
        //            if (v13) { FLIGHT }                // EF:26458-26524
        //            else if (byte_0x39_57 || v31) { settle } }  // EF:26526
        //
        // Routing action 62 into `ball_tick` gave every thrown sphere
        // the SETTLE arm, whose step is `axis_0x9A` (dest, 0 on a
        // thrown sphere) — so the six spheres Fool's Mana throws each
        // tick hovered on the caster's own x/y and pogoed straight up
        // and down. mc2l22 free run from t=9786, slot 900: retail
        // walks speed 328/324/320/316… and x 15808/16100/16388/16673…,
        // the port holds speed 328 and x 15512 FOREVER.
        if is_fool && self.ent[i].f126 != 0 {
            self.mc2_fool_flight(i);
            return false;
        }
        // The ballistic arm is `else if (+58)` (sub_27030 :29518): +58
        // is the fresh-spawn countdown (ctor 0x80) that the global
        // anim pass sub_54F00_55430 → sub_54F80 (:64318-20) steps down
        // once per tick, so a ball runs physics — gravity, downhill
        // roll, and the grounded MERGE scan — for its first 128 ticks
        // and then freezes WHEREVER IT IS for good (the corpus pins
        // it: a ball settles at spawn+128 and then ignores
        // overlapping live neighbors indefinitely). A ball still
        // mid-hop on a long slope at expiry hangs in the AIR — in
        // retail too (the whole body is behind the +58 gate and the
        // anim pass :64318 only decrements; no ground snap ever
        // reaches an expired ball). Player-observed on worm-death
        // balls down a hillside 2026-07-30: FAITHFUL, not a
        // deviation. MC1's decrement lives in mob_awake_pass (the
        // sub_54F00 port — which also RE-ARMS a settled ball to 16
        // near the human, the wake law): this handler gates on the
        // post-maintenance value exactly like retail's else-if, so
        // each 17-tick wake cycle moves 16 and freezes 1. Byte
        // semantics: retail reads +58 as a raw byte (the import
        // widens i8, so 0x80 arrives as -128). MC2's maintenance twin
        // is `sub_68C70` via `sub_68BF0`'s SECOND loop over the sphere
        // chain `dword_38523` (EF:55489-90), ported as the sphere leg
        // of [`Gen::mc2_awake_pass`] — it owns the decrement AND the
        // proximity re-arm, exactly like MC1's. This handler only
        // READS +58 (EF:26173); the sphere tick never writes it.
        let settle = (self.ent[i].f58 & 0xFF) as u8;
        if !mc2 {
            if settle == 0 {
                // Settled balls TRACK the ground (patch option
                // `ball_ground_track`, player-ruled — DEVIATIONS.md):
                // retail's freeze leaves a mid-hop ball hanging in
                // the air forever and lets terrain edits (volcano,
                // castle stamps) BURY a grounded one. Both directions
                // on purpose. Retail arm / conformance replay keep
                // the freeze. MC1-native only.
                if ctx.patches.ball_ground_track && !ctx.strict {
                    let (x, y) = (self.ent[i].x, self.ent[i].y);
                    let g = self.ground_z(x, y) as i16;
                    self.ent[i].z = g;
                }
                return false;
            }
        } else {
            // The MC2 settle law is the SAME shape at a different
            // home: TransformArcherToMana's whole moving body sits
            // behind `byte_0x39_57 || fresh-kick` (EF:26173), the
            // ctor seeds @0x39 = 128 (CreateManaSphere EF:36617 —
            // the port ctor's f58 = 0x80), and `mc2_awake_pass`'s
            // sphere leg steps it 1/tick to 0 (mc2l4 corpus: b39
            // 36→0, then f2c parks at −16 and the sphere never moves
            // again) — then re-arms it to 16 inside 24 tiles of the
            // player, same law as MC1's. The port previously ran
            // always-on physics here, dropping every authored
            // economy sphere to the pristine ground (the mc2l4
            // (10,39) z family). No ground-track deviation for MC2:
            // frozen means frozen, both modes. A settled decaying
            // sphere still runs the decay tail (EF:26289 sits
            // outside the mode branch).
            if settle == 0 && !kicked {
                // Call-free unless the claim intake sounded: see
                // [`crate::engine::features::no_mc2_m27_v34_sphere_transparent`].
                if !is_fool
                    && !claimed
                    && !crate::engine::features::no_mc2_m27_v34_sphere_transparent()
                {
                    self.m27_v34_transparent(i);
                }
                self.ball_decay_tail(i);
                return false;
            }
        }
        let mut vx = self.ent[i].dest_x as i16;
        let mut vy = self.ent[i].dest_y as i16;
        vx = vx.clamp(-64, 64);
        vy = vy.clamp(-64, 64);
        let (x0, y0, z0) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let mut x = x0.wrapping_add(vx as u16);
        let mut y = y0.wrapping_add(vy as u16);
        // Vertical (:29532-37 / EF:26188-91 — the twins are verbatim):
        // z steps by the +46 lift and gravity integrates EVERY moving
        // tick; there is no at-rest gate. The strict below-ground
        // clamp is what keeps a resting ball's observable z pinned
        // while its lift cycles 0 → −16 → 0 underneath.
        let mut z = z0.wrapping_add(self.ent[i].f46);
        self.ent[i].f46 = (self.ent[i].f46 - 16).max(-128);
        // ⭐ THE MC2 CAVE-WALL ESCAPE (EF:26192-26239), MC2-only — a
        // sphere whose stepped position pokes the rock (`sub_11E70`,
        // Terrain.cpp:2152 = [`Gen::cave_poke`]) does NOT stop: it
        // takes actSpeed 256 and FANS for an opening, ±170 at a time
        // out to ±1020 (`v30` alternating sign, `v32` advancing only
        // on the way back to +1), each candidate probed from the
        // CURRENT position — not the stepped one — and dropped onto
        // the terrain. The first opening becomes both the yaw and the
        // roll and the sphere lands there; if all twelve poke, retail
        // falls back to a flat +64/+64 nudge and — its own quirk —
        // leaves z at the LAST probe's terrain altitude, sampled at an
        // x/y it then discards. Either way the lift is slammed to
        // −128. mc2l3 t=5426: sphere 234 wedges under the rock and
        // retail relaunches it at speed 256 on a fanned heading.
        //
        // ⚠ Retail's `v35` arm (skip the fan, keep the position) is
        // the COLLECTOR TETHER, which this port returns out of well
        // above — the ballistic path is only ever reached with v35 = 0.
        if mc2 && self.is_cave() {
            let (fov, hover) = {
                let e = &self.ent[i];
                (
                    e.f84 as i32,
                    crate::mc2::behavior::BEHAVIOR[e.row156 as usize].v_12 as i32,
                )
            };
            if self.cave_poke(fov, hover, x, y) {
                self.ent[i].f126 = 256;
                let yaw0 = self.ent[i].f30 as i32;
                let (mut v32, mut v30) = (170i32, 1i32);
                let mut found = None;
                let mut last_z = z;
                while v32 <= 1024 {
                    let a = ((v30 * v32 + yaw0 + 512) as u16) & 0x7FF;
                    let mut p = (x0, y0, z0);
                    Self::polar_step(&mut p, a, 0, 256);
                    p.2 = self.ground_z(p.0, p.1) as i16;
                    last_z = p.2;
                    if !self.cave_poke(fov, hover, p.0, p.1) {
                        found = Some((a, p));
                        break;
                    }
                    v30 = -v30;
                    v32 += if v30 == 1 { 170 } else { 0 };
                }
                match found {
                    Some((a, p)) => {
                        self.ent[i].f30 = a;
                        self.ent[i].f34 = a;
                        (x, y, z) = p;
                    }
                    None => {
                        vx = 64;
                        vy = 64;
                        x = x0.wrapping_add(64);
                        y = y0.wrapping_add(64);
                        z = last_z;
                    }
                }
                self.ent[i].f46 = -128;
            }
        }
        let ground_full = self.ground_z(x, y);
        let ground = ground_full as i16;
        // Clamp + rebound ONLY when the step went STRICTLY below the
        // ground (`tempV13 > z` :29538 / `v22 > z` EF:26244): a ball
        // landing EXACTLY on it keeps its falling lift one more tick
        // and flips on the next. The mc1l0 replay corpus pins the
        // phase — the authored balls all fall 128-multiples onto flat
        // ground, and a `<=` clamp flips them one tick early (the
        // replay t=2 z+32 cohort; per-pair verify can never see it,
        // the import restores retail's +46 each pair). Rebound =
        // −impact/4 truncating, zeroed at ≤ 16 (:29542-49 /
        // EF:26244-52, the same formula in both binaries; the old
        // MC1-only `< -64` arm was the equivalent form for falls but
        // wrong for the climb-into-terrain case).
        if z < ground {
            z = ground;
            let v = self.ent[i].f46;
            let nb = -(v / 4);
            self.ent[i].f46 = if nb <= 16 { 0 } else { nb };
        }
        // ⭐ THE MC2 CAVE-CEILING CLAMP (EF:26256-63) — MC2-ONLY, and
        // it has no MC1 counterpart because MC1 has no ceiling plane:
        //
        //     if (isCaveLevel) {
        //         v24 = sub_10C60(&pred) - a1x->array_0x52_82.fov;
        //         if (v24 < (int16_t)pred.z) { w2C = -abs(w2C); pred.z = v24; }
        //     }
        //
        // A sphere thrown up inside a cave stops at the ROCK, its lift
        // forced downward — the ceiling twin of the ground rebound
        // above, and it must land BEFORE the `grounded` test, whose
        // retail form (`v22 == pred.z`, EF:26265) compares the terrain
        // altitude against the FULLY clamped z. mc2l3 t=5424: sphere
        // 234 arcs from 2937 at lift −5 and retail parks it on 2913,
        // the ceiling less its own fov, where the port flew on to 2932.
        if mc2 && self.is_cave() {
            let cap = (self.ceiling_z(x, y) as i16).wrapping_sub(self.ent[i].f84 as i16);
            if cap < z {
                self.ent[i].f46 = -self.ent[i].f46.abs();
                z = cap;
            }
        }
        // Grounded contact = post-clamp z ON the ground (`tempV13 ==
        // z` :29552 / `v22 == predicted.z` EF:26265) — true on an
        // exact landing too: the corpus rolls and frictions on the
        // landing tick (ball 223's +150 accumulator moves at t=1).
        let grounded = z == ground;
        // Downhill roll + friction — GROUNDED only, both games (MC1
        // sub_27030's `tempV13 == z` branch :29556-64 via
        // sub_41F50_42290 :52547; MC2 `sub_58030` inside
        // `TransformArcherToMana`'s `v22 == z` branch): a resting ball
        // takes the terrain gradient onto its velocity, so balls
        // stream down slopes and pool in basins. The helper is the
        // same RAW-heightmap forward difference over the ball's 2×2
        // tile quad in both binaries, added un-divided (a height byte
        // ≈ 32 world units), then the 250/256 friction. Airborne balls
        // keep their velocity. (An earlier port arm gave MC1
        // unconditional friction and no roll — contradicted by its own
        // source cite and by the retail corpus's rolling balls.)
        // ⭐⭐⭐ AND THE SLOPE READ IS RETAIL'S, IN RETAIL'S SCRATCH —
        // `sub_58030(&a1x->position, &predictedAxis_EB398ar)`
        // (EF:26271) writes the forward difference into the ENGINE'S
        // ONE GLOBAL AXIS, so a grounded roll leaves that global
        // holding two small height deltas instead of a position. See
        // [`crate::engine::features::mc2_ball_slope_pred_axis`] for
        // the shipped bytes and the mc2l13 witness.
        let mut slope_kick = None;
        if grounded {
            let (tx, ty) = ((x >> 8) as u8, (y >> 8) as u8);
            let h = |dx: u8, dy: u8| {
                self.t.height[tile(tx.wrapping_add(dx), ty.wrapping_add(dy))] as i32
            };
            let sx = h(0, 0) - h(1, 0) + h(0, 1) - h(1, 1);
            let sy = h(0, 0) + h(1, 0) - h(0, 1) - h(1, 1);
            slope_kick = Some((sx as i16 as u16, sy as i16 as u16));
            vx = ((vx as i32 + sx) * 250 / 256) as i16;
            vy = ((vy as i32 + sy) * 250 / 256) as i16;
        }
        self.ent[i].dest_x = vx as u16;
        self.ent[i].dest_y = vy as u16;
        if (x, y, z) != (x0, y0, z0) {
            self.move_relink(i, x, y, z);
        }
        // AFTER the commit: retail's `CopyEntityPosition_57CF0` runs
        // first (EF:26264) and `sub_58030` right behind it, so the
        // slope kick is the LAST thing the ball's tick leaves in the
        // global. `sub_58030` never writes offset 4, so z is kept.
        if mc2
            && let Some((kx, ky)) = slope_kick
            && crate::engine::features::mc2_ball_slope_pred_axis()
        {
            let kz = self.mc2_pred_axis.0.2;
            self.mc2_pred_axis = crate::engine::features::Mc2PredAxis((kx, ky, kz));
        }
        // Merge with an overlapping ball: absorb, despawn the other.
        // A DECAYING ball (the apocalypse-rain channel below) never
        // INITIATES a merge (EF:26268 gates `sub_36D50` on
        // `!(byte[1] & 0x20)`) — but a live ball may still absorb
        // it, which is retail's own mana-retention loophole (magnet/
        // balloon consolidation into a permanent sphere).
        let decaying = self.ent[i].flags & 0x2000 != 0;
        // BOTH games scan for a partner only inside the grounded
        // branch: MC1 `tempV13 == z` (:29552-55), MC2 `v22 ==
        // predicted.z` (EF:26265-69 — `sub_10A50` + `sub_36D50` sit
        // inside the rest-contact arm). An airborne or arcing ball
        // never initiates a merge; a kill's spawn scatter coalesces
        // only as the balls land, one merge per grounded tick.
        for j in self.ball_merge_candidates(i, decaying, grounded, is_fool) {
            if self.ent_overlap(i, j) {
                let (fi, fj) = (self.ent[i].f140, self.ent[j].f140);
                // MC2 owner rule (retail `sub_36D50` EF:26919): an
                // unowned ball defers to an owned partner (and ONLY
                // that arm carries the partner's claim lock across,
                // EF:26937); two OWNED balls go to the ladder in
                // [`Gen::mc2_ball_owner_contest`] — class-10 bank tags
                // lose first, then the owner records' `maxMana`. NOT
                // the survivor's own owner, which would colour a merged
                // ball as "the last ball merged".
                if is_fool && !no_m57_merge() {
                    // ⭐⭐⭐ THE FOOL'S SPHERE HAS ITS OWN MERGE BODY.
                    // The ball's grounded arm calls `sub_36D50`
                    // (EF:26269) and the m57's calls **`sub_36F30`**
                    // (EF:26594; shipped EXE file 0x5add3 `call
                    // 0x5b730`) — a different function, not a variant:
                    // no owner ladder at all, a TIER contest on
                    // `byte_0x46_70` (f71) with a coin flip from the
                    // SURVIVOR's own per-entity LCG when the tiers tie
                    // and the parents differ, and the loser's whole
                    // identity — parentId @0x28 (id24), owner @0x94
                    // (f144), tier @0x46 (f71), subSpell @0x2A (f44) —
                    // moving across on a win. EXE 0x5b73c-0x5b792 is
                    // the whole body: `mov cl,[eax+0x46]; cmp
                    // cl,[edx+0x46]; jge` … `imul bx,[eax+0x14],0x24a1;
                    // add ebx,0x24df` … `and ebx,1`, then the four
                    // copies and `add [eax+0x90]` before the hard free.
                    // Note the draw is the ENTITY stream (`rand_0x14`),
                    // not the global RNG, so this consumes no tick draw.
                    // ⭐⭐⭐ THE PARENT TEST IS ON RETAIL'S RAW `@0x28`,
                    // AND AN UNPARENTED SPHERE READS **0**, NOT ITS OWN
                    // SLOT. `NewEvent_4A050` (Events.cpp:563-79) memsets
                    // the record and seeds `id_0x1A_26 = slot` — `@0x1A`,
                    // the port's `f1a`. It never touches `parentId_0x28`.
                    // The port's `new_event` seeds `id24 = idx` instead
                    // and `obs_project_mc2` maps `id24 == slot` back to 0,
                    // which is right for the OBSERVATION and wrong for
                    // this COMPARISON. Shipped EXE 0x5b748-52:
                    //     mov cx,[edx+0x28] ; cmp cx,[eax+0x28]
                    //     je  0x5b76b       ; <-- PAST the draw
                    //     imul bx,[eax+0x14],0x24a1
                    // so equal parents skip the LCG entirely.
                    // Witness mc2l30 pair 262->263: slots 118 and 121 are
                    // both unparented (10,57)s, retail's `rand` HOLDS at
                    // 763 across the merge, the port drew
                    // lcg16(763) = 9377*763 + 9439 = 20666 — exactly what
                    // `--dump 262` prints. mc2l30 horizon 262 -> 2,972.
                    // ⭐ SIBLING: `Gen::probe_self_id`
                    // (`MGC_NO_PROBE_ID_UNFUSE`) un-fuses the OTHER half
                    // of the same `id24` fusion — it maps the fused
                    // families back to `@0x1A` = the slot for
                    // `sub_10780`'s self-exclusion. This one wants
                    // `@0x28` = 0. Same fusion, opposite halves.
                    let parent = |g: &Self, k: usize| {
                        if no_m57_parent_zero() || g.ent[k].id24 as usize != k {
                            g.ent[k].id24
                        } else {
                            0
                        }
                    };
                    let (pi, pj) = (parent(self, i), parent(self, j));
                    let take = if self.ent[i].f71 >= self.ent[j].f71 {
                        if pj != pi {
                            self.ent_rand(i) & 1 != 0
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if take {
                        let (pid, own, tier, sub) = {
                            let d = &self.ent[j];
                            (
                                // retail copies a raw 0 across; the port's
                                // "unparented" is its own slot
                                if no_m57_parent_zero() || d.id24 as usize != j {
                                    d.id24
                                } else {
                                    i as u16
                                },
                                d.f144,
                                d.f71,
                                d.f44,
                            )
                        };
                        let s = &mut self.ent[i];
                        s.id24 = pid;
                        s.f144 = own;
                        s.f71 = tier;
                        s.f44 = sub;
                    }
                } else if matches!(self.verbs.movement, crate::verbs::MovementVerb::Mc2) {
                    let (oi, oj) = (self.ent[i].f144, self.ent[j].f144);
                    let winner = if oi == 0 {
                        oj
                    } else if oj == 0 {
                        oi
                    } else {
                        self.mc2_ball_owner_contest(oi, oj, fi, fj)
                    };
                    self.ent[i].f144 = winner;
                    // Mana Lock across merges (EF:26936-40): ONLY the
                    // unclaimed-survivor arm carries the absorbed
                    // ball's claim lock — a gathered pile stays
                    // locked; every other merge arm lets the
                    // despawned ball's lock die with it (this churn
                    // is why retail locks LOOK timed in play).
                    if oi == 0 && oj != 0 && self.ent[j].flags & crate::mc2::mobs::F_CLAIM_LOCK != 0
                    {
                        self.ent[i].flags |= crate::mc2::mobs::F_CLAIM_LOCK;
                    }
                } else {
                    self.mc1_ball_owner_contest(i, j);
                }
                self.ent[i].f140 = fi + fj;
                // EF:26595 / EXE 0x5addc `call 0x5b120` — the m57's
                // merge is the ONLY caller of
                // `SetManaSphereColorAndRot_36920` inside `sub_35FB0`,
                // and it is unconditional. The shared tail below is
                // the BALL's gated resize (EF:26286), which a decaying
                // sphere skips, so the fool's arm re-derives here.
                if is_fool && !no_m57_merge() {
                    self.ball_resize(i);
                }
                // MC1's sub_277D0 frees the absorbed ball through
                // sub_41E90_421D0 (:52514-20) — the HARD free (unlink,
                // class 0, slot straight back on the stack), not the
                // 0x400 soft-kill: the donor is gone from the very
                // snapshot the merge lands in (pair 11→12 of the mc1l0
                // corpus: retail's 485 absorbs 479 and 479 is absent
                // at t=12; a soft-killed donor lingers to the sweep
                // and reads as extra-in-port).
                //
                // MC2'S TWIN IS THE SAME LAW. `sub_36D50` (EF:26919-
                // 26996) is a ladder of owner-resolution arms and
                // EVERY one of them ends `return sub_57F20(a2x)` —
                // and `sub_57F20` (Events.cpp:5209-39) is the hard
                // free: tile unlink, recycle-stack swap-removal,
                // `class = 0`, free-stack push. Nothing defers it to
                // the disable sweep. Corpus proof (mc2l24, the
                // doomsday fountain): the permanent shore sphere in
                // slot 845 absorbs the arriving rain — mana 141653 →
                // 143966 across t=64510 while slot 795 (mana 2313) is
                // ABSENT from the t=64511 snapshot, and again +78 as
                // slot 828 vanishes at t=64512. A soft-killed donor
                // would have lingered one snapshot AND withheld its
                // slot, which is exactly the extra-in-port the
                // fountain window measured.
                if mc2 && no_ball_merge_fix() {
                    self.ent[j].flags |= 0x400; // the pre-dig MC2 arm (A/B only)
                } else {
                    self.free_entity(j);
                }
                break;
            }
        }
        // Size re-derivation on MOVING ticks (:29569 / EF:26287) —
        // merged/claimed balls visibly grow/recolor in the original.
        // MC2 gates it off while decaying UNLESS the claim latch
        // fired this tick (EF:26286 `!(byte[1] & 0x20) || v36`).
        //
        // ⭐⭐⭐ AND THE (10,57) HAS NO SUCH TERM AT ALL. EF:26287 lives
        // in `TransformArcherToMana_35940` (EF:26015-26317), the
        // (10,39) BALL's action-0x29 tick. The m57 runs `sub_35FB0`
        // (EF:26318-26614), whose ONLY `SetManaSphereColorAndRot` is
        // the merge arm already fired above (EF:26595) — confirmed
        // against the shipped EXE's five call sites, see
        // [`no_m57_tick_resize`]. An enumerated list; the absence is
        // the law. So an m57 keeps whatever sprite its ctor stamped
        // while it was still UNOWNED, and a rival that claims one
        // never recolors it. mc2l6-rsg t=13796: retail stamps
        // `player_ent 0 -> 378` on slot 24 and leaves `f5a` at 56
        // (52 wild + size 4, mana 3689) for the rest of the take,
        // while the (10,39) in slot 804 — SAME owner, one tick later —
        // does go `f5a 53 -> 138` (105 + 8*color_art(2) + size 1). The
        // port recoloured the m57 to 141, and that one field was the
        // take's certification wall.
        let m57_no_resize = is_fool && !no_m57_tick_resize();
        let resized = (!(mc2 && decaying) || claimed) && !m57_no_resize;
        if resized {
            self.ball_resize(i);
        }
        // The hydra `v34` seam (`Gen::m27_v34_publish_sphere`): the
        // (10,39) ball's moving arm ends on `SetManaSphereColorAndRot_
        // 36920`, whose saved ESI — this tick's `getTerrainAlt` — lands
        // on W-64. (10,57)'s `sub_35FB0` is a different frame.
        if mc2 && !is_fool {
            self.m27_v34_publish_sphere(resized.then_some(ground_full as u32));
        }
        self.ball_decay_tail(i);
        false
    }

    /// `sub_35FB0`'s FLIGHT arm (EF:26457-26524) — the (10,57)
    /// THROWN sphere, the half of the m57 tick the (10,39) ball has
    /// no counterpart for. Fool's Mana (`sub_6C870` EF:57888-57922)
    /// launches six of these at `actSpeed = (tokenLCG & 0x7F) +
    /// clamp(4*caster.actSpeed, 140, 280)` on a ±85 yaw fan at the
    /// caster's pitch; everything below is what carries them.
    ///
    /// Verbatim, in retail's order:
    ///
    /// ```c
    /// v13 = a1x->actSpeed_0x82_130;
    /// if (v13) {
    ///   if (v13 <= 0)      { if (v13 < -4) actSpeed = v13 + 4; }
    ///   else if (v13 > 4)  { actSpeed = v13 - 4; }              // EF:26461-69
    ///   predictedAxis = a1x->position;
    ///   MoveEntity_57FA0(&predictedAxis, yaw, pitch, actSpeed);  // EF:26472-73
    ///   v14 = a1x->word_0x2C_44 - 16; a1x->word_0x2C_44 = v14;
    ///   if (v14 < -128) a1x->word_0x2C_44 = -128;
    ///   predictedAxis.z += a1x->word_0x2C_44;                    // EF:26474-78
    ///   if (isCaveLevel && sub_11E70(a1x, &predictedAxis)) {
    ///       predictedAxis = a1x->position; actSpeed = 0;
    ///       a1x->word_0x2C_44 = -128;                            // EF:26479-86
    ///   } else CopyEntityPosition_57CF0(a1x, &predictedAxis);    // EF:26489
    ///   v15 = getTerrainAlt_10C40(&predictedAxis);
    ///   if (v15 <= predictedAxis.z) { …cave ceiling kick… }      // EF:26491-507
    ///   else {
    ///       a1x->actSpeed_0x82_130 = 0;
    ///       a1x->position.z = v15;                               // EF:26511-12
    ///       if (sub_104D0_terrain_tile_is_water(&a1x->position) == 1) {
    ///           a1x->word_0x2C_44 = 0;
    ///           v16x = _4A190(&a1x->position, 10, 5);
    ///           if (v16x) PrepareEventSound_6E450(v16x, -1, 27);  // EF:26513-19
    ///       } else a1x->word_0x2C_44 = 128;                       // EF:26522
    ///   }
    /// }
    /// ```
    ///
    /// Three details a paraphrase loses:
    ///
    /// 1. **The decel NEVER crosses zero** (EF:26461-69 only steps when
    ///    `|v| > 4`), so a thrown sphere does not "run out of speed"
    ///    and drop into the settle arm — the ONLY exit is the LANDING
    ///    at EF:26511. mc2l22 slot 900 rides 328→324→…→4 and then
    ///    holds 4 until it hits the ground.
    /// 2. **Gravity DECREMENTS BEFORE IT IS ADDED** here (EF:26474-78),
    ///    where the settle arm adds first and decrements after
    ///    (EF:26542-46) — a flat 16-unit phase difference between the
    ///    two arms on the very first airborne tick.
    /// 3. **The cave-ceiling kick's `predictedAxis.z = v17` is DEAD**
    ///    (EF:26505): the position was already committed at EF:26489
    ///    and nothing reads the scratch axis afterwards, so unlike the
    ///    settle arm's ceiling clamp (EF:26579-87, committed at
    ///    EF:26588) the flight arm's ceiling contact changes only
    ///    `actSpeed` and `word_0x2C_44`, never the sphere's z.
    fn mc2_fool_flight(&mut self, i: usize) {
        // EF:26461-69 — decelerate 4/tick, never through zero.
        let v13 = self.ent[i].f126;
        if v13 <= 0 {
            if v13 < -4 {
                self.ent[i].f126 = v13 + 4;
            }
        } else if v13 > 4 {
            self.ent[i].f126 = v13 - 4;
        }
        // EF:26472-73 — the 3-D step off the LAUNCH pose (yaw AND
        // pitch), at the POST-decel speed.
        let (x0, y0, z0) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z)
        };
        let mut pos = (x0, y0, z0);
        let (yaw, pitch, spd) = {
            let e = &self.ent[i];
            (e.f30, e.f32, e.f126)
        };
        Self::polar_step(&mut pos, yaw, pitch, spd);
        // EF:26474-78 — gravity: DECREMENT FIRST, floor −128, THEN add.
        let g = (self.ent[i].f46 - 16).max(-128);
        self.ent[i].f46 = g;
        pos.2 = pos.2.wrapping_add(g);
        // EF:26479-86 — the cave-wall arm: revert the whole step, stop
        // dead, slam the lift to −128, and DO NOT commit the position.
        let mut commit = true;
        if self.is_cave() {
            let (fov, hover) = {
                let e = &self.ent[i];
                (
                    e.f84 as i32,
                    crate::mc2::behavior::BEHAVIOR[e.row156 as usize].v_12 as i32,
                )
            };
            if self.cave_poke(fov, hover, pos.0, pos.1) {
                pos = (x0, y0, z0);
                self.ent[i].f126 = 0;
                self.ent[i].f46 = -128;
                commit = false;
            }
        }
        if commit {
            self.move_relink(i, pos.0, pos.1, pos.2);
        }
        // EF:26491 — the terrain read is on the PREDICTED axis, which
        // by here is the sphere's own committed position either way.
        let ground = self.ground_z(pos.0, pos.1) as i16;
        if ground <= pos.2 {
            // EF:26494-507 — still airborne. In a cave a sphere that
            // pokes the rock takes a random ±1/±2 skid and its lift is
            // forced downward; the `predictedAxis.z = v17` beside it is
            // dead (see the doc comment), so z is NOT clamped here.
            if self.is_cave() {
                let cap =
                    (self.ceiling_z(pos.0, pos.1) as i16).wrapping_sub(self.ent[i].f84 as i16);
                if cap < pos.2 {
                    let r = self.ent_rand(i);
                    let s = ((r & 3) as i32 - 2) as i16;
                    self.ent[i].f126 = if s == 0 { 1 } else { s };
                    self.ent[i].f46 = -self.ent[i].f46.abs();
                }
            }
        } else {
            // EF:26509-23 — THE LANDING. actSpeed 0 hands the sphere to
            // the settle arm from the next tick on; the lift is re-armed
            // at +128 (the bounce) on land, or ZEROED on water with a
            // (10,5) splash and its sound 27.
            self.ent[i].f126 = 0;
            self.ent[i].z = ground;
            let (lx, ly) = {
                let e = &self.ent[i];
                (e.x, e.y)
            };
            if self.cap_bit(lx, ly) == 1 {
                self.ent[i].f46 = 0;
                if let Some(s) = self.mc2_spawn_splash(lx, ly, ground) {
                    self.snd(27, s);
                }
            } else {
                self.ent[i].f46 = 128;
            }
        }
    }

    /// The apocalypse-rain DECAY channel (`byte[1] |= 0x20` — port
    /// flag bit 13; the MC2 sphere mover's tail, EF:26289-307): the
    /// timed sphere counts its life down — at 12 the 67% death-fade
    /// bit (24) arms, at 6 it swaps to the bit-23 ghost, at 0 it
    /// expires. Only the doomsday mana rain (mc2::morph summit91)
    /// and the conformance import set the bit, so MC1 and ordinary
    /// spheres never enter; a balloon tether returns before this
    /// tail, reproducing retail's pickup-retains-the-ball behavior.
    /// Runs for SETTLED spheres too — retail's tail sits outside the
    /// mode branch.
    fn ball_decay_tail(&mut self, i: usize) {
        if self.ent[i].flags & 0x2000 == 0 {
            return;
        }
        self.ent[i].act_life -= 1;
        let l = self.ent[i].act_life;
        if l < 6 {
            if l == 0 {
                self.ent[i].flags |= 0x400;
            }
        } else if l == 6 {
            self.ent[i].flags = (self.ent[i].flags | 1 << 23) & !(1 << 24);
        } else if l == 12 {
            self.ent[i].flags |= 1 << 24;
        }
    }

    // ---- corpse pipeline ----------------------------------------------------

    /// The CorpseVerb seam (crate::verbs): MC1 scatters mana
    /// balls/jars. MC2's death drops (spell tokens, mana-sphere
    /// split/merge) live in the mc2 death handlers, which do not
    /// route through here — an MC2 world reaching THIS drop serves
    /// the MC1 scatter and says so in telemetry.
    pub(crate) fn corpse_drop(&mut self, i: usize) {
        match self.verbs.corpse {
            CorpseVerb::Mc1 => self.corpse_drop_mc1(i),
            CorpseVerb::Mc2 => {
                self.note_verb_fallback(VerbKind::Corpse);
                self.corpse_drop_mc1(i);
            }
        }
    }

    /// sub_27690 (:29663): the corpse's mana-ball drop — one unused
    /// draw on the CORPSE's seed (kept for stream parity), then the
    /// ball with two launch draws on its OWN seed.
    fn corpse_drop_mc1(&mut self, i: usize) {
        if self.ent[i].f140 <= 0 {
            return;
        }
        let _ = self.ent_rand(i); // :29674 — result unused, draw kept
        let (x, y, z, heading, mana, owner) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.f30, e.f140, e.f144)
        };
        if let Some(b) = self.spawn_mana_ball(x, y, z) {
            // ⭐ :29675-93 — EVERY operand of the drop is read off the
            // corpse AFTER the ball's allocation (`+140` :29679,
            // `+144` :29681, `+30` :29685, and the `+72` pair the lift
            // needs :29691-92), so a ball that seized its own corpse
            // reads the BALL's fields and its own zeroed position: no
            // mana, no claim, heading 0 and the ground under tile
            // (0,0). See [`no_mc1_self_seize`]. The closing
            // `a2x->+144 = 0` then lands on the ball.
            let seized = self.mc1_self_seized(b, i);
            let (x, y, z, heading, mana, owner) = if seized {
                let e = &self.ent[b];
                (e.x, e.y, e.z, e.f30, e.f140, e.f144)
            } else {
                (x, y, z, heading, mana, owner)
            };
            self.ent[b].f140 = mana;
            self.ent[b].f144 = owner;
            let d1 = self.ent_rand(b);
            let yaw = ((d1 % 0x71) as i32 - 56 + heading as i32) as u16 & 0x7FF;
            let d2 = self.ent_rand(b);
            let speed = (d2 % 0x30 + 16) as i16;
            // Heading only (:29688 `v2[15]`) — retail never writes the
            // ball's +34, so the drop is born with target_yaw 0 like
            // every non-homing (10,x); the ball tick never reads it.
            self.ent[b].f30 = yaw;
            // The launch speed persists in +126 (:29689 `v2[63]`) —
            // the mc1l0 corpus pins it: every castle-preclear house
            // drop carries 16..63 where the unstamped port ball read
            // the NewEvent 16.
            self.ent[b].f126 = speed;
            let vx = ((speed as i32 * crate::mc1::tables::SIN[yaw as usize]) >> 16) as i16;
            let vy = (-((speed as i32 * crate::mc1::tables::COS[yaw as usize]) >> 16)) as i16;
            self.ent[b].dest_x = vx as u16;
            self.ent[b].dest_y = vy as u16;
            let ground = self.ground_z(x, y) as i16;
            // Signed /8 toward zero (:29692's CFSHL ritual) — a death
            // more than 1024 above ground launches the ball DOWNWARD;
            // the old `.max(0)` flattened that to a zero lift.
            self.ent[b].f46 = ((1024 - (z.wrapping_sub(ground)) as i32) / 8) as i16;
        }
        self.ent[i].f144 = 0;
    }

    /// The corpse's death-flame puff: class-10 m1 at radius 0 with
    /// +24 = the corpse (:21866).
    pub(crate) fn corpse_puff(&mut self, i: usize) {
        let (x, y, z, id) = {
            let e = &self.ent[i];
            (e.x, e.y, e.z, e.id24)
        };
        if let Some(p) = self.spawn_effect(1, x, y, z) {
            // :21866-68 — `+24` is read off the corpse after the
            // allocation (identity on a self-seizure), and the
            // `sub_41E80(a1x)` in the caller then reaps the puff
            // ([`no_mc1_self_seize`]).
            if !self.mc1_self_seized(p, i) {
                self.ent[p].id24 = id;
            }
            self.ent[p].f26 = 0;
        }
    }

    // ---- helpers over private feature internals ------------------------------

    /// Ring cell offsets for radius lo..=hi — the real SEARCH.DAT
    /// ring table (the original's precomputed rings, row-major
    /// emission order + the dropped-last-cell quirk, features.rs
    /// `ring_cells`), sign-extended for unit-space scaling. The retail
    /// rings are ROUND (not a Chebyshev box = a square blast);
    /// tile-space callers (dig_disc) keep the raw u8 deltas and wrap
    /// mod 256.
    fn ring_cells_pub(&self, lo: i32, hi: i32) -> Vec<(i8, i8)> {
        self.ring_cells(lo, hi)
            .into_iter()
            .map(|(dx, dy)| (dx as i8, dy as i8))
            .collect()
    }

    /// The fire's scorch dig (sub_40D30(expl, 0, 0, -depth, 1)):
    /// the RING-0 DISC around the fire's rounded cell — the SEARCH.DAT
    /// 2x2 zero block minus the walker's dropped last cell, i.e. THREE
    /// cells: center, (+1,0), (0,+1). Also the MC2 fire's (sub_30D50 →
    /// sub_572C0 — same ring walk, EF:39730-40). A zero depth still
    /// runs the full cell update (MC1 :51647-88, MC2 EF:39535-47):
    /// the angle LATCH (`|= 1`) + restencil/retile land on all three
    /// cells, so the fire gate refuses later re-scorches there.
    pub(crate) fn dig_scorch(&mut self, i: usize, delta: i16) {
        let _ = self.dig_disc_pub(i, 0, 0, delta, true);
    }
}

// Global-stream helper kept close to the module using it.
#[allow(dead_code)]
pub(crate) fn global_draw(rand: &mut u32) -> u32 {
    lcg32(rand)
}

/// Law W5's other three call paths — `spreader_tick`, `napalm_tick`
/// and `napalm_tick_hw`. `blast_ring_tick` is corpus-pinned by
/// `conformance/fixtures/mc1l37/a-sprayer-seized-by-its-own-child-reads-live-operands.mgcr`
/// (ring slot 69, t=5670); these three measure as ZERO CHANGE across
/// the whole corpus — no take ever exhausted the pool inside one of
/// them — so a rig is the only witness they can have.
#[cfg(test)]
mod ring_seizure_tests {
    use crate::chassis::ChassisParams;
    use crate::engine::features::{FeatureAssets, Gen, Planes};
    use crate::engine::world::Hx;
    use crate::mc1::mobs::MobCtx;
    use crate::patches::WorldPatches;
    use crate::verbs::VerbSet;

    /// The sprayer's original owner id — deliberately NOT any pool
    /// slot the rig allocates, so "the members took the alias's `+24`"
    /// cannot be confused with "the members kept the sprayer's".
    const OWNER: u16 = 504;
    /// The sprayer's original `+30` (heading), likewise distinctive.
    const AIM: u16 = 724;
    /// Every ring cell in the rig is this delta, so each spray step is
    /// `pitch * 100` units along both axes — far enough that the
    /// ±64 jitter can never blur "around the ring" into "around the
    /// child".
    const CELL: i32 = 100;

    fn flat_gen(rings: Vec<Vec<(u8, u8)>>, verbs: VerbSet) -> Gen {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets {
            rings,
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        Gen::new(planes, assets, 1, ChassisParams::MC1, verbs)
    }

    fn ctx(g: &mut crate::engine::features::Gen) -> MobCtx {
        *g.view_mut(crate::engine::world::Hx::FIRST) = crate::engine::features::SeatView {
            x: 60000,
            y: 60000,
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

    /// A SEARCH.DAT stand-in whose ring `idx` is `n` copies of `CELL`.
    /// [`Gen::ring_cells`] drops the last cell of the outermost ring
    /// it fetches (the stop code), hence the `n + 1`.
    fn rings_at(idx: usize, n: usize) -> Vec<Vec<(u8, u8)>> {
        let mut r: Vec<Vec<(u8, u8)>> = (0..32).map(|_| vec![(0u8, 0u8), (0, 0)]).collect();
        r[idx] = vec![(CELL as u8, CELL as u8); n + 1];
        r
    }

    /// THE SELF-SEIZURE RIG. Stage a class-10 sprayer at (1000, 1000)
    /// with a distinctive owner/aim, then EMPTY THE FREE STACK and put
    /// the sprayer itself on top of the recycle stack, so the very
    /// first `new_event` inside its own spray loop hands back the
    /// sprayer's own slot — exactly retail's `sub_373F0`
    /// (`CARPET.EXE` file 0x4fbe8) sacrificing a live victim when the
    /// free list is dry. `refill` stays clear so the rig cannot
    /// re-seize the newborn fires it just made.
    fn rig(
        rings: Vec<Vec<(u8, u8)>>,
        verbs: VerbSet,
        model: u8,
        var26: i16,
        dummies: usize,
    ) -> (Gen, usize, Vec<usize>) {
        let mut g = flat_gen(rings, verbs);
        let s = g
            .spawn_effect(model, 1000, 1000, 100)
            .expect("sprayer slot");
        g.ent[s].id24 = OWNER;
        g.ent[s].f30 = AIM;
        g.ent[s].act_life = 5;
        // The one-shot sound latch already tripped (`+16 & 2`), and
        // the `0x10000` marker the spreader's children INHERIT.
        g.ent[s].flags |= 2 | 0x10000;
        g.ent[s].f26 = var26;
        // Recycle victims for the spray members that follow the
        // seizure, parked far outside the sprayer's damage extents.
        let members: Vec<usize> = (0..dummies)
            .map(|k| {
                let d = g.new_event().expect("victim slot");
                g.ent[d].class64 = 10;
                g.ent[d].model65 = 3;
                g.ent[d].x = 60000;
                g.ent[d].y = 60000 + k as u16 * 8;
                d
            })
            .collect();
        g.free.clear();
        // `mc2_recycle_pop` takes the LAST element first.
        let mut stack: Vec<u16> = members.iter().rev().map(|&d| d as u16).collect();
        stack.push(s as u16);
        g.mc2_recycle.stack = stack;
        g.mc2_recycle.refill = false;
        (g, s, members)
    }

    /// The origin a spray member was laid around, with the constant
    /// `pitch * CELL - 96` step removed. What is left is the `rand %
    /// 0x81 - 64` jitter, so this lands within 64 of the position the
    /// loop actually read for that member.
    fn origin_of(x: u16, pitch: i32) -> i32 {
        x as i32 - pitch * CELL + 96
    }

    /// The four assertions every self-seizing sprayer owes, on the
    /// FIRST member laid after the seizure.
    fn assert_reads_the_alias(g: &Gen, sprayer: usize, member: usize, pitch: i32) {
        assert!(
            g.ent[sprayer].class64 == 10 && g.ent[sprayer].model65 != 3,
            "the sprayer's own slot must have been seized for its first child"
        );
        assert_eq!(
            g.ent[sprayer].id24, sprayer as u16,
            "the child's `+24` is read off the alias AFTER the allocation, so it \
             self-assigns `new_event`'s own-slot stamp — not the sprayer's {OWNER}"
        );
        assert_ne!(
            g.ent[member].model65, 3,
            "the second spray member must actually have been allocated"
        );
        assert_eq!(
            g.ent[member].id24, sprayer as u16,
            "every later member takes the ALIAS's owner, not the sprayer's {OWNER}"
        );
        let laid_around = origin_of(g.ent[member].x, pitch);
        assert!(
            (laid_around - g.ent[sprayer].x as i32).abs() <= 64,
            "member laid around the CHILD at x={} (measured origin {laid_around})",
            g.ent[sprayer].x
        );
        assert!(
            (laid_around - 1000).abs() > 1000,
            "…and NOT around the sprayer's own x=1000 (measured origin {laid_around})"
        );
    }

    /// Park `s` at the top of an EMPTY free stack's recycle list, so
    /// the next `new_event` hands `s`'s own slot back — retail's dry
    /// `NewEvent_372C0` sacrificing a live victim (:43885-908).
    fn seize_next(g: &mut Gen, s: usize) {
        g.free.clear();
        g.mc2_recycle.stack = vec![s as u16];
        g.mc2_recycle.refill = false;
    }

    /// ⭐⭐⭐ **A CLASS-9 DETONATION CAN SEIZE ITS OWN BOLT, AND THEN
    /// THE PAYLOAD IS BORN AT THE MAP ORIGIN** — the trail law's
    /// (`MGC_NO_MC1_SELF_SEIZE`) second family, `sub_52770`'s
    /// `LABEL_25_26` block (:62759-72) and the m0/m1/m8/m12/m17/m19
    /// twins that share its shape.
    ///
    /// `CARPET.EXE` (file = VA + 0x187F8): the detonation's ctor call
    /// is `sub_373F0(a1 + 72, a1->+68, a1->+69)` and every stamp that
    /// follows — `v20[12] = a1+24`, `v20[15] = a1+30`, `v20[16] =
    /// a1+32`, `v20[22] = a1+44` — reads the bolt AFTER the allocator
    /// returned, so on a self-seizure they are identity stores, the
    /// accuracy call `sub_526C0(a1, …)` reads the CHILD's `+65`/`+24`
    /// (never a class-3 owner, so nothing scores), and the closing
    /// `sub_41E80(a1)` reap-flags the payload itself.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_MC1_SELF_SEIZE=1` restores the
    /// hoisted copies — the payload would be posed at the bolt's
    /// (30000, 30000, 1000) and carry owner 504 / yaw 724.
    #[test]
    fn a_detonation_that_seizes_its_own_bolt_lands_at_the_origin() {
        let mut g = flat_gen(rings_at(0, 1), VerbSet::MC1);
        let b = g.spawn_trail_bolt(30000, 30000, 1000).expect("bolt slot");
        g.ent[b].id24 = OWNER;
        g.ent[b].f30 = AIM;
        g.ent[b].f32 = 40;
        g.ent[b].f44 = 8000;
        g.ent[b].f68 = 10;
        g.ent[b].f69 = 0; // the (10,0) fire payload
        seize_next(&mut g, b);
        let rig_ctx = ctx(&mut g);
        g.proj_explode(b, &rig_ctx, None, true, true);
        assert!(
            g.mc2_recycle.seized == 1,
            "the rig must have sacrificed the bolt for its own payload"
        );
        assert_eq!(
            (g.ent[b].class64, g.ent[b].model65),
            (10, 0),
            "the bolt's record IS the payload now"
        );
        assert_eq!(
            (g.ent[b].x, g.ent[b].y, g.ent[b].z),
            (0, 0, 0),
            "the ctor read its position through the alias, i.e. off its own memset record"
        );
        assert_eq!(
            g.ent[b].id24, b as u16,
            "`+24` is read after the allocation, so the stamp is the identity"
        );
        assert_eq!(g.ent[b].f30, 0, "…and so is the heading");
        assert_ne!(g.ent[b].f44, 8000, "…and the payload copy");
        assert_ne!(g.ent[b].flags & 0x400, 0, "`sub_41E80(a1)` reaps the child");
        assert_eq!(
            g.shots[Hx::FIRST],
            0,
            "`sub_526C0` reads the child: no wizard shot"
        );
    }

    /// ⭐⭐ **A CORPSE THAT LOSES ITS SLOT TO ITS OWN MANA BALL DROPS
    /// NOTHING** — `sub_27690` (:29663-95) reads `+140` (the purse),
    /// `+144` (the claim) and `+30` off the CORPSE after the ball's
    /// allocation, and computes the launch lift from the corpse's
    /// `+72` against the ground under it. On a self-seizure all four
    /// come off the ball, whose purse is the ctor's, and the trailing
    /// `a2x->+144 = 0` clears the ball's own claim.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_MC1_SELF_SEIZE=1` — the ball
    /// would carry the corpse's 5,000 mana and the claim 77.
    #[test]
    fn a_corpse_seized_by_its_own_ball_drops_an_empty_one() {
        let mut g = flat_gen(rings_at(0, 1), VerbSet::MC1);
        let c = g.spawn_creature(9, 20000, 20000, 100).expect("corpse slot");
        g.ent[c].f140 = 5000;
        g.ent[c].f144 = 77;
        g.ent[c].f30 = AIM;
        seize_next(&mut g, c);
        g.corpse_drop(c);
        assert_eq!(
            (g.ent[c].class64, g.ent[c].model65),
            (10, 39),
            "the corpse's record IS the ball now"
        );
        assert_eq!((g.ent[c].x, g.ent[c].y, g.ent[c].z), (0, 0, 0));
        assert_ne!(g.ent[c].f140, 5000, "the purse is read off the BALL");
        assert_eq!(g.ent[c].f144, 0, "and `a2x->+144 = 0` lands on the ball");
    }

    /// ⭐⭐ **A CREATURE THAT SEIZES ITSELF FOR ITS OWN SHOT FIRES AN
    /// UNOWNED, UNTARGETED BOLT FROM THE MAP ORIGIN** — the shooter
    /// thunks read `+24`, `+146` and `+84` off the shooter after the
    /// ctor returns (`sub_1A8E0` :21890-98 and its eight siblings),
    /// and aim from the shooter's `+72`, which the seizure has turned
    /// into the bolt's own zeroed axis.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_MC1_SELF_SEIZE=1` — the bolt
    /// would be posed at the creature's (20000, 20000) and carry its
    /// owner and target.
    #[test]
    fn a_creature_seized_by_its_own_shot_fires_from_the_origin() {
        let mut g = flat_gen(rings_at(0, 1), VerbSet::MC1);
        let c = g
            .spawn_creature(0, 20000, 20000, 100)
            .expect("shooter slot");
        g.ent[c].id24 = OWNER;
        g.ent[c].f146 = 321;
        g.ent[c].f84 = 200;
        seize_next(&mut g, c);
        let fired = g.attack_thunk(c, 0, 321, 40000, 20000, 100, 3, 0xFF);
        assert!(fired, "the thunk allocated (by seizing the shooter)");
        assert_eq!(
            (g.ent[c].class64, g.ent[c].model65),
            (9, 0),
            "the shooter's record IS the fireball now"
        );
        assert_eq!(
            (g.ent[c].x, g.ent[c].y),
            (0, 0),
            "the bolt links at its own zeroed axis, not the shooter's"
        );
        assert_eq!(g.ent[c].id24, c as u16, "owner is the post-alloc identity");
        assert_eq!(g.ent[c].f146, 0, "target is the bolt's own (none)");
    }

    /// ⭐⭐⭐ **THE FIRE-SPREADER KEEPS ITS EMITTER IN ONE REGISTER, SO
    /// SEIZING ITS OWN RECORD RE-HOMES THE REST OF THE SPRAY** — law
    /// W5's second call path, `sub_25130` (:28127-76).
    ///
    /// Byte-proved in the shipped `CARPET.EXE` (file = VA + 0x187f8;
    /// the routine begins at file 0x3d928 = VA 0x25130). `a1x` never
    /// leaves `%ebx`: the ring pitch is `69 44 24 0c c0 00 00 00`
    /// (`imul eax,[esp+0xc],0xc0` = 192) and the position operands are
    /// re-read INSIDE the loop — `0f bf 4b 48` (`movsx ecx,word
    /// [ebx+0x48]`, +72 x) at file 0x3d9f2, `0f bf 43 4a` (+74 y) at
    /// 0x3da24, `66 8b 43 4c` (+76 z) at 0x3da39. The allocator call
    /// `e8 9a 21 01 00` (`call sub_373F0`) sits at file 0x3da49, and
    /// EVERY operand the child is stamped from is read AFTER it:
    /// `66 8b 53 18` (+24 owner) at 0x3da55, `66 8b 53 1e` (+30 aim)
    /// at 0x3da5d, and `8b 53 10 / 81 e2 00 00 01 00` (+16 masked with
    /// 0x10000, the inherited marker) at 0x3da65.
    ///
    /// So when the free stack is dry and `new_event` sacrifices the
    /// spreader ITSELF, `%ebx` aliases the newborn fire: the remaining
    /// cells are laid around the CHILD's position, they take the
    /// child's `+24` (which `new_event` just set to its own slot), the
    /// child's `+30` (0, not the spreader's heading) and the child's
    /// `+16` (which carries no 0x10000 to inherit).
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_EFFECT_RING_LIVE_OPERANDS=1`
    /// hoists all six operands above the loop; the members then sit
    /// around the spreader's own x=1000, carry `id24 = 504`,
    /// `f30 = 724` and the inherited 0x10000.
    #[test]
    fn a_self_seized_spreader_lays_its_ring_around_its_own_child() {
        let (mut g, s, members) = rig(rings_at(2, 4), VerbSet::MC1, 1, 2, 4);
        g.spreader_tick(s);
        assert!(
            g.mc2_recycle.seized >= 2,
            "the rig must have seized the spreader and at least one more slot"
        );
        assert_reads_the_alias(&g, s, members[0], 192);
        assert_eq!(
            g.ent[members[0]].f30, 0,
            "+30 is read off the alias too — the newborn's 0, not the spreader's {AIM}"
        );
        assert_eq!(
            g.ent[members[0]].flags & 0x10000,
            0,
            "…and so is the inherited +16 marker: the newborn has none to pass on"
        );
    }

    /// ⭐⭐⭐ **THE WALL-OF-FIRE CLOUD DOES IT TOO, AND ITS WAVE
    /// COUNTER IS THE PROOF** — law W5's third call path, base-MC1
    /// `sub_29780` (:31140-84).
    ///
    /// `CARPET.EXE` file 0x41f78 = VA 0x29780, same `%ebx` idiom:
    /// pitch `6b 44 24 0c 70` (`imul eax,[esp+0xc],0x70` = 112) at
    /// file 0x4200a, `0f bf 4b 48` (+72 x) at 0x4200f, `0f bf 43 4a`
    /// (+74 y) at 0x4203e, `66 8b 43 4c` (+76 z) at 0x42053, the
    /// allocator `e8 80 db 00 00` (`call sub_373F0`) at 0x42063, and
    /// then AFTER it `66 8b 53 18` (+24 owner) at 0x42071,
    /// `66 8b 53 2c` (+44 damage) at 0x42079, `66 83 7b 1a 00`
    /// (`cmp word [ebx+0x1a],0x0` — the +26 WAVE test that picks the
    /// child's life 14 or 1) at 0x42081, and `0f bf 53 1a` (+26 again,
    /// `<<8` then halved = wave·128 into the child's +46) at 0x420af.
    /// The wave step after the loop re-reads it once more:
    /// `66 8b 43 1a / 40 / 66 89 43 1a / 66 3d 0e 00` at file 0x420ee.
    ///
    /// A cloud that seizes its own record therefore reads its
    /// newborn's `+26` = 0 for the rest of the tick: the child gets
    /// the WAVE-0 fourteen-tick ground patch instead of a one-tick
    /// sheet, later members ride `+46 = 7·128` (the flame ctor's own
    /// `+26 = 7`) instead of `wave·128`, and the post-loop step lands
    /// on the child's counter.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_EFFECT_RING_LIVE_OPERANDS=1` —
    /// the seized slot then keeps life 1, the members read
    /// `f46 = 2·128 = 256`, and the cloud's counter steps 2 → 3.
    #[test]
    fn a_self_seized_napalm_cloud_lays_its_waves_around_its_own_child() {
        // `napalm_tick` walks rings 0..=1, so the rig fills both.
        let mut r: Vec<Vec<(u8, u8)>> = (0..32).map(|_| vec![(0u8, 0u8), (0, 0)]).collect();
        r[0] = vec![(CELL as u8, CELL as u8); 2];
        r[1] = vec![(CELL as u8, CELL as u8); 3];
        let (mut g, s, members) = rig(r, VerbSet::MC1, 53, 2, 4);
        assert!(!g.is_hidden_worlds(), "this leg is the base-MC1 branch");
        let rig_ctx = ctx(&mut g);
        g.napalm_tick(s, &rig_ctx);
        assert!(g.mc2_recycle.seized >= 2, "the rig must have self-seized");
        assert_reads_the_alias(&g, s, members[0], 112);
        assert_eq!(
            g.ent[s].act_life, 14,
            "the wave test read the NEWBORN's +26 = 0, so the child took the \
             persistent wave-0 patch life"
        );
        assert_eq!(
            g.ent[members[0]].f46,
            7 * 128,
            "later members ride the alias's +26 (the flame ctor's 7), not wave 2"
        );
        assert_eq!(
            g.ent[s].f26, 8,
            "and the post-loop wave step lands on the child's counter (7 + 1), \
             not the cloud's (2 + 1)"
        );
    }

    /// ⭐⭐⭐ **AND THE HIDDEN WORLDS GEOMETRY, WHOSE RING RADIUS IS
    /// THE SAME `+26`** — law W5's fourth call path, `sub_29780`'s
    /// `IsHiddenWord` branch (`reference/remc1hw/sub_main.cpp`
    /// :29740-72).
    ///
    /// Byte-proved in the shipped `HIDDEN.EXE` (same file = VA +
    /// 0x187f8 mapping): the branch's extents write
    /// `0f bf 53 1a / lea eax,[edx*4] / sub / shl 8 / sar 2` (192·var26)
    /// is at file 0x421d4, its pitch is `69 44 24 0c a0 00 00 00`
    /// (160) at file 0x42265, and the loop re-reads `0f bf 73 48`
    /// (+72 x) at 0x4226d, `0f bf 43 4a` (+74 y) at 0x4229f and
    /// `66 8b 43 4c` (+76 z) at 0x422b4. The allocator
    /// `e8 df de 00 00` sits at file 0x422c4 and BOTH inherited
    /// operands are read after it: `66 8b 43 18` (+24) at 0x422d2 and
    /// `66 8b 43 1e` (+30 yaw) at 0x422da. The ring step
    /// `66 8b 53 1a / 83 c2 02 / idiv si(7) / 66 89 53 1a` re-reads
    /// `+26` off `%ebx` after the loop, at file 0x4232b.
    ///
    /// WHAT WOULD BREAK IT: `MGC_NO_EFFECT_RING_LIVE_OPERANDS=1` —
    /// the members then keep the cloud's yaw 724 and its owner 504,
    /// sit around x=1000, and the radius steps (3 + 2) % 7 = 5 instead
    /// of the newborn's (0 + 2) % 7 = 2.
    #[test]
    fn a_self_seized_hw_napalm_cloud_lays_its_ring_around_its_own_child() {
        let (mut g, s, members) = rig(rings_at(3, 4), VerbSet::MC1HW, 53, 3, 4);
        assert!(g.is_hidden_worlds(), "this leg is the HW branch");
        let rig_ctx = ctx(&mut g);
        g.napalm_tick(s, &rig_ctx);
        assert!(g.mc2_recycle.seized >= 2, "the rig must have self-seized");
        assert_reads_the_alias(&g, s, members[0], 160);
        assert_eq!(
            g.ent[members[0]].f30, 0,
            "+30 is read off the alias — the newborn's 0, not the cloud's {AIM}"
        );
        assert_eq!(
            g.ent[s].f26, 2,
            "the ring step reads the child's +26 = 0 -> (0 + 2) % 7, not (3 + 2) % 7"
        );
    }

    /// Round 168 ([`no_mc1hw_shot_stats_m16`]): `HIDDEN.EXE`'s shot
    /// gate takes model 16 (file 0x6b40f `cmp $0x10,%al; jb ret; jbe
    /// ok`), `CARPET.EXE`'s returns on it. WITNESS mc1hwl9-crashed
    /// t=808. `MGC_NO_MC1HW_SHOT_STATS_M16=1` fails the HW assert.
    #[test]
    fn the_hidden_worlds_shot_gate_counts_the_beam_segment() {
        let (mc1, ..) = rig(rings_at(3, 4), VerbSet::MC1, 53, 3, 4);
        let (hw, ..) = rig(rings_at(3, 4), VerbSet::MC1HW, 53, 3, 4);
        for m in [0u8, 1, 3, 7, 8, 9, 19] {
            assert!(mc1.mc1_shot_counts(m) && hw.mc1_shot_counts(m), "model {m}");
        }
        for m in [2u8, 4, 5, 6, 10, 11, 12, 13, 14, 15, 17, 18, 20] {
            assert!(
                !mc1.mc1_shot_counts(m) && !hw.mc1_shot_counts(m),
                "model {m}"
            );
        }
        assert!(
            !mc1.mc1_shot_counts(16),
            "CARPET.EXE returns on the beam segment"
        );
        assert!(hw.mc1_shot_counts(16), "HIDDEN.EXE counts it");
    }

    /// Round 168 ([`Gen::mc1_undead_bolt_44`]): `sub_57800` leaves the
    /// Undead-Army bolt's `+44` at 1, or at the token's price when the
    /// owner's castle register names a record whose `+136` covers it
    /// (:65962-71) — an index test alone. WITNESS mc1hwl0 t=47086
    /// (13000 under a castle storing 30,000,000).
    /// `MGC_NO_MC1_UNDEAD_BOLT_44=1` answers `None`.
    #[test]
    fn the_undead_bolt_carries_one_or_the_housed_price() {
        let (mut g, ..) = rig(rings_at(3, 4), VerbSet::MC1, 53, 3, 4);
        assert_eq!(g.mc1_undead_bolt_44(0, 13000), Some(1), "no register: 1");
        let c = g.new_event().expect("a record");
        g.castle_reg[0] = c as u16;
        g.ent[c].f136 = 12_999;
        assert_eq!(
            g.mc1_undead_bolt_44(0, 13000),
            Some(1),
            "a store under the price"
        );
        g.ent[c].f136 = 13_000;
        assert_eq!(
            g.mc1_undead_bolt_44(0, 13000),
            Some(13000),
            "covered: the price"
        );
        // Blind: the record need not be a castle.
        assert_ne!(g.ent[c].class64, 3);
        assert_eq!(
            g.mc1_undead_bolt_44(1, 13000),
            Some(1),
            "another wizard's register"
        );
    }

    /// Round 168 ([`Gen::mc1_shot_stats_silent`]): the no-spawn end of
    /// the state 7 / 8 handler is `sub_526C0(a1, 0, ..)` (:63208) — a
    /// shot and never a hit, for the human's gated models only.
    /// WITNESS mc1l32-quick t=9962 (a rebounded fireball timing out).
    /// `MGC_NO_MC1_SILENT_END_SHOT=1` fails the first assert.
    #[test]
    fn a_silent_end_is_a_shot_and_never_a_hit() {
        let (mut g, ..) = rig(rings_at(3, 4), VerbSet::MC1, 53, 3, 4);
        let b = g.new_event().expect("a bolt");
        g.ent[b].class64 = 9;
        g.ent[b].model65 = 0;
        g.ent[b].id24 = crate::mc1::mobs::PLAYER_TARGET;
        let (s0, h0) = (g.shots[Hx::FIRST], g.hits[Hx::FIRST]);
        g.mc1_shot_stats_silent(b);
        assert_eq!(
            (g.shots[Hx::FIRST], g.hits[Hx::FIRST]),
            (s0 + 1, h0),
            "shots++ alone"
        );
        // A model behind the gate (the volcano lob) scores nothing…
        g.ent[b].model65 = 4;
        g.mc1_shot_stats_silent(b);
        assert_eq!(
            g.shots[Hx::FIRST],
            s0 + 1,
            "model 4 returns before `shots++`"
        );
        // …and neither does a bolt that is not the human's.
        g.ent[b].model65 = 0;
        g.ent[b].id24 = 600;
        g.mc1_shot_stats_silent(b);
        assert_eq!(
            g.shots[Hx::FIRST],
            s0 + 1,
            "the owner test is class 3, model 0"
        );
    }
}
