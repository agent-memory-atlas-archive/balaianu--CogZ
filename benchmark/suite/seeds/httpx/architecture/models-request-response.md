---
id: 9f136015-927e-429c-afe0-57fa24d5415e
title: "Request/Response models live in _models.py with lazy content"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["models", "streaming"]
---

Response.content is only populated after .read() (or .aread()); for streaming responses use client.stream() and iterate iter_bytes/iter_text/iter_lines. Request builds headers at construction; Response ties back to its request via response.request.
