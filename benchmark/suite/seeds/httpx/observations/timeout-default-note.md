---
id: afc76729-2953-4c88-8ae9-637da8035d8a
title: "Observed: bare httpx calls use a 5s timeout"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["8f05c692-3d8a-4b44-8490-20a3265dd6a9"]
---

Requests without an explicit timeout raise TimeoutException after 5s of inactivity.
