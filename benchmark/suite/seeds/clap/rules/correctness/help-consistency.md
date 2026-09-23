---
id: 35c3a65e-df91-4bb2-8efc-68340bf25c67
title: "Help/error output changes are user-visible breaking"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["help", "compat"]
confidence: 1.0
---

Help text is a contract — downstream golden tests depend on it. Output changes that create inconsistencies need the minor-release path.
