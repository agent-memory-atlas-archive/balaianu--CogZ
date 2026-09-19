---
id: f3c86f25-4ef3-4b97-915b-70b3532a362b
title: NLI softmax probabilities with bidirectional scoring
type: knowledge
status: active
created_at: "2026-09-01T12:26:41.940383925+00:00"
updated_at: "2026-09-05T09:13:27.804795109+00:00"
references: []
category: decisions
tags: ["nli", "contradiction", "softmax", "bidirectional"]
verified_against: ["034336be-ef90-55fb-8084-fc05f25701f3=30ea039cbd7cf18fd7b6d05c25bc890d6243193936d5e127fdf88dba08911b33", "07eb50dc-31a4-5530-be0b-dd10afd9dcc9=5a8f9609d1527ecd9dab99bc771b168e6b4eecdc25f2cbb2db4ca727713ec60f", "0f65e31a-4b23-59e2-9232-43229388bd69=ce0ff29aa64646f44f9afd66e6907d76dd3636b37fcc71f25def909f8653870a", "1293b931-434d-5542-ad4a-dbbe1805c50f=850801a922a0720b00245f59a9b005fd241cc8b6a1965ce5fabbe773105f1419", "b0ded8ae-028d-5861-b051-b97b2bedacee=a35c2cef65161e99c10b99e533a418a63fc9e62195fa221147d154ee26dc89c8"]
---

# NLI Softmax Probabilities with Bidirectional Scoring

The NLI contradiction detection layer in CogZ uses softmax probabilities rather than argmax-only labels. This allows threshold-based contradiction decisions instead of binary classification.

## Configuration

- contradiction_threshold: 0.70 (P(contradiction) must exceed this)
- contradiction_cosine_threshold: 0.85 (pre-filter: skip pairs with low embedding similarity)
- contradiction_length_ratio: 5.0 (pre-filter: skip pairs with very different lengths)

## Bidirectional scoring

Both forward (A to B) and reverse (B to A) NLI classification are run. The maximum P(contradiction) across both directions is used. This catches cases where one direction shows contradiction but the other does not.

## Pre-filters

1. Identical text fast-path: skip NLI for identical texts
2. Length ratio filter: skip if texts differ by more than 5:1
3. Cosine similarity filter: skip if embedding similarity below 0.85

These pre-filters reduce NLI invocations by approximately 90 percent while maintaining detection quality.