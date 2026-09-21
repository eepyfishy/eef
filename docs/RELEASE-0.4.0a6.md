# EEF v0.4.0a6 - interrupted-job reliability

Internal version `0.4.0-alpha.6`. Opt-in prerelease, not the completed roadmap.
Two Windows x86-64 EXE installers. No model is bundled or automatically installed.

## Changes

- Coordinator shutdown now checkpoints and drains in-flight jobs before closing
  the node gateway. A transport shutdown must not label an uncertain remote
  action as an ordinary failed action. Such jobs remain interrupted, are never
  automatically resumed, and cannot resume when retrying could repeat effects.
- CLI errors explain possible coordinator-version mismatch for HTTP 404 and
  report malformed replies as unconfirmed outcomes. No retry or fallback is
  added. Matching coordinator/node versions are recommended.
- Existing explicit, exact-artifact remote updates remain available with owner
  approval. Both automatic-update feeds remain paused. Installing this alpha
  does not opt a network into future alpha updates.

This is not remote cancellation, exactly-once execution, safe coordinator
clustering or self-coding/self-upgrading development. Already-sent actions may
finish after their coordinator loses confirmation. Models, permissions, private
test addresses and public update policy are not changed by this release.

## Validation

The source increment passed 185 Rust tests with and without dashboard support.
The new isolated transport regression failed against alpha.5 and passed four
consecutive times against the corrected headless build. Existing command/update
tests passed. Pre-packaging physical two-PC testing reproduced the original
failure and verified completed/failed job persistence. The fixed packaged
build's subsequent physical validation is recorded below.

### Packaged build and local deployment (2026-09-21)

Both installers were built from clean source commit
`2f0819c4eebb8183230a9a1cb1e3db6ac1869ab0`. Alpha.6 passed 185 Rust tests
with default features and 185 without dashboard support, formatting, and the
backend/browser dependency-boundary check. The optimized shutdown regression,
node-command suite and explicit-update command suite passed. The latter uses
mocked update confirmation and is not evidence of a physical artifact update.

Fresh installation and repeated quiet upgrades passed for both products:
configuration and startup choices were preserved, stale update selection was
backed up, existing version directories were retained, and installed inventories
matched. Bundled Python media imports and the minimal llama.cpp runtime passed.

The local coordinator was backed up and upgraded from alpha.5 to alpha.6.
Configuration, database and startup choice were unchanged. With an existing
physical alpha.5 node, the in-flight command regression now passes: the exact
remote test process was observed before coordinator restart, its job remained
interrupted with one attempt, unsafe resume was refused, and late completion did
not change the uncertain checkpoint. The remote node was not restarted and
configuration/permissions were unchanged. Only this test's terminal history was
removed after saving evidence. This is a bounded test, not an exactly-once guarantee.

A second physical regression verified successful remote read jobs, a missing-file
failure with exactly two bounded safe retries, subsequent healthy work, and
unchanged persisted results/status/attempt counts across coordinator restart.
The journal passed SQLite quick-check. Test-owned terminal records were removed
after saving evidence; no user job history was removed.

Packaging scans passed for the binaries, bundled runtimes, unpacked bundle and
both final installers using Defender engine `1.1.26080.3`, definitions
`1.459.299.0`. The installed coordinator also passed with definitions
`1.459.317.0`; both installers were rescanned successfully with those updated
definitions before publication. No exclusions or protection changes were made.

### Artifacts

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 17957721 | `91ece1f01041851db0bfd6bb393dda92cab33a45d2335a8208969f06f1085471` |
| `eefn-installer.exe` | 109874577 | `6dbac06542640670118d5ee056a02df8b9646073dbdbeb1ee9039e09a772cb84` |

### Physical explicit-update acceptance (2026-09-21)

After publication, GitHub reported exactly the two expected assets with matching
byte counts and SHA-256 digests. An existing owner-approved alpha.5 node was
updated using the installed coordinator's `node update` command, selecting the
published EEFN installer, exact hash/size and one-shot prerelease consent. One
request was sent, with no retry, shell installation fallback or feed edits.

The node confirmed alpha.6 was staged while alpha.5 was still running. Its staged
executable matched the build hash and passed a separate on-node Defender scan
(engine `1.1.26080.3`, definitions `1.459.311.0`) before the separately approved
restart. It reconnected as alpha.6 with the same stable node identity and a new
runtime ID, no pending restart or startup issues, and unchanged configuration and
permissions. All five version-matched network probes passed. Both live update
checks still reported paused feeds and no available update. No model was installed.

With both roles now on alpha.6, the physical in-flight interruption and durable-
job persistence regressions were repeated and both passed again. Their test-owned
terminal records were removed only after evidence was saved; no test jobs remain.

The manual Defender check above is validation performed for this deployment, not
a newly implemented updater feature. The physical test does not establish
rollback, interrupted-download recovery, durable update receipts or exactly-once
installation. Publication is not automatic deployment: feeds remain paused and
this release requires explicit installation/selection.

Builds remain unsigned. Defender scan success is not Microsoft clearance and
does not resolve every historical false-positive report. Do not bypass antivirus.
