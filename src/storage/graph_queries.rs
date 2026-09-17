//! Tool-facing graph queries — callers, impact sets, orphans,
//! knowledge references. Built on `edges`/`graph` primitives; each
//! function answers one structural question an agent actually asks.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::StorageError;
use super::crud::{Entity, get_entities_batch};
use super::graph::get_edges_involving_batch;

/// Structural edge types that make one code entity depend on
/// another. `contains` is excluded — it encodes location, not
/// dependency. `get_impact` traverses these incoming (dependents);
/// `find_orphans` checks their absence.
const DEPENDENCY_EDGE_TYPES: [&str; 3] = ["calls", "imports", "extends"];

/// Edge types that link knowledge entities to code — both manual
/// `references` and generated `auto_references` mark code the
/// knowledge describes. Same set stale-flagging follows.
const KNOWLEDGE_LINK_TYPES: [&str; 2] = ["references", "auto_references"];

/// A node in an impact set: the entity, its hop distance from the
/// seed, and the edge type that first reached it.
#[derive(Debug, Clone)]
pub struct ImpactedEntity {
    pub entity: Entity,
    pub depth: usize,
    pub via_edge: String,
}

/// Entities with `calls` edges pointing at `entity_id` — "who calls
/// this function?". Hydrated so callers get titles and file paths.
pub fn callers_of(
    conn: &Connection,
    entity_id: &str,
    limit: usize,
) -> Result<Vec<Entity>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT source_id FROM edges WHERE target_id = ? AND edge_type = 'calls' \
         ORDER BY weight DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![entity_id], |r| r.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r?);
    }
    ids.truncate(limit);
    ordered_batch(conn, &ids)
}

/// Transitive dependents of `entity_id` — entities that break (or may
/// need updating) when the seed changes. BFS over incoming dependency
/// edges up to `max_depth`; each node records its shortest depth and
/// the edge type that reached it first.
pub fn impact_set(
    conn: &Connection,
    entity_id: &str,
    max_depth: usize,
    limit: usize,
) -> Result<Vec<ImpactedEntity>, StorageError> {
    let mut depth_of: HashMap<String, usize> = HashMap::new();
    let mut via_of: HashMap<String, String> = HashMap::new();
    let mut frontier = vec![entity_id.to_string()];

    for depth in 1..=max_depth {
        if frontier.is_empty() || depth_of.len() >= limit {
            break;
        }
        let edges = get_edges_involving_batch(conn, &frontier)?;
        let mut next = Vec::new();
        for (source, target, edge_type) in edges {
            // Incoming only: the frontier node must be the target.
            if !DEPENDENCY_EDGE_TYPES.contains(&edge_type.as_str())
                || !frontier.contains(&target)
                || source == entity_id
                || depth_of.contains_key(&source)
            {
                continue;
            }
            depth_of.insert(source.clone(), depth);
            via_of.insert(source.clone(), edge_type);
            next.push(source);
        }
        frontier = next;
    }

    let mut ids: Vec<String> = depth_of.keys().cloned().collect();
    ids.sort();
    ids.truncate(limit);
    let mut out = Vec::new();
    for entity in ordered_batch(conn, &ids)? {
        out.push(ImpactedEntity {
            depth: depth_of[&entity.id],
            via_edge: via_of[&entity.id].clone(),
            entity,
        });
    }
    out.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.entity.id.cmp(&b.entity.id)));
    Ok(out)
}

/// Knowledge-layer entities pointing at `entity_id` via `references`
/// or `auto_references` — the set that goes stale when the seed's
/// behavior changes. Not transitive: these links document the entity
/// directly.
pub fn referencing_knowledge(
    conn: &Connection,
    entity_id: &str,
    limit: usize,
) -> Result<Vec<Entity>, StorageError> {
    let placeholders = KNOWLEDGE_LINK_TYPES
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT DISTINCT e.id FROM entities e \
         JOIN edges ed ON ed.source_id = e.id \
         WHERE ed.target_id = ? AND ed.edge_type IN ({placeholders}) \
         AND e.type IN ('observation', 'rule', 'knowledge') AND e.status = 'active'"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&entity_id];
    params.extend(
        KNOWLEDGE_LINK_TYPES
            .iter()
            .map(|t| t as &dyn rusqlite::ToSql),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r?);
    }
    ids.truncate(limit);
    ordered_batch(conn, &ids)
}

