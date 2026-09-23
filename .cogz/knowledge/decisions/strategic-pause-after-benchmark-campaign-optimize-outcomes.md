---
id: 11871800-f5f3-41ab-a1d0-64704ffb9a5b
title: "Strategic pause after benchmark campaign — optimize outcomes, not index breadth"
type: knowledge
status: active
created_at: "2026-09-22T22:54:49.863465037+00:00"
updated_at: "2026-09-22T22:54:49.863465037+00:00"
references: []
category: decisions
tags: ["strategy", "roadmap", "benchmark", "drift-risk"]
---

Decision (2026-09-22): after the five-corpus report card and the top-5 quota fixes, development pauses on additive retrieval work. The risk named: each individually-cheap indexing addition (stemming, docstring/comment indexing, file-level summaries, commit-message text, alias fields) compounds into a larger DB, slower embeddings, and more ranking surface every future change must re-verify. CogZ should be smart, not bloated — maximize gains, not the complexity needed to replicate benchmark numbers.

First-principles state, per the campaign: the value chain is retrieval -> pack -> agent behavior -> outcome. Packs already rescue weak top-k (pack recall .63-1.0 > search R@20 on every corpus). The binding constraint is no longer recall: agent anchoring is weak (+8pp — seeded knowledge pointed at files agents find anyway) and a task-correctness lift over bare agents is not demonstrated (36% vs 21-43%, within noise). Proven value today: ~2x faster task completion with seeded knowledge and fewer abandoned runs.

Direction agreed:
1. Deletion before addition — the five merge strategies were kept for experiments; campaign data now judges them. Candidates for removal: fixed/strength/gradient/calibrated if detect+quota dominates. Less config surface, less re-verification burden.
2. The next experiment is at the outcome layer, not indexing: seed non-obvious knowledge (gotchas, invariants that break silently, 'don't touch X without Y') on one corpus and measure whether top-5 placement plus pack delivery changes task outcomes. If it does, retrieval is sufficient and the work is content/consolidation. If it doesn't, indexing breadth is justified with a known failure to fix.
3. Any indexing change must clear the gate: does it move outcome metrics, not just R@20?

Moat thesis: homogeneous memory retrieval is solved elsewhere (mnemos MRR .97 on a pure memory store). CogZ's differentiator is the cognition layer — verified, deduplicated, promoted knowledge delivered through context packs on a mixed code+knowledge corpus.

Related commits: 29e21fa (top_diversity_share 0.3->0.2, dead-slot fix), 1ff25d9 (proportional window quota scoped to the result-cut boundary via SearchParams.window_quota).