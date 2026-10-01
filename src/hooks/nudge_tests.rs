//! Tests for write-back nudges — dedup, impressions, formatting.

use super::*;
use crate::storage::Storage;
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::events::{EventType, record_event};

fn indexed_file(conn: &rusqlite::Connection, path: &str) {
    let mut e = Entity::new(&format!("id-{path}"), "file", path, "c");
    e.file_path = Some(path.to_string());
    insert_entity(conn, &e).unwrap();
}

/// A zero-hit search then an indexed save is the canonical nudge
/// scenario; both nudge calls share it.
fn seed_search_miss(conn: &rusqlite::Connection) {
    indexed_file(conn, "src/answer.rs");
    record_event(
        conn,
        EventType::SearchPerformed,
        None,
        &serde_json::json!({"query": "queue busy notification", "returned": 0}),
    )
    .unwrap();
    record_event(
        conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/answer.rs"}),
    )
    .unwrap();
}

#[test]
fn fresh_suggestion_records_impression() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    seed_search_miss(&conn);

    let fresh = fresh_suggestions(&conn, "file_save");
    assert!(
        fresh.iter().any(|s| s.signal == "search_miss"),
        "search_miss should surface: {fresh:?}"
    );

    let impressions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'write_nudge_shown'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(impressions, 1);
}

#[test]
fn same_candidate_is_suppressed_within_window() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    seed_search_miss(&conn);

    let first = fresh_suggestions(&conn, "file_save");
    assert_eq!(first.len(), 1);
    // Second surface minutes later: nothing new to say.
    assert!(fresh_suggestions(&conn, "search").is_empty());
}

#[test]
fn expired_impression_stops_suppressing() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    seed_search_miss(&conn);
    assert_eq!(fresh_suggestions(&conn, "file_save").len(), 1);

    // Age the impression to just outside the 24h window but on the
    // cutoff's calendar date, kept in production RFC3339 — a naive
    // compare reads 'T' > ' ' and would hold the suppression past expiry.
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    let stale = (cutoff - chrono::Duration::hours(1)).max(
        cutoff.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()
            + chrono::Duration::milliseconds(1),
    );
    conn.execute(
        "UPDATE events SET created_at = ?1 WHERE event_type = 'write_nudge_shown'",
        rusqlite::params![stale.to_rfc3339()],
    )
    .unwrap();

    let next = fresh_suggestions(&conn, "file_save");
    assert_eq!(
        next.len(),
        1,
        "expired impression must not suppress: {next:?}"
    );
}

#[test]
fn new_candidate_still_surfaces_after_dedup() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    seed_search_miss(&conn);
    assert_eq!(fresh_suggestions(&conn, "file_save").len(), 1);

    // A second, distinct signal — hot file needs cross-day saves now:
    // same-day churn is task noise, not a durable signal.
    indexed_file(&conn, "src/hot.rs");
    for _ in 0..3 {
        record_event(
            &conn,
            EventType::FileSave,
            None,
            &serde_json::json!({"file_path": "src/hot.rs"}),
        )
        .unwrap();
    }
    // Different calendar date yet inside the 24h mining window: only a
    // timestamp just short of a day ago satisfies both (a full -1 day
    // lands exactly on/outside the window edge).
    conn.execute(
        "UPDATE events SET created_at = datetime('now', '-86390 seconds')
         WHERE id IN (
             SELECT id FROM events
             WHERE event_type = 'file_save'
               AND json_extract(payload, '$.file_path') = 'src/hot.rs'
             LIMIT 1)",
        [],
    )
    .unwrap();
    let next = fresh_suggestions(&conn, "file_save");
    assert!(
        next.iter().any(|s| s.signal == "hot_file"),
        "a new candidate must not be suppressed by the earlier nudge"
    );
    assert!(next.iter().all(|s| s.signal != "search_miss"));
}

#[test]
fn no_signal_stays_silent() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/x.rs"}),
    )
    .unwrap();
    assert!(fresh_suggestions(&conn, "file_save").is_empty());
    let impressions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'write_nudge_shown'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(impressions, 0, "silence must not mint impressions");
}

