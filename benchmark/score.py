#!/usr/bin/env python3
"""Deterministic scorer for CogZ regression benchmark raw output.

Metrics, per intent and overall:
  P@5 / MRR / Recall@20 on DIRECT results (graph_path_len <= 1)
  Expansion stats on expanded results (graph_path_len > 1) — separate,
    because expansion noise is the named failure mode
  Negative-query correctness: nothing above --floor among directs
  Rot detection: expected ID absent from corpus != retrieval failure

Usage: python3 score.py raw.json queries.json [--floor 0.05] [--k 5]
"""

import argparse
import json
import sys
from pathlib import Path


def score_run(raw: dict, qset: dict, k: int, floor: float) -> dict:
    corpus = set(raw.get("corpus_ids", []))
    qmap = {q["id"]: q for q in qset["queries"]}

    per_query = []
    for r in raw["results"]:
        qid = r["id"]
        q = qmap.get(qid)
        if q is None:
            continue
        expected = set(q.get("expected_entity_ids", []))
        intent = q["intent"]

        if "error" in r:
            per_query.append({"id": qid, "intent": intent, "error": r["error"]})
            continue

        results = r.get("results", [])
        direct = [x for x in results if (x.get("graph_path_len") or 1) <= 1]
        expanded = [x for x in results if (x.get("graph_path_len") or 1) > 1]
        direct_ids = [x["id"] for x in direct]
        expanded_ids = [x["id"] for x in expanded]
        all_ids = direct_ids + expanded_ids

        # Rot detection: expected IDs not in corpus at all
        rotted = [e for e in expected if corpus and e not in corpus]
        live_expected = expected - set(rotted)

        top_k_direct = direct_ids[:k]
        hits_k = [e for e in live_expected if e in top_k_direct]
        hits_all_direct = [e for e in live_expected if e in direct_ids]
        hits_expanded = [e for e in live_expected if e in expanded_ids]

        # MRR over direct results only
        rr = 0.0
        for rank, eid in enumerate(direct_ids, 1):
            if eid in live_expected:
                rr = 1.0 / rank
                break

        entry = {
            "id": qid,
            "intent": intent,
            "n_direct": len(direct),
            "n_expanded": len(expanded),
            "expected": len(expected),
            "rotted": len(rotted),
            "p_at_k": (len(hits_k) / k) if expected else None,
            "recall_at_20": (len(set(hits_all_direct) | set(hits_expanded)) / len(live_expected)) if live_expected else (1.0 if not expected else 0.0),
            "mrr": rr,
            "top_direct_relevance": direct[0]["relevance"] if direct else None,
            "hits_expanded_only": [e for e in hits_expanded if e not in hits_all_direct],
            "latency_ms": r.get("latency_ms"),
            "search_mode": r.get("search_mode"),
        }

        if intent == "negative":
            # Success = nothing returned above floor among directs.
            above = [x for x in direct if (x.get("relevance") or 0) >= floor]
            entry["negative_ok"] = len(above) == 0
            entry["negative_top"] = direct[0]["relevance"] if direct else None

        # context-pack metrics if present
        ctx = r.get("context")
        if ctx and "sections" in ctx:
            secs = ctx["sections"]
            sec_ids = [s.get("entity_id") for s in secs]
            entry["pack_recall"] = (len([e for e in live_expected if e in sec_ids]) / len(live_expected)) if live_expected else None
            entry["pack_sections"] = len(sec_ids)
            meta = ctx.get("metadata") or {}
            entry["pack_tokens"] = meta.get("size_tokens")
            entry["pack_dropped"] = len(meta.get("dropped_sources") or [])

            def _trunc(c):
                return c.endswith("...") or c.endswith("more lines)")
            truncated = [s for s in secs if _trunc(s.get("content") or "")]
            entry["pack_truncated"] = len(truncated)
            entry["pack_expected_truncated"] = len(
                [s for s in truncated if s.get("entity_id") in live_expected])

            # Section-overlap dup rate: max pairwise Jaccard over content
            # line-sets. File sections subsume their functions' lines —
            # that's the overlap this catches.
            line_sets = []
            for s in secs:
                lines = {ln.strip() for ln in (s.get("content") or "").splitlines()
                         if len(ln.strip()) > 3}
                line_sets.append(lines)
            dup_max = 0.0
            dup_pairs = 0
            for i in range(len(line_sets)):
                for j in range(i + 1, len(line_sets)):
                    a, b = line_sets[i], line_sets[j]
                    if not a or not b:
                        continue
                    jac = len(a & b) / len(a | b)
                    dup_max = max(dup_max, jac)
                    if jac >= 0.5:
                        dup_pairs += 1
            entry["pack_dup_max"] = round(dup_max, 3)
            entry["pack_dup_pairs"] = dup_pairs

        per_query.append(entry)

    # aggregate
    intents = {}
    for e in per_query:
        intents.setdefault(e["intent"], []).append(e)

    def agg(rows, field):
        vals = [x[field] for x in rows if x.get(field) is not None]
        return round(sum(vals) / len(vals), 3) if vals else None

    by_intent = {}
    for intent, rows in intents.items():
        by_intent[intent] = {
            "n": len(rows),
            "p_at_k": agg(rows, "p_at_k"),
            "mrr": agg(rows, "mrr"),
            "recall_at_20": agg(rows, "recall_at_20"),
            "avg_expanded": round(sum(x["n_expanded"] for x in rows) / len(rows), 1),
            "avg_latency_ms": agg(rows, "latency_ms"),
        }
        if intent == "negative":
            ok = [x for x in rows if x.get("negative_ok")]
            by_intent[intent]["negative_ok_rate"] = round(len(ok) / len(rows), 3) if rows else None

    overall_rows = [x for x in per_query if "error" not in x and x["intent"] != "negative"]
    out = {
        "k": k,
        "floor": floor,
        "total_queries": len(per_query),
        "errors": [x["id"] for x in per_query if "error" in x],
        "rotted_expectations": sum(x.get("rotted", 0) for x in per_query),
        "overall": {
            "p_at_k": agg(overall_rows, "p_at_k"),
            "mrr": agg(overall_rows, "mrr"),
            "recall_at_20": agg(overall_rows, "recall_at_20"),
            "avg_latency_ms": agg(overall_rows, "latency_ms"),
        },
        "by_intent": by_intent,
        "per_query": per_query,
    }

    pack_rows = [x for x in per_query if x.get("pack_sections") is not None]
    if pack_rows:
        out["pack"] = {
            "n": len(pack_rows),
            "recall": agg(pack_rows, "pack_recall"),
            "avg_sections": agg(pack_rows, "pack_sections"),
            "avg_tokens": agg(pack_rows, "pack_tokens"),
            "avg_dropped": agg(pack_rows, "pack_dropped"),
            "avg_truncated": agg(pack_rows, "pack_truncated"),
            "expected_truncated": sum(x.get("pack_expected_truncated", 0) for x in pack_rows),
            "avg_dup_max": agg(pack_rows, "pack_dup_max"),
            "packs_with_dup": len([x for x in pack_rows if x.get("pack_dup_pairs", 0) > 0]),
        }
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("raw")
    ap.add_argument("queries")
    ap.add_argument("--floor", type=float, default=0.05)
    ap.add_argument("--k", type=int, default=5)
    ap.add_argument("--out")
    args = ap.parse_args()

    raw = json.loads(Path(args.raw).read_text())
    qset = json.loads(Path(args.queries).read_text())
    report = score_run(raw, qset, args.k, args.floor)

    text = json.dumps(report, indent=2)
    if args.out:
        Path(args.out).write_text(text)
        print(f"wrote {args.out}", file=sys.stderr)

    o = report["overall"]
    print(f"\n=== CogZ benchmark — {report['total_queries']} queries ===")
    print(f"overall: P@{report['k']}={o['p_at_k']}  MRR={o['mrr']}  R@20={o['recall_at_20']}  avg {o['avg_latency_ms']}ms")
    print(f"errors: {report['errors'] or 'none'}  rotted-expected: {report['rotted_expectations']}")
    print(f"{'intent':<10} {'n':>3} {'P@k':>6} {'MRR':>6} {'R@20':>6} {'exp/q':>6} {'ms':>7}")
    for intent, m in report["by_intent"].items():
        extra = f"  neg_ok={m['negative_ok_rate']}" if intent == "negative" else ""
        print(f"{intent:<10} {m['n']:>3} {str(m['p_at_k']):>6} {str(m['mrr']):>6} {str(m['recall_at_20']):>6} {m['avg_expanded']:>6} {str(m['avg_latency_ms']):>7}{extra}")
    if "pack" in report:
        p = report["pack"]
        print(f"\npacks ({p['n']}): recall={p['recall']}  sections={p['avg_sections']}  "
              f"tokens={p['avg_tokens']}  dropped={p['avg_dropped']}  trunc={p['avg_truncated']}  "
              f"exp_trunc={p['expected_truncated']}  dup_max={p['avg_dup_max']}  "
              f"packs_w_dup={p['packs_with_dup']}")


if __name__ == "__main__":
    main()
