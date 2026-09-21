---
id: b8c2d3e4-09cc-414e-831c-8f5710374d4a
title: Single-file sync must include reference edge synchronization
type: knowledge
status: active
created_at: "2026-09-03T11:56:00Z"
updated_at: "2026-09-21T13:53:57.742969263+00:00"
references: []
category: gotchas
tags: ["sync", "references", "file-save", "edges"]
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "524f7d8f-9d30-5a5d-ab62-6a4472fa555b=7fca08c8e9e2fad1c942dba41ea47264f0f8bca7d23c047ec39f3d45ac726ca6", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "82980fb2-39c2-586a-93c9-bece4eb19b17=3f046218748b66d91c4af7478a6e603f9aec37f61eababd972e7b2aafac77d79", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "90ee1373-02db-5bd1-8fb4-90243806dbc7=70109705b8680295ac5d917107332fee814984264f14617fe505a44acd5de12a", "feca2a7c-1249-5d0f-b68f-295e59d141bd=87daebe7a505757b279cf42767603aca0a89297ce90bc38625a5b14e99606747"]
---

# Single-file sync must include reference edge synchronization

`sync_single_file` was created for the `file_save` hook and for MCP
write tools. It syncs the entity row (INSERT/UPDATE) but originally
did not call `sync_references` — the function that re-syncs
frontmatter-derived graph edges (`references`, `supports`,
`contradicts`, `derived_from`).

This meant editing a knowledge file's frontmatter references via a
hook-triggered sync would update the entity content but not the graph
edges. Graph expansion through those edges would miss the updated
connections.

## The fix

`sync_single_file` now calls `sync_references` after a successful
non-skipped entity sync. This ensures all four frontmatter-derived
edge types are updated on single-file changes.

## When this matters

- `file_save` hook: editing a `.cogz/` file's frontmatter references
- MCP write tools: `create_entity` (all three types)
  with `references` in the frontmatter
- `update_knowledge`: changing the `references` field

## When it doesn't matter

Code entities (function, class, file, module) don't have
frontmatter — their edges come from tree-sitter analysis, not from
`sync_references`. The fix only affects file-backed entities
(observation, rule, knowledge).
