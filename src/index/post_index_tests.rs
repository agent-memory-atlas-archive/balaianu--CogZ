use super::*;
use crate::files::frontmatter::FmValue;
use crate::files::{EntityFile, FileEntityType, write_entity_file};
use crate::index::drift_test_support::*;
use crate::storage;
use crate::storage::crud::{Entity, insert_entity};
use crate::storage::edges::{Edge, insert_edge};

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

/// Entity flagged `code_orphaned` through an `auto_references` edge
/// with no declared frontmatter refs — the production false-positive
/// where a junk auto-link left the doc permanently stale because heal
/// only consulted declared refs.

#[test]
fn heal_considers_auto_reference_edges() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(
            &conn,
            &make_code("aaaa0000-0000-4000-8000-000000000021", "h1"),
        )
        .unwrap();
        let mut dead = make_code("aaaa0000-0000-4000-8000-000000000022", "h2");
        dead.status = "stale".to_string();
        insert_entity(&conn, &dead).unwrap();
    }

    let auto_only = |id: &str, code_id: Option<&str>| -> EntityFile {
        let title = format!("note-{id}");
        let mut ef = EntityFile::new(&title, FileEntityType::Knowledge, "content");
        ef.id = id.to_string();
        ef.status = "active".to_string();
        let path = ef.file_path(&cogz);
        write_entity_file(&path, &ef).unwrap();
        let conn = storage.conn();
        let mut db = Entity::new(id, "knowledge", &title, "content");
        db.file_path = Some(
            path.strip_prefix(&cogz)
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        insert_entity(&conn, &db).unwrap();
        if let Some(t) = code_id {
            insert_edge(
                &conn,
                &Edge {
                    source_id: id.to_string(),
                    target_id: t.to_string(),
                    edge_type: "auto_references".to_string(),
                    weight: 1.0,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
            )
            .unwrap();
        }
        ef
    };

    // Live auto-ref → heals once the edge is considered.
    let live = auto_only(K_LIVE, Some("aaaa0000-0000-4000-8000-000000000021"));
    // Dead auto-ref → stays stale.
    let deadf = auto_only(K_DEAD, Some("aaaa0000-0000-4000-8000-000000000022"));
    // No anchors at all — the junk edge was cleaned on rebuild, so
    // nothing substantiates the orphan flag → heals.
    let bare = auto_only(K_ORPHAN, None);
    mark_stale(&storage, &cogz, &live, Some(STALE_REASON_ORPHANED));
    mark_stale(&storage, &cogz, &deadf, Some(STALE_REASON_ORPHANED));
    mark_stale(&storage, &cogz, &bare, Some(STALE_REASON_ORPHANED));

    assert_eq!(heal_stale_entities(&storage, &cogz), 2);

    let conn = storage.conn();
    for (id, expected) in [(K_LIVE, "active"), (K_DEAD, "stale"), (K_ORPHAN, "active")] {
        let status: String = conn
            .query_row("SELECT status FROM entities WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, expected, "entity {id}");
    }
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
fn stamping_skips_record_entities() {
    let (_d, storage, _cogz) = setup();
    let code_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    {
        let conn = storage.conn();
        insert_entity(&conn, &make_code(code_id, "hash1")).unwrap();
    }

    // Observation (record by default) gets no provenance stamps.
    let mut obs = EntityFile::new("saw it", FileEntityType::Observation, "content");
    obs.references = vec![code_id.to_string()];
    let n = {
        let conn = storage.conn();
        stamp_new_reference_hashes(&conn, &mut obs)
    };
    assert_eq!(n, 0);
    assert!(obs.frontmatter.get("verified_against").is_none());

    // Explicit living override on an observation restores stamping.
    obs.frontmatter
        .insert("volatility", FmValue::String("living".to_string()));
    let n = {
        let conn = storage.conn();
        stamp_new_reference_hashes(&conn, &mut obs)
    };
    assert_eq!(n, 1);
    assert!(obs.frontmatter.get("verified_against").is_some());
}

#[test]
fn heal_recovers_orphaned_record_unconditionally() {
    let (_d, storage, cogz) = setup();
    let dead_id = "deadbeef-dead-4ead-8ead-deadbeefdead";

    // A stale-flagged observation (record) — its only ref is dead.
    // Under the old regime this could be orphaned-flagged; the mark
    // was never legal, so it heals without a reference check.
    let obs = make_typed_entity(
        &storage,
        &cogz,
        "77777777-7777-4777-8777-777777777777",
        FileEntityType::Observation,
        serde_json::json!({}),
        dead_id,
        &[],
    );
    mark_stale(&storage, &cogz, &obs, Some(STALE_REASON_ORPHANED));

    // A living knowledge entity in the same state stays stale —
    // the dead anchor still substantiates the flag.
    let know = make_knowledge_no_edge(
        &storage,
        &cogz,
        "66666666-6666-4666-8666-666666666666",
        dead_id,
        &[],
    );
    mark_stale(&storage, &cogz, &know, Some(STALE_REASON_ORPHANED));

    let healed = heal_stale_entities(&storage, &cogz);
    assert_eq!(healed, 1, "only the record entity may heal");

    let conn = storage.conn();
    let obs_row = crate::storage::crud::get_entity(&conn, &obs.id).unwrap();
    assert_eq!(obs_row.status, "active");
    let know_row = crate::storage::crud::get_entity(&conn, &know.id).unwrap();
    assert_eq!(know_row.status, "stale", "living entity must not heal");

    let on_disk = read_entity_file(&obs.file_path(&cogz)).unwrap();
    assert_eq!(on_disk.status, "active");
    assert!(on_disk.frontmatter.get("stale_reason").is_none());
}
