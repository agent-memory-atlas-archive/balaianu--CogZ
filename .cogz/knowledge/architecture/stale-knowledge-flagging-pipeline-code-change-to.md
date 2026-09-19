---
id: 1d847b1e-6fd4-491d-86eb-bdbcdb7be8c7
title: Stale-knowledge flagging pipeline — code change to frontmatter status
type: knowledge
status: active
created_at: "2026-09-13T21:42:35.374039918+00:00"
updated_at: "2026-09-19T18:44:22.361541598+00:00"
references: ["eeeb377b-1fab-5129-b252-f88d01e403c7", "8c379ddd-7139-527a-8c34-5ed0539742ca", "90ee1373-02db-5bd1-8fb4-90243806dbc7"]
category: architecture
tags: ["architecture", "stale-flagging", "lifecycle", "file-first"]
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "524f7d8f-9d30-5a5d-ab62-6a4472fa555b=7fca08c8e9e2fad1c942dba41ea47264f0f8bca7d23c047ec39f3d45ac726ca6", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "68efa752-26c9-5772-a815-38d65cd1bd9e=76bc07cd7c9ddf3fd059e043b4148232c201e37708ebf3df21b7a782f082de2b", "705cd1ba-1460-5c33-9b59-2081d04d2a3e=6bc53ca55f05a53dc627fec9b34c9841c57053c48d8041535143f380893723d4", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "8c379ddd-7139-527a-8c34-5ed0539742ca=5739536919f7f902e396af02f7e94e467a5501d92b82aa4e513676f816696594", "90ee1373-02db-5bd1-8fb4-90243806dbc7=70109705b8680295ac5d917107332fee814984264f14617fe505a44acd5de12a", "999f4b68-6f3c-500c-98ef-625382e99439=af0f10373be56700a804659a9688c5494dca78d3c9b575ba7a68406061b1dc23", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "d1f079ad-8624-5b7b-80f7-0944eb68e14a=73d49fab4a8e28ee075d08ee9e5f993b1fea9149079d3368a63b77c99532e8ca", "eeeb377b-1fab-5129-b252-f88d01e403c7=c5fd6b88404ae2dfb7f8373ef2b21202d77b479fd3c045da5b2a3c393131e0e8"]
---

# Stale-knowledge flagging pipeline

When code entities change (reindex, file_save hook), `flag_stale_knowledge` marks knowledge-layer entities that reference them as `status: stale`.

Flow:
1. `get_edges_involving_batch(changed_code_ids)` → find sources with `references` OR `auto_references` edges pointing at changed code. Both edge types covered — auto-linked knowledge flags too.
2. Batch-fetch referencing entities, filter to type in {observation, rule, knowledge} AND status='active'.
3. Acquire `file_lock()` for the whole RMW batch.
4. Per entity: read canonical file → set frontmatter `status: stale` + `updated_at` → write file → validate via `transition_status` (illegal transition → revert file write) → `sync_single_file` re-derives DB row (status, hash, edges) from the file.
5. One `code_changed` event records all affected IDs.

Non-obvious constraints:
- File-first is literal: the frontmatter is flipped on disk BEFORE the DB is touched, and the DB row is re-derived from the file — not updated directly. This is what keeps file and DB from drifting.
- Deleted-file stale entities are transitional: `cogz reset` + `cogz index` does NOT recreate them — no canonical file exists. Surviving files' `references` frontmatter is the historical record; graph expansion skips dangling refs.
- Failed-file protection: files that fail to read/parse are excluded from stale-marking input — a parse error is not a deletion.
- Review workflow gap: entities flagged stale accumulate; `stale → active` requires manual review and frontmatter edit. No `doctor --review-stale` yet (backlog item 2).