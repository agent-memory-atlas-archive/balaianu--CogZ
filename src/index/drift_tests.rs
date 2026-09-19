use super::*;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::edges::{Edge, insert_edge};
use tempfile::TempDir;

const K1: &str = "11111111-1111-4111-8111-111111111111";
const K2: &str = "22222222-2222-4222-8222-222222222222";
const K3: &str = "33333333-3333-4333-8333-333333333333";
const K4: &str = "44444444-4444-4444-8444-444444444444";
const K_ORPHAN: &str = "55555555-5555-4555-8555-555555555555";
const K_MANUAL: &str = "66666666-6666-4666-8666-666666666666";
const K_DEAD: &str = "77777777-7777-4777-8777-777777777777";
const K_LIVE: &str = "88888888-8888-4888-8888-888888888888";

fn setup() -> (TempDir, Storage, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(cogz_dir.join("knowledge/uncategorized")).unwrap();
    let storage = Storage::open(&cogz_dir.join("cogz.db"), 768).unwrap();
    (dir, storage, cogz_dir)
}

fn make_code(id: &str, hash: &str) -> Entity {
    let mut e = Entity::new(id, "function", "func", "fn func() {}");
    e.content_hash = Some(hash.to_string());
    e
}

/// Write a knowledge file + DB row with a references edge to `code_id`.
/// `verified` entries become the `verified_against` frontmatter array.
fn make_knowledge(
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
fn make_knowledge_no_edge(
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

fn make_auto_ref_edge(storage: &Storage, source_id: &str, target_id: &str) {
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

fn edge_exists(storage: &Storage, source_id: &str, target_id: &str, edge_type: &str) -> bool {
    let conn = storage.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE source_id=?1 AND target_id=?2 AND edge_type=?3",
        rusqlite::params![source_id, target_id, edge_type],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

fn mark_stale(storage: &Storage, cogz_dir: &Path, ef: &EntityFile, reason: Option<&str>) {
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

fn drift_rows(storage: &Storage, entity_id: &str) -> Vec<(String, String)> {
    let conn = storage.conn();
    let mut stmt = conn
        .prepare("SELECT code_id, cause FROM entity_drift WHERE entity_id = ?1")
        .unwrap();
    stmt.query_map([entity_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .flatten()
        .collect()
}

#[test]
fn recompute_flags_changed_missing_unverified() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000c1", "h-new"),
        )
        .unwrap();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000c2", "h1"),
        )
        .unwrap();
        let mut dead = make_code("aaaa0000-0000-4000-8000-0000000000c3", "h1");
        dead.status = "stale".to_string();
        insert_entity(&conn, &dead).unwrap();
    }
    make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-0000000000c1",
        &[("aaaa0000-0000-4000-8000-0000000000c1", "h-old")],
    );
    make_knowledge(
        &storage,
        &cogz,
        K2,
        "aaaa0000-0000-4000-8000-0000000000c3",
        &[("aaaa0000-0000-4000-8000-0000000000c3", "h1")],
    );
    make_knowledge(
        &storage,
        &cogz,
        K3,
        "aaaa0000-0000-4000-8000-0000000000c2",
        &[],
    );
    make_knowledge(
        &storage,
        &cogz,
        K4,
        "aaaa0000-0000-4000-8000-0000000000c1",
        &[("aaaa0000-0000-4000-8000-0000000000c1", "h-new")],
    );

    let stats = recompute(&storage, &declared_references(&storage, &cogz));
    assert_eq!(stats.entities_with_drift, 3);
    assert_eq!(
        drift_rows(&storage, K1),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000c1".into(),
            "changed".into()
        )]
    );
    assert_eq!(
        drift_rows(&storage, K2),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000c3".into(),
            "missing".into()
        )]
    );
    assert_eq!(
        drift_rows(&storage, K3),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000c2".into(),
            "unverified".into()
        )]
    );
    assert!(drift_rows(&storage, K4).is_empty());
}

