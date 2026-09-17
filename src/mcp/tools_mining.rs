//! Write-path mining tool — `suggest_observations` surfaces structured
//! candidates from usage signals. Nothing is written: the agent's own
//! model reads the candidates and confirms through
//! `record_observation`, keeping CogZ the store and the agent the
//! distiller.

use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult};
use serde_json::json;

use crate::mcp::helpers::{mcp_internal_error, tool_success, validate_query_limit};
use crate::mcp::params::*;
use crate::mcp::server::CogzServer;

const DEFAULT_DAYS: u32 = 7;
const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 50;

pub async fn suggest_observations(
    server: &CogzServer,
    Parameters(params): Parameters<SuggestObservationsParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let days = params.days.unwrap_or(DEFAULT_DAYS).min(90);
    let limit =
        validate_query_limit(params.limit.unwrap_or(DEFAULT_LIMIT))?.min(MAX_LIMIT) as usize;

    let result = tokio::task::spawn_blocking(move || {
        let conn = storage.conn();
        let suggestions = crate::storage::mining::mine_suggestions(&conn, days, limit)?;
        // Audit trail — whether mining gets asked for is itself a
        // signal about the write path's adoption.
        if let Err(e) = crate::storage::events::record_event(
            &conn,
            crate::storage::events::EventType::SuggestionsRequested,
            None,
            &json!({ "days": days, "returned": suggestions.len() }),
        ) {
            tracing::warn!("failed to record suggestions_requested event: {e}");
        }
        Ok::<_, crate::storage::StorageError>(json!({
            "suggestions": suggestions,
            "count": suggestions.len(),
            "window_days": days,
        }))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("mining", &e.to_string()))?;

    Ok(tool_success(result))
}
