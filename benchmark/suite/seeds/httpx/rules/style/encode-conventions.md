---
id: daf5a79f-a7a4-4203-afbb-549bbd402b14
title: "Follow encode style: ruff lint, explicit exports"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: style
tags: ["style", "lint"]
confidence: 1.0
---

scripts/check runs ruff + mypy. __init__.py re-exports are explicit — adding a public name means updating __all__-equivalent imports AND the api docs page.
