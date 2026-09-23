#!/usr/bin/env python3
"""Fetch and index a benchmark corpus at its pinned SHA.

For `github` corpora: clone the repo, check out the pinned SHA, init
`.cogz`, write the shared benchmark config, drop suite seed files into
`.cogz/knowledge/`, and run `cogz index`. For `local` corpora: verify
the path exists and report its HEAD SHA (no fetching).

Usage:
    python3 fetch_corpus.py [--corpus NAME] [--cogz PATH]
        [--skip-index] [--corpora-dir DIR]
"""
import argparse
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

SUITE = Path(__file__).parent


def sh(cmd: list[str], cwd: Path | None = None, capture: bool = False):
    r = subprocess.run(cmd, cwd=cwd, capture_output=capture, text=True)
    if r.returncode != 0:
        sys.exit(f"FAILED {' '.join(cmd)}\n{r.stdout}{r.stderr}")
    return r


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=None, help="only this corpus")
    ap.add_argument(
        "--cogz", default=str(SUITE.parent.parent / "target/release/cogz")
    )
    ap.add_argument("--skip-index", action="store_true")
    ap.add_argument("--corpora-dir", default=None, help="override manifest corpora_dir")
    args = ap.parse_args()

    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    corpora_dir = Path(args.corpora_dir or manifest["suite"]["corpora_dir"])
    corpora_dir.mkdir(parents=True, exist_ok=True)
    template = (SUITE / manifest["suite"]["config_template"]).read_text()

    for c in manifest["corpus"]:
        if args.corpus and c["name"] != args.corpus:
            continue
        if c["source"] == "local":
            repo = Path(c["path"])
            if not repo.is_dir():
                sys.exit(f"{c['name']}: local path missing: {repo}")
            sha = sh(
                ["git", "rev-parse", "HEAD"], cwd=repo, capture=True
            ).stdout.strip()
            print(f"{c['name']}: local corpus at {repo} ({sha[:12]})")
            continue

        repo = corpora_dir / c["name"]
        if not repo.is_dir():
            print(f"{c['name']}: cloning {c['url']}")
            sh(["git", "clone", c["url"], str(repo)])
        cur = sh(["git", "rev-parse", "HEAD"], cwd=repo, capture=True).stdout.strip()
        if cur != c["sha"]:
            print(f"{c['name']}: checkout {c['sha'][:12]}")
            sh(["git", "checkout", c["sha"]], cwd=repo)
        else:
            print(f"{c['name']}: already at {c['sha'][:12]}")

        cogz_dir = repo / ".cogz"
        if not cogz_dir.is_dir():
            sh([args.cogz, "init"], cwd=repo)
        (cogz_dir / "config.toml").write_text(template.replace("{{NAME}}", c["name"]))

        seed_dir = SUITE / "seeds" / c["name"]
        if seed_dir.is_dir():
            for f in seed_dir.rglob("*.md"):
                dest = cogz_dir / "knowledge" / f.relative_to(seed_dir)
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(f, dest)

        if not args.skip_index:
            print(f"{c['name']}: indexing (this can take hours on large corpora)")
            sh([args.cogz, "index"], cwd=repo)
        print(f"{c['name']}: ready")


if __name__ == "__main__":
    main()
