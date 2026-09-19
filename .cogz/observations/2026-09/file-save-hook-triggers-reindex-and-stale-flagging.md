---
id: e7fb0c1d-09cc-414e-831c-8f5710374d4a
title: File-save hook triggers reindex and stale flagging automatically
type: observation
status: active
created_at: "2026-09-03T12:07:00Z"
updated_at: "2026-09-05T09:13:27.679308538+00:00"
references: []
source: agent
confidence: 0.95
supporting_ids: []
verified_against: ["05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "4372a2f6-df26-5afa-a409-ea6544806f6e=6bdbf770a9a782e625c7b6676ba4fad89eef1db69f6cd5b200fac0702553e792", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c32f19d9-ee2e-5595-9652-db1b72baaf32=71e03ab52cadc1ba61089d477bd8d51686a9473bc10949238e47193ed96a6be1", "e9837ff8-487d-5a4b-b76b-d7b8cf9ff290=dea18cd35f6f8219b080e56096a315c75d294e4340b169f35d9536d308dc165e"]
---

During the dogfooding run, the `file_save` hook was tested by
simulating a save on `src/update.rs`. The hook correctly:

1. Detected the file as a source file (not under `.cogz/`)
2. Triggered an incremental code reindex via git diff
3. Re-parsed the changed file with tree-sitter
4. Updated 27 code entities and created 8 new ones
5. Flagged 2 knowledge entries as stale (they reference code that
   changed)

The stale flagging works by checking if any knowledge/observation
entity's `references` point to code entities that were updated or
created during the reindex. When a referenced code entity changes,
the knowledge entry is marked stale via a file-first frontmatter
update (status → stale in the `.md` file, then synced to DB).

## What this means for agents

When an agent edits source code, the `file_save` hook automatically:
- Updates the code index (no manual `cogz reindex` needed)
- Flags knowledge that might be outdated
- The next `session_start` or `prompt_submit` hook will show stale
  knowledge in the context pack (with a stale marker), prompting
  the agent to verify or update it

This is the intended workflow: agents edit code, CogZ tracks what
knowledge might be affected, and future sessions are warned about
potentially outdated information.
