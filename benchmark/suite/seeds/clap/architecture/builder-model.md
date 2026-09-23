---
id: ae3a2a80-d98d-4d9b-b435-57a02512ad3c
title: "Everything is a builder: Command, Arg, ArgGroup"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["builder", "args"]
---

clap_builder/src/builder/: command.rs (Command/CommandBuilder), arg.rs (Arg), arg_group.rs (ArgGroup). Declarative config objects — parser consumes them at parse time. Arg actions (ArgAction::Set/Append/Count/SetTrue/Help/Version) in action.rs define per-flag semantics.
