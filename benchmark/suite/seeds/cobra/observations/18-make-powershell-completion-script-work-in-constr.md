---
id: 55a2e64d-1e4d-4fa1-b7a8-b1d8709f183d
title: "Make Powershell completion script work in constrained mode (#2196)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Creating CompletionResult objects is not allowed in Powershell constrained mode, so return results as strings if constrained mode is enabled (from commit 5a138f143f; touches powershell_completions.go)
