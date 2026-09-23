---
id: a323b447-6810-4be1-a85e-82a754f12e1d
title: "Requests are routed to transports by URL pattern mounts"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["routing", "transports", "client"]
---

Client._mounts is dict[URLPattern, Transport], sorted on construction in _client.py. _transport_for_url walks the mount patterns to pick the transport for each request — scheme, host, and port prefixes can each route to different transports. This is how proxies, custom per-domain transports, and test transports all share one dispatch mechanism.
