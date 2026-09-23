---
id: 7c15e713-4850-4b5f-b433-34ecc05070f1
title: "Change LineDecoder to match stdlib splitlines, resulting in significant speed up (#2423)"
type: observation
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: git-history
---

* Replace quadratic algo in LineDecoder (from commit 85c5898d8e; touches httpx/_decoders.py)
