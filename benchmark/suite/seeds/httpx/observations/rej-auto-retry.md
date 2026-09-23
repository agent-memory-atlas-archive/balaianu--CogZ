---
id: 67c0390e-0d3f-4b14-9d22-ca2ba8a9078e
title: "httpx retries failed requests automatically"
type: observation
status: rejected
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
---

Observation: believed failed requests retry once by default. REJECTED: no retries unless HTTPTransport(retries=N) is explicitly constructed, and then only for connect failures.
