# Architecture

CogZ is a local-first, code-aware engineering cognition runtime. This document describes the system architecture, module responsibilities, and data flow.

## Design principles

1. **Local-first, always.** No cloud services, no remote APIs for core functionality, no telemetry. The only network access is optional model downloads. Every feature must work offline after initial setup.

2. **Files are canonical, DB is derived.** Every entity is a Markdown file. The SQLite database is a derived index — disposable and fully rebuildable. `cogz reset` + `cogz index` reconstructs everything from files and source code.

3. **Memory is the primary constraint.** Every component must justify its memory footprint. Model inference is isolated so it can be loaded, unloaded, and swapped without affecting the rest of the system.

4. **Graceful degradation.** The system must function without ONNX models. FTS-only mode is always available — hooks, context packs, consolidation (title-based dedup), doctor, and prune all work without models.

5. **Agent- and model-agnostic.** CogZ does not tie to any specific agent or model. The MCP interface is stateless per the 2026-07-28 MCP spec (SEP-2577) — no Roots, no sessions. Every tool call specifies which repo it targets via an explicit `repo` parameter. Models are configurable and swappable.

## Tech stack

| Component | Choice | Rationale |
|---|---|---|
| Language | Rust (edition 2024) | ~28 MB binary, ~11 MB idle RAM. Real threads. Single binary deployment. |
| Storage | SQLite (WAL mode) | Local-first, embedded, no server. Per-repo, not global. |
| Vector search | sqlite-vec | Same SQLite database, no separate vector store. |
| Full-text search | SQLite FTS5 | Same database. Porter + unicode61 tokenizer. |
| Code parsing | tree-sitter | Multi-language AST extraction. |
| Embedding inference | ort (ONNX Runtime) | Rust-native inference, dynamic loading. |
| MCP server | rmcp | Native MCP protocol implementation. |
| CLI | clap | Standard Rust CLI framework. |
| Config | serde + TOML | Typed, validated at startup. |

## Module map

