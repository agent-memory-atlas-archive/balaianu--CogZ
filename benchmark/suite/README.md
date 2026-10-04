# CogZ benchmark suite

Reproducible measurement harness for CogZ across pinned external
corpora plus local reference repos.

## Layout

- `corpora.toml` — manifest: name, source (github/local), pinned sha
- `fetch_corpus.py` — clone @ sha → `cogz init` → inject seeds → index
- `seeds/<corpus>/` — `.cogz/`-shaped seed trees (knowledge/,
  observations/, rules/) — a simulated long-term corpus including
  stale, superseded, duplicate, and rejected entities
- `queries/<corpus>_{seeded,commit,negatives}.json` — query sets
- `run_suite.py` — battery: corpus metrics, seeded/commit/negatives
  search, pack metrics + budget sweep, pack recall @ 8k/4k, channel
  ablations (FTS-only / no-expand), determinism, index timing
- `run_invariants.py` — fixture checks (reindex≡rebuild, tombstones,
  drift precision, consolidation)
- `agent_replay.py` — fail-to-pass agent runs on real fix commits
- `mine_observations.py`, `mined/` — git-history observation mining

## Reproduce

```sh
export COGZ_BENCH_CORPORA=/path/to/corpora    # default: ../corpora

python3 fetch_corpus.py --corpus httpx        # clone, seed, index
python3 run_suite.py --corpus httpx           # full battery
python3 run_invariants.py                     # once per binary
python3 agent_replay.py --corpus cobra --list # preview replay tasks
python3 agent_replay.py --corpus cobra --tasks TASKS.json --arm both
```

## Methodology notes

- Agent replays export `git archive` of the parent tree into a fresh
  `git init` — no upstream history, so agents cannot read the fix
  commit. (Earlier worktree-based runs were contaminated by exactly
  that; results quarantined under `agent_contaminated/`.)
- Worktrees live under `$COGZ_REPLAY_TMP` (default: system temp),
  cargo uses a shared `$COGZ_REPLAY_TARGET` target dir.
- The `timing` phase runs last because its full-rebuild arm calls
  `cogz reset` + `cogz index` — that arm only runs on `github` corpora;
  `local` corpora (live dogfood DBs) get reindex timing only, since
  reset would destroy non-rebuildable telemetry.
- Latency numbers are contention-sensitive; recall metrics are not.
