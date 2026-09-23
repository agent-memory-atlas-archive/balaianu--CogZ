---
id: 4c78fba4-3693-422e-b627-16dc6f7a674e
title: "fix(complete): Two-pass quote fish env-completer"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Fish parses `--arguments` once at the outer level, then again when (from commit 49a05cdc99; touches clap_complete/src/env/shells.rs)
