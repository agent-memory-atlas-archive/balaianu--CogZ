#!/usr/bin/env python3
"""Tombstone integrity: pruned observations must leave the retrieval
surface but stay rebuildable.

Checks, on a fixture repo:
  1. rejected/superseded observations are pruned by
     `doctor --prune-observations --confirm` (file-first: canonical
     file becomes a pruned tombstone with empty title/body).
  2. Pruned entities are absent from search results (FTS + vector
     path — status filter must hold on both).
  3. Pruned entities lose content/embedding/FTS rows but KEEP graph
     edges (audit trail).
  4. reset + index rebuilds the tombstone (status='pruned' survives
     rebuild — the file is canonical).
  5. Active neighbors are untouched.

Usage: tombstone_check.py [--cogz PATH] [--keep]
Exit 0 = all invariants hold; exit 1 = violations printed.
"""

import argparse
import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

COGZ = str(Path(__file__).resolve().parents[1] / "target" / "release" / "cogz")

MAIN_RS = """//! Fixture.

fn target_fn() -> i32 {
    7
}

fn main() {
    println!("{}", target_fn());
}
"""

OBS_TPL = """---
id: {oid}
title: {title}
type: observation
status: {status}
created_at: 2026-01-01T00:00:00Z
updated_at: 2026-01-01T00:00:00Z
references: [{refs}]
---
{body}
"""

OBS_REJECTED = "61111111-1111-4111-8111-111111111111"
OBS_SUPERSEDED = "62222222-2222-4222-8222-222222222222"
OBS_ACTIVE = "63333333-3333-4333-8333-333333333333"


def run(cmd, cwd, **kw):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        sys.exit(f"FAILED {' '.join(map(str, cmd))}\n{r.stdout}\n{r.stderr}")
    return r


