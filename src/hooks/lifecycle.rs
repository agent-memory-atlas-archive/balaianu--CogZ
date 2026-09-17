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
    /// None when event recording exhausted busy retries — the hook
    /// still produces its output, only the audit row is missing.
    pub event_id: Option<i64>,
    /// Context pack for session_start and prompt_submit; None for tool events.
    pub context_pack: Option<ContextPack>,
    /// Observation UUID if an observation was recorded (post_tool_use only).
    pub observation_id: Option<String>,
    /// Reindex summary if a file_save triggered code reindexing.
    pub reindex_summary: Option<ReindexSummary>,
    /// Consolidation dry-run summary if session_end triggered it.
    pub consolidation_summary: Option<ConsolidationSummary>,
    /// Mined observation candidates available at session_end — a nudge
    /// to call `suggest_observations` before the trail goes cold.
    pub suggestion_count: Option<usize>,
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

    // Record the event. A transient lock from the detached reindex can
    // outlive busy_timeout — retry, then degrade to event_id=None so
    // pack assembly still ships (audit must never break the hook).
    let event_id = {
        let conn = storage.conn();
        with_busy_retry("record event", || {
            events::record_event(&conn, event_type, None, &payload)
        })
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
                // Resolve hits before the new pack boundary closes them:
                // a prompt naming a delivered entity or file is evidence
                // the context was used. Devin does not dispatch
                // PostToolUse, so prompt_submit is a primary hit surface.
                if input.event == LifecycleEvent::PromptSubmit {
                    let paths: Vec<String> =
                        input.prompt.map(prompt_file_tokens).unwrap_or_default();
                    let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
                    let hits = detect_touched_entities(&conn, cogz_dir, &path_refs, input.prompt);
                    if !hits.is_empty() {
                        with_busy_retry("record hits", || {
                            crate::storage::usage::record_hits(&conn, &hits)
                        });
                    }
                }
                with_busy_retry("close deliveries", || {
                    crate::storage::usage::close_open_deliveries(&conn)
                });
            }
            let (mode, query) = match input.event {
                LifecycleEvent::SessionStart => (ContextMode::ColdStart, None),
                _ => (ContextMode::Task, input.prompt),
            };
            let pack = assemble_pack(storage, config, query_model, code_model, mode, query)?;
            {
                let conn = storage.conn();
                let entries = delivered_entries(&pack);
                if let Some(delivery_id) = with_busy_retry("record delivery", || {
                    crate::storage::usage::record_delivery(
                        &conn,
                        crate::storage::usage::DeliveryKind::Pack,
                        event_id,
                    )
                }) {
                    with_busy_retry("record delivered", || {
                        crate::storage::usage::record_delivered(&conn, delivery_id, &entries)
                    });
                }
                // Persist pack metadata onto the event so pack shape
                // (size, pointer count) is analyzable later.
                if let Some(eid) = event_id {
                    let meta = serde_json::json!({
                        "pack": {
                            "size_tokens": pack.metadata.size_tokens,
                            "sections": pack.sections.len(),
                            "pointers": pack.metadata.pointer_ids.len(),
                            "search_mode": pack.metadata.search_mode,
                            "signals": pack.metadata.signals.as_ref().map(|s| {
                                serde_json::json!({
                                    "code_strength": s.code_strength,
                                    "code_gradient": s.code_gradient,
                                    "knowledge_strength": s.knowledge_strength,
                                    "knowledge_gradient": s.knowledge_gradient,
                                })
                            }),
                        }
                    });
                    with_busy_retry("annotate event", || {
                        events::annotate_event(&conn, eid, &meta)
                    });
                }
            }
            Some(pack)
        }
        _ => None,
    };

    // post_tool_use / file_save: event is recorded above (audit
    // trail) — plus usage hit detection: if the tool touched a file or
    // named an entity that a pending delivery surfaced, mark it used.
    // file_save doubles as the hit surface on hosts that never dispatch
    // post_tool_use (editing a delivered file is direct evidence of
    // use). We no longer auto-create observation files for every tool
    // use — that flooded .cogz/observations/ with low-value entries.
    // The agent decides what's salient via the record_observation MCP
    // tool.
    if matches!(
        input.event,
        LifecycleEvent::PostToolUse | LifecycleEvent::FileSave
    ) {
        let conn = storage.conn();
        let paths: Vec<&str> = input.file_path.into_iter().collect();
        let hits = detect_touched_entities(&conn, cogz_dir, &paths, input.tool_result);
        if !hits.is_empty() {
            with_busy_retry("record hits", || {
                crate::storage::usage::record_hits(&conn, &hits)
            });
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
        with_busy_retry("close deliveries", || {
            crate::storage::usage::close_open_deliveries(&conn)
        });
        drop(conn);
        Some(crate::hooks::handlers::handle_session_end(
            storage, config, cogz_dir, nli_model,
        ))
    } else {
        None
    };

    // session_end is the natural moment to surface unmined candidates —
    // the session's usage signals are closed and complete.
    let suggestion_count = if input.event == LifecycleEvent::SessionEnd {
        let conn = storage.conn();
        match crate::storage::mining::mine_suggestions(&conn, 7, 50) {
            Ok(s) if s.is_empty() => None,
            Ok(s) => Some(s.len()),
            Err(e) => {
                tracing::warn!("suggestion mining failed at session_end: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(LifecycleOutput {
        event_id,
        context_pack,
        observation_id,
        reindex_summary,
        consolidation_summary,
        suggestion_count,
    })
}

/// The (entity_id, tier) pairs a pack delivered: each section's own
/// tier plus every entity listed in the pointer index as `Pointer`.
fn delivered_entries(pack: &ContextPack) -> Vec<(String, crate::storage::usage::DeliveryTier)> {
    use crate::storage::usage::DeliveryTier;
    let mut entries: Vec<(String, DeliveryTier)> = pack
        .sections
        .iter()
        .map(|s| (s.entity_id.clone(), s.tier))
        .collect();
    entries.extend(
        pack.metadata
            .pointer_ids
            .iter()
            .map(|id| (id.clone(), DeliveryTier::Pointer)),
    );
    entries
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
    file_paths: &[&str],
    tool_result: Option<&str>,
) -> Vec<String> {
    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for raw in file_paths {
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

/// Lifecycle DB writes retry on transient lock contention: the
/// detached reindex can hold the write lock past busy_timeout, and a
/// dropped close leaks deliveries open until the next boundary. Other
/// errors warn once and give up — telemetry must never break the hook.
fn with_busy_retry<T>(
    what: &str,
    mut f: impl FnMut() -> Result<T, crate::storage::StorageError>,
) -> Option<T> {
    for attempt in 0..4 {
        match f() {
            Ok(v) => return Some(v),
            Err(e) if is_busy(&e) => {
                std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
            }
            Err(e) => {
                tracing::warn!("usage tracking: {what} failed: {e}");
                return None;
            }
        }
    }
    tracing::warn!("usage tracking: {what} failed: lock still held after retries");
    None
}

fn is_busy(e: &crate::storage::StorageError) -> bool {
    matches!(
        e,
        crate::storage::StorageError::Sqlite(rusqlite::Error::SqliteFailure(code, _))
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

/// Source extensions worth treating as file references inside a
/// prompt. Keeps token extraction cheap — a bare word like `fix`
/// never becomes a file candidate.
const PROMPT_FILE_EXTS: &[&str] = &[
    ".rs", ".py", ".go", ".js", ".jsx", ".ts", ".tsx", ".sh", ".md", ".toml", ".json",
];

/// Tokens in a prompt that look like file paths — either contain a
/// path separator or end in a known source extension. Prompts are the
/// main hit surface on hosts without post_tool_use, and users
/// reference files by path constantly ("fix src/storage/usage.rs").
fn prompt_file_tokens(prompt: &str) -> Vec<String> {
    prompt
        .split(|c: char| c.is_whitespace() || matches!(c, '`' | '"' | '\'' | '(' | ')' | ',' | ';'))
        .filter(|t| t.contains('/') || PROMPT_FILE_EXTS.iter().any(|e| t.ends_with(e)))
        .map(str::to_string)
        .collect()
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
