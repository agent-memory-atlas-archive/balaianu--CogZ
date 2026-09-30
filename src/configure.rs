//! `cogz configure` — write host-agent config so CogZ is wired into
//! the agent without hand-editing JSON. Two surfaces per agent: an
//! MCP server entry (`mcpServers.cogz` or Codex's TOML equivalent)
//! and lifecycle hooks (`cogz capture-event …` per event).
//!
//! Never clobbers: existing files are deep-merged (other servers and
//! other tools' hooks are preserved) and a `<file>.cogz.bak` backup is
//! written before any overwrite. Re-running is a no-op — an identical
//! merged result skips the write and the backup.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

/// Where config files land: inside the repo, or under the user's home.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Project,
    User,
}

/// A single file to create or merge into.
enum Patch {
    /// Deep-merge a JSON object into the file (parsed or `{}`).
    Json(PathBuf, Value),
    /// Merge a TOML table into the file (parsed or empty table).
    Toml(PathBuf, toml::Value),
}

/// What happened to one file.
enum Outcome {
    Written { path: PathBuf, backed_up: bool },
    Unchanged { path: PathBuf },
}

/// One supported agent: identity, detection markers, file plan.
struct Harness {
    id: &'static str,
    /// Marker paths relative to the repo root.
    repo_markers: &'static [&'static str],
    /// Marker paths relative to the user's home.
    home_markers: &'static [&'static str],
    project_patches: fn(&Path) -> Vec<Patch>,
    user_patches: fn(&Path) -> Vec<Patch>,
}

// ─── Payloads ─────────────────────────────────────────────────────

fn mcp_entry() -> Value {
    json!({"command": "cogz", "args": ["mcp-stdio"]})
}

fn hook_entry(command: &str, timeout: u64, matcher: &str) -> Value {
    json!({
        "matcher": matcher,
        "hooks": [{"type": "command", "command": command, "timeout": timeout}],
    })
}

/// Canonical PascalCase event map (Claude Code shape — the de facto
/// standard). Shared by claude-code, codex, and devin (plus extra).
fn hooks_canonical() -> Value {
    json!({
        "SessionStart": [hook_entry("cogz capture-event session_start --hook-json", 15, "")],
        "UserPromptSubmit": [hook_entry("cogz capture-event prompt_submit --hook-json", 15, "")],
        "PostToolUse": [
            hook_entry("cogz capture-event post_tool_use --hook-json --fts-only", 10, ""),
            hook_entry("cogz capture-event file_save --hook-json --fts-only", 20, "edit|write|notebook_edit"),
        ],
        "SessionEnd": [hook_entry("cogz capture-event session_end --hook-json --fts-only", 30, "")],
        "Stop": [hook_entry("cogz capture-event stop --hook-json --fts-only", 5, "")],
    })
}

/// Devin speaks the canonical map plus PostCompaction, which
/// re-injects the session-start pack after compaction.
fn hooks_devin() -> Value {
    let mut hooks = hooks_canonical().as_object().unwrap().clone();
    hooks.insert(
        "PostCompaction".into(),
        json!([hook_entry(
            "cogz capture-event session_start --hook-json",
            15,
            ""
        )]),
    );
    Value::Object(hooks)
}

/// Cursor hooks.json uses camelCase event names.
fn hooks_camel() -> Value {
    json!({
        "sessionStart": [hook_entry("cogz capture-event session_start --hook-json", 15, "")],
        "userPromptSubmit": [hook_entry("cogz capture-event prompt_submit --hook-json", 15, "")],
        "postToolUse": [
            hook_entry("cogz capture-event post_tool_use --hook-json --fts-only", 10, ""),
            hook_entry("cogz capture-event file_save --hook-json --fts-only", 20, "edit|write|notebook_edit"),
        ],
        "sessionEnd": [hook_entry("cogz capture-event session_end --hook-json --fts-only", 30, "")],
        "stop": [hook_entry("cogz capture-event stop --hook-json --fts-only", 5, "")],
    })
}

/// Gemini fires AfterTool where Claude fires PostToolUse.
fn hooks_gemini() -> Value {
    json!({
        "SessionStart": [hook_entry("cogz capture-event session_start --hook-json", 15, "")],
        "UserPromptSubmit": [hook_entry("cogz capture-event prompt_submit --hook-json", 15, "")],
        "AfterTool": [
            hook_entry("cogz capture-event post_tool_use --hook-json --fts-only", 10, ""),
            hook_entry("cogz capture-event file_save --hook-json --fts-only", 20, "edit|write|notebook_edit"),
        ],
        "SessionEnd": [hook_entry("cogz capture-event session_end --hook-json --fts-only", 30, "")],
    })
}

