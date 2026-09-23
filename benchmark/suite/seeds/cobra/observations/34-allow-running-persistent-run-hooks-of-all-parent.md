---
id: e0c5610b-8ae6-44f3-aec7-7622ab00ac19
title: "Allow running persistent run hooks of all parents (#2044)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Currently, only one of the persistent pre-runs and post-runs is executed. (from commit 4cafa37bc4; touches cobra.go, command.go)
