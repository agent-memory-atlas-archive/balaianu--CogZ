//! Search and context tools — search, get_context.
//!
//! Both tools embed the query with both models (when available) and
//! delegate to the search/context layers. The embedding happens
//! inside spawn_blocking to avoid holding the DB mutex during ONNX
//! inference.

use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult};

use crate::context::{AssembleParams, assemble_context};
use crate::mcp::helpers::{
    embed_query_for_search, mcp_internal_error, parse_context_mode, validate_query_limit,
};
use crate::mcp::params::*;
use crate::mcp::responses::{context_response, search_response, tool_success};
use crate::mcp::server::CogzServer;
use crate::search::{QueryEmbeddings, SearchParams, search as search_entities};

pub async fn search(
    server: &CogzServer,
    Parameters(params): Parameters<SearchToolParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let search_config = repo.config.search.clone();
    let default_limit = repo.config.search.max_results;
    let task_max_hops = repo.config.context.task_max_hops;
    let query_model = repo.query_model.clone();
    let code_model = repo.code_model.clone();
    let use_code = params.code_search.unwrap_or(false);
    let limit = validate_query_limit(params.limit.unwrap_or(default_limit).into())? as u32;

    let results = tokio::task::spawn_blocking(move || {
        let knowledge_emb = if use_code {
            None
        } else {
            embed_query_for_search(&query_model, &params.query)
        };
        let code_emb = embed_query_for_search(&code_model, &params.query);
        let embeddings = QueryEmbeddings {
            knowledge: knowledge_emb.as_deref(),
            code: code_emb.as_deref(),
        };
        let conn = storage.conn();
        let expand = params.expand.unwrap_or(true);
        let search_params = SearchParams {
            entity_type: params.entity_type,
            status: params.status,
            limit,
            expand,
            max_hops: if expand { task_max_hops } else { 0 },
            include_tests: true,
            silence_gate: true,
        };
        let results = search_entities(
            &conn,
            &params.query,
            embeddings,
            &search_params,
            &search_config,
        )?;
        // Usage tracking: record what was delivered so post_tool_use
        // hits can credit it. Best-effort — a tracking failure must
        // never fail a search.
        let delivered: Vec<(String, crate::storage::usage::DeliveryTier)> = results
            .results
            .iter()
            .map(|r| {
                (
                    r.entity.id.clone(),
                    crate::storage::usage::DeliveryTier::Full,
                )
            })
            .collect();
        match crate::storage::usage::record_delivery(
            &conn,
            crate::storage::usage::DeliveryKind::Search,
            None,
        ) {
            Ok(delivery_id) => {
                if let Err(e) =
                    crate::storage::usage::record_delivered(&conn, delivery_id, &delivered)
                {
                    tracing::warn!("usage tracking: record delivered failed: {e}");
                }
            }
            Err(e) => tracing::warn!("usage tracking: record delivery failed: {e}"),
        }
        let result_ids: Vec<String> = results
            .results
            .iter()
            .map(|r| r.entity.id.clone())
            .collect();
        if let Err(e) = crate::storage::usage::record_pointer_conversions(&conn, &result_ids) {
            tracing::warn!("usage tracking: pointer conversion failed: {e}");
        }

        // Query telemetry — feeds the search_miss mining pass and
        // usage analysis; queries were previously unlogged.
        if let Err(e) = crate::storage::events::record_event(
            &conn,
            crate::storage::events::EventType::SearchPerformed,
            None,
            &serde_json::json!({
                "query": params.query,
                "returned": results.results.len(),
                "filtered": results.filtered_count,
                "code_search": use_code,
            }),
        ) {
            tracing::warn!("search_performed event: {e}");
        }

        // Same push surface as the hooks: a fresh mining signal
        // (including this search's own miss) rides the response.
        let fresh = crate::hooks::nudge::fresh_suggestions(&conn, "search");
        let write_nudge = (!fresh.is_empty()).then(|| crate::hooks::nudge::format_json(&fresh));

        Ok::<_, crate::search::SearchError>((results, write_nudge))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("search", &e.to_string()))?;

    let (results, write_nudge) = results;
    let mut response = search_response(results);
    if let Some(nudge) = write_nudge {
        response["write_nudge"] = nudge;
    }
    Ok(tool_success(response))
}

pub async fn get_context(
    server: &CogzServer,
    Parameters(params): Parameters<GetContextParams>,
) -> Result<CallToolResult, McpError> {
    let mode_str = params.mode.as_deref().unwrap_or("task");
    let mode = parse_context_mode(mode_str, params.query.as_deref())?;
    let query_str = params.query.clone();
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let config = repo.config.clone();
    let query_model = repo.query_model.clone();
    let code_model = repo.code_model.clone();

    let pack = tokio::task::spawn_blocking(move || {
        let knowledge_embedding = params
            .query
            .as_deref()
            .and_then(|q| embed_query_for_search(&query_model, q));
        let code_embedding = params
            .query
            .as_deref()
            .and_then(|q| embed_query_for_search(&code_model, q));
        let conn = storage.conn();
        // An agent-pulled pack is the same delivery boundary as a
        // pushed one: prior pending entities become misses, and this
        // pack's sections are tracked for hits.
        if let Err(e) = crate::storage::usage::close_open_deliveries(&conn) {
            tracing::warn!("usage tracking: close deliveries failed: {e}");
        }
        let pack = assemble_context(
            &conn,
            &AssembleParams {
                mode,
                query: query_str.as_deref(),
                knowledge_embedding: knowledge_embedding.as_deref(),
                code_embedding: code_embedding.as_deref(),
                max_tokens: params.max_tokens,
                include_stale: params.include_stale.unwrap_or(false),
            },
            &config,
        )?;
        let mut delivered: Vec<(String, crate::storage::usage::DeliveryTier)> = pack
            .sections
            .iter()
            .map(|s| (s.entity_id.clone(), s.tier))
            .collect();
        delivered.extend(
            pack.metadata
                .pointer_ids
                .iter()
                .map(|id| (id.clone(), crate::storage::usage::DeliveryTier::Pointer)),
        );
        match crate::storage::usage::record_delivery(
            &conn,
            crate::storage::usage::DeliveryKind::Pack,
            None,
        ) {
            Ok(delivery_id) => {
                if let Err(e) =
                    crate::storage::usage::record_delivered(&conn, delivery_id, &delivered)
                {
                    tracing::warn!("usage tracking: record delivered failed: {e}");
                }
            }
            Err(e) => tracing::warn!("usage tracking: record delivery failed: {e}"),
        }

        let fresh = crate::hooks::nudge::fresh_suggestions(&conn, "get_context");
        let write_nudge = (!fresh.is_empty()).then(|| crate::hooks::nudge::format_json(&fresh));

        Ok::<_, crate::context::AssembleError>((pack, write_nudge))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("context", &e.to_string()))?;

    let (pack, write_nudge) = pack;
    let mut response = context_response(pack);
    if let Some(nudge) = write_nudge {
        response["write_nudge"] = nudge;
    }
    Ok(tool_success(response))
}
