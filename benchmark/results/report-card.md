# CogZ Version Report Card

Binary: `target/release/cogz` sha256 `1eb59536…` (post-determinism-fix HEAD, commits through `dd91fea`).
Host: 8-core CPU, 7.3GB RAM + 14.9GB swap. All embeddings CodeRankEmbed-int8 (code) + bge-base (knowledge), NLI nli-deberta-v3-xsmall.

## Executive summary

CogZ is **correct, deterministic, and cheap to operate** — the invariants hold up under adversarial fixtures, and consolidation is precise. Retrieval of *seeded knowledge* is excellent (R@20 0.95–1.0 on small/mid corpora) and packs reliably surface knowledge that top-5 search misses. Two honest weaknesses: (1) commit-message→entity retrieval is hard (R@20 ~0.4–0.57) — prose intent doesn't map cleanly to entity text; (2) at n=14 tasks, no agent arm beats bare on correctness within noise — the measured wins are speed (k arm ~2× faster) and completion reliability (fewer abandoned runs).

## Methodology & guardrails

- Pinned binary throughout; campaign manifest at `_campaign_manifest.json` (superseded hash noted).
- Determinism gate: same-DB double-run, diff modulo volatile fields. Search results byte-identical post-fix; packs verified identical under snapshot-restore protocol.
- Snapshot protocol (learned the hard way): **capture via `cp` is safe (last-checkpoint state); restore requires rename-swap + removing `-wal`/`-shm` — never `cp` over a live WAL database.** A plain `cp` restore on 2026-09-22 corrupted the dev DB; restored from intact snapshot.
- Resource cap honored: ≤2 concurrent `embed-bg`; corpus copies under `~/cogz_bench/corpora/`; originals untouched; no remote git ops.

## Corpus table

| corpus | files | LOC | entities | edges | embeddings | DB size |
|---|---|---|---|---|---|---|
| kinetik | 55 | 13.6k | 617 | 938 | 617 | 9.0MB |
| kaos-website | 37 | 4.1k | 155 | 174 | 155 | 7.5MB |
| api_tool | 235 | 75.7k | 2821 | 3771 | 2821 | 26.7MB |
| CogZ-py | 192 | 70.9k | 2775 | 3552 | 2775 | 27.0MB |
| CogZ | 184 | 48.0k | 2000 | 4251 | 2000 | 26.9MB |

