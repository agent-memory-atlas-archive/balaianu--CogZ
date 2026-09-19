---
id: d0e4f5a6-09cc-414e-831c-8f5710374d4a
title: Prune must tombstone DB before deleting canonical file
type: knowledge
status: stale
created_at: "2026-09-03T11:58:00Z"
updated_at: "2026-09-18T21:27:36.099486576+00:00"
references: []
category: gotchas
tags: ["prune", "doctor", "tombstone", "file-first", "ordering"]
stale_reason: code_orphaned
verified_against: ["2251d4bc-6530-5021-9614-e30a40d55bb2=5f062eb64b721584600c7459ef5bad3f693e4b136d573d1d994b3283a9224b3d", "25cd1b20-6fab-5ae7-b527-10574887f152=82f32efbf0b43836d5cad54a5a499c96b776e5b35c7b1e84f8978cead5d01982", "631d4606-14ef-5ffc-893e-6127b47c728f=f5c86993303b3fc8a1061ba5cf430424819fb1f22b266a3f4ff651e2fc242fb4", "89d32399-092e-5d96-b722-40a28f255013=7278c3a7a88f0674f6fb71113060849ef28994e0fd73a0a0c4c9696276a65886", "97553e25-97f2-58bc-8f08-7df5913f1ce6=b8a78ff6177ef57569f52ad6e4f3a6c23022501ae585922a4c26cfc30255966a", "989c84cd-c402-539a-9610-88a700241369=996683eb410ad58cab4578337cc9adcd9a4213b1907fadae3f51c8ff3260b48e", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

# Prune must tombstone DB before deleting canonical file

The file-first invariant says "write the file first, then sync the
DB." For deletion (prune), the inverse applies: **tombstone the DB
first, then delete the file.**

If the order is reversed (delete file, then tombstone DB), a failure
in `tombstone_entity` leaves the system in an unrecoverable state:
the canonical file is gone, but the DB entity is still active. The
content is lost permanently — `cogz reset` + `cogz index` cannot
rebuild from a deleted file.

## The correct order

1. `tombstone_entity(&conn, &entity_id)` — DB entity becomes a
   tombstone (status = "pruned", content/embedding/FTS removed)
2. `delete_embedding(&conn, &entity_id)` — remove vec0 entry
3. `std::fs::remove_file(&file_path)` — delete the canonical file

If step 1 fails, the file is still on disk and the entity is still
in its terminal state (rejected/superseded). This is safe — the
prune can be retried. If step 3 fails, the entity is already
tombstoned — the orphaned file is cosmetic and will be cleaned up
on the next sync.

## Where this was found

`run_prune` in `src/doctor/prune.rs` deleted the file first, then
called `tombstone_entity`. Found in the second full code audit
(2026-09-03).
