---
id: a210b0f2-9ff6-450c-b470-3371f0cde32c
title: "CLI-visible changes are breaking for downstream CLIs"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: compatibility
tags: ["compat", "api"]
confidence: 1.0
---

Cobra is a library — renaming a flag or changing help text breaks downstream CLIs' tests. Treat command API and generated output as public contract.
