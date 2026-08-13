//! In-app `.mgcr` playback (`--replay`) and the port recorder
//! (`--record`) — docs/RECORDING.md "Consumers".
//!
//! `--replay` is SOURCE-AGNOSTIC (player-ruled): a retail take plays
//! through inline input recovery (the shared laws in
//! `mgc_formats::recover`, the seeding in mgc-sim's conformance
//! module — one implementation with `mgc-conform replay`), a port
//! take (`source:"port"`, `channels.input:"exact"`) feeds its
//! recorded `FlightInput` stream directly. Both run PURE: the world
//! free-runs on the recorded input, divergence is REPORTED (the HUD
//! counter, the console), never corrected. A gap in a retail take
//! re-anchors a fresh segment from the next closure — a capture
//! artifact, not a resync.
//!
//! The tick cadence maps one recorded pair to one sim tick, so the
//! app's own pacing machinery (F3 game speed, P pause) is the
//! playback transport for free.

use crate::{LoadedLevel, Session};
use mgc_formats::mgcr::{
    self, Family, ObsMc1, PortInput, Recording, RecordingWriter, RetailMc1, RetailMc2,
    TerrainImage, decode_retail_mc1, decode_retail_mc2,
};
use mgc_formats::recover;
use mgc_sim::engine::features::Planes;
use mgc_sim::engine::world::conformance::{self, ThingTable};
use mgc_sim::flight::Mc2Ext;
use mgc_sim::mc1::spells::SpellId;
use mgc_sim::{AltitudeModel, FlightInput, Simulation, ThrustModel};
use serde::Deserialize as _;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySource {
    Retail,
    Port,
}

/// The opened take, header decoded — `game_main` resolves the level
/// path from it, then hands it to the App to become a driver once the
/// session exists.
pub struct ReplayFile {
    rec: Recording,
    pub game: String,
    pub level: u32,
    pub family: Family,
    pub source: ReplaySource,
    /// Port header sim closure: the recorded tier tags ("classic" /
    /// "enhanced") and the embedded start snapshot.
    pub sim_thrust: Option<String>,
    pub sim_altitude: Option<String>,
    /// The recorded retail-bug patch policy tag (see
    /// [`ReplayFile::patch_policy`]); absent on pre-option takes.
    pub sim_patches: Option<String>,
    pub snapshot: Option<Vec<u8>>,
}

