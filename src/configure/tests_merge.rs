//! Tests for `cogz configure` merge semantics — deep-merge, dedup,
//! backups, and preservation of hand-maintained entries.

use super::*;
use serde_json::json;
use std::fs;

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn json_mcp_merge_preserves_existing_servers() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(&mcp, r#"{"mcpServers":{"other":{"command":"other-cmd"}}}"#).unwrap();

    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();

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

    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();

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

    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();

    let merged = read_json(&mcp);
    assert_eq!(merged["mcpServers"]["cogz"]["args"], json!(["mcp-stdio"]));
}

#[test]
fn merge_creates_backup_of_existing_file() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    let original = r#"{"mcpServers":{"other":{"command":"other-cmd"}}}"#;
    fs::write(&mcp, original).unwrap();

    let out = run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();
    assert!(out.contains("backup"), "expected backup note:\n{out}");
    let backup = repo.path().join(".mcp.json.cogz.bak");
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
}

#[test]
fn rerun_is_idempotent_no_backup_no_duplicates() {
    let (repo, home) = dirs();
    run(repo.path(), "devin", Scope::Project, home.path(), "cogz").unwrap();
    let hooks_path = repo.path().join(".devin/hooks.v1.json");
    let first = fs::read_to_string(&hooks_path).unwrap();

    let out = run(repo.path(), "devin", Scope::Project, home.path(), "cogz").unwrap();
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

    run(repo.path(), "devin", Scope::User, home.path(), "cogz").unwrap();

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

    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();

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

    run(repo.path(), "codex", Scope::Project, home.path(), "cogz").unwrap();

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

/// Comments and key order in a hand-maintained Codex config survive
/// the merge — `toml_edit` rewrites the document in place rather than
/// reserializing it.
#[test]
fn codex_toml_merge_preserves_comments() {
    let (repo, home) = dirs();
    let codex = repo.path().join(".codex");
    fs::create_dir_all(&codex).unwrap();
    let cfg = codex.join("config.toml");
    fs::write(
        &cfg,
        "# my codex config\nmodel = \"gpt-5\" # pinned\n\n[mcp_servers.other]\ncommand = \"x\"\n",
    )
    .unwrap();

    run(repo.path(), "codex", Scope::Project, home.path(), "cogz").unwrap();

    let text = fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("# my codex config"));
    assert!(text.contains("model = \"gpt-5\" # pinned"));
    assert!(text.contains("[mcp_servers.cogz]"));
}

/// A hand-wired `[mcp_servers.cogz]` — absolute path, extra env —
/// is kept verbatim.
#[test]
fn codex_toml_merge_preserves_user_cogz_entry() {
    let (repo, home) = dirs();
    let codex = repo.path().join(".codex");
    fs::create_dir_all(&codex).unwrap();
    let cfg = codex.join("config.toml");
    let original = "[mcp_servers.cogz]\ncommand = \"/opt/cogz\"\nargs = [\"mcp-stdio\"]\nenv = { X = \"1\" }\n";
    fs::write(&cfg, original).unwrap();

    run(repo.path(), "codex", Scope::Project, home.path(), "cogz").unwrap();

    assert_eq!(fs::read_to_string(&cfg).unwrap(), original);
}

/// The first backup is `.cogz.bak`; a second overwrite must not
/// clobber it — it gets a `.cogz.bak.<timestamp>` name instead.
#[test]
fn second_backup_uses_timestamped_suffix() {
    let (repo, home) = dirs();
    let mcp = repo.path().join(".mcp.json");
    fs::write(&mcp, r#"{"v":1}"#).unwrap();

    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();
    let bak = repo.path().join(".mcp.json.cogz.bak");
    assert_eq!(fs::read_to_string(&bak).unwrap(), r#"{"v":1}"#);

    fs::write(
        &mcp,
        r#"{"v":2,"mcpServers":{"cogz":{"command":"cogz","args":["mcp-stdio"]}}}"#,
    )
    .unwrap();
    run(
        repo.path(),
        "claude-code",
        Scope::Project,
        home.path(),
        "cogz",
    )
    .unwrap();

    assert_eq!(fs::read_to_string(&bak).unwrap(), r#"{"v":1}"#);
    let stamped: Vec<_> = fs::read_dir(repo.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".mcp.json.cogz.bak.")
        })
        .collect();
    assert_eq!(stamped.len(), 1, "expected one timestamped backup");
    assert_eq!(
        fs::read_to_string(stamped[0].path()).unwrap(),
        r#"{"v":2,"mcpServers":{"cogz":{"command":"cogz","args":["mcp-stdio"]}}}"#
    );
}
