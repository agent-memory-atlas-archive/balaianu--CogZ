---
id: dbe38e41-7d8f-4771-86e0-402375bc92d2
title: "Unix sockets and local_address need a direct transport"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["uds", "transport"]
---

HTTPTransport(uds='/var/run/docker.sock') and HTTPTransport(local_address='0.0.0.0') are only available when constructing the transport directly — they are not Client kwargs. Pass the transport via Client(transport=...) to use them.
