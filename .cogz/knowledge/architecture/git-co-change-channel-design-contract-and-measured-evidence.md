---
id: 48c9abeb-9bd1-4582-822f-5028f300006a
title: Git co-change channel — design contract and measured evidence
type: knowledge
status: active
created_at: "2026-09-29T00:38:50.802630434+00:00"
updated_at: "2026-09-29T00:38:50.802630434+00:00"
references: []
category: architecture
tags: ["cochange", "search", "benchmark", "schema-v7", "retrieval"]
---

# Git co-change retrieval channel

Shipped (uncommitted, schema v7): the `cochange` table mines `git log -p -U0` at index time — each commit's new-side hunk ranges intersected with current entity `line_start`/`line_end` ranges — producing (term, entity_id, commit_id, commit_ts) rows where terms are `prf::content_terms` of the commit subject. At query time `hybrid_helpers::cochange_candidates` scores entities by summed inverse term frequency (Σ 1/df — bundled SQLite has no `ln()`) and `emit_cochange_expansions` pushes them as `graph_path_description="co-change"` expansions with a [file→member] path.

## Measured (httpx/clap/cobra, 60 commit queries each, temporal honesty via `before_ts`)

- Commit R@20 +.084/+.051/+.103 vs baseline; 18 more queries reach ≥1 expected entity. Seeded/knowledge/pack metrics flat (code-surface channel only). Latency +30–130ms.
- Entity-grained mining was the unlock: file-grained co-change fan-out (all members of touched files) measured −.013 to −.108. IDF weighting + quota were both needed — inverse frequency alone still lost candidates to the 60-cap.
- **Expansion-only by measurement.** Two direct-channel designs (file-level RRF list, entity-level unquota'd) both cost R@20 — co-change leads are context, not retrieval. The quota is additive: `co` gets limit/4, `rest` keeps the full expansion cap.
- Quota partition keys on the `COCHANGE_DESC` const — never a bare string literal; the emit site and cap partition must agree.

## Design contract for future changes

- `SearchParams::before_ts` (CLI `--before`, MCP `before`) bounds the mined history — the benchmark passes each query commit's own timestamp so evaluation never reads the answer.
- `MAX_COMMITS=20000` bounds the walk; byte-split + lossy decode survives binary patches; `git log` spawn failure or non-git repo → silent 0 rows.
- Rebuilt wholesale every `cogz index`/`reindex` (single transaction: delete+lookup+insert atomic). `cogz reset`+`index` reproduced 8,225 rows on cobra exactly.
- Emission order matters: emits BEFORE generic expansion loops so its members aren't claimed-and-floored by graph traversal, and iterates a score+id-sorted vec — HashMap order made cobra rank-20 results nondeterministic until sorted.
- `graph_path` must be len ≥2 ([file_id, member_id]) — len-1 paths classify as `kind=direct` in the response and would masquerade as direct hits.

Do not promote single-channel co-change hits into directs to chase MRR — measured negative; flat P@5/MRR is the honest boundary.