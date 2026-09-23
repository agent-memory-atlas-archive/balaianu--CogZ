---
id: 1db3948c-0ea5-4320-8cf6-c52d64b1e1ed
title: "HTTP/2 is an optional extra requiring the h2 package"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["http2", "optional"]
---

Client(http2=True) requires the httpx[http2] extra; without it the flag raises ImportError. Negotiation happens via ALPN in the TLS handshake — servers without h2 fall back to HTTP/1.1 silently.
