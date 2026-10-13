---
id: c2a95f1e-0000-4000-8000-00000000c201
title: "AST semantic hashing (v2) migrated 2145 code entities with zero false drift and zero re-embeds"
type: observation
status: active
created_at: "2026-10-13T00:00:00+00:00"
updated_at: "2026-10-13T00:00:00+00:00"
references: []
source: field-report
confidence: 0.9
---

Item 95 (backlog): code entities now carry `v2:` structural hashes
computed from the tree-sitter AST itself (node kinds + named-leaf
text, comment subtrees skipped) instead of `normalize_content`'s
hand-rolled comment stripper. Whitespace and comments cannot shift
the hash because they never enter the token stream — this also
kills item 23 (the `'a`-lifetime lexer bug) since nothing new is
hashed with the old scheme.

Transition design that made it a no-op upgrade: on sync, a legacy
stored hash is compared against a freshly computed *legacy* hash of
the new content — match means semantically unchanged, so only the
column is rewritten (`hash_migrated` counter), no re-embed and no
FTS churn. On the drift side, legacy `verified_against` baselines
are compared against recomputed legacy hashes of stored content when
the column has already migrated — identical verdicts to the pre-v2
binary. Legacy `normalize_content` survives only inside
`legacy_content_hash` until old baselines drain via re-verify.

Live self-corpus verification: 2145 quiet migrations, 49 real
updates + 14 created (the C2 source edits themselves — correctly
detected as semantic change), 0 stale flags, drift rows stable at 5.
Second index run: 2208 skipped, 0 migrated — fully idempotent.
Stale code entities (63 rows) keep legacy hashes permanently —
dead rows are never re-parsed, which is correct.

Side finding logged as item 104: full `cogz index` re-embeds all
163 file entities every run — `sync_parsed_file`'s hash-skip only
runs in the incremental path; `content_changed` is already computed
and could gate `synced_entity_ids` for embedding.
