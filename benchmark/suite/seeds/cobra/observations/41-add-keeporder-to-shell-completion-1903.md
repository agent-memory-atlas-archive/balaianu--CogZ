---
id: 34908ffb-e681-4c7a-9eae-253042ddb257
title: "Add keeporder to shell completion (#1903)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

This allows programs to request the shell to maintain the order of completions that was returned by the program (from commit 3daa4b9c36; touches bash_completionsV2.go, completions.go, fish_completions.go, powershell_completions.go)
