# CogZ

[![CI](https://github.com/balaianu/CogZ/actions/workflows/ci.yml/badge.svg)](https://github.com/balaianu/CogZ/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/github/v/release/balaianu/CogZ)](https://github.com/balaianu/CogZ/releases)
[![Buy Me A Coffee](https://img.shields.io/badge/☕-Buy%20Me%20A%20Coffee-yellow)](https://buymeacoffee.com/balaianu)

Local-first, code-aware engineering cognition for AI coding agents.

CogZ gives a coding agent persistent memory, contextual retrieval, and continuous cognition about a software repository — all running locally on your machine, no cloud services required.

Works with Claude Code, Cursor, Codex, Gemini CLI, GitHub Copilot, Devin, and any MCP-compatible agent.

## What it looks like

Real output from CogZ running on its own codebase:

```
$ cogz context --mode task "token budget estimation and context pack compression"

Context pack (mode: task)
Query: token budget estimation and context pack compression
Search mode: hybrid
Sections: 91
Token estimate: 8188

Dropped: 6 sections over token budget

---

## 1. [rule] New expansion channels: emit early, filter before seen-mark, sort deterministically, never displace directs (relevance: 0.5148)

Conventions proven across the sibling and co-change channels:

1. Emit before the generic expansion loops — candidates emitted
   later get claimed-and-floored by graph traversal …
2. Apply entity-type/test filters BEFORE `seen.insert` …
…

## 2. [rule] cfg-gated code must be typechecked per-target before release (relevance: 0.4690)

Code behind #[cfg(unix)]/cfg(target_os = ...) is invisible to host
builds, tests, and clippy — a compile error in a cfg'd branch ships
silently until a real target build sees it. The v0.5.0 Windows leg
failure is the canonical example.

## 3. [rule] Degradation must be loud, never silent (relevance: 0.3680)

Every degraded or failed code path must surface a signal …

## 4. [identity] CogZ (relevance: —)

Project: CogZ

## 5. [file] assemble.rs (relevance: 0.6993)

//! Context pack assembly — the tiered-push pipeline.
//! Tier 0 (baseline: identity + top rules) always ships for task and
//! escalation packs …

… 86 more sections …
```

That's not a text chunk from a vector search. The pack leads with validated rules — one learned from a release failure on this very project — plus the identity baseline and the actual source file, all ranked, traceable, and budgeted.

This repository already contains real dogfooding knowledge — CogZ has been used on its own codebase throughout development. You can clone it, install CogZ, and try the commands above against it directly.

## What it does

CogZ maintains a project-specific knowledge layer that connects what an agent learns to the code it is working with.

**Memory**

CogZ stores three kinds of project knowledge:

- **Observations** — things an agent has learned or noticed. Raw, unvalidated experience: bugs found, decisions made, patterns noticed.
- **Rules** — validated knowledge that should influence future work. Coding standards, design decisions, confirmed patterns.
- **Knowledge** — structured information about the codebase. Architecture explanations, module responsibilities, trade-off rationale.

These are stored as Markdown files with YAML frontmatter, linked to each other and to code entities in the repository. The files are the canonical source of truth — SQLite is a derived index, disposable and rebuildable. Your knowledge is portable, version-controlled, and editable by hand.

**Context**

Instead of giving an agent everything it knows, CogZ builds scoped context packs for the current situation. A context pack combines relevant rules, observations, knowledge, and code structures — ranked by relevance, traceable through the code graph, and limited by a token budget so the agent gets what matters for the task rather than the entire project history.

**Cognition**

CogZ periodically consolidates what has been learned: deduplicates entries, detects contradictions, promotes well-supported observations to rules, merges superseded entries, and flags knowledge as stale when the code it references changes.

## Quick start

**Linux / macOS / Windows (Git Bash):**
```bash
# Install
curl -fsSL https://raw.githubusercontent.com/balaianu/CogZ/master/install.sh | bash

# Initialize in a repo (add --configure auto to wire MCP + hooks for detected agents)
cd ~/your-project
cogz init

# Index (downloads models on first run, or use --no-download for FTS-only)
cogz index

# Verify it's working — entity counts, model status, DB stats
cogz status
```

**Windows (PowerShell):**
```powershell
# Install
irm https://raw.githubusercontent.com/balaianu/CogZ/master/install.ps1 | iex

# Initialize in a repo
cd your-project
cogz init
cogz index
```

See [Getting Started](docs/getting-started.md) for the mental model and a complete walkthrough.

## MCP integration

CogZ runs as a stateless MCP server over stdio. Every tool call specifies which repo it targets via a required `repo` parameter — no Roots, no session state, no fallbacks.

```json
{
  "mcpServers": {
    "cogz": {
      "command": "cogz",
      "args": ["mcp-stdio"]
    }
  }
}
```

The server exposes 15 tools: `create_entity`, `update_knowledge`, `verify_knowledge`, `reject_entity`, `query_entities`, `search`, `get_context`, `get_status`, `list_entities`, `consolidate`, `capture_event`, `get_callers`, `get_impact`, `find_orphans`, `suggest_observations`.

See [MCP Tools](docs/integration/mcp-tools.md) for full parameter reference and example responses. See [Agent Setup](docs/integration/agent-setup.md) for per-agent config files, hook formats, and verified capability notes for all six supported agents — or just run `cogz configure auto`.

## Hook integration

Hooks capture lifecycle events and inject context packs into agent sessions. CogZ's binary is the hook handler — no wrapper scripts needed.

```json
{
  "hooks": {
    "SessionStart": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "cogz capture-event session_start --hook-json",
        "timeout": 15
      }]
    }]
  }
}
```

See [Hooks](docs/integration/hooks.md) for all 7 event types and per-agent wiring guides.

## CLI commands

Normal operation is automatic: hooks fire on lifecycle events, the agent drives CogZ through MCP. The CLI is not needed for day-to-day use — it's available for setup, manual exploration, and automation if you want or need it.

| Command | Description |
|---|---|
| `cogz init` | Initialize `.cogz/` in a repository |
| `cogz configure <harnesses>` | Write agent MCP + hook config (`auto` detects installed agents) |
| `cogz index [--no-download]` | Sync files to DB + index source code |
| `cogz reindex` | Incremental reindex (changed files only) |
| `cogz search <query>` | Hybrid FTS + vector + graph search |
| `cogz context --mode <mode> [query]` | Assemble context pack |
| `cogz status` | DB stats, entity counts, model status |
| `cogz consolidate [--dry-run]` | Run promotion and merge |
| `cogz suggest [--days N]` | List mined observation candidates |
| `cogz verify <entity-id>` | Re-stamp a drifted entity's provenance |
| `cogz reject <entity-id>` | Mark an entity rejected (`--reason` stored) |
| `cogz capture-event <type>` | Capture lifecycle event from hooks |
| `cogz models <download\|list\|clean>` | Model management |
| `cogz doctor [--prune-observations]` | Health check, policy violations, usage metrics |
| `cogz update [--check]` | Self-update from GitHub releases |
| `cogz reset [--purge]` | Drop DB (optionally purge observations) |
| `cogz mcp-stdio` | Run MCP server over stdio |

See [CLI Reference](docs/cli-reference.md) for all flags and options.

## Requirements

### Minimum (FTS-only mode)

| Resource | Requirement |
|---|---|
| RAM | 256 MB free |
| Disk | 50 MB (binary + DB, no models) |
| CPU | any x86_64 or ARM64 |

Works without ONNX Runtime or model downloads. All hooks, FTS search, context packs, consolidation, doctor, and prune are functional. Vector search, embedding-based dedup, and contradiction detection are not available.

### Recommended (hybrid search mode)

| Resource | Requirement |
|---|---|
| RAM | 2 GB free |
| Disk | 550 MB (binary + ONNX Runtime + 3 models + DB) |
| CPU | any x86_64 or ARM64, 4+ cores speeds up batch embedding |

Full functionality including vector search, semantic dedup, and NLI contradiction detection. Models auto-download on first use and auto-unload after 5 min idle (RAM drops back to ~11 MB). See [Evaluations](docs/evaluations/) for the full resource consumption profile.

## Benchmarks

CogZ ships a self-contained retrieval benchmark (`benchmark/`) run against this repository's own `.cogz` corpus and source code — 78 labeled queries scored for P@5, MRR, and recall@20:

| Variant | MRR | Recall@20 |
|---|---|---|
| Hybrid (FTS + vector + graph) | 0.298 | 0.653 |
| FTS-only (degraded mode) | 0.184 | 0.600 |
| Hybrid, no graph expansion | 0.298 | 0.509 |

The vector channel lifts MRR ~60% over FTS alone; graph expansion adds +0.14 recall@20. Context packs reach 0.67 expected-entity recall at ~8K average tokens. Methodology, per-intent breakdowns, and the tuning sweep history are in [benchmark/README.md](benchmark/README.md).

## Architecture

- **Single Rust binary** — no runtime dependencies except optional ONNX models for vector search.
- **Files are canonical** — all entities are Markdown files. The SQLite DB is a derived index, disposable and rebuildable.
- **Code-aware** — tree-sitter indexes source code as first-class graph entities. Supported languages: Rust, Python, Go, JavaScript, TypeScript, TSX, Bash.
- **Graceful degradation** — works without ML models in FTS-only mode.
- **Local-first** — no cloud, no telemetry, no accounts. The only network access is optional model downloads.

See [Architecture](docs/design/architecture.md) for the full system design.

## Compatibility

| Platform | Support | Embeddings | FTS-only | Install |
|---|---|---|---|---|
| Linux x86_64 | Full | Auto-download | Yes | `install.sh` |
| Linux aarch64 | Full | Auto-download | Yes | `install.sh` |
| macOS arm64 (Apple Silicon) | Full | Auto-download | Yes | `install.sh` |
| macOS x86_64 (Intel) | Not supported | — | — | — |
| Windows x86_64 | Full | Auto-download | Yes | `install.ps1` or `install.sh` (Git Bash) |

**macOS Intel** is not supported because Microsoft dropped ONNX Runtime macOS Intel binaries after v1.22. Intel Mac users can run the arm64 binary under Rosetta 2 (with a compatible ORT build) or use `cargo install cogz` for FTS-only mode.

**Windows 10+** is required (bsdtar is bundled since build 17063, needed for ONNX Runtime auto-extraction).

Cross-platform team collaboration is supported: code entity UUIDs use forward-slash path normalization so the same source file produces the same entity ID on all platforms.

## Documentation

**User guides:**
- [Getting Started](docs/getting-started.md) — mental model and walkthrough
- [Configuration](docs/configuration.md) — full `config.toml` reference
- [CLI Reference](docs/cli-reference.md) — every command and flag

**Integration:**
- [MCP Tools](docs/integration/mcp-tools.md) — all 15 tool signatures and response shapes
- [Hooks](docs/integration/hooks.md) — lifecycle events and output format
- [Agent Setup](docs/integration/agent-setup.md) — all six agents + generic MCP, with per-agent effect coverage

**Design:**
- [Architecture](docs/design/architecture.md) — system overview and module map
- [Entity Model](docs/design/entity-model.md) — entity types, frontmatter, state machine
- [Search](docs/design/search.md) — hybrid FTS + vector, RRF, graph expansion
- [Consolidation](docs/design/consolidation.md) — dedup, contradiction, promotion, merge
- [Degradation](docs/design/degradation.md) — FTS-only mode and fallback behavior

**Contributing:**
- [Building](docs/dev/building.md) — build, release, cross-compile
- [Testing](docs/dev/testing.md) — test categories and mock models
- [Conventions](docs/dev/conventions.md) — code patterns and invariants
- [Dependencies](docs/dev/dependencies.md) — pinned versions and supply-chain policy
- [Schema](docs/dev/schema.md) — DB schema and migrations

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build, test, and PR guidelines.

## License

MIT — see [LICENSE](LICENSE).

## Support

If you find this tool useful, consider buying me a coffee:

[![Buy Me A Coffee](https://img.shields.io/badge/☕-Buy%20Me%20A%20Coffee-yellow)](https://buymeacoffee.com/balaianu)
