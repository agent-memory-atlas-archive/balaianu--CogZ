//! Rejection — the write path that puts `status: rejected` on an
//! entity. File-first like every canonical write: the frontmatter is
//! updated and re-synced, so the status lattice validates the
//! transition inside the sync layer (`active → rejected` only).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use crate::files::frontmatter::FmValue;
use crate::files::{read_entity_file, sync_single_file};
use crate::storage::events::{self, EventType};
use crate::storage::{Storage, StorageError, crud};

/// What a successful rejection changed.
#[derive(Debug)]
pub struct RejectOutcome {
    pub id: String,
    /// Path of the entity file relative to `.cogz/` (e.g.
    /// `knowledge/x.md`).
    pub file_path: PathBuf,
    pub reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RejectError {
    #[error("entity {0} not found")]
    NotFound(String),
    #[error(
        "entity {id} has type '{entity_type}' — only observation, rule and knowledge carry a verdict"
    )]
    NotEpistemic { id: String, entity_type: String },
    #[error("entity {0} has no canonical file — status changes are file-first")]
    NoFile(String),
    #[error("entity file {path} resolves outside .cogz/")]
    PathOutsideCogz { path: String },
    #[error("failed to read entity file {path}: {message}")]
    Read { path: String, message: String },
    #[error("failed to write entity file {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("sync failed for {path}: {message}")]
    Sync { path: String, message: String },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Reject an entity: write `status: rejected` into its canonical file
/// and sync. The transition is checked against the lattice before the
/// file is touched — an illegal transition (e.g. `stale → rejected`)
/// fails without modifying anything.
pub fn reject_entity_file(
    storage: &Arc<Storage>,
    cogz_dir: &std::path::Path,
    id: &str,
    reason: Option<&str>,
) -> Result<RejectOutcome, RejectError> {
    let entity = {
        let conn = storage.conn();
        crud::get_entity(&conn, id).map_err(|e| match e {
            StorageError::EntityNotFound(_) => RejectError::NotFound(id.to_string()),
            other => RejectError::Storage(other),
        })?
    };

    if !matches!(entity.r#type.as_str(), "observation" | "rule" | "knowledge") {
        return Err(RejectError::NotEpistemic {
            id: id.to_string(),
            entity_type: entity.r#type.clone(),
        });
    }
    let file_path = entity
        .file_path
        .as_ref()
        .ok_or_else(|| RejectError::NoFile(id.to_string()))?;
    let abs_path = if file_path.starts_with(".cogz") {
        cogz_dir.parent().unwrap_or(cogz_dir).join(file_path)
    } else {
        cogz_dir.join(file_path)
    };
    if let (Ok(canonical_abs), Ok(canonical_cogz)) =
        (abs_path.canonicalize(), cogz_dir.canonicalize())
        && !canonical_abs.starts_with(&canonical_cogz)
    {
        return Err(RejectError::PathOutsideCogz {
            path: abs_path.display().to_string(),
        });
    }

    // Pre-check against the stored status so an illegal transition
    // fails before the canonical file is touched — the sync layer
    // would reject the same transition, but after the file changed.
    crate::storage::status::transition_status(&entity.status, "rejected")?;

    // Canonical-file write lock scopes the read-modify-write-sync
    // sequence so a concurrent writer cannot interleave.
    let (entity_file, relative) = {
        let _file_lock = storage.file_lock();

        let mut entity_file = read_entity_file(&abs_path).map_err(|e| RejectError::Read {
            path: abs_path.display().to_string(),
            message: e.to_string(),
        })?;
        entity_file.status = "rejected".to_string();
        if let Some(reason) = reason {
            entity_file
                .frontmatter
                .insert("rejected_reason", FmValue::String(reason.to_string()));
        }
        entity_file.updated_at = chrono::Utc::now().to_rfc3339();

        let new_path =
            crate::mcp::helpers::write_entity_file_atomic(&abs_path, &entity_file, cogz_dir)
                .map_err(|e| RejectError::Write {
                    path: abs_path.display().to_string(),
                    source: e,
                })?;

        let relative = new_path
            .strip_prefix(cogz_dir)
            .unwrap_or(&new_path)
            .to_path_buf();
        let sync_result = sync_single_file(storage, cogz_dir, &relative.to_string_lossy());
        if let Some(err) = sync_result.errors.first() {
            return Err(RejectError::Sync {
                path: relative.display().to_string(),
                message: err.error.to_string(),
            });
        }
        (entity_file, relative)
    };

    {
        let conn = storage.conn();
        let payload = json!({
            "entity_type": entity.r#type,
            "reason": reason,
        });
        if let Err(e) = events::record_event(&conn, EventType::EntityRejected, Some(id), &payload) {
            tracing::warn!("failed to record entity_rejected event: {e}");
        }
    }

    Ok(RejectOutcome {
        id: entity_file.id,
        file_path: relative,
        reason: reason.map(str::to_string),
    })
}

#[cfg(test)]
#[path = "reject_tests.rs"]
mod tests;