```
src/
  lib.rs               Library root
  main.rs              CLI entry point (clap)
  cli.rs               CLI command dispatch
  cli_embed.rs         CLI embedding helpers
  net.rs               HTTP agents with bounded timeouts
  security/            Secret pattern scanning
  commands/            CLI command implementations
    mod.rs             index, reindex, status, reset, consolidate
    doctor.rs          doctor health check
    embed_bg.rs        background embedding process
    models.rs          models download/list/clean
    reindex_bg.rs      background reindex process
  config/              Typed config (TOML), validation
  storage/             SQLite layer
    schema.rs          Migrations, schema version
    crud.rs            Entity CRUD, EntityType enum
    crud_batch.rs      Batched entity ops (stale marks, tombstones, cascade deletes)
    edges.rs           Edge CRUD (graph relationships)
    embeddings.rs      vec0 embedding storage and KNN search
    events.rs          Domain event recording
    graph.rs            Graph traversal (BFS expansion)
    graph_queries.rs   Targeted graph queries (callers, impact, orphans)
    mining.rs          Write-path mining (observation candidates)
    query.rs           Entity queries (by type, by reference)
    status.rs          Status state machine
    access.rs          Entity access tracking
    usage.rs           Delivery + entity-usage tracking (tiers, hit rates)
  files/               File I/O and file→DB sync
    frontmatter.rs     Minimal YAML frontmatter parser
    entities.rs        EntityFile struct, read/write
    sync.rs            File→DB synchronization
    sync_ops.rs        Sync operations (create/update/stale)
    embed_sync.rs      Embedding computation and storage
    events.rs          Event recording for sync operations
    refs.rs            File-backed edge sync from frontmatter
  embed/               ONNX model loading and inference
    onnx.rs            Embedding model (CodeRankEmbed, bge-base)
    nli.rs             NLI model (contradiction detection)
    inference.rs       Batched tokenization and embedding extraction
    pooling.rs         Mean pooling over token embeddings
    model_type.rs      Code vs knowledge model kind
    runtime.rs         ONNX Runtime initialization
    download.rs        Model download via hf-hub
    registry.rs        Model ID → HF source mapping
    resources.rs       Resource checks for load decisions
    checksum.rs        ORT lib path + checksum verification
    cache.rs           Content-hash embedding cache
    model.rs           Traits: EmbeddingModel, NliModel
    suppress.rs        Stderr suppression during ONNX init
    similarity.rs      Cosine similarity, L2→cosine conversion
  search/              Hybrid search
    hybrid.rs          FTS + vector + graph fusion orchestrator
    hybrid_helpers.rs  Helpers extracted for the 400-line limit
    graph_retrieval.rs Graph-first channel: seeds → traversal → candidates
    prf.rs             Pseudo-relevance feedback (second FTS pass)
    rrf.rs             Reciprocal Rank Fusion
    rank.rs            Post-merge ranking: floor, provenance, MMR
    expand.rs          Post-merge graph expansion (BFS)
    balance.rs         Source-type balancing (experimental)
    scoring.rs         Relevance scoring, recency decay
    describe.rs        Graph path description
  context/             Context pack assembly
    assemble.rs        Tiered-push pipeline (Tier 0/1/2 assembly)
    baseline.rs        Tier-0 orientation builders
    query_sections.rs  Search results → Tier-1 sections
    modes.rs           ContextMode enum (cold_start, task, escalation)
    compress.rs        Token estimation, dedup, budget fitting
    code_map.rs        Cold-start code map generation
  consolidate/         Memory consolidation
    dedup.rs           Duplicate detection (title + embedding)
    contradict.rs      NLI-based contradiction detection
    promote.rs         Observation → rule promotion
    merge.rs           Duplicate merge with edge redirection
  index/               Code indexing
    mod.rs             Orchestration: index_code, reindex_code, reindex_single_file
    parse.rs           Source file parsing (shared by full scan and incremental)
    detection.rs       Unified change detection (wraps git_diff)
    baseline.rs        Baseline commit read/write/should-update
    reindex.rs         Processing layer (takes changed files, processes them)
    tree_sitter/       AST extraction, per-language (rust, python, go, js/ts/tsx, bash)
    gitignore.rs       Gitignore-aware source file scanner
    git_diff.rs        Git diff-based change detection
    code_graph/        Structural edge sync (calls, imports, extends, contains)
    sync/              Code entity DB sync
    auto_link.rs       Knowledge → code auto-linking
    stale_flagging.rs  Stale knowledge flagging on code changes
  mcp/                 MCP server
    server.rs          ServerHandler impl, repo/model caching
    repo_cache.rs      Repo cache: herd protection, LRU, staleness
    tools.rs           Tool router (17 tools)
    tools_write.rs     record_observation, create_rule, create_knowledge, update_knowledge
    tools_query.rs     query_*, list_entities
    tools_search.rs    search, get_context
    tools_graph.rs     get_callers, get_impact, find_orphans
    tools_mining.rs    suggest_observations
    tools_system.rs    get_status, consolidate, capture_event
    params.rs          Tool parameter structs
    responses.rs       Response JSON builders
    status.rs          get_status response builder
    helpers.rs         Shared helpers (file writes, response builders)
    entity_helpers.rs  Entity creation with dedup/contradiction
    update_knowledge.rs update_knowledge helper (extracted)
    dedup.rs           Re-export of consolidate/dedup
    errors.rs          Structured MCP error helpers
  hooks/               Lifecycle event handlers
    lifecycle.rs       Event dispatch, context packs, delivery/hit tracking
    capture.rs         CLI capture-event handler
    handlers.rs        Event-specific handlers (file_save, session_end)
    reindex.rs         Background reindex spawn + debounce
  doctor/              Health checks
    checks.rs          Doctor report, all check implementations
    checks_analysis.rs Extracted analysis helpers
    prune.rs           Observation pruning with tombstones
  init.rs              cogz init
  update.rs            Self-update from GitHub releases
```

