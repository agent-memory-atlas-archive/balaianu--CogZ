//! ONNX Runtime library discovery and auto-download.
//!
//! The `ort` crate with `load-dynamic` requires the ONNX Runtime
//! shared library at runtime. This module handles:
//!
//! 1. Checking if the library is already available (system install,
//!    `ORT_DYLIB_PATH`, or previously downloaded to cogz's lib dir).
//! 2. Downloading the CPU-only ONNX Runtime from GitHub releases to
//!    `~/.local/share/cogz/lib/` if not found.
//! 3. Initializing `ort` with the discovered path via `ort::init_from()`.
//!
//! Supported platforms for auto-download:
//! - Linux x86_64, Linux aarch64
//! - macOS arm64 (Apple Silicon)
//! - Windows x86_64
//!
//! macOS x86_64 (Intel) is not supported — Microsoft dropped macOS
//! Intel binaries after ORT 1.22. Intel Mac users can use Rosetta 2
//! or operate in FTS-only mode.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[path = "checksum.rs"]
mod checksum;

use super::suppress::{capture_stderr_during, with_diagnostics};
pub use checksum::ort_lib_path;
use checksum::{cogz_lib_dir, lib_exists, sha256_file};
use tracing::{info, warn};

/// ONNX Runtime version to download. ort 2.0.0-rc.13 supports ORT 1.28;
/// we use 1.27.0 to match the version FastEmbed ships, ensuring
/// comparable inference performance.
const ORT_VERSION: &str = "1.27.0";

// ── Platform-specific constants ─────────────────────────────────────

/// GitHub release asset name for the current platform.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const ORT_ASSET: &str = "onnxruntime-linux-x64-1.27.0.tgz";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const ORT_ASSET: &str = "onnxruntime-linux-aarch64-1.27.0.tgz";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const ORT_ASSET: &str = "onnxruntime-osx-arm64-1.27.0.tgz";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const ORT_ASSET: &str = "onnxruntime-win-x64-1.27.0.zip";

/// Fallback for unsupported platforms (e.g. macOS Intel). The
/// download function checks this and returns an error.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
const ORT_ASSET: &str = "";

/// SHA-256 of the GitHub release asset for the current platform,
/// pinned at ORT_VERSION bump time from the release's published
/// digests. The pin is the trust root: a compromised CDN could serve
/// a matching fake manifest alongside a bad binary, but can't make
/// the bytes hash to the pinned value.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const ORT_SHA256: &str = "547e40a48f1fe73e3f812d7c88a948612c23f896b91e4e2ee1e232d7b468246f";

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const ORT_SHA256: &str = "3e4d83ac06924a32a07b6d7f91ce6f852876153fc0bbdf931bf517a140bfbe48";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const ORT_SHA256: &str = "545e81c58152353acb0d1e8bd6ce4b62f830c0961f5b3acfedc790ffd76e477a";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const ORT_SHA256: &str = "c5c81710938e68079ff1a192b04897faabe4b43830d48f39f27ecd4e16138bfc";

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
const ORT_SHA256: &str = "";

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
const ORT_ARCHIVE_EXT: &str = "tgz";

#[cfg(target_os = "windows")]
const ORT_ARCHIVE_EXT: &str = "zip";

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
const ORT_ARCHIVE_EXT: &str = "";

/// Whether this platform supports ORT auto-download.
const fn ort_download_supported() -> bool {
    !ORT_ASSET.is_empty()
}

/// Whether the ORT auto-download is permitted for this process. Set
/// once by entry points that know the download policy (config
/// `embedding.auto_download`, `--no-download`); entry points that
/// never set it get the permissive default.
static ORT_DOWNLOAD_ALLOWED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Allow or forbid the ORT auto-download for this process. When
/// forbidden, `ensure_ort` uses only an already-installed library.
pub fn set_ort_download_allowed(allowed: bool) {
    ORT_DOWNLOAD_ALLOWED.store(allowed, std::sync::atomic::Ordering::Relaxed);
}

