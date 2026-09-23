---
id: 7518939f-aee3-4fcf-94bd-ea6073ad24e3
title: "Use the provided Args validators before custom logic"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["args", "api"]
confidence: 1.0
---

args.go has the standard validators (NoArgs, MinimumNArgs, RangeArgs, OnlyValidArgs, MatchAll). Compose them rather than re-validating inside Run.
