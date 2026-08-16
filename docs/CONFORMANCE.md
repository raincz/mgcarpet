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
a `.mgcr` after it is cut, and no suite run rewrites a manifest
either. The only mutable thing is the manifest TEXT, and only a human
edits it. Fullsize recordings NEVER enter git — too large, and useless
in their entirety; `/recordings` stays ignored. The full take remains
the source for `verify-deltas` runs and re-cutting, and it is the
final authority: a fixture is a derived artifact, and a derived
artifact that is wrong is regenerated, not repaired.

A **manifest** (`conformance/<level>.json`, committed) records per
fixture:

- `t` — the pair.
- `file` — the evidence file inside `dir`. Named for the LAW.
- `source` — which take the pair was cut from. PROVENANCE ONLY: it
  does not change what the fixture pins, and the take may be gone.
- `note` — free-form pointer to the ledger entry the law lives in.

**And nothing else. EXISTENCE IS THE ASSERTION.** There is no status
field: a fixture means *this law works, keep it working*, so it
either passes or it is a REGRESSION, and retracting one is `git rm`.

That is a deliberate demolition (2026-08-16b) of a machine that had
grown around a value which never varied — expected statuses
(`conforming`/`open`/`capture`), diff signatures and their FNV hashes,
`won_sig`/`won_atoms` receipts, `--promote`/`--demote`, and the
FIXED/DRIFT verdicts. All of it existed to describe fixtures that were
EXPECTED TO FAIL, and the corpus no longer contains any: every pending
fixture was fixed or deleted first. Keeping the machinery would have
meant maintaining a comparison whose answer was known.

The manifest survives the cull because a DECLARED LIST is checkable in
ways a directory is not, and both directions have caught real
mistakes:

- a file on disk but **not declared** — orphaned by a rename or a
  half-finished cut, silently testing nothing;
- a file **declared but missing** — a fixture deleted without its
  entry, which would otherwise just shrink the suite in silence.

Both are hard errors (exit 2), reported by law name.

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

`mgc-conform fixtures conformance/mc1l0.json` replays every declared
fixture. There are two outcomes and one of them is red:

| pair now | verdict |
|---|---|
| passes | ok |
| fails | **REGRESSION** — exit 1, named by law, atoms listed |

plus two structural errors that exit 2 before anything runs: an
undeclared evidence file, and a declared file that is missing.

The regression message carries the diff **signature** — the sorted,
deduped, slot- and value-free atom set (`missing:c,m`, `extra:c,m`,
`field:c,m:name`, `field:player.mana`, `rng`). It is no longer
COMPARED against anything; it is simply the best available description
of what moved, naming families rather than slots or numbers that shift
harmlessly.

**The suite is a pure reader.** No flag makes it rewrite a manifest,
so a test run cannot launder a failure into a new expectation — the
only way to change what is asserted is to edit the manifest or
`git rm` the evidence, both of which show up in review.

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
  the first place. This rule is now STRUCTURAL rather than advisory:
  there is no status field to file a pending lead under, so the suite
  cannot become a backlog again.
- **Some laws do not belong here at all.** Pair mode re-imports retail
  state every tick and the obs schema does not carry every field, so a
  law can be invisible to any possible fixture (`+52`/`+70`). Others
  have their only exemplar in a take that is permanently divergent for
  capture reasons — a format-1 recording replayed on pristine terrain
  can never produce a green pair, however correct the port is. Both
  get UNIT TESTS instead. Decide the lane per law before spending
  bundle bytes.
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
   `mgc-conform extract recordings/mc1l0.mgcr
   --out conformance/mc1l0.json`. The manifest gets the CONFORMING
   pairs, sampled every `--sample-every` (default 10), as unnamed
   CANDIDATES. Failing pairs are deduped by story and **printed, not
   written** — minimal exemplar first, for the ledger. An extract that
   files its own unexplained divergences as fixtures is how a corpus of
   7,830 accumulated in which 96% carried no note.
2. **Triage** — read the printed failing stories into
   docs/CONFORMANCE-FINDINGS.md, and into `known-deviations.json` if a
   family should be excused during triage. Then pick, from the
   candidates, the pairs that pin a law worth a file. The ledger is
   the argument; the manifest is only the enforcement.
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
4. **Fix** — when a port fix closes a law, cut its exemplar in DELIBERATELY,
   by name, in the same change: the fixture is the fix's receipt. If the
   law cannot be fixture-guarded (see Sizing), write the unit test instead
   and say so in the ledger entry.
5. **Append** — a NEW law fixed later (a playtest report, a new
   verify-deltas family) gets its exemplar added by hand: run
   `verify-deltas --dump <t>` to pick the minimal pair, cut it, and add
   `{t, file, source, note}`. There is nothing to measure or record —
   if it passes it belongs, and if it does not it is not fixed yet.
6. **Re-extract** — when a recording is superseded. Existing evidence
   files are self-contained and keep working: a fixture is a COPY, so a
   superseded take does not invalidate the suite cut from it, and the
   old fixtures need no bridging onto the new extract. (The two scripts
   that used to carry statuses and signatures across a re-extract,
   `carry_curation.py` and `classify_fixtures.py`, were deleted with
   the status machine — they had nothing left to carry.) Recording-side
   utilities (gap scan, level-transition boundary finder, conjoined-take
   cutter) live in `recordings/*.py`.

## Rules

- Every non-empty `note` should point at a ledger entry.
- A fixture is added when a law is FIXED and removed with `git rm`
  when it stops being worth asserting. There is no third state.
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
- **Run every probe through `tools/conform`**, and pass kill switches
  as `--env K=V` BEFORE the mode:
  `./tools/conform --env MGC_NO_DEAF_STATES=1 fixtures conformance/mc1l32.json`.
  The wrapper caps the address space (a bare `ulimit -v` in the calling
  shell would poison later cargo builds) and keeps every invocation,
  however instrumented, the SAME command — a shell prefix
  (`MGC_X=1 ./tools/conform …`) is a different command to the
  permission allowlist and stalls the run on an approval prompt.
  ⚠ It execs `target/release`, so `cargo build --release` after a
  schema change or the probe reads the old binary.
- `verify-deltas` reads per-fixture evidence files directly — the
  obs-stripped lead records are skipped and counted as `ungraded
  leads`. That is the microscope for a failing fixture: the suite says
  WHICH law broke, `--dump <t> --max-diffs N` says how.
- ⚠ The suite and `verify-deltas` must reconstruct input IDENTICALLY
  or a fixture's verdict drifts from the triage run that recorded it.
  Only FIRE rides the raw input channel; equips and demolish are
  rebuilt from the pair by `recover_pair_mc1`. The MC1 suite loop was
  missing that recovery until 2026-08-16 and filed three equip-lane
  pairs as port leads.
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
| `conformance/mc1l32.json` | mc1l32 + the retired bee-height cut | 21 |

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
