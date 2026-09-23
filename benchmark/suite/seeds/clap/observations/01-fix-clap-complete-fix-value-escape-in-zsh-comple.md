---
id: fc3c9e75-f3e5-408a-8716-f75b86fb71ab
title: "fix(clap_complete): Fix value escape in zsh completion"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Currently, if there's no help set in the possible values, the value (from commit a1b6be720e; touches clap_complete/src/aot/shells/zsh.rs)
