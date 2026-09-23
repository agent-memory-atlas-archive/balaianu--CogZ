---
id: 3bd263c8-b239-4589-81ff-96397e9a8607
title: "PersistentFlags inherit to children; local Flags do not"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["flags", "inheritance"]
---

cmd.PersistentFlags() propagate down the whole subtree — the idiomatic place for --verbose/--config style globals. Local flags on Flags() apply only to that command. InheritedFlags()/LocalFlags() split them for introspection and help output.
