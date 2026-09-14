//! Config validation. Extracted from settings.rs for file-size
//! compliance — `impl Config` can live in any module of the crate.

use super::ConfigError;
use super::settings::Config;

impl Config {
    /// Validate config values. Called after loading and after
    /// generating defaults.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.project.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "project name must not be empty".to_string(),
            ));
        }
        // db_path must be relative, contained beneath .cogz/, and
        // have no parent-component traversal. This prevents the
        // configured path from opening or creating a database outside
        // the repository. Defense-in-depth canonicalization at open
        // time further guards against symlinks.
        let db_path = std::path::Path::new(&self.storage.db_path);
        if db_path.is_absolute() {
            return Err(ConfigError::Validation(
                "storage.db_path must be a relative path beneath .cogz/".to_string(),
            ));
        }
        if db_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ConfigError::Validation(
                "storage.db_path must not contain '..' parent components".to_string(),
            ));
        }
        if !self.storage.db_path.starts_with(".cogz/") {
            return Err(ConfigError::Validation(
                "storage.db_path must be rooted beneath .cogz/ (e.g. '.cogz/cogz.db')".to_string(),
            ));
        }
        if self.embedding.dimension == 0 {
            return Err(ConfigError::Validation(
                "embedding dimension must be greater than 0".to_string(),
            ));
        }
        // Cross-reference configured dimension against the model registry.
        // A mismatch means the vec0 table is created at one dimension while
        // the model produces vectors at another — KNN will fail silently.
        for (model_id, label) in [
            (&self.embedding.code_model, "code_model"),
            (&self.embedding.knowledge_model, "knowledge_model"),
        ] {
            if let Some(entry) = crate::embed::registry::lookup(model_id)
                && entry.dim != self.embedding.dimension
            {
                return Err(ConfigError::Validation(format!(
                    "embedding.dimension ({}) does not match {} registry dimension ({}) for model '{}'. \
                     Set dimension to {} or change the model.",
                    self.embedding.dimension, label, entry.dim, model_id, entry.dim
                )));
            }
        }
        if self.search.rrf_k == 0 {
            return Err(ConfigError::Validation(
                "search.rrf_k must be greater than 0".to_string(),
            ));
        }
        if self.search.max_results == 0 {
            return Err(ConfigError::Validation(
                "search.max_results must be greater than 0".to_string(),
            ));
        }
        if !matches!(
            self.search.merge_strategy.as_str(),
            "strength" | "fixed" | "detect" | "gradient" | "calibrated"
        ) {
            return Err(ConfigError::Validation(format!(
                "search.merge_strategy must be 'strength', 'fixed', 'detect', 'gradient', or 'calibrated', got '{}'",
                self.search.merge_strategy
            )));
        }
        if !(0.0..1.0).contains(&self.search.min_relevance) {
            return Err(ConfigError::Validation(
                "search.min_relevance must be in [0.0, 1.0)".to_string(),
            ));
        }
        if !(0.0..1.0).contains(&self.search.silence_threshold) {
            return Err(ConfigError::Validation(
                "search.silence_threshold must be in [0.0, 1.0)".to_string(),
            ));
        }
        if !(0.0..=100.0).contains(&self.search.fts_title_weight) {
            return Err(ConfigError::Validation(
                "search.fts_title_weight must be in [0.0, 100.0]".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.search.mmr_lambda) {
            return Err(ConfigError::Validation(
                "search.mmr_lambda must be in [0.0, 1.0]".to_string(),
            ));
        }
        if self.search.provenance_boost < 0.0 {
            return Err(ConfigError::Validation(
                "search.provenance_boost must be >= 0.0".to_string(),
            ));
        }
        if self.consolidation.dedup_threshold < 0.0 || self.consolidation.dedup_threshold > 1.0 {
            return Err(ConfigError::Validation(
                "consolidation.dedup_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.consolidation.title_match_threshold < 0.0
            || self.consolidation.title_match_threshold > 1.0
        {
            return Err(ConfigError::Validation(
                "consolidation.title_match_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.retention.observation_prune_after_days == 0 {
            return Err(ConfigError::Validation(
                "retention.observation_prune_after_days must be greater than 0".to_string(),
            ));
        }
        if self.context.default_token_budget == 0 {
            return Err(ConfigError::Validation(
                "context.default_token_budget must be greater than 0".to_string(),
            ));
        }
        if self.context.task_token_budget == 0 {
            return Err(ConfigError::Validation(
                "context.task_token_budget must be greater than 0".to_string(),
            ));
        }
        if self.context.escalation_token_budget == 0 {
            return Err(ConfigError::Validation(
                "context.escalation_token_budget must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}
