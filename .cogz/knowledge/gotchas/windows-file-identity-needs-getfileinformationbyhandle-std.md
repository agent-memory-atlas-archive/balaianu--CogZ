---
id: ca9ca5e2-af9b-4019-a1aa-fa51f0f6aa75
title: Windows file identity needs GetFileInformationByHandle — std MetadataExt identity fields are unstable
type: knowledge
status: active
created_at: "2026-10-01T13:46:33.785256372+00:00"
updated_at: "2026-10-01T13:46:33.785256372+00:00"
references: ["9944eb9f-b0e9-5bf3-b0eb-b33b5475a2a6"]
category: gotchas
tags: ["windows", "ffi", "windows-sys", "file-identity", "cross-platform"]
verified_against: ["9944eb9f-b0e9-5bf3-b0eb-b33b5475a2a6=2623880ee0ed6dc7f2ac01543d303b5d1932b18fa52d38b8fc757632c0519a7e"]
---

`std::os::windows::fs::MetadataExt` on stable Rust exposes only `file_attributes`, `creation_time`, `last_access_time`, `last_write_time`, `file_size`. The useful identity methods — `volume_serial_number()` and `file_index()` — are behind the unstable `windows_by_handle` feature (rust#63010) and fail with E0658 on stable.

Stable path for real file identity: open the file, cast `file.as_raw_handle()` to `windows_sys::Win32::Foundation::HANDLE`, call `GetFileInformationByHandle` into a zeroed `BY_HANDLE_FILE_INFORMATION`, then `dev = dwVolumeSerialNumber`, `ino = nFileIndexHigh<<32 | nFileIndexLow`. `windows-sys` 0.59 with features `Win32_Foundation` + `Win32_Storage_FileSystem` — already in the dep tree transitively so the direct dep adds nothing to builds.

Verification trick when the full crate can't cross-check (aws-lc-sys needs MSVC's C toolchain even for `cargo check --target x86_64-pc-windows-msvc`): make a scratch crate depending only on windows-sys, paste the function, `cargo check --target x86_64-pc-windows-msvc` — typechecks the FFI signature in seconds.