---
id: 3d5567f4-0294-4f88-923c-d449a1e14b67
title: "ONNX Runtime bootstrap — discovery, checksum policy, platform matrix"
type: knowledge
status: active
created_at: "2026-09-13T21:42:36.555826008+00:00"
updated_at: "2026-09-21T13:09:42.567045888+00:00"
references: ["50362381-7da8-564d-acc8-fc88130ab3ba", "2cb3afdc-72f0-56c0-a4ff-401674f1e87d"]
category: architecture
tags: ["architecture", "onnx", "embedding", "degradation"]
verified_against: ["121b7634-cd8c-507a-88f6-96c9e60ca1fe=afac761f0bb1491c43eee899bba619a83593c939e28385680cefc7a69af9e813", "2cb3afdc-72f0-56c0-a4ff-401674f1e87d=fbcc31573b90e3de1fd0cdf09cc2a856061881bd297225e061aef31d7e348fa9", "50362381-7da8-564d-acc8-fc88130ab3ba=064299a0bbb3652d2eaa994bfe1484c5ab31356241be31b52095fee3732b6ec1", "650d6ff7-5271-55b8-be38-381525a35e27=01f7ab5b956c6ef1a4f1d00863fe86e778cf4fdabbb1a684050f1f8e8516e217", "6e0bea61-dc49-5c60-a4e6-7a65576d2191=33ce1c4b3385e37e1758fd8f15270783bb190a86b0bc6619470af6362a04bb47", "795bd6f4-14ab-5270-8b90-bd7dcc68a492=5c5d81f3d7620c60cae41903e3ff70c31fb7e1b0a9475460dc40dc95eeb8aa41", "8b994b90-d030-5c71-90b3-1366e90088a3=d899ee2cab66f2f5775d1d12d561fd3231f77278926a83d353b42b737e5f76c2", "b5cf26b5-dbf9-54ca-9d81-b3fd11236e90=68eb9d3fba7c1e3aaf2f923a1451533babec10a11eab38c636d3f262c23f668e", "e7910b28-053b-52de-846d-eeb9a5e1a549=182cfe304d7e7c7ef8fa4002f19098aa6ef094c2d848490bb94fec23611b35b4", "eef7d3ea-08f7-584c-bd19-ec30d124fb15=debc2a0bfc5d9514a84ba21fe9dcb2d3ab688e511ffc96e6298873f449525d0f"]
---

# ONNX Runtime bootstrap — discovery, download, verification

`ensure_ort()` runs once per process (`OnceLock`) before any ort API usage.

**Discovery order:** `ORT_DYLIB_PATH` env var → `~/.local/share/cogz/lib/` (previous download) → system paths (per-platform candidates like `/usr/lib/x86_64-linux-gnu/`).

**Download:** ORT 1.27.0 CPU-only from GitHub releases — chosen to match FastEmbed's shipped version for comparable inference. Checksum policy is deliberately asymmetric: SHA256SUMS fetched and verified; if the manifest *exists but lacks our asset* → hard fail (supply-chain red flag); if the manifest is *unavailable* (404) → proceed as degraded HTTPS-only mode with a warning. Microsoft doesn't always ship SHA256SUMS.

**Extraction defenses:** `tar xf --no-absolute-names` (tar-slip), then canonical-path containment check before loading (defense in depth). Windows: bsdtar handles .zip (ships with Win10 1803+); companion DLLs copied alongside.

**Platform matrix:** linux x86_64/aarch64, macOS aarch64, windows x86_64. macOS Intel excluded — Microsoft dropped ORT x86_64-apple-darwin binaries after 1.22; Rosetta or FTS-only are the fallbacks.

**Init quirk:** ORT C++ emits ~1258 duplicate schema-registration warnings to fd 2 during init — `suppress_stderr_during` dup2s stderr to /dev/null around `init_from` and `commit`. Harmless noise, suppressed because it floods terminals.

Failure → `ensure_ort` returns false → FTS-only mode. Never panics.

Known gap (2026-09-14): no network timeout on the download — a stalled connection hangs indefinitely (backlog item 26).