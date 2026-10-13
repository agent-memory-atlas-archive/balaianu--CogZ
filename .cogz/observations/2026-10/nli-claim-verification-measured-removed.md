---
id: 88132f23-03c4-4087-9fdc-5a248e580ef2
title: "NLI claim verification measured, falsified, and surgically removed; verify is now attestation-only"
type: observation
status: active
created_at: "2026-10-13T00:00:00+00:00"
updated_at: "2026-10-13T00:00:00+00:00"
references: []
source: field-report
confidence: 0.95
---

Item 96 (backlog) shipped an NLI gate on `verify` — contradiction
verdicts auto-staled knowledge whose references disagreed. Live
calibration on the self corpus (41 claim↔ref pairs, real NLI +
code models, `tests/test_verify_gate.rs` kept as the ignored probe)
falsified the design:

- NLI contradiction ≥0.70 on **19/41 pairs** — including accurate
  claims judged against helper functions they merely cite.
- Entailment ≈ 0 everywhere (max 0.23) — `supported` never fires.
- Code-embedding cosine (p0 0.32, p50 0.51, p90 0.65) does not
  separate true coverage from citation — contradiction stays high
  across the whole range, so a relatedness gate can't rescue it.
- Docstring-as-premise (in-distribution variant): only 17% of refs
  even have doc comments, and it still scored 0.956/0.743
  contradiction on accurate claims.

Root cause: `nli-deberta-v3-xsmall` collapses neutral→contradiction
when premise and hypothesis live in different registers. Code→prose
NLI is out of distribution — not a partially-broken signal, no
signal. An advisory mode would just be noise that trains users to
ignore the flag.

What was removed: `claim_verify.rs`, `ClaimVerdict`,
`verify_entity`'s NLI gate, `STALE_REASON_NLI`, `--force` on
CLI/MCP/doctor verify, `claim_entailment_threshold` and
`claim_relatedness_threshold` config. Three wrongly-staled entities
re-verified clean.

What was kept — all mechanically verified: provenance re-stamping,
per-ref resolution + `dead_refs` + `allow_dead_refs`, batch verify
surfaces (CLI + `doctor --verify`), hash-match re-anchoring, doctor
lints, session-end drift notice. NLI itself stays for consolidation
dedup + contradiction detection — that workload was separately
benchmarked (NL→NL, in-distribution, +7pp on tasksource).

Verify semantics now honest: operator attestation. "I assert this
holds — re-anchor it to current code, list which anchors resolved."
If the claim is wrong, `reject` — verify never claimed to detect
truth anyway; now it doesn't pretend.
