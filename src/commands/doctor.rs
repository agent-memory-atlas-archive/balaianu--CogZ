//! `cogz doctor` subcommand handler — health check and pruning.

use std::path::Path;

use cogz::config::Config;

use super::load_config;

/// `cogz doctor` — health check and optional observation pruning.
pub fn run_doctor(repo: &Path, prune_observations: bool, confirm: bool) -> anyhow::Result<()> {
    let (config, db_path) = load_config(repo)?;
    let cogz_dir = repo.join(".cogz");

    if prune_observations {
        return run_prune_report(&db_path, &config, &cogz_dir, confirm);
    }

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = cogz::storage::Storage::open(&db_path, config.embedding.dimension)?;
    let report = cogz::doctor::run_doctor(&storage, &config, &cogz_dir, repo);

    println!("CogZ doctor — health check\n");
    println!(
        "  DB integrity: {}",
        if report.db_healthy { "ok" } else { "FAILED" }
    );
    println!("  Schema version: {}", report.schema_version);
    println!("  Entities: {}", report.entity_count);
    println!("  Edges: {}", report.edge_count);
    println!(
        "  Models: {}",
        if report.models_available {
            "available"
        } else {
            "not downloaded (FTS-only mode)"
        }
    );

    if let Some(usage) = &report.usage {
        let pack_total = usage.pack_hits + usage.pack_misses;
        let pct = (usage.pack_hits * 100).checked_div(pack_total).unwrap_or(0);
        println!(
            "  Memory hit rate: {}% ({} hits / {} pack entities)",
            pct, usage.pack_hits, pack_total
        );
        let search_total = usage.search_hits + usage.search_misses;
        let spct = (usage.search_hits * 100)
            .checked_div(search_total)
            .unwrap_or(0);
        println!(
            "  Search hit rate: {}% ({} hits / {} search entities)",
            spct, usage.search_hits, search_total
        );
        println!(
            "  Dead weight: {} entities never retrieved in last 10 sessions",
            usage.dead_weight.len()
        );
        println!(
            "  Write quality: {} file entities never retrieved",
            usage.never_retrieved_files.len()
        );
    }

    if report.issues.is_empty() {
        println!("\n  No issues found.");
    } else {
        println!("\n  Issues ({}):", report.issues.len());
        for issue in &report.issues {
            let id = issue.entity_id.as_deref().unwrap_or("-");
            println!("    [{}] {} — {}", issue.kind, id, issue.message);
        }
    }

    Ok(())
}

fn run_prune_report(
    db_path: &Path,
    config: &Config,
    cogz_dir: &Path,
    confirm: bool,
) -> anyhow::Result<()> {
    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = cogz::storage::Storage::open(db_path, config.embedding.dimension)?;
    let report = cogz::doctor::run_prune(&storage, config, cogz_dir, confirm);

    if !confirm {
        println!("CogZ doctor — prune observations (dry-run)\n");
        if report.candidates.is_empty() {
            println!("  No prunable observations found.");
        } else {
            println!("  Prunable observations ({}):", report.candidates.len());
            for c in &report.candidates {
                println!(
                    "    {} [{}] age={}d — {}",
                    c.entity_id, c.status, c.age_days, c.file_path
                );
            }
            println!("\n  Run with --confirm to prune.");
        }
    } else {
        println!("CogZ doctor — prune observations (confirmed)\n");
        println!("  Pruned: {}", report.pruned);
        println!("  Skipped: {}", report.skipped);
        if report.tombstones_removed > 0 {
            println!("  Old tombstones removed: {}", report.tombstones_removed);
        }
    }

    Ok(())
}
