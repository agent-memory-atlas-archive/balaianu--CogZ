---
id: d494d63a-05f4-41fc-91a8-c56b5e525107
title: Why CogZ has a custom frontmatter parser instead of a YAML crate
type: knowledge
status: active
created_at: "2026-08-28T19:26:00Z"
updated_at: "2026-08-28T19:26:00Z"
references: []
category: decisions
tags: ["frontmatter", "yaml", "dependencies", "parser"]
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "6726eaee-0f96-5dff-b74d-1ca86eb1aee3=d9233415fa46854a12a0dc38ee5ebf84fffca39843619e72838051ae6950a790", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "8b994b90-d030-5c71-90b3-1366e90088a3=d899ee2cab66f2f5775d1d12d561fd3231f77278926a83d353b42b737e5f76c2", "bbb2c3b6-55dc-57ec-9c42-52100c2308e1=3fe159d042200cead5a5af7cfa7c09386c9f9bfa2b120f51ae62e3f988adaac5"]
---

`src/files/frontmatter.rs` is a hand-rolled YAML subset parser
(339 lines). It handles only: scalar key-value pairs, inline string
arrays, and basic types (string, int, float, bool).

**Why not `serde_yaml` or `yaml-rust2`:**

1. The frontmatter format is intentionally constrained — no nested
   mappings, no anchors, no multi-line strings. A full YAML parser
   is overkill for what is effectively `key: value` pairs.

2. Fewer dependencies. CogZ already has 15+ crates. Each one adds
   build time, binary size, and supply chain surface.

3. The parser preserves key insertion order (via `Vec<(String,
   FmValue)>`), which matters for stable file serialization. Most
   YAML parsers use `HashMap` or `BTreeMap` internally, losing
   order.

**Known limitations:**

- No multi-line strings (block scalars `|` and `>`)
- No nested mappings
- No anchors or aliases
- Inline arrays only (`["a", "b"]`), not block arrays

These are by design. If entity files ever need nested structures,
the `properties` JSON field in the DB schema is the escape hatch —
complex data goes there, not in frontmatter.
