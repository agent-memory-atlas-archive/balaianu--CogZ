---
id: 6bd4b3fe-b46c-4762-87f5-32135de6a963
title: "Timeout behavior is four separate phases"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-a"]
---

connect, read, write and pool timeouts each have their own limit — the Timeout value object in _config.py holds all four. Defaults are 5 seconds each.
