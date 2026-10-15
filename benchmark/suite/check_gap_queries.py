#!/usr/bin/env python3
"""Verify gap-query purity: no query term may match the gold entity's
*title* under the real FTS index (porter/unicode61). Content-column
overlap is reported but allowed — a body full of domain words is
legitimate lexical signal; the name is the gap we must preserve.

    python3 check_gap_queries.py --repo <corpus> --queries <gap.json>
"""
import argparse
import json
import sqlite3
import sys


def fts_hits(conn: sqlite3.Connection, column: str, term: str,
             rowid: int) -> int:
    t = term.replace('"', '""')
    return conn.execute(
        f'SELECT COUNT(*) FROM entities_fts '
        f'WHERE entities_fts MATCH ? AND rowid = ?',
        (f'{column} : "{t}"', rowid),
    ).fetchone()[0]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--queries", required=True)
    args = ap.parse_args()

    qs = json.load(open(args.queries))["queries"]
    db = sqlite3.connect(f"file:{args.repo}/.cogz/cogz.db?mode=ro",
                         uri=True)

    bad_title = 0
    body_hits = 0
    checked = 0
    for q in qs:
        terms = q["query"].split()
        for eid in q["expected_entity_ids"]:
            row = db.execute(
                "SELECT rowid, title FROM entities WHERE id = ?",
                (eid,)).fetchone()
            if not row:
                print(f"{q['id']}: MISSING entity {eid}")
                bad_title += 1
                continue
            rowid, title = row
            checked += 1
            leaks = [t for t in terms
                     if fts_hits(db, "title", t, rowid)]
            if leaks:
                print(f"{q['id']}: TITLE LEAK vs {title!r}: {leaks}")
                bad_title += 1
            # Informational: how much of the query still matches the
            # body text — shows how much FTS signal remains via content.
            body = [t for t in terms
                    if fts_hits(db, "content", t, rowid)]
            body_hits += bool(body)
            print(f"{q['id']}: gold={title!r} title-leaks={leaks or 'none'} "
                  f"content-term-hits={len(body)}/{len(terms)}")

    print(f"\n{checked} gold entities checked — "
          f"title leaks: {bad_title}, queries with content overlap: {body_hits}")
    return 1 if bad_title else 0


if __name__ == "__main__":
    sys.exit(main())
