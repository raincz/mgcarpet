#!/usr/bin/env python3
"""Detour-patch CARPET.EXE / HIDDEN.EXE to pace the sim tick loop and expose a
recorder mailbox.

Why
---
The DOSBox recorder (``tools/mc_dosbox_recorder.py``) needs to sample the
master world struct *between* sub-steps, while it is fully settled. Retail's
tick loop was never frame-capped: with DOSBox cycles cranked high, every
DOSBox host-park lands mid-entity-loop, so the recorder loses ticks
("saturation loss"). This tool installs a tiny wrapper stub around the
per-sub-step tick function (remc1 ``sub_41780_41AC0``) by REDIRECTING its
callers (the gameSpeed fan-out ``call``s) to the stub instead of detouring
the function entry -- the entry stays byte-for-byte pristine, so there are no
injected bytes there to be misdecoded. The stub paces, then ``call``s the
original tick fn and ``ret``s to the caller. It:

  1. paces ONE sub-step per rendered frame to a wall-clock deadline (the
     game's own ~120 Hz PIT counter): fps = 120 / period, so the default
     period 5 gives ~24 fps (the authentic Magic Carpet rate) regardless of
     how high DOSBox cycles are set. The excess cycles are burned in a
     wall-clock spin -- exactly the large *quiescent* window the recorder
     wants: the world struct is settled and untouched. Only the FIRST
     sub-step of a frame is paced, so the F3 game-speed feature (1x / 4x /
     16x sub-steps per frame) still speeds the SIM up while the frame rate
     stays put; at the default speed (1 sub-step/frame) every sub-step is the
     first, so pacing is bit-identical in effect to the old every-sub-step
     pacer.
  1b. holds that quiescent window open for a FLOOR of timer counts (``--floor``,
     default 2) even when the frame overran its deadline. Pacing to an absolute
     deadline is only as good as the compute fitting inside it: a heavy frame
     arrives with the deadline already passed, the spin falls through, and the
     window collapses to nothing -- a dropped frame and a torn delta. The floor
     is measured from where the frame settles, so its width does not depend on
     load, and it is charged only to the frames that already overran (raising
     ``--period`` instead would tax every frame). It applies to the MC2 arm too.
  2. maintains a mailbox in obj3's committed tail: a magic, a monotonic
     sub-step counter, an ``in_window`` flag raised only around a paced spin
     (never on a free-running sub-step, so the recorder never parks in a
     zero-width window), and the raw F3 gameSpeed (0/1/2) so the recorder can
     tell a legit speed-up from a capture loss. The recorder snipes on
     ``in_window==1`` keyed by the tick counter and gets one coherent
     snapshot per paced sub-step -- gap-free by construction, no +63
     heuristic.

The sim is unaffected: MC1's lockstep multiplayer proves per-tick logic is
wall-clock independent (the ~120 Hz counter feeds render/animation timing,
never sim state), so pacing changes only *when* ticks run, never *what* they
compute. The recorded tick sequence is identical to retail's.

Safety / provenance
-------------------
- Patches a COPY. gamedata/ stays pristine GOG; output is ``*_REC.EXE``.
- The stub lives in obj1's zero-filled code cave (read+exec); the mailbox in
  obj4's zero BSS tail (read+write). Neither overlaps game data.
- The ONLY bytes changed in the game's own code are the 4-byte rel32 of each
  redirected ``call`` -- the tick fn entry is untouched (an earlier version
  detoured the entry; the 10-byte overwrite decoded as a wild ``add eax,[eax]``
  when the dynamic recompiler picked the region up misaligned, so we redirect
  the call site instead).
- The stub WRITES only EAX/ECX/EDX (caller-clobber on a void, no-arg fn) and
  only READS EBX (the fan-out's live loop index, to pace just the first
  sub-step); the original tick fn saves/restores EBX/ESI/EDI/EBP, so the
  caller's callee-saved registers (its loop counter in EBX) survive unchanged.
- DOS/4GW relocates the image by one base and injected code gets no LE fixups,
  so the stub computes that load delta at runtime (call/pop) and addresses all
  globals and the original tick fn relatively (delta-invariant).
- A guard counter bounds the spin: if the timer ISR is ever masked (counter
  frozen) the stub releases after ~1 s of emulated time instead of hanging.
- The wall clock is NOT monotonic for the life of the process, but the mailbox
  IS (init is magic-gated, so the deadline survives a level exit). The game
  zeroes the clock in its fade/delay helper on the way back to the menu and
  restores an older value on quickload, so the pacer resyncs on a deadline that
  is too far AHEAD of the clock as well as too far behind -- otherwise the
  second level of a session spins out the guard every frame (~0.2 fps).

Usage
-----
    python3 tools/mc_exe_tickpatch.py CARPET.EXE          # -> CARPET_REC.EXE
    python3 tools/mc_exe_tickpatch.py HIDDEN.EXE -o HID_REC.EXE
    python3 tools/mc_exe_tickpatch.py CARPET.EXE --period 4   # ~30 fps
    python3 tools/mc_exe_tickpatch.py CARPET.EXE --verify-only CARPET_REC.EXE
"""
from __future__ import annotations

import argparse
import struct
import sys
from dataclasses import dataclass
from typing import Optional

# --------------------------------------------------------------------------
# Mailbox layout. The mailbox and the wall clock both live in obj3 (the data
# object), addressed OBJ3-RELATIVE: at runtime the stub derives obj3's real
# base (see build_stub) rather than assuming any load delta, so its writes
# always land in obj3 -- never in game memory. Offsets below are relative to
# the mailbox base; the mailbox itself sits in obj3's committed BSS tail.
# Kept in lockstep with tools/mc_dosbox_recorder.py's EXE_MB_* constants.
# --------------------------------------------------------------------------
OBJ3_BASE = 0x90000  # obj3 LINK base (vbase)
MB_OBJ3 = 0xA2C40  # obj3-relative mailbox base: past both builds' vsize
#                    (CARPET 0xa2c00 / HIDDEN 0xa2bf0), inside the committed
#                    page tail (< 0xa3000). Same offset works for both.
MB_MAGIC0 = MB_OBJ3 + 0x00  # 'MGCT'
MB_MAGIC1 = MB_OBJ3 + 0x04  # 'TIK1'
MB_TICK = MB_OBJ3 + 0x08  # u32 monotonic sub-step counter
MB_INWIN = MB_OBJ3 + 0x0C  # u32 1 while parked in the quiescent spin
MB_DEADLINE = MB_OBJ3 + 0x10  # u32 next release, in PIT counts
MB_SPEED = MB_OBJ3 + 0x14  # u32 raw F3 gameSpeed latch (0/1/2) -- lets the
#                            recorder tell a legit F3 speed-up from capture loss
MB_PERIOD = MB_OBJ3 + 0x18  # u32 sub-step period in PIT counts (default 1)
MB_GUEST = OBJ3_BASE + MB_OBJ3  # guest-LINK addr the recorder reads (0x132c40)

MAGIC0 = 0x5443474D  # "MGCT"
MAGIC1 = 0x314B4954  # "TIK1"

GUARD_ITERS = 0x04000000  # spin bail-out (~1 s emulated); never hit if ISR live
RESYNC_COUNTS = 30  # >250 ms behind schedule -> resync instead of catch-up burst

