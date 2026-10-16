#!/usr/bin/env bash
# File-size ratchet. The AGENTS.md target is 400 lines per source
# file; legacy files already exceed it. Instead of a mass split,
# the baseline freezes each oversized file at its current size:
# oversized files may only shrink, and no new file may exceed 400.
# Portable to bash 3.2 (macOS) — aggregation lives in awk.
set -euo pipefail
cd "$(dirname "$0")/.."

LIMIT=400
BASELINE=scripts/file-size-baseline.txt

# shellcheck disable=SC2016
find src -name '*.rs' -print0 | xargs -0 wc -l | awk \
    -v baseline="$BASELINE" -v limit="$LIMIT" '
BEGIN {
    n = 0
    while ((getline line < baseline) > 0) {
        if (split(line, a) == 2) { bsize[a[2]] = a[1] + 0; n++ }
    }
    fail = 0
}
$2 == "total" { next }
{
    size = $1 + 0
    path = $2
    if (path in bsize) {
        seen[path] = 1
        if (size > bsize[path]) {
            printf "GROWTH: %s is %d lines (baseline %d) — oversized files may only shrink\n", path, size, bsize[path]
            fail = 1
        } else if (size <= limit) {
            printf "STALE: %s no longer exceeds %d (now %d) — remove its line from %s\n", path, limit, size, baseline
            fail = 1
        }
    } else if (size > limit) {
        printf "OVER: %s is %d lines (limit %d) — split it, or add a justified baseline entry\n", path, size, limit
        fail = 1
    }
}
END {
    for (p in bsize) {
        if (!(p in seen)) {
            printf "STALE: %s no longer exceeds %d (now 0) — remove its line from %s\n", p, limit, baseline
            fail = 1
        }
    }
    if (!fail) printf "file-size ratchet passed (%d baselined, none grew)\n", n
    exit fail
}'
