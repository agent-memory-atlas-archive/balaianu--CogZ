---
id: e4c5470b-63f7-40a6-8a57-81b11da0c0dd
title: "feat(derive): Defer enum subcommand initialization"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

Add an explicit boolean defer attribute on Parser and Subcommand enums. (from commit 78d70f3c68; touches clap_derive/src/attr.rs, clap_derive/src/derives/subcommand.rs, clap_derive/src/item.rs, src/_derive/mod.rs)
