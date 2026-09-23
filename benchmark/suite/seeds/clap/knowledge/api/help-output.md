---
id: 9d90e02b-0245-4654-b28f-1b3548824409
title: "Help/error rendering lives in output/ with styled_str"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["help", "output"]
---

output/: help_template.rs renders help; usage.rs builds usage strings; styled_str.rs handles ANSI styling (feature-gated color). Error display flows through error/format.rs choosing a formatter (rich vs kind-only).
