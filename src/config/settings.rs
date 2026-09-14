//! Typed config structs matching `.cogz/config.toml`.
//!
//! Schema defined in `docs/configuration.md`.

use serde::{Deserialize, Serialize};

use super::calibration::CalibrationConfig;

/// Top-level config. Loaded from `.cogz/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    pub storage: StorageConfig,
    pub embedding: EmbeddingConfig,
    pub search: SearchConfig,
    pub consolidation: ConsolidationConfig,
    #[serde(default)]
    pub index: IndexConfig,
    pub retention: RetentionConfig,
    #[serde(default)]
    pub context: ContextConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Autodetected on init, saved, versioned. Stable identifier that
    /// survives folder renames. Metadata only — not a query filter.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Per-repo database path, relative to repo root.
    pub db_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub code_model: String,
    pub knowledge_model: String,
    pub dimension: usize,
    /// NLI model ID for contradiction detection (Phase 9).
    /// Empty string = use default (`nli-deberta-v3-xsmall`).
    #[serde(default)]
    pub nli_model: String,
    /// Auto-download models from HuggingFace on first use (default: true).
    #[serde(default = "default_true")]
    pub auto_download: bool,
    /// Seconds of idle time before unloading ONNX models from memory.
    /// 0 = never unload (keep resident for process lifetime).
    /// On a 7GB RAM system, unloading idle models frees ~300-500MB.
    /// Default: 300 (5 minutes).
    #[serde(default = "default_model_idle_ttl")]
    pub model_idle_ttl: u64,
    /// Minimum free memory (MB) required to load a model. If available
    /// RAM drops below this, model loading fails gracefully and the
    /// system degrades to FTS-only. 0 = no check.
    /// Default: 512.
    #[serde(default = "default_model_min_free_mb")]
    pub model_min_free_mb: u64,
}

fn default_true() -> bool {
    true
}

fn default_model_idle_ttl() -> u64 {
    300
}

