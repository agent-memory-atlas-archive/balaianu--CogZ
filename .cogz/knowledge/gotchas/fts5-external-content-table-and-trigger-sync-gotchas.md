---
id: 06dd8275-c7a2-4b4f-b5a4-455b762062e9
title: FTS5 external content table and trigger sync gotchas
type: knowledge
status: active
created_at: "2026-08-28T19:28:00Z"
updated_at: "2026-09-05T09:13:27.412266182+00:00"
references: []
category: gotchas
tags: ["fts5", "sqlite", "triggers", "sync", "gotcha"]
verified_against: ["1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "31c39a12-0e87-5fbb-b707-e762c3b54aea=e877e2528442154fd74f29ead36240bad82dfed2ebebbb006d0a332ad6a25cf5", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "4e0d412d-43f2-57db-8df0-788e0e21eb8d=5d65fec322c809af1d7b153866e3550c663711591edd7065f08261eabf2790c2", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "79c728e6-2f4c-5c88-9525-7846d1cd72fe=fafcf8bc9945faf056ac4096583922141a9d46e1fe98dca58403cd2a60ee9008", "818582ba-3b69-59a6-b3a1-ec7157b54346=f512d3eec168c212aab739763e8150455e59634d99cd72a043093e0243c207d9", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c52e5e65-76c0-5d40-9a72-3ddc27d2ab0c=1de7fe8a1dc7e381c6aa120e07c938dcd63bfaf84f85b625c68b8d012bbec0da", "d0751378-402d-58bc-bb06-7063e448fa50=ddc7c14939eeb17d4292fe1acd783b3a26b3419a5812388532b0336fd35a619d"]
---

The FTS5 table uses external content mode (`content='entities'`).
Three triggers keep it in sync: `entities_fts_ai` (after insert),
`entities_fts_ad` (after delete), `entities_fts_au` (after update).

**Gotcha 1:** The triggers use `entities_fts(rowid, ...)` for
inserts and `entities_fts(entities_fts, rowid, ...)` with
`'delete'` for deletes/updates. The `'delete'` special command is
required for external content tables — without it, FST entries
accumulate as orphans.

**Gotcha 2:** The update trigger does delete-then-insert, not a
direct update. This is the FTS5-recommended pattern for external
content tables.

**Gotcha 3:** If you ever bypass the triggers (e.g., raw SQL
`INSERT INTO entities`), the FTS index won't update. All entity
writes must go through `storage::crud` functions, which use
standard `INSERT`/`UPDATE` statements that fire the triggers.

**Gotcha 4:** The FTS5 tokenizer is `porter unicode61`. The Porter
stemmer handles English word variations (running → run). If
non-English content is common, consider adding a separate FTS
table with a different tokenizer.
