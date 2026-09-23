---
id: 1be0fa79-30a4-4a09-b652-5b8e70023a04
title: "Observed: local flags on root are invisible to subcommands"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["b7bb12d4-b70c-4b30-9b21-e667f000b5c3"]
---

Flags() on root are not inherited; only PersistentFlags() propagate down the tree.
