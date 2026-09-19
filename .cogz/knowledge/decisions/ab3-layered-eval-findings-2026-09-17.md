---
id: b7c3f1a2-9e4d-4a8b-8c5d-1f2e3a4b5c6d
title: "ab3 layered A/B evaluation: causal evidence for context-pack value and the delivery-model decision"
type: knowledge
status: stale
created_at: "2026-09-17T10:00:00Z"
updated_at: "2026-09-18T21:27:35.398672634+00:00"
references: []
category: decisions
tags: ["decision", "ab-eval", "context-packs", "tiered-push", "delivery-model"]
stale_reason: code_orphaned
verified_against: ["1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "314f35ea-6437-5b6a-ac91-d5f7c72f7ca8=92e84134b38583a86633fcc556dbf03ad4320eaa4ded2d979ed83d60d2f14339", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "6418285a-aa22-5b7a-ba4b-c68c49fd6aaa=4a09a3779b76dde9a5813dc8408095de6c7b8b548b2d9ee645f306fa0b3a5b20", "da11d218-6d48-589e-8c36-5abebe6bb7d8=51191e04bd84309046660216011fcd5e9b30f784bb9bc3b365e9a54eae730099", "e9837ff8-487d-5a4b-b76b-d7b8cf9ff290=dea18cd35f6f8219b080e56096a315c75d294e4340b169f35d9536d308dc165e", "feca2a7c-1249-5d0f-b68f-295e59d141bd=87daebe7a505757b279cf42767603aca0a89297ce90bc38625a5b14e99606747"]
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
