---
id: 72a676ab-895c-41cd-a05b-2ff47a801a28
title: "Observed: derive attrs compile to identical builder calls"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["f1296ce7-73d6-4de4-b6ca-003e0f5f4eb2"]
---

Both surfaces produce the same Command/Arg graph — #[arg(long)] == Arg::new(..).long(true).
