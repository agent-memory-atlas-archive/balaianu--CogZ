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

# Where the six agent configs diverge (src/configure/)

The MCP payload is identical everywhere (`{"command":"cogz","args":["mcp-stdio"]}`);
everything else differs. Verified against official docs during the 0.5.0 audit
— the first draft assumed Claude's shape everywhere and was wrong on three
harnesses:

- **Codex MCP is TOML** (`config.toml` `[mcp_servers.cogz]`); hooks are
  Claude-shaped JSON in `hooks.json` (PascalCase events incl. `PostCompact`).
  Non-managed hooks are hash-pinned — user must `/hooks`-trust them before
  they run.
- **Gemini puts MCP and hooks in one file** (`settings.json`, `mcpServers` +
  `hooks` keys). Nested matcher-group shape like Claude BUT different event
  names (`BeforeAgent`, `AfterTool`, `AfterAgent`, `PreCompress` — no
  `UserPromptSubmit`/`Stop`) and **`timeout` is milliseconds**, not seconds.
- **Cursor uses flat entries** in `{"version": 1, "hooks": {camelCase:
  [{"command","matcher"?,"timeout"?}]}}` — NOT the Claude matcher-group
  shape. Events: `beforeSubmitPrompt`, `postToolUse`, `afterFileEdit`
  (native — file_save needs no tool matcher), `stop`, `preCompact`.
- **Copilot hook files are flat too** (`{"type":"command","command",…,
  "timeoutSec"}` under `{"version":1,"hooks":{…}}`). `command` is the
  cross-platform fallback copied to `bash`/`powershell`. Event names have
  -ed/-Stop quirks: `userPromptSubmitted`, `agentStop`. No matcher field and
  no post-edit event → `file_save` has no Copilot analog.
- **Devin adds `PostCompaction`** (its PostCompact name) → `session_start`;
  user-scope hooks merge into `config.json`'s `hooks` key alongside
  unrelated config (verified against the real file on the dev machine).

Merge invariants that must hold: element-wise array dedup makes re-runs
idempotent (a cogz hook entry already present is skipped, foreign entries
untouched); `.cogz.bak` is written before any overwrite; a corrupt existing
file aborts with an error — never clobbered.
