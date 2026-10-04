#!/usr/bin/env python3
"""Offline rerank evaluation: rescore the stored top-K results of the
commit benchmark with a cross-encoder and measure the delta on the
suite metrics (P@5, MRR, R@20) without touching the product.

Only measures *ordering* benefit — a production reranker could also
widen the candidate pool (fetch 40, rerank, return 20), which this
cannot see.
"""
import argparse, json, os, sqlite3, sys, time
from pathlib import Path

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

BENCH = Path(__file__).parent.parent


class Reranker:
    def __init__(self, model_dir):
        model_dir = Path(model_dir)
        self.tok = Tokenizer.from_file(str(model_dir / "tokenizer.json"))
        self.tok.no_padding()
        self.tok.enable_truncation(512)
        self.sess = ort.InferenceSession(
            str(model_dir / "onnx/model_quantized.onnx"),
            providers=["CPUExecutionProvider"])
        self.inputs = {i.name for i in self.sess.get_inputs()}

    def score(self, query, docs):
        enc = self.tok.encode_batch([(query, d) for d in docs])
        maxlen = max(len(e.ids) for e in enc)
        ids = np.zeros((len(enc), maxlen), dtype=np.int64)
        mask = np.zeros((len(enc), maxlen), dtype=np.int64)
        tids = np.zeros((len(enc), maxlen), dtype=np.int64)
        for i, e in enumerate(enc):
            ids[i, :len(e.ids)] = e.ids
            mask[i, :len(e.attention_mask)] = e.attention_mask
            tids[i, :len(e.type_ids)] = e.type_ids
        feed = {"input_ids": ids, "attention_mask": mask,
                "token_type_ids": tids}
        feed = {k: v for k, v in feed.items() if k in self.inputs}
        logits = self.sess.run(None, feed)[0]
        s = logits.reshape(len(docs), -1)[:, 0]
        return 1.0 / (1.0 + np.exp(-s))


def load_docs(db_path, ids):
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        ph = ",".join("?" * len(ids))
        rows = conn.execute(
            f"select id, title, content from entities where id in ({ph})",
            list(ids)).fetchall()
        return {r[0]: (r[1] or "", r[2] or "") for r in rows}
    finally:
        conn.close()


def metrics(rows, k=5):
    pk, mrr, r20 = [], [], []
    for direct, expanded, live_expected in rows:
        if not live_expected:
            continue
        did = [x[0] for x in direct]
        eid = [x[0] for x in expanded]
        pk.append(len([e for e in live_expected if e in did[:k]]) / k)
        rr = next((1.0 / i for i, e in enumerate(did, 1)
                   if e in live_expected), 0.0)
        mrr.append(rr)
        hits = len(set(live_expected) & (set(did) | set(eid)))
        r20.append(hits / len(live_expected))
    n = max(1, len(r20))
    return sum(pk) / n, sum(mrr) / n, sum(r20) / n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--model", required=True)
    ap.add_argument("--phase", default="commit")
    ap.add_argument("--run", default=None,
                    help="run json path; default results/suite/{corpus}/{phase}_run.json")
    ap.add_argument("--topk", type=int, default=20,
                    help="direct-results cutoff for metrics")
    args = ap.parse_args()

    corpora = Path(os.environ.get("COGZ_BENCH_CORPORA",
                                  BENCH / "corpora"))
    repo = corpora / args.corpus
    run_path = (Path(args.run) if args.run
                else BENCH / f"results/suite/{args.corpus}/{args.phase}_run.json")
    raw = json.loads(run_path.read_text())
    qset = json.loads((BENCH / f"suite/queries/{args.corpus}_{args.phase}.json")
                      .read_text())
    qmap = {q["id"]: q for q in qset["queries"]}
    corpus_ids = set(raw["corpus_ids"])
    rr_model = Reranker(args.model)

    all_ids = {x["id"] for r in raw["results"] for x in r.get("results", [])}
    docs = load_docs(repo / ".cogz/cogz.db", list(all_ids))

    base_rows, rerank_rows, lat = [], [], []
    for qi, r in enumerate(raw["results"]):
        q = qmap.get(r["id"])
        if q is None:
            continue
        if qi % 10 == 0:
            print(f"  ...query {qi}/{len(raw['results'])}", file=sys.stderr, flush=True)
        res = r.get("results", [])
        direct = [(x["id"], x) for x in res
                  if (x.get("graph_path_len") or 1) <= 1]
        expanded = [(x["id"], x) for x in res
                    if (x.get("graph_path_len") or 1) > 1]
        live = [e for e in q.get("expected_entity_ids", [])
                if e in corpus_ids]
        base_rows.append((direct[:args.topk], expanded, live))

        texts = [(docs[i][0] + "\n" + docs[i][1])[:1500]
                 for i, _ in direct if i in docs]
        keep = [d for d in direct if d[0] in docs]
        if texts:
            t0 = time.monotonic()
            scores = rr_model.score(q["query"], texts)
            lat.append((time.monotonic() - t0) * 1000 / len(texts))
            order = np.argsort(-scores)
            direct = [keep[i] for i in order]
        rerank_rows.append((direct[:args.topk], expanded, live))

    b = metrics(base_rows)
    n = metrics(rerank_rows)
    print(f"{args.corpus}/{args.phase} {Path(args.model).name}")
    print(f"  baseline: P@5={b[0]:.3f} MRR={b[1]:.3f} R@20={b[2]:.3f}")
    print(f"  reranked: P@5={n[0]:.3f} MRR={n[1]:.3f} R@20={n[2]:.3f}")
    print(f"  delta:    {n[0]-b[0]:+.3f} {n[1]-b[1]:+.3f} {n[2]-b[2]:+.3f}")
    if lat:
        lat.sort()
        print(f"  rerank p50={lat[len(lat)//2]:.0f}ms/doc")


if __name__ == "__main__":
    main()
