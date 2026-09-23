---
id: 0b6603da-8f3a-47f6-a6cd-dc0e3803c4c2
title: "Event hooks run per request and response"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["hooks", "events"]
---

Client(event_hooks={'request': [...], 'response': [...]}) takes dicts of callable lists. Hooks receive the request/response object; response hooks can call response.read() to force-load content for inspection. Used for logging, tracing, auth refresh.
