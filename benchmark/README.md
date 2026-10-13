# CogZ Benchmark

Reproducible measurement of CogZ retrieval and context-pack quality on
pinned public corpora, plus CogZ's own repository as a dogfooding corpus.
Nothing in this directory depends on private code or machine-local paths:
corpora are fetched from public GitHub at pinned commits, and every
committed artifact records corpus names — never absolute paths.

## Corpora

| Corpus | Source | Pin | Lang | Entities |
|---|---|---|---|---|
| cobra | `github.com/spf13/cobra` | `adbc8813` | Go | 768 |
| httpx | `github.com/encode/httpx` | `b5addb64` | Python | 1,051 |
| clap | `github.com/clap-rs/clap` | `8ab46fe2` | Rust | 4,664 |
| cogz | this repository | working tree | Rust | 2,368 |

Each public corpus carries an injected `.cogz/` seed tree (observations,
rules, knowledge mined from real git history — `suite/seeds/`), simulating
a long-lived memory corpus on a project that never had one. `suite/tasks/`
holds agent-replay task sets per corpus.

## Layout

| File | Purpose |
|---|---|
| `run.py` | Search driver: one `cogz mcp-stdio` session, `search` (+ optional `get_context`) calls, raw JSON out |
| `score.py` | P@k / MRR / Recall@20 with CIs; negatives scored as "nothing above floor"; pack recall when context enabled |
| `pack_metrics.py` | Pack composition + silence gate + hybrid/FTS-only latency per budget |
| `corpus_metrics.py` | Entity/edge/embedding counts per corpus → `results/corpus_metrics.json` |
| `commit_ground_truth.py` | Builds commit-GT query sets from a repo's git history |
| `determinism_check.py` | Two identical runs must produce identical result lists |
| `reindex_equiv.py` | Incremental reindex ≡ reset+index, on a fixture repo |
| `tombstone_check.py` | Rejected/pruned entity lifecycle on a fixture repo |
| `drift_precision.py` | Stale-flag precision on synthetic reference churn |
| `consolidation_suite.py` | Dedup / contradiction / promotion checks via MCP |
| `suite/run_suite.py` | The standard battery: all phases per corpus |
| `suite/fetch_corpus.py` | Clone @ pin → `cogz init` → inject seeds → index |
| `suite/queries/` | `<corpus>_{seeded,commit,negatives}.json` query sets |
| `suite/run_invariants.py` | Corpus-independent fixture checks (runs the four above) |
| `suite/agent_replay.py` | Fail-to-pass agent replays on real fix commits |
| `queries.json`, `queries_v2.json` | Ad-hoc CogZ self-corpus query sets |

## Reproduce

```bash
export COGZ_BENCH_CORPORA=/path/to/corpora   # default: benchmark/corpora

# First time per corpus: clone, seed, index
python3 benchmark/suite/fetch_corpus.py --corpus httpx

# Full battery per corpus (~15 min each on CPU)
python3 benchmark/suite/run_suite.py --corpus httpx
python3 benchmark/suite/run_suite.py --corpus cobra
python3 benchmark/suite/run_suite.py --corpus clap
python3 benchmark/suite/run_suite.py --corpus cogz

# Corpus-independent invariants, once per binary
python3 benchmark/suite/run_invariants.py
```

Outputs land in `benchmark/results/suite/<corpus>/` and are committed as
the canonical record. `--skip PHASES` reruns subsets; `run_suite.py`
prints each command it runs.

## Current results

Search — seeded ground truth (entities planted by `fetch_corpus`):

| Corpus | n | P@5 | MRR | R@20 | Pack recall |
|---|---|---|---|---|---|
| cobra | 15 | .200 | .523 | .867 | .967 |
| httpx | 15 | .160 | .364 | .867 | .933 |
| clap | 15 | .169 | .259 | .789 | .782 |

Search — commit ground truth (co-change/temporal queries, one expected
entity each; harder set):

| Corpus | n | P@5 | MRR | R@20 |
|---|---|---|---|---|
| cobra | 60 | .093 | .279 | .472 |
| httpx | 60 | .080 | .252 | .332 |
| clap | 60 | .097 | .280 | .308 |
| cogz | 25 | .088 | .319 | .528 |

Channel ablations (commit GT) — what each channel is worth:

| Corpus | Hybrid MRR | FTS-only MRR | Hybrid R@20 | no-expand R@20 | FTS-only R@20 |
|---|---|---|---|---|---|
| cobra | .279 | .249 | .472 | .323 | .473 |
| httpx | .252 | .162 | .332 | .195 | .393 |
| clap | .280 | .173 | .308 | .281 | .342 |
| cogz | .319 | .297 | .528 | .418 | .499 |

