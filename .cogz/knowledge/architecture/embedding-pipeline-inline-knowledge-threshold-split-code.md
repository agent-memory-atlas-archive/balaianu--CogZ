---
id: 255d325d-2e9d-45d9-a66d-557bb689b5c6
title: "Embedding pipeline — inline knowledge, threshold-split code, embed-bg subprocess"
type: knowledge
status: active
created_at: "2026-09-13T21:43:28.248328810+00:00"
updated_at: "2026-10-13T11:40:54.264117689+00:00"
references: ["27d02160-8659-5285-8502-4df96c70537a", "c333b85b-1fea-530e-b07a-c402e21f8909"]
category: architecture
tags: ["architecture", "embedding", "background", "performance"]
verified_against: ["27d02160-8659-5285-8502-4df96c70537a=v2:469479600a9ef372182b0de28e1a69d6612b51a3d869aefbdd3230dc35f622ab", "c333b85b-1fea-530e-b07a-c402e21f8909=v2:17d451c61d808f5f64af1f9b23ff45246751eedc93bb119929e31ee0a6624aea"]
---

# Embedding pipeline

`embed_synced` processes `synced_entity_ids` ∪ `ids_missing_embeddings` — entities created/updated this sync plus active entities with no vector (the repair pass for model-down/interrupted/dropped embeddings). Content-hash-unchanged entities that already have vectors are never re-embedded.

**Split strategy:**
- Knowledge entities: always embedded inline — usually <50, fast.
- Code entities: inline only when ≤50 (`INLINE_CODE_THRESHOLD`); above that, deferred to a detached `cogz embed-bg --ids-file <tmp>` subprocess so `cogz index` returns immediately. The child re-opens the DB itself; IDs are passed via a temp file (PID+timestamp name for concurrent safety — see gotcha entry). The spawn itself is gated on `model_files_exist()` — without it a model-less corpus would spawn a useless process on every index.

**Lock pattern** (the reference implementation for lock-drop-infer-reacquire): fetch entity data under `conn()` → drop guard → model inference outside the lock → re-acquire to `store_embeddings`. Never hold the DB lock during ONNX inference.

**Model selection per entity type:** code → CodeRankEmbed (`code_embeddings` vec0 table), knowledge → bge-base (`knowledge_embeddings`). Separate tables because the spaces are incompatible — KNN must never compare across them.

**Availability degradation:** `model_files_exist()` gate before any embed call (including the embed-bg spawn); missing model → skip with warn, entities remain unembedded but are picked up by `ids_missing_embeddings` on the next index once the model exists. Stale-marked code entities and deleted-file tombstones lose their vectors — the vector lives iff the described content exists.