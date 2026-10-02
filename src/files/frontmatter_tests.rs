use super::*;

#[test]
fn roundtrip_preserves_control_chars() {
    // Trailing \r/\t and embedded \n must survive serialize→parse;
    // they force quoting + escaping.
    let mut fm = Frontmatter::new();
    fm.insert("cr", FmValue::String("a\0b\r".to_string()));
    fm.insert("nl", FmValue::String("line1\nline2".to_string()));
    fm.insert("tab", FmValue::String("x\ty".to_string()));

    let s = serialize(&fm);
    let fm2 = parse(&s).expect("serialized output must reparse");
    assert_eq!(fm2.entries, fm.entries, "serialize→parse diverged:\n{s}");
}

#[test]
fn serialize_is_idempotent() {
    let mut fm = Frontmatter::new();
    fm.insert("f", FmValue::Float(1.0));
    fm.insert("s", FmValue::String("true".to_string()));
    fm.insert(
        "arr",
        FmValue::Array(vec!["a,b".to_string(), "c".to_string()]),
    );

    let s1 = serialize(&fm);
    let s2 = serialize(&parse(&s1).unwrap());
    assert_eq!(s1, s2);
}

#[test]
fn whole_float_serializes_with_decimal_point() {
    // `1.0` must not serialize as `1` — that would reparse as Int.
    let mut fm = Frontmatter::new();
    fm.insert("v", FmValue::Float(1.0));
    let s = serialize(&fm);
    assert!(s.contains("1.0"), "got {s:?}");
    match &parse(&s).unwrap().entries[0].1 {
        FmValue::Float(f) => assert_eq!(*f, 1.0),
        other => panic!("expected Float, got {other:?}"),
    }
}
