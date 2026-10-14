//! Self-update from GitHub releases.
//!
//! Checks the latest release version from the GitHub API, downloads
//! the new binary if newer, verifies the checksum, and atomically
//! replaces the current binary.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::net::{api_agent, download_agent};

const GITHUB_API: &str = "https://api.github.com/repos/balaianu/CogZ/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Error during self-update.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("checksum file downloaded but no entry found for asset {0}")]
    ChecksumEntryNotFound(String),
    #[error("no SHA256SUMS checksum manifest in release — refusing to install unverified binary")]
    ChecksumManifestNotFound,
    #[error("no suitable asset found in release")]
    NoAsset,
    #[error("self-update is not supported on {0}")]
    UnsupportedPlatform(String),
}

/// GitHub release response (partial).
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Result of a version check.
#[derive(Debug, PartialEq)]
pub enum VersionStatus {
    UpToDate(String),
    UpdateAvailable { current: String, latest: String },
}

/// Fetch the latest release record once. The same JSON carries the
/// tag for the version check and the asset URLs for the download —
/// a second fetch could race a republished release.
fn fetch_latest_release() -> Result<Release, UpdateError> {
    let resp = api_agent()
        .get(GITHUB_API)
        .header("User-Agent", "cogz-self-update")
        .call()
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    resp.into_body()
        .read_json()
        .map_err(|e| UpdateError::Network(e.to_string()))
}

/// Check the latest release version from GitHub.
pub fn check_latest_version() -> Result<VersionStatus, UpdateError> {
    let release = fetch_latest_release()?;
    Ok(version_status(&release))
}

/// Compare the release tag against the running version.
fn version_status(release: &Release) -> VersionStatus {
    // Strip leading 'v' from tag name.
    let latest = release.tag_name.trim_start_matches('v').to_string();

    // Semver compare, not string equality: a locally-built version
    // ahead of the latest release (e.g. building master between
    // releases) must report UpToDate, not "downgrade available".
    let newer = match (semver_triple(&latest), semver_triple(CURRENT_VERSION)) {
        (Some(l), Some(c)) => l > c,
        _ => latest != CURRENT_VERSION,
    };

    if newer {
        VersionStatus::UpdateAvailable {
            current: CURRENT_VERSION.to_string(),
            latest,
        }
    } else {
        VersionStatus::UpToDate(latest)
    }
}

/// Parse "major.minor.patch", ignoring any pre-release or build
/// suffix. Release tags are always vX.Y.Z; a malformed version
/// returns None and the caller falls back to string equality.
fn semver_triple(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let maj = parts.next()?.parse().ok()?;
    let min = parts.next()?.parse().ok()?;
    let pat = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((maj, min, pat))
}

/// Detect the current platform's asset name.
/// Returns None for unsupported platforms — the caller should fail
/// with a clear error rather than downloading a wrong-arch binary.
fn platform_asset_name() -> Option<&'static str> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (os, arch) {
        ("linux", "x86_64") => Some("cogz-x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("cogz-aarch64-unknown-linux-gnu"),
        ("macos", "aarch64") => Some("cogz-aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("cogz-x86_64-pc-windows-msvc.exe"),
        _ => None,
    }
}

/// Find the download URL for the current platform's binary.
fn find_asset_url(release: &Release) -> Option<&str> {
    let target = platform_asset_name()?;
    release
        .assets
        .iter()
        .find(|a| a.name == target)
        .map(|a| a.browser_download_url.as_str())
}

/// Find the checksum URL.
fn find_checksum_url(release: &Release) -> Option<&str> {
    release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .map(|a| a.browser_download_url.as_str())
}

/// Download a file to a temporary path.
fn download_file(url: &str, dest: &Path) -> Result<(), UpdateError> {
    let resp = download_agent()
        .get(url)
        .header("User-Agent", "cogz-self-update")
        .call()
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    let mut file = std::fs::File::create(dest)?;
    let mut reader = resp.into_body().into_reader();
    std::io::copy(&mut reader, &mut file)?;
    file.sync_all()?;
    Ok(())
}

