//! Lifecycle event handlers — session_start, prompt_submit,
//! pre_tool_use, post_tool_use, file_save, session_end, stop.
//!
//! Each handler records a domain event. `session_start` and
//! `prompt_submit` also assemble a context pack for injection and
//! spawn a background reindex to catch non-hook changes. `post_tool_use`
//! records the event only (audit trail) — the agent decides what's
//! salient via the `create_entity` MCP tool. `file_save` triggers
//! a single-file code reindex and stale-knowledge flagging when the
//! saved file is a source file (not under `.cogz/`). `stop` is a
//! lightweight event that records the stop and returns — no context
//! pack, no side effects. `session_end` runs consolidation.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::json;

use crate::config::Config;
use crate::context::{
    AssembleParams, ContextMode, ContextPack, ContextSection, PackMetadata, assemble_context,
};
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
    /// Hook notices for agent injection — the drift notice when a
    /// file_save invalidated knowledge entities (the write-time half
    /// of the verify loop) and/or a write-back nudge when mining found
    /// fresh candidates. Rendered alongside (or instead of) a pack.
    pub notices: Option<String>,
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
            // Store the repo-relative form alongside the raw path —
            // mining matches it against entities.file_path and cannot
            // derive it later (it has no cogz_dir).
            let raw = input.file_path.unwrap_or("");
            json!({ "file_path": raw, "file_path_rel": repo_relative_path(raw, cogz_dir) })
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
    let mut context_pack = match input.event {
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
    // The agent decides what's salient via the create_entity MCP
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

    // file_save is also the edit-scoped delivery moment: rules
    // referencing entities on the saved path are actionable now —
    // the agent just touched the file they govern. Runs after the
    // reindex so fresh entity ids are visible. Silent when nothing
    // references the file — the hook injects nothing rather than
    // noise. Only fires when the event produced no pack of its own.
    if input.event == LifecycleEvent::FileSave
        && context_pack.is_none()
        && let Some(path) = input.file_path
        && !path.is_empty()
    {
        context_pack = scoped_rules_pack(storage, cogz_dir, event_id, path);
    }

    // file_save is also the write-time verify moment: the save may
    // have moved code that knowledge entities reference — and the
    // hook fires identically whether the edit came from the agent or
    // a human editor, so this is the surface that catches both. Runs
    // after the reindex so the drift rows are fresh. Orthogonal to the
    // scoped-rules pack (rules *about* the file vs knowledge
    // *invalidated by* the file) — both can fire on one save.
    let drift_notice = if input.event == LifecycleEvent::FileSave
        && let Some(path) = input.file_path
        && !path.is_empty()
    {
        drift_notice_for_path(storage, cogz_dir, path)
    } else {
        None
    };

    // Write-back nudge: mined candidates get pushed at the surfaces
    // where a learning moment just happened — saves, searches, prompt
    // boundaries, session edges. Fingerprint-deduped (once per
    // candidate per day) so this stays a signal, not a drip-feed.
    // The draft rides in the notice so create_entity is
    // confirm-not-compose; nothing is auto-written.
    let write_nudge = if matches!(
        input.event,
        LifecycleEvent::SessionStart
            | LifecycleEvent::PromptSubmit
            | LifecycleEvent::PostToolUse
            | LifecycleEvent::FileSave
            | LifecycleEvent::SessionEnd
    ) {
        let conn = storage.conn();
        // Mining reads untrusted tool-output payloads — a panic there
        // must not take the event's whole notice output down with it.
        let fresh = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::hooks::nudge::fresh_suggestions(&conn, input.event.as_str())
        }))
        .unwrap_or_else(|_| {
            tracing::warn!("write nudge: mining panicked — skipped");
            Vec::new()
        });
        if fresh.is_empty() {
            None
        } else {
            Some(crate::hooks::nudge::format_markdown(&fresh))
        }
    } else {
        None
    };

    let notices = [drift_notice, write_nudge]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
    let notices = (!notices.is_empty()).then_some(notices);

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
        notices,
        consolidation_summary,
        suggestion_count,
    })
}

/// Max drifted entities listed in a file_save notice — a signal, not
/// a report. The full queue lives in `cogz doctor`.
const DRIFT_NOTICE_LIMIT: usize = 5;

/// Knowledge entities drifted on references to the saved file's code —
/// the write-time verify cue. `entities_for_file` maps the path to its
/// code entities; `entities_drifted_on` inverts the drift index to find
/// what the edit invalidated. The notice is persistent, not
/// causal-diffed: every save of a file with still-drifted referencers
/// re-surfaces them until someone verifies — same cue-until-closed
/// contract as the read-side footer. Returns None when nothing on the
/// path is drifted.
fn drift_notice_for_path(
    storage: &Arc<Storage>,
    cogz_dir: &Path,
    file_path: &str,
) -> Option<String> {
    let conn = storage.conn();
    let mut entity_ids: Vec<String> = Vec::new();
    for candidate in normalize_file_candidates(file_path, cogz_dir) {
        if let Ok(found) = crate::storage::usage::entities_for_file(&conn, &candidate) {
            entity_ids.extend(found);
        }
    }
    entity_ids.sort();
    entity_ids.dedup();

    let drifted = crate::index::drift::entities_drifted_on(&conn, &entity_ids);
    if drifted.is_empty() {
        return None;
    }

    let mut notice = if drifted.len() == 1 {
        format!(
            "**1 knowledge entity references code in `{file_path}` and drifted since last verified:**"
        )
    } else {
        format!(
            "**{} knowledge entities reference code in `{file_path}` and drifted since last verified:**",
            drifted.len()
        )
    };
    for (id, title) in drifted.iter().take(DRIFT_NOTICE_LIMIT) {
        notice.push_str(&format!("\n- {title} (`{id}`)"));
    }
    if drifted.len() > DRIFT_NOTICE_LIMIT {
        notice.push_str(&format!(
            "\n- … and {} more (`cogz doctor` lists the queue)",
            drifted.len() - DRIFT_NOTICE_LIMIT
        ));
    }
    notice.push_str(
        "\n\nIf still accurate: `cogz verify <id>` (or `verify_knowledge` via MCP) re-stamps \
         provenance. If outdated: update the knowledge file.",
    );
    Some(notice)
}

