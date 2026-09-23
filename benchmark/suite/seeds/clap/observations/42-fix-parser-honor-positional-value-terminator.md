---
id: d68c2530-6301-496c-b256-e2ad25cfd6ea
title: "fix(parser): Honor positional value_terminator"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

The value terminator should be able to consume -- (from commit 3604b13117; touches clap_builder/src/parser/parser.rs)
