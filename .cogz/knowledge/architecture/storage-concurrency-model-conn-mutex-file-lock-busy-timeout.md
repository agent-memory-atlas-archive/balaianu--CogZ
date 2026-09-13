---
id: d0a7fa58-953f-4704-aa38-248ca39f1be6
title: "Storage concurrency model — conn mutex, file lock, busy_timeout"
type: knowledge
status: active
created_at: "2026-09-13T21:42:34.240350470+00:00"
updated_at: "2026-09-13T21:42:34.240350470+00:00"
references: ["047656c5-e449-58d7-b951-3b948a609cd9", "68efa752-26c9-5772-a815-38d65cd1bd9e"]
category: architecture
tags: ["architecture", "concurrency", "storage", "sqlite"]
---

# Storage concurrency model

Single `rusqlite::Connection` behind `std::sync::Mutex` — no pool, by design. Three distinct coordination layers, easy to conflate:

1. **DB lock** (`storage.conn()`): guards the SQLite connection. Poisoned-mutex recovery via `unwrap_or_else(|e| e.into_inner())` — a panicking task must not kill the server. Never hold during filesystem/network I/O; the reference pattern is `db_size_bytes` (query path under lock, drop guard, then `fs::metadata`).

2. **File lock** (`storage.file_lock()`): serializes canonical-file read-modify-write sequences (knowledge updates, stale flagging, merge, prune). In-process `Mutex<()>` AND an fs2 advisory lock on `.cogz/.lock` for cross-process safety — hooks, CLI, and MCP server can run concurrently. Acquired on demand, released on guard drop.

3. **SQLite-level**: WAL mode + `busy_timeout=5000` — concurrent processes (reindex-bg, embed-bg) wait instead of erroring. Foreign keys ON.

Lock-ordering constraint: don't hold `conn()` while doing file I/O, and don't hold `file_lock()` while doing long DB work. `flag_stale_knowledge` shows the real pattern — fetch under conn, `drop(conn)`, acquire file_lock, do file RMW, sync each file (which re-acquires conn briefly inside sync_single_file's transaction).

Known gap (2026-09-14): `.cogz/.lock` open failure degrades to in-process-only silently — see backlog item 24.