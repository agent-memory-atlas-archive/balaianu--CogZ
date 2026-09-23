---
id: 4c876d3d-bebe-4d43-90bd-c07b6bb7d9ac
title: "Positional arg validation runs between flag parsing and Run"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["args", "validation"]
---

args.go provides composable validators: NoArgs, ArbitraryArgs, OnlyValidArgs, MinimumNArgs, MaximumNArgs, ExactArgs, RangeArgs, MatchAll, ExactValidArgs. Command.Args is invoked in execute() after flags are parsed; a validator error triggers usage output.
