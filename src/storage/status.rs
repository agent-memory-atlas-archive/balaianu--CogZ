//! Status state machine — validates all entity status transitions.
//!
//! Legal transitions (from `docs/design/entity-model.md`):
//!
//! ```text
//! active    → stale | rejected | superseded
//! stale     → active | rejected | superseded
//! rejected  → pruned
//! superseded→ pruned
//! pruned    → (terminal)
//! ```

use super::StorageError;

/// Validate a status transition. Returns `Ok(())` if legal, `Err` if not.
pub fn transition_status(current: &str, next: &str) -> Result<(), StorageError> {
    let allowed: &[&str] = match current {
        "active" => &["stale", "rejected", "superseded"],
        // stale is a suspicion state, not terminal — a drifted entity can
        // later be superseded by a merge or rejected on review. Forbidding
        // those paths traps a file-written `superseded`/`rejected` status
        // as a permanent file↔DB divergence that errors on every sync.
        "stale" => &["active", "rejected", "superseded"],
        "rejected" => &["pruned"],
        "superseded" => &["pruned"],
        "pruned" => &[],
        _ => {
            return Err(StorageError::IllegalTransition {
                from: current.to_string(),
                to: next.to_string(),
            });
        }
    };

    if !allowed.contains(&next) {
        return Err(StorageError::IllegalTransition {
            from: current.to_string(),
            to: next.to_string(),
        });
    }

    Ok(())
}

/// All valid entity statuses.
pub const VALID_STATUSES: &[&str] = &["active", "stale", "superseded", "rejected", "pruned"];

/// Check if a status string is valid.
pub fn is_valid_status(status: &str) -> bool {
    VALID_STATUSES.contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The full 5x5 lattice — every status pair, plus an unknown source.
    #[test]
    fn transition_lattice() {
        let cases: &[(&str, &str, bool)] = &[
            ("active", "stale", true),
            ("active", "rejected", true),
            ("active", "superseded", true),
            ("active", "active", false),
            ("active", "pruned", false),
            ("stale", "active", true),
            ("stale", "rejected", true),
            ("stale", "superseded", true),
            ("stale", "stale", false),
            ("stale", "pruned", false),
            ("rejected", "pruned", true),
            ("rejected", "active", false),
            ("rejected", "stale", false),
            ("rejected", "superseded", false),
            ("rejected", "rejected", false),
            ("superseded", "pruned", true),
            ("superseded", "active", false),
            ("superseded", "stale", false),
            ("superseded", "rejected", false),
            ("superseded", "superseded", false),
            ("pruned", "active", false),
            ("pruned", "stale", false),
            ("pruned", "rejected", false),
            ("pruned", "superseded", false),
            ("pruned", "pruned", false),
            ("unknown", "active", false),
        ];
        for &(from, to, ok) in cases {
            assert_eq!(transition_status(from, to).is_ok(), ok, "{from} -> {to}");
        }
    }

    #[test]
    fn is_valid_status_checks() {
        assert!(is_valid_status("active"));
        assert!(is_valid_status("pruned"));
        assert!(!is_valid_status("unknown"));
        assert!(!is_valid_status(""));
    }
}
