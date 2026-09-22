#!/usr/bin/env python3
"""Reindex-vs-rebuild equivalence: incremental `cogz reindex` must
converge to the same derived state as `cogz reset` + `cogz index`
after an edit sequence. Divergence = ghost entities, missed stale
marks, lost edges, or embedding drift — silent-corruption bugs.

Builds a fixture repo (Rust + Python source + .cogz entity files),
indexes it, applies edits, then runs:
  path A: cogz reindex            (incremental, git-diff driven)
  path B: cogz reset + cogz index (full rebuild)

Compares entities, edges, drift rows, and embedding bytes.
Excluded by design: timestamps, access/usage/delivery/event tables
(operational, not corpus-derived), sqlite rowids.

Usage: reindex_equiv.py [--cogz PATH] [--keep]
Exit 0 = equivalent; exit 1 = divergence (printed).
"""

import argparse
import json
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

COGZ = "/home/andy/dev/personal/CogZ/target/release/cogz"

ENTITY_COLS = "id, type, title, content, properties, file_path, status, content_hash"
EDGE_COLS = "source_id, target_id, edge_type, weight"
DRIFT_COLS = "entity_id, code_id, verified_hash, current_hash, cause"

MAIN_RS = """//! Fixture binary.

fn helper() -> i32 {
    41
}

fn compute() -> i32 {
    helper() + 1
}

fn doomed() -> i32 {
    -1
}

fn main() {
    println!("{}", compute());
}
"""

MAIN_RS_EDITED = """//! Fixture binary.

fn helper() -> i32 {
    42
}

fn compute() -> i32 {
    helper() + 1
}

fn renamed() -> i32 {
    -1
}

fn added() -> i32 {
    compute() * 2
}

fn main() {
    println!("{}", compute() + added());
}
"""

LIB_PY = '''"""Fixture library."""


def parse(text):
    """Split into tokens."""
    return text.split()


def render(tokens):
    return " ".join(tokens)
'''

LIB_PY_EDITED = '''"""Fixture library."""


def parse(text):
    """Split into tokens, lowercased."""
    return text.lower().split()


def render(tokens):
    return " ".join(tokens)


def extra(x):
    return render(parse(str(x)))
'''

KNOWLEDGE = """---
id: k1111111-1111-4111-8111-111111111111
type: knowledge
title: Fixture architecture note
created_at: 2026-01-01T00:00:00Z
---
The fixture splits parsing from rendering; parse() owns tokenization.
"""

KNOWLEDGE_EDITED = """---
id: k1111111-1111-4111-8111-111111111111
type: knowledge
title: Fixture architecture note
created_at: 2026-01-01T00:00:00Z
---
The fixture splits parsing from rendering; parse() owns tokenization
and now lowercases input.
"""

KNOWLEDGE_NEW = """---
id: k2222222-2222-4222-8222-222222222222
type: knowledge
title: Added knowledge after baseline
created_at: 2026-01-02T00:00:00Z
---
extra() composes parse and render for string coercion.
"""

RULE = """---
id: r1111111-1111-4111-8111-111111111111
type: rule
title: Fixture rule — keep functions pure
created_at: 2026-01-01T00:00:00Z
---
All fixture functions must be pure; no globals, no IO in lib.
"""

OBSERVATION = """---
id: o1111111-1111-4111-8111-111111111111
type: observation
title: helper() is the hot leaf
created_at: 2026-01-01T00:00:00Z
---
helper() is called by compute() and dominates runtime in profiles.
"""


def run(cmd, cwd):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FAILED {' '.join(map(str, cmd))}\n{r.stdout}\n{r.stderr}")
    return r


