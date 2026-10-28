---
id: 34a9b2ce-93b0-464d-8b26-43fdde9de2db
title: Module map — src/ tree layout and module responsibilities
type: knowledge
status: active
created_at: "2026-10-28T10:20:46Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd"]
category: architecture
tags: ["architecture", "module-map", "orientation", "src-layout"]
verified_against: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd=3145d032b6d1c1836af620ad7d3c5b8d852477de0c76889f2cadf8c2423a502c"]
---

Condensed map of `src/` (canonical version in
`docs/design/architecture.md`; layering invariants in the
module-layering entity):

- **Roots:** `lib.rs` (library root), `main.rs`/`cli.rs` (clap entry
  + dispatch), `cli_embed.rs`/`cli_events.rs` (embedding helpers,
  event/mining surface), `net.rs` (bounded-timeout HTTP),
  `seeds.rs` (init seed entries), `init.rs` (`cogz init`),
  `update.rs` (self-update from GitHub releases).
- **`config/`** — typed TOML config, startup validation.
- **`storage/`** — the only module that writes SQL. `schema.rs`
  migrations; `crud*.rs` entity ops; `edges.rs` graph edges;
  `embeddings.rs` vec0/KNN; `events.rs` domain log; `graph.rs` +
  `graph_queries.rs` traversal and callers/impact/orphans;
  `mining*.rs` write-path mining (orchestration + five signal
  passes); `query.rs`, `status.rs` (state machine), `access.rs`,
  `usage.rs` (delivery tiers/hit rates).
- **`files/`** — canonical file I/O and file→DB sync:
  `frontmatter.rs` (YAML subset parser), `entities.rs`
  (EntityFile read/write), `sync.rs`/`sync_ops.rs`,
  `embed_sync.rs` (bridges files→embeddings without holding the DB
  lock during inference), `events.rs`, `refs.rs` (frontmatter edge
  sync).
- **`embed/`** — model inference: `onnx.rs` (embedding models),
  `nli.rs` (contradiction), `inference.rs`/`pooling.rs` (batched
  tokenization, mean pooling), `runtime.rs`/`download.rs`/
  `registry.rs`/`resources.rs`/`checksum.rs` (ORT init, hf-hub
  fetch, model IDs, RAM checks, lib verification), `cache.rs`,
  `model.rs` (EmbeddingModel/NliModel traits), `suppress.rs`,
  `similarity.rs`.
- **`search/`** — query orchestration: `hybrid.rs` orchestrator +
  `hybrid_helpers.rs`, `graph_retrieval.rs` (graph-first channel),
  `prf.rs`, `rrf.rs`, `rank.rs` (floor/provenance/MMR),
  `expand.rs` (post-merge BFS), `balance.rs`, `scoring.rs`,
  `describe.rs` (graph path text). Never loads models.
- **`context/`** — pack assembly: `assemble.rs` (tiered-push
  Tier 0/1/2), `baseline.rs` (Tier-0 orientation),
  `query_sections.rs`, `modes.rs` (cold_start/task/escalation),
  `compress.rs` (chars/4 token estimation, budget fitting),
  `code_map.rs`.
- **`consolidate/`** — `dedup.rs`, `contradict.rs`, `promote.rs`,
  `merge.rs` (see consolidation entity for semantics).
- **`index/`** — code indexing: `mod.rs` orchestrates
  index/reindex/single-file; `parse.rs` shared parsing;
  `detection.rs`/`git_diff.rs`/`baseline.rs` change detection;
  `reindex.rs` processing; `tree_sitter/` per-language extractors;
  `gitignore.rs` scanner; `code_graph/` + `sync/` entity+edge DB
  sync; `auto_link.rs` knowledge→code mentions;
  `stale_flagging.rs`; `drift.rs` facade + `refstate.rs`;
  `post_index.rs` (edge repair, backfill, heal); `verify.rs`;
  `reanchor.rs` (rename/move re-linking).
- **`mcp/`** — stateless MCP server: `server.rs` (repo/model
  caching), `repo_cache.rs` (herd protection/LRU/staleness),
  `tools.rs` router (15 tools) + `tools_*.rs` per group,
  `params.rs`/`responses.rs`, `entity_helpers.rs` (create with
  dedup/contradiction), `update_knowledge.rs`, `errors.rs`.
- **`hooks/`** — lifecycle: `lifecycle.rs` dispatch + delivery/hit
  tracking, `capture.rs`, `format.rs` (additionalContext JSON),
  `handlers.rs`, `nudge.rs` (write-back nudges), `reindex.rs`
  (debounced background spawn).
- **`doctor/`** — `checks.rs` + `checks_analysis.rs` health checks,
  `prune.rs` tombstoning.
- **`security/`** — secret pattern scanning (rejects on the write
  path and at file sync, since files arrive by hand/git too).
- **`configure/`** — agent MCP+hook config writer (claude-code,
  cursor, codex, gemini, copilot, devin), semantic merge + backups.
- **`commands/`** — CLI handlers: index/reindex/status/reset/
  consolidate/verify/reject, `doctor.rs`, `embed_bg.rs`,
  `models.rs`, `reindex_bg.rs`.

**Data flow in one line each:** write = MCP tool → scan → file →
DB → embed → dedup/contradiction → event (file-first). Read =
get_context → hybrid search → tiered assemble → budget fit. Index =
sync .cogz files → tree-sitter code + edges + auto-links → embed →
baseline. Reindex = git-diff since baseline → incremental code sync →
stale flagging → heal embeddings. Hook = capture-event → record →
debounced reindex-bg → optional context pack JSON.
