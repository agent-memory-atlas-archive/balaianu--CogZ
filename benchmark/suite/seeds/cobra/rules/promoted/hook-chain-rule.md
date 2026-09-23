---
id: e3713664-16e5-421e-8cb3-daa363d1cb6e
title: "Do not put cleanup in root PersistentPostRun expecting it to run for children"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

Cleanup shared across subcommands belongs in each command's own PostRun or a wrapper around Execute — PersistentPostRun on root does not reliably cover children.
