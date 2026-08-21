//! INPUT RECOVERY from retail `.mgcr` takes — the shared home for
//! every consumer that turns a recorded closure pair back into the
//! tick's player input: `mgc-conform` (`verify-deltas` pose channel,
//! `replay`) and the app's `--replay` (docs/RECORDING.md "Consumers").
//! Measurement provenance lives in docs/CONFORMANCE.md "The replay
//! verifier" / "The pose channel"; the laws here are decompile-
//! verified and corpus-measured — change them only with the ledger.
//!
//! The recovery is exact, byte-domain (no latency modeling):
//! - the move/fire byte (`Type_160/164 dw_0`) is stamped by retail's
//!   consume loop — bits 1/2 speed, 4/8 strafe, 0x10/0x20 the
//!   CONSUMED fire levels (cast dispatch is LEVEL-triggered via the
//!   reload ladder `f48 := f50`; no edge detection exists in retail).
//!   Corpus: 2,368/2,368 MC1 casts and 560/560 MC2 casts carry the
//!   bit on the dispatch record. PHASE is per-game: MC1 stamps
//!   post-pass (the pair reads record N), MC2 stamps in PlayerEvents
//!   (read record N+1).
//! - the stick enters the mover only through the low-pass filter
//!   `acc += (2·stick − acc)/4`, recorded at both ends of the pair,
//!   so the filter inverts exactly ([`recover_stick`]).
//! - equips/rebinds replay the recorded hand change itself
//!   ([`recover_pair_mc1`]/[`recover_pair_mc2`]).
//! - respawn rides the SPACE lane of the raw input channel; MC2 dates
//!   the press with the recentre witness ([`Mc2RespawnWitness`]), MC1
//!   has no latch and keeps the ±1-tick caveat (docs/RECORDING.md).

use crate::mgcr::{ObsMc1, RetailMc1, RetailMc2};

/// Invert the stick filter across one recorded tick: find a stick
/// value whose increment `(2·stick − acc)/4` (trunc toward zero, the
/// :49018 law) lands the accumulator exactly on acc@N+1. The mover
/// reads only the filtered value, so any solution is equivalent
/// downstream; the smallest-|stick| one is returned. None = no
/// command-range stick explains the transition (respawn wipe, a
/// non-mouse write).
pub fn recover_stick(acc_n: i16, acc_n1: i16) -> Option<i16> {
    let a = acc_n as i32;
    let d = acc_n1 as i32 - a;
    let (lo, hi) = if d > 0 {
        (4 * d, 4 * d + 3)
    } else if d < 0 {
        (4 * d - 3, 4 * d)
    } else {
        (-3, 3)
    };
    let mut best: Option<i16> = None;
    for n in lo..=hi {
        // n = 2·stick − acc: parity is pinned by acc.
        if (n + a) % 2 != 0 {
            continue;
        }
        let s = (n + a) / 2;
        if (-128..=127).contains(&s) && best.is_none_or(|b| s.abs() < (b as i32).abs()) {
            best = Some(s as i16);
        }
    }
    best
}

/// The knock the pair's mover consumed. Normally N's channel, clamped
/// ±128 like `take_knock_step`. A hit ARMS the channel mid-pass
/// before the carpet's slot, so when N+1's stored value is not the
/// pure decay of N's, reconstruct the armed value by un-decaying it
/// (measured on mc1hwl0 t=371: kmag 0→76 = 80 armed − 4, right at the
/// first dirty x/y window). Same channel shape both games (cap 128,
/// decay −4, snap <4; EF:59695-711).
pub fn consumed_knock(mag0: i16, dir0: u16, mag1: i16, dir1: u16) -> Option<(u16, i16)> {
    let m0 = mag0.clamp(-128, 128);
    let decay = if m0 == 0 {
        0
    } else {
        let d = m0 - 4 * m0.signum();
        if d.abs() < 4 { 0 } else { d }
    };
    let rearmed = mag1 != decay || (mag1 != 0 && dir1 != dir0);
    if rearmed && mag1 != 0 {
        let m = mag1 + 4 * mag1.signum();
        Some((dir1 & 0x7FF, m.clamp(-128, 128)))
    } else if m0 != 0 {
        Some((dir0 & 0x7FF, m0))
    } else {
        None
    }
}

