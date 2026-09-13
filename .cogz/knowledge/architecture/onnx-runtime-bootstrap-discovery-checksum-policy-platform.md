---
id: 3d5567f4-0294-4f88-923c-d449a1e14b67
title: "ONNX Runtime bootstrap — discovery, checksum policy, platform matrix"
type: knowledge
status: active
created_at: "2026-09-13T21:42:36.555826008+00:00"
updated_at: "2026-09-13T21:42:36.555826008+00:00"
references: ["50362381-7da8-564d-acc8-fc88130ab3ba", "2cb3afdc-72f0-56c0-a4ff-401674f1e87d"]
category: architecture
tags: ["architecture", "onnx", "embedding", "degradation"]
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