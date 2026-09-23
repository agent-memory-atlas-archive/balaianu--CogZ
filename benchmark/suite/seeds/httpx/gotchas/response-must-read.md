---
id: b0babf27-1f0d-49ae-b1f9-30ed776d8485
title: "Accessing .text on a streaming response raises ResponseNotRead"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: gotchas
tags: ["streaming", "gotcha"]
---

With client.stream() or stream=True, response.content/.text raise ResponseNotRead until .read()/.aread() is called or the stream is iterated. Streaming responses must also be closed — the context manager form of client.stream() handles this.
