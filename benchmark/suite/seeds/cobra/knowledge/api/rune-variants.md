---
id: a7df4e40-9a5d-4815-ae58-a36732edb23e
title: "E-suffixed hooks return errors; plain variants do not"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["errors", "hooks"]
---

RunE/PreRunE/PostRunE/PersistentPreRunE/PersistentPostRunE return error, propagated to ExecuteC's return. Plain Run/PreRun/etc return nothing. If both are set the E variant wins. Non-E hooks are for commands that handle errors internally.
