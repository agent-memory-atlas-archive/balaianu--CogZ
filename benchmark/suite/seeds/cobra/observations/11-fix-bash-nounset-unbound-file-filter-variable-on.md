---
id: 0273ca5a-a9ce-43cc-ab28-d1b3cec8ede6
title: "fix(bash): nounset unbound file filter variable on empty extension (#2228)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Happens at least if a flag is marked as filename, with "" given as (from commit 4ba5566f57; touches bash_completionsV2.go)
