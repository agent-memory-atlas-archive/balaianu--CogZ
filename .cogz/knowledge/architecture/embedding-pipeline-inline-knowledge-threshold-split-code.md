---
id: 255d325d-2e9d-45d9-a66d-557bb689b5c6
title: "Embedding pipeline — inline knowledge, threshold-split code, embed-bg subprocess"
type: knowledge
status: active
created_at: "2026-09-13T21:43:28.248328810+00:00"
updated_at: "2026-09-13T21:43:28.248328810+00:00"
references: ["27d02160-8659-5285-8502-4df96c70537a", "c333b85b-1fea-530e-b07a-c402e21f8909"]
category: architecture
tags: ["architecture", "embedding", "background", "performance"]
verified_against: ["1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "27d02160-8659-5285-8502-4df96c70537a=8e5e2c355a980636a675df7bf13cdc7babcf9b8f36987f33a44d95dc118bafdc", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "650d6ff7-5271-55b8-be38-381525a35e27=01f7ab5b956c6ef1a4f1d00863fe86e778cf4fdabbb1a684050f1f8e8516e217", "818582ba-3b69-59a6-b3a1-ec7157b54346=f512d3eec168c212aab739763e8150455e59634d99cd72a043093e0243c207d9", "82aada0c-b9fa-5c1d-9c62-4fcc15ef9ab0=79f9027b6b74d3be15327c6e1d294cf316658d6598162e1dcc8f1510fdec6cfe", "8d6dc4e1-8f11-5385-b9bc-0528fa254c36=af1a8165f64a639b17e354bfdc381fdb47084ffe0df27457ad54e1ebaa2b1bbf", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c333b85b-1fea-530e-b07a-c402e21f8909=3f7fd5d178a420a57531a9417602b0d8429827638e2ccadbc12b8f7ca4c596c2", "cc7e17c2-8fe5-5bed-971c-5f4bc57f4227=356e2886b41981770e033c1fe9056ea477e60f774d30f80e40afc180d617e2f1", "da11d218-6d48-589e-8c36-5abebe6bb7d8=51191e04bd84309046660216011fcd5e9b30f784bb9bc3b365e9a54eae730099", "f230dc20-44cd-519e-bbac-3a5ce749116f=f9264062a2a3f03786121308a0e5c9b2c449f9111b7146698033dcea8293c90d", "f4428095-b60e-5fef-b3ee-83fb9b5d8c31=e483afcd35a48024e7b08afc284e0b3d129b61d1312e33672502c9fa013ae3b5"]
---

# Embedding pipeline

`embed_synced` only processes `synced_entity_ids` — entities created or updated in this sync. Content-hash-unchanged entities are never re-embedded.

**Split strategy:**
- Knowledge entities: always embedded inline — usually <50, fast.
- Code entities: inline only when ≤50 (`INLINE_CODE_THRESHOLD`); above that, deferred to a detached `cogz embed-bg --ids-file <tmp>` subprocess so `cogz index` returns immediately. The child re-opens the DB itself; IDs are passed via a temp file (PID+timestamp name for concurrent safety — see gotcha entry).

**Lock pattern** (the reference implementation for lock-drop-infer-reacquire): fetch entity data under `conn()` → drop guard → model inference outside the lock → re-acquire to `store_embeddings`. Never hold the DB lock during ONNX inference.

**Model selection per entity type:** code → CodeRankEmbed (`code_embeddings` vec0 table), knowledge → bge-base (`knowledge_embeddings`). Separate tables because the spaces are incompatible — KNN must never compare across them.

**Availability degradation:** `model_files_exist()` gate before any embed call; missing model → skip with warn, entities simply remain unembedded. NOTE: there is no repair pass for entities that ended up unembedded — skipped embeddings stay missing until the entity's content changes (see observation re: migrate_v3 + embed_synced, backlog item 25).