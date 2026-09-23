---
id: 66babb49-e879-4928-a0f9-394601f76004
title: "clap's error model splits dev config errors from user input errors"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["dup-b"]
---

Builder misconfiguration is caught by debug asserts (dev-time panic); bad argv produces Error::exit with formatted help. Different failure classes by design.
