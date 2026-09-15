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

fn entity(id: &str, entity_type: &str) -> Entity {
    Entity::new(id, entity_type, id, "content")
}

fn seeds(ids: &[&str]) -> Vec<(String, f64)> {
    ids.iter().map(|id| (id.to_string(), 1.0)).collect()
}

fn direct(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn exclude(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
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
fn finds_entities_not_in_fts_results() {
    let conn = setup();
    insert_entity(&conn, &entity("seed_fn", "function")).unwrap();
    insert_entity(&conn, &entity("hidden_fn", "function")).unwrap();
    insert_edge(&conn, &edge("seed_fn", "hidden_fn", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["seed_fn"]),
        &direct(&["seed_fn"]),
        &exclude(&["seed_fn"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id, "hidden_fn");
    assert!(results[0].score > 0.0);
}

#[test]
fn one_hop_outscores_two_hop() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    insert_entity(&conn, &entity("hop1", "function")).unwrap();
    insert_entity(&conn, &entity("hop2", "function")).unwrap();
    // references edges weigh 1.0 — isolates the hop-decay term.
    insert_edge(&conn, &edge("s", "hop1", "references")).unwrap();
    insert_edge(&conn, &edge("hop1", "hop2", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        2,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();

    let score1 = results
        .iter()
        .find(|c| c.entity_id == "hop1")
        .unwrap()
        .score;
    let score2 = results
        .iter()
        .find(|c| c.entity_id == "hop2")
        .unwrap()
        .score;
    assert!(score1 > score2);
    assert!((score1 / score2 - 2.0).abs() < 1e-9);
}

#[test]
fn structural_edges_are_not_traversed() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    insert_entity(&conn, &entity("via_refs", "knowledge")).unwrap();
    insert_entity(&conn, &entity("via_imports", "function")).unwrap();
    insert_entity(&conn, &entity("via_calls", "function")).unwrap();
    insert_edge(&conn, &edge("s", "via_refs", "references")).unwrap();
    insert_edge(&conn, &edge("s", "via_imports", "imports")).unwrap();
    insert_edge(&conn, &edge("s", "via_calls", "calls")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();

    // Only curated edges count — structural fan-out stays in the
    // decayed expansion path, not the primary candidate pool.
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id, "via_refs");
}

#[test]
fn shared_neighbor_appears_once() {
    let conn = setup();
    insert_entity(&conn, &entity("s1", "function")).unwrap();
    insert_entity(&conn, &entity("s2", "function")).unwrap();
    insert_entity(&conn, &entity("shared", "function")).unwrap();
    insert_edge(&conn, &edge("s1", "shared", "references")).unwrap();
    insert_edge(&conn, &edge("s2", "shared", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s1", "s2"]),
        &direct(&["s1", "s2"]),
        &exclude(&["s1", "s2"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();

    assert_eq!(
        results.iter().filter(|c| c.entity_id == "shared").count(),
        1
    );
}

#[test]
fn empty_seeds_returns_empty() {
    let conn = setup();
    insert_entity(&conn, &entity("a", "function")).unwrap();
    insert_entity(&conn, &entity("b", "function")).unwrap();
    insert_edge(&conn, &edge("a", "b", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &[],
        &direct(&[]),
        &exclude(&[]),
        2,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();
    assert!(results.is_empty());
}

#[test]
fn excluded_entities_are_not_duplicated() {
    let conn = setup();
    insert_entity(&conn, &entity("s1", "function")).unwrap();
    insert_entity(&conn, &entity("s2", "function")).unwrap();
    insert_edge(&conn, &edge("s1", "s2", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s1"]),
        &direct(&["s1"]),
        &exclude(&["s1", "s2"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();
    assert!(results.is_empty());
}

#[test]
fn no_edges_returns_empty() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    insert_entity(&conn, &entity("island", "function")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        2,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();
    assert!(results.is_empty());
}

#[test]
fn respects_status_filter() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    let mut stale = entity("stale_fn", "function");
    stale.status = "stale".to_string();
    insert_entity(&conn, &stale).unwrap();
    insert_edge(&conn, &edge("s", "stale_fn", "references")).unwrap();

    let active = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        1,
        0.5,
        20,
        Some("active"),
        true,
    )
    .unwrap();
    assert!(active.is_empty());

    let stale_results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        1,
        0.5,
        20,
        Some("stale"),
        true,
    )
    .unwrap();
    assert_eq!(stale_results.len(), 1);
}

#[test]
fn truncates_to_max_candidates() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    for i in 0..5 {
        let id = format!("n{i}");
        insert_entity(&conn, &entity(&id, "function")).unwrap();
        insert_edge(&conn, &edge("s", &id, "references")).unwrap();
    }

    let results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        1,
        0.5,
        3,
        None,
        true,
    )
    .unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn weak_seed_candidates_are_marked_not_direct() {
    let conn = setup();
    insert_entity(&conn, &entity("strong_s", "function")).unwrap();
    insert_entity(&conn, &entity("weak_s", "function")).unwrap();
    insert_entity(&conn, &entity("via_strong", "function")).unwrap();
    insert_entity(&conn, &entity("via_weak", "function")).unwrap();
    insert_entity(&conn, &entity("via_both", "function")).unwrap();
    insert_edge(&conn, &edge("strong_s", "via_strong", "references")).unwrap();
    insert_edge(&conn, &edge("weak_s", "via_weak", "references")).unwrap();
    insert_edge(&conn, &edge("strong_s", "via_both", "references")).unwrap();
    insert_edge(&conn, &edge("weak_s", "via_both", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["strong_s", "weak_s"]),
        &direct(&["strong_s"]),
        &exclude(&["strong_s", "weak_s"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();

    let direct_of = |id: &str| results.iter().find(|c| c.entity_id == id).map(|c| c.direct);
    assert_eq!(direct_of("via_strong"), Some(true));
    assert_eq!(direct_of("via_weak"), Some(false));
    // A weak-only path can't mark a candidate direct, but a direct
    // path does even when a weak seed also reaches it.
    assert_eq!(direct_of("via_both"), Some(true));
}

#[test]
fn follows_incoming_edges() {
    let conn = setup();
    insert_entity(&conn, &entity("s", "function")).unwrap();
    insert_entity(&conn, &entity("caller", "function")).unwrap();
    insert_edge(&conn, &edge("caller", "s", "references")).unwrap();

    let results = graph_retrieve(
        &conn,
        &seeds(&["s"]),
        &direct(&["s"]),
        &exclude(&["s"]),
        1,
        0.5,
        20,
        None,
        true,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id, "caller");
}
