//! Shared fixtures for the drift/verify/post-index test modules.
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::files::frontmatter::FmValue;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::storage::Storage;
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::edges::{Edge, insert_edge};

pub(crate) const K1: &str = "11111111-1111-4111-8111-111111111111";
pub(crate) const K2: &str = "22222222-2222-4222-8222-222222222222";
pub(crate) const K3: &str = "33333333-3333-4333-8333-333333333333";
pub(crate) const K4: &str = "44444444-4444-4444-8444-444444444444";
pub(crate) const K_ORPHAN: &str = "55555555-5555-4555-8555-555555555555";
pub(crate) const K_MANUAL: &str = "66666666-6666-4666-8666-666666666666";
pub(crate) const K_DEAD: &str = "77777777-7777-4777-8777-777777777777";
pub(crate) const K_LIVE: &str = "88888888-8888-4888-8888-888888888888";

pub(crate) fn setup() -> (TempDir, Storage, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(cogz_dir.join("knowledge/uncategorized")).unwrap();
    let storage = Storage::open(&cogz_dir.join("cogz.db"), 768).unwrap();
    (dir, storage, cogz_dir)
}

pub(crate) fn make_code(id: &str, hash: &str) -> Entity {
    let mut e = Entity::new(id, "function", "func", "fn func() {}");
    e.content_hash = Some(hash.to_string());
    e
}

