//! Lifecycle event handlers — session_start, prompt_submit,
//! pre_tool_use, post_tool_use, file_save, session_end, stop.
//!
//! Each handler records a domain event. `session_start` and
//! `prompt_submit` also assemble a context pack for injection and
//! spawn a background reindex to catch non-hook changes. `post_tool_use`
//! records the event only (audit trail) — the agent decides what's
//! salient via the `record_observation` MCP tool. `file_save` triggers
//! a single-file code reindex and stale-knowledge flagging when the
//! saved file is a source file (not under `.cogz/`). `stop` is a
//! lightweight event that records the stop and returns — no context
//! pack, no side effects. `session_end` runs consolidation.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use crate::config::Config;
use crate::context::{AssembleParams, ContextMode, ContextPack, assemble_context};
use crate::embed::{NliModel, OnnxEmbeddingModel};
use crate::storage::{Storage, events};

/// Which lifecycle event triggered the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    SessionStart,
    PromptSubmit,
    PreToolUse,
    PostToolUse,
    FileSave,
    SessionEnd,
    Stop,
}

impl LifecycleEvent {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "session_start" => Some(Self::SessionStart),
            "prompt_submit" => Some(Self::PromptSubmit),
            "pre_tool_use" => Some(Self::PreToolUse),
            "post_tool_use" => Some(Self::PostToolUse),
            "file_save" => Some(Self::FileSave),
            "session_end" => Some(Self::SessionEnd),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::PromptSubmit => "prompt_submit",
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::FileSave => "file_save",
            Self::SessionEnd => "session_end",
            Self::Stop => "stop",
        }
    }

    fn event_type(&self) -> events::EventType {
        match self {
            Self::SessionStart => events::EventType::SessionStart,
            Self::PromptSubmit => events::EventType::PromptSubmit,
            Self::PreToolUse => events::EventType::PreToolUse,
            Self::PostToolUse => events::EventType::PostToolUse,
            Self::FileSave => events::EventType::FileSave,
            Self::SessionEnd => events::EventType::SessionEnd,
            Self::Stop => events::EventType::Stop,
        }
    }
}

impl std::fmt::Display for LifecycleEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Input parameters for a lifecycle event.
pub struct LifecycleInput<'a> {
    pub event: LifecycleEvent,
    pub prompt: Option<&'a str>,
    pub tool_name: Option<&'a str>,
    pub tool_result: Option<&'a str>,
    pub file_path: Option<&'a str>,
}

/// Result of handling a lifecycle event.
pub struct LifecycleOutput {
    pub event_id: i64,
    /// Context pack for session_start and prompt_submit; None for tool events.
    pub context_pack: Option<ContextPack>,
    /// Observation UUID if an observation was recorded (post_tool_use only).
    pub observation_id: Option<String>,
    /// Reindex summary if a file_save triggered code reindexing.
    pub reindex_summary: Option<ReindexSummary>,
    /// Consolidation dry-run summary if session_end triggered it.
    pub consolidation_summary: Option<ConsolidationSummary>,
}

/// Summary of a consolidation dry-run triggered by session_end.
#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationSummary {
    pub promotions: usize,
    pub merges: usize,
}

/// Summary of a file_save hook — either a code reindex (source files)
/// or a file sync + embedding (.cogz/ entity files).
#[derive(Debug, Clone, Serialize)]
pub struct ReindexSummary {
    /// True if a code reindex was triggered (source file saved).
    pub reindexed: bool,
    /// True if a .cogz/ file sync was triggered (entity file saved).
    pub synced: bool,
    pub created: usize,
    pub updated: usize,
    pub marked_stale: usize,
    pub stale_knowledge_flagged: usize,
    /// Entities embedded after sync (0 if model unavailable).
    pub embedded: usize,
}

