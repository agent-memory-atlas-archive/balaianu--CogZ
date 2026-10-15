//! Integration tests for the search pipeline.
//!
//! Tests the full search flow: insert entities with embeddings and
//! edges, run hybrid search, verify ranking, graph expansion, and
//! provenance paths.

use cogz::config::SearchConfig;
use cogz::embed::{EmbeddingCache, EmbeddingModel, MockEmbeddingModel};
use cogz::search::{QueryEmbeddings, SearchMode, SearchParams, search};
use cogz::storage::{self, Storage, crud::Entity};

fn setup_storage() -> Storage {
    Storage::open_memory().unwrap()
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
        top_diversity_share: 0.0,
        calibration: cogz::config::CalibrationConfig::default(),
        provenance_boost: 0.0,
        drift_penalty: 1.0,
        fts_title_weight: 1.0,
        mmr_lambda: 0.0,
        graph_first_enabled: false,
        graph_max_seeds: 10,
        graph_max_hops: 2,
        graph_hop_decay: 0.5,
        graph_weight: 0.35,
        silence_coverage_floor: 0.45,
        prf_enabled: false,
        prf_feedback_docs: 5,
        prf_max_terms: 8,
        sibling_expand_enabled: true,
        sibling_max_anchors: 8,
        cochange_enabled: true,
        graph_seed_min_sim: 0.0,
    }
}

fn insert_with_embedding(
    conn: &rusqlite::Connection,
    model: &dyn EmbeddingModel,
    id: &str,
    etype: &str,
    title: &str,
    content: &str,
) {
    let entity = Entity::new(id, etype, title, content);
    storage::crud::insert_entity(conn, &entity).unwrap();

    let text = format!("{title}\n\n{content}");
    let embeddings = model.embed(&[&text]).unwrap();
    storage::embeddings::insert_embedding(conn, id, etype, &embeddings[0]).unwrap();
}

fn insert_edge(conn: &rusqlite::Connection, source: &str, target: &str, edge_type: &str) {
    let edge = storage::edges::Edge {
        source_id: source.to_string(),
        target_id: target.to_string(),
        edge_type: edge_type.to_string(),
        weight: 1.0,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    storage::edges::insert_edge(conn, &edge).unwrap();
}

#[test]
fn fts_only_search_finds_relevant_entities() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "u1",
        "observation",
        "FTS5 ranking bug",
        "The RRF fusion produces incorrect rankings when k=60",
    );
    insert_with_embedding(
        &conn,
        &model,
        "u2",
        "rule",
        "Use SQLite",
        "Always use SQLite for local persistence",
    );
    insert_with_embedding(
        &conn,
        &model,
        "u3",
        "knowledge",
        "Search Architecture",
        "The search uses hybrid FTS and vector search",
    );

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
    assert!(!results.results.is_empty());
    assert_eq!(results.results[0].entity.id, "u1");
}

#[test]
fn hybrid_search_fuses_fts_and_vector() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    // u1 matches both FTS ("ranking") and is close in vector space
    insert_with_embedding(
        &conn,
        &model,
        "u1",
        "observation",
        "ranking bug",
        "ranking content about FTS5",
    );
    // u2 matches FTS but is far in vector space
    insert_with_embedding(
        &conn,
        &model,
        "u2",
        "observation",
        "ranking issue",
        "ranking but different context entirely",
    );

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };

    // Query embedding uses the exact same text as u1's embedding
    let query_text = "ranking bug\n\nranking content about FTS5";
    let query_vec = model.embed(&[query_text]).unwrap();

    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::knowledge(&query_vec[0]),
        &params,
        &default_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::KnowledgeHybrid);
    assert_eq!(results.results.len(), 2);
    // u1 should rank first (exact vec match + FTS match)
    assert_eq!(results.results[0].entity.id, "u1");
}

