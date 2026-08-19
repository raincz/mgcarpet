//! THE RAW SHADOW — the ungraded-lane detector (`MGC_RAW_SHADOW=1`).
//!
//! `EntObsMc1` carries 22 fields; `RetailEntMc1` carries 50-odd. Every
//! field in the gap is a lane the recording HOLDS, the importer
//! RESTORES and the graded diff can never see, so a handler that reads
//! one correctly and writes it wrong reports CLEAN in pair mode
//! forever. The lane started as `+70`/`+71`/`+58`/`+44` and each of the
//! four paid for itself (the frozen `+58` combat gate, the kraken's
//! `+71` burst counter, the crater's inherited `+44`); this module is
//! that comparison widened to every field the port models, and pointed
//! at BOTH runners:
//!
//! - **pair mode** (`verify-deltas`) imports retail state every tick,
//!   so a mismatch is a one-tick WRITE bug, attributable to the
//!   handler that ran;
//! - **the free run** (`replay`) carries its own copy for thousands of
//!   ticks, so a mismatch is where the port's own history first parts
//!   from retail's — which is the only instrument that can explain a
//!   segmented-replay break whose pair diff `D1(t)` is CLEAN.
//!
//! Run both: pair mode names the guilty handler, the free run names the
//! tick that mattered.

use mgc_formats::mgcr::RetailMc1;
use mgc_sim::engine::world::World;
use mgc_sim::engine::world::conformance::norm_retail_ai_state_mc1;
use std::collections::BTreeMap;
use std::io::Write as _;

/// The six damage mailboxes as lane names — `{amount, source}` per
/// channel (ch0 physical, ch1 mana-ball claim, ch3 mana steal, ch4
/// grip, ch5 balloon recall).
const MAIL_LANES: [(&str, &str); 6] = [
    ("mail0.amt", "mail0.src"),
    ("mail1.amt", "mail1.src"),
    ("mail2.amt", "mail2.src"),
    ("mail3.amt", "mail3.src"),
    ("mail4.amt", "mail4.src"),
    ("mail5.amt", "mail5.src"),
];

/// One lane's running verdict: how many rows, the FIRST one, the
/// tick span and the DISTINCT slot census ("45 rows across 1 slot"
/// and "45 rows across 40 slots" are completely different leads).
#[derive(Default, Clone)]
pub(crate) struct Lane {
    pub(crate) rows: u64,
    pub(crate) first_t: u64,
    pub(crate) last_t: u64,
    pub(crate) slots: std::collections::BTreeSet<u16>,
    pub(crate) example: String,
}

