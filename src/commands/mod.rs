//! CLI command handlers — status, index, reindex, reset, consolidate.
//!
//! These functions are the dispatch targets for the CLI subcommands
//! defined in `main.rs`. They handle config loading, storage setup,
//! and user-facing output via `println!`.

pub mod doctor;
pub mod embed_bg;
pub mod models;
pub mod reindex_bg;

use std::path::Path;

use anyhow::Context;
use cogz::config::Config;

use crate::cli;

pub use doctor::run_doctor;
pub use models::{run_models_clean, run_models_download, run_models_list};

/// Mine git history for the co-change channel — derived state,
/// rebuilt wholesale each index. Silent zero on non-git repos.
fn sync_cochange_report(storage: &cogz::storage::Storage, repo: &Path) -> anyhow::Result<()> {
    let rows = cogz::index::cochange::sync_cochange(storage, repo)?;
    if rows > 0 {
        println!("  Co-change index: {rows} rows");
    }
    Ok(())
}

pub(crate) fn load_config(repo: &Path) -> anyhow::Result<(Config, std::path::PathBuf)> {
    let cogz_dir = repo.join(".cogz");
    let config_path = cogz_dir.join("config.toml");

    if !config_path.exists() {
        anyhow::bail!(
            "No .cogz/ directory found in {}. Run `cogz init` first.",
            repo.display()
        );
    }

    let config = cogz::config::load(&config_path)?;
    // The config also governs the ORT binary download inside model
    // loading — `auto_download` must not leak past it.
    cogz::embed::set_ort_download_allowed(config.embedding.auto_download);
    let db_path = cogz::config::resolve_db_path(repo, &config.storage.db_path)?;
    Ok((config, db_path))
}

pub fn run_status(repo: &Path) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;

    if !db_path.exists() {
        println!("CogZ status for project: {}\n", config.project.name);
        println!(
            "  DB not found at {}. Run `cogz index` to build it.",
            db_path.display()
        );
        return Ok(());
    }

    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;

    println!("CogZ status for project: {}\n", config.project.name);
    println!("  DB: {}", db_path.display());
    println!("  DB size: {} bytes", storage.db_size_bytes());

    let conn = storage.conn();
    let schema_version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    println!("  Schema version: {}", schema_version);

    let counts = cogz::storage::query::entity_counts_by_type(&conn)?;
    println!("\n  Entities by type:");
    if counts.is_empty() {
        println!("    (none)");
    } else {
        for (etype, count) in &counts {
            println!("    {:12} {}", etype, count);
        }
    }

    let total = cogz::storage::crud::count_all(&conn)?;
    println!("\n  Total entities: {}", total);

    let stale = cogz::storage::query::count_stale(&conn)?;
    println!("  Stale entities: {}", stale);

    let drifted: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT entity_id) FROM entity_drift",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if drifted > 0 {
        println!("  Drifted entities: {} (verify or update)", drifted);
    }

    let edges = cogz::storage::edges::count_edges(&conn)?;
    println!("  Edges: {}", edges);

    let events = cogz::storage::events::count_events(&conn)?;
    println!("  Events: {}", events);

    let models_dir = cli::models_dir();
    let code_model = cogz::embed::OnnxEmbeddingModel::with_model_id(
        cogz::embed::ModelType::Code,
        &models_dir,
        config.embedding.dimension,
        &config.embedding.code_model,
    );
    let knowledge_model = cogz::embed::OnnxEmbeddingModel::with_model_id(
        cogz::embed::ModelType::Knowledge,
        &models_dir,
        config.embedding.dimension,
        &config.embedding.knowledge_model,
    );
    println!("\n  Models:");
    println!(
        "    Code ({}): {}",
        config.embedding.code_model,
        if code_model.model_files_exist() {
            "available"
        } else {
            "not found"
        }
    );
    println!(
        "    Knowledge ({}): {}",
        config.embedding.knowledge_model,
        if knowledge_model.model_files_exist() {
            "available"
        } else {
            "not found"
        }
    );
    let nli_model = cogz::embed::OnnxNliModel::new(&models_dir, &config.embedding.nli_model);
    println!(
        "    NLI ({}): {}",
        config.embedding.nli_model,
        if nli_model.model_files_exist() {
            "available"
        } else {
            "not found"
        }
    );
    let embedding_count = cogz::storage::embeddings::count_embeddings(&conn)?;
    println!("  Embeddings stored: {}", embedding_count);

    Ok(())
}

