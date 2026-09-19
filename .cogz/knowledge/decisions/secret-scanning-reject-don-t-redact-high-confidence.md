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
verified_against: ["07eb50dc-31a4-5530-be0b-dd10afd9dcc9=5a8f9609d1527ecd9dab99bc771b168e6b4eecdc25f2cbb2db4ca727713ec60f", "1f6687db-3a4e-55c2-80a3-301639b2c880=876bc8af885f46f941702c991a2cc34823b30ec08336830dad7a78476eb28b4b", "30a2a961-a437-5b93-92a8-ed6c459277c8=45281f39fa75bf870e6ac96d4426d414cbf64de65235498271a2ca525d79db54", "3e1560ff-3053-529a-a434-55ee043a760f=ca06a9e4230916af7e6e53677439a32ff463394cd6ad8ce013b1bfae119498ee", "8016da99-e96b-5fe1-a4c0-be2236709b7e=14d4b56693d8a2a249fb170e620c1015f426feb6df4fa9cbf98a0239597c4e01", "e20584dd-8554-5045-b1dd-83d9b82d4e28=ab65823cc501c0148565b4cfa28863145bfc6878e4d2faa6791ecbe2a4154729", "e55d57a0-8fb9-5d5d-9687-34faeb58c773=546fafc18b28959b0ae65ba98402ed3a1a4ad93feee7ae66cda7d9a7b4050984"]
---

# Secret scanning design decisions

`src/security/scan.rs` scans entity content before it's written to disk. Three deliberate constraints, each chosen to avoid a worse failure mode:

1. **Reject, don't redact.** Redaction can mangle content in subtle ways and the agent should rewrite without the secret — a rejected write forces the fix at the source. Returns the SecretKind plus a short preview (never the full secret) for the error message.

2. **High-confidence patterns only.** Every write rejection is disruptive, so patterns must be specific enough to rarely match non-secrets: private key blocks, GitHub/AWS/Slack/OpenAI/Anthropic token shapes, generic credential formats.

3. **No entropy-based detection.** Deliberately excluded — a code-aware knowledge base legitimately discusses hashes, UUIDs, and encoded data, so high-entropy string detection produces too many false positives here.

Net effect: narrow detection with near-zero false positives; misses obfuscated or unusual secret formats by design.