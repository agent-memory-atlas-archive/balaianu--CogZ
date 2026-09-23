---
id: 1fc0a961-1b8a-42f3-9a28-12bda648a8e6
title: "Test commands via SetArgs + SetOut/SetErr buffers"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["testing", "gotcha"]
---

The idiom: cmd.SetArgs([]string{...}), cmd.SetOut(buf), cmd.SetErr(buf), then Execute(). Works without subprocess; assert on buf contents and the returned error. Remember a fresh command instance per test — flag state persists across Execute calls.
