---
id: 6adc7533-ee7e-4831-b2e0-f0652ff9e6c3
title: "httpx splits timeouts into connect/read/write/pool"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-b"]
---

A single timeout= on Client applies to all four phases unless a Timeout instance with per-phase values is passed. Pool timeout covers waiting for a pooled connection.
