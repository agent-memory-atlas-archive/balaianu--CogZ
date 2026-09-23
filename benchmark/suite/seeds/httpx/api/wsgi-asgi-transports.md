---
id: 98c25da6-9726-4aef-995b-5c5c20a05041
title: "In-process testing uses WSGITransport or ASGITransport"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["testing", "wsgi", "asgi"]
---

WSGITransport(app=flask_app) and ASGITransport(app=asgi_app) let a Client call a Python app without a socket — the standard test pattern. raise_app_exceptions=False returns 500 responses for inspection instead of raising; script_name and remote_addr customize the WSGI environ.
