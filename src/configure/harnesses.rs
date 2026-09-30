//! Harness table and per-agent config payloads. Event names, file
//! locations, and entry shapes are verified against each agent's
//! official hook/MCP docs; divergences are recorded in the
//! `agent-config-format-divergences` knowledge entry and
//! docs/integration/agent-setup.md.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// A single file to create or merge into.
pub(super) enum Patch {
    /// Deep-merge a JSON object into the file (parsed or `{}`).
    Json(PathBuf, Value),
    /// Merge a TOML table into the file (parsed or empty table).
    Toml(PathBuf, toml::Value),
}

/// One supported agent: identity, detection markers, file plan.
pub(super) struct Harness {
    pub id: &'static str,
    /// Marker paths relative to the repo root.
    repo_markers: &'static [&'static str],
    /// Marker paths relative to the user's home.
    home_markers: &'static [&'static str],
    pub project_patches: fn(&Path) -> Vec<Patch>,
    pub user_patches: fn(&Path) -> Vec<Patch>,
}

// ─── Payloads ─────────────────────────────────────────────────────

fn mcp_entry() -> Value {
    json!({"command": "cogz", "args": ["mcp-stdio"]})
}

/// Claude-shaped matcher-group entry. `timeout` is in seconds for
/// Claude Code, Codex, and Devin.
fn hook_entry(command: &str, timeout: u64, matcher: &str) -> Value {
    let mut group = Map::new();
    if !matcher.is_empty() {
        group.insert("matcher".into(), Value::String(matcher.to_string()));
    }
    group.insert(
        "hooks".into(),
        json!([{"type": "command", "command": command, "timeout": timeout}]),
    );
    Value::Object(group)
}

fn hook_entries(bin: &str, args: &str, timeout: u64, matcher: &str) -> Value {
    json!([hook_entry(
        &format!("{bin} capture-event {args} --hook-json"),
        timeout,
        matcher
    )])
}

/// Canonical PascalCase event map (Claude Code shape — the de facto
/// standard Codex and Devin also speak). `edit_matcher` filters
/// PostToolUse down to file-writing tools for the file_save event.
/// PostCompact re-injects the session-start pack after compaction.
fn hooks_canonical(bin: &str, edit_matcher: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "SessionStart".into(),
        hook_entries(bin, "session_start", 15, ""),
    );
    map.insert(
        "UserPromptSubmit".into(),
        hook_entries(bin, "prompt_submit", 15, ""),
    );
    map.insert(
        "PostToolUse".into(),
        json!([
            hook_entry(
                &format!("{bin} capture-event post_tool_use --hook-json --fts-only"),
                10,
                ""
            ),
            hook_entry(
                &format!("{bin} capture-event file_save --hook-json --fts-only"),
                20,
                edit_matcher
            ),
        ]),
    );
    map.insert(
        "SessionEnd".into(),
        hook_entries(bin, "session_end --fts-only", 30, ""),
    );
    map.insert("Stop".into(), hook_entries(bin, "stop --fts-only", 5, ""));
    map.insert(
        "PostCompact".into(),
        hook_entries(bin, "session_start --fts-only", 15, ""),
    );
    map
}

/// Devin speaks the canonical map but calls the compaction event
/// PostCompaction and uses lowercase tool names in matchers.
fn hooks_devin(bin: &str) -> Value {
    let mut hooks = hooks_canonical(bin, "edit|write|notebook_edit");
    let post_compact = hooks.remove("PostCompact").unwrap();
    hooks.insert("PostCompaction".into(), post_compact);
    Value::Object(hooks)
}

fn claude_shaped_patch(path: PathBuf, bin: &str, edit_matcher: &str) -> Patch {
    Patch::Json(path, json!({"hooks": hooks_canonical(bin, edit_matcher)}))
}