#[test]
fn drift_counts_batches() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000011", "h-new"),
        )
        .unwrap();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000012", "h-new"),
        )
        .unwrap();
    }
    make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-000000000011",
        &[("aaaa0000-0000-4000-8000-000000000011", "h-old")],
    );
    make_knowledge(
        &storage,
        &cogz,
        K2,
        "aaaa0000-0000-4000-8000-000000000012",
        &[("aaaa0000-0000-4000-8000-000000000012", "h-old")],
    );
    recompute(&storage, &declared_references(&storage, &cogz));

    let conn = storage.conn();
    let counts = drift_counts(&conn, &[K1.to_string(), K2.to_string(), "nope".to_string()]);
    assert_eq!(counts.get(K1), Some(&1));
    assert_eq!(counts.get(K2), Some(&1));
    assert!(!counts.contains_key("nope"));
}

#[test]
fn backfill_stamps_only_missing_entries() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000011", "h1"),
        )
        .unwrap();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000012", "h2"),
        )
        .unwrap();
    }
    // K1 has no provenance → stamped. K2 keeps its existing entry.
    let ef1 = make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-000000000011",
        &[],
    );
    let ef2 = make_knowledge(
        &storage,
        &cogz,
        K2,
        "aaaa0000-0000-4000-8000-000000000012",
        &[("aaaa0000-0000-4000-8000-000000000012", "keep-me")],
    );

    let stamped = backfill_verified_against(&storage, &cogz, &declared_references(&storage, &cogz));
    assert_eq!(stamped, 1);

    let f1 = read_entity_file(&ef1.file_path(&cogz)).unwrap();
    let v1 = parse_verified_frontmatter(&f1.frontmatter);
    assert_eq!(
        v1.get("aaaa0000-0000-4000-8000-000000000011")
            .map(String::as_str),
        Some("h1")
    );

    let f2 = read_entity_file(&ef2.file_path(&cogz)).unwrap();
    let v2 = parse_verified_frontmatter(&f2.frontmatter);
    assert_eq!(
        v2.get("aaaa0000-0000-4000-8000-000000000012")
            .map(String::as_str),
        Some("keep-me")
    );
}

#[test]
fn heal_only_recovers_orphaned_stale_with_live_refs() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000011", "h1"),
        )
        .unwrap();
        let mut dead = make_code("aaaa0000-0000-4000-8000-000000000012", "h2");
        dead.status = "stale".to_string();
        insert_entity(&conn, &dead).unwrap();
    }
    let ef_orphan = make_knowledge(
        &storage,
        &cogz,
        K_ORPHAN,
        "aaaa0000-0000-4000-8000-000000000011",
        &[],
    );
    let ef_manual = make_knowledge(
        &storage,
        &cogz,
        K_MANUAL,
        "aaaa0000-0000-4000-8000-000000000011",
        &[],
    );
    let ef_dead = make_knowledge(
        &storage,
        &cogz,
        K_DEAD,
        "aaaa0000-0000-4000-8000-000000000012",
        &[],
    );
    mark_stale(&storage, &cogz, &ef_orphan, Some(STALE_REASON_ORPHANED));
    mark_stale(&storage, &cogz, &ef_manual, None);
    mark_stale(&storage, &cogz, &ef_dead, Some(STALE_REASON_ORPHANED));

    assert_eq!(heal_stale_entities(&storage, &cogz), 1);

    let conn = storage.conn();
    for (id, expected) in [(K_ORPHAN, "active"), (K_MANUAL, "stale"), (K_DEAD, "stale")] {
        let status: String = conn
            .query_row("SELECT status FROM entities WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, expected, "entity {id}");
    }
    let events = storage::events::get_recent_events(&conn, "stale_recovered", 10).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn verify_restamps_clears_drift_and_reactivates() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000011", "h-new"),
        )
        .unwrap();
    }
    let ef = make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-000000000011",
        &[("aaaa0000-0000-4000-8000-000000000011", "h-old")],
    );
    mark_stale(&storage, &cogz, &ef, Some(STALE_REASON_ORPHANED));
    recompute(&storage, &declared_references(&storage, &cogz));
    assert_eq!(drift_rows(&storage, K1).len(), 1);

    let (stamped, reactivated) = verify_entity(&storage, &cogz, K1).unwrap();
    assert_eq!(stamped, 1);
    assert!(reactivated);
    assert!(drift_rows(&storage, K1).is_empty());

    let f = read_entity_file(&ef.file_path(&cogz)).unwrap();
    assert_eq!(f.status, "active");
    let v = parse_verified_frontmatter(&f.frontmatter);
    assert_eq!(
        v.get("aaaa0000-0000-4000-8000-000000000011")
            .map(String::as_str),
        Some("h-new")
    );
    assert!(f.frontmatter.get("stale_reason").is_none());

    let conn = storage.conn();
    let events = storage::events::get_recent_events(&conn, "knowledge_verified", 10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["refs_stamped"], 1);
}

