# Security Policy

## Supported Versions

Security fixes are provided for the latest tagged release.

## Reporting A Vulnerability

Please use GitHub's private vulnerability reporting for this repository rather
than opening a public issue. Include the affected command, platform, a minimal
reproduction and the impact you believe is possible.

Do not include real credentials, private source code or proprietary repository
contents in a report. A maintainer will acknowledge the report within seven
days and coordinate disclosure after a fix is available.

## Safety Model

Code Mechanic reads and, only with `--apply --expect-plan`, may rewrite files
under an explicitly selected root. It rejects symlink escapes, stale content
hashes, ambiguous definitions, overlapping edits and post-edit parse errors.
The SQLite index is disposable and must never be treated as source authority.
