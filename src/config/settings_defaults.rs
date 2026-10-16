//! Default `Config` construction.

use super::*;

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
                auto_download: default_true(),
                force_fts_only: false,
                code_embeddings: true,
                model_idle_ttl: default_model_idle_ttl(),
                model_min_free_mb: default_model_min_free_mb(),
            },
            search: SearchConfig {
                fts_weight: 0.3,
                vec_weight: 0.4,
                code_vec_weight: default_code_vec_weight(),
                rrf_k: 60,
                max_results: 20,
                min_source_proportion: default_min_source_proportion(),
                source_balance_enabled: default_source_balance_enabled(),
                merge_strategy: default_merge_strategy(),
                min_relevance: default_min_relevance(),
                edge_weighted_expansion: default_true(),
                top_diversity_share: default_top_diversity_share(),
                calibration: CalibrationConfig::default(),
                provenance_boost: default_provenance_boost(),
                drift_penalty: default_drift_penalty(),
                fts_title_weight: default_fts_title_weight(),
                mmr_lambda: default_mmr_lambda(),
                graph_first_enabled: default_true(),
                graph_max_seeds: default_graph_max_seeds(),
                graph_max_hops: default_graph_max_hops(),
                graph_hop_decay: default_graph_hop_decay(),
                graph_weight: default_graph_weight(),
                silence_coverage_floor: default_silence_coverage_floor(),
                prf_enabled: default_true(),
                prf_feedback_docs: default_prf_feedback_docs(),
                prf_max_terms: default_prf_max_terms(),
                sibling_expand_enabled: default_true(),
                sibling_max_anchors: default_sibling_max_anchors(),
                cochange_enabled: default_true(),
                graph_seed_min_sim: default_graph_seed_min_sim(),
            },
            consolidation: ConsolidationConfig {
                dedup_threshold: 0.85,
                title_match_threshold: 0.85,
                contradiction_check: true,
                promotion_threshold: 3,
                contradiction_threshold: default_contradiction_threshold(),
                contradiction_cosine_threshold: default_contradiction_cosine_threshold(),
                contradiction_length_ratio: default_contradiction_length_ratio(),
                dedup_nli_threshold: default_dedup_nli_threshold(),
            },
            index: IndexConfig::default(),
            retention: RetentionConfig {
                observation_prune_after_days: 90,
                tombstone_max_count: 1000,
            },
            context: ContextConfig::default(),
        }
    }
}
