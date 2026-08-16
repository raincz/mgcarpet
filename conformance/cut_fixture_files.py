#!/usr/bin/env python3
"""Cut a manifest's fixtures into ONE SELF-CONTAINED FILE PER FIXTURE.

    cut_fixture_files.py <manifest.json> [<source.mgcr>] [--all] [--dry-run]

The bundle-per-take shape made the unit of storage (one 14-43 MB
git-lfs object) different from the unit of curation (a row in a JSON
array) and from the unit of test (one #[test] for all ten takes). The
consequences were all in one direction: appending a single fixture
re-froze a whole take, minting a fresh permanent LFS object; 96% of
the corpus accumulated with no note and no status because nothing made
that visible; and a failure named a tick rather than a law.

One file per fixture makes those four units the same object. The file
is named for the story it pins AND NOTHING ELSE — no tick, no take —
so `git rm` is the curation tool, `cargo test <lawname>` runs exactly
one pair, and the FILESYSTEM enforces one exemplar per story: merging
l32's two takes collapsed 54 files onto 39 distinct laws, one of them
pinned five times over. Which recording a pair came out of is
provenance (`source`), not identity: a fixture is a pair of retail
states for a LEVEL, and it outlives its take — mc1l32-bee-height's
recording is gone while its fixtures still guard three laws the
surviving take never captured.

    conformance/fixtures/<level>/<law-slug>.mgcr

Each file is an ordinary `.mgcr` holding the header plus THREE tick
records — `t-1`, `t`, `t+1`:

  * `t-1` warms the cast-edge predecessor (MC1 `prev_fire`,
    verify.rs:fire_bits_mc1 -> set_prev_fire; MC2 `prev_latch`) and
    carries the materialized terrain base. It does NOT need `obs`.
  * `t` is the state the runner imports.
  * `t+1` carries the `obs` the pair is graded against, and needs no
    terrain (the runner applies line L's block while processing L+1,
    so line t+1's block would never be read).

Terrain follows the retired freeze_fixtures.py exactly: accumulate across EVERY
record and materialize the running image into the window's first line
AFTER that line's own delta, so the image at exec time is
base(t-1) + delta(t) — the same one the bundle run graded under. Every
per-file window is a window-first, so this runs for all of them.

By default only fixtures carrying a note are cut: a fixture that
nobody could describe in a sentence is `--sample-every` ballast, and
the recording remains the source of truth if one is ever wanted back.
`--all` cuts every fixture in the manifest.
"""
import base64
import json
import os
import re
import struct
import subprocess
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
flags = {a for a in sys.argv[1:] if a.startswith("--")}
if not args:
    sys.exit(__doc__.strip().splitlines()[2].strip())

man_path = args[0]
man_dir = re.sub(r"[^/]+$", "", man_path) or "./"
man = json.load(open(man_path))
take = re.sub(r"\.json$", "", man_path.split("/")[-1])
src = args[1] if len(args) > 1 else man_dir + man["recording"]

def described(f):
    """The note with FREEZE boilerplate removed — what a human actually
    said about this pair. 119 of the corpus's 158 hand-noted pending
    rows carried only `f26/charge raw-lane surfacing — lanes not
    compared at freeze`, a bookkeeping artifact naming no retail law,
    and ~140 more carried classify_fixtures.py's machine triage. Both
    read as notes and neither is a story, so both strip to nothing and
    the fixture is treated as the ballast it is.

    This is the SELECTION predicate as well as the slug source: a
    fixture nobody could describe in a sentence does not earn a file.
    """
    n = f.get("note", "")
    for pat in (
        r"f26/charge raw-lane surfacing.*",
        r"auto-triage vs known-deviations:.*",
        r"\[carried from [^\]]*\]",
        r"\(see t=\d+\)",
    ):
        n = re.sub(pat, "", n, flags=re.I)
    return n.strip(" ;,")


def slugify(f):
    n = re.sub(r"[^a-z0-9]+", "-", described(f).lower()).strip("-")
    return n[:48].rstrip("-") or "unnamed"


chosen = [f for f in man["fixtures"] if "--all" in flags or described(f)]
if not chosen:
    sys.exit(f"{man_path}: nothing to cut (no described fixtures; use --all)")


# The directory is the LEVEL, not the take: two takes of the same level
# produce interchangeable fixtures, and splitting them by take hid
# duplicates behind a directory boundary.
level = re.sub(r"-[a-z][a-z0-9-]*$", "", take) if "-" in take else take
out_dir = f"{man_dir}fixtures/{level}"
wanted = {}          # tick -> (fixture, slug)
taken = {re.sub(r"\.mgcr$", "", os.path.basename(p))
         for p in __import__("glob").glob(f"{out_dir}/*.mgcr")}