# --------------------------------------------------------------------------
# The capture-window FLOOR (both arms).
#
# Both arms pace to an ABSOLUTE deadline -- MC1's `deadline += period`, MC2's
# `esi = timer_before_frame + 5`. That is fine while compute fits the budget
# and useless when it does not: a frame heavy enough to overrun its period
# (deaths, meteor swarms) arrives with the deadline already passed, the spin
# falls straight through, and `in_window` is raised and cleared within a
# handful of instructions -- a zero-width window the recorder cannot land in,
# so the take loses that frame and the delta tears. MC1 makes it worse by
# design: up to RESYNC_COUNTS of backlog is burned off with NO wait at all, so
# one heavy frame is followed by a burst of free-running ones.
#
# The floor is a RELATIVE wait, measured from the moment the frame settles, so
# its width does not depend on load: whatever the deadline says, the stub holds
# the quiescent window open for at least `floor` counts. It costs time only on
# the frames that already blew their budget -- unlike raising --period/--pace,
# which taxes every frame including the cheap ones.
#
# GRANULARITY LAW. The only clock either stub can read is an INTEGER tick
# counter (MC1 ~120 Hz = 8.33 ms/count, MC2 100 Hz = 10 ms/count), so
# `target = now + 1` guarantees nothing: enter a hair before the counter ticks
# and it releases immediately. FLOOR_MIN_GUARANTEED = 2 is the smallest value
# that guarantees a FULL count of real wait (and costs at most two). Sub-count
# precision would mean latching the PIT on port 0x40 -- far more code, and
# 8-10 ms is already an order of magnitude past what the recorder needs (three
# stable 224 KB reads).
FLOOR_DEFAULT = 2
FLOOR_MAX = 60  # ~0.5 s; kept well inside GUARD_ITERS so the guard never wins

# --------------------------------------------------------------------------
# MC2 / NETHERW.EXE arm. MC2 already frame-limits itself (InGameLoop_47320's
# native ~24 fps spin), so its takes are gap-free -- but ~33% are TORN: DOSBox
# can park the guest between PlayerEvents (per-player Turn++) and the entity
# pass, a settled-looking-but-mid-frame state. So the MC2 arm is SIGNAL-ONLY:
# no pacer, no spin, no wall clock. It wraps the frame-driver call and raises
# an `in_window` flag for exactly the interval when the frame is fully settled
# (post-draw) and the NEXT frame's Turn++ has not begun -- i.e. across MC2's
# own native limiter spin. The recorder captures only while in_window==1, so
# the Turn++-park tear is unobservable by construction.
#
# The hook is InGameLoop_47320's sole `call DrawAndEventsInGame_47560`:
#     mov esi,[GameTimerTurn]      ; esi = timer before the frame
#     call DrawAndEventsInGame     ; <-- redirected to the stub
#     add esi,5                    ; frame period = 5 timer ticks
#   spin:
#     cmp esi,[GameTimerTurn]      ; native frame-limiter busy-wait
#     ja spin                      ; <-- in_window is raised across THIS spin
# The stub: derive obj3 base, clear in_window (frame about to mutate), call the
# real frame driver, then bump a monotonic frame counter and set in_window=1.
# Continuity is that counter's delta -- NOT the per-player Turn, which advances
# mid-frame (inside PlayerEvents) by design and so can't gate the tear.
MB2_OBJ3 = 0xB42C0  # obj3-relative mailbox base == obj3.vsize (page-align lifts
#                     the segment limit over it; see patch_mc2). Guest 0x1842c0.
MB2_MAGIC0 = MB2_OBJ3 + 0x00  # 'MGCT'
MB2_MAGIC1 = MB2_OBJ3 + 0x04  # 'TIK2'
MB2_TICK = MB2_OBJ3 + 0x08  # u32 monotonic per-FRAME counter (bumped once/frame)
MB2_INWIN = MB2_OBJ3 + 0x0C  # u32 1 while parked in the settled inter-frame gap
OBJ3_BASE_MC2 = 0xD0000  # obj3 LINK base
MB2_GUEST = OBJ3_BASE_MC2 + MB2_OBJ3  # 0x1842c0 -- the guest-LINK addr

MAGIC2_1 = 0x324B4954  # "TIK2" (MC2 magic tail; MAGIC0 'MGCT' is shared)

# InGameLoop_47320 frame-limiter signature. Locates the hook without hardcoding
# any VA: the `mov esi,[obj3ref]` / `call rel32` / `add esi,5` / `cmp esi,[same
# obj3ref]` / `ja $-6` shape is unique. group 1 = the GameTimerTurn obj3-disp
# (read at runtime to derive obj3's real base), group 2 = the frame-driver
# rel32. The two disps must be identical (same GameTimerTurn global).
import re as _re

MC2_LIMITER_SIG = _re.compile(
    rb"\x8b\x35(....)"      # mov esi,[GameTimerTurn]   (obj3-rel disp32)
    rb"\xe8(....)"          # call DrawAndEventsInGame   (rel32 -> frame driver)
    rb"\x83\xc6."           # add esi,imm8               (frame period; --pace
    #                         rewrites the imm8, so wildcard it -- else a paced
    #                         exe stops being recognised as MC2)
    rb"\x3b\x35(....)"      # cmp esi,[GameTimerTurn]
    rb"\x77\xf8",           # ja $-6                     (native limiter spin)
    _re.S,
)

WALLCLOCK_FROM_STRUCTPTR = 0x1E2C  # wallclock obj3-offset = structptr_off - this
GAMESPEED_PTR_FROM_STRUCTPTR = 8  # obj3ptr global obj3-offset = structptr_off + this
#   (the gameSpeed fan-out reads its runtime struct through THIS pointer global,
#    a different global from the tick fn's struct ptr; +8 holds in both builds.)
GAMESPEED_BYTE_OFF = 0x96  # F3 gameSpeed byte within that runtime struct (0/1/2)

# sub_41780 head: push ebx/esi/edi/ebp ; sub esp,0x158 ; mov esi,[structptr] ;
# imul eax,[esi+4],0x24a1 ; add eax,0x24df  (the :52223 LCG draw). Used only to
# LOCATE the tick fn -- we redirect its callers, never overwrite the entry.
TICKFN_PROLOGUE = bytes.fromhex("5356575581ec58010000")  # 10 bytes


# --------------------------------------------------------------------------
# Minimal LE parser
# --------------------------------------------------------------------------
@dataclass
class Obj:
    vsize: int
    vbase: int
    flags: int
    pageidx: int
    npages: int


@dataclass
class LE:
    data: bytearray
    lx: int
    datapages: int
    objs: list


def parse_le(data: bytes) -> LE:
    lx = struct.unpack_from("<I", data, 0x3C)[0]
    if data[lx : lx + 2] != b"LE":
        raise ValueError("not an LE executable (no 'LE' at MZ+0x3C)")
    g = lambda off: struct.unpack_from("<I", data, lx + off)[0]
    objtab, nobj, datapages = g(0x40), g(0x44), g(0x80)
    objs = []
    for i in range(nobj):
        vsize, vbase, flags, pageidx, npages, _ = struct.unpack_from(
            "<6I", data, lx + objtab + i * 24
        )
        objs.append(Obj(vsize, vbase, flags, pageidx, npages))
    return LE(bytearray(data), lx, datapages, objs)


def obj_file_off(le: LE, obj: Obj) -> int:
    return le.datapages + (obj.pageidx - 1) * 0x1000


def va_to_file(le: LE, va: int) -> int:
    o = le.objs[0]
    if not (o.vbase <= va < o.vbase + o.npages * 0x1000):
        raise ValueError(f"VA {va:#x} not in obj1 code pages")
    return obj_file_off(le, o) + (va - o.vbase)


# --------------------------------------------------------------------------
# Locate the tick fn / cave / wallclock in a given build
# --------------------------------------------------------------------------
@dataclass
class Build:
    name: str
    hook_va: int  # tick fn entry (called by the stub; NEVER overwritten)
    call_sites: tuple  # VAs of the `call hook` instructions we redirect
    cave_va: int
    wallclock: int  # runtime flat addr of the ~120 Hz PIT counter (link space)
    structptr_off: int  # obj3-relative offset of the struct-ptr global (its
    #                     runtime disp32 lives in `mov esi,[..]` at hook_va+0xC)


