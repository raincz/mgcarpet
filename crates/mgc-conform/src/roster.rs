//! The KNOWN-DEVIATION ROSTER (docs/CONFORMANCE.md): a committed
//! rule list (`conformance/known-deviations.json`) that classifies
//! `verify-deltas` diff rows into NAMED, ledger-tracked families —
//! capture-domain closure gaps (terrain, input latency), registered
//! DEVIATIONS.md behavior, and open port leads — so a triaged take's
//! headline number is the UNEXPLAINED residue, not the gross row
//! count. The goal state on a fully triaged take: unexplained = 0,
//! everything either conforming or matched to a rule.
//!
//! Rules are deliberately SCOPED (take, family, field, onset window,
//! tile rect) and the runner always prints per-rule hit counts — a
//! rule that suddenly matches an order of magnitude more rows is a
//! visible signal, not a silent mask. The FIXTURE suite is untouched:
//! signatures stay raw so drift detection keeps its full resolution;
//! the roster shapes only the verify-deltas report and its CSV.

use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RuleStatus {
    /// A closure limitation of the recording (no terrain channel,
    /// input latency, mid-frame capture) — not a port bug.
    Capture,
    /// Registered intentional port behavior (docs/DEVIATIONS.md).
    Deviation,
    /// A known, ledger-tracked port lead awaiting its fix round.
    Open,
}

impl RuleStatus {
    pub fn tag(self) -> &'static str {
        match self {
            RuleStatus::Capture => "capture",
            RuleStatus::Deviation => "deviation",
            RuleStatus::Open => "open",
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Field,
    Missing,
    Extra,
}

#[derive(Deserialize)]
pub struct Rule {
    /// Short kebab-case name, unique; shown in the report + CSV.
    pub id: String,
    pub status: RuleStatus,
    /// One-line provenance — cite the ledger entry it rides.
    #[allow(dead_code)]
    pub note: String,
    /// Recording stems this rule applies to (e.g. "mc2l0"); absent =
    /// every take.
    #[serde(default)]
    pub takes: Option<Vec<String>>,
    /// Row kind; absent = any.
    #[serde(default)]
    pub kind: Option<RowKind>,
    #[serde(default)]
    pub class: Option<u8>,
    #[serde(default)]
    pub model: Option<u8>,
    /// Field name (field rows only; a field-bearing rule never
    /// matches missing/extra rows).
    #[serde(default)]
    pub field: Option<String>,
    /// Pair-tick onset window, inclusive.
    #[serde(default)]
    pub t_min: Option<u64>,
    #[serde(default)]
    pub t_max: Option<u64>,
    /// Tile-space rect [x0, y0, x1, y1] inclusive (CSV coordinates —
    /// world / 256). Rows with no coordinate context never match a
    /// rect-scoped rule.
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
    /// Explicit slot list.
    #[serde(default)]
    pub slots: Option<Vec<u16>>,
}

#[derive(Deserialize)]
pub struct Roster {
    pub rules: Vec<Rule>,
}

/// One diff row's matching context.
pub struct RowCtx<'a> {
    pub kind: RowKind,
    pub slot: Option<u16>,
    pub class: u8,
    pub model: u8,
    /// Field name for field rows.
    pub field: Option<&'a str>,
    /// Tile-space position, when entity context exists.
    pub pos: Option<(f64, f64)>,
}

impl Roster {
    pub fn load(path: &Path) -> Result<Option<Roster>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let r: Roster =
            serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut seen = std::collections::BTreeSet::new();
        for rule in &r.rules {
            if !seen.insert(&rule.id) {
                return Err(format!("duplicate roster rule id `{}`", rule.id));
            }
        }
        Ok(Some(r))
    }

    /// First matching rule's index, or None = unexplained.
    pub fn classify(&self, take: &str, t: u64, row: &RowCtx) -> Option<usize> {
        self.rules.iter().position(|r| {
            if let Some(takes) = &r.takes
                && !takes.iter().any(|s| s == take)
            {
                return false;
            }
            if let Some(k) = r.kind
                && k != row.kind
            {
                return false;
            }
            if let Some(c) = r.class
                && c != row.class
            {
                return false;
            }
            if let Some(m) = r.model
                && m != row.model
            {
                return false;
            }
            if let Some(f) = &r.field {
                match row.field {
                    Some(rf) if rf == f => {}
                    _ => return false,
                }
            }
            if let Some(t0) = r.t_min
                && t < t0
            {
                return false;
            }
            if let Some(t1) = r.t_max
                && t > t1
            {
                return false;
            }
            if let Some(slots) = &r.slots {
                match row.slot {
                    Some(s) if slots.contains(&s) => {}
                    _ => return false,
                }
            }
            if let Some([x0, y0, x1, y1]) = r.rect {
                match row.pos {
                    Some((x, y)) if x >= x0 && x <= x1 && y >= y0 && y <= y1 => {}
                    _ => return false,
                }
            }
            true
        })
    }
}

