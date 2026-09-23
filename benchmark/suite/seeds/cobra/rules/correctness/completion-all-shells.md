---
id: 1a078f19-6d25-451a-b169-22733ee0bbde
title: "Completion changes must cover bash, zsh, fish, powershell"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["completion", "shells"]
confidence: 1.0
---

The four *_completions.go generators plus the hidden __complete machinery must stay in sync — a fix in one shell's output without the others is a regression.
