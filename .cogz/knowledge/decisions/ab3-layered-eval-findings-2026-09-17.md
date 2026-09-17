---
id: b7c3f1a2-9e4d-4a8b-8c5d-1f2e3a4b5c6d
title: "ab3 layered A/B evaluation: causal evidence for context-pack value and the delivery-model decision"
type: knowledge
status: active
created_at: "2026-09-17T10:00:00Z"
updated_at: "2026-09-17T10:00:00Z"
references: []
category: decisions
tags: ["decision", "ab-eval", "context-packs", "tiered-push", "delivery-model"]
---

# ab3 layered A/B evaluation findings

First agent-in-the-loop causal measurement of CogZ context packs. 14 historical
telegram-acp-bot tasks, three arms: bare (no cogz), cogz (full tiered push),
thin (cold-start orientation only, task_max_results=0, pull tools kept).
Hardened harness: pruned clones + bwrap masks + blocked network + empty
per-run session DBs.

## Measured results (contamination-excluded)

- cogz vs bare (9 clean completed pairs): **cogz faster in 9/9, p=0.004,
  median -28% wall time**. cogz-only completions: 2 (05aa61d, 0be1c8c);
  bare-only: 0. Ground-truth file recall unchanged (0.9 vs 0.8, ns).
- thin vs bare (8 pairs): thin faster 6/8, p=0.29, median -9%. Directional,
  not significant.
- cogz vs thin (9 pairs): speed is a coin flip (5/4). Completion margin
  favors cogz: the two hardest tasks finished only under full push.
- FAIL_TO_PASS (real-commit tests on final worktrees): 05aa61d cogz 2 fails
  vs bare 17 fails — cogz's extra speed translated to more complete work,
  not rushed work. Most other tasks tied. Test-interface divergence
  (different method names than upstream) makes some rows inconclusive.
- In-run pack hit rates 42-81% (full arm); thin delivered 0 task entities
  but still beat bare on most tasks -> orientation (identity + code map)
  is the base-layer mechanism; task push is the margin on hard tasks.
- Pull tools underused in all arms (2-4 calls/run); agents default to
  grep/read. Pull-path value unmeasurable here.

## Delivery-model decision

Evidence supports: **always push orientation; gate task-push on
confidence/difficulty** rather than pushing task results every prompt.
Thin won several easy/medium tasks outright (mild anchoring/noise cost of
unneeded entities), while full push was the completion margin on the two
hardest tasks. This converges with the earlier PRF silence-gate finding:
push when confident, stay quiet otherwise. "Push the map, pull the
territory" is half right — the map carries the base benefit, and gated
task push earns its tokens where the task is hard.

## Harness lessons (for future evals)

- Never name worktrees by target SHA — agents noticed and looked the
  commit up.
- Public-repo benchmarks have unclosable answer channels: web_search runs
  outside the sandbox, reader proxies (r.jina.ai) bypass /etc/hosts
  blocks server-side, and PyPI serves the project's own post-parent
  wheels/sdists (dep access can't be fully blocked without breaking uv).
- 4/42 runs were contaminated post-hoc and excluded; audit exports for
  PyPI-of-self + proxy fetches, not just git/network forensics.
