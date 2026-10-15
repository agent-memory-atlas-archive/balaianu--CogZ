#!/usr/bin/env bash
# File-size ratchet. The AGENTS.md target is 400 lines per source
# file; 31 legacy files already exceed it. Instead of a mass split,
# the baseline freezes each oversized file at its current size:
# oversized files may only shrink, and no new file may exceed 400.
set -euo pipefail
cd "$(dirname "$0")/.."

LIMIT=400
BASELINE=scripts/file-size-baseline.txt
fail=0

declare -A baseline current
while read -r size path; do
    baseline[$path]=$size
done < "$BASELINE"

while read -r size path; do
    [ "$path" = "total" ] && continue
    current[$path]=$size
    exp="${baseline[$path]:-}"
    if [ -n "$exp" ]; then
        if [ "$size" -gt "$exp" ]; then
            echo "GROWTH: $path is $size lines (baseline $exp) — oversized files may only shrink"
            fail=1
        fi
    elif [ "$size" -gt "$LIMIT" ]; then
        echo "OVER: $path is $size lines (limit $LIMIT) — split it, or add a justified baseline entry"
        fail=1
    fi
done < <(find src -name '*.rs' | xargs wc -l)

for path in "${!baseline[@]}"; do
    size="${current[$path]:-0}"
    if [ "$size" -le "$LIMIT" ]; then
        echo "STALE: $path no longer exceeds $LIMIT (now $size) — remove its line from $BASELINE"
        fail=1
    fi
done

[ "$fail" -eq 0 ] && echo "file-size ratchet passed (${#baseline[@]} baselined, none grew)"
exit "$fail"