def search_ids(repo, query, cogz):
    """FTS-only CLI search → set of returned entity ids."""
    r = subprocess.run(
        [cogz, "search", "--repo", str(repo), query, "--json"],
        capture_output=True, text=True)
    if r.returncode != 0:
        # no --json flag? fall back to parsing text output
        out = r.stdout
        return {tok for tok in out.split() if len(tok) == 36 and "-" in tok}
    try:
        data = json.loads(r.stdout)
        return {x.get("id") or x.get("entity_id") for x in
                (data.get("results") if isinstance(data, dict) else data)}
    except json.JSONDecodeError:
        return {t for t in r.stdout.split() if len(t) == 36 and "-" in t}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cogz", default=COGZ)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp(prefix="cogz_tomb_"))
    try:
        repo = tmp / "repo"
        (repo / "src").mkdir(parents=True)
        (repo / ".cogz" / "observations").mkdir(parents=True)
        (repo / "src" / "main.rs").write_text(MAIN_RS)
        src_cfg = Path(__file__).resolve().parent.parent / ".cogz" / "config.toml"
        (repo / ".cogz" / "config.toml").write_text(
            src_cfg.read_text().replace('name = "CogZ"', 'name = "tombfix"'))

        # Index code first so observations can reference real UUIDs.
        run(["git", "init", "-q"], repo)
        (repo / ".gitignore").write_text(".cogz/cogz.db*\n")
        run(["git", "add", "-A"], repo)
        run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
             "commit", "-qm", "code"], repo)
        run([args.cogz, "index", "--repo", str(repo)], repo)

        db = repo / ".cogz" / "cogz.db"
        conn = sqlite3.connect(db)
        target_id = conn.execute(
            "SELECT id FROM entities WHERE title='target_fn'").fetchone()[0]
        conn.close()

        odir = repo / ".cogz" / "observations"
        (odir / "rej.md").write_text(OBS_TPL.format(
            oid=OBS_REJECTED, title="reject me note", status="rejected",
            refs=f'"{target_id}"',
            body="Rejected observation referencing target_fn."))
        (odir / "sup.md").write_text(OBS_TPL.format(
            oid=OBS_SUPERSEDED, title="supersede me note", status="superseded",
            refs=f'"{target_id}"',
            body="Superseded observation referencing target_fn."))
        (odir / "act.md").write_text(OBS_TPL.format(
            oid=OBS_ACTIVE, title="keep me note", status="active",
            refs=f'"{target_id}"',
            body="Active observation referencing target_fn."))
        run([args.cogz, "reindex", "--repo", str(repo)], repo)

        # Baseline: all three present, edges exist.
        conn = sqlite3.connect(db)
        pre_edges = conn.execute(
            "SELECT source_id, target_id, edge_type FROM edges"
            " WHERE source_id IN (?,?,?)",
            (OBS_REJECTED, OBS_SUPERSEDED, OBS_ACTIVE)).fetchall()
        conn.close()

        run([args.cogz, "doctor", "--repo", str(repo),
             "--prune-observations", "--confirm"], repo)

        conn = sqlite3.connect(db)
        conn.enable_load_extension(True)
        import sqlite_vec
        sqlite_vec.load(conn)
        rows = {r[0]: r for r in conn.execute(
            "SELECT id, status, title, content FROM entities").fetchall()}
        post_edges = conn.execute(
            "SELECT source_id, target_id, edge_type FROM edges"
            " WHERE source_id IN (?,?,?)",
            (OBS_REJECTED, OBS_SUPERSEDED, OBS_ACTIVE)).fetchall()
        emb_counts = {}
        for t in ("knowledge_embeddings", "code_embeddings"):
            for oid in (OBS_REJECTED, OBS_SUPERSEDED):
                emb_counts[f"{t}:{oid[:8]}"] = conn.execute(
                    f"SELECT COUNT(*) FROM {t} WHERE entity_id=?",
                    (oid,)).fetchone()[0]
        fts_rows = conn.execute(
            "SELECT e.id FROM entities_fts f JOIN entities e"
            " ON e.rowid = f.rowid WHERE e.id IN (?,?)"
            " AND (length(f.title) > 0 OR length(f.content) > 0)",
            (OBS_REJECTED, OBS_SUPERSEDED)).fetchall()
        conn.close()

        errors = []
        for oid, name in ((OBS_REJECTED, "rejected"), (OBS_SUPERSEDED, "superseded")):
            row = rows.get(oid)
            if not row or row[1] != "pruned":
                errors.append(f"{name} obs not pruned: {row}")
                continue
            if row[2] or row[3]:
                errors.append(f"{name} obs retains title/content after prune")
        if rows.get(OBS_ACTIVE, (None,))[1] != "active":
            errors.append("active observation was pruned or changed")
        # Declared `references` edges are the canonical contract.
        # auto_references are a derived cache and are legitimately not
        # recreated for pruned entities on rebuild.
        pre_set = {e for e in pre_edges
                   if e[2] == "references" and e[0] != OBS_ACTIVE}
        post_set = {e for e in post_edges
                    if e[2] == "references" and e[0] != OBS_ACTIVE}
        if not pre_set <= post_set:
            errors.append(f"edges lost at prune: {sorted(pre_set - post_set)}")
        if any(emb_counts.values()):
            errors.append(f"embeddings retained on pruned entities: {emb_counts}")

        # Retrieval exclusion: FTS must not retain rows for tombstones.
        if fts_rows:
            errors.append(f"FTS still indexes pruned entities: {fts_rows}")
        hits = search_ids(repo, "rejected observation", args.cogz)
        if OBS_REJECTED in hits or OBS_SUPERSEDED in hits:
            errors.append(f"pruned entities returned by search: {hits}")

        # Rebuild coherence: tombstone files must rebuild as pruned.
        run([args.cogz, "reset", "--repo", str(repo)], repo)
        run([args.cogz, "index", "--repo", str(repo)], repo)
        conn = sqlite3.connect(db)
        rebuilt = dict(conn.execute(
            "SELECT id, status FROM entities WHERE id IN (?,?,?)",
            (OBS_REJECTED, OBS_SUPERSEDED, OBS_ACTIVE)).fetchall())
        reb_edges = conn.execute(
            "SELECT COUNT(*) FROM edges WHERE source_id IN (?,?)"
            " AND edge_type = 'references'",
            (OBS_REJECTED, OBS_SUPERSEDED)).fetchone()[0]
        conn.close()
        if rebuilt.get(OBS_REJECTED) != "pruned" or rebuilt.get(OBS_SUPERSEDED) != "pruned":
            errors.append(f"tombstones did not survive rebuild: {rebuilt}")
        if reb_edges < len(pre_set & post_set):
            errors.append("tombstone references edges lost on rebuild")

        if args.keep:
            print(f"fixture kept at {tmp}")
        if errors:
            for e in errors:
                print(f"  VIOLATION: {e}")
            sys.exit(1)
        print("TOMBSTONE OK: pruned excluded from search/FTS/embeddings, "
              "edges + status survive rebuild")
    finally:
        if not args.keep:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
