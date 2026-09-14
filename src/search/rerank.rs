//! Cross-encoder rerank of the fused direct list, guarded against
//! prose-domain model bias.

use crate::embed::RerankModel;
use crate::storage::crud::EntityType;

use super::SearchResult;

/// Rerank the top `depth` direct results with a cross-encoder,
/// keeping the first `anchor` positions pinned. Code entities hold
/// their fused slots unless `rerank_code` is set.
///
/// The model scores each `(query, "title\ncontent")` pair jointly, so
/// its probabilities are comparable across channels regardless of
/// which embedding space retrieved the candidate — inside the
/// reranked window the merge-proportion mismatch dissolves. Entries
/// beyond `depth` keep their fused order. The anchor and the code-slot
/// pin exist because passage-domain cross-encoders systematically
/// prefer prose over code entities: pinned slots bound worst-case
/// damage while the remaining slots reorder freely by cross-encoder
/// score. Reranked entries' relevance becomes the cross-encoder
/// probability for display; expansion seeding in `hybrid` keeps the
/// pre-rerank fused score. Model failure is loud-warned and leaves
/// the order unchanged — the pre-rerank list is still a valid answer.
pub fn rerank_directs(
    query: &str,
    results: &mut [SearchResult],
    depth: usize,
    anchor: usize,
    rerank_code: bool,
    model: &dyn RerankModel,
) {
    let window = depth.min(results.len());
    if window <= anchor {
        return;
    }
    let slots: Vec<usize> = (anchor..window)
        .filter(|&i| {
            rerank_code
                || EntityType::parse(&results[i].entity.r#type)
                    .map(|t| !t.is_code())
                    .unwrap_or(true)
        })
        .collect();
    if slots.is_empty() {
        return;
    }
    let candidates: Vec<String> = slots
        .iter()
        .map(|&i| {
            let r = &results[i];
            format!(
                "{}\n{}",
                r.entity.title.as_deref().unwrap_or(""),
                r.entity.content
            )
        })
        .collect();
    let refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    let scores = match model.score_batch(query, &refs) {
        Ok(s) if s.len() == slots.len() => s,
        Ok(s) => {
            tracing::warn!(
                "reranker returned {} scores for {} candidates — skipping rerank",
                s.len(),
                slots.len()
            );
            return;
        }
        Err(e) => {
            tracing::warn!("cross-encoder rerank failed: {e}");
            return;
        }
    };
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut reordered: Vec<SearchResult> = Vec::with_capacity(slots.len());
    for &i in &order {
        let mut r = results[slots[i]].clone();
        r.relevance = scores[i];
        reordered.push(r);
    }
    for (slot, r) in slots.iter().zip(reordered) {
        results[*slot] = r;
    }
}

#[cfg(test)]
#[path = "rerank_tests.rs"]
mod tests;
