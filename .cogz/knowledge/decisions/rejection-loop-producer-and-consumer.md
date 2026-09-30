---
id: 9f1e2d3c-4b5a-4678-9a01-2e3f4a5b6c7d
title: Rejection loop — producer writes file-first, consumer warns never blocks
type: knowledge
status: active
created_at: "2026-09-30T00:00:00Z"
updated_at: "2026-09-30T16:10:00Z"
references: []
category: decisions
tags: ["rejection", "status-lattice", "dedup", "epistemic-loop"]
---

# The `rejected` loop has two halves — both shipped 2026-09-30

**Producer** (`8b50b70`+`5924430`): `files/reject.rs::reject_entity_file` is the
only writer of `status: rejected`. Both surfaces (`reject_entity` MCP,
`cogz reject` CLI) go through it. Two orderings matter:

- The lattice check (`transition_status`) runs **before** the file is
  touched, against the DB status — the sync layer would catch the same
  illegal transition but only *after* the canonical file changed, leaving
  file/DB divergence. Pre-check keeps failures atomic.
- `rejected_reason` is written into frontmatter, and sync lands arbitrary
  frontmatter keys into `entities.properties` — which is how the dedup
  pass reads the reason back without a schema change.

**Consumer**: `check_duplicate` keeps `status='active'` as the duplicate
set (a claim matching a rejected one is *not* a duplicate — the world
changes, re-recording must stay legal) and runs a **second pass** over
`status='rejected'` with identical title+embedding gates, returning
`rejected_match` as a warning. Warning-not-block is deliberate: the
verdict is evidence for the writer, not a veto.

**67 (stale handle)** piggybacks the same commit arc: `RepoState` pins
`dev+ino+btime` at open and `RepoCache::get` re-stats per hit (~2.4µs
measured). Birth time is what defeats the inode-reuse corner after
reset+reindex; dev+ino alone can collide.
