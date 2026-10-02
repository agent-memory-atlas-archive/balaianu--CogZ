#![no_main]

use libfuzzer_sys::fuzz_target;

// Frontmatter is a hand-rolled YAML-subset parser — the most
// bug-prone pure surface in the crate. Invariant: serialize is a
// fixpoint — parse(serialize(x)) must reparse and serialize to the
// identical text. This catches drift without tripping on NaN ≠ NaN.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok((fm_text, _body)) = cogz::files::frontmatter::split_frontmatter(text)
        && let Ok(fm) = cogz::files::frontmatter::parse(&fm_text)
    {
        let s1 = cogz::files::frontmatter::serialize(&fm);
        let fm2 = cogz::files::frontmatter::parse(&s1).expect("serialize output must reparse");
        let s2 = cogz::files::frontmatter::serialize(&fm2);
        assert_eq!(s1, s2, "serialize not idempotent");
    }

    let _ = cogz::files::frontmatter::parse(text);
});
