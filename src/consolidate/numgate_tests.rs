use super::*;

#[test]
fn same_slot_different_value_is_contradiction() {
    for (a, b) in [
        (
            "Observations older than 90 days are prunable",
            "Observations older than 30 days are prunable",
        ),
        ("The dedup threshold is 0.85", "The dedup threshold is 0.5"),
        (
            "Search returns at most 20 results",
            "Search returns at most 50 results",
        ),
    ] {
        assert_eq!(
            decide(a, b),
            Some(GateDecision::Contradiction),
            "expected contradiction: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn equivalent_values_across_units_are_not_contradictions() {
    for (a, b) in [
        (
            "The idle TTL is 300 seconds for models",
            "The idle TTL is 5 minutes for models",
        ),
        (
            "Free memory must be at least 512 mb",
            "Free memory must be at least 512 MB",
        ),
    ] {
        assert_ne!(
            decide(a, b),
            Some(GateDecision::Contradiction),
            "must not flag: {a:?} vs {b:?}"
        );
    }
}

#[test]
fn version_strings_are_not_quantities() {
    // "v2"/"v1.5" style labels must never be treated as numeric slots.
    let pairs = [
        ("migrate to schema v2", "migrate to schema v4"),
        ("uses tokenizer v1.5", "uses tokenizer v2.0"),
    ];
    for (a, b) in pairs {
        assert_eq!(decide(a, b), None, "deferred: {a:?} vs {b:?}");
    }
}

#[test]
fn unrelated_numeric_mentions_defer() {
    for (a, b) in [
        (
            "The graph walks at most 4 hops from the seed",
            "The tokenizer uses BPE with 50000 merges",
        ),
        (
            "Observations prune after 90 days",
            "The tokenizer vocabulary holds 50000 entries",
        ),
    ] {
        assert_eq!(decide(a, b), None, "must defer to NLI: {a:?} vs {b:?}");
    }
}

#[test]
fn missing_numbers_defer() {
    assert_eq!(decide("file first, db derived", "db first canonical"), None);
    assert_eq!(decide("has 3 hops", "no numbers here"), None);
}

#[test]
fn percentage_normalization() {
    assert_eq!(
        decide(
            "silence threshold is 5 percent for queries",
            "silence threshold is 5% for queries"
        ),
        Some(GateDecision::Equivalent)
    );
}