- **Graph expansion is still the most valuable channel on commit
  queries** — cutting it costs 3–15pt R@20 depending on corpus
  (largest on cobra/httpx, least on clap where rebuild-state corpora
  carry fewer accumulated edges).
- **The vector channel earns its keep on commit-query MRR** (+2 to
  +11pt over FTS-only on all four corpora) but not on raw R@20 —
  under the 20-row cap, vector seeds dilute rather than expand, and
  FTS-only matches or beats hybrid R@20 on the public corpora (cobra .473
  vs .472, clap .342 vs .308, httpx .393 vs .332) and nearly ties on
  cogz (.499 vs .528). Expansion dilution is the top open ranking issue.
- On seeded GT the same pattern holds — seed titles are strong
  lexical anchors; FTS-only ties hybrid on clap/httpx and beats it on
  cobra (.933 vs .867).

Pack recall at token budget (fraction of expected entities present in
the pack at each budget — commit GT):

| Corpus | 8192 | 4096 | Δ |
|---|---|---|---|
| cobra | .891 | .856 | −3.5pt |
| httpx | .757 | .736 | −2.1pt |
| clap | .486 | .478 | −0.8pt |
| cogz | .738 | .347 | −39.1pt |

At 8k the budget is already binding (packs average ~8,100–8,200 tokens).
4k loses recall on every corpus — mildly on the three public corpora,
sharply on cogz (its sections are large).
**8192 stays the default.**

Silence gate — 15 adjacent-domain negative queries per corpus, scored
as "no direct result above floor" (`neg_ok` = clean rate):

| Corpus | neg_ok |
|---|---|
| httpx | 0.73 |
| cobra | 0.67 |
| clap | 0.47 |
| cogz | 0.47 |

The gate under-fires: vocabulary-adjacent queries (e.g. "grpc server
command scaffolding" against a CLI library) retrieve confidently. The
generic nonsense probe inside `pack_metrics` shows the same shape
(1/3 leak on public corpora, 3/3 on cogz).

Latency (hybrid search, p50 / mean; CPU-only box, contention-sensitive):

| Corpus | Hybrid p50 | FTS-only p50 |
|---|---|---|
| cobra | 315 ms | 130 ms |
| httpx | 342 ms | 66 ms |
| clap | 550 ms | 96 ms |
| cogz | 443 ms | 95 ms |

Indexing cost (`timing.json` per corpus — wall-clock seconds):

| Corpus | no-op reindex | +1 file | −1 file | full rebuild |
|---|---|---|---|---|
| cobra | 1.3 | 3.9 | 0.7 | 25.9 |
| httpx | 3.2 | 2.5 | 1.0 | 29.0 |
| clap | 2.3 | 3.2 | 2.4 | 27.7 |
| cogz | 14.4* | 10.3 | 9.0 | skipped† |

\* the self-corpus carries ~2,500 entities — no-op reindex is dominated by
parsing/scanning live `.cogz` files, not churn (unchanged files are
hash-skipped). † full rebuild would erase live dogfood
telemetry (`events`, `entity_usage`), so `run_suite` skips it on
`local` corpora.

Memory (per `pack_metrics` session — RSS of the `mcp-stdio` process,
uniform across corpora):

| State | Idle | After first search |
|---|---|---|
| hybrid | 8 MB | ~765 MB |
| fts_only | 8 MB | ~20 MB |

Models cost ~745 MB resident — the FTS-only degradation path is the
low-resource answer.

Determinism: identical result lists across repeated runs on all corpora
(`determinism.txt` per corpus). Embedding batch-composition drift is
quantified in `_invariants/reindex_equiv.txt` as `EMBEDDING_DRIFT`
(currently n=10, cosine 0.967–0.985 — a known cosmetic nondeterminism).

## Methodology notes

- **Temporal isolation.** Commit queries carry their commit's timestamp;
  the co-change channel can only draw on strictly-earlier history.
- **Negatives are adjacent, not nonsense.** They name plausible-but-absent
  features, so a leaking result means the confidence gate could not tell
  the query was out-of-domain — a measured limitation, not a bad query.
- **Determinism runs the seeded set twice** and requires byte-identical
  result lists.
- **Agent replays** (`agent_replay.py`) export `git archive` of the parent
  tree into a fresh `git init` — no upstream history, so agents cannot
  read the fix commit. Replays are opt-in and not part of `run_suite.py`.
- FTS-only state is produced by config probing (`auto_download=false`,
  probe model names) — `pack_metrics` restores `config.toml` afterward.
