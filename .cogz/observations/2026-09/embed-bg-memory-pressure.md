---
id: 05b6bbcf-8e35-4320-9822-270964e4ddd2
title: "Concurrent embed-bg processes exhaust RAM — ~700MB ONNX footprint each, throttle to ~2"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
---

Running `cogz index`/`embed-bg` against many worktrees in parallel spawned
10–20 embedding processes, each loading ONNX models at ~600–700MB RSS. On
this 7.3GB box, available memory dropped below 1GB and swap grew
aggressively before throughput collapsed to ~0.23 embeddings/sec/process.

Two aggravating details:

- Re-running `cogz index` on the *same* worktree spawns another embed-bg on
  the same DB — three redundant workers were competing on one database.
- A throttle watchdog using `pgrep -f "embed-bg"` matched **its own
  cmdline** and went into a stuck loop re-CONTing processes — anchor the
  pattern (`pgrep -f 'cogz embed-bg'` with the binary path) or exclude self.

Operational rule for multi-worktree indexing: cap concurrent embed-bg at
~2–3 and let the queue drain; CPU-bound ONNX inference does not get faster
under memory pressure.
