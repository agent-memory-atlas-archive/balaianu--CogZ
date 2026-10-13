---
id: 7f3a1b9c-2d4e-4f8a-9b1c-5e6d7a8b9c0d
title: "Silence gate: embedding strength is topical proximity, IDF term coverage is answerability"
type: observation
status: active
created_at: "2026-10-13T00:00:00+00:00"
updated_at: "2026-10-13T00:00:00+00:00"
references: []
source: field-report
confidence: 0.9
---

Measurement across 4 corpora x 105 queries (clap/cobra/httpx/self):
knowledge-channel strength overlaps almost completely between
unanswerable queries (0.57-0.78) and seeded positives (0.58-0.86) —
on a big corpus any query finds a confident-looking neighbor ("parse
TOML" on clap scores 0.71 on parsing internals). Entity-miss queries
even produce sharp gradients (top-1 stands out on a plausible name),
so no vector signal separates the classes.

The discriminating signal is lexical: IDF-weighted share of query
content terms found in the fused top-10 text, corpus-absent terms at
max IDF. Negatives mean 0.40 vs positives 0.85; floor 0.45 silences
~60% of unanswerable queries at ~7% false-silence cost, vs ~8% from
the vector gate alone. It subsumes the old "any FTS hit yields" rule
and runs in FTS-only mode — pure SQL, ~5-10 doc-freq probes per
search, no model cost.

Hard residual is real: ~40% of negatives stay open because every
query term legitimately appears in the fused head (cobra really does
discuss config-file flag loading). Those need semantic verification,
not better thresholds. Also noted: the self-corpus negative suite is
slightly contaminated — benchmark tooling that embeds the negative
queries is itself indexed (df>0 for "quantum" via pack_metrics.py).