## Data flow

### Write path (MCP tool → file → DB)

```
Agent calls record_observation MCP tool
  → entity_helpers.rs: create_entity_file()
    → scan for secrets (reject if found)
    → write_entity_file_atomic() — writes Markdown file to .cogz/
    → sync_single_file() — syncs file to DB
    → embed_entity_text() — computes embedding (if model available)
    → store_embeddings() — stores embedding in vec0 table
    → check_duplicate() — title + embedding dedup
    → check_contradiction() — NLI contradiction detection
    → record_event() — domain event
  → return JSON response
```

The file is written first. If the file write fails, the DB is not updated. This is the file-first invariant.

### Read path (search → context pack)

```
Agent calls get_context MCP tool
  → assemble_context()
    → search::search(silence_gate: false) — hybrid FTS + vector with RRF
      fusion; per-result relevance floor applies, wholesale silencing does not
      → FTS5 query (always available)
      → KNN vector query (if embeddings available)
      → RRF fusion of FTS + vector results
      → Graph expansion (BFS from matched entities)
    → sort_by_priority() — rules > observations > knowledge > code
    → partition_dups() — demote excerpts subsumed by a kept section
    → compress_tail() — minimal excerpts for weak-evidence tail
    → fit_budget() — truncate/drop sections to fit token budget
    → relax_code_sections() — regrow code excerpts into headroom
    → overflow index — dropped entities listed as title/id pointers
  → return ContextPack JSON
```

### Index path (cogz index)

```
cogz index
  → files::sync_all() — sync .cogz/ files to DB
    → scan_entity_files() — walk .cogz/knowledge/, rules/, observations/
    → for each file: read, parse frontmatter, compute hash, sync to DB
    → mark stale: DB entities whose file was deleted
  → index::index_code() — index source code
    → parse::parse_source_files() — read + tree-sitter extract (shared with reindex)
    → sync_code_entities() — insert/update/stale-mark in DB
    → sync_code_edges() — structural edges (calls, imports, extends, contains)
    → sync_auto_links() — knowledge → code auto-references
  → embed_synced() — embed all new/updated entities
  → baseline::update_baseline() — record last_indexed_commit for incremental reindex
```

### Reindex path (cogz reindex)

```
cogz reindex
  → files::sync_incremental() — sync changed .cogz/ files to DB
  → index::reindex_code()
    → baseline::read_baseline() — get last_indexed_commit
    → detection::detect_changed_files() — git diff since baseline
    → reindex::reindex_files() — process changed files
      → parse::parse_source_files() — read + tree-sitter extract
      → sync_code_entities_incremental() — insert/update/stale-mark
      → sync_code_edges_incremental() — structural edges for changed files
      → sync_auto_links() — full rebuild of auto-link edges
      → mark_stale_for_deleted_files() — stale-mark removed files
    → baseline::update_baseline() — advance if no read failures
  → flag_stale_knowledge() — mark knowledge referencing changed code
  → embed_synced() — embed all new/updated entities
```

### Background reindex path (cogz reindex-bg)

```
cogz reindex-bg --repo <repo> --db <db>
  → files::sync_incremental() — sync changed .cogz/ files
  → index::reindex_code() — git-diff code reindex (same as cogz reindex)
  → flag_stale_knowledge() — mark stale knowledge for changed code
  → spawn_code_embed_background() — defer embedding to embed-bg
  → exit (detached process, no output to parent)
```

### Hook path (lifecycle event)

```
Agent fires session_start hook
  → cogz capture-event session_start --hook-json --fts-only
    → parse event type
    → handle_lifecycle_event()
      → record_event() — audit trail
      → spawn_reindex_bg() — detached background process (sync .cogz/ files, git-diff code reindex, stale flagging, embed-bg)
      → assemble_pack() — cold_start context pack
        → embed query (skipped with --fts-only)
        → assemble_context() — FTS-only retrieval
      → return context pack
    → print JSON to stdout: {"hookSpecificOutput": {...}}
```

