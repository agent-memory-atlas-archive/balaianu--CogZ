---
id: eb8b34a3-733a-4a98-94c0-77b4208acd46
title: "Observed: SetTrue/SetFalse actions make value parsing a no-op"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["c336c219-5cf5-4e78-91ac-853606a5af7d"]
---

Flags with ArgAction::SetTrue never reach the value parser — presence IS the value.
