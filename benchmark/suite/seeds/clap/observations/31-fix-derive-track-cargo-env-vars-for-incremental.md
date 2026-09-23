---
id: bb6f2c8e-9dff-4282-9502-ea523cbfc308
title: "fix(derive): Track Cargo env vars for incremental rebuilds"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

clap_derive reads CARGO_PKG_NAME, CARGO_PKG_AUTHORS, CARGO_PKG_VERSION, (from commit 34ef8a02f7; touches clap_derive/src/derives/parser.rs, clap_derive/src/item.rs)
