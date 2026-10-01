//! Seed entities written by `cogz init` — the first-contact corpus.
//!
//! Three entries teach an agent how this repository's memory works:
//! a scoped ingestion protocol, the entry-authorship contract, and
//! the pack/lifecycle mechanics. Seeds use deterministic UUID v5 ids
//! so every init produces the same entities — content-hash sync
//! treats them as no-ops and `references` can point at them.
//!
//! Content branches on `local_only`: committed corpora warn that
//! entries are team-facing documentation; local corpora note the
//! audience is the user and their agents.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::files::{EntityFile, FileEntityType, FmValue, write_entity_file};

/// Namespace for seed entity UUIDs — distinct from code entities so
/// seed ids never collide with derived code ids.
const SEED_NAMESPACE: Uuid = Uuid::from_bytes([
    0x9c, 0x0e, 0xd4, 0x71, 0x2f, 0x8a, 0x4e, 0x91, 0xa7, 0x62, 0x1b, 0x3f, 0xc5, 0x88, 0x04, 0x6d,
]);

/// Tag marking entities written by `cogz init`. Lets users (and
/// future tooling) distinguish bootstrapped guidance from learned
/// corpus content.
pub const SEED_TAG: &str = "cogz-seed";

fn seed_id(name: &str) -> String {
    Uuid::new_v5(&SEED_NAMESPACE, name.as_bytes()).to_string()
}

/// Sharing paragraph that differs between committed and local-only
/// corpora.
fn sharing_note(local_only: bool) -> &'static str {
    if local_only {
        "This corpus is local-only (`.cogz/` is fully gitignored): entries are personal \
         workspace memory for you and your agents — still write as if permanent, because \
         observations and rules cannot be edited after the fact."
    } else {
        "This corpus is committed to git (knowledge/, rules/, observations/ are tracked; \
         only the DB is ignored): entries are team-facing documentation. Write as if a \
         teammate will read and rely on them."
    }
}

/// The sharing paragraph inside the mechanics knowledge entry.
fn mechanics_sharing_note(local_only: bool) -> &'static str {
    if local_only {
        "Entries stay on this machine — `.cogz/` is gitignored in local-only mode. \
         Nothing here is shared through git."
    } else {
        "Entries are committed to git and travel with the repo — every clone shares \
         this memory. The DB alone is local and disposable."
    }
}

struct Seed {
    name: &'static str,
    title: &'static str,
    entity_type: FileEntityType,
    category: &'static str,
    tags: &'static [&'static str],
}

const SEEDS: &[Seed] = &[
    Seed {
        name: "first-contact-ingestion-protocol",
        title: "First-contact ingestion protocol for a fresh CogZ corpus",
        entity_type: FileEntityType::Rule,
        category: "ingestion",
        tags: &["cogz-seed", "ingestion", "onboarding"],
    },
    Seed {
        name: "entry-authorship-contract",
        title: "Entry authorship contract: type discipline, append-only, references, titles",
        entity_type: FileEntityType::Rule,
        category: "ingestion",
        tags: &["cogz-seed", "authorship", "quality"],
    },
    Seed {
        name: "cogz-memory-mechanics",
        title: "How this repository's CogZ memory works",
        entity_type: FileEntityType::Knowledge,
        category: "meta",
        tags: &["cogz-seed", "memory", "mechanics"],
    },
];

fn ingestion_protocol_body() -> String {
    "\
When an agent encounters this repository with a young corpus, it may be asked to run a
**first-contact ingestion pass**. If so:

**Scope**
- Read `README*`, `docs/`, module doc comments, and public-API docblocks.
- Skip vendored, generated, lockfile, and test-fixture content.
- Bound the pass — top ~20 documentation files (README + architecture + core modules)
  first. Do not marathon-ingest the entire repo in one session.

**Granularity — one fact per entry**
- A rule states ONE constraint. A knowledge entry documents ONE decision, component,
  or gotcha. An observation records ONE event or experience.
- A document covering N topics produces N small entries linked by `references` —
  never one giant multi-topic entry. Conflated entries poison dedup, retrieval
  precision, and drift detection.

**Provenance**
- Attach `references` to code entity UUIDs when an entry describes code — that is
  what lets drift detection warn when the code later changes.
- `confidence` reflects provenance: ~0.8 for doc-stated facts, ~0.5 for facts
  inferred from code.

**Dedup**
- Run `search` before writing — judgment catches near-duplicates the dedup
  warning misses.

Ingestion creates committed or local corpus state — ask the user before running a
pass; never auto-ingest.
"
    .to_string()
}