/// Copilot hook sets are standalone files with a version field.
fn hooks_copilot() -> Value {
    json!({"version": 1, "hooks": hooks_camel()})
}

fn mcp_json_patch(path: PathBuf) -> Patch {
    Patch::Json(path, json!({"mcpServers": {"cogz": mcp_entry()}}))
}

fn hooks_json_patch(path: PathBuf, hooks: Value) -> Patch {
    Patch::Json(path, json!({"hooks": hooks}))
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
        hooks_json_patch(repo.join(".claude/settings.json"), hooks_canonical()),
    ]
}

fn claude_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".claude.json")),
        hooks_json_patch(home.join(".claude/settings.json"), hooks_canonical()),
    ]
}

fn cursor_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".cursor/mcp.json")),
        hooks_json_patch(repo.join(".cursor/hooks.json"), hooks_camel()),
    ]
}

fn cursor_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".cursor/mcp.json")),
        hooks_json_patch(home.join(".cursor/hooks.json"), hooks_camel()),
    ]
}

fn codex_project(repo: &Path) -> Vec<Patch> {
    vec![
        codex_mcp_patch(repo.join(".codex/config.toml")),
        hooks_json_patch(repo.join(".codex/hooks.json"), hooks_canonical()),
    ]
}

fn codex_user(home: &Path) -> Vec<Patch> {
    vec![
        codex_mcp_patch(home.join(".codex/config.toml")),
        hooks_json_patch(home.join(".codex/hooks.json"), hooks_canonical()),
    ]
}

/// Gemini keeps MCP servers and hooks in the same settings file.
fn gemini_project(repo: &Path) -> Vec<Patch> {
    vec![Patch::Json(
        repo.join(".gemini/settings.json"),
        json!({"mcpServers": {"cogz": mcp_entry()}, "hooks": hooks_gemini()}),
    )]
}

fn gemini_user(home: &Path) -> Vec<Patch> {
    vec![Patch::Json(
        home.join(".gemini/settings.json"),
        json!({"mcpServers": {"cogz": mcp_entry()}, "hooks": hooks_gemini()}),
    )]
}

fn copilot_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".mcp.json")),
        Patch::Json(repo.join(".github/hooks/cogz.json"), hooks_copilot()),
    ]
}

fn copilot_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".copilot/mcp-config.json")),
        Patch::Json(home.join(".copilot/hooks/cogz.json"), hooks_copilot()),
    ]
}

fn devin_project(repo: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(repo.join(".devin/mcp_config.json")),
        hooks_json_patch(repo.join(".devin/hooks.v1.json"), hooks_devin()),
    ]
}

fn devin_user(home: &Path) -> Vec<Patch> {
    vec![
        mcp_json_patch(home.join(".config/devin/mcp_config.json")),
        hooks_json_patch(home.join(".config/devin/config.json"), hooks_devin()),
    ]
}

const HARNESSES: &[Harness] = &[
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

/// Resolve the `harnesses` argument: `auto` detects, otherwise a
/// comma-separated list of ids. Errors on unknown ids.
fn resolve_targets(arg: &str, repo: &Path, home: &Path) -> Result<Vec<&'static Harness>> {
    let mut wanted: Vec<String> = arg
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if wanted.is_empty() {
        bail!(
            "no harnesses given (expected one of: {}, or `auto`)",
            harness_ids().join(", ")
        );
    }
    if wanted.iter().any(|w| w == "auto") {
        if wanted.len() > 1 {
            bail!("`auto` cannot be combined with explicit harnesses");
        }
        wanted = detect(repo, home).iter().map(|s| s.to_string()).collect();
        if wanted.is_empty() {
            bail!(
                "no agent configs detected — pass harnesses explicitly ({})",
                harness_ids().join(", ")
            );
        }
    }
    let mut out: Vec<&'static Harness> = Vec::new();
    for w in &wanted {
        match HARNESSES.iter().find(|h| h.id == w) {
            Some(h) if !out.iter().any(|o| o.id == h.id) => out.push(h),
            Some(_) => {}
            None => bail!(
                "unknown harness '{w}' (expected one of: {}, or `auto`)",
                harness_ids().join(", ")
            ),
        }
    }
    Ok(out)
}

