//! Integration tests for Phase 11 hooks.
//!
//! Tests cover:
//! - session_start records event and returns cold_start context pack
//! - prompt_submit records event and returns task context pack
//! - pre_tool_use records event, no context pack, no observation
//! - post_tool_use records event and observation when both fields present
//! - post_tool_use records event only when fields missing
//! - Events are queryable from the DB
//! - Invalid event type returns error

use std::sync::Arc;

use cogz::config::Config;
use cogz::embed::{ModelType, OnnxEmbeddingModel};
use cogz::hooks::lifecycle::{LifecycleEvent, LifecycleInput, handle_lifecycle_event};
use cogz::storage::Storage;
use cogz::storage::events::get_recent_events;

fn setup() -> (Arc<Storage>, Config, std::path::PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cogz_dir = dir.path().join(".cogz");
    std::fs::create_dir_all(&cogz_dir).unwrap();
    std::fs::create_dir_all(cogz_dir.join("observations")).unwrap();
    std::fs::create_dir_all(cogz_dir.join("rules")).unwrap();
    std::fs::create_dir_all(cogz_dir.join("knowledge")).unwrap();

    let storage = Arc::new(Storage::open_memory().unwrap());
    let config = Config::default_for("test-hooks");
    (storage, config, cogz_dir, dir)
}

fn query_model(config: &Config) -> OnnxEmbeddingModel {
    let models_dir = cogz::embed::models_dir();
    OnnxEmbeddingModel::with_model_id(
        ModelType::Knowledge,
        &models_dir,
        config.embedding.dimension,
        &config.embedding.knowledge_model,
    )
}

fn code_model(config: &Config) -> OnnxEmbeddingModel {
    let models_dir = cogz::embed::models_dir();
    OnnxEmbeddingModel::with_model_id(
        ModelType::Code,
        &models_dir,
        config.embedding.dimension,
        &config.embedding.code_model,
    )
}

#[test]
fn session_start_records_event_and_returns_cold_start_pack() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::SessionStart,
            prompt: None,
            tool_name: None,
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    let pack = output
        .context_pack
        .expect("session_start should return a pack");
    assert_eq!(pack.mode, cogz::context::ContextMode::ColdStart);
    assert!(output.observation_id.is_none());

    let conn = storage.conn();
    let events = get_recent_events(&conn, "session_start", 10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "session_start");
}

#[test]
fn prompt_submit_records_event_and_returns_task_pack() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    // Need some entities for task mode to find.
    insert_test_observation(&storage, "Test knowledge", "Some content about testing");

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::PromptSubmit,
            prompt: Some("testing"),
            tool_name: None,
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    let pack = output
        .context_pack
        .expect("prompt_submit should return a pack");
    assert_eq!(pack.mode, cogz::context::ContextMode::Task);
    assert_eq!(pack.query, "testing");
    assert!(output.observation_id.is_none());

    let conn = storage.conn();
    let events = get_recent_events(&conn, "prompt_submit", 10).unwrap();
    assert_eq!(events.len(), 1);
    // Pack metadata is persisted onto the event for later analysis.
    let pack_meta = &events[0].payload["pack"];
    assert!(pack_meta["size_tokens"].is_number());
    assert!(pack_meta["sections"].is_number());
    assert!(pack_meta["pointers"].is_number());
    assert_eq!(
        pack_meta["search_mode"].as_str().unwrap(),
        pack.metadata.search_mode
    );
}

