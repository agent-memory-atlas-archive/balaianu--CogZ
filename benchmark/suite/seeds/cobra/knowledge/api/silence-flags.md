---
id: 5f9ac011-e857-4a68-b74b-e3b69db9fda2
title: "SilenceUsage and SilenceErrors control post-error output"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["errors", "ux"]
---

Command.SilenceUsage suppresses usage printing on error; SilenceErrors suppresses the error echo. Common pattern: SilenceUsage=true once execution has started (usage spam only makes sense for parse errors). Root-level silence gives the app full control of error formatting.
