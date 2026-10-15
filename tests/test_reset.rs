//! `cogz reset` safety: --purge requires --confirm (dry-run by
//! default), refuses while a background worker holds `.bg.lock`, and
//! preserves the generated `.gitignore`.

use std::fs;
use std::path::Path;
use std::process::Command;

const COGZ_BIN: &str = env!("CARGO_BIN_EXE_cogz");

fn run(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(COGZ_BIN)
        .args(args)
        .arg("--repo")
        .arg(repo)
        .output()
        .unwrap()
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    let obs = dir.path().join(".cogz/observations");
    fs::write(obs.join("note.md"), "---\nid: test\n---\nbody\n").unwrap();
    let out = run(dir.path(), &["index", "--no-download"]);
    assert!(out.status.success(), "index failed: {out:?}");
    dir
}

#[test]
fn purge_without_confirm_is_dry_run() {
    let repo = init_repo();
    let cogz = repo.path().join(".cogz");

    let out = run(repo.path(), &["reset", "--purge"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Dry run"),
        "expected dry-run notice:\n{stdout}"
    );

    assert!(
        cogz.join("cogz.db").exists(),
        "dry run must not drop the db"
    );
    assert!(cogz.join("observations/note.md").exists());
}

#[test]
fn purge_confirm_removes_observations_keeps_gitignore() {
    let repo = init_repo();
    let cogz = repo.path().join(".cogz");

    let out = run(repo.path(), &["reset", "--purge", "--confirm"]);
    assert!(
        out.status.success(),
        "confirmed purge failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(!cogz.join("cogz.db").exists());
    let obs_dir = cogz.join("observations");
    assert!(obs_dir.exists(), "observations dir itself is recreated");
    assert_eq!(fs::read_dir(&obs_dir).unwrap().count(), 0);
    assert!(
        cogz.join(".gitignore").exists(),
        "generated .gitignore must survive --purge"
    );
    assert!(cogz.join("config.toml").exists());
}

#[test]
fn plain_reset_drops_db_keeps_observations() {
    let repo = init_repo();
    let cogz = repo.path().join(".cogz");

    let out = run(repo.path(), &["reset"]);
    assert!(out.status.success());
    assert!(!cogz.join("cogz.db").exists());
    assert!(cogz.join("observations/note.md").exists());
}

#[test]
fn reset_refuses_while_background_worker_holds_lock() {
    let repo = init_repo();
    let cogz = repo.path().join(".cogz");

    let _guard = cogz::bg_lock::try_acquire(&cogz).expect("test acquires the bg lock");
    let out = run(repo.path(), &["reset"]);
    assert!(
        !out.status.success(),
        "reset must refuse while a bg worker holds .bg.lock"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("background worker"), "{stderr}");
    assert!(
        cogz.join("cogz.db").exists(),
        "refused reset leaves db intact"
    );
}