/// Cursor's hooks.json is `{"version": 1, "hooks": {camelCase:
/// [{"command", "matcher"?, "timeout"?}]}}` — flat entries, not the
/// Claude matcher-group shape. `afterFileEdit` is a native event so
/// file_save needs no tool-name matcher; `preCompact` covers
/// compaction. Cursor's stdout contract is `additional_context`
/// (snake_case), so generated commands pass `--hook-format cursor`.
/// `beforeSubmitPrompt` cannot inject context (output schema is
/// continue/user_message only), so prompt_submit runs --fts-only:
/// the event is recorded without generating an unconsumable pack.
fn cursor_hooks_patch(path: PathBuf, bin: &str) -> Patch {
    let e = |args: &str, timeout: u64| -> Value {
        json!([{
            "command": format!("{bin} capture-event {args} --hook-json --hook-format cursor"),
            "timeout": timeout
        }])
    };
    Patch::Json(
        path,
        json!({
            "version": 1,
            "hooks": {
                "sessionStart": e("session_start", 15),
                "beforeSubmitPrompt": e("prompt_submit --fts-only", 15),
                "postToolUse": e("post_tool_use --fts-only", 10),
                "afterFileEdit": e("file_save --fts-only", 20),
                "sessionEnd": e("session_end --fts-only", 30),
                "stop": e("stop --fts-only", 5),
                "preCompact": e("session_start --fts-only", 15),
            }
        }),
    )
}

/// Copilot hook sets are standalone versioned JSON files under
/// .github/hooks/ (project) or ~/.copilot/hooks/ (user). Entries are
/// flat: `command` is the cross-platform fallback Copilot copies to
/// bash and powershell; `timeoutSec` is seconds. PascalCase event
/// names switch stdin payloads to snake_case (matching the fields
/// `cogz capture-event` parses). Copilot's context-injection field is
/// top-level `additionalContext` → `--hook-format copilot`.
/// `UserPromptSubmit` hook output is dropped by Copilot's command-hook
/// runtime, so prompt_submit runs --fts-only (record-only). Copilot's
/// versioned format has no matcher field, so file_save has no analog.
fn copilot_hooks_patch(path: PathBuf, bin: &str) -> Patch {
    let e = |args: &str, timeout: u64| -> Value {
        json!([{
            "type": "command",
            "command": format!("{bin} capture-event {args} --hook-json --hook-format copilot"),
            "timeoutSec": timeout
        }])
    };
    Patch::Json(
        path,
        json!({
            "version": 1,
            "hooks": {
                "SessionStart": e("session_start", 15),
                "UserPromptSubmit": e("prompt_submit --fts-only", 15),
                "PostToolUse": e("post_tool_use --fts-only", 10),
                "SessionEnd": e("session_end --fts-only", 30),
                "Stop": e("stop --fts-only", 5),
            }
        }),
    )
}

/// Gemini nests hooks in settings.json beside mcpServers. Shape is
/// Claude-like matcher groups, but event names differ (BeforeAgent,
/// AfterTool, AfterAgent, PreCompress) and `timeout` is in
/// MILLISECONDS (default 60000), not seconds.
fn gemini_settings_patch(path: PathBuf, bin: &str) -> Patch {
    let g = |args: &str, timeout_ms: u64| -> Value {
        json!([{
            "name": "cogz",
            "type": "command",
            "command": format!("{bin} capture-event {args} --hook-json"),
            "timeout": timeout_ms
        }])
    };
    Patch::Json(
        path,
        json!({
            "mcpServers": {"cogz": mcp_entry()},
            "hooks": {
                "SessionStart": [{"hooks": g("session_start", 15000)}],
                "BeforeAgent": [{"matcher": "*", "hooks": g("prompt_submit", 15000)}],
                "AfterTool": [
                    {"hooks": g("post_tool_use --fts-only", 10000)},
                    {
                        "matcher": "write_file|replace|edit|notebook_edit",
                        "hooks": g("file_save --fts-only", 20000)
                    },
                ],
                "SessionEnd": [{"hooks": g("session_end --fts-only", 30000)}],
                "AfterAgent": [{"matcher": "*", "hooks": g("stop --fts-only", 5000)}],
                "PreCompress": [{"hooks": g("session_start --fts-only", 15000)}],
            }
        }),
    )
}

fn mcp_json_patch(path: PathBuf) -> Patch {
    Patch::Json(path, json!({"mcpServers": {"cogz": mcp_entry()}}))
}

fn codex_mcp_patch(path: PathBuf) -> Patch {
    Patch::Toml(
        path,
        toml::toml! {
            [mcp_servers.cogz]
            command = "cogz"
            args = ["mcp-stdio"]
        }
        .into(),
    )
}

// ─── Harness table ────────────────────────────────────────────────

