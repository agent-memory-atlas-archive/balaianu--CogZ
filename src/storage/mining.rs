//! Write-path mining — surfaces observation candidates from usage
//! signals without writing anything itself.
//!
//! Four signals: edits after zero-hit packs (uncharted work), entities
//! delivered then hit repeatedly (worth promoting), files edited over
//! and over (hot spots), and tool errors followed by a fix (pitfalls).
//! The consuming agent's model is the distiller — these are structured
//! leads, not auto-written knowledge.

use rusqlite::Connection;

use super::StorageError;

/// A mined observation candidate. The agent confirms or dismisses it
/// through `record_observation` — nothing here writes itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Suggestion {
    /// Which mining signal produced this candidate.
    pub signal: &'static str,
    /// Signal-specific evidence (event ids, counts, file paths).
    pub evidence: serde_json::Value,
    pub suggested_title: String,
    pub suggested_content: String,
    /// Entity ids worth referencing (e.g. the repeatedly-hit entity).
    pub suggested_refs: Vec<String>,
}

/// Minimum hits before an entity is flagged as recurring-reliance.
const RECURRING_HIT_MIN: i64 = 2;
/// Minimum saves before a file is flagged as a hot spot.
const HOT_FILE_MIN: i64 = 3;
/// How many later events to scan for the fixing call after an error.
const ERROR_FIX_WINDOW: i64 = 20;

/// Run all mining passes and return candidates, newest signals first,
/// capped at `limit`. `days` bounds how far back events are read.
pub fn mine_suggestions(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut out = Vec::new();
    out.extend(uncharted_edits(conn, days, limit)?);
    out.extend(recurring_use(conn, days, limit)?);
    out.extend(hot_files(conn, days, limit)?);
    out.extend(error_fixes(conn, days, limit)?);
    out.truncate(limit);
    Ok(out)
}

/// Pack deliveries where nothing delivered was ever hit, followed by
/// file edits before the next prompt boundary — the agent worked in
/// territory the pack didn't cover.
fn uncharted_edits(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.event_id, e.payload
         FROM deliveries d
         JOIN events e ON e.id = d.event_id
         WHERE d.kind = 'pack'
           AND d.created_at > datetime('now', '-' || ? || ' days')
           AND NOT EXISTS (
               SELECT 1 FROM entity_usage u
               WHERE u.delivery_id = d.id AND u.outcome = 'hit'
           )
         ORDER BY d.id DESC
         LIMIT ?",
    )?;
    let zero_hit = stmt
        .query_map(rusqlite::params![days, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut file_stmt = conn.prepare(
        "SELECT json_extract(payload, '$.file_path')
         FROM events
         WHERE event_type = 'file_save' AND id > ?
           AND id < COALESCE(
               (SELECT MIN(id) FROM events
                WHERE event_type IN ('prompt_submit', 'session_start', 'session_end')
                  AND id > ?),
               9223372036854775807)
         LIMIT 25",
    )?;

    let mut indexable_stmt = conn.prepare("SELECT 1 FROM entities WHERE file_path = ? LIMIT 1")?;

    let mut out = Vec::new();
    for (delivery_id, event_id, payload) in zero_hit {
        let files: Vec<String> = file_stmt
            .query_map(rusqlite::params![event_id, event_id], |r| {
                r.get::<_, Option<String>>(0)
            })?
            .filter_map(|r| r.ok().flatten())
            .collect();
        // Only indexable work counts — a zero-hit pack followed by
        // edits to files the index cannot contain (docs churn, removed
        // files, generated state) is a coverage boundary, not a
        // knowledge gap worth capturing.
        let indexable: Vec<String> = files
            .iter()
            .filter(|f| {
                indexable_stmt
                    .exists(rusqlite::params![f.as_str()])
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if indexable.is_empty() {
            continue;
        }
        let prompt = serde_json::from_str::<serde_json::Value>(&payload)
            .ok()
            .and_then(|v| v["prompt"].as_str().map(String::from))
            .unwrap_or_default();
        out.push(Suggestion {
            signal: "uncharted_edit",
            evidence: serde_json::json!({
                "delivery_id": delivery_id,
                "prompt": prompt,
                "files": files,
                "indexable_files": indexable,
            }),
            suggested_title: "Pack missed the work area".to_string(),
            suggested_content: format!(
                "Prompt '{}' delivered context nothing used; the work touched {}. \
                 Consider capturing what had to be discovered manually.",
                if prompt.is_empty() {
                    "(unknown)"
                } else {
                    &prompt
                },
                indexable.join(", ")
            ),
            suggested_refs: vec![],
        });
    }
    Ok(out)
}

/// Entities hit in multiple deliveries — repeated reliance is the
/// signal that an observation may deserve promotion to a rule.
fn recurring_use(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT u.entity_id, COUNT(*) hits, e.type, COALESCE(e.title, '')
         FROM entity_usage u
         JOIN entities e ON e.id = u.entity_id
         WHERE u.outcome = 'hit'
           AND u.created_at > datetime('now', '-' || ? || ' days')
         GROUP BY u.entity_id
         HAVING hits >= ?
         ORDER BY hits DESC
         LIMIT ?",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![days, RECURRING_HIT_MIN, limit as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .map(|(id, hits, etype, title)| Suggestion {
            signal: "recurring_use",
            evidence: serde_json::json!({
                "entity_id": id,
                "entity_type": etype,
                "hits": hits,
            }),
            suggested_title: format!("Recurring reliance on {title}"),
            suggested_content: format!(
                "The {etype} '{title}' was delivered and used {hits} times recently. \
                 If it is an observation, it may have earned promotion to a rule."
            ),
            suggested_refs: vec![id],
        })
        .collect())
}

