---
id: 977218db-a2c1-427c-b563-30b6286aaa72
title: "clap panics on developer error, exits gracefully on user error"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["errors", "debug"]
---

debug_asserts.rs runs build-time validation in debug builds only — invalid Command/Arg configs panic during development (duplicate ids, conflicting settings), while end-user input errors produce Error::exit with formatted output. Release builds skip the debug asserts entirely.