impl ReplayFile {
    pub fn open(path: &Path) -> Result<ReplayFile, String> {
        let rec = Recording::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let family = rec.header.family()?;
        let level = rec.header.level.ok_or("recording has no level number")?;
        let source = match rec.header.source.as_str() {
            "retail" => {
                if !rec.header.channels.state {
                    return Err(
                        "retail take has no state channel — input cannot be recovered".into(),
                    );
                }
                ReplaySource::Retail
            }
            "port" => {
                if rec.header.channels.input != "exact" {
                    return Err(format!(
                        "port recording has input channel {:?}, need \"exact\"",
                        rec.header.channels.input
                    ));
                }
                ReplaySource::Port
            }
            other => return Err(format!("unknown recording source {other:?}")),
        };
        let sim = rec.header.sim.as_ref();
        let s = |key: &str| {
            sim.and_then(|s| s.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        };
        if source == ReplaySource::Port {
            // The header's sim closure PINS the run — a snapshot from
            // another format version is a refusal, not a warning
            // (docs/RECORDING.md "Consumers").
            let ver = sim
                .and_then(|s| s.get("snapshot_version"))
                .and_then(|v| v.as_u64());
            if ver != Some(mgc_sim::snapshot::SNAPSHOT_VERSION as u64) {
                return Err(format!(
                    "port recording pins snapshot version {ver:?}, this build reads {}",
                    mgc_sim::snapshot::SNAPSHOT_VERSION
                ));
            }
        }
        let snapshot = sim
            .and_then(|s| s.get("start_mgcs_b64"))
            .and_then(|v| v.as_str())
            .map(mgcr::b64_decode)
            .transpose()?;
        Ok(ReplayFile {
            game: rec.header.game.clone(),
            level,
            family,
            source,
            sim_thrust: s("thrust_model"),
            sim_altitude: s("altitude_model"),
            sim_patches: s("patches"),
            snapshot,
            rec,
        })
    }

    /// The recorded tier tags as sim enums (port takes; a missing
    /// closure defaults faithful).
    pub fn models(&self) -> Result<(ThrustModel, AltitudeModel), String> {
        let thrust = match self.sim_thrust.as_deref() {
            None | Some("classic") => ThrustModel::Mc1,
            Some("enhanced") => ThrustModel::Enhanced,
            Some(other) => return Err(format!("unknown thrust_model {other:?}")),
        };
        let altitude = match self.sim_altitude.as_deref() {
            None | Some("classic") => AltitudeModel::Faithful,
            Some("enhanced") => AltitudeModel::ExtendedLift,
            Some(other) => return Err(format!("unknown altitude_model {other:?}")),
        };
        Ok((thrust, altitude))
    }

    /// The patch policy this take was recorded under (port takes).
    /// `"retail"` (every take since --record forced the retail arms)
    /// -> replay under GameplayPatches::retail_all(); an ABSENT key is
    /// a take from before the patches were options -> the legacy
    /// hard-wired set. Retail-source takes always pin retail arms.
    pub fn patch_policy(&self) -> Result<crate::config::GameplayPatches, String> {
        if self.source == ReplaySource::Retail {
            return Ok(crate::config::GameplayPatches::retail_all());
        }
        match self.sim_patches.as_deref() {
            Some("retail") => Ok(crate::config::GameplayPatches::retail_all()),
            None => Ok(crate::config::GameplayPatches::legacy()),
            Some(other) => Err(format!("unknown patches policy {other:?}")),
        }
    }
}

enum RetailPrev {
    Mc1(Box<RetailMc1>),
    Mc2(Box<RetailMc2>),
}

/// The live playback driver: pulls records, recovers/decodes input,
/// performs retail anchors, grades divergence after each step.
pub struct ReplayDriver {
    rec: Recording,
    source: ReplaySource,
    family: Family,
    // Retail chain state.
    timg: Option<TerrainImage>,
    pristine: Option<Planes>,
    things: Option<ThingTable>,
    witness: recover::Mc2RespawnWitness,
    prev: Option<(u64, RetailPrev)>,
    human_slot: u16,
    /// One pair was fed and awaits its post-step grade (`true` =
    /// capture-clean, gradeable).
    pending: Option<bool>,
    anchored_flag: bool,
    // Tallies.
    segments: u64,
    steps: u64,
    graded: u64,
    clean: u64,
    skipped: u64,
    stick_unrec: u64,
    diverged: Option<(u64, String)>,
    /// The on-screen counter line (④): refreshed every boundary.
    pub hud: String,
    /// The recorded pose for the GHOST billboard: (x, alt, z) tiles,
    /// yaw radians, sprite type index.
    pub ghost: Option<(f32, f32, f32, f32, u16)>,
    pub finished: bool,
}

const GHOST_RAD: f32 = std::f32::consts::TAU / 2048.0;

impl ReplayDriver {
    /// Turn an opened take into the live driver, capturing the
    /// world-side reset state (the session must be installed; for
    /// port takes the snapshot must already be restored).
    pub fn install(file: ReplayFile, sim: &mut Simulation) -> Result<ReplayDriver, String> {
        let (pristine, things) = match file.source {
            ReplaySource::Retail => {
                let w = sim
                    .world
                    .as_ref()
                    .ok_or("replay needs a living world (the level did not build one)")?;
                let things = matches!(file.family, Family::Mc2).then(|| w.thing_table_clone());
                (Some(w.planes_clone()), things)
            }
            ReplaySource::Port => (None, None),
        };
        let timg = file
            .rec
            .header
            .channels
            .terrain
            .as_ref()
            .map(TerrainImage::new);
        Ok(ReplayDriver {
            rec: file.rec,
            source: file.source,
            family: file.family,
            timg,
            pristine,
            things,
            witness: recover::Mc2RespawnWitness::default(),
            prev: None,
            human_slot: 0,
            pending: None,
            anchored_flag: false,
            segments: 0,
            steps: 0,
            graded: 0,
            clean: 0,
            skipped: 0,
            stick_unrec: 0,
            diverged: None,
            hud: String::from("REPLAY starting"),
            ghost: None,
            finished: false,
        })
    }

