//! Stderr suppression during ONNX Runtime initialization.
//!
//! The ONNX Runtime C++ library emits ~1258 duplicate schema
//! registration warnings directly to stderr (fd 2) during init.
//! These are harmless (idempotent re-registration) but flood
//! terminal output. This module provides a helper to temporarily
//! redirect stderr to /dev/null during a closure call.

/// Run a closure with stderr (fd 2) redirected to /dev/null, then
/// restore the original stderr — including if the closure panics.
///
/// On non-Unix platforms this is a no-op passthrough.
#[cfg(unix)]
pub fn suppress_stderr_during<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    use std::os::unix::io::AsRawFd;

    // Serialize with other stderr writers: they wait out the
    // redirection instead of being swallowed by /dev/null.
    let _lock = std::io::stderr().lock();

    let saved = unsafe { libc::dup(libc::STDERR_FILENO) };
    if saved < 0 {
        return f();
    }
    let _restore = StderrRestore(saved);

    match std::fs::File::open("/dev/null") {
        Ok(null) => {
            let _ = unsafe { libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO) };
        }
        Err(e) => {
            tracing::warn!("cannot open /dev/null for stderr suppression: {e}");
        }
    }

    f()
}

/// Restores fd 2 from the duplicated fd on drop, including during
/// panic unwinding.
#[cfg(unix)]
struct StderrRestore(libc::c_int);

#[cfg(unix)]
impl Drop for StderrRestore {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.0, libc::STDERR_FILENO);
            libc::close(self.0);
        }
    }
}

/// Run a closure with stderr (fd 2) redirected to /dev/null, then
/// restore the original stderr.
///
/// On non-Unix platforms this is a no-op passthrough.
#[cfg(not(unix))]
pub fn suppress_stderr_during<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    f()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn stderr_target() -> (u64, u64) {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(libc::STDERR_FILENO, &mut st) }, 0);
        (st.st_dev as u64, st.st_ino as u64)
    }

    #[test]
    fn restores_stderr_after_panic() {
        let before = stderr_target();
        let _ = std::panic::catch_unwind(|| suppress_stderr_during(|| panic!("boom")));
        assert_eq!(stderr_target(), before);
    }

    #[test]
    fn returns_closure_result() {
        assert_eq!(suppress_stderr_during(|| 42), 42);
    }
}
