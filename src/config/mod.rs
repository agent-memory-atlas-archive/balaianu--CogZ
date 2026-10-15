//! Config loading, validation, and default generation.

mod calibration;
mod settings;
mod validate;

pub use calibration::{CalibrationConfig, ChannelCalibration};
pub use settings::{
    Config, ConsolidationConfig, ContextConfig, EmbeddingConfig, IndexConfig, ProjectConfig,
    RetentionConfig, SearchConfig, StorageConfig,
};

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse config TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid value for {field}: expected {expected}, got {got}")]
    InvalidValue {
        field: &'static str,
        expected: &'static str,
        got: String,
    },
    #[error(
        "embedding.dimension ({configured}) does not match {field} registry dimension ({registry}) for model '{model}' — set dimension to {registry} or change the model"
    )]
    DimensionMismatch {
        field: &'static str,
        configured: usize,
        registry: usize,
        model: String,
    },
    #[error(".cogz/ directory not found at {} — run `cogz init` first", .0.display())]
    CogzDirMissing(std::path::PathBuf),
    #[error("DB parent directory not found: {}", .0.display())]
    DbParentMissing(std::path::PathBuf),
    #[error("DB path resolves outside .cogz/ (symlink escape?): {} -> {}", .resolved.display(), .canonical.display())]
    DbPathEscapes {
        resolved: std::path::PathBuf,
        canonical: std::path::PathBuf,
    },
    #[error("database not found at {} — run `cogz index` first", .0.display())]
    DbMissing(std::path::PathBuf),
}

/// Load config from a `.cogz/config.toml` file.
///
/// If the file does not exist, returns an error — `cogz init` must be
/// run first. The caller decides whether that's fatal or a prompt to
/// initialize.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&contents)?;
    config.validate()?;

    // Unknown keys are silently ignored by serde — warn so typos and
    // stale keys surface instead of looking applied.
    if let (Ok(raw), Ok(canon)) = (
        toml::from_str::<toml::Table>(&contents),
        toml::Value::try_from(&config),
    ) {
        let mut unknown = Vec::new();
        unknown_keys(&raw, &canon, "", &mut unknown);
        for key in unknown {
            tracing::warn!("unknown config key `{key}` in {} — ignored", path.display());
        }
    }
    Ok(config)
}

/// Keys present in `raw` but absent from `canon` (the deserialized
/// config serialized back — every known field materializes). Tables
/// recurse so `[search] silence_threshold` reports as
/// `search.silence_threshold`.
fn unknown_keys(raw: &toml::Table, canon: &toml::Value, prefix: &str, out: &mut Vec<String>) {
    for (k, v) in raw {
        let dotted = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match canon.get(k) {
            None => out.push(dotted),
            Some(cv) => {
                if let (Some(rt), true) = (v.as_table(), cv.is_table()) {
                    unknown_keys(rt, cv, &dotted, out);
                }
            }
        }
    }
}

/// Generate the default config TOML string for a given project name.
///
/// Used by `cogz init` to write the initial `config.toml`.
pub fn default_toml(project_name: &str) -> String {
    let config = Config::default_for(project_name);
    toml::to_string(&config).expect("default config serializes")
}

