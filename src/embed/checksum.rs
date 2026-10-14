use std::path::{Path, PathBuf};

use crate::embed::models_dir;
use crate::embed::runtime::ORT_LIB_NAME;

/// Get the cogz lib directory: `~/.local/share/cogz/lib/`.
pub(crate) fn cogz_lib_dir() -> PathBuf {
    models_dir()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("lib")
}

/// The expected path for the downloaded ONNX Runtime library.
pub fn ort_lib_path() -> PathBuf {
    cogz_lib_dir().join(ORT_LIB_NAME)
}

/// Check if the ONNX Runtime library is available at a given path.
pub(crate) fn lib_exists(path: &Path) -> bool {
    path.exists() && path.is_file()
}

/// Compute SHA-256 of a file.
pub(crate) fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
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
