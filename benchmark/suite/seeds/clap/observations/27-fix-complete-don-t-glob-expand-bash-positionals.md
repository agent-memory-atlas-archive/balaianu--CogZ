---
id: 8c138969-776f-43f3-a2ef-ef28c04aa678
title: "fix(complete): Don't glob-expand bash positionals"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

For a positional arg with no possible_values, the bash generator wrote (from commit dd4997ba2d; touches clap_complete/src/aot/shells/bash.rs)
