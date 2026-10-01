---
id: 7c1a9d4e-2f5b-43e8-9a67-1b2c3d4e5f6a
title: Agent config divergences — event names, file shapes, and merge semantics
type: knowledge
status: active
created_at: "2026-10-01T00:00:00Z"
updated_at: "2026-10-01T16:00:00Z"
references: []
category: gotchas
tags: ["configure", "hooks", "mcp", "onboarding", "harnesses"]
---

# Where the six agent configs diverge (src/configure/)

The MCP entry is `{"command":"cogz","args":["mcp-stdio"]}` for most harnesses,
but two require extra fields: **Cursor needs `"type": "stdio"`** (a missing or
Copilot-style `local` type can fail registration silently) and **Copilot needs
`"type": "local"` + `"tools": ["*"]`** (without `tools` nothing is exposed).
Codex writes the entry as TOML instead. Everything else differs per harness.
Verified against official docs during the 0.5.0 audit — the first draft assumed
Claude's shape everywhere and was wrong on three
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

Merge invariants that must hold: semantic dedup makes re-runs idempotent —
a `cogz capture-event <verb>` hook or a wired `mcpServers.cogz` entry is
recognized under any flag order, `matcher:""`-vs-absent, binary path, or
grouping (see `covers`/`hook_key`/`command_key`/`is_cogz_server` in
src/configure/mod.rs); `.cogz.bak` is written before any overwrite; a
corrupt existing file aborts with an error — never clobbered.

## Context-injection coverage (official docs, 0.5.x audit)

`additionalContext`/`additional_context` support per agent:

- Claude: SessionStart, UserPromptSubmit, PostToolUse inject. PostCompact
  CANNOT inject (stdout → debug log) but SessionStart refires on compact
  (`source: "compact"`) — re-injection lands via SessionStart.
- Codex: SessionStart (incl. `compact` matcher source), UserPromptSubmit,
  PostToolUse inject. PostCompact cannot. SessionEnd/Interrupt timeout is
  capped at 3s (1s default) — configure writes `timeout: 3`, consolidation
  is best-effort inside the cap.
- Gemini: SessionStart, BeforeAgent, AfterTool inject. PreCompress is
  advisory-only (systemMessage only) and there is NO post-compress event —
  compaction re-injection impossible. Timeouts are milliseconds.
- Cursor: sessionStart (fire-and-forget) and postToolUse inject
  (`additional_context` snake_case → `--hook-format cursor`).
  beforeSubmitPrompt returns only `continue` — prompt_submit is
  `--fts-only` record-only. preCompact observe-only; no post event.
- Copilot: sessionStart and postToolUse inject (top-level
  `additionalContext` → `--hook-format copilot`). userPromptSubmitted
  drops command-hook output — `--fts-only` correct. PascalCase names
  (`UserPromptSubmit`, `Stop`) are the documented VS-Code-compat aliases
  for `userPromptSubmitted`, `agentStop`; snake_case payloads. No
  compaction or post-edit event.
- Devin: SessionStart, UserPromptSubmit, PostCompaction, PostToolUse all
  inject — full coverage, the reference harness.
