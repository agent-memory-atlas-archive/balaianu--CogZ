---
id: d52a9d9f-8462-4c7b-991d-7ab70f662767
title: "Active help streams hints into shell completion"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["active-help", "completion"]
---

active_help.go: programs set messages via GetActiveHelpConfig/ActiveHelp; the __complete machinery emits them as completion descriptions. Controlled by the COBRA_ACTIVE_HELP env var (0 disables, 'local' limits to local commands).
