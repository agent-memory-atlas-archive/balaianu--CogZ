---
id: aededb8a-2bee-4336-b25a-3d10e512cba1
title: Context pack section priority in src/context/compress.rs follows a strict hie...
type: observation
status: active
created_at: "2026-09-01T12:34:17.378731187+00:00"
updated_at: "2026-09-01T12:34:17.378731187+00:00"
references: []
source: agent
confidence: 0.5
verified_against: ["1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "d0f7d397-05ed-5655-8564-9451ed18812b=b2fd9da73c51d9a7d6e97530dc10c5b7509e317069a5b0582c4291f71b9fafd7"]
---

Context pack section priority in src/context/compress.rs follows a strict hierarchy: identity (0) > rule (1) > observation (2) > knowledge (3) > code (4). When the token budget is tight, rules are always included before observations, and observations before knowledge. This means code entities are dropped first when budget is constrained.