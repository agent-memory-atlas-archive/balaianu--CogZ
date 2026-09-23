---
id: 63881166-24a5-4b40-907d-2619efc49709
title: "Unknown-command suggestions use Levenshtein distance"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["suggestions", "ux"]
---

command.go: when find() fails, cobra suggests 'did you mean' candidates at edit distance <= SuggestionsMinimumDistance (default 2). SuggestFor adds explicit aliases that are suggested but not runnable. ld() implements the distance.
