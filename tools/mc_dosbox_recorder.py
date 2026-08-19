#!/usr/bin/env python3
"""Record a retail Magic Carpet playthrough from a running DOSBox.

Launches DOSBox as a *child* process, latches onto its emulated guest RAM
via ``/proc/<pid>/mem``, locates the game's master world struct, and then
polls it — capturing the full sim state once per game tick into a
``.mgcr`` recording (zstd-compressed JSONL; ``docs/RECORDING.md`` is the
normative format spec). Each tick record carries up to three channels:

  * ``obs``   — the decoded observable projection (entities, wizards,
                RNG…), human-greppable and the retail-vs-port compare set;
  * ``state`` — the RAW master-struct image (plus MC1's external input
                registers): the full mutable-state closure, so field maps
                can improve after the fact and single-tick fixtures can
                initialize from any tick (``--no-state`` omits it);
  * ``input`` — the persistent raw input sampled at the tick boundary
                (approximate by nature — see the spec; retail recordings
                verify by state, never by replaying input).

Line 1 is the header record (game, level, build, channel declaration).

## The synchronisation problem (and how we handle it)

The retail sim ticks on its own clock; we cannot pause or step it, and we
do not get told when a tick happens.  So we POLL.  Two hazards:

  1. *Tearing* — reading the ~230 KB struct while the sim is mid-tick
     yields a half-updated image (some entities stepped, some not).
  2. *Missed ticks* — if DOSBox runs the sim faster than we can snapshot,
     we silently skip states.

We defend against (1) with CONSENSUS: read the mutable state N times
back-to-back and only accept a snapshot when consecutive reads are
byte-identical — proof the state was quiescent (between ticks) for the
whole read window.

For (2) there is a catch: retail has NO global logic-tick counter
(``dword_AC5D4`` is a free-running ~120 Hz wall clock, decoupled from the
sim).  But every active entity's ``+63`` byte increments exactly once per
tick, so the MODE of that increment across entities that persisted
between two accepted snapshots IS the number of ticks elapsed — idle
entities (castles) all step +1 and outvote any that reset on a state
change.  A mode > 1 means we missed ticks (lower DOSBox ``cycles`` and
re-run).  The wall clock is recorded too, as a liveness/ordering signal.

## Lifecycle

Launch DOSBox → locate the struct → **wait** for the player to start a
playthrough (the struct holds a live world only once a level loads) →
**wait** for the sim to actually be ticking (retail's world sim + wall
clock stay frozen through menus / a 'get ready' pause; the gameplay RNG at
struct+4 only advances inside the tick, so it is the go-live signal) →
record from the first live tick. One recording = one deliberate
playthrough; on a level change the base can move, so re-run rather than
following transitions.

Locating: DOS4GW does NOT map guest addresses to host memory affinely —
the heap (where the master struct lives) and the static data segment
(where the globals live) sit in independent frames. So we locate by
CONTENT, in two independent steps:
  * the HEAP struct, by scanning for the pristine embedded level record
    at struct+193795 (needs `--level` to know the bytes), and
  * the STATIC frame (wall clock, raw input), by scanning for a fixed
    data landmark (byte_99B58) and reading the globals relative to it —
    validated by the struct-pointer global there resolving back to the
    heap struct's own owner_ptr chain.
Entities allocate from the TOP of the pool (free stack built 999→1), so
the player's carpet + castle sit at high slots (~630 on level 0), not the
low ones — validation scans the whole pool. Liveness watches the ACTUAL
dosbox pid (re-acquiring across a launcher exit / re-exec), never the
launcher handle.

## Layout knowledge

MC1 and MC1HW share one engine (CARPET.EXE / HIDDEN.EXE) and an
identical master-struct layout — retail's own in-level save writes the
whole 232,713-byte ``str_AE400_AE3F0`` struct with a single ``fwrite``,
which is exactly the full mutable-state closure we want.  See
``docs/traces/mc1-campaign-save-menu.md`` for the field map.

MC2 uses a different engine and a different master struct (``D41A0_0``,
224,790 bytes).  Its layout is described by its own ``Layout`` /
``EntFields`` descriptor and selected by ``family == "mc2"``, which
routes to the MC2 decode / locate / continuity paths.  The differences
from MC1 that the family split covers: the pool record is 168 bytes with
its own field offsets (class at +0x3F, current life at +0x08, mana on the
entity at +0x90); the per-player record is a 2124-byte block at +0x2BDE
carrying the identity, spell hands and — crucially — a real per-frame
``Turn`` counter that replaces MC1's per-entity ``+63`` tick byte for
continuity; the persistent mouse aim lives inside the struct (so there is
no separate static frame); and the struct is located by the same
level-record needle idea but validated with a structural pool-census
filter instead of a build-variant header check.  See the
``mc2-recorder-field-map`` memory note and ``tools/mc2_dosbox_capture.py``
(the one-shot this generalises).

Needs permission to read the child's memory.  Because we are the parent,
default Linux (yama ptrace_scope<=1) usually allows this WITHOUT root; if
not, run under sudo or set ``kernel.yama.ptrace_scope=0``.

Provenance note for the ``state`` channel: retail's own in-level save
writes the whole MC1 struct with a single ``fwrite`` — the struct image
IS the game's own idea of the full mutable-state closure.
"""

from __future__ import annotations

import argparse
import atexit
import base64
import datetime
import json
import os
import shutil
import struct
import subprocess
import sys
import time
from dataclasses import dataclass
from typing import Iterator, Optional


# ---------------------------------------------------------------------------
# Layout descriptors
# ---------------------------------------------------------------------------
#
# Every game-specific constant lives here so the same engine below serves
# MC1/MC1HW/MC2.  Offsets are into the master struct unless noted "ent+"
# (into a 164/168-byte pool record) or "wiz+" (into a 2049-byte wizard
# record).  Sources: docs/traces/mc1-campaign-save-menu.md and the
# offset-named fields of crates/mgc-sim/src/engine/features.rs (::Ent).


@dataclass(frozen=True)
class EntFields:
    """Byte offsets of the fields we decode out of one pool record."""

    rand: int = 4  # per-entity LCG
    max_life: int = 8
    act_life: int = 12  # current LIFE (signed; dying drives it < 0)
    flags: int = 16  # bit0 active, bit1 dug, bit2 tiled, bit10 dead
    id24: int = 24  # owner/link id (friendly-fire key)
    class_: int = 64  # 0 = free slot
    model: int = 65
    sclass: int = 66
    smodel: int = 67
    x: int = 72  # position, 8.8 fixed
    y: int = 74
    z: int = 76  # altitude (signed)
    heading: int = 30  # applied yaw 0..2047 (:55146); the carpet's facing
    pitch: int = 32  # applied pitch (:55158)
    target_yaw: int = 34  # commanded yaw (11-bit; hi byte pitch for fliers)
    speed: int = 126  # forward speed this tick (:55147)
    max_speed: int = 128
    accel: int = 130
    # For the player carpet: f136 = mana capacity, f140 = current mana
    # (HUD mana bar :27376-77). Generic on other entities.
    f136: int = 136
    f140: int = 140
    chase: int = 146  # chase target pool slot
    model_ptr: int = 156  # &model-table row (guest pointer)
    owner_ptr: int = 160  # &wizard Type_160 (guest pointer; off-pool AI)
    tick_byte: Optional[int] = 63  # runtime loop ++ once per tick (:52417);
    # None for MC2 (its engine has no per-entity per-tick byte — continuity
    # comes from the per-player Turn counter instead).
    # MC2-only extras (left None on MC1, whose decoder never reads them).
    # MC2 carries two orientations: a world-space one (the LIVE facing —
    # ``heading``/``pitch`` point at it on MC2) and an "applied" camera/
    # target one that a 700-tick live recording showed rests at a constant
    # for the player, so it is captured separately here, not as the heading.
    applied_yaw: Optional[int] = None  # 0x52 camera/target yaw
    applied_pitch: Optional[int] = None  # 0x54
    action: Optional[int] = None  # 0x45 actionIndex (state machine)
    stagevar1: Optional[int] = None  # 0x48 per-entity StageVar binding
    stagevar2: Optional[int] = None  # 0x49
    mana_regen: Optional[int] = None  # 0x88 per-tick mana regen
    player_ent_idx: Optional[int] = None  # 0x94 owning-player back-index


@dataclass(frozen=True)
class BuildVariant:
    """One retail build's guest LINEAR addresses for the externals we
    read.  The offset-named symbols carry two addresses (e.g.
    ``str_AE400_AE3F0``, ``dword_AC5D4_AC5C4``, ``pressedKeys_12EEF0_12EEE0``)
    = the two shipped builds.  We auto-detect which is running by the
    struct-pointer check in :func:`find_static_base`."""

    name: str
    struct_ptr_guest: int  # global cell holding the heap pointer to struct
    wallclock_guest: int  # dword_AC5D4: ~120Hz PIT clock (NOT logic ticks)
    pressed_keys_guest: int  # pressedKeys_12EEF0 raw scancode array
    static_needle_guest: int  # guest addr of Layout.static_needle in THIS build
    mouse_cursor_guest: int  # mouse_9AD90 {i16 x, i16 y}: absolute aim source
    mouse_lbtn_guest: int  # mouseLeftButton2 (held) — fire-left
    mouse_rbtn_guest: int  # mouseRightButton2 (held) — fire-right
    # Optional extras (0 = absent; MC2 has them, MC1 does not):
    # the press LATCHES (set on the press edge, cleared when the held
    # state drops — catches clicks shorter than one poll) and the
    # cursor position SNAPSHOT taken by the game at the press (sole
    # retail consumer: the fly-assistant idle-recentre watchdog).
    mouse_latchl_guest: int = 0
    mouse_latchr_guest: int = 0
    press_pos_guest: int = 0
    # Guest addr of the CONTIGUOUS terrain-plane block (the format-2
    # terrain channel). Both engines keep the planes back-to-back in
    # static BSS, same order: type | height | shading | angle (+0 /
    # +0x10000 / +0x20000 / +0x30000), MC2 adding the cave ceiling at
    # +0x40000. MC1 A=0xCC1E0 / B=0xCC1D0 (dual-suffixed, fixup-table
    # verified both builds); MC2 = mapTerrainType_10B4E0 through the
    # struct-anchored frame. 0 = no terrain capture for this build.
    terrain_guest: int = 0


@dataclass(frozen=True)
class Layout:
    name: str
    struct_size: int
    # Master-struct field offsets.
    rng_off: int
    wizidx_off: int  # u16 current-wizard index (must be 0..7 — sanity gate)
    localplayer_off: int  # u16 local index, then u16 player count
    wizards_off: int  # MC2: per-player block base (D41A0_0+0x2BDE)
    wizard_stride: int  # MC2: per-player block stride (2124)
    wizard_count: int
    ctrlcmd_off: int  # per-player control-command array (char[N][stride])
    ctrlcmd_stride: int  # bytes per player's command record
    ctrlcmd_count: int  # number of player slots
    pool_off: int
    ent_stride: int
    ent_count: int
    level_rec_off: int  # embedded decompressed LEVELS.DAT record
    level_rec_size: int
    ent: EntFields
    # Wizard/per-player-record inner offsets (block-relative).
    wiz_playindex_off: int  # u16 index into the entity pool
    wiz_type160_off: int  # MC1: start of the spell/mana column (Type_160)
    # Type_160 inner offsets (relative to wiz + wiz_type160_off).
    t160_hand_left_off: int  # selected left-hand spell index (255 = none)
    t160_hand_right_off: int  # selected right-hand spell index
    # Guest builds (externals). Empty = externals/wall clock unavailable.
    build_variants: tuple
    pressed_keys_len: int
    # Which mutable byte ranges of the struct to compare for the consensus
    # (skip the big static regions — level record, sprite residency — to
    # cut per-poll cost). List of (start, end) struct-relative ranges.
    volatile_ranges: tuple = ()
    # Level-archive basename in the game's LEVELS dir (needle source).
    level_archive: str = "LEVELS"  # "DDLEVELS" for Hidden Worlds
    # Static-data landmark for the static-globals frame (empty = none).
    static_needle: bytes = b""
    # False for games whose field map is still a stub: refuse to run rather
    # than emit garbage.
    implemented: bool = True
    # Engine family — selects the game-specific decode/locate/continuity
    # paths below ("mc1" covers MC1 + MC1HW; "mc2" is the D41A0_0 engine).
    family: str = "mc1"
    # How elapsed logic ticks are counted between two clean snapshots:
    #   "tick_byte" — mode of the per-entity +63 increment (MC1/HW), or
    #   "turn"      — delta of the local player's per-frame Turn counter
    #                 (MC2, which has no per-entity tick byte).
    continuity: str = "tick_byte"
    # Bytes to skip at the front of the level record when cutting the
    # locate needle — MC2's record header has a volatile prefix (a level/
    # scratch byte at +7), so its needle starts at +8; MC1's is stable.
    needle_skip: int = 0
    # MC2 per-player block inner offsets (block-relative). Unused on MC1.
    pp_flag_off: int = 0  # dword status flags
    pp_isai_off: int = 0  # u8 1 = AI-controlled rival
    pp_turn_off: int = 0  # i32 per-frame Turn counter (continuity signal)
    pp_name_off: int = 0  # NUL-terminated wizard name
    pp_castle_off: int = 0  # i16 established castle's pool slot (0 = none)
    pp_hand_left_off: int = 0  # i16 left-hand spell index (-1 = none)
    pp_hand_right_off: int = 0  # i16 right-hand spell index
    # MC2 flight-command column (type_str_164) base within the per-player
    # block: the persistent steering accumulators live here, laid out like
    # MC1's Type_160 (cmd_speed at +12, the strafe/second-speed slot at
    # +16). 0 = no flight column mapped. Verified vs a mid-steer dump.
    pp_flight_off: int = 0
    # MC2 in-struct persistent mouse aim {i16 x, i16 y} with fly-assist at
    # off-2. 0 = the game keeps the mouse cursor in its own struct, so no
    # separate static frame is needed (unlike MC1). None = not in-struct.
    in_struct_mouse_off: Optional[int] = None
    # Terrain planes (the format-2 terrain channel, docs/RECORDING.md):
    # tuple of (name, offset) — offset from the build's `terrain_guest`
    # block base, blob order = declaration order. Empty = no terrain
    # capture for this game (the recording stays format 1).
    terrain_planes: tuple = ()
    # (width, height) of every declared plane, cells = bytes.
    terrain_dims: tuple = (256, 256)
    # A plane captured ONLY on cave levels (MC2's second heightmap —
    # retail never writes it on Day/Night levels, so off-cave it holds
    # BSS residue): (name, offset), gated by `terrain_cave_byte_off`.
    terrain_cave_plane: Optional[tuple] = None
    # Struct offset of the MapType byte (0=Day 1=Night 2=Cave); 0 = the
    # game has no cave concept.
    terrain_cave_byte_off: int = 0


