---
id: d4e5f6a7-b8c9-4d0e-8f1a-2b3c4d5e6f70
title: Usage instrumentation — persistent delivery boundaries (Phase 2)
type: knowledge
status: stale
created_at: "2026-09-15T00:00:00Z"
updated_at: "2026-09-16T11:55:44.773510253+00:00"
references: []
category: decisions
tags: ["usage-tracking", "instrumentation", "schema-v4", "hit-rate"]
---

Phase 2 usage tracking needed session-level state across
`cogz capture-event` invocations — but each hook call is a separate
process, so the spec's in-memory `RefCell<HashSet>` cannot survive
from `prompt_submit` to `post_tool_use`. Delivery state lives in
SQLite instead (schema v4: `deliveries` + `entity_usage`), which is
correct because usage rows are derived telemetry, not canonical data.

Semantics: a new pack or search delivery closes all open deliveries
(pending → miss). post_tool_use marks hits via file-path → entity
mapping plus title/id substring match in tool results. session_end
closes everything.

Gotchas found during implementation:

- Cold-start packs contain pseudo-sections (`repo`, `index`) whose
  entity_ids aren't real entities — `record_delivered` filters
  through `WHERE EXISTS (SELECT 1 FROM entities ...)` or they'd be
  permanent misses polluting the hit rate.
- `--repo` defaults to `.`, so `cogz_dir` is relative — absolute
  `file_path` values from hooks fail `strip_prefix` unless
  `normalize_file_candidates` canonicalizes `cogz_dir` first.
- Knowledge/rule hits undercount: the agent can apply a rule without
  re-reading its file. File-path attribution is reliable for code
  entities only.
