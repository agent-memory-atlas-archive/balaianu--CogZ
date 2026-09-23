---
id: 4b6657ec-5479-45e5-8bf0-73a8e91c5930
title: "refactor(builder): Rename internal blacklist field to conflicts"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

`Arg::blacklist` holds the ids an argument conflicts with. `conflicts` (from commit 14d47fb7e5; touches clap_builder/src/builder/arg.rs, clap_builder/src/builder/command.rs, clap_builder/src/builder/debug_asserts.rs, clap_builder/src/parser/validator.rs)
