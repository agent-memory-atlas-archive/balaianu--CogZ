# Database Schema

CogZ uses SQLite with WAL mode. The schema is versioned via `PRAGMA user_version`. Migrations are forward-only and idempotent.

**Current schema version:** 6

## Tables

### `entities`

Primary entity storage. All entity types (file-backed and code) share this table.

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PRIMARY KEY | UUID string |
| `type` | TEXT NOT NULL | `observation`, `rule`, `knowledge`, `function`, `class`, `file`, `module` |
| `title` | TEXT | Nullable for code entities |
| `content` | TEXT NOT NULL | Markdown body |
| `properties` | TEXT DEFAULT '{}' | JSON: type-specific fields |
| `file_path` | TEXT | Relative path; null for code entities without files |
| `status` | TEXT DEFAULT 'active' | See entity model state machine |
| `content_hash` | TEXT | SHA-256 of content for change detection |
| `created_at` | TEXT NOT NULL | RFC 3339 timestamp |
| `updated_at` | TEXT NOT NULL | RFC 3339 timestamp |

**Indexes:** `idx_entities_type`, `idx_entities_status`, `idx_entities_file_path`.

### `entities_fts`

FTS5 virtual table (external content table pointing to `entities`).

```sql
CREATE VIRTUAL TABLE entities_fts USING fts5(
    title,
    content,
    content='entities',
    content_rowid='rowid',
    tokenize='porter unicode61'
);
```

Synced via triggers:
- `entities_fts_ai` — AFTER INSERT
- `entities_fts_ad` — AFTER DELETE
- `entities_fts_au` — AFTER UPDATE (delete + insert)

### `edges`

Graph relationships between entities.

| Column | Type | Notes |
|---|---|---|
| `source_id` | TEXT NOT NULL | FK to `entities(id)` |
| `target_id` | TEXT NOT NULL | FK to `entities(id)` |
| `edge_type` | TEXT NOT NULL | See edge types in architecture doc |
| `weight` | REAL DEFAULT 1.0 | Edge weight |
| `created_at` | TEXT NOT NULL | RFC 3339 timestamp |

**Primary key:** `(source_id, target_id, edge_type)` — duplicate edges upsert (weight updated, `created_at` preserved).

**Indexes:** `idx_edges_source`, `idx_edges_target`.

### `code_embeddings`

sqlite-vec virtual table for code entity embeddings.

```sql
CREATE VIRTUAL TABLE code_embeddings USING vec0(
    embedding FLOAT[768],
    entity_id TEXT
);
```

Dimension matches `embedding.dimension` from config. Code and knowledge use separate tables because they use different embedding models with incompatible vector spaces.

### `knowledge_embeddings`

sqlite-vec virtual table for knowledge entity embeddings. Same structure as `code_embeddings`.

### `events`

Domain event log. Only domain events are recorded — never indexing operations. This keeps the table small and meaningful.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | Auto-increment |
| `event_type` | TEXT NOT NULL | See event types below |
| `entity_id` | TEXT | FK to `entities(id)`, nullable |
| `payload` | TEXT DEFAULT '{}' | JSON payload |
| `created_at` | TEXT NOT NULL | RFC 3339 timestamp |

**Indexes:** `idx_events_type`, `idx_events_entity`.

**Event types:** `observation_created`, `observation_edited`, `observation_rejected`, `rule_created`, `rule_edited`, `rule_promoted`, `knowledge_created`, `knowledge_updated`, `knowledge_merged`, `contradiction_found`, `code_changed`, `session_start`, `prompt_submit`, `pre_tool_use`, `post_tool_use`, `file_save`, `session_end`, `stop`.

### `meta`

Key-value metadata store.

| Column | Type | Notes |
|---|---|---|
| `key` | TEXT PRIMARY KEY | e.g. `last_indexed_commit` |
| `value` | TEXT NOT NULL | e.g. git SHA |

### `entity_access`

Access tracking (derived, not in canonical files).

| Column | Type | Notes |
|---|---|---|
| `entity_id` | TEXT PRIMARY KEY | FK to `entities(id)` |
| `access_count` | INTEGER DEFAULT 0 | Number of times accessed |
| `last_accessed` | TEXT | RFC 3339 timestamp |

### `deliveries`

Usage instrumentation — one row per context delivery (pack or search result). Derived state, disposable.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | Auto-increment |
| `kind` | TEXT NOT NULL | `pack`, `search`, or `pull` |
| `event_id` | INTEGER | FK to `events(id)`, nullable |
| `closed` | INTEGER DEFAULT 0 | 1 once a new delivery boundary or session_end closes it |
| `created_at` | TEXT NOT NULL | RFC 3339 timestamp |

