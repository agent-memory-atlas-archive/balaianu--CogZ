use super::*;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::edges::{Edge, insert_edge};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

const K1: &str = "11111111-1111-4111-8111-111111111111";
const STALE_A: &str = "aaaa0000-0000-4000-8000-000000000001";
const STALE_B: &str = "aaaa0000-0000-4000-8000-000000000002";
const LIVE_X: &str = "bbbb0000-0000-4000-8000-000000000001";
const LIVE_Y: &str = "bbbb0000-0000-4000-8000-000000000002";

fn setup() -> (TempDir, Storage, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(cogz_dir.join("knowledge/uncategorized")).unwrap();
    let storage = Storage::open(&cogz_dir.join("cogz.db"), 768).unwrap();
    (dir, storage, cogz_dir)
}

fn code_row(id: &str, hash: &str, status: &str, qname: &str) -> Entity {
    let mut e = Entity::new(id, "function", qname, "fn body() {}");
    e.content_hash = Some(hash.to_string());
    e.status = status.to_string();
    e.properties = serde_json::json!({"qualified_name": qname, "language": "rust"});
    e
}

fn knowledge(
    storage: &Storage,
    cogz_dir: &Path,
    id: &str,
    refs: &[&str],
    verified: &[(&str, &str)],
    ftype: FileEntityType,
) -> PathBuf {
    let title = format!("note-{id}");
    let mut ef = EntityFile::new(&title, ftype, "content");
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = refs.iter().map(|s| s.to_string()).collect();
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
    let db_type = match ftype {
        FileEntityType::Observation => "observation",
        FileEntityType::Rule => "rule",
        _ => "knowledge",
    };
    let mut db = Entity::new(id, db_type, &title, "content");
    db.file_path = Some(rel);
    insert_entity(&conn, &db).unwrap();
    for r in refs {
        let _ = insert_edge(
            &conn,
            &Edge {
                source_id: id.to_string(),
                target_id: r.to_string(),
                edge_type: "references".to_string(),
                weight: 1.0,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        );
    }
    drop(conn);
    path
}

fn read_refs(path: &Path) -> (Vec<String>, Vec<String>) {
    let ef = read_entity_file(path).unwrap();
    let verified = match ef.frontmatter.get("verified_against") {
        Some(FmValue::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    (ef.references, verified)
}

#[test]
fn moved_entity_reanchors_file_first() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(STALE_A, "v2:hashA", "stale", "f")).unwrap();
        insert_entity(&conn, &code_row(LIVE_X, "v2:hashA", "active", "f")).unwrap();
    }
    let path = knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_A],
        &[(STALE_A, "v2:hashA")],
        FileEntityType::Knowledge,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_A.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 1);
    assert_eq!(stats.files_rewritten, 1);
    assert_eq!(stats.unresolved, 0);
    assert_eq!(declared[K1], vec![LIVE_X.to_string()]);

    let (refs, verified) = read_refs(&path);
    assert_eq!(refs, vec![LIVE_X.to_string()]);
    assert_eq!(verified, vec![format!("{LIVE_X}=v2:hashA")]);

    // Edge retargeted through the resync.
    let conn = storage.conn();
    let target: String = conn
        .query_row(
            "SELECT target_id FROM edges WHERE source_id = ?1 AND edge_type = 'references'",
            [K1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(target, LIVE_X);
}

#[test]
fn ambiguous_match_is_not_reanchored() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(STALE_A, "v2:dup", "stale", "f")).unwrap();
        // Two identical live entities — which one is the moved code is
        // unknowable, so the ref must stay put.
        insert_entity(&conn, &code_row(LIVE_X, "v2:dup", "active", "g")).unwrap();
        insert_entity(&conn, &code_row(LIVE_Y, "v2:dup", "active", "h")).unwrap();
    }
    let path = knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_A],
        &[(STALE_A, "v2:dup")],
        FileEntityType::Knowledge,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_A.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 0);
    assert_eq!(stats.unresolved, 1);
    assert_eq!(declared[K1], vec![STALE_A.to_string()]);
    let (refs, _) = read_refs(&path);
    assert_eq!(refs, vec![STALE_A.to_string()]);
}

#[test]
fn duplicate_names_disambiguate_by_qualified_name() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(STALE_A, "v2:dup", "stale", "mod_a::f")).unwrap();
        insert_entity(&conn, &code_row(LIVE_X, "v2:dup", "active", "mod_a::f")).unwrap();
        insert_entity(&conn, &code_row(LIVE_Y, "v2:dup", "active", "mod_b::f")).unwrap();
    }
    knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_A],
        &[(STALE_A, "v2:dup")],
        FileEntityType::Knowledge,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_A.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 1);
    assert_eq!(declared[K1], vec![LIVE_X.to_string()]);
}

#[test]
fn record_referrer_is_never_rewritten() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(STALE_A, "v2:hashA", "stale", "f")).unwrap();
        insert_entity(&conn, &code_row(LIVE_X, "v2:hashA", "active", "f")).unwrap();
    }
    // Observations classify as record — their refs are historical.
    let path = knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_A],
        &[(STALE_A, "v2:hashA")],
        FileEntityType::Observation,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_A.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 0);
    assert_eq!(declared[K1], vec![STALE_A.to_string()]);
    let (refs, verified) = read_refs(&path);
    assert_eq!(refs, vec![STALE_A.to_string()]);
    assert_eq!(verified, vec![format!("{STALE_A}=v2:hashA")]);
}

#[test]
fn renamed_entity_stays_missing() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(STALE_A, "v2:hashA", "stale", "f")).unwrap();
        // Same shape, different name → different structural hash.
        insert_entity(&conn, &code_row(LIVE_X, "v2:hashB", "active", "g")).unwrap();
    }
    let path = knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_A],
        &[(STALE_A, "v2:hashA")],
        FileEntityType::Knowledge,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_A.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 0);
    assert_eq!(stats.unresolved, 1);
    let (refs, _) = read_refs(&path);
    assert_eq!(refs, vec![STALE_A.to_string()]);
}

#[test]
fn legacy_stale_hash_matches_via_name_and_content() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        // Pre-v2 stale row: legacy normalized-content hash.
        let legacy = crate::index::sync::legacy_content_hash("fn body() {}", "rust");
        insert_entity(&conn, &code_row(STALE_B, &legacy, "stale", "moved_fn")).unwrap();
        let mut live = code_row(LIVE_X, "v2:whatever", "active", "moved_fn");
        live.content = "fn body() {}".to_string();
        insert_entity(&conn, &live).unwrap();
        // Same name but genuinely different content → excluded.
        let mut other = code_row(LIVE_Y, "v2:other", "active", "moved_fn");
        other.content = "fn body() { changed(); }".to_string();
        insert_entity(&conn, &other).unwrap();
    }
    knowledge(
        &storage,
        &cogz,
        K1,
        &[STALE_B],
        &[(STALE_B, "v2:hashA")],
        FileEntityType::Knowledge,
    );

    let mut declared: HashMap<String, Vec<String>> =
        HashMap::from([(K1.to_string(), vec![STALE_B.to_string()])]);
    let stats = reanchor_moved_entities(&storage, &cogz, &mut declared);

    assert_eq!(stats.reanchored, 1);
    assert_eq!(declared[K1], vec![LIVE_X.to_string()]);
}
