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

use crate::mgcr::{Notify, ObsMc1, RetailMc1, RetailMc2};

/// A retail CHEAT fired by the recorded player — control opcode 30
/// (`0x1E`), `param1` = the discriminant below. Both engines bind the
/// same keys: ALT held + F1..F7 on MC1 (remc1 :20018-46 / :20423-61),
/// ALT + F1..F10 on MC2 (PlayerInput.cpp:95-160); the enable gate is
/// MC1's `-cheat N` command line and MC2's tester flag
/// (`setting_byte2_23 < 0`) or the wizard name "chronicle".
///
/// ## Why the toast, and not the keys
///
/// The opcode itself is UNRECOVERABLE from a capture: retail memsets
/// the 10-byte control command at the end of the same event pass
/// (remc1 :49044) and the recorder's settled window opens after that,
/// so opcode 30 appears zero times in any take. The raw key channel
/// does see the F-key, but it carries the documented ±1-tick
/// attribution caveat and cannot tell a held key from a repeat.
///
/// The handler's OWN on-screen message can do both: it names the cheat
/// and it re-arms a lifetime counter that otherwise only counts down,
/// and it lands in the per-player block INSIDE the captured closure.
/// Measured over the two cheat takes: mc1l0-test 23/23 fires and
/// mc2l0-test 103/103, each matching a key press edge 1:1 with zero
/// misses and zero false positives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cheat {
    /// 1 — grant every spell not already held, as a real pool
    /// manifestation each (MC1 class 12 ×24, MC2 class 15 ×26).
    AllSpells,
    /// 2 — spawn a (10,39) sphere holding 100000 mana, and top the
    /// caster's own mana up to its maximum.
    MoreMana,
    /// 3/4/5 — mass kill by model: rival players / castles / balloons.
    DestroyPlayers,
    DestroyCastles,
    DestroyBalloons,
    /// 6 — restore the caster to full life.
    Heal,
    /// 7 — kill every creature on the map.
    KillCreatures,
    /// 8 (MC2) — +100 volatile XP on all 26 spells, then re-derive
    /// every tier. THE TIER UNLOCK: [`Cheat::AllSpells`] grants spells
    /// at level 0 only, so a take that exercises tier 1/2 needs this.
    SpellXp,
    /// 9 (MC2) — toggle free spell usage (the built manifestation
    /// takes `mana = 1, manaRegen = 0`).
    FreeSpell,
    /// 10 (MC2) — toggle invincibility.
    Invincible,
}

impl Cheat {
    /// The retail sub-code (`param1` of control opcode 30).
    pub fn code(self) -> u8 {
        match self {
            Cheat::AllSpells => 1,
            Cheat::MoreMana => 2,
            Cheat::DestroyPlayers => 3,
            Cheat::DestroyCastles => 4,
            Cheat::DestroyBalloons => 5,
            Cheat::Heal => 6,
            Cheat::KillCreatures => 7,
            Cheat::SpellXp => 8,
            Cheat::FreeSpell => 9,
            Cheat::Invincible => 10,
        }
    }

    /// Parse a sub-code back (the `.mgcr` port-input lane).
    pub fn from_code(code: u8) -> Option<Cheat> {
        Some(match code {
            1 => Cheat::AllSpells,
            2 => Cheat::MoreMana,
            3 => Cheat::DestroyPlayers,
            4 => Cheat::DestroyCastles,
            5 => Cheat::DestroyBalloons,
            6 => Cheat::Heal,
            7 => Cheat::KillCreatures,
            8 => Cheat::SpellXp,
            9 => Cheat::FreeSpell,
            10 => Cheat::Invincible,
            _ => return None,
        })
    }

    /// A short label for reports.
    pub fn name(self) -> &'static str {
        match self {
            Cheat::AllSpells => "all-spells",
            Cheat::MoreMana => "more-mana",
            Cheat::DestroyPlayers => "destroy-players",
            Cheat::DestroyCastles => "destroy-castles",
            Cheat::DestroyBalloons => "destroy-balloons",
            Cheat::Heal => "heal",
            Cheat::KillCreatures => "kill-creatures",
            Cheat::SpellXp => "spell-xp",
            Cheat::FreeSpell => "free-spell",
            Cheat::Invincible => "invincible",
        }
    }
}

