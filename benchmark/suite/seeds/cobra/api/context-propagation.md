---
id: 5550c532-4af9-47cd-8e7d-02ab54f39f0d
title: "SetContext flows ctx through ExecuteC to every hook"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["context", "cancellation"]
---

root.SetContext(ctx) before Execute; every Command.Context() returns it inside Run/hooks. ExecuteContext is the all-in-one entry. Cancellation propagates to command execution — the right way to wire signal handling into a cobra app.
