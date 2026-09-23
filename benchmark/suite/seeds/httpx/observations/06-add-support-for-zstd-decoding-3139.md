---
id: 71997b22-5ece-463d-ad11-67cd0c3ed9a5
title: "Add support for zstd decoding (#3139)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

This adds support for zstd decoding using the python package zstandard. (from commit 392dbe45f0; touches httpx/_compat.py, httpx/_decoders.py, httpx/_models.py)