    /// The next tick's input, driving anchors/segments along the way.
    /// `None` = the take ended (or a fatal record error) — the caller
    /// hands control back to the player.
    pub fn next(&mut self, sim: &mut Simulation) -> Option<FlightInput> {
        match self.source {
            ReplaySource::Port => self.next_port(sim),
            ReplaySource::Retail => self.next_retail(sim),
        }
    }

    /// The world was re-imported since the last call (the caller
    /// clears its render mirrors — stale (slot, generation) pose
    /// pairs must not survive an import).
    pub fn take_anchored(&mut self) -> bool {
        std::mem::take(&mut self.anchored_flag)
    }

    fn finish_stream(&mut self) {
        self.finished = true;
        println!("replay: {}", self.summary());
    }

    pub fn summary(&self) -> String {
        let base = format!(
            "{} tick(s) in {} segment(s), {} graded ({} capture-skipped), {} clean",
            self.steps, self.segments, self.graded, self.skipped, self.clean
        );
        match &self.diverged {
            Some((t, lane)) => format!("{base}; DIVERGED since t={t} ({lane})"),
            None => format!("{base}; bit-exact throughout"),
        }
    }

    fn refresh_hud(&mut self, t: u64) {
        self.hud = match (&self.diverged, self.source) {
            (Some((dt, lane)), _) => format!("REPLAY t={t} — diverged since t={dt} ({lane})"),
            (None, ReplaySource::Retail) => {
                format!("REPLAY t={t} — pose bit-exact ({} graded)", self.graded)
            }
            (None, ReplaySource::Port) => {
                format!("REPLAY t={t} — hash-exact ({} graded)", self.graded)
            }
        };
    }

    // ---------------------------------------------------------- port

