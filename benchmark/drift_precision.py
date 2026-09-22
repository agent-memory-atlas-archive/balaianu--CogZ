#!/usr/bin/env python3
"""Drift-marking precision: does entity_drift flag exactly the
references whose targets changed, and nothing else?

Fixture flow:
  1. Build a repo with code + knowledge files whose `references`
     point at code entity UUIDs. Index → backfill stamps
     `verified_against` baselines.
  2. Edit code: change one function, delete one, leave two
     untouched — one untouched function shares a FILE with the
     changed one (tests entity-granularity, not file-granularity).
  3. reindex → score entity_drift rows against expectations.

Expected: changed/missing rows for true targets only; no rows for
unchanged targets (even in an edited file) or ref-less knowledge.
Exit 0 = precision/recall perfect; exit 1 = divergences printed.

Usage: drift_precision.py [--cogz PATH] [--keep]
"""

import argparse
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

COGZ = "/home/andy/dev/personal/CogZ/target/release/cogz"

MAIN_RS = """//! Fixture binary.

fn alpha() -> i32 {
    1
}

fn beta() -> i32 {
    2
}

fn gamma() -> i32 {
    3
}

fn main() {
    println!("{}", alpha() + beta() + gamma());
}
"""

MAIN_RS_EDITED = """//! Fixture binary.

fn alpha() -> i32 {
    10
}

fn gamma() -> i32 {
    3
}

fn main() {
    println!("{}", alpha() + gamma());
}
"""

OTHER_RS = """//! Second file — untouched.

fn delta() -> i32 {
    4
}
"""

# Knowledge templates; {refs} gets injected references arrays after
# the first index reveals the code entity UUIDs.
K_TPL = """---
id: {kid}
title: {title}
type: knowledge
created_at: 2026-01-01T00:00:00Z
updated_at: 2026-01-01T00:00:00Z
references: [{refs}]
---
{body}
"""


def run(cmd, cwd):
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FAILED {' '.join(map(str, cmd))}\n{r.stdout}\n{r.stderr}")
    return r


def code_ids(db):
    conn = sqlite3.connect(db)
    rows = conn.execute(
        "SELECT id, title FROM entities WHERE type='function'"
    ).fetchall()
    conn.close()
    return {t: i for i, t in rows}


def write_knowledge(root, ids):
    cases = {
        # changed target → expect 'changed'
        "k_change": ("alpha", "51000000-0000-4000-8000-000000000001"),
        # deleted target → expect 'missing'
        "k_gone": ("beta", "51000000-0000-4000-8000-000000000002"),
        # untouched target in edited file → expect NO row
        "k_samefile": ("gamma", "51000000-0000-4000-8000-000000000003"),
        # untouched target in untouched file → expect NO row
        "k_otherfile": ("delta", "51000000-0000-4000-8000-000000000004"),
        # mixed: one changed + one clean → expect exactly 'changed'
        "k_mixed": (["alpha", "delta"], "51000000-0000-4000-8000-000000000005"),
    }
    kdir = root / ".cogz" / "knowledge"
    kdir.mkdir(parents=True, exist_ok=True)
    for name, (targets, kid) in cases.items():
        if isinstance(targets, str):
            targets = [targets]
        refs = ", ".join(f'"{ids[t]}"' for t in targets)
        (kdir / f"{name}.md").write_text(
            K_TPL.format(kid=kid, title=f"case {name}", refs=refs,
                         body=f"Case {name} references {targets}."))
    # Control: knowledge with no references → never drifts.
    (kdir / "k_noref.md").write_text(
        K_TPL.format(kid="51000000-0000-4000-8000-000000000006",
                     title="case k_noref", refs="",
                     body="No references; must never drift."))
    return {name: kid for name, (_, kid) in cases.items()} | {
        "k_noref": "51000000-0000-4000-8000-000000000006"}