#[test]
fn graph_expansion_follows_edges() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "obs1",
        "observation",
        "FTS5 bug",
        "ranking bug in search",
    );
    insert_with_embedding(
        &conn,
        &model,
        "func1",
        "function",
        "build_sql",
        "builds the FTS search SQL",
    );
    insert_with_embedding(
        &conn,
        &model,
        "func2",
        "function",
        "search_all",
        "executes the search query",
    );

    insert_edge(&conn, "obs1", "func1", "references");
    insert_edge(&conn, "func1", "func2", "calls");

    let params = SearchParams {
        expand: true,
        max_hops: 2,
        ..Default::default()
    };
    // Floor disabled: this test covers traversal + provenance, not pruning.
    let no_floor = SearchConfig {
        min_relevance: 0.0,
        ..default_config()
    };
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &no_floor,
    )
    .unwrap();

    // Direct match: obs1
    assert!(results.results.iter().any(|r| r.entity.id == "obs1"));

    // Expanded: func1 (1-hop via references)
    let func1 = results.results.iter().find(|r| r.entity.id == "func1");
    assert!(func1.is_some());
    let func1 = func1.unwrap();
    assert_eq!(func1.graph_path, vec!["obs1", "func1"]);
    assert!(!func1.graph_path_description.is_empty());

    // Expanded: func2 (2-hop via calls)
    let func2 = results.results.iter().find(|r| r.entity.id == "func2");
    assert!(func2.is_some());
    let func2 = func2.unwrap();
    assert_eq!(func2.graph_path, vec!["obs1", "func1", "func2"]);
}

#[test]
fn min_relevance_prunes_weak_two_hop_structural_expansion() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "obs1",
        "observation",
        "FTS5 ranking bug",
        "ranking bug",
    );
    insert_with_embedding(
        &conn,
        &model,
        "func1",
        "function",
        "build_sql",
        "builds the search query",
    );
    insert_with_embedding(
        &conn,
        &model,
        "func2",
        "function",
        "search_all",
        "executes the search query",
    );

    insert_edge(&conn, "obs1", "func1", "references");
    insert_edge(&conn, "func1", "func2", "calls");

    let params = SearchParams {
        expand: true,
        max_hops: 2,
        ..Default::default()
    };
    // FTS-only: seed scores cap at ~0.5, so 1-hop references = 0.15
    // survives the 0.05 floor, but 2-hop calls = 0.5×0.09×0.5 = 0.0225
    // does not. Curated edges outrank structural at equal hops.
    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();

    let func1 = results.results.iter().find(|r| r.entity.id == "func1");
    assert!(func1.is_some());
    assert!(results.results.iter().all(|r| r.entity.id != "func2"));
    assert!(results.filtered_count >= 1);
}

#[test]
fn graph_expansion_disabled() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "obs1",
        "observation",
        "FTS5 bug",
        "ranking bug",
    );
    insert_with_embedding(
        &conn,
        &model,
        "func1",
        "function",
        "build_sql",
        "sql builder",
    );
    insert_edge(&conn, "obs1", "func1", "references");

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

    // Only direct match, no expansion
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].entity.id, "obs1");
}

#[test]
fn search_filters_by_entity_type() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "u1",
        "observation",
        "ranking",
        "ranking content",
    );
    insert_with_embedding(
        &conn,
        &model,
        "u2",
        "rule",
        "ranking rule",
        "ranking content rule",
    );

    let params = SearchParams {
        entity_type: Some("rule".to_string()),
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
    assert_eq!(results.results[0].entity.r#type, "rule");
}

#[test]
fn search_excludes_stale_by_default() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    let mut stale = Entity::new("u1", "observation", "ranking", "ranking content");
    stale.status = "stale".to_string();
    storage::crud::insert_entity(&conn, &stale).unwrap();
    let embeddings = model.embed(&["ranking\n\nranking content"]).unwrap();
    storage::embeddings::insert_embedding(&conn, "u1", "observation", &embeddings[0]).unwrap();

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
    assert!(results.results.is_empty());
}

#[test]
fn search_includes_stale_with_status_all() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    let mut stale = Entity::new("u1", "observation", "ranking", "ranking content");
    stale.status = "stale".to_string();
    storage::crud::insert_entity(&conn, &stale).unwrap();
    let embeddings = model.embed(&["ranking\n\nranking content"]).unwrap();
    storage::embeddings::insert_embedding(&conn, "u1", "observation", &embeddings[0]).unwrap();

    let params = SearchParams {
        status: Some("all".to_string()),
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
}

#[test]
fn search_no_results_for_nonexistent_query() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "u1",
        "observation",
        "Test",
        "some content here",
    );

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "nonexistent",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();
    assert!(results.results.is_empty());
}

