---
id: e1f5a6b7-09cc-414e-831c-8f5710374d4a
title: Checksum skip on missing entry is a security hole
type: knowledge
status: active
created_at: "2026-09-03T11:59:00Z"
updated_at: "2026-09-06T06:35:34.327688543+00:00"
references: []
category: gotchas
tags: ["security", "checksum", "update", "download", "onnx"]
verified_against: ["121b7634-cd8c-507a-88f6-96c9e60ca1fe=afac761f0bb1491c43eee899bba619a83593c939e28385680cefc7a69af9e813", "1d5d7b37-d0ed-5df5-8fe8-db062a983583=0476cdb1faca13472acc462530e30adf11ea36c9d7f81635ac20765e2716264c", "2eb60b46-4e76-56d0-ac99-c5a9ba2fdde4=ebbb64285fe69b8c5b841b2585d253763dd8d7b0813e0e12e04ec0a98dc11ecc", "619836ea-a07b-561b-81db-23c730e1f4ca=f88e8cbaa95a9161957439e119c64777154f0248d70afae906865cd71cdfd35c", "6e0bea61-dc49-5c60-a4e6-7a65576d2191=33ce1c4b3385e37e1758fd8f15270783bb190a86b0bc6619470af6362a04bb47", "b163134f-ee7b-5fd7-9ac2-76d2f972e9ec=3a2ffc25b5682629eefddd078550caac480f0cf1d72bcf7551f9924f00134562", "c32f19d9-ee2e-5595-9652-db1b72baaf32=71e03ab52cadc1ba61089d477bd8d51686a9473bc10949238e47193ed96a6be1", "eef7d3ea-08f7-584c-bd19-ec30d124fb15=debc2a0bfc5d9514a84ba21fe9dcb2d3ab688e511ffc96e6298873f449525d0f"]
---

# Checksum skip on missing entry is a security hole

When downloading binaries (self-update, ONNX Runtime), the checksum
verification flow has a subtle failure mode: the SHA256SUMS file is
downloaded successfully, but it contains no entry for the target
asset. The original code silently skipped verification in this case.

This is a security hole. A compromised release could ship a
SHA256SUMS file without the expected asset entry, causing the
downloaded binary to be installed without any integrity check.

## The correct behavior

- SHA256SUMS file not available (network error, 404): warn and
  proceed (degraded mode — better than blocking all updates)
- SHA256SUMS downloaded but no matching entry: **fail with an error**
- SHA256SUMS downloaded, entry found, hash mismatch: **fail with an
  error**
- SHA256SUMS downloaded, entry found, hash matches: proceed

## Both paths now enforce the rule

1. **Self-update** (`src/update.rs`): `ChecksumEntryNotFound` error
   variant. Fails the update if SHA256SUMS doesn't contain an entry
   for the platform asset.

2. **ONNX Runtime download** (`src/embed/runtime.rs`): Returns an
   error if SHA256SUMS is downloaded but has no entry for the ORT
   asset. Only falls back to warn-only when SHA256SUMS itself is
   unavailable (network error, 404) — that's degraded mode, not a
   security bypass.

## Why warn-only for SHA256SUMS unavailable (not missing entry)

ONNX Runtime is optional (FTS-only mode works without it). Blocking
`cogz index` because Microsoft's SHA256SUMS endpoint is temporarily
unavailable would be worse than proceeding without verification —
the download is still over HTTPS. But if the file IS available and
doesn't mention the asset we're downloading, that's suspicious and
must fail.
