---
id: b0d2e3f4-09cc-414e-831c-8f5710374d4a
title: Downloads must verify checksums or fail explicitly
type: rule
status: active
created_at: "2026-09-03T12:12:00Z"
updated_at: "2026-09-03T12:12:00Z"
references: []
category: security
tags: ["security", "checksum", "download", "update"]
confidence: 1
verified_against: ["121b7634-cd8c-507a-88f6-96c9e60ca1fe=afac761f0bb1491c43eee899bba619a83593c939e28385680cefc7a69af9e813", "2eb60b46-4e76-56d0-ac99-c5a9ba2fdde4=ebbb64285fe69b8c5b841b2585d253763dd8d7b0813e0e12e04ec0a98dc11ecc", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "6e0bea61-dc49-5c60-a4e6-7a65576d2191=33ce1c4b3385e37e1758fd8f15270783bb190a86b0bc6619470af6362a04bb47", "eef7d3ea-08f7-584c-bd19-ec30d124fb15=debc2a0bfc5d9514a84ba21fe9dcb2d3ab688e511ffc96e6298873f449525d0f"]
---

Any binary download (self-update, ONNX Runtime) must either verify
a checksum or explicitly fail. Silently skipping verification when
a SHA256SUMS file is downloaded but lacks a matching entry is a
security hole.

**Rule:** If a SHA256SUMS file is available but doesn't contain an
entry for the target asset, fail with an error. Do not proceed
with an unverified binary.

**Exception:** If the SHA256SUMS file itself is unavailable (network
error, 404), warn and proceed — this is degraded mode, not a
security bypass. The binary is still downloaded over HTTPS.

**Why:** A compromised release could ship a SHA256SUMS file without
the expected asset entry, causing the binary to be installed without
integrity verification. Failing on missing entries closes this gap.