    fn next_port(&mut self, sim: &mut Simulation) -> Option<FlightInput> {
        let tick = match self.rec.next_tick() {
            None => {
                self.finish_stream();
                return None;
            }
            Some(Err(e)) => {
                eprintln!("replay: record error: {e}");
                self.finish_stream();
                return None;
            }
            Some(Ok(t)) => t,
        };
        // The hash channel describes tick t's PRE-input state — the
        // sim sits exactly there right now (the phase convention).
        if let Some(want) = tick.hash {
            self.graded += 1;
            let got = sim.state_hash();
            if want == got {
                self.clean += 1;
            } else if self.diverged.is_none() {
                self.diverged = Some((tick.t, "hash".into()));
                println!(
                    "replay: hash desync at t={} — recorded {want:016x}, live {got:016x}",
                    tick.t
                );
            }
        }
        let input = match &tick.input {
            Some(v) => match PortInput::deserialize(v) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("replay: t={}: bad input record: {e}", tick.t);
                    self.finish_stream();
                    return None;
                }
            },
            None => PortInput::default(),
        };
        self.steps += 1;
        if self.segments == 0 {
            self.segments = 1;
        }
        self.refresh_hud(tick.t);
        Some(flight_input_from(&input))
    }

    // -------------------------------------------------------- retail

    fn next_retail(&mut self, sim: &mut Simulation) -> Option<FlightInput> {
        loop {
            let tick = match self.rec.next_tick() {
                None => {
                    self.finish_stream();
                    return None;
                }
                Some(Err(e)) => {
                    eprintln!("replay: record error: {e}");
                    self.finish_stream();
                    return None;
                }
                Some(Ok(t)) => t,
            };
            // The terrain image tracks the take continuously
            // (self-healing deltas) — installed only at anchors.
            if let (Some(img), Some(block)) = (self.timg.as_mut(), &tick.terrain) {
                if let Err(e) = img.apply(block) {
                    eprintln!("replay: t={}: terrain: {e}", tick.t);
                }
            }
            let Some(state) = &tick.state else {
                self.prev = None;
                continue;
            };
            let out = match self.family {
                Family::Mc1 => match decode_retail_mc1(state) {
                    Ok(st) => self.retail_tick_mc1(sim, &tick, st),
                    Err(e) => {
                        eprintln!("replay: t={}: {e}", tick.t);
                        self.finish_stream();
                        return None;
                    }
                },
                Family::Mc2 => match decode_retail_mc2(state) {
                    Ok(st) => self.retail_tick_mc2(sim, &tick, st),
                    Err(e) => {
                        eprintln!("replay: t={}: {e}", tick.t);
                        self.finish_stream();
                        return None;
                    }
                },
            };
            match out {
                Ok(Some(input)) => return Some(input),
                Ok(None) => continue, // anchored — pull the next record
                Err(e) => {
                    eprintln!("replay: t={}: {e}", tick.t);
                    self.finish_stream();
                    return None;
                }
            }
        }
    }

    fn retail_tick_mc1(
        &mut self,
        sim: &mut Simulation,
        tick: &mgcr::TickRecord,
        st: RetailMc1,
    ) -> Result<Option<FlightInput>, String> {
        let anchor = !matches!(&self.prev, Some((pt, RetailPrev::Mc1(_))) if tick.t == pt + 1);
        if anchor {
            let w = sim.world.as_mut().ok_or("no world")?;
            w.restore_planes(self.pristine.as_ref().expect("retail install"));
            let report = w
                .retail_import_mc1(&st)
                .map_err(|e| format!("import: {e}"))?;
            // Measured planes AFTER the import (the importer's
            // terrain replay double-applies on measured planes — the
            // replay verifier's proven order).
            if let Some((h, ty, ceil, an)) = self.timg.as_ref().and_then(|i| i.measured()) {
                w.install_measured_terrain(h, ty, ceil, an)
                    .map_err(|e| format!("terrain: {e}"))?;
            }
            let (fl, fr) = recover::mc1_fire(st.wizards[st.local_player as usize].move_bits);
            w.set_prev_fire(fl, fr);
            w.terrain_dirty = true;
            w.entities_dirty = true;
            self.human_slot = report.human_slot;
            sim.carpet = conformance::mc1_state_from_retail(&st, report.human_slot);
            sim.carpet_mc2 = Mc2Ext::default();
            sim.clear_accel_edge();
            sim.sync_flyer_from_carpet();
            self.segments += 1;
            self.anchored_flag = true;
            self.update_ghost_mc1(&st);
            self.prev = Some((tick.t, RetailPrev::Mc1(Box::new(st))));
            return Ok(None);
        }
        let (_, prev) = self.prev.take().expect("anchored");
        let RetailPrev::Mc1(pst) = prev else {
            unreachable!("family-stable stream")
        };
        let rp = recover::recover_pair_mc1(&pst, &st, tick.input.as_ref());
        if !rp.stick_ok() {
            self.stick_unrec += 1;
        }
        // The dw==48 strafe-freeze emulation (law in RecoveredPair).
        if rp.mc1_strafe_freeze() && sim.carpet.strafe != 0 {
            sim.carpet.strafe += 4 * sim.carpet.strafe.signum();
        }
        let input = FlightInput {
            stick_x: rp.stick().0,
            stick_y: rp.stick().1,
            fire_left: rp.fire_left,
            fire_right: rp.fire_right,
            equip_left: rp.equip_left.map(SpellId),
            equip_right: rp.equip_right.map(SpellId),
            respawn: rp.respawn,
            demolish: rp.demolish,
            mc1_move_byte: Some(rp.move_byte as u8),
            ..FlightInput::default()
        };
        // Gradeability decided from the recording alone; the pose
        // compare itself waits for the step (`grade`).
        let gradeable = tick
            .obs
            .as_ref()
            .and_then(|v| ObsMc1::deserialize(v).ok())
            .is_some_and(|obs| recover::capture_clean_mc1(&pst, &obs));
        self.pending = Some(gradeable);
        self.steps += 1;
        self.update_ghost_mc1(&st);
        self.prev = Some((tick.t, RetailPrev::Mc1(Box::new(st))));
        Ok(Some(input))
    }

    fn retail_tick_mc2(
        &mut self,
        sim: &mut Simulation,
        tick: &mgcr::TickRecord,
        st: RetailMc2,
    ) -> Result<Option<FlightInput>, String> {
        // The respawn witness folds EVERY state-bearing record in
        // stream order (dating law in mgc_formats::recover).
        let respawn = self.witness.observe(tick.input.as_ref());
        let anchor = !matches!(&self.prev, Some((pt, RetailPrev::Mc2(_))) if tick.t == pt + 1);
        if anchor {
            let w = sim.world.as_mut().ok_or("no world")?;
            w.restore_planes(self.pristine.as_ref().expect("retail install"));
            w.restore_thing_table(self.things.as_ref().expect("mc2 install"));
            let report = w
                .retail_import_mc2(&st)
                .map_err(|e| format!("import: {e}"))?;
            if let Some((h, ty, ceil, an)) = self.timg.as_ref().and_then(|i| i.measured()) {
                w.install_measured_terrain(h, ty, ceil, an)
                    .map_err(|e| format!("terrain: {e}"))?;
            }
            let (fl, fr) = recover::mc1_fire(st.players[st.local_player as usize].move_bits);
            w.set_prev_fire(fl, fr);
            w.terrain_dirty = true;
            w.entities_dirty = true;
            let row = w.mc2_carpet_row();
            self.human_slot = report.human_slot;
            let (s, ext) = conformance::mc2_state_from_retail(&st, report.human_slot, row);
            sim.carpet = s;
            sim.carpet_mc2 = ext;
            sim.clear_accel_edge();
            sim.sync_flyer_from_carpet();
            self.segments += 1;
            self.anchored_flag = true;
            self.update_ghost_mc2(&st);
            self.prev = Some((tick.t, RetailPrev::Mc2(Box::new(st))));
            return Ok(None);
        }
        let (_, prev) = self.prev.take().expect("anchored");
        let RetailPrev::Mc2(pst) = prev else {
            unreachable!("family-stable stream")
        };
        let rp = recover::recover_pair_mc2(&pst, &st, respawn, tick.input.as_ref());
        if !rp.stick_ok() {
            self.stick_unrec += 1;
        }
        let input = FlightInput {
            stick_x: rp.stick().0,
            stick_y: rp.stick().1,
            fire_left: rp.fire_left,
            fire_right: rp.fire_right,
            mc2_select: rp.mc2_select,
            respawn: rp.respawn,
            demolish: rp.demolish,
            // Retail's dw_0 bit 0x80 IS the barrel-roll command.
            barrel_roll: rp.move_byte & 0x80 != 0,
            mc1_move_byte: Some(rp.move_byte as u8),
            ..FlightInput::default()
        };
        let gradeable = recover::capture_clean_mc2(&pst, &st);
        self.pending = Some(gradeable);
        self.steps += 1;
        self.update_ghost_mc2(&st);
        self.prev = Some((tick.t, RetailPrev::Mc2(Box::new(st))));
        Ok(Some(input))
    }

    /// Post-step boundary grade (retail takes): the chained carpet vs
    /// the recorded pose, the pose channel's lane set. Reported, never
    /// corrected.
    pub fn grade(&mut self, sim: &Simulation) {
        let Some(gradeable) = self.pending.take() else {
            return;
        };
        let Some((t, prev)) = &self.prev else { return };
        let t = *t;
        if !gradeable {
            self.skipped += 1;
            self.refresh_hud(t);
            return;
        }
        let lanes = match prev {
            RetailPrev::Mc1(st) => conformance::pose_lanes_mc1(
                &sim.carpet,
                &st.ents[self.human_slot as usize],
                &st.wizards[st.local_player as usize],
            ),
            RetailPrev::Mc2(st) => conformance::pose_lanes_mc2(
                &sim.carpet,
                &st.ents[self.human_slot as usize],
                &st.players[st.local_player as usize],
            ),
        };
        self.graded += 1;
        if lanes.is_empty() {
            self.clean += 1;
        } else if self.diverged.is_none() {
            let (lane, want, got) = lanes[0];
            self.diverged = Some((t, lane.to_string()));
            println!("replay: first pose divergence t={t} — {lane}: retail {want} port {got}");
        }
        self.refresh_hud(t);
    }

    fn update_ghost_mc1(&mut self, st: &RetailMc1) {
        let e = &st.ents[usize::from(self.human_slot)];
        self.ghost = Some((
            e.x as f32 / 256.0,
            e.z as f32 / 256.0,
            e.y as f32 / 256.0,
            (e.f30 & 0x7FF) as f32 * GHOST_RAD,
            e.type86,
        ));
    }

    fn update_ghost_mc2(&mut self, st: &RetailMc2) {
        let e = &st.ents[usize::from(self.human_slot)];
        self.ghost = Some((
            e.x as f32 / 256.0,
            e.z as f32 / 256.0,
            e.y as f32 / 256.0,
            (e.yaw as u16 & 0x7FF) as f32 * GHOST_RAD,
            // The MC2 wizard-carpet art family, human color row.
            272 + mgc_sim::mc2::color_art(0) as u16,
        ));
    }
}

