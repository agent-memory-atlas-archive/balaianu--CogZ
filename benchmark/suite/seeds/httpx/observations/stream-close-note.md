---
id: 6336f7a8-b69d-4ab7-bc9b-25581bc21e31
title: "Observed: streamed responses leak connections if not closed"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["98d83946-1c90-4a7b-b0ab-b17d8c747692"]
---

client.stream() responses hold the pooled connection until read+closed.
