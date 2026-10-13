---
id: 632d3c6b-af68-43e2-ae5b-95be8fab4f52
title: "Secret scanning — reject, don't redact; high-confidence patterns only"
type: knowledge
status: active
created_at: "2026-09-13T21:43:08.543779922+00:00"
updated_at: "2026-09-13T21:43:08.543779922+00:00"
references: ["8016da99-e96b-5fe1-a4c0-be2236709b7e", "e55d57a0-8fb9-5d5d-9687-34faeb58c773"]
category: decisions
tags: ["decision", "security", "secrets"]
---

# Secret scanning design decisions

`src/security/scan.rs` scans entity content before it's written to disk. Three deliberate constraints, each chosen to avoid a worse failure mode:

1. **Reject, don't redact.** Redaction can mangle content in subtle ways and the agent should rewrite without the secret — a rejected write forces the fix at the source. Returns the SecretKind plus a short preview (never the full secret) for the error message.

2. **High-confidence patterns only.** Every write rejection is disruptive, so patterns must be specific enough to rarely match non-secrets: private key blocks, GitHub/AWS/Slack/OpenAI/Anthropic token shapes, generic credential formats.

3. **No entropy-based detection.** Deliberately excluded — a code-aware knowledge base legitimately discusses hashes, UUIDs, and encoded data, so high-entropy string detection produces too many false positives here.

Net effect: narrow detection with near-zero false positives; misses obfuscated or unusual secret formats by design.