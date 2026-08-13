//! The POSE CHANNEL, tier 1 (docs/CONFORMANCE.md "The pose channel"):
//! `verify-deltas` pins the human pose, so the player's own motion
//! column is the one lane the world diff never verifies — the pinned
//! slot's pose fields are runner INPUTS, tautologically clean. This
//! module steps the faithful mover ([`flight::mc1_move`]) beside the
//! pinned world tick: flight state seeded from the recorded closure
//! at N, input recovered from the recorded flight column, stepped
//! once, and diffed against the recorded pose at N+1 — bit-exact,
//! the movers being integer ports.
//!
//! Input needs no reconstruction guesswork on MC1:
//! - the move/fire byte (`Type_160 dw_0`) is stamped by the consume
//!   loop every tick — AFTER the entity pass, for the NEXT tick's
//!   mover — and survives to the settled snapshot: the pair's boolean
//!   inputs are read straight off record N;
//! - the stick enters the mover only through the low-pass filter
//!   (`acc += (2·stick − acc)/4`), whose accumulators are recorded at
//!   both ends of the pair, so the filter is inverted per pair
//!   ([`recover_stick`]) — exact, and any solution is equivalent
//!   downstream. The map screen needs no gate: retail zeroes the
//!   command there, the accumulators visibly decay, recovery returns
//!   a centered stick.
//!
//! The world lanes are untouched: the shadow step never mutates the
//! world (ground reads the pair tick's mid-walk height snapshot —
//! the mover's own probe phase — walls the settled world), and
//! fixture signatures cannot drift because `exec_pair` is not
//! involved.

use mgc_formats::mgcr::{RetailMc1, RetailMc2};
use mgc_sim::engine::world::World;
use mgc_sim::flight::{self, Mc1Input, Mc1State, Mc2Ext};
use std::collections::BTreeMap;

/// Why a pair was not stepped. Death/respawn poses are driven by the
/// world (fall integration, castle re-seed), warps by the pad/spell
/// consumers — sim-boundary machinery the one-tick mover shadow does
/// not own; accel-domain pairs wait on the importer seeding
/// `player.speed_boost` (registered follow-up).
const GATE_DEATH: &str = "death/respawn";
const GATE_WARP: &str = "warp";
const GATE_ACCEL: &str = "accel-domain";
const GATE_STICK: &str = "stick-unrecoverable";
const GATE_WIZARD: &str = "wizard-row-missing";
const GATE_DEBUFF: &str = "debuff (web-slow/paralyze)";

#[derive(Default)]
struct LaneStat {
    rows: u64,
    max_abs: i64,
    first: Option<(u64, i64, i64)>,
}

/// The channel's tally across a run, rendered after the world stats.
#[derive(Default)]
pub struct PoseLane {
    /// Fixture-grade pairs offered.
    pub pairs: u64,
    /// Pairs actually stepped (offered minus gated).
    pub stepped: u64,
    /// Stepped pairs with EVERY lane bit-exact.
    pub exact: u64,
    gates: BTreeMap<&'static str, u64>,
    lanes: BTreeMap<&'static str, LaneStat>,
    /// Pairs whose consumed move byte was exactly 48 (both fires, no
    /// move): retail short-circuits sub_46840 whole (:55759), so the
    /// held-strafe decay does not run that tick. The step emulates
    /// the freeze (see `run_pair_mc1`); the counter keeps the family
    /// visible for triage. MC1-only — MC2's sub_5F380 has no such
    /// short-circuit.
    pub dw48: u64,
    /// Which mover the run exercised (report label).
    arm: &'static str,
}

// The stick-filter inversion and consumed-knock reconstruction laws
// moved to the shared recovery home (mgc_formats::recover) so the
// app's `--replay` shares one implementation with the harness.
pub(crate) use mgc_formats::recover::{consumed_knock, recover_stick};

/// Wrapped signed distance on a 16-bit axis (x/y are wrapping).
fn wrap16(want: u16, got: u16) -> i64 {
    got.wrapping_sub(want) as i16 as i64
}

/// Wrapped signed distance on the 11-bit angle lanes.
fn wrap11(want: u16, got: u16) -> i64 {
    let d = (got.wrapping_sub(want) & 0x7FF) as i64;
    if d > 1024 { d - 2048 } else { d }
}