#[test]
fn pre_tool_use_records_event_only() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::PreToolUse,
            prompt: None,
            tool_name: Some("edit_file"),
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    assert!(
        output.context_pack.is_none(),
        "pre_tool_use should not return a pack"
    );
    assert!(
        output.observation_id.is_none(),
        "pre_tool_use should not record an observation"
    );

    let conn = storage.conn();
    let events = get_recent_events(&conn, "pre_tool_use", 10).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn post_tool_use_records_event_but_not_observation() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::PostToolUse,
            prompt: None,
            tool_name: Some("run_tests"),
            tool_result: Some("329 passed, 0 failed"),
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    assert!(output.context_pack.is_none());
    // post_tool_use no longer auto-records observations — the agent
    // decides what's salient via the create_entity MCP tool.
    assert!(
        output.observation_id.is_none(),
        "post_tool_use should not auto-record an observation"
    );

    // No observation file should be created.
    let obs_dir = cogz_dir.join("observations");
    if obs_dir.exists() {
        let obs_files: Vec<_> = std::fs::read_dir(&obs_dir)
            .unwrap()
            .flatten()
            .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
            .collect();
        assert!(obs_files.is_empty(), "no observation files should exist");
    }

    // The event should still be recorded.
    let conn = storage.conn();
    let events = get_recent_events(&conn, "post_tool_use", 10).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn post_tool_use_without_tool_result_records_event_only() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::PostToolUse,
            prompt: None,
            tool_name: Some("edit_file"),
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    assert!(
        output.observation_id.is_none(),
        "post_tool_use should not auto-record an observation"
    );
}

#[test]
fn lifecycle_event_parse_roundtrip() {
    assert_eq!(
        LifecycleEvent::parse("session_start"),
        Some(LifecycleEvent::SessionStart)
    );
    assert_eq!(
        LifecycleEvent::parse("prompt_submit"),
        Some(LifecycleEvent::PromptSubmit)
    );
    assert_eq!(
        LifecycleEvent::parse("pre_tool_use"),
        Some(LifecycleEvent::PreToolUse)
    );
    assert_eq!(
        LifecycleEvent::parse("post_tool_use"),
        Some(LifecycleEvent::PostToolUse)
    );
    assert_eq!(
        LifecycleEvent::parse("file_save"),
        Some(LifecycleEvent::FileSave)
    );
    assert_eq!(
        LifecycleEvent::parse("session_end"),
        Some(LifecycleEvent::SessionEnd)
    );
    assert_eq!(LifecycleEvent::parse("invalid"), None);
}

#[test]
fn file_save_for_cogz_file_triggers_sync_not_reindex() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::FileSave,
            prompt: None,
            tool_name: None,
            tool_result: None,
            file_path: Some(".cogz/knowledge/test.md"),
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    assert!(output.context_pack.is_none());
    assert!(output.observation_id.is_none());
    let summary = output
        .reindex_summary
        .expect("file_save should return a summary");
    assert!(
        !summary.reindexed,
        ".cogz/ files should not trigger code reindex"
    );
    assert!(summary.synced, ".cogz/ files should trigger file sync");

    let conn = storage.conn();
    let events = get_recent_events(&conn, "file_save", 10).unwrap();
    assert_eq!(events.len(), 1);
}

#[test]
fn file_save_without_path_records_event_only() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::FileSave,
            prompt: None,
            tool_name: None,
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    let summary = output
        .reindex_summary
        .expect("file_save should return a summary");
    assert!(!summary.reindexed);
    assert!(!summary.synced);
}

#[test]
fn session_end_records_event_and_runs_consolidation() {
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let output = handle_lifecycle_event(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::SessionEnd,
            prompt: None,
            tool_name: None,
            tool_result: None,
            file_path: None,
        },
    )
    .unwrap();

    assert!(output.event_id.is_some_and(|id| id > 0));
    assert!(output.context_pack.is_none());
    assert!(output.observation_id.is_none());
    assert!(
        output.reindex_summary.is_none(),
        "session_end should not trigger reindex"
    );
    let summary = output
        .consolidation_summary
        .expect("session_end should return a consolidation summary");
    // No entities meet promotion/merge thresholds, but consolidation runs.
    assert_eq!(summary.promotions, 0);
    assert_eq!(summary.merges, 0);

    let conn = storage.conn();
    let events = get_recent_events(&conn, "session_end", 10).unwrap();
    assert_eq!(events.len(), 1);
}

