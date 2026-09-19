---
id: 9f1c2a7e-3b4d-4e5f-8a6b-7c8d9e0f1a2b
title: "cogz index hung at exit: conn guard held across update_baseline re-lock"
type: observation
status: stale
created_at: "2026-09-15T22:55:00Z"
updated_at: "2026-09-16T11:55:44.723328670+00:00"
references: ["95663f30-f013-5db1-9bcf-fccf0d387e1c", "d9a05874-6f68-5705-841e-18305c903d30"]
verified_against: ["95663f30-f013-5db1-9bcf-fccf0d387e1c=137516c1ab6097543fd77c95d668d49b2a5313807587084e11043ca3b429cc25", "d9a05874-6f68-5705-841e-18305c903d30=8ee710398fc02a23f95c29bf6dd5accbb05f0c0f821def0cefc61e585e7a4083"]
---

`cogz index` deadlocked deterministically at exit after completing all work
(entities synced, edges built, embeddings stored, "Total entities" printed).

Root cause: `run_index` bound `let conn = storage.conn()` at function scope,
then `update_baseline(&storage, ...)` called `storage.conn()` again while the
first `MutexGuard` was still live. `std::sync::Mutex` is not reentrant → main
thread futex-waited on its own stack-resident mutex forever.

Diagnosis via strace: last syscall before hang was `futex(FUTEX_WAIT_BITSET)`
on a stack address (0x7ffc...) 0.5ms after `peel_to_commit` opened the HEAD
commit object — i.e. inside `update_baseline` right after `head_sha` returned.

Red herrings eliminated: ort thread pool (8 workers parked idle is normal),
memory pressure, git2 internals, the defunct `embed-bg` child (zombie is
by-design for detached spawn — reaped at parent exit).

Fix: scoped the guard — `let total = { let conn = storage.conn();
count_all(&conn)? };` so it drops before `update_baseline`.

Detection difficulty: every stage's output printed and all DB writes committed,
so the process looked complete from the outside. Any long-lived conn guard
followed by a call that re-acquires `storage.conn()` deadlocks the same way.
Watch for `let conn = storage.conn()` bound at function scope when later calls
take `&storage` instead of `&conn`.
