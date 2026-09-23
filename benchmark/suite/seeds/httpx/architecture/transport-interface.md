---
id: 5d354324-e410-404a-aadc-61dc1af64d6a
title: "All transports implement handle_request returning a Response"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["transports", "interface"]
---

Sync transports implement handle_request(request) -> Response, async implement handle_async_request. HTTPTransport (httpx/_transports/default.py) wraps httpcore's connection pool; WSGITransport/ASGITransport/MockTransport provide in-process alternatives. Anything satisfying the interface can be mounted — no registry needed.
