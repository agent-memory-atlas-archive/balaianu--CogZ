---
id: f94e5fc4-8698-4bb9-8ef8-84c8010d65bb
title: "Shell completion runs through hidden __complete commands"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["completion", "shells"]
---

completions.go registers a hidden `__complete` (and `__completeNoDesc`) command on the root. Shell scripts call it with the partial command line; it returns candidates + a directive (no file completion, etc.). Per-shell generators live in bash_completions*.go, fish_completions.go, zsh_completions.go, powershell_completions.go.