#[test]
fn search_with_mock_embedding_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(cogz_dir.join("observations")).unwrap();
    std::fs::create_dir_all(cogz_dir.join("rules")).unwrap();
    std::fs::create_dir_all(cogz_dir.join("knowledge")).unwrap();

    let config_toml = cogz::config::default_toml("test-search");
    std::fs::write(cogz_dir.join("config.toml"), config_toml).unwrap();

    // Create entity files
    let obs = "---\nid: 550e8400-e29b-41d4-a716-446655440001\ntitle: \"FTS5 ranking bug\"\ntype: observation\nstatus: active\ncreated_at: 2026-08-28T10:00:00Z\nupdated_at: 2026-08-28T10:00:00Z\nreferences: [\"550e8400-e29b-41d4-a716-446655440002\"]\nsource: agent\n---\n\nThe RRF fusion produces incorrect rankings when k=60.\n";
    std::fs::write(cogz_dir.join("observations/test-obs.md"), obs).unwrap();

    let rule = "---\nid: 550e8400-e29b-41d4-a716-446655440002\ntitle: \"Use parameterized queries\"\ntype: rule\nstatus: active\ncreated_at: 2026-08-28T10:00:00Z\nupdated_at: 2026-08-28T10:00:00Z\nreferences: []\n---\n\nAlways use parameterized queries for FTS5 search.\n";
    std::fs::write(cogz_dir.join("rules/test-rule.md"), rule).unwrap();

    let db_path = dir.path().join(".cogz/cogz.db");
    let storage = Storage::open(&db_path, 768).unwrap();

    // Sync files to DB
    let sync_result = cogz::files::sync_all(&storage, &cogz_dir);
    assert_eq!(sync_result.created, 2);

    // Embed entities with mock model
    let model = MockEmbeddingModel::new();
    let cache = EmbeddingCache::new();
    {
        let mut conn = storage.conn();
        let entities: Vec<_> = sync_result
            .synced_entity_ids
            .iter()
            .filter_map(|id| storage::crud::get_entity(&conn, id).ok())
            .collect();
        let embeddings = cogz::files::embed_sync::embed_entities(&model, &cache, &entities);
        cogz::files::embed_sync::store_embeddings(&mut conn, &embeddings);
    }

    // Search
    let config = cogz::config::load(&cogz_dir.join("config.toml")).unwrap();
    let params = SearchParams {
        expand: true,
        max_hops: 2,
        ..Default::default()
    };

    let results = {
        let conn = storage.conn();
        search(
            &conn,
            "ranking",
            QueryEmbeddings::none(),
            &params,
            &config.search,
        )
        .unwrap()
    };

    // Should find the observation
    assert!(!results.results.is_empty());
    let obs_result = results
        .results
        .iter()
        .find(|r| r.entity.title.as_deref() == Some("FTS5 ranking bug"));
    assert!(obs_result.is_some());

    // Graph expansion should find the rule (referenced by the observation)
    let rule_result = results
        .results
        .iter()
        .find(|r| r.entity.title.as_deref() == Some("Use parameterized queries"));
    assert!(rule_result.is_some());
    let rule_result = rule_result.unwrap();
    assert!(!rule_result.graph_path.is_empty());
}

#[test]
fn rrf_fusion_produces_sensible_ordering() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    // Three entities, all matching FTS for "ranking"
    insert_with_embedding(
        &conn,
        &model,
        "u1",
        "observation",
        "ranking first",
        "ranking content A",
    );
    insert_with_embedding(
        &conn,
        &model,
        "u2",
        "observation",
        "ranking second",
        "ranking content B",
    );
    insert_with_embedding(
        &conn,
        &model,
        "u3",
        "observation",
        "ranking third",
        "ranking content C",
    );

    let params = SearchParams {
        expand: false,
        ..Default::default()
    };

    // Query embedding matches u3's full text (title + content)
    let query_vec = model
        .embed(&["ranking third\n\nranking content C"])
        .unwrap();

    let results = search(
        &conn,
        "ranking",
        QueryEmbeddings::knowledge(&query_vec[0]),
        &params,
        &default_config(),
    )
    .unwrap();

    assert_eq!(results.search_mode, SearchMode::KnowledgeHybrid);
    assert_eq!(results.results.len(), 3);

    // u3 should rank first: it's rank 1 in vec (exact match) and rank 3 in FTS.
    // u1 should rank second: it's rank 1 in FTS but rank 3 in vec.
    // With vec_weight=0.6 > fts_weight=0.4, u3 wins.
    assert_eq!(results.results[0].entity.id, "u3");
}