fn insert_test_observation(storage: &Storage, title: &str, content: &str) {
    use cogz::storage::crud::{Entity, insert_entity};
    let entity = Entity::new(
        &uuid::Uuid::new_v4().to_string(),
        "observation",
        title,
        content,
    );
    let conn = storage.conn();
    insert_entity(&conn, &entity).unwrap();
}

#[test]
fn usage_tracking_delivers_hits_and_misses() {
    use cogz::storage::usage;
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    // The cold-start pack always includes the top-scored knowledge
    // entry — a knowledge entity is a guaranteed delivery without
    // depending on search/model behavior.
    let id = uuid::Uuid::new_v4().to_string();
    {
        use cogz::storage::crud::{Entity, insert_entity};
        let mut entity = Entity::new(
            &id,
            "knowledge",
            "usage tracking entity",
            "content about usage tracking",
        );
        entity.file_path = Some("knowledge/usage-tracking.md".to_string());
        let conn = storage.conn();
        insert_entity(&conn, &entity).unwrap();
    }

    let fire = |input: LifecycleInput| {
        handle_lifecycle_event(
            &storage, &config, &cogz_dir, &model, &code_mod, None, &input,
        )
        .unwrap()
    };

    let pack = fire(LifecycleInput {
        event: LifecycleEvent::SessionStart,
        prompt: None,
        tool_name: None,
        tool_result: None,
        file_path: None,
    })
    .context_pack
    .expect("pack");

    let delivered = pack.sections.iter().any(|s| s.entity_id == id);
    assert!(delivered, "test entity must be in the cold-start pack");

    // Touch the entity's file — an absolute path inside .cogz/.
    let abs_path = cogz_dir.join("knowledge/usage-tracking.md");
    fire(LifecycleInput {
        event: LifecycleEvent::PostToolUse,
        prompt: None,
        tool_name: Some("read_file"),
        tool_result: Some("ok"),
        file_path: Some(abs_path.to_str().unwrap()),
    });

    {
        let conn = storage.conn();
        let outcome: String = conn
            .query_row(
                "SELECT outcome FROM entity_usage WHERE entity_id = ?",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "hit");
    }

    // Next pack closes the previous delivery — untouched entities miss.
    fire(LifecycleInput {
        event: LifecycleEvent::PromptSubmit,
        prompt: Some("something else entirely"),
        tool_name: None,
        tool_result: None,
        file_path: None,
    });

    {
        let conn = storage.conn();
        let pack_summary = usage::usage_summary(&conn, Some(usage::DeliveryKind::Pack)).unwrap();
        assert!(pack_summary.hits >= 1);
        assert_eq!(pack_summary.pending, 0);
    }

    // session_end closes the remaining open delivery.
    fire(LifecycleInput {
        event: LifecycleEvent::SessionEnd,
        prompt: None,
        tool_name: None,
        tool_result: None,
        file_path: None,
    });
    let conn = storage.conn();
    let open: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deliveries WHERE closed = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 0);
}

#[test]
fn file_save_marks_delivered_entity_hit() {
    use cogz::storage::crud::{Entity, insert_entity};
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let id = uuid::Uuid::new_v4().to_string();
    let mut entity = Entity::new(&id, "knowledge", "file save hit entity", "body");
    entity.file_path = Some("knowledge/file-save-hit.md".to_string());
    {
        let conn = storage.conn();
        insert_entity(&conn, &entity).unwrap();
    }

    let fire = |input: LifecycleInput| {
        handle_lifecycle_event(
            &storage, &config, &cogz_dir, &model, &code_mod, None, &input,
        )
        .unwrap()
    };

    let pack = fire(LifecycleInput {
        event: LifecycleEvent::SessionStart,
        prompt: None,
        tool_name: None,
        tool_result: None,
        file_path: None,
    })
    .context_pack
    .expect("pack");
    assert!(pack.sections.iter().any(|s| s.entity_id == id));

    // Hosts without post_tool_use dispatch still send file_save on
    // edit/write — the path match must credit the delivered entity.
    let abs_path = cogz_dir.join("knowledge/file-save-hit.md");
    fire(LifecycleInput {
        event: LifecycleEvent::FileSave,
        prompt: None,
        tool_name: None,
        tool_result: None,
        file_path: Some(abs_path.to_str().unwrap()),
    });

    let conn = storage.conn();
    let outcome: String = conn
        .query_row(
            "SELECT outcome FROM entity_usage WHERE entity_id = ?",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "hit");
}

