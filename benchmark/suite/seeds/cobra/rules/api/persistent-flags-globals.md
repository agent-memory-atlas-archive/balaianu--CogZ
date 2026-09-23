---
id: 27600cd6-79af-403e-b95e-5f884a6a5c9f
title: "Global flags belong on PersistentFlags, not local Flags"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["flags", "api"]
confidence: 1.0
---

Flags that must be usable from any subcommand (verbose, config, output format) go on root's PersistentFlags — local Flags() do not inherit.