MC1_ENT = EntFields()

# The two shipped MC1/HW builds (offset-named symbols carry both). Which
# is running is auto-detected in find_static_base by the struct-pointer
# cross-check, so order is irrelevant.
MC1_BUILDS = (
    # A = CARPET.EXE (MC1), B = HIDDEN.EXE (HW), verified against dumps.
    BuildVariant("A", struct_ptr_guest=0x000AE400, wallclock_guest=0x000AC5D4,
                 pressed_keys_guest=0x0012EEF0, static_needle_guest=0x00099B58,
                 mouse_cursor_guest=0x0009AD90, mouse_lbtn_guest=0x0012EFE4,
                 mouse_rbtn_guest=0x0012EFE2,
                 terrain_guest=0x000CC1E0),  # mapTerrainType_CC1E0
    # Only the DUAL-suffixed globals shift −0x10 in build B (struct-ptr,
    # wallclock, pressed-keys, mouse buttons, terrain planes). The
    # single-address symbols byte_99B58 and mouse_9AD90 are at the SAME
    # guest addr in both builds.
    BuildVariant("B", struct_ptr_guest=0x000AE3F0, wallclock_guest=0x000AC5C4,
                 pressed_keys_guest=0x0012EEE0, static_needle_guest=0x00099B58,
                 mouse_cursor_guest=0x0009AD90, mouse_lbtn_guest=0x0012EFD4,
                 mouse_rbtn_guest=0x0012EFD2,
                 terrain_guest=0x000CC1D0),  # −0x10 (dual-suffixed)
)

# A fixed 16-byte static-data global (byte_99B58, sub_main.cpp:5740) —
# present from startup, at ONE place (no load-buffer copies), so scanning
# for it and dereferencing the struct pointer reaches the REAL struct.
MC1_STATIC_NEEDLE = bytes((0xB7, 0x71, 0x7D, 0x7A, 0x9D, 0x9A, 0x07, 0x5A,
                           0x1D, 0x1B, 0xDD, 0xDA, 0x3C, 0x39, 0x10, 0x0E))

# --- MC2 external input frame ----------------------------------------------
# MC2's raw input globals (what ReadGameUserInputs_89D10 reads) live in
# the SAME contiguous data image as D41A0_0, at a constant offset from
# it — but remc2's NAMED VAs are the decompiler's segment mapping, which
# is shifted −0xB0E98 against the D41A0-anchored runtime frame. The
# delta was measured by scanning two independent live dumps for the
# CONFIG.DAT keybind bytes (x_BYTE_EB39E_keys landed at frame VA 0x3A506
# in BOTH runs) and semantically confirmed: the cursor pair read
# (320,197) = dead-center 640x400 in the resting dump and (314,203)
# mid-steer, and the control-mode word read a stable 7.
#
# So every address below = remc2 named VA − 0xB0E98, and the frame base
# = struct_host − 0xD41A0 (the struct is itself a static at VA 0xD41A0,
# so no pointer chase and no separate needle — see pin_externals).
#
# Registers (EventsFunctions.cpp:51460-51500 = the driver intake):
# held state = the "2" registers @0x18074C/0x18074A (set while pressed,
# cleared on the release event); @0x180746/0x180744 are press LATCHES
# (set once at the press edge); @0xE375C/E = the cursor AT the press
# (fly-assistant watchdog datum, NOT the aim); @0xE3760 = the live
# cursor (the aim/attitude source); pressedKeys_180664 =
# the 128-cell scancode array (same shape as MC1's).
MC2_DATA_DELTA = -0xB0E98
MC2_BUILDS = (
    BuildVariant("gog",
                 struct_ptr_guest=0,  # struct is static — no pointer chase
                 wallclock_guest=0,  # not mapped (unused off-MC1)
                 pressed_keys_guest=0x180664 + MC2_DATA_DELTA,
                 static_needle_guest=0,  # frame anchors on the struct
                 mouse_cursor_guest=0xE3760 + MC2_DATA_DELTA,
                 mouse_lbtn_guest=0x18074C + MC2_DATA_DELTA,  # held left
                 mouse_rbtn_guest=0x18074A + MC2_DATA_DELTA,  # held right
                 mouse_latchl_guest=0x180746 + MC2_DATA_DELTA,
                 mouse_latchr_guest=0x180744 + MC2_DATA_DELTA,
                 press_pos_guest=0xE375C + MC2_DATA_DELTA,
                 # mapTerrainType_10B4E0..mapAngle_13B4E0 + the cave
                 # ceiling x_BYTE_14B4E0, contiguous statics in the same
                 # frame as the input registers (named VA − 0xB0E98).
                 terrain_guest=0x10B4E0 + MC2_DATA_DELTA),
)
# Frame validation anchors (pin_externals): the control-mode word
# x_WORD_1805C2_joystick (a small nonzero constant; 7 on the GOG
# install) and the keybind table x_BYTE_EB39E_keys (CONFIG.DAT
# scancodes — arrows 0x48/0x50/0x4B/0x4D on a stock config; we require
# plausibility, not exact values, so remapped keys still validate).
MC2_CTRLMODE_GUEST = 0x1805C2 + MC2_DATA_DELTA
MC2_KEYBIND_GUEST = 0xEB39E + MC2_DATA_DELTA
MC2_STRUCT_VA = 0xD41A0

# --- EXE tick-patch mailbox (tools/mc_exe_tickpatch.py) ---------------------
# When a *_REC.EXE (a tick-patched copy) is running, its stub keeps a mailbox
# in obj3's committed BSS tail. The recorder auto-detects it and switches to
# the windowed capture path: the stub raises `in_window` for the whole
# quiescent spin (the world struct is settled and untouched), and `tick` is a
# monotonic sub-step counter — so a capture taken while in_window==1 is
# guaranteed between-tick, and continuity is the counter's delta (no +63
# heuristic, no tear gate). The guest-linear address is the same in both
# builds (obj3 loads at 0x90000; mailbox at obj3+0xa2c40). The stub derives
# obj3's real runtime base from the game's own relocated struct pointer, and
# static_base (anchored to obj3's needle) maps the same address host-side.
# Keep in lockstep with tools/mc_exe_tickpatch.py's MB_* constants.
EXE_MB_BASE = 0x132C40  # guest-linear addr of the mailbox (obj3 tail)
EXE_MB_MAGIC = b"MGCTTIK1"  # 8 bytes the stub writes once, on first tick
EXE_MB_TICK = 0x08  # u32 monotonic sub-step counter
EXE_MB_INWIN = 0x0C  # u32 1 while parked in the quiescent spin
EXE_MB_PERIOD = 0x18  # u32 configured spin period (PIT counts)

# MC2 / NETHERW_REC.EXE mailbox. MC2 needs no pacer (it frame-limits itself),
# so its stub is SIGNAL-ONLY: same tick(+8)/in_window(+0xC) layout, a distinct
# magic, and no period field. in_window is raised across MC2's own native
# limiter spin -- state fully settled, next frame's Turn++ not begun -- so the
# ~33% Turn++-park tear is unobservable. The counter is per-FRAME (bumped once
# a frame), so continuity is its delta, not the per-player Turn (which advances
# mid-frame by design). Guest addr = obj3 tail; the recorder maps it off the
# same static base it derives for the MC2 input frame (struct_host - 0xD41A0).
EXE_MB2_BASE = 0x1842C0  # guest-linear addr of the MC2 mailbox (obj3 tail)
EXE_MB2_MAGIC = b"MGCTTIK2"  # 8 bytes the MC2 stub writes once, on first frame

# The shared plane order (both engines keep them contiguous, in this
# order, off the build's `terrain_guest` base — the same order retail's
# own map savestate writes them). Blob order for the terrain channel.
MC_TERRAIN_PLANES = (
    ("type", 0x00000),
    ("height", 0x10000),
    ("shading", 0x20000),
    ("angle", 0x30000),
)

# MC1 / MC1HW — identical engine + struct (save doc: "HW byte-identical").
LAYOUT_MC1 = Layout(
    name="mc1",
    struct_size=232713,
    rng_off=4,  # rand_4, LCG x=9377x+9439 (:52223)
    wizidx_off=8,  # var_u16_8: local/current wizard index (0..7 gate)
    localplayer_off=8,  # u16 local index @8, u16 player count @10
    wizards_off=13323,
    wizard_stride=2049,
    wizard_count=8,
    ctrlcmd_off=29715,  # char[8][10]: per-player control command
    ctrlcmd_stride=10,
    ctrlcmd_count=8,
    pool_off=29795,
    ent_stride=164,
    ent_count=1000,
    level_rec_off=193795,  # pristine decompressed LEVELS.DAT record
    level_rec_size=38812,
    ent=MC1_ENT,
    wiz_playindex_off=10,  # playIndex_13333 (pool slot of this wizard)
    wiz_type160_off=1103,  # str_1103: the Type_160 spell/flight column
    t160_hand_left_off=940,  # var_940: left-hand spell idx (0xFFFF = none)
    t160_hand_right_off=944,  # var_944: right-hand spell idx
    build_variants=MC1_BUILDS,
    pressed_keys_len=128,  # uint8[128], indexed by scancode & 0x7F
    volatile_ranges=(
        (0, 8600),  # header: rng, indices, free lists, banks
        (11274, 29795),  # scratch + wizards + control-command array
        (29795, 29795 + 164000),  # entity pool
    ),
    static_needle=MC1_STATIC_NEEDLE,
    terrain_planes=MC_TERRAIN_PLANES,
)

# MC1HW shares every offset; only the level data (needle) differs — it
# reads DDLEVELS, and the build variants already cover HIDDEN.EXE's
# "_AE3F0" address half.
LAYOUT_MC1HW = Layout(
    **{**LAYOUT_MC1.__dict__, "name": "mc1hw", "level_archive": "DDLEVELS"}
)

# MC2 — the D41A0_0 engine (a different struct/layout from MC1). Field map
# VERIFIED against two live dumps (level 0 and level 4): the local human
# decodes to class 3 model 0 with the right life/mana/Turn, and the
# level-record needle + structural filter locate exactly one struct. See
# docs/traces/mc2-*.md and the "mc2-recorder-field-map" memory note.
MC2_ENT = EntFields(
    rand=0x14,  # u16 per-entity LCG
    max_life=0x04,
    act_life=0x08,  # current life (verified: player = 10000)
    id24=0x28,  # parentId / owner id
    class_=0x3F,  # 0 = free slot; 3 = carpet/castle (player = model 0)
    model=0x40,
    x=0x4C,
    y=0x4E,
    z=0x50,  # altitude
    heading=0x1C,  # world-space yaw = the LIVE facing (700-tick recording
    # confirmed this sweeps with flight; the applied yaw @0x52 stayed at a
    # constant 100). 0..2047.
    pitch=0x1E,  # world-space pitch (live)
    speed=0x82,
    accel=0x84,  # minSpeed
    max_speed=0x86,
    f136=0x8C,  # maxMana
    f140=0x90,  # current mana (verified: player = 1000)
    tick_byte=None,  # MC2 has no per-tick byte — continuity is Turn
    applied_yaw=0x52,  # camera/target yaw (constant for the player)
    applied_pitch=0x54,
    action=0x45,
    stagevar1=0x48,
    stagevar2=0x49,
    mana_regen=0x88,
    player_ent_idx=0x94,
)

LAYOUT_MC2 = Layout(
    name="mc2",
    family="mc2",
    struct_size=224790,
    rng_off=8,  # gameplay LCG x=9377x+9439 (steps per tick — go-live signal)
    wizidx_off=0xC,  # local wizard index lives with the header pair below
    localplayer_off=0xC,  # u16 local index @0xC, u16 player count @0xE
    wizards_off=0x2BDE,  # per-player block base
    wizard_stride=2124,
    wizard_count=8,
    ctrlcmd_off=0x6E3E,  # char[8][10] per-player control command
    ctrlcmd_stride=10,
    ctrlcmd_count=8,
    pool_off=0x6E8E,  # 1000 x 168-byte entities (ends exactly at 0x2FECE)
    ent_stride=168,
    ent_count=1000,
    level_rec_off=0x2FECE,  # embedded pristine CLEVELS record (the needle)
    level_rec_size=26116,
    ent=MC2_ENT,
    wiz_playindex_off=0xA,  # playerIndex -> carpet entity slot
    wiz_type160_off=0,  # MC1-only; MC2 uses the pp_* offsets below
    t160_hand_left_off=0,
    t160_hand_right_off=0,
    build_variants=MC2_BUILDS,  # struct-anchored input frame (see above)
    pressed_keys_len=128,
    # Compare everything up to (not including) the in-struct mouse: that
    # covers all decoded sim state (header, per-player blocks, control
    # array, entity pool, level record, StageVars) while leaving out the
    # mouse aim, which the input poll may rewrite asynchronously to the sim
    # tick and would otherwise keep consensus from ever settling.
    volatile_ranges=((0, 0x36DEA),),
    implemented=True,
    continuity="turn",
    needle_skip=8,  # skip the record's volatile +7 header byte
    pp_flag_off=0x0,
    pp_isai_off=0x9,
    pp_turn_off=0x12,
    pp_name_off=0x39F,
    pp_castle_off=1080,  # CastleEntityIndex (block-relative)
    pp_hand_left_off=2103,  # SpellIndexLeft (block-relative)
    pp_hand_right_off=2105,  # SpellIndexRight
    pp_flight_off=998,  # type_str_164 base — cmd_speed at +12 (block+1010)
    terrain_planes=MC_TERRAIN_PLANES,
    # The cave CEILING (x_BYTE_14B4E0, block +0x40000): captured only
    # when MapType (struct+0x2FED4: 0=Day 1=Night 2=Cave) says Cave —
    # retail's generator never writes it off-cave (Terrain.cpp:19-56,
    # only the cave branch calls the ceiling builder sub_43B40), so on
    # Day/Night levels the array holds BSS residue, not terrain.
    terrain_cave_plane=("ceiling", 0x40000),
    terrain_cave_byte_off=0x2FED4,
    # in_struct_mouse_off intentionally UNSET: the field-map's mouse guess
    # (@0x36DEC) read 0 through a whole mid-steer dump, so it is NOT the aim
    # source. The steering intent is captured instead from the persistent
    # state — `heading` (world yaw, the direction) + the flight-command
    # accumulators (cmd_speed) in the player join; the per-frame mouse delta
    # is transient (zeroed between ticks) and equals the heading delta.
)