#[test]
fn search_limit_truncates_results() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    for i in 0..10 {
        insert_with_embedding(
            &conn,
            &model,
            &format!("u{i}"),
            "observation",
            &format!("ranking {i}"),
            &format!("ranking content {i}"),
        );
    }

    let params = SearchParams {
        limit: 3,
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

    assert_eq!(results.results.len(), 3);
}

#[test]
fn search_expansion_deduplicates_across_seeds() {
    let storage = setup_storage();
    let conn = storage.conn();

    // Two seeds that both reference the same target.
    // Without dedup, the target appears twice (once per seed).
    insert_with_embedding(
        &conn,
        &MockEmbeddingModel::new(),
        "seed1",
        "observation",
        "ranking bug one",
        "ranking content one",
    );
    insert_with_embedding(
        &conn,
        &MockEmbeddingModel::new(),
        "seed2",
        "observation",
        "ranking bug two",
        "ranking content two",
    );
    insert_with_embedding(
        &conn,
        &MockEmbeddingModel::new(),
        "shared_target",
        "knowledge",
        "Shared knowledge",
        "shared content",
    );

    // Both seeds reference the same target
    use cogz::storage::edges::{Edge, insert_edge};
    let now = chrono::Utc::now().to_rfc3339();
    insert_edge(
        &conn,
        &Edge {
            source_id: "seed1".to_string(),
            target_id: "shared_target".to_string(),
            edge_type: "references".to_string(),
            weight: 1.0,
            created_at: now.clone(),
        },
    )
    .unwrap();
    insert_edge(
        &conn,
        &Edge {
            source_id: "seed2".to_string(),
            target_id: "shared_target".to_string(),
            edge_type: "references".to_string(),
            weight: 1.0,
            created_at: now,
        },
    )
    .unwrap();

    let params = SearchParams {
        limit: 20,
        expand: true,
        max_hops: 2,
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

    // shared_target should appear exactly once, not twice
    let shared_count = results
        .results
        .iter()
        .filter(|r| r.entity.id == "shared_target")
        .count();
    assert_eq!(
        shared_count, 1,
        "expanded entity reachable from multiple seeds appears only once"
    );
}

#[test]
fn coverage_gate_silences_unanswerable_query() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "p1",
        "function",
        "parse_args",
        "fn parse_args() { tokenize the command line into flags }",
    );
    insert_with_embedding(
        &conn,
        &model,
        "p2",
        "function",
        "build_command",
        "fn build_command() { assemble the clap Command builder }",
    );

    // Query vocabulary absent from the corpus → coverage 0 → silence,
    // even though KNN would happily return the nearest neighbors.
    let params = SearchParams {
        limit: 20,
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "quantum entanglement routing protocol",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();
    assert!(
        results.results.is_empty(),
        "uncovered query must silence, got {} results",
        results.results.len()
    );
    assert_eq!(results.signals.and_then(|s| s.answer_coverage), Some(0.0));
}

#[test]
fn coverage_gate_passes_answerable_query() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "p1",
        "function",
        "parse_args",
        "fn parse_args() { tokenize the command line into flags }",
    );

    let params = SearchParams {
        limit: 20,
        expand: false,
        ..Default::default()
    };
    let results = search(
        &conn,
        "tokenize command line flags",
        QueryEmbeddings::none(),
        &params,
        &default_config(),
    )
    .unwrap();
    assert!(
        !results.results.is_empty(),
        "covered query must return results"
    );
    let cov = results.signals.and_then(|s| s.answer_coverage).unwrap();
    assert!(cov >= 0.45, "covered query coverage {cov} below floor");
}

#[test]
fn coverage_gate_disabled_at_zero_floor() {
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::new();

    insert_with_embedding(
        &conn,
        &model,
        "p1",
        "knowledge",
        "rust parser",
        "fn parse_args() { tokenize the command line into flags }",
    );

    let mut config = default_config();
    config.silence_coverage_floor = 0.0;
    let params = SearchParams {
        limit: 20,
        expand: false,
        ..Default::default()
    };
    // KNN must produce results the coverage gate would kill — embed
    // the entity's own text so the query vector matches it while the
    // query terms stay absent from the corpus.
    let query_vec = model
        .embed(&["rust parser\n\nfn parse_args() { tokenize the command line into flags }"])
        .unwrap();
    let results = search(
        &conn,
        "quantum entanglement routing protocol",
        QueryEmbeddings::knowledge(&query_vec[0]),
        &params,
        &config,
    )
    .unwrap();
    assert!(
        !results.results.is_empty(),
        "floor 0.0 must disable the coverage gate"
    );
}

