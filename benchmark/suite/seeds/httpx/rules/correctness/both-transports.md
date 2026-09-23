---
id: 6b0f1b52-8417-4d36-834d-b56a5949aca8
title: "Behavior must hold for sync and async paths"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["async", "sync"]
confidence: 1.0
---

Every feature/fix must work through both Client and AsyncClient — the sync/async pair in _client.py is parallel but not shared. Check _transports/default.py for both handle_request and handle_async_request.
