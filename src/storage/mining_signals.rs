//! The individual mining passes behind `mining::mine_suggestions` —
//! each reads events/usage rows and returns structured candidates.

use rusqlite::Connection;

use super::StorageError;
use super::mining::Suggestion;

/// Minimum hits before an entity is flagged as recurring-reliance.
const RECURRING_HIT_MIN: i64 = 2;
/// Minimum saves before a file is flagged as a hot spot.
const HOT_FILE_MIN: i64 = 3;
/// How many later events to scan for the fixing call after an error.
const ERROR_FIX_WINDOW: i64 = 20;

/// A silenced or empty search followed by indexable file edits before
/// the next prompt boundary — the agent needed an answer the corpus
/// didn't have and found it in the code. The strongest write-back
/// moment: the gap is demonstrated, not speculative.
pub(super) fn search_misses(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, payload FROM events
         WHERE event_type = 'search_performed'
           AND created_at > datetime('now', '-' || ? || ' days')
         ORDER BY id DESC",
    )?;
    let searches: Vec<(i64, String, i64)> = stmt
        .query_map(rusqlite::params![days], |r| {
            let payload: String = r.get(1)?;
            let v: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
            Ok((
                r.get(0)?,
                v["query"].as_str().unwrap_or("").to_string(),
                v["returned"].as_i64().unwrap_or(0),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut file_stmt = files_after_stmt(conn)?;
    let mut indexable_stmt = conn.prepare("SELECT 1 FROM entities WHERE file_path = ? LIMIT 1")?;

    let mut out = Vec::new();
    for (event_id, query, returned) in searches {
        if returned > 0 || query.is_empty() {
            continue;
        }
        let files = saved_files_after(&mut file_stmt, event_id)?;
        let indexable: Vec<String> = files
            .iter()
            .filter(|f| is_indexed(&mut indexable_stmt, f))
            .cloned()
            .collect();
        if indexable.is_empty() {
            continue;
        }
        out.push(Suggestion {
            signal: "search_miss",
            evidence: serde_json::json!({
                "search_event_id": event_id,
                "query": query,
                "files": indexable,
            }),
            suggested_title: format!("Search missed: {query}"),
            suggested_content: format!(
                "Search for '{query}' returned nothing; the work instead touched {}. \
                 The discovered answer is worth capturing so the next search lands.",
                indexable.join(", ")
            ),
            suggested_refs: vec![],
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

/// Pack deliveries where nothing delivered was ever hit, followed by
/// file edits before the next prompt boundary — the agent worked in
/// territory the pack didn't cover.
pub(super) fn uncharted_edits(
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

    let mut file_stmt = files_after_stmt(conn)?;

    let mut indexable_stmt = conn.prepare("SELECT 1 FROM entities WHERE file_path = ? LIMIT 1")?;

    let mut out = Vec::new();
    for (delivery_id, event_id, payload) in zero_hit {
        let files = saved_files_after(&mut file_stmt, event_id)?;
        // Only indexable work counts — a zero-hit pack followed by
        // edits to files the index cannot contain (docs churn, removed
        // files, generated state) is a coverage boundary, not a
        // knowledge gap worth capturing.
        let indexable: Vec<String> = files
            .iter()
            .filter(|f| is_indexed(&mut indexable_stmt, f))
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
pub(super) fn recurring_use(
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
            suggested_content: if etype == "observation" {
                format!(
                    "The observation '{title}' was delivered and used {hits} times recently — \
                     it may have earned promotion to a rule."
                )
            } else {
                format!(
                    "The {etype} '{title}' was delivered and used {hits} times recently — \
                     it is load-bearing. Worth recording why if that isn't already captured."
                )
            },
            suggested_refs: vec![id],
        })
        .collect())
}

/// Files saved repeatedly — a hot spot often means missing docs,
/// missing abstractions, or a convention the agent keeps re-deriving.
pub(super) fn hot_files(
    conn: &Connection,
    days: u32,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(
             json_extract(payload, '$.file_path_rel'),
             json_extract(payload, '$.file_path')), COUNT(*)
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
pub(super) fn error_fixes(
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
                    // A retry loop mints a fresh error_event_id per
                    // attempt but is one logical struggle — anchor dedup
                    // to the tool so a test-fix grind nudges once a day.
                    "dedup": tool,
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

/// file_save events between an anchor event and the next prompt/session
/// boundary. `file_path_rel` preferred; `file_path` is the legacy fallback.
fn files_after_stmt(conn: &Connection) -> Result<rusqlite::Statement<'_>, StorageError> {
    Ok(conn.prepare(
        "SELECT COALESCE(
             json_extract(payload, '$.file_path_rel'),
             json_extract(payload, '$.file_path'))
         FROM events
         WHERE event_type = 'file_save' AND id > ?
           AND id < COALESCE(
               (SELECT MIN(id) FROM events
                WHERE event_type IN ('prompt_submit', 'session_start', 'session_end')
                  AND id > ?),
               9223372036854775807)
         LIMIT 25",
    )?)
}

fn saved_files_after(
    stmt: &mut rusqlite::Statement<'_>,
    event_id: i64,
) -> Result<Vec<String>, StorageError> {
    Ok(stmt
        .query_map(rusqlite::params![event_id, event_id], |r| {
            r.get::<_, Option<String>>(0)
        })?
        .filter_map(|r| r.ok().flatten())
        .collect())
}

/// Whether a saved path resolves to anything in the index. Code
/// entities store repo-relative paths; entity files strip their
/// `.cogz/` prefix — check both forms.
fn is_indexed(stmt: &mut rusqlite::Statement<'_>, path: &str) -> bool {
    stmt.exists(rusqlite::params![path]).unwrap_or(false)
        || path
            .strip_prefix(".cogz/")
            .is_some_and(|inner| stmt.exists(rusqlite::params![inner]).unwrap_or(false))
}

/// Heuristic error detector for tool results. Exec-style results carry
/// a definitive `Exit code: N` trailer — trust it over keywords, which
/// legitimately appear inside command output (stack traces printed by
/// passing tests, `exception` in read source files). For tools without
/// the marker, failure text leads the result — match keywords only in
/// the head so file contents containing error words stay clean.
fn looks_like_error(result: &str) -> bool {
    if let Some(pos) = result.rfind("Exit code:")
        && let Some(code) = result[pos + "Exit code:".len()..]
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<i64>().ok())
    {
        return code != 0;
    }
    let head: String = result.chars().take(200).collect::<String>().to_lowercase();
    ["error", "failed", "panic", "traceback", "exception"]
        .iter()
        .any(|k| head.contains(k))
}

pub(super) fn snippet(s: &str, max: usize) -> String {
    let s = s.trim().replace('\n', " ");
    if s.len() <= max {
        return s;
    }
    // Byte-index slicing panics on multi-byte chars (e.g. ✅ in tool
    // output) — and this runs inside hook processes, where a panic
    // kills the event's entire notice output.
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}
