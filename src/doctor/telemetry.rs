//! Telemetry purge — the agent-activity log under user control.
//!
//! `events`, `deliveries`, and `entity_usage` record what the agent
//! did: prompts, tool results, which entities were delivered and how
//! they resolved. Payload text is already redacted and capped at
//! write time, but the tables are the system's surveillance surface —
//! purging them is a deliberate user action, dry-run by default.
//!
//! `entity_access` (aggregate counters that power cold-start ranking)
//! is kept: it holds no free text and is rebuildable activity residue,
//! not a log. `meta`, `entity_drift`, and `cochange` are index state,
//! not telemetry.

use crate::storage::{Storage, StorageError};

/// Row counts in each telemetry table before (dry-run) or after
/// (confirm) a purge.
#[derive(Debug, Default, Clone, Copy)]
pub struct TelemetryPurgeReport {
    pub events: i64,
    pub deliveries: i64,
    pub entity_usage: i64,
}

/// Count telemetry rows; with `confirm`, delete them. Delete order
/// respects the `entity_usage.delivery_id` foreign key.
pub fn purge_telemetry(
    storage: &Storage,
    confirm: bool,
) -> Result<TelemetryPurgeReport, StorageError> {
    let conn = storage.conn();
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    };
    let report = TelemetryPurgeReport {
        events: count("events"),
        deliveries: count("deliveries"),
        entity_usage: count("entity_usage"),
    };
    if confirm {
        conn.execute_batch(
            "DELETE FROM entity_usage;
             DELETE FROM deliveries;
             DELETE FROM events;",
        )?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::events::{EventType, record_event};

    #[test]
    fn purge_counts_then_deletes() {
        let storage = Storage::open_memory().unwrap();
        {
            let conn = storage.conn();
            for _ in 0..3 {
                record_event(&conn, EventType::PromptSubmit, None, &serde_json::json!({})).unwrap();
            }
            conn.execute(
                "INSERT INTO deliveries (kind, event_id, created_at) VALUES ('pack', 1, '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            crate::storage::crud::insert_entity(
                &conn,
                &crate::storage::crud::Entity::new("e1", "knowledge", "t", "c"),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO entity_usage (delivery_id, entity_id, outcome, created_at) VALUES (1, 'e1', 'hit', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        }

        let dry = purge_telemetry(&storage, false).unwrap();
        assert_eq!((dry.events, dry.deliveries, dry.entity_usage), (3, 1, 1));
        // Dry-run deletes nothing.
        let still = purge_telemetry(&storage, false).unwrap();
        assert_eq!(still.events, 3);

        let done = purge_telemetry(&storage, true).unwrap();
        assert_eq!((done.events, done.deliveries, done.entity_usage), (3, 1, 1));
        let after = purge_telemetry(&storage, false).unwrap();
        assert_eq!(
            (after.events, after.deliveries, after.entity_usage),
            (0, 0, 0)
        );
    }
}