/// Compute SHA-256 of a file.
fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
}

/// Parse a SHA256SUMS file and find the checksum for the target asset.
fn find_checksum(sums_content: &str, asset_name: &str) -> Option<String> {
    for line in sums_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 && parts[1] == asset_name {
            return Some(parts[0].to_string());
        }
    }
    None
}

/// Get the path to the current binary.
fn current_binary_path() -> Result<PathBuf, UpdateError> {
    std::env::current_exe().map_err(Into::into)
}

/// Run the self-update process.
///
/// 1. Check latest version from GitHub
/// 2. If newer, download the binary and checksum
/// 3. Verify checksum
/// 4. Atomically replace the current binary
pub fn run_update() -> Result<String, UpdateError> {
    // A previous Windows update may have left the moved-aside binary
    // beside the exe — the running image can be renamed but not
    // deleted, so cleanup happens on the next run.
    #[cfg(windows)]
    if let Ok(exe) = current_binary_path() {
        let old = exe.with_extension("exe.old");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }

    let release = fetch_latest_release()?;

    match version_status(&release) {
        VersionStatus::UpToDate(v) => Ok(format!("CogZ is up to date (v{})", v)),
        VersionStatus::UpdateAvailable { current, latest } => {
            let asset_name = platform_asset_name().ok_or_else(|| {
                UpdateError::UnsupportedPlatform(format!(
                    "{} {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            })?;
            let asset_url = find_asset_url(&release).ok_or(UpdateError::NoAsset)?;

            // Stage downloads in a private temp dir inside the install
            // directory: same filesystem so the replace rename is
            // atomic, and no predictable /tmp name another user could
            // pre-plant. TempDir cleans every exit path itself.
            let bin_path = current_binary_path()?;
            let bin_dir = bin_path.parent().ok_or_else(|| {
                UpdateError::Io(std::io::Error::other("binary path has no parent directory"))
            })?;
            let staging = tempfile::Builder::new()
                .prefix(".cogz-update-")
                .tempdir_in(bin_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))?;
            }
            let temp_binary = staging.path().join("cogz");
            let temp_checksum = staging.path().join("SHA256SUMS");

            download_file(asset_url, &temp_binary)?;

            // Download and verify checksum. The checksum asset is
            // required — a missing manifest is a supply-chain bypass,
            // not a degraded mode.
            let checksum_url =
                find_checksum_url(&release).ok_or(UpdateError::ChecksumManifestNotFound)?;
            download_file(checksum_url, &temp_checksum)?;
            let sums_content = std::fs::read_to_string(&temp_checksum)?;
            let expected_hash = find_checksum(&sums_content, asset_name)
                .ok_or_else(|| UpdateError::ChecksumEntryNotFound(asset_name.to_string()))?;
            let actual_hash = sha256_file(&temp_binary)?;
            if expected_hash != actual_hash {
                return Err(UpdateError::ChecksumMismatch {
                    expected: expected_hash,
                    actual: actual_hash,
                });
            }

            // Make the new binary executable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&temp_binary)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&temp_binary, perms)?;
            }

            // Backup current binary — required, not best-effort.
            // If backup fails, leave the original untouched.
            let backup = bin_path.with_extension("bak");
            if bin_path.exists() {
                std::fs::copy(&bin_path, &backup).map_err(|e| {
                    UpdateError::Io(std::io::Error::other(format!(
                        "failed to backup current binary: {e}"
                    )))
                })?;
            }

            // Atomic replace: the staged file is already in the
            // destination directory, so the rename is same-filesystem.
            #[cfg(windows)]
            {
                // A running exe can't be deleted or overwritten on
                // Windows, but it CAN be renamed. Move it aside,
                // place the new binary, and remove the stale .old on
                // the next run (cleanup at the top of run_update).
                let old = bin_path.with_extension("exe.old");
                if old.exists() {
                    let _ = std::fs::remove_file(&old);
                }
                std::fs::rename(&bin_path, &old).map_err(|e| {
                    UpdateError::Io(std::io::Error::other(format!(
                        "failed to move running binary aside: {e}"
                    )))
                })?;
                if let Err(e) = std::fs::rename(&temp_binary, &bin_path) {
                    let _ = std::fs::rename(&old, &bin_path);
                    return Err(UpdateError::Io(std::io::Error::other(format!(
                        "failed to place new binary: {e}"
                    ))));
                }
            }
            #[cfg(not(windows))]
            {
                // Unix rename overwrites atomically — this also works
                // while the target is the running process.
                if let Err(e) = std::fs::rename(&temp_binary, &bin_path) {
                    let _ = std::fs::remove_file(&bin_path);
                    if let Err(e2) = std::fs::rename(&temp_binary, &bin_path) {
                        if backup.exists() {
                            let _ = std::fs::rename(&backup, &bin_path);
                        }
                        return Err(UpdateError::Io(std::io::Error::other(format!(
                            "failed to replace binary (rename1: {e}, rename2: {e2})"
                        ))));
                    }
                }
            }

            let _ = std::fs::remove_file(&backup);

            Ok(format!("Updated CogZ from v{current} to v{latest}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_checksum_matches_correct_asset() {
        let sums =
            "abc123  cogz-x86_64-unknown-linux-gnu\ndef456  cogz-aarch64-unknown-linux-gnu\n";
        assert_eq!(
            find_checksum(sums, "cogz-x86_64-unknown-linux-gnu"),
            Some("abc123".to_string())
        );
        assert_eq!(
            find_checksum(sums, "cogz-aarch64-unknown-linux-gnu"),
            Some("def456".to_string())
        );
    }

    #[test]
    fn find_checksum_returns_none_for_missing_asset() {
        let sums = "abc123  cogz-x86_64-unknown-linux-gnu\n";
        assert_eq!(find_checksum(sums, "cogz-windows-x86_64.exe"), None);
    }

    #[test]
    fn find_checksum_handles_empty_file() {
        assert_eq!(find_checksum("", "any-asset"), None);
    }

    #[test]
    fn find_checksum_handles_malformed_lines() {
        let sums = "not-a-valid-line\nabc123  cogz-x86_64-unknown-linux-gnu\n";
        assert_eq!(
            find_checksum(sums, "cogz-x86_64-unknown-linux-gnu"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn semver_triple_parses_release_shapes() {
        assert_eq!(semver_triple("0.5.8"), Some((0, 5, 8)));
        assert_eq!(semver_triple("1.27.0"), Some((1, 27, 0)));
        assert_eq!(semver_triple("0.6.0-rc.1"), Some((0, 6, 0)));
        assert_eq!(semver_triple("v0.5.8"), None);
        assert_eq!(semver_triple("0.5"), None);
        assert_eq!(semver_triple("0.5.8.1"), None);
        assert_eq!(semver_triple("latest"), None);
    }

    #[test]
    fn semver_ordering_handles_version_bumps() {
        let newer =
            |latest: &str, current: &str| match (semver_triple(latest), semver_triple(current)) {
                (Some(l), Some(c)) => l > c,
                _ => latest != current,
            };
        assert!(newer("0.5.9", "0.5.8"));
        assert!(newer("0.6.0", "0.5.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.5.8", "0.5.8"));
        // Local build ahead of the release must not offer a downgrade.
        assert!(!newer("0.5.8", "0.5.9-dev"));
        assert!(!newer("0.5.8", "0.6.0"));
        // Malformed falls back to string inequality.
        assert!(!newer("0.5.8", "0.5.8"));
    }

    #[test]
    fn platform_asset_name_returns_some_on_supported_platforms() {
        // This test runs on the current platform. It verifies that
        // platform_asset_name returns a value (the current platform
        // is always supported since CI runs on supported OSes).
        let result = platform_asset_name();
        assert!(
            result.is_some(),
            "platform_asset_name returned None on {} {} — \
             this platform should be supported or CI should not run here",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
}
