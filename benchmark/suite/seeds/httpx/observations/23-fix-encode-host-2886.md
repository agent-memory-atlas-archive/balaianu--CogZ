---
id: 8924edbb-5049-496b-a662-73570a839a10
title: "Fix encode host (#2886)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

* Fix requiring dot literal rather than any character in IPv4 (from commit e63b6594f2; touches httpx/_urlparse.py)
