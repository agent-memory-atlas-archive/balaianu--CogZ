//! Lifecycle event handlers for file_save and session_end.
//!
//! `handle_file_save` handles two paths:
//! - Source files (not under `.cogz/`): single-file code reindex +
//!   stale-knowledge flagging.
//! - `.cogz/` entity files: incremental file sync + embedding, so
//!   human-edited knowledge/rules/observations reach the DB without
//!   a manual `cogz reindex`.
//!
//! `handle_session_end` runs consolidation (promotion + merge).

use std::path::Path;
use std::sync::Arc;

use crate::config::Config;
use crate::embed::{EmbeddingCache, EmbeddingModel, NliModel, OnnxEmbeddingModel};
use crate::files::embed_sync::{embed_entities, store_embeddings};
use crate::files::sync_single_file;
use crate::hooks::lifecycle::{ConsolidationSummary, ReindexSummary};
use crate::storage::Storage;
use crate::storage::crud::get_entities_batch;

/// Handle a file_save event. The path determines which pipeline runs:
///
/// - **Source file** (not under `.cogz/`): single-file code reindex
///   via `reindex_single_file`, then `flag_stale_knowledge` for any
///   changed or deleted code entities.
///
/// - **`.cogz/` entity file**: incremental file sync via
///   `sync_incremental` (file → DB), then embed synced entities using
///   the knowledge model. This keeps the DB in sync when a human edits
///   knowledge/rules/observations directly, without requiring a manual
///   `cogz reindex`.
pub fn handle_file_save(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &Path,
    query_model: &OnnxEmbeddingModel,
    file_path: Option<&str>,
) -> ReindexSummary {
    let path = match file_path {
        Some(p) => p,
        None => {
            return ReindexSummary {
                reindexed: false,
                synced: false,
                created: 0,
                updated: 0,
                marked_stale: 0,
                stale_knowledge_flagged: 0,
                embedded: 0,
            };
        }
    };

    // Normalize separators before the .cogz/ dispatch check — Windows
    // harnesses report `C:\repo\.cogz\...` paths, which would otherwise
    // fall into the source-file branch and skip the entity sync.
    let normalized = crate::index::normalize_path(path);
    if normalized.starts_with(".cogz/")
        || normalized.contains("/.cogz/")
        || normalized.starts_with("./.cogz/")
    {
        return handle_cogz_file_save(storage, cogz_dir, query_model, &normalized);
    }

    handle_source_file_save(storage, config, cogz_dir, path)
}

/// Sync a `.cogz/` entity file to the DB and embed it. Syncs only
/// the saved file (not the entire `.cogz/` directory), then embeds
/// the entity using the knowledge model. Follows the lock discipline:
/// fetch under lock, embed without lock, store under lock.
fn handle_cogz_file_save(
    storage: &Arc<Storage>,
    cogz_dir: &Path,
    query_model: &OnnxEmbeddingModel,
    file_path: &str,
) -> ReindexSummary {
    tracing::debug!("file_save hook: syncing {file_path}");

    let result = sync_single_file(storage, cogz_dir, file_path);

    if !result.errors.is_empty() {
        tracing::warn!(
            "file_save sync: {} error(s), first: {}",
            result.errors.len(),
            result.errors[0].error
        );
    }

    // Embed synced entities. .cogz/ files are always knowledge-type
    // entities (observations, rules, knowledge), so the knowledge
    // model (query_model) is the correct one.
    let embedded = if result.synced_entity_ids.is_empty() || !query_model.is_available() {
        0
    } else {
        embed_synced_entities(storage, query_model, &result.synced_entity_ids)
    };

    // Maintenance on canonical edits too — a human fixing references
    // heals orphaned knowledge; new references get provenance stamps.
    // Debounced: the sweep is a global scan and save events can
    // arrive in bursts; everything it does converges idempotently,
    // so skipping within the window only delays, never loses work.
    let post = post_index_debounced(storage, cogz_dir);

    ReindexSummary {
        reindexed: false,
        synced: true,
        created: result.created,
        updated: result.updated,
        marked_stale: result.marked_stale,
        stale_knowledge_flagged: post.stale_flagged,
        embedded,
    }
}

/// Embed entities that were synced. Fetches entity data under the DB
/// lock, drops the lock for ONNX inference, then re-acquires to store
/// vectors. Returns the count of entities successfully embedded.
fn embed_synced_entities(
    storage: &Arc<Storage>,
    query_model: &OnnxEmbeddingModel,
    entity_ids: &[String],
) -> usize {
    let cache = EmbeddingCache::new();

    // Phase 1: fetch entity data under the lock, then drop it.
    let entities: Vec<_> = {
        let conn = storage.conn();
        match get_entities_batch(&conn, entity_ids) {
            Ok(entities) => entities,
            Err(e) => {
                tracing::warn!("failed to fetch entities for embedding: {e}");
                return 0;
            }
        }
    };

    if entities.is_empty() {
        return 0;
    }

    // Phase 2: embed without holding the DB lock.
    let embeddings = embed_entities(query_model, &cache, &entities);

    if embeddings.is_empty() {
        return 0;
    }

    // Phase 3: store vectors under the lock.
    let mut conn = storage.conn();
    store_embeddings(&mut conn, &embeddings)
}

