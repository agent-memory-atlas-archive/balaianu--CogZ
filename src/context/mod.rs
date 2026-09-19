//! Context assembly — builds scoped context packs from search results
//! and recent entities, with token budgeting and graph provenance.
//!
//! The context layer sits on top of search. It uses search results
//! (with graph expansion) as input, then applies mode-specific logic
//! and token budgeting to produce a `ContextPack`.

pub mod assemble;
mod baseline;
pub mod code_map;
pub mod compress;
pub mod modes;
mod query_sections;

pub use crate::storage::usage::DeliveryTier;
pub use assemble::{AssembleError, AssembleParams, assemble_context};
pub use modes::ContextMode;

/// A context pack — the primary output of CogZ for agent consumption.
#[derive(Debug, Clone)]
pub struct ContextPack {
    pub query: String,
    pub mode: ContextMode,
    pub sections: Vec<ContextSection>,
    pub metadata: PackMetadata,
}

/// A single section within a context pack.
#[derive(Debug, Clone)]
pub struct ContextSection {
    /// Entity type: "observation", "rule", "knowledge", "function", etc.
    pub source: String,
    /// Entity UUID.
    pub entity_id: String,
    /// Entity title.
    pub title: String,
    /// Entity content (possibly truncated to fit token budget).
    pub content: String,
    /// Relevance score. For task/escalation: RRF score (or decayed
    /// seed score for graph-expanded entities). For cold_start:
    /// composite score (confidence + access + recency). 0.0 for
    /// structural sections (identity, code map, knowledge index).
    pub relevance: f32,
    /// Entity IDs tracing from the seed entity to this one.
    /// For direct matches: `[entity_id]`. For expanded: the full path.
    pub graph_path: Vec<String>,
    /// Human-readable description of the graph path (e.g.
    /// "Search pipeline → hybrid.rs → fuse"). Empty for direct
    /// matches and cold-start sections.
    pub graph_path_description: String,
    /// Delivery tier: `Baseline` orientation, `Full` content, or
    /// `Pointer` (index entry — presence without depth).
    pub tier: DeliveryTier,
    /// Number of the entity's code references that diverged since the
    /// entity was last verified (`entity_drift` rows). 0 for code
    /// entities, structural sections, and fully-verified knowledge.
    pub drift_count: usize,
}

/// Metadata about a context pack's construction.
#[derive(Debug, Clone)]
pub struct PackMetadata {
    /// Estimated token count of the pack (chars/4 heuristic).
    pub size_tokens: usize,
    /// Unique source types of the included sections (e.g. "observation", "rule").
    /// Deduplicated — each type appears once regardless of how many sections
    /// of that type are included.
    pub selected_sources: Vec<String>,
    /// Entities that were dropped to fit the token budget, with reasons.
    pub dropped_sources: Vec<String>,
    /// How search was executed: "hybrid" or "fts_only".
    pub search_mode: String,
    /// Entity ids listed in the pointer index — delivered as pull
    /// affordances, not content.
    pub pointer_ids: Vec<String>,
    /// Retrieval signals from the pack's search, when the query ran
    /// in a vector-capable mode. Persisted on prompt_submit events so
    /// delivery quality is analyzable per prompt. `None` for
    /// cold-start and FTS-only packs.
    pub signals: Option<crate::search::ChannelSignals>,
}
