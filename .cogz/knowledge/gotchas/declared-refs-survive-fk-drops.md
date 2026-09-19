---
id: 2ab59412-28b2-4a91-944a-b5905d2f94ff
title: Declared references must be read from canonical files — materialized edges silently drop on FK violation
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-19T18:44:24.266572437+00:00"
references: ["bc587df9-ed9b-5c9f-82db-698153296fbb", "0beb5d53-436a-5886-8a73-bd8f2bf6265b", "5dbbedce-8ac5-59a4-b83e-d4685d72a58f", "87603b8f-7700-5756-a43c-2565e658a854"]
category: gotchas
tags: ["drift", "edges", "foreign-key", "file-first", "sync-ordering"]
verified_against: ["0beb5d53-436a-5886-8a73-bd8f2bf6265b=50170679d98bc7aec13a3bfe64aa761431cdadf88e1380520ad1c611111114a7", "5dbbedce-8ac5-59a4-b83e-d4685d72a58f=1abf1e58e677f692950eb6a1faca0cc6f97dccfe68fafa0450cfab222a2b34dc", "87603b8f-7700-5756-a43c-2565e658a854=8c383401db166e2990ad19a8b1ed1ebd994b4d19449ecb254d5caa4a73a52f85", "bc587df9-ed9b-5c9f-82db-698153296fbb=9e2e76f9ca52cc01629ea4396413170f884342bf7f46080da0cc805f3da033e2"]
---

# Declared references must be read from canonical files

`edges.target_id` has a foreign-key constraint and edge insertion uses
`insert_edge_skip_fk_violation`. When knowledge files sync **before** the
code entities they reference exist (the normal index order), every declared
`references` edge is **silently dropped**. Three downstream breaks:

1. `verified_against` backfill read only materialized edges → declared refs
   stayed unstamped → permanent `unverified` drift rows.
2. `auto_references` edges (derived by name-matching) *were* present → they
   got stamped as provenance — derived edges polluting canonical truth.
3. Missing/deleted targets could never produce drift rows — no edge, no row.

## The fix pattern

`declared_references()` re-parses frontmatter `references` from canonical
files and passes the map into every post-index step (recompute, backfill,
heal, repair). `repair_reference_edges()` re-materializes edges that can now
resolve. Rule: **canonical files are the authority for declared refs; the
edges table is a cache that must be repaired, never trusted.**

Also: `auto_references` must be excluded from anything claiming to be
verification/provenance — they are retrieval hints only.

## Reference meaning depends on target type

Found by dogfooding: a decision doc referencing an earlier (now `stale`)
decision produced `missing` drift that `cogz verify` could never clear —
recompute flagged any non-active target, and verify only stamped active
ones. Now scoped by target type:

- **Stale/absent CODE targets** → `missing`, always. The referenced code is
  gone; the knowledge describes a ghost. Verify stamps the last-known hash
  but the drift stays until the code returns or the ref is removed.
- **Stale knowledge-type targets** → navigational. The doc still exists, so
  the hash check applies: unverified → `unverified` drift, stamped → clean.
  Verify can close these.