#[test]
fn post_index_pass_flags_backfills_and_heals() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000aa", "h1"),
        )
        .unwrap();
        let mut dead = make_code("aaaa0000-0000-4000-8000-0000000000bb", "h2");
        dead.status = "stale".to_string();
        insert_entity(&conn, &dead).unwrap();
    }
    // References dead code → flagged stale + heal-blocked (ref still dead).
    make_knowledge(
        &storage,
        &cogz,
        K_ORPHAN,
        "aaaa0000-0000-4000-8000-0000000000bb",
        &[],
    );
    // References live code, no provenance → backfilled.
    make_knowledge(
        &storage,
        &cogz,
        K_LIVE,
        "aaaa0000-0000-4000-8000-0000000000aa",
        &[],
    );

    let stats = post_index_pass(&storage, &cogz);
    assert_eq!(stats.stale_flagged, 1);
    assert_eq!(stats.verified_backfilled, 1);
    assert_eq!(stats.healed, 0);

    let status: String = {
        let conn = storage.conn();
        conn.query_row("SELECT status FROM entities WHERE id=?1", [K_ORPHAN], |r| {
            r.get(0)
        })
        .unwrap()
    };
    assert_eq!(status, "stale");
    assert_eq!(
        drift_rows(&storage, K_ORPHAN),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000bb".into(),
            "missing".into()
        )]
    );
}

#[test]
fn declared_refs_produce_drift_without_materialized_edges() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000c1", "h-new"),
        )
        .unwrap();
    }
    // Edge FK-dropped (target didn't exist at sync time): file declares
    // the reference but no edge row exists.
    make_knowledge_no_edge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-0000000000c1",
        &[("aaaa0000-0000-4000-8000-0000000000c1", "h-old")],
    );
    // Declared target that does not exist at all → "missing" drift
    // even though an edge could never have been inserted.
    make_knowledge_no_edge(
        &storage,
        &cogz,
        K2,
        "aaaa0000-0000-4000-8000-000000000099",
        &[],
    );

    let stats = recompute(&storage, &declared_references(&storage, &cogz));
    assert_eq!(stats.entities_with_drift, 2);
    assert_eq!(
        drift_rows(&storage, K1),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000c1".into(),
            "changed".into()
        )]
    );
    assert_eq!(
        drift_rows(&storage, K2),
        vec![(
            "aaaa0000-0000-4000-8000-000000000099".into(),
            "missing".into()
        )]
    );
}

