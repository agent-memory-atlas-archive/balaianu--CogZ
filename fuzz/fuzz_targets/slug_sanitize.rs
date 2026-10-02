#![no_main]

use libfuzzer_sys::fuzz_target;

// Slugs and categories become real filesystem paths. The contract:
// ASCII slug characters only, bounded length, no separators or
// traversal, sanitize_category never empty.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    for out in [
        cogz::files::entities::slugify(text),
        cogz::files::entities::sanitize_category(text),
        cogz::files::entities::slug_with_hash(&cogz::files::entities::slugify(text), text),
    ] {
        assert!(out.is_ascii(), "non-ascii output: {out:?}");
        assert!(!out.contains('/'), "path separator survived: {out:?}");
        assert!(!out.contains('\\'), "backslash survived: {out:?}");
        assert!(!out.contains(".."), "traversal survived: {out:?}");
        assert!(!out.contains('\0'), "nul survived: {out:?}");
        assert!(out.len() <= 70, "unbounded slug: {} chars", out.len());
        assert!(
            out.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "unexpected chars in {out:?}"
        );
    }

    assert!(!cogz::files::entities::sanitize_category(text).is_empty());
});
