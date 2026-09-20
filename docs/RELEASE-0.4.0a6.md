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
tests passed. Physical two-PC testing reproduced the original failure and
verified completed/failed job persistence; it did not yet validate the fix.

Final optimized build, installer validation, hashes and physical deployment
results will be recorded below after verification. Publication is not deployment.

Builds remain unsigned. Defender scan success is not Microsoft clearance and
does not resolve every historical false-positive report. Do not bypass antivirus.
