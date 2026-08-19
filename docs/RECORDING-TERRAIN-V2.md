# Terrain channel — `.mgcr` format v2 plan

**Status: LANDED 2026-08-05 (steps 1-4 of the order of work; the terrain
channel is now a normative section of `docs/RECORDING.md`). REMAINING:
step 5 (player records the refreshed corpus — the recorder needs its
first live v2 take) and step 6 (the re-triage round, which needs that
corpus).** Landed shape:

- Plane locations: decompile-dug + fixup-table/memimage-corroborated for
  all three games (both agents' full reports are reflected in
  RECORDING.md's "Plane sources" paragraph and the recorder comments).
  Both engines: contiguous statics `type|height|shading|angle`
  (+0/+0x10000/+0x20000/+0x30000) — MC1/HW off `0xCC1E0`(A)/`0xCC1D0`(B)
  in the existing byte_99B58 frame; MC2 off `0x10B4E0 + MC2_DATA_DELTA`
  in the existing struct-anchored frame, ceiling `+0x40000` cave-only
  (MapType @struct+0x2FED4). No new needle needed anywhere.
- Recorder: planes ride the consensus/windowed capture (byte-stable
  across the same frozen window as the struct), differ emits base on the
  first emitted record + record-relative deltas (`--terrain-selftest`
  covers the emitter incl. the gap self-heal). Validation gates: MC1
  shading ∈ [28,47] (also the level-generated gate), MC2 height ≤200
  soft bound; failed pin = format-1 recording, never garbage.
  **The planes are not equal witnesses.** MC1 shading admits no
  tolerance — one cell of 65536 outside [28,47] fails — so it settles
  alignment AND generator-readiness on its own; the height bound is a
  soft 99% heuristic that exists for MC2, where nothing else checks
  alignment. `pin_terrain` therefore validates every plane and *then*
  decides: once shading has vouched for an MC1 frame, a height reading
  past the clamp is data, not misalignment, and the channel is kept with
  a warning. Height stays fatal for MC2. ⚠ The height bound is **not** a
  ceiling on how high a level may sit: building stamps are added
  unclamped, and seven low-lying MC1 levels measure exactly 0 cells over
  200 (max 169–192) while **mc1l32 — a maze in the sky, its base at the
  volcano-plateau cap with walls built on top — measures 1695 (2.6 %)**.
  That mis-calibration silently cost mc1l32 four takes, each degraded to
  format 1 and unrepairable after the fact (terrain re-anchors pristine).
- Format: format 2 (strict superset; v1 takes parse untouched),
  `channels.terrain` declaration, per-record `base_b64`/`delta_b64`,
  `mgc_formats::mgcr::{TerrainDecl, TerrainBlock, TerrainImage,
  encode/decode_terrain_delta}`.
- Harness: `check-decode` validates + accumulates the channel;
  `verify-deltas` (both families) installs the accumulated MEASURED
  height+type before every pair (`World::install_measured_terrain`,
  layered over the pristine restore so shading/angle/ceiling keep level
  values) with `--no-terrain` as the A/B; the fixture suite streams the
  same accumulator so suite pairs run on the terrain the triage graded
  them under (this subsumes the freeze-embedding idea — the suite
  replays from the recording anyway). Shading/angle/ceiling are
  captured-but-not-yet-installed (banked: install once a real take
  shows whether pristine bakes drift).

Original plan below for the remaining steps' context.

## Ruling context

- The terrain-capture family is the largest remaining number in the
  corpus (mc1hwl0 ≈2.17M unexplained field rows; `mc2-walker-ground-z`
  233,822 on mc2l24; `mc2-castle-pad-z` 166 real rows on l4; every
  guard/walker/ball terrain rule). The recording carries every terraform
  CAUSE but no terrain STATE, and the harness grades pair closures — so
  accumulated edits are invisible at import.
- **Importer carry-forward is a DEAD BRANCH (player-ruled 2026-08-05):**
  recordings contain GAPS (graphic overload and other recorder stalls).
  A carried-forward terrain loses every edit that fires inside a gap and
  silently poisons everything downstream. Do not revisit.
- **Decision: record terrain in the recorder, as deltas relative to the
  PREVIOUS RECORDED TICK** (not game-incremental edit events). A delta
  across a gap then simply contains everything the gap changed —
  self-healing by construction. Most records carry an empty delta;
  terraform windows carry tens of cells; volcano/doomsday storms carry
  hundreds. Size stays negligible next to the entity block.

## Recorder side (`tools/mc_dosbox_recorder.py`)

1. **Locate the level planes in guest memory, per game.** Heights are
   byte-per-cell (corpus math: `z = cell_byte × 32 (+ clearance)`), plus
   the tile-TYPE plane (the die-gate/paint laws read types). Recipe: the
   remc2 `Terrain.cpp` / remc1 globals name the arrays; anchor their
   data-segment offsets exactly like the existing lanes (the
   `base160`-style build-constant anchoring already proven for the
   entity pool). MC1HW is engine-identical to MC1 — expect the same
   offsets in HIDDEN.EXE, but verify (the HW fall-through trap).
   Deliverable: `plane_height_guest` / `plane_type_guest` (+ dims) per
   `MC1_BUILDS` / `MC2_BUILDS` entry.
2. **Read both planes every record** in the same parked window as the
   struct (`GuestMem.pread` over `/proc/<pid>/mem` — a 64–128 KB read is
   microseconds; bandwidth is a non-issue, unlike the old pipe-cadence
   worry that motivated sparse keyframes).
3. **Diff host-side against the previous RECORD's planes** and emit the
   changed cells. Record 0 of a take emits the FULL planes (the base
   image). Encoding suggestion: per plane, a count + packed
   `(cell_u16, value_u8)` runs; empty delta = count 0 (two bytes).
4. The tickpatch re-record round carries this (along with the PP_CASTLE
   recorder fix) — one re-recording session refreshes the whole corpus.

## Format side (`mgc_formats::mgcr`)

- Versioned optional block per record: absent on v1 takes (all current
  recordings still parse; the channel is additive).
- Decoder exposes `terrain_base` (record-0 planes) and per-record
  `terrain_delta`. A streaming consumer maintains the running image in
  O(delta) per record — the runner already streams records in order, so
  windowed runs get terrain for free while decoding the prefix.

## Harness side (`mgc-conform` + conformance import)

1. `retail_import_mc{1,2}` installs the accumulated MEASURED terrain
   before the pair runs (replacing the pristine plane + inference stack
   as the primary source).
2. **The reconstructions become sensors, not sources**: pad replays,
   riser endcaps, prop-z inversion stay behind flags and get compared
   against the measured planes on the new corpus — agreement certifies
   the inference; disagreement is a dig (either the reconstruct or the
   edit law).
3. **`cut_fixture_files.py` embeds each fixture's terrain state** (base +
   accumulated delta at the fixture tick, or a materialized plane) so
   suites stay self-contained and selected-pair execution stays O(pair).