LAYOUTS = {
    "mc1": LAYOUT_MC1,
    "mc1hw": LAYOUT_MC1HW,
    "mc2": LAYOUT_MC2,
}


# ---------------------------------------------------------------------------
# Process + memory access
# ---------------------------------------------------------------------------


def _read_comm(pid: int) -> str:
    try:
        with open(f"/proc/{pid}/comm") as f:
            return f.read().strip()
    except OSError:
        return ""


def _children(pid: int) -> list[int]:
    kids: list[int] = []
    try:
        with open(f"/proc/{pid}/task/{pid}/children") as f:
            kids = [int(x) for x in f.read().split()]
    except OSError:
        pass
    return kids


def find_dosbox_descendant(
    root_pid: int, timeout: float = 10.0, global_fallback: bool = True
) -> int:
    """Locate the actual dosbox process under `root_pid` (which may be a
    launcher/shell wrapper). Returns the first descendant whose comm looks
    like DOSBox, or `root_pid` itself if it already is. `global_fallback`
    (only for the launch path, where a launcher may have double-forked
    away) permits a system-wide pgrep as a last resort."""
    deadline = time.time() + timeout
    while True:
        stack = [root_pid]
        seen = set()
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            if "dosbox" in _read_comm(pid).lower():
                return pid
            stack.extend(_children(pid))
        if time.time() >= deadline:
            break
        time.sleep(0.2)
    if global_fallback:
        try:
            out = subprocess.run(
                ["pgrep", "-f", "dosbox"], capture_output=True, text=True
            ).stdout.split()
            if out:
                return int(out[0])
        except (OSError, ValueError):
            pass
    raise SystemExit(f"no dosbox process found under pid {root_pid}")


@dataclass
class Region:
    lo: int
    hi: int


def rw_regions(pid: int, min_size: int) -> Iterator[Region]:
    with open(f"/proc/{pid}/maps") as f:
        for line in f:
            parts = line.split()
            span = parts[0].split("-")
            lo, hi = int(span[0], 16), int(span[1], 16)
            if parts[1].startswith("rw") and hi - lo >= min_size:
                yield Region(lo, hi)


class GuestMem:
    """Random-access reader over a process's /proc/<pid>/mem, re-openable
    so a re-exec (pid change) can be followed without losing the tool."""

    def __init__(self, pid: int):
        self.pid = -1
        self.fd = -1
        self.reopen(pid)

    def reopen(self, pid: int) -> None:
        self.close()
        self.fd = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
        self.pid = pid

    def close(self) -> None:
        if self.fd >= 0:
            try:
                os.close(self.fd)
            except OSError:
                pass
            self.fd = -1

    def pread(self, addr: int, size: int) -> Optional[bytes]:
        """Read `size` bytes at host address `addr`; None on fault."""
        try:
            buf = os.pread(self.fd, size, addr)
        except OSError:
            return None
        return buf if len(buf) == size else None

    def read_region(self, r: Region) -> Optional[bytes]:
        return self.pread(r.lo, r.hi - r.lo)


# ---------------------------------------------------------------------------
# Locating the struct
# ---------------------------------------------------------------------------


def build_needle(
    layout: Layout, args: argparse.Namespace
) -> tuple[bytes, int]:
    """Return (needle_bytes, struct_offset_of_needle).

    Default source: the decompressed LEVELS.DAT / CLEVELS record for
    `--level`, which retail embeds pristine at struct+level_rec_off — a
    byte-known, highly distinctive anchor. The needle starts `needle_skip`
    bytes into the record by default (MC2 skips a volatile header byte);
    --needle-rec-off overrides that."""
    rec_off = (
        layout.needle_skip if args.needle_rec_off is None else args.needle_rec_off
    )
    off = layout.level_rec_off + rec_off
    if args.needle_file:
        data = open(args.needle_file, "rb").read()
    else:
        data = extract_level_record(layout, args)
    needle = data[rec_off : rec_off + args.needle_len]
    if len(needle) < args.needle_len:
        raise SystemExit("needle source shorter than requested needle length")
    return needle, off


