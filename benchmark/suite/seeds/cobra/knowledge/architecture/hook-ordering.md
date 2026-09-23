---
id: 785f5367-f893-43f3-a8e3-6f9b9c15622f
title: "Hook order is PersistentPreRun, PreRun, Run, PostRun, PersistentPostRun"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["hooks", "lifecycle"]
---

command.go execute() runs them in that order. Persistent*Run hooks inherit down the tree — a child without its own PersistentPreRun gets the parent's. All hooks only fire if the command declares Run/RunE. Note the asymmetry: parent PersistentPreRun fires for children, but PersistentPostRun runs only if the *executed* command's chain reaches it.
