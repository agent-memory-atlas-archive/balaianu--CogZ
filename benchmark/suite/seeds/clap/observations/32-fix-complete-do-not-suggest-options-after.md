---
id: d17c9461-2d09-4042-9305-cf5ca4ffcd11
title: "fix(complete): Do not suggest options after '--'"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

This fixes a surprising behaviour I experienced with the new dynamic (from commit 7ffe7399ff; touches clap_complete/src/engine/complete.rs)