def build(root, cogz):
    (root / "src").mkdir(parents=True)
    (root / "src" / "main.rs").write_text(MAIN_RS)
    (root / "src" / "other.rs").write_text(OTHER_RS)
    src_cfg = Path(__file__).resolve().parent.parent / ".cogz" / "config.toml"
    (root / ".cogz").mkdir(exist_ok=True)
    (root / ".cogz" / "config.toml").write_text(
        src_cfg.read_text().replace('name = "CogZ"', 'name = "driftfix"'))
    run(["git", "init", "-q"], root)
    (root / ".gitignore").write_text(".cogz/cogz.db*\n")
    run(["git", "add", "-A"], root)
    run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
         "commit", "-qm", "code baseline"], root)
    run([cogz, "index", "--repo", str(root)], root)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cogz", default=COGZ)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp(prefix="cogz_drift_"))
    try:
        repo = tmp / "repo"
        build(repo, args.cogz)
        db = repo / ".cogz" / "cogz.db"

        ids = code_ids(db)
        needed = ["alpha", "beta", "gamma", "delta"]
        missing = [t for t in needed if t not in ids]
        if missing:
            sys.exit(f"fixture error: code entities not found: {missing}")

        kids = write_knowledge(repo, ids)
        run(["git", "add", "-A"], repo)
        run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
             "commit", "-qm", "knowledge"], repo)
        # Index stamps verified_against baselines for the new refs.
        run([args.cogz, "reindex", "--repo", str(repo)], repo)

        # Edit code: change alpha, delete beta, keep gamma/delta.
        (repo / "src" / "main.rs").write_text(MAIN_RS_EDITED)
        run(["git", "add", "-A"], repo)
        run(["git", "-c", "user.email=t@t", "-c", "user.name=t",
             "commit", "-qm", "code edits"], repo)
        run([args.cogz, "reindex", "--repo", str(repo)], repo)

        conn = sqlite3.connect(db)
        drift = conn.execute(
            "SELECT entity_id, code_id, cause FROM entity_drift"
        ).fetchall()
        status = dict(conn.execute(
            "SELECT id, status FROM entities").fetchall())
        conn.close()

        # Expectations: (entity_id, code_id) → cause
        exp = {
            (kids["k_change"], ids["alpha"]): "changed",
            (kids["k_gone"], ids["beta"]): "missing",
            (kids["k_mixed"], ids["alpha"]): "changed",
        }
        # Everything else must produce no row, notably:
        #   k_samefile→gamma (file edited, entity unchanged)
        #   k_otherfile→delta, k_mixed→delta, k_noref (nothing)
        got = {(e, c): cause for e, c, cause in drift}

        errors = []
        for (e, c), cause in exp.items():
            if got.get((e, c)) != cause:
                errors.append(
                    f"missing/wrong drift row: {e[:8]}→{c[:8]} "
                    f"expected {cause}, got {got.get((e, c))}")
        for (e, c), cause in got.items():
            if (e, c) not in exp:
                errors.append(
                    f"FALSE POSITIVE: {e[:8]}→{c[:8]} cause={cause}")
        tp = sum(1 for k in exp if got.get(k) == exp[k])
        fp = sum(1 for k in got if k not in exp)
        prec = tp / (tp + fp) if tp + fp else 0.0
        rec = tp / len(exp)
        print(f"drift rows: {len(got)} | expected: {len(exp)}")
        print(f"precision={prec:.3f} recall={rec:.3f}")
        # Stale-flag precision: k_gone orphaned → stale; others active.
        for name, kid in kids.items():
            s = status.get(kid)
            print(f"  {name}: status={s}")
        if status.get(kids["k_gone"]) != "stale":
            errors.append("k_gone not flagged stale despite orphaned ref")
        for name in ("k_change", "k_samefile", "k_otherfile",
                     "k_mixed", "k_noref"):
            if status.get(kids[name]) != "active":
                errors.append(f"{name} wrongly {status.get(kids[name])}")

        if args.keep:
            print(f"fixture kept at {tmp}")
        if errors:
            for e in errors:
                print(f"  {e}")
            sys.exit(1)
        print("DRIFT PRECISION: all rows and statuses as expected")
    finally:
        if not args.keep:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
