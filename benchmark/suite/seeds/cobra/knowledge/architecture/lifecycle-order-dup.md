---
id: e89ed7b2-3044-4111-a9ce-f524e11e4079
title: "Hook lifecycle: persistent-pre, pre, run, post, persistent-post"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-b"]
---

execute() in command.go fires PersistentPreRun then PreRun before Run, then PostRun and PersistentPostRun. Hooks are gated on Run being declared.
