//! Tests for hybrid search.

use super::super::super::storage::crud::insert_entity;
use super::super::super::storage::edges::Edge;
use super::super::super::storage::edges::insert_edge;
use super::super::super::storage::embeddings::insert_embedding;
use super::super::super::storage::ensure_vec_extension;
use super::super::super::storage::schema::run_migrations;
use super::*;

#[path = "hybrid_tests_extra.rs"]
mod hybrid_tests_extra;

fn setup() -> Connection {
    ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();
    conn
}

fn default_config() -> SearchConfig {
    SearchConfig {
        fts_weight: 0.3,
        vec_weight: 0.4,
        code_vec_weight: 0.3,
        rrf_k: 60,
        max_results: 20,
        min_source_proportion: 0.2,
        source_balance_enabled: false,
        merge_strategy: "strength".to_string(),
        min_relevance: 0.05,
        edge_weighted_expansion: true,
        silence_threshold: 0.0,
        top_diversity_share: 0.0,
        calibration: crate::config::CalibrationConfig::default(),
        provenance_boost: 0.0,
        fts_title_weight: 1.0,
        mmr_lambda: 0.0,
        graph_first_enabled: false,
        graph_max_seeds: 10,
        graph_max_hops: 2,
        graph_hop_decay: 0.5,
        graph_weight: 0.35,
        silence_strength_floor: 0.64,
        prf_enabled: false,
        prf_feedback_docs: 5,
        prf_max_terms: 8,
        graph_seed_min_sim: 0.0,
    }
}

fn fixed_config() -> SearchConfig {
    SearchConfig {
        merge_strategy: "fixed".to_string(),
        min_relevance: 0.0,
        edge_weighted_expansion: false,
        silence_threshold: 0.0,
        top_diversity_share: 0.0,
        calibration: crate::config::CalibrationConfig::default(),
        provenance_boost: 0.0,
        fts_title_weight: 1.0,
        mmr_lambda: 0.0,
        ..default_config()
    }
}

fn balanced_config() -> SearchConfig {
    SearchConfig {
        source_balance_enabled: true,
        ..default_config()
    }
}

fn edge(source: &str, target: &str, edge_type: &str) -> Edge {
    Edge {
        source_id: source.to_string(),
        target_id: target.to_string(),
        edge_type: edge_type.to_string(),
        weight: 1.0,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[test]
fn fts_only_search() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new(
            "u1",
            "observation",
            "FTS5 ranking bug",
            "ranking issue content",
        ),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("u2", "rule", "Unrelated", "completely different"),
    )
    .unwrap();

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::FtsOnly);
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].entity.id, "u1");
    assert!(results.results[0].relevance > 0.0);
}

#[test]
fn hybrid_search() {
    let conn = setup();
    let e1 = Entity::new("u1", "observation", "FTS5 ranking", "ranking content");
    insert_entity(&conn, &e1).unwrap();
    insert_embedding(&conn, "u1", "observation", &vec![0.1_f32; 768]).unwrap();

    let e2 = Entity::new("u2", "observation", "Other", "different content");
    insert_entity(&conn, &e2).unwrap();
    insert_embedding(&conn, "u2", "observation", &vec![0.9_f32; 768]).unwrap();

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let query_vec = vec![0.1_f32; 768];
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::knowledge(&query_vec),
        &params,
        &default_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::KnowledgeHybrid);
    // u1 matches both FTS and vec, should be first
    assert_eq!(results.results[0].entity.id, "u1");
}

#[test]
fn search_with_type_filter() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("u1", "observation", "ranking", "ranking content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("u2", "rule", "ranking", "ranking content"),
    )
    .unwrap();

    let params = SearchParams {
        entity_type: Some("observation".to_string()),
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();

    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].entity.r#type, "observation");
}

