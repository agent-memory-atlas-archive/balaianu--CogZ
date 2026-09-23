---
id: 86e0eaa3-b3e7-4f48-8304-2e05e3a92e62
title: "Remove the default 'completion' cmd if it is alone (#1559)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

When a program has no sub-commands, its root command can accept (from commit 24ada7fe71; touches command.go, completions.go)