fn claude_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".mcp.json")),
        claude_shaped_patch(
            repo.join(".claude/settings.json"),
            "cogz",
            "Edit|Write|MultiEdit|NotebookEdit",
        ),
    ]
}

fn claude_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".claude.json")),
        claude_shaped_patch(
            home.join(".claude/settings.json"),
            "cogz",
            "Edit|Write|MultiEdit|NotebookEdit",
        ),
    ]
}

fn cursor_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".cursor/mcp.json")),
        cursor_hooks_patch(repo.join(".cursor/hooks.json"), "cogz"),
    ]
}

fn cursor_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".cursor/mcp.json")),
        cursor_hooks_patch(home.join(".cursor/hooks.json"), "cogz"),
    ]
}

fn codex_project(repo: &Path) -> Vec<Patch> {
    vec![
        codex_mcp_patch(repo.join(".codex/config.toml")),
        claude_shaped_patch(
            repo.join(".codex/hooks.json"),
            "cogz",
            "Edit|Write|NotebookEdit|apply_patch|edit|write",
        ),
    ]
}

fn codex_user(home: &Path) -> Vec<Patch> {
    vec![
        codex_mcp_patch(home.join(".codex/config.toml")),
        claude_shaped_patch(
            home.join(".codex/hooks.json"),
            "cogz",
            "Edit|Write|NotebookEdit|apply_patch|edit|write",
        ),
    ]
}

fn gemini_project(repo: &Path) -> Vec<Patch> {
    vec![gemini_settings_patch(
        repo.join(".gemini/settings.json"),
        "cogz",
    )]
}

fn gemini_user(home: &Path) -> Vec<Patch> {
    vec![gemini_settings_patch(
        home.join(".gemini/settings.json"),
        "cogz",
    )]
}

fn copilot_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".mcp.json")),
        copilot_hooks_patch(repo.join(".github/hooks/cogz.json"), "cogz"),
    ]
}

fn copilot_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".copilot/mcp-config.json")),
        copilot_hooks_patch(home.join(".copilot/hooks/cogz.json"), "cogz"),
    ]
}

fn devin_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".devin/mcp_config.json")),
        claude_devin_patch(repo.join(".devin/hooks.v1.json"), "cogz"),
    ]
}

fn devin_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".config/devin/mcp_config.json")),
        claude_devin_patch(home.join(".config/devin/config.json"), "cogz"),
    ]
}

fn claude_devin_patch(path: PathBuf, bin: &str) -> Patch {
    Patch::Json(path, json!({"hooks": hooks_devin(bin)}))
}

pub(super) const HARNESSES: &[Harness] = &[
    Harness {
        id: "claude-code",
        repo_markers: &[".claude", ".mcp.json"],
        home_markers: &[".claude", ".claude.json"],
        project_patches: claude_project,
        user_patches: claude_user,
    },
    Harness {
        id: "cursor",
        repo_markers: &[".cursor"],
        home_markers: &[".cursor"],
        project_patches: cursor_project,
        user_patches: cursor_user,
    },
    Harness {
        id: "codex",
        repo_markers: &[".codex"],
        home_markers: &[".codex"],
        project_patches: codex_project,
        user_patches: codex_user,
    },
    Harness {
        id: "gemini",
        repo_markers: &[".gemini"],
        home_markers: &[".gemini"],
        project_patches: gemini_project,
        user_patches: gemini_user,
    },
    Harness {
        id: "copilot",
        repo_markers: &[".github/hooks"],
        home_markers: &[".copilot"],
        project_patches: copilot_project,
        user_patches: copilot_user,
    },
    Harness {
        id: "devin",
        repo_markers: &[".devin"],
        home_markers: &[".config/devin"],
        project_patches: devin_project,
        user_patches: devin_user,
    },
];

/// All valid harness ids, for errors and help text.
pub fn harness_ids() -> Vec<&'static str> {
    HARNESSES.iter().map(|h| h.id).collect()
}

/// Detect installed harnesses from marker files/dirs in the repo or
/// the user's home. Returns harness ids, in table order.
pub fn detect(repo: &Path, home: &Path) -> Vec<&'static str> {
    HARNESSES
        .iter()
        .filter(|h| {
            h.repo_markers.iter().any(|m| repo.join(m).exists())
                || h.home_markers.iter().any(|m| home.join(m).exists())
        })
        .map(|h| h.id)
        .collect()
}
