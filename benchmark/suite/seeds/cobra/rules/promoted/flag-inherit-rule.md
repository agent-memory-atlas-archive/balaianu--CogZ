---
id: bfcc7754-c1b3-4f91-b790-f9c19d6896bc
title: "Shared flags must be PersistentFlags on the root command"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

If a flag should work on every subcommand, declare it via root.PersistentFlags() — local Flags() never inherit.