4. **Roster re-triage** once the first v2 take grades: every terrain
   capture rule (`mc2-guard-terrain`, `mc2-walker-ground-z`,
   `mc2l24-ball-terrain-roll`, `mc2-castle-pad-z`, hw terrain bulk, …)
   gets re-measured against ground truth; retire what the channel
   explains, and what REMAINS is a real edit-law bug with a cell-level
   diff to dig at.

## Free instruments the channel adds

- **Record-0 planes = the stock-bake validator**: diff retail's t=0
  terrain against our baked `.mgcl`/generator output per level. This is
  the long-wanted decisive instrument for the MC2 creature die-off /
  roughness-fencing mystery (the v_16=20 block metric is
  height-sensitive; see the flocking memory).
- **Edit-law grading between records**: with measured terrain at r−1 and
  r, the port can replay pair (r−1 → r) and diff its OWN terrain writes
  cell-by-cell against retail's delta — terraform conformance, not just
  entity conformance. (The `--csv` per-family attribution extends with a
  terrain row kind.)
- Torn pairs keep their terrain deltas even though entity grading skips
  them — the terrain stream has no tear problem (planes are stable
  mid-walk except for the active edit, and the delta self-heals at the
  next record regardless).

## Order of work (next session)

1. Locate planes (3 games) — decompile + memimage corroboration.
2. Recorder: read/diff/emit + a `check-decode`-style self-test on a
   short local take.
3. Format v2 block + decoder + streaming accumulator.
4. Harness: import installation + freeze embedding + `--no-terrain`
   A/B flag.
5. Player records the refreshed corpus (tickpatch + PP_CASTLE +
   terrain, one session).
6. Re-triage round: roster + the reconstruction-vs-measurement audit +
   the stock-bake diff report.
