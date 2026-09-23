---
id: 0e378618-7ffa-4779-95f3-214958144f5b
title: "Help and version short-circuit before validators"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["help", "gotcha"]
---

Help/Version/HelpRequested actions exit during parse — required-arg validation never runs for `--help`. Custom help flags must use ArgAction::Help to get this short-circuit, not a manual flag.
