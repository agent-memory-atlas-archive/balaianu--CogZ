---
id: afc76729-2953-4c88-8ae9-637da8035d8a
title: "Observed: bare httpx calls use a 5s timeout"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["dcb5b84a-3423-41e5-a146-cc12739b8f30"]
---

Requests without an explicit timeout raise TimeoutException after 5s of inactivity.
