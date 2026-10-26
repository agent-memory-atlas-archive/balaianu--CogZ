//! File application for generated patches: read, merge, back up,
//! and atomically write. Parse errors on an existing file abort
//! rather than clobber.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::{Outcome, Patch, merge_json, merge_toml_doc};

/// Read, merge, and write one file. Returns the outcome; parse errors
/// on an existing file abort rather than clobber.
pub(super) fn apply(patch: &Patch) -> Result<Outcome> {
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
