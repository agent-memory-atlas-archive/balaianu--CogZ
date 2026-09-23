---
id: 96e21085-8a19-4125-926f-367ea8babdf1
title: "Timeouts are a 4-tuple: connect, read, write, pool"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["timeouts", "config"]
---

Default is 5s for all four. Timeout(10.0) sets all; fine-grained config uses Timeout(timeout=5.0, connect=10.0). Pool timeout is the wait for a pooled connection to free up — not network IO. timeout=None disables entirely. Enforced per-request or as Client default.