Indexing is fast (2–5s foreground for code; ~1.9min for CogZ-py's 178 knowledge files + 2597 code entities at ~1.0GB peak RSS). Embedding is the bottleneck: ~800MB RSS per `embed-bg`, ~35–70 entities/min under two-way contention.

## Correctness invariants (P1)

| check | result |
|---|---|
| reindex vs reset+index | **converges** — identical active entities/edges/FTS; stale tombstones and stale embedding rows retained by reindex (documented asymmetry, benign) |
| drift marking | precision 1.0, recall 1.0 — changed refs flagged, unchanged entities in edited files not flagged, orphans→stale |
| pack determinism | 8/8 identical in-session; deterministic given fixed DB snapshot. **Stateful caveat:** `get_context` increments access counts → baseline rule ordering can shift across calls. Snapshot-restore between runs. |
| tombstone integrity | edges to pruned entities survive rebuild after `b2a66b3` fix; pruned excluded from search |

## Retrieval quality

### Commit-derived ground truth (commit message → touched entities; hard benchmark)

| corpus | n | P@5 | MRR | R@20 | pack recall |
|---|---|---|---|---|---|
| CogZ | 25 | .120 [.08,.17] | .374 | .529 | .677 |
| kinetik | 15 | .133 [.03,.27] | .169 | .433 | .774 |
| kaos-website | 7 | .143 [.06,.26] | .202 | .571 | — |
| api_tool | 60 | .087 [.05,.12] | .187 | .432 | .737 |
| CogZ-py | 60 | .080 [.05,.12] | .244 | .382 | .537 |

Consistent across corpora: ~40–57% of commit-touched entities land in top-20; few land in top-5. Commit prose is a hard query form (intent-level, not identifier-level).

### Seeded knowledge (queries targeting authored entities)

| corpus | entities | R@20 | pack recall | P@5 |
|---|---|---|---|---|
| kaos-website | 155 | 1.00 | 1.00 | .20 |
| kinetik | 617 | .95 | .90 | .12 |
| api_tool | 2821 | .60 | .90 | .06 |
| CogZ-py | 2775 | .60 | .633 | .053 |

Pattern: seeded-knowledge recall degrades with corpus size (1.0 → .95 → .60) as code entities compete for rank slots. **Packs are the rescue path**: pack recall (.63–1.0) consistently exceeds R@20 — context assembly surfaces knowledge that search ranking buries. P@5 is weak (.05–.20) — code entities outrank knowledge at the top; the strongest argument for rank-precision work or a knowledge-priority tier in top-k.

### Generic-memory comparison (Mnemos, same content)

Mnemos on the same seed facts (8 memories + 20 distractors): 7/8 rank-1, MRR≈.97 — essentially perfect on a small homogeneous store. This is **not** a like-for-like difficulty comparison (28 memories vs 2821 mixed entities), but it frames the differentiation question correctly: memory-only retrieval is solved at small scale; CogZ's harder problem is *unified* code+knowledge ranking.

## Context packs & budgets

Budget sweep (pack recall of expected entities):

| corpus | 8192 | 4096 | 2048 | 1024 |
|---|---|---|---|---|
| kinetik (15q) | .774 | .720 | .434 | .312 |
| api_tool (60q) | .737 | .667 | .509 | .373 |

- **Cliff at 2048** on both corpora — 8k→4k is gentle (−0.05 to −0.07), 4k→2k drops sharply.
- ~55–60% of sections get body-truncated even at 8k — truncation, not dropping, is the dominant fit mechanism; `exp_trunc` (expected entities truncated) is high at every budget.
- Pack composition: on CogZ (real knowledge) 15–25% of sections are knowledge/rule/observation; on knowledge-free corpora packs are ~95% code.
- No duplicate sections observed (`packs_w_dup=0` everywhere); overflow pointers emitted when budget binds.
- Pack latency excluded from search latency above — assembly is fast relative to search.

## Latency (clean machine, warm)

| corpus | hybrid p50 | hybrid p95 | fts p50 | cold-start (model load) |
|---|---|---|---|---|
| kaos (155) | 259ms | 279ms | 201ms | 1.70s |
| kinetik (617) | 267ms | 283ms | 149ms | 1.70s |
| CogZ (2000) | 518ms | 541ms | 283ms | 1.92s |
| CogZ-py (2775) | 376ms | 609ms | 322ms | 2.02s |
| api_tool (2821) | 374ms | 608ms | 370ms | 1.82s |

- Hybrid adds ~50–240ms over FTS (query embedding on CPU). Earlier "5.7× slower" numbers were embed-contention noise.
- Sub-second at all tested scales; p95 < 650ms hybrid.
- 3 concurrent mcp-stdio clients: ~1.6× per-query slowdown (CPU oversubscription on embed inference; each process has its own DB connection — no Mutex serialization between processes).
- Silence gate: gibberish queries → 0 hits. Mixed queries ("xyzzy plover unsupported nonsense terms") return hits on the real tokens ("terms") — arguably correct weak-signal behavior, not a gate defect.

## Consolidation quality (P5 suite + real corpus)

- **Dedup at insert**: P=1.0, R=1.0 — all 6 true dupes flagged (5 embedding ≥0.89, 1 exact-title), 0/9 false positives on near-dupes and distinct entities. Suite is small; directional.
- **Contradiction (NLI)**: 4/4 true contradictions caught (including the subtle FTS↔KNN ordering flip), 0/4 false alarms on compatible same-topic facts.
- **Promotion**: threshold exact (2 supporters → none; 3rd → promoted to rule with `derived_from`), idempotent on re-run, `references` correctly do not count as `supports`.
- **Real-world dedup**: CogZ-py carries 178 knowledge entities where ~91% are near-duplicates (14 identical "test title" artifacts etc.). `consolidate` merge reports 0 (merge is observation-scoped); `cogz doctor` catches all of them — 11,641 pairwise near-duplicate reports covering 162 entities. Coverage is complete but O(n²) pairwise reporting is unwieldy — cluster-level reporting would be a UX improvement.

## Agent replay (P7 — telegram-acp-bot, 14 tasks, existing campaign data)

| arm | n | exit0 | clean tasks | ~secs | ~steps | ~prompt-tokens |
|---|---|---|---|---|---|---|
| bare | 14 | 10 | 5 (36%) | 1748 | 70 | 4.6M |
| cogz | 14 | 13 | 4 (29%) | 1301 | 68 | 4.8M |
| hyb | 14 | 14 | 3 (21%) | 1223 | 57 | 3.5M |
| thin | 14 | 12 | 4 (29%) | 1429 | 63 | 4.0M |
| fix | 14 | 13 | 5 (36%) | 1378 | 83 | 5.5M |
| k (seeded) | 14 | 14 | 6 (43%) | **871** | 82 | 5.0M |
| k2 | 14 | 13 | 4 (29%) | 998 | 76 | 4.8M |
| new | 14 | 13 | 6 (43%) | 1272 | 85 | 5.9M |

- **Correctness is a wash** at n=14: bare 36% vs CogZ arms 21–43%. Wilson CIs overlap ~±15–20pp; no arm beats bare significantly.
- **Speed is a real win**: seeded-knowledge arm finishes ~2× faster (871s vs 1748s) with equal-or-better correctness. Seeded knowledge also tripled pull adoption (5.2 vs 1.6 pull calls/run).
- **Completion reliability**: CogZ arms abandon less (13–14/14 exit-0 vs 10/14 bare) — though several arms were hurt by the stale-binary schema mismatch (fixed since).
- **Anchoring is weak**: 56% of k-arm touched files were knowledge-referenced vs 48% for bare — +8pp lift. Seeded refs point at the same important files agents find anyway.
- Generic-memory *agent* arm not run (would need mnemos-in-sandbox plumbing + ~14 more real agent sessions; retrieval comparison above is the partial substitute).

## Findings & fixes shipped this campaign

- `889edec` deterministic tie-breaks across all ranking paths (HashMap→Vec→score-only sorts in hybrid/graph/rrf/expand/prf — the real nondeterminism root cause)
- `2936aae` section `tier` preserved in run serialization; determinism checker scopes correctly
- `b2a66b3` tombstone reference-edge repair on rebuild
- Harness: `--pack-tokens`, bootstrap CIs, latency percentiles, corpus metrics, commit-GT generator, pack-metrics probe, consolidation suite, drift/tombstone/reindex fixtures

## Weaknesses (honest)

1. **Top-k knowledge precision**: P@5 ≤ .20 even for queries whose answer is a seeded knowledge entity — code drowns knowledge at the top of mixed rankings.
2. **Commit-message retrieval**: R@20 ~0.4–0.57 — prose-intent queries underperform; acceptable but limits "why did this change" use cases.
3. **Embedding reproducibility**: same text embeds differently across batch compositions (cosine ~0.97) — deterministic per batch, but KNN-boundary jitter across index invocations is real.
4. **Outcome parity at n=14**: CogZ doesn't demonstrably improve task correctness yet — it buys speed and reliability. If the goal is outcome lift, the lever is probably seeded knowledge quality + pack content, not retrieval plumbing.
5. **Access-count feedback in packs**: `get_context` mutates state that reorders later baseline sections — fine by design, but consumers replaying packs should know.
6. **Doctor dup reporting is O(n²)-verbose**; merge covers observations only.
7. **Operational hazard**: WAL-file snapshot restore via raw `cp` corrupts the DB — needs docs/tooling (or a `cogz snapshot` command).
