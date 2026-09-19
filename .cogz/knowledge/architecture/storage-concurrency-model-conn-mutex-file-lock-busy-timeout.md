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
verified_against: ["047656c5-e449-58d7-b951-3b948a609cd9=2573200a85d75170d672883dfed5d39c5e19f3c4d15c0dcb8d3c5e1aa82bfe59", "062d0a8c-99db-51aa-917a-15860b6b8b3e=50c225bdc7901f3be380287d9b622000d8efc2e7d90740103218d009ceeec7d7", "1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "2251d4bc-6530-5021-9614-e30a40d55bb2=5f062eb64b721584600c7459ef5bad3f693e4b136d573d1d994b3283a9224b3d", "2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "31c39a12-0e87-5fbb-b707-e762c3b54aea=e877e2528442154fd74f29ead36240bad82dfed2ebebbb006d0a332ad6a25cf5", "6418285a-aa22-5b7a-ba4b-c68c49fd6aaa=4a09a3779b76dde9a5813dc8408095de6c7b8b548b2d9ee645f306fa0b3a5b20", "68efa752-26c9-5772-a815-38d65cd1bd9e=76bc07cd7c9ddf3fd059e043b4148232c201e37708ebf3df21b7a782f082de2b", "9087219f-11ca-53a2-a34d-bb452dac1c24=b135d9d33a11da8d28276490dadd59868a33b2b5c8c46396e2dd5ed3fb3101dd", "90ee1373-02db-5bd1-8fb4-90243806dbc7=70109705b8680295ac5d917107332fee814984264f14617fe505a44acd5de12a", "da11d218-6d48-589e-8c36-5abebe6bb7d8=51191e04bd84309046660216011fcd5e9b30f784bb9bc3b365e9a54eae730099", "ea86b801-ccab-54d8-913e-bb3435d731b4=827b67be78a091fe103bc9f7d4cd04bf7dfffb1ea7c2540bee15afb9c2346b02", "edf944ac-8262-5137-bb33-e972dae56799=a7da996410377f65b88b43d652dda9dd4fa8509173f8835e0139fd7844ac051e", "eeeb377b-1fab-5129-b252-f88d01e403c7=c5fd6b88404ae2dfb7f8373ef2b21202d77b479fd3c045da5b2a3c393131e0e8", "f230dc20-44cd-519e-bbac-3a5ce749116f=f9264062a2a3f03786121308a0e5c9b2c449f9111b7146698033dcea8293c90d", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

# Storage concurrency model

Single `rusqlite::Connection` behind `std::sync::Mutex` — no pool, by design. Three distinct coordination layers, easy to conflate:

1. **DB lock** (`storage.conn()`): guards the SQLite connection. Poisoned-mutex recovery via `unwrap_or_else(|e| e.into_inner())` — a panicking task must not kill the server. Never hold during filesystem/network I/O; the reference pattern is `db_size_bytes` (query path under lock, drop guard, then `fs::metadata`).

2. **File lock** (`storage.file_lock()`): serializes canonical-file read-modify-write sequences (knowledge updates, stale flagging, merge, prune). In-process `Mutex<()>` AND an fs2 advisory lock on `.cogz/.lock` for cross-process safety — hooks, CLI, and MCP server can run concurrently. Acquired on demand, released on guard drop.

3. **SQLite-level**: WAL mode + `busy_timeout=5000` — concurrent processes (reindex-bg, embed-bg) wait instead of erroring. Foreign keys ON.

Lock-ordering constraint: don't hold `conn()` while doing file I/O, and don't hold `file_lock()` while doing long DB work. `flag_stale_knowledge` shows the real pattern — fetch under conn, `drop(conn)`, acquire file_lock, do file RMW, sync each file (which re-acquires conn briefly inside sync_single_file's transaction).

Known gap (2026-09-14): `.cogz/.lock` open failure degrades to in-process-only silently — see backlog item 24.