---
id: dc65ea0d-fcb8-4cdc-bd75-c83eaabf549c
title: "Use more permissible types in ASGIApp  (#3109)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

* Use the type.MutableMapping instead of Dict (from commit 4de13707ee; touches httpx/_transports/asgi.py)