def find_build(le: LE) -> Build:
    o1 = le.objs[0]
    code_off = obj_file_off(le, o1)
    code = bytes(le.data[code_off : code_off + o1.npages * 0x1000])

    # Anchor on the prologue immediately followed by the struct load + LCG draw.
    # mov esi,[imm32] ; imul eax,[esi+4],0x24a1 ; add eax,0x24df
    import re

    pat = re.compile(
        re.escape(TICKFN_PROLOGUE)
        + rb"\x8b\x35(....)\x69\x46\x04\xa1\x24\x00\x00\x05\xdf\x24\x00\x00",
        re.S,
    )
    hits = list(pat.finditer(code))
    if len(hits) == 0:
        raise SystemExit(
            "tick-fn signature not found (0 hits). This is not a pristine "
            "CARPET.EXE / HIDDEN.EXE -- already patched, or an unexpected build."
        )
    if len(hits) != 1:
        raise SystemExit(f"expected exactly 1 tick-fn signature, found {len(hits)}")
    m = hits[0]
    hook_va = o1.vbase + m.start()
    structptr_pre = struct.unpack("<I", m.group(1))[0]
    structptr_runtime = OBJ3_BASE + structptr_pre
    wallclock = structptr_runtime - WALLCLOCK_FROM_STRUCTPTR

    # Validate the wallclock is an incremented counter (ISR writer present).
    wc_pre = struct.pack("<I", wallclock - OBJ3_BASE)
    if (b"\xff\x05" + wc_pre) not in code:
        raise ValueError(
            f"wallclock {wallclock:#x}: no 'inc [wc]' writer -- derivation suspect"
        )

    # Validate the F3 gameSpeed fan-out is the shape the pacer stub relies on
    # (obj3ptr global @ structptr_off+8, then gameSpeed byte @ +0x96). Match the
    # FULL `mov ebx,[obj3ptr] ; mov bl,[ebx+0x96]` form -- NOT a bare
    # `96 00 00 00` (a decoy flat-global `mov dl,[0x96]` lives elsewhere in the
    # code). Fail loudly if absent so an unexpected build can't be mispatched.
    gs_ptr_pre = structptr_pre + GAMESPEED_PTR_FROM_STRUCTPTR
    fanout_sig = (b"\x8b\x1d" + struct.pack("<I", gs_ptr_pre)
                  + b"\x8a\x9b" + struct.pack("<I", GAMESPEED_BYTE_OFF))
    if fanout_sig not in code:
        raise SystemExit(
            f"gameSpeed fan-out signature not found "
            f"(`mov ebx,[obj3+{gs_ptr_pre:#x}] ; mov bl,[ebx+{GAMESPEED_BYTE_OFF:#x}]`"
            f" = {fanout_sig.hex()}). The pacer's gameSpeed derivation is not "
            f"valid on this build -- refusing to patch."
        )

    # The call sites: `E8 rel32` (5 bytes) whose target is the tick fn. These
    # are the gameSpeed fan-out (remc1 :41677/41683/41688) -- redirecting them
    # to the stub leaves the tick fn's entry completely untouched (so no
    # detour bytes to be misdecoded), which is the whole point.
    call_sites = []
    for i in range(len(code) - 5):
        if code[i] == 0xE8:
            tgt = o1.vbase + i + 5 + struct.unpack_from("<i", code, i + 1)[0]
            if tgt == hook_va:
                call_sites.append(o1.vbase + i)
    if not call_sites:
        raise SystemExit(f"no `call {hook_va:#x}` sites found to redirect")

    # Cave = obj1's zero tail past vsize.
    cave_va = o1.vbase + o1.vsize
    cave_off = code_off + o1.vsize
    cave_end = code_off + o1.npages * 0x1000
    if any(le.data[cave_off:cave_end]):
        raise ValueError("obj1 tail cave is not zero-filled")

    # The mailbox lives in obj3's committed BSS tail (past vsize, within the
    # last committed page). Verify MB_OBJ3 is beyond obj3's declared data and
    # still inside the page DOS/4GW commits.
    obj3 = le.objs[2]
    if obj3.vbase != OBJ3_BASE:
        raise ValueError(f"obj3 vbase {obj3.vbase:#x} != {OBJ3_BASE:#x}")
    committed = (obj3.vsize + 0xFFF) & ~0xFFF
    if not (obj3.vsize <= MB_OBJ3 and MB_OBJ3 + 0x20 <= committed):
        raise ValueError(
            f"mailbox obj3-off {MB_OBJ3:#x} not in obj3 tail "
            f"[vsize {obj3.vsize:#x}, committed {committed:#x})")

    name = "CARPET" if wallclock == 0xAC5D4 else ("HIDDEN" if wallclock == 0xAC5C4 else "?")
    return Build(name, hook_va, tuple(call_sites), cave_va, wallclock, structptr_pre)


# --------------------------------------------------------------------------
# Tiny assembler: raw bytes + label-relative branches, two-pass resolve.
# --------------------------------------------------------------------------
class Asm:
    def __init__(self, base_va: int):
        self.base = base_va
        self.items = []  # ('raw', bytes) | ('label', name) | ('br', op, width, target)
        self.size = 0

    def raw(self, b: bytes):
        self.items.append(("raw", b))
        self.size += len(b)

    def label(self, name: str):
        self.items.append(("label", name))

    def br8(self, op: int, target: str):
        self.items.append(("br", bytes([op]), 1, target))
        self.size += 2

    def jmp32(self, target: str):
        self.items.append(("br", b"\xe9", 4, target))
        self.size += 5

    # Convenience encoders. DATA references are OBJ3-relative ([edx + off],
    # ModRM 0x82 = mod=10 rm=010/edx): the stub holds obj3's real runtime base
    # in EDX (derived from the game's own relocated struct-ptr), so writes land
    # in obj3, never in game memory. During the preamble EDX briefly holds the
    # obj1 load delta instead, used only to read one relocated code disp.
    def call_next(self):  # call $+5 (pushes EIP of the following instr)
        self.raw(b"\xe8\x00\x00\x00\x00")

    def pop_edx(self):
        self.raw(b"\x5a")

    def push_edx(self):
        self.raw(b"\x52")

    def sub_edx_imm(self, imm):
        self.raw(b"\x81\xea" + struct.pack("<I", imm & 0xFFFFFFFF))

    def sub_eax_imm(self, imm):  # sub eax, imm32
        self.raw(b"\x2d" + struct.pack("<I", imm & 0xFFFFFFFF))

    def add_eax_imm(self, imm):  # add eax, imm32
        self.raw(b"\x05" + struct.pack("<I", imm & 0xFFFFFFFF))

    def mov_edx_eax(self):  # mov edx, eax
        self.raw(b"\x89\xc2")

    def mov_eax_m(self, a):  # mov eax,[edx+a]
        self.raw(b"\x8b\x82" + struct.pack("<I", a))

    def mov_m_eax(self, a):  # mov [edx+a],eax
        self.raw(b"\x89\x82" + struct.pack("<I", a))

    def mov_m_imm(self, a, imm):  # mov dword [edx+a],imm
        self.raw(b"\xc7\x82" + struct.pack("<I", a) + struct.pack("<I", imm & 0xFFFFFFFF))

    def inc_m(self, a):  # inc dword [edx+a]
        self.raw(b"\xff\x82" + struct.pack("<I", a))

    def add_eax_m(self, a):  # add eax,[edx+a]
        self.raw(b"\x03\x82" + struct.pack("<I", a))

    def test_eax(self):
        self.raw(b"\x85\xc0")

    def movzx_eax_byte_eax(self, disp):  # movzx eax, byte [eax+disp32]  (base EAX)
        self.raw(b"\x0f\xb6\x80" + struct.pack("<I", disp))

    def cmp_ebx_imm8(self, imm):  # cmp ebx, imm8
        self.raw(b"\x83\xfb" + struct.pack("<b", imm))

    def sub_eax_m(self, a):  # sub eax,[edx+a]
        self.raw(b"\x2b\x82" + struct.pack("<I", a))

    def cmp_eax_m(self, a):  # cmp eax,[edx+a]
        self.raw(b"\x3b\x82" + struct.pack("<I", a))

    def cmp_eax_imm(self, imm):
        self.raw(b"\x3d" + struct.pack("<I", imm & 0xFFFFFFFF))

    def mov_ecx_imm(self, imm):
        self.raw(b"\xb9" + struct.pack("<I", imm & 0xFFFFFFFF))

    def push_ecx(self):
        self.raw(b"\x51")

    def pop_ecx(self):
        self.raw(b"\x59")

    def dec_ecx(self):
        self.raw(b"\x49")

    def assemble(self) -> bytes:
        # pass 1: label offsets
        pos, labels = 0, {}
        for it in self.items:
            if it[0] == "raw":
                pos += len(it[1])
            elif it[0] == "label":
                labels[it[1]] = pos
            else:
                pos += 1 + it[2]
        # pass 2: emit
        out = bytearray()
        pos = 0
        for it in self.items:
            if it[0] == "raw":
                out += it[1]
                pos += len(it[1])
            elif it[0] == "label":
                pass
            else:
                _, opb, width, tgt = it
                nextpos = pos + 1 + width
                disp = labels[tgt] - nextpos
                out += opb
                if width == 1:
                    if not (-128 <= disp <= 127):
                        raise ValueError(f"rel8 to {tgt} out of range ({disp})")
                    out += struct.pack("<b", disp)
                else:
                    out += struct.pack("<i", disp)
                pos = nextpos
        return bytes(out)


