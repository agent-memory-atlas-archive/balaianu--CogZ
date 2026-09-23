---
id: 2e62524a-6041-4d70-98db-ab8bf1d82f18
title: "Auth classes implement a multi-step request flow"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["auth", "digest"]
---

httpx/_auth.py: BasicAuth, DigestAuth, NetRCAuth, and the Auth base class. DigestAuth is the non-obvious one — it implements requires_response_body=True and yields a second request after seeing the 401 challenge, so auth is a generator flow (auth_flow yields Requests), not a simple header injector.
