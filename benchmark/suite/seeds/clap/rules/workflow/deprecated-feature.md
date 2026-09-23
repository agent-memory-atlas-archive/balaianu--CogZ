---
id: 391445e7-aab2-494f-b25f-3d89838eff51
title: "Deprecations ship behind the `deprecated` feature flag"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: workflow
tags: ["api", "releases"]
confidence: 1.0
---

New API + deprecation of old API land as separate commits in the same PR; old items get #[doc(hidden)] plus deprecated-since note with the issue link. The `deprecated` flag lets users migrate on their own timetable.