/// Files saved repeatedly — a hot spot often means missing docs,
/// missing abstractions, or a convention the agent keeps re-deriving.
fn hot_files(conn: &Connection, days: u32, limit: usize) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT json_extract(payload, '$.file_path'), COUNT(*)
         FROM events
         WHERE event_type = 'file_save'
           AND created_at > datetime('now', '-' || ? || ' days')
         GROUP BY 1
         HAVING COUNT(*) >= ?
         ORDER BY 2 DESC
         LIMIT ?",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![days, HOT_FILE_MIN, limit as i64], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .filter_map(|(path, saves)| {
            let path = path?;
            (!path.is_empty()).then(|| Suggestion {
                signal: "hot_file",
                evidence: serde_json::json!({ "file_path": path, "saves": saves }),
                suggested_title: format!("Frequently edited: {path}"),
                suggested_content: format!(
                    "{path} was saved {saves} times recently — a hot spot. \
                     Worth recording what makes it churn-prone."
                ),
                suggested_refs: vec![],
            })
        })
        .collect())
}

/// A tool error followed within a few events by a clean call on the
/// same tool — the fix sequence is the raw material of a pitfall note.
fn error_fixes(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, payload FROM events
         WHERE event_type = 'post_tool_use'
           AND created_at > datetime('now', '-' || ? || ' days')
         ORDER BY id",
    )?;
    let calls: Vec<(i64, String, String)> = stmt
        .query_map(rusqlite::params![days], |r| {
            let payload: String = r.get(1)?;
            let v: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
            Ok((
                r.get(0)?,
                v["tool_name"].as_str().unwrap_or("").to_string(),
                v["tool_result"].as_str().unwrap_or("").to_string(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for (i, (err_id, tool, result)) in calls.iter().enumerate() {
        if tool.is_empty() || !looks_like_error(result) {
            continue;
        }
        let fixed = calls
            .iter()
            .skip(i + 1)
            .take(ERROR_FIX_WINDOW as usize)
            .find(|(_, t, r)| t == tool && !looks_like_error(r));
        if let Some((fix_id, _, fix_result)) = fixed {
            out.push(Suggestion {
                signal: "error_fix",
                evidence: serde_json::json!({
                    "error_event_id": err_id,
                    "fix_event_id": fix_id,
                    "tool_name": tool,
                }),
                suggested_title: format!("{tool} error and fix"),
                suggested_content: format!(
                    "A {tool} call failed ('{}') and a later call succeeded ('{}'). \
                     The error→fix pair may contain a reusable pitfall.",
                    snippet(result, 120),
                    snippet(fix_result, 120),
                ),
                suggested_refs: vec![],
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// Heuristic error detector for tool results — substring match keeps
/// it model-free; false positives just mean a weaker candidate.
fn looks_like_error(result: &str) -> bool {
    let r = result.to_lowercase();
    ["error", "failed", "panic", "traceback", "exception"]
        .iter()
        .any(|k| r.contains(k))
}

fn snippet(s: &str, max: usize) -> String {
    let s = s.trim().replace('\n', " ");
    if s.len() <= max {
        s
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
#[path = "mining_tests.rs"]
mod tests;
