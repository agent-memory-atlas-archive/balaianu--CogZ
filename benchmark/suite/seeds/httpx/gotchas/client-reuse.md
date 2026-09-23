---
id: 87b67156-3138-4ac1-b2e7-0b33691b336c
title: "Top-level calls create a fresh connection pool every time"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["performance", "gotcha"]
---

httpx.get() in a loop opens and closes a pool per call — no keep-alive, no cookie persistence, TLS handshake each time. Client as context manager is the correct pattern for >1 request; AsyncClient likewise.
