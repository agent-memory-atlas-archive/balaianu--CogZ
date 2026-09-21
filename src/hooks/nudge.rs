//! Write-back nudges — surfacing mined observation candidates at the
//! moment they matter, instead of waiting for the agent to pull
//! `suggest_observations` (which measured adoption says it never does).
//!
//! The mechanism is deliberately dumb: mining signals are heuristic and
//! free, the draft is pre-composed so `create_entity` is confirm-not-
//! compose, and the agent's own model does any polish — no background
//! LLM pass. Ignored nudges stay discoverable via `suggest_observations`;
//! nothing is auto-written.

use rusqlite::Connection;

use crate::storage::events::{self, EventType};
use crate::storage::mining::{self, Suggestion};

/// How far back mining reads when nudging. Fresh signals only — a
/// week-old candidate in a hook notice reads as nagging.
const NUDGE_DAYS: u32 = 1;
/// Max candidates mined per nudge check (dedup pool, not display).
const NUDGE_POOL: usize = 8;
/// A fingerprint shown inside this window is suppressed — the same
/// candidate re-surfaces at most daily while it stays mined.
const SUPPRESS_HOURS: &str = "-24 hours";

/// Mine candidates and keep only those not nudged within the suppress
/// window. When any remain, records a `write_nudge_shown` event carrying
/// their fingerprints — that event is both the dedup ledger and the
/// impression side of the adoption funnel.
pub fn fresh_suggestions(conn: &Connection, surface: &str) -> Vec<Suggestion> {
    let candidates = match mining::mine_suggestions(conn, NUDGE_DAYS, NUDGE_POOL) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("write nudge: mining failed: {e}");
            return Vec::new();
        }
    };
    if candidates.is_empty() {
        return candidates;
    }

    let shown = shown_fingerprints(conn);
    let fresh: Vec<Suggestion> = candidates
        .into_iter()
        .filter(|s| !shown.contains(&s.fingerprint()))
        .collect();
    if fresh.is_empty() {
        return fresh;
    }

    let fingerprints: Vec<String> = fresh.iter().map(|s| s.fingerprint()).collect();
    if let Err(e) = events::record_event(
        conn,
        EventType::WriteNudgeShown,
        None,
        &serde_json::json!({
            "surface": surface,
            "count": fresh.len(),
            "fingerprints": fingerprints,
        }),
    ) {
        tracing::warn!("write nudge: impression event failed: {e}");
    }
    fresh
}

/// Fingerprints shown inside the suppress window, from
/// `write_nudge_shown` payloads.
fn shown_fingerprints(conn: &Connection) -> std::collections::HashSet<String> {
    let mut stmt = match conn.prepare(
        "SELECT payload FROM events
         WHERE event_type = 'write_nudge_shown'
           AND created_at > datetime('now', ?)",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("write nudge: dedup query failed: {e}");
            return std::collections::HashSet::new();
        }
    };
    let rows = stmt.query_map(rusqlite::params![SUPPRESS_HOURS], |r| r.get::<_, String>(0));
    let mut shown = std::collections::HashSet::new();
    match rows {
        Ok(rows) => {
            for row in rows.flatten() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&row)
                    && let Some(arr) = v["fingerprints"].as_array()
                {
                    for f in arr.iter().filter_map(|f| f.as_str()) {
                        shown.insert(f.to_string());
                    }
                }
            }
        }
        Err(e) => tracing::warn!("write nudge: dedup scan failed: {e}"),
    }
    shown
}

/// Markdown nudge for hook `additionalContext` — names the top
/// candidate verbatim so the write is confirm-not-compose.
pub fn format_markdown(suggestions: &[Suggestion]) -> String {
    if suggestions.is_empty() {
        return String::new();
    }
    let top = &suggestions[0];
    let mut out = format!(
        "---\n💡 **{} observation candidate(s) detected from recent work.** Top: *{}* — {}",
        suggestions.len(),
        top.suggested_title,
        top.suggested_content,
    );
    if !top.suggested_refs.is_empty() {
        out.push_str(&format!(
            "\nCandidate references: {}",
            top.suggested_refs.join(", ")
        ));
    }
    out.push_str(
        "\n\nIf it captures something non-obvious, record it: `create_entity` \
         (entity_type=\"observation\", title/content/refs above — polish the draft or write \
         your own). Review all candidates: `cogz suggest`.",
    );
    out
}

/// JSON nudge block for MCP tool responses — the drafted fields ride
/// along so `create_entity` is a one-call confirm.
pub fn format_json(suggestions: &[Suggestion]) -> serde_json::Value {
    let top = &suggestions[0];
    serde_json::json!({
        "candidates": suggestions.len(),
        "top": {
            "signal": top.signal,
            "title": top.suggested_title,
            "content": top.suggested_content,
            "references": top.suggested_refs,
        },
        "action": "If this captures something non-obvious from your work, record it via \
                   create_entity (entity_type=\"observation\", title/content/references as \
                   drafted or rewritten). Review all candidates via suggest_observations \
                   or `cogz suggest`.",
    })
}

#[cfg(test)]
#[path = "nudge_tests.rs"]
mod tests;