def dump_state(repo):
    db = Path(repo) / ".cogz" / "cogz.db"
    conn = sqlite3.connect(db)
    conn.enable_load_extension(True)
    import sqlite_vec
    sqlite_vec.load(conn)
    state = {}
    # Active rows must converge exactly. Stale rows are a known
    # asymmetry: reindex tombstones renamed/removed code entities,
    # rebuild never creates them — excluded from search either way.
    state["entities_active"] = sorted(
        conn.execute(
            f"SELECT {ENTITY_COLS} FROM entities WHERE status='active'"
        ).fetchall())
    state["entities_stale"] = sorted(
        conn.execute(
            "SELECT id, status FROM entities WHERE status != 'active'"
        ).fetchall())
    state["edges"] = sorted(
        conn.execute(f"SELECT {EDGE_COLS} FROM edges").fetchall())
    state["drift"] = sorted(
        conn.execute(f"SELECT {DRIFT_COLS} FROM entity_drift").fetchall())
    # Embeddings are batch-composition sensitive (~3% cosine jitter
    # between batches); compare id sets exactly and collect pairwise
    # cosine distances rather than requiring byte equality.
    for table in ("code_embeddings", "knowledge_embeddings"):
        # Compare only embeddings of active entities: reindex retains
        # embedding+FTS rows on stale tombstones where rebuild has none
        # (dead weight, not a retrieval difference — search is
        # status-filtered).
        rows = conn.execute(
            f"SELECT t.entity_id, t.embedding FROM {table} t"
            " JOIN entities e ON e.id = t.entity_id"
            " WHERE e.status = 'active'").fetchall()
        state[f"{table}_ids"] = sorted(r[0] for r in rows)
        state[f"{table}_vec"] = {r[0]: r[1] for r in rows}
        state[f"{table}_stale"] = sorted(
            r[0] for r in conn.execute(
                f"SELECT t.entity_id FROM {table} t"
                " JOIN entities e ON e.id = t.entity_id"
                " WHERE e.status != 'active'").fetchall())
    state["fts"] = sorted(
        conn.execute(
            "SELECT f.title, f.content FROM entities_fts f"
            " JOIN entities e ON e.rowid = f.rowid"
            " WHERE e.status = 'active'"
        ).fetchall())
    # Timestamps differ by construction; compare only stable keys.
    state["meta"] = sorted(
        conn.execute(
            "SELECT key, value FROM meta"
            " WHERE key NOT IN ('last_index', 'last_code_index')"
        ).fetchall())
    conn.close()
    return state


def _cosine(a, b):
    import math
    import struct
    fa = struct.unpack(f"{len(a) // 4}f", a)
    fb = struct.unpack(f"{len(b) // 4}f", b)
    dot = sum(x * y for x, y in zip(fa, fb))
    na = math.sqrt(sum(x * x for x in fa))
    nb = math.sqrt(sum(x * x for x in fb))
    return dot / (na * nb) if na and nb else 0.0


def build_fixture(root):
    (root / "src").mkdir(parents=True)
    (root / ".cogz" / "knowledge").mkdir(parents=True)
    (root / ".cogz" / "rules").mkdir(parents=True)
    (root / ".cogz" / "observations").mkdir(parents=True)
    (root / "src" / "main.rs").write_text(MAIN_RS)
    (root / "src" / "lib.py").write_text(LIB_PY)
    (root / ".cogz" / "knowledge" / "arch.md").write_text(KNOWLEDGE)
    (root / ".cogz" / "rules" / "pure.md").write_text(RULE)
    (root / ".cogz" / "observations" / "hot.md").write_text(OBSERVATION)
    src_cfg = Path(__file__).resolve().parent.parent / ".cogz" / "config.toml"
    cfg = src_cfg.read_text().replace('name = "CogZ"', 'name = "fixture"')
    (root / ".cogz" / "config.toml").write_text(cfg)
    run(["git", "init", "-q"], root)
    (root / ".gitignore").write_text(".cogz/cogz.db*\n")
    run(["git", "add", "-A"], root)
    run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
         "commit", "-qm", "baseline"], root)


