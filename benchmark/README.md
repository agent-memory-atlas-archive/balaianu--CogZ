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
| cobra | 15 | .200 | .531 | .967 | .967 |
| httpx | 15 | .173 | .358 | .917 | .900 |
| clap | 15 | .185 | .278 | .839 | .782 |

Search — commit ground truth (co-change/temporal queries, one expected
entity each; harder set):

| Corpus | n | P@5 | MRR | R@20 |
|---|---|---|---|---|
| cobra | 60 | .103 | .288 | .501 |
| httpx | 60 | .077 | .254 | .364 |
| clap | 60 | .090 | .242 | .390 |
| cogz | 25 | .104 | .352 | .560 |

Channel ablations (commit GT) — what each channel is worth:

| Corpus | Hybrid MRR | FTS-only MRR | Hybrid R@20 | no-expand R@20 | FTS-only R@20 |
|---|---|---|---|---|---|
| cobra | .288 | .254 | .501 | .345 | .484 |
| httpx | .254 | .162 | .364 | .211 | .433 |
| clap | .242 | .175 | .390 | .291 | .409 |
| cogz | .352 | .302 | .560 | .451 | .507 |

- **Graph expansion is the most valuable channel** — cutting it costs
  10–16pt R@20 on every corpus at flat MRR; it surfaces entities that
  direct ranking misses.
- **The vector channel earns its keep on commit queries** (MRR +3 to
  +9pt on httpx/clap/cogz) but not uniformly — on seeded GT, FTS-only
  sometimes *beats* hybrid MRR (cobra .79 vs .53, clap .59 vs .28):
  seed titles are strong lexical anchors, so semantic similarity adds
  noise rather than signal there.
- FTS-only degrades gracefully everywhere — never below ~75% of hybrid
  R@20 — which is what the degraded-mode promise needs.

Pack recall at token budget (fraction of expected entities present in
the pack at each budget — commit GT):

| Corpus | 8192 | 4096 | Δ |
|---|---|---|---|
| cobra | .896 | .868 | −2.8pt |
| httpx | .772 | .752 | −2.0pt |
| clap | .704 | .665 | −3.9pt |
| cogz | .755 | .510 | −24.5pt |

At 8k the budget is already binding (packs average 8,089–8,190 tokens).
4k loses recall on every corpus — mildly on the three public corpora,
sharply on cogz (its sections are large: sections/pack drop 85→39).
**8192 stays the default.**

Silence gate — 5 adjacent-domain negative queries per corpus, scored as
"no direct result above floor 0.05" (`neg_ok` = clean rate):

| Corpus | neg_ok |
|---|---|
| httpx | 0.4 |
| clap | 0.2 |
| cobra | 0.0 |
| cogz | 0.0 |

The gate under-fires: vocabulary-adjacent queries (e.g. "grpc server
command scaffolding" against a CLI library) retrieve confidently. The
generic nonsense probe inside `pack_metrics` shows the same shape
(1/3 leak on public corpora, 3/3 on cogz).

Latency (hybrid search, p50 / mean; CPU-only box, contention-sensitive):

| Corpus | Hybrid p50 | FTS-only p50 |
|---|---|---|
| cobra | 642 ms | 838 ms |
| httpx | 528 ms | 346 ms |
| clap | 715 ms | 395 ms |
| cogz | 617 ms | 361 ms |

Indexing cost (`timing.json` per corpus — wall-clock seconds):

| Corpus | no-op reindex | +1 file | −1 file | full rebuild |
|---|---|---|---|---|
| cobra | 1.0 | 2.2 | 0.6 | 23.9 |
| httpx | 2.7 | 5.0 | 1.9 | 58.6 |
| clap | 16.5 | 16.5 | 10.3 | 91.7 |
| cogz | 58.6* | 19.1 | 14.3 | skipped† |

\* the self-corpus reindex ran against ~900 uncommitted working-tree
changes (this very benchmark diff) — real-world dirty-tree cost, not a
clean checkout number. † full rebuild would erase live dogfood
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
