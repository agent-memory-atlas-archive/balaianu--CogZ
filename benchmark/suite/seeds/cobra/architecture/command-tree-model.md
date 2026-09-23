---
id: 5549f67c-69cf-4647-bee0-dc9a2eefbd12
title: "Commands form a tree resolved by ExecuteC and find()"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["commands", "dispatch"]
---

Command is a node: root.Execute() -> ExecuteC walks args via find() to resolve the target subcommand, then calls execute(). TraverseChildren controls whether args are parsed as flags during descent or treated strictly as command names until the target is found.
