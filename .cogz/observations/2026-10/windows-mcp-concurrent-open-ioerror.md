---
id: 057b7c1d-0a20-4e09-95da-34d507e67c88
title: "Windows: concurrent Storage::open fails 'disk I/O error' — unconditional journal_mode write pragma"
type: observation
status: active
created_at: "2026-10-18T00:00:00+00:00"
updated_at: "2026-10-18T00:00:00+00:00"
references: []
source: field-report
confidence: 0.85
---

CI scheduled run 2026-10-18 (windows job only, same sha green the day
before): test_mcp_stdio's real-repo tests each spawn an MCP server on
the same .cogz/cogz.db concurrently. Storage::open ran
pragma_update(journal_mode, WAL) unconditionally — a write pragma that
takes an exclusive lock even as a no-op, with busy_timeout not yet set,
so the second process failed instantly. Fix (patch candidate, local):
set busy_timeout first, then only write journal_mode when current mode
is not already wal. Real-world scenario, not just test hygiene: two
agent MCP clients on one repo is a legitimate setup.
