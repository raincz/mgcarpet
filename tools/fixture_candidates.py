#!/usr/bin/env python3
"""Rank fixture candidates for a CERTIFIED level by REVERSION PROBE.

    tools/fixture_candidates.py <prefix.tsv> <head.tsv> [--top N] [--tick T] [--sig S]

A certified level is silent under `verify-deltas` at HEAD, so nothing in
a HEAD run says which ticks are load-bearing and which are ballast. The
only instrument that can say is a binary built at the commit JUST
BEFORE the level's fix commit (`tools/conform-rig`): every pair it calls
divergent is a pair where the landed laws actually do work.

    candidate ticks = divergent PRE-FIX  ∧  raw-clean at HEAD

The left half is non-vacuity (revert the laws and this fixture goes
red); the right half is the "fixed work only" rule (a fixture is added
only when it PASSES). Both are measured, neither is guessed.

Ticks are grouped by SIGNATURE — the slot- and value-free atom set,
mirroring mgc-conform's `fixtures::signature`: `field:<class>,<model>:<name>`,
`missing:<class>,<model>`, `extra:<class>,<model>`. One signature is one
story, so the minimal-tick exemplar of each signature is the fixture
worth cutting and the rest of that group is duplicate coverage.

`pose` rows are ignored on BOTH sides: `PairDiff::clean()` tests only
rng/missing/extra/fields, so a pose row neither fails a fixture nor
proves one non-vacuous.

The `rule` column (a known-deviation / roster attribution) is reported
per signature but never filters: a rule-tagged row still counts as
divergence for `clean()`, which is the verdict a fixture inherits.
"""
import collections
import sys

# `--top 60` and `--top=60` both work: a flag left bare takes the next
# positional as its value, so a stray `--top 400` cannot silently fall
# back to the default and print a truncated table that looks complete.
VALUED = ("top", "tick", "sig")
args, flags, pending = [], {}, None
for a in sys.argv[1:]:
    if a.startswith("--"):
        k, eq, v = a[2:].partition("=")
        flags[k] = v if eq else True
        pending = k if not eq and k in VALUED else None
    elif pending:
        flags[pending] = a
        pending = None
    else:
        args.append(a)
if len(args) < 2:
    sys.exit(__doc__.strip().splitlines()[2].strip())

GRADED = ("field", "missing", "extra")


def load(path):
    """tick -> {atoms}, tick -> {rules}, and the raw rows per tick."""
    atoms = collections.defaultdict(set)
    rules = collections.defaultdict(set)
    rows = collections.defaultdict(list)
    with open(path) as fh:
        head = fh.readline().rstrip("\n").split("\t")
        col = {n: i for i, n in enumerate(head)}
        for line in fh:
            f = line.rstrip("\n").split("\t")
            if len(f) < len(head):
                continue
            kind = f[col["kind"]]
            if kind not in GRADED:
                continue
            t = int(f[col["t"]])
            cm = f"{f[col['class']]},{f[col['model']]}"
            atoms[t].add(
                f"field:{cm}:{f[col['field']]}" if kind == "field" else f"{kind}:{cm}"
            )
            rules[t].add(f[col["rule"]] or "-")
            rows[t].append(f)
    return atoms, rules, rows


pre_atoms, pre_rules, pre_rows = load(args[0])
head_atoms, _, _ = load(args[1])

# --tick T: explain one tick and stop. The question an agent asks when
# the ledger names a tick and it wants to know what that tick proves.
if "tick" in flags:
    t = int(flags["tick"])
    print(f"t={t}")
    print(f"  pre-fix : {'DIVERGENT' if t in pre_atoms else 'clean (NO non-vacuity)'}")
    print(f"  HEAD    : {'DIVERGENT (fixture would FAIL)' if t in head_atoms else 'clean'}")
    if t in pre_atoms:
        print(f"  rules   : {' '.join(sorted(pre_rules[t]))}")
        print(f"  atoms   : {' '.join(sorted(pre_atoms[t]))}")
        for r in pre_rows[t][:12]:
            print(f"    slot {r[2]} ({r[3]},{r[4]}) {r[5]}: retail {r[6]} port {r[7]}")
    sys.exit(0)

cand = sorted(set(pre_atoms) - set(head_atoms))
groups = collections.defaultdict(list)
for t in cand:
    groups[" ".join(sorted(pre_atoms[t]))].append(t)

if "sig" in flags:
    want = flags["sig"]
    for sig, ts in groups.items():
        if want in sig:
            print(f"{len(ts):5d} ticks  {sig}")
            print(f"       {ts[:40]}")
    sys.exit(0)

print(
    f"pre-fix divergent ticks: {len(pre_atoms)}   "
    f"HEAD divergent ticks: {len(head_atoms)}   "
    f"CANDIDATES (pre-fix divergent, HEAD clean): {len(cand)}"
)
print(f"distinct stories (signatures): {len(groups)}\n")
top = 60 if flags.get("top", 60) is True else int(flags.get("top", 60))
ranked = sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[1][0]))
print(f"{'ticks':>6}  {'first':>7}  {'rules':<28}  signature")
for sig, ts in ranked[:top]:
    rules = ",".join(sorted(set().union(*(pre_rules[t] for t in ts))))
    print(f"{len(ts):6d}  {ts[0]:7d}  {rules[:28]:<28}  {sig[:150]}")
if len(ranked) > top:
    print(f"\n… {len(ranked) - top} more signatures (--top N)")