/// Code entities with no incoming dependency edges — dead-code
/// candidates. Test entities are excluded by default (they have no
/// callers by design). Entry points like `main` still surface — the
/// graph can't distinguish entry points from orphans, so the agent
/// judges.
pub fn orphan_code_entities(
    conn: &Connection,
    entity_types: &[&str],
    limit: usize,
) -> Result<Vec<Entity>, StorageError> {
    if entity_types.is_empty() {
        return Ok(Vec::new());
    }
    let type_placeholders = entity_types
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id FROM entities e \
         WHERE e.type IN ({type_placeholders}) AND e.status = 'active' \
         AND NOT EXISTS ( \
             SELECT 1 FROM edges ed \
             WHERE ed.target_id = e.id \
             AND ed.edge_type IN ('calls', 'imports', 'extends') \
         ) \
         AND (e.file_path IS NULL OR NOT (\
             e.file_path LIKE 'tests/%' \
             OR e.file_path LIKE '%/tests/%' \
             OR e.file_path LIKE '%/tests.rs' \
             OR e.file_path LIKE '%_tests.rs' \
             OR e.file_path LIKE '%_test.go' \
             OR e.file_path LIKE '%/test_%.py' \
             OR e.file_path LIKE '%/_test.py' \
             OR e.file_path LIKE '%.test.js' \
             OR e.file_path LIKE '%.spec.js' \
             OR e.file_path LIKE '%.test.mjs' \
             OR e.file_path LIKE '%.spec.mjs' \
             OR e.file_path LIKE '%.test.cjs' \
             OR e.file_path LIKE '%.spec.cjs' \
             OR e.file_path LIKE '%.test.jsx' \
             OR e.file_path LIKE '%.spec.jsx' \
             OR e.file_path LIKE '%.test.ts' \
             OR e.file_path LIKE '%.spec.ts' \
             OR e.file_path LIKE '%.test.tsx' \
             OR e.file_path LIKE '%.spec.tsx' \
             OR e.file_path LIKE '%/__tests__/%' \
             OR e.file_path LIKE '%/test_%.sh' \
             OR e.file_path LIKE '%/_test.sh' \
             OR e.file_path LIKE '%/test_%.bash' \
             OR e.file_path LIKE '%/_test.bash')) \
         ORDER BY e.file_path, e.id"
    );
    let params: Vec<&dyn rusqlite::ToSql> = entity_types
        .iter()
        .map(|t| t as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r?);
    }
    ids.truncate(limit);
    ordered_batch(conn, &ids)
}