/// SPACE (scancode 57 = 0x39) — retail's RESPAWN key, read off the
/// raw input channel's `keys_down` lane. The input dispatcher raises
/// `PlayerAction` 0xF from it, and PI:1102 accepts that command only
/// while `life < 0 && actionIndex == 3`, so a SPACE held in any other
/// state is inert and the port's own `LifeState::Dead` gate
/// reproduces the filter. Without this lane the replayed MC2 human
/// can never leave the corpse state, and the two ticks where retail
/// rebuilds the spellbook (mc2l3 t=15314→15315 and t=20611→20612)
/// are unreachable. MC2 dates the press with [`Mc2RespawnWitness`];
/// MC1 has no press latch, so the caller reads the pair's END record
/// and accepts the ±1-tick dating caveat (docs/RECORDING.md).
pub fn respawn_key(input: Option<&serde_json::Value>) -> bool {
    key_held(input, 57)
}

/// A scancode's held state off the raw input channel's `keys_down`
/// lane (the recorder emits every held scancode, unfiltered).
pub fn key_held(input: Option<&serde_json::Value>, scancode: i64) -> bool {
    input
        .and_then(|i| i.get("keys_down"))
        .and_then(|k| k.as_array())
        .is_some_and(|k| k.iter().any(|v| v.as_i64() == Some(scancode)))
}

/// The recorded LIVE cursor `(x, y)` — `input.mouse`, the twin of
/// [`press_pos`]'s press snapshot.
pub fn mouse_pos(input: Option<&serde_json::Value>) -> Option<(i16, i16)> {
    let p = input?.get("mouse")?;
    let g = |k: &str| p.get(k).and_then(|v| v.as_i64()).map(|v| v as i16);
    Some((g("x")?, g("y")?))
}

/// The recorded cursor-AT-PRESS `(x, y)` — `input.mouse_press_pos`,
/// raw twin `state.ext.press_b64` ([`crate::mgcr::Ext::press`]).
///
/// **It is NOT the cast's aim.** The ISR snapshots it on every press
/// edge (EF:51478-97) and the poll copies it to
/// `unk_18058C.x_DWORD_1805B8/1805BC` (EF:49664-65 and the three
/// sibling control-mode arms), but the ONLY consumer downstream is
/// `sub_1A7A0_fly_asistant` (PI:1988-2013) — the fly-assistant
/// idle-recentre watchdog. The player's aim/attitude command is
/// computed from the LIVE cursor, and the cast gate `sub_5F660`
/// (EF:60874) takes no aim argument at all: the launch direction
/// comes off the caster entity's own pose.
pub fn press_pos(input: Option<&serde_json::Value>) -> Option<(i16, i16)> {
    let p = input?.get("mouse_press_pos")?;
    let g = |k: &str| p.get(k).and_then(|v| v.as_i64()).map(|v| v as i16);
    Some((g("x")?, g("y")?))
}

/// DATING THE MC2 RESPAWN PRESS. The key registers carry no press
/// LATCH (the mouse's disambiguator), and the corpus shows BOTH sides
/// of retail's poll: SPACE first appears at record 15314 with the
/// reset in frame 15315 (pressed AFTER that frame's poll), and at
/// record 20612 with the reset in frame 20612 (pressed BEFORE it).
/// No held-key rule can split those, so the witness adds retail's
/// own: the 0xF handler runs `SetCenterScreenForFlyAssistant_6EDB0`
/// (EF:37653), which slams the cursor to the screen centre — so a
/// record whose cursor JUMPED this pair and now equals the
/// press-position snapshot is a record whose frame ran the command:
///
/// ```text
///   fire(pair) = space(end) && (space(start) || recentred(end))
/// ```
///
/// Measured over the whole mc2l3 take: 348 records carry the
/// recentre shape (ordinary clicks), and exactly TWO of them also
/// have SPACE down — the two reset frames. Feed [`observe`] every
/// record in stream order (anchors included).
///
/// [`observe`]: Mc2RespawnWitness::observe
#[derive(Default)]
pub struct Mc2RespawnWitness {
    prev_space: bool,
    prev_mouse: Option<(i16, i16)>,
}

impl Mc2RespawnWitness {
    /// Fold one record's raw input channel; returns whether the frame
    /// that produced this record ran the respawn command.
    pub fn observe(&mut self, input: Option<&serde_json::Value>) -> bool {
        let press = press_pos(input);
        let space = respawn_key(input);
        let mouse = mouse_pos(input);
        let recentred = mouse.is_some() && mouse != self.prev_mouse && mouse == press;
        let fire = space && (self.prev_space || recentred);
        self.prev_space = space;
        self.prev_mouse = mouse.or(self.prev_mouse);
        fire
    }
}