impl PoseLane {
    fn gate(&mut self, why: &'static str) {
        *self.gates.entry(why).or_default() += 1;
    }

    fn note(
        &mut self,
        csv: &mut Option<&mut dyn std::io::Write>,
        t: u64,
        ctx: (u16, u8, u8, f64, f64, i16),
        name: &'static str,
        want: i64,
        got: i64,
        delta: i64,
    ) -> std::io::Result<bool> {
        if want == got {
            return Ok(false);
        }
        let lane = self.lanes.entry(name).or_default();
        lane.rows += 1;
        lane.max_abs = lane.max_abs.max(delta.abs());
        lane.first.get_or_insert((t, want, got));
        if let Some(w) = csv.as_mut() {
            let (slot, c, m, x, y, z) = ctx;
            writeln!(
                w,
                "{t}\tpose\t{slot}\t{c}\t{m}\t{name}\t{want}\t{got}\t{x}\t{y}\t{z}\t"
            )?;
        }
        Ok(true)
    }

    /// Step one fixture-grade pair through the shadow mover. `world`
    /// is the imported state@N (terrain installed) and is only read.
    /// `ground_mid` is the reconstructed MID-WALK height plane
    /// (verify.rs: measured endpoints phased per cell by the pair
    /// tick's own snapshot oracle) — retail's carpet probes ground
    /// at its own walk slot, after the tick's lower-slot terraform
    /// and before the higher-slot digs, an image neither record
    /// endpoint holds (t=567 vs t=1210, the two failure families).
    /// When absent, the world's settled planes.
    pub fn run_pair_mc1(
        &mut self,
        world: &World,
        ground_mid: Option<&[u8]>,
        pst: &RetailMc1,
        st: &RetailMc1,
        human_slot: u16,
        t: u64,
        mut csv: Option<&mut dyn std::io::Write>,
    ) -> std::io::Result<()> {
        self.arm = "mc1 mover";
        self.pairs += 1;
        let e0 = &pst.ents[human_slot as usize];
        let e1 = &st.ents[human_slot as usize];
        let (Some(w0), Some(w1)) = (
            pst.wizards.get(pst.local_player as usize),
            st.wizards.get(st.local_player as usize),
        ) else {
            self.gate(GATE_WIZARD);
            return Ok(());
        };
        // Death/respawn ticks: the pose is world-driven (fall
        // integration, castle re-seed), not mover output.
        if matches!(e0.f70, 2 | 3) || matches!(e1.f70, 2 | 3) {
            self.gate(GATE_DEATH);
            return Ok(());
        }
        // Warp ticks: pads/teleport spells move the player outside
        // the mover (max mover reach ≈ 450 units/tick; 8 tiles is
        // far beyond it).
        if wrap16(e0.x, e1.x).abs() > 2048 || wrap16(e0.y, e1.y).abs() > 2048 {
            self.gate(GATE_WARP);
            return Ok(());
        }
        // Accelerate-domain: the importer does not seed
        // `player.speed_boost`, so `accel_override` reads None on the
        // conformance world and the ±160/±240 regime cannot step
        // faithfully yet. The N+1 side also gates: the mover's own
        // integration clamps at ±80, so any bigger recorded landing
        // is the spell ARM tick — spell-domain, not mover-domain.
        if e0.f126.abs() > 80
            || w0.cmd_speed.abs() > 80
            || e1.f126.abs() > 80
            || w1.cmd_speed.abs() > 80
        {
            self.gate(GATE_ACCEL);
            return Ok(());
        }
        let (Some(sx), Some(sy)) = (
            recover_stick(w0.roll_acc as i16, w1.roll_acc as i16),
            recover_stick(w0.pitch_acc as i16, w1.pitch_acc as i16),
        ) else {
            self.gate(GATE_STICK);
            return Ok(());
        };
        // The pair's boolean inputs are the move byte recorded AT N:
        // retail's consume loop stamps dw_0 AFTER the entity pass,
        // for the NEXT tick's mover (measured on mc1l0 — mb@56's
        // strafe bit moves the strafe column across 56→57, mb@59's
        // down bit drops tgt across 59→60). Direct capture, not
        // inference.
        let mb = w0.move_bits;
        let inp = Mc1Input {
            stick_x: sx,
            stick_y: sy,
            speed_up: mb & 1 != 0,
            speed_down: mb & 2 != 0,
            strafe_left: mb & 4 != 0,
            strafe_right: mb & 8 != 0,
        };
        let mut s = Mc1State {
            x: e0.x,
            y: e0.y,
            z: e0.z,
            yaw: e0.f30 & 0x7FF,
            roll_f: w0.roll_acc as i16,
            pitch_f: w0.pitch_acc as i16,
            aim_pitch: e0.f32 & 0x7FF,
            eff_pitch: w0.eff_pitch & 0x7FF,
            act_speed: e0.f126,
            tgt_speed: w0.cmd_speed,
            strafe: w0.strafe,
            tick_ctr: e0.f63,
            rand: e0.rand,
        };
        // move byte 48 exactly: retail skips sub_46840 whole, so a
        // held strafe does NOT decay this tick. The mover cannot skip
        // its decay, so pre-feed one quantum — the decay lands back
        // on the recorded value and the polar step sees the frozen
        // strafe, matching retail's arithmetic exactly.
        if mb == 48 {
            self.dw48 += 1;
            if s.strafe != 0 {
                s.strafe += 4 * s.strafe.signum();
            }
        }
        let knock = consumed_knock(w0.knock_mag, w0.knock_dir, w1.knock_mag, w1.knock_dir);
        let ground = |x: u16, y: u16| match ground_mid {
            Some(h) => World::ground_z_on_plane(h, x, y),
            None => world.ground_z_engine(x, y),
        };
        flight::mc1_move(&mut s, &inp, None, knock, &ground, &|cur, prop| {
            world.player_wall_gate_fixed(cur, prop)
        });
        self.stepped += 1;
        let ctx = (
            human_slot,
            e1.class64,
            e1.model65,
            e1.x as f64 / 256.0,
            e1.y as f64 / 256.0,
            e1.z,
        );
        let mut dirty = false;
        macro_rules! lane {
            ($name:literal, $want:expr, $got:expr, $delta:expr) => {
                dirty |= self.note(&mut csv, t, ctx, $name, $want, $got, $delta)?;
            };
            ($name:literal, $want:expr, $got:expr) => {
                lane!($name, $want, $got, $got - $want);
            };
        }
        lane!("pose.x", e1.x as i64, s.x as i64, wrap16(e1.x, s.x));
        lane!("pose.y", e1.y as i64, s.y as i64, wrap16(e1.y, s.y));
        lane!("pose.z", e1.z as i64, s.z as i64);
        lane!(
            "pose.yaw",
            (e1.f30 & 0x7FF) as i64,
            s.yaw as i64,
            wrap11(e1.f30 & 0x7FF, s.yaw)
        );
        lane!(
            "pose.aim_pitch",
            (e1.f32 & 0x7FF) as i64,
            s.aim_pitch as i64,
            wrap11(e1.f32 & 0x7FF, s.aim_pitch)
        );
        lane!(
            "pose.eff_pitch",
            (w1.eff_pitch & 0x7FF) as i64,
            (s.eff_pitch & 0x7FF) as i64,
            wrap11(w1.eff_pitch & 0x7FF, s.eff_pitch & 0x7FF)
        );
        lane!("pose.act_speed", e1.f126 as i64, s.act_speed as i64);
        lane!("pose.tgt_speed", w1.cmd_speed as i64, s.tgt_speed as i64);
        lane!("pose.strafe", w1.strafe as i64, s.strafe as i64);
        lane!("pose.roll_f", w1.roll_acc as i16 as i64, s.roll_f as i64);
        lane!("pose.pitch_f", w1.pitch_acc as i16 as i64, s.pitch_f as i64);
        lane!("pose.tick_ctr", e1.f63 as i64, s.tick_ctr as i64);
        lane!("pose.rand", e1.rand as i64, s.rand as i64);
        if !dirty {
            self.exact += 1;
        }
        Ok(())
    }

