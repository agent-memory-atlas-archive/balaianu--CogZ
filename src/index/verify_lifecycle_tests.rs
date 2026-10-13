use super::{VerifyOptions, verify_entity};
use crate::files::read_entity_file;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::index::drift::{STALE_REASON_ORPHANED, parse_verified_frontmatter, recompute};
use crate::index::drift_test_support::*;
use crate::index::refstate::declared_references;
use crate::storage;
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::edges::{Edge, insert_edge};

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

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.stamped, 1);
    assert!(out.reactivated);
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
    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert!(
        out.reactivated,
        "stale knowledge ref is navigational, not dead"
    );
    assert_eq!(out.stamped, 2, "both existing targets stamp");

    // refs [live code, stale code] — dead anchor still blocks.
    let ef_b = make_two_ref(K2, stale_code);
    mark_stale(&storage, &cogz, &ef_b, Some(STALE_REASON_ORPHANED));
    let out = verify_entity(&storage, &cogz, K2, &VerifyOptions::default()).unwrap();
    assert!(!out.reactivated, "stale code ref is a dead anchor");
}

/// Verify stamps and stale marks rewrite a target's frontmatter — under
/// raw-byte hashing that churned `content_hash` and re-drifted every
/// referrer, so the queue could never reach quiescence on connected
/// knowledge graphs. Semantic hashing excludes bookkeeping keys.
#[test]

fn verify_record_strips_provenance_and_reactivates() {
    let (_d, storage, cogz) = setup();
    let dead_id = "deadbeef-dead-4ead-8ead-deadbeefdead";

    // Stale record entity with a dead ref AND a legacy provenance
    // stamp — verify must strip the stamp and reactivate even though
    // the anchor is dead.
    let obs = make_typed_entity(
        &storage,
        &cogz,
        "88888888-8888-4888-8888-888888888888",
        FileEntityType::Observation,
        serde_json::json!({}),
        dead_id,
        &[(dead_id, "oldhash")],
    );
    mark_stale(&storage, &cogz, &obs, Some(STALE_REASON_ORPHANED));

    let out = verify_entity(&storage, &cogz, &obs.id, &VerifyOptions::default())
        .expect("verify must succeed");
    assert_eq!(out.stamped, 0, "records carry no provenance");
    assert!(out.reactivated, "dead anchors don't block record verify");

    let on_disk = read_entity_file(&obs.file_path(&cogz)).unwrap();
    assert_eq!(on_disk.status, "active");
    assert!(on_disk.frontmatter.get("verified_against").is_none());
    assert!(on_disk.frontmatter.get("stale_reason").is_none());
}