/// Write a knowledge file + DB row with a references edge to `code_id`.
/// `verified` entries become the `verified_against` frontmatter array.
pub(crate) fn make_knowledge(
    storage: &Storage,
    cogz_dir: &Path,
    id: &str,
    code_id: &str,
    verified: &[(&str, &str)],
) -> EntityFile {
    let title = format!("note-{id}");
    let mut ef = EntityFile::new(&title, FileEntityType::Knowledge, "content");
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = vec![code_id.to_string()];
    if !verified.is_empty() {
        let entries: Vec<String> = verified.iter().map(|(k, v)| format!("{k}={v}")).collect();
        ef.frontmatter
            .insert("verified_against", FmValue::Array(entries));
    }
    let path = ef.file_path(cogz_dir);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    write_entity_file(&path, &ef).unwrap();
    let rel = path
        .strip_prefix(cogz_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();

    let conn = storage.conn();
    let mut db = Entity::new(id, "knowledge", &title, "content");
    db.file_path = Some(rel);
    if !verified.is_empty() {
        let arr: Vec<serde_json::Value> = verified
            .iter()
            .map(|(k, v)| serde_json::json!(format!("{k}={v}")))
            .collect();
        db.properties = serde_json::json!({"verified_against": arr});
    }
    insert_entity(&conn, &db).unwrap();
    insert_edge(
        &conn,
        &Edge {
            source_id: id.to_string(),
            target_id: code_id.to_string(),
            edge_type: "references".to_string(),
            weight: 1.0,
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .unwrap();
    drop(conn);
    ef
}

/// Knowledge file + DB row declaring `references` in frontmatter but
/// WITHOUT the materialized edge — the FK-dropped scenario where the
/// target didn't exist when the file was synced.
pub(crate) fn make_knowledge_no_edge(
    storage: &Storage,
    cogz_dir: &Path,
    id: &str,
    code_id: &str,
    verified: &[(&str, &str)],
) -> EntityFile {
    let title = format!("note-{id}");
    let mut ef = EntityFile::new(&title, FileEntityType::Knowledge, "content");
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = vec![code_id.to_string()];
    if !verified.is_empty() {
        let entries: Vec<String> = verified.iter().map(|(k, v)| format!("{k}={v}")).collect();
        ef.frontmatter
            .insert("verified_against", FmValue::Array(entries));
    }
    let path = ef.file_path(cogz_dir);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    write_entity_file(&path, &ef).unwrap();
    let rel = path
        .strip_prefix(cogz_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();

    let conn = storage.conn();
    let mut db = Entity::new(id, "knowledge", &title, "content");
    db.file_path = Some(rel);
    if !verified.is_empty() {
        let arr: Vec<serde_json::Value> = verified
            .iter()
            .map(|(k, v)| serde_json::json!(format!("{k}={v}")))
            .collect();
        db.properties = serde_json::json!({"verified_against": arr});
    }
    insert_entity(&conn, &db).unwrap();
    drop(conn);
    ef
}

pub(crate) fn make_auto_ref_edge(storage: &Storage, source_id: &str, target_id: &str) {
    let conn = storage.conn();
    insert_edge(
        &conn,
        &Edge {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            edge_type: "auto_references".to_string(),
            weight: 1.0,
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .unwrap();
}

pub(crate) fn edge_exists(
    storage: &Storage,
    source_id: &str,
    target_id: &str,
    edge_type: &str,
) -> bool {
    let conn = storage.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE source_id=?1 AND target_id=?2 AND edge_type=?3",
        rusqlite::params![source_id, target_id, edge_type],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

pub(crate) fn mark_stale(
    storage: &Storage,
    cogz_dir: &Path,
    ef: &EntityFile,
    reason: Option<&str>,
) {
    let mut stale = ef.clone();
    stale.status = "stale".to_string();
    if let Some(r) = reason {
        stale
            .frontmatter
            .insert("stale_reason", FmValue::String(r.to_string()));
    }
    write_entity_file(&stale.file_path(cogz_dir), &stale).unwrap();
    let conn = storage.conn();
    conn.execute("UPDATE entities SET status='stale' WHERE id=?1", [&ef.id])
        .unwrap();
}

pub(crate) fn drift_rows(storage: &Storage, entity_id: &str) -> Vec<(String, String)> {
    let conn = storage.conn();
    let mut stmt = conn
        .prepare("SELECT code_id, cause FROM entity_drift WHERE entity_id = ?1")
        .unwrap();
    stmt.query_map([entity_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .flatten()
        .collect()
}

pub(crate) fn make_typed_entity(
    storage: &Storage,
    cogz_dir: &Path,
    id: &str,
    ftype: FileEntityType,
    extra_props: serde_json::Value,
    code_id: &str,
    verified: &[(&str, &str)],
) -> EntityFile {
    let title = format!("note-{id}");
    let mut ef = EntityFile::new(&title, ftype, "content");
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = vec![code_id.to_string()];
    if !verified.is_empty() {
        let entries: Vec<String> = verified.iter().map(|(k, v)| format!("{k}={v}")).collect();
        ef.frontmatter
            .insert("verified_against", FmValue::Array(entries));
    }
    let mut props = serde_json::Map::new();
    if !verified.is_empty() {
        let arr: Vec<serde_json::Value> = verified
            .iter()
            .map(|(k, v)| serde_json::json!(format!("{k}={v}")))
            .collect();
        props.insert("verified_against".to_string(), serde_json::json!(arr));
    }
    for (k, v) in extra_props.as_object().into_iter().flatten() {
        props.insert(k.clone(), v.clone());
        if let Some(s) = v.as_str() {
            ef.frontmatter.insert(k, FmValue::String(s.to_string()));
        }
    }
    let path = ef.file_path(cogz_dir);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    write_entity_file(&path, &ef).unwrap();
    let rel = path
        .strip_prefix(cogz_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();

    let conn = storage.conn();
    let mut db = Entity::new(id, ftype.as_str(), &title, "content");
    db.file_path = Some(rel);
    db.properties = serde_json::Value::Object(props);
    insert_entity(&conn, &db).unwrap();
    drop(conn);
    ef
}

pub(crate) const CODE_A: &str = "aaaa0000-0000-4000-8000-000000000001";
pub(crate) const CODE_B: &str = "aaaa0000-0000-4000-8000-000000000002";

pub(crate) fn code_row(id: &str, content: &str) -> Entity {
    let mut e = Entity::new(id, "function", "func", content);
    e.content_hash = Some("h".to_string());
    e
}

/// Entity file + DB row + references edges.
pub(crate) fn entity(
    storage: &Storage,
    cogz_dir: &Path,
    id: &str,
    body: &str,
    refs: &[&str],
    ftype: FileEntityType,
) -> EntityFile {
    let title = format!("note-{id}");
    let mut ef = EntityFile::new(&title, ftype, body);
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = refs.iter().map(|s| s.to_string()).collect();
    let path = ef.file_path(cogz_dir);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    write_entity_file(&path, &ef).unwrap();
    let rel = path
        .strip_prefix(cogz_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();

    let conn = storage.conn();
    let mut db = Entity::new(id, ftype.as_str(), &title, body);
    db.file_path = Some(rel);
    insert_entity(&conn, &db).unwrap();
    for r in refs {
        insert_edge(
            &conn,
            &Edge {
                source_id: id.to_string(),
                target_id: r.to_string(),
                edge_type: "references".to_string(),
                weight: 1.0,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .unwrap();
    }
    drop(conn);
    ef
}

pub(crate) fn read_back(cogz_dir: &Path, ef: &EntityFile) -> EntityFile {
    crate::files::read_entity_file(&ef.file_path(cogz_dir)).unwrap()
}
