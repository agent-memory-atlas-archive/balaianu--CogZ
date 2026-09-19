---
id: 7e3a2f1b-8c4d-4e5b-9a2f-1d6c3b5a7e8f
title: Code entities incorrectly marked stale by file sync phase
type: observation
status: active
created_at: "2026-08-30T13:45:00Z"
updated_at: "2026-09-04T09:15:30.421727866+00:00"
references: []
source: agent
confidence: 0.9
verified_against: ["1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "989c84cd-c402-539a-9610-88a700241369=996683eb410ad58cab4578337cc9adcd9a4213b1907fadae3f51c8ff3260b48e", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "b63300fd-272f-5464-ade9-ab1d1d0612ea=2e4c0cb5aa06beed9871f1202c542deabce73d7a1cd5eed5db0b96b82a4fad0d", "fe238340-1e76-5003-b9f7-fb6ef71e2a91=189dc6c950e87ee6656104c0f62f5c7207937f87e14eb94862c033c266b57cc0"]
---

The `mark_deleted_as_stale` function in `files/sync.rs` used
`file_path IS NOT NULL` to find file-backed entities. But code
entities (function, class, file, module) also have `file_path` set
to their source file path. This caused all 654 code entities to be
marked stale during the file sync phase of `cogz reindex`, then
reactivated by the code indexing phase.

The fix: filter by entity type (`observation`, `rule`, `knowledge`)
instead of `file_path IS NOT NULL`. Code entities are managed by the
index layer, not the file sync layer.

This was a performance issue (654 unnecessary status updates per
reindex) and produced misleading output ("Stale: 654" during file
sync). The final state was correct because the code indexing phase
reactivated everything, but the intermediate state was wrong.
