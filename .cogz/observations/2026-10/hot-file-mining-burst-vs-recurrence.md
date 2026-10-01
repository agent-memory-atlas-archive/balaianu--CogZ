---
id: b4f2c8d1-9e35-4f67-a812-7c3d9e0f5a21
title: "Usage-frequency mining signals must require recurrence, not burst counts"
type: observation
status: active
created_at: "2026-10-01T16:45:00+00:00"
updated_at: "2026-10-01T16:45:00+00:00"
references: []
source: field-report
confidence: 0.9
---

Field report: the `hot_file` mining pass suggested "README.md saved 7
times — a hot spot" from a single deep-audit session where the file was
corrected as findings landed. Burst frequency within one session is a
symptom of the task, not durable knowledge about the file.

Fix landed: `hot_files` now requires `COUNT(DISTINCT date(created_at))
>= 2` alongside the `>= 3` save minimum — cross-day recurrence is the
session-agnostic proxy (events carry no session id, and session_start
hooks may not fire on every harness). Suggestion text now says "saved N
times across D days" so the evidence stays honest.

Generalizable: any "X happened N times" mining signal should ask *when*
the N occurrences landed before claiming a durable pattern.
