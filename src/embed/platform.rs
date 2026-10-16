//! Platform constants for the ONNX Runtime download.

/// ONNX Runtime version to download. ort 2.0.0-rc.13 supports ORT 1.28;
/// we use 1.27.0 to match the version FastEmbed ships, ensuring
/// comparable inference performance.
pub(crate) const ORT_VERSION: &str = "1.27.0";

// ── Platform-specific constants ─────────────────────────────────────

/// GitHub release asset name for the current platform.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) const ORT_ASSET: &str = "onnxruntime-linux-x64-1.27.0.tgz";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) const ORT_ASSET: &str = "onnxruntime-linux-aarch64-1.27.0.tgz";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) const ORT_ASSET: &str = "onnxruntime-osx-arm64-1.27.0.tgz";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) const ORT_ASSET: &str = "onnxruntime-win-x64-1.27.0.zip";

/// Fallback for unsupported platforms (e.g. macOS Intel). The
/// download function checks this and returns an error.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
pub(crate) const ORT_ASSET: &str = "";

/// SHA-256 of the GitHub release asset for the current platform,
/// pinned at ORT_VERSION bump time from the release's published
/// digests. The pin is the trust root: a compromised CDN could serve
/// a matching fake manifest alongside a bad binary, but can't make
/// the bytes hash to the pinned value.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) const ORT_SHA256: &str =
    "547e40a48f1fe73e3f812d7c88a948612c23f896b91e4e2ee1e232d7b468246f";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) const ORT_SHA256: &str =
    "3e4d83ac06924a32a07b6d7f91ce6f852876153fc0bbdf931bf517a140bfbe48";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) const ORT_SHA256: &str =
    "545e81c58152353acb0d1e8bd6ce4b62f830c0961f5b3acfedc790ffd76e477a";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub(crate) const ORT_SHA256: &str =
    "c5c81710938e68079ff1a192b04897faabe4b43830d48f39f27ecd4e16138bfc";

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
pub(crate) const ORT_SHA256: &str = "";

/// The shared library file name for the current platform.
#[cfg(target_os = "linux")]
pub(crate) const ORT_LIB_NAME: &str = "libonnxruntime.so";

#[cfg(target_os = "macos")]
pub(crate) const ORT_LIB_NAME: &str = "libonnxruntime.dylib";

#[cfg(target_os = "windows")]
pub(crate) const ORT_LIB_NAME: &str = "onnxruntime.dll";

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) const ORT_LIB_NAME: &str = "";

/// Archive extension for the current platform's ORT download.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) const ORT_ARCHIVE_EXT: &str = "tgz";

#[cfg(target_os = "windows")]
pub(crate) const ORT_ARCHIVE_EXT: &str = "zip";

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) const ORT_ARCHIVE_EXT: &str = "";

/// Whether this platform supports ORT auto-download.
pub(crate) const fn ort_download_supported() -> bool {
    !ORT_ASSET.is_empty()
}
