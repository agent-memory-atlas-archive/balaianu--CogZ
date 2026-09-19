---
id: d6eafb0c-09cc-414e-831c-8f5710374d4a
title: KNN returns embeddings for non-active entities
type: observation
status: active
created_at: "2026-09-03T12:06:00Z"
updated_at: "2026-09-04T09:05:09.907375497+00:00"
references: []
source: agent
confidence: 0.9
supporting_ids: []
verified_against: ["13e57457-1cb5-52df-a954-06a47dbeedd5=d01511e2e75b099f8e91863746bebda683171b953bfebb2a175f1d1f58f1b196", "2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "3ff469fd-a376-53d7-b555-07a9f1b321e5=967f2b9cb3da0bc0172a48df7a557ae3485197c6b10cc040419ec29b9a00f63b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "6c13bd51-5ca8-532a-8828-20c95debe650=16962e335c0871f99f5002be9b08184091285e451f4de92f1ea0d37252e24bfb", "818582ba-3b69-59a6-b3a1-ec7157b54346=f512d3eec168c212aab739763e8150455e59634d99cd72a043093e0243c207d9", "824cd680-d240-5c71-a14d-7884d349ed37=151d17c00aadd106dda03e60e80391ebc7fc5cadd84692de03d877452f840055", "82aada0c-b9fa-5c1d-9c62-4fcc15ef9ab0=79f9027b6b74d3be15327c6e1d294cf316658d6598162e1dcc8f1510fdec6cfe", "8b994b90-d030-5c71-90b3-1366e90088a3=d899ee2cab66f2f5775d1d12d561fd3231f77278926a83d353b42b737e5f76c2", "dc869bb9-f4fe-5d72-9c46-05276c04ba4c=63418581c6e3ed1d3f7e24458b290848b297221b71f0b104a591e0f6acbe34bc"]
---

The vec0 embedding table retains embeddings for entities after
their status changes. A rejected, superseded, or pruned entity's
embedding is still in vec0 and will be returned by KNN search.

This is by design — removing embeddings on status change would be
destructive (the entity might transition back: stale → active).
But it means any consumer of KNN results must filter by status
after retrieval, not assume the results are all active.

## Where this matters

- `check_duplicate` in `src/consolidate/dedup.rs`: KNN neighbors
  must be filtered against the active entity list
- `expand_with_paths` in `src/search/expand.rs`: already filters
  by status via `batch_check_status` — this was correct
- Context assembly: uses search results which are already
  status-filtered — this was correct

## What would break if we removed embeddings on status change

- `stale → active` transitions would need re-embedding (expensive,
  and the content hasn't changed — the embedding is still valid)
- Tombstoned entities would lose their embedding permanently —
  if a tombstone is ever un-tombstoned (currently not supported,
  but the schema allows it), the embedding would need recomputation
- The vec0 table would need DELETE operations, which are more
  expensive than UPDATEs in sqlite-vec

The current design (keep embeddings, filter at query time) is
correct and performant.
