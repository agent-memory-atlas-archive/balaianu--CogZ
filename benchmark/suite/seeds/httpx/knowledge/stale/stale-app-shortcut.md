---
id: 8da125f6-2c1e-4f9a-a247-d16f9fadbaca
title: "Client(app=...) shortcut wires an in-process WSGI app"
type: knowledge
status: stale
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: stale
tags: ["stale", "historical"]
---

httpx.Client(app=flask_app) binds the WSGI app directly. [Deprecated 0.27 in favor of transport=httpx.WSGITransport(app=...); removed 0.28.]
