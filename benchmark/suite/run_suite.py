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
import subprocess
import sys
import tomllib
from pathlib import Path

SUITE = Path(__file__).parent
BENCH = SUITE.parent
COGZ = BENCH.parent / "target/release/cogz"
PHASES = ("corpus", "seeded", "commit", "negatives", "packs", "sweep")


def run(cmd: list[str], out: Path | None = None) -> None:
    print("+", " ".join(cmd))
    r = subprocess.run(cmd, capture_output=True, text=True)
    if out:
        out.write_text(r.stdout + r.stderr)
    if r.returncode != 0:
        print(f"FAILED: {r.stderr[-800:]}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--cogz", default=str(COGZ))
    ap.add_argument("--skip", nargs="*", default=[], choices=PHASES)
    ap.add_argument("--pack-tokens", type=int, default=8192)
    args = ap.parse_args()

    manifest = tomllib.loads((SUITE / "corpora.toml").read_text())
    entry = next(c for c in manifest["corpus"] if c["name"] == args.corpus)
    repo = Path(entry["path"]) if entry["source"] == "local" \
        else Path(manifest["suite"]["corpora_dir"]) / entry["name"]

    outdir = BENCH / manifest["suite"]["results_dir"] / args.corpus
    outdir.mkdir(parents=True, exist_ok=True)
    manifest_out = {
        "corpus": args.corpus, "repo": str(repo),
        "sha": entry.get("sha"), "cogz": args.cogz,
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

    print(f"{args.corpus}: battery complete -> {outdir}")


if __name__ == "__main__":
    main()
