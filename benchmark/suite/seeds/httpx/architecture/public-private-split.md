---
id: 23bb8dce-7964-4606-b7fd-f813458d5898
title: "Public API is re-exported from underscore modules"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["api", "structure"]
---

All implementation lives in underscore-prefixed modules (httpx/_client.py, _models.py, _config.py, _urls.py, _content.py, _decoders.py). httpx/__init__.py re-exports the public surface. The top-level functions in _api.py (httpx.get/post/...) each create a short-lived Client — convenience calls do not share a connection pool.
