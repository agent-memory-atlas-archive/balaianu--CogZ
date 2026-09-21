//! Write-path mining — surfaces observation candidates from usage
//! signals without writing anything itself.
//!
//! Five signals live in `mining_signals`: edits after zero-hit packs
//! (uncharted work), searches that missed then got answered by edits,
//! entities delivered then hit repeatedly, files edited over and over
//! (hot spots), and tool errors followed by a fix (pitfalls).
//! The consuming agent's model is the distiller — these are structured
//! leads, not auto-written knowledge.

use rusqlite::Connection;

use super::StorageError;
use super::mining_signals::{
    error_fixes, hot_files, recurring_use, search_misses, uncharted_edits,
};

/// A mined observation candidate. The agent confirms or dismisses it
/// through `create_entity` — nothing here writes itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Suggestion {
    /// Which mining signal produced this candidate.
    pub signal: &'static str,
    /// Signal-specific evidence (event ids, counts, file paths).
    pub evidence: serde_json::Value,
    pub suggested_title: String,
    pub suggested_content: String,
    /// Entity ids worth referencing (e.g. the repeatedly-hit entity).
    pub suggested_refs: Vec<String>,
}

impl Suggestion {
    /// Stable identity of a candidate across mining runs — keyed on the
    /// signal's anchor (the file, entity, or event it describes), not
    /// the whole evidence payload, so e.g. a hot file's growing save
    /// count doesn't mint a new fingerprint on every save.
    pub fn fingerprint(&self) -> String {
        for key in [
            "dedup",
            "file_path",
            "entity_id",
            "delivery_id",
            "error_event_id",
            "search_event_id",
        ] {
            if let Some(v) = self.evidence.get(key) {
                let v = v
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string());
                return format!("{}:{}", self.signal, v);
            }
        }
        format!("{}:{}", self.signal, self.evidence)
    }
}

/// Run all mining passes and return candidates, newest signals first,
/// capped at `limit`. `days` bounds how far back events are read.
pub fn mine_suggestions(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut out = Vec::new();
    out.extend(uncharted_edits(conn, days, limit)?);
    out.extend(search_misses(conn, days, limit)?);
    out.extend(recurring_use(conn, days, limit)?);
    out.extend(hot_files(conn, days, limit)?);
    out.extend(error_fixes(conn, days, limit)?);
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
#[path = "mining_tests.rs"]
mod tests;
