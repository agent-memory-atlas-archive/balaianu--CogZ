---
id: 5d96f1a1-6b4d-45c3-94cf-fb5abc238111
title: "Observed: subcommands run parent PersistentPreRun but not its PersistentPostRun"
type: observation
status: superseded
created_at: "2026-09-22T00:00:00Z"
updated_at: "2026-09-22T00:00:00Z"
references: []
source: agent
superseded_by: ["e3713664-16e5-421e-8cb3-daa363d1cb6e"]
---

Hook inheritance is asymmetric: child executes root's PersistentPreRun; PersistentPostRun only fires if the executed command's chain reaches it.
