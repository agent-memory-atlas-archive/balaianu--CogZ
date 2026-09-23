---
id: 18f9928e-1c17-4f7c-89a5-ecd5a3b65392
title: "Active help must degrade cleanly without env support"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: correctness
tags: ["active-help"]
confidence: 1.0
---

Active help emits via completion machinery; it must no-op when COBRA_ACTIVE_HELP=0 or the shell lacks support — never break completion to deliver hints.