def extract_level_record(layout: Layout, args: argparse.Namespace) -> bytes:
    """Shell out to the built `mgc-import extract` to decompress one
    LEVELS.DAT entry (the exact bytes retail embeds in the struct)."""
    tool = args.mgc_import or shutil.which("mgc-import") or _find_mgc_import()
    if not tool:
        raise SystemExit(
            "need the level record for the needle: pass --needle-file, or "
            "--mgc-import <path>, or build target/release/mgc-import"
        )
    dat = args.levels_dat or _default_levels(layout, ".DAT")
    tab = args.levels_tab or _default_levels(layout, ".TAB")
    out = os.path.join(_scratch(), f"{layout.name}-lvl{args.level}.bin")
    r = subprocess.run(
        [tool, "extract", dat, tab, str(args.level), out],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0 or not os.path.exists(out):
        raise SystemExit(f"mgc-import extract failed: {r.stderr.strip()}")
    return open(out, "rb").read()


def _find_mgc_import() -> Optional[str]:
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(here)
    for p in ("target/release/mgc-import", "target/debug/mgc-import"):
        cand = os.path.join(root, p)
        if os.path.exists(cand):
            return cand
    return None


def _default_levels(layout: Layout, suffix: str) -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(here)
    rel = {
        "mc1": "gamedata/Magic Carpet Plus/CARPET.CD/LEVELS",
        "mc1hw": "gamedata/Magic Carpet Plus/CARPET.CD/LEVELS",
        "mc2": "gamedata/Magic Carpet 2/GAME/NETHERW/CLEVELS",
    }[layout.name]
    return os.path.join(root, rel, layout.level_archive + suffix)


def _scratch() -> str:
    d = os.environ.get("TMPDIR", "/tmp")
    return d


@dataclass
class Located:
    """Where the struct lives in host memory + the flat-map pin."""

    region: Region
    struct_host: int  # host address of struct byte 0 (HEAP frame)
    static_base: Optional[int] = None  # base of the STATIC-globals frame
    struct_guest: Optional[int] = None  # struct's guest linear address
    build: Optional[BuildVariant] = None  # which retail build is running
    mailbox_host: Optional[int] = None  # host addr of the EXE tick-patch mailbox
    mailbox_period: Optional[int] = None  # its configured spin period (PIT counts)
    # Host addresses of the layout's terrain planes (parallel to
    # Layout.terrain_planes); None = terrain channel unavailable.
    terrain_hosts: Optional[tuple] = None


def locate_struct(
    mem: GuestMem, layout: Layout, needle: bytes, needle_off: int
) -> Located:
    """Scan rw regions for the needle, derive the struct base, validate
    against pool sanity (player = class 3; ≥1 castle = class 3 model 2)."""
    hits: list[tuple[Region, int]] = []
    for r in rw_regions(mem.pid, layout.struct_size):
        blob = mem.read_region(r)
        if blob is None:
            continue
        i = blob.find(needle)
        while i >= 0:
            base = i - needle_off
            if base >= 0 and base + layout.struct_size <= len(blob):
                hits.append((r, r.lo + base))
            i = blob.find(needle, i + 1)
    if not hits:
        raise SystemExit(
            "level-record needle not found — is the level loaded?"
        )
    last = "hit(s) rejected"
    for region, struct_host in hits:
        data = mem.pread(struct_host, layout.struct_size)
        if data is None:
            continue
        ok, why = _validate_struct(data, layout)
        if ok:
            return Located(region=region, struct_host=struct_host)
        last = why
    raise SystemExit(f"{len(hits)} needle hit(s), none valid: {last}")


# MC2 populated-struct signature: every live entity class byte falls in
# this set (0 = free). Load-buffer / disk-cache copies of the level record
# carry a garbage pool and fail the census below.
MC2_ENTITY_CLASSES = frozenset({0, 2, 3, 5, 9, 10, 11, 14, 15})


def _validate_struct(data: bytes, layout: Layout) -> tuple[bool, str]:
    """A populated struct has the current-wizard index in 0..7 and ≥1
    carpet/castle (class 3). Scans ALL pool slots — entities allocate from
    the TOP of the pool (the free stack is built 999→1), so the player's
    carpet and castle sit at high slot numbers (~627-630 on level 0), NOT
    in the low slots. Returns (ok, reason); "no class-3 yet" is the normal
    state between level-load and the first spawn — the caller keeps
    waiting."""
    if layout.family == "mc2":
        return _validate_struct_mc2(data, layout)
    if layout.build_variants:  # header offsets mapped
        widx = _u16(data, layout.wizidx_off)
        if widx > 7:
            return False, f"wizard-idx@{layout.wizidx_off}={widx} (not 0..7)"
    off = layout.pool_off + layout.ent.class_
    st = layout.ent_stride
    class3 = sum(1 for s in range(1, layout.ent_count)
                 if data[off + s * st] == 3)
    if class3 < 1:
        return False, "no class-3 carpet/castle yet (waiting for spawn)"
    return True, f"{class3} class-3 entities"


def _validate_struct_mc2(data: bytes, layout: Layout) -> tuple[bool, str]:
    """Structural filter for MC2's static D41A0_0 struct (no build-variant
    header to gate on). The level-record needle can also match the pristine
    copies retail leaves in load buffers / disk caches; only the real
    struct additionally has a populated, well-formed world at the fixed
    offsets. Rejects a copy unless ALL of: slot 0 free, RNG a real LCG
    value, local index 0..7, player count 1..8, ≥1 class-3, and every one
    of the 1000 pool class bytes is a known entity class."""
    if _u8(data, layout.pool_off + layout.ent.class_) != 0:
        return False, "slot 0 not free"
    rng = _u32(data, layout.rng_off)
    if rng <= 0x10000:
        return False, f"rng@{layout.rng_off}=0x{rng:x} (not a live LCG value)"
    local = _u16(data, layout.localplayer_off)
    if local > 7:
        return False, f"local-player@{layout.localplayer_off}={local} (>7)"
    pcount = _u16(data, layout.localplayer_off + 2)
    if not 1 <= pcount <= 8:
        return False, f"player-count={pcount} (not 1..8)"
    off = layout.pool_off + layout.ent.class_
    st = layout.ent_stride
    class3 = 0
    for s in range(layout.ent_count):
        c = data[off + s * st]
        if c not in MC2_ENTITY_CLASSES:
            return False, f"garbage class {c} at slot {s} (buffer copy)"
        if c == 3:
            class3 += 1
    if class3 < 1:
        return False, "no class-3 carpet/castle yet (waiting for spawn)"
    return True, f"pcount={pcount} local={local} {class3} class-3"


def _owner_chain_ok(data: bytes, layout: Layout, struct_guest: int) -> bool:
    """True if some active entity's owner_ptr (+160) resolves to a wizard
    Type_160 at `struct_guest` — ties the heap struct to the static frame
    (whose struct-pointer global yields the guest address)."""
    e = layout.ent
    for s in range(1, layout.ent_count):
        o = layout.pool_off + s * layout.ent_stride
        if data[o + e.class_] == 0:
            continue
        op = _u32(data, o + e.owner_ptr)
        if op in (0, 0xFFFFFFFF):
            continue
        rel = op - struct_guest - layout.wizards_off - layout.wiz_type160_off
        if (rel >= 0 and rel % layout.wizard_stride == 0
                and rel // layout.wizard_stride < layout.wizard_count):
            return True
    return False


def find_static_base(
    mem: GuestMem, layout: Layout, struct_data: bytes
) -> Optional[tuple[int, BuildVariant, int]]:
    """Locate the STATIC frame that holds the game's globals (wall clock,
    raw input, struct pointer). DOS4GW does NOT map the heap and the
    static segment with a single affine base, so the struct (found by the
    level-record needle) and the globals live in independent frames; this
    finds the static one by its own content landmark. Scan for byte_99B58,
    derive the static base, read the struct-pointer global there, and
    accept when it resolves against the heap struct's own owner_ptr chain.
    Returns (static_base, build, struct_guest) or None."""
    for r in rw_regions(mem.pid, 1 << 16):
        blob = mem.read_region(r)
        if blob is None:
            continue
        i = blob.find(layout.static_needle)
        while i >= 0:
            hit = r.lo + i
            for v in layout.build_variants:
                static_base = hit - v.static_needle_guest
                if static_base < 0:
                    continue
                pv = mem.pread(static_base + v.struct_ptr_guest, 4)
                if pv is None:
                    continue
                struct_guest = struct.unpack("<I", pv)[0]
                if struct_guest and _owner_chain_ok(
                        struct_data, layout, struct_guest):
                    return static_base, v, struct_guest
            i = blob.find(layout.static_needle, i + 1)
    return None


def _pid_alive(pid: int) -> bool:
    """True unless the pid is gone or a reaped zombie. We check the ACTUAL
    attached pid, never the launcher `child` handle — a wrapper/re-exec
    can exit while dosbox keeps running as a different process."""
    try:
        with open(f"/proc/{pid}/stat", "rb") as f:
            data = f.read()
    except OSError:
        return False
    # "pid (comm) STATE ..."; comm may contain spaces/parens.
    state = data[data.rfind(b")") + 2:].split(b" ", 1)[0]
    return state != b"Z"


def ensure_attached(
    mem: GuestMem, launch_root: int, child: Optional[subprocess.Popen]
) -> bool:
    """Keep `mem` attached to a live dosbox. If the current pid died,
    re-acquire (a launcher may have exited, or dosbox re-execed into a new
    pid) and reopen. False only when no dosbox process can be found."""
    if _pid_alive(mem.pid):
        return True
    try:
        newpid = find_dosbox_descendant(
            launch_root, timeout=1.0, global_fallback=child is not None)
    except SystemExit:
        return False
    try:
        mem.reopen(newpid)
    except OSError:
        return False
    print(f"re-attached to dosbox pid {newpid}", file=sys.stderr)
    return True


def wait_for_struct(
    mem: GuestMem, layout: Layout, needle: bytes, needle_off: int,
    timeout: float, launch_root: int, child: Optional[subprocess.Popen],
) -> Located:
    """Block until the struct can be located (level-record needle) AND has
    a live world in it. Wait patiently (indefinitely if timeout<=0) — the
    player may still be in menus / intro FMVs."""
    deadline = None if timeout <= 0 else time.time() + timeout
    last_note = 0.0
    while True:
        if not ensure_attached(mem, launch_root, child):
            raise SystemExit("dosbox exited before gameplay started")
        try:
            return locate_struct(mem, layout, needle, needle_off)
        except (SystemExit, OSError) as exc:
            reason = str(exc)
        now = time.time()
        if now - last_note > 3.0:
            print(f"waiting for gameplay to start… ({reason})",
                  file=sys.stderr)
            last_note = now
        if deadline and now > deadline:
            raise SystemExit("timed out waiting for the struct to populate")
        time.sleep(0.25)


def _live_probe(mem: GuestMem, loc: Located, layout: Layout):
    """A tuple that changes iff the sim advanced: the gameplay RNG
    (struct+4, stepped only inside the sim tick — menus use a different
    RNG) plus the wall clock when pinned."""
    d = mem.pread(loc.struct_host + layout.rng_off, 4)
    if d is None:
        return None
    return (struct.unpack("<I", d)[0], read_wallclock(mem, loc, layout))


def wait_until_live(
    mem: GuestMem, loc: Located, layout: Layout, timeout: float,
    launch_root: int, child: Optional[subprocess.Popen],
) -> None:
    """Block until the sim is actually ticking (gameplay begun / unpaused)
    so recording starts on the first live tick, not on a frozen pool."""
    deadline = None if timeout <= 0 else time.time() + timeout
    prev = _live_probe(mem, loc, layout)
    last_note = time.time()
    while True:
        if not ensure_attached(mem, launch_root, child):
            raise SystemExit("dosbox exited before gameplay began")
        time.sleep(0.15)
        cur = _live_probe(mem, loc, layout)
        if prev is not None and cur is not None and cur != prev:
            print("gameplay is live — recording.", file=sys.stderr)
            return
        prev = cur if cur is not None else prev
        now = time.time()
        if now - last_note > 3.0:
            print("waiting for gameplay to begin (sim frozen — paused or "
                  "still in a menu)…", file=sys.stderr)
            last_note = now
        if deadline and now > deadline:
            raise SystemExit("timed out waiting for gameplay to begin")


# ---------------------------------------------------------------------------
# Decoding a snapshot
# ---------------------------------------------------------------------------


def _u8(d, o):
    return d[o]


def _i8(d, o):
    return struct.unpack_from("<b", d, o)[0]


def _u16(d, o):
    return struct.unpack_from("<H", d, o)[0]


def _i16(d, o):
    return struct.unpack_from("<h", d, o)[0]


def _u32(d, o):
    return struct.unpack_from("<I", d, o)[0]


def _i32(d, o):
    return struct.unpack_from("<i", d, o)[0]


def decode_entity(d: bytes, base: int, layout: Layout, slot: int) -> dict:
    if layout.family == "mc2":
        return _decode_entity_mc2(d, base, layout, slot)
    e = layout.ent
    o = base + slot * layout.ent_stride
    return {
        "slot": slot,
        "class": _u8(d, o + e.class_),
        "model": _u8(d, o + e.model),
        "sclass": _u8(d, o + e.sclass),
        "smodel": _u8(d, o + e.smodel),
        "flags": _u32(d, o + e.flags),
        "id": _u16(d, o + e.id24),
        "life": _i32(d, o + e.act_life),
        "max_life": _u32(d, o + e.max_life),
        "x": _u16(d, o + e.x) / 256.0,
        "y": _u16(d, o + e.y) / 256.0,
        "z": _i16(d, o + e.z),
        "heading": _u16(d, o + e.heading),  # applied yaw 0..2047
        "pitch": _u16(d, o + e.pitch),
        "target_yaw": _u16(d, o + e.target_yaw),
        "speed": _i16(d, o + e.speed),
        "mana": _u32(d, o + e.f140),  # current mana (player carpet)
        "mana_max": _u32(d, o + e.f136),
        "chase": _u16(d, o + e.chase),
        "owner_ptr": _u32(d, o + e.owner_ptr),
        "tick_byte": _u8(d, o + e.tick_byte),  # ++ once per tick (:52406)
        "rand": _u32(d, o + e.rand),
    }


def _decode_entity_mc2(d: bytes, base: int, layout: Layout, slot: int) -> dict:
    """One MC2 (D41A0_0) 168-byte pool record. ``heading``/``pitch`` are
    the world-space orientation — the LIVE facing that tracks flight (a
    700-tick recording confirmed it; the applied camera/target yaw @0x52
    rests at a constant for the player and is captured separately).
    Life/mana/pos/speed are all in the entity here."""
    e = layout.ent
    o = base + slot * layout.ent_stride
    return {
        "slot": slot,
        "class": _u8(d, o + e.class_),
        "model": _u8(d, o + e.model),
        "life": _i32(d, o + e.act_life),
        "max_life": _i32(d, o + e.max_life),
        "x": _u16(d, o + e.x) / 256.0,
        "y": _u16(d, o + e.y) / 256.0,
        "z": _i16(d, o + e.z),
        "heading": _i16(d, o + e.heading),  # world yaw = live facing
        "pitch": _i16(d, o + e.pitch),  # world pitch
        "applied_yaw": _i16(d, o + e.applied_yaw),  # camera/target (constant)
        "applied_pitch": _i16(d, o + e.applied_pitch),
        "speed": _i16(d, o + e.speed),
        "mana": _i32(d, o + e.f140),
        "mana_max": _i32(d, o + e.f136),
        "owner": _u16(d, o + e.id24),  # parentId
        "action": _u8(d, o + e.action),  # state-machine action index
        "sv1": _u8(d, o + e.stagevar1),
        "sv2": _u8(d, o + e.stagevar2),
        "player_ent_idx": _u16(d, o + e.player_ent_idx),
        "rand": _u16(d, o + e.rand),
    }


def decode_control(d: bytes, layout: Layout, player: int) -> dict:
    """One player's 10-byte control-command record — the processed
    per-tick input that actually drives the carpet (:49017-49021)."""
    o = layout.ctrlcmd_off + player * layout.ctrlcmd_stride
    move = _u8(d, o + 5)
    return {
        "player": player,
        "opcode": _u8(d, o + 0),  # 0 = empty this tick
        "param1": _u8(d, o + 1),
        "param2": _u8(d, o + 2),
        "aim_yaw": _i8(d, o + 3),  # from mouse-X, [-127..127]
        "aim_pitch": _i8(d, o + 4),  # from mouse-Y
        "move_fire": move,  # 1 thrust 2 decel 4/8 strafe 16/32 fire L/R
        "thrust": bool(move & 1),
        "decel": bool(move & 2),
        "strafe_left": bool(move & 4),
        "strafe_right": bool(move & 8),
        "fire_left": bool(move & 16),
        "fire_right": bool(move & 32),
    }


def _hand(v: int) -> Optional[int]:
    return None if v in (0xFFFF, 0xFF) else v  # 0xFFFF = empty hand


def decode_wizard(d: bytes, layout: Layout, i: int) -> dict:
    w = layout.wizards_off + i * layout.wizard_stride
    t160 = w + layout.wiz_type160_off
    return {
        "index": i,
        "play_index": _u16(d, w + layout.wiz_playindex_off),
        "hand_left": _hand(_u16(d, t160 + layout.t160_hand_left_off)),
        "hand_right": _hand(_u16(d, t160 + layout.t160_hand_right_off)),
        "castle": _u16(d, t160 + 50),  # var_50: established castle slot
        # Persistent flight/steering state (survives the tick, unlike the
        # control slot): commanded speed v_12@12, strafe v_16@16, and the
        # roll/pitch accumulators @327/@329 that drive heading and pitch.
        "flight": {
            "cmd_speed": _i16(d, t160 + 12),
            "strafe": _i16(d, t160 + 16),
            "roll_acc": _u16(d, t160 + 327),
            "pitch_acc": _u16(d, t160 + 329),
        },
    }


def _name(d: bytes, o: int, cap: int = 24) -> str:
    return d[o : o + cap].split(b"\x00", 1)[0].decode("latin1", "replace")


def _hand_i16(v: int) -> Optional[int]:
    return None if v < 0 else v  # MC2 spell hands: -1 = empty


def decode_player_mc2(d: bytes, layout: Layout, i: int) -> dict:
    """One MC2 2124-byte per-player block. Unlike MC1's Type_160 column,
    mana is NOT here (it's on the carpet entity); this block carries the
    identity, the per-frame Turn counter (continuity), the equipped spell
    hands and the established castle slot."""
    b = layout.wizards_off + i * layout.wizard_stride
    t = b + layout.pp_flight_off  # type_str_164 flight column
    return {
        "index": i,
        "is_ai": bool(_u8(d, b + layout.pp_isai_off)),
        "play_index": _u16(d, b + layout.wiz_playindex_off),  # carpet slot
        "turn": _i32(d, b + layout.pp_turn_off),  # per-frame counter
        "name": _name(d, b + layout.pp_name_off),
        "castle": _i16(d, b + layout.pp_castle_off),  # 0 = none established
        "hand_left": _hand_i16(_i16(d, b + layout.pp_hand_left_off)),
        "hand_right": _hand_i16(_i16(d, b + layout.pp_hand_right_off)),
        # Persistent flight-command accumulators (the steering intent that
        # survives the tick, unlike the zeroed control slot). MC1-parallel
        # layout: cmd_speed @+12 (verified == the carpet's forward speed
        # across resting + mid-steer dumps), the strafe/second-speed slot
        # @+16. The commanded direction is the carpet `heading` itself.
        "flight": {
            "cmd_speed": _i16(d, t + 12),
            "v16": _i16(d, t + 16),
        },
    }


def decode_control_mc2(d: bytes, layout: Layout, player: int) -> dict:
    """One MC2 10-byte control command. Same shape as MC1's, but the
    opcode encodes the cast (31 = cast-left, 32 = cast-right, 20 = button);
    the game consumes and zeroes it each frame, so it reads empty at a
    between-tick snapshot (persistent aim lives in the in-struct mouse)."""
    o = layout.ctrlcmd_off + player * layout.ctrlcmd_stride
    return {
        "player": player,
        "opcode": _u8(d, o + 0),  # 0 = empty; 31/32 cast L/R; 20 button
        "param1": _u8(d, o + 1),
        "param2": _u8(d, o + 2),
        "aim_yaw": _i8(d, o + 3),
        "aim_pitch": _i8(d, o + 4),
        "buttons": _u8(d, o + 5),  # fire/button bits (raw)
    }


def decode_snapshot(d: bytes, layout: Layout) -> dict:
    """The ``obs`` channel: the decoded observable projection of one
    quiescent struct image (docs/RECORDING.md). The record's ``t`` key
    owns the tick number; this dict is pure state."""
    local = _u16(d, layout.localplayer_off)
    pcount = _u16(d, layout.localplayer_off + 2)
    ents = [
        decode_entity(d, layout.pool_off, layout, s)
        for s in range(1, layout.ent_count)
        if d[layout.pool_off + s * layout.ent_stride + layout.ent.class_] != 0
    ]
    snap = {
        "rng": _u32(d, layout.rng_off),
        "n_active": len(ents),
    }
    if layout.family == "mc2":  # per-player blocks + control + join
        players = [decode_player_mc2(d, layout, p) for p in range(pcount)]
        controls = [decode_control_mc2(d, layout, p) for p in range(pcount)]
        snap.update({
            "local_player": local,
            "player_count": pcount,
            "players": players,
            "control": controls,
            "player": _player_join_mc2(ents, players, controls, local),
        })
    elif layout.build_variants:  # MC1/HW header + wizards + control mapped
        wizards = [decode_wizard(d, layout, i)
                   for i in range(layout.wizard_count)]
        controls = [decode_control(d, layout, p)
                    for p in range(layout.ctrlcmd_count)]
        snap.update({
            "local_player": local,
            "player_count": pcount,
            "wizards": wizards,
            "control": controls,
            "player": _player_join(ents, wizards, controls, local),
        })
    snap["entities"] = ents
    return snap


def _player_join_mc2(ents, players, controls, local) -> Optional[dict]:
    """The human's carpet state, joined per-player-block→pool→control.
    Mirrors :func:`_player_join`'s output keys so the sanity print and any
    downstream consumer stay uniform across games."""
    if local >= len(players):
        return None
    p = players[local]
    slot = p["play_index"]
    carpet = next((e for e in ents if e["slot"] == slot), None)
    if carpet is None:
        return None
    ctrl = controls[local] if local < len(controls) else None
    return {
        "carpet_slot": slot,
        "name": p["name"],
        "is_ai": p["is_ai"],
        "turn": p["turn"],  # per-frame continuity counter
        "life": carpet["life"],
        "max_life": carpet["max_life"],
        "mana": carpet["mana"],
        "mana_max": carpet["mana_max"],
        "x": carpet["x"], "y": carpet["y"], "z": carpet["z"],
        "heading": carpet["heading"], "pitch": carpet["pitch"],
        "applied_yaw": carpet["applied_yaw"],
        "applied_pitch": carpet["applied_pitch"],
        "speed": carpet["speed"],
        "hand_left": p["hand_left"],
        "hand_right": p["hand_right"],
        "castle": p["castle"],
        "flight": p["flight"],  # persistent steering command (cmd_speed …)
        "control": ctrl,  # processed input this tick (0 at snapshot — consumed)
    }


def _player_join(ents, wizards, controls, local) -> Optional[dict]:
    """The human's carpet state, joined from wizard→pool→control."""
    if local >= len(wizards):
        return None
    slot = wizards[local]["play_index"]
    carpet = next((e for e in ents if e["slot"] == slot), None)
    if carpet is None:
        return None
    ctrl = controls[local] if local < len(controls) else None
    return {
        "carpet_slot": slot,
        "life": carpet["life"],
        "max_life": carpet["max_life"],
        "mana": carpet["mana"],
        "mana_max": carpet["mana_max"],
        "x": carpet["x"], "y": carpet["y"], "z": carpet["z"],
        "heading": carpet["heading"], "pitch": carpet["pitch"],
        "speed": carpet["speed"],
        "hand_left": wizards[local]["hand_left"],
        "hand_right": wizards[local]["hand_right"],
        "castle": wizards[local]["castle"],
        "flight": wizards[local]["flight"],  # persistent steering state
        "control": ctrl,  # processed input this tick (0 at snapshot — consumed)
    }


# ---------------------------------------------------------------------------
# Output: the .mgcr sink, the header record, one tick record
# ---------------------------------------------------------------------------


class RecordSink:
    """The recording stream (docs/RECORDING.md is normative): line 1 the
    header record, every later line one tick record. ``-`` (stdout) and
    paths ending in ``.jsonl`` write plain JSONL; any other path writes
    the ``.mgcr`` container (a zstd stream of the same lines). The zstd
    writer closes a full frame every FLUSH_EVERY records, bounding what a
    crash can lose; a sink that never received a record deletes its file
    on close (a failed run leaves no empty artifact)."""

    FLUSH_EVERY = 32

    def __init__(self, path: str):
        self.path = path
        self._n = 0
        self._closed = False
        self._zw = self._zstd = self._raw = None
        if path == "-":
            self._fh, self._own = sys.stdout, False
        elif path.endswith(".jsonl"):
            self._fh, self._own = open(path, "w"), True
        else:
            try:
                import zstandard
            except ImportError as exc:
                raise SystemExit(
                    f"writing {path} needs the `zstandard` package; "
                    "install it, or use a .jsonl path for uncompressed "
                    "output") from exc
            self._zstd = zstandard
            self._raw = open(path, "wb")
            self._zw = zstandard.ZstdCompressor(level=9).stream_writer(
                self._raw)
            self._fh, self._own = None, True

    def write(self, rec: dict) -> None:
        line = json.dumps(rec, separators=(",", ":")) + "\n"
        self._n += 1
        if self._zw is not None:
            self._zw.write(line.encode())
            if self._n % self.FLUSH_EVERY == 0:
                self._zw.flush(self._zstd.FLUSH_FRAME)
        else:
            self._fh.write(line)
            self._fh.flush()

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self._zw is not None:
            self._zw.close()  # ends the frame (also closes the raw file)
            self._raw.close()  # idempotent
        elif self._own:
            self._fh.close()
        if self._n == 0 and self._own:
            try:
                os.unlink(self.path)
            except OSError:
                pass


def build_header(args: argparse.Namespace, layout: Layout, loc: Located,
                 cmd: list) -> dict:
    """The header record (docs/RECORDING.md, line 1)."""
    have_ext = (loc.static_base is not None
                or layout.in_struct_mouse_off is not None)
    channels: dict = {
        "input": "raw" if have_ext else "none",
        "obs": True,
        "state": not args.no_state,
        "hash": False,
    }
    # The terrain channel makes the recording format 2 (docs/RECORDING.md:
    # format 2 = format 1 + the optional terrain channel; writers stamp 2
    # exactly when the header declares it). The plane list is the PINNED
    # set (the cave ceiling appears only on MC2 cave levels).
    if loc.terrain_hosts is not None:
        channels["terrain"] = {
            "planes": [name for name, _ in loc.terrain_hosts],
            "dims": list(layout.terrain_dims),
        }
    hdr: dict = {
        "type": "header",
        "format": 2 if loc.terrain_hosts is not None else 1,
        "game": layout.name,
        "level": args.level,
        "source": "retail",
        # Nominal replay cadence — the port's TICK_RATE_HZ. Retail MC2
        # genuinely turns once per 24 fps frame; retail MC1 ran uncapped,
        # so 24 is the port's chosen cadence, not a retail measurement.
        "tick_hz": 24,
        "channels": channels,
        "tool": {"name": "mc_dosbox_recorder", "git": _git_rev()},
        "created": datetime.datetime.now(datetime.timezone.utc)
        .strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    if loc.build is not None:
        hdr["build"] = loc.build.name
    hdr["capture"] = (
        {"samples": args.samples, "cmd": cmd} if cmd
        else {"samples": args.samples, "attached_pid": args.pid})
    # Emitted snapshots passed the inter-tick tear gate (pair_clean);
    # recordings without this flag predate it and carry mid-pass
    # states — fixture runners must re-classify their pairs.
    hdr["capture"]["tear_gate"] = layout.family == "mc1"
    # When a tick-patched exe is running, snapshots are window-gated (taken
    # while the stub's in_window flag is raised) — strictly stronger than
    # the tear gate, and each `t` is the stub's authoritative sub-step
    # counter rather than a +63-mode estimate.
    if loc.mailbox_host is not None:
        hdr["capture"]["window_gated"] = True
        hdr["capture"]["exe_patch"] = {
            "mailbox_guest": EXE_MB2_BASE if layout.family == "mc2" else EXE_MB_BASE,
            "spin_period_counts": loc.mailbox_period,  # null for MC2 (signal-only)
        }
    return hdr


def _git_rev() -> Optional[str]:
    try:
        return subprocess.run(
            ["git", "-C", os.path.dirname(os.path.abspath(__file__)),
             "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, timeout=5,
        ).stdout.strip() or None
    except Exception:
        return None


def build_record(tick: int, data: bytes, layout: Layout, mem: GuestMem,
                 loc: Located, want_state: bool) -> dict:
    """One tick record. Phase convention (docs/RECORDING.md): ``obs`` /
    ``state`` describe the world AT tick ``t``; ``input`` is the
    persistent raw input at that boundary — the best estimate of what the
    tick t → t+1 will consume."""
    rec: dict = {"t": tick, "obs": decode_snapshot(data, layout)}
    wall = read_wallclock(mem, loc, layout)
    if wall is not None:
        rec["wallclock"] = wall
    ext, ext_raw = read_externals(mem, loc, layout, data)
    if ext is not None:
        rec["input"] = ext
    if want_state:
        st = {"struct_b64": _b64(data)}
        if ext_raw:
            st["ext"] = ext_raw
        rec["state"] = st
    return rec


# ---------------------------------------------------------------------------
# The poll loop (consensus + continuity)
# ---------------------------------------------------------------------------


def volatile_view(data: bytes, layout: Layout) -> bytes:
    if not layout.volatile_ranges:
        return data
    return b"".join(data[a:b] for (a, b) in layout.volatile_ranges)


def read_wallclock(mem: GuestMem, loc: Located, layout: Layout) -> Optional[int]:
    """Read dword_AC5D4 — a free-running ~120 Hz PIT clock. NOT a logic-
    tick counter (it advances asynchronously to the sim); recorded only
    as a liveness / ordering signal. Lives in the static frame."""
    if loc.static_base is None or loc.build is None:
        return None
    if loc.build.wallclock_guest == 0:  # not mapped (MC2)
        return None
    v = mem.pread(loc.static_base + loc.build.wallclock_guest, 4)
    return None if v is None else struct.unpack("<I", v)[0]


def read_externals(
    mem: GuestMem, loc: Located, layout: Layout, data: Optional[bytes] = None
) -> tuple[Optional[dict], Optional[dict]]:
    """Raw, PERSISTENT input — held scancodes, the mouse cursor (MC1's aim
    source) and held mouse buttons (fire). Unlike the in-struct control
    slot, which retail zeroes mid-tick after consuming it (so it reads
    empty at our between-tick snapshot), these survive across ticks.

    Returns ``(decoded, raw)``: the decoded ``input`` channel, and the raw
    register images for the ``state`` channel's closure (``ext``) — both
    derived from ONE read of each register, so they can never disagree.

    On MC1/HW the registers live in the separate static frame (pinned
    separately); on MC2 the persistent aim is inside the master struct, so
    it is read from the quiescent `data` snapshot directly (and the struct
    image already carries it — no separate raw slice needed)."""
    if layout.in_struct_mouse_off is not None:
        return _externals_mc2(data, layout), None
    if loc.static_base is None or loc.build is None:
        return None, None
    b, sb = loc.build, loc.static_base
    ext: dict = {}
    raw: dict = {}
    keys = mem.pread(sb + b.pressed_keys_guest, layout.pressed_keys_len)
    if keys is not None:
        ext["keys_down"] = [i for i, v in enumerate(keys) if v]
        raw["keys_b64"] = _b64(keys)
    cur = mem.pread(sb + b.mouse_cursor_guest, 4)
    if cur is not None:
        x, y = struct.unpack("<hh", cur)
        ext["mouse"] = {"x": x, "y": y}  # absolute cursor → aim
        raw["cursor_b64"] = _b64(cur)
    lb = mem.pread(sb + b.mouse_lbtn_guest, 2)
    rb = mem.pread(sb + b.mouse_rbtn_guest, 2)
    if lb is not None and rb is not None:
        ext["mouse_buttons"] = {
            "left": struct.unpack("<h", lb)[0] != 0,
            "right": struct.unpack("<h", rb)[0] != 0,
        }
        raw["lbtn_b64"] = _b64(lb)
        raw["rbtn_b64"] = _b64(rb)
    # MC2 extras: the press LATCHES (edge-set, catch sub-poll clicks)
    # and the game's own cursor-at-press snapshot (fly-assistant
    # watchdog datum; the aim source is the live cursor @0xE3760).
    if b.mouse_latchl_guest and b.mouse_latchr_guest:
        ll = mem.pread(sb + b.mouse_latchl_guest, 2)
        lr = mem.pread(sb + b.mouse_latchr_guest, 2)
        if ll is not None and lr is not None:
            ext["mouse_clicks"] = {
                "left": struct.unpack("<h", ll)[0] != 0,
                "right": struct.unpack("<h", lr)[0] != 0,
            }
            raw["latch_b64"] = _b64(ll + lr)
    if b.press_pos_guest:
        pp = mem.pread(sb + b.press_pos_guest, 4)
        if pp is not None:
            x, y = struct.unpack("<hh", pp)
            ext["mouse_press_pos"] = {"x": x, "y": y}
            raw["press_b64"] = _b64(pp)
    return ext or None, raw or None


def _b64(b: bytes) -> str:
    return base64.b64encode(b).decode("ascii")


def _externals_mc2(data: Optional[bytes], layout: Layout) -> Optional[dict]:
    """MC2's persistent aim: the in-struct mouse {i16 x, i16 y} plus the
    fly-assist toggle just before it. Read from the same quiescent
    snapshot as the rest of the state."""
    if data is None:
        return None
    o = layout.in_struct_mouse_off
    return {
        "mouse": {"x": _i16(data, o), "y": _i16(data, o + 2)},
        "fly_assist": _i16(data, o - 2),
    }


# ---------------------------------------------------------------------------
# Terrain planes (the format-2 terrain channel)
# ---------------------------------------------------------------------------


def _terrain_cells(layout: Layout) -> int:
    w, h = layout.terrain_dims
    return w * h


def pin_terrain(mem: GuestMem, loc: Located, layout: Layout) -> None:
    """Resolve the terrain-plane block to per-plane host addresses and
    validate them. Leaves ``loc.terrain_hosts`` None (and the recording
    on format 1, no terrain channel) when the block cannot be resolved
    or fails validation — a wrong frame must never record garbage as
    terrain.

    Both engines keep the planes as CONTIGUOUS statics off one base
    (`BuildVariant.terrain_guest`) in the frame the recorder already
    pins: MC1/HW the byte_99B58 static frame (build B −0x10), MC2 the
    struct-anchored data image. Validators (from the decompile digs):
    MC1/HW shading is hard-clamped to [28,47] by every writer AND
    all-zero until the level generator's final bake — one check gates
    both alignment and readiness; MC2 height is generator-clamped ≤196
    (terraform can push a handful of cells past — soft 99% bound)."""
    if not layout.terrain_planes:
        print("terrain: this layout declares no terrain planes — recording "
              "without the terrain channel", file=sys.stderr)
        return
    if loc.static_base is None or loc.build is None:
        print("terrain: no static frame — recording without the terrain "
              "channel", file=sys.stderr)
        return
    if loc.build.terrain_guest == 0:
        print(f"terrain: build {loc.build.name} has no terrain base address "
              "— recording without the terrain channel", file=sys.stderr)
        return
    n = _terrain_cells(layout)
    base = loc.static_base + loc.build.terrain_guest
    planes = list(layout.terrain_planes)
    if layout.terrain_cave_plane is not None and layout.terrain_cave_byte_off:
        mt = mem.pread(loc.struct_host + layout.terrain_cave_byte_off, 1)
        if mt is not None and mt[0] == 2:  # MapType Cave
            planes.append(layout.terrain_cave_plane)
            print("terrain: cave level — capturing the ceiling plane too",
                  file=sys.stderr)
    hosts, why = [], {}
    for name, off in planes:
        host = base + off
        a = mem.pread(host, n)
        b = mem.pread(host, n)
        if a is None or b is None or a != b:
            print(f"terrain: plane '{name}' unreadable/unstable @host "
                  f"0x{host:x} — recording without the terrain channel",
                  file=sys.stderr)
            return
        why[name] = _implausible_plane(layout, name, a)
        hosts.append((name, host))

    # Weigh the failures rather than rejecting on the first one, because the
    # planes are not equally good witnesses. For MC1 the SHADING plane answers
    # both questions the validators exist to ask — is the frame aligned, and
    # has the generator finished — and answers them with no tolerance at all:
    # one cell of 65536 outside [28,47] fails it, so a misaligned frame cannot
    # survive it. Once shading has vouched, a height reading past the
    # generator's clamp is DATA, not misalignment: building stamps are added
    # unclamped, so a level authored high carries thousands of such cells
    # legitimately (mc1l32 is a maze in the sky — 1695 cells, 2.6%, against a
    # 1% bound calibrated on low-lying levels that measure exactly 0). Warn
    # there instead of dropping the channel; the old behaviour cost mc1l32
    # four takes, each silently degraded to format 1 and unrepairable after
    # the fact. MC2's shading has no validator, so height stays its primary
    # alignment check and stays fatal.
    shading_is_oracle = layout.family == "mc1"  # mirrors _implausible_plane
    vouched = (shading_is_oracle and "shading" in why
               and why["shading"] is None)
    fatal = [(nm, w) for nm, w in why.items()
             if w is not None and not (vouched and nm == "height")]
    if not fatal:
        for nm, w in why.items():
            if w is not None:
                print(f"terrain: plane '{nm}' is outside the generator's "
                      f"clamp ({w}), but shading vouches for the frame — "
                      f"keeping the terrain channel", file=sys.stderr)
    if fatal:
        nm, w = fatal[0]
        print(f"terrain: plane '{nm}' failed validation ({w}) — "
              "recording without the terrain channel", file=sys.stderr)
        return
    loc.terrain_hosts = tuple(hosts)
    names = ", ".join(nm for nm, _ in hosts)
    print(f"terrain planes pinned ({names}), {n} cells each — "
          "format-2 terrain channel on", file=sys.stderr)


def _implausible_plane(layout: Layout, name: str, data: bytes
                       ) -> Optional[str]:
    """None if the plane image is plausible, else the reason."""
    if name == "shading" and layout.family == "mc1":
        # Every retail writer clamps shading into [28,47] (bake
        # sub_329C0 :40248-63, repaints :41139-47/:41432-40), and the
        # plane is all-zero until the generator's final bake — so this
        # one check rejects both a misaligned frame and a not-yet-
        # generated level.
        bad = sum(1 for v in data if not 28 <= v <= 47)
        if bad:
            return f"{bad} shading cells outside [28,47]"
        return None
    if name == "height":
        # Generation clamps ≤196 (MC1 :40296-305, MC2 Terrain.cpp:103);
        # building stamps are added UNCLAMPED, so require 99% within
        # bounds, not all. The 1% is a floor under MC2, where nothing
        # else checks alignment — it is NOT a bound on how high a level
        # may legitimately sit. Measured on MC1: seven low-lying levels
        # give exactly 0 cells over 200 (max height 169-192), while
        # mc1l32, a maze in the sky, gives 1695 (2.6%). pin_terrain
        # therefore treats this as fatal only when shading has not
        # already vouched for the frame.
        over = sum(1 for v in data if v > 200)
        if over > len(data) // 100:
            return f"{over} height cells > 200"
        if len(set(data)) < 8:
            return "near-constant height field"
    return None


def read_terrain(mem: GuestMem, loc: Located, layout: Layout
                 ) -> Optional[list]:
    """One read of every pinned plane, or None (also on a read fault)."""
    if loc.terrain_hosts is None:
        return None
    n = _terrain_cells(layout)
    planes = []
    for _name, host in loc.terrain_hosts:
        p = mem.pread(host, n)
        if p is None:
            return None
        planes.append(p)
    return planes


def _diff_plane(prev: bytes, cur: bytes, row: int = 256) -> list:
    """Changed cells as (index, new_value). Row-chunked: the whole-plane
    equality check (C speed) catches the common no-edit record, and only
    rows that differ get the per-byte scan — a terraform window touches
    tens of cells in a handful of rows."""
    if prev == cur:
        return []
    cells = []
    for o in range(0, len(cur), row):
        if prev[o:o + row] == cur[o:o + row]:
            continue
        for i in range(o, min(o + row, len(cur))):
            if prev[i] != cur[i]:
                cells.append((i, cur[i]))
    return cells


class TerrainDiffer:
    """Turns per-record plane images into the format-2 terrain channel
    (docs/RECORDING.md): the first EMITTED record carries the full base
    image; every later one a delta against the PREVIOUS EMITTED record —
    so a `t` gap's edits are simply contained in the next record's delta
    (self-healing). Returns None for a no-change record (the `terrain`
    key is omitted entirely)."""

    def __init__(self) -> None:
        self.prev: Optional[list] = None

    def channel(self, planes: Optional[list]) -> Optional[dict]:
        if planes is None:
            return None
        if self.prev is None:
            self.prev = planes
            return {"base_b64": _b64(b"".join(planes))}
        parts = []
        changed = False
        for pv, cv in zip(self.prev, planes):
            cells = _diff_plane(pv, cv)
            changed = changed or bool(cells)
            parts.append(struct.pack("<I", len(cells)) + b"".join(
                struct.pack("<HB", i, v) for i, v in cells))
        self.prev = planes
        return {"delta_b64": _b64(b"".join(parts))} if changed else None


def attach_terrain(rec: dict, differ: Optional["TerrainDiffer"],
                   planes: Optional[list]) -> dict:
    """Attach the terrain channel to a record at WRITE time (the base/
    delta decision depends on emission order, so it cannot happen at
    build_record time — the deferred first record may be rebuilt)."""
    if differ is not None:
        ch = differ.channel(planes)
        if ch is not None:
            rec["terrain"] = ch
    return rec


def capture_clean(
    mem: GuestMem, loc: Located, layout: Layout, samples: int, retries: int
) -> Optional[tuple]:
    """Return ``(struct_bytes, terrain_planes)`` once `samples`
    consecutive reads agree on the volatile state (terrain planes, when
    pinned, are part of the consensus — they must be byte-stable across
    the same frozen window; ``terrain_planes`` is None when unpinned).
    ⚠ Consensus proves only that the guest was FROZEN for the window —
    DOSBox regularly parks MID-entity-loop, so a consensus image can be
    a mid-tick state (half the pool stepped). `pair_clean` in the poll
    loop is the actual inter-tick gate; this alone only rules out READ
    tearing. None if no stable window was caught in `retries` attempts
    (sim faster than we can snapshot)."""
    for _ in range(retries):
        first = mem.pread(loc.struct_host, layout.struct_size)
        if first is None:
            return None
        tfirst = read_terrain(mem, loc, layout)
        vfirst = volatile_view(first, layout)
        stable = True
        for _ in range(samples - 1):
            nxt = mem.pread(loc.struct_host, layout.struct_size)
            if nxt is None:
                return None
            if volatile_view(nxt, layout) != vfirst:
                stable = False
                break
            if loc.terrain_hosts is not None:
                tnxt = read_terrain(mem, loc, layout)
                if tnxt != tfirst:
                    stable = False
                    break
        if stable:
            return first, tfirst
    return None


def tick_delta(prev: bytes, cur: bytes, layout: Layout) -> Optional[int]:
    """Logic ticks elapsed between two quiescent snapshots.

    MC1/HW have NO global logic-tick counter, but each active entity's
    +63 byte increments exactly once per tick (:52406). Across entities
    that persisted unchanged (same class+model), the MODE of the +63
    increment is the true tick count — idle entities (castles especially)
    all step +1/tick and outvote any that reset on a state change.
    None if too few stable entities to trust (fall back to change-detect).
    Note: wraps mod 256, so keep polling faster than 256 ticks/sample.

    MC2 instead exposes a real per-frame Turn counter in the local
    player's block; at the default game speed one frame is one sim tick,
    so its forward delta is the tick count directly."""
    if layout.continuity == "turn":
        return _tick_delta_turn(prev, cur, layout)
    from collections import Counter

    e = layout.ent
    ps, st = layout.pool_off, layout.ent_stride
    votes: Counter = Counter()
    for s in range(1, layout.ent_count):
        o = ps + s * st
        c = cur[o + e.class_]
        if c == 0 or c != prev[o + e.class_]:
            continue
        if cur[o + e.model] != prev[o + e.model]:
            continue
        votes[(cur[o + e.tick_byte] - prev[o + e.tick_byte]) & 0xFF] += 1
    if sum(votes.values()) < 3:
        return None
    return votes.most_common(1)[0][0]


def _tick_delta_turn(prev: bytes, cur: bytes, layout: Layout) -> Optional[int]:
    """Ticks elapsed = forward delta of the local player's Turn counter.
    None (→ change-detect fallback) if the local index is out of range or
    the counter went backwards (a level change / reset moved the base)."""
    local = _u16(cur, layout.localplayer_off)
    if local >= layout.wizard_count:
        return None
    off = layout.wizards_off + local * layout.wizard_stride + layout.pp_turn_off
    dv = _i32(cur, off) - _i32(prev, off)
    return dv if dv >= 0 else None


def pair_clean(prev: bytes, cur: bytes, layout: Layout, dv: int) -> Optional[str]:
    """Inter-tick TEAR GATE (MC1/HW) — None if clean, else the reason.

    Consensus only proves the guest was FROZEN — DOSBox regularly
    parks MID-entity-loop, leaving a snapshot whose +63 clocks split
    into contiguous stepped/unstepped slot bands and whose global LCG
    may not have drawn yet (proven on the first corpus: the "12.5%
    RNG stall" and the "asleep set" were both this artifact). A
    genuine inter-tick pair advances EVERY persisted entity's +63 by
    exactly dv (retail's dispatch table is static and every live
    state row ticks, sub_main :52356/:52406) and draws the global LCG
    exactly dv times (one per sub-step, :52223).

    Deviant discrimination: a TEAR's deviants sit exactly one pass
    short or long (step == dv∓1 — the cursor bands), while ambient
    CHURN (spawn re-use overwrites +63 with the spawn ordinal,
    :43882/:43907 — constant on HW's class-10 weather families) lands
    on arbitrary values. Churn is unlimited; tear-suspects are capped."""
    e = layout.ent
    ps, st = layout.pool_off, layout.ent_stride
    tear_suspects = 0
    lo = (dv - 1) & 0xFF
    hi = (dv + 1) & 0xFF
    for s in range(1, layout.ent_count):
        o = ps + s * st
        c = cur[o + e.class_]
        if c == 0 or c != prev[o + e.class_]:
            continue
        if cur[o + e.model] != prev[o + e.model]:
            continue
        step = (cur[o + e.tick_byte] - prev[o + e.tick_byte]) & 0xFF
        if step != dv & 0xFF and step in (lo, hi):
            tear_suspects += 1
            if tear_suspects > 2:
                return "clock-band"
    r = _u32(prev, layout.rng_off)
    for _ in range(dv):
        r = (9377 * r + 9439) & 0xFFFFFFFF
    if r != _u32(cur, layout.rng_off):
        return "rng-parity"
    return None


# ---------------------------------------------------------------------------
# EXE tick-patch mailbox: detection + windowed capture path
# ---------------------------------------------------------------------------
def find_mailbox(mem: GuestMem, loc: Located, layout: Layout) -> None:
    """Detect a *_REC.EXE tick-patch mailbox and pin it onto `loc`.

    Tries the deterministic address first (static_base + the build's mailbox
    guest addr — the stub's mailbox lives at a fixed guest-linear addr and
    DOS4GW maps the static objects affinely from that base), then falls back
    to a magic scan. Leaves `loc.mailbox_host` None if no patched exe is
    running — the recorder then uses the legacy tear-gate path unchanged.

    MC1 (CARPET/HIDDEN_REC): magic MGCTTIK1 @ 0x132C40, has a spin period.
    MC2 (NETHERW_REC): magic MGCTTIK2 @ 0x1842C0, signal-only (no period);
    the same static base the MC2 input frame uses (struct_host - 0xD41A0)
    maps it, since struct + mailbox are both in obj3 (contiguous)."""
    if layout.family == "mc2":
        magic, mb_base, has_period = EXE_MB2_MAGIC, EXE_MB2_BASE, False
    elif layout.family == "mc1":
        magic, mb_base, has_period = EXE_MB_MAGIC, EXE_MB_BASE, True
    else:
        return
    if loc.static_base is not None:
        host = loc.static_base + mb_base
        sig = mem.pread(host, len(magic))
        if sig == magic:
            loc.mailbox_host = host
    if loc.mailbox_host is None:  # fallback: scan for the magic
        for r in rw_regions(mem.pid, 1 << 16):
            blob = mem.read_region(r)
            if blob is None:
                continue
            i = blob.find(magic)
            if i >= 0:
                loc.mailbox_host = r.lo + i
                break
    if loc.mailbox_host is not None and has_period:
        pv = mem.pread(loc.mailbox_host + EXE_MB_PERIOD, 4)
        if pv is not None:
            loc.mailbox_period = struct.unpack("<I", pv)[0]


def read_mailbox(mem: GuestMem, loc: Located) -> Optional[tuple[int, int]]:
    """(tick_counter, in_window) from the mailbox, or None on a read fault."""
    v = mem.pread(loc.mailbox_host + EXE_MB_TICK, 8)  # tick @+8, inwin @+0xC
    if v is None:
        return None
    tick, inwin = struct.unpack("<II", v)
    return tick, inwin


def capture_windowed(
    mem: GuestMem, loc: Located, layout: Layout, samples: int, retries: int
) -> Optional[tuple]:
    """Capture the struct DURING a quiescent spin, keyed to the mailbox.

    Returns (struct_bytes, terrain_planes, tick_counter) once a read lands
    with in_window==1 and stays there — same tick counter, byte-stable
    struct (and terrain planes, when pinned) — across `samples` reads.
    This is strictly stronger than `capture_clean`: the stub only raises
    in_window when the previous sub-step has fully settled and the next
    one's LCG draw has not begun, so the window is provably between-tick
    (no mid-pass tear possible). None if no window was caught in
    `retries` attempts (recorder starved, or the game is paused)."""
    for _ in range(retries):
        mb = read_mailbox(mem, loc)
        if mb is None:
            return None
        tick, inwin = mb
        if inwin != 1:
            continue  # between windows — the ~1 ms compute; try again at once
        data = mem.pread(loc.struct_host, layout.struct_size)
        if data is None:
            return None
        terrain = read_terrain(mem, loc, layout)
        vfirst = volatile_view(data, layout)
        stable = True
        for _ in range(samples - 1):
            mb2 = read_mailbox(mem, loc)
            if mb2 is None or mb2 != (tick, 1):
                stable = False  # window closed / advanced mid-read
                break
            nxt = mem.pread(loc.struct_host, layout.struct_size)
            if nxt is None:
                return None
            if volatile_view(nxt, layout) != vfirst:
                stable = False
                break
        # The window must still be open after the terrain read too, or
        # the planes may belong to the next frame.
        if stable and loc.terrain_hosts is not None:
            mb3 = read_mailbox(mem, loc)
            if mb3 != (tick, 1):
                stable = False
        if stable:
            return data, terrain, tick
    return None


def poll_loop_windowed(
    mem: GuestMem,
    loc: Located,
    layout: Layout,
    sink: RecordSink,
    args: argparse.Namespace,
    launch_root: int,
    child: Optional[subprocess.Popen],
) -> None:
    """Capture loop for a tick-patched exe. Every snapshot is window-clean
    by construction and the sub-step counter is authoritative, so this is
    the +63 tear-gate loop with the guesswork removed: continuity is the
    counter delta, and there is no first-record deferral (the anchor is
    already vouched-for)."""
    period = 1.0 / args.poll_hz if args.poll_hz > 0 else 0.0
    # Between windows we only re-read the 8-byte mailbox (the peek gate above),
    # so polling is cheap — spin tight to avoid sleeping through a window that
    # collapsed to a sliver (a heavy frame overran MC2's ~50 ms budget, leaving
    # little or no native spin). `--poll-hz N` sets this to 1/N; unthrottled
    # (the default) floors at 0.1 ms, ~5x the old default.
    idle = period if period else 0.0001
    signal_only = loc.mailbox_period is None
    pacing = "signal-only (native limiter)" if signal_only else \
        f"spin-period={loc.mailbox_period} counts"
    print(
        f"polling (windowed / exe-patch mailbox): samples={args.samples} "
        f"build={loc.build.name if loc.build else '?'} {pacing} idle={idle*1e3:.2f}ms",
        file=sys.stderr,
    )
    base: Optional[int] = None  # counter value mapped to t=0
    prev_ctr: Optional[int] = None
    differ = TerrainDiffer() if loc.terrain_hosts is not None else None
    emitted = gaps = missed = starved = 0
    while args.max_ticks == 0 or emitted < args.max_ticks:
        # Cheap gate: one 8-byte mailbox read decides whether a full 224 KB
        # struct scan is even worth it. Only pull the struct when we are in a
        # FRESH window — in_window==1 AND the frame counter has advanced past
        # the last emitted frame. The window spans many ms and gets polled
        # repeatedly, so this skips the redundant scan of an already-captured
        # frame (the counter itself tells us nothing changed).
        peek = read_mailbox(mem, loc)
        if peek is not None:
            ptick, pin = peek
            if pin != 1 or (prev_ctr is not None and ptick == prev_ctr):
                time.sleep(idle)
                continue
        cap = capture_windowed(mem, loc, layout, args.samples, args.retries)
        if cap is None:
            if not ensure_attached(mem, launch_root, child):
                print("dosbox exited — stopping.", file=sys.stderr)
                break
            starved += 1
            if starved % 50 == 0:
                print("! no quiescent window caught — game paused, or the "
                      "recorder is being starved.", file=sys.stderr)
            time.sleep(period or 0.001)
            continue
        starved = 0
        data, terrain, ctr = cap
        if prev_ctr is None:
            base, prev_ctr = ctr, ctr
            sink.write(attach_terrain(
                build_record(0, data, layout, mem, loc, not args.no_state),
                differ, terrain))
            emitted += 1
            continue
        dv = ctr - prev_ctr
        if dv == 0:
            time.sleep(idle)  # still the same frame's window
            continue
        if dv < 0:  # counter reset — a level change moved the world; stop
            print(f"! sub-step counter went backwards ({prev_ctr}→{ctr}) — a "
                  f"level change reset the mailbox; stopping.", file=sys.stderr)
            break
        if dv > 1:
            gaps += 1
            missed += dv - 1
            hint = ("a heavy frame ate MC2's native limiter spin — reduce "
                    "in-game detail (smaller viewport / flat shading / lower "
                    "res) or re-patch with a wider frame budget "
                    "(mc_exe_tickpatch.py --pace N, N>5); raising --poll-hz "
                    "helps only at the margin" if signal_only
                    else "raise --poll-hz or --retries")
            print(f"! gap: {dv - 1} frame(s) missed before tick {ctr - base} "
                  f"(window not caught in time — {hint})", file=sys.stderr)
        sink.write(attach_terrain(
            build_record(ctr - base, data, layout, mem, loc,
                         not args.no_state), differ, terrain))
        emitted += 1
        prev_ctr = ctr
        if emitted % 20 == 0:
            print(f"  {emitted} snapshots (tick={ctr - base}, missed={missed})",
                  file=sys.stderr)
        if period:
            time.sleep(period)
    print(f"done: {emitted} snapshots spanning "
          f"{0 if prev_ctr is None else prev_ctr - base} sub-steps, "
          f"{gaps} gap(s) / {missed} missed (window-gated, no tears possible).",
          file=sys.stderr)


def poll_loop(
    mem: GuestMem,
    loc: Located,
    layout: Layout,
    sink: RecordSink,
    args: argparse.Namespace,
    launch_root: int,
    child: Optional[subprocess.Popen],
) -> None:
    period = 1.0 / args.poll_hz if args.poll_hz > 0 else 0.0
    build = loc.build.name if loc.build else "?"
    have_ext = loc.static_base is not None or layout.in_struct_mouse_off is not None
    ext = "yes" if have_ext else "no"
    print(
        f"polling: samples={args.samples} build={build} externals={ext}",
        file=sys.stderr,
    )
    prev: Optional[bytes] = None
    first_rec: Optional[dict] = None  # deferred tick-0 record (see below)
    first_planes: Optional[list] = None  # its terrain planes, kept alongside
    differ = TerrainDiffer() if loc.terrain_hosts is not None else None
    tick = 0
    emitted = gaps = missed = torn = 0
    torn_why: dict = {}
    streak = 0  # consecutive mid-tick rejections against the current prev
    streak_why: dict = {}
    warned_dv = 1
    streak_t0 = 0.0  # host clock at the streak's first rejection
    streak_wc0: Optional[int] = None  # guest PIT wall clock, ditto

    def streak_span() -> str:
        """How long the current streak has really lasted — host seconds,
        and (when the static frame is pinned) guest GAMEPLAY seconds via
        the ~120 Hz PIT clock. Bootstrap streaks can't count pending
        ticks (the anchor keeps moving), so this is the only honest
        measure of what a stall is skipping."""
        s = f" [{time.monotonic() - streak_t0:.1f}s"
        wc = read_wallclock(mem, loc, layout)
        if streak_wc0 is not None and wc is not None:
            s += f", ~{(wc - streak_wc0) / 120:.1f}s of gameplay"
        return s + "]"

    def reject(why: str, dv_est: int) -> None:
        """Count a mid-tick park; report tick LOSS live. One rejection is
        routine (resample and the boundary usually turns up), but once the
        +63 mode says a boundary passed while every park was mid-tick, data
        is being lost NOW — say so, once per newly-pending tick, instead of
        letting a silent streak surface later as a bare gap."""
        nonlocal torn, streak, warned_dv, streak_t0, streak_wc0
        torn += 1
        streak += 1
        torn_why[why] = torn_why.get(why, 0) + 1
        streak_why[why] = streak_why.get(why, 0) + 1
        if streak == 1:
            streak_t0 = time.monotonic()
            streak_wc0 = read_wallclock(mem, loc, layout)
        if dv_est > warned_dv:
            warned_dv = dv_est
            why_s = ", ".join(f"{k}×{v}" for k, v in sorted(streak_why.items()))
            print(f"! mid-tick parks only — {dv_est - 1} tick(s) pending "
                  f"after {streak} rejects ({why_s}){streak_span()}. The sim "
                  f"is saturating the emulated CPU (no inter-tick idle "
                  f"parks); if this persists, raise DOSBox cycles.",
                  file=sys.stderr)
        elif streak % 500 == 0:  # e.g. a bootstrap stall, where dv can't grow
            why_s = ", ".join(f"{k}×{v}" for k, v in sorted(streak_why.items()))
            print(f"! {streak} consecutive mid-tick parks ({why_s})"
                  f"{streak_span()} — no clean boundary caught yet.",
                  file=sys.stderr)

    wc_poll0 = read_wallclock(mem, loc, layout)  # go-live guest clock
    while args.max_ticks == 0 or emitted < args.max_ticks:
        cap = capture_clean(mem, loc, layout, args.samples, args.retries)
        if cap is None:
            if not ensure_attached(mem, launch_root, child):
                print("dosbox exited — stopping.", file=sys.stderr)
                break
            print("! no stable window — lower DOSBox `cycles`.",
                  file=sys.stderr)
            time.sleep(period or 0.005)
            continue
        data, terrain = cap
        if prev is None:
            # The first snapshot has no pair to gate it, and a mid-tick
            # park recorded unvetted would poison every later pair (the
            # gate keeps prev on rejection, so a torn anchor starves the
            # loop for as long as its cursor band persists). Build the
            # record now — externals/wallclock belong to THIS moment —
            # but write it only once the first clean pair vouches for it.
            prev = data
            first_rec = build_record(0, data, layout, mem, loc,
                                     not args.no_state)
            first_planes = terrain
            continue
        dv = tick_delta(prev, data, layout)
        if dv is None:  # can't measure — treat any change as +1 tick
            if volatile_view(prev, layout) == volatile_view(data, layout):
                time.sleep(period or 0.001)
                continue
            dv = 1
        if dv == 0:
            # The +63 mode says "no tick" — but if the global LCG moved,
            # the tick-top draw (:52223) already happened and this is a
            # park EARLY in the entity pass (cursor below slot ~500),
            # i.e. the same tear as a clock band, not a same-tick wait.
            # Mistaking it for "still the same tick" is what made whole
            # rejection streaks silent and uncounted.
            if (layout.family == "mc1"
                    and _u32(data, layout.rng_off) != _u32(prev, layout.rng_off)):
                reject("mid-pass-early", 1)
                if first_rec is not None:
                    prev = data  # unvetted anchor — prefer the newer candidate
                    first_rec = build_record(0, data, layout, mem, loc,
                                             not args.no_state)
                    first_planes = terrain
                time.sleep(period if period else 0)
                continue
            # Same tick (rng unchanged). While the anchor is unvetted,
            # refresh it anyway: a torn anchor read against its OWN
            # completing boundary also lands here (the splice already
            # carries the boundary's rng), and the newer read is never
            # the worse candidate of two same-tick claims.
            if first_rec is not None:
                prev = data
                first_rec = build_record(0, data, layout, mem, loc,
                                         not args.no_state)
                first_planes = terrain
            time.sleep(period or 0.001)  # wait for the tick to advance
            continue
        # Tear gate (MC1/HW): reject mid-pass snapshots — resample
        # until the frozen window is a true inter-tick boundary.
        # Retry HOT (yield, not a poll period): during saturation clean
        # parks are rare, and sleeping past one is how ticks get away.
        # Rejections that outlive the tick surface as gaps below.
        if layout.family == "mc1":
            why = pair_clean(prev, data, layout, dv)
            if why is not None:
                reject(why, dv)
                if first_rec is not None:
                    # While the anchor is unvetted the blame is ambiguous —
                    # replace it with the newer candidate so a torn first
                    # snapshot cannot starve the bootstrap forever.
                    prev = data
                    first_rec = build_record(0, data, layout, mem, loc,
                                             not args.no_state)
                    first_planes = terrain
                time.sleep(period if period else 0)
                continue
        if first_rec is not None:  # the pair vouches for the anchor: flush it
            sink.write(attach_terrain(first_rec, differ, first_planes))
            wc_anchor = first_rec.get("wallclock")
            if wc_poll0 is not None and wc_anchor is not None:
                late = (wc_anchor - wc_poll0) / 120
                if late > 0.5:  # bootstrap burned real gameplay before t=0
                    print(f"! first verified boundary came ~{late:.1f}s of "
                          f"gameplay after polling began — that stretch is "
                          f"NOT in the recording (t=0 anchors here).",
                          file=sys.stderr)
            first_rec = None
            emitted += 1
        if dv > 1:
            gaps += 1
            missed += dv - 1
            why_s = (" (" + ", ".join(f"{k}×{v}" for k, v in
                                      sorted(streak_why.items())) + ")"
                     ) if streak_why else ""
            print(f"! gap: {dv - 1} tick(s) missed before tick "
                  f"{tick + dv} — {streak} mid-tick park(s) rejected"
                  f"{why_s}", file=sys.stderr)
        tick += dv
        sink.write(attach_terrain(
            build_record(tick, data, layout, mem, loc, not args.no_state),
            differ, terrain))
        emitted += 1
        prev = data
        streak = 0
        streak_why = {}
        warned_dv = 1
        if emitted % 20 == 0:
            print(f"  {emitted} snapshots (tick={tick}, missed={missed}, "
                  f"torn-rejected={torn})", file=sys.stderr)
        if period:
            time.sleep(period)
    why = (" (" + ", ".join(f"{k}: {v}" for k, v in sorted(torn_why.items()))
           + ")") if torn_why else ""
    print(f"done: {emitted} snapshots spanning {tick} ticks, "
          f"{gaps} gap(s) / {missed} missed, {torn} torn snapshot(s) "
          f"rejected{why}.", file=sys.stderr)


def _decode_terrain_delta(blob: bytes, plane_count: int, cells: int) -> list:
    """Reference decoder mirroring mgc_formats::mgcr::decode_terrain_delta
    byte for byte — used only by --terrain-selftest to prove the emitter
    and the Rust reader agree on the wire format."""
    out, o = [], 0
    for _ in range(plane_count):
        (n,) = struct.unpack_from("<I", blob, o)
        o += 4
        plane = []
        for _ in range(n):
            c, v = struct.unpack_from("<HB", blob, o)
            o += 3
            assert c < cells, "cell out of range"
            plane.append((c, v))
        out.append(plane)
    assert o == len(blob), "trailing bytes"
    return out


def terrain_selftest() -> None:
    """Synthetic end-to-end check of the terrain channel emitter: base on
    the first emitted record, omitted key on no-change records, exact
    delta bytes, and the gap self-heal (a skipped record's edits land in
    the next delta). Exits nonzero on any failure."""
    cells = 256  # a 16x16 world keeps the test readable
    h0 = bytes((i % 100 for i in range(cells)))
    t0 = bytes(cells)
    differ = TerrainDiffer()

    b64d = base64.b64decode
    ch0 = differ.channel([h0, t0])
    assert ch0 is not None and "base_b64" in ch0, "first record carries base"
    assert b64d(ch0["base_b64"]) == h0 + t0, "base = planes concatenated"

    assert differ.channel([h0, t0]) is None, "no change → no terrain key"

    # A terraform window: two height cells + one type cell.
    h1 = bytearray(h0)
    h1[5], h1[200] = 77, 78
    t1 = bytearray(t0)
    t1[5] = 9
    ch1 = differ.channel([bytes(h1), bytes(t1)])
    dec = _decode_terrain_delta(b64d(ch1["delta_b64"]), 2, cells)
    assert dec == [[(5, 77), (200, 78)], [(5, 9)]], f"delta wrong: {dec}"

    # Gap self-heal: an edit made in a record the recorder never emitted
    # (h2) must appear in the NEXT emitted record's delta (h3's, which
    # also has its own edit).
    h2 = bytearray(h1)
    h2[10] = 50  # the "lost" record's edit
    h3 = bytearray(h2)
    h3[11] = 51
    ch3 = differ.channel([bytes(h3), bytes(t1)])
    dec = _decode_terrain_delta(b64d(ch3["delta_b64"]), 2, cells)
    assert dec == [[(10, 50), (11, 51)], []], f"gap edits lost: {dec}"

    # Full-plane churn (a doomsday storm) round-trips too.
    h4 = bytes(((i * 7 + 3) % 251 for i in range(cells)))
    ch4 = differ.channel([h4, bytes(t1)])
    dec = _decode_terrain_delta(b64d(ch4["delta_b64"]), 2, cells)
    img = bytearray(h3)
    for c, v in dec[0]:
        img[c] = v
    assert bytes(img) == h4, "accumulated image diverged"

    print("terrain selftest: OK", file=sys.stderr)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument(
        "--game", choices=list(LAYOUTS), default="mc1",
        help="which engine layout to use (default: mc1)",
    )
    ap.add_argument(
        "--level", type=int, default=None,
        help="loaded campaign level index — its embedded level record is the "
             "content needle that locates the struct. Required unless "
             "--needle-file is given.",
    )
    ap.add_argument(
        "--out", default="-",
        help="output path — .mgcr (zstd JSONL, docs/RECORDING.md) unless "
             "the path ends in .jsonl or is - for stdout, which write "
             "plain JSONL (default: -)",
    )
    ap.add_argument(
        "--no-state", action="store_true",
        help="omit the raw state channel (the full struct image per tick)."
             " Fixture verification NEEDS that channel; omit it only for "
             "lightweight scouting runs",
    )
    ap.add_argument("--pid", type=int, help="attach to an existing dosbox pid")
    ap.add_argument(
        "--once", action="store_true",
        help="locate + decode ONE snapshot, print sanity, write header + "
             "that one record to --out, exit",
    )
    ap.add_argument("--samples", type=int, default=3,
                    help="reads that must agree for a clean snapshot")
    ap.add_argument("--retries", type=int, default=200,
                    help="stability attempts before giving up a tick")
    ap.add_argument("--max-ticks", type=int, default=200,
                    help="stop after this many snapshots (0 = until Ctrl-C "
                         "or dosbox exits)")
    ap.add_argument("--poll-hz", type=float, default=0.0,
                    help="throttle polling (0 = as fast as possible)")
    ap.add_argument("--wait-timeout", type=float, default=0.0,
                    help="seconds to wait for the level to load AND the sim "
                         "to go live (0 = wait indefinitely)")
    ap.add_argument("--no-wait-live", action="store_true",
                    help="don't wait for gameplay to be ticking before "
                         "recording (capture whatever's loaded, even paused)")
    # Needle plumbing.
    ap.add_argument("--needle-file",
                    help="raw level-record bytes to use as the needle")
    ap.add_argument("--needle-rec-off", type=int, default=None,
                    help="offset into the level record to start the needle "
                         "(default: per-game — 0 for MC1, 8 for MC2 to skip "
                         "its volatile record-header byte)")
    ap.add_argument("--needle-len", type=int, default=256)
    ap.add_argument("--mgc-import", help="path to the mgc-import binary")
    ap.add_argument("--levels-dat", help="override LEVELS.DAT path")
    ap.add_argument("--levels-tab", help="override LEVELS.TAB path")
    ap.add_argument("--terrain-selftest", action="store_true",
                    help="run the terrain-channel emitter selftest "
                         "(synthetic, no DOSBox) and exit")
    ap.add_argument(
        "cmd", nargs=argparse.REMAINDER,
        help="-- <dosbox command …> to launch as a child",
    )
    args = ap.parse_args()

    if args.terrain_selftest:
        terrain_selftest()
        return

    layout = LAYOUTS[args.game]
    if not layout.implemented:
        raise SystemExit(
            f"--game {layout.name} is not supported by this recorder yet: "
            "its struct/field map is still a stub.")

    # Open the sink first: an unwritable path or a missing zstandard
    # module must fail NOW, not after minutes of waiting for a level.
    # close() is idempotent and atexit-registered, so ANY exit path (a
    # locate timeout, a SystemExit) still reclaims an empty output file.
    sink = RecordSink(args.out)
    atexit.register(sink.close)

    cmd = args.cmd[1:] if args.cmd and args.cmd[0] == "--" else args.cmd
    child: Optional[subprocess.Popen] = None
    if args.pid:
        launch_root = args.pid
        if "dosbox" in _read_comm(args.pid).lower():
            pid = args.pid
        else:  # a launcher pid — search its descendants, but not the world
            pid = find_dosbox_descendant(
                args.pid, timeout=2.0, global_fallback=False)
    else:
        if not cmd:
            raise SystemExit("give either --pid or -- <dosbox command>")
        print(f"launching: {' '.join(cmd)}", file=sys.stderr)
        child = subprocess.Popen(cmd)
        launch_root = child.pid
        pid = find_dosbox_descendant(child.pid)
    print(f"attached to dosbox pid {pid}", file=sys.stderr)

    try:
        mem = GuestMem(pid)
    except OSError as exc:
        raise SystemExit(
            f"cannot open /proc/{pid}/mem ({exc}). Reading another "
            "process's memory needs permission: run under sudo, or set "
            "kernel.yama.ptrace_scope=0. (As the launching parent this "
            "usually works without root — check the pid is really dosbox.)"
        )

    # Locate the heap struct by its embedded level record (content-based:
    # DOS4GW does not map guest addresses to host affinely, so a fixed
    # address won't do). Needs --level (or --needle-file for the bytes).
    if args.level is None and not args.needle_file:
        raise SystemExit(
            "--level <n> (the loaded level) is required to locate the "
            "struct by its embedded level record; or pass --needle-file.")
    needle, needle_off = build_needle(layout, args)
    print(f"locating the world struct via the level-record needle "
          f"(struct+{needle_off})…", file=sys.stderr)

    # The struct only holds a live world once the player has actually
    # started a playthrough — wait patiently through the menus / intro
    # FMVs. One recording = one deliberate playthrough; the base can move
    # on a level change, so re-run rather than following transitions.
    loc = wait_for_struct(mem, layout, needle, needle_off,
                          args.wait_timeout, launch_root, child)
    print(f"located struct @host 0x{loc.struct_host:x} "
          f"in region 0x{loc.region.lo:x}-0x{loc.region.hi:x}",
          file=sys.stderr)

    # Attach the separate STATIC-globals frame (wall clock + raw input).
    pin_externals(mem, loc, layout)
    if loc.static_base is not None:
        sg = (f"struct guest 0x{loc.struct_guest:x}, "
              if loc.struct_guest is not None else "")
        print(f"externals: build {loc.build.name}, {sg}"
              f"static base host 0x{loc.static_base:x}",
              file=sys.stderr)
    else:
        print("externals (wall clock / raw input) unavailable — the "
              "in-struct control slot + entity state are still captured",
              file=sys.stderr)

    # Pin the terrain planes (the format-2 terrain channel). Needs the
    # frames above; a failed pin degrades to a format-1 recording.
    pin_terrain(mem, loc, layout)

    # Retail's world sim + wall clock don't advance until gameplay proper
    # begins (a 'get ready' pause or menu leaves the pool frozen). Wait for
    # the gameplay RNG (struct+4, stepped only inside the sim tick) to move
    # before recording, so snapshots start on the first live tick.
    if not args.no_wait_live:
        wait_until_live(mem, loc, layout, args.wait_timeout, launch_root, child)

    # The pre-live pin legitimately fails while a level is still
    # GENERATING (the shading validator doubles as the readiness
    # gate: all-zero until the final bake), and an attach from the
    # menu always hits that window — so give the terrain channel a
    # second chance now the sim is live. (Both 2026-08-08 l32 takes
    # silently degraded to format 1 exactly this way.)
    if loc.terrain_hosts is None:
        pin_terrain(mem, loc, layout)

    # Detect a tick-patched exe (CARPET/HIDDEN_REC.EXE or NETHERW_REC.EXE): its
    # stub exposes a mailbox once the sim has ticked once, so probe AFTER go-live.
    find_mailbox(mem, loc, layout)
    if loc.mailbox_host is not None:
        pacing = (f"spin-period {loc.mailbox_period} counts"
                  if loc.mailbox_period is not None else "signal-only")
        print(f"exe tick-patch detected: mailbox @host 0x{loc.mailbox_host:x} "
              f"({pacing}) — windowed capture, tear gate not needed.",
              file=sys.stderr)

    if args.once:
        if loc.mailbox_host is not None:
            cap = capture_windowed(mem, loc, layout, args.samples, args.retries)
            cap = cap[:2] if cap else None
        else:
            cap = capture_clean(mem, loc, layout, args.samples, args.retries)
        if cap is None:
            raise SystemExit("could not get a stable (non-torn) snapshot")
        data, terrain = cap
        rec = attach_terrain(
            build_record(0, data, layout, mem, loc, not args.no_state),
            TerrainDiffer() if loc.terrain_hosts is not None else None,
            terrain)
        print_sanity(rec)
        sink.write(build_header(args, layout, loc, cmd))
        sink.write(rec)
        sink.close()
        return

    sink.write(build_header(args, layout, loc, cmd))
    try:
        if loc.mailbox_host is not None:
            poll_loop_windowed(mem, loc, layout, sink, args, launch_root, child)
        else:
            poll_loop(mem, loc, layout, sink, args, launch_root, child)
    except KeyboardInterrupt:
        print("\ninterrupted.", file=sys.stderr)
    finally:
        sink.close()
        mem.close()
        if child and child.poll() is None:
            child.terminate()


def pin_externals(mem: GuestMem, loc: Located, layout: Layout) -> None:
    """Attach the STATIC-globals frame so the wall clock and raw input
    become readable. Independent of the heap struct (DOS4GW maps the two
    non-affinely), so it is found by its own landmark; see
    :func:`find_static_base`. Leaves externals unavailable if not found —
    the core recorder (struct + pool + in-struct control slot) is
    unaffected.

    MC2: `D41A0_0` is itself a static at VA 0xD41A0, so the input frame
    is the struct's own frame — base = struct_host − 0xD41A0, validated
    by the control-mode word + keybind-table plausibility (the anchors
    documented at MC2_BUILDS). No needle scan, no pointer chase."""
    if layout.family == "mc2":
        if not layout.build_variants:
            return
        base = loc.struct_host - MC2_STRUCT_VA
        if base < 0 or not _mc2_input_frame_ok(mem, base):
            print("mc2 input frame did not validate — recording without "
                  "the input channel", file=sys.stderr)
            return
        loc.static_base = base
        loc.build = layout.build_variants[0]
        return
    if not (layout.static_needle and layout.build_variants):
        return
    data = mem.pread(loc.struct_host, layout.struct_size)
    if data is None:
        return
    found = find_static_base(mem, layout, data)
    if found is not None:
        loc.static_base, loc.build, loc.struct_guest = found


def _mc2_input_frame_ok(mem: GuestMem, base: int) -> bool:
    """Sanity-gate the MC2 input frame before trusting it: the control
    mode is a small nonzero constant and the keybind table holds
    plausible scancodes (< 0x80, first four nonzero — the UP/DOWN/LEFT/
    RIGHT bindings; a remapped config still passes)."""
    mode = mem.pread(base + MC2_CTRLMODE_GUEST, 2)
    kb = mem.pread(base + MC2_KEYBIND_GUEST, 10)
    if mode is None or kb is None:
        return False
    m = struct.unpack("<h", mode)[0]
    if not 1 <= m <= 15:
        return False
    if any(b == 0 or b >= 0x80 for b in kb[:4]):
        return False
    print(f"mc2 input frame validated (mode {m}, "
          f"keybinds {list(kb[:4])})", file=sys.stderr)
    return True


def print_sanity(rec: dict) -> None:
    obs = rec["obs"]
    print("\n=== sanity ===", file=sys.stderr)
    hdr = f"rng=0x{obs['rng']:08X} active_entities={obs['n_active']}"
    if "wallclock" in rec:
        hdr += f" wallclock={rec['wallclock']}"
    if "local_player" in obs:
        hdr = (f"local_player={obs['local_player']} "
               f"players={obs['player_count']} ") + hdr
    print(hdr, file=sys.stderr)
    census: dict = {}
    for e in obs["entities"]:
        census[(e["class"], e["model"])] = census.get(
            (e["class"], e["model"]), 0) + 1
    print("class/model census (class 3 = carpets/castles):", file=sys.stderr)
    for (c, m), n in sorted(census.items()):
        print(f"  class {c:2d} model {m:3d}: {n}", file=sys.stderr)
    p = obs.get("player")
    if p:
        who = f"'{p['name']}' " if p.get("name") else ""
        turn = f" turn={p['turn']}" if "turn" in p else ""
        print(f"player carpet: {who}slot {p['carpet_slot']} "
              f"life={p['life']}/{p['max_life']} "
              f"mana={p['mana']}/{p['mana_max']} "
              f"pos=({p['x']:.1f},{p['y']:.1f},{p['z']}) "
              f"heading={p['heading']} hands=({p['hand_left']},"
              f"{p['hand_right']}) castle={p['castle']}{turn}", file=sys.stderr)
    raw = rec.get("input")
    if raw:
        if "keys_down" in raw:
            print(f"raw keys down: {raw['keys_down']}", file=sys.stderr)
        if "mouse" in raw:
            print(f"mouse aim: {raw['mouse']}"
                  + (f" fly_assist={raw['fly_assist']}"
                     if "fly_assist" in raw else ""), file=sys.stderr)
    st = rec.get("state")
    if st:
        n = len(st["struct_b64"]) * 3 // 4
        print(f"state channel: struct image {n} bytes"
              + (" + ext registers" if "ext" in st else ""), file=sys.stderr)


if __name__ == "__main__":
    main()
