//! MCP tool router — dispatches tool calls to implementation modules.
//!
//! Tool implementations are grouped by category:
//! - `tools_write` — create_entity, update_knowledge, verify_knowledge
//! - `tools_query` — query_entities, list_entities
//! - `tools_search` — search, get_context
//! - `tools_graph` — get_callers, get_impact, find_orphans
//! - `tools_mining` — suggest_observations
//! - `tools_system` — get_status, consolidate
//!
//! Parameter structs live in `params.rs`; shared helpers in `helpers.rs`.

use rmcp::{
    ErrorData as McpError, handler::server::wrapper::Parameters, model::CallToolResult, tool,
    tool_router,
};

use crate::mcp::params::*;
use crate::mcp::server::CogzServer;
use crate::mcp::{tools_graph, tools_mining, tools_query, tools_search, tools_system, tools_write};

#[tool_router(vis = "pub")]
impl CogzServer {
    #[tool(
        name = "create_entity",
        description = "Create a knowledge-layer entity. `entity_type` picks the lifecycle, not the topic — ask what the entry IS: 'observation' = something that happened (bug found, surprising behavior, decision noticed) — raw, append-only, unvalidated; consolidation promotes the good ones to rules. 'rule' = a verified directive agents must always follow (conventions, constraints) — pushed into every context pack; change via supersede, not edits. 'knowledge' = a curated reference doc (architecture, gotchas, design decisions) — the only editable type (update_knowledge). Requires: content always; title+category for knowledge."
    )]
    async fn create_entity(
        &self,
        params: Parameters<CreateEntityParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_write::create_entity(self, params).await
    }

    #[tool(
        name = "update_knowledge",
        description = "Update an existing knowledge entry's content — use to correct or extend documentation when facts change. The only entity type allowing in-place edits; observations and rules are append-only."
    )]
    async fn update_knowledge(
        &self,
        params: Parameters<UpdateKnowledgeParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_write::update_knowledge(self, params).await
    }

    #[tool(
        name = "verify_knowledge",
        description = "Re-verify a knowledge entity against its referenced code — re-stamps verified_against provenance, clears drift annotations, and reactivates the entity if it was stale. Use after reading drift-flagged or stale knowledge and confirming it is still accurate — not for changing content (use update_knowledge instead)."
    )]
    async fn verify_knowledge(
        &self,
        params: Parameters<VerifyKnowledgeParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_write::verify_knowledge(self, params).await
    }

    #[tool(
        name = "query_entities",
        description = "Browse knowledge-layer entities of one type — 'observation' (raw findings, recency order), 'rule' (verified directives, confidence order), or 'knowledge' (curated docs). Use to enumerate what exists before writing (avoid duplicates) or to review a type — for ranked retrieval on a question use search; for code entities use list_entities or search."
    )]
    async fn query_entities(
        &self,
        params: Parameters<QueryEntitiesParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_query::query_entities(self, params).await
    }

    #[tool(
        name = "search",
        description = "Ranked search across all entities — code (functions, files, classes) plus rules, observations, and knowledge — using hybrid lexical + semantic retrieval with graph expansion. Use when you know the concept but not the exact name, or want related entities surfaced automatically. For exact identifier/text matches, grep is faster and equally precise."
    )]
    async fn search(
        &self,
        params: Parameters<SearchToolParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_search::search(self, params).await
    }

    #[tool(
        name = "get_context",
        description = "Assemble a context pack — a scoped, ranked bundle of orientation, rules, relevant code, and knowledge for a task query. Use at the start of substantial work on a topic instead of reading files broadly — narrower than a session-start pack, broader than a single search."
    )]
    async fn get_context(
        &self,
        params: Parameters<GetContextParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_search::get_context(self, params).await
    }

    #[tool(
        name = "get_callers",
        description = "Find entities that call this function — answers 'who calls X?' with resolved call edges, not text matches. Use instead of grepping for the name when you need real callers (not comments, strings, or same-named functions)."
    )]
    async fn get_callers(
        &self,
        params: Parameters<GetCallersParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_graph::get_callers(self, params).await
    }

    #[tool(
        name = "get_impact",
        description = "Transitive dependents of an entity — what breaks or needs updating when it changes (incoming calls/imports/extends up to max_depth hops), plus knowledge that references it. Use before renaming, deleting, or changing a signature."
    )]
    async fn get_impact(
        &self,
        params: Parameters<GetImpactParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_graph::get_impact(self, params).await
    }

    #[tool(
        name = "find_orphans",
        description = "Find code entities with no incoming calls/imports/extends edges — dead-code candidates. Use during cleanup audits. Entry points like main() surface by design; test code is excluded."
    )]
    async fn find_orphans(
        &self,
        params: Parameters<FindOrphansParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_graph::find_orphans(self, params).await
    }

    #[tool(
        name = "suggest_observations",
        description = "Mine recent session usage for observation candidates — zero-hit packs followed by edits, hot files, error→fix sequences. Use at natural stopping points to capture what the session learned; confirm salient suggestions via create_entity."
    )]
    async fn suggest_observations(
        &self,
        params: Parameters<SuggestObservationsParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_mining::suggest_observations(self, params).await
    }

    #[tool(
        name = "get_status",
        description = "CogZ system status — entity counts, DB stats, model availability, staleness. Use to check the index is fresh and retrieval is at full capability before relying on it."
    )]
    async fn get_status(
        &self,
        params: Parameters<GetStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_system::get_status(self, params).await
    }

    #[tool(
        name = "list_entities",
        description = "List IDs and titles for all entities of a type. Use to enumerate a type or resolve an entity_id for get_callers/get_impact — returns no content; for substance use search or query_* tools."
    )]
    async fn list_entities(
        &self,
        params: Parameters<ListEntitiesParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_query::list_entities(self, params).await
    }

    #[tool(
        name = "consolidate",
        description = "Run deferred consolidation — promote supported observations to rules, merge confirmed duplicates. Housekeeping, not a write path; dedup and contradiction checks already run on every insert."
    )]
    async fn consolidate(
        &self,
        params: Parameters<ConsolidateParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_system::consolidate(self, params).await
    }

    #[tool(
        name = "capture_event",
        description = "Capture a lifecycle event — called by hook scripts, not intended for direct use. session_start/prompt_submit return context packs; file_save reindexes and may return rules governing the edited file; session_end runs consolidation."
    )]
    async fn capture_event(
        &self,
        params: Parameters<CaptureEventParams>,
    ) -> Result<CallToolResult, McpError> {
        tools_system::capture_event(self, params).await
    }
}