fn default_model_min_free_mb() -> u64 {
    512
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchConfig {
    pub fts_weight: f64,
    pub vec_weight: f64,
    /// Weight for code vector search results in RRF fusion. Used when
    /// a code query embedding is available. Defaults to 0.3.
    #[serde(default = "default_code_vec_weight")]
    pub code_vec_weight: f64,
    pub rrf_k: u32,
    pub max_results: u32,
    /// Floor for each source type's proportion in balanced fusion.
    /// Ensures neither code nor knowledge is completely suppressed
    /// even when the query strongly favors one. 0.2 = each source
    /// gets at least 20% of the RRF weight. Defaults to 0.2.
    #[serde(default = "default_min_source_proportion")]
    pub min_source_proportion: f64,
    /// Enable query-sensitive source balancing. When true, the hybrid
    /// search detects code/knowledge proportions from KNN distance
    /// spread and FTS pool sizes. When false (default), uses a fixed
    /// 0.5/0.5 split — the evaluation showed this outperforms all
    /// balance detection variants on overall retrieval quality.
    /// The balance detection code is kept for future experimentation
    /// with better signals (e.g. trained classifiers, NLI-based
    /// intent detection).
    #[serde(default = "default_source_balance_enabled")]
    pub source_balance_enabled: bool,
    /// How code and knowledge channel scores are merged.
    /// "strength": scale each channel's normalized list by absolute
    /// KNN cosine strength — NOTE: absolute cosines are not comparable
    /// across different embedding models (bge runs ~0.7, CodeRankEmbed
    /// ~0.45 for real matches), so this systematically under-weights
    /// the code channel. "gradient": within-batch distinctiveness
    /// (model-agnostic) as an unbounded weight. "fixed": legacy
    /// 0.5/0.5 split. "detect": proportion detection normalized to
    /// shares (equivalent to `source_balance_enabled = true`).
    /// "calibrated": detect shares × per-channel logistic-calibrated
    /// strength — proportion decides the split, calibrated presence
    /// decides whether a channel surfaces at all.
    #[serde(default = "default_merge_strategy")]
    pub merge_strategy: String,
    /// Minimum relevance for a result to be returned; 0.0 disables.
    /// Filters tail noise and weak graph expansions. Defaults to 0.05 —
    /// any value ≥ 0.09 makes all two-hop expansions unreachable
    /// (max expanded score is seed × 0.3² = 0.09).
    #[serde(default = "default_min_relevance")]
    pub min_relevance: f64,
    /// Weight graph-expansion scores by edge type: curated semantic
    /// edges (references, supports, contradicts, ...) outrank mass
    /// structural fan-out (imports, contains) in the expansion cap.
    /// Defaults to true; set false for the legacy flat-decay behavior.
    #[serde(default = "default_true")]
    pub edge_weighted_expansion: bool,
    /// Silence gate: when neither channel's KNN batch shows a
    /// distinctive match (within-batch gradient below this on both),
    /// search returns empty rather than a confident-looking list of
    /// irrelevant entities. Model-agnostic (relative spread, not
    /// absolute cosine). 0.0 disables. Skipped in FTS-only mode —
    /// with no embeddings there is no signal to judge by.
    /// Calibrated on the CogZ self-corpus: negative queries measured
    /// ≤0.015 max gradient, real queries ≥0.024 → 0.02 sits in the
    /// gap. Thin margin; recalibrate via `signals` in the response
    /// when changing embedding models.
    #[serde(default = "default_silence_threshold")]
    pub silence_threshold: f64,
    /// Minority-channel slot guarantee: when a channel's share of the
    /// merge weight is at least this value, its best result is
    /// promoted into the top-5 window if ranking pushed it out.
    /// Recovers mixed-intent coverage that channel concentration
    /// loses. 0.0 disables.
    #[serde(default = "default_top_diversity_share")]
    pub top_diversity_share: f64,
    /// Per-channel logistic calibration for `merge_strategy =
    /// "calibrated"`: maps each channel's absolute top-3 cosine onto
    /// a shared [0,1] "probability a distinctive match exists" scale.
    /// Constants are model-pair-specific — measured on the CogZ
    /// self-corpus for CodeRankEmbed-int8 + bge-base-en-v1.5.
    #[serde(default)]
    pub calibration: CalibrationConfig,
    /// Provenance prior: multiply each direct result's score by
    /// `1 + boost × ln(1 + curated_in_degree)` — entities that
    /// knowledge/rules deliberately linked earn a bump over unlinked
    /// neighbors. 0.0 disables. Default 0.3 — the measured optimum
    /// on the CogZ self-corpus (0.15 under-boosts, 0.5 over-boosts).
    #[serde(default = "default_provenance_boost")]
    pub provenance_boost: f64,
    /// FTS5 column weight for `title` vs `content` in BM25. 1.0 is
    /// the library default (uniform); values > 1.0 make exact-name
    /// and title hits rank above body-term matches. Default 5.0 —
    /// measured optimum (3.0 showed no measurable effect).
    #[serde(default = "default_fts_title_weight")]
    pub fts_title_weight: f64,
    /// MMR diversity: rerank the merged direct list by
    /// `λ·relevance − (1−λ)·max_cosine_to_selected` within each
    /// channel. Deduplicates near-identical entities out of the top
    /// slots. 0.0 disables (pure relevance order). Default 0.7 —
    /// measured on the CogZ self-corpus (0.5 ≈ 0.7, marginally worse).
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
    /// Cross-encoder rerank: rescore the top `rerank_depth` direct
    /// results with a joint (query, candidate) model after merge,
    /// before graph expansion. Unlike per-channel cosine scores,
    /// cross-encoder scores are comparable across channels — the
    /// rerank dissolves merge-proportion mismatches inside the
    /// reranked window. Requires the model on disk; absent model →
    /// stage skipped (never required). Default false: measured
    /// neutral on the v2 self-corpus (+0.003 MRR, +9% latency) —
    /// opt-in until a reranker earns the cost.
    #[serde(default)]
    pub rerank_enabled: bool,
    /// How many top direct results the cross-encoder rescores.
    /// Deeper reranking costs one batched inference per query
    /// (~30-60ms at depth 20 for TinyBERT on CPU).
    #[serde(default = "default_rerank_depth")]
    pub rerank_depth: usize,
    /// How many top fused positions stay pinned during rerank.
    /// Passage-domain cross-encoders systematically prefer prose
    /// over code entities; the anchor bounds worst-case damage to
    /// the contested tail while preserving deep-rank lifts.
    #[serde(default = "default_rerank_anchor")]
    pub rerank_anchor: usize,
    /// Whether the cross-encoder may reorder code entities
    /// (function/class/file/module). Default false: code entities
    /// hold their fused slots because passage-domain rerankers have
    /// no meaningful signal for source code. Set true only with a
    /// reranker trained on code retrieval.
    #[serde(default)]
    pub rerank_code: bool,
    /// HuggingFace model ID for the cross-encoder reranker. Must be
    /// an ONNX-exported cross-encoder under the models dir.
    #[serde(default = "default_reranker_model")]
    pub reranker_model: String,
}

fn default_code_vec_weight() -> f64 {
    0.3
}

fn default_min_source_proportion() -> f64 {
    0.2
}

fn default_source_balance_enabled() -> bool {
    false
}

fn default_merge_strategy() -> String {
    // Benchmark-validated (32 judged queries, self-corpus): detect
    // ordering beat fixed on MRR (+27%) and graph recall, and beat
    // strength everywhere — absolute cosines are not comparable
    // across the two embedding models.
    "detect".to_string()
}

fn default_min_relevance() -> f64 {
    0.05
}

fn default_silence_threshold() -> f64 {
    0.02
}

fn default_top_diversity_share() -> f64 {
    0.3
}

fn default_provenance_boost() -> f64 {
    0.3
}

fn default_fts_title_weight() -> f64 {
    5.0
}

fn default_mmr_lambda() -> f64 {
    0.7
}

fn default_rerank_depth() -> usize {
    20
}

fn default_rerank_anchor() -> usize {
    3
}

fn default_reranker_model() -> String {
    crate::embed::registry::DEFAULT_RERANKER_MODEL.to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    pub dedup_threshold: f64,
    pub title_match_threshold: f64,
    pub contradiction_check: bool,
    pub promotion_threshold: u32,
    /// Minimum P(contradiction) to flag a pair as contradicting.
    /// Calibrated against XNLI dev set in CogZ-py. Below this, the
    /// pair may be related-but-not-contradictory.
    #[serde(default = "default_contradiction_threshold")]
    pub contradiction_threshold: f64,
    /// Minimum embedding cosine similarity for a contradiction pair.
    /// Genuine contradictions share the same topic with opposing
    /// claims, so their embeddings should be very similar.
    #[serde(default = "default_contradiction_cosine_threshold")]
    pub contradiction_cosine_threshold: f64,
    /// Maximum text length ratio for a contradiction pair. Texts
    /// differing by more than this ratio are likely different content
    /// types, not a genuine contradiction.
    #[serde(default = "default_contradiction_length_ratio")]
    pub contradiction_length_ratio: f64,
    /// Minimum bidirectional P(entailment) to confirm a duplicate pair.
    /// Both A entails B AND B entails A must score above this. True
    /// duplicates entail mutually; a subset-fact does not.
    #[serde(default = "default_dedup_nli_threshold")]
    pub dedup_nli_threshold: f64,
}

fn default_contradiction_threshold() -> f64 {
    0.70
}
fn default_contradiction_cosine_threshold() -> f64 {
    0.85
}
fn default_contradiction_length_ratio() -> f64 {
    5.0
}
fn default_dedup_nli_threshold() -> f64 {
    0.85
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Glob patterns for files to index despite being gitignored.
    /// Patterns are relative to the repo root and use standard glob
    /// syntax (`*`, `**`, `?`, `[abc]`). Allow overrides both gitignore
    /// and deny.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Glob patterns for files to exclude from indexing even if they
    /// are not gitignored. Patterns are relative to the repo root and
    /// use the same glob syntax as `allow`. Use cases: vendored code,
    /// generated files not covered by .gitignore, benchmark files.
    /// Allow patterns take precedence over deny.
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionConfig {
    pub observation_prune_after_days: u32,
    pub tombstone_max_count: u32,
}

/// Context assembly configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Default token budget for cold_start context packs.
    pub default_token_budget: usize,
    /// Token budget for task context packs. Larger than cold_start
    /// to accommodate code entities alongside knowledge entries.
    #[serde(default = "default_task_token_budget")]
    pub task_token_budget: usize,
    /// Token budget for escalation context packs. Same as task by
    /// default — escalation widens search depth, not just budget.
    #[serde(default = "default_escalation_token_budget")]
    pub escalation_token_budget: usize,
    /// Number of recent rules to include in cold_start mode.
    pub cold_start_rules: usize,
    /// Max search results in task mode before expansion.
    pub task_max_results: u32,
    /// Graph expansion hops in task mode.
    pub task_max_hops: usize,
    /// Max search results in escalation mode before expansion.
    pub escalation_max_results: u32,
    /// Graph expansion hops in escalation mode.
    pub escalation_max_hops: usize,
}

fn default_task_token_budget() -> usize {
    8192
}

fn default_escalation_token_budget() -> usize {
    8192
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            default_token_budget: 4096,
            task_token_budget: 8192,
            escalation_token_budget: 8192,
            cold_start_rules: 5,
            task_max_results: 25,
            task_max_hops: 2,
            escalation_max_results: 20,
            escalation_max_hops: 3,
        }
    }
}