def build_passthrough(b: Build) -> bytes:
    """A bare wrapper: `call <tick fn> ; ret`, nothing else. Wired in place of
    the full stub, it isolates whether merely calling the tick fn through a
    cave trampoline is the problem, independent of any pacing logic."""
    rel = b.hook_va - (b.cave_va + 5)  # E8 rel32 at cave_va+0, so +5
    return b"\xe8" + struct.pack("<i", rel) + b"\xc3"


def build_stub(b: Build, period: int, floor: int = FLOOR_DEFAULT) -> bytes:
    a = Asm(b.cave_va)
    wc_off = b.structptr_off - WALLCLOCK_FROM_STRUCTPTR  # wallclock obj3-offset
    gs_ptr_off = b.structptr_off + GAMESPEED_PTR_FROM_STRUCTPTR  # obj3ptr global

    # --- derive obj3's real runtime base into EDX ---
    # DOS/4GW relocates objects independently and injected code gets no LE
    # fixups, so we can't assume any load base. Instead read the game's OWN
    # relocated pointer: `mov esi,[structptr]` at hook_va+0xC holds the disp32
    # that the loader fixed up to (obj3_base + structptr_off). Step 1 gets the
    # obj1 load delta (call/pop) purely to locate that code disp; step 2 reads
    # it and subtracts structptr_off to recover obj3_base. From then on EDX is
    # obj3_base and every data ref is obj3-relative, so writes stay in obj3.
    a.call_next()  # push EIP of the pop below
    a.pop_edx()  # edx = runtime(pop)
    a.sub_edx_imm(b.cave_va + 5)  # edx = obj1 load delta (link of pop = cave+5)
    a.mov_eax_m(b.hook_va + 0xC)  # eax = [edx + disp_va] = obj3_base + structptr_off
    a.sub_eax_imm(b.structptr_off)  # eax = obj3_base (runtime)
    a.mov_edx_eax()  # edx = obj3_base for all data refs below

    # --- one-time init (gated on the magic, robust to a non-zero tail) ---
    a.mov_eax_m(MB_MAGIC0)
    a.cmp_eax_imm(MAGIC0)
    a.br8(0x74, "after_init")  # je after_init  (already initialised)
    a.mov_m_imm(MB_MAGIC1, MAGIC1)
    a.mov_m_imm(MB_PERIOD, period)
    a.mov_m_imm(MB_TICK, 0)
    a.mov_eax_m(wc_off)
    a.mov_m_eax(MB_DEADLINE)
    a.mov_m_imm(MB_MAGIC0, MAGIC0)  # write magic LAST -> mailbox is atomic-ish
    a.label("after_init")

    # --- bump the sub-step counter and publish gameSpeed EVERY sub-step ---
    a.inc_m(MB_TICK)
    a.mov_eax_m(gs_ptr_off)  # eax = *obj3ptr = the runtime struct pointer
    a.movzx_eax_byte_eax(GAMESPEED_BYTE_OFF)  # eax = gameSpeed (0/1/2)
    a.mov_m_eax(MB_SPEED)  # publish raw gameSpeed for the recorder

    # --- decide whether THIS sub-step paces (law A: one paced sub-step/frame) --
    # The F3 fan-out runs the tick fn 1/4/16x per frame (gameSpeed 0/1/2) with
    # EBX = the loop index (1 on the first sub-step, 2..N after). Pacing every
    # sub-step would nullify F3 (N steps x one per-step wait = same real-time
    # sim rate at 1/N the fps). Instead pace exactly ONE sub-step per frame:
    #   * gameSpeed 0 (the default) runs the tick fn once/frame -> always pace
    #     (bit-identical in effect to the old every-sub-step pacer), and we do
    #     this via `test eax; jz` WITHOUT reading EBX (which is loop garbage at
    #     speed 0, never 1);
    #   * gameSpeed 1/2 -> pace only the first sub-step (EBX==1); sub-steps
    #     2..N run FREE, so the SIM speeds up 4x/16x while fps stays put.
    a.test_eax()
    a.br8(0x74, "pace")  # jz pace   (speed 0 -> pace unconditionally)
    a.cmp_ebx_imm8(1)
    a.br8(0x75, "skip")  # jne skip  (a later sub-step of a fast frame runs free)

    a.label("pace")
    a.mov_m_imm(MB_INWIN, 1)  # window raised ONLY around a real spin

    # --- floor: deadline = max(deadline, now + floor) -------------------------
    # The clamp, not the spin, is where the floor lives: push the deadline far
    # enough ahead of NOW that the spin below cannot fall straight through, then
    # let the existing wait/guard/resync machinery do the waiting unchanged.
    #   * keeping up (deadline - now > floor)  -> no-op, healthy takes unaffected
    #   * overrunning (now >= deadline)        -> wait exactly `floor`, every
    #     frame, and the catch-up burst is gone with it (the deadline is rebuilt
    #     from NOW each overrun, so backlog cannot accumulate).
    # Signed throughout: `now` and `deadline` are both PIT counts and the
    # difference is small in either direction, so `jle` reads correctly whether
    # the deadline is ahead of or behind the clock.
    if floor:
        a.mov_eax_m(wc_off)  # eax = now
        a.add_eax_imm(floor)  # eax = now + floor
        a.sub_eax_m(MB_DEADLINE)  # eax = (now + floor) - deadline
        a.br8(0x7E, "no_floor")  # jle no_floor  (deadline already far enough out)
        a.add_eax_m(MB_DEADLINE)  # eax = now + floor
        a.mov_m_eax(MB_DEADLINE)
        a.label("no_floor")

    # --- spin until now >= deadline (or bail on a frozen counter) ---
    # diff = now - deadline as a SIGNED i32: negative => still waiting,
    # non-negative => the deadline passed. Signed handles both the normal
    # "deadline slightly ahead" wait and a post-pause "deadline far behind"
    # resync with the same subtraction (no unsigned underflow).
    #
    # The wall clock is NOT monotonic across a level: the game's delay helper
    # (remc1 sub_10300, reached from the screen-fade path on the way back to
    # the menu) spins until the clock reaches a target and then ZEROES it, and
    # ALT+L quickload restores the clock from the savegame. Either one leaves
    # `now` far BEHIND a deadline the mailbox carried over from the previous
    # level -- so on level 2 every paced sub-step would spin out the full guard
    # (~0.2 fps) until the clock climbed back to a stale value minutes ahead.
    # Bound the wait in the other direction too: a deadline more than one
    # period + the catch-up slack AHEAD of the clock cannot be schedule, only a
    # backwards clock step, so drop it and resync. Checked inside the loop so
    # it self-heals whenever the step lands, not just at stub entry.
    # `floor` too, not just `period`: the clamp above can legitimately leave the
    # deadline `floor` counts ahead of the clock, and a floor larger than
    # `period` would otherwise read as a backwards clock step and resync away
    # the very wait we just installed.
    back_limit = max(period, floor) + RESYNC_COUNTS
    a.mov_ecx_imm(GUARD_ITERS)
    a.label("spin")
    a.mov_eax_m(wc_off)  # eax = now
    a.sub_eax_m(MB_DEADLINE)  # eax = now - deadline (signed)
    a.br8(0x79, "passed")  # jns passed  (now >= deadline)
    a.cmp_eax_imm(-back_limit)  # still waiting: is the deadline absurdly far off?
    a.br8(0x7C, "resync")  # jl resync  (clock stepped backwards -> stale deadline)
    a.dec_ecx()
    a.br8(0x75, "spin")  # jnz spin  (keep waiting)
    a.br8(0xEB, "release")  # guard expired (counter frozen) -> release

    a.label("passed")
    a.cmp_eax_imm(RESYNC_COUNTS)  # eax >= 0 here
    a.br8(0x72, "release")  # jb release  (within one catch-up bound)
    a.label("resync")
    a.mov_eax_m(wc_off)  # clock jumped (long pause / level exit) -> drop backlog
    a.mov_m_eax(MB_DEADLINE)  # deadline = now

    a.label("release")
    a.mov_eax_m(MB_DEADLINE)
    a.add_eax_m(MB_PERIOD)
    a.mov_m_eax(MB_DEADLINE)  # deadline += period (fixed cadence, no drift)
    a.mov_m_imm(MB_INWIN, 0)
    a.label("skip")

    body = a.assemble()

    # --- call the ORIGINAL (untouched) tick fn, then return to the caller ---
    # A relative call: both the stub and the tick fn are in obj1, so the rel32
    # is position-independent (delta-invariant). The stub WRITES only
    # eax/ecx/edx and only READS ebx (the fan-out's loop index); the tick fn
    # saves/restores ebx/esi/edi/ebp itself, so the caller's callee-saved regs
    # (its loop counter in ebx) survive intact.
    call_pos = len(body)
    rel = b.hook_va - (b.cave_va + call_pos + 5)
    return body + b"\xe8" + struct.pack("<i", rel) + b"\xc3"  # call hook ; ret


