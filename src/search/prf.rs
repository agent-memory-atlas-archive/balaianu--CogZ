//! Pseudo-relevance feedback — expand the FTS query with informative
//! terms mined from the top lexical hits, then re-run FTS.
//!
//! Anti-leakage queries describe a concept without naming it, so the
//! expected entity's identifiers never match the query. The top hits
//! often *contain* the missing vocabulary (a knowledge file that says
//! "run_migrations migrates the schema"), so feeding its terms back
//! into the query surfaces entities the original phrasing could not.

use std::collections::{HashMap, HashSet};

use crate::storage::crud::Entity;

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "have", "been", "were", "they", "this", "that", "with", "from", "each",
    "which", "their", "will", "would", "there", "what", "about", "when", "make", "like", "just",
    "over", "such", "into", "than", "them", "then", "only", "some", "could", "other", "does",
    "how", "where", "why", "who", "should", "must", "shall", "being", "between", "through",
    // code keywords that dominate function bodies without carrying meaning
    "self", "let", "mut", "pub", "impl", "struct", "enum", "fn", "mod", "use", "return", "match",
    "else", "loop", "while", "break", "continue", "const", "static", "trait", "type", "where",
    "async", "await", "move", "dyn", "ref", "box", "vec", "new", "true", "false", "none", "some",
    "string", "result", "option", "error", "unwrap", "expect", "clone",
];

/// Split text into lowercase terms: non-alphanumeric boundaries plus
/// camelCase and snake_case splits, so `runMigrations` and
/// `run_migrations` both yield `run`, `migrations`.
fn tokenize(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for chunk in text.split(|c: char| !c.is_alphanumeric()) {
        if chunk.is_empty() {
            continue;
        }
        // camelCase boundary: lowercase/digit followed by uppercase
        let mut term = String::new();
        for (i, c) in chunk.chars().enumerate() {
            if i > 0 && c.is_uppercase() {
                let prev = chunk.chars().nth(i - 1).unwrap_or(' ');
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    if term.len() >= 2 {
                        terms.push(term.to_lowercase());
                    }
                    term.clear();
                }
            }
            term.push(c);
        }
        if term.len() >= 2 {
            terms.push(term.to_lowercase());
        }
    }
    terms
}

/// Extract up to `max_terms` expansion terms from feedback entities.
///
/// A term must appear in at least two feedback documents to qualify —
/// a single-hit term is usually identifier junk, while recurring terms
/// carry the document cluster's shared vocabulary. Title terms count
/// triple: titles are the densest signal an entity has.
pub fn expansion_terms(feedback: &[Entity], query: &str, max_terms: usize) -> Vec<String> {
    let query_terms: HashSet<String> = tokenize(query).into_iter().collect();
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();

    let mut score: HashMap<String, f64> = HashMap::new();
    let mut df: HashMap<String, usize> = HashMap::new();

    for entity in feedback {
        let mut seen: HashSet<String> = HashSet::new();
        for term in tokenize(entity.title.as_deref().unwrap_or_default()) {
            if seen.insert(term.clone()) {
                *df.entry(term.clone()).or_default() += 1;
            }
            *score.entry(term).or_default() += 3.0;
        }
        for term in tokenize(&entity.content) {
            if seen.insert(term.clone()) {
                *df.entry(term.clone()).or_default() += 1;
            }
            *score.entry(term).or_default() += 1.0;
        }
    }

    let mut terms: Vec<(String, f64)> = score
        .into_iter()
        .filter(|(t, _)| {
            t.len() >= 4
                && !t.chars().all(|c| c.is_ascii_digit())
                && !stop.contains(t.as_str())
                && !query_terms.contains(t)
                && df.get(t).copied().unwrap_or(0) >= 2
        })
        .collect();
    terms.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    terms.truncate(max_terms);
    terms.into_iter().map(|(t, _)| t).collect()
}

/// Build the expanded FTS query: original query plus the expansion
/// terms. `fts_search` joins all terms with OR, so appending is the
/// whole interpolation.
pub fn expand_query(query: &str, terms: &[String]) -> String {
    if terms.is_empty() {
        return query.to_string();
    }
    format!("{} {}", query, terms.join(" "))
}

#[cfg(test)]
#[path = "prf_tests.rs"]
mod tests;