/// Runs `post_index_pass` at most once per `POST_INDEX_DEBOUNCE_SECS`
/// on the `.cogz/` save path. The sweep reads every knowledge file
/// and scans the whole entity graph — per save that cost dwarfs the
/// sync it follows. Returns zeroed stats when skipped; the next pass
/// (here or from `cogz index`/`reindex`) does the full work.
fn post_index_debounced(storage: &Storage, cogz_dir: &Path) -> crate::index::drift::PostIndexStats {
    const POST_INDEX_DEBOUNCE_SECS: i64 = 60;
    let fresh = {
        let conn = storage.conn();
        crate::storage::get_meta(&conn, "last_post_index_pass")
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .is_some_and(|last| {
                chrono::Utc::now().signed_duration_since(last).num_seconds()
                    < POST_INDEX_DEBOUNCE_SECS
            })
    };
    if fresh {
        crate::index::drift::PostIndexStats::default()
    } else {
        crate::index::drift::post_index_pass(storage, cogz_dir)
    }
}

/// Reindex a source file after it is saved. Calls
/// `reindex_single_file` then `flag_stale_knowledge` for any changed
/// or deleted code entities.
fn handle_source_file_save(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &Path,
    path: &str,
) -> ReindexSummary {
    let repo_root = cogz_dir.parent().unwrap_or_else(|| Path::new("."));

    tracing::debug!("file_save hook: triggering reindex for {}", path);

    let result =
        crate::index::reindex_single_file(storage, repo_root, path, config.index.max_file_bytes);

    let post = crate::index::drift::post_index_pass(storage, cogz_dir);
    let stale_flagged = post.stale_flagged;

    ReindexSummary {
        reindexed: true,
        synced: false,
        created: result.created,
        updated: result.updated,
        marked_stale: result.marked_stale,
        stale_knowledge_flagged: stale_flagged,
        embedded: 0,
    }
}

/// Handle a session_end event: run consolidation (promotion + merge)
/// for real. The configured thresholds are the safety mechanism — if
/// they're met, the system should act. This aligns with the first
/// principle that consolidation is continuous, not batch. Rules
/// created by promotion are git-tracked and reviewable; merged
/// entities are superseded (not deleted) and remain in the graph.
pub fn handle_session_end(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &Path,
    nli_model: Option<&dyn NliModel>,
) -> ConsolidationSummary {
    let promoted = match crate::consolidate::promote::run_promotion(
        storage,
        cogz_dir,
        &config.consolidation,
        false,
    ) {
        Ok(p) => p.len(),
        Err(e) => {
            tracing::warn!("session_end consolidation (promote) failed: {}", e);
            0
        }
    };

    let merged = match crate::consolidate::merge::run_merge(
        storage,
        cogz_dir,
        &config.consolidation,
        nli_model,
        false,
    ) {
        Ok(m) => m.len(),
        Err(e) => {
            tracing::warn!("session_end consolidation (merge) failed: {}", e);
            0
        }
    };

    ConsolidationSummary {
        promotions: promoted,
        merges: merged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::drift_test_support::*;
    use crate::storage::crud::{Entity, get_entity, insert_entity};

    #[test]
    fn post_index_debounced_skips_within_window() {
        let (_d, storage, cogz) = setup();
        // A stale code entity plus knowledge referencing it gives a
        // real sweep observable work: flag_stale_knowledge marks the
        // knowledge stale.
        let insert_dead = |conn: &rusqlite::Connection, id: &str| {
            let mut dead = Entity::new(id, "function", "func", "fn func() {}");
            dead.status = "stale".to_string();
            dead.content_hash = Some("h".to_string());
            insert_entity(conn, &dead).unwrap();
        };
        {
            let conn = storage.conn();
            insert_dead(&conn, "aaaa0000-0000-4000-8000-000000000099");
        }
        make_knowledge(
            &storage,
            &cogz,
            K1,
            "aaaa0000-0000-4000-8000-000000000099",
            &[],
        );

        let stats = post_index_debounced(&storage, &cogz);
        assert_eq!(stats.stale_flagged, 1, "first call must run the sweep");

        // A second flagged pair is observable only if the sweep runs.
        {
            let conn = storage.conn();
            insert_dead(&conn, "aaaa0000-0000-4000-8000-000000000098");
        }
        make_knowledge(
            &storage,
            &cogz,
            K2,
            "aaaa0000-0000-4000-8000-000000000098",
            &[],
        );

        let stats = post_index_debounced(&storage, &cogz);
        assert_eq!(
            stats.stale_flagged, 0,
            "within the debounce window the sweep must be skipped"
        );
        {
            let conn = storage.conn();
            let e = get_entity(&conn, K2).unwrap();
            assert_eq!(e.status, "active", "skipped sweep must not flag");
        }

        // Backdate the stamp beyond the window — the sweep runs again.
        {
            let conn = storage.conn();
            let old = (chrono::Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
            crate::storage::set_meta(&conn, "last_post_index_pass", &old).unwrap();
        }
        let stats = post_index_debounced(&storage, &cogz);
        assert_eq!(stats.stale_flagged, 1, "stale stamp must let it run");
    }
}
