#!/usr/bin/env bash
# Docs drift guard: fails when dependency versions in
# docs/dev/dependencies.md disagree with Cargo.toml, or when the
# documented MCP tool count disagrees with the #[tool] annotations.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
say() { printf '%s\n' "$*"; }

# ── Dependency versions ─────────────────────────────────────────
# Every `[dep] "x"` / `[dep] { version = "x" ... }` line in
# Cargo.toml must appear as `dep` | `x` (or `=x` for exact pins)
# in dependencies.md.
while IFS=$'\t' read -r name ver; do
    [ -z "$name" ] && continue
    # Strip the `=` exact-pin marker for the doc form check.
    bare="${ver#=}"
    if grep -qE "\| *\`${name//./\\.}\` *\| *\`${ver//./\\.}\`" docs/dev/dependencies.md \
        || grep -qE "\| *\`${name//./\\.}\` *\| *\`=${bare//./\\.}" docs/dev/dependencies.md; then
        continue
    fi
    say "DRIFT: Cargo.toml pins $name $ver — not found in docs/dev/dependencies.md"
    fail=1
done < <(awk '
        /^\[/ { in_dep = ($0 ~ /dependencies/) ? 1 : 0 }
        in_dep && match($0, /^[a-z0-9_-]+ *= *(\{ *version *= *)?"=?[0-9]/) {
            name = $1
            line = $0
            sub(/^[^"]*"/, "", line)
            sub(/".*/, "", line)
            print name "\t" line
        }
    ' Cargo.toml)

# ── MCP tool count ──────────────────────────────────────────────
tools=$(grep -rEc '^[[:space:]]*#\[tool\(' src/mcp/tools*.rs | awk -F: '{s+=$2} END {print s}')
for doc in docs/design/degradation.md AGENTS.md; do
    for claim in $(grep -oE 'All [0-9]+ tools|[0-9]+ MCP tool' "$doc" | grep -oE '[0-9]+'); do
        if [ "$claim" != "$tools" ]; then
            say "DRIFT: $doc claims $claim tools — found $tools #[tool] annotations in src/mcp/"
            fail=1
        fi
    done
done

if [ "$fail" -eq 0 ]; then
    say "docs drift check passed ($tools MCP tools, dep versions in sync)"
fi
exit "$fail"
