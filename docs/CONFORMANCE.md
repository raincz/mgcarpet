# The conformance suite

The automated regression arm of the conformance program: failing (and
sampled passing) pairs from retail recordings, encoded as committed
fixture manifests and replayed against the current sim on every test
run. It sits BESIDE the unit/golden tests — goldens pin the port
against itself (refactor guard); the suite pins it against RETAIL
(fidelity guard). Divergence triage stays in
`docs/CONFORMANCE-FINDINGS.md`; this suite is how a triaged finding
becomes an enforced expectation.

## Shape

A **fixture** IS its pair of retail states — two consecutive tick
records out of a recording — and it lives in **its own file, named
for the law it pins**:

    conformance/fixtures/<level>/<law-slug>.mgcr

An ordinary `.mgcr` holding the header plus THREE tick records,
`t-1 .. t+1`. Line `t-1` warms the cast-edge predecessor (MC1
`prev_fire` via `verify::fire_bits_mc1`; MC2 `prev_latch`) and carries
the materialized terrain base; line `t` is the state the runner
imports; line `t+1` carries the `obs` the pair is graded against. The
runner replays it through the same core as `verify-deltas`
(`verify::exec_pair`; one implementation, by construction) — import
state@t, tick once, diff obs@t+1.

ONE FILE, ONE PAIR, ONE WORLD. Each fixture gets its own freshly built
world, so a verdict is a property of the fixture. The old bundle
runner ticked every selected pair on ONE world, and that was not
academic: on mc2l0, `t=11334` conformed only after 400-800 unrelated
pairs had ticked first and `t=3918` after 60-200 — a committed green
fixture whose greenness was an artifact of its neighbours. (Measured
corpus-wide: 7,828 of 7,830 pairs were order-independent, so the hole
was narrow, but it was invisible and it was real.)

The directory is the LEVEL and the file name is the LAW — neither
mentions the take. A fixture is a pair of retail states for a level;
which recording it was cut from is provenance (`source` in the
manifest), not identity, and the take may not survive: mc1l32's suite
draws on two takes, and the recording for one of them is gone while
its fixtures still guard three laws the other never captured.

Because the name IS the law, the filesystem enforces one exemplar per
story — you cannot commit two files with the same name. Merging l32's
two takes collapsed 54 files onto **39 distinct laws**: `class-12
ecology churn` had been pinned five times over, `stuck explosion`
twice within a single take, and a directory boundary had been hiding
all of it.

Why one file rather than one bundle per take: it makes the unit of
STORAGE, of CURATION, of TEST and of REVIEW the same object. Under
bundles those were four different things — a row in a JSON array, a
14-43 MB binary, one `#[test]` for all ten takes, and nothing
reviewable — and the corpus grew to 7,830 fixtures of which 96%
carried no note and no status, because nothing made that visible.
Now `ls` is the curation review, `git rm` is the curation tool, a
failure names a law instead of a tick, `cargo test <lawname>` runs
exactly one pair, and appending costs one ~20 KB blob instead of
re-freezing a whole take (which minted a fresh permanent LFS object
every time — 18 versions, 178 MB, for 10 live bundles).

Evidence files are COMMITTED via git-lfs (`.gitattributes` tracks
`conformance/fixtures/**/*.mgcr`) and are WRITE-ONCE: nothing rewrites
a `.mgcr` after it is cut. Everything mutable — status, signature,
note — lives in the manifest TEXT, so `--promote` never touches a
binary. Fullsize recordings NEVER enter git — too large, and useless
in their entirety; `/recordings` stays ignored. The full take remains
the source for `verify-deltas` runs and re-cutting, and it is the
final authority: a fixture is a derived artifact, and a derived
artifact that is wrong is regenerated, not repaired.

A **manifest** (`conformance/<level>.json`, committed) records per
fixture:

- `t` — the pair.
- `status` — the expected verdict:
  - `conforming`: passed at extraction; must stay green. The
    regression corpus.
  - `open`: known-failing PORT lead; expected to fail with the
    recorded signature until the law is fixed. Carries a ledger note.
  - `capture`: known capture-domain limitation (terrain closure,
    input latency) — expected to fail, NOT a port bug. Kept so the
    day the closure gap is fixed, the whole class flips visibly.
