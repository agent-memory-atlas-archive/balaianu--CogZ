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

## Version delta (local corpora re-run under current binary)

| corpus | seeded R@20 before | after |
|---|---|---|
| kaos-website | (pre-slot baseline) | **1.000** (pack 1.0) |
| api_tool | 0.7 | 0.8 (quota run) |
| CogZ-py | 0.6 | 0.77 (quota run) |
| CogZ self | 0.681 | 0.723 (quota run) |

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

Final harness: `git archive` of parent tree + `git init` baseline,
sanitized prompts, no-lookup/no-outside-files instruction, corpus
`.git` renamed away during each agent run, entity purge after
worktree `cogz index`.

cogz arm: `.cogz` (files+db) copied into worktree, `cogz index` at
parent-sha content + purge; prompt instructs `cogz
search`/`get-context` use.

### Results — 24 runs, identical outcomes

| task | bare | cogz | bare wall | cogz wall |
|---|---|---|---|---|
| cobra 746ef071 os.Args mutation | pass | pass | 74s | 139s |
| cobra 24ada7fe default completion cmd | pass | pass | 703s | 1735s |
| cobra 6b0bd307 flag value vs subcommand | pass | pass | 259s | 193s |
| cobra 10cf7be9 group presence check | pass | pass | 164s | 240s |
| httpx 47f4a96f empty zstd | pass | pass | 62s | 73s |
| httpx 49d74a2e header None error | pass | pass | 88s | 109s |
| httpx 99cba6ac RFC 2069 digest | pass | pass | 93s | 128s |
| httpx 1e110964 iter_text empty str | pass | pass | 100s | 501s |
| clap 144e5cb4 --help propagation | **fail** | **fail** | 998s | 893s |
| clap 3604b131 value_terminator | **timeout** | **fail** | 1800s | 942s |
| clap 24dfa0d5 mangen display_order | pass | pass | 396s | 808s |
| clap 5335f54d mut_subcommands | pass | pass | 1284s | 1118s |

- **Outcome lift: none.** Both arms pass 10/12 — the same ten. The
  two parser tasks defeat both. (The one earlier divergence —
  3604b131 cogz "pass" — was the index leak, reproducible only while
  the fix's own entities were readable.)
- **Wall-clock: cogz arm is slower on 7/8 easy tasks** (median
  +~45s; 24ada7fe +1032s, 1e110964 +401s, 24dfa0d5 +412s) and modestly
  faster on the two hard fails. Consistent with extra exploration
  steps and the mandatory-search preamble; on clap the cogzify index
  pass also delays agent start by minutes.
- **Adoption is narrated, not invisible**: transcripts show cogz-arm
  agents querying on hard/unclear tasks ("Let me check CogZ's indexed
  observations for this commit", direct SQLite pokes at the DB) and
  skipping it entirely on easy ones (746ef071: zero cogz in a 1.6KB
  transcript). On every pass the knowledge layer contributed
  metadata-level grounding ("the upstream fix touched `_decoders.py`
  and `_models.py`") — never the fix itself. Post-purge, nothing in
  the index could hand over the answer; agents derived it from code.
- **Parametric recall caveat**: one agent identified the task as a
  real upstream cobra commit and tried to recall the patch from
  memory ("upstream cobra PR #xxxx moved the check to `ExecuteC`").
  Tasks drawn from famous OSS history are partially solvable by
  recall, not just reasoning — this inflates absolute pass rates on
  both arms symmetrically. The bare-vs-cogz comparison stands; the
  absolute numbers shouldn't be read as "agent solves arbitrary bugs".

Bottom line: at n=12 real fixes, CogZ availability changed **what the
agent did** (queried, read observations, sometimes poked the DB
directly) but not **whether it succeeded**. The binding constraint
remains outcome-level knowledge content — metadata-level observations
ground the search but don't carry the fix.

## Environment notes

- /tmp is a 3.7GB tmpfs; Rust target dirs must live elsewhere.
- `devin -p` in print mode needs `--permission-mode dangerous` to
  allow edits+test runs in throwaway worktrees, and
  `--respect-workspace-trust false` for untrusted dirs.
- The embedded-model latency column is load-sensitive; clap's cleaner
  numbers (p50 ~800ms on 4.6k entities) vs httpx/cobra (~1400ms)
  reflect contention during their runs, not corpus size.
