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
   `conformance/cut_fixture_files.py conformance/mc1l0.json recordings/mc1l0.mgcr`
   copies each fixture's `t-1 .. t+1` window out of the take into its
   own `conformance/fixtures/<level>/<law>.mgcr`, slugging the file
   name from the note. Commit the files (git-lfs) with the manifest.
   By default it cuts only fixtures whose note survives boilerplate
   stripping — `--all` overrides, `--dry-run` previews the names. It
   verifies line coverage itself: a window missing any of its three
   lines fails HERE rather than becoming a silently unreachable
   fixture. It also SKIPS any law already pinned in the level dir, so
   re-running it to append is safe.
   ⚠ **The cutter does NOT write the manifest** — it only reads it.
   `dir`, and each row's `file`, are yours to author, and `file` must
   match the slug the cutter derived from that row's `note`. Run
   `--dry-run` first, which prints exactly the names it will write.
   The runner REFUSES a manifest with no `dir` (an un-cut extract is
   not a suite), and both directions of the declared-vs-on-disk diff
   are hard errors: an undeclared `.mgcr` in the dir tests nothing, and
   a declared file that is absent fails by law name with exit 2.
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

## The reversion probe: mining a certified level

A certified level is a measurement problem of its own. Once mc1l0 is
bit-exact end-to-end, `verify-deltas` on it is nearly silent — which
means a HEAD run cannot tell a load-bearing tick from ballast, and
every tick looks equally worth pinning. Picking fixture ticks by
reading the ledger and guessing is precisely how a corpus reaches
7,830 fixtures that catch nothing.

The instrument that *can* tell is a binary built at the commit JUST
BEFORE that level's fix commit. Every pair it calls divergent is a
pair where the landed laws actually do work:

    fixture candidate  =  divergent PRE-FIX  ∧  raw-clean at HEAD

The left conjunct is non-vacuity — revert the laws and the fixture goes
red. The right is the fixed-work rule. Both are measured, neither is
guessed. Two tools implement it:

- **`tools/conform-rig <rig> [--env K=V]… <mode> …`** runs a pre-fix
  `mgc-conform` out of a detached worktree under
  `.claude/worktrees/rig-<rig>/`, same argv contract as `tools/conform`.
  Its usage text names each rig's commit and how to build a new one.
  ⚠ It `cd`s into the rig so the default `--baked baked` resolves
  through a symlink back to the real bake — so pass recordings and
  `--csv` destinations as ABSOLUTE paths.
- **`tools/fixture_candidates.py <prefix.tsv> <head.tsv>`** diffs the
  two `--csv` runs and ranks the candidates. It groups ticks by
  SIGNATURE — the slot- and value-free atom set, mirroring
  `fixtures::signature` — because one signature is one story: the
  group's minimal tick is the exemplar worth cutting and the rest is
  duplicate coverage. `--tick T` explains a single tick (non-vacuous?
  green at HEAD? which rows?), which is the form to reach for when the
  ledger names a tick and you want to know what pinning it would prove.

Both sides ignore `pose` rows, because `PairDiff::clean()` tests only
rng/missing/extra/fields: a pose row neither fails a fixture nor proves
one non-vacuous. Rows carrying a `rule` tag (a known-deviation or
roster attribution) are NOT ignored — a tagged row still fails
`clean()`, so it is still valid non-vacuity evidence.

Measured on the three certified MC1 levels, this collapses the search
by three orders of magnitude — 2,018 / 1,125 / 7,822 candidate ticks
onto 89 / 21 / 214 distinct stories, of which the ledger can name a
few dozen. ⚠ A candidate is only a candidate: the suite runs each
fixture in ISOLATION, on its own freshly built world, which is a
stricter test than the full-take run. A tick that is clean in the HEAD
sweep can still fail once cut, and the doctrine there is to delete it,
not to nurse it.