/// The handlers' message strings, verbatim (remc1 :48904-49009, remc2
/// EF:37817-91). The ON/OFF toggles share a prefix — matching the
/// prefix keeps the TOGGLE semantic (the recording tells us it
/// flipped, not which way, and retail's own state is the flip).
const CHEAT_TOASTS: &[(&str, Cheat)] = &[
    (".. CHEAT: access all spells", Cheat::AllSpells),
    (".. CHEAT: more mana", Cheat::MoreMana),
    (".. CHEAT: destroy all players", Cheat::DestroyPlayers),
    (".. CHEAT: destroy all castles", Cheat::DestroyCastles),
    (".. CHEAT: destroy all balloons", Cheat::DestroyBalloons),
    (".. CHEAT: heal", Cheat::Heal),
    (".. CHEAT: Kill all creatures", Cheat::KillCreatures),
    (".. CHEAT: More Spell Experience Points", Cheat::SpellXp),
    (".. CHEAT: Free Spell Usage", Cheat::FreeSpell),
    (".. CHEAT: Invincability", Cheat::Invincible),
];

/// The cheat this toast NAMES, ignoring whether it just fired.
fn cheat_named(cur: &Notify) -> Option<Cheat> {
    let text = cur.text();
    CHEAT_TOASTS
        .iter()
        .find(|(s, _)| text.starts_with(s))
        .map(|&(_, c)| c)
}

/// The cheat fired across one recorded MC1 pair, from the caster's own
/// message slot: the counter must have been RE-ARMED (it only counts
/// down otherwise) and the text must name a cheat. Repeats of the
/// same cheat are distinguished by the counter alone — the text does
/// not change between them.
///
/// ⚠⚠ THE FIRE EDGE IS PER-GAME — see [`Notify::fired_since_mc1`].
/// MC1's counter clamps at 0 where MC2's wraps to 0xFFFF, so sharing
/// one rule made every expired MC1 cheat toast re-fire its cheat on
/// every subsequent tick.
pub fn cheat_fired_mc1(prev: &Notify, cur: &Notify) -> Option<Cheat> {
    cheat_named(cur).filter(|_| cur.fired_since_mc1(prev))
}

/// The MC2 twin of [`cheat_fired_mc1`], on MC2's own expiry rule
/// ([`Notify::fired_since_mc2`]).
pub fn cheat_fired_mc2(prev: &Notify, cur: &Notify) -> Option<Cheat> {
    cheat_named(cur).filter(|_| cur.fired_since_mc2(prev))
}

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
    /// A retail cheat the recorded player fired on this pair — the
    /// one input verb that MUTATES the world rather than steering it,
    /// so a free-running consumer has to apply it or diverge
    /// permanently from that tick ([`Cheat`]).
    pub cheat: Option<Cheat>,
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
        cheat: cheat_fired_mc1(&pw.notify, &cw.notify),
        ..RecoveredPair::default()
    }
}

