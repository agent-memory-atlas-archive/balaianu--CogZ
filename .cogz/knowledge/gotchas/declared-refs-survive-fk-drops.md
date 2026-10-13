---
id: 2ab59412-28b2-4a91-944a-b5905d2f94ff
title: Declared references must be read from canonical files — materialized edges silently drop on FK violation
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-10-13T09:48:27.812050360+00:00"
references: ["5dbbedce-8ac5-59a4-b83e-d4685d72a58f", "971025a2-7bd8-57f4-8090-b48135c54d7c", "dc3c0133-d914-5b1c-a324-2cf476724f4b", "bdf67392-ca0a-503e-99e8-a11d9c57c3a6"]
category: gotchas
tags: ["drift", "edges", "foreign-key", "file-first", "sync-ordering"]
verified_against: ["5dbbedce-8ac5-59a4-b83e-d4685d72a58f=v2:4c32b0628d49a9e7f633eb84ecb7f0e0fa34db9d57739e5a9aa1b5b1c6990de6", "971025a2-7bd8-57f4-8090-b48135c54d7c=v2:b106187e040729da49a19f88f338a9a9d1eeff6c339b460c21a0737bc60ca376", "bdf67392-ca0a-503e-99e8-a11d9c57c3a6=v2:6c80376aa0ae2b27485671ff845244080c86217c9d0ac748c1ae960b619a0549", "dc3c0133-d914-5b1c-a324-2cf476724f4b=v2:2c7a2815215a28acb7ceb483e413ebb85026d842181738ac4458efdc96b7d40a"]
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
