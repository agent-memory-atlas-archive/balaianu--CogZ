---
id: 96e57f2c-4e8b-4241-8639-ec3c4dac0775
title: "URL pattern mounts pick the transport per request"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-b"]
---

The client's mount table is dict[URLPattern, Transport] — patterns on scheme/host/port route each request; first match wins after sorting.
