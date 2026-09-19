---
id: f7a1b2c3-09cc-414e-831c-8f5710374d4a
title: "Transaction leak on early return with ? operator"
type: knowledge
status: active
created_at: "2026-09-03T11:55:00Z"
updated_at: "2026-09-04T09:12:03.861728462+00:00"
references: []
category: gotchas
tags: ["transaction", "sqlite", "error-handling", "mutex"]
verified_against: ["23c987cf-baf8-5ee9-9964-359d33027b7e=5e1922c9289543a2c42a79be19f53a7797052fdd2a8c35fe0534d15124ffd8bf", "2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "3697d6d1-8724-505c-851a-d7cdbec8acf5=fa96e67278ae946fc8b4c323e8080605b7a5d07f27c6af553597b304a05ed2cd", "746f7c70-0ad8-52c6-82f8-d0fe8f33b691=aad616542c6c2f94f34184c08e5ee16d2c012d7a1cb46c1bd59784c09dfde3f4", "8daa25f1-e56d-5a80-900d-3a9c8f4eb551=c01cd07070795b9c7281ccf9fda6d2bc2abcaa62e7321033150b289fd707a417", "cd2b1f38-508c-5f6d-adeb-7f7d1969568a=844744081f2603acb00c39ac825a37f18307b6dd827e05f14c6ddcfd5fe77467", "f230dc20-44cd-519e-bbac-3a5ce749116f=f9264062a2a3f03786121308a0e5c9b2c449f9111b7146698033dcea8293c90d", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

# Transaction leak on early return with ? operator

When using raw `BEGIN`/`COMMIT` transactions on the shared SQLite
connection, any `?` early return inside the transaction body leaks
the transaction. The connection stays in transaction mode, breaking
all subsequent DB operations — every query after the leak fails with
"cannot start a transaction within a transaction" or similar.

## The pattern that breaks

```rust
conn.execute_batch("BEGIN")?;
redirect_edges(&conn, ...)?;  // if this fails, transaction leaks
update_entity(&conn, ...)?;   // if this fails, transaction leaks
conn.execute_batch("COMMIT")?;
```

## The fix: closure with explicit ROLLBACK

Wrap the transactional operations in a closure. If the closure
returns an error, execute ROLLBACK before propagating:

```rust
let tx_result: Result<(), Error> = (|| {
    redirect_edges(&conn, ...)?;
    update_entity(&conn, ...)?;
    Ok(())
})();

if let Err(e) = tx_result {
    let _ = conn.execute_batch("ROLLBACK");
    return Err(e);
}
conn.execute_batch("COMMIT")?;
```

## Why not use rusqlite's Transaction guard?

`rusqlite::Transaction` rolls back on drop, which would handle this
automatically. But the shared connection behind `std::sync::Mutex`
makes borrowing the guard across the mutex boundary awkward. The
closure pattern is simpler and equally correct.

## Where this was found

`merge_one` in `src/consolidate/merge.rs` had this bug. The
`redirect_edges`, `update_entity`, and `record_event` calls all used
`?` inside a manually managed transaction. Found in the second full
code audit (2026-09-03).
