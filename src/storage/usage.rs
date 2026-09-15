//! Usage instrumentation for delivered context (Phase 2 measurement).
//!
//! `deliveries` marks each boundary where context was handed to the
//! agent (a context pack on prompt_submit/session_start, or a search
//! result list). `entity_usage` records each entity in a delivery as
//! `pending` until a post_tool_use event marks it `hit` or the next
//! delivery boundary closes it as `miss`. All rows are disposable
//! derived state — canonical data still lives in the entity files.

use rusqlite::Connection;

use super::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryKind {
    Pack,
    Search,
}

impl DeliveryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pack => "pack",
            Self::Search => "search",
        }
    }

    fn from(s: &str) -> Result<Self, StorageError> {
        match s {
            "pack" => Ok(Self::Pack),
            "search" => Ok(Self::Search),
            other => Err(StorageError::InvalidUsage(format!(
                "unknown delivery kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Delivery {
    pub id: i64,
    pub kind: DeliveryKind,
    pub event_id: Option<i64>,
    pub closed: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct EntityUsage {
    pub delivery_id: i64,
    pub entity_id: String,
    pub outcome: String,
    pub created_at: String,
}

#[derive(Debug, Default, Clone)]
pub struct UsageSummary {
    pub hits: usize,
    pub misses: usize,
    pub pending: usize,
    pub hit_entity_ids: Vec<String>,
    pub miss_entity_ids: Vec<String>,
}

/// Open a new delivery and return its id. Callers should close the
/// previous open deliveries first (see `close_open_deliveries`).
pub fn record_delivery(
    conn: &Connection,
    kind: DeliveryKind,
    event_id: Option<i64>,
) -> Result<i64, StorageError> {
    conn.execute(
        "INSERT INTO deliveries (kind, event_id, closed, created_at) \
         VALUES (?, ?, 0, datetime('now'))",
        rusqlite::params![kind.as_str(), event_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Record each delivered entity as pending until a hit or the next
/// delivery boundary resolves it.
pub fn record_delivered(
    conn: &Connection,
    delivery_id: i64,
    entity_ids: &[String],
) -> Result<(), StorageError> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO entity_usage (delivery_id, entity_id, outcome, created_at) \
             SELECT ?, ?, 'pending', datetime('now') \
             WHERE EXISTS (SELECT 1 FROM entities WHERE id = ?)",
        )?;
        for id in entity_ids {
            stmt.execute(rusqlite::params![delivery_id, id, id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Mark entities as used across every open delivery. One entity may
/// appear in both the latest pack and a search result; the hit counts
/// against each delivery that surfaced it.
pub fn record_hits(conn: &Connection, entity_ids: &[String]) -> Result<usize, StorageError> {
    if entity_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction()?;
    let mut updated = 0usize;
    {
        let mut stmt = tx.prepare(
            "UPDATE entity_usage SET outcome = 'hit' \
             WHERE entity_id = ? AND outcome = 'pending' \
             AND delivery_id IN (SELECT id FROM deliveries WHERE closed = 0)",
        )?;
        for id in entity_ids {
            updated += stmt.execute(rusqlite::params![id])?;
        }
    }
    tx.commit()?;
    Ok(updated)
}

/// Close all open deliveries: pending rows become misses and the
/// deliveries are marked closed. Called at each new delivery boundary
/// and at session_end.
pub fn close_open_deliveries(conn: &Connection) -> Result<usize, StorageError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE entity_usage SET outcome = 'miss' WHERE outcome = 'pending' \
         AND delivery_id IN (SELECT id FROM deliveries WHERE closed = 0)",
        [],
    )?;
    let closed = tx.execute("UPDATE deliveries SET closed = 1 WHERE closed = 0", [])?;
    tx.commit()?;
    Ok(closed)
}

/// Entity ids whose file_path matches exactly, or sits under the given
/// path (useful when the hook reports a directory or a path with a
/// different leading prefix).
pub fn entities_for_file(conn: &Connection, file_path: &str) -> Result<Vec<String>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM entities \
         WHERE file_path = ? OR file_path LIKE ? ESCAPE '\\'",
    )?;
    let like = format!("%{}", escape_like(file_path));
    let rows = stmt.query_map(rusqlite::params![file_path, like], |r| r.get(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Entities still pending in open deliveries, with titles for
/// content-level hit detection.
pub fn pending_entities(conn: &Connection) -> Result<Vec<(String, String)>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT u.entity_id, e.title FROM entity_usage u \
         JOIN entities e ON e.id = u.entity_id \
         JOIN deliveries d ON d.id = u.delivery_id \
         WHERE u.outcome = 'pending' AND d.closed = 0",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Most recent delivery, if any.
pub fn latest_delivery(conn: &Connection) -> Result<Option<Delivery>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, event_id, closed, created_at FROM deliveries ORDER BY id DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    match rows.next() {
        Some(Ok((id, kind, event_id, closed, created_at))) => Ok(Some(Delivery {
            id,
            kind: DeliveryKind::from(&kind)?,
            event_id,
            closed: closed != 0,
            created_at,
        })),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Aggregate usage outcomes, optionally scoped to a delivery kind.
pub fn usage_summary(
    conn: &Connection,
    kind: Option<DeliveryKind>,
) -> Result<UsageSummary, StorageError> {
    let (sql, kind_param): (&str, Option<&str>) = match kind {
        Some(k) => (
            "SELECT u.outcome, u.entity_id FROM entity_usage u \
             JOIN deliveries d ON d.id = u.delivery_id WHERE d.kind = ?",
            Some(k.as_str()),
        ),
        None => ("SELECT outcome, entity_id FROM entity_usage", None),
    };
    let mut stmt = conn.prepare(sql)?;
    let mut summary = UsageSummary::default();
    let rows = stmt.query_map(rusqlite::params_from_iter(kind_param), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for r in rows {
        let (outcome, entity_id) = r?;
        match outcome.as_str() {
            "hit" => {
                summary.hits += 1;
                summary.hit_entity_ids.push(entity_id);
            }
            "miss" => {
                summary.misses += 1;
                summary.miss_entity_ids.push(entity_id);
            }
            _ => summary.pending += 1,
        }
    }
    Ok(summary)
}

/// Entities never delivered in any recorded delivery — the dead-weight
/// candidates for doctor reporting.
pub fn never_delivered(conn: &Connection) -> Result<Vec<String>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM entities WHERE status != 'pruned' \
         AND id NOT IN (SELECT DISTINCT entity_id FROM entity_usage)",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Entities never delivered since a timestamp — dead weight scoped to
/// the recent window (e.g. "last N sessions").
pub fn never_delivered_since(conn: &Connection, since: &str) -> Result<Vec<String>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM entities WHERE status != 'pruned' \
         AND id NOT IN (SELECT DISTINCT u.entity_id FROM entity_usage u \
                        JOIN deliveries d ON d.id = u.delivery_id \
                        WHERE d.created_at >= ?)",
    )?;
    let rows = stmt.query_map(rusqlite::params![since], |r| r.get(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Timestamp of the N-th most recent session_start event — the cutoff
/// for "last N sessions" scoping. None when fewer than N sessions exist.
pub fn session_cutoff(conn: &Connection, n: usize) -> Result<Option<String>, StorageError> {
    conn.query_row(
        "SELECT created_at FROM events WHERE event_type = 'session_start' \
         ORDER BY id DESC LIMIT 1 OFFSET ?",
        rusqlite::params![n.saturating_sub(1) as i64],
        |r| r.get(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other.into()),
    })
}

/// File-backed entity types never delivered — write-quality signal:
/// an observation, rule, or knowledge entry that retrieval never
/// surfaces may be unreadable, mis-titled, or irrelevant.
pub fn never_delivered_by_type(
    conn: &Connection,
    types: &[&str],
) -> Result<Vec<String>, StorageError> {
    if types.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = types.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id FROM entities WHERE status != 'pruned' \
         AND type IN ({placeholders}) \
         AND id NOT IN (SELECT DISTINCT entity_id FROM entity_usage)"
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        types.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |r| r.get(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn insert_entity(conn: &Connection, id: &str, file_path: &str) {
        conn.execute(
            "INSERT INTO entities (id, type, title, content, file_path, status, created_at, updated_at) \
             VALUES (?, 'knowledge', ?, 'c', ?, 'active', '2026-01-01', '2026-01-01')",
            rusqlite::params![id, format!("title-{id}"), file_path],
        )
        .unwrap();
    }

    #[test]
    fn delivery_records_pending_entities() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        insert_entity(&conn, "e1", "knowledge/x.md");
        insert_entity(&conn, "e2", "knowledge/y.md");
        let did = record_delivery(&conn, DeliveryKind::Pack, None).unwrap();
        record_delivered(&conn, did, &["e1".to_string(), "e2".to_string()]).unwrap();
        let s = usage_summary(&conn, Some(DeliveryKind::Pack)).unwrap();
        assert_eq!(s.pending, 2);
        assert_eq!(s.hits, 0);
    }

    #[test]
    fn hit_marks_entity_used_across_open_deliveries() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        insert_entity(&conn, "e1", "knowledge/x.md");
        let d1 = record_delivery(&conn, DeliveryKind::Pack, None).unwrap();
        let d2 = record_delivery(&conn, DeliveryKind::Search, None).unwrap();
        record_delivered(&conn, d1, &["e1".to_string()]).unwrap();
        record_delivered(&conn, d2, &["e1".to_string()]).unwrap();
        let updated = record_hits(&conn, &["e1".to_string()]).unwrap();
        assert_eq!(updated, 2);
        let s = usage_summary(&conn, None).unwrap();
        assert_eq!(s.hits, 2);
        assert_eq!(s.pending, 0);
    }

    #[test]
    fn boundary_close_flips_pending_to_miss() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        insert_entity(&conn, "e1", "knowledge/x.md");
        insert_entity(&conn, "e2", "knowledge/y.md");
        let d1 = record_delivery(&conn, DeliveryKind::Pack, None).unwrap();
        record_delivered(&conn, d1, &["e1".to_string(), "e2".to_string()]).unwrap();
        record_hits(&conn, &["e1".to_string()]).unwrap();
        close_open_deliveries(&conn).unwrap();
        let s = usage_summary(&conn, Some(DeliveryKind::Pack)).unwrap();
        assert_eq!(s.hits, 1);
        assert_eq!(s.misses, 1);
        assert_eq!(s.miss_entity_ids, vec!["e2".to_string()]);

        // Hits recorded after close must not resurrect misses.
        let updated = record_hits(&conn, &["e2".to_string()]).unwrap();
        assert_eq!(updated, 0);
    }

    #[test]
    fn entities_for_file_matches_exact_and_partial() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        insert_entity(&conn, "e1", "src/storage/usage.rs");
        assert_eq!(
            entities_for_file(&conn, "src/storage/usage.rs").unwrap(),
            vec!["e1"]
        );
        assert_eq!(
            entities_for_file(&conn, "storage/usage.rs").unwrap(),
            vec!["e1"]
        );
        assert!(
            entities_for_file(&conn, "knowledge/other.md")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn never_delivered_excludes_delivered_and_pruned() {
        let storage = Storage::open_memory().unwrap();
        let conn = storage.conn();
        insert_entity(&conn, "e1", "knowledge/x.md");
        insert_entity(&conn, "e2", "knowledge/y.md");
        insert_entity(&conn, "e3", "knowledge/z.md");
        conn.execute("UPDATE entities SET status = 'pruned' WHERE id = 'e3'", [])
            .unwrap();
        let did = record_delivery(&conn, DeliveryKind::Search, None).unwrap();
        record_delivered(&conn, did, &["e1".to_string()]).unwrap();
        let mut dead = never_delivered(&conn).unwrap();
        dead.sort();
        assert_eq!(dead, vec!["e2".to_string()]);
    }
}
