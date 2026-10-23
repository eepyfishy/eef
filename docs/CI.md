# GitHub core verification

The `Core verification` workflow runs on pushes to `main` and `validation/**`,
pull requests targeting `main`, and manual dispatch. Each run checks out its
triggering revision, not a developer's uncommitted worktree.

The matrix tests Ubuntu 24.04 and Windows 2025 with default dashboard features
and with `--no-default-features`. All four jobs check formatting, the
backend-to-browser dependency boundary, and the locked workspace/all-target
Rust regression suite. Rust is pinned to 1.98.1 and Node to 24.20.0.

The Windows headless job additionally builds command services and example
helpers, then runs 23 sequential native synthetic regression scripts. Python
3.11.9 is selected explicitly for the Python fixtures. These tests use isolated
fixture configuration, loopback services, and synthetic artifacts, not installed
user nodes or downloaded language models. The workflow file is the authoritative
list of included suites; future feature tests must be added explicitly.

Each job fetches the locked dependency graph before offline builds/tests.
Official checkout, Node setup, and Python setup actions are pinned by full
commit SHA. Dependency caches and artifact uploads are not enabled. Private
fixture journals, credentials, and child-process logs must not be uploaded.
Jobs fail on the first failing step; matrix jobs are independent and are not
automatically retried. Superseded runs on the same ref are cancelled.

The workflow has only `contents: read`, does not persist checkout credentials,
and does not use project authentication secrets. Pull requests run through
`pull_request`, never a privileged `pull_request_target` execution path. It
cannot publish releases, sign installers, update nodes, or modify update feeds.
The project's isolated maintainer login needs `workflow` scope to publish the
workflow file; this is separate from the CI job's read-only token permissions.

## Acceptance boundaries

Green CI verifies only the selected revision and included synthetic checks.
It does not establish useful model quality, physical two-PC acceptance,
successful public package repair, fresh installation/migration/uninstallation,
Defender clearance, code signing, or stable v0.4.0 readiness. Package-doctor and
unavailable-model native suites still require separately verified local runtime
artifacts and are not included in this workflow. The ignored latency profile is
not a CI performance guarantee.

CI setup does not change the application version or prerelease exclusion from
automatic updates. Existing historical notes saying CI stayed local describe
their checkpoints; this workflow supersedes that policy once published.
