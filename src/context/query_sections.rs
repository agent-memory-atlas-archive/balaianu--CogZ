//! Search-driven sections — Tier-1 content for task and escalation
//! packs. Runs hybrid search, converts results to sections, and keeps
//! the full (unsummarized) code content for the relax pass.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::config::SearchConfig;
use crate::search::{
    self, ChannelSignals, QueryEmbeddings, SearchMode, SearchParams, SearchResult,
};

use super::assemble::AssembleError;
use super::{ContextSection, DeliveryTier};

/// Sections plus the full (unsummarized) content of code entities,
/// kept for the headroom relax pass, plus the retrieval signals the
/// Tier-1 gate reads.
pub(super) type SectionBundle = (
    Vec<ContextSection>,
    SearchMode,
    HashMap<String, String>,
    Option<ChannelSignals>,
);

/// Build sections from search results (task and escalation modes).
#[allow(clippy::too_many_arguments)]
pub(super) fn query_sections(
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
        // Packs ship whatever survives the relevance floor — the
        // silence gate is an agent-facing answer semantic, not a
        // delivery policy. Its batch-level predicate can't separate
        // "task phrased differently than identifiers" from "no match"
        // on code corpora.
        silence_gate: false,
    };

    let embeddings = QueryEmbeddings {
        knowledge: knowledge_embedding,
        code: code_embedding,
    };
    let results = search::search(conn, query, embeddings, &params, search_config)?;
    let search_mode = results.search_mode;
    let signals = results.signals;

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

    Ok((sections, search_mode, full_content, signals))
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
        tier: DeliveryTier::Full,
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
