---
id: 7718be7f-6bbd-4970-9022-7e81a3685c14
title: "SSL_CERT_FILE and SSL_CERT_DIR configure certificate verification"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["ssl", "environment"]
---

Certifi is the default CA bundle; SSL_CERT_FILE/SSL_CERT_DIR env vars point at alternate bundles without code changes. verify=False or verify='/path/to/ca.pem' on Client overrides per-client.
