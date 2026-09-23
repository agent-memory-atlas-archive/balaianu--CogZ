---
id: 756bbf78-7043-4951-a635-375d6199db14
title: "Only persistent flags propagate to subcommands"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-a"]
---

Local flags declared via Flags() stay on their command; PersistentFlags() are visible to the whole subtree.
