---
id: 8c536f37-d2da-423f-8034-f9cde54b3736
title: "Parsing runs clap_lex tokenize -> parser -> arg_matcher -> validator"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["parser", "pipeline"]
---

clap_lex lexes argv into raw args (os_str handling, short/long/unified); parser/parser.rs drives consumption; arg_matcher.rs records which args matched with what values; validator.rs enforces required/conflicts/groups after matching. Error construction flows through parser/error.rs into the error/ module.
