# tools

Pinned, run-once oracle tools that are not part of the engine.

## mc2-genlevel

Standalone MC2 terrain generation: the original algorithm (diamond-square
fractal + rivers + surface typing), carved **verbatim** out of remc2
(`vendor/`, see `mc2-genlevel/vendor/PROVENANCE.md`) with a thin shim
header and CLI around it. `mgc-import bake` invokes it per level to
produce the `terrain/*.bin` package members; the engine itself never
links or runs it.

Build:

```sh
make -C tools/mc2-genlevel
```

`bake` finds the binary at its default build location, or via the
`MGC_GENLEVEL` environment variable; without it, packages bake without
terrain members.

Validation: output is byte-identical to remc2's DOSBox-verified
regression memimages (all four generated arrays, confirmed on levels
whose fixtures contain no post-generation entity edits; the test
`baked_terrain_matches_remc2_fixture` re-checks this when a remc2
checkout is present, override its location with `MGC_REMC2`).

## rip-mc2-cdaudio.py

Pulls the 27 redbook soundtrack tracks out of the GOG MC2 install's
`game.gog` CD image into FLAC files (needs ffmpeg), for the engine's
future music support. Game *data* files are never extracted this way —
the importer reads them straight from the image (`mgc_import::iso`).

## mc_dosbox_recorder.py

Records a retail playthrough from a running DOSBox into a `.mgcr`
recording — zstd-compressed JSONL, one record per game tick;
**`docs/RECORDING.md` is the normative format spec**. Line 1 is the
header (game, level, build, channel declaration); each tick record
carries the decoded observable projection (`obs`), the RAW master-struct
image plus MC1's external input registers (`state` — the full
mutable-state closure, the fixture-initialization source; retail's own
in-level save writes this exact struct with a single `fwrite`), and the
persistent raw input at the tick boundary (`input`, approximate by
nature — retail recordings verify by state, never by replaying input).
`--no-state` drops the closure for lightweight scouting runs; an output
path ending in `.jsonl` (or `-` for stdout) writes plain JSONL. Writing
`.mgcr` needs the python `zstandard` package. Expect very roughly
~20 KB/tick compressed with the state channel (see
`docs/traces/mc1-campaign-save-menu.md` for the struct map it reads, and
the memory note "retail-conformance-recorder" for the design).