### `entity_usage`

One row per entity per delivery. `outcome` starts `pending`, becomes `hit` when a post_tool_use touches the entity, or `miss` when the delivery closes without a touch.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | Auto-increment |
| `delivery_id` | INTEGER NOT NULL | FK to `deliveries(id)` |
| `entity_id` | TEXT NOT NULL | FK to `entities(id)` |
| `outcome` | TEXT DEFAULT 'pending' | `pending`, `hit`, `miss` |
| `tier` | TEXT DEFAULT 'full' | `baseline`, `full`, `pointer` — which push tier delivered it |
| `created_at` | TEXT NOT NULL | RFC 3339 timestamp |

**Indexes:** `idx_usage_unique` (delivery_id, entity_id), `idx_usage_delivery`, `idx_usage_entity`, `idx_usage_outcome`.

### `entity_drift`

One row per knowledge reference whose recorded `verified_against` provenance diverges from the referenced entity's current state. Derived state — fully rebuilt by `recompute()` on every index path; never written to canonical files.

| Column | Type | Notes |
|---|---|---|
| `entity_id` | TEXT NOT NULL | FK to `entities(id)` — the knowledge entity |
| `code_id` | TEXT NOT NULL | Referenced entity that diverged |
| `verified_hash` | TEXT | Hash recorded in `verified_against` (NULL when never stamped) |
| `current_hash` | TEXT | Referenced entity's current content hash (NULL when absent) |
| `cause` | TEXT NOT NULL | `changed`, `missing`, `unverified` |

Primary key `(entity_id, code_id)`. **Index:** `idx_drift_entity` on `entity_id` — powers `drift_counts()` batch lookups during search.

## Migrations

Migrations are forward-only and idempotent. On a fresh database, all migrations run in order. On an existing database, only new migrations run.

### v1 — Initial schema

Creates all tables, indexes, FTS5 virtual table, FTS5 sync triggers, and vec0 embedding tables. The `embedding_dim` parameter sets the vec0 dimension.

### v2 — Meta table

Adds the `meta` table for key-value metadata (e.g. `last_indexed_commit` for incremental reindex).

### v3 — Split embedding tables

Splits the old single `entity_embeddings` table into `code_embeddings` and `knowledge_embeddings`. Existing embeddings are migrated to the correct table based on entity type, then the old table is dropped.

On fresh databases (v1 already creates the two new tables), this migration is a no-op.

### v4 — Usage instrumentation

Adds `deliveries` (one row per context-pack or search-result delivery, with `closed` lifecycle) and `entity_usage` (one row per delivered entity: `pending` → `hit` | `miss`). Both tables are disposable derived state — they track whether delivered context was used, never canonical content.

### v5 — Delivery tiers

Adds `entity_usage.tier` (`baseline` | `full` | `pointer`, default `full`) recording which push tier delivered each entity — the measurement delivery-layer instrumentation reads. `deliveries.kind` also accepts `pull` (targeted graph-tool results). The migration guards the `ALTER TABLE` with a column-existence check — SQLite has no `ADD COLUMN IF NOT EXISTS`.

### v6 — Knowledge drift

Adds `entity_drift` — per-reference divergence between a knowledge entity's `verified_against` provenance (canonical frontmatter) and the referenced entity's current content hash. Recomputed by the post-index maintenance pass (`flag` → `backfill` → `recompute` → `heal`); `verified_against` itself lives in files, so the table is fully disposable.

## Version checking

`check_version()` verifies the DB schema version matches `SCHEMA_VERSION`. A mismatch returns `SchemaVersionMismatch { db, expected }`. This happens when a newer binary opens an older DB (or vice versa). The fix is `cogz reset` + `cogz index` to rebuild from files.

## Rebuildability

The DB is disposable. `cogz reset` deletes it; `cogz index` rebuilds it from:
- `.cogz/knowledge/`, `.cogz/rules/`, `.cogz/observations/` (file-backed entities)
- Source code (code entities via tree-sitter)
- Frontmatter `references` fields (graph edges)
- AST analysis (structural edges: calls, imports, extends, contains)
- Auto-linking (knowledge → code `auto_references` edges)

Embeddings are recomputed during reindex. The DB is fully derived from files and source code.

## See also

- [Architecture](../design/architecture.md) — system overview
- [Entity Model](../design/entity-model.md) — entity types and state machine
- [Conventions](conventions.md) — file-first invariant
