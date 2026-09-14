use super::*;
use std::collections::HashMap;

// ─── merge_channels (pure merge math) ────────────────────────────

#[test]
fn merge_channels_weak_channel_sinks() {
    let code = vec![("c1".to_string(), 1.0), ("c2".to_string(), 0.8)];
    let knowledge = vec![("k1".to_string(), 1.0), ("k2".to_string(), 0.6)];
    // Quota (0.5/0.5) ties the two top hits; absolute strength
    // (knowledge 0.9, code 0.15) must order all knowledge first.
    let (fused, _) = merge_channels(code, knowledge, 0.15, 0.9, 0.0);
    assert_eq!(fused[0].0, "k1");
    assert_eq!(fused[1].0, "k2");
    assert!(fused[2].1 > fused[3].1 || fused[2].0 == "c1");
}

#[test]
fn merge_channels_floor_filters_and_counts() {
    let code = vec![("c1".to_string(), 1.0), ("c2".to_string(), 0.5)];
    let knowledge = vec![("k1".to_string(), 1.0)];
    let (fused, filtered) = merge_channels(code, knowledge, 0.9, 0.08, 0.05);
    // k1 = 1.0 × 0.08 = 0.08 ≥ 0.05 survives; c1/c2 = 0.9/0.45 survive.
    assert_eq!(fused.len(), 3);
    assert_eq!(filtered, 0);
    let (fused, filtered) = merge_channels(
        vec![("c1".to_string(), 1.0)],
        vec![("k1".to_string(), 1.0)],
        0.9,
        0.04,
        0.05,
    );
    // k1 = 0.04 < 0.05 → filtered. Weak channel can vanish entirely.
    assert_eq!(fused.len(), 1);
    assert_eq!(filtered, 1);
    assert_eq!(fused[0].0, "c1");
}

#[test]
fn merge_channels_zero_floor_disables() {
    let (fused, filtered) = merge_channels(
        vec![("c1".to_string(), 1.0)],
        vec![("k1".to_string(), 1.0)],
        0.9,
        0.01,
        0.0,
    );
    assert_eq!(fused.len(), 2);
    assert_eq!(filtered, 0);
}

// ─── apply_diversity_slots ───────────────────────────────────────

#[test]
fn diversity_slot_promotes_minority_into_window() {
    // Minority channel (knowledge, share 0.35 ≥ 0.3) absent from the
    // top-5 window → its best item is promoted to position 4.
    let mut fused: Vec<(String, f64, Chan)> = (0..6)
        .map(|i| (format!("c{i}"), 1.0 - i as f64 * 0.1, Chan::Code))
        .collect();
    fused.push(("k1".to_string(), 0.3, Chan::Knowledge));
    apply_diversity_slots(&mut fused, 0.65, 0.35, 0.3, 20);
    assert_eq!(fused[4].0, "k1");
    assert!(fused[..5].iter().any(|(_, _, c)| *c == Chan::Knowledge));
}

#[test]
fn diversity_slot_skips_when_minority_too_weak() {
    // Share 0.2 < 0.3 → no guarantee; the channel must earn the slot.
    let mut fused: Vec<(String, f64, Chan)> = (0..6)
        .map(|i| (format!("c{i}"), 1.0 - i as f64 * 0.1, Chan::Code))
        .collect();
    fused.push(("k1".to_string(), 0.3, Chan::Knowledge));
    apply_diversity_slots(&mut fused, 0.8, 0.2, 0.3, 20);
    assert_eq!(fused[5].0, "c5");
    assert_eq!(fused[6].0, "k1");
}

#[test]
fn diversity_slot_noop_when_channel_present_or_disabled() {
    let mut fused: Vec<(String, f64, Chan)> = vec![
        ("k1".to_string(), 0.9, Chan::Knowledge),
        ("c1".to_string(), 0.8, Chan::Code),
    ];
    apply_diversity_slots(&mut fused, 0.6, 0.4, 0.3, 20);
    assert_eq!(fused[0].0, "k1");
    // Disabled (share 0) leaves order untouched even with an absent channel.
    let mut fused: Vec<(String, f64, Chan)> = (0..6)
        .map(|i| (format!("c{i}"), 1.0 - i as f64 * 0.1, Chan::Code))
        .collect();
    fused.push(("k1".to_string(), 0.3, Chan::Knowledge));
    apply_diversity_slots(&mut fused, 0.6, 0.4, 0.0, 20);
    assert_eq!(fused[4].0, "c4");
}

// ─── logistic ────────────────────────────────────────────────────

#[test]
fn logistic_midpoint_and_asymptotes() {
    assert!((logistic(0.32, 0.32, 0.04) - 0.5).abs() < 1e-9);
    assert!(logistic(0.45, 0.32, 0.04) > 0.95);
    assert!(logistic(0.2, 0.32, 0.04) < 0.05);
}

// ─── mmr_rerank / cosine_similarity ──────────────────────────────

#[test]
fn cosine_similarity_basic() {
    let a = vec![1.0_f32, 0.0, 0.0];
    let b = vec![1.0_f32, 0.0, 0.0];
    let c = vec![0.0_f32, 1.0, 0.0];
    assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    assert!(cosine_similarity(&a, &c).abs() < 1e-6);
    // Zero vector yields 0, not NaN.
    let z = vec![0.0_f32; 3];
    assert_eq!(cosine_similarity(&z, &a), 0.0);
}

#[test]
fn mmr_rerank_diversifies_within_channel() {
    // Two near-duplicate knowledge items + one diverse knowledge item.
    // With lambda < 1 the diverse item should outrank the duplicate
    // despite lower relevance.
    let mut emb = HashMap::new();
    emb.insert("dup1".to_string(), vec![1.0_f32, 0.0]);
    emb.insert("dup2".to_string(), vec![0.99_f32, 0.01]);
    emb.insert("div".to_string(), vec![0.0_f32, 1.0]);
    let candidates = vec![
        ("dup1".to_string(), 0.9_f64, Chan::Knowledge),
        ("dup2".to_string(), 0.85_f64, Chan::Knowledge),
        ("div".to_string(), 0.8_f64, Chan::Knowledge),
    ];
    let ranked = mmr_rerank(candidates, &emb, 0.5);
    assert_eq!(ranked[0].0, "dup1"); // highest relevance seeds first
    assert_eq!(ranked[1].0, "div"); // diversity beats the near-dup
    assert_eq!(ranked[2].0, "dup2");
}

#[test]
fn mmr_rerank_ignores_cross_channel_similarity() {
    // Identical vectors across channels are NOT duplicates — the
    // code item keeps its rank by relevance.
    let mut emb = HashMap::new();
    emb.insert("k1".to_string(), vec![1.0_f32, 0.0]);
    emb.insert("c1".to_string(), vec![1.0_f32, 0.0]);
    let candidates = vec![
        ("k1".to_string(), 0.9_f64, Chan::Knowledge),
        ("c1".to_string(), 0.8_f64, Chan::Code),
    ];
    let ranked = mmr_rerank(candidates, &emb, 0.5);
    assert_eq!(ranked[0].0, "k1");
    assert_eq!(ranked[1].0, "c1");
}

#[test]
fn mmr_rerank_missing_embeddings_pure_relevance() {
    let emb = HashMap::new();
    let candidates = vec![
        ("a".to_string(), 0.9_f64, Chan::Knowledge),
        ("b".to_string(), 0.8_f64, Chan::Knowledge),
    ];
    let ranked = mmr_rerank(candidates, &emb, 0.5);
    assert_eq!(ranked[0].0, "a");
    assert_eq!(ranked[1].0, "b");
}
