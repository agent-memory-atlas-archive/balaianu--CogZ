//! Targeted graph tools — get_callers, get_impact, find_orphans.
//!
//! Three structural questions agents actually ask, each a thin
//! wrapper over `storage::graph_queries`. Results are recorded as
//! `pull` deliveries so post_tool_use hits measure pull efficacy.

use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult};
use serde_json::json;

use crate::mcp::helpers::{
    mcp_internal_error, mcp_invalid_parameter, tool_success, validate_query_limit,
};
use crate::mcp::params::*;
use crate::mcp::server::CogzServer;
use crate::storage::usage::{self, DeliveryKind};

const DEFAULT_IMPACT_DEPTH: usize = 2;
const MAX_IMPACT_DEPTH: usize = 4;
const DEFAULT_LIMIT: i64 = 50;
const DEFAULT_ORPHAN_TYPES: [&str; 2] = ["function", "class"];

pub async fn get_callers(
    server: &CogzServer,
    Parameters(params): Parameters<GetCallersParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let entity_id = params.entity_id.clone();
    let limit = validate_query_limit(params.limit.unwrap_or(DEFAULT_LIMIT))? as usize;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let seed = crate::storage::crud::get_entity(&conn, &entity_id)?;
        let callers = crate::storage::graph_queries::callers_of(&conn, &entity_id, limit)?;
        let ids: Vec<String> = callers.iter().map(|e| e.id.clone()).collect();
        record_pull(&conn, &ids);
        // Pulling about an entity is engagement with it: credit any
        // open delivery that surfaced the seed, and convert pointers
        // the result list re-exposed.
        if let Err(e) = usage::record_hits(&conn, std::slice::from_ref(&seed.id)) {
            tracing::warn!("usage tracking: record hits failed: {e}");
        }
        if let Err(e) = usage::record_pointer_conversions(&conn, &ids) {
            tracing::warn!("usage tracking: pointer conversion failed: {e}");
        }
        Ok::<_, crate::storage::StorageError>(json!({
            "entity_id": seed.id,
            "entity_type": seed.r#type,
            "title": seed.title,
            "callers": callers.iter().map(entity_brief).collect::<Vec<_>>(),
            "count": callers.len(),
        }))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(graph_error)?;

    Ok(tool_success(result))
}

pub async fn get_impact(
    server: &CogzServer,
    Parameters(params): Parameters<GetImpactParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let entity_id = params.entity_id.clone();
    let depth = params
        .max_depth
        .unwrap_or(DEFAULT_IMPACT_DEPTH)
        .min(MAX_IMPACT_DEPTH);
    let limit = validate_query_limit(params.limit.unwrap_or(DEFAULT_LIMIT))? as usize;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let seed = crate::storage::crud::get_entity(&conn, &entity_id)?;
        let impacted = crate::storage::graph_queries::impact_set(&conn, &entity_id, depth, limit)?;
        let referenced_by =
            crate::storage::graph_queries::referencing_knowledge(&conn, &entity_id, limit)?;
        let mut ids: Vec<String> = impacted.iter().map(|i| i.entity.id.clone()).collect();
        ids.extend(referenced_by.iter().map(|e| e.id.clone()));
        record_pull(&conn, &ids);
        if let Err(e) = usage::record_hits(&conn, std::slice::from_ref(&seed.id)) {
            tracing::warn!("usage tracking: record hits failed: {e}");
        }
        if let Err(e) = usage::record_pointer_conversions(&conn, &ids) {
            tracing::warn!("usage tracking: pointer conversion failed: {e}");
        }
        Ok::<_, crate::storage::StorageError>(json!({
            "entity_id": seed.id,
            "entity_type": seed.r#type,
            "title": seed.title,
            "impacted": impacted
                .iter()
                .map(|i| {
                    let mut v = entity_brief(&i.entity);
                    v["depth"] = json!(i.depth);
                    v["via_edge"] = json!(i.via_edge);
                    v
                })
                .collect::<Vec<_>>(),
            "referenced_by": referenced_by.iter().map(entity_brief).collect::<Vec<_>>(),
            "count": impacted.len(),
        }))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(graph_error)?;

    Ok(tool_success(result))
}

pub async fn find_orphans(
    server: &CogzServer,
    Parameters(params): Parameters<FindOrphansParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let limit = validate_query_limit(params.limit.unwrap_or(DEFAULT_LIMIT))? as usize;
    let types: Vec<String> = match params.entity_type.as_deref() {
        Some(t) => vec![t.to_string()],
        None => DEFAULT_ORPHAN_TYPES.iter().map(|s| s.to_string()).collect(),
    };
    for t in &types {
        if !matches!(t.as_str(), "function" | "class" | "file" | "module") {
            return Err(mcp_invalid_parameter(&format!(
                "entity_type '{t}' must be one of: function, class, file, module"
            )));
        }
    }

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let refs: Vec<&str> = types.iter().map(|s| s.as_str()).collect();
        let orphans = crate::storage::graph_queries::orphan_code_entities(&conn, &refs, limit)?;
        let ids: Vec<String> = orphans.iter().map(|e| e.id.clone()).collect();
        record_pull(&conn, &ids);
        if let Err(e) = usage::record_pointer_conversions(&conn, &ids) {
            tracing::warn!("usage tracking: pointer conversion failed: {e}");
        }
        Ok::<_, crate::storage::StorageError>(json!({
            "orphans": orphans.iter().map(entity_brief).collect::<Vec<_>>(),
            "count": orphans.len(),
            "note": "no incoming calls/imports/extends edges — entry points like main() surface here by design",
        }))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(graph_error)?;

    Ok(tool_success(result))
}

/// Compact entity shape for graph tool responses — enough to decide
/// whether to pull the full entity, not enough to flood the result.
fn entity_brief(e: &crate::storage::crud::Entity) -> serde_json::Value {
    json!({
        "id": e.id,
        "type": e.r#type,
        "title": e.title,
        "file_path": e.file_path,
    })
}

/// Track a pull delivery so post_tool_use hits measure whether the
/// agent acted on what these tools surfaced. Best-effort — tracking
/// failure must never fail the tool call.
fn record_pull(conn: &rusqlite::Connection, entity_ids: &[String]) {
    let entries: Vec<(String, usage::DeliveryTier)> = entity_ids
        .iter()
        .map(|id| (id.clone(), usage::DeliveryTier::Full))
        .collect();
    match usage::record_delivery(conn, DeliveryKind::Pull, None) {
        Ok(delivery_id) => {
            if let Err(e) = usage::record_delivered(conn, delivery_id, &entries) {
                tracing::warn!("usage tracking: record delivered failed: {e}");
            }
        }
        Err(e) => tracing::warn!("usage tracking: record delivery failed: {e}"),
    }
}

fn graph_error(e: crate::storage::StorageError) -> McpError {
    match e {
        crate::storage::StorageError::EntityNotFound(id) => {
            mcp_invalid_parameter(&format!("entity not found: {id}"))
        }
        other => mcp_internal_error("graph query", &other.to_string()),
    }
}