#[test]
fn balanced_fusion_code_favored_when_query_closer_to_code() {
    let conn = setup();

    // Knowledge entity — matches FTS for "indexing"
    insert_entity(
        &conn,
        &Entity::new(
            "k1",
            "knowledge",
            "Search pipeline indexing",
            "indexing pipeline content",
        ),
    )
    .unwrap();
    // Code entity — also matches FTS for "indexing"
    insert_entity(
        &conn,
        &Entity::new(
            "f1",
            "function",
            "index_code",
            "fn index_code() { indexing }",
        ),
    )
    .unwrap();

    // Knowledge embedding: far from query
    insert_embedding(&conn, "k1", "knowledge", &vec![0.9_f32; 768]).unwrap();
    // Code embedding: close to query
    insert_embedding(&conn, "f1", "function", &vec![0.1_f32; 768]).unwrap();

    let query_vec = vec![0.1_f32; 768];
    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "indexing",
        QueryEmbeddings::both(&query_vec, &query_vec),
        &params,
        &balanced_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::Hybrid);
    // Both should be present
    assert_eq!(results.results.len(), 2);
    // Code entity should rank higher — query embedding is closer to code space
    assert_eq!(results.results[0].entity.id, "f1");
    assert_eq!(results.results[1].entity.id, "k1");
}

#[test]
fn balanced_fusion_knowledge_favored_when_query_closer_to_knowledge() {
    let conn = setup();

    insert_entity(
        &conn,
        &Entity::new(
            "k1",
            "knowledge",
            "Search pipeline indexing",
            "indexing pipeline content",
        ),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new(
            "f1",
            "function",
            "index_code",
            "fn index_code() { indexing }",
        ),
    )
    .unwrap();

    // Knowledge embedding: close to query
    insert_embedding(&conn, "k1", "knowledge", &vec![0.1_f32; 768]).unwrap();
    // Code embedding: far from query
    insert_embedding(&conn, "f1", "function", &vec![0.9_f32; 768]).unwrap();

    let query_vec = vec![0.1_f32; 768];
    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "indexing",
        QueryEmbeddings::both(&query_vec, &query_vec),
        &params,
        &balanced_config(),
    )
    .unwrap();

    assert_eq!(results.results.len(), 2);
    // Knowledge entity should rank higher — query embedding is closer to knowledge space
    assert_eq!(results.results[0].entity.id, "k1");
    assert_eq!(results.results[1].entity.id, "f1");
}

// ─── edge_weight ─────────────────────────────────────────────────

#[test]
fn edge_weight_orders_curated_above_structural() {
    assert_eq!(edge_weight("references"), 1.0);
    assert_eq!(edge_weight("supports"), 1.0);
    assert!(edge_weight("auto_references") > edge_weight("imports"));
    assert!(edge_weight("references") > edge_weight("calls"));
    assert_eq!(edge_weight("unknown_edge_type"), 0.5);
}

// ─── strength merge end-to-end ───────────────────────────────────

#[test]
fn strength_merge_suppresses_far_channel() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new(
            "k1",
            "knowledge",
            "Search pipeline indexing",
            "indexing pipeline content",
        ),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new(
            "f1",
            "function",
            "index_code",
            "fn index_code() { indexing }",
        ),
    )
    .unwrap();
    // Knowledge embedding identical to query; code embedding far.
    insert_embedding(&conn, "k1", "knowledge", &vec![0.1_f32; 768]).unwrap();
    insert_embedding(&conn, "f1", "function", &vec![0.9_f32; 768]).unwrap();

    let query_vec = vec![0.1_f32; 768];
    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "indexing",
        QueryEmbeddings::both(&query_vec, &query_vec),
        &params,
        &default_config(),
    )
    .unwrap();

    // Under "fixed" both survive (quota). Under "strength" + floor,
    // the far code channel's normalized-1.0 top hit scales by its
    // absolute strength (~0 for an orthogonal vector) and is dropped.
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].entity.id, "k1");
    assert_eq!(results.filtered_count, 1);
}

#[test]
fn fixed_strategy_keeps_quota_behavior() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("k1", "knowledge", "indexing", "indexing content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new(
            "f1",
            "function",
            "index_code",
            "fn index_code() { indexing }",
        ),
    )
    .unwrap();
    insert_embedding(&conn, "k1", "knowledge", &vec![0.1_f32; 768]).unwrap();
    insert_embedding(&conn, "f1", "function", &vec![0.9_f32; 768]).unwrap();

    let query_vec = vec![0.1_f32; 768];
    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "indexing",
        QueryEmbeddings::both(&query_vec, &query_vec),
        &params,
        &fixed_config(),
    )
    .unwrap();

    assert_eq!(results.results.len(), 2);
    assert_eq!(results.filtered_count, 0);
}

