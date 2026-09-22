#!/usr/bin/env python3
"""Determinism check: two benchmark runs must produce identical results
modulo wall-clock fields (latency_ms) and baseline-tier relevance, which
is intentionally stateful (cold_start_score reads access_count — bumped
on every delivery — and recency relative to wall-clock).

Any other difference in result sets, ordering, relevance scores, or
context sections is a correctness signal.

Protocol: get_context mutates the access counts that feed baseline rule
selection, so packs are NOT idempotent across runs against a live DB.
Restore an identical snapshot before each run:

    cp .cogz/cogz.db /tmp/snap.db            # also -wal/-shm if present
    run.py ... --out run_a.json
    cp /tmp/snap.db .cogz/cogz.db            # restore identical state
    run.py ... --out run_b.json
    determinism_check.py run_a.json run_b.json

Exit 0 = identical; exit 1 = differences found (printed).
"""

import json
import sys
from pathlib import Path

IGNORE = {"latency_ms"}

# Baseline-tier section relevance is intentionally stateful:
# cold_start_score = w·confidence + w·ln(access_count+1) + w·recency(now),
# so it drifts with accumulated deliveries and wall-clock between runs.
# Pack *composition* (ids, order, content) is the deterministic contract;
# compare relevance only for non-baseline tiers.
BASELINE_VOLATILE = {"relevance"}


def strip_volatile(obj):
    if isinstance(obj, dict):
        if obj.get("tier") == "baseline":
            return {
                k: strip_volatile(v)
                for k, v in obj.items()
                if k not in IGNORE and k not in BASELINE_VOLATILE
            }
        return {k: strip_volatile(v) for k, v in obj.items() if k not in IGNORE}
    if isinstance(obj, list):
        return [strip_volatile(v) for v in obj]
    return obj


def main():
    a = json.loads(Path(sys.argv[1]).read_text())
    b = json.loads(Path(sys.argv[2]).read_text())

    ra = {r["id"]: strip_volatile(r) for r in a.get("results", [])}
    rb = {r["id"]: strip_volatile(r) for r in b.get("results", [])}

    diffs = []
    if set(ra) != set(rb):
        diffs.append(f"query id sets differ: {sorted(set(ra) ^ set(rb))}")

    for qid in sorted(set(ra) & set(rb)):
        if ra[qid] != rb[qid]:
            ea, eb = ra[qid], rb[qid]
            keys = set(ea) | set(eb)
            for key in sorted(keys):
                if ea.get(key) != eb.get(key):
                    diffs.append(f"{qid}.{key}: {str(ea.get(key))[:200]} != {str(eb.get(key))[:200]}")

    if diffs:
        print(f"NON-DETERMINISTIC: {len(diffs)} difference(s)")
        for d in diffs[:50]:
            print(f"  {d}")
        sys.exit(1)
    print(f"DETERMINISTIC: {len(ra)} queries identical across runs")


if __name__ == "__main__":
    main()