/// MC1 dw_0 fire bits (0x10/0x20) — the CONSUMED per-tick fire
/// levels, stamped by the same consume loop as the move bits.
pub fn mc1_fire(mb: u32) -> (bool, bool) {
    (mb & 0x10 != 0, mb & 0x20 != 0)
}

/// One pair's recovered input, format-domain (raw ids and bits; the
/// consumer widens into its own input types). MC1 fills the equip
/// lanes, MC2 the select lane; the stick lanes are per-axis Options
/// so an unrecoverable transition (respawn wipe, a non-mouse write)
/// stays visible to the consumer's gating.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecoveredPair {
    pub stick_x: Option<i16>,
    pub stick_y: Option<i16>,
    /// The consumed move/fire byte driving the pair's mover (bits 1/2
    /// speed, 4/8 strafe, 0x10/0x20 fire) — record N on MC1, N+1 on
    /// MC2 (the per-game stamp phase, module doc).
    pub move_byte: u32,
    pub fire_left: bool,
    pub fire_right: bool,
    /// MC1: a recorded hand change across the pair, resolved to the
    /// INTERNAL spell id via the acquisition list at N+1.
    pub equip_left: Option<u8>,
    pub equip_right: Option<u8>,
    /// MC2: a recorded hand change as the pane select
    /// `(spell, tier, hand)`; `(255, 0, hand)` = the unbind commit.
    pub mc2_select: Option<(u8, u8, u8)>,
    /// Both MC2 hands changed in one pair — one select per tick, the
    /// left wins, the right is DROPPED (counted by consumers).
    pub rebind_dropped: bool,
    pub respawn: bool,
    /// Shift+L, destroy own castle one level. MC1: the move byte IS
    /// the witness (`dw_0 == 48`, retail's own predicate :55760 —
    /// measured 18/18 on mc1l0, zero false positives over 7,098
    /// records). MC2: the castle-entity witness (no move-byte trace
    /// there — the command rides `PlayerAction` 0x2A).
    pub demolish: bool,
    /// MC2: the consumed target-speed COMMAND at N+1 (per-player
    /// `cmd_speed` — mouse-proportional, not the ±16 key servo). Fed
    /// to the mover as the pair's speed target the way the stick
    /// lanes feed the filters.
    pub mc2_cmd_speed: Option<i16>,
    /// MC2: the carpet is PARKED by a modal UI (big map / spell
    /// book — retail keeps playing but stops the carpet dead).
    /// Witness: command 0 AND the carpet entity frozen in place at
    /// zero speed across the pair (mc2l0 t=598: speed 80→0 in one
    /// tick, x/y/z pinned for the whole window, yaw/pitch still
    /// servoing the re-centred cursor).
    pub mc2_park: bool,
}

impl RecoveredPair {
    /// Both stick axes recovered (a pair the mover chain may own).
    pub fn stick_ok(&self) -> bool {
        self.stick_x.is_some() && self.stick_y.is_some()
    }

    /// The stick to feed the mover — centered where unrecoverable.
    pub fn stick(&self) -> (i16, i16) {
        (self.stick_x.unwrap_or(0), self.stick_y.unwrap_or(0))
    }

    /// Move byte exactly 48: retail's DEMOLISH command word
    /// (`MakeControlCommand(6, 48)` from Shift+L — the only writer
    /// that can produce exactly 16|32). It short-circuits sub_46840
    /// WHOLE (:55759): no move, no casts, and the held strafe's
    /// decay freezes. `Mc1Input` carries no fire state, so the
    /// consumer pre-feeds one decay quantum — the mover's decay
    /// lands back on the frozen value. MC1-only — MC2's sub_5F380
    /// has no such short-circuit.
    pub fn mc1_strafe_freeze(&self) -> bool {
        self.move_byte == 48
    }
}

