---
id: 0d0d4879-a5fe-46d3-a07b-a0075fff8f13
title: "Verify with make test-full, make clippy-full, make doc"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: testing
tags: ["testing", "ci"]
confidence: 1.0
---

The Makefile trio is the CI gate. cargo test alone misses feature-combination and UI tests.
