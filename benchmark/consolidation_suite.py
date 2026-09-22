#!/usr/bin/env python3
"""P5 consolidation suite: dedup precision/recall, NLI contradiction
catch rate vs false alarms, observation->rule promotion thresholds.
Drives create_entity/consolidate over mcp-stdio in a fresh fixture repo.
Usage: consolidation_suite.py [--out out.json]"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time

COGZ = "/home/andy/dev/personal/CogZ/target/release/cogz"

def rpc(proc, method, params, rid):
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}) + "\n")
    proc.stdin.flush()
    while True:
        msg = json.loads(proc.stdout.readline())
        if msg.get("id") == rid:
            return msg["result"]

class Mcp:
    def __init__(self, repo):
        self.proc = subprocess.Popen([COGZ, "mcp-stdio"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                     cwd=repo, text=True, bufsize=1)
        self.repo = repo
        self.rid = 0
        rpc(self.proc, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                      "clientInfo": {"name": "p5", "version": "0"}}, 0)
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.proc.stdin.flush()

    def call(self, tool, args):
        self.rid += 1
        args = dict(args); args["repo"] = self.repo
        res = rpc(self.proc, "tools/call", {"name": tool, "arguments": args}, self.rid)
        return json.loads(res["content"][0]["text"])

    def close(self):
        self.proc.terminate()
        self.proc.wait(timeout=10)

# ---------- fixtures ----------
SEED_KNOWLEDGE = [
    ("k-storage", "Storage uses SQLite with WAL mode",
     "The entity store is a single SQLite database running in WAL mode. All entity state lives in .cogz/cogz.db."),
    ("k-search", "Search uses hybrid FTS plus vector KNN fused via RRF",
     "Hybrid retrieval runs FTS5 and vector KNN in parallel and fuses the ranked lists with reciprocal rank fusion."),
    ("k-packs", "Context packs assemble baseline plus query-relevant sections",
     "Context assembly combines baseline rules with query-scored sections and fits them under a token budget."),
    ("k-models", "ONNX models run on CPU with int8 quantization",
     "Embedding and NLI inference use ONNX Runtime on CPU. Models are int8-quantized for memory and speed."),
    ("k-uuids", "Entity IDs are UUID strings, never integers",
     "Every entity ID is a UUID string in file frontmatter and the database. Integer IDs are not used anywhere."),
    ("k-ffirst", "Files are canonical; the DB is a derived artifact",
     "Every write lands in a markdown file first. The database is rebuilt from files and is disposable."),
]

TRUE_DUPES = [
    ("td1", "Storage layer is SQLite running in WAL mode",
     "Entity state lives in one SQLite database configured for WAL journaling.", "k-storage"),
    ("td2", "Storage uses SQLite with WAL",
     "The entity store is a single SQLite database running in WAL mode.", "k-storage"),
    ("td3", "Retrieval fuses FTS and KNN results with RRF",
     "Hybrid search runs full-text and vector retrieval in parallel and merges rankings via reciprocal rank fusion.", "k-search"),
    ("td4", "The canonical store is markdown files; DB is derived",
     "Writes go to files first. The SQLite database is a disposable derived artifact rebuilt from those files.", "k-ffirst"),
    ("td5", "Context packs assemble baseline plus query-relevant sections",
     "Context assembly combines baseline rules with query-scored sections under a token budget.", "k-packs"),
    ("td6", "All entity identifiers are UUID strings",
     "Entity IDs are UUID strings throughout the system; integer identifiers are never used.", "k-uuids"),
]

NEAR_DUPES = [
    ("nd1", "WAL mode requires SQLite 3.7 or newer",
     "Write-ahead logging was added in SQLite 3.7. Older versions lack WAL support entirely.", "k-storage"),
    ("nd2", "RRF uses rank constant k=60",
     "Reciprocal rank fusion computes 1/(k+rank) with k=60, the standard constant from the original paper.", "k-search"),
    ("nd3", "Pack budgets use the chars-per-token heuristic",
     "Token estimation divides character count by four. No external tokenizer dependency is used.", "k-packs"),
    ("nd4", "int8 quantization cuts model size roughly 4x",
     "Quantizing weights to int8 reduces memory footprint with modest quality loss on retrieval tasks.", "k-models"),
    ("nd5", "Code entities use deterministic UUID v5",
     "Function and class IDs are UUID v5 derived from file path and qualified name, stable across rebuilds.", "k-uuids"),
]

DISTINCT = [
    ("dd1", "The renderer uses a two-pass layout algorithm",
     "Layout runs measure-then-position passes so intrinsic sizes propagate before placement."),
    ("dd2", "Authentication tokens expire after 24 hours",
     "Session tokens carry a 24-hour TTL. Expired tokens are rejected at the auth middleware."),
    ("dd3", "Batch inserts run in a single transaction",
     "Bulk entity inserts wrap all writes in one transaction for atomicity and speed."),
    ("dd4", "Structured logs go to stderr as JSON",
     "All diagnostics emit JSON lines on stderr. Stdout is reserved for command output."),
]

CONTRA_SEED = [
    ("cs1", "The index batch size is 64 entities",
     "Embedding batches process 64 entities per pass through the ONNX session."),
    ("cs2", "Session tokens expire after 24 hours",
     "Auth middleware rejects tokens older than 24 hours. The TTL is fixed in config."),
    ("cs3", "The search pipeline runs FTS before vector KNN",
     "Full-text retrieval executes first, then vector KNN runs on the query embedding."),
    ("cs4", "Model loading is lazy on first use",
     "ONNX models load on the first call that needs them, not at process start."),
]

TRUE_CONTRA = [
    ("tc1", "The index batch size is 128 entities",
     "Embedding batches process 128 entities per pass through the ONNX session.", "cs1"),
    ("tc2", "Session tokens expire after 72 hours",
     "Auth middleware rejects tokens older than 72 hours. The TTL is fixed in config.", "cs2"),
    ("tc3", "The search pipeline runs vector KNN before FTS",
     "Vector KNN executes first on the query embedding, then full-text retrieval runs.", "cs3"),
    ("tc4", "Models load eagerly at process startup",
     "ONNX models are loaded when the process starts, before any call that needs them.", "cs4"),
]

FALSE_ALARMS = [
    ("fa1", "The index batch size defaults to 64 unless overridden",
     "Embedding batches process 64 entities per pass by default; the size is configurable.", "cs1"),
    ("fa2", "Session tokens are stored as SHA-256 hashes",
     "Token rows hold SHA-256 hashes of the secret. Plaintext tokens are never persisted.", "cs2"),
    ("fa3", "FTS uses SQLite FTS5 with porter stemming",
     "The full-text index is FTS5 with the porter tokenizer for English stemming.", "cs3"),
    ("fa4", "Models unload after five minutes idle",
     "An idle TTL of 300 seconds releases model memory when the process is not serving queries.", "cs4"),
]

def make_fixture():
    repo = tempfile.mkdtemp(prefix="cogz-p5-")
    subprocess.run([COGZ, "init"], cwd=repo, check=True, capture_output=True)
    subprocess.run([COGZ, "index"], cwd=repo, check=True, capture_output=True)
    return repo

def dedup_phase(mcp):
    ids = {}
    for key, title, body in SEED_KNOWLEDGE:
        r = mcp.call("create_entity", {"entity_type": "knowledge", "title": title,
                                       "content": body, "category": "architecture"})
        ids[key] = r["id"]
    def probe(items):
        rows = []
        for key, title, body, *_ in items:
            r = mcp.call("create_entity", {"entity_type": "knowledge", "title": title,
                                           "content": body, "category": "architecture"})
            flagged = bool(r.get("dedup_flagged") or r.get("duplicate_warning"))
            w = r.get("duplicate_warning") or {}
            rows.append({"id": key, "flagged": flagged,
                         "match": w.get("existing_title"), "sim": w.get("similarity"),
                         "via": w.get("title_match")})
            ids[key] = r["id"]
        return rows
    return {"true_dupes": probe(TRUE_DUPES), "near_dupes": probe(NEAR_DUPES),
            "distinct": probe(DISTINCT), "ids": ids}

def contradiction_phase(mcp):
    ids = {}
    for key, title, body in CONTRA_SEED:
        r = mcp.call("create_entity", {"entity_type": "observation", "title": title, "content": body})
        ids[key] = r["id"]
    def probe(items):
        rows = []
        for key, title, body, tgt in items:
            r = mcp.call("create_entity", {"entity_type": "observation", "title": title, "content": body})
            rows.append({"id": key, "flagged": bool(r.get("contradiction_flagged")),
                         "target": tgt})
            ids[key] = r["id"]
        return rows
    return {"true_contra": probe(TRUE_CONTRA), "false_alarms": probe(FALSE_ALARMS), "ids": ids}

def promotion_phase(mcp):
    r = mcp.call("create_entity", {"entity_type": "observation",
                                   "title": "Repeated embedding runs leave lingering bg processes",
                                   "content": "embed-bg processes remain alive after completing their batch."})
    target = r["id"]
    supporters = []
    for i in range(2):
        s = mcp.call("create_entity", {"entity_type": "observation",
                                       "title": f"embed-bg linger sighting {i+1}",
                                       "content": f"Saw a finished embed-bg in do_wait (sighting {i+1}).",
                                       "supporting_ids": [target]})
        supporters.append(s["id"])
    dry2 = mcp.call("consolidate", {"dry_run": True})
    s3 = mcp.call("create_entity", {"entity_type": "observation",
                                    "title": "embed-bg linger sighting 3",
                                    "content": "Third sighting of a finished embed-bg stuck in do_wait.",
                                    "supporting_ids": [target]})
    supporters.append(s3["id"])
    dry3 = mcp.call("consolidate", {"dry_run": True})
    real = mcp.call("consolidate", {"dry_run": False})
    again = mcp.call("consolidate", {"dry_run": True})
    # edge-type selectivity: obs with 3 references but no supports
    r2 = mcp.call("create_entity", {"entity_type": "observation",
                                    "title": "Referenced but unsupported observation",
                                    "content": "This observation is referenced by others but unsupported."})
    for i in range(3):
        mcp.call("create_entity", {"entity_type": "observation",
                                   "title": f"plain referrer {i+1}",
                                   "content": f"Refers to the unsupported observation (case {i+1}).",
                                   "references": [r2["id"]]})
    dry_refs = mcp.call("consolidate", {"dry_run": True})
    def promoted_ids(res):
        return [p["observation_id"] for p in res.get("promoted", [])]
    return {"below_threshold": promoted_ids(dry2), "at_threshold": promoted_ids(dry3),
            "real_run": real, "idempotence": promoted_ids(again),
            "refs_not_supports": promoted_ids(dry_refs),
            "target_id": target, "refs_target_id": r2["id"]}

def score_dedup(res):
    tp = sum(1 for r in res["true_dupes"] if r["flagged"])
    fn = len(res["true_dupes"]) - tp
    fp = sum(1 for r in res["near_dupes"] + res["distinct"] if r["flagged"])
    prec = tp / (tp + fp) if tp + fp else None
    rec = tp / (tp + fn) if tp + fn else None
    return {"tp": tp, "fn": fn, "fp": fp, "precision": prec, "recall": rec}

def score_contra(res):
    caught = sum(1 for r in res["true_contra"] if r["flagged"])
    fa = sum(1 for r in res["false_alarms"] if r["flagged"])
    return {"caught": caught, "of": len(res["true_contra"]),
            "false_alarms": fa, "fa_of": len(res["false_alarms"])}

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    a = ap.parse_args()
    repo = make_fixture()
    print(f"fixture: {repo}", file=sys.stderr)
    mcp = Mcp(repo)
    try:
        t0 = time.monotonic()
        dedup = dedup_phase(mcp)
        contra = contradiction_phase(mcp)
        promo = promotion_phase(mcp)
        elapsed = time.monotonic() - t0
    finally:
        mcp.close()
    report = {
        "repo": repo, "elapsed_s": round(elapsed, 1),
        "dedup": {"score": score_dedup(dedup),
                  "detail": {k: v for k, v in dedup.items() if k != "ids"}},
        "contradiction": {"score": score_contra(contra),
                          "detail": {k: v for k, v in contra.items() if k != "ids"}},
        "promotion": promo,
    }
    print(json.dumps(report, indent=1))
    if a.out:
        json.dump(report, open(a.out, "w"), indent=1)

if __name__ == "__main__":
    main()
