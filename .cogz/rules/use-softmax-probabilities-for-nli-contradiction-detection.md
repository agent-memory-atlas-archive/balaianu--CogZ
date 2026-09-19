---
id: 077cf823-5cc5-4251-b6b0-dd14ea8f6c05
title: Use softmax probabilities for NLI contradiction detection
type: rule
status: active
created_at: "2026-09-01T12:27:21.108864327+00:00"
updated_at: "2026-09-06T20:50:05.730370350+00:00"
references: []
confidence: 0.9
verified_against: ["034336be-ef90-55fb-8084-fc05f25701f3=30ea039cbd7cf18fd7b6d05c25bc890d6243193936d5e127fdf88dba08911b33", "07eb50dc-31a4-5530-be0b-dd10afd9dcc9=5a8f9609d1527ecd9dab99bc771b168e6b4eecdc25f2cbb2db4ca727713ec60f", "0f65e31a-4b23-59e2-9232-43229388bd69=ce0ff29aa64646f44f9afd66e6907d76dd3636b37fcc71f25def909f8653870a"]
---

NLI contradiction detection must use softmax probabilities with bidirectional scoring, not argmax-only labels. The contradiction threshold is 0.70, with cosine similarity and length ratio pre-filters. This matches the Python CogZ benchmark findings.