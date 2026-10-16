//! Tests for the NLI model wrapper.

use super::*;

#[test]
fn label_map_default_is_deberta_order() {
    let map = LabelMap::DEFAULT;
    assert_eq!(map.contradiction, 0);
    assert_eq!(map.entailment, 1);
    assert_eq!(map.neutral, 2);
}

#[test]
fn label_map_parses_standard_config() {
    let json = r#"{"id2label": {"0": "contradiction", "1": "entailment", "2": "neutral"}}"#;
    let map = LabelMap::from_config_json(json).unwrap();
    assert_eq!(map.contradiction, 0);
    assert_eq!(map.entailment, 1);
    assert_eq!(map.neutral, 2);
}

#[test]
fn label_map_parses_alternate_order() {
    let json = r#"{"id2label": {"0": "entailment", "1": "neutral", "2": "contradiction"}}"#;
    let map = LabelMap::from_config_json(json).unwrap();
    assert_eq!(map.contradiction, 2);
    assert_eq!(map.entailment, 0);
    assert_eq!(map.neutral, 1);
}

#[test]
fn label_map_parses_label_prefixed_names() {
    let json = r#"{"id2label": {"0": "LABEL_0", "1": "LABEL_1", "2": "LABEL_2"}}"#;
    // Generic labels don't match — falls back to default.
    let map = LabelMap::from_config_json(json).unwrap();
    assert_eq!(map, LabelMap::DEFAULT);
}

#[test]
fn label_map_returns_none_for_invalid_json() {
    assert!(LabelMap::from_config_json("not json").is_none());
}

#[test]
fn label_map_returns_none_without_id2label() {
    assert!(LabelMap::from_config_json(r#"{"architectures": []}"#).is_none());
}
