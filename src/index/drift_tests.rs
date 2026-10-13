use super::*;
use crate::files::read_entity_file;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::index::drift_test_support::*;
use crate::index::post_index::backfill_verified_against;
use crate::index::refstate::declared_references;
use crate::storage::crud::{Entity, insert_entity};

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
fn legacy_baseline_compares_via_v1_of_v2_column() {
    let (_d, storage, cogz) = setup();
    // Code entity whose column hash has migrated to the structural
    // scheme, with content and language stored for the legacy compare.
    let mut code = make_code("aaaa0000-0000-4000-8000-0000000000d1", "v2:structural-hash");
    code.content = "fn func() { let x = 1; }".to_string();
    code.properties = serde_json::json!({"language": "rust"});
    {
        let conn = storage.conn();
        insert_entity(&conn, &code).unwrap();
    }

    // Baseline stamped under the old normalized-content scheme.
    let legacy = crate::index::sync::legacy_content_hash(&code.content, "rust");
    make_knowledge(&storage, &cogz, K1, &code.id, &[(&code.id, &legacy)]);
    // A wrong baseline must still produce `changed`.
    let other = crate::index::sync::legacy_content_hash("fn other() {}", "rust");
    make_knowledge(&storage, &cogz, K2, &code.id, &[(&code.id, &other)]);
    // A v2 baseline compares against the column directly.
    make_knowledge(
        &storage,
        &cogz,
        K3,
        &code.id,
        &[(&code.id, "v2:structural-hash")],
    );

    let stats = recompute(&storage, &declared_references(&storage, &cogz));
    assert_eq!(stats.entities_with_drift, 1);
    assert_eq!(
        drift_rows(&storage, K2),
        vec![(code.id.clone(), "changed".into())]
    );
    assert!(drift_rows(&storage, K1).is_empty());
    assert!(drift_rows(&storage, K3).is_empty());
}
