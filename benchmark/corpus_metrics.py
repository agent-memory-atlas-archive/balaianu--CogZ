#!/usr/bin/env python3
"""Corpus metrics snapshot — per-corpus stats for the report card.

Collects from each corpus DB + repo:
  entity counts by type/status, edge counts by type, language
  distribution, DB size, embedded coverage, drift rows,
  git commit count, source file/LOC counts.

Usage: corpus_metrics.py CORPUS_DIR [CORPUS_DIR ...]
       corpus_metrics.py --all        # every dir under $COGZ_BENCH_CORPORA
Writes benchmark/results/corpus_metrics.json
"""

import json
import os
import sqlite3
import subprocess
import sys
from pathlib import Path

RESULTS = Path(__file__).parent / "results" / "corpus_metrics.json"
LANG_EXT = {".rs": "rust", ".py": "python", ".ts": "ts", ".tsx": "tsx",
            ".js": "js", ".jsx": "js", ".go": "go", ".sh": "bash"}
SKIP_DIRS = {".git", "node_modules", "target", ".venv", "venv",
             "site-packages", "dist", "build", "vendor", "__pycache__",
             ".cogz", ".mypy_cache", ".pytest_cache"}


def source_stats(root):
    files = 0
    loc = 0
    langs = {}
    for p in root.rglob("*"):
        if not p.is_file() or p.suffix not in LANG_EXT:
            continue
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        files += 1
        lang = LANG_EXT[p.suffix]
        try:
            n = p.read_text(errors="replace").count("\n")
        except OSError:
            n = 0
        loc += n
        langs[lang] = langs.get(lang, 0) + 1
    return files, loc, langs


def db_stats(db_path):
    conn = sqlite3.connect(db_path)
    conn.enable_load_extension(True)
    import sqlite_vec
    sqlite_vec.load(conn)
    out = {}
    out["entities_by_type"] = dict(conn.execute(
        "SELECT type, COUNT(*) FROM entities GROUP BY type").fetchall())
    out["entities_by_status"] = dict(conn.execute(
        "SELECT status, COUNT(*) FROM entities GROUP BY status").fetchall())
    out["edges_by_type"] = dict(conn.execute(
        "SELECT edge_type, COUNT(*) FROM edges GROUP BY edge_type").fetchall())
    out["edges"] = sum(out["edges_by_type"].values())
    out["code_embeddings"] = conn.execute(
        "SELECT COUNT(*) FROM code_embeddings").fetchone()[0]
    out["knowledge_embeddings"] = conn.execute(
        "SELECT COUNT(*) FROM knowledge_embeddings").fetchone()[0]
    out["drift_rows"] = conn.execute(
        "SELECT COUNT(*) FROM entity_drift").fetchone()[0]
    out["db_bytes"] = db_path.stat().st_size
    langs = {}
    for (props,) in conn.execute(
            "SELECT properties FROM entities WHERE type IN"
            " ('function','class','file','module')").fetchall():
        try:
            lang = json.loads(props).get("language", "?")
        except json.JSONDecodeError:
            lang = "?"
        langs[lang] = langs.get(lang, 0) + 1
    out["code_entity_langs"] = langs
    conn.close()
    return out


def git_commits(root):
    r = subprocess.run(["git", "-C", str(root), "rev-list", "--count", "HEAD"],
                       capture_output=True, text=True)
    return int(r.stdout.strip()) if r.returncode == 0 else 0


def collect(root):
    root = Path(root)
    db = root / ".cogz" / "cogz.db"
    files, loc, langs = source_stats(root)
    m = {"corpus": root.name.lower(), "source_files": files, "loc": loc,
         "src_langs": langs, "git_commits": git_commits(root)}
    if db.exists():
        m.update(db_stats(db))
    else:
        m["indexed"] = False
    return m


def main():
    args = sys.argv[1:]
    if args == ["--all"]:
        base = Path(os.environ.get("COGZ_BENCH_CORPORA",
                                   Path(__file__).resolve().parent / "corpora"))
        args = [str(d) for d in sorted(base.iterdir()) if d.is_dir()]
    if not args:
        sys.exit("usage: corpus_metrics.py CORPUS_DIR... | --all")
    allm = {}
    for a in args:
        m = collect(a)
        allm[Path(a).name.lower()] = m
        emb = m.get("code_embeddings", 0) + m.get("knowledge_embeddings", 0)
        print(f"{Path(a).name}: {m['source_files']} files, {m['loc']} LOC, "
              f"{sum(m.get('entities_by_type', {}).values())} entities, "
              f"{m.get('edges', 0)} edges, {emb} embeddings, "
              f"db={m.get('db_bytes', 0) // 1024}KB")
    RESULTS.parent.mkdir(exist_ok=True)
    existing = {}
    if RESULTS.exists():
        existing = json.loads(RESULTS.read_text())
    existing.update(allm)
    RESULTS.write_text(json.dumps(existing, indent=2))
    print(f"wrote {RESULTS.name}")


if __name__ == "__main__":
    main()
