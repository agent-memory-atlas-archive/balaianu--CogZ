#!/usr/bin/env python3
"""Run the standard measurement battery on a corpus.

Per corpus: corpus metrics -> seeded-GT search (+context packs) ->
commit-GT search -> negatives -> pack metrics -> pack budget sweep.
All outputs land in results/suite/<corpus>/ for scoring.

Usage:
    python3 run_suite.py --corpus httpx [--cogz PATH] [--skip PHASES]
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time
import tomllib
from pathlib import Path

SUITE = Path(__file__).parent
BENCH = SUITE.parent
COGZ = BENCH.parent / "target/release/cogz"
PHASES = ("corpus", "seeded", "commit", "negatives", "packs", "sweep",
          "ablations", "determinism", "timing")

PROBE_SRC = {
    "rust": "fn bench_probe_tmp() {}\n",
    "go": "package main\n\nfunc benchProbeTmp() {}\n",
    "python": "def bench_probe_tmp():\n    pass\n",
}
PROBE_EXT = {"rust": "rs", "go": "go", "python": "py"}


def fts_only_config(text: str) -> str:
    # Supported no-model switch: skips model resolution entirely, so
    # the FTS arm measures the degraded path rather than a failed-load
    # fallback. Replaces the old bogus-model-name probe, which config
    # validation now rejects.
    if re.search(r'^force_fts_only\s*=', text, flags=re.M):
        return re.sub(r'^force_fts_only\s*=.*$', 'force_fts_only = true',
                      text, flags=re.M)
    return text.replace('[embedding]',
                        '[embedding]\nforce_fts_only = true', 1)


def run(cmd: list[str], out: Path | None = None) -> int:
    print("+", " ".join(cmd))
    r = subprocess.run(cmd, capture_output=True, text=True)
    if out:
        out.write_text(r.stdout + r.stderr)
    if r.returncode != 0:
        print(f"FAILED: {r.stderr[-800:]}")
    return r.returncode


def embed_bg_running() -> bool:
    # Match by comm + argv, not pgrep -f: a shell wrapper's cmdline
    # contains the literal pattern too, so pgrep -f self-matches
    # (monitors, this script's own invocation). A real embedder has
    # comm=cogz and `embed-bg` as an argv element.
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if (proc / "comm").read_text().strip() != "cogz":
                continue
            if b"embed-bg" in (proc / "cmdline").read_bytes().split(b"\0"):
                return True
        except OSError:
            continue
    return False


def wait_for_embedders(timeout_s: int = 7200) -> bool:
    """Block until no `cogz embed-bg` is running anywhere.

    Index/reindex spawn detached embedders that outlive the foreground
    command and compete for CPU/DB. Timing arms must not start — and
    must not end — while one is alive, or the numbers measure
    contention instead of the code. Cheap when nothing is running.
    """
    t0 = time.monotonic()
    while embed_bg_running():
        if time.monotonic() - t0 > timeout_s:
            print("warn: embed-bg still running after timeout")
            return False
        time.sleep(5)
    return True


def embedding_state(cogz: str, repo: Path) -> dict:
    """Entity/embedding counts — the completeness signal needed to
    interpret query-phase scores (missing vectors depress recall)."""
    r = subprocess.run([cogz, "status", "--repo", str(repo)],
                       capture_output=True, text=True)
    out = {}
    for line in r.stdout.splitlines():
        m = re.match(r"\s*(Total entities|Stale entities|"
                     r"Embeddings stored|Edges):\s+(\d+)", line)
        if m:
            key = m.group(1).lower().replace(" ", "_")
            out[key] = int(m.group(2))
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--cogz", default=str(COGZ))
    ap.add_argument("--skip", nargs="*", default=[], choices=PHASES)
    ap.add_argument("--pack-tokens", type=int, default=8192)
    args = ap.parse_args()

    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    entry = next(c for c in manifest["corpus"] if c["name"] == args.corpus)
    # corpora_dir comes from the environment — machine paths are not committed.
    corpora_dir = Path(os.environ.get(
        "COGZ_BENCH_CORPORA", BENCH / manifest["suite"]["corpora_dir"]))
    repo = (SUITE / entry["path"]).resolve() if entry["source"] == "local" \
        else corpora_dir / entry["name"]

    outdir = BENCH / manifest["suite"]["results_dir"] / args.corpus
    outdir.mkdir(parents=True, exist_ok=True)
    manifest_out = {
        "corpus": args.corpus,
        "sha": entry.get("sha"), "cogz": Path(args.cogz).name,
        "config": (repo / ".cogz/config.toml").read_text(),
    }
    (outdir / "_manifest.json").write_text(json.dumps(manifest_out, indent=1))

    skip = set(args.skip)
    if "corpus" not in skip:
        run(["python3", str(BENCH / "corpus_metrics.py"), str(repo)],
            outdir / "corpus.txt")

    for phase, qfile in (("seeded", "seeded"), ("commit", "commit"),
                         ("negatives", "negatives")):
        if phase in skip:
            continue
        q = SUITE / "queries" / f"{args.corpus}_{qfile}.json"
        if not q.exists():
            print(f"skip {phase}: {q} missing")
            continue
        ctx = ["--with-context"] if phase == "seeded" else []
        # Commit queries carry their own commit's timestamp so the
        # co-change channel can only draw on strictly-earlier history —
        # no leakage from the query commit or anything after it.
        if phase == "commit":
            ctx.append("--temporal")
        raw = outdir / f"{phase}_run.json"
        run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
             "--queries", str(q), "--out", str(raw), "--cogz", args.cogz,
             *ctx], outdir / f"{phase}_run.log")
        run(["python3", str(BENCH / "score.py"), str(raw), str(q)],
            outdir / f"{phase}_score.txt")

    if "packs" not in skip:
        run(["python3", str(BENCH / "pack_metrics.py"), "--repo", str(repo),
             "--pack-tokens", str(args.pack_tokens),
             "--out", str(outdir / "pack_metrics.json")],
            outdir / "pack_metrics.log")

    if "sweep" not in skip:
        for budget in (8192, 4096, 2048, 1024):
            run(["python3", str(BENCH / "pack_metrics.py"), "--repo", str(repo),
                 "--pack-tokens", str(budget),
                 "--out", str(outdir / f"pack_sweep_{budget}.json")])
        # Pack recall at each budget — the composition sweep shows what fits,
        # this shows what is lost.
        q = SUITE / "queries" / f"{args.corpus}_commit.json"
        if q.exists():
            for budget in (8192, 4096):
                raw = outdir / f"pack_recall_{budget}.json"
                run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
                     "--queries", str(q), "--out", str(raw),
                     "--cogz", args.cogz, "--with-context", "--temporal",
                     "--pack-tokens", str(budget)], outdir / f"pack_recall_{budget}.log")
                run(["python3", str(BENCH / "score.py"), str(raw), str(q)],
                    outdir / f"pack_recall_{budget}_score.txt")

    if "ablations" not in skip:
        abl_qs = [(ph, SUITE / "queries" / f"{args.corpus}_{ph}.json")
                  for ph in ("seeded", "commit")]
        abl_qs = [(ph, q) for ph, q in abl_qs if q.exists()]
        # no-expand arm: direct channels only, no graph expansion.
        for ph, q in abl_qs:
            ctx = ["--temporal"] if ph == "commit" else []
            raw = outdir / f"ab_noexpand_{ph}.json"
            run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
                 "--queries", str(q), "--out", str(raw),
                 "--cogz", args.cogz, "--no-expand", *ctx],
                outdir / f"ab_noexpand_{ph}.log")
            run(["python3", str(BENCH / "score.py"), str(raw), str(q)],
                outdir / f"ab_noexpand_{ph}_score.txt")
        # fts arm: probe config disables models — FTS path only.
        cfg = repo / ".cogz" / "config.toml"
        if abl_qs and cfg.exists():
            original = cfg.read_text()
            cfg.write_text(fts_only_config(original))
            try:
                for ph, q in abl_qs:
                    ctx = ["--temporal"] if ph == "commit" else []
                    raw = outdir / f"ab_fts_{ph}.json"
                    run(["python3", str(BENCH / "run.py"), "--repo",
                         str(repo), "--queries", str(q), "--out", str(raw),
                         "--cogz", args.cogz, *ctx],
                        outdir / f"ab_fts_{ph}.log")
                    run(["python3", str(BENCH / "score.py"), str(raw),
                         str(q)], outdir / f"ab_fts_{ph}_score.txt")
            finally:
                cfg.write_text(original)

    if "determinism" not in skip:
        q = SUITE / "queries" / f"{args.corpus}_seeded.json"
        if not q.exists():
            q = SUITE / "queries" / f"{args.corpus}_commit.json"
        if q.exists():
            for tag in ("det_a", "det_b"):
                run(["python3", str(BENCH / "run.py"), "--repo", str(repo),
                     "--queries", str(q), "--out", str(outdir / f"{tag}.json"),
                     "--cogz", args.cogz])
            run(["python3", str(BENCH / "determinism_check.py"),
                 str(outdir / "det_a.json"), str(outdir / "det_b.json")],
                outdir / "determinism.txt")

    # timing runs last: the full-index arm rebuilds the corpus DB.
    if "timing" not in skip:
        timing = {"corpus": args.corpus}
        # Embedding completeness in the DB state the query phases
        # actually measured — scores are uninterpretable without it.
        timing["embedding_state_at_scoring"] = embedding_state(
            args.cogz, repo)

        def timed(cmd: list[str]) -> float | None:
            # A leftover embedder from an earlier arm (or an earlier
            # corpus) turns this measurement into contention noise.
            wait_for_embedders()
            t0 = time.monotonic()
            rc = run(cmd)
            dt = round(time.monotonic() - t0, 1) if rc == 0 else None
            # The arm may have spawned its own embedder in the tail —
            # drain it before the next arm or corpus starts.
            wait_for_embedders()
            return dt

        timing["reindex_noop_s"] = timed(
            [args.cogz, "reindex", "--repo", str(repo)])
        ext = PROBE_EXT.get(entry.get("language"), "py")
        probe = repo / f"bench_probe_tmp.{ext}"
        probe.write_text(PROBE_SRC.get(entry.get("language"),
                                       PROBE_SRC["python"]))
        try:
            timing["reindex_add_file_s"] = timed(
                [args.cogz, "reindex", "--repo", str(repo)])
        finally:
            probe.unlink(missing_ok=True)
        timing["reindex_remove_file_s"] = timed(
            [args.cogz, "reindex", "--repo", str(repo)])
        # Full rebuild destroys non-rebuildable telemetry (events, usage) —
        # only worth it on fixture corpora, never on a live dogfood DB.
        if entry["source"] == "github":
            run([args.cogz, "reset", "--repo", str(repo)])
            timing["full_index_s"] = timed(
                [args.cogz, "index", "--repo", str(repo)])
            timing["embedding_state_after_rebuild"] = embedding_state(
                args.cogz, repo)
        (outdir / "timing.json").write_text(json.dumps(timing, indent=1))

    print(f"{args.corpus}: battery complete -> {outdir}")


if __name__ == "__main__":
    main()
