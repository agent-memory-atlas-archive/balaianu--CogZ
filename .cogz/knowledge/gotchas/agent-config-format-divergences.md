---
id: 7c1a9d4e-2f5b-43e8-9a67-1b2c3d4e5f6a
title: Agent config divergences — event names, file shapes, and merge semantics
type: knowledge
status: active
created_at: "2026-10-01T00:00:00Z"
updated_at: "2026-10-01T00:00:00Z"
references: []
category: gotchas
tags: ["configure", "hooks", "mcp", "onboarding", "harnesses"]
---

# Where the six agent configs diverge (src/configure.rs)

The MCP payload is identical everywhere (`{"command":"cogz","args":["mcp-stdio"]}`);
everything else differs:

- **Codex MCP is TOML** (`config.toml` `[mcp_servers.cogz]`); its hooks are
  still JSON. TOML merge is table-merge + leaf-override — comments in the
  user's existing file are lost on rewrite (data preserved).
- **Gemini puts MCP and hooks in one file** (`settings.json`, `mcpServers` +
  `hooks` keys) and fires `AfterTool` where Claude Code fires `PostToolUse`.
- **Cursor/Copilot use camelCase events** (`sessionStart`, `postToolUse`…).
- **Copilot hooks are per-set files** (`.github/hooks/cogz.json`) with a
  `version` field — merge adds the file, not a key inside a shared file.
- **Devin adds `PostCompaction`** → `session_start` so context survives
  compaction; its user-scope hooks merge into `config.json`'s `hooks` key
  alongside unrelated config (the merge preserves foreign keys — verified
  against the real file on the dev machine).

Merge invariants that must hold: element-wise array dedup makes re-runs
idempotent (a cogz hook entry already present is skipped, foreign entries
untouched); `.cogz.bak` is written before any overwrite; a corrupt existing
file aborts with an error — never clobbered.