impl Config {
    /// Create a default config for a given project name.
    pub fn default_for(project_name: &str) -> Self {
        Self {
            project: ProjectConfig {
                name: project_name.to_string(),
            },
            storage: StorageConfig {
                db_path: ".cogz/cogz.db".to_string(),
            },
            embedding: EmbeddingConfig {
                code_model: crate::embed::registry::DEFAULT_CODE_MODEL.to_string(),
                knowledge_model: crate::embed::registry::DEFAULT_KNOWLEDGE_MODEL.to_string(),
                dimension: crate::embed::registry::DEFAULT_DIMENSION,
                nli_model: crate::embed::registry::DEFAULT_NLI_MODEL.to_string(),
                auto_download: true,
                model_idle_ttl: 300,
                model_min_free_mb: 512,
            },
            search: SearchConfig {
                fts_weight: 0.3,
                vec_weight: 0.4,
                code_vec_weight: 0.3,
                rrf_k: 60,
                max_results: 20,
                min_source_proportion: 0.2,
                source_balance_enabled: false,
                merge_strategy: default_merge_strategy(),
                min_relevance: default_min_relevance(),
                edge_weighted_expansion: true,
                silence_threshold: 0.02,
                top_diversity_share: 0.3,
                calibration: CalibrationConfig::default(),
                provenance_boost: default_provenance_boost(),
                fts_title_weight: default_fts_title_weight(),
                mmr_lambda: default_mmr_lambda(),
                rerank_enabled: false,
                rerank_depth: default_rerank_depth(),
                rerank_anchor: default_rerank_anchor(),
                rerank_code: false,
                reranker_model: default_reranker_model(),
            },
            consolidation: ConsolidationConfig {
                dedup_threshold: 0.85,
                title_match_threshold: 0.85,
                contradiction_check: true,
                promotion_threshold: 3,
                contradiction_threshold: 0.70,
                contradiction_cosine_threshold: 0.85,
                contradiction_length_ratio: 5.0,
                dedup_nli_threshold: 0.85,
            },
            index: IndexConfig {
                allow: vec![],
                deny: vec![],
            },
            retention: RetentionConfig {
                observation_prune_after_days: 90,
                tombstone_max_count: 1000,
            },
            context: ContextConfig::default(),
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
