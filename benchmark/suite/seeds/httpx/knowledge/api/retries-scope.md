---
id: a97d2be1-f08f-4fdd-954b-a0051766e59f
title: "Transport retries only cover connect failures"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["retries", "transports"]
---

HTTPTransport(retries=N) retries only ConnectError and ConnectTimeout — never read/write errors or 5xx responses. Anything broader requires an external retry layer (tenacity) or a custom transport wrapper. This is a common wrong assumption.
