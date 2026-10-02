# Security Policy

## Supported Versions

Only the latest release receives security fixes. CogZ is pre-1.0; patch
releases ship fixes on the current minor line.

## Reporting a Vulnerability

Please do not open public issues for security reports. Use GitHub's
private vulnerability reporting on this repository: Security →
Advisories → Report a vulnerability.

Reports covering these areas are especially welcome:

- Write paths that could corrupt the canonical Markdown corpus
- Hook payloads injected into agent configuration
- Secret-scanning bypasses (entities that should be refused but aren't)
- SQLite-derived state diverging from the canonical files

You will get an acknowledgement within a few days. If the report is
confirmed, a fix ships in the next patch release with credit in the
changelog unless you ask otherwise.
