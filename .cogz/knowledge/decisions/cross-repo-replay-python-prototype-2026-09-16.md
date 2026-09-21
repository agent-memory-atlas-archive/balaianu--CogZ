---
id: 7f2a9c14-3b5e-4d8a-9f61-2e8c4a1b7d30
title: "Cross-repo historical replay: Python-prototype generalization findings"
type: knowledge
status: active
created_at: "2026-09-16T16:30:00+00:00"
updated_at: "2026-09-21T13:53:58.437469888+00:00"
references: []
category: decisions
tags: ["measurement", "replay", "cross-repo", "ablation"]
verified_against: ["047656c5-e449-58d7-b951-3b948a609cd9=2573200a85d75170d672883dfed5d39c5e19f3c4d15c0dcb8d3c5e1aa82bfe59", "05bf18cf-a8bf-59b7-88ae-e280991f1c38=00e02a75f77d4f2865171ecd12232bc8a618873be8edb30f76222611ff13f5dd", "07eb50dc-31a4-5530-be0b-dd10afd9dcc9=5a8f9609d1527ecd9dab99bc771b168e6b4eecdc25f2cbb2db4ca727713ec60f", "1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "47f57663-237c-52a7-9481-dd39a2c7ce87=228a6567c34998d60eb7b361dbe5bf16d4f69b72ee308edf27317df89bca0cb7", "684e8d66-2b67-51f6-8408-7fbfacc5ced0=b77d4405c385d6b18e431bc8244fd59328fd4a682dbd96b215670fd31eec1397", "795bd6f4-14ab-5270-8b90-bd7dcc68a492=5c5d81f3d7620c60cae41903e3ff70c31fb7e1b0a9475460dc40dc95eeb8aa41", "80270134-25d8-5193-a790-f3bfa5247182=4c095032e338c834e5eb7ee190a0bbead7a17ce2c1e14a9022f4c7a5bab5ad1e", "8da4c6f0-bbbb-54bf-8e3f-39baa9c183dd=668e49f38f93b498db29de253cf4c12df2fb3c45491510db4cd6c152c1535bcd", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "bd10f9c8-b51f-537d-adf9-cef57d23c8e9=30b53cd3a67993c8863973b752cdd8dfba8ae85c090ebebb2cda032a2038fb77", "df753c4c-818b-5f3e-bab3-faeea3cf4d74=586e4d5db5d85e78c461ab517f3dc8c21acac0ca900f29397c9ddfd9f6168806"]
---

# Cross-repo replay: Rust CogZ measured against the Python prototype's history

Second full-history replay, run against the earlier Python implementation to test
whether delivery measurements generalize beyond self-dogfooding.

## Setup

- Legacy `.cogz` moved aside to `.cogz-legacy` (preserved); fresh Rust
  `cogz init --local-only`.
- All 178 legacy knowledge files converted from HTML-comment metadata to
  YAML frontmatter via a one-off conversion script (converted=178 skipped=0).
- Index: 178 knowledge + 2,597 code entities = 2,775 total.
- Replayed all 452 commits (2026-06-15 → 2026-08-07) as 30 day-sessions:
  commit subjects as `prompt_submit`, diff file lists as `file_save`.
- FTS-only mode; debug binary with busy-retry fix.

## Results (Python prototype, deliveries 1-482, all sim data)

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