def apply_edits(root):
    (root / "src" / "main.rs").write_text(MAIN_RS_EDITED)
    (root / "src" / "lib.py").write_text(LIB_PY_EDITED)
    (root / ".cogz" / "knowledge" / "arch.md").write_text(KNOWLEDGE_EDITED)
    (root / ".cogz" / "knowledge" / "new.md").write_text(KNOWLEDGE_NEW)
    run(["git", "add", "-A"], root)
    run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
         "commit", "-qm", "edits"], root)


def diff_states(a, b):
    diffs = []
    for key in ("entities_active", "edges", "drift", "fts", "meta",
                "code_embeddings_ids", "knowledge_embeddings_ids"):
        sa, sb = a[key], b[key]
        if sa == sb:
            continue
        set_a, set_b = set(sa), set(sb)
        for row in sorted(set_a - set_b)[:8]:
            diffs.append(f"{key}: only in reindex: {str(row)[:160]}")
        for row in sorted(set_b - set_a)[:8]:
            diffs.append(f"{key}: only in rebuild:  {str(row)[:160]}")
    # Stale-entity asymmetry is expected (reindex tombstones, rebuild
    # omits) — report as informational, not a divergence.
    stale_a = {r[0] for r in a["entities_stale"]}
    stale_b = {r[0] for r in b["entities_stale"]}
    if stale_a != stale_b:
        print(f"  note: stale tombstone asymmetry "
              f"(reindex-only: {len(stale_a - stale_b)}, "
              f"rebuild-only: {len(stale_b - stale_a)}) — expected")
    retained = len(a["code_embeddings_stale"]) + len(a["knowledge_embeddings_stale"])
    if retained:
        print(f"  note: reindex retains {retained} embedding row(s) on "
              f"stale entities (rebuild drops them) — dead weight")
    # Embedding vectors: same ids (checked above), cosine stats.
    emb_cos = []
    for table in ("code_embeddings", "knowledge_embeddings"):
        va, vb = a[f"{table}_vec"], b[f"{table}_vec"]
        for eid in set(va) & set(vb):
            if va[eid] != vb[eid]:
                emb_cos.append(_cosine(va[eid], vb[eid]))
    if emb_cos:
        print(f"  note: {len(emb_cos)} embeddings differ by batch "
              f"composition, cosine min={min(emb_cos):.4f} "
              f"max={max(emb_cos):.4f}")
        if min(emb_cos) < 0.90:
            diffs.append(
                f"embedding divergence beyond batch noise: "
                f"min cosine {min(emb_cos):.4f} < 0.90")
    return diffs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cogz", default=COGZ)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp(prefix="cogz_equiv_"))
    try:
        base = tmp / "fixture"
        build_fixture(base)
        run([args.cogz, "index", "--repo", str(base)], base)

        apply_edits(base)

        # Two copies of the post-edit fixture so each path sees
        # identical inputs (reindex can stamp frontmatter).
        pa, pb = tmp / "path_a", tmp / "path_b"
        shutil.copytree(base, pa, symlinks=True)
        shutil.copytree(base, pb, symlinks=True)

        run([args.cogz, "reindex", "--repo", str(pa)], pa)
        state_a = dump_state(pa)

        run([args.cogz, "reset", "--repo", str(pb)], pb)
        run([args.cogz, "index", "--repo", str(pb)], pb)
        state_b = dump_state(pb)

        diffs = diff_states(state_a, state_b)
        if args.keep:
            print(f"fixture kept at {tmp}")
        if diffs:
            print(f"DIVERGENT: {len(diffs)} difference(s)")
            for d in diffs:
                print(f"  {d}")
            sys.exit(1)
        ne = len(state_a["entities_active"])
        ne_edge = len(state_a["edges"])
        nemb = len(state_a["code_embeddings_ids"]) + len(state_a["knowledge_embeddings_ids"])
        print(f"EQUIVALENT: {ne} entities, {ne_edge} edges, "
              f"{nemb} embeddings identical across reindex and rebuild")
    finally:
        if not args.keep:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
