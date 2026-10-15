//! Identifier-splitting for FTS. unicode61 indexes `verifyAuthToken`
//! as a single opaque token, so a query for "auth" or "token" misses
//! it. When `index.split_identifiers` is enabled, insert/update writes
//! derive subtokens from the entity title into `entities.search_terms`,
//! which the FTS table indexes as a third column. Default: disabled —
//! existing and freshly indexed rows keep `search_terms = ''`.

use std::sync::atomic::{AtomicBool, Ordering};

static SPLIT_IDENTIFIERS: AtomicBool = AtomicBool::new(false);

/// Enable or disable identifier-splitting at write time (config
/// `index.split_identifiers`). Set once by entry points from config.
pub fn set_split_identifiers(enabled: bool) {
    SPLIT_IDENTIFIERS.store(enabled, Ordering::Relaxed);
}

pub(crate) fn split_identifiers_enabled() -> bool {
    SPLIT_IDENTIFIERS.load(Ordering::Relaxed)
}

/// Derive FTS subtokens from an identifier-bearing title.
/// `verifyAuthToken` → "verify auth token"; `enableHTTP2` →
/// "enable http 2 http2" (letter+digit merges are emitted too, since
/// "http2" is itself a plausible query term). Returns "" when the
/// title yields no internal splits — snake_case and plain words are
/// already handled by the unicode61 tokenizer.
pub(crate) fn derive_search_terms(title: &str) -> String {
    let mut terms: Vec<String> = Vec::new();
    for token in title.split(|c: char| !c.is_alphanumeric()) {
        let parts = split_identifier(token);
        if parts.len() > 1 {
            terms.extend(parts.iter().map(String::as_str).map(str::to_string));
            // Merge letter-run + digit-run adjacents back together so
            // "http2", "utf8", "sha256" stay reachable as single terms.
            for pair in parts.windows(2) {
                if pair[0].chars().all(|c| c.is_alphabetic())
                    && pair[1].chars().all(|c| c.is_ascii_digit())
                {
                    terms.push(format!("{}{}", pair[0], pair[1]));
                }
            }
        }
    }
    terms.join(" ")
}

/// Split one identifier on case and digit boundaries:
/// lower→upper, acronym→word (`HTTPRequest` → `HTTP`, `Request`),
/// and letter↔digit transitions.
fn split_identifier(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (prev, cur) = (chars[i - 1], chars[i]);
        let next = chars.get(i + 1).copied();
        // Acronym→word boundary (HTTPRequest → HTTP|Request) fires
        // only when the uppercase run before it is ≥2 chars — a lone
        // leading capital must not split (`OAuth` stays whole).
        let upper_run = (0..i)
            .rev()
            .take_while(|&j| chars[j].is_uppercase())
            .count();
        let boundary = (prev.is_lowercase() && cur.is_uppercase())
            || (prev.is_uppercase()
                && cur.is_uppercase()
                && next.is_some_and(|n| n.is_lowercase())
                && upper_run >= 2)
            || (prev.is_ascii_digit() != cur.is_ascii_digit());
        if boundary {
            parts.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    parts.push(chars[start..].iter().collect());
    parts.retain(|p: &String| !p.is_empty());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_identifier_boundaries() {
        for (title, expected) in [
            ("verifyAuthToken", vec!["verify", "Auth", "Token"]),
            ("HTTPRequest", vec!["HTTP", "Request"]),
            ("enableHTTP2", vec!["enable", "HTTP", "2"]),
            ("OAuth2", vec!["OAuth", "2"]),
            ("utf8Decoder", vec!["utf", "8", "Decoder"]),
            ("plainword", vec!["plainword"]),
        ] {
            assert_eq!(split_identifier(title), expected, "{title}");
        }
    }

    #[test]
    fn derived_terms_cover_query_forms() {
        // Snake_case and plain words produce nothing — unicode61
        // already splits them. Internal splits emit subtokens plus
        // letter+digit merges ("http2" must stay a reachable term).
        assert_eq!(derive_search_terms("verify_token"), "");
        assert_eq!(derive_search_terms("send"), "");
        assert_eq!(derive_search_terms("verifyAuthToken"), "verify Auth Token");
        let t = derive_search_terms("enableHTTP2");
        assert!(t.contains("HTTP"), "{t}");
        assert!(t.contains("http2") || t.contains("HTTP2"), "{t}");
    }

    #[test]
    fn flag_gates_population_and_fts_matches() {
        let storage = crate::storage::Storage::open_memory().unwrap();
        let conn = storage.conn();
        let e = crate::storage::Entity::new(
            "id-1",
            "function",
            "verifyAuthToken",
            "fn verifyAuthToken(t: &str) -> bool {}",
        );

        // Flag off → no terms, FTS cannot find "auth".
        crate::storage::crud::insert_entity(&conn, &e).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities_fts WHERE entities_fts MATCH '\"auth\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);

        // Flag on → update rewrites terms, "auth" and "token" hit.
        set_split_identifiers(true);
        crate::storage::crud::update_entity(&conn, &e).unwrap();
        set_split_identifiers(false);
        for term in ["auth", "token"] {
            let hit: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM entities_fts WHERE entities_fts MATCH ?1",
                    [format!("\"{term}\"")],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(hit, 1, "term {term} must match split identifier");
        }
    }
}
