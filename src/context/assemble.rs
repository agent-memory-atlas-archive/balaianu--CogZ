//! Context pack assembly — the tiered-push pipeline.
//!
//! Tier 0 (baseline: identity + top rules) always ships for task and
//! escalation packs when `context.tiered_push` is on. Tier 1 is
//! search-driven content and ships whatever retrieval returns — an
//! empty result set naturally yields an orientation-only pack. Tier 2
//! is the overflow/pointer index itself: every dropped entity keeps a
//! discoverable entry the agent can pull.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::config::Config;
use crate::search::SearchMode;

use super::baseline::{baseline_rules, cold_start_sections};
use super::compress::{
    compress_tail, fit_budget, relax_code_sections, section_tokens, sort_by_priority,
};
use super::modes::ContextMode;
use super::query_sections::query_sections;
use super::{ContextPack, ContextSection, DeliveryTier, PackMetadata};

/// Error during context assembly.
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("search error: {0}")]
    Search(#[from] crate::search::SearchError),
    #[error("mode {0} requires a query")]
    QueryRequired(ContextMode),
}

/// Parameters for context assembly.
#[derive(Debug, Default)]
pub struct AssembleParams<'a> {
    pub mode: ContextMode,
    pub query: Option<&'a str>,
    /// Override token budget (defaults to mode-specific config value).
    pub max_tokens: Option<usize>,
    /// Pre-computed knowledge embedding for the query (None → FTS-only).
    pub knowledge_embedding: Option<&'a [f32]>,
    /// Pre-computed code embedding for the query (None → FTS-only).
    pub code_embedding: Option<&'a [f32]>,
    /// Include stale entities in the pack.
    pub include_stale: bool,
}

/// Assemble a context pack.
///
/// # Errors
/// Returns `AssembleError::QueryRequired` if mode is Task or Escalation
/// without a query. Propagates storage and search errors.
pub fn assemble_context(
    conn: &Connection,
    params: &AssembleParams,
    config: &Config,
) -> Result<ContextPack, AssembleError> {
    let token_budget = params.max_tokens.unwrap_or(match params.mode {
        ContextMode::ColdStart => config.context.default_token_budget,
        ContextMode::Task => config.context.task_token_budget,
        ContextMode::Escalation => config.context.escalation_token_budget,
    });

    // Status filter: stale entities are excluded by default.
    let search_status: Option<&str> = if params.include_stale {
        Some("all")
    } else {
        None
    };
    let cold_start_status: Option<&str> = if params.include_stale {
        None
    } else {
        Some("active")
    };

    let (mut sections, search_mode, full_content, signals) = match params.mode {
        ContextMode::ColdStart => {
            let sections = cold_start_sections(conn, config, cold_start_status)?;
            (sections, SearchMode::FtsOnly, HashMap::new(), None)
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

    // Tier 0 — baseline orientation (identity + top rules) rides every
    // task and escalation pack when tiered push is on. It is never
    // gated and never budget-fitted: the floor is structural.
    let tiered_active = config.context.tiered_push && params.mode != ContextMode::ColdStart;
    let mut tier0: Vec<ContextSection> = Vec::new();
    if tiered_active {
        tier0.push(ContextSection {
            source: "identity".to_string(),
            entity_id: "repo".to_string(),
            title: config.project.name.clone(),
            content: format!("Project: {}", config.project.name),
            relevance: 0.0,
            graph_path: vec![],
            graph_path_description: String::new(),
            tier: DeliveryTier::Baseline,
        });
        tier0.extend(baseline_rules(
            conn,
            cold_start_status,
            config.context.tier0_rules,
        )?);
    }

    let mut dropped: Vec<String> = Vec::new();
    let mut pointer_ids_out: Vec<String> = Vec::new();
    let mut overflow: Vec<ContextSection> = Vec::new();

    let mut kept: Vec<ContextSection> = {
        // Baseline first so partition_dups prefers it on identical
        // content (e.g. a rule surfaced by both orientation and search).
        let mut all = tier0;
        all.append(&mut sections);
        sort_by_priority(&mut all, params.mode);
        // A file excerpt subsumes its functions' opening lines — dedup
        // before budgeting so overlap can't spend the budget twice. Runs
        // on full excerpts: compressing first would shrink line-sets below
        // the Jaccard threshold and let the duplicates through.
        let (all_kept, dedup_losers) = super::compress::partition_dups(all);
        dropped.extend(
            dedup_losers
                .iter()
                .map(|s| format!("{}:{} (duplicate content)", s.source, s.title)),
        );
        overflow.extend(dedup_losers);

        // Baseline is structural — it ships unconditionally, but only
        // when the tiered path built one. Cold-start sections carry the
        // Baseline label for delivery tracking yet still budget-fit as
        // a single pool: the whole cold-start pack IS the orientation.
        let (baseline, mut tier1): (Vec<ContextSection>, Vec<ContextSection>) = if tiered_active {
            all_kept
                .into_iter()
                .partition(|s| s.tier == DeliveryTier::Baseline)
        } else {
            (Vec::new(), all_kept)
        };
        let baseline_tokens: usize = baseline.iter().map(section_tokens).sum();
        let tier1_budget = token_budget.saturating_sub(baseline_tokens);
        // Weak-evidence tail entities get minimal excerpts — a signature
        // is enough for context and the freed budget fits more entities.
        compress_tail(&mut tier1);
        let (mut tier1_kept, budget_dropped) = fit_budget(tier1, tier1_budget);
        dropped.extend(
            budget_dropped
                .iter()
                .map(|s| format!("{}:{} (over token budget)", s.source, s.title)),
        );
        overflow.extend(budget_dropped.iter().cloned());

        let mut kept = baseline;
        kept.append(&mut tier1_kept);
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
        kept
    };

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
        let mut pointer_ids: Vec<String> = Vec::new();
        for s in overflow.iter().take(25) {
            let line = format!("- [{}] {} — id {}", s.source, s.title, s.entity_id);
            let cost = super::compress::estimate_tokens(&line) + 1;
            if cost > avail {
                break;
            }
            avail -= cost;
            index.push(line);
            pointer_ids.push(s.entity_id.clone());
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
                tier: DeliveryTier::Pointer,
            });
        }
        pointer_ids_out.extend(pointer_ids);
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
            pointer_ids: pointer_ids_out,
            signals,
        },
    })
}