/// Union freshly-synced entity ids with entities that have no
/// embedding row — the embed pass heals coverage gaps, not just
/// freshly-changed entities (model down at index time, dropped
/// migration rows, interrupted background embed).
fn embed_targets(
    storage: &cogz::storage::Storage,
    synced: &[String],
    space: cogz::storage::embeddings::EmbeddingSpace,
) -> Vec<String> {
    let mut ids: std::collections::BTreeSet<String> = synced.iter().cloned().collect();
    {
        let conn = storage.conn();
        match cogz::storage::embeddings::ids_missing_embeddings(&conn, space) {
            Ok(missing) => ids.extend(missing),
            Err(e) => tracing::warn!("missing-embedding scan failed: {e}"),
        }
    }
    ids.into_iter().collect()
}

pub fn run_index(repo: &Path, no_download: bool) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    // --no-download must also cover the ORT binary fetch inside model
    // loading — not just the HuggingFace model downloads below.
    cogz::embed::set_ort_download_allowed(config.embedding.auto_download && !no_download);
    let cogz_dir = repo.join(".cogz");
    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;

    if config.embedding.auto_download && !no_download {
        models::auto_download_models(&config);
    }

    println!("Indexing {}...", cogz_dir.display());
    let result = cogz::files::sync_all(&storage, &cogz_dir);

    println!(
        "  Created: {}\n  Updated: {}\n  Stale: {}\n  Skipped: {}",
        result.created, result.updated, result.marked_stale, result.skipped
    );

    if !result.errors.is_empty() {
        println!("\n  Errors ({}):", result.errors.len());
        for err in &result.errors {
            println!("    {}: {}", err.file_path.display(), err.error);
        }
    }

    let file_targets = embed_targets(
        &storage,
        &result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Knowledge,
    );
    if !file_targets.is_empty() {
        let embedded = cli::embed_synced(&storage, &config, &file_targets, &cogz_dir, false);
        if embedded > 0 {
            println!("  Embedded: {}", embedded);
        }
    }

    println!("\nIndexing source code...");
    let code_result = cogz::index::index_code(&storage, repo, &config);
    println!(
        "  Code entities: {} created, {} updated, {} stale, {} skipped, {} hash-migrated",
        code_result.created,
        code_result.updated,
        code_result.marked_stale,
        code_result.skipped,
        code_result.hash_migrated
    );

    let code_targets = embed_targets(
        &storage,
        &code_result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Code,
    );
    if !code_targets.is_empty() {
        let embedded = cli::embed_synced(&storage, &config, &code_targets, &cogz_dir, false);
        if embedded > 0 {
            println!("  Code embedded: {}", embedded);
        }
    }

    sync_cochange_report(&storage, repo)?;

    let post = cogz::index::drift::post_index_pass(&storage, &cogz_dir);
    if post.stale_flagged > 0
        || post.healed > 0
        || post.drifted_entities > 0
        || post.verified_backfilled > 0
        || post.reanchored > 0
    {
        println!(
            "  Stale knowledge flagged: {} | drifted: {} | healed: {} | provenance stamped: {} | re-anchored: {}",
            post.stale_flagged,
            post.drifted_entities,
            post.healed,
            post.verified_backfilled,
            post.reanchored
        );
    }

    if result.created + result.updated + code_result.created + code_result.updated == 0 {
        println!("  Already up to date — no file or source changes since last index.");
    }

    // Scope the guard: update_baseline re-acquires the connection.
    let total = {
        let conn = storage.conn();
        cogz::storage::crud::count_all(&conn)?
    };
    println!("\n  Total entities: {}", total);

    // Only advance the baseline if all source files were successfully
    // read and parsed. On partial failure, preserve the previous
    // baseline so the next reindex retries the failed files.
    if cogz::index::baseline::should_update_baseline(code_result.failed_files != 0) {
        cogz::index::baseline::update_baseline(&storage, repo);
    }

    Ok(())
}