// ------------------------------------------------------------ recorder

/// The port recorder (`--record`): `source:"port"`, `input:"exact"`,
/// hash channel on, the start state embedded as `start_mgcs_b64` (so
/// mid-level/campaign starts replay exactly — a pristine level boot
/// is just the t=0 special case of the same law).
pub struct PortRecorder {
    w: RecordingWriter,
    path: PathBuf,
}

impl PortRecorder {
    pub fn begin(
        path: &Path,
        sim: &Simulation,
        game: &str,
        level: u32,
        thrust: ThrustModel,
        altitude: AltitudeModel,
    ) -> Result<PortRecorder, String> {
        let header = serde_json::json!({
            "format": 1,
            "game": game,
            "level": level,
            "source": "port",
            "tick_hz": mgc_sim::TICK_RATE_HZ,
            "channels": {"input": "exact", "obs": false, "state": false, "hash": true},
            "build": env!("CARGO_PKG_VERSION"),
            "tool": {"name": "mgcarpet --record", "version": env!("CARGO_PKG_VERSION")},
            "sim": {
                "snapshot_version": mgc_sim::snapshot::SNAPSHOT_VERSION,
                "thrust_model": match thrust {
                    ThrustModel::Mc1 => "classic",
                    ThrustModel::Enhanced => "enhanced",
                },
                "altitude_model": match altitude {
                    AltitudeModel::Faithful => "classic",
                    AltitudeModel::ExtendedLift => "enhanced",
                },
                // The retail-bug patch policy: --record forces every
                // patch to its retail arm for the whole session, and
                // says so here. A header WITHOUT this key is a take
                // from before the patches became options — it replays
                // under the legacy hard-wired set (ReplayFile::
                // patch_policy).
                "patches": "retail",
                "start_mgcs_b64": mgcr::b64_encode(&sim.snapshot()),
            },
        });
        Ok(PortRecorder {
            w: RecordingWriter::create(path, &header)?,
            path: path.to_owned(),
        })
    }

