---
id: c336c219-5cf5-4e78-91ac-853606a5af7d
title: "Boolean flags must use SetTrue/SetFalse, not value parsing"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

A bool flag built with value_parser reads the NEXT arg as its value — use ArgAction::SetTrue so `--flag` stands alone.
