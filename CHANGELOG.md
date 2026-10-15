# Changelog

All notable changes to this project are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

Independent audit remediation pass (pre-release hardening).

### Security

- Extended the secret scanner and made it cover serialized frontmatter,
  not just the body.
- Detect duplicate entity IDs during sync and report them in
  `cogz doctor` — the winning file is deterministic.
- `cogz reset --purge` is safe by default: dry-run first, lock guards,
  WAL checkpoint.
- `update_knowledge` and other write paths enforce MCP input limits.

### Correctness

- Migrations run in a single `BEGIN IMMEDIATE` transaction — a failed
  migration rolls back instead of leaving a partial schema.
- Fixed stdin UTF-8 handling on multi-byte boundaries and recovery of
  truncated hook payloads.
- Tombstone transition errors report the actual current status.
- Replaced `String` error payloads in `ConfigError`/`StorageError`
  with typed variants carrying field names, paths, and source errors.
- Permanent read failures no longer pin the incremental baseline.
- Repository resolution moved off the async runtime thread in MCP
  handlers.
- All float comparisons use total ordering.

### Performance

- `vec_rowid` map (schema v9) removes vec0 `entity_id` full scans on
  every embedding write/delete.
- Reference re-sync is scoped to changed files plus referrers of newly
  created entities; post-index maintenance on `.cogz/` saves is
  debounced.
- Index telemetry timestamps are normalized and indexed; cutoff
  filters are indexable.
- Telemetry sweeps opportunistically (at most once per 24h) instead of
  only at `session_end`; `--days` clamps to the retention window.
- Indexed file size capped via `index.max_file_bytes` (default 1 MiB).
- Co-change mining streams commits in batches instead of buffering
  them all; edge sync consumes parse output instead of re-reading
  sources.
- Background workers are gated to one per repo via `.cogz/.bg.lock`;
  the canonical file lock is bounded.

### Operations

- `cogz configure` preserves user config: format-preserving `toml_edit`
  merges, atomic writes, timestamped backups.
- ORT stderr is captured to a tempfile and replayed on init failure.
- Co-change mining uses `git2` — the `git` binary is no longer needed
  at runtime.
- Library code no longer writes to stdout; hook output is returned and
  printed by the CLI.
- Unknown config keys warn instead of silently passing.
- Measured hook latency documented: `session_start` ~0.4 s,
  `prompt_submit` ~2.7 s FTS-only / ~6 s with models (~790 MB peak).

## [0.5.8] - 2026-10-05

- MCP registry publish automated via GitHub OIDC in the release
  workflow; fixed the OCI package shape.
- Dependency bumps: sha2 0.11, tokenizers 0.23, rmcp 3.5.1.

## [0.5.7] - 2026-10-05

- MCP registry publish assets: `server.json`, publish script, GHCR
  release tags, `glama.json`.
- Reworked the benchmark suite to public pinned corpora and new
  phases.
- Docker build stage pinned to bookworm; `g++` installed for the
  esaxx-rs C++ dependency.

## [0.5.6] - 2026-10-02

- Serialized real-repo index setup in MCP stdio tests.
- Dropped `tokenizers` default features (progressbar) — removes
  indicatif/number_prefix from the tree.

## [0.5.5] - 2026-10-02

- Adapted `walk_descendants` to tree-sitter 0.26 `u32` child indexing.
- Renamed deprecated rmcp `ServerInfo`/`ClientInfo` to
  `ServerConfig`/`ClientConfig`.
- `checks:write` granted to the audit job; external-PR verification
  policy added to AGENTS.md.

## [0.5.4] - 2026-10-02

- OpenSSF Scorecard workflow + badge, CodeQL analysis, `cargo audit`
  in CI.
- `cargo-fuzz` suite over parser and path-safety boundaries.
- Fixed frontmatter roundtrip drift and unbounded category slugs.
- git2 0.21 and rustls 0.23.45 for RUSTSEC advisories.

## [0.5.3] - 2026-10-02

- Measured agent outcomes and honest limits added to the README
  benchmarks.

## [0.5.2] - 2026-10-01

- Telemetry timestamps compare as instants across write formats.
- `hot_file` mining requires cross-day recurrence.
- Doc fact-check sweep (schema v7, silence-gate FTS escape, module
  map); benchmark results refreshed.

## [0.5.1] - 2026-10-01

- `cogz index` prints "already up to date" on a no-op.
- Hot-file mining filtered to indexed files — doc churn no longer
  produces noise nudges.
- Fixed the silence gate discarding literal-title FTS hits.

## [0.5.0] - 2026-10-01

- Windows build fixed via `GetFileInformationByHandle` file identity.
- Seed first-contact corpus on `init`; young-corpus ingestion nudge.
- Hook payloads fixed for Cursor, Copilot, and Gemini.
- Audit fixes: Windows path separators, sync-boundary secret scan,
  panic-safe context packs, telemetry retention.

[Unreleased]: https://github.com/balaianu/CogZ/compare/v0.5.8...HEAD
[0.5.8]: https://github.com/balaianu/CogZ/releases/tag/v0.5.8
[0.5.7]: https://github.com/balaianu/CogZ/releases/tag/v0.5.7
[0.5.6]: https://github.com/balaianu/CogZ/releases/tag/v0.5.6
[0.5.5]: https://github.com/balaianu/CogZ/releases/tag/v0.5.5
[0.5.4]: https://github.com/balaianu/CogZ/releases/tag/v0.5.4
[0.5.3]: https://github.com/balaianu/CogZ/releases/tag/v0.5.3
[0.5.2]: https://github.com/balaianu/CogZ/releases/tag/v0.5.2
[0.5.1]: https://github.com/balaianu/CogZ/releases/tag/v0.5.1
[0.5.0]: https://github.com/balaianu/CogZ/releases/tag/v0.5.0
