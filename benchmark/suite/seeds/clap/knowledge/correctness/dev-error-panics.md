---
id: acd1065b-b769-47bd-9f93-d31925b1b076
title: "Developer errors must panic, user errors must exit gracefully"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["errors", "convention"]
confidence: 1.0
---

Never surface a builder misconfiguration as a runtime user-facing error — debug_asserts catch it in dev. Conversely never panic on bad user input; construct a styled Error and exit. (clap CONTRIBUTING convention.)
