---
id: 4451da44-25f8-467c-9100-b22c62e7c89f
title: "Test commands with SetArgs + buffers, never subprocess"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["testing"]
confidence: 1.0
---

cmd.SetArgs, cmd.SetOut/SetErr(bytes.Buffer), then Execute(). Fresh Command instance per test — flag state persists across Execute calls on the same instance.
