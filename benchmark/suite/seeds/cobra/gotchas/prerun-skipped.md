---
id: b9b64e62-8368-40ae-963a-77fe6a6fb267
title: "Hooks never fire on commands without Run"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["hooks", "gotcha"]
---

A Command with only PersistentPreRun and no Run/RunE is a no-op — the whole hook chain is gated on Run being declared. Adding a stub Run is required for hook-only commands.
