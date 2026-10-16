//! `cogz reject` — mark an entity rejected with an optional reason.

use std::path::Path;

use super::load_config;

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
