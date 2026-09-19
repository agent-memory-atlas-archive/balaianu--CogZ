---
id: aa6fefb2-87ad-4fc6-a531-44e204535419
title: "Degradation must be loud, never silent"
type: rule
status: active
created_at: "2026-09-13T21:43:09.786665067+00:00"
updated_at: "2026-09-19T18:44:23.164180694+00:00"
references: ["cbfb76f0-1cf9-5705-9cdd-6f4d03b82440", "047656c5-e449-58d7-b951-3b948a609cd9", "cc7e17c2-8fe5-5bed-971c-5f4bc57f4227"]
confidence: 0.95
verified_against: ["047656c5-e449-58d7-b951-3b948a609cd9=2573200a85d75170d672883dfed5d39c5e19f3c4d15c0dcb8d3c5e1aa82bfe59", "cbfb76f0-1cf9-5705-9cdd-6f4d03b82440=044b74b95320ece145e435291874c2d2789b42545d5882428bbd741140dc4a6a", "cc7e17c2-8fe5-5bed-971c-5f4bc57f4227=356e2886b41981770e033c1fe9056ea477e60f774d30f80e40afc180d617e2f1"]
---

Every degraded or failed code path must surface a signal — `tracing::warn!` at minimum, an error return when the caller can act on it. Never `.ok()`-discard, `if let Ok(...)`-swallow, or `unwrap_or_default()`-mask a failure that changes system behavior. Graceful degradation means the system keeps working AND reports what it lost; silent degradation is indistinguishable from correct operation and is the worst failure mode for a system whose value proposition is trustworthy memory. Applies to: lock acquisition, stale-marking queries, migrations, dedup comparison-set truncation, network calls, sync errors.