---
id: 41475444-1c89-470d-aabd-3dc242585cbd
title: "Stale PATH binary silently killed CLI adoption mid-benchmark — one agent deleted .cogz entirely"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
---

During replay run6, `~/.local/bin/cogz` was a weeks-old build expecting schema
v5 while all worktree DBs were v6. Six of fourteen agents tried `cogz
search`/`cogz context`, got `schema version mismatch: db has 6, binary
expects 5`, and abandoned the tool. One agent responded by **deleting the
entire `.cogz/` directory** — losing that task's telemetry.

MCP tools were unaffected (they invoke the absolute release-binary path);
only the PATH-mediated CLI was broken. That made the failure nearly
invisible: deliveries kept recording while a whole access channel silently
failed.

Deployment lesson: any environment where agents get CogZ via both a
hook/MCP-pinned binary and a PATH binary needs the two kept in lockstep —
or the PATH binary should refuse more loudly/fail to an obvious error. A
"version" probe comparing the binary's expected schema against the DB's
before running would have surfaced this in seconds instead of six tasks in.
