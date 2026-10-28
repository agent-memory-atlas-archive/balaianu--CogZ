---
id: 09e8e6bf-50d7-4b85-9b9c-0375e128f253
title: Graceful degradation — FTS-only baseline and model fallback paths
type: knowledge
status: active
created_at: "2026-10-28T10:20:46Z"
updated_at: "2026-10-28T10:20:46Z"
references: ["ac43ee1b-2322-4ba1-afcf-cd2464a2d065"]
category: architecture
tags: ["degradation", "fts-only", "models", "resilience"]
verified_against: ["ac43ee1b-2322-4ba1-afcf-cd2464a2d065=6d5439ce80de3225a8a7babcd69c146c18ba969718f834f08d46f415bab277d3"]
---

CogZ is designed to run with zero ONNX models — FTS-only is the
baseline, everything else is enhancement (see
`docs/design/degradation.md`).

**Works without models:** init, index/reindex (files sync, code
entities indexed without vectors), search and context (FTS + graph +
PRF — PRF is the only vocabulary-mismatch recall path left),
consolidate (title-dedup only), capture-event, doctor + pruning, the
full MCP surface, all hooks. `search_mode` in results reports
`fts_only`.

**Doesn't work:** vector KNN results, embedding-based dedup, NLI
contradiction detection, NLI-confirmed merge (candidates reported,
never merged — conservative), code embeddings, and the silence gate
vector signals (the coverage gate itself still runs — it needs no
embeddings; what's skipped is silencing decisions that needed KNN
channel signals).

**Why models go missing:** `auto_download = false` / `--no-download`;
free RAM below `model_min_free_mb` (512 MB default — degrade rather
than swap); ONNX Runtime library not found (`ort` loads it
dynamically, auto-downloaded on first use); corrupt model files —
`clean_broken_cache()` runs before every load and removes
`.incomplete` files >1h and empty `refs/main` files. Every path logs
a warning, sets `available: false`, continues. No panics.

**Lifecycle:** models are lazy (real ONNX load on first
`embed()`/`classify()`) and auto-unload after `model_idle_ttl`
(300 s default) — RAM drops back to ~11 MB. `auto_download` is
opt-out: `cogz index` fetches from HuggingFace on first use.

**Partial modes:** code model only → `code_hybrid` (FTS + code
vector); knowledge model only → `knowledge_hybrid`.

**Hooks always pass `--fts-only`** — loading ONNX on every lifecycle
event is the cost, not the point.

RAM profile: bare binary ~11 MB; FTS-only with DB ~30 MB; all models
~300–500 MB; post-unload ~11 MB.
