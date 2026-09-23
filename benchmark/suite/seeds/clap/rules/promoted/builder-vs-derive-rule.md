---
id: 29e5e16d-0823-4019-ba60-61ad1c33cd94
title: "Choose builder or derive per command — never mix for the same arg"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

Mixing derive structs with manual builder tweaks on the same command produces duplicate/conflicting arg definitions. Pick one surface per Command subtree.
