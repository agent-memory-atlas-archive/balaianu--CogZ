use super::*;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::index::drift_test_support::*;
use crate::index::refstate::declared_references;
use crate::storage::crud::{Entity, insert_entity};

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

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.stamped, 1, "stale doc's last-known hash is stamped");
    let out2 = verify_entity(&storage, &cogz, K2, &VerifyOptions::default()).unwrap();
    assert_eq!(out2.stamped, 0);
    let out3 = verify_entity(&storage, &cogz, K3, &VerifyOptions::default()).unwrap();
    assert_eq!(out3.stamped, 1, "stale code hash stamps but still drifts");

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

/// File + DB row + references edge for any knowledge-side type, with
/// extra properties (volatility, category) mirrored into frontmatter
/// and DB `properties` — the same shape sync produces.

#[test]
fn record_volatility_exempts_from_drift() {
    let (_d, storage, cogz) = setup();

    // One dead (missing) reference target per entity — the strongest
    // drift cause. If an entity produces a row, drift ran on it.
    let cases: &[(&str, FileEntityType, serde_json::Value, bool)] = &[
        (
            "obs-default",
            FileEntityType::Observation,
            serde_json::json!({}),
            false,
        ),
        (
            "know-decision",
            FileEntityType::Knowledge,
            serde_json::json!({"category": "decision"}),
            false,
        ),
        (
            "know-decisions",
            FileEntityType::Knowledge,
            serde_json::json!({"category": "decisions"}),
            false,
        ),
        (
            "know-benchmark",
            FileEntityType::Knowledge,
            serde_json::json!({"category": "benchmark"}),
            false,
        ),
        (
            "know-incident",
            FileEntityType::Knowledge,
            serde_json::json!({"category": "incident"}),
            false,
        ),
        (
            "know-explicit-rec",
            FileEntityType::Knowledge,
            serde_json::json!({"volatility": "record"}),
            false,
        ),
        (
            "obs-explicit-living",
            FileEntityType::Observation,
            serde_json::json!({"volatility": "living"}),
            true,
        ),
        (
            "rule-default",
            FileEntityType::Rule,
            serde_json::json!({}),
            true,
        ),
        (
            "know-architecture",
            FileEntityType::Knowledge,
            serde_json::json!({"category": "architecture"}),
            true,
        ),
        (
            "know-no-category",
            FileEntityType::Knowledge,
            serde_json::json!({}),
            true,
        ),
    ];

    let mut declared = HashMap::new();
    for (i, (_, ftype, props, _)) in cases.iter().enumerate() {
        let id = format!("99999999-9999-4999-8999-9999999999{i:02}");
        let dead_id = format!("deaddead-dead-4ead-8ead-deaddeadde{i:02}");
        make_typed_entity(&storage, &cogz, &id, *ftype, props.clone(), &dead_id, &[]);
        declared.insert(id, vec![dead_id]);
    }

    let stats = recompute(&storage, &declared);
    assert_eq!(
        stats.drift_rows,
        cases.iter().filter(|c| c.3).count(),
        "only living entities may produce drift rows"
    );

    for (i, (name, _, _, expect_drift)) in cases.iter().enumerate() {
        let id = format!("99999999-9999-4999-8999-9999999999{i:02}");
        let rows = drift_rows(&storage, &id);
        assert_eq!(
            !rows.is_empty(),
            *expect_drift,
            "{name}: expected drift={expect_drift}, got {rows:?}"
        );
        if *expect_drift {
            assert_eq!(rows[0].1, "missing");
        }
    }
}