/// Recover the input consumed across an MC2 pair (records N → N+1).
/// `respawn` is the [`Mc2RespawnWitness`] verdict for the END record
/// (the witness folds EVERY record in stream order, anchors included,
/// so its state machine lives outside the pair). `input_end` is the
/// END record's raw input channel (the demolish key corroboration).
/// ⭐ THE WALL DEAD-STOP IS A SIM EFFECT, NOT AN INPUT — so the
/// capture's own speed command has to be un-done before it can be
/// replayed as one.
///
/// A BLOCKED move restores the carpet's position and zeroes
/// `speed_0xc_12` at the END of the frame (EF:59595-602) — after
/// `sub_5D530`'s servo has already stepped `actSpeed` from the command
/// as it stood mid-frame. The capture at N+1 therefore holds the
/// POST-block 0, and handing that straight back as the pair's target
/// applies the dead-stop a whole frame early. Consumers model the stop
/// themselves (`flight::move_mc2`, `out.zero_speed`), so they re-zero
/// on their own; what they need is the command the frame actually
/// served.
///
/// That command is N's, advanced by the frame's OWN ±16 key
/// integration (`sub_5F380`, EF:60748 — the guarded form: a key only
/// steps while the target is inside ±80), which the move_bits lane
/// still reports. mc2l3 t=3407: the carpet flies into a wall at 48
/// with forward held, retail integrates the command to 64 and the
/// servo lands actSpeed on 64 before the block wipes it — reading the
/// wiped 0 back produced 48. mc2l3 t=2639 is the other branch: backing
/// at −80 with the target already at the floor, the integration is a
/// no-op and the servo holds −80 for the frame.
///
/// Witness (`pose_forced`) = the carpet's pose was NOT the mover's
/// output this pair, with the command newly 0 while `actSpeed` is
/// still RUNNING. Two shapes reach it and both zero the command from
/// outside the servo:
///   * FROZEN — the blocked move above. (The modal park is the
///     `speed == 0` twin of the same freeze; the two stay disjoint.)
///   * WARPED — the pose jumped further than any mover step could
///     carry it, so a spell/pad placed it. mc2l3 t=3933: a Teleport
///     lands the carpet 119 tiles away and clears the command; retail
///     holds actSpeed at −16 for that frame and drops to 0 at 3934,
///     where the port read the cleared command and dropped a tick
///     early.
///
/// ⭐⭐ A BLOCK THAT LASTS MORE THAN ONE FRAME HAS N's COMMAND ALREADY
/// WIPED. The two shapes above both entered the block from a live
/// command, so the first draft also demanded `prev_cmd != 0` — an
/// accidental property of its two exemplars, not part of the law. Hold
/// a wall down and the stop fires EVERY frame: from the second one on
/// the capture reads 0 at both ends while `sub_5F380` keeps re-adding
/// its +16, so `actSpeed` STALLS at 16 instead of decaying. mc2l3
/// t=6514..6516 is three such frames in a row — retail holds 16, 16,
/// 16 with forward held and the pose pinned, then drops to 0 at 6517
/// the moment the key releases. The integration is the law; the
/// starting value is just its argument.
pub fn mc2_pair_cmd_speed(
    prev_cmd: i16,
    cur_cmd: i16,
    move_bits: u32,
    pose_forced: bool,
    cur_speed: i16,
) -> i16 {
    if !(cur_cmd == 0 && cur_speed != 0 && pose_forced) {
        return cur_cmd;
    }
    let mut dir: i16 = 0;
    if move_bits & 1 != 0 && prev_cmd < 80 {
        dir = 1;
    }
    if move_bits & 2 != 0 && prev_cmd > -80 {
        dir = -1;
    }
    (prev_cmd + 16 * dir).clamp(-80, 80)
}

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
    // ⭐⭐ A TIER SWAP IS A SELECT THAT MOVES NO HAND POINTER. The
    // pane commits every pick through the same handler (PlayerAction
    // 0x1F/0x20, EF:37898-928: persist the tier, bind the quick-slot,
    // SetSpell, sound 14) and TWO recorded shapes reach it — the
    // pointer moves because a DIFFERENT spell was equipped, or the
    // pointer stays and only `array_0x437[spell]` moves because the
    // player picked a higher TIER of the spell already in that hand.
    // Keying recovery on the pointer alone saw the first and dropped
    // the second, which is how EVERY upgrade past level 0 arrives:
    // mc2l0 t=7728 records `hand_pending 1 -> 0`, `sel[0] 0 -> 1` and
    // the notification "FireBall" -> "Rapid Fire", with both hand
    // pointers untouched — retail re-prices the held class-15 token
    // (mana_max 100 -> 250, @0x2A 250 -> 160) and the port stayed on
    // the base tier for the rest of the take. Every MC2 recording
    // that uses a spell above level 0 breaks at its first swap.
    //
    // The hand is named by the pane's PENDING byte at the START
    // record (`byte_0x457_1111`: 1 = a left equip mid-flight, 2 =
    // right, PI:806-91), which the commit clears to 0. With no
    // pending byte to read, fall back to whichever hand already holds
    // the spell — the bind is a no-op there by definition.
    let tier_swap = (0..26).find(|&s| pp.sel[s] != cp.sel[s]).and_then(|s| {
        let hand = match pp.hand_pending {
            1 => 0u8,
            2 => 1u8,
            _ if cp.hand_left == s as i16 => 0,
            _ if cp.hand_right == s as i16 => 1,
            _ => return None,
        };
        Some((s as u8, cp.sel[s], hand))
    });
    let (mc2_select, rebind_dropped) = match (left, right) {
        (Some(l), Some(_)) => (Some(l), true),
        (l, r) => (l.or(r).or(tier_swap), false),
    };
    // Demolish (Shift+L → `PlayerAction` 0x2A, EF:37991-96): MC2's
    // command never touches the move byte, so the witness is the own
    // CASTLE's KILL EDGE — alive at the START record, exactly −1 at
    // the END record — corroborated by the held Shift+L scancodes
    // (L = 38, either shift 42/54; measured at mc2l24 t=41798).
    //
    // ⭐⭐⭐ THE ACTION IS NOT PART OF THE WITNESS. Retail's handler is
    // three lines — `if (castle > Entities[0]) { if (level == 1)
    // byte_0x1BE_446 = 1; castle->life_0x8 = -1; }` — and it tests
    // NOTHING about the castle's state. An `action45 == 6` clause is
    // therefore a test the PORT invented, and it only happens to hold
    // for a demolish that lands on a castle standing at rest: the
    // standing tick converts life < 0 into action 6 in the SAME tick,
    // so the END record shows 6. A castle demolished again while it is
    // still inside the previous rung's build state machine (action 5)
    // never reaches the destroy intake that tick, so the clause
    // silently dropped the press.
    //
    // mc2l3's self-destruct is exactly that case, and it is the level's
    // certification blocker: the player HAMMERS Shift+L to walk the
    // castle down rung by rung, and the second press lands at t=15910
    // with the castle at action 5 / `word_0x2E_46` 3 (the repaint
    // painter it just minted). Retail writes life −1 anyway; the port
    // saw action 5 and replayed no press at all, so the whole rest of
    // the demolish ladder — and the re-site that opens the sealed
    // chamber — never happened.
    //
    // The kill EDGE (rather than `life == -1` outright) is what keeps
    // the parked castle from re-firing for the ~45 ticks it sits dead
    // at action 5 with the key still down; the write itself is
    // idempotent, but the +3000 surcharge latch below is not.
    let demolish = {
        let castle = cp.castle_ent as usize;
        let alive_before = pst
            .ents
            .get(castle)
            .is_some_and(|c| c.class3f == 3 && c.life >= 0);
        cp.castle_ent > 0
            && alive_before
            && st
                .ents
                .get(castle)
                .is_some_and(|c| c.class3f == 3 && c.life == -1)
            && key_held(input_end, 38)
            && (key_held(input_end, 42) || key_held(input_end, 54))
    };
    // The modal park (big map / spell book): the game keeps running
    // but the carpet stops dead. Witness = the consumed command at 0
    // AND the carpet entity pinned in place at zero speed across the
    // pair.
    //
    // ⚠⚠ THE OLD WITNESS'S OWN JUSTIFICATION WAS FALSE — it read "a
    // live zero-crossing never freezes the position too", and every
    // zero-crossing freezes it. `sub_5D530` runs the ±16 servo BEFORE
    // the polar step (EF:59636-44, then :59668), so on the very tick a
    // braking command reaches 0 the step is taken at speed 0 and x/y
    // do not move at all. mc2l3 t=605 is the counterexample: the down
    // key walks the command 16 → 0, retail's carpet holds 25240/48943
    // exactly, and all three park clauses fire on a carpet nobody
    // parked. Harmless while the harness pinned the command every
    // tick; fatal once the register carries itself, because the park
    // arm zeroes BOTH registers ahead of `sub_5F380` and the next key
    // press then integrates from 0 instead of 16.
    //
    // A held speed key is the discriminator: the modal screens eat the
    // movement keys, so a real park never carries one.
    let ci = cp.play_index as usize;
    let mc2_park = cp.cmd_speed == 0
        && mb & 3 == 0
        && matches!((pst.ents.get(ci), st.ents.get(ci)), (Some(p), Some(c))
            if c.speed == 0 && p.x == c.x && p.y == c.y);
    // Pose NOT mover-driven this pair: frozen (a blocked move) or
    // warped further than any mover step could carry it (2048 is the
    // pose channel's own warp gate; the mover's reach is ~450).
    let pose_forced = matches!((pst.ents.get(ci), st.ents.get(ci)), (Some(p), Some(c))
        if (p.x == c.x && p.y == c.y)
            || (c.x.wrapping_sub(p.x) as i16).unsigned_abs() > 2048
            || (c.y.wrapping_sub(p.y) as i16).unsigned_abs() > 2048);
    let cur_speed = st.ents.get(ci).map_or(0, |c| c.speed);
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
        mc2_cmd_speed: Some(mc2_pair_cmd_speed(
            pp.cmd_speed,
            cp.cmd_speed,
            mb,
            pose_forced,
            cur_speed,
        )),
        mc2_park,
        cheat: cheat_fired_mc2(&pp.notify, &cp.notify),
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

