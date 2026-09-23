---
id: 1be0fa79-30a4-4a09-b652-5b8e70023a04
title: "Observed: local flags on root are invisible to subcommands"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["bfcc7754-c1b3-4f91-b790-f9c19d6896bc"]
---

Flags() on root are not inherited; only PersistentFlags() propagate down the tree.