fn authorship_contract_body(local_only: bool) -> String {
    format!(
        "\
**Pick the right type**
- `observation`: an event or experience (a flaky test hit, a surprising behavior, a bug found).
- `rule`: ONE imperative constraint (\"always X\", \"never Y\").
- `knowledge`: ONE durable fact, decision, or component description.
Wrong-type entries break promotion (observation→rule) and dedup semantics.

**Append-only means write as if permanent**
- Observations and rules cannot be edited after creation — a wrong entry is
  `reject`ed (with a reason) or superseded, not fixed in place.
- No session scratch, no \"TODO investigate\", no churn data. If it will be noise
  next month, don't write it.
- Never record secrets, API keys, or credentials in any entry.

**Titles are the retrieval surface**
- The title is both the dedup key and the FTS surface. Name the fact, not the
  topic: \"JWT refresh happens in middleware/auth.rs\", not \"notes on auth\".

**References wire the drift graph**
- Link `references` to code entity UUIDs for anything that describes code —
  an entry without references cannot warn when its code changes.

{}
",
        sharing_note(local_only)
    )
}

fn mechanics_body(local_only: bool) -> String {
    format!(
        "\
# How this repository's CogZ memory works

CogZ is a file-first memory layer: canonical entity files live under `.cogz/`
(markdown + YAML frontmatter); the SQLite DB is derived and rebuildable
(`cogz reset` + `cogz index` restores it).

**Context packs**
- `session_start` delivers a cold-start pack (top relevant knowledge/rules).
- `prompt_submit` delivers a query-scoped pack matching the prompt.
- `file_save` delivers rules scoped to the saved file plus drift notices.
- `get_context` (MCP) pulls a task-scoped pack on demand when the injected
  pack doesn't cover the work.

**Status lifecycle**
- `active`: current. `stale`: referenced code changed — verify before relying
  (`verify_knowledge` re-stamps). `superseded`: replaced by a newer entry.
  `rejected`: was wrong — re-adding a rejected claim triggers a
  `rejected_match` warning carrying the recorded reason. `pruned`: tombstone.
- Drift markers in packs (⚠ drift) mean verify-before-trust, not decoration.

**Writing**
- Prefer the MCP tools (`create_entity`, `reject_entity`, `update_knowledge`,
  `verify_knowledge`) over hand-editing files — tools record events and keep
  file↔DB invariants. Hand edits are legal but lose the event trail.
- `suggest_observations` surfaces mined candidates; `consolidate` runs
  dedup/contradiction/promotion maintenance.

**Seeds**
- Entries tagged `cogz-seed` were written by `cogz init` — bootstrapped
  guidance, not learned knowledge. Delete or supersede freely once the real
  corpus outgrows them.

{}
",
        mechanics_sharing_note(local_only)
    )
}

fn seed_entity(seed: &Seed, local_only: bool) -> EntityFile {
    let body = match seed.name {
        "first-contact-ingestion-protocol" => ingestion_protocol_body(),
        "entry-authorship-contract" => authorship_contract_body(local_only),
        _ => mechanics_body(local_only),
    };
    let mut entity = EntityFile::new(seed.title, seed.entity_type, &body);
    entity.id = seed_id(seed.name);
    entity
        .frontmatter
        .insert("category", FmValue::String(seed.category.to_string()));
    entity.frontmatter.insert(
        "tags",
        FmValue::Array(seed.tags.iter().map(|t| t.to_string()).collect()),
    );
    if seed.entity_type == FileEntityType::Rule {
        entity.frontmatter.insert("confidence", FmValue::Float(1.0));
    }
    entity
}

/// Write the seed entity files under `.cogz/`. Returns the paths
/// written. Files are canonical — the DB picks them up on the next
/// index/sync; no DB write happens here.
pub fn write_seeds(cogz_dir: &Path, local_only: bool) -> Result<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(SEEDS.len());
    for seed in SEEDS {
        let entity = seed_entity(seed, local_only);
        let path = entity.file_path(cogz_dir);
        write_entity_file(&path, &entity)
            .with_context(|| format!("failed to write seed {}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::entities::read_entity_file;
    use std::fs;

    #[test]
    fn seeds_write_three_parseable_files() {
        let dir = tempfile::tempdir().unwrap();
        let cogz = dir.path().join(".cogz");
        fs::create_dir_all(&cogz).unwrap();

        let paths = write_seeds(&cogz, false).unwrap();
        assert_eq!(paths.len(), 3);

        for path in &paths {
            let entity = read_entity_file(path).unwrap();
            assert_eq!(entity.status, "active");
            let tags = entity
                .frontmatter
                .get("tags")
                .and_then(|v| v.as_array())
                .unwrap();
            assert!(tags.iter().any(|t| t == SEED_TAG));
        }
    }

    #[test]
    fn seed_ids_are_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let cogz = dir.path().join(".cogz");
        fs::create_dir_all(&cogz).unwrap();

        let first = write_seeds(&cogz, false).unwrap();
        let ids1: Vec<String> = first
            .iter()
            .map(|p| read_entity_file(p).unwrap().id)
            .collect();
        let second = write_seeds(&cogz, false).unwrap();
        let ids2: Vec<String> = second
            .iter()
            .map(|p| read_entity_file(p).unwrap().id)
            .collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn local_only_changes_sharing_language() {
        let dir = tempfile::tempdir().unwrap();
        let cogz = dir.path().join(".cogz");
        fs::create_dir_all(&cogz).unwrap();

        let contract_path = |prefix: &str| {
            fs::read_dir(cogz.join("rules"))
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(prefix))
                })
                .unwrap()
        };

        write_seeds(&cogz, false).unwrap();
        let team = read_entity_file(&contract_path("entry-authorship-contract")).unwrap();

        write_seeds(&cogz, true).unwrap();
        let local = read_entity_file(&contract_path("entry-authorship-contract")).unwrap();

        assert!(team.body.contains("committed to git"));
        assert!(!team.body.contains("local-only"));
        assert!(local.body.contains("local-only"));
        assert!(!local.body.contains("committed to git"));
    }
}
