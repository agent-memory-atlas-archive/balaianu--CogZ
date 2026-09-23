---
id: 2567c72c-9236-42b7-a2c4-23746ae23dee
title: "Mount patterns dispatch requests to transports"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-a"]
---

self._mounts maps URLPattern to transport; _transport_for_url iterates sorted patterns. Proxies and test transports plug in through the same dict.
