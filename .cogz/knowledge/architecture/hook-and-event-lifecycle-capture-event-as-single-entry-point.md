---
id: fafa51e8-83a0-41cf-896d-78ac2f4ad720
title: Hook and event lifecycle — capture-event as single entry point
type: knowledge
status: stale
created_at: "2026-09-13T21:42:37.625349447+00:00"
updated_at: "2026-09-15T16:50:47.735614872+00:00"
references: ["59203bdf-b181-50bc-94c2-eb5e755667c7", "2f2833b0-4956-5683-8c29-b5b06ede478c"]
category: architecture
tags: ["architecture", "hooks", "events", "lifecycle"]
---

# Hook and event lifecycle

`cogz capture-event <type>` is the single CLI entry point for all agent lifecycle events — the binary IS the hook handler (no wrapper scripts; see decisions knowledge). `run_capture_event` dispatches to `handle_lifecycle_event`.

**Input paths:** fields come from CLI args first; with `--hook-json` the remainder is parsed from stdin JSON (Claude Code / Devin hook payload shape: `prompt`, `tool_name`, `tool_response.output|error`, `tool_input.file_path`). Stdin capped at 1MB — larger tool outputs are truncated but the event still records. Remaining bytes are drained via `io::sink()` so the pipe doesn't break.

**Per-event behavior:**
- `session_start` / `prompt_submit` → assemble context pack → printed to stdout (or wrapped as `hookSpecificOutput.additionalContext` with --hook-json).
- `pre_tool_use` / `post_tool_use` → event recorded only (audit trail). The agent decides salience via `record_observation` — deliberate design: observation judgment is the agent's job, not the hook's.
- `file_save` → incremental code reindex + stale-knowledge flagging.
- `session_end` → consolidation (promotion + merge) runs, counts reported.

**Silent-skip contract:** with `--hook-json`, missing `.cogz/` or missing DB prints `{}` and exits 0 — hooks must never break the agent loop. Without the flag, same conditions are hard errors (CLI users need the message).

**Latency escape hatch:** `--fts-only` skips ONNX model creation entirely — ~2s vs ~40s. Exists because synchronous hooks felt the 40s model-load pain.

**post_tool_use volume:** these events store full tool output and dominate the events table (~63% of rows). Retention pruning is backlog item 3.