    /// One tick: `t`/`hash` describe the PRE-step state, `input` is
    /// what the step consumed (the phase convention).
    pub fn record(&mut self, t: u64, input: &FlightInput, hash: u64) -> Result<(), String> {
        self.w.write_record(&serde_json::json!({
            "t": t,
            "input": port_input_from(input),
            "hash": format!("{hash:016x}"),
        }))
    }

    pub fn finish(self) -> (PathBuf, u64, Result<(), String>) {
        let n = self.w.records();
        (self.path, n, self.w.finish())
    }
}

fn port_input_from(i: &FlightInput) -> PortInput {
    PortInput {
        thrust: i.thrust,
        strafe: i.strafe,
        lift: i.lift,
        yaw_delta: i.yaw_delta,
        pitch_delta: i.pitch_delta,
        stick_x: i.stick_x,
        stick_y: i.stick_y,
        fire_left: i.fire_left,
        fire_right: i.fire_right,
        equip_left: i.equip_left.map(|s| s.0),
        equip_right: i.equip_right.map(|s| s.0),
        mc2_select: i.mc2_select,
        spell_ring: i.spell_ring,
        full_stop: i.full_stop,
        respawn: i.respawn,
        demolish: i.demolish,
        barrel_roll: i.barrel_roll,
        raw_dx: i.raw_dx,
        mc1_move_byte: i.mc1_move_byte,
    }
}

fn flight_input_from(p: &PortInput) -> FlightInput {
    FlightInput {
        thrust: p.thrust,
        strafe: p.strafe,
        lift: p.lift,
        yaw_delta: p.yaw_delta,
        pitch_delta: p.pitch_delta,
        stick_x: p.stick_x,
        stick_y: p.stick_y,
        fire_left: p.fire_left,
        fire_right: p.fire_right,
        equip_left: p.equip_left.map(SpellId),
        equip_right: p.equip_right.map(SpellId),
        mc2_select: p.mc2_select,
        spell_ring: p.spell_ring,
        full_stop: p.full_stop,
        respawn: p.respawn,
        demolish: p.demolish,
        barrel_roll: p.barrel_roll,
        raw_dx: p.raw_dx,
        mc1_move_byte: p.mc1_move_byte,
    }
}

