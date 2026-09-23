#!/usr/bin/env python3
"""Mine git history for observation seed material.

Extracts commit subjects + bodies for interesting commits (fix/feat/
refactor/perf) up to each corpus's pinned SHA, filtered to real
behavior-affecting changes. Emits a candidates JSON per corpus for
curation into observation entities — committed output keeps the
corpus frozen at pin time.

Usage: python3 mine_observations.py [--corpus NAME] [--limit N]
"""
import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

SUITE = Path(__file__).parent
NOISE = re.compile(
    r"^(chore|docs|style|test|build|ci|bump|merge|release|revert)"
    r"|^Merge (pull request|branch)|^Bump |dependency|changelog",
    re.I,
)


def commits(repo: Path, sha: str) -> list[dict]:
    fmt = "%H%x1f%s%x1f%b%x1f%P%x1e"
    out = subprocess.run(
        ["git", "log", "--format=" + fmt, sha],
        cwd=repo, capture_output=True, text=True, check=True,
    ).stdout
    rows = []
    for rec in out.strip().split("\x1e"):
        parts = rec.strip().split("\x1f")
        if len(parts) < 4:
            continue
        h, subj, body, parents = parts[0], parts[1], parts[2], parts[3]
        rows.append({
            "sha": h, "subject": subj.strip(), "body": body.strip(),
            "merge": len(parents.split()) > 1,
        })
    return rows


def files_of(repo: Path, sha: str) -> list[str]:
    out = subprocess.run(
        ["git", "show", "--format=", "--name-only", sha],
        cwd=repo, capture_output=True, text=True,
    ).stdout
    return [l.strip() for l in out.splitlines() if l.strip()]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=None)
    ap.add_argument("--limit", type=int, default=80)
    args = ap.parse_args()

    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    outdir = SUITE / "mined"
    outdir.mkdir(exist_ok=True)

    for c in manifest["corpus"]:
        if c["source"] != "github":
            continue
        if args.corpus and c["name"] != args.corpus:
            continue
        repo = Path(manifest["suite"]["corpora_dir"]) / c["name"]
        if not repo.is_dir():
            sys.exit(f"{c['name']}: not fetched")
        cands = []
        for cm in commits(repo, c["sha"]):
            if cm["merge"] or NOISE.match(cm["subject"]):
                continue
            interesting = bool(re.match(r"^(fix|feat|perf|refactor)", cm["subject"], re.I)) \
                or len(cm["body"]) > 100
            if not interesting:
                continue
            fs = files_of(repo, cm["sha"])
            src = [f for f in fs if re.search(r"\.(py|rs|go|ts|tsx|js)$", f)
                   and "test" not in f.lower()]
            if not src:
                continue
            cands.append({
                "sha": cm["sha"], "subject": cm["subject"],
                "body": cm["body"][:800], "files": src[:8],
            })
            if len(cands) >= args.limit:
                break
        out = outdir / f"{c['name']}_obs_candidates.json"
        out.write_text(json.dumps(cands, indent=1))
        print(f"{c['name']}: {len(cands)} candidates -> {out}")


if __name__ == "__main__":
    main()
