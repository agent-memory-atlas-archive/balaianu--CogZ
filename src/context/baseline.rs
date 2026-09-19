//! Baseline sections — the structural Tier-0 layer of a context pack.
//!
//! Identity, code map, scored rules, and the knowledge index ship in
//! every cold-start pack; the identity + rules subset rides task and
//! escalation packs as orientation. Baseline sections are never gated
//! or budget-fitted when they serve as Tier 0 — the floor is
//! structural so delivery policies can never shrink it away.

use rusqlite::Connection;

use crate::config::Config;
use crate::storage::crud::Entity;
use crate::storage::query::get_entities_by_type;

use super::assemble::AssembleError;
use super::code_map::code_map_sections;
use super::{ContextSection, DeliveryTier};

/// Build sections for cold_start mode: repo identity, code map,
/// scored rules, and knowledge index.
///
/// Uses composite scoring (confidence + access frequency + recency
/// decay) instead of pure recency to select the most relevant rules
/// and knowledge. Includes a bounded code map summary so the agent
/// knows the project structure without flooding the token budget.
pub(super) fn cold_start_sections(
    conn: &Connection,
    config: &Config,
    status: Option<&str>,
) -> Result<Vec<ContextSection>, AssembleError> {
    let now = chrono::Utc::now();
    let weights = crate::search::scoring::ScoreWeights::default();

    let mut sections = Vec::new();

    // 1. Repo identity
    sections.push(ContextSection {
        source: "identity".to_string(),
        entity_id: "repo".to_string(),
        title: config.project.name.clone(),
        content: format!("Project: {}", config.project.name),
        relevance: 0.0,
        graph_path: vec![],
        graph_path_description: String::new(),
        tier: DeliveryTier::Baseline,
        drift_count: 0,
    });

    // 2. Code map summary — top modules and files by connectivity.
    sections.extend(code_map_sections(conn, status));

    // 3. Scored rules — selected by composite score, not just recency.
    sections.extend(baseline_rules(
        conn,
        status,
        config.context.cold_start_rules,
    )?);

    // 4. Knowledge index — titles only for most entries, full content
    // for the top scored entry.
    let knowledge = get_entities_by_type(conn, "knowledge", status, 1000)?;
    let knowledge_ids: Vec<String> = knowledge.iter().map(|e| e.id.clone()).collect();
    let knowledge_access = crate::storage::access::get_access_counts_batch(conn, &knowledge_ids)?;

    let mut scored_knowledge: Vec<(f64, &Entity)> = knowledge
        .iter()
        .map(|e| {
            let count = knowledge_access.get(&e.id).copied().unwrap_or(0);
            let score = crate::search::scoring::cold_start_score(e, count, &now, &weights);
            (score, e)
        })
        .collect();
    scored_knowledge.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Full content for top entry, titles only for the rest.
    if let Some((score, top)) = scored_knowledge.first() {
        let id = top.id.clone();
        sections.push(ContextSection {
            source: "knowledge".to_string(),
            entity_id: id.clone(),
            title: top.title.clone().unwrap_or_default(),
            content: top.content.clone(),
            relevance: *score as f32,
            graph_path: vec![id],
            graph_path_description: String::new(),
            tier: DeliveryTier::Baseline,
            drift_count: 0,
        });
    }

    // Title-only index for remaining knowledge entries.
    let remaining: Vec<String> = scored_knowledge
        .iter()
        .skip(1)
        .map(|(_, e)| format!("- {}", e.title.as_deref().unwrap_or("(untitled)")))
        .collect();
    if !remaining.is_empty() {
        sections.push(ContextSection {
            source: "knowledge_index".to_string(),
            entity_id: "index".to_string(),
            title: "Knowledge Index".to_string(),
            content: remaining.join("\n"),
            relevance: 0.0,
            graph_path: vec![],
            graph_path_description: String::new(),
            tier: DeliveryTier::Baseline,
            drift_count: 0,
        });
    }

    // Increment access counts for included rules and knowledge so the
    // frequency component of composite scoring reflects cold-start
    // usage, not just search usage.
    let accessed: Vec<String> = sections
        .iter()
        .filter(|s| s.source == "rule" || s.source == "knowledge")
        .map(|s| s.entity_id.clone())
        .collect();
    if !accessed.is_empty()
        && let Err(e) = crate::storage::access::increment_access_batch(conn, &accessed)
    {
        tracing::warn!("failed to increment access counts: {e}");
    }

    Ok(sections)
}

/// Top-scored rules as baseline sections — the composite score
/// (confidence + access frequency + recency decay) picks the most
/// durable conventions. Shared by cold_start's rule block and the
/// Tier-0 orientation in task/escalation packs.
pub(super) fn baseline_rules(
    conn: &Connection,
    status: Option<&str>,
    limit: usize,
) -> Result<Vec<ContextSection>, AssembleError> {
    let now = chrono::Utc::now();
    let weights = crate::search::scoring::ScoreWeights::default();
    let rules = get_entities_by_type(conn, "rule", status, 1000)?;
    let rule_ids: Vec<String> = rules.iter().map(|e| e.id.clone()).collect();
    let access_counts = crate::storage::access::get_access_counts_batch(conn, &rule_ids)?;

    let mut scored_rules: Vec<(f64, &Entity)> = rules
        .iter()
        .map(|e| {
            let count = access_counts.get(&e.id).copied().unwrap_or(0);
            let score = crate::search::scoring::cold_start_score(e, count, &now, &weights);
            (score, e)
        })
        .collect();
    scored_rules.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut out = Vec::new();
    for (score, entity) in scored_rules.iter().take(limit) {
        let id = entity.id.clone();
        out.push(ContextSection {
            source: "rule".to_string(),
            entity_id: id.clone(),
            title: entity.title.clone().unwrap_or_default(),
            content: entity.content.clone(),
            relevance: *score as f32,
            graph_path: vec![id],
            graph_path_description: String::new(),
            tier: DeliveryTier::Baseline,
            drift_count: 0,
        });
    }
    Ok(out)
}
