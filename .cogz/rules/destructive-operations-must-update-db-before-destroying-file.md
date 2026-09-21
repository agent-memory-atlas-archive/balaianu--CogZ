---
id: c1e3f4a5-09cc-414e-831c-8f5710374d4a
title: Destructive operations must update DB before destroying the file
type: rule
status: active
created_at: "2026-09-03T12:13:00Z"
updated_at: "2026-09-21T13:53:58.134592834+00:00"
references: []
category: correctness
tags: ["prune", "deletion", "ordering", "file-first"]
confidence: 1
verified_against: ["2251d4bc-6530-5021-9614-e30a40d55bb2=5f062eb64b721584600c7459ef5bad3f693e4b136d573d1d994b3283a9224b3d", "25cd1b20-6fab-5ae7-b527-10574887f152=82f32efbf0b43836d5cad54a5a499c96b776e5b35c7b1e84f8978cead5d01982", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "89d32399-092e-5d96-b722-40a28f255013=7278c3a7a88f0674f6fb71113060849ef28994e0fd73a0a0c4c9696276a65886", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562"]
---

The file-first invariant says "write the file first, then sync the
DB." For destructive operations (prune, delete), the inverse
applies: **update the DB first, then destroy the file.**

**Why:** If the DB update fails, the file is still on disk and the
entity is in its pre-destruction state — a safe, retryable state.
If the file is destroyed first and the DB update fails, the content
is lost permanently (`cogz reset` + `cogz index` cannot rebuild from
a deleted file).

**Applies to:**
- `run_prune`: tombstone entity → delete embedding → delete file
- Any future code that deletes canonical entity files

**Reference pattern:** `run_prune` in `src/doctor/prune.rs`.
