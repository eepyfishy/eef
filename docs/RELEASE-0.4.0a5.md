# EEF v0.4.0a5 - explicit update correction

Internal version `0.4.0-alpha.5`. Opt-in prerelease, not the completed roadmap.
Two Windows x86-64 EXE installers. No model is bundled or automatically installed.

## Corrected update behavior

- Automatic and legacy feed updates remain stable-only. Both public feeds are
  paused, with no downloadable artifact, because all existing releases are now
  prereleases. Alpha.5 reports `feed_paused: true`; older clients reject the feed
  with a check error. Neither behavior installs a release automatically.
- `eef node update` selects one exact HTTPS artifact, internal version, SHA-256
  and byte count. `--allow-prerelease` approves that artifact only, not future
  automatic alpha updates. The node must allow remote updates in both its applied
  policy and current saved configuration. No process/filesystem grant is needed.
- Authorization is checked again before activation; installation is serialized
  with other update operations. Hash, size, bundle product/version, downgrade and
  pending-selection checks protect the installation. Saved settings/feed remain
  unchanged. `eef node update-status` reads the actual installed selection.
- Restart is a separate approved command. Unconfirmed replies are not replayed;
  older nodes reject the new action without shell or feed-replacement fallback.

Alpha.4 and older lack the corrected command. They need a normal installer
bootstrap before using it. Use the existing dedicated install directory and back
up configuration/data. Do not disable Defender or expand node permissions just
to bypass an update refusal. See [commands](COMMANDS.md) for exact syntax/limits.

## Security and validation limits

Builds remain unsigned. Local Defender scan success is not Microsoft clearance;
previous second-PC detections remain unresolved. No private test addresses,
credentials, special developer access or automatic analytics are added.

The source fix previously passed 183 Rust tests with and without dashboard
support plus isolated command/permission/reply tests. Those tests used synthetic
payloads and did not execute a downloaded release. This release adds paused-feed
coverage. Final build, installer, real-artifact update and deployment results are
recorded below only after validation; publication alone does not prove deployment.
