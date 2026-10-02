#![no_main]

use libfuzzer_sys::fuzz_target;

// Full entity-file parse (frontmatter + schema validation + body)
// over arbitrary input — this is what every .cogz write produces and
// every index consumes. Roundtrip on success catches drift between
// serializer and parser.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(entity) = cogz::files::entities::EntityFile::from_content(text) {
        let rewritten = entity.to_file_content();
        let reparsed = cogz::files::entities::EntityFile::from_content(&rewritten)
            .expect("to_file_content output must reparse");
        assert_eq!(entity.title, reparsed.title);
        assert_eq!(entity.body, reparsed.body);
    }
});
