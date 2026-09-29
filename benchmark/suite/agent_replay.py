#!/usr/bin/env python3
"""Agent-replay arm: fail-to-pass tasks mined from real fix commits.

For each selected fix commit at <sha>:
  worktree at <sha>^, test-file changes applied (fix withheld) ->
  headless devin run (bare arm) or devin + cogz index (cogz arm) ->
  run the commit's test command -> record pass/fail, wall time, diff.

Usage:
    python3 agent_replay.py --corpus cobra --list
    python3 agent_replay.py --corpus cobra --tasks TASK.json --arm both
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SUITE = Path(__file__).parent
BENCH = SUITE.parent

# Per-corpus: how to spot test files, how to extract the test targets
# added by a commit, and how to run them.
TEST_SPEC = {
    "httpx": {
        "test_re": re.compile(r"^tests/.*\.py$"),
        "func_re": re.compile(r"^\+.*def (test_\w+)", re.M),
        "cmd": "python3 -m pytest {files} -x -q --no-header",
    },
    "cobra": {
        "test_re": re.compile(r"_test\.go$"),
        "func_re": re.compile(r"^\+.*func (Test\w+)", re.M),
        "cmd": "go test ./... -run '{funcs}' -count=1",
    },
    "clap": {
        "test_re": re.compile(r"^tests/(builder|derive|macros|examples)/"
                              r"[^/]+\.rs$|^tests/[^/]+\.rs$"),
        "func_re": re.compile(r"^\+.*fn (\w+)", re.M),
        "cmd": "cargo test -p clap --test {stems}",
    },
    "cogz": {
        # unit tests live in src/**/tests.rs and *_tests.rs modules
        "test_re": re.compile(r"(^|/)tests\.rs$|_tests\.rs$"),
        "func_re": re.compile(r"^\+\s*fn (\w+)", re.M),
        "func_sep": " ",
        # libtest accepts multiple filters (OR'd) after `--`
        "cmd": "cargo test -- {funcs}",
    },
}


def git(repo: Path, *a: str, check: bool = True) -> subprocess.CompletedProcess:
    r = subprocess.run(["git", "-C", str(repo), *a],
                       capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"git {a}: {r.stderr[-400:]}")
    return r


def pair_candidates(repo: Path, spec: dict, since: int = 600) -> list[dict]:
    """For repos (clap) that split fix: and test: into sibling commits.

    A `test(...)` commit T pairs with a `fix|feat` commit F within the
    3 preceding commits. Task = F's message; test payload = T's files.
    """
    log = git(repo, "log", "--no-merges", f"-{since}",
              "--format=%H%x09%s").stdout.splitlines()
    commits = [l.split("\t", 1) for l in log]
    out = []
    for i, (tsha, tsubj) in enumerate(commits):
        if not re.match(r"test(\(|:)", tsubj):
            continue
        tfiles = git(repo, "diff-tree", "--no-commit-id", "-r",
                     "--name-only", tsha).stdout.split()
        tests = [f for f in tfiles if spec["test_re"].search(f)]
        if not tests:
            continue
        for fsha, fsubj in commits[i + 1: i + 4]:
            if not re.match(r"(fix|feat|perf|refactor)(\(|:)", fsubj):
                continue
            ffiles = git(repo, "diff-tree", "--no-commit-id", "-r",
                         "--name-only", fsha).stdout.split()
            src = [f for f in ffiles if not spec["test_re"].search(f)
                   and f.endswith((".rs", ".py", ".go"))]
            if src and len(ffiles) <= 12:
                out.append({"sha": fsha, "test_sha": tsha,
                            "subject": fsubj,
                            "body": git(repo, "log", "-1", "--format=%B",
                                        fsha).stdout.strip(),
                            "test_files": tests, "src_files": src,
                            "test_funcs": []})
                break
    return out


def candidates(repo: Path, spec: dict, since: int = 600) -> list[dict]:
    """Fix commits that add test functions and touch non-test source."""
    log = git(repo, "log", "--no-merges", f"-{since}",
              "--format=%H%x09%s").stdout
    out = []
    for line in log.splitlines():
        sha, subj = line.split("\t", 1)
        files = git(repo, "diff-tree", "--no-commit-id", "-r",
                    "--name-only", sha).stdout.split()
        tests = [f for f in files if spec["test_re"].search(f)]
        src = [f for f in files if not spec["test_re"].search(f)
               and not f.startswith(("docs/", ".github/", "CHANGELOG"))]
        if not tests or not src or len(files) > 12:
            continue
        diff = git(repo, "show", "--format=", "--unified=0",
                   sha, "--", *tests).stdout
        funcs = spec["func_re"].findall(diff)
        if not funcs:
            continue
        body = git(repo, "log", "-1", "--format=%B", sha).stdout.strip()
        out.append({"sha": sha, "subject": subj, "body": body,
                    "test_files": tests, "src_files": src,
                    "test_funcs": sorted(set(funcs))})
    return out


REPLAY_TMP = Path.home() / "cogz_bench" / "replay_tmp"
# shared target dir keeps Rust incremental artifacts off tmpfs
CARGO_TARGET = Path.home() / ".cache" / "cogz-replay-target"


# Commits of history each worktree carries. The co-change channel
# needs real `git log` to mine; the fix commit is a descendant of the
# fetched parent — never transferred — so `git show <fix-sha>` still
# fails while history up to the parent is real.
HISTORY_DEPTH = 500


def setup_worktree(repo: Path, sha: str, task: dict) -> Path:
    """Worktree at parent sha with bounded history.

    A git worktree shares the repo's full history — the agent can just
    `git show <fix-sha>` and copy the answer. Export used to mean git
    archive + fresh init, which starved the co-change channel of the
    history it mines. Instead: temp branch pinned at the parent sha,
    shallow fetch of only its ancestors (descendants never transfer),
    then the test payload applied on top.
    """
    REPLAY_TMP.mkdir(parents=True, exist_ok=True)
    wt = Path(tempfile.mkdtemp(prefix=f"cogz_replay_{sha[:8]}_",
                               dir=REPLAY_TMP))
    parent = git(repo, "rev-parse", f"{sha}^").stdout.strip()
    tmp_branch = f"replay/{sha[:8]}/{wt.name}"
    git(repo, "branch", "-f", tmp_branch, parent)
    try:
        subprocess.run(["git", "init", "-q"], cwd=wt, check=True)
        subprocess.run(
            ["git", "-C", str(wt), "fetch", "-q",
             f"--depth={HISTORY_DEPTH}", str(repo),
             f"{tmp_branch}:refs/heads/main"],
            check=True, capture_output=True, text=True)
    finally:
        git(repo, "branch", "-q", "-D", tmp_branch)
    subprocess.run(["git", "-C", str(wt), "checkout", "-q", "main"],
                   check=True)
    # apply the test payload (fix commit's test files, or paired test
    # commit's) without revealing it in history
    src_sha = task.get("test_sha") or sha
    show = git(repo, "show", "--format=", "--unified=3", src_sha, "--",
               *task["test_files"]).stdout
    subprocess.run(["git", "apply", "--whitespace=nowarn"],
                   input=show, text=True, cwd=wt, check=True)
    subprocess.run(["git", "add", "-A"], cwd=wt, check=True)
    subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
                    "commit", "-qm", "baseline"], cwd=wt, check=True)
    return wt


def purge_stale_code_entities(wt: Path) -> int:
    """Drop code entities whose definitions no longer exist in the tree.

    `cogz index` refreshes content for modified functions but does not
    remove entities for definitions deleted inside a still-existing
    file — copied from the pin-sha DB, they would leak the upstream
    fix's code to the agent. FTS rows are removed by trigger; orphaned
    embedding rows never join back to entities.
    """
    import sqlite3
    # named-definition types only — impl-block titles ("impl X<..> for Y")
    # never literally appear in source and file/module entities have no
    # definable name in the body
    code_types = ("function", "method", "struct", "enum", "trait",
                  "const", "type", "macro", "interface")
    conn = sqlite3.connect(wt / ".cogz" / "cogz.db")
    dead = []
    for eid, fp, title, typ in conn.execute(
            "SELECT id, file_path, title, type FROM entities"
            " WHERE file_path IS NOT NULL"):
        p = wt / fp
        if not p.exists():
            # knowledge-layer entities store paths relative to .cogz/
            if (wt / ".cogz" / fp).exists():
                continue
            dead.append(eid)
        elif typ in code_types and title:
            try:
                if title not in p.read_text(errors="replace"):
                    dead.append(eid)
            except OSError:
                dead.append(eid)
    conn.executemany("DELETE FROM entities WHERE id = ?",
                     [(i,) for i in dead])
    conn.commit()
    conn.close()
    return len(dead)


def cogzify(wt: Path, corpus_root: Path, cogz: str) -> int:
    """Carry the corpus's .cogz into the worktree, then reindex so the
    code index matches the parent-sha tree (only changed files re-parse).

    When the worktree already has a tracked `.cogz` (CogZ dogfoods —
    knowledge files are committed), keep the parent-state files and
    copy only the pin DB for embedding reuse; the purge then drops
    post-parent knowledge entities whose files don't exist here.
    """
    import shutil
    if (wt / ".cogz").exists():
        shutil.copy2(corpus_root / ".cogz" / "cogz.db",
                     wt / ".cogz" / "cogz.db")
    else:
        shutil.copytree(corpus_root / ".cogz", wt / ".cogz",
                        symlinks=True)
    # zero access tracking so post-run counts reflect only the agent's
    # session — the copied DB carries the corpus's own access history
    import sqlite3
    conn = sqlite3.connect(wt / ".cogz" / "cogz.db")
    conn.execute("DELETE FROM entity_access")
    conn.commit()
    conn.close()
    # fresh git history → reindex diff can't resolve the manifest sha;
    # full index reuses embeddings via content-hash for unchanged files
    subprocess.run([cogz, "index", "--repo", str(wt)],
                   capture_output=True, check=True)
    return purge_stale_code_entities(wt)


def agent_run(wt: Path, prompt: str, cogz_dir: Path | None = None,
              timeout: int = 1800) -> dict:
    # The agent calls `cogz` by name — PATH must resolve it to the
    # binary under test, not whatever is installed. A stale install
    # hits schema mismatches on the worktree DB and the agent abandons
    # the tool, silently zeroing the arm's adoption signal.
    env = dict(os.environ)
    if cogz_dir:
        env["PATH"] = f"{cogz_dir}:{env['PATH']}"
    t0 = time.time()
    try:
        r = subprocess.run(
            ["devin", "-p", prompt, "--respect-workspace-trust", "false",
             "--permission-mode", "dangerous"],
            cwd=wt, capture_output=True, text=True, timeout=timeout,
            env=env)
        return {"rc": r.returncode, "wall_s": round(time.time() - t0, 1),
                "out": r.stdout, "err": r.stderr}
    except subprocess.TimeoutExpired as e:
        out = e.stdout or ""
        if isinstance(out, bytes):
            out = out.decode("utf-8", "replace")
        return {"rc": -9, "wall_s": round(time.time() - t0, 1),
                "out": out, "err": "TIMEOUT", "timeout": True}


def run_task(repo: Path, corpus: str, task: dict, arm: str,
             cogz: str, outdir: Path) -> dict:
    wt = setup_worktree(repo, task["sha"], task)
    spec = TEST_SPEC[corpus]
    files = " ".join(task["test_files"])
    funcs = spec.get("func_sep", "|").join(task["test_funcs"])
    # test target = file stem, or the group dir for nested suites
    # (clap's tests/builder/*.rs run via `cargo test --test builder`)
    stems = " ".join(sorted({
        Path(f).parent.name if len(Path(f).parts) > 2 else Path(f).stem
        for f in task["test_files"]}))
    cmd = spec["cmd"].format(files=files, funcs=funcs, stems=stems)
    purged = 0
    if arm == "cogz":
        purged = cogzify(wt, repo, cogz)
        pre = ("This repo is indexed with CogZ (see `.cogz/`). Before "
               "reading code, run `cogz search` and `cogz get-context` "
               "to retrieve relevant knowledge and code entities.\n\n")
    else:
        # tracked .cogz (CogZ dogfoods) would leak knowledge to the
        # bare arm — strip it
        import shutil
        shutil.rmtree(wt / ".cogz", ignore_errors=True)
        pre = ""
    def sanitize(text: str) -> str:
        # strip upstream references that let an agent fetch the real fix
        text = re.sub(r"\(#\d+\)", "", text)
        text = re.sub(r"(?i)(fix(e[sd])?|close[sd]?|resolve[sd]?|see)\s+"
                      r"(#\d+|https?://\S+)", "", text)
        text = re.sub(r"https?://\S+", "", text)
        return text.strip()

    prompt = (f"{pre}Repository: {wt} — a private codebase with no "
              f"upstream. Do not consult external sources, GitHub, or "
              f"the web; work only from the code and tests here. Do not "
              f"read or search files outside {wt} — no other copy of "
              f"this project exists on this machine.\n"
              f"Task: {sanitize(task['subject'])}\n\n"
              f"{sanitize(task['body'])[:1500]}\n\n"
              f"Make the minimal source changes so that `cd {wt} && {cmd}` "
              f"passes. Do not modify test files.")
    # hide the corpus's real history for the agent's lifetime — the
    # upstream fix commit is reachable in <repo>/.git and agents WILL
    # go looking for it
    gitdir = repo / ".git"
    hidden = repo / ".git_replay_off"
    if gitdir.exists():
        gitdir.rename(hidden)
    try:
        res = agent_run(wt, prompt, Path(cogz).parent)
    finally:
        if hidden.exists():
            hidden.rename(gitdir)
    diff = git(wt, "diff", "--stat", check=False).stdout.strip()
    env = {**__import__("os").environ,
           "CARGO_TARGET_DIR": str(CARGO_TARGET)}
    t = subprocess.run(cmd, shell=True, cwd=wt, env=env,
                       capture_output=True, text=True, timeout=900)
    full = res.pop("out", "") or ""
    (outdir / f"{task['sha'][:8]}_{arm}.transcript.txt").write_text(full)
    record = {"corpus": corpus, "arm": arm, "sha": task["sha"],
              "subject": task["subject"], "test_cmd": cmd,
              "test_rc": t.returncode, "passed": t.returncode == 0,
              "agent": {**res, "tail": full[-20000:]},
              "diff_stat": diff,
              "used_cogz": bool(re.search(
                  r"(?i)cogz\s+(search|get-context|status|query)",
                  full + res["err"])),
              "purged_entities": purged,
              "test_tail": t.stdout[-1200:] + t.stderr[-1200:]}
    if arm == "cogz":
        # get_context bumps access_count — deterministic usage signal
        db = wt / ".cogz" / "cogz.db"
        if db.exists():
            q = subprocess.run(
                ["sqlite3", str(db),
                 "SELECT count(*) FROM entity_access WHERE access_count > 0"],
                capture_output=True, text=True, timeout=30)
            try:
                record["cogz_entities_accessed"] = int(q.stdout.strip())
            except ValueError:
                pass
    (outdir / f"{task['sha'][:8]}_{arm}.json").write_text(
        json.dumps(record, indent=1))
    import shutil
    shutil.rmtree(wt, ignore_errors=True)
    return record


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--repo")
    ap.add_argument("--arm", choices=["bare", "cogz", "both"],
                    default="both")
    ap.add_argument("--cogz", default=str(BENCH.parent /
                                          "target/release/cogz"))
    ap.add_argument("--tasks", help="JSON list of candidate dicts")
    ap.add_argument("--n", type=int, default=3)
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    import tomllib
    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    entry = next(c for c in manifest["corpus"] if c["name"] == args.corpus)
    repo = Path(args.repo or (entry["path"] if entry["source"] == "local"
                 else Path(manifest["suite"]["corpora_dir"]) / entry["name"]))

    if args.tasks:
        tasks = json.loads(Path(args.tasks).read_text())
    elif args.corpus == "clap":
        tasks = pair_candidates(repo, TEST_SPEC[args.corpus])
    else:
        tasks = candidates(repo, TEST_SPEC[args.corpus])
    if args.list:
        for t in tasks[:30]:
            print(f"{t['sha'][:8]} {t['subject'][:60]} "
                  f"(tests: {len(t['test_funcs'])}, src: {len(t['src_files'])})")
        sys.exit(0)

    outdir = BENCH / "results/suite" / args.corpus / "agent"
    outdir.mkdir(parents=True, exist_ok=True)
    arms = ["bare", "cogz"] if args.arm == "both" else [args.arm]
    for task in tasks[: args.n]:
        for arm in arms:
            print(f"replay {task['sha'][:8]} [{arm}]: {task['subject'][:55]}")
            rec = run_task(repo, args.corpus, task, arm, args.cogz, outdir)
            print(f"  -> passed={rec['passed']} wall={rec['agent']['wall_s']}s")


if __name__ == "__main__":
    main()
