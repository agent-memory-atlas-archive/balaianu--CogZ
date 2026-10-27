//! Tests for hook payload capture.

use super::*;

#[test]
fn oversized_payload_still_yields_fields() {
    // A PostToolUse over the 1 MiB cap: the output tail is cut
    // mid-string so serde can't parse, but the leading fields
    // survive whole and must still be recorded.
    let big_output = "x".repeat(2 * 1024 * 1024);
    let payload = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"/repo/src/lib.rs"}},"tool_response":{{"success":true,"output":"{big_output}"}}}}"#
    );
    let mut cursor = std::io::Cursor::new(payload.into_bytes());
    let (bytes, capped) = read_stdin_capped(&mut cursor);
    assert!(capped);
    assert_eq!(bytes.len(), 1024 * 1024);

    let raw = String::from_utf8_lossy(&bytes);
    let (prompt, tool_name, tool_result, file_path) = parse_hook_payload(&raw, capped);
    assert_eq!(tool_name.as_deref(), Some("Write"));
    assert_eq!(file_path.as_deref(), Some("/repo/src/lib.rs"));
    // The output string was cut mid-value — it must not leak
    // a half-decode into the event.
    assert_eq!(tool_result, None);
    assert_eq!(prompt, None);
}

#[test]
fn multibyte_char_survives_chunk_boundary() {
    // Place a 4-byte char so its bytes straddle the 8192-byte
    // read boundary — per-chunk lossy decode used to emit U+FFFD
    // here. U+1D11E is 4 bytes (F0 9D 84 9E).
    let prefix_len = 8192 - 2 - "{\"prompt\":\"".len();
    let pad = "a".repeat(prefix_len);
    let payload = format!("{{\"prompt\":\"{pad}𝄞end\"}}");
    assert!(
        payload.as_bytes()[8190..8196]
            .windows(4)
            .any(|w| w == "𝄞".as_bytes()),
        "char must straddle the boundary"
    );

    let mut cursor = std::io::Cursor::new(payload.into_bytes());
    let (bytes, capped) = read_stdin_capped(&mut cursor);
    assert!(!capped);
    let raw = String::from_utf8_lossy(&bytes);
    let (prompt, ..) = parse_hook_payload(&raw, capped);
    let prompt = prompt.unwrap();
    assert!(prompt.contains('𝄞'));
    assert!(!prompt.contains('\u{FFFD}'));
}

#[test]
fn hook_json_silent_skip_emits_nothing() {
    // Strict consumers (Devin post_compact) parse stdout against a
    // typed output schema — a bare `{}` matches no variant and fails.
    // Absent stdout is the portable no-op.
    fn capture_in(repo: &Path) -> CaptureResult {
        let input = CaptureInput {
            repo,
            event_str: "session_start",
            prompt: None,
            prompt_file: None,
            tool_name: None,
            tool_result: None,
            file_path: None,
            hook_json: true,
            fts_only: true,
            hook_format: crate::hooks::format::HookFormat::Claude,
        };
        run_capture_event(&input).expect("silent skip must succeed")
    }

    let bare = tempfile::tempdir().unwrap();
    assert_eq!(capture_in(bare.path()).emission, "");

    let configured = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(configured.path().join(".cogz")).unwrap();
    std::fs::write(
        configured.path().join(".cogz/config.toml"),
        crate::config::default_toml("test"),
    )
    .unwrap();
    assert_eq!(capture_in(configured.path()).emission, "");
}
