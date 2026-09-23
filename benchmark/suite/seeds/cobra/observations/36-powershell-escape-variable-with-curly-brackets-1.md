---
id: a73ff038-6e5c-457b-8e3a-5c495c2fcb61
title: "powershell: escape variable with curly brackets (#1960)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

This fixes an issue with program names that include a dot, in our case (from commit fdee73b4a0; touches powershell_completions.go)
