---
id: 03ce7613-1159-404b-b437-3272268e87fc
title: "Streaming responses must be closed or fully consumed"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

Use `with client.stream(...)` or await response.aclose()/aread() — an unclosed stream starves the connection pool.