#[test]
fn fingerprint_is_stable_across_volatile_evidence() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    indexed_file(&conn, "src/hot.rs");
    for _ in 0..3 {
        record_event(
            &conn,
            EventType::FileSave,
            None,
            &serde_json::json!({"file_path": "src/hot.rs"}),
        )
        .unwrap();
    }
    // hot_file requires recurrence across distinct days — backdate one
    // save to just inside the 24h window so the signal fires (a full
    // -1 day lands on/outside the window edge).
    conn.execute(
        "UPDATE events SET created_at = datetime('now', '-86390 seconds')
         WHERE id = (
             SELECT MIN(id) FROM events
             WHERE event_type = 'file_save'
               AND json_extract(payload, '$.file_path') = 'src/hot.rs')",
        [],
    )
    .unwrap();
    let first = mining::mine_suggestions(&conn, 1, 8).unwrap();
    let fp1 = first[0].fingerprint();

    // A fourth save bumps the evidence count — the fingerprint must
    // not mint anew, or dedup never holds for hot files.
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/hot.rs"}),
    )
    .unwrap();
    let second = mining::mine_suggestions(&conn, 1, 8).unwrap();
    let fp2 = second[0].fingerprint();
    assert_eq!(fp1, fp2, "volatile evidence must not shift identity");
    assert_eq!(fp1, "hot_file:src/hot.rs");
}

#[test]
fn search_miss_ignores_hits_and_empty_queries() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    indexed_file(&conn, "src/answer.rs");
    for payload in [
        serde_json::json!({"query": "found it", "returned": 3}),
        serde_json::json!({"query": "", "returned": 0}),
    ] {
        record_event(&conn, EventType::SearchPerformed, None, &payload).unwrap();
    }
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/answer.rs"}),
    )
    .unwrap();

    let out = mining::mine_suggestions(&conn, 1, 8).unwrap();
    assert!(out.iter().all(|s| s.signal != "search_miss"));
}

#[test]
fn search_miss_stops_at_prompt_boundary() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    indexed_file(&conn, "src/answer.rs");
    record_event(
        &conn,
        EventType::SearchPerformed,
        None,
        &serde_json::json!({"query": "q", "returned": 0}),
    )
    .unwrap();
    // The save lands in the NEXT prompt's territory — not evidence
    // that the miss drove the discovery.
    record_event(&conn, EventType::PromptSubmit, None, &serde_json::json!({})).unwrap();
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/answer.rs"}),
    )
    .unwrap();

    let out = mining::mine_suggestions(&conn, 1, 8).unwrap();
    assert!(out.iter().all(|s| s.signal != "search_miss"));
}

#[test]
fn file_path_rel_is_matched_when_raw_is_absolute() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    indexed_file(&conn, "src/answer.rs");
    record_event(
        &conn,
        EventType::SearchPerformed,
        None,
        &serde_json::json!({"query": "q", "returned": 0}),
    )
    .unwrap();
    // Hooks hand absolute paths; the normalized form is what mining
    // can match against entities.file_path.
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({
            "file_path": "/home/u/repo/src/answer.rs",
            "file_path_rel": "src/answer.rs",
        }),
    )
    .unwrap();

    let out = mining::mine_suggestions(&conn, 1, 8).unwrap();
    assert!(
        out.iter().any(|s| s.signal == "search_miss"),
        "rel-normalized save should resolve the miss: {out:?}"
    );
}

#[test]
fn format_carries_the_draft() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    seed_search_miss(&conn);
    let fresh = fresh_suggestions(&conn, "file_save");

    let md = format_markdown(&fresh);
    assert!(md.contains("Search missed: queue busy notification"));
    assert!(md.contains("create_entity"));
    assert!(md.contains("cogz suggest"));

    let json = format_json(&fresh);
    assert_eq!(json["candidates"], 1);
    assert_eq!(json["top"]["signal"], "search_miss");
    assert!(
        json["top"]["content"]
            .as_str()
            .unwrap()
            .contains("src/answer.rs")
    );
}
