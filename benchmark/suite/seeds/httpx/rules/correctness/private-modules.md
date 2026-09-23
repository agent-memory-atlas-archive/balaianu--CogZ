---
id: 1a7cdd8f-905a-4925-8746-0fa148513595
title: "New implementation goes in underscore modules"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["api", "structure"]
confidence: 1.0
---

Public names never live in non-underscore modules. Anything new goes to the matching _*.py; only re-export through __init__.py when it is meant as public API.
