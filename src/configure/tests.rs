//! Tests for `cogz configure` — file plans, merge semantics,
//! backups, idempotency, and detection.

use super::*;
use serde_json::json;

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn each_harness_writes_mcp_entry() {
    let (repo, home) = dirs();
    for id in harness_ids() {
        let out = run(repo.path(), id, Scope::Project, home.path()).unwrap();
        assert!(out.contains("wrote"), "{id} should write files:\n{out}");
        let expected = match id {
            "claude-code" | "copilot" => repo.path().join(".mcp.json"),
            "cursor" => repo.path().join(".cursor/mcp.json"),
            "codex" => repo.path().join(".codex/config.toml"),
            "gemini" => repo.path().join(".gemini/settings.json"),
            "devin" => repo.path().join(".devin/mcp_config.json"),
            other => panic!("unmapped harness {other}"),
        };
        assert!(expected.exists(), "{id} MCP file missing: {expected:?}");
    }
}

#[test]
fn json_mcp_merge_preserves_existing_servers() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(&mcp, r#"{"mcpServers":{"other":{"command":"other-cmd"}}}"#).unwrap();

    run(repo.path(), "claude-code", Scope::Project, home.path()).unwrap();

    let merged = read_json(&mcp);
    assert_eq!(merged["mcpServers"]["other"]["command"], "other-cmd");
    assert_eq!(merged["mcpServers"]["cogz"]["command"], "cogz");
    assert_eq!(merged["mcpServers"]["cogz"]["args"], json!(["mcp-stdio"]));
}

/// A hand-wired `mcpServers.cogz` — absolute binary path, extra keys —
/// is already installed wiring, not a gap: configure must not rewrite
/// it to the canonical bare `cogz` form.
#[test]
fn existing_cogz_mcp_entry_is_preserved() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(
        &mcp,
        r#"{"mcpServers":{"cogz":{"command":"/opt/dev/CogZ/target/release/cogz","args":["mcp-stdio"],"env":{"X":"1"}}}}"#,
    )
    .unwrap();

    run(repo.path(), "claude-code", Scope::Project, home.path()).unwrap();

    let merged = read_json(&mcp);
    assert_eq!(
        merged["mcpServers"]["cogz"]["command"],
        "/opt/dev/CogZ/target/release/cogz"
    );
    assert_eq!(merged["mcpServers"]["cogz"]["env"]["X"], "1");
}

/// A `cogz` entry that doesn't invoke `mcp-stdio` isn't wired yet —
/// configure completes it with the canonical args.
#[test]
fn incomplete_cogz_mcp_entry_is_completed() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(&mcp, r#"{"mcpServers":{"cogz":{"command":"cogz"}}}"#).unwrap();

    run(repo.path(), "claude-code", Scope::Project, home.path()).unwrap();

    let merged = read_json(&mcp);
    assert_eq!(merged["mcpServers"]["cogz"]["args"], json!(["mcp-stdio"]));
}

#[test]
fn merge_creates_backup_of_existing_file() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    let original = r#"{"mcpServers":{"other":{"command":"other-cmd"}}}"#;
    fs::write(&mcp, original).unwrap();

    let out = run(repo.path(), "claude-code", Scope::Project, home.path()).unwrap();
    assert!(out.contains("backup"), "expected backup note:\n{out}");
    let backup = repo.path().join(".mcp.json.cogz.bak");
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
}

#[test]
fn rerun_is_idempotent_no_backup_no_duplicates() {
    let (repo, home) = dirs();
    run(repo.path(), "devin", Scope::Project, home.path()).unwrap();
    let hooks_path = repo.path().join(".devin/hooks.v1.json");
    let first = fs::read_to_string(&hooks_path).unwrap();

    let out = run(repo.path(), "devin", Scope::Project, home.path()).unwrap();
    assert!(
        out.contains("already configured"),
        "rerun should no-op:\n{out}"
    );
    assert_eq!(fs::read_to_string(&hooks_path).unwrap(), first);
    assert!(!repo.path().join(".devin/hooks.v1.json.cogz.bak").exists());
}