# --------------------------------------------------------------------------
# Patch / verify
# --------------------------------------------------------------------------
def patch(le: LE, b: Build, period: int, wire: bool = True, passthrough: bool = False,
          extend: bool = True, floor: int = FLOOR_DEFAULT) -> bytes:
    o1 = le.objs[0]
    stub = build_passthrough(b) if passthrough else build_stub(b, period, floor)
    cave_off = va_to_file(le, b.cave_va)
    if cave_off + len(stub) > obj_file_off(le, o1) + o1.npages * 0x1000:
        raise ValueError("stub overflows the cave")
    le.data[cave_off : cave_off + len(stub)] = stub

    # Both the code cave (obj1 tail) and the mailbox (obj3 tail) sit PAST their
    # object's declared vsize, so at runtime those tails fall outside the
    # segment limit: jumping into obj1's tail faults, and WRITES into obj3's
    # tail don't persist (the magic never sticks -> init re-runs every call ->
    # the pacing deadline is reset to `now` every call -> no throttle).
    # Page-align both vsizes so the tails become declared, in-limit segment
    # space. The file already provides / commits these pages; only the declared
    # size was short of page-aligned.
    if extend:
        objtab = struct.unpack_from("<I", le.data, le.lx + 0x40)[0]
        new1 = (o1.vsize + 0xFFF) & ~0xFFF
        if b.cave_va + len(stub) > o1.vbase + new1:
            raise ValueError("stub crosses the page boundary; extend by another page")
        struct.pack_into("<I", le.data, le.lx + objtab + 0 * 24, new1)
        o1.vsize = new1

        o3 = le.objs[2]
        new3 = (o3.vsize + 0xFFF) & ~0xFFF
        if o3.vbase + MB_OBJ3 + 0x20 > o3.vbase + new3:
            raise ValueError("mailbox past obj3's page-aligned vsize")
        struct.pack_into("<I", le.data, le.lx + objtab + 2 * 24, new3)
        o3.vsize = new3

    if not wire:
        return stub  # --inert: stub written, call sites untouched (never executed)

    # Redirect each `call hook` to `call stub` -- rewrite only the 4-byte rel32.
    # The tick fn's entry is left byte-for-byte untouched.
    for cs in b.call_sites:
        off = va_to_file(le, cs)
        if le.data[off] != 0xE8:
            raise ValueError(f"call site {cs:#x} is not an E8 call")
        rel = b.cave_va - (cs + 5)
        le.data[off + 1 : off + 5] = struct.pack("<i", rel)
    return stub


def verify(path: str, period: int, inert: bool = False, passthrough: bool = False) -> None:
    import shutil

    from collections import Counter

    data = open(path, "rb").read()
    le = parse_le(data)
    o1 = le.objs[0]
    code_off = obj_file_off(le, o1)
    code = data[code_off : code_off + o1.npages * 0x1000]

    # obj1's vsize is page-aligned by the patch so the cave is in-limit; locate
    # the stub independently of vsize (it is NOT at vbase+vsize any more).
    redirected = 0
    if inert:  # no redirects -- find the full stub's distinctive preamble
        idx = code.find(b"\xe8\x00\x00\x00\x00\x5a\x81\xea")
        if idx < 0:
            raise SystemExit("VERIFY FAIL: stub preamble not found in obj1")
        cave_va = o1.vbase + idx
    else:  # the redirected calls all target the stub -- that is cave_va
        cnt = Counter()
        for i in range(len(code) - 5):
            if code[i] == 0xE8:
                t = o1.vbase + i + 5 + struct.unpack_from("<i", code, i + 1)[0]
                cnt[t] += 1
        cave_va = next((t for t in sorted(cnt, reverse=True)
                        if cnt[t] >= 3 and code[t - o1.vbase] == 0xE8), None)
        if cave_va is None:
            raise SystemExit("VERIFY FAIL: no 3-way redirected call target (stub)")
        redirected = cnt[cave_va]

    # From cave_va, the stub's `call <hook> ; ret` is the first `E8 rel32 C3`
    # (the call/pop preamble is `E8 00000000 5A`, whose +5 byte is 5A not C3).
    rel = cave_va - o1.vbase
    end_j = next((j for j in range(0, 400)
                  if code[rel + j] == 0xE8 and code[rel + j + 5] == 0xC3), None)
    if end_j is None:
        raise SystemExit("VERIFY FAIL: no `call hook ; ret` in the stub")
    hook_va = cave_va + end_j + 5 + struct.unpack_from("<i", code, rel + end_j + 1)[0]
    stub_len = end_j + 6
    aligned = "page-aligned" if o1.vsize % 0x1000 == 0 else f"NOT page-aligned ({o1.vsize:#x})"

    # Read the floor back OUT of the patched image rather than trusting the
    # argument: `add eax,imm32 ; sub eax,[edx+MB_DEADLINE]` is the clamp's
    # signature and occurs nowhere else (the spin's own `sub eax,[deadline]` is
    # preceded by a disp32 tail byte, never by an `05` opcode).
    fm = _re.search(
        rb"\x05(....)\x2b\x82" + _re.escape(struct.pack("<I", MB_DEADLINE)),
        code[rel : rel + stub_len],
        _re.S,
    )
    floor_val = struct.unpack("<I", fm.group(1))[0] if fm else 0
    fl = (f"floor {floor_val} counts (>={(floor_val - 1) * 1000 / 120:.1f} ms window)"
          if floor_val else "floor OFF")

    if inert:
        print(f"VERIFY {path}: OK (INERT)")
        print(f"  stub present @ {cave_va:#x} ({stub_len} bytes) but NO call site "
              f"targets it -- never executed; obj1.vsize {aligned}; {fl}")
        return

    print(f"VERIFY {path}: OK")
    print(f"  {redirected} call site(s) -> stub @ {cave_va:#x}; stub -> original "
          f"tick fn @ {hook_va:#x}; {stub_len} bytes; obj1.vsize {aligned}; "
          f"entry untouched; {fl}")
    if shutil.which("ndisasm"):
        import subprocess
        import tempfile

        with tempfile.NamedTemporaryFile(delete=False, suffix=".bin") as tf:
            tf.write(code[rel : rel + stub_len])
            tmp = tf.name
        out = subprocess.run(
            ["ndisasm", "-b", "32", "-o", hex(cave_va), tmp], capture_output=True, text=True
        ).stdout
        print("  --- stub disassembly ---")
        for ln in out.strip().splitlines():
            print("   ", ln)