pub fn run_reindex(repo: &Path) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;

    println!("Reindexing (incremental) {}...", cogz_dir.display());
    let result = cogz::files::sync_incremental(&storage, &cogz_dir);

    println!(
        "  Created: {}\n  Updated: {}\n  Stale: {}\n  Skipped: {}",
        result.created, result.updated, result.marked_stale, result.skipped
    );

    if !result.errors.is_empty() {
        println!("\n  Errors ({}):", result.errors.len());
        for err in &result.errors {
            println!("    {}: {}", err.file_path.display(), err.error);
        }
    }

    let file_targets = embed_targets(
        &storage,
        &result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Knowledge,
    );
    if !file_targets.is_empty() {
        let embedded = cli::embed_synced(&storage, &config, &file_targets, &cogz_dir, false);
        if embedded > 0 {
            println!("  Embedded: {}", embedded);
        }
    }

    println!("\nReindexing source code...");
    let code_result = cogz::index::reindex_code(&storage, repo, &config);

    if code_result.incremental {
        println!("  (incremental — git diff)");
    } else {
        println!("  (full scan — no git baseline)");
    }
    println!(
        "  Code entities: {} created, {} updated, {} stale, {} skipped, {} hash-migrated",
        code_result.created,
        code_result.updated,
        code_result.marked_stale,
        code_result.skipped,
        code_result.hash_migrated
    );

    let code_targets = embed_targets(
        &storage,
        &code_result.synced_entity_ids,
        cogz::storage::embeddings::EmbeddingSpace::Code,
    );
    if !code_targets.is_empty() {
        let embedded = cli::embed_synced(&storage, &config, &code_targets, &cogz_dir, false);
        if embedded > 0 {
            println!("  Code embedded: {}", embedded);
        }
    }

    sync_cochange_report(&storage, repo)?;

    let post = cogz::index::drift::post_index_pass(&storage, &cogz_dir);
    if post.stale_flagged > 0 || post.healed > 0 || post.drifted_entities > 0 || post.reanchored > 0
    {
        println!(
            "  Stale knowledge flagged: {} | drifted: {} | healed: {} | provenance stamped: {} | re-anchored: {}",
            post.stale_flagged,
            post.drifted_entities,
            post.healed,
            post.verified_backfilled,
            post.reanchored
        );
    }

    Ok(())
}

pub fn run_reset(repo: &Path, purge: bool, confirm: bool) -> anyhow::Result<()> {
    let (_config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");
    let obs_dir = cogz_dir.join("observations");

    if purge && !confirm {
        let obs_count = walkdir::WalkDir::new(&obs_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count();
        println!("Dry run — pass --confirm to apply.\n");
        println!("  would drop database: {}", db_path.display());
        println!(
            "  would purge {obs_count} observation files in {}",
            obs_dir.display()
        );
        println!("  knowledge, rules, config, and .gitignore are preserved.");
        return Ok(());
    }

    // Refuse to run under an active background worker, then take the
    // canonical write lock — deleting the DB under either is the
    // corruption window this guards.
    let _bg_guard = cogz::bg_lock::try_acquire(&cogz_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "a background worker is active on this repo — wait for it to finish or check {}",
            cogz_dir.join(".bg.lock").display()
        )
    })?;
    let _file_guard = cogz::storage::lock_repo_file(&db_path)
        .context("cannot acquire .cogz/.lock — another cogz process holds it")?;

    // Flush the WAL into the main file before unlinking so a concurrent
    // reader's committed rows aren't stranded in a deleted -wal.
    if db_path.exists() {
        let conn = rusqlite::Connection::open(&db_path)
            .with_context(|| format!("cannot open {}", db_path.display()))?;
        conn.pragma_update(None, "busy_timeout", "5000")?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(conn);
    }

    for suffix in &["", "-wal", "-shm"] {
        let path = format!("{}{}", db_path.display(), suffix);
        let path = std::path::Path::new(&path);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    println!("Dropped database: {}", db_path.display());

    if purge {
        if obs_dir.exists() {
            std::fs::remove_dir_all(&obs_dir)?;
            std::fs::create_dir_all(&obs_dir)?;
            println!("Purged observations: {}", obs_dir.display());
        }
        println!("\n  Knowledge, rules, config, and .gitignore preserved.");
    }

    println!("\nRun `cogz index` to rebuild the database from files.");
    println!(
        "Restart any running `cogz mcp-stdio` servers — they hold a handle to the deleted DB file."
    );

    Ok(())
}

pub fn run_consolidate(repo: &Path, dry_run: bool) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;

    // Load the NLI model for merge confirmation. The model is
    // lazy-loaded — always pass it so NLI can load on demand.
    // If the model can't load, confirm_duplicate_nli returns false
    // and candidates are skipped (conservative — no merge without
    // confirmation).
    let nli_model = {
        let models_dir = crate::cli_embed::models_dir();
        cogz::embed::OnnxNliModel::with_resource_config(
            &models_dir,
            &config.embedding.nli_model,
            config.embedding.model_idle_ttl,
            config.embedding.model_min_free_mb,
        )
    };
    let nli_ref: Option<&dyn cogz::embed::NliModel> = Some(&nli_model);

    println!(
        "Consolidating{}...",
        if dry_run { " (dry run)" } else { "" }
    );

    let promoted = cogz::consolidate::promote::run_promotion(
        &storage,
        &cogz_dir,
        &config.consolidation,
        dry_run,
    )?;
    println!("\n  Promoted: {}", promoted.len());
    for p in &promoted {
        if dry_run {
            println!(
                "    [dry-run] observation {} → would create rule ({})",
                p.observation_id, p.reason
            );
        } else {
            println!(
                "    observation {} → rule {} ({})",
                p.observation_id, p.new_rule_id, p.reason
            );
        }
    }

    let merged = cogz::consolidate::merge::run_merge(
        &storage,
        &cogz_dir,
        &config.consolidation,
        nli_ref,
        dry_run,
    )?;
    println!("\n  Merged: {}", merged.len());
    for m in &merged {
        println!(
            "    {} superseded by {} ({}){}",
            m.superseded_id,
            m.survivor_id,
            m.reason,
            if dry_run { " [dry-run]" } else { "" }
        );
    }

    if dry_run {
        println!("\n  (dry run — no changes made)");
    }

    Ok(())
}