fn ort_download_permitted() -> bool {
    ORT_DOWNLOAD_ALLOWED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether all model loading is forbidden for this process. Set once
/// by entry points from config `embedding.force_fts_only`; model
/// constructors check it and produce permanently-unavailable models.
static FTS_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Allow or forbid all model loading for this process (config
/// `embedding.force_fts_only`). Default: loading allowed.
pub fn set_fts_only(fts_only: bool) {
    FTS_ONLY.store(fts_only, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn fts_only() -> bool {
    FTS_ONLY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Static init guard — ensures `ort::init_from` is called exactly once.
static ORT_INIT: OnceLock<bool> = OnceLock::new();

/// System library paths to check for an existing ONNX Runtime install.
/// Returns platform-appropriate candidate paths.
fn system_lib_candidates() -> Vec<&'static str> {
    let mut candidates = Vec::new();

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        candidates.extend_from_slice(&[
            "/usr/lib/x86_64-linux-gnu/libonnxruntime.so",
            "/usr/lib/x86_64-linux-gnu/libonnxruntime.so.1",
            "/usr/lib/x86_64-linux-gnu/libonnxruntime.so.1.23",
            "/usr/local/lib/libonnxruntime.so",
            "/usr/lib/libonnxruntime.so",
        ]);
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        candidates.extend_from_slice(&[
            "/usr/lib/aarch64-linux-gnu/libonnxruntime.so",
            "/usr/lib/aarch64-linux-gnu/libonnxruntime.so.1",
            "/usr/local/lib/libonnxruntime.so",
            "/usr/lib/libonnxruntime.so",
        ]);
    }

    #[cfg(target_os = "macos")]
    {
        candidates.extend_from_slice(&[
            "/opt/homebrew/lib/libonnxruntime.dylib",
            "/usr/local/lib/libonnxruntime.dylib",
        ]);
    }

    #[cfg(target_os = "windows")]
    {
        candidates.extend_from_slice(&[
            "C:\\Program Files\\onnxruntime\\lib\\onnxruntime.dll",
            "C:\\onnxruntime\\lib\\onnxruntime.dll",
        ]);
    }

    candidates
}

/// Try to find an existing ONNX Runtime library. Checks:
/// 1. `ORT_DYLIB_PATH` env var (user override)
/// 2. cogz's lib dir (previously downloaded)
/// 3. System library paths
fn find_existing_lib() -> Option<PathBuf> {
    // 1. User override via env var
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH")
        && !path.is_empty()
        && lib_exists(Path::new(&path))
    {
        return Some(PathBuf::from(path));
    }

    // 2. Previously downloaded to cogz lib dir
    let cogz_path = ort_lib_path();
    if lib_exists(&cogz_path) {
        return Some(cogz_path);
    }

    // 3. System library paths
    for candidate in system_lib_candidates() {
        if lib_exists(Path::new(candidate)) {
            return Some(PathBuf::from(candidate));
        }
    }

    None
}

/// Download and extract the ONNX Runtime library to cogz's lib dir.
/// Downloads the CPU-only build from GitHub releases and verifies it
/// against the pinned per-target SHA-256 before extracting.
fn download_ort() -> Result<PathBuf, std::io::Error> {
    if !ort_download_supported() {
        return Err(std::io::Error::other(
            "ONNX Runtime auto-download is not supported on this platform. \
             Install onnxruntime manually and set ORT_DYLIB_PATH, or use FTS-only mode.",
        ));
    }

    let lib_dir = cogz_lib_dir();
    std::fs::create_dir_all(&lib_dir)?;

    let lib_path = ort_lib_path();
    if lib_exists(&lib_path) {
        return Ok(lib_path);
    }

    let url = format!(
        "https://github.com/microsoft/onnxruntime/releases/download/v{}/{}",
        ORT_VERSION, ORT_ASSET
    );

    info!("downloading ONNX Runtime {} from GitHub", ORT_VERSION);

    // Stage inside the private cogz lib dir, not the shared system
    // temp — a predictable temp_dir() path lets another user pre-plant
    // a directory we then write the archive into. TempDir removes
    // itself on every exit path.
    let staging = tempfile::Builder::new()
        .prefix("ort-")
        .tempdir_in(&lib_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let temp_dir = staging.path();

    let archive_name = format!("onnxruntime.{}", ORT_ARCHIVE_EXT);
    let archive_path = temp_dir.join(&archive_name);

    // Download the archive using ureq (already a dependency).
    let response = crate::net::download_agent()
        .get(&url)
        .call()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NetworkUnreachable, e.to_string()))?;
    let mut body = response.into_body().into_reader();
    let mut file = std::fs::File::create(&archive_path)?;
    std::io::copy(&mut body, &mut file)?;
    drop(file);

    // Verify the archive against the pinned hash before extracting.
    // Fail closed on any mismatch — an unverified ORT binary is never
    // loaded. The pinned const also removes the need for a second
    // network fetch: upstream doesn't always ship SHA256SUMS.
    let actual_hash = sha256_file(&archive_path)?;
    if actual_hash != ORT_SHA256 {
        return Err(std::io::Error::other(format!(
            "ONNX Runtime checksum mismatch: expected {ORT_SHA256}, got {actual_hash} — \
             refusing to install unverified binary"
        )));
    }
    info!("ONNX Runtime checksum verified against pinned hash");

    // Extract the archive. `tar` is available on all supported
    // platforms, but the binary differs:
    // - Linux: GNU tar — pass --no-absolute-names against tar slip.
    // - macOS and Windows 10 1803+: bsdtar — strips absolute paths,
    //   refuses `..`, and refuses symlink-altered targets by DEFAULT,
    //   and has no --no-absolute-names flag (it would fail outright).
    let extract_dir = temp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir)?;

    let mut tar = std::process::Command::new("tar");
    tar.arg("xf").arg(&archive_path).arg("-C").arg(&extract_dir);
    #[cfg(target_os = "linux")]
    tar.arg("--no-absolute-names");
    let output = tar.output()?;

    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "archive extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // Find the shared library in the extracted directory.
    // Verify the found path is inside extract_dir (defense-in-depth
    // against tar slip even after --no-absolute-names).
    let mut found_lib: Option<PathBuf> = None;
    for entry in walkdir::WalkDir::new(&extract_dir) {
        let entry = entry?;
        if entry.file_name() == ORT_LIB_NAME {
            let path = entry.path().to_path_buf();
            // Canonicalize both paths to compare them reliably.
            let canonical_extract = extract_dir
                .canonicalize()
                .unwrap_or_else(|_| extract_dir.clone());
            let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical_path.starts_with(&canonical_extract) {
                return Err(std::io::Error::other(
                    "archive contains path outside extraction directory — refusing to load",
                ));
            }
            found_lib = Some(path);
            break;
        }
    }

    let Some(src_lib) = found_lib else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} not found in archive", ORT_LIB_NAME),
        ));
    };

    // Copy the library to cogz's lib dir
    std::fs::copy(&src_lib, &lib_path)?;

    // On Windows, the DLL may have companion libraries that need to be
    // in the same directory. Copy all .dll files from the lib directory
    // in the archive.
    #[cfg(target_os = "windows")]
    {
        if let Some(src_lib_dir) = src_lib.parent()
            && let Ok(entries) = std::fs::read_dir(src_lib_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("dll")
                    && let Some(name) = path.file_name()
                {
                    let dest = lib_dir.join(name);
                    if !dest.exists() {
                        let _ = std::fs::copy(&path, &dest);
                    }
                }
            }
        }
    }

    info!(
        "ONNX Runtime installed to {} ({}KB)",
        lib_path.display(),
        lib_path.metadata().map(|m| m.len() / 1024).unwrap_or(0)
    );

    Ok(lib_path)
}