It launches DOSBox as a **child** (so reading its memory usually needs no
root under the default ptrace policy), then locates the master world
struct by CONTENT — scanning guest RAM for the loaded level's pristine
embedded record (so it needs `--level <n>`, the level you're in). DOS4GW
doesn't map guest addresses to host memory affinely, so fixed addresses
don't work; the globals (wall clock, raw input) live in a *separate*
static frame found by its own landmark. It waits patiently through
menus/FMVs, then waits for the sim to actually be ticking before
recording. It takes N consecutive byte-identical reads as proof of a
non-torn, between-ticks snapshot (CONSENSUS), and — since retail has no
global logic-tick counter — counts elapsed ticks from the mode of the
per-entity `+63` increment across persistent entities. Consensus only
proves the guest was frozen; the inter-tick tear gate then rejects
mid-pass parks (cursor clock bands, LCG parity, and the early-cursor
park whose only tell is a moved LCG under a zero +63 mode). When the
sim saturates the emulated CPU (level-start spawn storms, heavy
combat) every park is mid-tick and ticks are unrecoverable — the
recorder reports that loss live per pending tick and folds the streak
breakdown into the gap line, rather than leaving a silent `t` jump.
The first record is only written once the first clean pair vouches
for it (an unvetted mid-tick anchor would starve the whole stream).

```sh
# locate + decode ONE clean snapshot, print a sanity census
./tools/mc_dosbox_recorder.py --game mc1 --level 0 --once -- dosbox -conf … CARPET.EXE

# record ~200 ticks (park the player somewhere safe first)
./tools/mc_dosbox_recorder.py --game mc1 --level 3 --out run.mgcr --max-ticks 200 \
    -- dosbox -conf … CARPET.EXE
#   --pid <n>          attach to an already-running dosbox instead
#   --no-wait-live     don't wait for gameplay to be ticking
#   --no-state         omit the raw closure (fixtures NEED it)

# inspect a recording
zstdcat run.mgcr | head -1 | jq .        # the header
zstdcat run.mgcr | jq -c 'select(.t==5) | .obs.player'
```

Start the game and load the level you named with `--level`; the tool
waits for it. Games:

* `mc1` — validated against a live dump.
* `mc1hw` — shares MC1's engine + struct; reads DDLEVELS. Core +
  externals both verified, build auto-detected (CARPET.EXE=A,
  HIDDEN.EXE=B).
* `mc2` — the D41A0_0 engine (a different struct). Field map verified
  against two live struct dumps (levels 0 and 4): the human decodes to
  class 3 model 0 with the right life/mana and the level-record needle
  locates exactly one struct. The differences from MC1 are handled
  internally by `family == "mc2"` — a 168-byte pool record (facing is the
  world-space yaw at +0x1C, verified live), a 2124-byte per-player block
  whose per-frame `Turn` counter drives continuity (MC2 has no per-entity
  tick byte) and whose flight column holds the persistent steering command
  (`cmd_speed`), and a structural pool-census locate filter. MC2 keeps no
  separate static frame and exposes no usable raw input register, so
  steering intent is read from that persistent state (heading + cmd_speed)
  rather than a mouse/key register. It reads CLEVELS, so `--level` extracts
  the matching MC2 level record via `mgc-import`.

```sh
# MC2: record a level-0 playthrough
./tools/mc_dosbox_recorder.py --game mc2 --level 0 --out mc2run.mgcr \
    -- dosbox -conf … MC2.EXE
```

If capture reports missed-tick gaps, lower DOSBox `cycles` (or raise the
resolution) so the sim runs slow enough to snapshot every tick — or,
better, record against a tick-patched exe (below), which removes gaps by
construction.

## mc_exe_tickpatch.py

Patches a COPY of a retail exe so the recorder can capture every sub-step
cleanly. Two arms, auto-selected from the binary:

* **MC1 — `CARPET.EXE` / `HIDDEN.EXE` (pacer + mailbox).** MC1's tick loop
  was never frame-capped, so at high DOSBox `cycles` every host-park lands
  mid-entity-loop and ticks are lost. This arm paces the sim to ~24 fps and
  exposes a window. Details below.
* **MC2 — `NETHERW.EXE` (signal-only, no pacer).** MC2 already frame-limits
  itself (`InGameLoop_47320`'s native `while (before+5 > GameTimerTurn)`
  spin), so its takes are gap-free — but ~33 % are **torn**: DOSBox can park
  the guest between `PlayerEvents` (per-player `Turn++`) and the entity pass,
  a settled-looking but mid-frame state that passes read-consensus. This arm
  adds no pacer; it just wraps the sole `call DrawAndEventsInGame_47560` in
  the loop and raises an `in_window` flag for exactly the interval when the
  frame is fully settled (post-draw) and the next frame's `Turn++` has not
  begun — i.e. across MC2's own native limiter spin, plus a `--floor` of the
  stub's own (below) so that window has a guaranteed width even when a heavy
  frame leaves the native spin with nothing left. The recorder captures
  only while `in_window==1`, so the `Turn++`-park tear is unobservable by
  construction. The mailbox (magic `MGCTTIK2` + a monotonic **per-frame**
  counter + `in_window`) lives in obj3's committed BSS tail (guest
  `0x1842c0`); the stub derives obj3's real base by reading the game's own
  fixed-up `GameTimerTurn` disp (delta-safe, exactly like the MC1 arm), and
  both `vsize`s are page-aligned so the cave executes and the mailbox writes
  persist. Continuity is that counter's delta — never the per-player `Turn`,
  which advances mid-frame inside `PlayerEvents` and so can't gate the tear.

  ```sh
  python3 tools/mc_exe_tickpatch.py NETHERW.EXE     # -> NETHERW_REC.EXE
  #   --floor N      minimum capture window in 100 Hz counts (default 2;
  #                  0 = off). See "Missed frames" below — this is the fix
  #                  for heavy levels, ahead of --pace.
  #   --verify-only NETHERW_REC.EXE   re-disassemble the stub and check
  #   --inert / --no-extend           the same isolation diagnostics
  # then record — the recorder detects MGCTTIK2 and window-gates MC2:
  ./tools/mc_dosbox_recorder.py --game mc2 --level 0 --out mc2run.mgcr \
      --max-ticks 0 -- dosbox -conf … NETHERW_REC.EXE
  ```

  Because MC2 has no pacer, `--period` is ignored for it and the header
  stamps `spin_period_counts: null`. The recorder gates each capture on a
  cheap 8-byte mailbox read: it only pulls the full 224 KB struct when
  `in_window==1` **and** the per-frame counter has advanced past the last
  captured frame, so an already-recorded window is never re-scanned.

  **Missed frames on graphics-heavy levels.** MC2's native limiter budget is
  **absolute** — `turn_sampled_before_the_frame + 5` ticks of the 100 Hz PIT
  (≈50 ms, real-time-locked, independent of DOSBox `cycles`) — so the window
  it leaves is `budget − compute`. A frame heavy enough to overrun that budget
  (deaths, meteor swarms) reaches the spin with the deadline already passed,
  leaves an almost-zero window that no poll rate can catch, and the recorder
  drops the frame — the cause of sporadic 1–2 frame gaps and the torn deltas
  around them. Fixes, in order of leverage:

  1. **`--floor N`** (default 2, `0` disables) — the stub holds `in_window`
     open for at least N timer counts *after* the frame settles, so the width
     no longer depends on compute at all. This is the only fix that is
     load-**independent**: it is measured from where the frame lands, not from
     an absolute deadline, and it is charged **only** to the frames that
     already overran. The counter is integral (100 Hz), so `N=1` guarantees
     nothing and `N=2` is the smallest value that guarantees a full count
     (≥10 ms, ≤20 ms). Raise to 3–4 only if heavy scenes still drop frames.
  2. **`--pace N`** re-patches the frame-period byte (`add esi,5`→`add esi,N`,
     N>5), widening the absolute budget so compute is less likely to exhaust
     it — sim-neutral (one `Turn`+entity pass per frame either way; the
     recorded frame sequence is byte-identical, just paced slower), but it
     taxes *every* frame including the cheap ones and still cannot guarantee a
     window on a frame heavy enough to blow the wider budget too.
     **MEASURED 2026-08-19: with `--floor 2` this knob was not needed at all.**
     mc2l24 — the final level, worst content in the game — recorded gap-free
     at the *untouched* native period 5, where the pre-floor guidance was
     `--pace 12` (~8.3 fps). Treat `--pace` as a diagnostic for a level that
     defeats the floor, not as standard practice: it costs ~2.4x the recording
     wall-clock and the floor covers the same failure for ~10 ms a spike.
  3. Reduce in-game detail (smaller viewport via `[`/`]`, flat shading, lower
     res) to shrink compute.
  4. Raise DOSBox `cycles` if the host has headroom (more cycles → compute
     finishes sooner → wider window; note this is the *opposite* of the
     tear-gate path's "lower cycles" advice).

  `--poll-hz` helps only at the margin — the windowed loop already polls at a
  0.1 ms floor by default.

The MC1 arm installs a 249-byte
wrapper stub around the per-sub-step tick function (remc1
`sub_41780_41AC0`) by redirecting the tick fn's callers (the 3
gameSpeed-fanout `call`s — rewriting only their 4-byte rel32) so they
enter the stub, which paces, then `call`s the original untouched tick fn
and `ret`s. The function entry is left byte-for-byte intact (a first
version detoured the entry, which decoded as a wild `add eax,[eax]` under
the dynamic recompiler's misaligned decode). The stub
(1) spins on the game's own PIT counter (~120 Hz, measured live) until one
period elapses, so `fps = 120/period` ≈ 24 fps at the default period 5,
and the *spare* cycles become a wide quiescent window. It paces exactly
**one sub-step per rendered frame** (the first — detected via the fan-out's
live loop index in `EBX`), so the F3 game-speed feature (1× / 4× / 16×
sub-steps per frame) still speeds the *sim* up 4×/16× while the frame rate
holds; at the default speed of one sub-step/frame every sub-step is the
first, so pacing is bit-identical in effect to the earlier every-sub-step
pacer.
(1b) **Floors the window** (`--floor N`, default 2 counts, `0` disables).
Pacing to an absolute deadline is only as good as the compute fitting inside
it: a sub-step heavy enough to overrun its period arrives with the deadline
already passed, the spin falls straight through, and `in_window` is raised and
cleared within a handful of instructions — a zero-width window the recorder
cannot land in, so the frame is dropped and its delta tears. Worse, the
release path tolerates up to 30 counts of backlog with *no* wait at all, so
one heavy sub-step is followed by a burst of free-running ones. The floor
clamps `deadline = max(deadline, now + N)` before the spin, which makes the
window's width independent of load and deletes the catch-up burst with it
(the deadline is rebuilt from `now` on every overrun, so backlog cannot
accumulate). It is a **no-op while the game is keeping up** — steady-state
waits are identical with and without it — and costs time only on the
sub-steps that already blew their budget, unlike raising `--period`, which
taxes every frame. The PIT counter is integral (~120 Hz), so `N=1` guarantees
nothing (enter a hair before it ticks and the spin releases immediately) and
`N=2` is the smallest value that guarantees a full count (≥8.3 ms, ≤16.7 ms);
the tool rejects `1` outright. And
(2) keeps a mailbox (magic + monotonic sub-step counter + `in_window` flag,
raised only around a paced spin + the raw F3 `gameSpeed` 0/1/2) in obj3's
committed tail, addressed via a runtime-derived obj3 base
(read from the game's own relocated struct pointer, since DOS/4GW loads
each object at an independent base — assuming a uniform delta made the
stub write into game memory and crash). **Both obj1 (code cave) and obj3
(mailbox) have their `vsize` page-aligned so those tails are inside the
segment limit — else the cave won't execute and the mailbox writes won't
persist (the pacing deadline resets every call).** The PIT counter is
**not monotonic for the life of the process** while the mailbox is — the game
zeroes the clock in its fade/delay helper (`sub_10300`) on the way back to the
menu, and restores an older value on ALT+L quickload — so the pacer resyncs on
a deadline too far *ahead* of the clock as well as too far behind. Without that
second guard, the second level of a session inherits a deadline minutes in the
future and spins out its guard counter every frame (**~0.2 fps** until the
clock climbs back). The recorder auto-detects the mailbox and
switches to windowed capture (`docs/RECORDING.md` → "Tick-patched
capture"): no tear gate, no `+63` guessing, gap-free by construction.
The sim is unaffected — MC1's lockstep multiplayer proves per-tick logic
is wall-clock independent, so pacing changes only *when* ticks run.

**gamedata/ stays pristine GOG** — the tool writes a `*_REC.EXE`
alongside the input and never touches the original. The stub lives in
obj1's zero code cave; the mailbox in obj3's zero BSS tail; nothing else
in the binary changes (verified by byte-diff: only the two `vsize` fields
(obj1 + obj3, page-aligned), the three call-site rel32s, and the cave —
the tick fn entry stays byte-identical). Diagnostics: `--inert` writes the
stub but wires no call site (proved the cave is safe); `--passthrough`
wires a bare `call tickfn;ret` (proved execution was the issue);
`--no-extend` skips the `vsize` page-align (reproduces the crash).

```sh
# produce the patched copies (~24 fps, the authentic rate)
python3 tools/mc_exe_tickpatch.py CARPET.EXE          # -> CARPET_REC.EXE
python3 tools/mc_exe_tickpatch.py HIDDEN.EXE          # -> HIDDEN_REC.EXE
#   --period 4     ~30 fps ;  --period 6  ~20 fps  (fps = 120/period)
#   --floor N      minimum capture window in ~120 Hz counts (default 2;
#                  0 = off). Guarantees a window on sub-steps that overrun
#                  their period — deaths, meteor swarms — where the pacer
#                  alone leaves none. Raise to 3–4 if gaps persist.
#   --verify-only PATCHED.EXE   re-disassemble the hook + stub and check

# record against the patched exe — recorder detects the mailbox itself
./tools/mc_dosbox_recorder.py --game mc1 --level 3 --out run.mgcr \
    --max-ticks 0 -- dosbox -conf … CARPET_REC.EXE
# the header then stamps capture.window_gated + capture.exe_patch
```

Live-run checklist (what a real recording session should confirm):

1. `mgcarpet.json` / DOSBox mount points at the patched `*_REC.EXE`.
2. On go-live the recorder prints `exe tick-patch detected: mailbox …` —
   if not, the stub never ran (still in a menu) or the magic scan failed.
   MC1 reports a `spin-period`; MC2 reports `signal-only`.
3. **MC1 only:** in-game feel is a steady ~24 fps even with `cycles=max`
   (period 5); raising cycles widens the window (longer spin), never speeds
   the game. F3 game-speed still works: cycling to 4×/16× speeds the *sim*
   up (more sub-steps per frame) while the frame rate stays ~24 fps, and
   returning to 1× restores it — only the first sub-step of each frame is
   paced. **MC2** keeps its own native rate (no pacing was added).
4. `done: … window-gated, no tears possible` with 0 gaps across a full
   playthrough, including level-start fades and big explosions (the
   structural gaps the tear-gate recorder could not close). Deliberately
   *provoke* the heavy cases — deaths, several meteors at once — since
   those are exactly the frames that overrun the pacing deadline and are
   the floor's whole reason to exist. `--verify-only` prints the floor it
   read back out of the patched image, so confirm it is non-zero before
   blaming a gap on anything else. If gaps survive at `--floor 2`, step to
   3 or 4 before reaching for `--period` / `--pace`; if they survive that,
   the window is not the problem.
5. Sim parity: a windowed recording of a fixed level should decode the
   same tick sequence as the tear-gated recorder for the ticks both
   captured (the signal/pacing must not perturb sim state).
6. **MC2 specifically:** re-record a level whose old tear-gated take had
   many torn pairs (mc2l0/l4/l30) and confirm the torn-slot exclusions
   drop out — the whole point of the arm. Old MC2 takes stay tear-gated;
   only RE-RECORDED takes get the window.

## MC1 oracle (planned)

Reference dumps for MC1 terrain generation, via instrumented DOSBox
running the original binary (the dosbox-x-remc2 methodology), until/
unless the generator can be carved out of the dormant remc1
decompilation — or MC2's generator proves compatible with MC1 seeds,
which should be tested against the DOSBox dumps first.
