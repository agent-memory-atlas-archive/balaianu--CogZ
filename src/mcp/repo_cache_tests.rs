//! Tests for the DB-file staleness check: a `cogz reset` unlinks the
//! database a cached RepoState writes to, and the next cache hit must
//! evict rather than keep writing to a dead inode.

use super::*;
use crate::config::Config;
use crate::embed::{ModelType, OnnxEmbeddingModel, OnnxNliModel};
use crate::storage::Storage;

fn dummy_state(
    storage: Arc<Storage>,
    cogz_dir: &std::path::Path,
    db_path: Option<PathBuf>,
) -> Arc<RepoState> {
    let config = Config::default_for("test-project");
    let models_dir = cogz_dir.join("models");
    std::fs::create_dir_all(&models_dir).unwrap();
    let dim = config.embedding.dimension;
    let ttl = config.embedding.model_idle_ttl;
    let min_mb = config.embedding.model_min_free_mb;
    make_repo_state(
        storage,
        config.clone(),
        cogz_dir.to_path_buf(),
        db_path,
        Arc::new(OnnxEmbeddingModel::with_resource_config(
            ModelType::Knowledge,
            &models_dir,
            dim,
            &config.embedding.knowledge_model,
            ttl,
            min_mb,
        )),
        Arc::new(OnnxEmbeddingModel::with_resource_config(
            ModelType::Code,
            &models_dir,
            dim,
            &config.embedding.code_model,
            ttl,
            min_mb,
        )),
        Arc::new(OnnxNliModel::with_resource_config(
            &models_dir,
            &config.embedding.nli_model,
            ttl,
            min_mb,
        )),
    )
}

/// A repo root with a real on-disk database and a cached state that
/// was opened against it.
fn file_backed_fixture() -> (tempfile::TempDir, RepoCache, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    let cogz_dir = repo.join(".cogz");
    std::fs::create_dir_all(&cogz_dir).unwrap();
    let db_path = cogz_dir.join("cogz.db");
    let storage = Arc::new(Storage::open(&db_path, 768).unwrap());
    let state = dummy_state(storage, &cogz_dir, Some(db_path.clone()));
    let cache = RepoCache::new();
    cache.insert_ready(repo.clone(), state);
    (dir, cache, repo, db_path)
}

#[test]
fn cache_hit_when_db_unchanged() {
    let (_dir, cache, repo, _db) = file_backed_fixture();
    assert!(matches!(cache.get(&repo), Ok(Some(_))));
}

/// A cached state whose `db_identity` is minted from a real file on
/// disk while the storage itself is in-memory. Holding a live SQLite
/// handle on `db_path` would make delete/replace impossible on
/// platforms without unlink-while-open (Windows) — the identity
/// comparison under test doesn't depend on the handle being live.
fn identity_fixture() -> (tempfile::TempDir, RepoCache, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    let cogz_dir = repo.join(".cogz");
    std::fs::create_dir_all(&cogz_dir).unwrap();
    let db_path = cogz_dir.join("cogz.db");
    std::fs::write(&db_path, b"placeholder db bytes").unwrap();
    let storage = Arc::new(Storage::open_memory().unwrap());
    let state = dummy_state(storage, &cogz_dir, Some(db_path.clone()));
    let cache = RepoCache::new();
    cache.insert_ready(repo.clone(), state);
    (dir, cache, repo, db_path)
}

#[test]
fn cache_evicts_when_db_deleted() {
    let (_dir, cache, repo, db_path) = identity_fixture();
    std::fs::remove_file(&db_path).unwrap();
    assert!(cache.get(&repo).is_err());
    // `get` signals staleness; the caller (resolve_repo) removes the
    // entry. After removal the cache reports a plain miss.
    cache.remove(&repo);
    assert!(matches!(cache.get(&repo), Ok(None)));
}

#[test]
fn cache_evicts_when_db_replaced() {
    let (_dir, cache, repo, db_path) = identity_fixture();
    // The reset+reindex sequence: unlink, then a new file at the same
    // path. Even if the filesystem recycles the inode/file index, the
    // birth time differs.
    std::fs::remove_file(&db_path).unwrap();
    std::fs::write(&db_path, b"recreated db bytes").unwrap();
    assert!(cache.get(&repo).is_err());
}

#[test]
fn memory_backed_state_is_never_stale() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    let cogz_dir = repo.join(".cogz");
    std::fs::create_dir_all(&cogz_dir).unwrap();
    let storage = Arc::new(Storage::open_memory().unwrap());
    let state = dummy_state(storage, &cogz_dir, None);
    let cache = RepoCache::new();
    cache.insert_ready(repo.clone(), state);
    assert!(matches!(cache.get(&repo), Ok(Some(_))));
}