/// The tally across a run. Keyed `(class, model, field)` because one
/// key is one story — a lane that is wrong on `(10,23)` and right on
/// `(10,25)` is a different bug from one that is wrong on both.
#[derive(Default)]
pub(crate) struct Shadow {
    pub(crate) lanes: BTreeMap<(u8, u8, &'static str), Lane>,
    /// The WIZEXT half (`World::wiz_shadow_mc1`), keyed `(wiz, field)`
    /// — the Lane's slot census holds the ARRAY INDEX there (0 for
    /// scalars).
    pub(crate) wiz_lanes: BTreeMap<(u8, &'static str), Lane>,
    /// Free-stack verdict: mismatched pairs / compared pairs / first
    /// example.
    pub(crate) free: (u64, u64, String),
    /// Optional per-row TSV (`MGC_RAW_SHADOW_ROWS=<path>`). Wizard
    /// rows ride the same file with class 255, model = wiz, slot =
    /// array index.
    rows: Option<std::io::BufWriter<std::fs::File>>,
    /// `MGC_RAW_SHADOW_LANE=<class>,<model>,<field>` — the lane
    /// magnifier: every row of exactly that lane prints to stdout as
    /// it lands, so the census line's one `e.g.` widens to the whole
    /// story without a TSV round-trip.
    watch: Option<(u8, u8, String)>,
    /// `MGC_WIZ_SHADOW_LANE=<wiz>,<field>` — the wizard-lane twin of
    /// the magnifier.
    wiz_watch: Option<(u8, String)>,
}

impl Shadow {
    /// Build the tally if `MGC_RAW_SHADOW` is set, opening the row TSV
    /// if `MGC_RAW_SHADOW_ROWS` names one. `None` = the instrument is
    /// off and costs nothing.
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        if std::env::var_os("MGC_RAW_SHADOW").is_none() {
            return Ok(None);
        }
        let rows = match std::env::var_os("MGC_RAW_SHADOW_ROWS") {
            Some(p) => {
                let mut w = std::io::BufWriter::new(
                    std::fs::File::create(&p).map_err(|e| format!("shadow rows: {e}"))?,
                );
                writeln!(w, "t\tslot\tclass\tmodel\tfield\tretail\tport")
                    .map_err(|e| format!("shadow rows: {e}"))?;
                Some(w)
            }
            None => None,
        };
        let watch = match std::env::var("MGC_RAW_SHADOW_LANE") {
            Ok(v) => {
                let mut it = v.splitn(3, ',');
                match (it.next(), it.next(), it.next()) {
                    (Some(c), Some(m), Some(f)) => {
                        let c = c
                            .trim()
                            .parse()
                            .map_err(|_| format!("MGC_RAW_SHADOW_LANE: bad class in {v:?}"))?;
                        let m = m
                            .trim()
                            .parse()
                            .map_err(|_| format!("MGC_RAW_SHADOW_LANE: bad model in {v:?}"))?;
                        Some((c, m, f.trim().to_string()))
                    }
                    _ => {
                        return Err(format!(
                            "MGC_RAW_SHADOW_LANE={v:?}: want <class>,<model>,<field>"
                        ));
                    }
                }
            }
            Err(_) => None,
        };
        let wiz_watch = match std::env::var("MGC_WIZ_SHADOW_LANE") {
            Ok(v) => {
                let mut it = v.splitn(2, ',');
                match (it.next(), it.next()) {
                    (Some(w), Some(f)) => {
                        let w = w
                            .trim()
                            .parse()
                            .map_err(|_| format!("MGC_WIZ_SHADOW_LANE: bad wiz in {v:?}"))?;
                        Some((w, f.trim().to_string()))
                    }
                    _ => return Err(format!("MGC_WIZ_SHADOW_LANE={v:?}: want <wiz>,<field>")),
                }
            }
            Err(_) => None,
        };
        Ok(Some(Shadow {
            rows,
            watch,
            wiz_watch,
            ..Default::default()
        }))
    }

    fn hit(&mut self, key: (u8, u8, &'static str), t: u64, slot: u16, a: i64, b: i64) {
        let lane = self.lanes.entry(key).or_default();
        lane.rows += 1;
        if lane.example.is_empty() {
            lane.first_t = t;
            lane.example = format!("t={t} slot {slot}: retail {a} port {b}");
        }
        lane.last_t = t;
        lane.slots.insert(slot);
        if let Some(w) = self.rows.as_mut() {
            let _ = writeln!(w, "{t}\t{slot}\t{}\t{}\t{}\t{a}\t{b}", key.0, key.1, key.2);
        }
        if let Some((wc, wm, wf)) = self.watch.as_ref()
            && *wc == key.0
            && *wm == key.1
            && wf == key.2
        {
            println!(
                "  LANE ({},{}) {} t={t} slot {slot}: retail {a} port {b}",
                key.0, key.1, key.2
            );
        }
    }

    /// The wizard-lane twin of [`Self::hit`]; `idx` is the array index
    /// (0 for scalars).
    fn wiz_hit(&mut self, key: (u8, &'static str), t: u64, idx: u16, a: i64, b: i64) {
        let lane = self.wiz_lanes.entry(key).or_default();
        lane.rows += 1;
        if lane.example.is_empty() {
            lane.first_t = t;
            lane.example = format!("t={t} wiz {} [{idx}]: retail {a} port {b}", key.0);
        }
        lane.last_t = t;
        lane.slots.insert(idx);
        if let Some(w) = self.rows.as_mut() {
            let _ = writeln!(w, "{t}\t{idx}\t255\t{}\t{}\t{a}\t{b}", key.0, key.1);
        }
        if let Some((ww, wf)) = self.wiz_watch.as_ref()
            && *ww == key.0
            && wf == key.1
        {
            println!(
                "  WIZ LANE {} {} t={t} [{idx}]: retail {a} port {b}",
                key.0, key.1
            );
        }
    }

    /// Diff every ungraded per-entity lane of `world` against the
    /// recorded state `st`, at tick `t`.
    ///
    /// Only slots the recording agrees are the SAME entity are
    /// compared — a slot desync would otherwise report every field on
    /// every shifted slot and drown the lane it is meant to expose.
    pub(crate) fn compare_ents_mc1(
        &mut self,
        world: &World,
        st: &RetailMc1,
        human_slot: u16,
        t: u64,
    ) {
        for g in world.raw_shadow_mc1() {
            if g.slot == human_slot {
                continue;
            }
            let Some(w) = st.ents.get(g.slot as usize) else {
                continue;
            };
            if w.class64 != g.class || w.model65 != g.model {
                continue;
            }
            // Lanes that hold a POOL SLOT need the obs projection's own
            // untranslation: the port carries the human as
            // `PLAYER_TARGET` (0xFFFF) because its carpet is not a pool
            // record, where the recording carries the real slot.
            // Without it every latch, claim and mail source naming the
            // human reads as a mismatch — 400k rows of it on mc1l2, all
            // of them the sentinel.
            let untr = |v: i64| {
                if v == u16::MAX as i64 {
                    human_slot as i64
                } else {
                    v
                }
            };
            let mut hits: Vec<(&'static str, i64, i64)> = vec![
                ("f70", w.f70 as i64, g.f70 as i64),
                ("f71", w.f71 as i64, g.f71 as i64),
                // ⚠ COMPARE THE BYTE, NOT THE NUMBER. The recording's
                // `+58` is signed and the port widens it to i16, but
                // the port's own decrements wrap as u8 — retail's -6
                // and the port's 250 are the SAME byte and only the
                // sign interpretation differs. Masking keeps this lane
                // honest; the sign question is its own lead.
                ("f58", w.f58 as i64 & 0xFF, g.f58 as i64 & 0xFF),
                ("f44", w.f44 as i64, g.f44 as i64),
                ("f26", w.f26 as i64, g.f26 as i64),
                ("f28", w.f28 as i64, g.f28 as i64),
                ("f36", w.f36 as i64, g.f36 as i64),
                ("f38", w.f38 as i64, untr(g.f38 as i64)),
                ("f40", w.f40 as i64, untr(g.f40 as i64)),
                ("f46", w.f46 as i64, g.f46 as i64),
                ("f50", w.f50 as i64, g.f50 as i64),
                ("f52", w.f52 as i64, g.f52 as i64),
                ("f54", w.f54 as i64, g.f54 as i64),
                ("f56", w.f56 as i64, g.f56 as i64),
                ("f59", w.f59 as i64, g.f59 as i64),
                ("f68", w.f68 as i64, g.f68 as i64),
                ("f69", w.f69 as i64, g.f69 as i64),
                ("f78", w.f78 as i64, g.f78 as i64),
                ("f80", w.f80 as i64, g.f80 as i64),
                ("f82", w.f82 as i64, g.f82 as i64),
                ("f84", w.f84 as i64, g.f84 as i64),
                ("type86", w.type86 as i64, g.type86 as i64),
                ("frame88", w.frame88 as i64, g.frame88 as i64),
                ("frames89", w.frames89 as i64, g.frames89 as i64),
                ("f128", w.f128 as i64, g.f128 as i64),
                ("f130", w.f130 as i64, g.f130 as i64),
                ("f144", w.f144 as i64, untr(g.f144 as i64)),
                ("dest_x", w.dest_x as i64, g.dest_x as i64),
                ("dest_y", w.dest_y as i64, g.dest_y as i64),
                ("site_z", w.site_z as i64, g.site_z as i64),
            ];
            for (k, (amt, src)) in MAIL_LANES.iter().enumerate() {
                hits.push((amt, w.mail[k].0 as i64, g.mail[k].0 as i64));
                hits.push((src, w.mail[k].1 as i64, untr(g.mail[k].1 as i64)));
            }
            // The TILE LINKS are structural, not a lane: the port's
            // carpet lives outside the pool, so a chain that threads
            // THROUGH the human can never agree link-for-link. Skip
            // exactly those rows and keep the rest — chain ORDER is
            // where every membership law this campaign has found landed
            // first.
            for (name, a, b) in [
                ("next20", w.next20 as i64, g.next20 as i64),
                ("prev22", w.prev22 as i64, g.prev22 as i64),
            ] {
                if a != human_slot as i64 {
                    hits.push((name, a, b));
                }
            }
            for (name, a, b) in hits {
                if a != b {
                    self.hit((g.class, g.model, name), t, g.slot, a, b);
                }
            }
        }
    }

    /// Diff every ungraded WIZEXT/brain lane against the recording's
    /// wizard slice — the Type_160 half of the shadow. Pair mode names
    /// the handler that wrote a register wrong THIS tick; the free run
    /// names the first tick the port's carried wizard state parts from
    /// retail's, which is the only instrument that can explain a
    /// carpet-motion break whose pair diff is CLEAN (the mc1l4 t=5378
    /// family this was built for).
    pub(crate) fn compare_wiz_mc1(&mut self, world: &World, st: &RetailMc1, t: u64) {
        for ws in world.wiz_shadow_mc1() {
            let Some(w) = st.wizards.get(ws.wiz as usize) else {
                continue;
            };
            // Eliminated on either side, or a carpet-slot desync: the
            // roster/graded comparison owns those stories.
            if w.play_index == 0 || (ws.wiz != 0 && w.play_index != ws.ent) {
                continue;
            }
            let ent = st.ents.get(w.play_index as usize);
            for &(name, port) in &ws.scalars {
                let retail: i64 = match name {
                    "charge" => w.charge as i64,
                    "knock_dir" => w.knock_dir as i64,
                    "knock_mag" => w.knock_mag as i64,
                    "danger" => w.danger as i64,
                    "aggro" => w.aggro as i64,
                    "banked_houses" => w.banked_houses as i64,
                    "castle_alert" => w.castle_alert as i64,
                    "player_alert" => w.player_alert as i64,
                    "balloon_alert" => w.balloon_alert as i64,
                    "kills" => w.kills as i64,
                    "shots" => w.shots as i64,
                    "hits" => w.hits as i64,
                    "cmd_speed" => w.cmd_speed as i64,
                    "strafe" => w.strafe as i64,
                    "grace" => w.grace as i64,
                    "regen_stall" => w.regen_stall as i64,
                    "life_rate" => w.life_rate as i64,
                    "ai_state" => norm_retail_ai_state_mc1(w.ai_state),
                    "burst" => w.burst as i64,
                    "poverty" => (w.poverty != 0) as i64,
                    // The port's human-target sentinel is not retail's
                    // computed sig; those rows have no retail twin.
                    "target_sig" if port == u16::MAX as i64 => continue,
                    "target_sig" => match ent {
                        Some(e) => e.f148 as i64,
                        None => continue,
                    },
                    "mana_delta" => match ent {
                        Some(e) => e.f132 as i64,
                        None => continue,
                    },
                    _ => continue,
                };
                if retail != port {
                    self.wiz_hit((ws.wiz, name), t, 0, retail, port);
                }
            }
            for (name, port) in &ws.arrays {
                let retail: Vec<i64> = match *name {
                    "hate" => w.hate.iter().map(|&v| v as i64).collect(),
                    "war" => w.war.iter().map(|&v| (v != 0) as i64).collect(),
                    "learn" => w.learn.iter().map(|&v| v as i64).collect(),
                    "cooldown" => w.cooldown.iter().map(|&v| v as i64).collect(),
                    "owned" => w.owned_slots.iter().map(|&v| v as i64).collect(),
                    "acq" => w.spell_list.iter().map(|&v| v as i64).collect(),
                    "balloon_reg" => w.balloon_reg.iter().map(|&v| v as i64).collect(),
                    _ => continue,
                };
                for (i, (&a, &b)) in retail.iter().zip(port).enumerate() {
                    if a != b {
                        self.wiz_hit((ws.wiz, name), t, i as u16, a, b);
                    }
                }
            }
        }
    }

    /// Diff the port's free list against the recording's, filtered the
    /// way the importer itself filters it — so any difference is the
    /// port's own allocator ORDER, never the importer's census.
    ///
    /// This is the widest ungraded lane in the harness: a port that
    /// pushes a freed slot at the wrong moment, or frees a different
    /// NUMBER of slots, reads clean in pair mode forever and only bites
    /// a free run, as balanced same-`(class, model)` missing/extra rows
    /// once the two allocators hand out different slots for one spawn.
    pub(crate) fn compare_free_mc1(
        &mut self,
        world: &World,
        st: &RetailMc1,
        human_slot: u16,
        t: u64,
    ) {
        let pool = st.ents.len();
        let want: Vec<u16> = st
            .free_stack
            .iter()
            .copied()
            .filter(|&s| (s as usize) < pool && s != human_slot && st.ents[s as usize].class64 == 0)
            .collect();
        let got = world.free_stack_mc1();
        self.free.1 += 1;
        if want != got {
            self.free.0 += 1;
            if self.free.2.is_empty() {
                // Depth from the TOP is what matters: the next spawn
                // pops the end, so depth 0 diverging is a slot handed
                // out wrong THIS tick.
                let depth = want
                    .iter()
                    .rev()
                    .zip(got.iter().rev())
                    .position(|(a, b)| a != b);
                self.free.2 = format!(
                    "t={t} len retail {} port {}, top retail {:?} port {:?}, first top-diff depth {}",
                    want.len(),
                    got.len(),
                    want.last(),
                    got.last(),
                    depth.map_or("none (prefix)".into(), |d| d.to_string()),
                );
            }
        }
    }

    /// The report block. `by_first` orders lanes by the tick they FIRST
    /// part rather than by family — the free run's question is "what
    /// broke first", the pair run's is "which family is worst".
    pub(crate) fn render(&self, by_first: bool) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let total: u64 = self.lanes.values().map(|l| l.rows).sum();
        let _ = writeln!(
            s,
            "  RAW SHADOW (every ungraded per-entity lane): {total} mismatches"
        );
        let mut keys: Vec<_> = self.lanes.iter().collect();
        if by_first {
            keys.sort_by_key(|(k, l)| (l.first_t, k.0, k.1, k.2));
        }
        for (k, lane) in keys {
            let _ = writeln!(
                s,
                "    ({:>3},{:>3}) {}: {} rows t={}..{} across {} slot(s)  e.g. {}",
                k.0,
                k.1,
                k.2,
                lane.rows,
                lane.first_t,
                lane.last_t,
                lane.slots.len(),
                lane.example
            );
        }
        let wiz_total: u64 = self.wiz_lanes.values().map(|l| l.rows).sum();
        let _ = writeln!(
            s,
            "  WIZEXT SHADOW (Type_160 wizard/brain lanes): {wiz_total} mismatches"
        );
        let mut keys: Vec<_> = self.wiz_lanes.iter().collect();
        if by_first {
            keys.sort_by_key(|(k, l)| (l.first_t, k.0, k.1));
        }
        for (k, lane) in keys {
            let _ = writeln!(
                s,
                "    wiz {} {}: {} rows t={}..{} across {} idx  e.g. {}",
                k.0,
                k.1,
                lane.rows,
                lane.first_t,
                lane.last_t,
                lane.slots.len(),
                lane.example
            );
        }
        let _ = writeln!(
            s,
            "    free stack: {} / {} boundaries mismatched{}",
            self.free.0,
            self.free.1,
            if self.free.2.is_empty() {
                String::new()
            } else {
                format!("  e.g. {}", self.free.2)
            }
        );
        s
    }
}