#[test]
fn test_file_predicates_agree() {
    // One predicate, three call sites: the SQL scalar, the FTS
    // exclude_tests filter, and the KNN exclude_tests filter must
    // all agree with is_test_file. LIKE wildcards previously let
    // `latest.go`/`testing.py`/`contest.py` slip through as tests.
    let storage = setup_storage();
    let conn = storage.conn();
    let model = MockEmbeddingModel::default();

    let paths = [
        // Production paths the LIKE wildcards over-matched
        ("src/latest.go", false),
        ("src/testing.py", false),
        ("src/contest.py", false),
        ("src/test_utils.rs", false),
        ("src/protest.go", false),
        ("src/contestant_test.go", true),
        // Real test paths per convention
        ("tests/test_foo.rs", true),
        ("tests.rs", true),
        ("src/foo_tests.rs", true),
        ("foo_test.go", true),
        ("pkg/sub/foo_test.go", true),
        ("test_x.py", true),
        ("x_test.py", true),
        ("pkg/sub/test_bar.py", true),
        ("pkg/sub/baz_test.py", true),
        ("a/__tests__/b.ts", true),
        ("src/app.test.js", true),
        ("src/app.spec.tsx", true),
        ("src/app.test.mjs", true),
        ("scripts/test_deploy.sh", true),
        ("scripts/check_test.bash", true),
        // Non-test lookalikes
        ("src/main.rs", false),
        ("src/lib.go", false),
        ("app.py", false),
        ("src/utils.ts", false),
        ("scripts/deploy.sh", false),
        ("tests_readme.md", false),
    ];

    // 1. The SQL scalar agrees with the Rust predicate on every path.
    for (path, expected) in &paths {
        let got: i64 = conn
            .query_row("SELECT is_test_path(?)", [path], |r| r.get(0))
            .unwrap();
        assert_eq!(got != 0, *expected, "is_test_path disagrees on {path}");
        assert_eq!(
            cogz::index::is_test_file(path),
            *expected,
            "is_test_file disagrees on {path}"
        );
    }

    // 2. FTS + KNN exclusion: insert a production and a test entity
    // per language, then exclude_tests must return production only.
    let corpus = [
        ("p1", "src/latest.go", false),
        ("p2", "src/testing.py", false),
        ("p3", "src/main.rs", false),
        ("t1", "foo_test.go", true),
        ("t2", "test_x.py", true),
        ("t3", "tests/test_bar.rs", true),
    ];
    for (id, file_path, _) in &corpus {
        let mut e = Entity::new(id, "function", "handler", "alpha beta gamma");
        e.file_path = Some(file_path.to_string());
        storage::crud::insert_entity(&conn, &e).unwrap();
        let emb = model.embed(&["alpha beta gamma"]).unwrap();
        storage::embeddings::insert_embedding(&conn, id, "function", &emb[0]).unwrap();
    }

    let fts = storage::query::fts_search(
        &conn,
        "alpha",
        Some("function"),
        Some("active"),
        true,
        50,
        1.0,
    )
    .unwrap();
    let fts_paths: Vec<_> = fts.iter().filter_map(|e| e.file_path.clone()).collect();
    for (_, path, is_test) in &corpus {
        assert_eq!(
            fts_paths.iter().any(|p| p == path),
            !is_test,
            "FTS exclusion wrong for {path}"
        );
    }

    let q = model.embed(&["alpha beta gamma"]).unwrap();
    let knn = storage::embeddings::knn_search_with_filters(
        &conn,
        storage::embeddings::EmbeddingSpace::Code,
        &q[0],
        50,
        Some("function"),
        Some("active"),
        true,
    )
    .unwrap();
    let knn_ids: Vec<_> = knn.iter().map(|(id, _)| id.as_str()).collect();
    for (id, _, is_test) in &corpus {
        assert_eq!(
            knn_ids.contains(id),
            !is_test,
            "KNN exclusion wrong for {id}"
        );
    }
}