/// One diff row's classification.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// Needs investigation — matched by nothing.
    Unexplained,
    /// Matched roster rule (index into `Roster::rules`).
    Rule(usize),
    /// The row is clean in the port run driven by the OTHER
    /// `--pin-pose` sample. Retail's player pose changes mid-tick at
    /// the carpet's pool slot, so handlers on the two sides of that
    /// slot saw different poses — the once-per-tick capture holds
    /// only one of them, and every pinned run is wrong for one side.
    /// Runner-built (no roster provenance), reported separately.
    PosePhase,
    /// A missing/extra atom paired with a same-(class,model) atom of
    /// the opposite kind WITHIN the pair: the port emits the entity,
    /// but in a different pool slot than retail. See
    /// [`RuleTags::slot_desync`] for the ruled basis (session-4
    /// free-list slot-order desync + open-leads 0b mass-spawn waves).
    /// Runner-built (computed per pair, no roster provenance),
    /// reported separately.
    SlotDesync,
    /// A mover's x/y/heading/pitch row on a slot whose z row THIS
    /// pair is already claimed by a terrain rule: the walker
    /// knock-on of the terrain closure (the ground under the mover
    /// diverges, so its ground-following motion diverges with it).
    /// The mc1l0-terrain-z rule note predicted exactly this family
    /// ("the walker x/y/heading knock-on"); the 2026-08-13 (5,3)
    /// t=2978 flock measured it — every shadowed row rides a
    /// terrain-z-tagged z sibling on the same slot. Runner-built,
    /// reported separately.
    TerrainShadow,
}

impl Tag {
    pub fn known(self) -> bool {
        self != Tag::Unexplained
    }
}

/// Per-row rule tags for one pair, index-aligned with the PairDiff's
/// missing / extra / fields vectors.
#[derive(Default)]
pub struct RuleTags {
    pub missing: Vec<Tag>,
    pub extra: Vec<Tag>,
    pub fields: Vec<Tag>,
}

impl RuleTags {
    pub fn all_known(&self) -> bool {
        self.missing.iter().all(|t| t.known())
            && self.extra.iter().all(|t| t.known())
            && self.fields.iter().all(|t| t.known())
    }

