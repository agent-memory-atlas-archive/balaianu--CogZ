---
id: 7f3e2a10-9b4c-4d55-a812-60e1a5c9d311
title: "Long-lived MCP server keeps a dead DB handle after cogz reset — writes silently vanish"
type: observation
status: active
created_at: "2026-09-29T04:15:00+00:00"
updated_at: "2026-09-29T04:15:00+00:00"
references: []
supporting_ids: ["c336876e-0600-400f-bbfb-76c5c808c655"]
source: dogfood-mcp-session
confidence: 0.5
---

Observed in a live session: an `mcp-stdio` server that connected before
`cogz reset` kept its `Connection` to the deleted DB inode.
`create_entity` afterwards returned success — the file was written, and
SQLite happily synced into the orphaned inode. The entity existed on
disk but was absent from the rebuilt DB. SQLite allows writes to
deleted-but-open files, so there is no error anywhere: the tool reports
success and the write is gone.

Recovery: `cogz reindex` — the file-first invariant means nothing is
lost; the unsynced file syncs on next index pass.

Gap worth a fix later: the server has no freshness check on its DB
handle (compare `db_path` inode/mtime vs the open connection, reopen on
mismatch — cheap enough if scoped to write paths, or a lighter check at
session start). Until then, the operational rule is: restart MCP
servers after `cogz reset` / DB replacement. `run_reset` prints "Run
cogz index to rebuild" but says nothing about live MCP sessions — a
one-line warning there would close most of the trap cheaply.
