//! Integration tests for context assembly.

use cogz::config::Config;
use cogz::context::{AssembleParams, ContextMode, assemble_context};
use cogz::storage::Storage;
use cogz::storage::crud::{Entity, insert_entity};
use cogz::storage::edges::{Edge, insert_edge};
use cogz::storage::embeddings::insert_embedding;

fn default_config() -> Config {
    Config::default_for("test")
}

fn edge(source: &str, target: &str) -> Edge {
    Edge {
        source_id: source.to_string(),
        target_id: target.to_string(),
        edge_type: "references".to_string(),
        weight: 1.0,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[test]
fn cold_start_produces_compact_pack() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("r1", "rule", "Rule A", "rule content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("r2", "rule", "Rule B", "rule content b"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "Obs A", "obs content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("k1", "knowledge", "Knowledge A", "knowledge content"),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert_eq!(pack.mode, ContextMode::ColdStart);
    // Identity + 2 rules + 1 knowledge (top scored) + knowledge index
    assert!(pack.sections.iter().any(|s| s.source == "identity"));
    assert!(pack.sections.iter().any(|s| s.source == "rule"));
    // Cold start now includes scored knowledge, not raw observations
    assert!(pack.sections.iter().any(|s| s.source == "knowledge"));
    // Observations are intentionally excluded from cold start — they're
    // often noisy and unvalidated
    assert!(!pack.sections.iter().any(|s| s.source == "observation"));
    assert_eq!(pack.query, "");
}

#[test]
fn task_mode_produces_query_scoped_pack() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new(
            "o1",
            "observation",
            "FTS5 ranking bug",
            "ranking bug content",
        ),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new(
            "r1",
            "rule",
            "Use parameterized queries",
            "always parameterize",
        ),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert_eq!(pack.mode, ContextMode::Task);
    assert_eq!(pack.query, "ranking");
    assert!(!pack.sections.is_empty());
    assert!(pack.sections.iter().any(|s| s.entity_id == "o1"));
}

#[test]
fn task_mode_includes_graph_paths() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "FTS5 bug", "ranking bug content"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("k1", "knowledge", "Search design", "how search works"),
    )
    .unwrap();
    insert_edge(&conn, &edge("o1", "k1")).unwrap();

    // graph_first off: this test covers decayed-expansion provenance
    // in packs. With graph-first on, k1 enters as a direct hit
    // (path ["k1"]) — the behavior this asserts against.
    let mut config = default_config();
    config.search.graph_first_enabled = false;
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    let o1 = pack.sections.iter().find(|s| s.entity_id == "o1");
    assert!(o1.is_some());
    assert_eq!(o1.unwrap().graph_path, vec!["o1"]);

    let k1 = pack.sections.iter().find(|s| s.entity_id == "k1");
    assert!(k1.is_some());
    assert_eq!(k1.unwrap().graph_path, vec!["o1", "k1"]);
}

#[test]
fn escalation_produces_wider_pack() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    for i in 0..15 {
        insert_entity(
            &conn,
            &Entity::new(
                &format!("o{i}"),
                "observation",
                &format!("Item {i}"),
                "ranking content",
            ),
        )
        .unwrap();
    }

    let config = default_config();
    let task_params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let esc_params = AssembleParams {
        mode: ContextMode::Escalation,
        query: Some("ranking"),
        ..Default::default()
    };
    let task_pack = assemble_context(&conn, &task_params, &config).unwrap();
    let esc_pack = assemble_context(&conn, &esc_params, &config).unwrap();

    assert!(esc_pack.sections.len() >= task_pack.sections.len());
}

#[test]
fn token_budget_is_respected() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    for i in 0..10 {
        let big_content = format!("rule body {i} {}", "x".repeat(2000));
        insert_entity(
            &conn,
            &Entity::new(&format!("r{i}"), "rule", &format!("Rule {i}"), &big_content),
        )
        .unwrap();
    }

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        max_tokens: Some(100),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert!(pack.metadata.size_tokens <= 100);
    assert!(!pack.metadata.dropped_sources.is_empty());
}

#[test]
fn dropped_sources_are_listed() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    let big_content = format!("first body {}", "x".repeat(2000));
    let big_content2 = format!("second body {}", "y".repeat(2000));
    insert_entity(&conn, &Entity::new("r1", "rule", "R1", &big_content)).unwrap();
    insert_entity(&conn, &Entity::new("r2", "rule", "R2", &big_content2)).unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        max_tokens: Some(50),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert!(!pack.metadata.dropped_sources.is_empty());
    for src in &pack.metadata.dropped_sources {
        assert!(src.contains("over token budget"));
    }
}

