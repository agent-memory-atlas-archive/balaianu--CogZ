//! File application for generated patches: read, merge, back up,
//! and atomically write. Parse errors on an existing file abort
//! rather than clobber.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::{Outcome, Patch, merge_json, merge_toml_doc, shell_argv0};

/// Read, merge, and write one file. Returns the outcome; parse errors
/// on an existing file abort rather than clobber.
pub(super) fn apply(patch: &Patch, bin: &str) -> Result<Outcome> {
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
            upgrade_json(&mut base, bin);
            let new_text = format!("{}\n", serde_json::to_string_pretty(&base).unwrap());
            let changed = existing.as_deref() != Some(new_text.as_str());
            (path, new_text, changed)
        }
        Patch::Toml(path, patch_value) => {
            let existing = read_if_exists(path)?;
            let mut base: toml_edit::DocumentMut = match &existing {
                Some(text) => text.parse().with_context(|| {
                    format!(
                        "existing {} is not valid TOML — fix or remove it",
                        path.display()
                    )
                })?,
                None => toml_edit::DocumentMut::new(),
            };
            let patch_text =
                toml::to_string(patch_value).context("failed to serialize TOML patch")?;
            let patch_doc: toml_edit::DocumentMut = patch_text
                .parse()
                .context("generated TOML patch is invalid")?;
            merge_toml_doc(base.as_table_mut(), patch_doc.as_table());
            upgrade_toml(base.as_table_mut(), bin);
            let new_text = base.to_string();
            let changed = existing.as_deref() != Some(new_text.as_str());
            (path, new_text, changed)
        }
    };

    if !changed {
        return Ok(Outcome::Unchanged { path: path.clone() });
    }

    let existed = path.exists();
    if existed {
        let backup = backup_path(path);
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
    crate::files::atomic::atomic_write(path, merged_text.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(Outcome::Written {
        path: path.clone(),
        backed_up: existed,
    })
}

/// First backup is `<file>.cogz.bak`; if that already exists (a
/// previous run's snapshot of the user's hand edits), write
/// `<file>.cogz.bak.<unix-seconds>` so earlier backups survive.
fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap().to_string_lossy();
    let plain = path.with_file_name(format!("{name}.cogz.bak"));
    if !plain.exists() {
        return plain;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Same-second reruns collide; suffix them rather than overwrite.
    for i in 0.. {
        let suffix = if i == 0 {
            format!("{ts}")
        } else {
            format!("{ts}.{i}")
        };
        let candidate = path.with_file_name(format!("{name}.cogz.bak.{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

// ─── Bare-argv0 upgrade ───────────────────────────────────────────

/// Rewrite a `command` value whose argv[0] is the literal `cogz` to
/// invoke `bin` instead. A bare `cogz` resolves via PATH, which hook
/// runtimes often lack — dedup counts such entries as wired, so
/// repair must happen here post-merge rather than by appending. Only
/// the argv[0] token changes: flags, wrappers, and non-cogz commands
/// are left exactly as the user wrote them.
fn rewrite_argv0(s: &str, bin: &str) -> Option<String> {
    if s == "cogz" {
        return Some(bin.to_string());
    }
    let rest = s.strip_prefix("cogz ")?;
    Some(format!("{} {rest}", shell_argv0(bin)))
}

fn upgrade_json(v: &mut Value, bin: &str) {
    match v {
        Value::Object(map) => {
            for (k, item) in map.iter_mut() {
                if k == "command" {
                    if let Some(s) = item.as_str()
                        && let Some(new) = rewrite_argv0(s, bin)
                    {
                        *item = Value::String(new);
                    }
                } else {
                    upgrade_json(item, bin);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| upgrade_json(i, bin)),
        _ => {}
    }
}

fn upgrade_toml(table: &mut toml_edit::Table, bin: &str) {
    for (key, item) in table.iter_mut() {
        if key == "command" {
            if let Some(s) = item.as_str()
                && let Some(new) = rewrite_argv0(s, bin)
            {
                *item = toml_edit::value(new);
            }
        } else {
            match item {
                toml_edit::Item::Table(t) => upgrade_toml(t, bin),
                toml_edit::Item::ArrayOfTables(arrays) => {
                    for t in arrays.iter_mut() {
                        upgrade_toml(t, bin);
                    }
                }
                _ => {}
            }
        }
    }
}
