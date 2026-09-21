# CogZ Retrieval Benchmark

Self-contained regression benchmark for CogZ retrieval quality. Everything
in this directory runs against CogZ's own repository — its `.cogz/` corpus
(knowledge, rules, observations) plus its indexed source code. No external
datasets or repositories are required.

## Layout

| File | Purpose |
|---|---|
| `queries.json` | 32 labeled queries (v1 set) — early smoke corpus |
| `queries_v2.json` | 78 labeled queries (v2 set) — current regression corpus |
| `run.py` | Runner: drives one persistent `cogz mcp-stdio` session, issues `search` (+ optional `get_context`) calls, dumps raw results |
| `score.py` | Scorer: computes P@k / MRR / Recall@20 on direct results, expansion stats separately, negative-query and rotted-expectation handling |
| `results/` | Committed score outputs from measured runs |

Each query in a query set carries an `id`, an `intent` class
(`knowledge`, `code`, `mixed`, `graph`, `hardneg`, `neg`), the query text,
and `expected_entity_ids`/`expected_titles` the retrieval should surface.

## Running

```bash
# Raw run — search only
python3 benchmark/run.py --repo /path/to/repo \
    --queries benchmark/queries_v2.json --out benchmark/results/raw.json

# With context-pack metrics (adds get_context task-mode stats)
python3 benchmark/run.py --repo /path/to/repo \
    --queries benchmark/queries_v2.json --with-context --out raw.json

# Ablation flags
python3 benchmark/run.py ... --no-expand      # expand=false on all searches
python3 benchmark/run.py ... --code-search    # code model for query embedding

# Score
python3 benchmark/score.py benchmark/results/raw.json \
    benchmark/queries_v2.json [--floor 0.05] [--k 5]
```

`score.py` separates **direct** results (graph path ≤ 1) from **expanded**
results, because expansion noise is a named failure mode and must not hide
inside direct-precision metrics. `hardneg`/`neg` queries assert that nothing
scores above `--floor`; `rotted_expectations` reports expected entities that
no longer exist in the corpus (a corpus-health signal, not a retrieval miss).

## Committed results (v2 query set, k=5, floor=0.05)

| Run | Variant | P@5 | MRR | Recall@20 | Avg latency |
|---|---|---|---|---|---|
| `k3_hybrid.json` | full pipeline (FTS + vec + graph expansion) | 0.103 | **0.336** | **0.659** | 3.7 s |
| `k3_fts.json` | FTS-only degradation | 0.097 | 0.204 | 0.634 | 0.6 s |
| `k3_noexpand.json` | hybrid, no graph expansion | 0.100 | 0.336 | 0.506 | 3.8 s |
| `v2.json` | earlier pipeline snapshot | 0.106 | 0.367 | 0.581 | 0.4 s |

Context-pack metrics (`k3_hybrid`, `--with-context`): pack recall 0.816,
avg 8,174 tokens per pack, avg 73.7 sections, zero packs with duplicate
content above threshold.

Reading the deltas:

- **Vector channel doubles MRR** (0.204 FTS-only → 0.336 hybrid) — semantic
  recall matters for knowledge phrasing that doesn't share keywords.
- **Graph expansion adds +0.15 recall@20** (0.506 → 0.659) at flat MRR —
  expansion surfaces entities that direct ranking misses, at the cost of
  latency (ONNX embedding dominates the 3.7 s; FTS-only runs in 0.6 s).
- Latency is dominated by model inference on a CPU-only box; see
  `docs/evaluations/` for the resource profile.

`results/` also contains tuning-grid outputs (`detect_*.json`,
`calibrated_*.json`, `v2_*.json`) from silence-gate and expansion-parameter
sweeps — kept as the audit trail for the shipped defaults.
