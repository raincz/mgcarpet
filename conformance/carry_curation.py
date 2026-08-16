#!/usr/bin/env python3
"""Carry fixture curation across a re-record (docs/CONFORMANCE.md
"Re-extract").

    carry_curation.py <manifest.json> [<git-rev>]

Signatures capture the STORY of a failure, so they are the bridge
between the old take's curated manifest (statuses + ledger notes) and
a fresh extract: every new fixture whose sig appears in the old
manifest inherits its status and note. Prints the reconciliation —
carried, new-story (unmatched new sigs), and vanished-story (old sigs
absent from the new extract: resolved, drifted, or just not exercised
by the new gameplay).

A SIGNATURE IS NOT ENOUGH ON ITS OWN. `--promote` clears `sig` and
`atoms` (the pair conforms now, so it has no failure to describe), and
a sig-only bridge cannot see a sig-less fixture at all: the promoted
fixture — the one that a fix EARNED — was dropped by the re-extract,
and the vanished report, iterating that same sig-keyed dict, could not
even name what it had lost. So the bridge is three-tier: live `sig`,
then the `won_sig` receipt a promotion leaves behind, then the tick
for curated fixtures whose take was re-extracted rather than
re-recorded. Anything curated and unclaimed is reported LOUDLY — the
one thing this script must never do is lose a human's work in silence.
"""
import json
import subprocess
import sys

if len(sys.argv) < 2:
    sys.exit(__doc__.strip().splitlines()[2].strip())
path = sys.argv[1]
rev = sys.argv[2] if len(sys.argv) > 2 else "HEAD"

old = json.loads(
    subprocess.run(
        ["git", "show", f"{rev}:{path}"], capture_output=True, text=True, check=True
    ).stdout
)
new = json.load(open(path))


def curated(f):
    """Did a human, or a fix, invest in this fixture? A triage note, a
    promotion receipt, or any hand-set non-conforming status. Exactly
    the set that must survive a re-extract; everything else is
    `--sample-every` corpus that the next extract regenerates."""
    return (
        bool(f.get("note"))
        or bool(f.get("won_sig"))
        or f.get("status") != "conforming"
    )


old_by_sig = {f["sig"]: f for f in old["fixtures"] if f.get("sig")}
old_by_won = {f["won_sig"]: f for f in old["fixtures"] if f.get("won_sig")}
old_by_tick = {f["t"]: f for f in old["fixtures"] if curated(f)}

claimed = set()
carried = newborn = revived = by_tick = 0

for f in new["fixtures"]:
    sig = f.get("sig")
    if sig and sig in old_by_sig:
        o = old_by_sig[sig]
        f["status"] = o["status"]
        if o.get("note"):
            f["note"] = o["note"]
        if o.get("won_sig"):
            f["won_sig"] = o["won_sig"]
            f["won_atoms"] = o.get("won_atoms", [])
        claimed.add(id(o))
        carried += 1
    elif sig and sig in old_by_won:
        # The story a fix once closed is failing again in the fresh
        # extract. The extract already statused it `open`; do NOT quietly
        # carry the old `conforming` over the top of that.
        o = old_by_won[sig]
        if o.get("note"):
            f["note"] = o["note"]
        claimed.add(id(o))
        revived += 1
        print(f"  REVIVED story t={f['t']} (was promoted at old t={o['t']}): {' '.join(f['atoms'])}")
    elif f["t"] in old_by_tick:
        # Same take re-extracted rather than re-recorded, so ticks still
        # line up. This is the only bridge a PROMOTED fixture has: it
        # conforms, so it carries no live sig to match on.
        o = old_by_tick[f["t"]]
        if o.get("note"):
            f["note"] = o["note"]
        if o.get("won_sig"):
            f["won_sig"] = o["won_sig"]
            f["won_atoms"] = o.get("won_atoms", [])
        if o["status"] == "conforming" and o.get("won_sig"):
            f["status"] = "conforming"
        claimed.add(id(o))
        by_tick += 1
    elif sig:
        newborn += 1
        print(f"  NEW story t={f['t']}: {' '.join(f['atoms'])}")

vanished = [f for f in old["fixtures"] if curated(f) and id(f) not in claimed]
for f in vanished:
    why = "promoted receipt" if f.get("won_sig") else f["status"]
    print(f"  VANISHED ({why}, old t={f['t']}): {f.get('note', '')[:80]}")

json.dump(new, open(path, "w"), indent=2)
open(path, "a").write("\n")
print(
    f"== {path}: {carried} carried, {by_tick} carried by tick, {revived} revived, "
    f"{newborn} new stories, {len(vanished)} curated fixtures NOT re-extracted"
)
