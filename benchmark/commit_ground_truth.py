#!/usr/bin/env python3
"""Commit-derived ground truth: turn git history into labeled
retrieval queries — query = commit message, expected = the code
entities whose line ranges the commit touched.

Entity ids are v5(file_path:type:qualified_name), so expectations stay
valid only for entities that still exist with the same name/path —
the generator filters against the live DB and drops dead ids.

Method:
  git log (skip merges) → per commit: message + `git show -U0` hunks
  → new-side line ranges per file → entities whose [line_start,
  line_end] intersects → expected ids.

Sampling: skips merge/trivial commits (<4 words), huge diffs (>10
files — usually refactors/vendored churn), and commits whose changed
entities all vanished from the current index.

Usage: commit_ground_truth.py REPO [--max N] [--out FILE]
"""

import argparse
import json
import re
import sqlite3
import subprocess
import sys
from pathlib import Path

HUNK = re.compile(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
MIN_WORDS = 4
MAX_FILES = 10


def git(root, *args):
    r = subprocess.run(["git", "-C", str(root), *args],
                       capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ""


def entity_index(db_path):
    """file_path → [(line_start, line_end, entity_id, title)]"""
    conn = sqlite3.connect(db_path)
    rows = conn.execute(
        "SELECT id, title, properties FROM entities"
        " WHERE status='active' AND type IN ('function','class')"
    ).fetchall()
    conn.close()
    by_file = {}
    for eid, title, props in rows:
        try:
            p = json.loads(props)
            fp, ls, le = p.get("file_path"), p.get("line_start"), p.get("line_end")
        except json.JSONDecodeError:
            continue
        if fp and ls and le:
            by_file.setdefault(fp, []).append((int(ls), int(le), eid, title))
    return by_file


def changed_ranges(diff_text):
    """file → [(start, end)] new-side line ranges from -U0 hunks."""
    out = {}
    cur = None
    for line in diff_text.splitlines():
        if line.startswith("+++ b/"):
            cur = line[6:]
            continue
        if line.startswith("+++"):
            cur = None
            continue
        m = HUNK.match(line)
        if m and cur:
            start = int(m.group(1))
            span = int(m.group(2) or 1)
            out.setdefault(cur, []).append((start, start + max(span - 1, 0)))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("repo")
    ap.add_argument("--max", type=int, default=60)
    ap.add_argument("--out", default=None)
    ap.add_argument("--min-commit-age-days", type=int, default=0,
                    help="skip commits newer than this (stabilize ids)")
    args = ap.parse_args()
    repo = Path(args.repo).resolve()
    db = repo / ".cogz" / "cogz.db"
    by_file = entity_index(db)
    if not by_file:
        sys.exit("no indexed entities — run cogz index first")

    rev_args = ["rev-list", "HEAD", "--no-merges", "--max-count=2000"]
    if args.min_commit_age_days:
        rev_args.append(f"--before={args.min_commit_age_days} days ago")
    revs = git(repo, *rev_args).split()

    queries = []
    seen_msgs = set()
    for sha in revs:
        if len(queries) >= args.max:
            break
        msg = git(repo, "log", "-1", "--format=%s", sha).strip()
        words = msg.split()
        if len(words) < MIN_WORDS or msg.lower() in seen_msgs:
            continue
        diff = git(repo, "show", "--format=", "--unified=0", sha)
        ranges = changed_ranges(diff)
        if not ranges or len(ranges) > MAX_FILES:
            continue
        expected = []
        titles = []
        for fp, spans in ranges.items():
            for (ls, le, eid, title) in by_file.get(fp, []):
                if any(ls <= e and s <= le for s, e in spans):
                    expected.append(eid)
                    titles.append(title)
        expected = sorted(set(expected))
        if not expected or len(expected) > 20:
            continue
        seen_msgs.add(msg.lower())
        queries.append({
            "id": f"c{sha[:8]}",
            "intent": "commit",
            "query": msg,
            "expected_entity_ids": expected,
            "expected_titles": sorted(set(titles)),
            "notes": f"commit {sha[:10]}, {len(ranges)} files",
        })

    out = args.out or str(
        Path(__file__).parent / "results" / f"gt_{repo.name}.json")
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    json.dump({"version": "commit-gt-v1", "corpus": repo.name,
               "queries": queries}, open(out, "w"), indent=1)
    print(f"{repo.name}: {len(queries)} commit-derived queries → {out}")


if __name__ == "__main__":
    main()