# --------------------------------------------------------------------------
# MC2 / NETHERW.EXE: locate the frame-limiter hook, build the signal stub,
# patch, verify. Signal-only (no pacer), so the stub is tiny and the MC1 path
# stays untouched.
# --------------------------------------------------------------------------
@dataclass
class BuildMC2:
    name: str
    call_site: int  # VA of `call DrawAndEventsInGame_47560` we redirect
    frame_fn: int  # VA of the frame driver (the stub calls the original)
    cave_va: int
    obj3ref_va: int  # VA of a fixed-up obj3 disp32 (the GameTimerTurn ref) whose
    #                  runtime value = obj3_base + obj3ref_off -- read to recover
    #                  obj3's real load base, exactly as the MC1 stub does.
    obj3ref_off: int  # obj3-relative offset that disp holds
    period_va: int  # VA of the `add esi,N` immediate byte (the native frame
    #                 period in 100 Hz ticks; default 5 = ~20 fps). Widening it
    #                 with --pace guarantees a wide capture window on heavy
    #                 frames whose compute would otherwise eat the native spin.
    period_now: int  # the current period byte (5 on a pristine NETHERW)


def find_build_mc2(le: LE) -> BuildMC2:
    o1 = le.objs[0]
    code_off = obj_file_off(le, o1)
    code = bytes(le.data[code_off : code_off + o1.npages * 0x1000])

    # Double-patch guard: the limiter signature wildcards the call rel32, so it
    # still matches an already-patched exe -- but the stub's preamble is unique
    # and only present once we've patched. Refuse cleanly.
    if b"\xe8\x00\x00\x00\x00\x5a\x81\xea" in code:
        raise SystemExit("already patched (stub preamble present) -- refusing")

    hits = list(MC2_LIMITER_SIG.finditer(code))
    if len(hits) == 0:
        raise SystemExit(
            "MC2 frame-limiter signature not found (0 hits). Not a pristine "
            "NETHERW.EXE -- already patched, or an unexpected build."
        )
    if len(hits) != 1:
        raise SystemExit(f"expected exactly 1 MC2 limiter signature, found {len(hits)}")
    m = hits[0]
    ref1 = struct.unpack("<I", m.group(1))[0]
    ref2 = struct.unpack("<I", m.group(3))[0]
    if ref1 != ref2:
        raise SystemExit("MC2 limiter: the two GameTimerTurn disps differ")
    # Layout inside the match: mov esi,[disp](6) | call rel32(5) |
    #                          add esi,imm8(3: 83 c6 NN) | cmp(6) | ja(2).
    call_site = o1.vbase + m.start() + 6
    rel = struct.unpack("<I", m.group(2))[0]
    frame_fn = (call_site + 5 + rel) & 0xFFFFFFFF
    obj3ref_va = o1.vbase + m.start() + 2  # the disp32 field of `mov esi,[..]`
    obj3ref_off = ref1
    period_va = o1.vbase + m.start() + 13  # the imm8 of `add esi,N`
    period_now = code[m.start() + 13]

    # obj3 must hold GameTimerTurn (the disp is an obj3-relative offset).
    obj3 = le.objs[2]
    if obj3.vbase != OBJ3_BASE_MC2:
        raise ValueError(f"obj3 vbase {obj3.vbase:#x} != {OBJ3_BASE_MC2:#x}")
    if not (0 <= obj3ref_off < obj3.vsize):
        raise ValueError(f"GameTimerTurn obj3-off {obj3ref_off:#x} outside obj3")

    # Cave = obj1's zero tail past vsize (in-limit after patch_mc2 page-aligns).
    cave_va = o1.vbase + o1.vsize
    cave_off = code_off + o1.vsize
    cave_end = code_off + o1.npages * 0x1000
    if any(le.data[cave_off:cave_end]):
        raise ValueError("obj1 tail cave is not zero-filled")

    # Mailbox in obj3's committed BSS tail (page-align lifts the DS limit over
    # it). MB2_OBJ3 sits at obj3.vsize, so page-aligning vsize covers it.
    committed = (obj3.vsize + 0xFFF) & ~0xFFF
    if not (obj3.vsize <= MB2_OBJ3 and MB2_OBJ3 + 0x10 <= committed):
        raise ValueError(
            f"mailbox obj3-off {MB2_OBJ3:#x} not in obj3 tail "
            f"[vsize {obj3.vsize:#x}, page {committed:#x})"
        )
    return BuildMC2("NETHERW", call_site, frame_fn, cave_va, obj3ref_va,
                    obj3ref_off, period_va, period_now)


def build_stub_mc2(b: BuildMC2, floor: int = FLOOR_DEFAULT) -> bytes:
    """Signal-only wrapper. On each frame:
      1. derive obj3's real runtime base (read the game's own fixed-up
         GameTimerTurn disp, minus its obj3 offset -- delta-safe like MC1);
      2. clear in_window (the frame driver is about to mutate the world);
      3. call the ORIGINAL frame driver (Turn++, entity pass, draw);
      4. bump a monotonic per-frame counter and raise in_window;
      5. hold that window open for at least `floor` timer counts.
    in_window is therefore up from just after the draw, across MC2's native
    limiter spin, until the next frame's mutation -- a settled window keyed by
    the counter. Step 5 is what makes the width load-independent: the native
    limiter's budget is absolute (`turn_before_frame + N`), so a frame whose
    compute eats it leaves no spin at all, and the floor is then the entire
    window. It sits in the TAIL, after the counter bump, so a window the
    recorder sees announced as fresh is the same one being held open.
    Touches only eax/edx (caller-clobber; esi=turn and ebx=loop counter, which
    InGameLoop reads after the call, are preserved) plus ecx, which the floor
    spin's guard borrows under a push/pop so it survives too; the frame driver
    saves/restores its own callee-saved regs."""
    a = Asm(b.cave_va)
    # --- derive obj3 base into edx ---
    a.call_next()  # push EIP of pop
    a.pop_edx()  # edx = runtime(pop)
    a.sub_edx_imm(b.cave_va + 5)  # edx = obj1 load delta (link of pop = cave+5)
    a.mov_eax_m(b.obj3ref_va)  # eax = [delta + refVA] = obj3_base + obj3ref_off
    a.sub_eax_imm(b.obj3ref_off)  # eax = obj3_base (runtime)
    a.mov_edx_eax()  # edx = obj3_base for all mailbox refs

    # --- one-time init (magic-gated; robust to a non-zero tail) ---
    a.mov_eax_m(MB2_MAGIC0)
    a.cmp_eax_imm(MAGIC0)
    a.br8(0x74, "after_init")  # je after_init
    a.mov_m_imm(MB2_MAGIC1, MAGIC2_1)
    a.mov_m_imm(MB2_TICK, 0)
    a.mov_m_imm(MB2_INWIN, 0)
    a.mov_m_imm(MB2_MAGIC0, MAGIC0)  # magic LAST -> mailbox is atomic-ish
    a.label("after_init")

    # --- close the window: the frame is about to mutate the world ---
    a.mov_m_imm(MB2_INWIN, 0)
    head = a.assemble()

    # --- call the ORIGINAL frame driver, preserving obj3 base across it ---
    # push edx ; call frame_fn ; pop edx. The original call site pushed no
    # argument (turn is passed in esi), so the extra push is invisible to the
    # callee (it reads no stack arg) and the stack stays balanced.
    push_pos = len(head)  # `push edx` (1 byte) then `call` (5 bytes)
    call_pos = push_pos + 1
    rel = b.frame_fn - (b.cave_va + call_pos + 5)
    mid = b"\x52" + b"\xe8" + struct.pack("<i", rel) + b"\x5a"  # push edx;call;pop edx

    # --- open the window: frame settled; native limiter spin follows ---
    t = Asm(b.cave_va + len(head) + len(mid))
    t.inc_m(MB2_TICK)
    t.mov_m_imm(MB2_INWIN, 1)

    # --- floor: hold the window open at least `floor` timer counts -----------
    # Same counter the native limiter spins on, so this composes with it rather
    # than replacing it: the total window is floor + max(0, native spin). ECX is
    # borrowed for the frozen-timer guard and restored, because InGameLoop's
    # live registers across the call are not fully known -- the stub's standing
    # rule is to hand back everything but eax/edx.
    if floor:
        t.mov_eax_m(b.obj3ref_off)  # eax = GameTimerTurn (now)
        t.add_eax_imm(floor)  # eax = release target
        t.push_ecx()
        t.mov_ecx_imm(GUARD_ITERS)
        t.label("fspin")
        t.cmp_eax_m(b.obj3ref_off)  # target vs now
        t.br8(0x7E, "fdone")  # jle fdone  (target reached -> release)
        t.dec_ecx()
        t.br8(0x75, "fspin")  # jnz fspin  (keep waiting)
        t.label("fdone")  # guard expired (ISR masked) falls through here too
        t.pop_ecx()
    t.raw(b"\xc3")  # ret
    return head + mid + t.assemble()


