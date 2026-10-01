---
id: 4bfc7dd1-bf5f-47da-8b67-41f341326d77
title: cfg-gated code must be typechecked per-target before release
type: rule
status: active
created_at: "2026-10-01T13:46:34.818969216+00:00"
updated_at: "2026-10-01T13:46:34.818969216+00:00"
references: []
confidence: 0.9
---

Code behind `#[cfg(unix)]`/`#[cfg(windows)]`/`cfg(target_os = ...)` is invisible to host builds, host tests, and host clippy — a compile error in a cfg'd branch ships silently until a real target build sees it. Before merging or releasing, every cfg'd branch must be typechecked for its target: `cargo check --target <triple>` when installed, an isolated scratch crate replicating the FFI surface when full-crate cross-check is impossible (e.g. C deps needing the target toolchain), or at minimum a CI matrix leg before tagging. The v0.5.0 Windows leg failure (unstable windows_by_handle APIs) is the canonical example — logic was reviewed, compilation never was.