/// Max rules pushed per edit-scoped delivery — a reminder, not a
/// briefing. Files with more governing rules truncate to the most
/// recently updated.
const SCOPED_RULE_LIMIT: usize = 5;

/// Per-rule content cap (~200 tokens). Full text stays on disk —
/// the pack tells the agent a rule exists and gives enough to act on.
const SCOPED_CONTENT_CHARS: usize = 800;

/// Build an edit-scoped pack for a file_save event: entities on the
/// saved path → active rules referencing them (`references` +
/// `auto_references` edges). The delivery is recorded as
/// `DeliveryKind::Scoped` so its hit rate is measurable separately
/// from packs. Returns None when nothing references the file — the
/// hook stays silent rather than injecting noise.
fn scoped_rules_pack(
    storage: &Arc<Storage>,
    cogz_dir: &Path,
    event_id: Option<i64>,
    file_path: &str,
) -> Option<ContextPack> {
    let conn = storage.conn();

    let mut entity_ids: Vec<String> = Vec::new();
    for candidate in normalize_file_candidates(file_path, cogz_dir) {
        match crate::storage::usage::entities_for_file(&conn, &candidate) {
            Ok(found) => entity_ids.extend(found),
            Err(e) => tracing::warn!("scoped delivery: file lookup failed: {e}"),
        }
    }
    entity_ids.sort();
    entity_ids.dedup();

    let rules = match crate::storage::graph_queries::rules_referencing(
        &conn,
        &entity_ids,
        SCOPED_RULE_LIMIT,
    ) {
        Ok(rules) => rules,
        Err(e) => {
            tracing::warn!("scoped delivery: rule lookup failed: {e}");
            return None;
        }
    };
    if rules.is_empty() {
        return None;
    }

    // Rules on a just-saved file are prime drift candidates — surface
    // the marker so the pack's verify cue reaches edit-scoped pushes too.
    let drift = crate::index::drift::drift_counts(
        &conn,
        &rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
    );

    let sections: Vec<ContextSection> = rules
        .iter()
        .map(|rule| {
            let content = if rule.content.chars().count() > SCOPED_CONTENT_CHARS {
                let head: String = rule.content.chars().take(SCOPED_CONTENT_CHARS).collect();
                format!("{head}\n\n…")
            } else {
                rule.content.clone()
            };
            ContextSection {
                source: rule.r#type.clone(),
                entity_id: rule.id.clone(),
                title: rule.title.clone().unwrap_or_else(|| rule.id.clone()),
                content,
                relevance: 0.0,
                graph_path: vec![rule.id.clone()],
                graph_path_description: String::new(),
                tier: crate::storage::usage::DeliveryTier::Full,
                drift_count: drift.get(&rule.id).copied().unwrap_or(0),
            }
        })
        .collect();
    let size_tokens: usize = sections.iter().map(|s| s.content.len() / 4).sum();
    let rule_ids: Vec<String> = rules.iter().map(|r| r.id.clone()).collect();

    // Same delivery bookkeeping as packs — scoped is its own kind so
    // hit rates split by delivery moment.
    if let Some(delivery_id) = with_busy_retry("record scoped delivery", || {
        crate::storage::usage::record_delivery(
            &conn,
            crate::storage::usage::DeliveryKind::Scoped,
            event_id,
        )
    }) {
        let entries: Vec<(String, crate::storage::usage::DeliveryTier)> = rule_ids
            .iter()
            .map(|id| (id.clone(), crate::storage::usage::DeliveryTier::Full))
            .collect();
        with_busy_retry("record scoped delivered", || {
            crate::storage::usage::record_delivered(&conn, delivery_id, &entries)
        });
    }
    if let Some(eid) = event_id {
        let meta = serde_json::json!({ "scoped_rules": rule_ids });
        with_busy_retry("annotate event", || {
            events::annotate_event(&conn, eid, &meta)
        });
    }

    Some(ContextPack {
        query: file_path.to_string(),
        mode: ContextMode::Task,
        sections,
        metadata: PackMetadata {
            size_tokens,
            selected_sources: vec!["rule".to_string()],
            dropped_sources: Vec::new(),
            search_mode: "scoped".to_string(),
            pointer_ids: Vec::new(),
            signals: None,
        },
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

/// Repo-relative form of a hook-supplied save path, best-effort:
/// absolute paths strip the canonical repo root, relative paths lose
/// their `./` prefix. Stored on file_save events so mining can match
/// `entities.file_path` without knowing `cogz_dir`.
fn repo_relative_path(raw: &str, cogz_dir: &Path) -> String {
    let path = Path::new(raw);
    let repo_root = cogz_dir.parent().unwrap_or_else(|| Path::new("."));
    let repo_abs = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    for base in [repo_root, repo_abs.as_path()] {
        if let Ok(rel) = path.strip_prefix(base) {
            return rel.to_string_lossy().to_string();
        }
    }
    raw.trim_start_matches("./").to_string()
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
