//! Context pack assembly — builds a [`ContextPack`] from search
//! results or recent entities, depending on the mode.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::config::{Config, SearchConfig};
use crate::search::{self, QueryEmbeddings, SearchMode, SearchParams, SearchResult};
use crate::storage::crud::Entity;
use crate::storage::query::get_entities_by_type;

use super::code_map::code_map_sections;
use super::compress::{
    compress_tail, fit_budget, relax_code_sections, section_tokens, sort_by_priority,
};
use super::modes::ContextMode;
use super::{ContextPack, ContextSection, PackMetadata};

/// Error during context assembly.
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("search error: {0}")]
    Search(#[from] search::SearchError),
    #[error("storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("query required for {0} mode")]
    QueryRequired(ContextMode),
}

/// Parameters for assembling a context pack.
pub struct AssembleParams<'a> {
    /// The mode determines retrieval strategy and priorities.
    pub mode: ContextMode,
    /// The query for task/escalation modes. Ignored for cold_start.
    pub query: Option<&'a str>,
    /// Optional knowledge-model query embedding for hybrid search.
    pub knowledge_embedding: Option<&'a [f32]>,
    /// Optional code-model query embedding for code-aware search.
    pub code_embedding: Option<&'a [f32]>,
    /// Override the token budget from config. If None, use config default.
    pub max_tokens: Option<usize>,
    /// Include stale entities in results.
    pub include_stale: bool,
}

impl Default for AssembleParams<'_> {
    fn default() -> Self {
        Self {
            mode: ContextMode::Task,
            query: None,
            knowledge_embedding: None,
            code_embedding: None,
            max_tokens: None,
            include_stale: false,
        }
    }
}

/// Assemble a context pack.
pub fn assemble_context(
    conn: &Connection,
    params: &AssembleParams<'_>,
    config: &Config,
) -> Result<ContextPack, AssembleError> {
    let token_budget = params.max_tokens.unwrap_or(match params.mode {
        ContextMode::ColdStart => config.context.default_token_budget,
        ContextMode::Task => config.context.task_token_budget,
        ContextMode::Escalation => config.context.escalation_token_budget,
    });
    // For cold_start: None means no filter (all statuses), Some("active") filters to active.
    // For search: the resolve_status_filter function maps None → active, "all" → no filter.
    let cold_start_status = if params.include_stale {
        None
    } else {
        Some("active")
    };
    let search_status = if params.include_stale {
        Some("all")
    } else {
        None
    };

    let (sections, search_mode, full_content) = match params.mode {
        ContextMode::ColdStart => {
            let sections = cold_start_sections(conn, config, cold_start_status)?;
            (sections, SearchMode::FtsOnly, HashMap::new())
        }
        ContextMode::Task | ContextMode::Escalation => {
            let query = params
                .query
                .ok_or(AssembleError::QueryRequired(params.mode))?;
            let (max_results, max_hops) = if params.mode == ContextMode::Escalation {
                (
                    config.context.escalation_max_results,
                    config.context.escalation_max_hops,
                )
            } else {
                (
                    config.context.task_max_results,
                    config.context.task_max_hops,
                )
            };
            query_sections(
                conn,
                query,
                params.knowledge_embedding,
                params.code_embedding,
                max_results,
                max_hops,
                search_status,
                &config.search,
            )?
        }
    };

    let mut sections = sections;
    sort_by_priority(&mut sections, params.mode);
    // A file excerpt subsumes its functions' opening lines — dedup
    // before budgeting so overlap can't spend the budget twice. Runs
    // on full excerpts: compressing first would shrink line-sets below
    // the Jaccard threshold and let the duplicates through.
    let (mut sections, dedup_losers) = super::compress::partition_dups(sections);
    let mut dropped: Vec<String> = dedup_losers
        .iter()
        .map(|s| format!("{}:{} (duplicate content)", s.source, s.title))
        .collect();
    // Weak-evidence tail entities get minimal excerpts — a signature
    // is enough for context and the freed budget fits more entities.
    compress_tail(&mut sections);
    let (mut kept, budget_dropped) = fit_budget(sections, token_budget);
    dropped.extend(
        budget_dropped
            .iter()
            .map(|s| format!("{}:{} (over token budget)", s.source, s.title)),
    );

    // Every dropped entity keeps a discoverable pointer in the
    // overflow index — the agent can pull one by title/id instead of
    // never learning it exists. Covers dedup losers, budget drops,
    // and regrown dupes whose minimal excerpt still duplicates.
    let mut overflow: Vec<ContextSection> = dedup_losers;
    overflow.extend(budget_dropped.iter().cloned());

    if !full_content.is_empty() {
        relax_code_sections(&mut kept, &full_content, token_budget);
        // Regrown excerpts can recreate the overlap dedup removed — a
        // file relaxed to 30 lines again covers its functions. Demote
        // the regrown dupe back to a minimal excerpt rather than
        // dropping it: the content is delivered via the container, and
        // the entity stays present as a pointer. If even the minimal
        // excerpt still duplicates, the entity drops to the index.
        let (deduped, regrowth_dupes) = super::compress::partition_dups(kept);
        kept = deduped;
        for s in regrowth_dupes {
            dropped.push(format!("{}:{} (duplicate content)", s.source, s.title));
            let mut demoted = s;
            demoted.content = super::compress::excerpt_lines(&demoted.content, 4);
            if super::compress::is_dup_of(&demoted, &kept) {
                overflow.push(demoted);
            } else {
                kept.push(demoted);
            }
        }
    }

    // The index itself must fit inside whatever budget remains.
    if !overflow.is_empty() {
        let title = "Also relevant";
        let mut avail = token_budget.saturating_sub(kept.iter().map(section_tokens).sum::<usize>());
        avail = avail.saturating_sub(super::compress::estimate_tokens(title));
        overflow.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut index: Vec<String> = Vec::new();
        for s in overflow.iter().take(25) {
            let line = format!("- [{}] {} — id {}", s.source, s.title, s.entity_id);
            let cost = super::compress::estimate_tokens(&line) + 1;
            if cost > avail {
                break;
            }
            avail -= cost;
            index.push(line);
        }
        if !index.is_empty() {
            kept.push(ContextSection {
                source: "overflow_index".to_string(),
                entity_id: "index".to_string(),
                title: title.to_string(),
                content: index.join("\n"),
                relevance: 0.0,
                graph_path: vec![],
                graph_path_description: String::new(),
            });
        }
    }

    let size_tokens = kept.iter().map(section_tokens).sum();
    // selected_sources lists entity types delivered — the synthetic
    // overflow index is navigation, not an entity type.
    let selected_sources: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for s in &kept {
            if s.source != "overflow_index" && !seen.contains(&s.source) {
                seen.push(s.source.clone());
            }
        }
        seen
    };

    Ok(ContextPack {
        query: params.query.unwrap_or("").to_string(),
        mode: params.mode,
        sections: kept,
        metadata: PackMetadata {
            size_tokens,
            selected_sources,
            dropped_sources: dropped,
            search_mode: search_mode.as_str().to_string(),
        },
    })
}

