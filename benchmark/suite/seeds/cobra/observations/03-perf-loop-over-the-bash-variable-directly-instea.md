---
id: aea5003e-e5bf-4220-93a8-f478ff83a191
title: "perf: Loop over the bash variable directly instead of starting subprocesses. (#2333)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Today, the loop is printing the completions one value per line and then (from commit 10d4b48a79; touches bash_completionsV2.go)
