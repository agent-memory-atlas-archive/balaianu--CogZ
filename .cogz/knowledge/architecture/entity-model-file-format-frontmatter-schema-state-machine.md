---
id: cf109957-2279-457e-8e1e-fc6166765df7
title: "Entity model — file format, frontmatter schema, state machine"
type: knowledge
status: active
created_at: "2026-10-28T10:20:46Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd"]
category: architecture
tags: ["entity-model", "frontmatter", "state-machine", "drift", "volatility", "pruning"]
verified_against: ["a9d8f4cd-a22a-4e0c-a25a-418c92564dcd=3145d032b6d1c1836af620ad7d3c5b8d852477de0c76889f2cadf8c2423a502c"]
---

Seven entity types: three file-backed (observation, rule, knowledge —
random UUID v4) and four DB-only code entities (function, class,
file, module — deterministic UUID v5 from
`{file_path}:{entity_type}:{qualified_name}`, so `reset` + `index`
rebuilds identical IDs and `references` edges survive). Code entities
come from tree-sitter (Rust, Python, Go, JS/TS/TSX, Bash); paths are
normalized to forward slashes for cross-platform ID stability.

**File format:** Markdown + YAML frontmatter, parsed by a hand-rolled
YAML subset (`src/files/frontmatter.rs` — scalars + inline arrays
only, no nested mappings). Required fields: `id`, `title`, `type`,
`status`, `created_at`, `updated_at` (RFC 3339), `references`
(UUIDs). Optional: `volatility` (`living`|`record`). Type-specific:
observations get `source`/`supporting_ids`; rules get
`confidence`/`derived_from`; knowledge gets `category` (required —
becomes the `knowledge/<category>/` subdir) and `tags`.

**Layout:** `knowledge/<category>/<slug>.md`, `rules/<slug>.md`,
`observations/<YYYY-MM>/<slug>.md`. Slug: lowercase title, hyphens,
≤60 chars.

**State machine:** `active → stale | rejected | superseded`;
`stale → active | rejected | superseded`; `rejected|superseded →
pruned`; `pruned` terminal. Illegal transitions fail in
`transition_status()`. `stale` is a suspicion state, not terminal —
code-orphaned entities (`stale_reason: code_orphaned`) auto-reactivate
when all references resolve active again; manually-flagged stale
never does. `rejected` = the claim is *wrong* (verdict via reject),
`superseded` = replaced by merge (`superseded_by` field).

**Drift ≠ stale:** a changed-but-live reference produces drift, not a
status change. `verified_against` frontmatter
(`["<uuid>=<hash>"]`) records the code state at authoring/last
verify — added at creation, on reference updates, and by index-time
backfill; only `cogz verify` rewrites it. The derived `entity_drift`
table (`changed`/`missing`/`unverified` rows) is recomputed on every
index/file-save and demotes results by `search.drift_penalty`
(0.7^count) — drifted entities stay retrievable. `verify` is
attestation: re-stamps resolvable refs, clears drift rows,
reactivates fully-resolved entities. A *wrong* claim goes through
`reject`, not verify.

**Volatility** gates drift tracking: `living` = claim about current
state (architecture, gotchas, rules — default for rules and most
knowledge categories); `record` = immutable dated history (default
for observations and `decisions`/`benchmark`/`replay`/`incident`
categories). Records skip provenance, drift demotion, and mechanical
stale flagging; explicit `volatility:` overrides either way.

**Update policy:** observations append-only (no edit tool exists —
doctor catches body edits via content hash); rules append-only for
substance (supersede + recreate); knowledge is the only editable
type (`update_knowledge` overwrites + resyncs). Status changes are
in-place frontmatter through `transition_status()`.

**Auto-links:** knowledge content is scanned for file paths and
unique symbols → `auto_references` edges (DB-only, rebuildable —
never in frontmatter).

**Pruning:** only `rejected`/`superseded` observations older than
`retention.observation_prune_after_days` (90d). Manual —
`cogz doctor --prune-observations --confirm`; dry-run by default.
Pruned entities become tombstones (no content/embedding/FTS, edges
preserved, `tombstone_max_count` = 1000 cap). Rules and knowledge
are never pruned.