/// Handle a lifecycle event: record it, optionally assemble a context
/// pack, optionally record an observation.
pub fn handle_lifecycle_event(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &Path,
    query_model: &OnnxEmbeddingModel,
    code_model: &OnnxEmbeddingModel,
    nli_model: Option<&dyn NliModel>,
    input: &LifecycleInput,
) -> Result<LifecycleOutput, LifecycleError> {
    let event_type = input.event.event_type();

    // Build the event payload.
    let payload = match input.event {
        LifecycleEvent::SessionStart => json!({}),
        LifecycleEvent::PromptSubmit => {
            json!({ "prompt": input.prompt.unwrap_or("") })
        }
        LifecycleEvent::PreToolUse => {
            json!({ "tool_name": input.tool_name.unwrap_or("") })
        }
        LifecycleEvent::PostToolUse => {
            json!({
                "tool_name": input.tool_name.unwrap_or(""),
                "tool_result": input.tool_result.unwrap_or(""),
            })
        }
        LifecycleEvent::FileSave => {
            json!({ "file_path": input.file_path.unwrap_or("") })
        }
        LifecycleEvent::SessionEnd => json!({}),
        LifecycleEvent::Stop => json!({}),
    };

    // Record the event.
    let event_id = {
        let conn = storage.conn();
        events::record_event(&conn, event_type, None, &payload)?
    };

    // Spawn background reindex for session_start and prompt_submit
    // to catch changes from non-hook events (branch switches, pulls,
    // merges, human edits). Done before context pack assembly so
    // that a context pack failure doesn't skip the recovery path.
    // session_start always spawns (primary recovery, fires once per
    // session). prompt_submit is debounced to avoid redundant spawns.
    let repo_root = cogz_dir.parent().unwrap_or_else(|| Path::new("."));
    match input.event {
        LifecycleEvent::SessionStart => {
            crate::hooks::reindex::spawn_reindex_bg(repo_root, true);
        }
        LifecycleEvent::PromptSubmit => {
            crate::hooks::reindex::spawn_reindex_bg(repo_root, false);
        }
        _ => {}
    }

    // Assemble context pack for session_start and prompt_submit.
    // A new pack is a usage boundary: entities pending in earlier
    // deliveries that the agent never touched become misses. The
    // delivery window is pack-to-pack (or pack-to-session-end), which
    // is the honest unit of "did the agent use what we gave it".
    let context_pack = match input.event {
        LifecycleEvent::SessionStart | LifecycleEvent::PromptSubmit => {
            {
                let conn = storage.conn();
                if let Err(e) = crate::storage::usage::close_open_deliveries(&conn) {
                    tracing::warn!("usage tracking: close deliveries failed: {e}");
                }
            }
            let (mode, query) = match input.event {
                LifecycleEvent::SessionStart => (ContextMode::ColdStart, None),
                _ => (ContextMode::Task, input.prompt),
            };
            let pack = assemble_pack(storage, config, query_model, code_model, mode, query)?;
            {
                let conn = storage.conn();
                let entity_ids: Vec<String> =
                    pack.sections.iter().map(|s| s.entity_id.clone()).collect();
                match crate::storage::usage::record_delivery(
                    &conn,
                    crate::storage::usage::DeliveryKind::Pack,
                    Some(event_id),
                ) {
                    Ok(delivery_id) => {
                        if let Err(e) =
                            crate::storage::usage::record_delivered(&conn, delivery_id, &entity_ids)
                        {
                            tracing::warn!("usage tracking: record delivered failed: {e}");
                        }
                    }
                    Err(e) => tracing::warn!("usage tracking: record delivery failed: {e}"),
                }
            }
            Some(pack)
        }
        _ => None,
    };

    // post_tool_use: event is recorded above (audit trail) — plus usage
    // hit detection: if the tool touched a file or named an entity that
    // a pending delivery surfaced, mark it used. We no longer
    // auto-create observation files for every tool use — that flooded
    // .cogz/observations/ with low-value entries. The agent decides
    // what's salient via the record_observation MCP tool.
    if input.event == LifecycleEvent::PostToolUse {
        let conn = storage.conn();
        let hits = detect_touched_entities(&conn, cogz_dir, input.file_path, input.tool_result);
        if !hits.is_empty()
            && let Err(e) = crate::storage::usage::record_hits(&conn, &hits)
        {
            tracing::warn!("usage tracking: record hits failed: {e}");
        }
    }
    let observation_id: Option<String> = None;

    // For file_save, either reindex source code (source files) or
    // sync + embed .cogz/ entity files. This keeps both the code index
    // and the knowledge DB fresh without requiring a manual `cogz
    // reindex` after every change.
    let reindex_summary = if input.event == LifecycleEvent::FileSave {
        Some(crate::hooks::handlers::handle_file_save(
            storage,
            config,
            cogz_dir,
            query_model,
            input.file_path,
        ))
    } else {
        None
    };

    // For session_end, run consolidation (promotion + merge) for real.
    // The configured thresholds are the safety mechanism — if they're
    // met, the system acts. This aligns with the first principle that
    // consolidation is continuous, not batch. Rules created by promotion
    // are git-tracked and reviewable; merged entities are superseded
    // (not deleted) and remain in the graph.
    let consolidation_summary = if input.event == LifecycleEvent::SessionEnd {
        // Session boundary closes all open deliveries — anything the
        // agent didn't touch by now is a miss.
        let conn = storage.conn();
        if let Err(e) = crate::storage::usage::close_open_deliveries(&conn) {
            tracing::warn!("usage tracking: close deliveries failed: {e}");
        }
        drop(conn);
        Some(crate::hooks::handlers::handle_session_end(
            storage, config, cogz_dir, nli_model,
        ))
    } else {
        None
    };

    Ok(LifecycleOutput {
        event_id,
        context_pack,
        observation_id,
        reindex_summary,
        consolidation_summary,
    })
}