/// Ensure the ONNX Runtime is available and initialize `ort` with the
/// correct library path. Must be called before any `ort` API usage.
///
/// If the library is already available (system, env var, or previously
/// downloaded), uses it directly. If not, downloads the CPU-only
/// runtime to `~/.local/share/cogz/lib/`.
///
/// If the library cannot be found or downloaded, returns false — the
/// caller should operate in FTS-only mode.
pub fn ensure_ort() -> bool {
    *ORT_INIT.get_or_init(|| {
        // Try to find an existing library first.
        let lib_path = match find_existing_lib() {
            Some(path) => path,
            None => {
                if !ort_download_permitted() {
                    warn!("ONNX Runtime not installed and auto-download is disabled");
                    warn!("system will operate in FTS-only mode");
                    return false;
                }
                // Try to download it.
                match download_ort() {
                    Ok(path) => path,
                    Err(e) => {
                        warn!("ONNX Runtime not found and download failed: {}", e);
                        warn!("system will operate in FTS-only mode");
                        warn!("to enable embeddings, install onnxruntime or set ORT_DYLIB_PATH");
                        return false;
                    }
                }
            }
        };

        // Initialize ort with the discovered library path.
        // This must happen before any Session::builder() call.
        // In ort 2.0.0-rc.13, init_from returns Result and commit returns bool.
        //
        // The ONNX Runtime C++ library emits ~1258 duplicate schema
        // registration warnings directly to stderr (fd 2) during init.
        // These are harmless (idempotent re-registration) but flood
        // terminal output. Capture stderr during init, then restore —
        // on failure the captured tail is the only place the C++ side
        // says why.
        let (init_result, init_stderr) =
            capture_stderr_during(|| ort::init_from(lib_path.to_string_lossy().to_string()));
        match init_result {
            Ok(builder) => {
                let (committed, commit_stderr) = capture_stderr_during(|| builder.commit());
                if committed {
                    info!("ONNX Runtime initialized from {}", lib_path.display());
                    true
                } else {
                    warn!(
                        "ONNX Runtime commit failed from {}",
                        with_diagnostics(lib_path.display().to_string(), &commit_stderr)
                    );
                    false
                }
            }
            Err(e) => {
                warn!(
                    "ONNX Runtime init failed from {}: {}",
                    lib_path.display(),
                    with_diagnostics(e.to_string(), &init_stderr)
                );
                false
            }
        }
    })
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
