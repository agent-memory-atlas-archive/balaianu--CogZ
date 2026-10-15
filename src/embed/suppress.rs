//! Stderr capture during ONNX Runtime initialization.
//!
//! The ONNX Runtime C++ library emits ~1258 duplicate schema
//! registration warnings directly to stderr (fd 2) during init.
//! These are harmless (idempotent re-registration) but flood
//! terminal output. Redirecting them to a temp file instead of
//! /dev/null keeps the diagnostics: when init or model load fails,
//! the captured tail explains why (missing symbols, ABI mismatch,
//! corrupt model) instead of leaving only a bare error code.
//!
//! A temp file rather than a pipe: the warning volume exceeds any
//! reasonable pipe buffer, and a full pipe would block ORT's writer
//! mid-init. Captured text returned to callers is tail-bounded.

#[cfg(unix)]
use std::io::{Read, Seek, SeekFrom};

/// Bytes of captured stderr kept for diagnostics — the tail, where
/// failure causes land after the warning flood.
const CAPTURE_TAIL_BYTES: u64 = 4096;

/// Run a closure with stderr (fd 2) redirected to a temp file, then
/// restore the original stderr — including if the closure panics.
/// Returns the closure result and the tail of captured stderr.
///
/// On non-Unix platforms this is a no-op passthrough with an empty
/// capture.
#[cfg(unix)]
pub fn capture_stderr_during<F, T>(f: F) -> (T, String)
where
    F: FnOnce() -> T,
{
    use std::os::unix::io::AsRawFd;

    // Serialize with other stderr writers: they wait out the
    // redirection instead of interleaving into the capture file.
    let _lock = std::io::stderr().lock();

    let saved = unsafe { libc::dup(libc::STDERR_FILENO) };
    if saved < 0 {
        return (f(), String::new());
    }
    let _restore = StderrRestore(saved);

    let mut tmp = match tempfile::tempfile() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("cannot open stderr capture file: {e}");
            return (f(), String::new());
        }
    };
    unsafe {
        libc::dup2(tmp.as_raw_fd(), libc::STDERR_FILENO);
    }

    let result = f();

    let mut captured = String::new();
    if let Ok(len) = tmp.stream_position() {
        let start = len.saturating_sub(CAPTURE_TAIL_BYTES);
        if tmp.seek(SeekFrom::Start(start)).is_ok() {
            let mut buf = Vec::new();
            if tmp.read_to_end(&mut buf).is_ok() {
                captured = String::from_utf8_lossy(&buf).into_owned();
            }
        }
    }
    (result, captured)
}

/// Fold captured ORT stderr into a failure message. Empty captures
/// leave the message untouched.
pub fn with_diagnostics(msg: String, captured: &str) -> String {
    let trimmed = captured.trim();
    if trimmed.is_empty() {
        msg
    } else {
        format!("{msg} | ort stderr: {trimmed}")
    }
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

/// Non-Unix passthrough — no fd redirection available.
#[cfg(not(unix))]
pub fn capture_stderr_during<F, T>(f: F) -> (T, String)
where
    F: FnOnce() -> T,
{
    (f(), String::new())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // fd 2 redirection is process-global — parallel tests that each
    // redirect would snapshot one another's tempfiles. Serialize the
    // fd-touching tests.
    static FD2_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn stderr_target() -> (u64, u64) {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(libc::STDERR_FILENO, &mut st) }, 0);
        (st.st_dev as u64, st.st_ino as u64)
    }

    #[test]
    fn restores_stderr_after_panic() {
        let _fd2 = FD2_LOCK.lock().unwrap();
        let before = stderr_target();
        let _ = std::panic::catch_unwind(|| capture_stderr_during(|| panic!("boom")));
        assert_eq!(stderr_target(), before);
    }

    // eprintln!/print! are intercepted by the test harness before fd
    // 2 — only a raw write exercises the fd redirection.
    fn write_fd2(s: &str) {
        unsafe {
            libc::write(
                libc::STDERR_FILENO,
                s.as_ptr() as *const libc::c_void,
                s.len(),
            );
        }
    }

    #[test]
    fn returns_closure_result_and_capture() {
        let _fd2 = FD2_LOCK.lock().unwrap();
        let (result, captured) = capture_stderr_during(|| {
            write_fd2("ort-warning-test-marker\n");
            42
        });
        assert_eq!(result, 42);
        assert!(captured.contains("ort-warning-test-marker"));
    }

    #[test]
    fn capture_is_tail_bounded() {
        let _fd2 = FD2_LOCK.lock().unwrap();
        let big = "x".repeat((CAPTURE_TAIL_BYTES * 2) as usize);
        let (_r, captured) = capture_stderr_during(|| write_fd2(&big));
        assert!(captured.len() <= CAPTURE_TAIL_BYTES as usize + 8);
    }

    #[test]
    fn with_diagnostics_appends_only_when_present() {
        assert_eq!(with_diagnostics("failed".into(), ""), "failed");
        assert_eq!(with_diagnostics("failed".into(), "  \n"), "failed");
        assert!(with_diagnostics("failed".into(), "boom").contains("boom"));
    }
}
