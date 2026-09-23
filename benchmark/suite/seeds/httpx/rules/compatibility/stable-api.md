---
id: ffa00c11-7896-4ec2-bfae-d9e8fd493c21
title: "1.0 API is stable — no breaking changes to public surface"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: compatibility
tags: ["api", "compat"]
confidence: 1.0
---

httpx is past 1.0: the re-exported public API (Client, AsyncClient, models, exceptions) cannot break. Changes go behind new parameters or private modules; deprecate before removing.
