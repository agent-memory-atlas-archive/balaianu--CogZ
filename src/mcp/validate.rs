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

/// Field caps for write tools — the canonical parser accepts any
/// length, so the boundary enforces sane sizes before an oversized
/// write lands on disk and in the embed queue.
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_CONTENT_BYTES: usize = 256 * 1024;
pub const MAX_TAGS: usize = 32;

/// Validate title (≤ 200 chars), content (≤ 256 KiB), and tag count
/// (≤ 32) for create/update write tools. `title`/`tags` are validated
/// only when present — auto-generated titles are already bounded.
pub fn validate_write_fields(
    title: &str,
    content: &str,
    tags: Option<&[String]>,
) -> Result<(), McpError> {
    let title_len = title.chars().count();
    if title_len > MAX_TITLE_CHARS {
        return Err(mcp_invalid_parameter(&format!(
            "title is {title_len} chars (max {MAX_TITLE_CHARS})"
        )));
    }
    if content.len() > MAX_CONTENT_BYTES {
        return Err(mcp_invalid_parameter(&format!(
            "content is {} bytes (max {MAX_CONTENT_BYTES})",
            content.len()
        )));
    }
    if let Some(tags) = tags
        && tags.len() > MAX_TAGS
    {
        return Err(mcp_invalid_parameter(&format!(
            "tags has {} entries (max {MAX_TAGS})",
            tags.len()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_fields_boundaries() {
        let ok_title = "t".repeat(MAX_TITLE_CHARS);
        let long_title = "t".repeat(MAX_TITLE_CHARS + 1);
        let ok_content = "c".repeat(MAX_CONTENT_BYTES);
        let long_content = "c".repeat(MAX_CONTENT_BYTES + 1);
        let ok_tags: Vec<String> = (0..MAX_TAGS).map(|i| i.to_string()).collect();
        let long_tags: Vec<String> = (0..=MAX_TAGS).map(|i| i.to_string()).collect();

        assert!(validate_write_fields(&ok_title, &ok_content, Some(&ok_tags)).is_ok());
        assert!(validate_write_fields(&long_title, "", None).is_err());
        assert!(validate_write_fields("", &long_content, None).is_err());
        assert!(validate_write_fields("", "", Some(&long_tags)).is_err());
        // Multibyte titles count chars, not bytes.
        let multibyte_ok = "日".repeat(MAX_TITLE_CHARS);
        assert!(validate_write_fields(&multibyte_ok, "", None).is_ok());
        let multibyte_long = "日".repeat(MAX_TITLE_CHARS + 1);
        assert!(validate_write_fields(&multibyte_long, "", None).is_err());
    }
}
