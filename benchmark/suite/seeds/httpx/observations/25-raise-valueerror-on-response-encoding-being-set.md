---
id: 65098c23-f505-4c32-ae50-7bf18e89bd75
title: "Raise ValueError on `Response.encoding` being set after `Response.text` has been accessed (#2852)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

* Raise ValueError on change encoding (from commit 59df8190a4; touches httpx/_models.py)