#[test]
fn post_index_pass_repairs_dropped_reference_edges() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000aa", "h1"),
        )
        .unwrap();
    }
    make_knowledge_no_edge(
        &storage,
        &cogz,
        K_LIVE,
        "aaaa0000-0000-4000-8000-0000000000aa",
        &[],
    );
    // Target that will never resolve → repair skips, drift still records.
    make_knowledge_no_edge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-000000000099",
        &[],
    );
    assert!(!edge_exists(
        &storage,
        K_LIVE,
        "aaaa0000-0000-4000-8000-0000000000aa",
        "references"
    ));

    let stats = post_index_pass(&storage, &cogz);
    assert_eq!(stats.edges_repaired, 1);
    assert!(edge_exists(
        &storage,
        K_LIVE,
        "aaaa0000-0000-4000-8000-0000000000aa",
        "references"
    ));
    assert!(!edge_exists(
        &storage,
        K1,
        "aaaa0000-0000-4000-8000-000000000099",
        "references"
    ));
    // Repair ran before backfill → K_LIVE got stamped from the declared ref.
    let ef_path = cogz.join(format!("knowledge/uncategorized/note-{K_LIVE}.md"));
    let f = read_entity_file(&ef_path).unwrap();
    let v = parse_verified_frontmatter(&f.frontmatter);
    assert_eq!(
        v.get("aaaa0000-0000-4000-8000-0000000000aa")
            .map(String::as_str),
        Some("h1")
    );
}

#[test]
fn auto_references_are_not_provenance() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000c1", "h1"),
        )
        .unwrap();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-0000000000c2", "h2"),
        )
        .unwrap();
    }
    // K1 declares c1 in frontmatter; an auto_references edge points at c2.
    make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-0000000000c1",
        &[],
    );
    make_auto_ref_edge(&storage, K1, "aaaa0000-0000-4000-8000-0000000000c2");
    // K2 has ONLY an auto_references edge — no declared refs at all.
    let title = format!("note-{K2}");
    let mut ef2 = EntityFile::new(&title, FileEntityType::Knowledge, "content");
    ef2.id = K2.to_string();
    ef2.status = "active".to_string();
    let path2 = ef2.file_path(&cogz);
    write_entity_file(&path2, &ef2).unwrap();
    {
        let conn = storage.conn();
        let mut db = Entity::new(K2, "knowledge", &title, "content");
        db.file_path = Some(
            path2
                .strip_prefix(&cogz)
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        insert_entity(&conn, &db).unwrap();
    }
    make_auto_ref_edge(&storage, K2, "aaaa0000-0000-4000-8000-0000000000c2");

    let declared = declared_references(&storage, &cogz);
    let stamped = backfill_verified_against(&storage, &cogz, &declared);
    assert_eq!(stamped, 1);
    recompute(&storage, &declared);

    // K1: only the declared c1 ref was stamped — auto c2 untouched.
    let ef_path = cogz.join(format!("knowledge/uncategorized/note-{K1}.md"));
    let v = parse_verified_frontmatter(&read_entity_file(&ef_path).unwrap().frontmatter);
    assert_eq!(
        v.get("aaaa0000-0000-4000-8000-0000000000c1")
            .map(String::as_str),
        Some("h1")
    );
    assert!(!v.contains_key("aaaa0000-0000-4000-8000-0000000000c2"));
    // K2: zero declared refs → no stamp, no drift rows.
    assert!(drift_rows(&storage, K1).is_empty());
    assert!(drift_rows(&storage, K2).is_empty());
    let v2 = parse_verified_frontmatter(&read_entity_file(&path2).unwrap().frontmatter);
    assert!(v2.is_empty());
}

