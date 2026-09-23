---
id: f1321786-beb6-4979-8d95-4502ee49f398
title: "Verification is scripts/check + scripts/test"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["testing", "ci"]
confidence: 1.0
---

Always run ./scripts/check (lint+typecheck) and ./scripts/test before claiming a change works. scripts/coverage enforces 100% coverage — new code without tests fails CI.
