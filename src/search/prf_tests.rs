//! Tests for pseudo-relevance feedback term extraction.

use super::*;
use crate::storage::crud::Entity;

fn entity(title: &str, content: &str) -> Entity {
    Entity::new("x", "knowledge", title, content)
}

#[test]
fn tokenize_splits_identifiers() {
    let terms = tokenize("run_migrations camelCase mixedCASE_word");
    assert!(terms.contains(&"run".to_string()));
    assert!(terms.contains(&"migrations".to_string()));
    assert!(terms.contains(&"camel".to_string()));
    assert!(terms.contains(&"case".to_string()));
}

#[test]
fn expansion_terms_require_two_documents() {
    // "migrations" appears in both docs; "flagging" only in one.
    let a = entity("a", "schema migrations track state");
    let b = entity("b", "migrations run on startup, flagging stale docs");
    let terms = expansion_terms(&[a, b], "unrelated query", 8);
    assert!(terms.contains(&"migrations".to_string()));
    assert!(!terms.contains(&"flagging".to_string()));
}

#[test]
fn expansion_terms_skip_query_and_stopwords() {
    let a = entity("a", "the schema entity storage layer");
    let b = entity("b", "entity storage with the schema");
    let terms = expansion_terms(&[a, b], "entity", 8);
    assert!(!terms.contains(&"entity".to_string()));
    assert!(!terms.contains(&"the".to_string()));
    assert!(terms.contains(&"schema".to_string()));
}

#[test]
fn expansion_terms_title_weighted() {
    // Both terms appear in both docs' content, but "special" is also
    // in a title — it must outrank "common".
    let a = entity("special handling", "common path special case");
    let b = entity("b", "common path special case");
    let terms = expansion_terms(&[a, b], "q", 1);
    assert_eq!(terms, vec!["special".to_string()]);
}

#[test]
fn expand_query_appends_terms() {
    assert_eq!(expand_query("a b", &[]), "a b");
    assert_eq!(
        expand_query("a", &["x".to_string(), "y".to_string()]),
        "a x y"
    );
}
