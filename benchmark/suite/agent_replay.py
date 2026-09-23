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


def setup_worktree(repo: Path, sha: str, task: dict) -> Path:
    wt = Path(tempfile.mkdtemp(prefix=f"cogz_replay_{sha[:8]}_"))
    git(repo, "worktree", "add", "--detach", str(wt), f"{sha}^")
    # bring in the test payload only (from the fix commit, or from the
    # paired test commit for split fix:/test: histories)
    src_sha = task.get("test_sha") or sha
    git(wt, "checkout", src_sha, "--", *task["test_files"])
    return wt


def cogzify(wt: Path, seed_root: Path, cogz: str) -> None:
    """Fresh .cogz in the worktree: suite seeds + index at parent sha."""
    subprocess.run([cogz, "init", "--repo", str(wt)],
                   capture_output=True, check=True)
    import shutil
    for f in seed_root.rglob("*.md"):
        dest = wt / ".cogz" / f.relative_to(seed_root)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(f, dest)
    subprocess.run([cogz, "index", "--repo", str(wt)],
                   capture_output=True, check=True)


def agent_run(wt: Path, prompt: str, timeout: int = 1800) -> dict:
    t0 = time.time()
    r = subprocess.run(
        ["devin", "-p", prompt, "--respect-workspace-trust", "false",
         "--permission-mode", "dangerous"],
        cwd=wt, capture_output=True, text=True, timeout=timeout)
    return {"rc": r.returncode, "wall_s": round(time.time() - t0, 1),
            "tail": r.stdout[-3000:], "err": r.stderr[-1500:]}


def run_task(repo: Path, corpus: str, task: dict, arm: str,
             cogz: str, outdir: Path) -> dict:
    wt = setup_worktree(repo, task["sha"], task)
    spec = TEST_SPEC[corpus]
    files = " ".join(task["test_files"])
    funcs = "|".join(task["test_funcs"])
    # test target = file stem, or the group dir for nested suites
    # (clap's tests/builder/*.rs run via `cargo test --test builder`)
    stems = " ".join(sorted({
        Path(f).parent.name if len(Path(f).parts) > 2 else Path(f).stem
        for f in task["test_files"]}))
    cmd = spec["cmd"].format(files=files, funcs=funcs, stems=stems)
    if arm == "cogz":
        cogzify(wt, SUITE / "seeds" / corpus, cogz)
        pre = ("This repo is indexed with CogZ (see `.cogz/`). Before "
               "reading code, run `cogz search` and `cogz get-context` "
               "to retrieve relevant knowledge and code entities.\n\n")
    else:
        pre = ""
    prompt = (f"{pre}Repository: {wt} (checked out just before a fix).\n"
              f"Task: {task['subject']}\n\n{task['body'][:1500]}\n\n"
              f"Make the minimal source changes so that `cd {wt} && {cmd}` "
              f"passes. Do not modify test files.")
    res = agent_run(wt, prompt)
    diff = git(wt, "diff", "--stat", check=False).stdout.strip()
    t = subprocess.run(cmd, shell=True, cwd=wt, capture_output=True,
                       text=True, timeout=900)
    record = {"corpus": corpus, "arm": arm, "sha": task["sha"],
              "subject": task["subject"], "test_cmd": cmd,
              "test_rc": t.returncode, "passed": t.returncode == 0,
              "agent": res, "diff_stat": diff,
              "test_tail": t.stdout[-1200:] + t.stderr[-1200:]}
    (outdir / f"{task['sha'][:8]}_{arm}.json").write_text(
        json.dumps(record, indent=1))
    git(repo, "worktree", "remove", "--force", str(wt), check=False)
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
