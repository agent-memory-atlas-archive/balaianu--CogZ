---
id: 6fd6f5fe-d983-4c9c-be26-405606749540
title: get_status Mutex Deadlock
type: knowledge
status: active
created_at: "2026-08-29T02:35:00Z"
updated_at: "2026-09-06T10:31:59.659446237+00:00"
references: []
category: gotchas
tags: ["mcp", "deadlock", "mutex", "storage"]
verified_against: ["062d0a8c-99db-51aa-917a-15860b6b8b3e=50c225bdc7901f3be380287d9b622000d8efc2e7d90740103218d009ceeec7d7", "2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "31c39a12-0e87-5fbb-b707-e762c3b54aea=e877e2528442154fd74f29ead36240bad82dfed2ebebbb006d0a332ad6a25cf5", "3697d6d1-8724-505c-851a-d7cdbec8acf5=fa96e67278ae946fc8b4c323e8080605b7a5d07f27c6af553597b304a05ed2cd", "9087219f-11ca-53a2-a34d-bb452dac1c24=b135d9d33a11da8d28276490dadd59868a33b2b5c8c46396e2dd5ed3fb3101dd", "aa787435-c08b-584f-8552-0dd0dd8739bd=3bb1d880fa4852de8f3c63e809c93706217899fee77df11df9aafc744f4e0600", "f230dc20-44cd-519e-bbac-3a5ce749116f=f9264062a2a3f03786121308a0e5c9b2c449f9111b7146698033dcea8293c90d", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

# get_status Mutex Deadlock

The `get_status` MCP tool deadlocks if it calls `storage.db_size_bytes()`
while already holding the storage mutex via `storage.conn()`.

## Root cause

`Storage::db_size_bytes()` internally calls `self.conn()` to acquire the
mutex, query `PRAGMA database_list` for the DB file path, then drop the
lock before doing `std::fs::metadata`. But if the caller already holds
the mutex (via a live `conn` guard), the second `conn()` call blocks
forever — `std::sync::Mutex` is not reentrant.

## Fix

Get the DB path under a short-lived lock, drop it, do the filesystem
I/O, then re-acquire the lock for the remaining queries:

```rust
let db_path = {
    let conn = storage.conn();
    conn.query_row("PRAGMA database_list", [], |r| r.get::<_, String>(2)).ok()
};
let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
let conn = storage.conn(); // re-acquire for queries
```

## General lesson

Any function that internally acquires the storage mutex (like
`db_size_bytes`) must not be called while the mutex is already held.
This is the same pattern documented in the "Mutex and I/O" section of
AGENTS.md, but it applies to *any* mutex-acquiring helper, not just
filesystem I/O.
