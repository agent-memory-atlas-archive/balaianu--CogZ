---
id: 5af708db-cdc3-4d00-8fe9-99669510e19f
title: "Flag group constraints are declared then validated post-parse"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["flags", "validation"]
---

flag_groups.go provides MarkFlagsMutuallyExclusive, MarkFlagsRequiredTogether, MarkFlagsOneRequired, MarkFlagsDependentRequired. They annotate flags; ValidateFlagGroups runs after parsing, before Run. This is annotation+validation, not parser-level exclusion.