#[cfg(test)]
mod cheat_tests {
    use super::*;

    fn notify(text: &str, ticks: u16) -> Notify {
        let mut raw = [0u8; Notify::CAP];
        raw[..text.len()].copy_from_slice(text.as_bytes());
        Notify::from_parts(raw, ticks)
    }

    /// The counter is what dates a cheat, not the text: retail re-arms
    /// it to 100 and it ticks DOWN, so a snapshot taken after the
    /// firing tick reads 99. Any INCREASE is a fresh `ShowMessage`.
    /// Both engines agree on this much.
    #[test]
    fn a_repeat_of_the_same_cheat_is_a_fresh_fire() {
        let a = notify(".. CHEAT: more mana", 96);
        let b = notify(".. CHEAT: more mana", 99);
        // …and the ordinary count-down in between is not.
        let c = notify(".. CHEAT: more mana", 98);
        for fired in [cheat_fired_mc1, cheat_fired_mc2] {
            assert_eq!(fired(&a, &b), Some(Cheat::MoreMana));
            assert_eq!(fired(&b, &c), None);
        }
    }

    /// ⚠⚠ THE EXPIRED SLOT IS WHERE THE TWO ENGINES PART, AND GETTING
    /// IT WRONG RE-FIRES THE CHEAT FOREVER.
    ///
    /// remc1 decrements only from inside `if (periods > 0)`
    /// (:26526/:26531), so an MC1 toast CLAMPS at 0 and holds there
    /// with its text intact. Applying MC2's rule — which expects the
    /// step off 0 to wrap to 0xFFFF — reads every one of those
    /// stationary boundaries as a fresh fire, which is exactly what
    /// made an MC1 cheat take dispense mana on every tick from ~100
    /// ticks after the single real press.
    #[test]
    fn an_expired_mc1_toast_clamps_at_zero_and_never_refires() {
        let dead = notify(".. CHEAT: more mana", 0);
        assert_eq!(cheat_fired_mc1(&dead, &dead), None);
        // The last real step down, and the clamp it lands on.
        assert_eq!(
            cheat_fired_mc1(&notify(".. CHEAT: more mana", 1), &dead),
            None
        );
        // NON-VACUITY: the MC2 rule is what got this wrong.
        assert!(dead.fired_since_mc2(&dead));
        assert!(!dead.fired_since_mc1(&dead));
        // A genuine re-press out of the clamped slot still registers.
        assert_eq!(
            cheat_fired_mc1(&dead, &notify(".. CHEAT: more mana", 99)),
            Some(Cheat::MoreMana)
        );
    }

