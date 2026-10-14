//! Input validation for MCP write tools.
//!
//! The canonical-file parser rejects malformed references, IDs, and
//! out-of-range fields — but only *after* the file has been written.
//! Validating at the input boundary keeps unparseable files from ever
//! reaching disk.

use rmcp::ErrorData as McpError;

use crate::mcp::errors::mcp_invalid_parameter;

/// Reject a list field unless every entry is a well-formed UUID.
/// `references` and `supporting_ids` must be UUIDs: non-UUID values
/// produce canonical files the parser rejects on every later sync.
pub fn validate_uuid_list(field: &str, values: Option<&[String]>) -> Result<(), McpError> {
    if let Some(values) = values {
        for v in values {
            if uuid::Uuid::parse_str(v).is_err() {
                return Err(mcp_invalid_parameter(&format!(
                    "invalid {field} entry '{v}': must be a valid UUID"
                )));
            }
        }
    }
    Ok(())
}

/// Reject a confidence value outside [0, 1]. NaN is rejected by the
/// range check.
pub fn validate_confidence(confidence: Option<f64>) -> Result<(), McpError> {
    if let Some(c) = confidence
        && !(0.0..=1.0).contains(&c)
    {
        return Err(mcp_invalid_parameter(&format!(
            "invalid confidence {c}: must be between 0 and 1"
        )));
    }
    Ok(())
}
