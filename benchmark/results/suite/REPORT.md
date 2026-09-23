# CogZ benchmark suite — external corpus results

Date: 2026-09-23. Binary: `cogz 0.3.0` (target/release @ HEAD incl.
diversity-slot fix 29e21fa + window quota 1ff25d9). Machine: 8-core,
7.3GB RAM, ONNX local inference. All latencies measured under varying
background load — treat as upper bounds, not clean numbers.

## Corpora (pinned, reproducible via fetch_corpus.py)

| corpus | lang | pinned sha | entities | knowledge seeded |
|---|---|---|---|---|
| httpx | Python | 8ab46fe2-era | 1051 | ~85 |
| cobra | Go | (suite corpora.toml) | 768 | ~84 |
| clap | Rust | (suite corpora.toml) | 4664 | ~85 |

Seeds per corpus: ~15 knowledge, ~10 rules, ~49 observations mined
from real git history, 5 stale, 2 superseded pairs, 2 duplicate
pairs, 2 rejected observations — a simulated long-term `.cogz`.

## Seeded knowledge retrieval (15 queries/corpus)

| corpus | P@5 | MRR | R@20 | pack recall |
|---|---|---|---|---|
| cobra | 0.187 | 0.502 | **0.967** | 0.967 |
| httpx | 0.173 | 0.358 | **0.917** | 0.933 |
| clap | 0.169 | 0.264 | **0.772** | 0.782 |

- R@20 stays high on unseen external code — the window quota
  generalizes; not tuned to these corpora.
- P@5 flat at ~0.17–0.19 across all sizes. The quota places rescued
  knowledge in the tail of the window (by design); top-5 placement is
  a different mechanism and remains unaddressed.
- Scale gradient confirmed on external code: R@20 and pack recall
  both degrade monotonically with corpus size (768 → 4664 entities).
  The ~4–5k range is where seeded-knowledge delivery starts fraying.

## Commit-GT retrieval (60 queries/corpus)

| corpus | P@5 | MRR | R@20 |
|---|---|---|---|
| cobra | 0.103 | 0.288 | 0.398 |
| httpx | 0.077 | 0.254 | 0.278 |
| clap | 0.090 | 0.242 | 0.335 |

Consistent with the original campaign: commit prose → code entity is
the weakest surface (vocabulary gap; ranking can't rescue true misses).

## Negatives (5 queries/corpus)

| corpus | neg_ok |
|---|---|
| cobra | 0.4 |
| httpx | 0.4 |
| clap | 0.6 |

Pattern replicated: absurd queries gate to silence; plausible-but-
absent in-domain features (grpc scaffolding, websockets, yaml flags)
return full result sets. Additionally observed: httpx's nonsense query
"quantum entanglement routing protocol" returned 32 entities all at
score 0.000 — common-token overlap ("protocol") produced FTS hits the
gate doesn't suppress. Zero-score rows in output is arguably a bug or
at minimum a reporting wart worth a backlog item.

## Determinism & invariants

- clap seeded run twice back-to-back: byte-identical result sets
  (DETERMINISTIC).
- reindex ≡ rebuild: EQUIVALENT (14 entities/16 edges; embedding
  cosine min 0.9669 — batch-composition noise, known item 57).
- tombstones: OK.
- drift precision: 1.000 P / 1.000 R.
- consolidation suite: pass.

## Version delta (local corpora re-run under current binary)

| corpus | seeded R@20 before | after |
|---|---|---|
| kaos-website | (pre-slot baseline) | **1.000** (pack 1.0) |
| api_tool | 0.7 | 0.8 (quota run) |
| CogZ-py | 0.6 | 0.77 (quota run) |
| CogZ self | 0.681 | 0.723 (quota run) |

## Agent replay (fail-to-pass on real fix commits)

Bare arm (`devin -p`, no cogz): worktree at parent sha + test-file
diff applied; agent must make new tests pass.

| corpus | tasks | bare pass |
|---|---|---|
| cobra | 4 | 4/4 |
| httpx | 4 | 4/4 (incl. straggler) |
| clap | 4 | 2/4 |

clap failures are real: both are subtle `parser.rs` fixes where a
bare agent produced an insufficient patch. First clap attempt was
polluted by /tmp tmpfs quota — fixed by moving worktrees to real disk
and a shared CARGO_TARGET_DIR.

cogz arm: pending (running).

## Environment notes

- /tmp is a 3.7GB tmpfs; Rust target dirs must live elsewhere.
- `devin -p` in print mode needs `--permission-mode dangerous` to
  allow edits+test runs in throwaway worktrees, and
  `--respect-workspace-trust false` for untrusted dirs.
- The embedded-model latency column is load-sensitive; clap's cleaner
  numbers (p50 ~800ms on 4.6k entities) vs httpx/cobra (~1400ms)
  reflect contention during their runs, not corpus size.