// ─── Merge machinery ──────────────────────────────────────────────

/// Deep-merge `patch` into `base`. Objects merge per key; arrays
/// append elements not already present (element-wise equality, so a
/// hook entry already installed is skipped and other tools' entries
/// survive untouched); scalars take the patch value.
fn merge_json(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(&k) {
                    Some(existing) => merge_json(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (Value::Array(b), Value::Array(p)) => {
            for item in p {
                if !b.contains(&item) {
                    b.push(item);
                }
            }
        }
        (b, p) => *b = p,
    }
}

/// Same merge for TOML tables — arrays are absent from the payloads
/// we write, so table-merge + leaf-override suffices.
fn merge_toml(base: &mut toml::Value, patch: toml::Value) {
    if let (toml::Value::Table(b), toml::Value::Table(p)) = (base, patch) {
        for (k, v) in p {
            match b.get_mut(&k) {
                Some(existing) => merge_toml(existing, v),
                None => {
                    b.insert(k, v);
                }
            }
        }
    }
}

/// Read, merge, and write one file. Returns the outcome; parse errors
/// on an existing file abort rather than clobber.
fn apply(patch: &Patch) -> Result<Outcome> {
    let (path, merged_text, changed) = match patch {
        Patch::Json(path, patch_value) => {
            let existing = read_if_exists(path)?;
            let mut base: Value = match &existing {
                Some(text) => serde_json::from_str(text).with_context(|| {
                    format!(
                        "existing {} is not valid JSON — fix or remove it",
                        path.display()
                    )
                })?,
                None => Value::Object(Map::new()),
            };
            merge_json(&mut base, patch_value.clone());
            let new_text = format!("{}\n", serde_json::to_string_pretty(&base).unwrap());
            let changed = existing.as_deref() != Some(new_text.as_str());
            (path, new_text, changed)
        }
        Patch::Toml(path, patch_value) => {
            let existing = read_if_exists(path)?;
            let mut base: toml::Value = match &existing {
                Some(text) => toml::from_str(text).with_context(|| {
                    format!(
                        "existing {} is not valid TOML — fix or remove it",
                        path.display()
                    )
                })?,
                None => toml::Value::Table(toml::map::Map::new()),
            };
            merge_toml(&mut base, patch_value.clone());
            let new_text =
                toml::to_string_pretty(&base).context("failed to serialize merged TOML")?;
            let changed = existing.as_deref() != Some(new_text.as_str());
            (path, new_text, changed)
        }
    };

    if !changed {
        return Ok(Outcome::Unchanged { path: path.clone() });
    }

    let existed = path.exists();
    if existed {
        let backup = path.with_file_name(format!(
            "{}.cogz.bak",
            path.file_name().unwrap().to_string_lossy()
        ));
        fs::copy(path, &backup).with_context(|| {
            format!(
                "failed to back up {} to {}",
                path.display(),
                backup.display()
            )
        })?;
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, merged_text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(Outcome::Written {
        path: path.clone(),
        backed_up: existed,
    })
}

fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

// ─── Entry point ──────────────────────────────────────────────────

/// Configure the named harnesses (or `auto`) for `repo` at `scope`.
/// `home` is the user's home directory — a parameter so tests can
/// supply a fake HOME.
///
/// Returns the report text for the caller to print.
pub fn run(repo: &Path, arg: &str, scope: Scope, home: &Path) -> Result<String> {
    let targets = resolve_targets(arg, repo, home)?;
    let mut report = String::new();
    for h in targets {
        report.push_str(&format!("{}:\n", h.id));
        let patches = match scope {
            Scope::Project => (h.project_patches)(repo),
            Scope::User => (h.user_patches)(home),
        };
        for patch in &patches {
            match apply(patch)? {
                Outcome::Written { path, backed_up } => {
                    let note = if backed_up {
                        " (backup: .cogz.bak)"
                    } else {
                        ""
                    };
                    report.push_str(&format!("  wrote {}{}\n", path.display(), note));
                }
                Outcome::Unchanged { path } => {
                    report.push_str(&format!("  {} already configured\n", path.display()));
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
#[path = "configure_tests.rs"]
mod tests;
