//! Crash-safe file writes for canonical entity files.

use std::io::Write;
use std::path::Path;

/// Write `content` to `path` atomically: a uniquely-named temp file in
/// the same directory is written, fsynced, and renamed over `path`.
/// The original is never truncated in place — either the rename
/// completes and the new content is visible, or the old file remains.
/// On Unix the parent directory is fsynced so the rename itself
/// survives a power loss.
pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("entity");
    // A UUID temp name plus create_new: a crashed predecessor's temp
    // file can't collide, and a symlink pre-planted at a predictable
    // name can't redirect the write outside .cogz.
    let temp_path = parent.join(format!(".{stem}.{}.tmp", uuid::Uuid::new_v4()));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let result = file.write_all(content).and_then(|()| file.sync_all());
        drop(file);
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(e);
        }
    }
    if let Err(e) = replace_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }
    fsync_dir(parent);
    Ok(())
}

#[cfg(unix)]
fn replace_file(temp: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::rename(temp, dest)
}

/// Windows `rename` fails when the destination exists. MoveFileExW
/// with MOVEFILE_REPLACE_EXISTING is the atomic-overwrite primitive.
#[cfg(windows)]
fn replace_file(temp: &Path, dest: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};

    let temp_w: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let dest_w: Vec<u16> = dest.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe { MoveFileExW(temp_w.as_ptr(), dest_w.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Best-effort directory fsync. Some filesystems can't fsync a
/// directory (FUSE mounts, network filesystems); the rename or
/// create is still ordered correctly at the VFS level, so failures
/// are ignorable.
#[cfg(unix)]
pub(crate) fn fsync_dir(dir: &Path) {
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

#[cfg(not(unix))]
pub(crate) fn fsync_dir(_: &Path) {}
