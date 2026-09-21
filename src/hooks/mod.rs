//! Hooks — lifecycle event capture and context injection.
//!
//! Called by `cogz capture-event` (CLI) and `capture_event` (MCP tool).
//! For `session_start` and `prompt_submit`, generates a context pack
//! for injection into the agent's context window and spawns a
//! background reindex to catch non-hook changes. For `pre_tool_use`
//! and `post_tool_use`, records the event and optionally an observation.
//! For `file_save`, triggers a single-file code reindex. For
//! `session_end`, runs consolidation.

pub mod capture;
pub mod format;
pub mod handlers;
pub mod lifecycle;
pub mod nudge;
pub mod reindex;

pub use capture::{CaptureError, CaptureInput, CaptureResult, run_capture_event};
pub use lifecycle::{LifecycleEvent, handle_lifecycle_event};
