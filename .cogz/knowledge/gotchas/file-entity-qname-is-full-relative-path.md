---
id: c83eed6c-2223-4388-9efc-432b6115820a
title: "File-entity qualified_name is the full repo-relative path, not the basename"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-19T18:44:24.375021716+00:00"
references: ["a0f5b27b-3ea4-5ef7-ba6f-69b7a9c08443"]
category: gotchas
tags: ["uuid5", "code-entities", "references", "seed-scripts"]
verified_against: ["a0f5b27b-3ea4-5ef7-ba6f-69b7a9c08443=ba15d63dddadb23d908c082105bccf3b1c2668697b685c05287a44044632de35"]
---

# File-entity qualified_name = full relative path

`code_entity_uuid` input is `{file_path}:{entity_type}:{qualified_name}`.
For `file` entities the qualified_name is the **full repo-relative path**
(`src/telegram_acp_bot/telegram/bot.py`), not the basename (`bot.py`).

Burned the ab3 seed script: it generated file-reference UUIDs from basenames
and produced `missing` drift rows on perfectly live file entities. Function
entities use bare names, methods use `Class::method`, files use the path —
three different qname shapes, all easy to guess wrong.

Any script that mints `references` UUIDs externally must reproduce the exact
qualified_name the extractor used, or resolve names against the DB instead
of guessing.