    /// MC1 ages the slot in the RENDER loop, so a recorded boundary
    /// with no intervening draw leaves the counter unmoved. That is a
    /// hold, not a fire — and unlike MC2 there is no `== prev - 1`
    /// requirement to trip over.
    #[test]
    fn an_mc1_boundary_without_a_draw_is_a_hold_not_a_fire() {
        let a = notify(".. CHEAT: heal", 57);
        assert_eq!(cheat_fired_mc1(&a, &a), None);
        // Two frames drawn inside one recorded boundary: still decay.
        assert_eq!(cheat_fired_mc1(&a, &notify(".. CHEAT: heal", 55)), None);
    }

    /// The MC2 side of the same seam, kept pinned beside its twin: the
    /// counter steps off 0 to 65535 and PARKS, and a real fire out of
    /// that parked slot is the `@65535 -> @99` shape (the
    /// mc2l0-spells-galore t=909 corpus row).
    #[test]
    fn an_expired_mc2_toast_parks_on_ffff() {
        let zero = notify(".. CHEAT: more mana", 0);
        let parked = notify(".. CHEAT: more mana", u16::MAX);
        assert_eq!(cheat_fired_mc2(&zero, &parked), None);
        assert_eq!(cheat_fired_mc2(&parked, &parked), None);
        assert_eq!(
            cheat_fired_mc2(
                &notify("Lightning Tower", u16::MAX),
                &notify(".. CHEAT: more mana", 99)
            ),
            Some(Cheat::MoreMana)
        );
    }

    /// Non-cheat toasts share the lane (level-up re-arms to 200, the
    /// spell-select toast to 20) and must never be mistaken for one.
    #[test]
    fn ordinary_toasts_are_not_cheats() {
        let a = notify("", 0);
        for fired in [cheat_fired_mc1, cheat_fired_mc2] {
            assert_eq!(fired(&a, &notify("Lightning Tower", 19)), None);
            assert_eq!(
                fired(&a, &notify("has been banished from the realm.", 99)),
                None
            );
        }
    }