    /// The MC2 twin: same shadow-step shape over [`flight::mc2_move`]
    /// with the world's cave/gate/stuck closures. Phase difference:
    /// MC2 stamps the move byte in PlayerEvents BEFORE the entity
    /// pass, so the pair consumes the byte recorded at N+1 (MC1's
    /// post-pass stamp reads at N). No flutter LCG on the MC2 path;
    /// web-slow/paralyze pairs are gated until the debuff-phase story
    /// is measured.
    pub fn run_pair_mc2(
        &mut self,
        world: &World,
        pst: &RetailMc2,
        st: &RetailMc2,
        human_slot: u16,
        t: u64,
        mut csv: Option<&mut dyn std::io::Write>,
    ) -> std::io::Result<()> {
        self.arm = "mc2 mover";
        self.pairs += 1;
        let e0 = &pst.ents[human_slot as usize];
        let e1 = &st.ents[human_slot as usize];
        let (Some(p0), Some(p1)) = (
            pst.players.get(pst.local_player as usize),
            st.players.get(st.local_player as usize),
        ) else {
            self.gate(GATE_WIZARD);
            return Ok(());
        };
        if e0.action45 != 0 || e1.action45 != 0 {
            self.gate(GATE_DEATH);
            return Ok(());
        }
        if wrap16(e0.x, e1.x).abs() > 2048 || wrap16(e0.y, e1.y).abs() > 2048 {
            self.gate(GATE_WARP);
            return Ok(());
        }
        if e0.speed.abs() > 80
            || p0.cmd_speed.abs() > 80
            || e1.speed.abs() > 80
            || p1.cmd_speed.abs() > 80
        {
            self.gate(GATE_ACCEL);
            return Ok(());
        }
        if p0.move_speed != 0 || p1.move_speed != 0 || p0.mobilize != 0 || p1.mobilize != 0 {
            self.gate(GATE_DEBUFF);
            return Ok(());
        }
        let (Some(sx), Some(sy)) = (
            recover_stick(p0.roll_acc as i16, p1.roll_acc as i16),
            recover_stick(p0.pitch_acc as i16, p1.pitch_acc as i16),
        ) else {
            self.gate(GATE_STICK);
            return Ok(());
        };
        let mb = p1.move_bits;
        let inp = Mc1Input {
            stick_x: sx,
            stick_y: sy,
            speed_up: mb & 1 != 0,
            speed_down: mb & 2 != 0,
            strafe_left: mb & 4 != 0,
            strafe_right: mb & 8 != 0,
        };
        let mut s = Mc1State {
            x: e0.x,
            y: e0.y,
            z: e0.z,
            yaw: e0.yaw as u16 & 0x7FF,
            roll_f: p0.roll_acc as i16,
            pitch_f: p0.pitch_acc as i16,
            aim_pitch: e0.pitch as u16 & 0x7FF,
            eff_pitch: p0.eff_pitch & 0x7FF,
            act_speed: e0.speed,
            tgt_speed: p0.cmd_speed,
            strafe: p0.strafe,
            tick_ctr: 0,
            rand: 0,
        };
        let mut ext = Mc2Ext {
            water_ctr: p0.water_ctr as u16,
            nudge_latch: p0.nudge_latch != 0,
            row: world.mc2_carpet_row(),
            ..Default::default()
        };
        let knock = consumed_knock(p0.knock_mag, p0.knock_dir, p1.knock_mag, p1.knock_dir);
        flight::mc2_move(
            &mut s,
            &mut ext,
            &inp,
            None,
            knock,
            &|x, y| world.ground_z_engine(x, y),
            &|x, y| world.player_cave_ceiling(x, y),
            &|cur, prop| world.player_mc2_gate(cur, prop),
            &|pos, latched| world.player_mc2_stuck(pos, latched),
        );
        self.stepped += 1;
        let ctx = (
            human_slot,
            e1.class3f,
            e1.model40,
            e1.x as f64 / 256.0,
            e1.y as f64 / 256.0,
            e1.z,
        );
        let mut dirty = false;
        macro_rules! lane {
            ($name:literal, $want:expr, $got:expr, $delta:expr) => {
                dirty |= self.note(&mut csv, t, ctx, $name, $want, $got, $delta)?;
            };
            ($name:literal, $want:expr, $got:expr) => {
                lane!($name, $want, $got, $got - $want);
            };
        }
        lane!("pose.x", e1.x as i64, s.x as i64, wrap16(e1.x, s.x));
        lane!("pose.y", e1.y as i64, s.y as i64, wrap16(e1.y, s.y));
        lane!("pose.z", e1.z as i64, s.z as i64);
        lane!(
            "pose.yaw",
            (e1.yaw as u16 & 0x7FF) as i64,
            s.yaw as i64,
            wrap11(e1.yaw as u16 & 0x7FF, s.yaw)
        );
        lane!(
            "pose.aim_pitch",
            (e1.pitch as u16 & 0x7FF) as i64,
            s.aim_pitch as i64,
            wrap11(e1.pitch as u16 & 0x7FF, s.aim_pitch)
        );
        lane!(
            "pose.eff_pitch",
            (p1.eff_pitch & 0x7FF) as i64,
            (s.eff_pitch & 0x7FF) as i64,
            wrap11(p1.eff_pitch & 0x7FF, s.eff_pitch & 0x7FF)
        );
        lane!("pose.act_speed", e1.speed as i64, s.act_speed as i64);
        lane!("pose.tgt_speed", p1.cmd_speed as i64, s.tgt_speed as i64);
        lane!("pose.strafe", p1.strafe as i64, s.strafe as i64);
        lane!("pose.roll_f", p1.roll_acc as i16 as i64, s.roll_f as i64);
        lane!("pose.pitch_f", p1.pitch_acc as i16 as i64, s.pitch_f as i64);
        if !dirty {
            self.exact += 1;
        }
        Ok(())
    }