#[test]
fn task_mode_without_query_returns_error() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: None,
        ..Default::default()
    };
    let result = assemble_context(&conn, &params, &config);
    assert!(result.is_err());
}

#[test]
fn cold_start_respects_config_limits() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    for i in 0..10 {
        insert_entity(
            &conn,
            &Entity::new(
                &format!("r{i}"),
                "rule",
                &format!("Rule {i}"),
                &format!("rule body {i} distinct"),
            ),
        )
        .unwrap();
    }

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    let rule_count = pack.sections.iter().filter(|s| s.source == "rule").count();
    assert_eq!(rule_count, 5);
}

#[test]
fn include_stale_includes_stale_entities() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    let mut e1 = Entity::new("r1", "rule", "Stale rule", "stale rule body");
    e1.status = "stale".to_string();
    insert_entity(&conn, &e1).unwrap();
    insert_entity(
        &conn,
        &Entity::new("r2", "rule", "Active rule", "active rule body"),
    )
    .unwrap();

    let config = default_config();

    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();
    assert!(!pack.sections.iter().any(|s| s.entity_id == "r1"));

    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        include_stale: true,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();
    assert!(pack.sections.iter().any(|s| s.entity_id == "r1"));
}

#[test]
fn fts_only_search_mode_in_metadata() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(&conn, &Entity::new("r1", "rule", "Rule", "content")).unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert_eq!(pack.metadata.search_mode, "fts_only");
}

#[test]
fn selected_sources_lists_unique_source_types() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(&conn, &Entity::new("r1", "rule", "R1", "content")).unwrap();
    insert_entity(&conn, &Entity::new("r2", "rule", "R2", "content")).unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::ColdStart,
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    // selected_sources contains unique source types, not per-section entries.
    // With 2 rules + identity, sections = 3 but source types = 2 (identity + rule).
    assert!(
        pack.metadata
            .selected_sources
            .contains(&"identity".to_string())
    );
    assert!(pack.metadata.selected_sources.contains(&"rule".to_string()));
    assert_eq!(
        pack.metadata.selected_sources.len(),
        2,
        "expected deduplicated source types, got {:?}",
        pack.metadata.selected_sources
    );
}

#[test]
fn task_pack_includes_tier0_baseline() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("r1", "rule", "Rule A", "rule content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "FTS5 bug", "ranking bug content"),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    use cogz::context::DeliveryTier;
    // Tier 0: identity + rule ship as baseline alongside search hits.
    let identity = pack.sections.iter().find(|s| s.source == "identity");
    assert!(identity.is_some(), "task pack should carry orientation");
    assert_eq!(identity.unwrap().tier, DeliveryTier::Baseline);
    let baseline_rule = pack
        .sections
        .iter()
        .find(|s| s.entity_id == "r1" && s.tier == DeliveryTier::Baseline);
    assert!(baseline_rule.is_some());
    // Tier 1: the search hit ships full content.
    let hit = pack.sections.iter().find(|s| s.entity_id == "o1");
    assert_eq!(hit.unwrap().tier, DeliveryTier::Full);
}

#[test]
fn task_pack_baseline_survives_tight_budget() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(&conn, &Entity::new("r1", "rule", "Short rule", "short")).unwrap();
    let big = format!("big body {}", "x".repeat(2000));
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "ranking hit", &big),
    )
    .unwrap();

    let config = default_config();
    // Budget smaller than the search hit — Tier 0 still ships and the
    // oversized hit is truncated into what remains rather than pushing
    // the baseline out.
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        max_tokens: Some(120),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    use cogz::context::DeliveryTier;
    assert!(pack.sections.iter().any(|s| s.source == "identity"));
    assert!(
        pack.sections
            .iter()
            .any(|s| s.entity_id == "r1" && s.tier == DeliveryTier::Baseline)
    );
    let hit = pack.sections.iter().find(|s| s.entity_id == "o1");
    assert!(hit.is_some());
    assert!(hit.unwrap().content.len() < 2000);
}

#[test]
fn dedup_loser_keeps_pointer_index_entry() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    // Identical content — the dedup loser should still be discoverable.
    let body = format!("ranking investigation {}", "z".repeat(400));
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "first finding", &body),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o2", "observation", "second finding", &body),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    use cogz::context::DeliveryTier;
    let idx = pack.sections.iter().find(|s| s.source == "overflow_index");
    assert!(idx.is_some(), "dedup loser should keep a pointer entry");
    assert_eq!(idx.unwrap().tier, DeliveryTier::Pointer);
    assert!(pack.metadata.pointer_ids.contains(&"o2".to_string()));
}

