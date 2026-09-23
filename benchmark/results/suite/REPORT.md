# CogZ benchmark suite — external corpus results

Date: 2026-09-23. Binary: `cogz 0.3.0` (target/release @ HEAD incl.
diversity-slot fix 29e21fa + window quota 1ff25d9). Machine: 8-core,
7.3GB RAM, ONNX local inference. All latencies measured under varying
background load — treat as upper bounds, not clean numbers.

## Corpora (pinned, reproducible via fetch_corpus.py)

| corpus | lang | pinned sha | entities | knowledge seeded |
|---|---|---|---|---|
| httpx | Python | b5addb64 | 1051 | ~85 |
| cobra | Go | adbc8813 | 768 | ~84 |
| clap | Rust | 8ab46fe2 | 4664 | ~85 |

Seeds per corpus: ~15 knowledge, ~10 rules, ~49 observations mined
from real git history, 5 stale, 2 superseded pairs, 2 duplicate
pairs, 2 rejected observations — a simulated long-term `.cogz`.

## Seeded knowledge retrieval (15 queries/corpus)

| corpus | P@5 | MRR | R@20 | pack recall |
|---|---|---|---|---|
| cobra | 0.187 | 0.502 | **0.967** | 0.967 |
| httpx | 0.173 | 0.358 | **0.917** | 0.933 |
| clap | 0.169 | 0.264 | **0.772** | 0.782 |

- R@20 stays high on unseen external code — the window quota
  generalizes; not tuned to these corpora.
- P@5 flat at ~0.17–0.19 across all sizes. The quota places rescued
  knowledge in the tail of the window (by design); top-5 placement is
  a different mechanism and remains unaddressed.
- Scale gradient confirmed on external code: R@20 and pack recall
  both degrade monotonically with corpus size (768 → 4664 entities).
  The ~4–5k range is where seeded-knowledge delivery starts fraying.

## Commit-GT retrieval (60 queries/corpus)

| corpus | P@5 | MRR | R@20 |
|---|---|---|---|
| cobra | 0.103 | 0.288 | 0.398 |
| httpx | 0.077 | 0.254 | 0.278 |
| clap | 0.090 | 0.242 | 0.335 |