#[test]
fn prompt_submit_resolves_hits_before_boundary() {
    use cogz::storage::crud::{Entity, insert_entity};
    let (storage, config, cogz_dir, _dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let id = uuid::Uuid::new_v4().to_string();
    let mut entity = Entity::new(&id, "knowledge", "prompt referenced knowledge", "body");
    entity.file_path = Some("knowledge/prompt-ref.md".to_string());
    {
        let conn = storage.conn();
        insert_entity(&conn, &entity).unwrap();
    }

    let fire = |input: LifecycleInput| {
        handle_lifecycle_event(
            &storage, &config, &cogz_dir, &model, &code_mod, None, &input,
        )
        .unwrap()
    };

    let pack = fire(LifecycleInput {
        event: LifecycleEvent::SessionStart,
        prompt: None,
        tool_name: None,
        tool_result: None,
        file_path: None,
    })
    .context_pack
    .expect("pack");
    assert!(pack.sections.iter().any(|s| s.entity_id == id));

    // A prompt naming the delivered entity's file is evidence the
    // context was used — it must resolve BEFORE this prompt's pack
    // closes the prior delivery as misses.
    fire(LifecycleInput {
        event: LifecycleEvent::PromptSubmit,
        prompt: Some("now update knowledge/prompt-ref.md please"),
        tool_name: None,
        tool_result: None,
        file_path: None,
    });

    let conn = storage.conn();
    let outcome: String = conn
        .query_row(
            "SELECT outcome FROM entity_usage WHERE entity_id = ? AND delivery_id = \
             (SELECT MIN(delivery_id) FROM entity_usage WHERE entity_id = ?)",
            [&id, &id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "hit");
}

/// Fire a file_save lifecycle event for `rel_path` under the fixture
/// repo and return the output.
fn fire_file_save(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &std::path::Path,
    model: &OnnxEmbeddingModel,
    code_mod: &OnnxEmbeddingModel,
    rel_path: &str,
) -> cogz::hooks::lifecycle::LifecycleOutput {
    handle_lifecycle_event(
        storage,
        config,
        cogz_dir,
        model,
        code_mod,
        None,
        &LifecycleInput {
            event: LifecycleEvent::FileSave,
            prompt: None,
            tool_name: None,
            tool_result: None,
            file_path: Some(rel_path),
        },
    )
    .unwrap()
}

/// Write a source file under the fixture repo root and save it once
/// so the code index holds its entities. Returns the `file` entity id
/// the reindex created for it.
#[allow(clippy::too_many_arguments)]
fn index_source_file(
    storage: &Arc<Storage>,
    config: &Config,
    cogz_dir: &std::path::Path,
    repo_root: &std::path::Path,
    model: &OnnxEmbeddingModel,
    code_mod: &OnnxEmbeddingModel,
    rel_path: &str,
    contents: &str,
) -> String {
    let abs = repo_root.join(rel_path);
    std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
    std::fs::write(&abs, contents).unwrap();

    fire_file_save(storage, config, cogz_dir, model, code_mod, rel_path);

    let conn = storage.conn();
    conn.query_row(
        "SELECT id FROM entities WHERE type = 'file' AND file_path = ?",
        [rel_path],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| panic!("reindex should have created a file entity for {rel_path}"))
}

#[test]
fn file_save_pushes_rules_governing_saved_file() {
    use cogz::storage::crud::{Entity, insert_entity};
    use cogz::storage::edges::{Edge, insert_edge};
    let (storage, config, cogz_dir, dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let file_id = index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/foo.rs",
        "pub fn foo_work() {}\n",
    );

    // A rule that references the file entity — the shape
    // auto_references produces when rule content names the path.
    let rule_id = uuid::Uuid::new_v4().to_string();
    let mut rule = Entity::new(
        &rule_id,
        "rule",
        "no unwrap in foo module",
        "Never use unwrap() in the foo module — return typed errors.",
    );
    rule.status = "active".to_string();
    {
        let conn = storage.conn();
        insert_entity(&conn, &rule).unwrap();
        insert_edge(
            &conn,
            &Edge {
                source_id: rule_id.clone(),
                target_id: file_id.clone(),
                edge_type: "references".to_string(),
                weight: 1.0,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .unwrap();
    }

    // Saving the governed file pushes the rule as an edit-scoped pack.
    let output = fire_file_save(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        "src/foo.rs",
    );
    let pack = output
        .context_pack
        .expect("file_save on a governed file should push its rules");
    assert!(pack.sections.iter().any(|s| s.entity_id == rule_id));
    assert!(
        pack.sections
            .iter()
            .all(|s| s.tier == cogz::context::DeliveryTier::Full)
    );

    // The delivery is instrumented as its own kind so scoped hit
    // rates split out from packs.
    let conn = storage.conn();
    let scoped: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deliveries WHERE kind = 'scoped'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(scoped, 1);
    let delivered: String = conn
        .query_row(
            "SELECT u.entity_id FROM entity_usage u \
             JOIN deliveries d ON d.id = u.delivery_id \
             WHERE d.kind = 'scoped'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(delivered, rule_id);
}

#[test]
fn file_save_silent_when_no_rules_govern_file() {
    use cogz::storage::crud::{Entity, insert_entity};
    use cogz::storage::edges::{Edge, insert_edge};
    let (storage, config, cogz_dir, dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    let foo_id = index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/foo.rs",
        "pub fn foo_work() {}\n",
    );
    let bar_id = index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/bar.rs",
        "pub fn bar_work() {}\n",
    );

    // The rule governs bar, not foo.
    let rule_id = uuid::Uuid::new_v4().to_string();
    let mut rule = Entity::new(&rule_id, "rule", "bar module rule", "bar must stay pure");
    rule.status = "active".to_string();
    {
        let conn = storage.conn();
        insert_entity(&conn, &rule).unwrap();
        insert_edge(
            &conn,
            &Edge {
                source_id: rule_id,
                target_id: bar_id,
                edge_type: "references".to_string(),
                weight: 1.0,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .unwrap();
    }

    // Saving foo — an indexed file with no governing rules — stays
    // silent. So does a file the index knows nothing about.
    for path in ["src/foo.rs", "README.md"] {
        let output = fire_file_save(&storage, &config, &cogz_dir, &model, &code_mod, path);
        assert!(
            output.context_pack.is_none(),
            "file_save on {path} should not push rules"
        );
    }

    let conn = storage.conn();
    let scoped: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deliveries WHERE kind = 'scoped'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(scoped, 0);
    let _ = foo_id;
}

/// Write a knowledge entity (canonical file + DB row + references
/// edge) whose `verified_against` baseline pins `code_id` to `hash` —
/// the state `post_index_pass` compares against after a reindex.
fn verified_knowledge_on(
    storage: &Storage,
    cogz_dir: &std::path::Path,
    id: &str,
    title: &str,
    code_id: &str,
    hash: &str,
) {
    use cogz::files::{EntityFile, FileEntityType, FmValue, write_entity_file};
    use cogz::storage::crud::{Entity, insert_entity};
    use cogz::storage::edges::{Edge, insert_edge};

    let mut ef = EntityFile::new(title, FileEntityType::Knowledge, "content");
    ef.id = id.to_string();
    ef.status = "active".to_string();
    ef.references = vec![code_id.to_string()];
    ef.frontmatter.insert(
        "verified_against",
        FmValue::Array(vec![format!("{code_id}={hash}")]),
    );
    let path = ef.file_path(cogz_dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_entity_file(&path, &ef).unwrap();
    let rel = path
        .strip_prefix(cogz_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();

    let conn = storage.conn();
    let mut db = Entity::new(id, "knowledge", title, "content");
    db.file_path = Some(rel);
    db.properties = serde_json::json!({"verified_against": [format!("{code_id}={hash}")]});
    insert_entity(&conn, &db).unwrap();
    insert_edge(
        &conn,
        &Edge {
            source_id: id.to_string(),
            target_id: code_id.to_string(),
            edge_type: "references".to_string(),
            weight: 1.0,
            created_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .unwrap();
}

#[test]
fn file_save_surfaces_knowledge_drifted_by_edit() {
    let (storage, config, cogz_dir, dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/foo.rs",
        "pub fn foo_work() {}\n",
    );

    // Knowledge pinned to the function's current hash.
    let (fn_id, fn_hash): (String, String) = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT id, content_hash FROM entities \
             WHERE file_path = 'src/foo.rs' AND type = 'function'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("foo_work should be indexed")
    };
    verified_knowledge_on(
        &storage,
        &cogz_dir,
        "dddddddd-0000-4000-8000-000000000001",
        "foo_work invariants",
        &fn_id,
        &fn_hash,
    );

    // The edit — same input whether it came from an agent tool or a
    // human editor. The hook can't and shouldn't distinguish.
    std::fs::write(
        dir.path().join("src/foo.rs"),
        "pub fn foo_work() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let output = fire_file_save(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        "src/foo.rs",
    );

    let notice = output
        .notices
        .expect("save that invalidated knowledge should carry a drift notice");
    assert!(notice.contains("foo_work invariants"), "notice: {notice}");
    assert!(notice.contains("dddddddd-0000"), "notice: {notice}");
    assert!(notice.contains("src/foo.rs"), "notice: {notice}");
    assert!(notice.contains("cogz verify"), "notice: {notice}");
}

#[test]
fn file_save_no_notice_when_nothing_drifted() {
    let (storage, config, cogz_dir, dir) = setup();
    let model = query_model(&config);
    let code_mod = code_model(&config);

    index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/foo.rs",
        "pub fn foo_work() {}\n",
    );

    // Knowledge exists but pins a different file's code — editing
    // foo.rs must not surface it.
    index_source_file(
        &storage,
        &config,
        &cogz_dir,
        dir.path(),
        &model,
        &code_mod,
        "src/bar.rs",
        "pub fn bar_work() {}\n",
    );
    let (bar_fn, bar_hash): (String, String) = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT id, content_hash FROM entities \
             WHERE file_path = 'src/bar.rs' AND type = 'function'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    verified_knowledge_on(
        &storage,
        &cogz_dir,
        "dddddddd-0000-4000-8000-000000000002",
        "unrelated note",
        &bar_fn,
        &bar_hash,
    );

    std::fs::write(
        dir.path().join("src/foo.rs"),
        "pub fn foo_work() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let output = fire_file_save(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        "src/foo.rs",
    );
    assert!(
        output.notices.is_none(),
        "save unrelated to any knowledge should stay silent: {:?}",
        output.notices
    );

    // An identical save — verified against the new hash — also stays
    // silent: only drift *caused* surfaces, not drift already known.
    let (fn_id, fn_hash): (String, String) = {
        let conn = storage.conn();
        conn.query_row(
            "SELECT id, content_hash FROM entities \
             WHERE file_path = 'src/foo.rs' AND type = 'function'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    verified_knowledge_on(
        &storage,
        &cogz_dir,
        "dddddddd-0000-4000-8000-000000000003",
        "fresh note",
        &fn_id,
        &fn_hash,
    );
    let output = fire_file_save(
        &storage,
        &config,
        &cogz_dir,
        &model,
        &code_mod,
        "src/foo.rs",
    );
    // No drift text — though the third save of foo.rs may legitimately
    // produce a hot-file write nudge; that's the orthogonal half of
    // `notices`, not what this test guards.
    if let Some(n) = &output.notices {
        assert!(!n.contains("drifted"), "unexpected drift notice: {n}");
    }
}