def patch_mc2(le: LE, b: BuildMC2, wire: bool = True, extend: bool = True,
              pace: Optional[int] = None, floor: int = FLOOR_DEFAULT) -> bytes:
    o1 = le.objs[0]

    # Optional: widen the native frame period so a heavy frame's compute can't
    # eat the whole limiter spin (the capture window). One byte -- the imm8 of
    # `add esi,N`. Purely a real-time pacing change: the sim still runs one
    # PlayerEvents (Turn++) + one entity pass per frame, so the recorded frame
    # sequence is byte-identical, just held longer. N in 1..127 (higher = lower
    # fps, wider window). This is the definitive zero-gap fix for graphics-heavy
    # levels; without it, signal-only relies on compute fitting the ~50 ms budget.
    if pace is not None:
        if not (1 <= pace <= 127):
            raise ValueError("--pace must be in 1..127 (imm8 frame period)")
        poff = va_to_file(le, b.period_va)
        if le.data[poff - 2 : poff] != b"\x83\xc6":  # `add esi,` guard
            raise ValueError(f"period byte @ {b.period_va:#x} is not an `add esi,imm8`")
        le.data[poff] = pace

    stub = build_stub_mc2(b, floor)
    cave_off = va_to_file(le, b.cave_va)
    if cave_off + len(stub) > obj_file_off(le, o1) + o1.npages * 0x1000:
        raise ValueError("stub overflows the cave")
    le.data[cave_off : cave_off + len(stub)] = stub

    # Page-align obj1.vsize (so the code cave is inside the CS limit and will
    # execute) and obj3.vsize (so the mailbox is inside the DS limit and its
    # writes persist) -- the same two lifts the MC1 arm needs.
    if extend:
        objtab = struct.unpack_from("<I", le.data, le.lx + 0x40)[0]
        new1 = (o1.vsize + 0xFFF) & ~0xFFF
        if b.cave_va + len(stub) > o1.vbase + new1:
            raise ValueError("stub crosses the page boundary; extend by another page")
        struct.pack_into("<I", le.data, le.lx + objtab + 0 * 24, new1)
        o1.vsize = new1

        o3 = le.objs[2]
        new3 = (o3.vsize + 0xFFF) & ~0xFFF
        if MB2_OBJ3 + 0x10 > new3:
            raise ValueError("mailbox past obj3's page-aligned vsize")
        struct.pack_into("<I", le.data, le.lx + objtab + 2 * 24, new3)
        o3.vsize = new3

    if not wire:
        return stub  # --inert

    off = va_to_file(le, b.call_site)
    if le.data[off] != 0xE8:
        raise ValueError(f"call site {b.call_site:#x} is not an E8 call")
    rel = b.cave_va - (b.call_site + 5)
    le.data[off + 1 : off + 5] = struct.pack("<i", rel)
    return stub


def verify_mc2(path: str, inert: bool = False) -> None:
    import shutil

    data = open(path, "rb").read()
    le = parse_le(data)
    o1 = le.objs[0]
    code_off = obj_file_off(le, o1)
    code = data[code_off : code_off + o1.npages * 0x1000]

    # The stub preamble is distinctive: E8 00000000 5A 81 EA (call/pop/sub).
    idx = code.find(b"\xe8\x00\x00\x00\x00\x5a\x81\xea")
    if idx < 0:
        raise SystemExit("VERIFY FAIL: MC2 stub preamble not found in obj1")
    cave_va = o1.vbase + idx

    # The stub's `push edx ; call frame_fn ; pop edx` is the only `52 E8.. 5A`.
    rel = cave_va - o1.vbase
    j = next(
        (k for k in range(0, 400)
         if code[rel + k] == 0x52 and code[rel + k + 1] == 0xE8 and code[rel + k + 6] == 0x5A),
        None,
    )
    if j is None:
        raise SystemExit("VERIFY FAIL: no `push edx ; call frame_fn ; pop edx`")
    frame_fn = cave_va + j + 6 + struct.unpack_from("<i", code, rel + j + 2)[0]

    # Tail after `pop edx`: inc(6) + mov dword(10), then either `ret` outright
    # (floor OFF) or the 30-byte floor block ending in `ret`. Decode it rather
    # than assuming a length -- and fail loudly on a shape we did not emit, so
    # the reported stub_len can never silently under-run the real stub.
    p = rel + j + 7 + 6 + 10
    floor_val = 0
    if code[p] != 0xC3:
        fm = _re.match(
            rb"\x8b\x82(....)\x05(....)\x51\xb9....\x3b\x82(....)\x7e.\x49\x75.\x59\xc3",
            code[p:], _re.S,
        )
        if fm is None:
            raise SystemExit("VERIFY FAIL: unrecognised MC2 stub tail (floor block)")
        if fm.group(1) != fm.group(3):
            raise SystemExit("VERIFY FAIL: floor spin samples two different timers")
        floor_val = struct.unpack("<I", fm.group(2))[0]
        p += fm.end() - 1  # land on the `ret`
    stub_len = p + 1 - rel
    fl = (f"floor {floor_val} counts (>={(floor_val - 1) * 10:.0f} ms window)"
          if floor_val else "floor OFF")
    aligned = "page-aligned" if o1.vsize % 0x1000 == 0 else f"NOT page-aligned ({o1.vsize:#x})"

    redirected = 0
    call_site = None
    if not inert:
        for i in range(len(code) - 5):
            if code[i] == 0xE8:
                t = o1.vbase + i + 5 + struct.unpack_from("<i", code, i + 1)[0]
                if t == cave_va:
                    redirected += 1
                    call_site = i
        if redirected != 1:
            raise SystemExit(f"VERIFY FAIL: expected 1 redirected call, found {redirected}")

    if inert:
        print(f"VERIFY {path}: OK (INERT)")
        print(f"  MC2 stub @ {cave_va:#x} ({stub_len} bytes) but NO call site targets "
              f"it; obj1.vsize {aligned}; {fl}")
        return
    # The native frame period is the `add esi,N` imm8 right after the call.
    period = code[call_site + 7] if code[call_site + 5 : call_site + 7] == b"\x83\xc6" else None
    per = (f"; frame period {period} (~{100 / period:.1f} fps @ 100 Hz)"
           if period else "")
    print(f"VERIFY {path}: OK")
    print(f"  1 call site -> stub @ {cave_va:#x}; stub -> frame driver @ {frame_fn:#x}; "
          f"{stub_len} bytes; mailbox guest {MB2_GUEST:#x} (MGCTTIK2); obj1.vsize "
          f"{aligned}; entry untouched; {fl}{per}")
    if shutil.which("ndisasm"):
        import subprocess
        import tempfile

        with tempfile.NamedTemporaryFile(delete=False, suffix=".bin") as tf:
            tf.write(code[rel : rel + stub_len])
            tmp = tf.name
        out = subprocess.run(
            ["ndisasm", "-b", "32", "-o", hex(cave_va), tmp], capture_output=True, text=True
        ).stdout
        print("  --- stub disassembly ---")
        for ln in out.strip().splitlines():
            print("   ", ln)


