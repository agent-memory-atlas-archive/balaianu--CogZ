---
id: b3c4d5e6-f789-4abc-def0-123456789001
title: "Code indexing pipeline — scan, parse, sync, edges"
type: knowledge
status: active
created_at: "2026-08-29T23:20:00Z"
updated_at: "2026-09-21T13:53:58.034346150+00:00"
references: []
category: architecture
tags: ["indexing", "tree-sitter", "code-entities", "phase-8"]
verified_against: ["05d4789c-c12b-5741-a45e-51274dee5b2e=fd9d0708cb270fa766c760e4e3637d78ecbfa37ce428db69f499af54239116e5", "07eb50dc-31a4-5530-be0b-dd10afd9dcc9=5a8f9609d1527ecd9dab99bc771b168e6b4eecdc25f2cbb2db4ca727713ec60f", "111c241f-2c06-527a-b68f-65a313ba673c=0a98b67753c2801a68d78bcee2417c630f07b3071d379fb642dcc22482d1bf47", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "48e6a6bf-5926-5da5-822f-b9160e4a31ba=c90d2d835a56e853d194ffac77be217b67710d1f3e40dfad28e4b31e5d057cc5", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "75c47608-25f8-5a1b-a405-b1a39ab334e2=aa3b5bdf1e78598a153aad11825bff88fbf261a6044b0f900172de1fb4961e30", "83524a92-a730-5dbf-ae96-0153c029add1=24d1ddb27cc77381724e4bacf7b3444758136b3705b3b1368fdcd985d9885c80", "8677ca42-fa21-5c89-bb0a-dba2e79d7766=0dd47f35b18b2db771d4f308351dc6f0000ceb9e479a539eb85bc77a8d574759", "95551ad2-bf6f-57e3-8a95-7caac39fe529=658be9b98247b24c8da7c617ca030084d8c5aeb37bca983ebdf570b3a6c38412", "989c84cd-c402-539a-9610-88a700241369=996683eb410ad58cab4578337cc9adcd9a4213b1907fadae3f51c8ff3260b48e", "b0ded8ae-028d-5861-b051-b97b2bedacee=a35c2cef65161e99c10b99e533a418a63fc9e62195fa221147d154ee26dc89c8", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c8137f86-7816-56e6-88f2-ba27bc14df75=9f8c2825f66a9d563d6ecfc266bcf4870bb5610d85e78e5c92bf786f6989cd92", "d2c359d3-4f00-584b-bf51-223a35866e80=0c25a7ae0055aa33eb2333c912abd6f38e1f85e89f65c0d0d79c5dac48dea507", "dde236a6-bc0a-585b-854b-424e594a65d1=0977e538b28fed82593d92d995afd90ebb7edf4e067f2da3843f7dd189a7a924", "e9837ff8-487d-5a4b-b76b-d7b8cf9ff290=dea18cd35f6f8219b080e56096a315c75d294e4340b169f35d9536d308dc165e", "eb398c8d-d529-50eb-8062-91a947670b69=b8329671e07a1a455710c73dae21fdac11cb52f1a91866375f2a4f5913c11c15"]
---

The code indexing pipeline runs as part of `cogz index` and
`cogz reindex`, after the file sync phase.

## Pipeline stages

1. **Scan** (`index/gitignore.rs`): Walk the repo with the `ignore`
   crate, respecting `.gitignore`. Exclude `.cogz/`. A second pass
   checks `[index].allow` glob patterns and adds matching files even
   if gitignored. Returns relative paths.

2. **Parse** (`index/tree_sitter.rs` + `tree_sitter/python.rs`):
   Tree-sitter parses each source file. Rust and Python are
   supported. Always emits a `file` entity. Extracts functions,
   classes (Rust: structs, enums, traits, impls; Python: classes),
   and modules. Each entity has `file_path`, `line_start`, `line_end`,
   `language`, `qualified_name`, `signature`, and `kind`.

3. **Sync** (`index/sync/mod.rs`): Entities are synchronized to the
   DB with deterministic UUID v5 IDs based on
   `{file_path}:{entity_type}:{qualified_name}`. Content hash change
   detection avoids unnecessary updates. Removed source files →
   entities marked `stale`. `created_at` is preserved on updates.

4. **Edges** (`index/code_graph/mod.rs` + `code_graph/python.rs`):
   Structural edges are built from the AST. Three edge types:
   `calls` (function → function), `imports` (module/file →
   module/file), `extends` (class → class). Name-to-UUID matching
   uses both qualified and simple name lookup.

## Key design decisions

- **Rust impl blocks** get qualified names prefixed with `impl`
  (e.g. `impl Point`, `impl Display for Point`) to avoid UUID
  collisions with the struct entity of the same type name.
- **Code entities are database-only** — no Markdown files on disk.
  They're rebuildable from source.
- **The `ignore` crate requires repository context** for gitignore
  behavior. Tests create a minimal `.git` directory.
- **Module splitting**: `tree_sitter.rs`, `code_graph/mod.rs`, and
  `sync/mod.rs` were split into submodules to stay under the 400-line
  file limit. Python extraction lives in `python.rs` submodules.
