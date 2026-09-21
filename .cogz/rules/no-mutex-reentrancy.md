---
id: 0b83613e-61d6-4a60-af20-2e377d72e4f7
title: No mutex reentrancy in storage helpers
type: rule
status: active
created_at: "2026-08-29T02:50:00Z"
updated_at: "2026-09-21T13:53:58.334738643+00:00"
references: []
confidence: 0.95
verified_against: ["062d0a8c-99db-51aa-917a-15860b6b8b3e=50c225bdc7901f3be380287d9b622000d8efc2e7d90740103218d009ceeec7d7", "31c39a12-0e87-5fbb-b707-e762c3b54aea=e877e2528442154fd74f29ead36240bad82dfed2ebebbb006d0a332ad6a25cf5", "9087219f-11ca-53a2-a34d-bb452dac1c24=b135d9d33a11da8d28276490dadd59868a33b2b5c8c46396e2dd5ed3fb3101dd", "ea627870-3363-58c3-ab97-37064a46831f=afc7c6fde1b93799f61a470009da635599bb09375e8cb5bc3c933dd4c336e01a", "f230dc20-44cd-519e-bbac-3a5ce749116f=f9264062a2a3f03786121308a0e5c9b2c449f9111b7146698033dcea8293c90d", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

Any function that internally calls `storage.conn()` (acquiring the
SQLite mutex) must not be called while the mutex is already held.
`std::sync::Mutex` is not reentrant — a second `conn()` call from the
same thread will deadlock.

Before calling any `Storage` method that acquires the mutex internally
(e.g. `db_size_bytes`), drop the current `conn` guard first. If you need
data from the DB and then need to call such a method, fetch the data,
drop the guard, call the method, then re-acquire if needed.
