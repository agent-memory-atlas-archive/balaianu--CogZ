---
id: 58fdf56a-2cac-4ad9-b9cf-d6cea567bfba
title: "clap has two equivalent APIs: builder and derive"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-a"]
---

clap_builder exposes Command/Arg builders; clap_derive's #[derive(Parser)] compiles to the same builder calls. No semantic difference at runtime.