⚠⚠ **A rig sees only the laws its own commit landed.** The probe
answers "is this tick non-vacuous *for this commit*", NOT "does this
fixture guard anything at all". A fixture pinning a law fixed in an
EARLIER campaign is clean on both sides and scores as vacuous, because
the rig already contains that law. Measured on the pre-existing mc1l0
suite: 2 of its 4 fixtures read clean under the `l0` rig, and one of
them (`mana-magnet-10-54-lifecycle-ball-pull`) pins a real fix from a
different level's campaign — **player-ruled 2026-08-17 to KEEP, on the
rule that a passing fixture stays.** So never read "clean pre-fix" as
a delete signal on its own: it is a delete signal only for a fixture
CUT FROM THAT COMMIT, where it means the tick never exercised the law
its note claims.

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
| `conformance/mc1l0.json` | mc1l0 — CERTIFIED bit-exact (0..7097) | 18 |
| `conformance/mc1l1.json` | mc1l1 — CERTIFIED bit-exact (0..10709) | 10 |
| `conformance/mc1l2.json` | mc1l2 — CERTIFIED bit-exact (0..10588) | 16 |
| `conformance/mc1l3.json` | mc1l3 — the scratch-record chase bearing | 1 |
| `conformance/mc1l4.json` | mc1l4 — the rival speed-token + ball chain | 2 |
| `conformance/mc1l5.json` | mc1l5 — hit-arm, `+144` census, soft-kill ×2 | 5 |
| `conformance/mc1l32.json` | mc1l32 + the retired bee-height cut | 21 |
| `conformance/mc1l42.json` | mc1l42 — CERTIFIED bit-exact (0..30878) | 17 |

90 fixtures, **0.3 s**. The three certified MC1 levels were
mined in one pass (2026-08-17) by the reversion probe above; every
fixture cut there is measured non-vacuous against its level's own
pre-fix binary. ⚠ Their files are ~120-180 KB each rather than
mc1l32's ~13 KB, because these takes are FORMAT 2 and each fixture
carries a materialized terrain base (~350 KB uncompressed). That is
the price of the terrain channel, and it is the same channel that
makes the level minable at all — mc1l32's format-1 pairs cannot be cut
green at any size.

For scale, the corpus this replaced
was 7,830 fixtures across 10 takes in 158 MB of bundles taking ~40 s —
about 83% of the whole workspace test time. Seven takes were removed
outright (mc1hwl0, mc1l5, mc2l0, mc2l3, mc2l4, mc2l24, mc2l30): 6,783
fixtures carrying **three hand-written notes between them**. Their
manifests remain in git history (`git show <rev>:conformance/mc2l3.json`)
and their takes remain in `recordings/`, so re-cutting one is a command,
not an archaeology project.

mc1l1 and mc1l2 now carry their campaigns' named law exemplars; all
three certified levels are still owed a REPLAY TRACK (the broad net —
see Sizing), which remains undecided between retail-obs checkpoints and
a self-pinned hash chain.

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

## The pose pair (and the `pose-phase` classifier it retired)

Retail's player pose is TWO-VALUED within a tick: the carpet moves at
its pool slot in the middle of the entity pass, so handlers at slots
below it read the pre-move pose and handlers above it the post-move
pose.

**Pair mode drives BOTH samples.** `verify-deltas` feeds the walk the
record's pose PAIR — state@N below the carpet slot, state@N+1 above,
swapped at the carpet's own slot exactly where `tick_flight`'s
`FlightDrive` branch swaps for the free replay. `MGC_NO_POSE_PAIR=1`
restores the old single-sample walk, where `--pin-pose n|n1` chose
which sample drove the whole pass; under the pair, `--pin-pose` steers
only the alt probe below.

The single-sample walk was not merely imprecise — it could not
reproduce retail at all wherever two entities on OPPOSITE sides of the
carpet slot are coupled inside one tick. mc1l42 t=65 is the exemplar:
the genie at slot 101 (below the carpet at 331) mints its steal seeker
bearing on the PRE-move carpet, 309; the newborn seeker at slot 356
(above it) refreshes `+34` on the POST-move carpet, 321 — exactly
retail's record — and then eases one turn-cap step, landing on
retail's `+30` of 320. No pinned sample can emit 320, which is why
`retail − port@pin-n` was EXACTLY ±11 (one turn cap, never a fraction
of one) in 201 of 201 (9,8) heading rows and 16 of 16 (10,25) rows.
Landing the pair took mc1l42's CSV from 54,746 rows to 330 and its raw
residue from 1,209 to 276, with all 77 fixtures unchanged.

The recording still holds ONE sample per tick, so the alt pass remains
as a residual classifier: `verify-deltas` re-runs every dirty pair
under the other pose sample (`--no-pose-alt` disables) and a row clean
in either run is tagged `pose-phase` — capture, not a lead. Under the
pose pair that tag is nearly dead (53,157 rows → 26 on mc1l42); a
surviving `pose-phase` row now means a genuinely unmodelled phase, not
the routine slot-split. Row-level
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

### `--segmented` — the segmented free run

The plain mode names only the FIRST break and runs wild after it, so
one early defect hides the whole rest of the take. `--segmented` drops
the "never correct" clause: a true incremental deviation re-anchors the
free state from the recording exactly the way a capture gap already
does, and the take reads as a sequence of MAXIMAL CONTINUOUS SEGMENTS.