/// Resolve and verify the DB path against the repository root.
///
/// Joins `db_path` (from config) with `repo_root`, canonicalizes the
/// nearest existing parent, and verifies the result is beneath the
/// canonical `.cogz` directory. This is a defense-in-depth check that
/// catches symlinks pointing outside the repository — the structural
/// validation in `Config::validate()` already rejects absolute paths
/// and `..` components, but a symlink inside `.cogz/` could still
/// escape.
pub fn resolve_db_path(repo_root: &Path, db_path: &str) -> Result<std::path::PathBuf, ConfigError> {
    let cogz_dir = repo_root.join(".cogz");
    let resolved = repo_root.join(db_path);

    // Canonicalize the .cogz directory (must exist).
    let canonical_cogz = cogz_dir
        .canonicalize()
        .map_err(|_| ConfigError::CogzDirMissing(cogz_dir.clone()))?;

    // Canonicalize the nearest existing parent of the DB path.
    let parent = resolved.parent().unwrap_or(&cogz_dir);
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| ConfigError::DbParentMissing(parent.to_path_buf()))?;

    // Verify the canonical parent is beneath the canonical .cogz dir.
    if !canonical_parent.starts_with(&canonical_cogz) {
        return Err(ConfigError::DbPathEscapes {
            resolved,
            canonical: canonical_parent,
        });
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_roundtrips() {
        let toml_str = default_toml("test-project");
        let config: Config = toml::from_str(&toml_str).unwrap();
        config.validate().unwrap();
        assert_eq!(config.project.name, "test-project");
    }

    #[test]
    fn default_config_values() {
        let config = Config::default_for("my-repo");
        assert_eq!(config.project.name, "my-repo");
        assert_eq!(config.storage.db_path, ".cogz/cogz.db");
        assert_eq!(config.embedding.dimension, 768);
        assert_eq!(config.search.rrf_k, 60);
        assert_eq!(config.consolidation.dedup_threshold, 0.85);
        assert_eq!(config.retention.observation_prune_after_days, 90);
    }

    #[test]
    fn validation_rejects_empty_project_name() {
        let mut config = Config::default_for("test");
        config.project.name = String::new();
        let err = config.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidValue {
                field: "project.name",
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_negative_rrf_k() {
        let mut config = Config::default_for("test");
        config.search.rrf_k = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("rrf_k"));
    }

    #[test]
    fn validation_rejects_dimension_mismatch() {
        let mut config = Config::default_for("test");
        config.embedding.dimension = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("dimension"));
    }

    #[test]
    fn validation_errors_carry_machine_readable_fields() {
        let mut cases = Vec::new();
        for value in ["/abs/db", "db.sqlite", ".cogz/../db.sqlite"] {
            let mut c = Config::default_for("test");
            c.storage.db_path = value.to_string();
            cases.push((c, "storage.db_path"));
        }
        let mut c = Config::default_for("test");
        c.search.min_relevance = 1.5;
        cases.push((c, "search.min_relevance"));
        let mut c = Config::default_for("test");
        c.context.task_token_budget = 0;
        cases.push((c, "context.task_token_budget"));
        let mut c = Config::default_for("test");
        c.embedding.code_model = "bad id!".to_string();
        cases.push((c, "embedding.code_model"));

        for (config, field) in cases {
            let err = config.validate().unwrap_err();
            match err {
                ConfigError::InvalidValue {
                    field: f, expected, ..
                } => {
                    assert_eq!(f, field);
                    assert!(!expected.is_empty());
                }
                other => panic!("expected InvalidValue for {field}, got {other}"),
            }
        }
    }

    #[test]
    fn validation_reports_dimension_mismatch_typed() {
        let mut config = Config::default_for("test");
        config.embedding.code_model = "BAAI/bge-base-en-v1.5".to_string();
        config.embedding.dimension = 1;
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::DimensionMismatch {
                configured, model, ..
            } => {
                assert_eq!(configured, 1);
                assert!(!model.is_empty());
            }
            other => panic!("expected DimensionMismatch, got {other}"),
        }
    }

    #[test]
    fn resolve_db_path_reports_missing_and_escaping() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();

        // No .cogz at all → CogzDirMissing.
        let err = resolve_db_path(repo, ".cogz/cogz.db").unwrap_err();
        assert!(matches!(err, ConfigError::CogzDirMissing(_)), "got {err}");

        // .cogz exists but the db parent dir does not → DbParentMissing.
        std::fs::create_dir_all(repo.join(".cogz")).unwrap();
        let err = resolve_db_path(repo, ".cogz/nodir/cogz.db").unwrap_err();
        assert!(matches!(err, ConfigError::DbParentMissing(_)), "got {err}");

        // A symlinked directory escaping .cogz → DbPathEscapes.
        #[cfg(unix)]
        {
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, repo.join(".cogz/escape")).unwrap();
            let err = resolve_db_path(repo, ".cogz/escape/cogz.db").unwrap_err();
            assert!(
                matches!(err, ConfigError::DbPathEscapes { .. }),
                "got {err}"
            );
        }
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, default_toml("file-test")).unwrap();

        let config = load(&path).unwrap();
        assert_eq!(config.project.name, "file-test");
    }

    #[test]
    fn load_missing_file_errors() {
        let path = std::path::Path::new("/nonexistent/config.toml");
        assert!(load(path).is_err());
    }

    #[test]
    fn unknown_keys_reports_stale_and_nested() {
        let mut contents = default_toml("stale-test");
        contents = contents.replace("[search]", "[search]\nsilence_threshold = 0.05");
        contents.push_str("\n[bogus]\nx = 1\n");

        let config: Config = toml::from_str(&contents).unwrap();
        let raw: toml::Table = toml::from_str(&contents).unwrap();
        let canon = toml::Value::try_from(&config).unwrap();
        let mut out = Vec::new();
        unknown_keys(&raw, &canon, "", &mut out);

        // toml::Table orders keys alphabetically.
        assert_eq!(
            out,
            vec!["bogus".to_string(), "search.silence_threshold".to_string()]
        );

        // Stale keys still load — they're warnings, not errors.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, contents).unwrap();
        assert!(load(&path).is_ok());
    }
}
