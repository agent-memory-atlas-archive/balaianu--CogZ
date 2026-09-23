---
id: 727eaeab-9f07-4d95-8d72-f13f1e055c3e
title: "Subcommands nest as Command children with propagated settings"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["subcommands", "command"]
---

Command::subcommand() builds the tree; AppSettings/AppFlags control propagation (GlobalVersion, PropagateVersion, ArgsNegateSubcommands...). Version/help propagation and env fallbacks are resolved at build time via _propagate().
