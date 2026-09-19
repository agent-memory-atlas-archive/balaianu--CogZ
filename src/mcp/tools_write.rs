//! Write tools — create_entity (observation/rule/knowledge),
//! update_knowledge.
//!
//! All write tools follow the file-first invariant: the entity file
//! is written before the DB is touched. The actual write-and-sync
//! logic lives in `helpers.rs` and `update_knowledge.rs`.

use rmcp::{ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult};
use serde_json::json;

use crate::files::frontmatter::FmValue;
use crate::files::{EntityFile, FileEntityType};
use crate::mcp::helpers::{
    auto_title, create_entity_file, mcp_internal_error, mcp_invalid_parameter,
    update_knowledge_file, write_and_sync,
};
use crate::mcp::params::*;
use crate::mcp::responses::tool_success;
use crate::mcp::server::CogzServer;

pub async fn record_observation(
    server: &CogzServer,
    Parameters(params): Parameters<RecordObservationParams>,
) -> Result<CallToolResult, McpError> {
    let title = params.title.unwrap_or_else(|| auto_title(&params.content));
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let config = repo.config.clone();
    let cogz_dir = repo.cogz_dir.clone();
    let query_model = repo.query_model.clone();
    let nli_model = repo.nli_model.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut entity = EntityFile::new(&title, FileEntityType::Observation, &params.content);

        if let Some(refs) = params.references.as_deref() {
            entity.references = refs.to_vec();
        }

        if let Some(supporting) = params.supporting_ids.as_deref()
            && !supporting.is_empty()
        {
            entity
                .frontmatter
                .insert("supporting_ids", FmValue::Array(supporting.to_vec()));
        }

        let source = params.source.unwrap_or_else(|| "agent".to_string());
        entity.frontmatter.insert("source", FmValue::String(source));
        entity.frontmatter.insert("confidence", FmValue::Float(0.5));

        create_entity_file(
            &storage,
            &config,
            &cogz_dir,
            &entity,
            Some(&query_model),
            Some(&*nli_model),
        )
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))??;

    Ok(tool_success(json!(result)))
}

pub async fn create_rule(
    server: &CogzServer,
    Parameters(params): Parameters<CreateRuleParams>,
) -> Result<CallToolResult, McpError> {
    let title = params.title.unwrap_or_else(|| auto_title(&params.content));
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let config = repo.config.clone();
    let cogz_dir = repo.cogz_dir.clone();
    let query_model = repo.query_model.clone();
    let nli_model = repo.nli_model.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut entity = EntityFile::new(&title, FileEntityType::Rule, &params.content);

        if let Some(refs) = params.references.as_deref() {
            entity.references = refs.to_vec();
        }

        let confidence = params.confidence.unwrap_or(1.0);
        entity
            .frontmatter
            .insert("confidence", FmValue::Float(confidence));

        create_entity_file(
            &storage,
            &config,
            &cogz_dir,
            &entity,
            Some(&query_model),
            Some(&*nli_model),
        )
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))??;

    Ok(tool_success(json!(result)))
}

pub async fn create_knowledge(
    server: &CogzServer,
    Parameters(params): Parameters<CreateKnowledgeParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let config = repo.config.clone();
    let cogz_dir = repo.cogz_dir.clone();
    let query_model = repo.query_model.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut entity = EntityFile::new(&params.title, FileEntityType::Knowledge, &params.content);

        entity
            .frontmatter
            .insert("category", FmValue::String(params.category.clone()));

        if let Some(tags) = &params.tags {
            entity
                .frontmatter
                .insert("tags", FmValue::Array(tags.clone()));
        }

        if let Some(refs) = &params.references {
            entity.references = refs.clone();
        }

        write_and_sync(
            &storage,
            &config,
            &cogz_dir,
            &entity,
            "knowledge",
            Some(&query_model),
            None,
        )
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))??;

    Ok(tool_success(json!(result)))
}

pub async fn update_knowledge(
    server: &CogzServer,
    Parameters(params): Parameters<UpdateKnowledgeParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let cogz_dir = repo.cogz_dir.clone();
    let query_model = repo.query_model.clone();

    let result = tokio::task::spawn_blocking(move || {
        update_knowledge_file(&storage, &cogz_dir, &params, Some(&query_model))
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))??;

    Ok(tool_success(json!(result)))
}

pub async fn verify_knowledge(
    server: &CogzServer,
    Parameters(params): Parameters<VerifyKnowledgeParams>,
) -> Result<CallToolResult, McpError> {
    let repo = server.resolve_repo(&params.repo)?;
    let storage = repo.storage.clone();
    let cogz_dir = repo.cogz_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::index::drift::verify_entity(&storage, &cogz_dir, &params.id).map(
            |(refs_stamped, reactivated)| {
                json!({
                    "id": params.id,
                    "refs_stamped": refs_stamped,
                    "reactivated": reactivated,
                })
            },
        )
    })
    .await
    .map_err(|e| mcp_internal_error("spawn_blocking", &e.to_string()))?
    .map_err(|e| mcp_internal_error("verify_knowledge", &e.to_string()))?;

    Ok(tool_success(result))
}

/// Single write entry point for the three knowledge-layer lifecycle
/// classes. Validates per-type requirements, then dispatches to the
/// type-specific implementation.
pub async fn create_entity(
    server: &CogzServer,
    Parameters(params): Parameters<CreateEntityParams>,
) -> Result<CallToolResult, McpError> {
    match params.entity_type.as_str() {
        "observation" => {
            record_observation(
                server,
                Parameters(RecordObservationParams {
                    repo: params.repo,
                    content: params.content,
                    title: params.title,
                    references: params.references,
                    supporting_ids: params.supporting_ids,
                    source: params.source,
                }),
            )
            .await
        }
        "rule" => {
            create_rule(
                server,
                Parameters(CreateRuleParams {
                    repo: params.repo,
                    content: params.content,
                    title: params.title,
                    references: params.references,
                    confidence: params.confidence,
                }),
            )
            .await
        }
        "knowledge" => {
            let title = params
                .title
                .ok_or_else(|| mcp_invalid_parameter("title is required for knowledge entities"))?;
            let category = params.category.ok_or_else(|| {
                mcp_invalid_parameter("category is required for knowledge entities")
            })?;
            create_knowledge(
                server,
                Parameters(CreateKnowledgeParams {
                    repo: params.repo,
                    title,
                    content: params.content,
                    category,
                    tags: params.tags,
                    references: params.references,
                }),
            )
            .await
        }
        other => Err(mcp_invalid_parameter(&format!(
            "invalid entity_type '{other}': expected observation, rule, or knowledge"
        ))),
    }
}