    /// The SLOT-DESYNC classifier — a COMPUTED rule (literal id
    /// `slot-desync`), the missing/extra twin of the pose-phase pass.
    ///
    /// Ruled basis (cite both):
    ///   • Session-4 ruling "MC2 FIRE-SPRAY RING LOOP … RULED on the
    ///     residual: the (10,0) missing/extra bulk = FREE-LIST
    ///     SLOT-ORDER DESYNC, not law — proven at l4 t=9082: missing
    ///     and extra fires have IDENTICAL x/y, differing only in slot"
    ///     (docs/CONFORMANCE-FINDINGS.md, Resolved). A single-snapshot
    ///     import cannot recover retail's within-tick free-then-reuse
    ///     LIFO order, so the port emits the SAME entity at the SAME
    ///     place in a different pool slot; the presence matcher (keyed
    ///     by slot) reports it as one missing + one extra.
    ///   • Open-leads 0b "MC2L24 SCRIPTED CREATURE WAVES — SPAWN, BUT
    ///     SLOT-DESYNCED": a mass-spawn wave lands its creatures in
    ///     desynced slots — BALANCED extra+missing of the SAME
    ///     (class,model) in the same pair (whole-take totals balance:
    ///     (5,3) 63/60, (14,1) 4/4, (5,9) 6/8). The port DOES spawn
    ///     the waves; the free-list slot-order infrastructure limit at
    ///     mass-spawn ticks is not a missing trigger.
    ///
    /// RE-SCOPED DOMAIN (2026-08-04 housekeeping): the session-9
    /// pool-base decode fix ELIMINATED the late-take desync at its
    /// root (`free-stack fallback` 14k pairs → 0; the rule's l24 rows
    /// went from 124+ to the low 200s take-wide, all in the EARLY-WAVE
    /// region t=3569/13330, which predates the first shifted snapshot
    /// at t=54932 and has its own un-dug cause). The rule's remaining
    /// LEGITIMATE domain is that early-wave family; a hit-count rise
    /// anywhere ELSE is a regression signal (allocator, importer
    /// stacks, or the mgcr pool-base recovery), not comfort.
    ///
    /// CONSERVATIVE by construction: within a SINGLE pair, only atoms
    /// of the same (class,model) whose missing and extra counts can be
    /// PAIRED are tagged, and only `min(missing, extra)` per side.
    /// Genuinely one-sided COUNT residue — a real unported spawn or
    /// despawn — stays `Unexplained`, so the rule never swallows a
    /// lead. Pairing iterates the SMALLER side and greedily takes the
    /// nearest x/y partner on the larger side (a desynced atom rests at
    /// the SAME place, so its slot-shifted twin is the nearest one):
    /// the count residue left over is therefore the genuinely-unmatched
    /// extreme, not an arbitrary slot-order pick. Absent coordinates
    /// fall back to count-matching (INF distance, first-unused).
    ///
    /// Runs on the residue AFTER the roster pass and BEFORE the
    /// pose-phase pass (only rows still `Unexplained` are considered),
    /// so it claims nothing another family explained; running it before
    /// pose-phase is required — at a spawn wave the port-side EXTRA
    /// rows are pose-phase (their exact slot differs under the other
    /// pose) while the retail-side MISSING rows are not, so a
    /// pose-first order would strip the extras and leave the balanced
    /// missing family orphaned. FIELD rows are out of the ruled scope
    /// (missing/extra only) and untouched.
    pub fn slot_desync(
        &mut self,
        missing: &[(u16, u8, u8)],
        extra: &[(u16, u8, u8)],
        pos: &dyn Fn(u16) -> Option<(f64, f64)>,
    ) {
        use std::collections::BTreeMap;
        // Group still-unexplained row indices by (class, model), one
        // side at a time.
        let mut miss: BTreeMap<(u8, u8), Vec<usize>> = BTreeMap::new();
        let mut ext: BTreeMap<(u8, u8), Vec<usize>> = BTreeMap::new();
        for (i, (_, c, m)) in missing.iter().enumerate() {
            if self.missing[i] == Tag::Unexplained {
                miss.entry((*c, *m)).or_default().push(i);
            }
        }
        for (i, (_, c, m)) in extra.iter().enumerate() {
            if self.extra[i] == Tag::Unexplained {
                ext.entry((*c, *m)).or_default().push(i);
            }
        }
        for (key, mi) in &miss {
            let Some(ei) = ext.get(key) else { continue };
            // (row index, position) for each side of this family.
            let mvec: Vec<(usize, Option<(f64, f64)>)> =
                mi.iter().map(|&i| (i, pos(missing[i].0))).collect();
            let evec: Vec<(usize, Option<(f64, f64)>)> =
                ei.iter().map(|&i| (i, pos(extra[i].0))).collect();
            // Iterate the smaller side so the unmatched count residue
            // stays on the larger (one-sided) side, untagged.
            let miss_smaller = mvec.len() <= evec.len();
            let (small, large) = if miss_smaller {
                (&mvec, &evec)
            } else {
                (&evec, &mvec)
            };
            let mut used = vec![false; large.len()];
            for &(sidx, sp) in small {
                let mut best: Option<(usize, f64)> = None;
                for (li, &(_, lp)) in large.iter().enumerate() {
                    if used[li] {
                        continue;
                    }
                    let d = match (sp, lp) {
                        (Some((ax, ay)), Some((bx, by))) => (ax - bx).hypot(ay - by),
                        _ => f64::INFINITY,
                    };
                    match best {
                        Some((_, bd)) if bd <= d => {}
                        _ => best = Some((li, d)),
                    }
                }
                if let Some((li, _)) = best {
                    used[li] = true;
                    let lidx = large[li].0;
                    if miss_smaller {
                        self.missing[sidx] = Tag::SlotDesync;
                        self.extra[lidx] = Tag::SlotDesync;
                    } else {
                        self.extra[sidx] = Tag::SlotDesync;
                        self.missing[lidx] = Tag::SlotDesync;
                    }
                }
            }
        }
    }
}
