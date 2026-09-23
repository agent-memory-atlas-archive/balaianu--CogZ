---
id: 4b011f9e-41ba-4034-9ce2-02585ad401c0
title: "Derive macros lower to builder calls"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["derive", "macros"]
---

clap_derive parses struct/enum attrs (attr.rs, item.rs) and emits clap_builder code — there is no separate derive runtime. #[command]/#[arg] attributes map onto Command/Arg builder methods; dummies.rs generates stub impls so user code compiles even when macro expansion fails.
