---
id: b352753d-509e-4fce-8a5e-4b46504e8d6d
title: "Reindex leaves orphan entities for definitions removed inside a file"
type: observation
status: active
created_at: "2026-09-23T00:00:00Z"
updated_at: "2026-09-23T00:00:00Z"
references: []
---

Found via agent-replay contamination: a `.cogz` DB indexed at clap pin
8ab46fe2 was copied into a worktree checked out at `3604b131^` and
`cogz index` re-run. Entities for functions added between the two shas
(e.g. `multiple_value_terminator_positional` in
`tests/builder/multiple_values.rs`) stayed `active` with full post-fix
body content and were retrievable by the agent — a code-answer leak.

Modified definitions ARE updated correctly: the `parse` entity's content
matched the parent-sha source after reindex (fix-only comment absent).
Only *removed* definitions orphan — the per-file parse updates entities
it produces but never deletes the ones it no longer produces. The
indexer's "N stale" counter does not flag them.

Consequence beyond benchmarks: any repo that had a function deleted or
renamed keeps a ghost entity whose content describes code that no longer
exists. FTS and vector search can surface it as current.

Backlog #63. Harness-side purge implemented in
`benchmark/suite/agent_replay.py::purge_stale_code_entities` (title-in-file
check; ~zero false positives at pin on clap's 4664 entities).
