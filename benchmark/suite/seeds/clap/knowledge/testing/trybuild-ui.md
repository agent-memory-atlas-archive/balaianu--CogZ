---
id: 12aa901e-0902-4c05-92ee-651ce6d938e5
title: "Derive macro changes need trybuild UI tests"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["testing", "derive"]
confidence: 1.0
---

clap_derive compile-fail cases live under tests/derive_ui*; new attributes or validation must add a .rs/.stderr pair and bless with TRYBUILD=overwrite.
