//! Config validation. Extracted from settings.rs for file-size
//! compliance — `impl Config` can live in any module of the crate.

use super::ConfigError;
use super::settings::Config;

fn invalid(field: &'static str, expected: &'static str, got: impl std::fmt::Debug) -> ConfigError {
    ConfigError::InvalidValue {
        field,
        expected,
        got: format!("{got:?}"),
    }
}

impl Config {
    /// Validate config values. Called after loading and after
    /// generating defaults.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.project.name.trim().is_empty() {
            return Err(invalid(
                "project.name",
                "a non-empty name",
                &self.project.name,
            ));
        }
        // db_path must be relative, contained beneath .cogz/, and
        // have no parent-component traversal. This prevents the
        // configured path from opening or creating a database outside
        // the repository. Defense-in-depth canonicalization at open
        // time further guards against symlinks.
        let db_path = std::path::Path::new(&self.storage.db_path);
        if db_path.is_absolute() {
            return Err(invalid(
                "storage.db_path",
                "a relative path beneath .cogz/",
                &self.storage.db_path,
            ));
        }
        if db_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(invalid(
                "storage.db_path",
                "no '..' parent components",
                &self.storage.db_path,
            ));
        }
        if !self.storage.db_path.starts_with(".cogz/") {
            return Err(invalid(
                "storage.db_path",
                "a path rooted beneath .cogz/ (e.g. '.cogz/cogz.db')",
                &self.storage.db_path,
            ));
        }
        if self.embedding.dimension == 0 {
            return Err(invalid(
                "embedding.dimension",
                "> 0",
                self.embedding.dimension,
            ));
        }
        // Model IDs flow into HF download URLs and local cache paths.
        // Require the owner/name shape before any of that happens.
        for (model_id, field) in [
            (&self.embedding.code_model, "embedding.code_model"),
            (&self.embedding.knowledge_model, "embedding.knowledge_model"),
            (&self.embedding.nli_model, "embedding.nli_model"),
        ] {
            if !model_id.is_empty() && !is_valid_model_id(model_id) {
                return Err(invalid(
                    field,
                    "a HuggingFace repo id in owner/name form (letters, digits, '.', '_', '-')",
                    model_id,
                ));
            }
        }
        // Cross-reference configured dimension against the model registry.
        // A mismatch means the vec0 table is created at one dimension while
        // the model produces vectors at another — KNN will fail silently.
        for (model_id, field) in [
            (&self.embedding.code_model, "embedding.code_model"),
            (&self.embedding.knowledge_model, "embedding.knowledge_model"),
        ] {
            if let Some(entry) = crate::embed::registry::lookup(model_id)
                && entry.dim != self.embedding.dimension
            {
                return Err(ConfigError::DimensionMismatch {
                    field,
                    configured: self.embedding.dimension,
                    registry: entry.dim,
                    model: model_id.clone(),
                });
            }
        }
        if self.search.rrf_k == 0 {
            return Err(invalid("search.rrf_k", "> 0", self.search.rrf_k));
        }
        if self.search.max_results == 0 {
            return Err(invalid(
                "search.max_results",
                "> 0",
                self.search.max_results,
            ));
        }
        if !matches!(
            self.search.merge_strategy.as_str(),
            "strength" | "fixed" | "detect" | "gradient" | "calibrated"
        ) {
            return Err(invalid(
                "search.merge_strategy",
                "'strength', 'fixed', 'detect', 'gradient', or 'calibrated'",
                &self.search.merge_strategy,
            ));
        }
        if !(0.0..1.0).contains(&self.search.min_relevance) {
            return Err(invalid(
                "search.min_relevance",
                "[0.0, 1.0)",
                self.search.min_relevance,
            ));
        }
        if !(0.0..=100.0).contains(&self.search.fts_title_weight) {
            return Err(invalid(
                "search.fts_title_weight",
                "[0.0, 100.0]",
                self.search.fts_title_weight,
            ));
        }
        if !(0.0..=1.0).contains(&self.search.mmr_lambda) {
            return Err(invalid(
                "search.mmr_lambda",
                "[0.0, 1.0]",
                self.search.mmr_lambda,
            ));
        }
        if self.search.provenance_boost < 0.0 {
            return Err(invalid(
                "search.provenance_boost",
                ">= 0.0",
                self.search.provenance_boost,
            ));
        }
        if self.consolidation.dedup_threshold < 0.0 || self.consolidation.dedup_threshold > 1.0 {
            return Err(invalid(
                "consolidation.dedup_threshold",
                "[0.0, 1.0]",
                self.consolidation.dedup_threshold,
            ));
        }
        if self.consolidation.title_match_threshold < 0.0
            || self.consolidation.title_match_threshold > 1.0
        {
            return Err(invalid(
                "consolidation.title_match_threshold",
                "[0.0, 1.0]",
                self.consolidation.title_match_threshold,
            ));
        }
        if self.retention.observation_prune_after_days == 0 {
            return Err(invalid(
                "retention.observation_prune_after_days",
                "> 0",
                self.retention.observation_prune_after_days,
            ));
        }
        if self.context.default_token_budget == 0 {
            return Err(invalid(
                "context.default_token_budget",
                "> 0",
                self.context.default_token_budget,
            ));
        }
        if self.context.task_token_budget == 0 {
            return Err(invalid(
                "context.task_token_budget",
                "> 0",
                self.context.task_token_budget,
            ));
        }
        if self.context.escalation_token_budget == 0 {
            return Err(invalid(
                "context.escalation_token_budget",
                "> 0",
                self.context.escalation_token_budget,
            ));
        }
        Ok(())
    }
}

/// Validate a HuggingFace model/repo id: `owner/name` where owner is
/// `[A-Za-z0-9][A-Za-z0-9._-]*`, name is `[A-Za-z0-9._-]+`, with no
/// `..` segments and no leading `/`. Mirrors the shape HF accepts —
/// anything else would become a URL or cache path we didn't intend.
fn is_valid_model_id(id: &str) -> bool {
    let Some((owner, name)) = id.split_once('/') else {
        return false;
    };
    if name.contains('/') || id.contains("..") || id.starts_with('/') {
        return false;
    }
    let valid_owner = {
        let mut chars = owner.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let valid_name = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    valid_owner && valid_name
}