/// Entities the agent touched in a tool call. Attribution is
/// approximate: a file read/write credits every entity on that path
/// (file + its functions/classes), and a tool result mentioning a
/// pending entity's title or id credits that entity. Knowledge-entity
/// hits undercount — the agent can act on a rule without re-reading
/// its file.
fn detect_touched_entities(
    conn: &rusqlite::Connection,
    cogz_dir: &Path,
    file_path: Option<&str>,
    tool_result: Option<&str>,
) -> Vec<String> {
    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(raw) = file_path {
        for candidate in normalize_file_candidates(raw, cogz_dir) {
            match crate::storage::usage::entities_for_file(conn, &candidate) {
                Ok(found) => ids.extend(found),
                Err(e) => tracing::warn!("usage tracking: file lookup failed: {e}"),
            }
        }
    }

    if let Some(text) = tool_result
        && !text.is_empty()
        && let Ok(pending) = crate::storage::usage::pending_entities(conn)
    {
        for (id, title) in pending {
            if text.contains(&id) || (title.len() >= 8 && text.contains(&title)) {
                ids.insert(id);
            }
        }
    }

    ids.into_iter().collect()
}

/// Candidate `entities.file_path` forms for a hook-supplied path.
/// Code entities store repo-relative paths; entity files store
/// cogz-relative paths. cogz_dir may itself be relative (`./.cogz`
/// when --repo is `.`), so strip prefixes against its canonical
/// absolute form too. Returns every form worth matching.
fn normalize_file_candidates(raw: &str, cogz_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let path = Path::new(raw);

    let cogz_abs = std::fs::canonicalize(cogz_dir).unwrap_or_else(|_| cogz_dir.to_path_buf());
    for base in [cogz_dir, cogz_abs.as_path()] {
        if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().to_string());
        }
        if let Some(repo_root) = base.parent()
            && let Ok(rel) = path.strip_prefix(repo_root)
        {
            let rel = rel.to_string_lossy().to_string();
            out.push(rel.clone());
            if let Some(inner) = rel.strip_prefix(".cogz/") {
                out.push(inner.to_string());
            }
        }
    }

    let trimmed = raw.trim_start_matches("./").trim_start_matches('/');
    out.push(trimmed.to_string());
    if let Some(inner) = trimmed.strip_prefix(".cogz/") {
        out.push(inner.to_string());
    }

    out.sort();
    out.dedup();
    out
}

/// Assemble a context pack, embedding the query with both models
/// when available. The code embedding enables vector search over
/// code entities (functions, classes, files) alongside knowledge.
fn assemble_pack(
    storage: &Arc<Storage>,
    config: &Config,
    query_model: &OnnxEmbeddingModel,
    code_model: &OnnxEmbeddingModel,
    mode: ContextMode,
    query: Option<&str>,
) -> Result<ContextPack, LifecycleError> {
    let knowledge_embedding = query.and_then(|q| {
        use crate::embed::EmbeddingModel;
        query_model
            .embed_query(&[q])
            .ok()
            .and_then(|v| v.into_iter().next())
    });

    let code_embedding = query.and_then(|q| {
        use crate::embed::EmbeddingModel;
        code_model
            .embed_query(&[q])
            .ok()
            .and_then(|v| v.into_iter().next())
    });

    let pack = {
        let conn = storage.conn();
        assemble_context(
            &conn,
            &AssembleParams {
                mode,
                query,
                knowledge_embedding: knowledge_embedding.as_deref(),
                code_embedding: code_embedding.as_deref(),
                max_tokens: None,
                include_stale: false,
            },
            config,
        )?
    };

    Ok(pack)
}

/// Error during lifecycle event handling.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("context assembly error: {0}")]
    Context(#[from] crate::context::AssembleError),
    #[error("file write error: {0}")]
    FileWrite(#[from] std::io::Error),
}