for f in chosen:
    s = slugify(f)
    if s in taken:
        print(f"  skip t={f['t']}: law `{s}` is already pinned")
        continue
    taken.add(s)
    wanted[f["t"]] = (f, s)

lines_needed = set()
for t in wanted:
    lines_needed |= {t - 1, t, t + 1}

if "--dry-run" in flags:
    print(f"{man_path}: would cut {len(wanted)} files into {out_dir}/")
    for t in sorted(wanted):
        print(f"  {wanted[t][1]}.mgcr")
    sys.exit(0)

os.makedirs(out_dir, exist_ok=True)

T_RE = re.compile(rb'^\{"t":(\d+),')
TERRAIN_RE = re.compile(rb'"terrain":(\{[^{}]*\})')


class TerrainAccum:
    """Mirrors mgc_formats::mgcr::TerrainImage."""

    def __init__(self, decl):
        self.n = decl["dims"][0] * decl["dims"][1]
        self.planes = [bytearray(self.n) for _ in decl["planes"]]

    def apply_line(self, line):
        m = TERRAIN_RE.search(line)
        if not m:
            return
        block = json.loads(m.group(1))
        if "base_b64" in block:
            raw = base64.b64decode(block["base_b64"])
            assert len(raw) == self.n * len(self.planes), "bad terrain base"
            for i, p in enumerate(self.planes):
                p[:] = raw[i * self.n:(i + 1) * self.n]
        if "delta_b64" in block:
            raw = base64.b64decode(block["delta_b64"])
            o = 0
            for p in self.planes:
                (cnt,) = struct.unpack_from("<I", raw, o)
                o += 4
                for _ in range(cnt):
                    cell, val = struct.unpack_from("<HB", raw, o)
                    o += 3
                    p[cell] = val
            assert o == len(raw), "trailing terrain delta bytes"

    def base_b64(self):
        return base64.b64encode(b"".join(self.planes)).decode("ascii")


dec = subprocess.Popen(["zstdcat", src], stdout=subprocess.PIPE, bufsize=1 << 22)
header = json.loads(dec.stdout.readline())
assert header["type"] == "header", header
decl = header.get("channels", {}).get("terrain")
accum = TerrainAccum(decl) if decl else None

# One streaming pass fills every file's three lines.
buf = {t: {} for t in wanted}
for line in dec.stdout:
    m = T_RE.match(line)
    if not m:
        continue
    tt = int(m.group(1))
    if accum is not None:
        accum.apply_line(line)
    if tt not in lines_needed:
        continue
    for t in wanted:
        if tt not in (t - 1, t, t + 1):
            continue
        rec = json.loads(line)
        if tt == t - 1:
            if accum is not None:
                rec["terrain"] = {"base_b64": accum.base_b64()}
            rec.pop("obs", None)
        elif tt == t:
            rec.pop("obs", None)
        else:
            rec.pop("terrain", None)
        buf[t][tt] = json.dumps(rec, separators=(",", ":")).encode() + b"\n"
dec.stdout.close()
dec.wait()

written, broken = [], []
for t, (f, slug) in sorted(wanted.items()):
    have = buf[t]
    if any(x not in have for x in (t - 1, t, t + 1)):
        broken.append(t)
        continue
    name = f"{slug}.mgcr"
    h = dict(header)
    h.setdefault("capture", {})
    # Stamped now, read later: adding a digest field after files are
    # committed would re-freeze every one of them (a fresh permanent
    # LFS object each), whereas Header.capture is already a
    # pass-through Value. `entry_sha256` is the level data's identity
    # (mgc_formats::Meta.source.entry_sha256) — the fixture's unstated
    # premise, which today makes a red ambiguous between "the port
    # regressed" and "your game data differs".
    h["capture"]["fixture"] = {
        "take": take,
        "slug": slug,
        "t": t,
        "lines": [t - 1, t, t + 1],
        "note": f.get("note", ""),
        "source": src.split("/")[-1],
    }
    payload = json.dumps(h, separators=(",", ":")).encode() + b"\n"
    payload += b"".join(have[x] for x in (t - 1, t, t + 1))
    enc = subprocess.Popen(
        ["zstd", "-19", "-q", "-f", "-o", f"{out_dir}/{name}"],
        stdin=subprocess.PIPE,
    )
    enc.communicate(payload)
    written.append((name, os.path.getsize(f"{out_dir}/{name}")))

if broken:
    sys.exit(
        f"{man_path}: source {src} lacks the [t-1,t,t+1] lines for t={broken} — "
        f"an incomplete file would be silently unreachable; pass the full take"
    )

total = sum(s for _, s in written)
for name, size in written:
    print(f"  {size:8d}  {name}")
print(
    f"== {man_path}: cut {len(written)} files ({total} B, "
    f"mean {total // max(1, len(written))} B) into {out_dir}/"
)
