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
use serde_json::{Map, Value};

mod harnesses;

use harnesses::{HARNESSES, Harness, Patch};
pub use harnesses::{detect, harness_ids};

/// Where config files land: inside the repo, or under the user's home.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Project,
    User,
}

/// What happened to one file.
enum Outcome {
    Written { path: PathBuf, backed_up: bool },
    Unchanged { path: PathBuf },
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
/// append elements not already present; scalars take the patch value.
/// Hook entries dedupe semantically — see `covers` — so hand-written
/// entries that differ only in flag order, binary path, matcher
/// style, or hook grouping are recognized as already installed.
fn merge_json(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            // An existing cogz server entry is already wired — keep the
            // user's command/args (path variants, env, wrappers); same
            // contract as hook dedup: install what's missing, never
            // rewrite what's wired.
            if !is_cogz_server(b) || !is_cogz_server(&p) {
                for (k, v) in p {
                    match b.get_mut(&k) {
                        Some(existing) => merge_json(existing, v),
                        None => {
                            b.insert(k, v);
                        }
                    }
                }
            }
        }
        (Value::Array(b), Value::Array(p)) => {
            for item in p {
                if !b.iter().any(|x| covers(x, &item)) {
                    b.push(item);
                }
            }
        }
        (b, p) => *b = p,
    }
}

/// Does `existing` already install `item`? Strict equality, or — for
/// hook-shaped objects — an equivalent matcher (`""` ≡ absent) whose
/// commands already include all of `item`'s commands.
fn covers(existing: &Value, item: &Value) -> bool {
    if existing == item {
        return true;
    }
    match (hook_key(existing), hook_key(item)) {
        (Some((matcher_e, cmds_e)), Some((matcher_i, cmds_i))) => {
            matcher_e == matcher_i && cmds_i.iter().all(|c| cmds_e.contains(c))
        }
        _ => false,
    }
}

/// Normalized identity of a hook element: its matcher plus the
/// command keys it wires. Handles both the Claude-shape matcher
/// group (`{"matcher"?, "hooks": [{"command", …}]}`) and flat entries
/// (`{"command", "matcher"?, …}` used by Cursor and Copilot). Returns
/// `None` for non-hook-shaped objects, which then compare strictly.
fn hook_key(v: &Value) -> Option<(String, Vec<String>)> {
    let obj = v.as_object()?;
    let matcher = obj
        .get("matcher")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    match obj.get("hooks") {
        Some(Value::Array(hooks)) => {
            if !obj.keys().all(|k| k == "matcher" || k == "hooks") {
                return None;
            }
            let cmds: Option<Vec<String>> = hooks
                .iter()
                .map(|h| h.get("command").and_then(|c| c.as_str()).map(command_key))
                .collect();
            Some((matcher, cmds?))
        }
        _ => obj
            .get("command")
            .and_then(|c| c.as_str())
            .map(|c| (matcher, vec![command_key(c)])),
    }
}

/// Dedup identity of a command string. A `cogz capture-event <verb>`
/// invocation reduces to its verb — flag variants (`--fts-only`,
/// `--hook-format …`) are user customization, not a distinct hook —
/// while argv[0] is basename-normalized (`cogz` ≡ `/usr/bin/cogz` ≡
/// `cogz.exe`). Anything else compares as a sorted token multiset so
/// flag order doesn't matter.
fn command_key(cmd: &str) -> String {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    if toks.first().map(|t| argv0_base(t)) == Some("cogz")
        && let Some(verb) = toks
            .iter()
            .position(|t| *t == "capture-event")
            .and_then(|i| toks.get(i + 1))
    {
        return format!("cogz-event:{verb}");
    }
    let mut owned: Vec<String> = toks.iter().map(|t| t.to_string()).collect();
    if let Some(first) = owned.first_mut() {
        *first = argv0_base(first).to_string();
    }
    owned.sort();
    owned.join(" ")
}

/// Basename of argv[0] in a command string, `.exe` stripped —
/// `cogz` ≡ `/usr/bin/cogz` ≡ `C:\Tools\cogz.exe`.
fn argv0_base(cmd: &str) -> &str {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let base = first.rsplit(['/', '\\']).next().unwrap_or(first);
    base.strip_suffix(".exe").unwrap_or(base)
}

/// An MCP server entry already wired to `cogz mcp-stdio` — any argv[0]
/// path variant counts. Mirrors hook dedup: a hand-maintained entry
/// pointing at a different binary location or carrying extra keys
/// (`env`, `cwd`) is user wiring, not a gap to fill.
fn is_cogz_server(obj: &Map<String, Value>) -> bool {
    let Some(cmd) = obj.get("command").and_then(|c| c.as_str()) else {
        return false;
    };
    if argv0_base(cmd) != "cogz" {
        return false;
    }
    cmd.contains("mcp-stdio")
        || obj
            .get("args")
            .and_then(|a| a.as_array())
            .is_some_and(|args| args.iter().any(|a| a.as_str() == Some("mcp-stdio")))
}

/// Same merge for TOML tables — arrays are absent from the payloads
/// we write, so table-merge + leaf-override suffices. The cogz-server
/// guard mirrors `merge_json`: an entry already wired to
/// `cogz mcp-stdio` is kept as written.
fn merge_toml(base: &mut toml::Value, patch: toml::Value) {
    if let (toml::Value::Table(b), toml::Value::Table(p)) = (base, patch) {
        if is_cogz_server_toml(b) && is_cogz_server_toml(&p) {
            return;
        }
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

/// TOML counterpart of `is_cogz_server` — same shape, TOML values.
fn is_cogz_server_toml(t: &toml::map::Map<String, toml::Value>) -> bool {
    serde_json::to_value(t)
        .ok()
        .as_ref()
        .and_then(|v| v.as_object())
        .is_some_and(is_cogz_server)
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
mod tests;
