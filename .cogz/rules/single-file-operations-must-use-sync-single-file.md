---
id: a9c1d2e3-09cc-414e-831c-8f5710374d4a
title: Single-file operations must use sync_single_file not sync_incremental
type: rule
status: active
created_at: "2026-09-03T12:11:00Z"
updated_at: "2026-09-17T08:16:41.050838088+00:00"
references: []
category: performance
tags: ["sync", "performance", "mcp", "hot-path"]
confidence: 1
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "28355f1f-9b69-53e5-9e6f-9ac8eb768d32=a37d066461e96c730f50c30757074f3bf4186adf26fb99563cb52b3564a6fd30", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "82980fb2-39c2-586a-93c9-bece4eb19b17=3f046218748b66d91c4af7478a6e603f9aec37f61eababd972e7b2aafac77d79", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "8fd942ae-05cd-5ab6-8778-8d0752e77554=b66f3c9515ed1218d874935a7194244993600ed394f6ed57548a867a1cb5d632", "90ee1373-02db-5bd1-8fb4-90243806dbc7=70109705b8680295ac5d917107332fee814984264f14617fe505a44acd5de12a", "976c689b-c6dd-5c27-b493-3cdb9b5be0f2=0bd5162b7b90e664723b267f4226acee2b68549feb6bc9c2d5758a1460acffc0", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "feca2a7c-1249-5d0f-b68f-295e59d141bd=87daebe7a505757b279cf42767603aca0a89297ce90bc38625a5b14e99606747"]
---

When code knows which single file was modified, it must use
`sync_single_file` — not `sync_incremental` (which scans all
entity files) or `sync_all` (which re-syncs everything).

**Applies to:**
- MCP write tools (`write_and_sync`, `update_knowledge_file`)
- `file_save` hook handler
- Any future code that syncs one known file

**Why:** `sync_incremental` reads and parses every `.cogz/` Markdown
file even when only one changed. With 41 files, that's 41x more
I/O than necessary. `sync_single_file` reads only the target file,
syncs the entity row, and calls `sync_references` for frontmatter
edges — O(1) relative to entity count.

**When to use sync_incremental:** `cogz reindex` and `cogz index`,
where scanning all files is the intended behavior (detecting
deletions, discovering new files).
