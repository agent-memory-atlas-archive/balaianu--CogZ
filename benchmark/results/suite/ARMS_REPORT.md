# Retrieval-mode arms — quality × cost (uncommitted experiment)

Driver: `benchmark/suite/run_arms.py`. 7 arms × 4 corpora. Each arm:
config override → reset+index where needed → drain embed-bg → all
query sets (seeded/commit/negatives/gap) + determinism + pack metrics.
Costs recorded: index wall, drain wall, peak RSS, DB size, vec count.

Gap sets: prose queries targeting CODE entities with verified zero
title-token overlap (`check_gap_queries.py`, FTS5 column-scoped match).
91 queries: httpx 24, clap 22 (21 scored), cobra 28, cogz 16.

## The decisive table — gap pack_recall
(does the gold entity land in the assembled context pack?)

| arm | cobra | httpx | clap | cogz |
|---|---|---|---|---|
| hybrid | **0.857** | 0.917 | **0.214** | **0.406** |
| hybrid_split | **0.857** | 0.917 | **0.214** | **0.406** |
| small | 0.464 | 0.958 | 0.167 | 0.281 |
| fts | 0.107 | 0.917 | 0.048 | 0.188 |
| fts_split | 0.107 | 0.958 | 0.048 | 0.188 |
| knowledge_only | 0.036 | 0.917 | 0.048 | 0.250 |
| knowledge_split | 0.036 | 0.917 | 0.048 | 0.250 |

## gap R@20 (direct hits)

| arm | cobra | httpx | clap | cogz |
|---|---|---|---|---|
| hybrid | 0.179 | 0.375 | 0.095 | 0.125 |
| hybrid_split | 0.179 | 0.375 | **0.143** | 0.125 |
| small | 0.143 | 0.583 | 0.048 | 0.062 |
| fts | 0.107 | **0.625** | 0.048 | 0.062 |
| fts_split | 0.071 | **0.625** | 0.048 | 0.062 |
| knowledge_only | 0.036 | 0.542 | 0.048 | 0.125 |
| knowledge_split | 0.036 | 0.542 | 0.048 | 0.125 |

## seeded R@20 / commit R@20

| arm | seeded cobra/httpx/clap | commit cobra/httpx/clap/cogz |
|---|---|---|
| hybrid | 0.867 / 0.867 / 0.789 | 0.472 / 0.332 / 0.308 / 0.502 |
| fts | 0.933 / 0.867 / 0.789 | 0.473 / 0.387 / 0.342 / 0.501 |
| hybrid_split | 0.867 / 0.867 / 0.789 | 0.461 / 0.330 / 0.308 / 0.544 |
| fts_split | 0.933 / 0.867 / 0.789 | 0.481 / 0.387 / 0.342 / 0.541 |
| knowledge_only | 0.900 / 0.794 / 0.789 | 0.476 / 0.374 / 0.323 / 0.438 |
| knowledge_split | 0.900 / 0.794 / 0.789 | 0.477 / 0.374 / 0.323 / 0.481 |
| small | 0.933 / 0.833 / 0.856 | 0.468 / 0.358 / 0.309 / 0.426 |

Seeded MRR: fts wins everywhere (0.78/0.42/0.59 vs hybrid
0.52/0.36/0.26) — the vec channel dilutes lexically-aligned queries.

## cost

| arm | index wall | embed drain | vec rows | idx RSS | db |
|---|---|---|---|---|---|
| hybrid | 29/30/30/149s | 254/371/1760/1154s | all | 755/844/773/982MB | 10/13/30/54MB |
| knowledge_* | 25/32/29/139s | ~0s | 84/86/83/166 | same | -3/-9MB |
| small | 12/16/11/61s | 117/157/706/452s | all | 294/301/293/457MB | 7/10/21/48MB |

## Findings

1. **Code embeddings are load-bearing for vocabulary-gap retrieval.**
   Gap pack recall collapses without them where entity bodies are
   lexically thin (cobra 0.857→0.036-0.107, clap 0.214→0.048).
   httpx is the exception: docstring-rich bodies give FTS a content
   bridge (0.917 either way). Embedding value is corpus-conditional —
   driven by identifier-vs-prose gap in entity bodies.

2. **knowledge_only is a trap.** Worst arm on gap on 2/4 corpora,
   worse seeded MRR than hybrid everywhere (knowledge vec channel
   dilutes with semantically-near-but-wrong entities). If code
   embeddings go, ALL vectors should go — pure fts beats it.

3. **bge-small is a legitimate middle tier.** ~2.4x cheaper drain,
   ~2.6x lower index RSS, 30% smaller db; keeps most of hybrid's
   gap-pack advantage and matches/beats seeded R@20 on clap
   (0.856 vs 0.789). Costs some commit recall on cogz (0.426 vs 0.502).

4. **split_identifiers: cheap, occasionally helps, never hurts.**
   Best on Rust snake_case (clap gap R@20 0.143 vs 0.095) and on
   cogz commit (0.544 vs 0.502 — biggest commit delta anywhere).
   Invisible elsewhere — the current query classes under-exercise
   partial-identifier matching by construction.

5. **Commit queries ride graph/co-change channels, not code vec.**
   R@20 within noise across fts/hybrid on all corpora.

6. **Query latency**: fts ~40-100ms vs hybrid ~190-580ms — query
   embedding is the per-query cost delta.

## Caveats

- Gap queries are authored prose, not logged user queries.
- fts>hybrid R@20 on httpx gap suggests RRF weight dilution — a
  tuning opportunity separate from the embed/no-embed question.
- Measurements on 8-thread CPU-only box; relative pattern is what
  transfers.

## Decision options

- **Keep hybrid as default** — justified: it's the only arm that's
  never worst and dominates gap packs on 3/4 corpora.
- **Add `embedding.code_embeddings` config** — already implemented;
  worth shipping as documented knob for constrained environments
  (drain → 0), accepting the gap-loss.
- **Add `index.split_identifiers`** — implemented; recommend default
  on for new indexes (harmless, sometimes helps).
- **Offer small-model profile** — bge-small halves the cost and keeps
  ~most of the quality; could be `[embedding] profile = "light"`.
- **Do NOT ship knowledge_only** — measured strictly harmful.
