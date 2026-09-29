---
id: b8835ad3-1131-4326-9aa8-05984c69e3b8
title: "New expansion channels: emit early, filter before seen-mark, sort deterministically, never displace directs"
type: rule
status: active
created_at: "2026-09-29T00:39:10.858095057+00:00"
updated_at: "2026-09-29T00:39:10.858095057+00:00"
references: ["48c9abeb-9bd1-4582-822f-5028f300006a"]
confidence: 0.9
verified_against: ["48c9abeb-9bd1-4582-822f-5028f300006a=3865b967ca9e665634d1ec0a24df1417931c62c1442786c9bc6bfc7341e0a141"]
---

Conventions proven across the sibling and co-change channels (both emitted via `emit_*_expansions` in `src/search/hybrid_helpers.rs`):

1. **Emit before the generic expansion loops** — candidates emitted later get claimed-and-floored by graph traversal (`seen` marked at generic score), erasing the channel's relevance signal.
2. **Apply entity-type/test filters BEFORE `seen.insert`** — marking a filtered-out candidate permanently blocks re-emission; check first, mark only on accept.
3. **Iterate a deterministically-sorted structure.** `emitted`/`candidates` HashMap order is per-process random — the "same query twice" determinism check caught cobra rank-20 results flipping. Sort by (score desc, id) anywhere a capped emit loop draws order from a map.
4. **Expansions never displace direct results** — emit only into capacity below the merge cap; a bounded quota (channel gets ≤ limit/4) keeps it additive rather than crowding. `graph_path` must be len ≥2 so responses classify `kind=expanded`, never `direct`.
5. **String-channel identity via a shared const** (`COCHANGE_DESC`) — the emit site and the top-N quota partition must agree or the bound silently breaks.

Applies to any future retrieval-adjacent channel feeding the expansion loop.