**Certification is ONE segment end to end.** The number that matters is
therefore resets in EXCESS of the gap-forced ones — a take with capture
gaps structurally cannot certify, and that is a property of the
RECORDING, not of the port. The summary prints
`segments / gap-forced / DEVIATION-forced` and collapses the reset
ticks into runs, because a wrong law usually fails on a RUN of adjacent
ticks (one carcass, one respawn, one clash): triage the CLUSTER count,
not the reset count.

Every reset tick names itself as a fixture candidate, which is what
THE REVERSION PROBE (`tools/conform-rig`) was built to do the hard way
— that probe stays useful for mining laws that landed before this
instrument existed, and stops being needed for new work.

⚠ **CHECK `D1(t)` BEFORE REACHING FOR THE CUTTER.** The candidate list
is a list of FREE-RUN breaks, and the fixture runner is PAIR mode, so
the two only overlap where the break is LOCAL. A reset whose pair diff
is CLEAN is by construction a law the importer restores every tick
(`+44`, the acquisition list, the free stack, the whole flight chain):
no fixture can hold it, however real it is, and the lane is a UNIT
TEST (§Sizing). DIRTY → cut the fixture; CLEAN → write the test.

How the split actually falls depends on how mined the take is. On the
first session to work this list (2026-08-18b, mc1l42 22 clusters → 1
and mc1l2 3 → 1) exactly ONE of ten closed laws was pair-visible — but
those two takes had already been driven to 1 and 3 graded rows by
earlier sessions, so what was left was, by selection, the residue no
pair could see. Expect the opposite mix on a FRESH recording: there
the pair-dirty families come first and the segmented list is mostly
their downstream noise.

⭐ **Attribution falls out of running it beside `verify-deltas`.** At a
break tick `t`, the pair diff `D1(t)` DIRTY means the error is LOCAL to
`t`; `D1(t)` CLEAN means it was INHERITED from earlier in the segment
through a field the obs schema does not grade — the first detector this
project has for the `+70`/`+52` obs blind spots.

`--classify` (both games) automates exactly that doctrine: at every
reset-CLUSTER HEAD it runs the pair `t-1 → t` on a scratch world
(`exec_pair` / `exec_pair_mc2`, the fixture semantics — pose pair,
measured terrain@t-1, the verify command law) and tags the cluster
`[LOCAL]` or `[INHERITED]` in the reset list, plus a summary count. It says which
branch every break is on BEFORE any code is written, and the LOCAL
heads feed the fixture cutter directly. Cost: one extra world build
plus one pair per cluster head.

⚠ **A reset restores entity state from the recording but TERRAIN from
the measured channel**, so a format-1 take (no terrain channel) resets
to PRISTINE planes and re-breaks immediately; the mode prints a warning
and the reset count is a capture artifact. mc1l32 is such a take
(42,925 "resets" — meaningless until it is re-recorded with terrain).

### The two instruments that attribute a CLEAN-`D1(t)` break

When a segmented break's pair diff is clean, the error was inherited
through a lane nothing grades, and these are how it gets named.

**`MGC_RAW_SHADOW=1` — every ungraded per-entity lane, in BOTH
runners** (`crates/mgc-conform/src/shadow.rs`; one implementation, by
construction). `EntObsMc1` carries 22 fields, `RetailEntMc1` carries
50-odd, and every field in the gap is a lane the recording HOLDS, the
importer RESTORES and the graded diff can never see. It started as
`+70`/`+71`/`+58`/`+44` — each of which paid for itself — and is now
all of `+26`…`site_z`, the six damage mailboxes and the tile links.

Run it in both modes, because they answer different questions:

- on `verify-deltas` the state is re-imported every tick, so a
  mismatch is a one-tick WRITE bug, attributable to the handler that
  ran;
- on `replay` the port has carried its own copy since the anchor, so
  the FIRST tick a lane parts is the first tick the port's HISTORY
  parts from retail's.

`MGC_RAW_SHADOW_ROWS=<path>` dumps every row as a TSV
(`t, slot, class, model, field, retail, port`) — the summary keeps one
example per `(class, model, field)`, which is the wrong resolution when
the question is WHICH TICK a lane first parts.
`MGC_RAW_SHADOW_LANE=<class>,<model>,<field>` is the in-place
magnifier: every row of exactly that lane prints to stdout as it
lands, no TSV round-trip. The census line itself carries the tick
span and the DISTINCT SLOT COUNT
(`45 rows t=1312..4200 across 1 slot(s)`) because "45 rows on one
slot" and "45 rows across 40 slots" are completely different leads
and used to print identically. Two lanes are handled
specially and both are load-bearing: slot-valued fields
(`+38`/`+40`/`+144`/mail sources) get the obs projection's own
`PLAYER_TARGET` untranslation, without which the sentinel alone is
~400,000 rows on mc1l2; and tile links that thread THROUGH the human
are skipped, because the port's carpet is not a pool record and no
chain of ours can match link-for-link there.

