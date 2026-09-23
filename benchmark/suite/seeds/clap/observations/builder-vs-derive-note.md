---
id: 72a676ab-895c-41cd-a05b-2ff47a801a28
title: "Observed: derive attrs compile to identical builder calls"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["29e5e16d-0823-4019-ba60-61ad1c33cd94"]
---

Both surfaces produce the same Command/Arg graph — #[arg(long)] == Arg::new(..).long(true).