`prompt_submit` follows the same path but with `ContextMode::Task` and a debounced background reindex (60s window). `file_save` skips the context pack and background reindex — it calls `reindex_single_file` (fast, no git diff) + `flag_stale_knowledge` for source files, or `sync_single_file` + embed for `.cogz/` files.

## Concurrency model

- **Single SQLite `Connection` behind `std::sync::Mutex`.** No connection pool, no `r2d2`, no `tokio-rusqlite`.
- **`PRAGMA busy_timeout = 5000`.** Background processes (reindex-bg, embed-bg) open their own connections. WAL mode allows concurrent readers but serializes writers — `busy_timeout` makes writer contention retry for 5 seconds instead of failing immediately.
- **`file_lock()` for multi-step canonical writes.** Merge, prune, and update_knowledge acquire an OS-backed exclusive lock on `.cogz/.lock` to serialize cross-process write sequences. Single-step writes (reindex, sync) rely on `busy_timeout` for contention.
- **DB calls from async MCP handlers go through `tokio::task::spawn_blocking`.** Never call rusqlite directly from an async context.
- **Don't hold the mutex during filesystem or network I/O.** Acquire the lock, get the data, drop the lock, then do the I/O.
- **Models are lazy-loaded and shared.** The MCP server caches model instances across repos. Models auto-unload after `model_idle_ttl` seconds of inactivity.

## Database

The database lives at `.cogz/cogz.db` (configurable via `storage.db_path`). It is gitignored and fully rebuildable.

**Tables:**
- `entities` — all entities (file-backed and code)
- `edges` — graph relationships
- `entities_fts` — FTS5 virtual table (synced via triggers)
- `code_embeddings` — vec0 virtual table for code entity embeddings
- `knowledge_embeddings` — vec0 virtual table for knowledge entity embeddings
- `events` — domain event log
- `meta` — key-value metadata (e.g. `last_indexed_commit`)
- `entity_access` — access tracking (derived)

See [Schema](../dev/schema.md) for the full schema and migration history.

## Entity types

| Type | Source | File-backed | UUID | Description |
|---|---|---|---|---|
| observation | Agent | yes | v4 | Raw, unvalidated experience |
| rule | Agent | yes | v4 | Validated directive |
| knowledge | Agent | yes | v4 | Structured documentation |
| function | tree-sitter | no | v5 | Code entity |
| class | tree-sitter | no | v5 | Code entity |
| file | tree-sitter | no | v5 | Code entity |
| module | tree-sitter | no | v5 | Code entity |

See [Entity Model](entity-model.md) for frontmatter schema, state machine, and update policy.

## Edge types

| Edge type | Source | Description |
|---|---|---|
| `references` | Frontmatter | Entity references another entity or file path |
| `auto_references` | Auto-linking | Knowledge → code auto-link (DB-only, rebuildable) |
| `supports` | Agent | Observation supports another observation (for promotion) |
| `derived_from` | Promotion | Rule derived from an observation |
| `superseded_by` | Merge | Superseded entity points to survivor |
| `contradicts` | NLI | Entity contradicts another entity |
| `calls` | tree-sitter | Function calls another function |
| `imports` | tree-sitter | Module/file imports another |
| `extends` | tree-sitter | Class extends another class |
| `contains` | tree-sitter | File/module contains a function/class/module |

## See also

- [Entity Model](entity-model.md) — entity types, frontmatter, state machine
- [Search](search.md) — hybrid FTS + vector, RRF, graph expansion
- [Consolidation](consolidation.md) — dedup, contradiction, promotion, merge
- [Degradation](degradation.md) — FTS-only mode and fallback behavior
- [Schema](../dev/schema.md) — DB schema and migrations
- [Conventions](../dev/conventions.md) — code patterns and invariants
