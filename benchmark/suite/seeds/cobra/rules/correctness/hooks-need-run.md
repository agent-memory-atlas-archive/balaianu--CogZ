---
id: 96101179-ac96-4283-95e7-2be5fbadc284
title: "Never ship a hook-only command"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["hooks", "lifecycle"]
confidence: 1.0
---

PreRun/PersistentPreRun only execute when Run/RunE is declared. A command intended to do work must declare Run — otherwise the hooks silently never fire.
