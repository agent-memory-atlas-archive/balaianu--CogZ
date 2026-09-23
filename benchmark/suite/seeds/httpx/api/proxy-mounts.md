---
id: f29dff3c-928c-4d09-864c-ba14cb4bbcc0
title: "Per-scheme/domain proxies are configured via mounts, not the proxy arg"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["proxy", "mounts"]
---

Client(mounts={'http://': HTTPTransport(proxy=...), 'https://internal.example.com': None}) routes different URL patterns through different proxies or bypasses them. The mounts dict wins over a single proxy= argument — proxies are just transports carrying a Proxy config.
