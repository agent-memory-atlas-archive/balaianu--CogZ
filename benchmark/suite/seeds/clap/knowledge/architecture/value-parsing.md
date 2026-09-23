---
id: 7d858e72-1073-4b13-b409-c93a4c093177
title: "ValueParser converts raw OsStr into typed values"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["values", "parsing"]
---

builder/value_parser.rs: value_parser! macro resolves types via autoref specialization; typed parsers (RangedI64ValueParser, PathBufValueParser, etc.) validate at parse time. PossibleValue + value_hint (value_hint.rs) drive completions and validation of fixed choices.
