use super::super::super::storage::crud::Entity;
use super::super::super::storage::crud::insert_entity;
use super::super::super::storage::edges::Edge;
use super::super::super::storage::edges::insert_edge;
use super::super::super::storage::ensure_vec_extension;
use super::super::super::storage::schema::run_migrations;
use super::*;

fn setup() -> Connection {
    ensure_vec_extension();
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn, 768).unwrap();
    conn
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
fn expand_1_hop() {
    let conn = setup();
    insert_entity(
        &conn,
        &Entity::new("obs1", "observation", "Bug Report", "c"),
    )
    .unwrap();
    insert_entity(&conn, &Entity::new("func1", "function", "build_sql", "c")).unwrap();

    insert_edge(&conn, &edge("obs1", "func1", "references")).unwrap();

    let exclude = HashSet::new();
    let results =
        expand_with_paths(&conn, &["obs1".to_string()], 1, &exclude, None, true, None).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id, "func1");
    assert_eq!(results[0].graph_path, vec!["obs1", "func1"]);
    assert_eq!(results[0].edge_path, vec!["references"]);
    assert_eq!(results[0].seed_id, "obs1");
}

#[test]
fn expand_2_hops() {
    let conn = setup();
    insert_entity(&conn, &Entity::new("obs1", "observation", "Bug", "c")).unwrap();
    insert_entity(&conn, &Entity::new("func1", "function", "build_sql", "c")).unwrap();
    insert_entity(&conn, &Entity::new("func2", "function", "search_all", "c")).unwrap();

    insert_edge(&conn, &edge("obs1", "func1", "references")).unwrap();
    insert_edge(&conn, &edge("func1", "func2", "calls")).unwrap();

    let exclude = HashSet::new();
    let results =
        expand_with_paths(&conn, &["obs1".to_string()], 2, &exclude, None, true, None).unwrap();

    assert_eq!(results.len(), 2);
    let func2 = results.iter().find(|r| r.entity_id == "func2").unwrap();
    assert_eq!(func2.graph_path, vec!["obs1", "func1", "func2"]);
    assert_eq!(func2.edge_path, vec!["references", "calls"]);
}

#[test]
fn expand_excludes_specified_ids() {
    let conn = setup();
    insert_entity(&conn, &Entity::new("obs1", "observation", "Bug", "c")).unwrap();
    insert_entity(&conn, &Entity::new("func1", "function", "build_sql", "c")).unwrap();

    insert_edge(&conn, &edge("obs1", "func1", "references")).unwrap();

    let mut exclude = HashSet::new();
    exclude.insert("func1".to_string());

    let results =
        expand_with_paths(&conn, &["obs1".to_string()], 1, &exclude, None, true, None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn expand_no_edges() {
    let conn = setup();
    insert_entity(&conn, &Entity::new("obs1", "observation", "Bug", "c")).unwrap();

    let exclude = HashSet::new();
    let results =
        expand_with_paths(&conn, &["obs1".to_string()], 2, &exclude, None, true, None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn expand_zero_hops() {
    let conn = setup();
    insert_entity(&conn, &Entity::new("obs1", "observation", "Bug", "c")).unwrap();

    let exclude = HashSet::new();
    let results =
        expand_with_paths(&conn, &["obs1".to_string()], 0, &exclude, None, true, None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn expand_filters_by_status() {
    let conn = setup();
    insert_entity(&conn, &Entity::new("obs1", "observation", "Bug", "c")).unwrap();
    let mut stale = Entity::new("func1", "function", "build_sql", "c");
    stale.status = "stale".to_string();
    insert_entity(&conn, &stale).unwrap();

    insert_edge(&conn, &edge("obs1", "func1", "references")).unwrap();

    let exclude = HashSet::new();
    let results = expand_with_paths(
        &conn,
        &["obs1".to_string()],
        1,
        &exclude,
        Some("active"),
        true,
        None,
    )
    .unwrap();
    assert!(results.is_empty());

    let results = expand_with_paths(
        &conn,
        &["obs1".to_string()],
        1,
        &exclude,
        Some("stale"),
        true,
        None,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
}
