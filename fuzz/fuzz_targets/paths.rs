#![no_main]

use libfuzzer_sys::fuzz_target;

// Path normalization decides identity across platforms. Invariants:
// idempotent, never produces backslashes, never panics on weird
// separators/drive letters/trailing junk.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let once = cogz::index::normalize_path(text);
    let twice = cogz::index::normalize_path(&once);
    assert!(!once.contains('\\'), "backslash survived: {once:?}");
    assert_eq!(once, twice, "normalize_path not idempotent");
});
