---
id: 490ed79b-4ca4-48b9-9737-56b781ba0473
title: "ArgGroup models mutual requirements and conflicts between args"
type: knowledge
status: active
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
category: api
tags: ["groups", "args"]
---

builder/arg_group.rs — groups collect args for requires/conflicts/multiple semantics: requires_all, conflicts_with_all, multiple(true) allows several members at once. Group membership is declared via Arg::group or ArgGroup::arg.