/// Hydrate ids preserving the caller's order.
fn ordered_batch(conn: &Connection, ids: &[String]) -> Result<Vec<Entity>, StorageError> {
    let entities = get_entities_batch(conn, ids)?;
    let by_id: HashMap<String, Entity> = entities.into_iter().map(|e| (e.id.clone(), e)).collect();
    let mut out = Vec::with_capacity(ids.len());
    let mut seen = HashSet::new();
    for id in ids {
        if seen.insert(id.clone())
            && let Some(e) = by_id.get(id)
        {
            out.push(e.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::crud::insert_entity;
    use crate::storage::edges::{Edge, insert_edge};
    use crate::storage::ensure_vec_extension;
    use crate::storage::schema::run_migrations;

    fn setup() -> Connection {
        ensure_vec_extension();
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, 768).unwrap();
        conn
    }

    fn entity(id: &str, r#type: &str, title: &str) -> Entity {
        let mut e = Entity::new(id, r#type, title, "content");
        e.status = "active".to_string();
        e
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

    // caller -> callee (calls); main -> caller (calls)
    fn call_chain(conn: &Connection) {
        for (id, t) in [
            ("callee", "function"),
            ("caller", "function"),
            ("main_fn", "function"),
        ] {
            insert_entity(conn, &entity(id, t, id)).unwrap();
        }
        insert_edge(conn, &edge("caller", "callee", "calls")).unwrap();
        insert_edge(conn, &edge("main_fn", "caller", "calls")).unwrap();
    }

    #[test]
    fn callers_of_returns_direct_callers() {
        let conn = setup();
        call_chain(&conn);
        let callers = callers_of(&conn, "callee", 50).unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].id, "caller");
        assert!(callers_of(&conn, "main_fn", 50).unwrap().is_empty());
    }

    #[test]
    fn impact_set_transitive_dependents_with_depth() {
        let conn = setup();
        call_chain(&conn);
        let impacted = impact_set(&conn, "callee", 3, 50).unwrap();
        assert_eq!(impacted.len(), 2);
        assert_eq!(impacted[0].entity.id, "caller");
        assert_eq!(impacted[0].depth, 1);
        assert_eq!(impacted[0].via_edge, "calls");
        assert_eq!(impacted[1].entity.id, "main_fn");
        assert_eq!(impacted[1].depth, 2);
    }

    #[test]
    fn impact_set_respects_max_depth_and_direction() {
        let conn = setup();
        call_chain(&conn);
        let one_hop = impact_set(&conn, "callee", 1, 50).unwrap();
        assert_eq!(one_hop.len(), 1);
        // Outgoing direction is not traversed: callee is a dependency
        // of caller, not a dependent.
        assert!(impact_set(&conn, "main_fn", 3, 50).unwrap().is_empty());
    }

    #[test]
    fn impact_set_ignores_non_dependency_edges() {
        let conn = setup();
        call_chain(&conn);
        insert_entity(&conn, &entity("k1", "knowledge", "doc")).unwrap();
        insert_edge(&conn, &edge("k1", "callee", "references")).unwrap();
        // references is a knowledge link, not a dependency — it must
        // not appear in the transitive impact set.
        let impacted = impact_set(&conn, "callee", 3, 50).unwrap();
        assert!(impacted.iter().all(|i| i.entity.id != "k1"));
    }

    #[test]
    fn referencing_knowledge_finds_manual_and_auto_links() {
        let conn = setup();
        call_chain(&conn);
        insert_entity(&conn, &entity("k1", "knowledge", "doc")).unwrap();
        insert_entity(&conn, &entity("o1", "observation", "obs")).unwrap();
        insert_edge(&conn, &edge("k1", "callee", "references")).unwrap();
        insert_edge(&conn, &edge("o1", "callee", "auto_references")).unwrap();
        let refs = referencing_knowledge(&conn, "callee", 50).unwrap();
        assert_eq!(refs.len(), 2);
        // Stale/draft knowledge doesn't surface.
        let mut stale = entity("s1", "knowledge", "stale doc");
        stale.status = "stale".to_string();
        insert_entity(&conn, &stale).unwrap();
        insert_edge(&conn, &edge("s1", "callee", "references")).unwrap();
        let refs = referencing_knowledge(&conn, "callee", 50).unwrap();
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn orphans_exclude_called_functions() {
        let conn = setup();
        call_chain(&conn);
        let orphans = orphan_code_entities(&conn, &["function"], 50).unwrap();
        let ids: Vec<&str> = orphans.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"main_fn"));
        assert!(!ids.contains(&"caller"));
        assert!(!ids.contains(&"callee"));
    }

    #[test]
    fn orphans_exclude_test_files() {
        let conn = setup();
        // A function in a test file is excluded even though nothing
        // calls it — tests have no callers by design. Patterns mirror
        // `index::gitignore::is_test_file` like `query.rs` does.
        let mut test_fn = entity("test_fn", "function", "test_fn");
        test_fn.file_path = Some("tests/foo.rs".to_string());
        insert_entity(&conn, &test_fn).unwrap();
        let orphans = orphan_code_entities(&conn, &["function"], 50).unwrap();
        assert!(orphans.iter().all(|e| e.id != "test_fn"));
    }
}
