---
id: 664577e8-a932-4fcc-88f3-6669fb6e6b0f
title: reindex_single_file must relativize hook-supplied absolute paths — ghost UUIDv5 entities otherwise
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-10-13T09:46:12.078091130+00:00"
references: ["aa751672-ca20-57c4-b509-08a29e3b40d4", "a0f5b27b-3ea4-5ef7-ba6f-69b7a9c08443", "ce88f80f-708c-508d-b671-f8a65b9bb9d4"]
category: gotchas
tags: ["indexing", "uuid5", "hooks", "ghost-entities", "path-normalization"]
verified_against: ["a0f5b27b-3ea4-5ef7-ba6f-69b7a9c08443=v2:e6a40fd8195e05443fbb8f206f5edc542318c5837a37fb0a73e1e8d275cd8585", "aa751672-ca20-57c4-b509-08a29e3b40d4=v2:79e9ff149638f95b586457f4bb28f69f029d48ddce4bb6b81c8ec37873d5bd56", "ce88f80f-708c-508d-b671-f8a65b9bb9d4=v2:c5e510f6c5d1e54f22fb4eeb59cb449d66a623e3dc046242f337a169f118f0a5"]
---

# Hook-supplied paths are absolute — relativize before keying

File-save hooks pass the absolute `file_path`. `reindex_single_file` used to
feed it straight into entity extraction, so every entity in the saved file
was minted **twice**: once under the repo-relative key (full index) and once
under the absolute-path key (hook reindex). UUIDv5 input is
`{file_path}:{type}:{qualified_name}` — different path string, different UUID.

Found in the wild: 208 ghost entities in a replay worktree — `bridge.py` had
74 real + 74 ghost rows, both active, doubling that file's retrieval surface.

## The fix

`reindex_single_file` now canonicalizes `repo_root`, strips it from absolute
inputs, and runs `components()` normalization on the relative branch too —
`./src/foo.py` must land on `src/foo.py`'s UUID (`components()` preserves a
leading `CurDir`, so strip it explicitly). Regression tests cover both the
absolute and `./`-prefixed cases.

Any other path entering `code_entity_uuid` — from hooks, CLI args, or test
fixtures — needs the same treatment. `sync_single_file` (the `.cogz/` path)
was already safe: it canonicalizes before computing the entity key.
