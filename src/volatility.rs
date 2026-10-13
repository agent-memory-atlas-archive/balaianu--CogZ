//! Entity volatility — the classification behind drift tracking.
//!
//! `living` entities are claims about the current state of the world:
//! architecture docs, gotchas, how-it-works knowledge, rules. They can
//! drift — the drift machinery recomputes their `verified_against`
//! provenance against code state on every index.
//!
//! `record` entities are immutable dated history: observations, and
//! knowledge in dated-fact categories (decisions, benchmark results,
//! replays, incidents). The fact that something was measured or
//! decided on a date does not drift — it was true when written.
//! Records are exempt from drift tracking and drift-based demotion;
//! only semantic contradiction (NLI verification) can mark them.

/// Claim about current state — subject to drift tracking.
pub const LIVING: &str = "living";
/// Immutable dated fact — exempt from drift tracking.
pub const RECORD: &str = "record";

/// Knowledge categories that classify as `record` by default.
/// `decisions` were made when they were made; benchmark/replay/
/// incident entries describe a specific run or event. Both are
/// dated history, not claims about current code.
pub(crate) const RECORD_CATEGORIES: &[&str] =
    &["decision", "decisions", "benchmark", "replay", "incident"];

/// True when `v` is a valid `volatility` frontmatter value.
pub fn is_valid(v: &str) -> bool {
    matches!(v, LIVING | RECORD)
}

/// Effective volatility for an entity. An explicit `volatility`
/// frontmatter value wins (it lands in `properties` via sync);
/// otherwise the default derives from type and knowledge category.
pub fn classify(entity_type: &str, properties: &serde_json::Value) -> &'static str {
    if let Some(v) = properties.get("volatility").and_then(|v| v.as_str())
        && is_valid(v)
    {
        return if v == RECORD { RECORD } else { LIVING };
    }
    if entity_type == "observation" {
        return RECORD;
    }
    if entity_type == "knowledge"
        && let Some(cat) = properties.get("category").and_then(|c| c.as_str())
        && RECORD_CATEGORIES.contains(&cat)
    {
        return RECORD;
    }
    LIVING
}

/// Effective volatility for a parsed entity file — the file-domain
/// twin of `classify`, for consumers that act before sync (creation-
/// time provenance stamping).
pub fn classify_file(entity_file: &crate::files::EntityFile) -> &'static str {
    let explicit = entity_file
        .frontmatter
        .get("volatility")
        .and_then(|v| v.as_str());
    if let Some(v) = explicit
        && is_valid(v)
    {
        return if v == RECORD { RECORD } else { LIVING };
    }
    if entity_file.entity_type == crate::files::FileEntityType::Observation {
        return RECORD;
    }
    if entity_file.entity_type == crate::files::FileEntityType::Knowledge {
        let cat = entity_file
            .frontmatter
            .get("category")
            .and_then(|v| v.as_str());
        if cat.is_some_and(|c| RECORD_CATEGORIES.contains(&c)) {
            return RECORD;
        }
    }
    LIVING
}
