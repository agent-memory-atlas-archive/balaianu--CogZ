//! Render lifecycle outputs for agent consumption — markdown pack
//! text for stdout, and hook-protocol JSON (`additionalContext`).

use crate::context::ContextPack;
use crate::hooks::lifecycle::LifecycleEvent;

/// Build the `additionalContext` markdown for a hook event — pack
/// body, drift notice, or both. `None` means the hook stays silent.
fn hook_additional_context(
    event: &LifecycleEvent,
    pack: Option<&ContextPack>,
    file_path: Option<&str>,
    notice: Option<&str>,
) -> Option<String> {
    let mut markdown = String::new();
    if let Some(pack) = pack {
        let body = format_context_pack(pack);
        // A file_save pack is edit-scoped delivery — the agent needs
        // to know why these rules arrived mid-session.
        markdown = if matches!(event, LifecycleEvent::FileSave) {
            format!(
                "Rules governing `{}` — pushed because you just saved it:\n\n{}",
                file_path.unwrap_or("the saved file"),
                body
            )
        } else {
            body
        };
    }
    if let Some(notice) = notice {
        if !markdown.is_empty() {
            markdown.push('\n');
        }
        markdown.push_str(notice);
    }
    if markdown.is_empty() {
        None
    } else {
        Some(markdown)
    }
}

/// Print a hook-compatible JSON response. A context pack and/or a
/// drift notice are wrapped in `hookSpecificOutput.additionalContext`.
/// When neither is present, prints `{}` (no action).
pub fn print_hook_json(
    event: &LifecycleEvent,
    pack: Option<&ContextPack>,
    file_path: Option<&str>,
    notice: Option<&str>,
) {
    let Some(markdown) = hook_additional_context(event, pack, file_path, notice) else {
        println!("{{}}");
        return;
    };
    let event_name = match event {
        LifecycleEvent::SessionStart => "SessionStart",
        LifecycleEvent::PromptSubmit => "UserPromptSubmit",
        LifecycleEvent::PostToolUse => "PostToolUse",
        LifecycleEvent::SessionEnd => "SessionEnd",
        LifecycleEvent::Stop => "Stop",
        LifecycleEvent::PreToolUse => "PreToolUse",
        LifecycleEvent::FileSave => "PostToolUse",
    };
    let json = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event_name,
            "additionalContext": markdown,
        }
    });
    println!(
        "{}",
        serde_json::to_string(&json).unwrap_or_else(|_| "{}".into())
    );
}

/// Format a context pack as markdown text for agent injection.
fn format_context_pack(pack: &ContextPack) -> String {
    let mut out = String::new();

    for (i, section) in pack.sections.iter().enumerate() {
        let relevance = if section.relevance > 0.0 {
            format!("{:.4}", section.relevance)
        } else {
            "—".to_string()
        };

        out.push_str(&format!(
            "## {}. [{}] {} (relevance: {}){}\n\n",
            i + 1,
            section.source,
            section.title,
            relevance,
            if section.drift_count > 0 {
                format!(
                    " ⚠ drift: {} ref(s) changed since verified — verify before relying",
                    section.drift_count
                )
            } else {
                String::new()
            }
        ));

        if section.graph_path.len() > 1 {
            if !section.graph_path_description.is_empty() {
                out.push_str(&format!(
                    "  graph path: {}\n\n",
                    section.graph_path_description
                ));
            } else {
                out.push_str(&format!(
                    "  graph path: {} -> {}\n\n",
                    section.graph_path.first().unwrap_or(&section.entity_id),
                    section.entity_id
                ));
            }
        }

        out.push_str(&section.content);
        out.push_str("\n\n");
    }

    out.push_str(&drift_footer(
        pack.sections.iter().filter(|s| s.drift_count > 0).count(),
    ));

    out
}

/// Footer line telling the agent how to close the drift loop — the
/// per-section `⚠ drift` marker flags risk, this names the remedy.
fn drift_footer(drifted: usize) -> String {
    if drifted == 0 {
        return String::new();
    }
    format!(
        "---\n{drifted} section(s) above have drifted references. If you confirm one is still \
         accurate, re-stamp it with `cogz verify <entity-id>` or the `verify_knowledge` MCP tool.\n"
    )
}

/// Print a context pack in a format suitable for CLI/human consumption.
pub fn print_context_pack(pack: &ContextPack) {
    if !pack.metadata.dropped_sources.is_empty() {
        eprintln!(
            "Dropped: {} sections over token budget",
            pack.metadata.dropped_sources.len()
        );
    }

    let markdown = format_context_pack(pack);
    print!("{}", markdown);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextMode, ContextSection, PackMetadata};
    use crate::storage::usage::DeliveryTier;

    fn pack_with_drift(drift_counts: &[usize]) -> ContextPack {
        let sections = drift_counts
            .iter()
            .enumerate()
            .map(|(i, &d)| ContextSection {
                source: "knowledge".to_string(),
                entity_id: format!("e{i}"),
                title: format!("Section {i}"),
                content: "body".to_string(),
                relevance: 0.5,
                graph_path: vec![format!("e{i}")],
                graph_path_description: String::new(),
                tier: DeliveryTier::Full,
                drift_count: d,
            })
            .collect();
        ContextPack {
            query: "q".to_string(),
            mode: ContextMode::Task,
            sections,
            metadata: PackMetadata {
                size_tokens: 10,
                selected_sources: vec![],
                dropped_sources: vec![],
                search_mode: "fts_only".to_string(),
                pointer_ids: vec![],
                signals: None,
            },
        }
    }

    #[test]
    fn drift_footer_names_verify_path() {
        let text = format_context_pack(&pack_with_drift(&[0, 2]));
        assert!(text.contains("⚠ drift: 2"));
        assert!(text.contains("1 section(s) above have drifted"));
        assert!(text.contains("cogz verify"));
        assert!(text.contains("verify_knowledge"));
    }

    #[test]
    fn no_footer_without_drift() {
        let text = format_context_pack(&pack_with_drift(&[0, 0]));
        assert!(!text.contains("verify_knowledge"));
        assert!(!text.contains("drifted references"));
    }

    #[test]
    fn notice_alone_produces_additional_context() {
        // Write-time cue with no scoped pack — the hook must still
        // speak, or human edits to ungoverned files drift silently.
        let md = hook_additional_context(
            &LifecycleEvent::FileSave,
            None,
            Some("src/foo.rs"),
            Some("**1 knowledge entity drifted**"),
        )
        .expect("notice alone should produce context");
        assert!(md.contains("knowledge entity drifted"));
    }

    #[test]
    fn notice_appends_to_file_save_pack() {
        let pack = pack_with_drift(&[0]);
        let md = hook_additional_context(
            &LifecycleEvent::FileSave,
            Some(&pack),
            Some("src/foo.rs"),
            Some("NOTICE"),
        )
        .unwrap();
        assert!(md.contains("Rules governing `src/foo.rs`"));
        assert!(md.ends_with("NOTICE"));
    }

    #[test]
    fn empty_when_nothing_to_say() {
        assert!(
            hook_additional_context(&LifecycleEvent::FileSave, None, Some("x"), None).is_none()
        );
    }
}
