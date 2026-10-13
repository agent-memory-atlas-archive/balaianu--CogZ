//! Answer-evidence coverage for the silence gate.
//!
//! Embedding strength measures topical proximity, not answerability:
//! on a large corpus an unrelated query always finds a nearest
//! neighbor above the strength floor ("parse TOML" against clap
//! scores 0.71 on parsing internals). The discriminating evidence is
//! lexical — does the text we would actually return contain the
//! query's discriminating terms? A term absent from the entire
//! corpus is the strongest negative evidence; a term the corpus has
//! but the top results lack is nearly as strong. Coverage is the
//! IDF-weighted share of query content terms present in the fused
//! top-K, with corpus-absent terms counted at maximum IDF.
//!
//! Calibrated on 105 queries across 4 corpora (clap/cobra/httpx/cogz):
//! negatives mean 0.40 vs positives 0.85; a 0.45 floor silences ~2/3
//! of unanswerable queries at ~4% false-silence cost.

use rusqlite::Connection;

use super::SearchError;
use crate::storage::crud::Entity;
use crate::storage::{crud, query};

use super::prf::content_terms;

/// Results whose combined text is checked for answer evidence. The
/// fused head is what the caller would actually read — coverage past
/// it measures noise the gate doesn't ship.
const COVERAGE_WINDOW: usize = 10;

/// IDF-weighted fraction of `query`'s content terms found in the top
/// `COVERAGE_WINDOW` entities' title+content. Returns 1.0 when the
/// query yields no content terms — no lexical evidence to judge by.
pub(crate) fn answer_coverage(
    conn: &Connection,
    query_text: &str,
    results: &[&Entity],
) -> Result<f64, SearchError> {
    let terms = content_terms(query_text);
    if terms.is_empty() {
        return Ok(1.0);
    }
    let corpus_size = crud::count_all(conn)?.max(1) as f64;
    let dfs = query::fts_doc_freq(conn, &terms)?;
    let window: String = results
        .iter()
        .take(COVERAGE_WINDOW)
        .map(|e| format!("{} {}", e.title.as_deref().unwrap_or(""), e.content).to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let mut total = 0.0;
    let mut covered = 0.0;
    for (term, df) in terms.iter().zip(dfs.iter()) {
        // df=0 → max IDF: a term the corpus lacks entirely is the
        // strongest possible "no answer here" evidence.
        let w = (corpus_size / (*df).max(1) as f64).ln();
        total += w;
        if *df > 0 && window.contains(term.as_str()) {
            covered += w;
        }
    }
    Ok(if total > 0.0 { covered / total } else { 1.0 })
}
