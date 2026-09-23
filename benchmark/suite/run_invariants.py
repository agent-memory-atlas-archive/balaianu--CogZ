#!/usr/bin/env python3
"""Run corpus-independent invariant checks once per binary.

Each check builds its own fixture repo in a tempdir. Outputs land in
results/suite/_invariants/.

Usage: python3 run_invariants.py [--cogz PATH]
"""
import argparse
import subprocess
from pathlib import Path

SUITE = Path(__file__).parent
BENCH = SUITE.parent
COGZ = BENCH.parent / "target/release/cogz"
CHECKS = ("reindex_equiv.py", "tombstone_check.py", "drift_precision.py",
          "consolidation_suite.py")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cogz", default=str(COGZ))
    args = ap.parse_args()
    outdir = BENCH / "results/suite/_invariants"
    outdir.mkdir(parents=True, exist_ok=True)
    for check in CHECKS:
        name = check.removesuffix(".py")
        cmd = ["python3", str(BENCH / check)]
        if check == "consolidation_suite.py":
            cmd += ["--out", str(outdir / f"{name}.json")]
        else:
            cmd += ["--cogz", args.cogz]
        print("+", " ".join(cmd))
        r = subprocess.run(cmd, capture_output=True, text=True)
        (outdir / f"{name}.txt").write_text(r.stdout + r.stderr)
        print(f"  rc={r.returncode}")
    print(f"invariants -> {outdir}")


if __name__ == "__main__":
    main()
