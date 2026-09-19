#!/usr/bin/env python3
"""CogZ regression benchmark runner — drives one persistent `cogz mcp-stdio`
session over newline-delimited JSON-RPC, runs queries.json, dumps raw results.

Usage:
    python3 run.py --repo /path/to/repo --queries queries.json --out results/raw_baseline.json
    python3 run.py --repo ... --with-context          # also get_context task-mode packs
    python3 run.py --repo ... --no-expand             # expand=false on all searches
    python3 run.py --repo ... --code-search           # code_search=true (code model for query)
"""

import argparse
import json
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

ENTITY_TYPES = ["function", "class", "file", "module", "knowledge", "rule", "observation"]


class McpSession:
    """Minimal newline-delimited JSON-RPC client for `cogz mcp-stdio`."""

    def __init__(self, cogz_bin: str, repo: str):
        self.proc = subprocess.Popen(
            [cogz_bin, "mcp-stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=repo,
            text=True,
            bufsize=1,
        )
        self._id = 0
        self._rpc("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "cogz-bench", "version": "0.1"},
        })
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _send(self, msg: dict):
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()

    def _rpc(self, method: str, params: dict, timeout: float = 120.0):
        self._id += 1
        rid = self._id
        self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("mcp-stdio closed stdout unexpectedly")
            msg = json.loads(line)
            if msg.get("id") == rid:
                if "error" in msg:
                    raise RuntimeError(f"RPC error on {method}: {msg['error']}")
                return msg["result"]
        raise TimeoutError(f"timeout on {method}")

    def call_tool(self, name: str, arguments: dict):
        return self._rpc("tools/call", {"name": name, "arguments": arguments})

    def tool_json(self, name: str, arguments: dict):
        """Call a tool and parse its JSON text payload."""
        res = self.call_tool(name, arguments)
        for item in res.get("content", []):
            if item.get("type") == "text":
                return json.loads(item["text"])
        return res

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.terminate()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()


def collect_corpus_ids(sess: McpSession, repo: str) -> set:
    """Corpus = entities default search can return: status='active'.

    Reads the DB directly — list_entities is capped at 1000/type, which
    silently truncates corpora with >1000 functions and registers the
    survivors as falsely 'rotted'.
    """
    db = Path(repo) / ".cogz" / "cogz.db"
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        return {r[0] for r in conn.execute("SELECT id FROM entities WHERE status = 'active'")}
    finally:
        conn.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--queries", default=str(Path(__file__).parent / "queries.json"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--cogz", default="target/release/cogz")
    ap.add_argument("--limit", type=int, default=20)
    ap.add_argument("--with-context", action="store_true")
    ap.add_argument("--no-expand", action="store_true")
    ap.add_argument("--code-search", action="store_true")
    ap.add_argument("--status", default=None, help='entity status filter passed to search, e.g. "all"')
    args = ap.parse_args()

    repo = str(Path(args.repo).resolve())
    qset = json.loads(Path(args.queries).read_text())

    sess = McpSession(args.cogz if Path(args.cogz).is_absolute() else str(Path(repo) / args.cogz), repo)
    try:
        print("collecting corpus ids...", file=sys.stderr)
        corpus_ids = collect_corpus_ids(sess, repo)
        print(f"corpus: {len(corpus_ids)} entities", file=sys.stderr)

        raw = {"repo": repo, "corpus_ids": sorted(corpus_ids), "args": vars(args), "results": []}
        for q in qset["queries"]:
            t0 = time.monotonic()
            try:
                params = {
                    "repo": repo,
                    "query": q["query"],
                    "limit": args.limit,
                    "expand": not args.no_expand,
                    "code_search": args.code_search,
                }
                if args.status:
                    params["status"] = args.status
                data = sess.tool_json("search", params)
            except Exception as exc:
                raw["results"].append({"id": q["id"], "error": str(exc)})
                print(f"  {q['id']}: ERROR {exc}", file=sys.stderr)
                continue
            entry = {
                "id": q["id"],
                "search_mode": data.get("search_mode"),
                "filtered_count": data.get("filtered_count"),
                "signals": data.get("signals"),
                "latency_ms": round((time.monotonic() - t0) * 1000),
                "results": [
                    {
                        "id": r.get("id"),
                        "type": r.get("type"),
                        "title": r.get("title"),
                        "relevance": r.get("relevance"),
                        "graph_path_len": len(r.get("graph_path") or []),
                        "drift_count": r.get("drift_count"),
                        "stale": r.get("stale"),
                    }
                    for r in data.get("results", [])
                ],
            }
            if args.with_context:
                t1 = time.monotonic()
                try:
                    ctx_params = {"repo": repo, "query": q["query"], "mode": "task"}
                    if args.status:
                        ctx_params["include_stale"] = args.status == "all"
                    ctx = sess.tool_json("get_context", ctx_params)
                    entry["context"] = {
                        "latency_ms": round((time.monotonic() - t1) * 1000),
                        "sections": [
                            {"entity_id": s.get("entity_id"), "source": s.get("source"),
                             "title": s.get("title"), "relevance": s.get("relevance"),
                             "drift_count": s.get("drift_count"),
                             "content": s.get("content"),
                             "graph_path": s.get("graph_path"),
                             "graph_path_description": s.get("graph_path_description")}
                            for s in ctx.get("sections", [])
                        ],
                        "metadata": ctx.get("metadata"),
                    }
                except Exception as exc:
                    entry["context"] = {"error": str(exc)}
            raw["results"].append(entry)
            print(f"  {q['id']}: {len(entry['results'])} results, {entry['latency_ms']}ms", file=sys.stderr)
    finally:
        sess.close()

    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(raw, indent=2))
    print(f"wrote {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
