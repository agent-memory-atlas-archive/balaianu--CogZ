---
id: 43513881-613a-42a0-8973-7baa3241a530
title: "PersistentFlags vs Flags: inheritance boundary"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-b"]
---

Anything meant for the whole CLI goes on PersistentFlags — Flags() definitions are per-command only.