**`MGC_STATE_DUMP=<t>:<path>` on `replay` — the sectioned whole-world
dump** (`World::debug_state_sections`). The shadow covers everything
the RECORDING holds; this covers everything the PORT holds, which is
strictly more: the terrain planes, the tile heads, the wizard
registers, the THING table, the player column, `Gen::exhausted`. It
fires at the first tick at or after `t`, **anchor ticks included** — so
a run seeded at `t` dumps retail's imported state and a run that walked
there dumps the port's, and diffing the two names the state that
parted. A byte offset into a 400 KB blob is not an answer; a section
name is. (Format: one line per section, `name TAB bytelen TAB hex`. An
`Ent` inside the `ent` section is 143 bytes in the `Snap for Ent`
field order, after the Vec's 4-byte length header.)

**`MGC_TEAR_TRACE=<t0>:<t1>` on `replay` — why a boundary is called
TORN.** `recover::capture_clean_mc1` decides whether a boundary can be
graded at all, and it is a HEURISTIC (a `+63` step census plus the
one-step global-LCG test), not a record of missing data — it predates
the recorder's monotonic frame counter, when tick identity had to be
inferred. This splits the verdict into its two clauses and names the
suspects, which is the only way to tell a real tear from a false one.

⚠⚠ **AN ENTITY-POOL OVERFLOW LOOKS EXACTLY LIKE A TORN CAPTURE, BY
CONSTRUCTION** — `NewEvent` seeds `+63` from the slot index, so a slot
reaped and re-minted into the same `(class, model)` lands on precisely
the value its predecessor was walking, i.e. the tear signature. The
per-entity LCG `+4` is what settles identity (`NewEvent` re-seeds it),
and the census now skips a slot whose `rand` changed. Measured on
mc1l42: boundaries t=6612..6623 were ALL called torn on re-minted
`(9,9)` beam nodes while the global-LCG clause passed at every tick;
the recording is gapless and untorn. TORN went **105 → 0**, and the
take's first divergence moved from t=6624 (15/14 missing/extra, 1,486
field rows) to **t=6618, one row**. The general lesson: an instrument
that degrades exactly where the game gets busy will hide its worst bugs
under its own noise floor.

⭐ Worked example, mc1l42 t=6624: the shadow said the free run was
bit-identical to the recording on every modelled lane AND the free
stack at t=6623, `--start` bisection put the birth of the divergence in
tick 6618, and the state dump then showed `Gen::exhausted` jumping
0 → 74 in that single tick — i.e. the ENTITY POOL IS FULL and both
engines are dropping spawns. Neither of the other instruments could
have said that.

### What the PORT holds, what RETAIL changed, who WROTE it

The mc1l4 bucket[0] session spent its budget on two facts no
instrument could print: "what does the PORT's record hold at tick T"
(everything above either reads the recording or compares projections)
and "what changed in RETAIL across the break" (a divergence report
shows what DIFFERS, never what CHANGED — the cause of a break is
routinely state both sides share, which structurally cannot appear in
the diff). Three instruments close that gap; all three run on BOTH
families (the MC2 twins joined through `retail_ent_lanes_mc2` /
`World::port_ent_lanes_mc2` landed 2026-08-22f).

