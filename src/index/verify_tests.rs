use super::{VerifyOptions, VerifySelection, batch_verify, verify_entity};
use crate::files::{FileEntityType, write_entity_file};
use crate::index::drift_test_support::*;
use crate::storage::crud::insert_entity;

/// Stale entity with live refs: stamps every ref, clears the stale
/// mark and `stale_reason`, reactivates.
#[test]
fn stale_verify_stamps_and_reactivates() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn check() { validate(x); }")).unwrap();
    }
    let ef = entity(
        &storage,
        &cogz,
        K1,
        "check validates x",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    mark_stale(&storage, &cogz, &ef, Some("code_changed"));

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.stamped, 1);
    assert_eq!(out.resolved, 1);
    assert!(out.dead_refs.is_empty());
    assert!(out.reactivated);

    let f = read_back(&cogz, &ef);
    assert_eq!(f.status, "active");
    assert!(f.frontmatter.get("stale_reason").is_none());
    assert!(f.frontmatter.get("verified_against").is_some());
}

/// Verify clears the entity's drift rows — the queue is drained by
/// attestation, not by status.
#[test]
fn verify_clears_drift_rows() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    entity(
        &storage,
        &cogz,
        K1,
        "content",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    {
        let conn = storage.conn();
        conn.execute(
            "INSERT INTO entity_drift (entity_id, code_id, verified_hash, current_hash, cause) \
             VALUES (?1, ?2, 'old', 'new', 'changed')",
            rusqlite::params![K1, CODE_A],
        )
        .unwrap();
    }

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.stamped, 1);
    let conn = storage.conn();
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_drift WHERE entity_id = ?1",
            [K1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 0);
}

/// One live ref + one absent ref: the dead anchor is reported and
/// blocks reactivation — absent means no edge (FK would fail); the
/// frontmatter declaration is what verify reads.
#[test]
fn dead_refs_report_and_block_reactivation() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    let ef = entity(
        &storage,
        &cogz,
        K1,
        "content",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    let mut ef2 = ef.clone();
    ef2.references = vec![CODE_A.to_string(), CODE_B.to_string()];
    write_entity_file(&ef2.file_path(&cogz), &ef2).unwrap();
    mark_stale(&storage, &cogz, &ef2, Some("code_orphaned"));

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.resolved, 1);
    assert_eq!(out.dead_refs, vec![CODE_B.to_string()]);
    assert!(!out.reactivated);
    let f = read_back(&cogz, &ef2);
    assert_eq!(f.status, "stale");
}

/// `allow_dead_refs` is the explicit-authority path over the dead
/// anchor — the ref is still reported, not hidden.
#[test]
fn allow_dead_refs_reactivates_with_partial_anchors() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    let ef = entity(
        &storage,
        &cogz,
        K1,
        "content",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    let mut ef2 = ef.clone();
    ef2.references = vec![CODE_A.to_string(), CODE_B.to_string()];
    write_entity_file(&ef2.file_path(&cogz), &ef2).unwrap();
    mark_stale(&storage, &cogz, &ef2, Some("code_orphaned"));

    let out = verify_entity(
        &storage,
        &cogz,
        K1,
        &VerifyOptions {
            allow_dead_refs: true,
        },
    )
    .unwrap();
    assert!(out.reactivated, "caller asserted over the dead anchor");
    assert_eq!(out.dead_refs, vec![CODE_B.to_string()]);
}

/// Records reactivate unconditionally on verify and track no
/// provenance — `verified_against` is stripped, never stamped.
#[test]
fn stale_record_reactivates_unconditionally() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    let ef = entity(
        &storage,
        &cogz,
        K1,
        "on 2026-10-10 the gate was red",
        &[CODE_A],
        FileEntityType::Observation,
    );
    mark_stale(&storage, &cogz, &ef, Some("manual"));

    let out = verify_entity(&storage, &cogz, K1, &VerifyOptions::default()).unwrap();
    assert_eq!(out.stamped, 0);
    assert!(out.reactivated);
    let f = read_back(&cogz, &ef);
    assert_eq!(f.status, "active");
    assert!(f.frontmatter.get("verified_against").is_none());
}

/// Batch `drifted` selects only entities with drift rows.
#[test]
fn batch_drifted_selects_drift_rows() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    entity(
        &storage,
        &cogz,
        K1,
        "one",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    entity(
        &storage,
        &cogz,
        K2,
        "two",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    {
        let conn = storage.conn();
        conn.execute(
            "INSERT INTO entity_drift (entity_id, code_id, cause) VALUES (?1, ?2, 'changed')",
            rusqlite::params![K1, CODE_A],
        )
        .unwrap();
    }

    let rows = batch_verify(
        &storage,
        &cogz,
        VerifySelection::Drifted,
        &VerifyOptions::default(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entity_id, K1);
    assert!(rows[0].outcome.as_ref().unwrap().stamped == 1);
}

/// Batch `all` selects every living file-backed entity — unflagged
/// active entities get stamped too. Records are excluded.
#[test]
fn batch_all_selects_living_excludes_records() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    entity(
        &storage,
        &cogz,
        K1,
        "living claim",
        &[CODE_A],
        FileEntityType::Knowledge,
    );
    entity(
        &storage,
        &cogz,
        K2,
        "a dated fact",
        &[CODE_A],
        FileEntityType::Observation,
    );

    let rows = batch_verify(
        &storage,
        &cogz,
        VerifySelection::All,
        &VerifyOptions::default(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entity_id, K1);
}

/// Batch `unverified` selects living entities with no provenance
/// baseline — bootstrap coverage the code-change triggers can't reach.
#[test]
fn batch_unverified_selects_missing_baseline() {
    let (_d, storage, cogz) = setup();
    {
        let conn = storage.conn();
        insert_entity(&conn, &code_row(CODE_A, "fn f() {}")).unwrap();
    }
    entity(
        &storage,
        &cogz,
        K1,
        "never verified",
        &[CODE_A],
        FileEntityType::Knowledge,
    );

    let rows = batch_verify(
        &storage,
        &cogz,
        VerifySelection::Unverified,
        &VerifyOptions::default(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entity_id, K1);
    assert_eq!(rows[0].outcome.as_ref().unwrap().stamped, 1);
}
