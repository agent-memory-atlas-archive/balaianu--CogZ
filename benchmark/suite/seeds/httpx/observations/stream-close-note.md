---
id: 6336f7a8-b69d-4ab7-bc9b-25581bc21e31
title: "Observed: streamed responses leak connections if not closed"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["03ce7613-1159-404b-b437-3272268e87fc"]
---

client.stream() responses hold the pooled connection until read+closed.