    /// The ON/OFF toggles share a prefix — the recording says it
    /// flipped, and retail's own flag is the direction.
    #[test]
    fn toggle_cheats_match_on_the_shared_prefix() {
        let a = notify("", 0);
        for s in [
            ".. CHEAT: Free Spell Usage ON",
            ".. CHEAT: Free Spell Usage OFF",
        ] {
            assert_eq!(cheat_fired_mc1(&a, &notify(s, 99)), Some(Cheat::FreeSpell));
            assert_eq!(cheat_fired_mc2(&a, &notify(s, 99)), Some(Cheat::FreeSpell));
        }
    }

    /// Every handler string in both engines resolves, and the sub-code
    /// round-trips (the `.mgcr` port-input lane depends on it).
    #[test]
    fn every_toast_maps_and_every_code_round_trips() {
        let a = notify("", 0);
        for &(text, want) in CHEAT_TOASTS {
            assert_eq!(cheat_fired_mc1(&a, &notify(text, 99)), Some(want), "{text}");
            assert_eq!(cheat_fired_mc2(&a, &notify(text, 99)), Some(want), "{text}");
            assert_eq!(Cheat::from_code(want.code()), Some(want), "{text}");
        }
    }
}

#[cfg(test)]
mod wall_stop_tests {
    use super::mc2_pair_cmd_speed;

    /// ⭐ THE WALL/WARP DEAD-STOP IS A SIM EFFECT, NOT AN INPUT
    /// ([`mc2_pair_cmd_speed`]). Pair-BLIND by construction — the
    /// recovery is harness machinery, so no `.mgcr` fixture can pin
    /// it; all three rows below are measured off mc2l3.
    #[test]
    fn the_mc2_dead_stop_is_undone_before_the_command_is_replayed() {
        // The ordinary pair: the capture's command IS what the frame
        // served, whatever the carpet did.
        assert_eq!(mc2_pair_cmd_speed(48, 64, 1, false, 64), 64, "live command");
        assert_eq!(
            mc2_pair_cmd_speed(48, 64, 1, true, 64),
            64,
            "a frozen carpet with a NONZERO command is not a dead-stop"
        );
        // mc2l3 t=2639 — backing into a wall at the floor. The command
        // reads 0 at N+1 because the block wiped it; the frame served
        // −80, and the guarded integration cannot step past ±80, so
        // the servo holds actSpeed at −80 for that frame.
        assert_eq!(
            mc2_pair_cmd_speed(-80, 0, 2, true, -80),
            -80,
            "the wiped command is the pre-block one, and ±80 is a floor"
        );
        // mc2l3 t=3407 — flying into a wall at 48 with forward HELD:
        // `sub_5F380` integrated the command to 64 before the mover,
        // and only then did the block wipe it.
        assert_eq!(
            mc2_pair_cmd_speed(48, 0, 1, true, 48),
            64,
            "the frame's own ±16 key integration still applies"
        );
        // mc2l3 t=3933 — a Teleport places the carpet 119 tiles away
        // and clears the command; `pose_forced` covers the warp too.
        assert_eq!(
            mc2_pair_cmd_speed(-16, 0, 16, true, -16),
            -16,
            "a warp clears the command the same way a wall does"
        );
        // The modal park is the `speed == 0` twin and must NOT be
        // re-armed: it really did zero the command as an input.
        assert_eq!(
            mc2_pair_cmd_speed(-80, 0, 0, true, 0),
            0,
            "the modal park stays disjoint from the dead-stop"
        );
        // mc2l3 t=6514..6516 — the SECOND and later frames of one
        // block: N's command is already wiped, forward is still held,
        // and retail's servo stalls actSpeed at 16 rather than
        // decaying to 0. Demanding `prev_cmd != 0` read this as an
        // ordinary zero command and dropped the carpet a tick early.
        assert_eq!(
            mc2_pair_cmd_speed(0, 0, 1, true, 16),
            16,
            "a multi-frame block re-integrates from an already-wiped command"
        );
        // ...and with no key held the wiped command really is 0, so
        // the widened gate stays a no-op on every quiet blocked frame.
        assert_eq!(
            mc2_pair_cmd_speed(0, 0, 0, true, 16),
            0,
            "no key, nothing to re-integrate"
        );
    }
}
