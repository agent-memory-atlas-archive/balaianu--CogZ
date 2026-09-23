---
id: 8f05c692-3d8a-4b44-8490-20a3265dd6a9
title: "Always set an explicit timeout on Client"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: promoted
tags: ["promoted"]
confidence: 1.0
---

Do not rely on the implicit 5s default in production paths — declare timeout= on the Client so pool/read behavior is deliberate.