#[test]
fn tiered_push_disabled_omits_baseline() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("r1", "rule", "Rule A", "rule content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "FTS5 bug", "ranking bug content"),
    )
    .unwrap();

    let mut config = default_config();
    config.context.tiered_push = false;
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert!(!pack.sections.iter().any(|s| s.source == "identity"));
    assert!(pack.sections.iter().any(|s| s.entity_id == "o1"));
    // r1 was not search-relevant, so it must not appear at all.
    assert!(!pack.sections.iter().any(|s| s.entity_id == "r1"));
}

#[test]
fn task_pack_with_no_search_hits_is_orientation_only() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("r1", "rule", "Rule A", "rule content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "unrelated", "nothing matching"),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("zzqqxy nonexistent token"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    use cogz::context::DeliveryTier;
    assert!(pack.sections.iter().any(|s| s.source == "identity"));
    assert!(!pack.sections.iter().any(|s| s.tier == DeliveryTier::Full));
    assert!(!pack.sections.iter().any(|s| s.entity_id == "o1"));
}

#[test]
fn task_pack_ships_semantic_hits_below_silence_confidence() {
    // Embeddings orthogonal to the query produce silence-gate-level
    // signals — pack assembly ships them anyway; silence is a
    // search-surface semantic, not a delivery policy.
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    for i in 0..3 {
        let id = format!("k{i}");
        insert_entity(
            &conn,
            &Entity::new(
                &id,
                "knowledge",
                &format!("note {i}"),
                ["alpha", "beta", "gamma"][i],
            ),
        )
        .unwrap();
        let mut v = vec![0.0_f32; 768];
        v[1] = 1.0;
        v[2] = i as f32 * 1e-4;
        insert_embedding(&conn, &id, "knowledge", &v).unwrap();
    }
    let mut qv = vec![0.0_f32; 768];
    qv[0] = 1.0;

    // Floor at zero isolates the silence gate: anything retrieval
    // returns must reach the pack.
    let mut config = default_config();
    config.search.min_relevance = 0.0;
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("qqqzzz"),
        knowledge_embedding: Some(&qv),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();
    assert!(pack.sections.iter().any(|s| s.entity_id == "k0"));
    assert!(pack.metadata.signals.is_some());
}

#[test]
fn escalation_pack_has_baseline() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();
    insert_entity(
        &conn,
        &Entity::new("r1", "rule", "Rule A", "rule content a"),
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity::new("o1", "observation", "FTS5 bug", "ranking bug content"),
    )
    .unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Escalation,
        query: Some("ranking"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    assert!(pack.sections.iter().any(|s| s.source == "identity"));
}

#[test]
fn code_entities_are_summarized_in_task_mode() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();

    // A function entity with 20+ lines of content
    let long_body = (1..=25)
        .map(|i| format!("    let x{i} = compute{i}();"))
        .collect::<Vec<_>>()
        .join("\n");
    let func_content = format!("pub fn big_function() -> Result<()> {{\n{long_body}\n}}");

    let mut func = Entity::new("f1", "function", "big_function", &func_content);
    func.file_path = Some("src/main.rs".to_string());
    insert_entity(&conn, &func).unwrap();

    let config = default_config();
    // Tight budget: no headroom for the relax pass, so the code
    // section must stay excerpted.
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("big_function"),
        max_tokens: Some(60),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    let func_section = pack
        .sections
        .iter()
        .find(|s| s.entity_id == "f1")
        .expect("function should be in pack");

    assert!(
        func_section.content.len() < func_content.len(),
        "expected excerpted content under tight budget"
    );
    assert!(func_section.content.contains("pub fn big_function"));
}

#[test]
fn code_entities_expand_into_budget_headroom() {
    let storage = Storage::open_memory().unwrap();
    let conn = storage.conn();

    let long_body = (1..=25)
        .map(|i| format!("    let x{i} = compute{i}();"))
        .collect::<Vec<_>>()
        .join("\n");
    let func_content = format!("pub fn big_function() -> Result<()> {{\n{long_body}\n}}");

    let mut func = Entity::new("f1", "function", "big_function", &func_content);
    func.file_path = Some("src/main.rs".to_string());
    insert_entity(&conn, &func).unwrap();

    let config = default_config();
    let params = AssembleParams {
        mode: ContextMode::Task,
        query: Some("big_function"),
        ..Default::default()
    };
    let pack = assemble_context(&conn, &params, &config).unwrap();

    let func_section = pack
        .sections
        .iter()
        .find(|s| s.entity_id == "f1")
        .expect("function should be in pack");

    // Spare budget is spent growing the section to full content.
    assert_eq!(func_section.content, func_content);
}
