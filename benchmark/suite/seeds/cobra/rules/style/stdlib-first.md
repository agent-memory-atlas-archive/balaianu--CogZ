---
id: 0aa3b659-190c-417c-9573-06298fb239a4
title: "Prefer stdlib + pflag; dependencies need justification"
type: rule
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: style
tags: ["deps"]
confidence: 1.0
---

Cobra's only hard deps are pflag (flags) and mousetrap (windows). New dependencies bloat every downstream CLI — additions need maintainer sign-off.