/// Recover the input consumed across an MC1 pair (records N → N+1).
/// `input_end` is the END record's raw input channel (the respawn
/// SPACE lane — MC1's ±1-tick dating caveat, docs/RECORDING.md).
pub fn recover_pair_mc1(
    pst: &RetailMc1,
    st: &RetailMc1,
    input_end: Option<&serde_json::Value>,
) -> RecoveredPair {
    let pw = &pst.wizards[pst.local_player as usize];
    let cw = &st.wizards[st.local_player as usize];
    let mb = pw.move_bits;
    // Byte 48 exactly = the DEMOLISH command word: retail's :55760
    // short-circuit skips the WHOLE mover including both cast calls,
    // so a demolish tick fires NEITHER hand despite carrying both
    // fire bits (measured: dw_0 == 48 on exactly the 18 demolish
    // press edges of mc1l0, and the castle's `act_life = -1` lands
    // on the very next record each time).
    let demolish = mb == 48;
    let (fire_left, fire_right) = if demolish {
        (false, false)
    } else {
        mc1_fire(mb)
    };
    // Equips: a recorded hand change across the pair replays as the
    // equip command (resolved to the internal spell id via the
    // acquisition list at N+1).
    let equip = |prev_raw: u16, cur_raw: u16| -> Option<u8> {
        (prev_raw != cur_raw)
            .then(|| st.hand_spell(st.local_player as usize, cur_raw))
            .flatten()
    };
    RecoveredPair {
        stick_x: recover_stick(pw.roll_acc as i16, cw.roll_acc as i16),
        stick_y: recover_stick(pw.pitch_acc as i16, cw.pitch_acc as i16),
        move_byte: mb,
        fire_left,
        fire_right,
        equip_left: equip(pw.hand_left, cw.hand_left),
        equip_right: equip(pw.hand_right, cw.hand_right),
        respawn: respawn_key(input_end),
        demolish,
        ..RecoveredPair::default()
    }
}

/// Recover the input consumed across an MC2 pair (records N → N+1).
/// `respawn` is the [`Mc2RespawnWitness`] verdict for the END record
/// (the witness folds EVERY record in stream order, anchors included,
/// so its state machine lives outside the pair). `input_end` is the
/// END record's raw input channel (the demolish key corroboration).
pub fn recover_pair_mc2(
    pst: &RetailMc2,
    st: &RetailMc2,
    respawn: bool,
    input_end: Option<&serde_json::Value>,
) -> RecoveredPair {
    let pp = &pst.players[pst.local_player as usize];
    let cp = &st.players[st.local_player as usize];
    // MC2 stamps the move byte in PlayerEvents — read the END record.
    // Fire rides the same CONSUMED byte: measured strictly stronger
    // than the press-latch law — 560/560 retail arms carry the bit on
    // the same record, and the latch's extra edges are UI clicks the
    // byte correctly omits (ledger §THE REPLAY VERIFIER).
    let mb = cp.move_bits;
    let (fire_left, fire_right) = mc1_fire(mb);
    // Hand rebinds: a recorded hand change replays as the pane select
    // (tier = the recorded per-spell selection at N+1; out-of-range
    // spell = the unbind commit).
    let rebind = |hand: u8, prev: i16, cur: i16| -> Option<(u8, u8, u8)> {
        (prev != cur).then(|| {
            if (0..26i16).contains(&cur) {
                (cur as u8, cp.sel[cur as usize], hand)
            } else {
                (255, 0, hand)
            }
        })
    };
    let left = rebind(0, pp.hand_left, cp.hand_left);
    let right = rebind(1, pp.hand_right, cp.hand_right);
    let (mc2_select, rebind_dropped) = match (left, right) {
        (Some(l), Some(_)) => (Some(l), true),
        (l, r) => (l.or(r), false),
    };
    // Demolish (Shift+L → `PlayerAction` 0x2A, EF:37991-96): MC2's
    // command never touches the move byte, so the witness is the own
    // CASTLE at the END record carrying the demolish write — life
    // exactly −1 in the destroy intake (action 6) — corroborated by
    // the held Shift+L scancodes (L = 38, either shift 42/54;
    // measured at mc2l24 t=41798). The write is idempotent, so a
    // castle parked at −1 across records re-fires harmlessly.
    let demolish = {
        let castle = cp.castle_ent;
        castle > 0
            && st
                .ents
                .get(castle as usize)
                .is_some_and(|c| c.class3f == 3 && c.life == -1 && c.action45 == 6)
            && key_held(input_end, 38)
            && (key_held(input_end, 42) || key_held(input_end, 54))
    };
    // The modal park (big map / spell book): the game keeps running
    // but the carpet stops dead. Witness = the consumed command at 0
    // AND the carpet entity pinned in place at zero speed across the
    // pair — a live zero-crossing never freezes the position too.
    let ci = cp.play_index as usize;
    let mc2_park = cp.cmd_speed == 0
        && matches!((pst.ents.get(ci), st.ents.get(ci)), (Some(p), Some(c))
            if c.speed == 0 && p.x == c.x && p.y == c.y);
    RecoveredPair {
        stick_x: recover_stick(pp.roll_acc as i16, cp.roll_acc as i16),
        stick_y: recover_stick(pp.pitch_acc as i16, cp.pitch_acc as i16),
        move_byte: mb,
        fire_left,
        fire_right,
        mc2_select,
        rebind_dropped,
        respawn,
        demolish,
        mc2_cmd_speed: Some(cp.cmd_speed),
        mc2_park,
        ..RecoveredPair::default()
    }
}

