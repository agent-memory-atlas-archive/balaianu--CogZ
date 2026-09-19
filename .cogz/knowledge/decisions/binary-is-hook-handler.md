---
id: b2c3d4e5-6f7a-4b8c-9d0e-1f2a3b4c5d6e
title: "Binary is the hook handler, no wrapper scripts"
type: knowledge
status: active
created_at: "2026-09-02T16:05:00Z"
updated_at: "2026-09-02T16:05:00Z"
references: []
category: decisions
tags: ["hooks", "architecture", "agent-agnostic"]
verified_against: ["25681389-1cf6-5c17-9507-5644adbb6410=3eb1c41f3a2140157b25a5d19c2c9168cff017810ae495d57de1399287cbf9c0", "6418285a-aa22-5b7a-ba4b-c68c49fd6aaa=4a09a3779b76dde9a5813dc8408095de6c7b8b548b2d9ee645f306fa0b3a5b20", "664079d1-c6e9-56cd-94aa-19306f85bdac=daef876a0091b046636e8d9573a5c9d487f48f49da1d58ad2179ae1126035e3b", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "da11d218-6d48-589e-8c36-5abebe6bb7d8=51191e04bd84309046660216011fcd5e9b30f784bb9bc3b365e9a54eae730099"]
---

CogZ's `capture-event` command is the hook handler itself. No wrapper
shell scripts are shipped or installed. The binary handles:

1. Repo discovery (cwd or `--repo`)
2. Silent skip when no `.cogz/` exists (`--hook-json` mode)
3. stdin parsing for agent hook payloads (`--hook-json` mode)
4. JSON output wrapping (`{"hookSpecificOutput": {...}}`)
5. FTS-only fast path (`--fts-only` skips ONNX model loading)

This keeps CogZ agent-agnostic. The docs show how to wire `cogz
capture-event` into any agent's hook config. No scripts to copy, no
paths to customize, no files to embed in the binary.

The `--fts-only` flag is critical for hook use: without it, each hook
call loads the ONNX runtime (~40s). With it, hook calls complete in
~0.5s using FTS-only search. The MCP server (persistent process) is
the preferred path when vector search is needed.