**`dump-state <take> <t> <slot>… --port` — the port-side state dump.**
Free-runs the world to `t` (the replay driver underneath; `--start
<t0>` anchors late, so `--start t-1` is the PAIR-IMPORT view) and
prints every lane of the requested slots side by side with retail's
record, `≠`-marked, joined BY LANE NAME through the shared table
(`conformance::retail_ent_lanes_mc1` / `World::port_ent_lanes_mc1`,
or the `_mc2` twins — the driver dispatches on the take's family).
ALL fields print, graded and ungraded alike; `—` = a lane the port
does not model (MC1: `f61`/`f62`, wizard `f132`, `f148`, bare `f48`;
MC2: per-CLASS, mirroring `import_ent_mc2`'s dual homes — a `—` on
one record can be a live lane on the next).
Representation merges are translated back into retail conventions
(the `PLAYER_TARGET` untranslation, `f58` as the unsigned byte, the
class-12 owner re-homed to `f42`, the castle transform sub-state on
the `f48` lane — where retail's pure-wait `4` legitimately reads as
port `1`). MC2 conventions: byte lanes print as the canonical
unsigned byte, `rand` compares the LOW 16 bits (retail's u16 stream),
the raw `flags` lane is always `—` — compare the translated-bit
sub-lanes (`flags.b0_done2`…`flags.b2_x20`) under it — and the
`f1a`/`owner28` pair is split back out of the fused `id24` per the
`obs_project_mc2` owner families. The free-stack tails print
alongside (MC2 adds the recycle stack — free-first pop order).
`--at-slot <n>` samples MID-WALK: the pool is snapshotted as the tick
INTO `t` reaches slot `n`, before `n` dispatches — "what did slot 71
hold when slot 388 ran", the question that was twice a hand-written
`eprintln!` + rebuild. The walk loop is game-shared, so this works on
MC2 unchanged (the mid-walk pose-phase family's native instrument).
The retail column stays the boundary state
(retail has no mid-walk sample); the header says so.

**`explain <take> <t> [<slot>…]` — retail's OWN t-1 → t changelog.**
Not a comparison: both endpoints are the recording's. Prints the
global deltas (LCG with the DRAW COUNT between endpoints, free/recycle
stack, spawn ordinals), then the pool filtered to TRANSITIONS —
records born / freed / died (life SIGN) / `+70` state / owner /
class — each with its full changed-lane list, then per-wizard deltas
(scalars + changed array elements). A level has few transitions per
tick, so the list is short by construction; castle 71's
`act_life 800 -> -1` at mc1l4 t=1017 is its first line, which is
exactly the fact three probes and a rebuild cycle were once spent
recovering. Named slots always print in full, plus every record they
point at (`+146`, `+52`/`+54`, `+144`, `+42`, `+38`/`+40`, the six
mail sources) — the pointer chase, automated. Needs adjacent state
records (a gap refuses honestly). The MC2 arm mirrors all of it:
transitions on `class3f`/life-sign/`action45`/`owner28`/identity/
collapse-mark, pointees through `@0x1A`/`@0x28`/`@0x24`/`@0x26`/
`@0x32`/`@0x34`/`@0x96`/`@0x94` + BOTH tile-chain words (`@0x16`/
`@0x18` — the recorded-order law's lanes) + mail sources, per-player
deltas (scalars, the nine spell/AI arrays, and the toast — the
in-closure cheat witness). ⚠ the MC2 DRAW COUNT walks the LOW-16
chain: MC2 mixes widths on the global rand (some draws write only
the u16 half — measured mc2l3 t=9→10, 8,336 low16 draws with no u32
walk reaching the endpoint), so a pure-u32 tick prints `draws=N` and
a mixed tick `draws=N, low16 …` (mod-65,536 ambiguous near the cap).

**`MGC_WRITE_TRACE=<slot>[:<field>]` — handler attribution, the 80 %
write barrier** (`World::tick_inner`). Snapshots the watched `Ent`
around every pre/post pass and every dispatch, diffs after, and
attributes each changed field to THAT pass or handler:

    WRITE t=1016 slot 71 act_life 800 -> -1  by carpet_dispatch (mc1 mail/flight/wizard/regen, slot 358)
    WRITE t=1224 slot 217 z 2157 -> 2201  by slot 217 (10,6) f70=6

Works under BOTH runners (`verify-deltas` stamps the pair tick into
`DEBUG_TICK`; `replay` always did). Without `:<field>` every lane but
the phase clock `f63` traces (its own dispatch steps it every tick);
naming `f63` explicitly traces exactly it. Rows go to stderr. The
writer label is the dispatched record's `(class, model, f70)` triple
plus the named pre/post passes (`tick-top reap`, `awake_pass`,
`carpet_dispatch`, `post-walk mail drains`, `tick tail`) — three of
five digs in the bucket[0] session reduced to precisely this line.
Zero cost when the variable is unset.

**`replay --brief` — the corpus regression sweep as one command.**
One machine-readable line per take:

    BRIEF mc1l4 mode=world terrain=measured end=14755 segments=1359 gaps=0 devs=1358 graded=14755 clean=13397 horizon=5375 first=5376 sig=extra(9,0)slot267x1

`horizon` = the last bit-exact boundary before the take's first
divergence (`END` when nothing diverged), `first` the divergence tick,
`sig` its compact signature (`(c,m)slot:fields` / `pose:lanes` /
`missing(c,m)` / `rng`), and under `--classify` a `local=/inherited=`
tail. A whole-corpus sweep is a loop over takes whose output DIFFS
against a saved baseline — the discipline the campaign already asks
for after every landed law, previously hand-rolled with shell loops
per take.

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