/// Build sections for cold_start mode: repo identity, code map,
/// scored rules, and knowledge index.
///
/// Uses composite scoring (confidence + access frequency + recency
/// decay) instead of pure recency to select the most relevant rules
/// and knowledge. Includes a bounded code map summary so the agent
/// knows the project structure without flooding the token budget.
fn cold_start_sections(
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
    });

    // 2. Code map summary — top modules and files by connectivity.
    sections.extend(code_map_sections(conn, status));

    // 3. Scored rules — selected by composite score, not just recency.
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

    for (score, entity) in scored_rules.iter().take(config.context.cold_start_rules) {
        let id = entity.id.clone();
        sections.push(ContextSection {
            source: "rule".to_string(),
            entity_id: id.clone(),
            title: entity.title.clone().unwrap_or_default(),
            content: entity.content.clone(),
            relevance: *score as f32,
            graph_path: vec![id],
            graph_path_description: String::new(),
        });
    }

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

/// Sections plus the full (unsummarized) content of code entities,
/// kept for the headroom relax pass.
type SectionBundle = (Vec<ContextSection>, SearchMode, HashMap<String, String>);

/// Build sections from search results (task and escalation modes).
#[allow(clippy::too_many_arguments)]
fn query_sections(
    conn: &Connection,
    query: &str,
    knowledge_embedding: Option<&[f32]>,
    code_embedding: Option<&[f32]>,
    max_results: u32,
    max_hops: usize,
    status: Option<&str>,
    search_config: &SearchConfig,
) -> Result<SectionBundle, AssembleError> {
    let params = SearchParams {
        entity_type: None,
        status: status.map(|s| s.to_string()),
        limit: max_results,
        expand: max_hops > 0,
        max_hops,
        include_tests: false,
    };

    let embeddings = QueryEmbeddings {
        knowledge: knowledge_embedding,
        code: code_embedding,
    };
    let results = search::search(conn, query, embeddings, &params, search_config)?;
    let search_mode = results.search_mode;

    let mut full_content = HashMap::new();
    let sections = results
        .results
        .into_iter()
        .map(|r| {
            if is_code_entity(&r.entity.r#type) {
                full_content.insert(r.entity.id.clone(), r.entity.content.clone());
            }
            search_result_to_section(r)
        })
        .collect();

    Ok((sections, search_mode, full_content))
}

/// Convert a search result to a context section.
/// Code entities (function, class, file, module) are summarized to
/// avoid flooding the token budget with full source code. Knowledge
/// entities (observation, rule, knowledge) are included in full.
fn search_result_to_section(result: SearchResult) -> ContextSection {
    let content = if is_code_entity(&result.entity.r#type) {
        summarize_code_content(&result.entity.content, &result.entity.r#type)
    } else {
        result.entity.content
    };
    ContextSection {
        source: result.entity.r#type,
        entity_id: result.entity.id,
        title: result.entity.title.unwrap_or_default(),
        content,
        relevance: result.relevance,
        graph_path: result.graph_path,
        graph_path_description: result.graph_path_description,
    }
}

/// Check if an entity type is a code entity (extracted from source).
fn is_code_entity(entity_type: &str) -> bool {
    matches!(entity_type, "function" | "class" | "file" | "module")
}

/// Summarize code entity content to keep context packs compact.
/// Modules get 1 line (just the declaration), files get 15 lines
/// (header + first items), functions and classes get 10 lines
/// (signature + first lines of body). An ellipsis is appended if
/// the content was truncated.
fn summarize_code_content(content: &str, entity_type: &str) -> String {
    let max_lines = match entity_type {
        "module" => 1,
        "file" => 15,
        _ => 10,
    };
    super::compress::excerpt_lines(content, max_lines)
}