// ------------------------------------------------- capture-grade laws

/// Is an MC1 boundary gradeable? The recorder's snapshot can tear
/// across retail's entity pass; a torn snapshot grades nothing (the
/// consumer's chain runs on regardless). Tear witnesses: entities
/// whose per-tick byte did not advance exactly once across the pair,
/// and the global LCG not being exactly one step ahead of N's.
///
/// ⚠⚠ **A SLOT THAT WAS REAPED AND RE-MINTED IS NOT A TEAR WITNESS.**
/// The `+63` census assumes the record at a slot is the SAME entity at
/// both ends of the pair, and `class`/`model` equality does not
/// establish that: a mass-spawn burst recycles slots into the same
/// `(class, model)` constantly. Worse, the collision is SYSTEMATIC
/// rather than chance — `NewEvent` seeds `+63` from the slot index
/// (:43883), so a re-minted record lands on exactly the value its
/// predecessor had been walking, i.e. a delta of 0, i.e. a "tear".
///
/// The per-entity LCG (`+4`) settles it: `NewEvent` re-seeds it
/// (`slot + global rand`, :43882), so `rand` changing across the pair
/// means a DIFFERENT entity and the `+63` comparison is meaningless.
///
/// Measured on mc1l42's kraken clash, where this mattered: boundaries
/// t=6612..6623 were all called TORN on 3-7 suspects, **every one of
/// them a re-minted `(9,9)` beam node**, while the global-LCG clause —
/// the strong witness — passed at every single tick. The recording is
/// gapless and untorn there; the heuristic went blind for twelve ticks
/// precisely because the level was spawning hard, which is where the
/// grading is worth most. Un-blinding it moves mc1l42's first
/// divergence from t=6624 (a mass-spawn slot desync, six ticks
/// downstream and nearly unreadable) to t=6618 (one entity, one flag).
pub fn capture_clean_mc1(pst: &RetailMc1, retail: &ObsMc1) -> bool {
    let mut tear_suspects = 0u32;
    for re in &retail.entities {
        let prev = &pst.ents[re.slot as usize];
        if prev.class64 == 0 || prev.class64 != re.class || prev.model65 != re.model {
            continue;
        }
        if re.rand != prev.rand {
            continue; // re-minted slot: a different entity, not a tear
        }
        if matches!(re.tick_byte.wrapping_sub(prev.f63), 0 | 2) {
            tear_suspects += 1;
            if tear_suspects > 2 {
                return false;
            }
        }
    }
    let mut x = pst.rand;
    x = x.wrapping_mul(9377).wrapping_add(9439);
    x == retail.rng
}

/// Is an MC2 pair fixture-grade? Step-1 dominance of the per-entity
/// phase byte across entities live (same class+model) at both ends.
/// Pairs with no live-in-both population (never happens on real
/// levels) fail closed.
pub fn capture_clean_mc2(pst: &RetailMc2, st: &RetailMc2) -> bool {
    let (mut d0, mut d1, mut d2) = (0u32, 0u32, 0u32);
    for slot in 1..pst.ents.len().min(st.ents.len()) {
        let (a, b) = (&pst.ents[slot], &st.ents[slot]);
        if a.class3f == 0 || a.class3f != b.class3f || a.model40 != b.model40 {
            continue;
        }
        match b.phase3e.wrapping_sub(a.phase3e) {
            0 => d0 += 1,
            1 => d1 += 1,
            2 => d2 += 1,
            _ => {}
        }
    }
    d1 > 0 && d1 >= d0 && d1 >= d2
}
