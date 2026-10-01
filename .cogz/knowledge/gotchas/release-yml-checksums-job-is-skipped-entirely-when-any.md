---
id: 77839cb6-714e-4372-b018-f0aac6f2c472
title: release.yml checksums job is skipped entirely when any matrix leg fails — partial releases ship without SHA256SUMS
type: knowledge
status: active
created_at: "2026-10-01T13:46:44.547751810+00:00"
updated_at: "2026-10-01T13:46:44.547751810+00:00"
references: []
category: gotchas
tags: ["ci", "release", "github-actions", "checksums", "retag"]
---

In `.github/workflows/release.yml`, the `checksums` job declares `needs: build-and-release`, which requires *every* matrix leg to succeed. One failed platform → the whole checksums job is `skipped` → the GitHub release publishes binaries with no SHA256SUMS, and installers/`cogz update` degrade to warn-only verification. A partial release is worse than a failed one because nothing surfaces the missing manifest.

Remediation when a just-published release is broken (minutes old, no real consumers): delete the GitHub release, force-move the tag to the fix commit, `git push -f origin <tag>`. The rerun rebuilds all targets from the tagged commit and re-uploads, so provenance stays honest. Prefer this over a phantom patch bump when the broken release would otherwise linger in the releases list — but only inside the minutes-old window; once consumers may have pulled it, cut a patch release instead.