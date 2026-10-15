#!/usr/bin/env python3
"""Paired per-query comparison of two arms across corpora.

For each shared query id, computes per-query metrics under both arms —
hit@5, reciprocal rank, hit@20, pack hit — then reports win/loss/tie
counts, mean deltas, and a bootstrap CI on the delta. Paired analysis
is what aggregate tables hide: which queries actually change.

    python3 compare_arms.py --a hybrid --b small [--corpus clap]
"""
import argparse
import json
import random
import sys
from pathlib import Path

RESULTS = Path(__file__).parent.parent / "results" / "suite"
PHASES = ("seeded", "commit", "gap")


def load(path: Path, queries: dict) -> dict:
    """query_id -> per-query metric row for one arm."""
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return {}
    out = {}
    for r in data.get("results", []):
        qid = r["id"]
        exp = queries.get(qid, [])
        live = [e for e in exp]  # scorer uses corpus_ids; keep simple
        hits = [h.get("entity_id") or h.get("id") for h in r.get("results", [])]
        sec_ids = {s.get("entity_id") for s in
                   r.get("context", {}).get("sections", [])}
        rank = next((i + 1 for i, h in enumerate(hits) if h in live), None)
        out[qid] = {
            "hit5": int(bool(rank and rank <= 5)),
            "rr": 1.0 / rank if rank else 0.0,
            "hit20": int(bool(rank and rank <= 20)),
            "pack": int(bool(set(live) & sec_ids)),
        }
    return out


def boot_ci(deltas: list[float], iters: int = 4000) -> tuple[float, float]:
    if not deltas:
        return (0.0, 0.0)
    random.seed(7)
    means = sorted(
        sum(random.choices(deltas, k=len(deltas))) / len(deltas)
        for _ in range(iters)
    )
    return (means[int(0.025 * iters)], means[int(0.975 * iters)])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--a", required=True)
    ap.add_argument("--b", required=True)
    ap.add_argument("--corpus", default=None)
    args = ap.parse_args()

    corpora = [args.corpus] if args.corpus else [
        d.name for d in RESULTS.iterdir() if (d / "arms").is_dir()]

    qdir = Path(__file__).parent / "queries"
    for corpus in corpora:
        print(f"\n===== {corpus}: {args.a} vs {args.b} =====")
        for ph in PHASES:
            qf = qdir / f"{corpus}_{ph}.json"
            pa = RESULTS / corpus / "arms" / f"{args.a}_{ph}.json"
            pb = RESULTS / corpus / "arms" / f"{args.b}_{ph}.json"
            if not (qf.exists() and pa.exists() and pb.exists()):
                continue
            queries = {q["id"]: q["expected_entity_ids"]
                       for q in json.loads(qf.read_text())["queries"]}
            A, B = load(pa, queries), load(pb, queries)
            shared = sorted(set(A) & set(B))
            if not shared:
                continue
            print(f"  {ph} (n={len(shared)})")
            for metric in ("hit5", "rr", "hit20", "pack"):
                deltas = [B[q][metric] - A[q][metric] for q in shared]
                wins = sum(1 for d in deltas if d > 0)
                losses = sum(1 for d in deltas if d < 0)
                ties = len(deltas) - wins - losses
                mean = sum(deltas) / len(deltas)
                lo, hi = boot_ci(deltas)
                sig = "*" if lo > 0 or hi < 0 else " "
                print(f"    {metric:5} Δ={mean:+.3f} [{lo:+.3f},{hi:+.3f}]{sig}"
                      f"  win={wins} loss={losses} tie={ties}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
