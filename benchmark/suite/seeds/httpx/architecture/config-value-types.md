---
id: ae4a2a04-8ad6-4ba6-a667-fe885b44724c
title: "Timeout, Limits, and Proxy are value objects in _config.py"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["config", "types"]
---

Timeout(connect/read/write/pool), Limits(max_connections, max_keepalive_connections, keepalive_expiry), and Proxy(url, auth, headers) are constructed in _config.py. URL, QueryParams, Headers, Cookies live in _urls.py/_models.py as immutable-ish value types used across the whole API.