- `sig`/`atoms` — the pair's diff **signature**: the sorted, deduped,
  slot- and value-free atom set (`missing:c,m`, `extra:c,m`,
  `field:c,m:name`, `field:player.mana`, `rng`). It captures the
  STORY of the failure, not its exact numbers, so it is stable across
  incidental drift.
- `won_sig`/`won_atoms` — the signature this fixture carried while it
  was an expected failure, kept when `--promote` closes it. A promoted
  fixture's receipt: what it proved, after it stopped failing.
- `source` — which take the pair was cut from. PROVENANCE ONLY: it
  does not change what the fixture pins, and the take may be gone.
- `note` — free-form triage pointer (ledger entry, family name).

Fullsize recordings and `baked/` stay local corpus data (like the
goldens' baked tree); the fixture files are repo artifacts. The
cargo test SKIPS with a printed `CONFORM-SKIP:` note when the baked
tree is absent, or when the evidence is (an LFS-less checkout leaves
pointer stubs), so a checkout without the corpus stays green.

**Under `MGC_REQUIRE_GOLDENS` every one of those skips is a FAILURE**
— the exact twin of `mgc-sim`'s `common::golden_skip`. Green-because-
skipped is the failure mode this lane is most exposed to: `baked/` is
925 MB of gitignored derived data, so CI, a fresh worktree and every
subagent used to take the skip and report the FIDELITY LANE AS PASSING
having executed nothing at all. Set the variable anywhere the suite is
expected to actually run.

## Verdicts

`mgc-conform fixtures conformance/mc1l0.json` replays every fixture
and classifies:

| manifest says | pair now | verdict |
|---|---|---|
| conforming | passes | ok |
| conforming | fails | **REGRESSION** — exit 1 |
| open / capture | fails, same signature | ok (expected) |
| open / capture | fails, different signature | **DRIFT** — exit 1 until acknowledged |
| open / capture | **passes** | **FIXED** — exit 1 until acknowledged |

The FIXED case being red is deliberate: progress must be
acknowledged, never silently absorbed. `--promote` accepts it
(status → conforming, live signature moved to `won_sig`) and refreshes
drifted signatures, rewriting the manifest — the diff then shows up in
review as the fix's conformance receipt.

DRIFT is red for the same reason. A changed signature means the pair
now fails a DIFFERENT way than the manifest recorded, so the recorded
expectation has stopped describing reality; under a subtle regression
that is the only signal there is, and it used to print and exit 0.

`--promote` moves a won signature to `won_sig`/`won_atoms` rather than
clearing it. A promoted fixture conforms and so carries no live `sig`
— and without a record of what it once proved it is byte-indistinguish-
able from a `--sample-every` corpus pair, which is precisely how
`carry_curation.py` (a signature bridge) used to drop the fixtures a
fix had EARNED, without even listing them as vanished.

## Sizing: only fixed work, only what can be named

A fixture is an ASSERTION — *this works, keep it working* — and
nothing else. Two rules decide membership, and both are cheap to
apply:

- **Only FIXED work.** Pending leads are not fixtures. A known-failing
  behaviour belongs in docs/CONFORMANCE-FINDINGS.md and, if it should
  be excused during triage, in `known-deviations.json`. Encoding a
  backlog item as an expected-failure fixture makes the suite half
  assertion and half TODO list, and it is the reason the corpus needed
  a status machine, a signature-drift lane, and a `--promote` dance in
  the first place.
- **Only what can be NAMED.** A fixture's file name is a claim about
  what it pins. If nobody can describe the pair in a sentence, it is
  `--sample-every` ballast and it does not earn a file. This is not a
  quality bar so much as an honesty one: measured on the pre-migration
  corpus, 96.1% of 7,830 fixtures carried no note and no status, and a
  reversion probe that deliberately broke three landed retail laws
  found that corpus caught **none** of them, while the one law it did
  catch fired 157 times where one would have done.

The broad regression net is NOT sampled pairs. For a level certified
bit-exact end-to-end, a full-horizon replay track covers every tick
rather than a 3-27% sample, at a fraction of the bytes, and names the
first divergent tick when it breaks. Pair fixtures exist alongside it
for field-level ATTRIBUTION and for laws the certified levels do not
exercise. Anything a deleted fixture would have caught surfaces as a
break in a certified recording, at which point a new fixture is cut
from real data.

## Lifecycle

1. **Extract** — after a recording session:
   `mgc-conform extract recordings/mc1l0.mgcr --input-delay 2
   --out conformance/mc1l0.json`. Failing pairs dedup by signature,
   keeping minimal exemplars up to `--max-open` (default 24);
   conforming pairs sample as the generic corpus. The extract is a
   STARTING POINT — curate it down to one exemplar per story before
   committing.
2. **Triage** — everything failing extracts as `open`. Curate:
   collapse same-story exemplars, reclassify closure-domain ones to
   `capture`, write notes citing ledger entries. Statuses are
   ledger-governed; the manifest is the enforcement, the ledger is
   the argument.
3. **Cut** — after curation:
   `conformance/cut_fixture_files.py conformance/mc1l0.json` copies
   each fixture's `t-1 .. t+1` window out of the take into its own
   `conformance/fixtures/<level>/<law>.mgcr`, slugging the file
   name from the note, and rewrites the manifest to carry `dir` plus a
   `file` per fixture. Commit the files (git-lfs) with the manifest.
   By default it cuts only fixtures whose note survives boilerplate
   stripping — `--all` overrides, `--dry-run` previews the names. It
   verifies line coverage itself: a window missing any of its three
   lines fails HERE rather than becoming a silently unreachable
   fixture. The runner REFUSES a manifest with no `dir` (an un-cut
   extract is not a suite).
4. **Fix** — a port fix flips its fixtures to FIXED; run with
   `--promote` and commit the manifest with the fix.
5. **Append** — a NEW failure found later (a playtest report, a new
   verify-deltas family) gets its exemplar added by hand: run
   `verify-deltas --dump <t>` to pick the minimal pair, add the
   entry with status `open` and the measured signature (run the
   suite once; it will report the drift/signature to record — or
   add with an empty `sig` and let `--promote` fill it).
6. **Re-extract** — when a recording is superseded. The frozen
   bundle keeps the OLD suite fully replayable even after its take
   is deleted — archive the manifest+bundle pair if its exemplars
   are still earning their keep. Signatures make the old and new
   manifests comparable:
   `conformance/carry_curation.py` ports statuses + notes onto the
   fresh extract by a three-tier bridge — live `sig`, then the
   `won_sig` receipt (a story that closed and has now come back is
   reported `REVIVED`, and its extracted `open` status is NOT
   overwritten), then the tick, which is the only bridge a promoted
   fixture has when the take was re-EXTRACTED rather than re-recorded.
   Every curated fixture — anything with a note, a receipt, or a
   hand-set status — that finds no home is reported `VANISHED`.
   `conformance/classify_fixtures.py` then
   auto-triages the still-noteless fixtures from the verify-deltas
   `--csv` rule column (all rows capture-explained → `capture`, else
   `open`, note = matched rule ids). Recording-side utilities
   (gap scan, level-transition boundary finder, conjoined-take cutter)
   live in `recordings/*.py`.

## Rules

- Never hand-edit `sig`/`atoms`/`won_sig`/`won_atoms` — they are
  measured values; use `--promote` to refresh.
- Statuses may be hand-edited freely; that is what they are for.
  Every non-empty `note` should point at a ledger entry.
- The suite runs the manifest's own `pin_pose` — reproducibility over
  CLI convenience. (`input_delay` is gone: it was 0 in every manifest
  and both pair loops discarded it, superseded by the dw_0 cast lane.)
- Never rewrite a `.mgcr` after it is cut. Evidence is write-once; the
  verdict is text. Renaming a fixture (a better slug) rewrites the
  manifest's `file` and the tree entry, and mints no new LFS object.
- A fixture that is wrong is DELETED, not repaired. The take is the
  authority, and it can always mint a replacement with real data.
- Keep suites per take (`mc1l0.json`, `mc1l32.json`, …); a
  re-recorded take gets a fresh extract, not an edit of the old one.
- Conformance (and goldens) run against PRISTINE bakes only. A bake
  with community-overlay files applied (docs/MODDING.md) carries a
  `MODDED` marker at the baked root and `meta.overlay` in each
  substituted package; `mgc-conform` hard-refuses such a level and the
  golden suites report it as a skip (= failure under
  `MGC_REQUIRE_GOLDENS=1`). Delete `baked/` and rebake without
  `gamedata/overlay/` before any conformance work.

## Current suites

| manifest | take | fixtures |
|---|---|---|
| `conformance/mc1l0.json` | mc1l0 — certified bit-exact end-to-end (0..7097) | 4 |
| `conformance/mc1l32.json` | mc1l32 + the retired bee-height cut (13 of the 39 come from it, including the bee z-law, HIT-ABORT and terrain-datum corner, which the surviving take never captured) | 39 |

43 fixtures, 1.6 MB, **0.2 s**. For scale, the corpus this replaced
was 7,830 fixtures across 10 takes in 158 MB of bundles taking ~40 s —
about 83% of the whole workspace test time. Seven takes were removed
outright (mc1hwl0, mc1l5, mc2l0, mc2l3, mc2l4, mc2l24, mc2l30): 6,783
fixtures carrying **three hand-written notes between them**. Their
manifests remain in git history (`git show <rev>:conformance/mc2l3.json`)
and their takes remain in `recordings/`, so re-cutting one is a command,
not an archaeology project.

mc1l1 and mc1l2 are certified bit-exact and have never been extracted;
they are owed a replay track plus their campaigns' named law exemplars.

The known-deviation roster

`conformance/known-deviations.json` (loaded by `verify-deltas` unless
`--no-roster`) classifies every diff row into NAMED, ledger-tracked
families so the run's headline is the UNEXPLAINED residue, not the
gross row count. The goal state on a fully triaged take: unexplained
= 0 — everything either conforming or matched to a rule.

Rules carry `status`:
- `capture` — a closure limitation of the recording (terrain channel,
  input latency, mid-frame window); not a port bug.
- `deviation` — intentional port behavior registered in
  docs/DEVIATIONS.md.
- `open` — a real, ledger-tracked port lead awaiting its fix round
  (known ≠ resolved; these are the working backlog).

Rules match first-hit in order and scope on take stem, row kind
(field/missing/extra), class, model, field name, pair-tick window,
tile rect and slot list. Every rule's `note` MUST cite its
CONFORMANCE-FINDINGS.md entry — a rule without provenance is a
suppression, not a classification, and does not belong in the file.

Guard rails:
- The runner prints per-rule hit counts (rows / pairs) on every run;
  a rule whose count jumps an order of magnitude is the regression
  signal — the roster surfaces it rather than hiding it.
- The `--csv` output carries the matched rule id in its final `rule`
  column (empty = unexplained), so offline triage can both filter
  known families and audit what a rule actually swallowed.
- The FIXTURE suite ignores the roster entirely: signatures stay raw
  so drift detection keeps full resolution.
- When a fix or closure lands, retire or re-scope the rules it
  obsoletes in the same change (the ledger's Resolved entry is the
  cue), exactly like fixture promotion.

### Visual-only families (player-ruled 2026-08-06)

A field lane VERIFIED as visual-only — a write-only spawn stamp, a
purely decorative entity (smoke, contrail puffs), or born-dead pool
bookkeeping (lightning trail nodes) — is classified `capture` rather
than dug: cycles go to gameplay divergence, not sprite noise. The
verification bar is non-negotiable and goes in the rule's note plus a
ledger entry:
- Read EVERY consumer of the lane (the tick handler AND external
  scans) before calling it inert — "fires are stationary and never
  read f30" took reading fire_tick, and the same sweep proved ball
  heading is NOT visual (it feeds the merge-walk each tick: those
  rows stay).
- Only field rows are eligible. Missing/extra atoms keep full weight
  — spawn cadence is gameplay evidence even for decorative entities.
- Lanes that are knock-ons of a REAL open lead (fire x/y under the
  spawn-cadence churn) are classified with `status: open`, citing the
  parent lead — explained, still on the books, never "capture".
- The per-rule hit count remains the tripwire: a visual-only rule
  whose count jumps means the family changed character — re-verify.

Report lines: `N pairs fully explained (conforming + explained = M)`
is the roster-aware conformance tier; `UNEXPLAINED rows: F field,
M missing, E extra` is the number a triage session works to zero.

## The pose-phase classifier

Retail's player pose is TWO-VALUED within a tick: the carpet moves at
its pool slot in the middle of the entity pass, so handlers at slots
below it read the pre-move pose and handlers above it the post-move
pose. The recording holds ONE sample per tick, so whichever
`--pin-pose` drives a pair, one side of the carpet-slot boundary sees
a pose that is one tick removed from what retail's same-slot handler
saw — aim yaw/pitch and pose-reactive steps diverge by exactly that
skew.

`verify-deltas` therefore re-runs every dirty pair under the OTHER
pose sample (`--no-pose-alt` disables): a row that is clean in either
run is tagged `pose-phase` — capture, not a lead. Row-level
either-matching is deliberately the union of both phases, which is
the slot-split semantics without needing the split point (below-slot
rows match the `n` run, above-slot rows the `n1` run).

Wiring mirrors the roster: `pose-phase` rows leave the UNEXPLAINED
headline and count toward `pairs fully explained`; the `--csv` rule
column carries the literal `pose-phase`; the report prints the
reclassified row/pair totals; FIXTURE signatures stay raw. The tag is
runner-built (no roster entry, no ledger rule) because it is derived
per pair from the recording itself, not from a triaged family. The
button channel is derived the same way on MC2 — cast-consume latency
is NOT unobservable there: the recorded press LATCH says per press
whether retail's poll had already taken it at snapshot time, so the
MC2 arm reconstructs the cast phase exactly and ignores
`--input-delay` (`verify_mc2::align_cmd_mc2`; ledger §"THE RECORDER'S
SNAPSHOT STRADDLES RETAIL'S INPUT POLL"). MC1 has no latch register
and stays `--input-delay`-modeled with cast-edge pairs bucketed
capture.

## The pose channel

`verify-deltas` pins the human pose, so the player's own motion column
is the one lane the world diff never verifies — the pinned slot's pose
fields are runner INPUTS, tautologically clean. The POSE CHANNEL
(`crates/mgc-conform/src/pose_lane.rs`; on by default,
`--no-pose-lane` disables) closes that hole: for every fixture-grade
pair it seeds the faithful mover's flight state from the recorded
closure at N, steps `flight::mc1_move`/`mc2_move` once against the
imported world, and diffs the stepped pose against the recorded pose
at N+1 — bit-exact, the movers being integer ports. Lanes: x/y/z,
yaw, aim/eff pitch, actual/target speed, strafe, the stick-filter
accumulators, and (MC1) the flutter clock + private LCG.

Input needs no reconstruction guesswork:

- the consumed move/fire byte (`Type_160/164 dw_0`) is stamped by the
  consume loop every tick and SURVIVES to the settled snapshot. The
  phase differs per game and is corpus-measured: MC1 stamps AFTER the
  entity pass (pair N→N+1 reads record N), MC2 stamps in PlayerEvents
  BEFORE it (read record N+1).
- the stick enters the mover only through the low-pass filter
  (`acc += (2·stick − acc)/4`), whose accumulators are recorded at
  BOTH ends of the pair, so the filter inverts exactly per pair
  (`pose_lane::recover_stick`); any solution is equivalent downstream.
  The MC1 map screen needs no gate — retail zeroes the command there
  and recovery returns a centered stick.
- a knock/buffet armed mid-pass (writers sit below the carpet's slot)
  reconstructs by un-decaying the N+1 channel.

Terrain probes run on the MEASURED terrain@N+1 — terraform writers
run before the carpet's slot, measured on mc1l0: every eff_pitch/z
residue row sat on a live terraform window. Gates classify what a
one-tick mover shadow cannot own: death/respawn, warps, the
Accelerate/Speed-spell domain (the importer does not seed
`player.speed_boost` yet), MC2 web-slow/paralyze pairs, and
unrecoverable stick transitions.

First full-corpus grades (2026-08-07, ~196k pairs stepped, % of
stepped bit-exact): mc1l0 99.93 · mc1hwl0 99.87 · mc2l0 99.88 ·
mc2l4 99.97 · mc2l30 99.75 · mc2l24 99.13 · mc2l3 97.0 — the l3
residue is one positioned lead (the MC2 commit gate refusing
water-skim moves retail allowed; ledger §POSE CHANNEL). CSV rows
carry `kind = pose`, field `pose.<lane>`, empty rule column. The
FIXTURE suite does not run the channel — signatures stay pose-free by
construction (the shadow step never touches `exec_pair`). Triage
microscopes: `--example flight_dump_mc1` / `flight_dump_mc2` (the
recorded flight column per tick).

## The replay verifier

`mgc-conform replay <take.mgcr>` (crates/mgc-conform/src/replay.rs)
is PURE INPUT REPLAY — the zero-drift instrument: seed the world ONCE
from the recording's first closure, then free-run, feeding only the
per-tick input recovered from the recording. The mover steps OUTSIDE
the world tick exactly like the app (`Simulation::step`'s faithful
path, integer-only), `World::tick(pose, cmd)` after; nothing pins,
nothing re-imports, and divergence is REPORTED at every recorded
boundary, never corrected. The headline is the BIT-EXACT HORIZON —
graded boundaries clean from the anchor before the first divergence —
plus per-channel firsts (pose / rng / entity-set / fields) and
post-divergence traffic. A recording `t` gap re-anchors a fresh
segment (a capture artifact, not a resync); `--start <t>` anchors
late; `--csv` emits the verify-deltas TSV shape.

`--pose-only` is the tier-2 chain: the FLIGHT state chains while the
world context re-imports per pair — it isolates the mover +
input-recovery chain from world fidelity. World-driven pose domains
(death/respawn, warps, the accel domain, debuffs, unrecoverable stick
transitions) re-seed the chain silently and are counted as gates.

Input recovery is exact, byte-domain (no `--input-delay` modeling):

- The consumed move/fire byte (`dw_0`, RECORDING.md): bits 1/2/4/8
  move, **0x10 = left fire, 0x20 = right fire** — retail's cast
  decision is a pure function of this byte + sim state (the cast
  dispatch is LEVEL-triggered through the manifestation reload
  ladder; edge inference is unnecessary and wrong). Corpus-measured:
  MC1 2,368/2,368 single-shot casts at byte-record +2; MC2 560/560
  arms on the same record — strictly stronger than the press-latch
  law, whose extra edges are UI clicks the byte correctly omits.
  This RETIRES MC1's ±1-tick cast caveat for byte-domain consumers.
- Stick: the pose channel's filter inversion per recorded pair.
- Equips: recorded hand changes replay as equip commands (MC1
  acquisition-list resolve; MC2 pane select with the recorded
  per-spell tier, out-of-range = unbind).
- Respawn: the SPACE lane (MC2 with the recentre witness; MC1 keeps
  the keyboard ±1 caveat).
- Demolish (Shift+L): MC1/HW ride the move byte itself — `dw_0 ==
  48` IS the command word (retail's :55760 predicate; such a tick
  fires NEITHER hand — the whole mover short-circuits). MC2 has no
  move-byte trace (`PlayerAction` 0x2A): the witness is the own
  castle at the END record with `life == -1` in the destroy intake
  (action 6), corroborated by the held Shift+L scancodes.

Chained replay only matches SETTLED boundaries, so capture-phase
families (pose-phase, mid-pass terraform, add-mailbox) EVAPORATE
here — and conversely every OPEN world family becomes a
chain-breaker. Findings land in the ledger under §THE REPLAY
VERIFIER.

The recovery laws above LIVE in `mgc_formats::recover` (the shared
home) and the chain seeding/pose-lane compare in mgc-sim's
conformance module — one implementation drives this verifier AND the
game's own `--replay` / `--replay-check` (RECORDING.md "Consumers").
The in-app chain is certified against this verifier: identical
first-divergence boundaries on mc1l0 (pose t=563) and mc2l3 (pose
t=244), 2026-08-07. The app feeds the recovered move byte to the
faithful movers verbatim (`FlightInput::mc1_move_byte`) — the float
axes cannot express retail's both-bits-held states — and its
faithful tier hands `World::tick` the INTEGER carpet pose (the
quantization-risk fix; the enhanced tier keeps the float flyer).
