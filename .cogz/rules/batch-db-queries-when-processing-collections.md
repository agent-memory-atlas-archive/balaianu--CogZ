---
id: ac57f6c8-0669-4062-a9f5-315b419f7c73
title: Batch DB queries when processing collections
type: rule
status: active
created_at: "2026-08-28T19:32:00Z"
updated_at: "2026-08-28T19:32:00Z"
references: []
confidence: 1
validation_count: 3
supporting_ids: []
verified_against: ["2d301065-3f1b-5d81-937e-9972bf96faef=e836c44e68b157bf561636ecdeeafbcb50eaed075370f3bc687439d10ae82144", "3697d6d1-8724-505c-851a-d7cdbec8acf5=fa96e67278ae946fc8b4c323e8080605b7a5d07f27c6af553597b304a05ed2cd", "57ccb59a-d5aa-5055-be72-96185be8a731=9f20bd2176673fa22686fbfd088f551008c6bd1836277bca22453c4957277830", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "aa787435-c08b-584f-8552-0dd0dd8739bd=3bb1d880fa4852de8f3c63e809c93706217899fee77df11df9aafc744f4e0600", "bf2034f7-7971-53f2-8043-ebcb4e48f0b7=b84f02df18e41809232a530cf32c9a6c4d90fbf8138d3253bbaa0e0c4fc0d95a", "c06b1c0c-a87c-5b6e-94eb-e90eedc70aa2=c7e35f5be39504d3729a9bfe89fee0a16690bdaeda11cb55282c5d9e71bd1043", "cd6079a6-fd71-563b-835a-ada3cfbf7138=9467df305000a698bb55265f7f0961378813297ded3e10958ee631d1e3b2a7a1", "d1f079ad-8624-5b7b-80f7-0944eb68e14a=73d49fab4a8e28ee075d08ee9e5f993b1fea9149079d3368a63b77c99532e8ca"]
---

When processing a collection of items that each need a SQL query,
batch them into a single query using `IN (?, ?, ...)` instead of
one query per item.

**Reference implementations:**

- `get_neighbors_batch` — fetches all neighbors for a frontier in
  2 queries (outgoing + incoming), not N queries per frontier node.

- `get_edges_involving_batch` — fetches all edges touching a node
  set in 1 query, returning (source, target, edge_type) triples.

- `build_path_descriptions_batch` — fetches all titles and all
  edge types for multiple paths in 2 queries total.

**Pattern for building placeholders:**

```rust
let placeholders = (0..ids.len())
    .map(|_| "?")
    .collect::<Vec<_>>()
    .join(",");
let params: Vec<&dyn rusqlite::ToSql> =
    ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
let sql = format!("SELECT ... WHERE id IN ({placeholders})");
```

**When params are bound multiple times** (e.g., `source_id IN (...)
OR target_id IN (...)`), chain the params:

```rust
let params: Vec<&dyn rusqlite::ToSql> = ids
    .iter()
    .chain(ids.iter())
    .map(|s| s as &dyn rusqlite::ToSql)
    .collect();
```

Apply this proactively to any new code that loops over a
collection and queries the DB per item.
