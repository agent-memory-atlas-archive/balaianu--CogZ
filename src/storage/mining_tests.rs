//! Tests for write-path mining.

use super::*;
use crate::storage::Storage;
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::events::{EventType, record_event};
use crate::storage::usage::{
    DeliveryKind, DeliveryTier, close_open_deliveries, record_delivered, record_delivery,
};

#[test]
fn hot_file_signal() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    for _ in 0..4 {
        record_event(
            &conn,
            EventType::FileSave,
            None,
            &serde_json::json!({"file_path": "src/hot.rs"}),
        )
        .unwrap();
    }
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/cold.rs"}),
    )
    .unwrap();

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    let hot: Vec<_> = out.iter().filter(|s| s.signal == "hot_file").collect();
    assert_eq!(hot.len(), 1);
    assert_eq!(hot[0].evidence["file_path"], "src/hot.rs");
    assert_eq!(hot[0].evidence["saves"], 4);
}

#[test]
fn recurring_use_signal() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    insert_entity(&conn, &Entity::new("e1", "observation", "Useful obs", "c")).unwrap();
    for _ in 0..3 {
        let d = record_delivery(&conn, DeliveryKind::Pack, None).unwrap();
        record_delivered(&conn, d, &[("e1".to_string(), DeliveryTier::Full)]).unwrap();
        crate::storage::usage::record_hits(&conn, &["e1".to_string()]).unwrap();
        close_open_deliveries(&conn).unwrap();
    }

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    let rec: Vec<_> = out.iter().filter(|s| s.signal == "recurring_use").collect();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].suggested_refs, vec!["e1"]);
}

#[test]
fn uncharted_edit_signal() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    // src/parser.rs must be indexable for the miss to count as a
    // knowledge gap rather than a coverage boundary.
    let mut file_entity = Entity::new("f1", "file", "parser.rs", "c");
    file_entity.file_path = Some("src/parser.rs".to_string());
    insert_entity(&conn, &file_entity).unwrap();
    let prompt_ev = record_event(
        &conn,
        EventType::PromptSubmit,
        None,
        &serde_json::json!({"prompt": "fix the parser"}),
    )
    .unwrap();
    let d = record_delivery(&conn, DeliveryKind::Pack, Some(prompt_ev)).unwrap();
    record_delivered(&conn, d, &[("e1".to_string(), DeliveryTier::Full)]).unwrap();
    close_open_deliveries(&conn).unwrap(); // e1 resolves to miss
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/parser.rs"}),
    )
    .unwrap();

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    let un: Vec<_> = out
        .iter()
        .filter(|s| s.signal == "uncharted_edit")
        .collect();
    assert_eq!(un.len(), 1);
    assert_eq!(
        un[0].evidence["files"],
        serde_json::json!(["src/parser.rs"])
    );
    assert_eq!(
        un[0].evidence["indexable_files"],
        serde_json::json!(["src/parser.rs"])
    );
}

#[test]
fn uncharted_edit_skips_unindexable_work() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    let prompt_ev = record_event(
        &conn,
        EventType::PromptSubmit,
        None,
        &serde_json::json!({"prompt": "update docs"}),
    )
    .unwrap();
    let d = record_delivery(&conn, DeliveryKind::Pack, Some(prompt_ev)).unwrap();
    record_delivered(&conn, d, &[("e1".to_string(), DeliveryTier::Full)]).unwrap();
    close_open_deliveries(&conn).unwrap();
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "docs/state.json"}),
    )
    .unwrap();

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    assert!(out.iter().all(|s| s.signal != "uncharted_edit"));
}

#[test]
fn error_fix_signal() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    record_event(
        &conn,
        EventType::PostToolUse,
        None,
        &serde_json::json!({"tool_name": "bash", "tool_result": "error: missing flag"}),
    )
    .unwrap();
    record_event(
        &conn,
        EventType::PostToolUse,
        None,
        &serde_json::json!({"tool_name": "bash", "tool_result": "ok"}),
    )
    .unwrap();

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    let ef: Vec<_> = out.iter().filter(|s| s.signal == "error_fix").collect();
    assert_eq!(ef.len(), 1);
    assert_eq!(ef[0].suggested_title, "bash error and fix");
}

#[test]
fn hit_pack_is_not_uncharted() {
    let s = Storage::open_memory().unwrap();
    let conn = s.conn();
    insert_entity(&conn, &Entity::new("e1", "observation", "t", "c")).unwrap();
    let ev = record_event(&conn, EventType::PromptSubmit, None, &serde_json::json!({})).unwrap();
    let d = record_delivery(&conn, DeliveryKind::Pack, Some(ev)).unwrap();
    record_delivered(&conn, d, &[("e1".to_string(), DeliveryTier::Full)]).unwrap();
    crate::storage::usage::record_hits(&conn, &["e1".to_string()]).unwrap();
    record_event(
        &conn,
        EventType::FileSave,
        None,
        &serde_json::json!({"file_path": "src/x.rs"}),
    )
    .unwrap();

    let out = mine_suggestions(&conn, 7, 10).unwrap();
    assert!(out.iter().all(|s| s.signal != "uncharted_edit"));
}