    /// The report block (empty string when the channel never ran).
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        if self.pairs == 0 {
            return out;
        }
        let pct = |n: u64, d: u64| {
            if d == 0 {
                0.0
            } else {
                n as f64 * 100.0 / d as f64
            }
        };
        let _ = writeln!(
            out,
            "  POSE CHANNEL ({}): {} stepped / {} pairs — {} bit-exact ({:.1}% of stepped)",
            self.arm,
            self.stepped,
            self.pairs,
            self.exact,
            pct(self.exact, self.stepped),
        );
        if !self.gates.is_empty() {
            let gates: Vec<String> = self.gates.iter().map(|(k, v)| format!("{k} {v}")).collect();
            let _ = writeln!(out, "    gated: {}", gates.join(", "));
        }
        if self.dw48 > 0 {
            let _ = writeln!(
                out,
                "    move-byte-48 pairs (sub_46840 skip): {}",
                self.dw48
            );
        }
        for (name, l) in &self.lanes {
            let (t, want, got) = l.first.unwrap_or_default();
            let _ = writeln!(
                out,
                "    {name}: {} rows, max |d| {}, first t={t} want {want} got {got}",
                l.rows, l.max_abs
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every (acc, stick) transition the filter can produce must be
    /// invertible to a stick that reproduces the same landing value.
    #[test]
    fn stick_recovery_inverts_the_filter() {
        for acc in (-600i32..=600).step_by(7) {
            for stick in -128i32..=127 {
                let a = acc as i16;
                let next = a + ((2 * stick - acc) / 4) as i16;
                let rec = recover_stick(a, next)
                    .unwrap_or_else(|| panic!("unrecoverable acc {acc} stick {stick}"));
                let landed = a + ((2 * rec as i32 - acc) / 4) as i16;
                assert_eq!(landed, next, "acc {acc} stick {stick} rec {rec}");
            }
        }
    }

    /// A transition no command-range stick can explain (a respawn
    /// wipe) is refused, not approximated.
    #[test]
    fn impossible_transition_is_refused() {
        assert_eq!(recover_stick(0, 300), None);
        assert_eq!(recover_stick(500, 0), None);
    }

    /// The angle-lane wrap helpers measure the short way around.
    #[test]
    fn wrapped_deltas() {
        assert_eq!(wrap16(0xFFF0, 0x0010), 0x20);
        assert_eq!(wrap11(2040, 8), 16);
        assert_eq!(wrap11(8, 2040), -16);
    }
}
