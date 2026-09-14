use super::*;
use crate::embed::model::{EmbeddingError, EmbeddingResult};
use crate::storage::crud::Entity;

/// Positional scorer: returns `scores[i]` for the i-th candidate.
/// `fail` simulates an inference error; `short` returns fewer scores
/// than candidates to exercise the length-mismatch guard.
struct StubReranker {
    scores: Vec<f32>,
    fail: bool,
    short: bool,
}

impl RerankModel for StubReranker {
    fn score_batch(&self, _q: &str, candidates: &[&str]) -> EmbeddingResult<Vec<f32>> {
        if self.fail {
            return Err(EmbeddingError::InferenceFailed("boom".to_string()));
        }
        let n = if self.short {
            candidates.len().saturating_sub(1)
        } else {
            candidates.len()
        };
        Ok(self.scores.iter().take(n).cloned().collect())
    }
    fn model_name(&self) -> &str {
        "stub"
    }
    fn is_available(&self) -> bool {
        true
    }
}

fn stub_result(id: &str, relevance: f32) -> SearchResult {
    SearchResult {
        entity: Entity::new(id, "knowledge", id, "content"),
        relevance,
        graph_path: vec![id.to_string()],
        graph_path_description: String::new(),
    }
}

#[test]
fn rerank_directs_reorders_and_rescores_window() {
    let mut results = vec![
        stub_result("a", 0.9),
        stub_result("b", 0.8),
        stub_result("c", 0.7),
    ];
    let model = StubReranker {
        scores: vec![0.1, 0.5, 0.95],
        fail: false,
        short: false,
    };
    rerank_directs("q", &mut results, 20, 0, true, &model);
    let ids: Vec<&str> = results.iter().map(|r| r.entity.id.as_str()).collect();
    assert_eq!(ids, ["c", "b", "a"]);
    // Relevance is replaced by the cross-encoder probability.
    assert!((results[0].relevance - 0.95).abs() < 1e-6);
    assert!((results[2].relevance - 0.1).abs() < 1e-6);
}

#[test]
fn rerank_directs_depth_caps_window() {
    let mut results = vec![
        stub_result("a", 0.9),
        stub_result("b", 0.8),
        stub_result("c", 0.7),
        stub_result("d", 0.6),
    ];
    let model = StubReranker {
        scores: vec![0.1, 0.9],
        fail: false,
        short: false,
    };
    rerank_directs("q", &mut results, 2, 0, true, &model);
    let ids: Vec<&str> = results.iter().map(|r| r.entity.id.as_str()).collect();
    // Only the first two are rescored; c and d keep place and score.
    assert_eq!(ids, ["b", "a", "c", "d"]);
    assert!((results[3].relevance - 0.6).abs() < 1e-6);
}

#[test]
fn rerank_directs_code_slots_pinned() {
    let mut results = vec![
        stub_result("a", 0.9),
        SearchResult {
            entity: Entity::new("f1", "function", "fn_a", "code"),
            relevance: 0.8,
            graph_path: vec!["f1".to_string()],
            graph_path_description: String::new(),
        },
        stub_result("b", 0.7),
        stub_result("c", 0.6),
    ];
    let model = StubReranker {
        scores: vec![0.3, 0.9, 0.5],
        fail: false,
        short: false,
    };
    // rerank_code=false: f1 is excluded from scoring and keeps slot 1.
    rerank_directs("q", &mut results, 20, 0, false, &model);
    let ids: Vec<&str> = results.iter().map(|r| r.entity.id.as_str()).collect();
    assert_eq!(ids, ["b", "f1", "c", "a"]);
    assert!((results[1].relevance - 0.8).abs() < 1e-6);
}

#[test]
fn rerank_directs_anchor_pins_fused_head() {
    let mut results = vec![
        stub_result("a", 0.9),
        stub_result("b", 0.8),
        stub_result("c", 0.7),
        stub_result("d", 0.6),
    ];
    let model = StubReranker {
        scores: vec![0.9, 0.1, 0.5],
        fail: false,
        short: false,
    };
    rerank_directs("q", &mut results, 20, 1, true, &model);
    let ids: Vec<&str> = results.iter().map(|r| r.entity.id.as_str()).collect();
    // "a" is anchored: b/c/d reorder by CE score; its fused score stays.
    assert_eq!(ids, ["a", "b", "d", "c"]);
    assert!((results[0].relevance - 0.9).abs() < 1e-6);
    assert!((results[1].relevance - 0.9).abs() < 1e-6);
}

#[test]
fn rerank_directs_failure_modes_keep_fused_order() {
    for (name, model) in [
        (
            "inference error",
            StubReranker {
                scores: vec![],
                fail: true,
                short: false,
            },
        ),
        (
            "short score vector",
            StubReranker {
                scores: vec![0.9, 0.1, 0.5],
                fail: false,
                short: true,
            },
        ),
    ] {
        let mut results = vec![
            stub_result("a", 0.9),
            stub_result("b", 0.8),
            stub_result("c", 0.7),
        ];
        rerank_directs("q", &mut results, 20, 0, true, &model);
        let ids: Vec<&str> = results.iter().map(|r| r.entity.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"], "{name}");
        assert!((results[0].relevance - 0.9).abs() < 1e-6, "{name}");
    }
}
