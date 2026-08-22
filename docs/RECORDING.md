# The `.mgcr` gameplay recording format

Version 2 — this document is normative. Any tool that reads or writes
`.mgcr` recordings should treat this file as the specification; changes
to the format must land in the same commit as changes to this document.
(Pre-1.0: no backward compatibility is owed to any earlier sample
recordings — player ruling 2026-07-29.)

Format 2 is a strict superset of format 1: it adds the optional
**terrain channel**. Readers accept both; format-1 takes simply carry
no terrain. Writers stamp `"format":2` when (and only when) the header
declares the terrain channel.

## Design goals

One format, three roles:

1. **Conformance ground truth** — a retail playthrough captured tick by
   tick from DOSBox ("what would retail do"), carrying the full raw
   state closure so it can be re-decoded forever as the field maps
   improve. A human has to play these; nothing may be lost at record
   time.
2. **Single-tick fixtures** — every adjacent tick pair (N, N+1) is an
   independent test: initialize the sim from the recorded state at N,
   apply the tick-N input, tick once, diff against N+1. Divergence at
   one tick never invalidates the rest of the run.
3. **Demos** — input-only recordings made by the port, replayed on the
   deterministic sim (`--replay`). Retail precedent: MC1's attract mode
   (`MOVIE/MVI00000.DAT`) is exactly this — an input recording replayed
   on the game's own deterministic engine. Tiny (no state channel);
   verified by the per-tick golden hash.

Channels are optional per recording; the header declares what is
present. Retail recordings verify by **state**; port recordings replay
by **input** and verify by **hash**.

## Container

A `.mgcr` file is a zstd-compressed stream of UTF-8 JSON lines
(inspect with `zstdcat`). Tools also accept the uncompressed `.jsonl`.
Line 1 MUST be the header record; every following line is a tick
record, in strictly increasing tick order.

