---
id: bbd2b00f-a28c-490f-8358-f49fea7dba88
title: "Async behavior must be verified on asyncio and trio"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["async", "testing"]
confidence: 1.0
---

AsyncClient runs on anyio over asyncio or trio — a fix verified only under asyncio may still fail under trio. tests/ parametrizes backends; new async code needs both.
