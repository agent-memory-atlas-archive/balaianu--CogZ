---
id: 1370b54e-abe7-48f7-bc88-dce7f79e319e
title: "The doc/ package generates man, markdown, and yaml docs"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: architecture
tags: ["docs", "generation"]
---

doc/ subtree (md_docs.go, man_docs.go, yaml_docs.go, util.go) walks the command tree and emits per-command files: GenMarkdownTree, GenManTree (troff via md2man), GenYamlTree. Docs reflect the live command tree — regenerate on every CLI change.
