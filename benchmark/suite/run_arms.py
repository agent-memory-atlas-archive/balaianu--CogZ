#!/usr/bin/env python3
"""Retrieval-mode experiment driver — quality × cost across arms.

Each arm applies config overrides to the corpus's `.cogz/config.toml`,
rebuilds the index when the arm changes indexed state, drains any
background embedder, and runs every query set that exists
(seeded/commit/negatives/gap) plus pack metrics and a determinism
probe. Per-arm cost is recorded alongside quality: index wall time,
embed drain wall time, peak RSS of index + embed-bg, DB size.

For `source = "local"` corpora the live `.cogz/cogz.db*` is snapshotted
before the first arm and restored afterwards — arms run `reset`+`index`
on the real DB but nothing is lost.

Usage:
    python3 run_arms.py --corpus httpx [--cogz PATH] [--only ARMS...]
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

SUITE = Path(__file__).parent
BENCH = SUITE.parent
COGZ = BENCH.parent / "target/release/cogz"

sys.path.insert(0, str(SUITE))
from run_suite import embed_bg_running, embedding_state, run  # noqa: E402

# Arm definitions: name -> ({section: {key: toml_literal}}, needs_index).
# needs_index is true when the arm changes what gets indexed or
# embedded; query-time-only toggles reuse the previous arm's index.
ARMS = [
    # Baseline: current behavior, both vec spaces populated.
    ("hybrid", {}, True),
    # Query-time-only degradation — reuses the hybrid index.
    ("fts", {"embedding": {"force_fts_only": "true"}}, False),
    # Split-identifier FTS on a fully-embedded index.
    ("hybrid_split", {"index": {"split_identifiers": "true"}}, True),
    # Split-identifier FTS without any models — reuses hybrid_split's
    # index since force_fts_only is a query-time flag.
    ("fts_split", {"index": {"split_identifiers": "true"},
                   "embedding": {"force_fts_only": "true"}}, False),
    # Knowledge-only embeddings: code vec space stays empty.
    ("knowledge_only", {"embedding": {"code_embeddings": "false"}}, True),
    # D = B + C: split identifiers + knowledge-only embeddings.
    ("knowledge_split", {"index": {"split_identifiers": "true"},
                         "embedding": {"code_embeddings": "false"}}, True),
    # Small-model arm: registered 384d model for both spaces —
    # bge-small is a known-good Qdrant-optimized ONNX entry, unlike
    # an unvetted repo that would hit the fallback layout guess.
    ("small", {"embedding": {
        "code_model": '"BAAI/bge-small-en-v1.5"',
        "knowledge_model": '"BAAI/bge-small-en-v1.5"',
        "dimension": "384",
    }}, True),
]


def set_toml_key(text: str, section: str, key: str, value: str) -> str:
    """Set section.key = value in a TOML document — replaces an
    existing key inside the named section or inserts it right after
    the section header. Appends the section if absent."""
    lines = text.splitlines()
    out: list[str] = []
    in_section = False
    done = False
    header = f"[{section}]"
    for i, line in enumerate(lines):
        stripped = line.strip()
        is_header = stripped.startswith("[") and stripped.endswith("]")
        if is_header:
            if in_section and not done:
                out.append(f"{key} = {value}")
                done = True
            in_section = stripped == header
            out.append(line)
            continue
        if in_section and not done and re.match(
                rf"^{re.escape(key)}\s*=", stripped):
            out.append(f"{key} = {value}")
            done = True
            continue
        out.append(line)
    if in_section and not done:
        out.append(f"{key} = {value}")
        done = True
    if not done:
        out.append(header)
        out.append(f"{key} = {value}")
    return "\n".join(out) + "\n"


# Keys that exist only because an arm adds them — stripped to recover
# the pristine config. Baseline keys arms *replace* (code_model etc.)
# must stay: stripping them would silently drop the corpus's pinning.
EXPERIMENT_ONLY_KEYS = {
    "force_fts_only", "code_embeddings", "split_identifiers",
}


def strip_experiment_keys(text: str) -> str:
    """Remove experiment-added keys, returning pristine config."""
    return "\n".join(
        line for line in text.splitlines()
        if not any(re.match(rf"^{re.escape(k)}\s*=", line.strip())
                   for k in EXPERIMENT_ONLY_KEYS)
    ) + "\n"


def apply_overrides(text: str, overrides: dict) -> str:
    for section, keys in overrides.items():
        for key, value in keys.items():
            text = set_toml_key(text, section, key, value)
    return text


def embed_bg_rss_kb() -> int:
    """Current total RSS across live `cogz embed-bg` processes."""
    total = 0
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if (proc / "comm").read_text().strip() != "cogz":
                continue
            if b"embed-bg" not in (proc / "cmdline").read_bytes().split(b"\0"):
                continue
            for line in (proc / "status").read_text().splitlines():
                if line.startswith("VmRSS"):
                    total += int(line.split()[1])
        except OSError:
            continue
    return total


def drain_embedders(timeout_s: int = 7200) -> tuple[float, int]:
    """Wait for embed-bg to finish; return (drain_seconds, peak_rss_kb)."""
    t0 = time.monotonic()
    peak = embed_bg_rss_kb()
    while embed_bg_running():
        if time.monotonic() - t0 > timeout_s:
            print("warn: embed-bg still running after timeout")
            break
        time.sleep(5)
        peak = max(peak, embed_bg_rss_kb())
    return round(time.monotonic() - t0, 1), peak


def timed_rss(cmd: list[str], log: Path) -> tuple[bool, int]:
    """Run cmd under /usr/bin/time -v; return (ok, max_rss_kb)."""
    print("+", " ".join(cmd))
    r = subprocess.run(["/usr/bin/time", "-v", *cmd],
                       capture_output=True, text=True)
    log.write_text(r.stdout + r.stderr)
    m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", r.stderr)
    if r.returncode != 0:
        print(f"FAILED: {r.stderr[-800:]}")
    return r.returncode == 0, int(m.group(1)) if m else 0


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--cogz", default=str(COGZ))
    ap.add_argument("--only", nargs="*", default=None,
                    help="arm names to run (default: all)")
    ap.add_argument("--pack-tokens", type=int, default=8192)
    args = ap.parse_args()

    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    entry = next(c for c in manifest["corpus"] if c["name"] == args.corpus)
    corpora_dir = Path(os.environ.get(
        "COGZ_BENCH_CORPORA", BENCH / manifest["suite"]["corpora_dir"]))
    repo = (SUITE / entry["path"]).resolve() if entry["source"] == "local" \
        else corpora_dir / entry["name"]

    outdir = BENCH / manifest["suite"]["results_dir"] / args.corpus / "arms"
    outdir.mkdir(parents=True, exist_ok=True)

    cfg = repo / ".cogz" / "config.toml"
    pristine = strip_experiment_keys(cfg.read_text())

    query_sets = [ph for ph in ("seeded", "commit", "negatives", "gap")
                  if (SUITE / "queries" / f"{args.corpus}_{ph}.json").exists()]

    # Live-corpus safety: snapshot the DB so arms can reset+index
    # freely; restore at the end.
    local = entry["source"] == "local"
    db_files = list((repo / ".cogz").glob("cogz.db*"))
    snap = {f: f.with_suffix(f.suffix + ".arm_snapshot") for f in db_files}
    if local:
        for src, dst in snap.items():
            shutil.copy2(src, dst)

    only = set(args.only) if args.only else None
    costs: dict[str, dict] = {}
    have_index = False

    try:
        for name, overrides, needs_index in ARMS:
            if only and name not in only:
                continue
            print(f"=== arm {name} ===")
            cfg.write_text(apply_overrides(pristine, overrides))
            arm_cost: dict = {"config": overrides}

            if needs_index or not have_index:
                run([args.cogz, "reset", "--repo", str(repo), "--confirm"],
                    outdir / f"{name}_reset.log")
                t0 = time.monotonic()
                ok, index_rss = timed_rss(
                    [args.cogz, "index", "--repo", str(repo)],
                    outdir / f"{name}_index.log")
                arm_cost["index_wall_s"] = (
                    round(time.monotonic() - t0, 1) if ok else None)
                arm_cost["index_peak_rss_kb"] = index_rss
                drain_s, drain_rss = drain_embedders()
                arm_cost["embed_drain_s"] = drain_s
                arm_cost["embed_peak_rss_kb"] = drain_rss
                arm_cost["db_bytes"] = sum(
                    f.stat().st_size for f in (repo / ".cogz").iterdir()
                    if f.name.startswith("cogz.db")
                    and not f.name.endswith(".arm_snapshot"))
                arm_cost["state"] = embedding_state(args.cogz, repo)
                have_index = True
            else:
                arm_cost["reused_index"] = True

            for ph in query_sets:
                q = SUITE / "queries" / f"{args.corpus}_{ph}.json"
                # gap needs pack-level recall as much as seeded — the
                # vocabulary-gap question is exactly about pack quality.
                ctx = ["--with-context"] if ph in ("seeded", "gap") else []
                if ph == "commit":
                    ctx.append("--temporal")
                raw = outdir / f"{name}_{ph}.json"
                run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
                     "--queries", str(q), "--out", str(raw),
                     "--cogz", args.cogz, *ctx],
                    outdir / f"{name}_{ph}.log")
                run(["python3", str(BENCH / "score.py"), str(raw), str(q)],
                    outdir / f"{name}_{ph}_score.txt")

            if query_sets:
                q = SUITE / "queries" / f"{args.corpus}_{query_sets[0]}.json"
                for tag in ("a", "b"):
                    run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
                         "--queries", str(q),
                         "--out", str(outdir / f"{name}_det_{tag}.json"),
                         "--cogz", args.cogz])
                run(["python3", str(BENCH / "determinism_check.py"),
                     str(outdir / f"{name}_det_a.json"),
                     str(outdir / f"{name}_det_b.json")],
                    outdir / f"{name}_determinism.txt")

            run(["python3", str(BENCH / "pack_metrics.py"), "--repo", str(repo),
                 "--pack-tokens", str(args.pack_tokens),
                 "--out", str(outdir / f"{name}_pack_metrics.json")],
                outdir / f"{name}_pack_metrics.log")

            costs[name] = arm_cost
            (outdir / "arm_costs.json").write_text(json.dumps(costs, indent=1))
    finally:
        cfg.write_text(pristine)
        if local:
            # Remove arm-created DB files first — a stray -wal/-shm
            # left over would attach to the restored snapshot.
            for f in (repo / ".cogz").glob("cogz.db*"):
                if f not in snap.values():
                    f.unlink()
            for src, dst in snap.items():
                shutil.move(str(dst), str(src))
            print("restored local corpus DB snapshot")

    print(f"{args.corpus}: arms complete -> {outdir}")


if __name__ == "__main__":
    main()