Consistent with the original campaign: commit prose → code entity is
the weakest surface (vocabulary gap; ranking can't rescue true misses).

## Negatives (5 queries/corpus)

| corpus | neg_ok |
|---|---|
| cobra | 0.4 |
| httpx | 0.4 |
| clap | 0.6 |

Pattern replicated: absurd queries gate to silence; plausible-but-
absent in-domain features (grpc scaffolding, websockets, yaml flags)
return full result sets. Additionally observed: httpx's nonsense query
"quantum entanglement routing protocol" returned 32 entities all at
score 0.000 — common-token overlap ("protocol") produced FTS hits the
gate doesn't suppress. Zero-score rows in output is arguably a bug or
at minimum a reporting wart worth a backlog item.

## Determinism & invariants

- Seeded run twice back-to-back on all three corpora: byte-identical
  result sets (DETERMINISTIC ×3).

## Pack budget sweep (avg knowledge sections per pack)

| corpus | 8192 | 4096 | 2048 | 1024 |
|---|---|---|---|---|
| cobra | 4.6 | 4.6 | 2.4 | 1.6 |
| httpx | 3.0 | 3.0 | 0.2 | 0.0 |
| clap | 2.2 | 2.2 | 0.4 | 0.0 |

Knowledge delivery has a hard budget cliff between 4k and 2k tokens —
below ~2k the pack is code-only for the larger corpora. Replicates the
campaign's sweep finding on external code.
- reindex ≡ rebuild: EQUIVALENT (14 entities/16 edges; embedding
  cosine min 0.9669 — batch-composition noise, known item 57).
- tombstones: OK.
- drift precision: 1.000 P / 1.000 R.
- consolidation suite: pass.

## Version delta (same corpora, same queries — report-card binary vs current)

Seeded-knowledge retrieval (score.py on identical GT files):

| corpus | n | R@20 old | R@20 now | P@5 old | P@5 now |
|---|---|---|---|---|---|
| kaos-website | 8 | 1.00 | 1.00 | .20 | .20 |
| kinetik | 10 | .95 | .95 | .12 | .20 |
| api_tool | 10 | .60 | .80 | .06 | .12 |
| CogZ-py | 15 | .60 | .83 | .05 | .20 |

Commit-GT retrieval:

| corpus | n | R@20 old | R@20 now | P@5 old | P@5 now |
|---|---|---|---|---|---|
| kinetik | 15 | .433 | .562 | .133 | .173 |
| kaos-website | 7 | .571 | .743 | .143 | .143 |
| api_tool | 60 | .432 | .412 | .087 | .063 |
| CogZ-py | 60 | .382 | .353 | .080 | .077 |

Read: the knowledge-delivery work (window quota + diversity slots)
produced a large, consistent gain where it was needed — seeded R@20
+0.20/+0.23 on the two ~2800-entity corpora, P@5 roughly doubled on
every corpus that wasn't already saturated. Commit-GT is mixed:
kinetik and kaos improved +0.13/+0.17, api_tool and CogZ-py slipped
−0.02/−0.03 (within noise at these n; the vocabulary-gap finding is
unchanged). Small corpora were already at ceiling.

## Agent replay (fail-to-pass on real fix commits)

`devin -p` headless, `--permission-mode dangerous`, sanitized task
text (PR/issue refs stripped), corpus `.git` hidden during the run,
fresh-init worktree — four contamination vectors were found and
closed during harness development:

1. **shared worktree history** — agent ran `git show <fix-sha>` and
   copied the upstream diff verbatim (runs quarantined to
   `agent_contaminated/`).
2. **upstream web lookup** — agent searched the PR number from the
   commit message and fetched the patch (quarantined to
   `agent_v2_lookup/`).
3. **filesystem neighbors** — agent found the benchmark corpus's own
   clone with full history via filesystem search (v3 →
   `agent_v3_fsleak/`).
4. **stale index entities** — the copied pin-sha `.cogz` DB retained
   `active` entities for functions the fix commit had added (reindex
   never deletes code entities whose definitions vanish inside a
   file — real bug, backlog #63). On clap 3604b131 the agent read the
   withheld test functions straight out of the index and passed.
   quarantined to `agent_v4_idxleak/`; harness now purges entities
   whose file is absent or whose title no longer appears in it
   (`purge_stale_code_entities`, 113–223 orphans per clap worktree).
   **#63 is fixed in product (5adf3e1)**: `sync_code_entities` now
   runs the per-file removed-definition sweep the incremental path
   always had; e2e verified (pin-sha DB → parent tree → 155 stale vs
   44 pre-fix; orphan not retrievable). Purge stays as a second line.
5. **package-registry sources** — found in v6: the cogz-arm agent on
   clap 3604b131 ignored the no-outside-files rule and read the
   *published* `clap_builder-4.6.0/4.6.6` sources in
   `~/.cargo/registry`, porting the upstream fix verbatim — which
   still failed, because the published code had drifted from the
   parent-era fix commit. Applies to any benchmark corpus that ships
   itself as a package; bare arm never touched it (0 mentions).
   Mitigation would need filesystem sandboxing, not instructions.

Final harness: `git archive` of parent tree + `git init` baseline,
sanitized prompts, no-lookup/no-outside-files instruction, corpus
`.git` renamed away during each agent run, entity purge after
worktree `cogz index`.

cogz arm: `.cogz` (files+db) copied into worktree, `cogz index` at
parent-sha content + purge; prompt instructs `cogz
search`/`get-context` use.

### Results — v5 (pre-product-fix clean run) and v6 (post-#63-fix rerun)

The v6 rerun repeated all 24 cells under the orphan-fixed binary
(`cogz index` itself now marks removed definitions stale; the harness
purge still deletes them outright — both mechanisms agreed on the
orphan set, 174/178/207 purged on cobra, 1–45 on httpx, 113–223 on
clap). Access tracking is zeroed at `cogzify` time so
`cogz_entities_accessed` counts only the agent's session (cobra/httpx
predate that fix and show corpus carryover instead).

| task | v5 bare | v5 cogz | v6 bare | v6 cogz | v6 walls (b/c) |
|---|---|---|---|---|---|
| cobra 746ef071 os.Args mutation | pass | pass | pass | pass | 134s / 76s |
| cobra 24ada7fe default completion cmd | pass | pass | pass | pass | 524s / 619s |
| cobra 6b0bd307 flag value vs subcommand | pass | pass | pass | pass | 149s / 100s |
| cobra 10cf7be9 group presence check | pass | pass | pass | pass | 136s / 235s |
| httpx 47f4a96f empty zstd | pass | pass | pass | pass | 70s / 135s |
| httpx 49d74a2e header None error | pass | pass | pass | pass | 73s / 98s |
| httpx 99cba6ac RFC 2069 digest | pass | pass | pass | pass | 76s / 74s |
| httpx 1e110964 iter_text empty str | pass | pass | pass | pass | 46s / 625s |
| clap 144e5cb4 --help propagation | fail | fail | fail | fail | 402s / 322s |
| clap 3604b131 value_terminator | timeout | fail | **pass** | **fail** | 1693s / 492s |
| clap 24dfa0d5 mangen display_order | pass | pass | pass | pass | 440s / 558s |
| clap 5335f54d mut_subcommands | pass | pass | pass | pass | 662s / 505s |

Totals: v5 bare 10/12, cogz 10/12 — v6 **bare 11/12, cogz 10/12**.

- **Outcome lift: still none — and one cell flipped the other way.**
  3604b131 is a borderline task at the 1800s cap: bare timed out in
  v5, passed in v6 (1693s). The cogz arm failed both times — in v6 it
  finished fast and *wrong*: it found the upstream fix via the cargo
  registry (vector 5), ported a version that had drifted from the
  parent-era commit, and shipped with one test red, rationalizing the
  failure as a stale assertion. Bare ground through the actual code
  and got all 908 tests green.
- **Same-task variance is the dominant noise source at n=12.** One
  task sitting at the timeout boundary swung the bare total by one;
  treat per-arm totals ±1 as noise.
- **Adoption (v6, deterministic signal):** `entity_access` counts
  post-run — clap cogz arms touched 64–120 entities each (agents used
  CogZ on all four clap tasks, the hard ones hardest). Narration
  confirms: on 3604b131 the agent wrote "CogZ knows about this
  specific commit — the observation references commit `3604b13117`"
  — the seeded observation gave real metadata grounding (the fix
  exists, where it touched) but not the diff, which CogZ does not
  store by design.
- **Wall-clock**: cogz arm slower on most easy tasks (mandatory-search
  preamble + cogzify index time), faster on some fails (shorter wrong
  path) — same shape as v5.
- **Parametric recall caveat** (unchanged): agents recognize famous
  upstream commits and try recall; inflates absolute pass rates
  symmetrically on both arms.

Bottom line: two clean campaigns, 48 runs total. CogZ availability
reliably changes agent *behavior* (queries on hard tasks, real
metadata grounding — the observation layer correctly identified the
relevant fix commit) but does not change *outcomes* at this n — and
can no more rescue a wrong-path agent than the leak could rescue it
before. The constraint remains knowledge content: observations say
*that* a fix exists and *what* it touched, not *what it changed*.

## Environment notes

- /tmp is a 3.7GB tmpfs; Rust target dirs must live elsewhere.
- `devin -p` in print mode needs `--permission-mode dangerous` to
  allow edits+test runs in throwaway worktrees, and
  `--respect-workspace-trust false` for untrusted dirs.
- The embedded-model latency column is load-sensitive; clap's cleaner
  numbers (p50 ~800ms on 4.6k entities) vs httpx/cobra (~1400ms)
  reflect contention during their runs, not corpus size.
