---
id: 6380289a-e489-43bd-94e7-49be75945035
title: "default_value silently loses to overrides_self and multiples"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["args", "gotcha"]
---

An arg with both a default and .overrides_with_self() or ArgAction::Append behaves unexpectedly — defaults apply when absent but each occurrence appends/overrides per action. num_args + value_delimiter interact with defaults in ways the builder won't warn about in release.
