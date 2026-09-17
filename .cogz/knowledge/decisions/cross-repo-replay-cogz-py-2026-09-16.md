---
id: 7f2a9c14-3b5e-4d8a-9f61-2e8c4a1b7d30
title: "Cross-repo historical replay: CogZ-py generalization findings"
type: knowledge
status: active
created_at: "2026-09-16T16:30:00+00:00"
updated_at: "2026-09-16T16:30:00+00:00"
references: []
category: "decisions"
tags: ["measurement", "replay", "cross-repo", "ablation"]
---

# Cross-repo replay: Rust CogZ measured against CogZ-py history

Second full-history replay, run against the old Python CogZ repo to test
whether delivery measurements generalize beyond self-dogfooding.

## Setup

- Legacy `.cogz` moved aside to `.cogz-legacy` (preserved); fresh Rust
  `cogz init --local-only`.
- All 178 legacy knowledge files converted from HTML-comment metadata to
  YAML frontmatter via `/tmp/convert_legacy.py` (converted=178 skipped=0).
- Index: 178 knowledge + 2,597 code entities = 2,775 total.
- Replayed all 452 commits (2026-06-15 → 2026-08-07) as 30 day-sessions:
  commit subjects as `prompt_submit`, diff file lists as `file_save`.
- FTS-only mode; debug binary with busy-retry fix.

## Results (CogZ-py, deliveries 1-482, all sim data)

- Overall: 4,894 hits / 35,813 resolved = 13.7% (Rust repo: 11.4%)
- Tier: baseline 0/30; full 4,777/34,862 = 13.7%; pointer 117/921 = 12.7%
- Type: function 16.7%, class 10.1%, file 7.3%, knowledge 0/592 = 0%
- Pack size: ~74 entities, ~27 file paths per pack (Rust: ~80 / ~41)
- Coverage: 128/188 touched-and-still-indexed files delivered = 68%
  (Rust: ~94%). Per-pack file-space share 14% → ~4.8x over chance
  (Rust: ~4x)
- Repo churn: 866 distinct files touched over history, only 188 survive
  at HEAD — 78% of the touched-file universe turned over.

## The big confound: unindexed docs churn

44% of all save events (1,147/2,592) touched `docs/` paths — the EOS
automation era dominated late history with `docs/eos/engine-state/*.json`
and iteration reports. The code index never contains docs, so late-history
work was structurally unhittable. Weekly hit rate by delivery bucket:
11.3 / 24.9 / 20.5 / 19.0 / 0.7 / 5.6 — the collapse maps exactly to the
docs-churn era (400+ docs saves vs ~30 code saves per window), not to
knowledge staleness.

## What this means

1. **The ~12% baseline generalizes.** Second repo, different language,
   3x the history: 13.7%. The instrument is measuring a stable property
   of prompt-scoped retrieval, not a CogZ-on-CogZ artifact.
2. **Pointer tier works at scale.** 12.7% hit rate with n=921 (Rust had
   ~12% with tiny n). Gate-demoted entities get used at nearly the same
   rate as full-content entities — the gate is not losing useful ones.
3. **Coverage vs churn is the real limit.** Packs can only deliver files
   that exist at index time. A repo with heavy file turnover caps
   achievable recall; 68% coverage on py is near the practical ceiling
   given 78% file churn.
4. **Knowledge hits stay at zero without prompt citations.** 26 of 178
   imported entities were ever delivered; none were hit. File-touch
   hit-detection cannot measure knowledge use — still the open problem.
   Title/entity-name matching in prompts is the only viable surface.
5. **Busy-retry held under real contention.** 452 commits + background
   embedding + reindex churn: zero lifecycle failures, one final
   session_end leaked (closed manually — same tail-window race as Rust).

## Caveats

- Imported knowledge is ~2 months stale relative to replayed history in
  the early window (anachronism runs backward here: knowledge written
  *during* the original era was present for *earlier* commits).
- 153 legacy summaries were heavily duplicated; consolidation merged 0 —
  dedup requires near-identical titles, legacy titles differ. Known gap.
- Docs are not indexed (by design — code index only). Hit-rate collapse
  in docs-era windows is a measurement boundary, not a regression.
