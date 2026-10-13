use super::*;

#[test]
fn sanitize_category_strips_path_traversal() {
    assert_eq!(sanitize_category("../../etc/passwd"), "etc-passwd");
    assert_eq!(sanitize_category("/absolute/path"), "absolute-path");
    assert_eq!(sanitize_category(".."), "uncategorized");
    assert_eq!(sanitize_category("normal"), "normal");
    assert_eq!(sanitize_category("with spaces"), "with-spaces");
    assert_eq!(sanitize_category(""), "uncategorized");
}

#[test]
fn knowledge_file_path_stays_within_cogz_dir() {
    let mut ef = EntityFile::new("Test Title", FileEntityType::Knowledge, "body");
    ef.frontmatter
        .insert("category", FmValue::String("../../../outside".to_string()));
    let cogz_dir = Path::new("/tmp/cogz");
    let path = ef.file_path(cogz_dir);
    // Path must stay within cogz_dir/knowledge/
    assert!(path.starts_with(cogz_dir.join("knowledge")));
    // No path traversal components
    assert!(
        !path
            .components()
            .any(|c| { matches!(c, std::path::Component::ParentDir) })
    );
}

#[test]
fn sanitize_category_truncates_long_input() {
    let long = "a".repeat(300);
    let out = sanitize_category(&long);
    assert!(out.len() <= MAX_SLUG_LEN, "got {} chars", out.len());
}

#[test]
fn volatility_frontmatter_validated_on_parse() {
    let base = |vol: &str| {
        format!(
            "---\nid: 11111111-1111-4111-8111-111111111111\ntitle: t\n\
             type: knowledge\nstatus: active\n\
             created_at: \"2026-10-12T00:00:00Z\"\nupdated_at: \"2026-10-12T00:00:00Z\"\n\
             references: []\nvolatility: {vol}\n---\n\nbody"
        )
    };
    assert!(EntityFile::from_content(&base("living")).is_ok());
    assert!(EntityFile::from_content(&base("record")).is_ok());
    let err = EntityFile::from_content(&base("bogus")).unwrap_err();
    assert!(format!("{err:?}").contains("volatility"));
}