def is_mc2(le: LE) -> bool:
    """A NETHERW.EXE has the MC2 limiter signature; CARPET/HIDDEN have the MC1
    tick-fn prologue. Peek for the former."""
    o1 = le.objs[0]
    code_off = obj_file_off(le, o1)
    code = bytes(le.data[code_off : code_off + o1.npages * 0x1000])
    return MC2_LIMITER_SIG.search(code) is not None


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("exe", help="CARPET.EXE / HIDDEN.EXE (MC1) or NETHERW.EXE (MC2), a pristine copy")
    ap.add_argument("-o", "--out", help="output path (default: <NAME>_REC.EXE)")
    ap.add_argument(
        "--period",
        type=int,
        default=5,
        help="frame period in ~120 Hz PIT counts. fps = 120 / period: "
             "default 5 -> ~24 fps; 4 -> ~30 fps; 6 -> ~20 fps "
             "(measured live: period 30 gave ~4 fps).",
    )
    ap.add_argument(
        "--pace",
        type=int,
        default=None,
        metavar="N",
        help="MC2 only: widen the native frame period to N 100 Hz ticks "
             "(default 5 = ~20 fps). Higher N = lower fps but a WIDER capture "
             "window, guaranteeing a quiescent spin even on graphics-heavy "
             "frames whose compute would otherwise eat the window (the cause of "
             "sporadic missed frames). Sim-neutral: the recorded frame sequence "
             "is byte-identical, just paced slower. Try 10-15 for heavy levels.",
    )
    ap.add_argument(
        "--floor",
        type=int,
        default=FLOOR_DEFAULT,
        metavar="N",
        help=f"BOTH arms: minimum capture window, in timer counts, held open on "
             f"every paced frame even when the frame overran its budget (default "
             f"{FLOOR_DEFAULT}; 0 disables). Both stubs pace to an ABSOLUTE "
             f"deadline, so a frame heavy enough to overrun it (deaths, meteor "
             f"swarms) leaves a zero-width window and the recorder drops the "
             f"frame -- the floor makes the width load-independent. The counter "
             f"is integral (MC1 ~120 Hz, MC2 100 Hz), so N=1 guarantees nothing "
             f"and N=2 is the smallest value that guarantees a full count "
             f"(8.3 / 10 ms). Costs time ONLY on frames that already blew their "
             f"budget, unlike --period / --pace. Max {FLOOR_MAX}.",
    )
    ap.add_argument("--verify-only", metavar="PATCHED", help="just re-verify an already-patched exe")
    ap.add_argument(
        "--inert",
        action="store_true",
        help="DIAGNOSTIC: write the stub into the cave but do NOT redirect any "
             "call site, so the stub is never executed. If the game still "
             "crashes, the cave write itself (not the stub logic) is the problem.",
    )
    ap.add_argument(
        "--passthrough",
        action="store_true",
        help="DIAGNOSTIC: wire the call sites to a bare `call <tick fn> ; ret` "
             "trampoline (no pacing, no delta, no mailbox). Isolates whether "
             "calling the tick fn through the cave is itself the problem.",
    )
    ap.add_argument(
        "--no-extend",
        action="store_true",
        help="DIAGNOSTIC: do NOT page-align obj1's vsize. The cave stays past "
             "the declared code size (unloaded / outside the CS limit), so this "
             "reproduces the crash -- use it to A/B against the default fix.",
    )
    args = ap.parse_args(argv)

    if not (0 <= args.floor <= FLOOR_MAX):
        raise SystemExit(f"--floor must be in 0..{FLOOR_MAX} (0 = off)")
    if args.floor == 1:
        raise SystemExit(
            "--floor 1 guarantees no wait at all: the timer is an integer "
            "counter, so entering a hair before it ticks releases immediately. "
            "Use 2 (the smallest value that guarantees a full count) or 0 to "
            "disable the floor."
        )

    if args.verify_only:
        vdata = open(args.verify_only, "rb").read()
        if is_mc2(parse_le(vdata)):
            verify_mc2(args.verify_only, inert=args.inert)
        else:
            verify(args.verify_only, args.period, inert=args.inert, passthrough=args.passthrough)
        return 0

    data = open(args.exe, "rb").read()
    le = parse_le(data)

    def _out_path():
        if args.out:
            return args.out
        import os

        base = os.path.basename(args.exe)
        stem, ext = os.path.splitext(base)
        return os.path.join(os.path.dirname(args.exe) or ".", f"{stem}_REC{ext or '.EXE'}")

    # --- MC2 / NETHERW: signal-only (no pacer), optional frame-period widen. ---
    if is_mc2(le):
        if args.passthrough:
            raise SystemExit("--passthrough is an MC1-only diagnostic")
        b2 = find_build_mc2(le)
        mode = "  [INERT: stub written, NOT wired]" if args.inert else ""
        mode += "  [--no-extend: vsize NOT page-aligned]" if args.no_extend else ""
        pace_note = (f"  [--pace {args.pace}: period {b2.period_now}->{args.pace}, "
                     f"~{100 / max(args.pace, 1):.1f} fps]" if args.pace is not None else "")
        print(f"build={b2.name}  hook(call)={b2.call_site:#x}  frame_fn={b2.frame_fn:#x}  "
              f"cave={b2.cave_va:#x}  mailbox={MB2_GUEST:#x}  "
              f"timer=obj3+{b2.obj3ref_off:#x}{mode}{pace_note}")
        stub = patch_mc2(le, b2, wire=not args.inert, extend=not args.no_extend,
                         pace=args.pace, floor=args.floor)
        out = _out_path()
        with open(out, "wb") as f:
            f.write(le.data)
        tag = ", INERT" if args.inert else ""
        pace_tag = f", pace={args.pace}" if args.pace is not None else ", signal-only"
        floor_tag = f", floor={args.floor}" if args.floor else ", floor=OFF"
        print(f"wrote {out}  (stub {len(stub)} B{pace_tag}{floor_tag}{tag})")
        verify_mc2(out, inert=args.inert)
        return 0

    # --- MC1 / CARPET / HIDDEN: pacer + mailbox. ---
    b_ = find_build(le)
    mode = ("  [INERT: stub written, NOT wired]" if args.inert
            else "  [PASSTHROUGH: bare call/ret trampoline]" if args.passthrough else "")
    mode += "  [--no-extend: vsize NOT page-aligned]" if args.no_extend else ""
    print(f"build={b_.name}  hook={b_.hook_va:#x}  cave={b_.cave_va:#x}  "
          f"wallclock={b_.wallclock:#x}{mode}")
    stub = patch(le, b_, args.period, wire=not args.inert, passthrough=args.passthrough,
                 extend=not args.no_extend, floor=args.floor)

    out = _out_path()
    with open(out, "wb") as f:
        f.write(le.data)
    tag = ", INERT" if args.inert else ", PASSTHROUGH" if args.passthrough else ""
    floor_tag = f", floor={args.floor}" if args.floor else ", floor=OFF"
    print(f"wrote {out}  (stub {len(stub)} B, period={args.period}{floor_tag}{tag})")
    verify(out, args.period, inert=args.inert, passthrough=args.passthrough)
    return 0


if __name__ == "__main__":
    sys.exit(main())
