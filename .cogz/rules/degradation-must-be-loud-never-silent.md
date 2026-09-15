---
id: aa6fefb2-87ad-4fc6-a531-44e204535419
title: "Degradation must be loud, never silent"
type: rule
status: stale
created_at: "2026-09-13T21:43:09.786665067+00:00"
updated_at: "2026-09-15T16:50:46.832643542+00:00"
references: ["e251d8d5-24b1-5758-b4d6-f598690f2721", "047656c5-e449-58d7-b951-3b948a609cd9", "cc7e17c2-8fe5-5bed-971c-5f4bc57f4227"]
confidence: 0.95
---

Every degraded or failed code path must surface a signal — `tracing::warn!` at minimum, an error return when the caller can act on it. Never `.ok()`-discard, `if let Ok(...)`-swallow, or `unwrap_or_default()`-mask a failure that changes system behavior. Graceful degradation means the system keeps working AND reports what it lost; silent degradation is indistinguishable from correct operation and is the worst failure mode for a system whose value proposition is trustworthy memory. Applies to: lock acquisition, stale-marking queries, migrations, dedup comparison-set truncation, network calls, sync errors.