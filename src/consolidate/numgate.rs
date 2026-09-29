//! Deterministic numeric-slot gate for contradiction detection.
//!
//! NLI models are weak at numeric comparisons ("90 days" vs "30 days").
//! When both texts place numbers in the same definitional slot, the
//! comparison is decided symbolically instead of by the neural model.
//! The gate is deliberately conservative: anything ambiguous defers
//! to NLI. Calibrated on `benchmark/nli_compare` — 3/3 precision,
//! ~2% coverage. It complements NLI; it does not replace it.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

static NUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Boundary excludes '.' so version tails like the ".5" in "v1.5"
    // can't leak through as bare numbers; a real "0.85" still matches
    // at its leading digit.
    Regex::new(r"(?:^|[^A-Za-z0-9.])(\d+(?:\.\d+)?)\s*(seconds?|secs?|minutes?|mins?|hours?|days?|months?|years?|ms|milliseconds?|kb|mb|gb|bytes?|%|percent|hops?|tokens?|chars?|queries|files?|tasks?|terms?|docs?|seeds?)?").unwrap()
});
static TOK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z]+").unwrap());

const STOP: &[&str] = &[
    "the", "a", "an", "is", "are", "to", "of", "in", "on", "by", "for", "with", "and", "or",
    "that", "this", "it", "its", "as", "be", "can", "will", "must", "should", "not", "no", "only",
    "each", "every", "all", "any", "at", "from", "after", "before", "when", "if", "then", "than",
    "so", "such",
];

/// Canonical numeric value plus its unit class for cross-unit equality.
/// `None` class means a bare number with no recognized unit.
#[derive(Debug, Clone)]
struct Num {
    value: f64,
    class: Option<String>,
}

fn canon(value: f64, unit: Option<&str>) -> Num {
    let u = unit.map(|u| u.to_lowercase()).unwrap_or_default();
    let u = u.strip_suffix('s').unwrap_or(&u);
    let time = [
        ("second", 1.0),
        ("sec", 1.0),
        ("m", 0.001),
        ("millisecond", 0.001),
        ("minute", 60.0),
        ("min", 60.0),
        ("hour", 3600.0),
        ("day", 86400.0),
        ("month", 2_592_000.0),
        ("year", 31_536_000.0),
    ];
    let size = [
        ("byte", 1.0),
        ("kb", 1024.0),
        ("mb", 1_048_576.0),
        ("gb", 1_073_741_824.0),
    ];
    if u == "%" || u == "percent" {
        return Num {
            value,
            class: Some("%".to_string()),
        };
    }
    for (name, mult) in time {
        if u == name {
            return Num {
                value: value * mult,
                class: Some("time".to_string()),
            };
        }
    }
    for (name, mult) in size {
        if u == name {
            return Num {
                value: value * mult,
                class: Some("size".to_string()),
            };
        }
    }
    Num {
        value,
        class: match u {
            "" => None,
            _ => Some(u.to_string()),
        },
    }
}

/// A numeric literal plus the content tokens within ±45 chars of it —
/// the "slot" the number fills. Slot similarity decides whether two
/// numbers on opposite sides play the same definitional role.
struct Slot {
    num: Num,
    context: HashSet<String>,
}

fn nums_and_slots(text: &str) -> Vec<Slot> {
    let words: Vec<(usize, usize, String)> = TOK_RE
        .find_iter(&text.to_lowercase())
        .map(|m| (m.start(), m.end(), m.as_str().to_string()))
        .collect();

    NUM_RE
        .captures_iter(text)
        .filter_map(|cap| {
            let m = cap.get(1)?;
            let value: f64 = m.as_str().parse().unwrap_or(0.0);
            let num = canon(value, cap.get(2).map(|u| u.as_str()));
            let mut context = HashSet::new();
            for (ws, we, w) in &words {
                let before = *we <= m.start() && m.start() - we < 45;
                let after = *ws >= m.end() && ws - m.end() < 45;
                if before || after {
                    context.insert(w.clone());
                }
            }
            let context: HashSet<String> = context
                .into_iter()
                .filter(|t| !STOP.contains(&t.as_str()) && t.len() > 2)
                .collect();
            Some(Slot { num, context })
        })
        .collect()
}

/// Gate decision for a candidate pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Same definitional slot, different canonical value.
    Contradiction,
    /// Same definitional slot, equal canonical value — not a conflict.
    Equivalent,
}

/// Decide a pair deterministically when both texts carry numbers in the
/// same slot. Returns `None` when numbers are absent, play different
/// roles, or the context match is too weak — the pair then falls
/// through to NLI.
pub fn decide(a: &str, b: &str) -> Option<GateDecision> {
    let sa = nums_and_slots(a);
    let sb = nums_and_slots(b);
    if sa.is_empty() || sb.is_empty() {
        return None;
    }

    let mut best_c = 0.0f64;
    let mut best_e = 0.0f64;
    let mut typed: Vec<(f64, f64)> = Vec::new();
    for x in &sa {
        for y in &sb {
            if x.num.class == y.num.class
                && matches!(x.num.class.as_deref(), Some("time" | "size" | "%"))
            {
                typed.push((x.num.value, y.num.value));
            }
            if x.context.is_empty() || y.context.is_empty() || x.num.class != y.num.class {
                continue;
            }
            let inter = x.context.intersection(&y.context).count() as f64;
            let union = x.context.union(&y.context).count() as f64;
            let jac = inter / union;
            if jac >= 0.65 {
                if (x.num.value - y.num.value).abs() > f64::EPSILON {
                    best_c = best_c.max(jac);
                } else {
                    best_e = best_e.max(jac);
                }
            }
        }
    }

    if best_c >= 0.65 {
        return Some(GateDecision::Contradiction);
    }
    if best_e >= 0.6 {
        return Some(GateDecision::Equivalent);
    }
    if typed.is_empty() {
        return None;
    }

    // Fallback: same unit class on both sides plus moderate whole-text
    // overlap — catches typed quantities whose slots are too sparse for
    // the jaccard check.
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let ta: HashSet<&str> = TOK_RE
        .find_iter(&a_lower)
        .map(|m| m.as_str())
        .filter(|t| !STOP.contains(t) && t.len() > 2)
        .collect();
    let tb: HashSet<&str> = TOK_RE
        .find_iter(&b_lower)
        .map(|m| m.as_str())
        .filter(|t| !STOP.contains(t) && t.len() > 2)
        .collect();
    let tjac = ta.intersection(&tb).count() as f64 / ta.union(&tb).count().max(1) as f64;
    if tjac >= 0.4 {
        if typed.iter().any(|(x, y)| (x - y).abs() > f64::EPSILON) {
            return Some(GateDecision::Contradiction);
        }
        return Some(GateDecision::Equivalent);
    }
    None
}

#[cfg(test)]
#[path = "numgate_tests.rs"]
mod tests;