// ── Graph-first retrieval ─────────────────────────────────────────

fn graph_config() -> SearchConfig {
    SearchConfig {
        graph_first_enabled: true,
        ..default_config()
    }
}

#[test]
fn graph_first_surfaces_unreachable_function() {
    let conn = setup();
    // obs1 matches the query lexically; f1 doesn't but is one
    // `references` hop away. With graph-first on, f1 enters the
    // merge as a primary candidate instead of decayed context.
    insert_entity(
        &conn,
        &Entity::new("obs1", "observation", "ranking bug", "ranking content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("f1", "function", "fn", "fn f1() { unrelated }"),
    )
    .unwrap();
    insert_edge(&conn, &edge("obs1", "f1", "references")).unwrap();

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let enabled = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &graph_config(),
    )
    .unwrap();
    let disabled = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();

    assert!(enabled.results.iter().any(|r| r.entity.id == "f1"));
    assert!(!disabled.results.iter().any(|r| r.entity.id == "f1"));
}

#[test]
fn graph_first_works_fts_only() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("obs1", "observation", "ranking bug", "ranking content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("f1", "function", "fn", "fn f1() { unrelated }"),
    )
    .unwrap();
    insert_edge(&conn, &edge("obs1", "f1", "references")).unwrap();

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &graph_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::FtsOnly);
    assert!(results.results.iter().any(|r| r.entity.id == "f1"));
}

#[test]
fn graph_first_empty_when_no_edges() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("obs1", "observation", "ranking bug", "ranking content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("island", "function", "fn", "fn island() {}"),
    )
    .unwrap();

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &graph_config(),
    )
    .unwrap();

    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].entity.id, "obs1");
}

#[test]
fn graph_first_disabled_matches_legacy() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("obs1", "observation", "ranking bug", "ranking content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("f1", "function", "fn", "fn f1() { unrelated }"),
    )
    .unwrap();
    insert_edge(&conn, &edge("obs1", "f1", "references")).unwrap();

    // expand=true: legacy decayed expansion still surfaces f1 as
    // context — disabled means graph candidates never enter the
    // merge, so f1 can only appear via the expansion path.
    let params = SearchParams {
        expand: true,
        max_hops: 1,
        ..Default::default()
    };
    let config = SearchConfig {
        min_relevance: 0.0,
        ..default_config()
    };
    let results = search(&conn, "ranking", QueryEmbeddings::none(), &params, &config).unwrap();

    let f1 = results
        .results
        .iter()
        .find(|r| r.entity.id == "f1")
        .unwrap();
    assert_eq!(f1.graph_path, vec!["obs1", "f1"]);
}

// ── Silence gate ────────────────────────────────────────────────

#[test]
fn silence_gate_truth_table() {
    use super::super::ChannelSignals;
    let flat_low = ChannelSignals {
        code_strength: 0.3,
        knowledge_strength: 0.3,
        code_gradient: 0.01,
        knowledge_gradient: 0.01,
    };
    // Flat gradients + weak absolute strength on both channels: silence.
    assert!(hybrid_helpers::should_silence(&flat_low, 0.02, 0.64));

    // Flat gradient but a genuinely close match escapes — this is the
    // "uniformly decent neighbors" case the floor was added for.
    let flat_strong = ChannelSignals {
        knowledge_strength: 0.68,
        ..flat_low.clone()
    };
    assert!(!hybrid_helpers::should_silence(&flat_strong, 0.02, 0.64));

    // A distinctive gradient on either channel also escapes.
    let grad = ChannelSignals {
        code_gradient: 0.5,
        ..flat_low.clone()
    };
    assert!(!hybrid_helpers::should_silence(&grad, 0.02, 0.64));

    // threshold 0 disables the gate entirely.
    assert!(!hybrid_helpers::should_silence(&flat_low, 0.0, 0.64));
}