/// Hand-written entries equivalent to ours — different flag order,
/// `matcher: ""` instead of absent, absolute binary path, extra
/// hooks sharing the group — must not be duplicated on configure.
#[test]
fn rerun_dedupes_handwritten_equivalents() {
    let (repo, home) = dirs();
    let cfg_dir = home.path().join(".config/devin");
    fs::create_dir_all(&cfg_dir).unwrap();
    let cfg = cfg_dir.join("config.json");
    fs::write(
        &cfg,
        r#"{"hooks":{
            "SessionStart":[
                {"matcher":"","hooks":[
                    {"type":"command","command":"/home/u/.local/bin/cogz capture-event session_start --hook-json","timeout":5},
                    {"type":"command","command":"bash ~/hooks/other.sh","timeout":10}
                ]}
            ],
            "PostToolUse":[
                {"matcher":"","hooks":[{"type":"command","command":"cogz capture-event post_tool_use --hook-json --fts-only","timeout":10}]},
                {"matcher":"edit|write|notebook_edit","hooks":[{"type":"command","command":"cogz capture-event file_save --hook-json --fts-only","timeout":20}]}
            ],
            "UserPromptSubmit":[
                {"matcher":"","hooks":[{"type":"command","command":"cogz capture-event prompt_submit --hook-json --fts-only","timeout":15}]}
            ],
            "Stop":[
                {"matcher":"","hooks":[{"type":"command","command":"cogz capture-event stop --hook-json --fts-only","timeout":5}]}
            ],
            "SessionEnd":[
                {"matcher":"","hooks":[{"type":"command","command":"cogz capture-event session_end --hook-json --fts-only","timeout":30}]}
            ],
            "PostCompaction":[
                {"matcher":"","hooks":[{"type":"command","command":"cogz capture-event session_start --hook-json --fts-only","timeout":15}]}
            ]
        }}"#,
    )
    .unwrap();

    run(repo.path(), "devin", Scope::User, home.path()).unwrap();

    let merged = read_json(&cfg);
    let mut seen = std::collections::HashMap::new();
    for (event, groups) in merged["hooks"].as_object().unwrap() {
        for g in groups.as_array().unwrap() {
            let matcher = g["matcher"].as_str().unwrap_or("");
            for h in g["hooks"].as_array().unwrap() {
                let cmd = h["command"].as_str().unwrap_or("");
                if cmd.contains("cogz capture-event") {
                    *seen
                        .entry((event.as_str(), matcher, command_key(cmd)))
                        .or_insert(0) += 1;
                }
            }
        }
    }
    let dupes: Vec<_> = seen.iter().filter(|(_, n)| **n > 1).collect();
    assert!(dupes.is_empty(), "duplicated hook entries: {dupes:?}");
    // file_save under its own matcher still got wired (2 verbs total
    // for PostToolUse), everything else one verb per event.
    assert_eq!(seen.len(), 7, "expected one entry per (event, verb)");
    // Foreign hook inside a shared group survives.
    let starts = merged["hooks"]["SessionStart"].as_array().unwrap();
    assert!(
        starts.iter().any(|g| g["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["command"].as_str().unwrap().contains("other.sh"))),
        "foreign hook in shared group must survive"
    );
}

#[test]
fn hooks_merge_keeps_other_tools_entries() {
    let (repo, home) = dirs();
    let settings = repo.path().join(".claude");
    fs::create_dir_all(&settings).unwrap();
    let settings = settings.join("settings.json");
    fs::write(
        &settings,
        r#"{"hooks":{"SessionStart":[{"matcher":"","hooks":[{"type":"command","command":"my-tool session","timeout":5}]}]},"model":"opus"}"#,
    )
    .unwrap();

    run(repo.path(), "claude-code", Scope::Project, home.path()).unwrap();

    let merged = read_json(&settings);
    let starts = merged["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(starts.len(), 2, "existing hook + cogz hook");
    assert_eq!(merged["model"], "opus");
}

#[test]
fn codex_mcp_merges_toml() {
    let (repo, home) = dirs();
    let codex = repo.path().join(".codex");
    fs::create_dir_all(&codex).unwrap();
    let cfg = codex.join("config.toml");
    fs::write(
        &cfg,
        "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
    )
    .unwrap();

    run(repo.path(), "codex", Scope::Project, home.path()).unwrap();

    let merged: toml::Value = toml::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
    assert_eq!(merged["model"].as_str().unwrap(), "gpt-5");
    assert_eq!(
        merged["mcp_servers"]["other"]["command"].as_str().unwrap(),
        "x"
    );
    assert_eq!(
        merged["mcp_servers"]["cogz"]["command"].as_str().unwrap(),
        "cogz"
    );
}

#[test]
fn gemini_writes_mcp_and_hooks_to_one_file() {
    let (repo, home) = dirs();
    run(repo.path(), "gemini", Scope::Project, home.path()).unwrap();
    let settings = read_json(&repo.path().join(".gemini/settings.json"));
    assert_eq!(settings["mcpServers"]["cogz"]["command"], "cogz");
    assert!(settings["hooks"]["SessionStart"].is_array());
    assert!(settings["hooks"]["AfterTool"].is_array());
    // Gemini-specific names — UserPromptSubmit/Stop do not exist there.
    assert!(settings["hooks"]["BeforeAgent"].is_array());
    assert!(settings["hooks"]["AfterAgent"].is_array());
    // Gemini timeouts are milliseconds, not seconds.
    let entry = &settings["hooks"]["SessionStart"][0]["hooks"][0];
    assert_eq!(entry["timeout"], json!(15000));
}

#[test]
fn cursor_writes_flat_versioned_hooks() {
    let (repo, home) = dirs();
    run(repo.path(), "cursor", Scope::Project, home.path()).unwrap();
    let hooks = read_json(&repo.path().join(".cursor/hooks.json"));
    assert_eq!(hooks["version"], json!(1));
    // Flat {command, timeout} entries, camelCase event names.
    let entry = &hooks["hooks"]["beforeSubmitPrompt"][0];
    assert!(entry["command"].as_str().unwrap().contains("prompt_submit"));
    // Cursor reads additional_context, not hookSpecificOutput.
    assert!(
        entry["command"]
            .as_str()
            .unwrap()
            .contains("--hook-format cursor")
    );
    assert!(entry.get("hooks").is_none(), "entries must be flat");
    // Native file-edit event; no postToolUse matcher needed.
    assert!(hooks["hooks"]["afterFileEdit"].is_array());
    assert!(hooks["hooks"]["stop"].is_array());
}

#[test]
fn copilot_writes_versioned_hookset_file() {
    let (repo, home) = dirs();
    run(repo.path(), "copilot", Scope::Project, home.path()).unwrap();
    let hooks = read_json(&repo.path().join(".github/hooks/cogz.json"));
    assert_eq!(hooks["version"], json!(1));
    // PascalCase names → Copilot sends snake_case stdin payloads.
    assert!(hooks["hooks"]["SessionStart"].is_array());
    assert!(hooks["hooks"]["UserPromptSubmit"].is_array());
    assert!(hooks["hooks"]["Stop"].is_array());
    // Flat entries use `command` (cross-platform) + `timeoutSec`.
    let entry = &hooks["hooks"]["SessionStart"][0];
    assert_eq!(entry["type"], json!("command"));
    assert_eq!(entry["timeoutSec"], json!(15));
    assert!(entry["command"].as_str().unwrap().contains("session_start"));
    // Copilot reads top-level additionalContext, not hookSpecificOutput.
    assert!(
        entry["command"]
            .as_str()
            .unwrap()
            .contains("--hook-format copilot")
    );
}

/// Codex clamps SessionEnd (and Interrupt) hooks to 3 seconds max —
/// the canonical 30s would be rejected or truncate consolidation.
#[test]
fn codex_session_end_respects_3s_cap() {
    let (repo, home) = dirs();
    run(repo.path(), "codex", Scope::Project, home.path()).unwrap();
    let hooks = read_json(&repo.path().join(".codex/hooks.json"));
    let end = hooks["hooks"]["SessionEnd"].as_array().unwrap();
    assert_eq!(end[0]["hooks"][0]["timeout"], json!(3));
}

#[test]
fn devin_includes_postcompaction() {
    let (repo, home) = dirs();
    run(repo.path(), "devin", Scope::Project, home.path()).unwrap();
    let hooks = read_json(&repo.path().join(".devin/hooks.v1.json"));
    let post = hooks["hooks"]["PostCompaction"].as_array().unwrap();
    assert!(
        post[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("session_start")
    );
}

#[test]
fn user_scope_writes_under_home() {
    let (repo, home) = dirs();
    run(repo.path(), "devin", Scope::User, home.path()).unwrap();
    assert!(home.path().join(".config/devin/mcp_config.json").exists());
    let cfg = read_json(&home.path().join(".config/devin/config.json"));
    assert!(cfg["hooks"]["SessionStart"].is_array());
    assert!(!repo.path().join(".devin").exists());
}

#[test]
fn detect_finds_home_and_repo_markers() {
    let (repo, home) = dirs();
    assert!(detect(repo.path(), home.path()).is_empty());

    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::create_dir_all(repo.path().join(".devin")).unwrap();
    let found = detect(repo.path(), home.path());
    assert_eq!(found, vec!["claude-code", "devin"]);
}

#[test]
fn auto_resolves_detected_harnesses() {
    let (repo, home) = dirs();
    fs::create_dir_all(home.path().join(".cursor")).unwrap();
    let out = run(repo.path(), "auto", Scope::Project, home.path()).unwrap();
    assert!(out.starts_with("cursor:"));
    assert!(repo.path().join(".cursor/mcp.json").exists());
}

#[test]
fn unknown_harness_and_empty_auto_error() {
    let (repo, home) = dirs();
    assert!(run(repo.path(), "nvim", Scope::Project, home.path()).is_err());
    assert!(run(repo.path(), "auto", Scope::Project, home.path()).is_err());
    assert!(run(repo.path(), "auto,devin", Scope::Project, home.path()).is_err());
}

#[test]
fn corrupt_existing_file_errors_without_touching_it() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(&mcp, "{not json").unwrap();
    let result = run(repo.path(), "claude-code", Scope::Project, home.path());
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&mcp).unwrap(), "{not json");
}
