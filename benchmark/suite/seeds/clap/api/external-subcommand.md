---
id: 7412b8c6-898d-4bf8-9467-d76ff53907ed
title: "external_subcommand catches unmatched subcommands"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["subcommands", "plugins"]
---

Arg::external_subcommand (or .allow_external_subcommands) captures unknown trailing args as (name, Vec<OsString>) — the mechanism for plugin-style CLIs. Bypasses all validation of the captured tail.
