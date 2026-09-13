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
---

# Embedding pipeline

`embed_synced` only processes `synced_entity_ids` — entities created or updated in this sync. Content-hash-unchanged entities are never re-embedded.

**Split strategy:**
- Knowledge entities: always embedded inline — usually <50, fast.
- Code entities: inline only when ≤50 (`INLINE_CODE_THRESHOLD`); above that, deferred to a detached `cogz embed-bg --ids-file <tmp>` subprocess so `cogz index` returns immediately. The child re-opens the DB itself; IDs are passed via a temp file (PID+timestamp name for concurrent safety — see gotcha entry).

**Lock pattern** (the reference implementation for lock-drop-infer-reacquire): fetch entity data under `conn()` → drop guard → model inference outside the lock → re-acquire to `store_embeddings`. Never hold the DB lock during ONNX inference.

**Model selection per entity type:** code → CodeRankEmbed (`code_embeddings` vec0 table), knowledge → bge-base (`knowledge_embeddings`). Separate tables because the spaces are incompatible — KNN must never compare across them.

**Availability degradation:** `model_files_exist()` gate before any embed call; missing model → skip with warn, entities simply remain unembedded. NOTE: there is no repair pass for entities that ended up unembedded — skipped embeddings stay missing until the entity's content changes (see observation re: migrate_v3 + embed_synced, backlog item 25).