#[test]
fn stale_knowledge_target_is_navigational_stale_code_is_missing() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        // Stale KNOWLEDGE target — exists with a hash, navigational.
        let mut doc = Entity::new(
            "aaaa0000-0000-4000-8000-0000000000d1",
            "knowledge",
            "old decision",
            "prior content",
        );
        doc.status = "stale".to_string();
        doc.content_hash = Some("h-stale".into());
        insert_entity(&conn, &doc).unwrap();
        // Stale CODE target — the referenced code is gone.
        let mut dead = make_code("aaaa0000-0000-4000-8000-0000000000d2", "h-dead");
        dead.status = "stale".to_string();
        insert_entity(&conn, &dead).unwrap();
    }
    // K1: lineage ref to a stale doc → `unverified` until stamped.
    make_knowledge(
        &storage,
        &cogz,
        K1,
        "aaaa0000-0000-4000-8000-0000000000d1",
        &[],
    );
    // K2: ref to an absent target → `missing` forever.
    make_knowledge_no_edge(
        &storage,
        &cogz,
        K2,
        "aaaa0000-0000-4000-8000-0000000000ee",
        &[],
    );
    // K3: ref to stale code → `missing`, and verify can't clear it.
    make_knowledge(
        &storage,
        &cogz,
        K3,
        "aaaa0000-0000-4000-8000-0000000000d2",
        &[],
    );

    let declared = declared_references(&storage, &cogz);
    recompute(&storage, &declared);
    assert_eq!(
        drift_rows(&storage, K1),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000d1".into(),
            "unverified".into()
        )]
    );
    assert_eq!(
        drift_rows(&storage, K3),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000d2".into(),
            "missing".into()
        )]
    );

    let (stamped, _) = verify_entity(&storage, &cogz, K1).unwrap();
    assert_eq!(stamped, 1, "stale doc's last-known hash is stamped");
    let (stamped2, _) = verify_entity(&storage, &cogz, K2).unwrap();
    assert_eq!(stamped2, 0);
    let (stamped3, _) = verify_entity(&storage, &cogz, K3).unwrap();
    assert_eq!(stamped3, 1, "stale code hash stamps but still drifts");

    recompute(&storage, &declared_references(&storage, &cogz));
    assert!(drift_rows(&storage, K1).is_empty());
    assert_eq!(
        drift_rows(&storage, K2),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000ee".into(),
            "missing".into()
        )]
    );
    assert_eq!(
        drift_rows(&storage, K3),
        vec![(
            "aaaa0000-0000-4000-8000-0000000000d2".into(),
            "missing".into()
        )]
    );
}

