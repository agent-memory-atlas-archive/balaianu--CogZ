//! CLI entry points for the event/mining surface: `cogz suggest`,
//! `cogz capture-event`, and the MCP stdio server.
//!
//! Binary-only — bridges CLI commands to `hooks`/`storage`/`mcp`.

use cogz::storage::Storage;

/// Run `cogz suggest` — list mined observation candidates. The CLI
/// twin of the `suggest_observations` MCP tool: same mining pass,
/// same audit event, text output instead of JSON.
pub fn run_suggest(repo: &std::path::Path, days: u32, limit: u32) -> anyhow::Result<()> {
    let cogz_dir = repo.join(".cogz");
    let config_path = cogz_dir.join("config.toml");

    if !config_path.exists() {
        anyhow::bail!(
            "No .cogz/ directory found in {}. Run `cogz init` first.",
            repo.display()
        );
    }

    let config = cogz::config::load(&config_path)?;
    let db_path = cogz::config::resolve_db_path(repo, &config.storage.db_path)?;

    if !db_path.exists() {
        anyhow::bail!(
            "Database not found at {}. Run `cogz index` first.",
            db_path.display()
        );
    }

    let storage = Storage::open(&db_path, config.embedding.dimension)?;
    let conn = storage.conn();
    let days = days.min(90);
    let suggestions = cogz::storage::mining::mine_suggestions(&conn, days, limit as usize)?;

    if let Err(e) = cogz::storage::events::record_event(
        &conn,
        cogz::storage::events::EventType::SuggestionsRequested,
        None,
        &serde_json::json!({ "days": days, "returned": suggestions.len() }),
    ) {
        eprintln!("usage tracking: suggestions_requested event failed: {e}");
    }

    if suggestions.is_empty() {
        println!("No observation candidates in the last {days} day(s).");
        return Ok(());
    }

    println!(
        "{} candidate(s) mined over the last {days} day(s):\n",
        suggestions.len()
    );
    for (i, s) in suggestions.iter().enumerate() {
        println!("{}. [{}] {}", i + 1, s.signal, s.suggested_title);
        println!("   {}", s.suggested_content);
        if !s.suggested_refs.is_empty() {
            println!("   refs: {}", s.suggested_refs.join(", "));
        }
        println!();
    }
    println!(
        "If a candidate captures something non-obvious, record it via create_entity \
         (or a manual .cogz/observations/ file) — polish the draft or write your own."
    );
    Ok(())
}

/// Run the MCP server over stdio. The server starts with no
/// pre-loaded repos. Every tool call must provide a `repo` parameter
/// specifying the project root. The server opens and caches repos
/// on demand.
pub fn run_mcp_stdio() -> anyhow::Result<()> {
    let models_dir = crate::cli_embed::models_dir();
    let server = cogz::mcp::CogzServer::with_models_dir_only(&models_dir);

    // tracing must go to stderr, not stdout — stdout is the MCP transport
    tracing::info!("Starting CogZ MCP server over stdio");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(cogz::mcp::server::run_stdio(server))
}

/// Run `cogz capture-event` — capture a lifecycle event from hook
/// scripts. For session_start and prompt_submit, prints a context
/// pack to stdout for agent injection. For file_save, prints reindex
/// summary to stderr. Status info goes to stderr so stdout stays
/// clean for context pack injection.
pub fn run_capture_event(input: &cogz::hooks::CaptureInput) -> anyhow::Result<()> {
    let result = cogz::hooks::run_capture_event(input)?;

    // In hook-json mode, stdout is reserved for the JSON response.
    // Status info goes to stderr so it doesn't corrupt the JSON.
    if !input.hook_json {
        match result.event_id {
            Some(id) => eprintln!("Event {} recorded (id: {})", input.event_str, id),
            None => eprintln!(
                "Event {} processed (record skipped: db busy)",
                input.event_str
            ),
        }
    }
    if let Some(ref obs_id) = result.observation_id {
        eprintln!("Observation recorded: {}", obs_id);
    }
    if let Some(ref summary) = result.reindex_summary {
        if summary.reindexed {
            eprintln!(
                "Code reindex: {} created, {} updated, {} stale, {} knowledge flagged",
                summary.created,
                summary.updated,
                summary.marked_stale,
                summary.stale_knowledge_flagged
            );
        } else if summary.synced {
            eprintln!(
                "File sync: {} created, {} updated, {} stale, {} embedded",
                summary.created, summary.updated, summary.marked_stale, summary.embedded
            );
        }
    }
    if let Some(ref summary) = result.consolidation_summary {
        eprintln!(
            "Consolidation: {} promoted, {} merged",
            summary.promotions, summary.merges
        );
    }
    if let Some(count) = result.suggestion_count {
        eprintln!("Observations: {count} mined candidate(s) — run `cogz suggest` to review");
    }

    Ok(())
}