// -------------------------------------------------------- replay-check

/// Headless verification playback (`--replay-check`): the whole take
/// through the app's own `Simulation::step` path, no window — the
/// certification instrument for the in-app replay chain (its retail
/// results must match `mgc-conform replay`'s, which certifies the
/// integer-pose faithful path end to end). Exit truth: `Ok(true)` =
/// zero divergence.
pub fn replay_check(mut level: LoadedLevel, mut file: ReplayFile) -> Result<bool, String> {
    let w = level.world.take().ok_or("level built no world")?;
    let mut sim = Simulation::with_world(w);
    let (thrust, altitude) = file.models()?;
    sim.thrust_model = thrust;
    sim.altitude_model = altitude;
    if let Some(start) = level.start {
        sim.flyer = start;
        sim.sync_carpet_from_flyer();
    }
    if let Some(snap) = file.snapshot.take() {
        sim.restore(&snap).map_err(|e| format!("snapshot: {e}"))?;
    }
    let mut d = ReplayDriver::install(file, &mut sim)?;
    while let Some(input) = d.next(&mut sim) {
        sim.step(&input);
        d.grade(&sim);
    }
    println!("replay-check: {}", d.summary());
    Ok(d.diverged.is_none())
}

/// The ghost's translucent billboard, if the session can resolve the
/// sprite (see `entities::ghost_billboard`).
pub fn ghost_billboard(driver: &ReplayDriver, sess: &Session) -> Option<mgc_render::Billboard> {
    let (x, alt, z, yaw, type_index) = driver.ghost?;
    let index = sess.level.sprites.as_ref().map(|(i, _)| i);
    let dims = |id: u16| {
        index
            .and_then(|i| i.sprites.get(id as usize))
            .map(|s| (s.width, s.height, s.flags))
    };
    crate::entities::ghost_billboard(sess.level.game, type_index, x, alt, z, yaw, &dims)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The port loop, closed: record a world-less faithful session,
    /// reopen the file, replay it onto a fresh sim — every tick must
    /// hash-verify and the end states must agree bit for bit (the
    /// `verify-replay` contract, docs/RECORDING.md "Consumers").
    #[test]
    fn port_record_replay_roundtrip() {
        let path = std::env::temp_dir().join("mgcapp-port-roundtrip.mgcr");
        let mut a = Simulation::with_terrain(Vec::new());
        a.thrust_model = ThrustModel::Mc1;
        a.altitude_model = AltitudeModel::Faithful;
        let mut rec =
            PortRecorder::begin(&path, &a, "mc1", 0, a.thrust_model, a.altitude_model).unwrap();
        for t in 0..200u64 {
            let input = FlightInput {
                thrust: if t % 50 < 25 { 1.0 } else { 0.0 },
                stick_x: (t % 40) as i16 - 20,
                stick_y: 5,
                fire_left: t % 7 == 0,
                ..FlightInput::default()
            };
            rec.record(a.tick, &input, a.state_hash()).unwrap();
            a.step(&input);
        }
        let (_, n, res) = rec.finish();
        res.unwrap();
        assert_eq!(n, 200);

        let mut file = ReplayFile::open(&path).unwrap();
        assert_eq!(file.source, ReplaySource::Port);
        let (thrust, altitude) = file.models().unwrap();
        let mut b = Simulation::with_terrain(Vec::new());
        b.thrust_model = thrust;
        b.altitude_model = altitude;
        b.restore(&file.snapshot.take().unwrap()).unwrap();
        let mut d = ReplayDriver::install(file, &mut b).unwrap();
        let mut steps = 0;
        while let Some(input) = d.next(&mut b) {
            b.step(&input);
            d.grade(&b);
            steps += 1;
        }
        assert_eq!(steps, 200);
        assert!(d.diverged.is_none(), "{}", d.summary());
        assert_eq!(d.graded, 200);
        assert_eq!(a.state_hash(), b.state_hash());
        let _ = std::fs::remove_file(&path);
    }
}
