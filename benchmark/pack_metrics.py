#!/usr/bin/env python3
"""Per-corpus pack/latency metrics: pack composition, silence-gate,
FTS-only vs hybrid latency, budget binding. Runs one MCP session per
config state. Usage: pack_metrics.py --repo <path> [--queries q.json]
[--fts-only] [--out out.json]"""
import argparse, json, os, re, statistics, subprocess, sys, time
from pathlib import Path

COGZ = str(Path(__file__).resolve().parents[1] / "target" / "release" / "cogz")

DEFAULT_QUERIES = [
    "how does the main entry point work",
    "error handling strategy",
    "configuration and initialization",
    "data validation logic",
    "how are requests processed",
]
SILENCE_QUERIES = [
    "quantum entanglement decoherence shielding",
    "xyzzy plover unsupported nonsense terms",
    "flibberty gibbet wampus ziggurat",
]

def rpc(proc, method, params, rid):
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}) + "\n")
    proc.stdin.flush()
    while True:
        msg = json.loads(proc.stdout.readline())
        if msg.get("id") == rid:
            return msg["result"]

def open_mcp(repo):
    proc = subprocess.Popen([COGZ, "mcp-stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, cwd=repo, text=True, bufsize=1)
    rpc(proc, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                             "clientInfo": {"name": "pm", "version": "0"}}, 0)
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    proc.stdin.flush()
    return proc

def search(proc, repo, q, rid):
    t0 = time.monotonic()
    res = rpc(proc, "tools/call", {"name": "search", "arguments": {"repo": repo, "query": q, "limit": 20}}, rid)
    lat = (time.monotonic() - t0) * 1000
    d = json.loads(res["content"][0]["text"])
    return lat, d.get("results", [])

def get_context(proc, repo, q, tokens, rid):
    res = rpc(proc, "tools/call", {"name": "get_context",
                                   "arguments": {"repo": repo, "query": q, "mode": "task", "max_tokens": tokens}}, rid)
    return json.loads(res["content"][0]["text"])

def rss_mb(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("VmRSS"):
                return int(line.split()[1]) // 1024
    except (OSError, ValueError):
        pass
    return None

def percentile(vals, p):
    vals = sorted(vals)
    if not vals:
        return None
    k = (len(vals) - 1) * p
    lo, hi = int(k), min(int(k) + 1, len(vals) - 1)
    return vals[lo] + (vals[hi] - vals[lo]) * (k - lo)

def run_state(repo, queries, silence, pack_tokens, label):
    proc = open_mcp(repo)
    out = {"label": label, "search": {}, "packs": []}
    rid = 10
    lats = []
    out["search"]["ram_idle_mb"] = rss_mb(proc.pid)
    # warm-up query (model load included, kept separate)
    lat, _ = search(proc, repo, queries[0], rid); rid += 1
    out["search"]["cold_first_ms"] = round(lat, 1)
    out["search"]["ram_loaded_mb"] = rss_mb(proc.pid)
    reps = 3 if len(queries) <= 6 else 2
    for i in range(reps):
        for q in queries:
            lat, results = search(proc, repo, q, rid); rid += 1
            lats.append(lat)
    lats_sorted = sorted(lats)
    out["search"]["n"] = len(lats)
    out["search"]["p50_ms"] = round(percentile(lats_sorted, 0.50), 1)
    out["search"]["p95_ms"] = round(percentile(lats_sorted, 0.95), 1)
    out["search"]["p99_ms"] = round(percentile(lats_sorted, 0.99), 1)
    out["search"]["mean_ms"] = round(statistics.mean(lats_sorted), 1)
    # silence gate
    neg_hits = 0
    for q in silence:
        _, results = search(proc, repo, q, rid); rid += 1
        if results:
            neg_hits += 1
    out["search"]["silence_gate"] = {"queries": len(silence), "with_hits": neg_hits}
    # pack composition + budget binding
    for q in queries:
        d = get_context(proc, repo, q, pack_tokens, rid); rid += 1
        md = d["metadata"]
        src = {}
        for s in d["sections"]:
            key = f"{s['source']}/{s.get('tier', '?')}"
            src[key] = src.get(key, 0) + 1
        out["packs"].append({
            "query": q, "sections": len(d["sections"]), "size_tokens": md["size_tokens"],
            "budget": pack_tokens, "dropped": len(md.get("dropped_sources", [])),
            "composition": src,
        })
    proc.terminate()
    proc.wait(timeout=10)
    return out

def set_fts_only(repo, on):
    cfg = os.path.join(repo, ".cogz", "config.toml")
    txt = open(cfg).read()
    if on:
        txt = re.sub(r'^code_model\s*=.*$', 'code_model = "fts_only_probe"', txt, flags=re.M)
        txt = re.sub(r'^knowledge_model\s*=.*$', 'knowledge_model = "fts_only_probe"', txt, flags=re.M)
        txt = re.sub(r'^auto_download\s*=.*$', 'auto_download = false', txt, flags=re.M)
    else:
        txt = open(cfg + ".bak").read()
    open(cfg, "w").write(txt)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--queries", help="JSON list of query strings (default: generic)")
    ap.add_argument("--pack-tokens", type=int, default=8192)
    ap.add_argument("--fts-only", action="store_true", help="run only the FTS arm")
    ap.add_argument("--out")
    a = ap.parse_args()
    queries = json.load(open(a.queries)) if a.queries else DEFAULT_QUERIES
    cfg = os.path.join(a.repo, ".cogz", "config.toml")
    created_backup = not os.path.exists(cfg + ".bak")
    if created_backup:
        import shutil; shutil.copy(cfg, cfg + ".bak")
    states = []
    if not a.fts_only:
        states.append(run_state(a.repo, queries, SILENCE_QUERIES, a.pack_tokens, "hybrid"))
    set_fts_only(a.repo, True)
    try:
        states.append(run_state(a.repo, queries, SILENCE_QUERIES, a.pack_tokens, "fts_only"))
    finally:
        set_fts_only(a.repo, False)
        if created_backup and os.path.exists(cfg + ".bak"):
            os.remove(cfg + ".bak")
    result = {"corpus": Path(a.repo).name.lower(), "pack_tokens": a.pack_tokens, "states": states}
    print(json.dumps(result, indent=1))
    if a.out:
        json.dump(result, open(a.out, "w"), indent=1)

if __name__ == "__main__":
    main()
