//! `cogz configure` — write host-agent config so CogZ is wired into
//! the agent without hand-editing JSON. Two surfaces per agent: an
//! MCP server entry (`mcpServers.cogz` or Codex's TOML equivalent)
//! and lifecycle hooks (`cogz capture-event …` per event).
//!
//! Never clobbers: existing files are deep-merged (other servers and
//! other tools' hooks are preserved) and a `<file>.cogz.bak` backup is
//! written before any overwrite — later backups get a timestamped
//! suffix instead of overwriting the first. Writes are atomic
//! (temp-file + rename). Re-running is a no-op — an identical merged
//! result skips the write and the backup.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::{Map, Value};

mod apply;
mod harnesses;
mod hook_payloads;

use apply::apply;
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
/// `cogz.exe`, and a quoted path is the same argv[0]). Anything else
/// compares as a sorted token multiset so flag order doesn't matter.
fn command_key(cmd: &str) -> String {
    let (argv0, rest) = split_argv0(cmd);
    let rtoks: Vec<&str> = rest.split_whitespace().collect();
    if argv0_base(argv0) == "cogz"
        && let Some(verb) = rtoks
            .iter()
            .position(|t| *t == "capture-event")
            .and_then(|i| rtoks.get(i + 1))
    {
        return format!("cogz-event:{verb}");
    }
    let mut owned: Vec<String> = rtoks.iter().map(|t| t.to_string()).collect();
    owned.push(argv0_base(argv0).to_string());
    owned.sort();
    owned.join(" ")
}

/// Split a command string into argv[0] and the remainder. A
/// double-quoted argv[0] may span whitespace; an unquoted argv[0] is
/// the first token.
fn split_argv0(cmd: &str) -> (&str, &str) {
    let t = cmd.trim_start();
    if let Some(r) = t.strip_prefix('"')
        && let Some(i) = r.find('"')
    {
        return (&r[..i], r[i + 1..].trim_start());
    }
    match t.find(char::is_whitespace) {
        Some(i) => (&t[..i], t[i..].trim_start()),
        None => (t, ""),
    }
}

/// Basename of an argv[0], `.exe` stripped —
/// `cogz` ≡ `/usr/bin/cogz` ≡ `C:\Tools\cogz.exe`.
fn argv0_base(argv0: &str) -> &str {
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
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
    if argv0_base(split_argv0(cmd).0) != "cogz" {
        return false;
    }
    cmd.contains("mcp-stdio")
        || obj
            .get("args")
            .and_then(|a| a.as_array())
            .is_some_and(|args| args.iter().any(|a| a.as_str() == Some("mcp-stdio")))
}

/// Same merge for TOML tables via `toml_edit`, which preserves the
/// user's comments and formatting. Existing leaves are never
/// overwritten — same contract as `merge_json` scalars inside a
/// cogz-server entry, extended to every key: a hand-maintained
/// `command`/`args` variant is user wiring, not a gap to fill. The
/// cogz-server guard mirrors `merge_json`: an entry already wired to
/// `cogz mcp-stdio` is kept as written.
fn merge_toml_doc(base: &mut toml_edit::Table, patch: &toml_edit::Table) {
    if is_cogz_server_table(base) && is_cogz_server_table(patch) {
        return;
    }
    for (k, v) in patch.iter() {
        match base.get_mut(k) {
            Some(toml_edit::Item::Table(bt)) => {
                if let Some(pt) = v.as_table() {
                    merge_toml_doc(bt, pt);
                }
            }
            Some(existing) if matches!(existing, toml_edit::Item::None) => {
                *existing = v.clone();
            }
            Some(_) => {}
            None => {
                base.insert(k, v.clone());
            }
        }
    }
}

/// TOML counterpart of `is_cogz_server` — same shape, `toml_edit`
/// accessors.
fn is_cogz_server_table(t: &toml_edit::Table) -> bool {
    let Some(cmd) = t.get("command").and_then(|c| c.as_str()) else {
        return false;
    };
    if argv0_base(split_argv0(cmd).0) != "cogz" {
        return false;
    }
    cmd.contains("mcp-stdio")
        || t.get("args")
            .and_then(|a| a.as_array())
            .is_some_and(|args| args.iter().any(|a| a.as_str() == Some("mcp-stdio")))
}

// ─── Entry point ──────────────────────────────────────────────────

/// Absolute path of the running cogz binary — what generated commands
/// should invoke. Hook runtimes and MCP spawners often run with a
/// minimal PATH (`~/.local/bin` absent), so a bare `cogz` resolves to
/// nothing (exit 127). Falls back to `cogz` when the exe path can't be
/// determined.
pub fn self_bin() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "cogz".to_string())
}

/// argv[0] safe to embed in a generated hook command string —
/// double-quoted when the path contains whitespace so shell-splitting
/// keeps it one token. MCP `command` fields carry the raw path
/// instead (argv[0] passed directly, not shell-parsed).
pub(super) fn shell_argv0(bin: &str) -> String {
    if bin.chars().any(char::is_whitespace) {
        format!("\"{bin}\"")
    } else {
        bin.to_string()
    }
}

/// Configure the named harnesses (or `auto`) for `repo` at `scope`.
/// `home` is the user's home directory — a parameter so tests can
/// supply a fake HOME. `bin` is argv[0] written into generated hook and
/// MCP commands — callers pass `self_bin()`.
///
/// Returns the report text for the caller to print.
pub fn run(repo: &Path, arg: &str, scope: Scope, home: &Path, bin: &str) -> Result<String> {
    let targets = resolve_targets(arg, repo, home)?;
    let mut report = String::new();
    for h in targets {
        report.push_str(&format!("{}:\n", h.id));
        let patches = match scope {
            Scope::Project => (h.project_patches)(repo, bin),
            Scope::User => (h.user_patches)(home, bin),
        };
        for patch in &patches {
            match apply(patch, bin)? {
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
#[cfg(test)]
mod tests_merge;