pub fn run_verify(
    repo: &Path,
    entity_id: Option<&str>,
    selection: Option<cogz::index::drift::VerifySelection>,
    allow_dead_refs: bool,
) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;
    let opts = cogz::index::drift::VerifyOptions { allow_dead_refs };

    if let Some(selection) = selection {
        return run_batch_verify(&storage, &cogz_dir, selection, &opts);
    }

    let entity_id = entity_id
        .ok_or_else(|| anyhow::anyhow!("verify needs an entity id or a selection flag"))?;
    let out = cogz::index::drift::verify_entity(&storage, &cogz_dir, entity_id, &opts)?;

    println!(
        "Verified {entity_id}: {}/{} reference(s) resolved{}{}",
        out.resolved,
        out.resolved + out.dead_refs.len(),
        if out.reactivated {
            ", entity reactivated (stale → active)"
        } else {
            ""
        },
        if out.dead_refs.is_empty() {
            String::new()
        } else {
            format!(", dead refs: {}", out.dead_refs.join(", "))
        },
    );
    if !out.reactivated && !out.dead_refs.is_empty() {
        println!("  still stale — resolve the dead refs or use `--allow-dead-refs`.");
    }
    Ok(())
}

pub(crate) fn run_batch_verify(
    storage: &cogz::storage::Storage,
    cogz_dir: &Path,
    selection: cogz::index::drift::VerifySelection,
    opts: &cogz::index::drift::VerifyOptions,
) -> anyhow::Result<()> {
    let rows = cogz::index::drift::batch_verify(storage, cogz_dir, selection, opts);
    let mut verified = 0;
    let mut reactivated = 0;
    let mut still_stale = 0;
    let mut failed = 0;
    for row in &rows {
        match &row.outcome {
            Ok(o) => {
                verified += 1;
                if o.reactivated {
                    reactivated += 1;
                }
                if !o.dead_refs.is_empty() {
                    still_stale += 1;
                    println!(
                        "  ~ {} — {}/{} refs resolved, dead: {}",
                        row.title,
                        o.resolved,
                        o.resolved + o.dead_refs.len(),
                        o.dead_refs.join(", ")
                    );
                }
            }
            Err(e) => {
                failed += 1;
                println!("  ! {} — verify failed: {}", row.title, e);
            }
        }
    }
    println!(
        "Batch verify ({}): {} entities — {} verified, {} reactivated{}{}",
        selection.as_str(),
        rows.len(),
        verified,
        reactivated,
        if still_stale > 0 {
            format!(", {still_stale} with dead refs")
        } else {
            String::new()
        },
        if failed > 0 {
            format!(", {failed} failed")
        } else {
            String::new()
        },
    );
    if still_stale > 0 {
        println!(
            "Entries with dead refs stay stale — resolve anchors or `verify <id> --allow-dead-refs`."
        );
    }
    Ok(())
}

pub fn run_reject(repo: &Path, entity_id: &str, reason: Option<&str>) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = std::sync::Arc::new(cogz::storage::Storage::open(
        &db_path,
        config.embedding.dimension,
    )?);
    let outcome = cogz::files::reject::reject_entity_file(&storage, &cogz_dir, entity_id, reason)?;

    println!(
        "Rejected {}: {}{}",
        outcome.id,
        outcome.file_path.display(),
        outcome
            .reason
            .as_deref()
            .map(|r| format!(" — {r}"))
            .unwrap_or_default(),
    );
    Ok(())
}

pub use embed_bg::run_embed_bg;
pub use reindex_bg::run_reindex_bg;
