# Hooks

Hooks capture lifecycle events from an AI coding agent and inject context packs into the agent's session. CogZ's binary is the hook handler — no wrapper scripts needed.

## How it works

When an agent fires a lifecycle event (session start, prompt submit, tool use, file save, etc.), it calls `cogz capture-event` with the event type and optional context. CogZ:

1. Records the event in the database (audit trail).
2. For `session_start` and `prompt_submit`: assembles a context pack and prints it to stdout for the agent to consume as injected context. Also spawns a background reindex process to catch changes from non-hook events (branch switches, pulls, merges, human edits in another terminal).
3. For `file_save`: triggers a single-file code reindex (fast — no git diff) and runs post-index maintenance (flags orphaned knowledge, backfills `verified_against` provenance, recomputes `entity_drift`, heals exact-revert staleness) if the saved file is a source file. Syncs `.cogz/` entity files if the saved file is under `.cogz/`. Also pushes edit-scoped context: active rules that reference entities on the saved path (via `references`/`auto_references` edges) are returned as a compact pack — silent when nothing governs the file.
4. For `session_end`: closes any open usage deliveries (pending entities become misses) and runs consolidation (promotion + merge).

## The `--hook-json` flag

When `--hook-json` is passed, output is wrapped in the JSON format expected by agent hook systems:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "session_start",
    "additionalContext": "...context pack as text..."
  }
}
```

If no `.cogz/` directory is found in the current repo, CogZ prints `{}` and exits silently — the hook is a no-op for repos that don't use CogZ.

## The `--fts-only` flag

The `--fts-only` flag skips ONNX model loading and uses FTS-only search for context assembly. This is critical for hook use:

- **Without `--fts-only`:** each hook call loads the ONNX runtime (~40s on first load, ~2s on subsequent calls with warm models).
- **With `--fts-only`:** hook calls complete in ~0.5s using FTS-only search.

For hooks, speed matters more than ranking quality. The MCP server (persistent process) is the preferred path when vector search is needed — it keeps models loaded across calls.

## Event types

| Event | When | What CogZ does | Output |
|---|---|---|---|
| `session_start` | Agent session begins | Records event, assembles cold_start context pack, spawns background reindex | Context pack (recent rules + observations) |
| `prompt_submit` | User submits a prompt | Records event, resolves usage hits on still-open deliveries (prompt names a delivered entity's file path or title), then assembles task context pack, spawns background reindex (debounced 60s) | Context pack (query-scoped retrieval) |
| `pre_tool_use` | Before a tool call | Records event only | None (audit trail) |
| `post_tool_use` | After a tool call | Records event; marks usage hits on delivered entities (tool touched an entity's file, or output mentioned its id/title) | None |
| `file_save` | A file is saved | Records event, marks usage hits on delivered entities in the saved file, triggers single-file code reindex if source file, runs post-index maintenance (stale flag + provenance backfill + drift recompute + heal), syncs `.cogz/` file if under `.cogz/`, pushes rules governing the saved file when they exist | Reindex summary + scoped rules (when present) |
| `session_end` | Agent session ends | Records event, closes open usage deliveries, runs consolidation (promotion + merge), counts mined observation candidates | Consolidation summary + `suggestion_count` |
| `stop` | Agent stops | Records event only | None |

## CLI usage

```bash
# Session start (from a hook config)
cogz capture-event session_start --hook-json

# Prompt submit (from a hook config, reading prompt from stdin or file)
cogz capture-event prompt_submit --hook-json --prompt "implement auth"

# File save (from a hook config, triggered on edit/write)
cogz capture-event file_save --hook-json --fts-only --file-path src/auth.rs

# Session end (from a hook config)
cogz capture-event session_end --hook-json --fts-only

# Manual (no hook JSON, plain text output)
cogz capture-event session_start --repo ~/my-project
```

## Full hook configuration

This is the canonical hook config. Copy it into your agent's hook configuration file — see [Agent Setup](agent-setup.md) for the file path and any agent-specific differences (event names, matchers, supported events).

```json
{
  "hooks": {
    "SessionStart": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "cogz capture-event session_start --hook-json",
        "timeout": 15
      }]
    }],
    "UserPromptSubmit": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "cogz capture-event prompt_submit --hook-json",
        "timeout": 15
      }]
    }],
    "PostToolUse": [
      {
        "matcher": "",
        "hooks": [{
          "type": "command",
          "command": "cogz capture-event post_tool_use --hook-json --fts-only",
          "timeout": 10
        }]
      },
      {
        "matcher": "edit|write|notebook_edit",
        "hooks": [{
          "type": "command",
          "command": "cogz capture-event file_save --hook-json --fts-only",
          "timeout": 20
        }]
      }
    ],
    "SessionEnd": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "cogz capture-event session_end --hook-json --fts-only",
        "timeout": 30
      }]
    }],
    "Stop": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "cogz capture-event stop --hook-json --fts-only",
        "timeout": 5
      }]
    }]
  }
}
```

**Timeouts:** `session_start` and `prompt_submit` need 15s — context assembly is hybrid when models are installed (model load + query embedding adds a few seconds; background reindex is detached and doesn't block). Record-only events keep `--fts-only` because they never assemble a pack. `file_save` needs 20s (single-file reindex). `session_end` needs 30s (consolidation). `stop` needs 5s (event recording only).

**`--fts-only`:** forces lexical-only mode even when models are installed — useful for pack-producing events on constrained machines, at the cost of semantic retrieval in packs.

## Background reindex

`session_start` and `prompt_submit` spawn a detached `cogz reindex-bg` process that catches changes from non-hook events — branch switches, pulls, merges, human edits in another terminal. This is the recovery path that `file_save`'s single-file reindex doesn't cover.

- **`session_start`** always spawns (primary recovery, fires once per session).
- **`prompt_submit`** is debounced (60s window) to avoid redundant spawns when the user sends many messages in quick succession.
- The background process syncs `.cogz/` entity files, runs git-diff-based code reindex, runs post-index maintenance (stale flag + provenance backfill + drift recompute + heal), and defers embedding to `embed-bg`. It runs detached — the hook returns immediately without waiting for it.
- The debounce marker is a temp file keyed by the canonical repo path, so `.`, absolute paths, and symlinks to the same repo share one marker.

**PostCompaction:** After context compaction, the agent loses its injected context. Re-inject by treating it as a session start:

```json
"PostCompaction": [{
  "matcher": "",
  "hooks": [{
    "type": "command",
    "command": "cogz capture-event session_start --hook-json --fts-only",
    "timeout": 10
  }]
}]
```

## Context pack output

For `session_start` and `prompt_submit`, the context pack is printed as formatted text inside the `additionalContext` field of the hook JSON output. The pack includes:

- **Sections** — each with an entity's title, type, and content (possibly truncated to fit the token budget)
- **Source priority** — rules > observations > knowledge > code
- **Graph provenance** — for task/escalation mode, each section includes the graph path from the query match to this entity
- **Token budget** — sections are sorted by priority then relevance, and truncated/dropped to fit the configured budget

## See also

- [Agent Setup](agent-setup.md) — per-agent config file locations and supported events
- [CLI Reference](../cli-reference.md) — all `capture-event` flags
- [Design: Degradation](../design/degradation.md) — how hooks work without models
