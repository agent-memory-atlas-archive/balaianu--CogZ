---
id: 78fe7be0-c777-4c7b-afab-1d378f16daeb
title: Files are canonical — DB is disposable and derived
type: rule
status: stale
created_at: "2026-08-28T19:34:00Z"
updated_at: "2026-09-18T21:27:34.897038209+00:00"
references: []
confidence: 1
validation_count: 2
supporting_ids: []
stale_reason: code_orphaned
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "989c84cd-c402-539a-9610-88a700241369=996683eb410ad58cab4578337cc9adcd9a4213b1907fadae3f51c8ff3260b48e", "999f4b68-6f3c-500c-98ef-625382e99439=af0f10373be56700a804659a9688c5494dca78d3c9b575ba7a68406061b1dc23", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c8137f86-7816-56e6-88f2-ba27bc14df75=9f8c2825f66a9d563d6ecfc266bcf4870bb5610d85e78e5c92bf786f6989cd92"]
---

Every write writes the file first, then syncs the DB. The DB is
disposable — `cogz reset` + `cogz index` must rebuild everything
from files alone.

**What this means in practice:**

- `update_knowledge` (Phase 7) overwrites the file, then re-syncs.
  It does not UPDATE the DB directly.
- Observations are append-only — no `update_observation` tool
  exists or will be added.
- Status changes edit frontmatter in-place, then sync. The status
  state machine (`transition_status`) validates transitions.
- Deleted files → DB entity marked `stale`, not deleted. Edges
  preserved.

**The rebuildability test:** After any change, `cogz reset` +
`cogz index` must produce the same DB state. If it doesn't, the
change broke the invariant.

**What code entities (Phase 8) add:** Code entities have no file
on disk — they're extracted from source by tree-sitter. They're
rebuildable from source code, not from `.cogz/` files. The
principle holds: the source of truth (source files) is canonical,
the DB is derived. Code entities use deterministic UUID v5 IDs
(`{file_path}:{entity_type}:{qualified_name}`) so the same source
produces the same entities across rebuilds.
