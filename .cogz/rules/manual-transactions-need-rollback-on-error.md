---
id: f8b0c1d2-09cc-414e-831c-8f5710374d4a
title: Manual transactions need ROLLBACK on error
type: rule
status: active
created_at: "2026-09-03T12:10:00Z"
updated_at: "2026-09-04T09:12:04.108524067+00:00"
references: []
category: correctness
tags: ["transaction", "error-handling", "sqlite"]
confidence: 1
verified_against: ["8daa25f1-e56d-5a80-900d-3a9c8f4eb551=c01cd07070795b9c7281ccf9fda6d2bc2abcaa62e7321033150b289fd707a417", "cd2b1f38-508c-5f6d-adeb-7f7d1969568a=844744081f2603acb00c39ac825a37f18307b6dd827e05f14c6ddcfd5fe77467"]
---

When using raw `BEGIN`/`COMMIT` on the shared SQLite connection,
every `?` early return inside the transaction body leaks the
transaction. The connection stays in transaction mode, breaking
all subsequent DB operations.

**Rule:** Wrap transactional operations in a closure. If the
closure returns an error, execute `ROLLBACK` before propagating.

**Reference pattern:** `merge_one` in `src/consolidate/merge.rs`.

**Why not rusqlite's Transaction guard:** The shared connection
behind `std::sync::Mutex` makes borrowing the guard across the
mutex boundary awkward. The closure pattern is simpler and equally
correct.
