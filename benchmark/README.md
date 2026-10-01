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
| `v052_hybrid.json` | full pipeline (FTS + vec + graph expansion) — v0.5.1+, grown corpus | 0.085 | **0.298** | **0.653** | 0.69 s |
| `v052_fts.json` | FTS-only degradation | 0.071 | 0.184 | 0.600 | 0.43 s |
| `v052_noexpand.json` | hybrid, no graph expansion | 0.085 | 0.298 | 0.509 | 0.68 s |
| `k3_hybrid.json` | v0.5.0-era corpus snapshot | 0.103 | 0.336 | 0.659 | 3.7 s |
| `k3_fts.json` | FTS-only degradation (v0.5.0 era) | 0.097 | 0.204 | 0.634 | 0.6 s |
| `k3_noexpand.json` | hybrid, no graph expansion (v0.5.0 era) | 0.100 | 0.336 | 0.506 | 3.8 s |
| `v2.json` | earlier pipeline snapshot | 0.106 | 0.367 | 0.581 | 0.4 s |

Context-pack metrics (`v052_hybrid`, `--with-context`): pack recall 0.670,
avg 8,182 tokens per pack, avg 69.9 sections, zero packs with duplicate
content above threshold.

Reading the deltas:

- **Vector channel lifts MRR ~60%** (0.184 FTS-only → 0.298 hybrid) —
  semantic recall matters for knowledge phrasing that doesn't share
  keywords.
- **Graph expansion adds +0.14 recall@20** (0.509 → 0.653) at flat MRR —
  expansion surfaces entities that direct ranking misses, at the cost of
  latency (ONNX inference dominates the 0.69 s; FTS-only runs in 0.43 s).
- The v052 dip vs the k3 snapshot (MRR 0.298 vs 0.336, P@5 0.085 vs
  0.103) is corpus growth — 1 rotted expectation and a denser candidate
  pool — with overlapping confidence intervals, not a pipeline
  regression. Latency improved ~5× (3.7 s → 0.69 s) from warm
  model reuse in the persistent benchmark session.
- Latency is dominated by model inference on a CPU-only box; see
  `docs/evaluations/` for the resource profile.

`results/` also contains tuning-grid outputs (`detect_*.json`,
`calibrated_*.json`, `v2_*.json`) from silence-gate and expansion-parameter
sweeps — kept as the audit trail for the shipped defaults.
