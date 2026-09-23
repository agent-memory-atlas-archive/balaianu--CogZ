---
id: da5fe68a-1967-407b-96ee-ee985e28a636
title: "clap is a workspace: builder, derive, lex, complete, mangen"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["workspace", "architecture"]
---

clap_builder is the core (Command, Arg, parser). clap_derive provides the #[derive(Parser)] proc-macros that emit builder calls. clap_lex is the low-level tokenizer. clap_complete and clap_mangen are output generators (shell completions, man pages). The top-level src/lib.rs is a thin facade re-exporting clap_builder.