#[test]
fn verify_reactivation_ignores_navigational_stale_knowledge_refs() {
    let (_d, storage, cogz) = setup();
    let live_code = "aaaa0000-0000-4000-8000-0000000000c1";
    let stale_kn = "99999999-9999-4999-8999-999999999999";
    let stale_code = "aaaa0000-0000-4000-8000-0000000000c2";
    {
        let conn = storage.conn();
        insert_entity(&conn, &make_code(live_code, "h1")).unwrap();
        let mut sk = Entity::new(stale_kn, "knowledge", "stale doc", "body");
        sk.status = "stale".to_string();
        sk.content_hash = Some("kh".to_string());
        insert_entity(&conn, &sk).unwrap();
        let mut sc = make_code(stale_code, "h2");
        sc.status = "stale".to_string();
        insert_entity(&conn, &sc).unwrap();
    }

    // refs [live code, stale knowledge] — navigational ref must not
    // block reactivation (mirrors recompute's type-scoped semantics).
    let make_two_ref = |id: &str, second: &str| {
        let title = format!("note-{id}");
        let mut ef = EntityFile::new(&title, FileEntityType::Knowledge, "content");
        ef.id = id.to_string();
        ef.references = vec![live_code.to_string(), second.to_string()];
        let path = ef.file_path(&cogz);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_entity_file(&path, &ef).unwrap();
        let rel = path
            .strip_prefix(&cogz)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let conn = storage.conn();
        let mut db = Entity::new(id, "knowledge", &title, "content");
        db.file_path = Some(rel);
        insert_entity(&conn, &db).unwrap();
        for t in [live_code, second] {
            insert_edge(
                &conn,
                &Edge {
                    source_id: id.to_string(),
                    target_id: t.to_string(),
                    edge_type: "references".to_string(),
                    weight: 1.0,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            )
            .unwrap();
        }
        drop(conn);
        ef
    };

    let ef_a = make_two_ref(K1, stale_kn);
    mark_stale(&storage, &cogz, &ef_a, Some(STALE_REASON_ORPHANED));
    let (stamped, reactivated) = verify_entity(&storage, &cogz, K1).unwrap();
    assert!(reactivated, "stale knowledge ref is navigational, not dead");
    assert_eq!(stamped, 2, "both existing targets stamp");

    // refs [live code, stale code] — dead anchor still blocks.
    let ef_b = make_two_ref(K2, stale_code);
    mark_stale(&storage, &cogz, &ef_b, Some(STALE_REASON_ORPHANED));
    let (_, reactivated) = verify_entity(&storage, &cogz, K2).unwrap();
    assert!(!reactivated, "stale code ref is a dead anchor");
}

/// Verify stamps and stale marks rewrite a target's frontmatter — under
/// raw-byte hashing that churned `content_hash` and re-drifted every
/// referrer, so the queue could never reach quiescence on connected
/// knowledge graphs. Semantic hashing excludes bookkeeping keys.
#[test]
fn bookkeeping_rewrites_do_not_cascade_drift() {
    let (_dir, storage, cogz) = setup();
    std::fs::create_dir_all(cogz.join("knowledge/decisions")).unwrap();

    let write = |id: &str, refs: &[&str], body: &str, bookkeeping: bool| {
        let mut ef = EntityFile::new(&format!("note-{id}"), FileEntityType::Knowledge, body);
        ef.id = id.to_string();
        // Pin semantic fields — EntityFile::new stamps now() which would
        // itself drift referrers (created_at is semantic content).
        ef.created_at = "2026-01-01T00:00:00Z".to_string();
        ef.references = refs.iter().map(|r| r.to_string()).collect();
        if bookkeeping {
            ef.frontmatter.insert(
                "verified_against",
                FmValue::Array(vec!["deadbeef=00aa".to_string()]),
            );
            ef.updated_at = "2030-01-01T00:00:00Z".to_string();
        }
        let path = ef.file_path(&cogz);
        write_entity_file(&path, &ef).unwrap();
    };

    // B is the target; C references B with a stamped baseline.
    write(K1, &[], "target body", false);
    crate::files::sync_all(&storage, &cogz);
    let b_semantic: String = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT json_extract(properties,'$._semantic_hash') FROM entities WHERE id=?1",
            [K1],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(!b_semantic.is_empty(), "sync must store _semantic_hash");

    let mut ef_c = EntityFile::new("note-c", FileEntityType::Knowledge, "referrer body");
    ef_c.id = K2.to_string();
    ef_c.references = vec![K1.to_string()];
    ef_c.frontmatter.insert(
        "verified_against",
        FmValue::Array(vec![format!("{K1}={b_semantic}")]),
    );
    write_entity_file(&ef_c.file_path(&cogz), &ef_c).unwrap();
    crate::files::sync_all(&storage, &cogz);

    // Bookkeeping-only rewrite of B (what verify_entity writes): new
    // provenance stamps + timestamp bump. Raw hash changes; semantic
    // content does not.
    write(K1, &[], "target body", true);
    crate::files::sync_all(&storage, &cogz);
    let stats = post_index_pass(&storage, &cogz);
    let c_drift: i64 = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM entity_drift WHERE entity_id=?1",
            [K2],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        c_drift, 0,
        "provenance churn on B must not drift referrer C (stats: {stats:?})"
    );

    // Control: a real body edit on B still drifts C.
    write(K1, &[], "target body — materially changed", true);
    crate::files::sync_all(&storage, &cogz);
    post_index_pass(&storage, &cogz);
    let cause: String = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT cause FROM entity_drift WHERE entity_id=?1 AND code_id=?2",
            rusqlite::params![K2, K1],
            |r| r.get(0),
        )
        .unwrap_or_default()
    };
    assert_eq!(cause, "changed", "real edits must still drift referrers");
}
