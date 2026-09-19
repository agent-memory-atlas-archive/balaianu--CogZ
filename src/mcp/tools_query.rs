//! Query tools — query_entities (observation/rule/knowledge),
//! list_entities.
//!
//! All read-only. Each acquires the DB connection inside
//! spawn_blocking, delegates to a helper for the actual query, and
//! builds the response via `query_response` or `list_entities_response`.

use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult};

use crate::mcp::helpers::{
    list_entities_response, mcp_internal_error, mcp_invalid_parameter, query_by_type_with_refs,
    query_knowledge_with_refs, validate_query_limit,
};
use crate::mcp::params::*;
use crate::mcp::responses::{query_response, tool_success};
use crate::mcp::server::CogzServer;
use crate::storage::usage::{self, DeliveryKind};

/// Record returned entities as a pull delivery so later hits — a
/// reference in a write, a file touch — attribute back to this call.
fn record_query_pull(conn: &rusqlite::Connection, entity_ids: &[String]) {
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
    if let Err(e) = usage::record_pointer_conversions(conn, entity_ids) {
        tracing::warn!("usage tracking: pointer conversion failed: {e}");
    }
}

pub async fn query_observations(
    server: &CogzServer,
    Parameters(params): Parameters<QueryObservationsParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let limit = validate_query_limit(params.limit.unwrap_or(20))?;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let result = query_by_type_with_refs(
            &conn,
            "observation",
            params.status.as_deref(),
            limit,
            params.references.as_deref(),
        )?;
        record_query_pull(
            &conn,
            &result.0.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        );
        Ok::<_, crate::storage::StorageError>(result)
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("query", &e.to_string()))?;

    let (entities, refs_map) = result;
    Ok(tool_success(query_response(
        entities,
        "observations",
        &refs_map,
    )))
}

pub async fn query_rules(
    server: &CogzServer,
    Parameters(params): Parameters<QueryRulesParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let limit = validate_query_limit(params.limit.unwrap_or(20))?;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let result = query_by_type_with_refs(
            &conn,
            "rule",
            params.status.as_deref(),
            limit,
            params.references.as_deref(),
        )?;
        record_query_pull(
            &conn,
            &result.0.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        );
        Ok::<_, crate::storage::StorageError>(result)
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("query", &e.to_string()))?;

    let (entities, refs_map) = result;
    Ok(tool_success(query_response(entities, "rules", &refs_map)))
}

pub async fn query_knowledge(
    server: &CogzServer,
    Parameters(params): Parameters<QueryKnowledgeParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let limit = validate_query_limit(params.limit.unwrap_or(20))?;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let result = query_knowledge_with_refs(
            &conn,
            params.status.as_deref(),
            params.category.as_deref(),
            params.tags.as_deref(),
            limit,
        )?;
        record_query_pull(
            &conn,
            &result.0.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        );
        Ok::<_, crate::storage::StorageError>(result)
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("query", &e.to_string()))?;

    let (entities, refs_map) = result;
    Ok(tool_success(query_response(
        entities,
        "knowledge",
        &refs_map,
    )))
}

pub async fn list_entities(
    server: &CogzServer,
    Parameters(params): Parameters<ListEntitiesParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let entity_type = params.entity_type.clone();
    let status = params.status.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        list_entities_response(&conn, &entity_type, status.as_deref())
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("query", &e.to_string()))?;

    Ok(tool_success(result))
}

/// Single browse entry point for the knowledge layer. `entity_type`
/// selects the lifecycle class; type-specific filters are documented
/// on the params and rejected when they don't apply.
pub async fn query_entities(
    server: &CogzServer,
    Parameters(params): Parameters<QueryEntitiesParams>,
) -> Result<CallToolResult, McpError> {
    match params.entity_type.as_str() {
        "observation" => {
            query_observations(
                server,
                Parameters(QueryObservationsParams {
                    repo: params.repo,
                    status: params.status,
                    references: params.references,
                    limit: params.limit,
                }),
            )
            .await
        }
        "rule" => {
            query_rules(
                server,
                Parameters(QueryRulesParams {
                    repo: params.repo,
                    status: params.status,
                    references: params.references,
                    limit: params.limit,
                }),
            )
            .await
        }
        "knowledge" => {
            query_knowledge(
                server,
                Parameters(QueryKnowledgeParams {
                    repo: params.repo,
                    category: params.category,
                    tags: params.tags,
                    status: params.status,
                    limit: params.limit,
                }),
            )
            .await
        }
        other => Err(mcp_invalid_parameter(&format!(
            "invalid entity_type '{other}': expected observation, rule, or knowledge"
        ))),
    }
}
