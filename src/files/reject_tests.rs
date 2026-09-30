//! Tests for the rejection write path — the only producer of
//! `status: rejected`.

use super::*;

const OBS_1: &str = "11111111-1111-4111-8111-111111111111";
const OBS_2: &str = "22222222-2222-4222-8222-222222222222";
const OBS_3: &str = "33333333-3333-4333-8333-333333333333";
const OBS_4: &str = "44444444-4444-4444-8444-444444444444";

fn fixture() -> (tempfile::TempDir, Arc<Storage>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(cogz_dir.join("observations")).unwrap();
    let storage = Arc::new(Storage::open_memory().unwrap());
    (dir, storage, cogz_dir)
}

fn observation(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: \"Test observation\"\ntype: observation\nstatus: active\ncreated_at: 2026-09-30T00:00:00Z\nupdated_at: 2026-09-30T00:00:00Z\nreferences: []\nsource: agent\nconfidence: 0.5\n---\n\nSomething observed.\n"
    )
}

/// Write an observation file and sync it so the entity exists in the
/// DB. Returns the file's path relative to `.cogz/`.
fn add_observation(dir: &std::path::Path, storage: &Arc<Storage>, id: &str) -> PathBuf {
    let rel = PathBuf::from(format!("observations/{id}.md"));
    let abs = dir.join(".cogz").join(&rel);
    std::fs::write(&abs, observation(id)).unwrap();
    let result = sync_single_file(storage, &dir.join(".cogz"), &rel.to_string_lossy());
    assert!(result.errors.is_empty(), "fixture sync failed");
    rel
}

#[test]
fn rejects_active_entity_file_first() {
    let (dir, storage, cogz_dir) = fixture();
    let rel = add_observation(dir.path(), &storage, OBS_1);

    let outcome = reject_entity_file(
        &storage,
        &cogz_dir,
        OBS_1,
        Some("superseded by direct measurement"),
    )
    .unwrap();
    assert_eq!(outcome.id, OBS_1);
    assert_eq!(outcome.file_path, rel);

    // The canonical file carries the verdict and the reason.
    let file = read_entity_file(&cogz_dir.join(&outcome.file_path)).unwrap();
    assert_eq!(file.status, "rejected");
    assert_eq!(
        file.frontmatter
            .get("rejected_reason")
            .and_then(|v| v.as_str()),
        Some("superseded by direct measurement")
    );

    // The derived DB agrees — and the event log recorded why.
    let conn = storage.conn();
    let status: String = conn
        .query_row("SELECT status FROM entities WHERE id = ?1", [OBS_1], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "rejected");
    let events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'entity_rejected' AND entity_id = ?1",
            [OBS_1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
}

#[test]
fn rejects_stale_entity_fails_before_touching_file() {
    let (dir, storage, cogz_dir) = fixture();
    add_observation(dir.path(), &storage, OBS_2);

    // Drift the entity to stale through the lattice (stale → rejected
    // is not a legal move).
    {
        let conn = storage.conn();
        crud::update_status(&conn, OBS_2, "stale").unwrap();
    }

    let err = reject_entity_file(&storage, &cogz_dir, OBS_2, None).unwrap_err();
    assert!(matches!(
        err,
        RejectError::Storage(StorageError::IllegalTransition { .. })
    ));

    // The canonical file must be untouched — still active, still the
    // original content.
    let file = read_entity_file(&cogz_dir.join(format!("observations/{OBS_2}.md"))).unwrap();
    assert_eq!(file.status, "active");
}

#[test]
fn rejecting_twice_is_illegal() {
    let (dir, storage, cogz_dir) = fixture();
    add_observation(dir.path(), &storage, OBS_3);
    reject_entity_file(&storage, &cogz_dir, OBS_3, None).unwrap();

    let err = reject_entity_file(&storage, &cogz_dir, OBS_3, None).unwrap_err();
    assert!(matches!(
        err,
        RejectError::Storage(StorageError::IllegalTransition { .. })
    ));
}

#[test]
fn rejects_code_entity_as_not_epistemic() {
    let (dir, storage, cogz_dir) = fixture();
    add_observation(dir.path(), &storage, OBS_4);
    {
        let conn = storage.conn();
        conn.execute(
            "UPDATE entities SET type = 'function', file_path = NULL WHERE id = ?1",
            [OBS_4],
        )
        .unwrap();
    }

    let err = reject_entity_file(&storage, &cogz_dir, OBS_4, None).unwrap_err();
    assert!(matches!(err, RejectError::NotEpistemic { .. }));
}

#[test]
fn rejects_missing_entity_as_not_found() {
    let (_dir, storage, cogz_dir) = fixture();
    let err = reject_entity_file(&storage, &cogz_dir, OBS_1, None).unwrap_err();
    assert!(matches!(err, RejectError::NotFound(_)));
}