Writers MUST serialize floats so they round-trip bit-exactly
(serde_json's shortest-round-trip encoding does). 64-bit hashes are
hex **strings** — JSON numbers are doubles and cannot carry a u64.

## The header record

Common fields:

```json
{"type":"header","format":2,
 "game":"mc1|mc1hw|mc2","level":3,
 "source":"retail|port",
 "tick_hz":24,
 "channels":{"input":"exact|raw|none","obs":true,"state":true,"hash":false,
             "terrain":{"planes":["type","height","shading","angle"],"dims":[256,256]}},
 "tool":{"name":"mc_dosbox_recorder","git":"<rev>"},
 "created":"2026-07-29T12:00:00Z"}
```

`channels.terrain` (format 2, optional) declares the measured terrain
planes: `planes` names them in the order they appear in every terrain
blob; `dims` is `[width, height]`, shared by all of them. Absent =
the recording carries no terrain channel.

`source:"retail"` adds: `"build":"A|B"` (CARPET.EXE / HIDDEN.EXE
address half), plus free-form capture provenance (DOSBox version,
cycles). The `capture` object also carries `tear_gate` (bool: emit-time
inter-tick gating ran) and, for a tick-patched exe,
`window_gated: true` with `exe_patch: {mailbox_guest, spin_period_counts}`
(see "Tick-patched capture") — where each `t` is the stub's
authoritative sub-step counter.

`source:"port"` adds `"sim"`, the **sim-config closure** — everything
that feeds the state hash, pinned so `--replay` can refuse (or
force-apply) a mismatched environment:

- `thrust_model`, `altitude_model` — the flight tiers are **sim
  physics**, not presentation (`Simulation::thrust_model` doc:
  *"fixed per run; replay headers must record them once replays
  exist"*; DEVIATIONS.md "enhanced flight": *"Selected once at the sim
  boundary; replays record it"*).
- `snapshot_version`, pool sizes (the pool size feeds the hash) —
  `entity_pool_size` and `awake_range`, present only when the take was
  recorded with the override. Chassis geometry is decided BEFORE the
  world exists, so the replay has to read them from the header; by the
  time the start snapshot's identity block would catch a mismatch it
  can only refuse.
- `patches` — the retail-bug patch policy (`gameplay · patches`,
  DEVIATIONS.md "Patch options"). `--record` forces every patch to its
  RETAIL arm for the whole session and stamps `"patches": "retail"`;
  `--replay` pins to the recorded policy. A port header WITHOUT the
  key predates the option class (2026-08-08) and replays under the
  legacy hard-wired set (`GameplayPatches::legacy()` — the sim those
  takes were recorded against). Retail-source takes always pin the
  retail arms.
- every sim-reaching option from the options registry, including
  sim-affecting dev instruments (e.g. `dev.lift_unclamped`);
  presentation-only options are excluded and never recorded.
- RNG seed(s) and level/campaign provenance; for mid-level starts an
  embedded start snapshot (`"start_mgcs_b64"`), otherwise the pristine
  level is the tick-0 state.

## Tick records

```json
{"t":N, "input":…, "obs":…, "state":…, "hash":…, "terrain":…, "wallclock":…}
```

**Phase convention:** the state-bearing channels (`obs`, `state`,
`hash`) describe the world **at** tick N (t=0 = the initial state);
`input` is the input **consumed by the tick that advances N to N+1**.
Replay therefore reads record N, applies its input, steps once, and
checks against record N+1.

### `input` — the sim-boundary input, per player

- Port (`channels.input:"exact"`): the serialized `FlightInput`
  superset — **both** encodings (the classic virtual stick
  `stick_x`/`stick_y` and the enhanced float axes
  `thrust`/`strafe`/`lift`/`yaw_delta`/`pitch_delta`), casts, equips,
  `full_stop`, and any other sim-reaching verbs. The Rust type is
  normative by reference; its serialization is versioned by `format`.
  Recording both encodings keeps the stream *mechanically* feedable to
  either thrust model (see Cross-model replay below).
- Retail (`channels.input:"raw"`): the persistent externals sampled at
  the tick boundary — held scancodes, mouse cursor and held buttons.
  On MC1/HW they live in the separate static frame; on MC2 they are
  the `ReadGameUserInputs` register block in the struct's own data
  image (held state = the "2" registers @0x18074C/0x18074A, press
  LATCHES @0x180746/0x180744, live cursor @0xE3760, the game's own
  cursor-at-press snapshot @0xE375C, `pressedKeys` ×128 — remc2 named
  VAs; the runtime frame sits at −0xB0E98 from them, anchored on the
  located struct and validated by the control-mode word + keybind
  table; see the recorder's `MC2_BUILDS`). MC2's `ext` adds
  `latch_b64` and `press_b64` to the four standard register blobs.
  This is an **approximation**: the in-struct 10-byte control command
  is consumed and zeroed mid-tick, so a click shorter than one tick
  can be missed or land ±1 tick — MC2's press latch narrows this (it
  is set at the press edge and survives to the release), and the
  press-position pair records the aim the cast actually used. Raw
  input is advisory; retail recordings are validated by state, never
  by replaying input. (MC2 takes recorded before 2026-07-30 predate
  the MC2 register map and carry `input:"none"`.)

  **The consumed move/fire byte outranks the externals.** Both games
  also RECORD the consumed per-tick command byte inside the state
  closure (`Type_160`/`Type_str_164 dw_0`: bits 1/2 speed, 4/8
  strafe, 0x10 left fire, 0x20 right fire) — the byte retail's own
  tick acted on, stamped by the consume loop (MC1 post-pass, read at
  record N; MC2 in PlayerEvents, read at N+1). Corpus-measured
  against retail's own arms: MC1 2,368/2,368 single-shot casts at
  exactly byte-record +2, MC2 560/560 arms on the same record — so
  consumers that need exact input (the pose channel, `replay`)
  read the byte and the ±1 caveat below applies only to the raw
  externals.

  **MC2 phase caveat — the snapshot straddles retail's poll, and the
  latch resolves it.** The recorder parks in the settled tail of frame
  `r` (after the entity pass, before frame `r+1`'s `PlayerEvents`), so
  a press visible at record `r` may have been consumed by frame `r`'s
  own poll *or* still be pending for frame `r+1`. The press LATCH is
  cleared the instant `HandleMouseButtons_18F80` consumes it, so a
  latch still up at the snapshot means "not yet polled": the input
  frame `r` actually consumed is
  `held(r) && !latch(r) || latch(r-1)`, and THAT is the stream a
  consumer must feed to the tick that advances `r-1` → `r`. Measured
  against retail's own arm ticks over the whole MC2 corpus: 4,814 of
  4,815 right-hand casts land on its rising edge with zero offset
  (`mgc-conform`'s `verify_mc2::align_cmd_mc2`). Consumers of the MC1
  `input` channel have no latch register and keep the ±1 caveat.

Retail's own multiplayer lockstep puts exactly the per-tick 10-byte
control commands on the wire — the "consumed command per player per
tick" unit is retail's own canon, and this channel is deliberately
shaped like it.

**Cheats: the witness is the toast, not the key.** Both engines expose
a cheat menu on control opcode 30 (`0x1E`, `param1` = sub-code 1..7 on
MC1, 1..10 on MC2; ALT+F-key in both). It is the one recorded verb
that MUTATES the world instead of steering it, so a free-running
consumer must apply it or diverge permanently from that tick. It
cannot be read off the control slot — retail memsets the 10-byte
command in the same event pass (remc1 :49044), before any capture
window opens, and opcode 30 appears **zero** times in either cheat
take. The raw key channel does see the F-key but carries the ±1-tick
caveat above and cannot separate a held key from a repeat.

The handler's OWN on-screen message settles both: it names the cheat
and it re-arms a lifetime counter that otherwise only counts down, and
it lands in the per-player block INSIDE the state closure (MC1 text at
wizard `+28 + 68·i`, counter at `+64`; MC2 text at block `+0x1C`,
counter at `+0x4D`). A cheat fired iff the counter INCREASED across
the pair and the text matches a handler string — repeats are dated by
the counter alone, since the text does not change between them.
Measured: mc1l0-test 23/23 fires and mc2l0-test 103/103, each matching
a key press edge 1:1, zero misses and zero false positives
(`mgc_formats::recover::Cheat`).

PHASE is per-game and both arms are corpus-dated
(`engine::world::cheats`): MC1's handler runs in `DrawAndEventsInGame`'s
command pass AHEAD of the tick function whose stub holds the capture
window, so its writes are visible at `t=N` UN-TICKED — the port applies
it at the TAIL of the pair tick. MC2's rides `PlayerEvents` INSIDE the
frame the recorder samples the tail of, so its mints do tick that
frame — the port applies it at the tick TOP, beside the MC2 respawn.
Port takes carry the sub-code in `PortInput.cheat`, so a `--replay
--record` transcode of a cheated take still reproduces itself.

### `obs` — the shared observable projection

The decoded, human-greppable view: RNG word, wizards/players, control
slots, active entities with their gameplay fields — the same schema
whether decoded from retail memory or emitted by the port, so one
comparator serves retail-vs-port and port-vs-port. All values are
exact integers or exactly-round-tripping floats; comparison is
equality, never tolerance.

### `state` — the raw retail closure (retail only)

The full master-struct image, base64 (`"struct_b64"`; ~227 KB MC1/HW,
~220 KB MC2 — includes the pool, the per-wizard/per-player AI columns,
the control array, the RNG word, and the embedded pristine level
record; retail's own in-level save writes this exact MC1 struct with a
single `fwrite`, so the image is the game's own idea of its closure),
plus on MC1/HW the external input registers from the static frame
(`"ext"`: `keys_b64` pressed-scancode array, `cursor_b64` mouse cursor,
`lbtn_b64`/`rbtn_b64` held buttons — raw register bytes). The static
frame sits outside the consensus window, so `ext` carries the same
±1-tick attribution caveat as the `input` channel.
Consecutive images are nearly identical, so zstd collapses the channel;
no delta scheme is needed. This channel is the fixture-initialization
source and the licence to improve field maps after the fact. The
closure is *believed* complete; a delta-verify failure that survives
triage is the detector for state living outside it.

`wallclock` (retail): the free-running ~120 Hz PIT clock — a liveness/
ordering signal only, never part of the closure.

### `terrain` — the measured terrain channel (format 2)

The live terrain planes, read from guest memory in the same settled
window as `state` and recorded **relative to the previous record**:

```json
{"terrain":{"base_b64":"…"}}          // the take's FIRST record only
{"terrain":{"delta_b64":"…"}}         // any later record with edits
```

- **`base_b64`** — the full plane set at the first recorded tick: the
  declared planes concatenated in header order, each `width × height`
  bytes, verbatim guest-linear layout (cell index = the plane's linear
  byte offset). This is the t≈0 image — it doubles as the stock-bake
  validator (diff against the port's generated level terrain).
- **`delta_b64`** — per declared plane, in order: a `u32` LE count,
  then `count × (u16 LE cell, u8 value)` — the cells that changed
  since the **previous record** and their new values. An absent
  `terrain` key = empty delta (nothing changed). A record after a `t`
  gap simply carries everything the gap changed — the channel is
  self-healing by construction, which is WHY deltas are
  record-relative and not game-event-incremental: recorder stalls
  can never lose a terraform. Decoders MUST reject truncated blobs,
  trailing bytes and out-of-range cells outright (a torn blob never
  half-applies).

The channel describes the world **at** tick N (same phase convention
as `obs`/`state`). A streaming consumer maintains the running image in
O(delta) per record (`mgc_formats::mgcr::TerrainImage`); a consumer
that starts mid-stream without the base may still accumulate deltas
but must not treat the planes as absolute (`TerrainImage::based`).
Torn/excluded pairs keep their terrain deltas — planes are stable
mid-entity-pass except for the active edit, and the next record's
delta re-syncs regardless.

**Plane sources (both engines, decompile-verified 2026-08-05):** the
planes are CONTIGUOUS static arrays in guest memory, captured in
their guest order `type | height | shading | angle` at block offsets
+0/+0x10000/+0x20000/+0x30000. MC1/HW: base `mapTerrainType` guest
`0xCC1E0` (build A) / `0xCC1D0` (build B, dual-suffixed), reached via
the recorder's byte_99B58 static frame; MC1 shading is hard-clamped
to [28,47] by every retail writer — the recorder's alignment AND
level-generated gate — and is NOT derivable from height (flat cells
take an LCG roll at bake), which is why it must be captured. MC2:
base `mapTerrainType_10B4E0` through the struct-anchored data frame
(named VA − 0xB0E98), plus the cave-only `ceiling` plane
(`x_BYTE_14B4E0`, +0x40000) appended to the declared list **only when
the level's MapType (struct+0x2FED4) is Cave** — retail never writes
it on Day/Night levels, so off-cave it holds BSS residue, not
terrain. Cell = `tile_y*256 + tile_x`; world z = `height[cell] × 32`
(floor and ceiling alike).

Size: empty deltas are 4 bytes per plane before compression;
terraform windows tens of cells; volcano/doomsday storms hundreds —
negligible next to `state`.

### `hash` — the port verification channel (port only)

The golden state hash at tick N, as a hex string. Inputs + hashes is
full byte-exact determinism verification at a few dozen bytes per
tick — and it is the desync checksum retail's lockstep never had, so a
future multiplayer inherits it unchanged.

## Gaps

Recorders SHOULD emit gap-free streams (lower DOSBox cycles until they
do). A jump of k>1 in `t` is legal but breaks the fixture pairing
across it; runners count and report pair coverage.

The known gap mechanism on a tear-gated recorder is a SIM-DOMINATED
stretch: whenever the guest's cycles are spent inside the entity pass,
every DOSBox park lands mid-tick and no clean boundary is exposed —
those ticks are unrecoverable by sampling, whatever the poll rate. Two
flavors:
- LOAD-shaped: sim logic swells (ambient spawn storms, heavy combat)
  or host stalls (audio buffer pressure) eat the budget. Mitigations:
  raise cycles until the game reaches its frame cap, raise the GAME's
  render load (its SVGA mode — render cycles never touch the sim
  struct, so render-bound frames are wide capture windows), bigger
  mixer buffers or sound off.
- STRUCTURAL, fixed-length: full-screen flash/fade sequences (big
  explosions, the level-start fade) draw almost nothing for ~9-10
  frames, the frame collapses to sim+flip, the game momentarily runs
  FAST, and the renderer capture window vanishes — a deterministic
  ~9-tick gap that no cycles/render/sound setting can remove. These
  are exactly the transition-dense ticks fixtures want. The structural
  fix is the tick-patched exe (below): it makes the game pace itself,
  so a quiescent window exists every sub-step regardless of render
  load, closing both gap flavors at once.
The recorder must classify mid-tick parks (including the early-cursor
case, where the tick-top LCG has drawn but the +63 mode still reads
0 — indistinguishable from "same tick" without the RNG check) and
report the loss LIVE, per pending tick, not only as a bare `t` jump
discovered afterwards. (A tick-patched exe removes the guesswork —
see "Tick-patched capture".)

## Capture tearing (the inter-tick gate)

Read-consensus (N byte-identical reads of the volatile ranges) proves
only that the guest was FROZEN — DOSBox regularly parks
**mid-entity-loop**, so a consensus image can be a mid-tick state:
entities below the loop cursor already stepped, entities above not,
and the global LCG possibly not yet drawn. On the first recorded
corpus ~75% of MC1 snapshots were mid-pass; the artifacts masqueraded
as sim findings (a "12.5% RNG stall", an "asleep set" of
+63-frozen entities) until the fixture runner proved the stepped
set always formed one contiguous slot band — the loop cursor.

The MC1/HW law: a snapshot pair is a true inter-tick pair iff every
persisted entity's `+63` clock advanced by exactly `dv` (retail's
dispatch table is static; every live state row ticks) AND the global
LCG advanced exactly `dv` steps (one draw per sub-step). Recorders
MUST enforce this at emit time (`pair_clean`). Deviant
discrimination: only steps of exactly `dv±1` count as tear suspects
(the cursor-band signature — one pass short or long); arbitrary-step
deviants are ambient spawn CHURN (slot re-use overwrites `+63` with
the spawn ordinal — constant on HW's weather families, and a flat
deviant cap starves the recorder there). Headers stamp
`capture.tear_gate: true`; recordings
without the stamp carry torn states, and fixture runners MUST
re-classify their pairs with the same test and exclude torn ones from
conformance verdicts.

The MC2 law (measured on the mc2l0 corpus, 2026-07-30) is different:
neither Turn continuity nor LCG-step parity discriminates. Retail's
frame order is `PlayerEvents` (Turn++) → `UpdateEntities` (one
unconditional LCG top-draw, then the slot-order dispatch), so a
DOSBox park between the two yields a snapshot whose Turn has advanced
but whose entities have not — Turn delta is +1 on EVERY adjacent
pair, torn or not, and the draw count per tick is activity-dependent
(0..16+, mode 1) with most frozen pairs still showing one draw. The
working discriminator is the per-entity phase byte `byte_0x3E_62`
(incremented once per handler run, per entity, per pass): a true
inter-tick pair is **step-1 dominant** over the entities live at both
ends (`d1 ≥ max(d0, d2)`, deltas taken mod 256 with values outside
{0,1,2} ignored as animation wraps). A Turn-side park produces an
all-0 pair (positions frozen; measured moved-fraction 0.04) followed
by an all-2 pair — ~30% of mc2l0's pairs. The runner applies this
gate from the raw states (`mgc-conform`'s `capture_clean_mc2`);
recorder-side emit gating for MC2 is still open work.

The FIRST record has no pair to gate it, so recorders MUST NOT write
it unvetted (a mid-tick anchor rejects every later pair against it and
starves the stream): hold the candidate and flush it only once the
first clean pair vouches for it, replacing the anchor with the newer
read whenever a bootstrap pair is rejected.

## Tick-patched capture (windowed)

The tear gate is a *reconstruction* — it infers, after the fact,
whether a frozen snapshot happened to land between ticks. The exe
tick-patch (`tools/mc_exe_tickpatch.py`) removes the inference by
making the game cooperate. It installs a 249-byte wrapper stub around
the per-sub-step tick function (remc1 `sub_41780_41AC0`) of a COPY of the
binary — `CARPET_REC.EXE` / `HIDDEN_REC.EXE`, never the pristine
gamedata — by redirecting the tick fn's callers (rewriting each
gameSpeed-fanout `call`'s 4-byte rel32) so they enter the stub, which
paces, then `call`s the original untouched tick fn and `ret`s. The
function entry stays byte-for-byte intact (an earlier version overwrote
the entry with a detour, which decoded as a wild `add eax,[eax]` when
the dynamic recompiler picked the region up misaligned). Every sub-step
the stub does two things:

1. **Paces to a wall-clock deadline.** It spins on the game's own PIT
   counter (measured live at ~120 Hz) until one period (default 5 counts)
   has elapsed since the last release, so `fps = 120 / period` ≈ **24 fps**
   at period 5 — the authentic Magic Carpet rate — regardless of how high
   DOSBox `cycles` is set; the excess cycles are burned in the spin. Exactly
   **one sub-step per rendered frame** is paced (the first, detected via the
   gameSpeed fan-out's live loop index in `EBX`), so the F3 game-speed feature
   still speeds the *sim* up 4×/16× while the frame rate holds; at the default
   speed of one sub-step per frame every sub-step is the first. (Both
   obj1's cave and obj3's mailbox must be page-aligned via their `vsize`
   fields, or the tail is outside the segment limit — the code cave won't
   execute and the mailbox writes won't persist.) This is the frame cap
   retail never had; it
   is a *presentation* throttle only. MC1's sim is wall-clock
   independent (its lockstep multiplayer proves it: the PIT counter
   feeds render/animation timing, never sim state), so pacing changes
   *when* sub-steps run, never *what* they compute — the recorded tick
   sequence is byte-identical to an unpaced run.

   **Floors the window** (`--floor N`, default 2 counts, `0` disables).
   A deadline is only as good as the compute fitting inside it: a sub-step
   heavy enough to overrun its period — deaths, several meteors at once —
   arrives with the deadline already passed, the spin falls straight
   through, and `in_window` is raised and cleared within a handful of
   instructions. That zero-width window is unlandable, so the recorder
   drops the frame and the delta across it tears; and because the release
   path tolerates up to 30 counts of backlog with no wait at all, one
   heavy sub-step is followed by a burst of free-running ones. The floor
   clamps `deadline = max(deadline, now + N)` before the spin, making the
   window's width independent of load and deleting the catch-up burst with
   it. It is a **no-op while the game keeps up** (steady-state waits are
   unchanged) and is charged only to the sub-steps that already overran —
   unlike lowering `--period`, which taxes every frame. The PIT counter is
   integral, so `N=1` guarantees nothing and `N=2` is the smallest value
   guaranteeing a full count (≥8.3 ms); the tool rejects `1`.

2. **Publishes a mailbox** in obj3's committed tail (guest-linear
   `0x132c40`, same address in both builds; the stub derives obj3's real
   runtime base from the game's own relocated struct pointer so its writes
   stay in obj3 and never corrupt game memory): an 8-byte magic
   (`MGCTTIK1`), a monotonic sub-step counter (`+8`), and an
   `in_window` flag (`+0xC`) raised for the whole spin. The spin *is*
   the quiescent window — the world struct is fully settled from the
   previous sub-step and the current one's LCG draw has not begun — and
   it is proportional to the spare cycle budget, so on a fast host it is
   ~7 ms wide on a typical sub-step. On a sub-step with *no* spare budget
   the width is whatever `--floor` guarantees (≥8.3 ms at the default 2)
   rather than zero, which is what makes "every sub-step, bursts included"
   true rather than aspirational.

A recorder that finds the magic switches to **windowed capture**: take
the struct only while `in_window==1`, require the counter and struct to
stay put across the consensus reads, and use the counter's delta as
continuity. This is strictly stronger than the tear gate (a
between-tick window is guaranteed by construction, not inferred) and
`t` is the stub's authoritative sub-step index, not a `+63`-mode
estimate. Such recordings stamp `capture.window_gated: true` and
`capture.exe_patch: {mailbox_guest, spin_period_counts}`; consumers may
treat window-gated snapshots as tear-free without re-running
`pair_clean`. To avoid re-scanning a window it has already captured, the
recorder reads only the 8-byte mailbox first and pulls the full struct
only when `in_window==1` **and** the counter has advanced past the last
emitted frame.

### MC2 / NETHERW arm (signal-only)

MC2 already frame-limits itself — `InGameLoop_47320` runs the whole frame
(`DrawAndEventsInGame_47560`: `PlayerEvents`→ entity pass → draw) then
spins `while (before+5 > GameTimerTurn)` until 5 timer ticks elapse. So
MC2 takes are gap-free, but ~33 % are **torn**: DOSBox can park the guest
between `PlayerEvents` (`Turn++`) and the entity pass — a settled-looking
but mid-frame state (the phase-byte law above). The `NETHERW_REC.EXE` arm
therefore adds **no pacer**; it only *signals* the true boundary. It
redirects the loop's sole `call DrawAndEventsInGame_47560` to a wrapper
that clears `in_window` (the frame is about to mutate), calls the original
frame driver, then bumps a monotonic **per-frame** counter and raises
`in_window`. The flag is thus up from just after the draw, across MC2's
native limiter spin, until the next frame's `Turn++` — a settled window,
so the `Turn++`-park tear is unobservable by construction.

MC2's native budget is **absolute** (`turn_sampled_before_the_frame + 5`
ticks of the 100 Hz PIT), so a frame heavy enough to overrun it leaves no
spin at all and the window collapses to nothing — the same failure the MC1
pacer has, arriving by the same route. The MC2 stub therefore carries the
same **`--floor N`** (default 2 counts, `0` disables): after the counter
bump and the `in_window` raise, it spins on `GameTimerTurn` until N counts
have passed, so the total window is `floor + max(0, native spin)`. Placing
it in the tail — *after* the counter bump — means the window the recorder
sees announced as fresh is the same one being held open. `--pace N` (which
widens the absolute budget) remains available but is now the second-line
knob: it taxes every frame and still cannot guarantee a window on a frame
that blows the wider budget too. The mailbox
(magic `MGCTTIK2`, counter `+8`, `in_window` `+0xC`; **no period field**)
sits in obj3's committed BSS tail (guest `0x1842c0`); the stub derives
obj3's real base by reading the game's own fixed-up `GameTimerTurn` disp,
and both `vsize`s are page-aligned (same segment-limit requirement as
MC1). Continuity is the counter's delta — **never** the per-player `Turn`,
which advances mid-frame inside `PlayerEvents` and so cannot gate the
tear. Window-gated MC2 recordings stamp `spin_period_counts: null`. Old
tear-gated MC2 takes are unaffected; only RE-RECORDED takes get the
window (retiring the per-entity torn-slot exclusion). The tear gate
remains the path for any unpatched exe.

## Consumers

- **`--replay <file>`** (the game; LANDED): SOURCE-AGNOSTIC — one
  flag plays both arms (player-ruled):
  - **Retail takes** (`source:"retail"`, state channel required):
    inline input recovery (the shared laws in
    `mgc_formats::recover` — the consumed move/fire byte fed to the
    movers verbatim via `FlightInput::mc1_move_byte`, the inverted
    stick filter, hand equips/rebinds, the respawn witness, the cheat
    toast), world seeded by `retail_import_*` at the first closure,
    gaps re-anchor fresh segments. PURE replay: divergence is graded
    at every capture-clean boundary (the pose channel's lane set) and
    reported, never corrected. A recorded cheat with no port handler
    is REPORTED as such — it guarantees divergence from that tick, and
    an unexplained wall is worse than a named one.
  - **Port recordings** (`source:"port"`, `input:"exact"`): pins the
    header's sim closure (tier tags applied; a foreign
    `snapshot_version` is a refusal, not a warning), restores the
    embedded `start_mgcs_b64`, feeds the input channel and asserts
    the hash channel live.
  Either way the HUD carries a bit-exact / "diverged since t=N"
  counter and a translucent GHOST billboard rides at the recorded
  pose (retail takes) — a mid-demo desync is surfaced on screen,
  never silently absorbed. Playback speed is a viewer control (F3;
  presentation only); per-tick semantics are invariant.
  **`--replay-check <file>`** is the headless twin: whole take, drift
  summary on stdout, exit 0 only on zero divergence. Its retail
  results are certified against `mgc-conform replay`'s (identical
  first-divergence boundaries on mc1l0 t=563 / mc2l3 t=244,
  2026-08-07).
- **`--record <out.mgcr>`** (the game; LANDED): write the running
  session as a port recording — `source:"port"`, `input:"exact"`
  (`mgc_formats::mgcr::PortInput`, the serialization mirror of the
  sim's `FlightInput`), hash channel on, and the start state embedded
  as `start_mgcs_b64` (a pristine level boot is just the t=0 special
  case, so mid-level and campaign starts replay exactly). Writer =
  `mgc_formats::mgcr::RecordingWriter` (zstd JSONL, `.jsonl` stays
  plain). Recording ends with the session (level switch or exit
  finalizes the stream).
  The header also carries the OFFLINE chassis overrides the session
  ran with — `entity_pool_size` and `awake_range`, written only when
  overridden — and `--replay` builds its world from those rather than
  from the replaying run's own CLI/config. They cannot be recovered
  any later: the start snapshot's identity block opens on
  `chassis.pool_slots` / `chassis.awake_gate_sq`, so a world built at
  any other size REFUSES the snapshot ("snapshot is for a different
  world"), which is what made a `--pool-slots N` take unreplayable by
  every invocation including the one that recorded it. A take from
  before these keys reads as "not overridden", i.e. the faithful
  default it was recorded under.
- **`--replay <in.mgcr> --record <out.mgcr>`** (LANDED) — re-record a
  take as an INPUT-ONLY port take, for sharing. The combination used to
  be refused. A retail take is ~500 KB/tick and nearly all of it is the
  two channels the replay never hands to the sim: `state` (61%) and
  `obs` (39%); the tick's player input is 0.0% of the file. It is NOT
  the `input` channel either — that holds the raw DOSBox externals
  (`keys_down`/`mouse`) that retail's consume loop filters and latches
  before the mover sees them, so the crop cannot be a channel filter.
  But a replay ALREADY recovers the exact input every tick and
  `--record` already writes exactly that format, so letting them meet
  is the whole feature. Measured 104-264x smaller. `--replay-check
  <in> --record <out>` is the headless twin; `tools/strip-recordings`
  batches it and VERIFIES each output by replaying it.
  Two rules make the result reproduce, both in `begin_replay_recording`:
  1. **Start after the anchor.** A retail take seeds the world from its
     first closure inside the driver's first `next`, so a snapshot taken
     at session install captures a pre-seed world and the take desyncs
     on tick one.
  2. **Carry the import pin** (`World::import_pin`). The seeding is an
     import, and an imported world holds config/state the snapshot
     deliberately skips — `strict_retail`, `measured_terrain`, the
     carpet slot, `castle_reg`, and the rest of the residue. Missing
     one shows up as a take that cannot reproduce its own hash channel
     (mc1l0 without it: 17 ticks; with `strict_retail` only: 59; with
     the pin: all 7,097). Grow the list as more turn up — the recipe is
     to read the divergence tick's INPUT record, which names the event
     that first touched the missing lane: mc1l4 desynced at t=105 and
     t=104 is its first `fire_left`, which is `wiz_charge` (stamped
     onto the manifestation's `f26` at spawn, so invisible until a
     spell is actually cast). Note that enumerating the importer's
     writes needs indexed and nested assignments too, not just
     `self.field =` — `wiz_charge` is written as `self.wiz_charge[i]`.
     STILL OPEN: mc1l1 desyncs at t=2850, which is a `demolish`
     (`mc1_move_byte: 48`) — so the lane is something
     `World::player_castle` resolves through, the `wizext+50`/
     `castle_reg` side. `castle_reg` itself is already pinned and reads
     all-zero there, so it is one hop further out.
  A re-recorded take is NOT a conformance fixture: the retail channels
  ARE the oracle, and an input-only take has nothing to grade. Keep the
  originals; `mgc-conform` reads those.
  Multi-segment takes are out of scope by ruling — a re-anchor is a
  capture gap that input alone cannot cross, so the recording stops
  there and says so (all takes should be single-segment; fix the rig,
  not the consumer).
- **Puppet playback** (any recording, retail included): drive the
  recorded poses through the renderer with **no sim** — watch the
  actual retail run inside the port. Presentation styling is free
  here: e.g. enhanced-style banking is a pure function of turn rate ×
  forward speed, both recoverable from the pose stream, so a retail
  run can be *shown* banking into its curves without touching physics.
- **The fixture runner** — `mgc-conform` (crates/mgc-conform):
  - `check-decode` (any recording): re-decode every tick's raw
    `state` through the Rust decoders (`mgc_formats::mgcr`) and
    demand value equality with the stored `obs` channel — pins the
    Rust decode against the recorder's.
  - `verify-deltas` (retail; MC1/HW and MC2 wired): for each
    adjacent tear-gate-clean pair, import the raw `state` at N onto a
    pristine-built world (`World::retail_import_mc1` /
    `World::retail_import_mc2` — pool slot-for-slot incl. hidden
    state, the LIVE free-stack order, globals, the human column
    routed outside the pool), tick once with **pin-the-human** (the
    recorded carpet pose drives `World::tick`, so world fidelity
    verifies with zero dependence on input reconstruction), and diff
    the port's obs projection (`World::obs_project_mc1` /
    `obs_project_mc2`) against the recorded `obs` at N+1. The MC2
    arm additionally excludes per-entity-torn slots (phase-byte
    delta ≠ 1 inside an accepted pair) from field comparison.
    Reports: fixture-grade vs torn pair counts, per-tick LCG
    draw-count histogram, the +63 phase-clock table, entity-set
    events by (class, model), and per-field mismatch counters with
    examples. `--pin-pose n|n1`, `--input-delay k` (cast
    reconstruction from the raw input channel — MC1 only; the MC2
    arm derives the cast phase from the press latch instead, see
    below), `--dump t`.
    A deviations allowlist keyed to DEVIATIONS.md entries is still
    open work.
  - `extract` / `fixtures` — the FIXTURE SUITE (docs/CONFORMANCE.md):
    lift triaged pairs into a committed manifest
    (`conformance/*.json`, expected status per pair) and replay them
    as an automated expected-status test on every `cargo test`
    (crates/mgc-conform/tests/suite.rs; skips when the recording or
    baked tree is absent).
  - `replay` (retail): PURE INPUT REPLAY (docs/CONFORMANCE.md "The
    replay verifier") — seed the world ONCE from the first closure,
    then free-run feeding only the input stream recovered from the
    recording (the consumed move/fire byte, the inverted stick
    filter, hand equips, the respawn key); divergence is reported at
    every recorded boundary and never corrected. A `t` gap re-anchors
    a fresh segment, so gap-free takes replay as one unbroken chain —
    recorders should keep striving for gap-free streams.
  - `verify-replay` (port): init from header, feed inputs, compare
    the hash at every tick — LANDED as the game's own
    `--replay-check` (the port arm of the source-agnostic `--replay`
    above; the round-trip is pinned by
    `mgc-app replay::tests::port_record_replay_roundtrip`).

## Cross-model replay (sandbox, not replay)

The input channel carries both encodings, so a recording *can* be fed
to a sim configured with a different `thrust_model`/`altitude_model`.
This is a **sandbox**, not a reproduction: the flight tiers are
different in-sim physics (chase-the-pointer steering vs. the retail
stick law; hold-to-fly drag vs. the speed-target chase; crosshair-lead
casting vs. hull-heading casting), and the pilot flew closed-loop
against one of them. Expect the trajectory to diverge within seconds
and compound. Tools MUST void the hash channel and mark the session as
non-verifying when the header's models are overridden.

## Size expectations (non-normative)

Input-only demo: tens of bytes/tick — minutes of gameplay in tens of
KB. Full retail capture: ~230 KB/tick raw before compression; the
between-tick redundancy lets container-level zstd absorb the channel
(measured ~20 KB/tick under an adversarial synthetic worst case —
incompressible base image, fully random 2 KB/tick churn; real structs
are mostly sparse and churn is clustered, so expect better). The
decoded `obs` channel is ~170 KB/tick uncompressed JSON; it exists for
greppability and comparison, not economy.
